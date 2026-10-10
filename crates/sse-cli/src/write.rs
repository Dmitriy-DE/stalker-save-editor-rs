use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use sse_core::{Error, ExitCode as CommandExitCode, SaveBuffer};
use sse_xray::writer::{self, Change, ChangeSet, Placement};
use sse_xray::Save;

use crate::{configured_backup_directory, S2_LEGACY_EDIT_REFUSAL};

enum WriteFailure {
    Usage(String),
    Core(Error),
}

impl From<Error> for WriteFailure {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

impl From<std::io::Error> for WriteFailure {
    fn from(value: std::io::Error) -> Self {
        Self::Core(Error::from(value))
    }
}

#[derive(Default)]
struct WriteOptions {
    output: Option<PathBuf>,
    backup_directory: Option<PathBuf>,
    in_place: bool,
    money: Option<u32>,
    stacks: HashMap<u32, u32>,
    stack_order: Vec<u32>,
    additions: Vec<AddOption>,
    durability: Vec<(u32, f32)>,
    upgrades: Vec<(u32, Vec<String>)>,
    placements: Vec<(u32, Placement)>,
    moves: Vec<(u32, MoveDestination)>,
    removals: Vec<u32>,
    unsupported_operations: Vec<String>,
}

struct PublishedWrite {
    output_path: PathBuf,
    backup_path: PathBuf,
    output_sha256: String,
    size: usize,
    maintenance_warning: Option<String>,
}

struct WritePublication<'a> {
    source_path: &'a Path,
    expected_source_sha256: &'a str,
    replacement: &'a [u8],
    output_path: &'a Path,
    backup_directory: &'a Path,
    summary: sse_storage::transaction::EditSummary,
    in_place: bool,
}

struct AddOption {
    template_object: Option<u16>,
    item_key: String,
    quantity: u32,
}

enum MoveDestination {
    Actor,
    Object(u32),
}

pub(super) fn run_write(arguments: &[String]) -> u8 {
    match prepare_and_export(arguments) {
        Ok(()) => CommandExitCode::Done as u8,
        Err(WriteFailure::Usage(message)) => {
            eprintln!("Error: {message}");
            CommandExitCode::Usage as u8
        }
        Err(WriteFailure::Core(error)) => {
            eprintln!("Error: {error}");
            error.exit_code() as u8
        }
    }
}

fn prepare_and_export(arguments: &[String]) -> Result<(), WriteFailure> {
    let command = arguments.first().map(String::as_str).unwrap_or_default();
    let mut options = WriteOptions::default();
    let mut positional_money = None;
    let mut positional_stack = None;
    let path = match command {
        "set-money" => {
            if arguments.len() < 3 {
                return Err(WriteFailure::Usage("set-money requires SAVE and MONEY.".to_owned()));
            }
            positional_money = Some(parse_unsigned(write_argument(arguments, 2)?)?);
            parse_write_options(arguments, 3, &mut options)?;
            PathBuf::from(write_argument(arguments, 1)?)
        }
        "set-stack" => {
            if arguments.len() < 4 {
                return Err(WriteFailure::Usage(
                    "set-stack requires SAVE, HANDLE and COUNT.".to_owned(),
                ));
            }
            positional_stack = Some((
                parse_unsigned(write_argument(arguments, 2)?)?,
                parse_unsigned(write_argument(arguments, 3)?)?,
            ));
            parse_write_options(arguments, 4, &mut options)?;
            PathBuf::from(write_argument(arguments, 1)?)
        }
        "edit" => {
            if arguments.len() < 2 {
                return Err(WriteFailure::Usage("edit requires SAVE.".to_owned()));
            }
            parse_write_options(arguments, 2, &mut options)?;
            if !options.unsupported_operations.is_empty() {
                return Err(WriteFailure::Core(Error::Refused(format!(
                    "The requested write kind is not supported by a confirmed Core writer: {}",
                    options.unsupported_operations.join(", ")
                ))));
            }
            if options.money.is_none()
                && options.stacks.is_empty()
                && options.additions.is_empty()
                && options.durability.is_empty()
                && options.upgrades.is_empty()
                && options.placements.is_empty()
                && options.moves.is_empty()
                && options.removals.is_empty()
            {
                return Err(WriteFailure::Core(Error::Refused("No changes requested.".to_owned())));
            }
            PathBuf::from(write_argument(arguments, 1)?)
        }
        _ => return Err(WriteFailure::Usage("unknown write command".to_owned())),
    };

    match command {
        "set-money" => {
            if options.money.is_some() {
                return Err(WriteFailure::Usage(
                    "set-money takes MONEY once: remove the --money option.".to_owned(),
                ));
            }
            options.money = positional_money;
        }
        "set-stack" => {
            if !options.stacks.is_empty() {
                return Err(WriteFailure::Usage(
                    "set-stack takes HANDLE and COUNT once: remove the --stack option.".to_owned(),
                ));
            }
            options.stacks.clear();
            options.stack_order.clear();
            let (handle, count) = positional_stack
                .ok_or_else(|| WriteFailure::Usage("set-stack requires SAVE, HANDLE and COUNT".to_owned()))?;
            options.stacks.insert(handle, count);
            options.stack_order.push(handle);
        }
        "edit" => {}
        _ => return Err(WriteFailure::Usage("unknown write command".to_owned())),
    }

    if options.in_place && options.output.is_some() {
        return Err(WriteFailure::Usage(
            "--in-place cannot be combined with --output.".to_owned(),
        ));
    }
    if options.in_place
        && (!options.additions.is_empty()
            || !options.durability.is_empty()
            || !options.upgrades.is_empty()
            || !options.placements.is_empty()
            || !options.moves.is_empty()
            || !options.removals.is_empty()
            || !options.unsupported_operations.is_empty())
    {
        return Err(WriteFailure::Core(Error::Refused(
            "In-place CLI writes currently support only money and stack-count edits.".to_owned(),
        )));
    }

    let source = SaveBuffer::read(&path)?;
    if let Ok(s2_save) = sse_s2::S2Save::from_bytes(source.as_slice()) {
        return prepare_and_export_s2(&path, source.as_slice(), &s2_save, &options);
    }
    let save = Save::read(source.as_slice())?;
    let stack_count = options.stacks.len();
    let change_count = stack_count
        .saturating_add(usize::from(options.money.is_some()))
        .saturating_add(options.additions.len())
        .saturating_add(options.durability.len())
        .saturating_add(options.upgrades.len())
        .saturating_add(options.placements.len())
        .saturating_add(options.moves.len())
        .saturating_add(options.removals.len());
    let mut changes = Vec::with_capacity(change_count);
    let mut stack_output = Vec::with_capacity(stack_count);
    let mut addition_output = Vec::with_capacity(options.additions.len());
    let mut durability_output = Vec::with_capacity(options.durability.len());
    let mut upgrade_output = Vec::with_capacity(options.upgrades.len());
    let mut placement_output = Vec::with_capacity(options.placements.len());
    let mut move_output = Vec::with_capacity(options.moves.len());
    let mut removal_output = Vec::with_capacity(options.removals.len());
    if let Some(new_value) = options.money {
        changes.push(Change::SetMoney {
            target_object: save.actor_id(),
            old_value: save.money()?,
            new_value,
        });
    }
    let inventory = if options.stacks.is_empty() && options.durability.is_empty() {
        None
    } else {
        Some(save.inventory()?)
    };
    if !options.stacks.is_empty() {
        let inventory = inventory.as_deref().unwrap_or(&[]);
        for handle in &options.stack_order {
            let new_value = options
                .stacks
                .get(handle)
                .copied()
                .ok_or_else(|| WriteFailure::Core(Error::damaged("missing parsed stack option")))?;
            let target_object = u16::try_from(*handle).map_err(|_| {
                WriteFailure::Core(Error::Refused(format!(
                    "X-Ray item handle 0x{handle:X} exceeds 16 bits"
                )))
            })?;
            let old_value = inventory
                .iter()
                .find(|item| item.handle == target_object)
                .and_then(|item| item.count)
                .ok_or_else(|| {
                    WriteFailure::Core(Error::Refused(format!(
                        "object 0x{handle:04X} is not a confirmed editable ammo stack"
                    )))
                })?;
            let new_value = u16::try_from(new_value).map_err(|_| {
                WriteFailure::Core(Error::Refused(format!(
                    "ammo count for 0x{handle:04X} must be in 1..65535"
                )))
            })?;
            stack_output.push((target_object, new_value));
            changes.push(Change::SetStack {
                target_object,
                old_value,
                new_value,
            });
        }
    }

    for (handle, condition) in &options.durability {
        let target_object = object_id(*handle)?;
        let old_value = inventory
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|item| item.handle == target_object)
            .and_then(|item| item.condition)
            .ok_or_else(|| {
                WriteFailure::Core(Error::Refused(format!(
                    "object 0x{handle:04X} has no confirmed durability value"
                )))
            })?;
        changes.push(Change::SetDurability {
            target_object,
            old_value,
            new_value: *condition,
        });
        durability_output.push((target_object, *condition));
    }

    for (handle, destination) in &options.placements {
        let target_object = object_id(*handle)?;
        changes.push(Change::SetPlacement {
            target_object,
            destination: *destination,
        });
        placement_output.push((target_object, *destination));
    }

    for (handle, destination) in &options.moves {
        let target_object = object_id(*handle)?;
        let new_parent = match destination {
            MoveDestination::Actor => save.actor_id(),
            MoveDestination::Object(parent) => object_id(*parent)?,
        };
        let old_parent = save
            .registry_objects()
            .iter()
            .find(|record| record.object_id == target_object)
            .map(|record| record.parent_id)
            .ok_or_else(|| WriteFailure::Core(Error::Refused(format!("object 0x{handle:04X} is missing"))))?;
        changes.push(Change::MoveItem {
            target_object,
            old_parent,
            new_parent,
        });
        move_output.push((target_object, old_parent, new_parent));
    }

    for handle in &options.removals {
        let target_object = object_id(*handle)?;
        changes.push(Change::RemoveItem { target_object });
        removal_output.push(target_object);
    }

    for (handle, new_value) in &options.upgrades {
        let target_object = object_id(*handle)?;
        let old_value = writer::current_upgrades(&save, target_object)?;
        changes.push(Change::SetUpgrades {
            target_object,
            old_value,
            new_value: new_value.clone(),
        });
        upgrade_output.push((target_object, new_value.clone()));
    }

    let mut reserved_object_ids = save
        .registry_objects()
        .iter()
        .map(|record| record.object_id)
        .collect::<HashSet<_>>();
    for addition in &options.additions {
        let template_object = match addition.template_object {
            Some(handle) => handle,
            None => automatic_add_template(&save, &addition.item_key).ok_or_else(|| {
                WriteFailure::Core(Error::Refused(format!(
                    "no confirmed registry template matches item key '{}'",
                    addition.item_key
                )))
            })?,
        };
        if addition.template_object.is_some() {
            let template = save
                .registry_objects()
                .iter()
                .find(|record| record.object_id == template_object)
                .ok_or_else(|| {
                    WriteFailure::Core(Error::Refused(format!(
                        "explicit add template 0x{template_object:04X} does not exist"
                    )))
                })?;
            if !template.name.eq_ignore_ascii_case(&addition.item_key) {
                return Err(WriteFailure::Core(Error::Refused(format!(
                    "explicit TEMPLATE:KEY must use the template's section '{}', not '{}'",
                    template.name, addition.item_key
                ))));
            }
        }
        let object_id = allocate_object_id(&mut reserved_object_ids, template_object)?;
        let quantity = u16::try_from(addition.quantity).map_err(|_| {
            WriteFailure::Core(Error::Refused(format!(
                "item quantity for '{}' must fit in 1..65535",
                addition.item_key
            )))
        })?;
        changes.push(Change::AddItem {
            template_object,
            item_key: addition.item_key.clone(),
            object_id,
            quantity,
        });
        addition_output.push((addition.item_key.clone(), quantity));
    }

    let output = writer::apply(&save, &ChangeSet::new(changes))?;
    let source_sha256 = sse_codecs::sha256::sha256_hex(source.as_slice());
    let output_path = options.output.clone().unwrap_or_else(|| default_output_path(&path));
    let backup_directory = options
        .backup_directory
        .clone()
        .unwrap_or_else(configured_backup_directory);
    let receipt = publish_cli_write(
        WritePublication {
            source_path: &path,
            expected_source_sha256: &source_sha256,
            replacement: output.as_slice(),
            output_path: &output_path,
            backup_directory: &backup_directory,
            summary: write_edit_summary(&options),
            in_place: options.in_place,
        },
        |bytes| verify_xray_money_and_stacks(bytes, &options),
    )?;
    println!("Output: {}", receipt.output_path.display());
    println!("Size: {}", receipt.size);
    println!("Backup: {}", receipt.backup_path.display());
    println!("SHA256: {}", receipt.output_sha256);
    if let Some(warning) = receipt.maintenance_warning.as_deref() {
        eprintln!("Warning: backup rotation did not complete: {warning}");
    }
    if let Some(money) = options.money {
        println!("Money: {money}");
    }
    for (handle, count) in stack_output {
        println!("Stack 0x{handle:08X}: {count}");
    }
    for (key, quantity) in addition_output {
        println!("Add {key} x{quantity}");
    }
    for (handle, condition) in durability_output {
        println!("Durability 0x{handle:08X}: {}", format_condition(condition));
    }
    for (handle, upgrades) in upgrade_output {
        println!("Upgrades 0x{handle:04X}: {}", upgrades.join(","));
    }
    for (handle, placement) in placement_output {
        println!("Placement 0x{handle:04X}: {}", placement_name(placement));
    }
    for (handle, old_parent, new_parent) in move_output {
        println!("Move 0x{handle:04X}: 0x{old_parent:04X} -> 0x{new_parent:04X}");
    }
    for handle in removal_output {
        println!("Remove 0x{handle:04X}");
    }
    Ok(())
}

fn publish_cli_write(
    publication: WritePublication<'_>,
    verify_readback: impl FnOnce(&[u8]) -> Result<(), Error>,
) -> Result<PublishedWrite, WriteFailure> {
    let WritePublication {
        source_path,
        expected_source_sha256,
        replacement,
        output_path,
        backup_directory,
        summary,
        in_place,
    } = publication;
    if in_place {
        let request = sse_storage::transaction::ReplacementRequest::new(
            source_path,
            expected_source_sha256,
            replacement,
            backup_directory,
        )
        .with_summary(summary);
        let (receipt, (), ()) = sse_storage::transaction::replace_transaction(
            &sse_storage::transaction::StdFileSystem,
            request,
            |_, _| Ok(()),
            verify_readback,
        )?;
        return Ok(PublishedWrite {
            output_path: receipt.source_path,
            backup_path: receipt.backup_path,
            output_sha256: receipt.output_sha256,
            size: replacement.len(),
            maintenance_warning: receipt.maintenance_warning,
        });
    }

    verify_readback(replacement)?;
    let receipt = sse_storage::transaction::export_transaction(
        source_path,
        expected_source_sha256,
        replacement,
        output_path,
        backup_directory,
        summary,
    )?;
    let read_back = std::fs::read(&receipt.output_path)?;
    if read_back.as_slice() != replacement || sse_codecs::sha256::sha256_hex(&read_back) != receipt.output_sha256 {
        return Err(WriteFailure::Core(Error::System(
            "export read-back did not match the prepared save".to_owned(),
        )));
    }
    Ok(PublishedWrite {
        output_path: receipt.output_path,
        backup_path: receipt.backup_path,
        output_sha256: receipt.output_sha256,
        size: read_back.len(),
        maintenance_warning: receipt.maintenance_warning,
    })
}

fn write_edit_summary(options: &WriteOptions) -> sse_storage::transaction::EditSummary {
    sse_storage::transaction::EditSummary {
        money: options.money,
        stack_count: options.stacks.len(),
        move_count: options.moves.len(),
        detach_count: options.removals.len(),
        add_count: options.additions.len(),
        durability_count: options.durability.len(),
        upgrade_count: options.upgrades.len(),
        ..sse_storage::transaction::EditSummary::default()
    }
}

fn verify_xray_money_and_stacks(bytes: &[u8], options: &WriteOptions) -> Result<(), Error> {
    let save = Save::read(bytes)?;
    if let Some(expected) = options.money {
        if save.money()? != expected {
            return Err(Error::damaged("saved wallet value differs after read-back"));
        }
    }
    if options.stacks.is_empty() {
        return Ok(());
    }
    let inventory = save.inventory()?;
    for (handle, expected) in &options.stacks {
        let object_id = u16::try_from(*handle).map_err(|_| Error::damaged("X-Ray item handle exceeds 16 bits"))?;
        let expected = u16::try_from(*expected).map_err(|_| Error::damaged("X-Ray item count exceeds 16 bits"))?;
        if !inventory
            .iter()
            .any(|item| item.handle == object_id && item.count == Some(expected))
        {
            return Err(Error::damaged("saved stack count differs after read-back"));
        }
    }
    Ok(())
}

fn verify_s2_money_and_stacks(bytes: &[u8], options: &WriteOptions) -> Result<(), Error> {
    let save = sse_s2::S2Save::from_bytes(bytes)?;
    if let Some(expected) = options.money {
        if save.money() != expected {
            return Err(Error::damaged("saved S2 wallet value differs after read-back"));
        }
    }
    let items = save.items();
    for (handle, expected) in &options.stacks {
        if !items
            .iter()
            .any(|item| item.handle == *handle && item.count == *expected)
        {
            return Err(Error::damaged("saved S2 stack count differs after read-back"));
        }
    }
    if save.container().stored_crc32() != save.container().computed_crc32() {
        return Err(Error::damaged("S2 CRC differs after read-back"));
    }
    Ok(())
}

fn prepare_and_export_s2(
    path: &Path,
    source: &[u8],
    save: &sse_s2::S2Save,
    options: &WriteOptions,
) -> Result<(), WriteFailure> {
    if save.index().is_legacy() {
        return Err(WriteFailure::Core(s2_legacy_write_error()));
    }
    if !options.additions.is_empty()
        || !options.durability.is_empty()
        || !options.upgrades.is_empty()
        || !options.placements.is_empty()
        || !options.moves.is_empty()
        || !options.removals.is_empty()
        || !options.unsupported_operations.is_empty()
    {
        return Err(WriteFailure::Core(Error::Refused(
            "S2 CLI currently supports only money and stack-count edits.".to_owned(),
        )));
    }

    let stack_count = options.stacks.len();
    let change_count = stack_count.saturating_add(usize::from(options.money.is_some()));
    let mut changes = Vec::with_capacity(change_count);
    let mut stack_output = Vec::with_capacity(stack_count);
    if let Some(amount) = options.money {
        changes.push(sse_s2::S2Change::SetMoney(amount));
    }
    for handle in &options.stack_order {
        let count = options
            .stacks
            .get(handle)
            .copied()
            .ok_or_else(|| WriteFailure::Core(Error::damaged("missing parsed S2 stack option")))?;
        changes.push(sse_s2::S2Change::SetStackCount { handle: *handle, count });
        stack_output.push((*handle, count));
    }

    let output = save.write_changes(&changes)?;
    let source_sha256 = sse_codecs::sha256::sha256_hex(source);
    let output_path = options.output.clone().unwrap_or_else(|| default_output_path(path));
    let backup_directory = options
        .backup_directory
        .clone()
        .unwrap_or_else(configured_backup_directory);
    let receipt = publish_cli_write(
        WritePublication {
            source_path: path,
            expected_source_sha256: &source_sha256,
            replacement: &output,
            output_path: &output_path,
            backup_directory: &backup_directory,
            summary: write_edit_summary(options),
            in_place: options.in_place,
        },
        |bytes| verify_s2_money_and_stacks(bytes, options),
    )?;
    println!("Output: {}", receipt.output_path.display());
    println!("Size: {}", receipt.size);
    println!("Backup: {}", receipt.backup_path.display());
    println!("SHA256: {}", receipt.output_sha256);
    if let Some(warning) = receipt.maintenance_warning.as_deref() {
        eprintln!("Warning: backup rotation did not complete: {warning}");
    }
    if let Some(money) = options.money {
        println!("Money: {money}");
    }
    for (handle, count) in stack_output {
        println!("Stack 0x{handle:08X}: {count}");
    }
    Ok(())
}

fn s2_legacy_write_error() -> Error {
    Error::Refused(S2_LEGACY_EDIT_REFUSAL.to_owned())
}

fn write_argument(arguments: &[String], index: usize) -> Result<&str, WriteFailure> {
    arguments
        .get(index)
        .map(String::as_str)
        .ok_or_else(|| WriteFailure::Usage("missing write argument".to_owned()))
}

fn parse_write_options(arguments: &[String], mut index: usize, options: &mut WriteOptions) -> Result<(), WriteFailure> {
    while index < arguments.len() {
        let option = arguments
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| WriteFailure::Usage("missing write option".to_owned()))?;
        index = index
            .checked_add(1)
            .ok_or_else(|| WriteFailure::Usage("write option index overflow".to_owned()))?;
        if option == "--in-place" {
            if options.in_place {
                return Err(WriteFailure::Usage("duplicate --in-place option.".to_owned()));
            }
            options.in_place = true;
            continue;
        }
        let value = arguments
            .get(index)
            .map(String::as_str)
            .ok_or_else(|| WriteFailure::Usage(format!("Missing value after {option}.")))?;
        if value.starts_with("--") {
            return Err(WriteFailure::Usage(format!(
                "Missing value after {option}: got {value}."
            )));
        }
        index = index
            .checked_add(1)
            .ok_or_else(|| WriteFailure::Usage("write option index overflow".to_owned()))?;
        match option {
            "-o" | "--output" => options.output = Some(PathBuf::from(value)),
            "--backup-dir" => options.backup_directory = Some(PathBuf::from(value)),
            "--money" => options.money = Some(parse_unsigned(value)?),
            "--stack" => {
                let (handle, count) = parse_assignment(value)?;
                if options.stacks.insert(handle, count).is_some() {
                    return Err(WriteFailure::Usage(format!("duplicate stack handle 0x{handle:X}")));
                }
                options.stack_order.push(handle);
            }
            "--add" => {
                let (template_and_key, quantity) = parse_string_assignment(value)?;
                let (template_object, item_key) = match template_and_key.split_once(':') {
                    Some((template, key)) => {
                        let template = u16::try_from(parse_unsigned(template)?)
                            .map_err(|_| WriteFailure::Core(Error::damaged("add template handle exceeds 16 bits")))?;
                        (Some(template), key)
                    }
                    None => (None, template_and_key),
                };
                if item_key.is_empty() {
                    return Err(WriteFailure::Core(Error::damaged("item key must not be empty")));
                }
                options.additions.push(AddOption {
                    template_object,
                    item_key: item_key.to_owned(),
                    quantity: parse_unsigned(quantity)?,
                });
            }
            "--durability" => {
                let (handle, condition) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                let condition = condition
                    .parse::<f32>()
                    .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid durability '{condition}'"))))?;
                if options.durability.iter().any(|(previous, _)| *previous == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate durability handle 0x{handle:X}")));
                }
                options.durability.push((handle, condition));
            }
            "--upgrade" => {
                let (handle, requested) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.upgrades.iter().any(|(previous, _)| *previous == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate upgrade handle 0x{handle:X}")));
                }
                options
                    .upgrades
                    .push((handle, requested.split(',').map(str::to_owned).collect()));
            }
            "--placement" => {
                let (handle, placement) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.placements.iter().any(|(previous, _)| *previous == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate placement handle 0x{handle:X}")));
                }
                options.placements.push((handle, parse_placement(placement)?));
            }
            "--move" => {
                let (handle, destination) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                let destination = if destination == "actor" {
                    MoveDestination::Actor
                } else {
                    MoveDestination::Object(parse_unsigned(destination)?)
                };
                options.moves.push((handle, destination));
            }
            "--remove" | "--delete" => {
                options.removals.push(parse_unsigned(value)?);
            }
            "--detach" | "--attach" | "--raw" => {
                options.unsupported_operations.push(option.to_owned());
            }
            _ => return Err(WriteFailure::Usage(format!("Unknown option: {option}."))),
        }
    }
    Ok(())
}

fn parse_placement(value: &str) -> Result<Placement, WriteFailure> {
    match value {
        "ruck" => Ok(Placement::Ruck),
        "belt" => Ok(Placement::Belt),
        _ => {
            let Some(slot) = value.strip_prefix("slot:") else {
                return Err(WriteFailure::Core(Error::damaged(format!(
                    "invalid placement '{value}'"
                ))));
            };
            let slot = slot
                .parse::<u8>()
                .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid slot '{slot}'"))))?;
            Ok(Placement::Slot(slot))
        }
    }
}

fn object_id(handle: u32) -> Result<u16, WriteFailure> {
    u16::try_from(handle).map_err(|_| {
        WriteFailure::Core(Error::Refused(format!(
            "X-Ray object handle 0x{handle:X} exceeds 16 bits"
        )))
    })
}

fn allocate_object_id(reserved: &mut HashSet<u16>, template: u16) -> Result<u16, WriteFailure> {
    let mut candidate = template;
    for _ in 0..u16::MAX {
        candidate = candidate.wrapping_add(1);
        if candidate != 0 && candidate != u16::MAX && reserved.insert(candidate) {
            return Ok(candidate);
        }
    }
    Err(WriteFailure::Core(Error::Refused(
        "no unused X-Ray object id is available".to_owned(),
    )))
}

fn automatic_add_template(save: &Save, item_key: &str) -> Option<u16> {
    let same_section = |record: &&sse_xray::RegistryObject| record.name.eq_ignore_ascii_case(item_key);
    let preferred = save.registry_objects().iter().find(|record| {
        same_section(record)
            && record.story_id == Some(u32::MAX)
            && record.spawn_story_id == Some(u32::MAX)
            && save.custom_data(record).is_some_and(|data| data.is_empty())
    });
    preferred
        .or_else(|| save.registry_objects().iter().find(same_section))
        .map(|record| record.object_id)
}

fn placement_name(placement: Placement) -> String {
    match placement {
        Placement::Ruck => "ruck".to_owned(),
        Placement::Belt => "belt".to_owned(),
        Placement::Slot(slot) => format!("slot:{slot}"),
    }
}

fn format_condition(condition: f32) -> String {
    let mut formatted = format!("{condition:.4}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

fn parse_assignment(value: &str) -> Result<(u32, u32), WriteFailure> {
    let (handle, count) = parse_string_assignment(value)?;
    Ok((parse_unsigned(handle)?, parse_unsigned(count)?))
}

fn parse_string_assignment(value: &str) -> Result<(&str, &str), WriteFailure> {
    let Some((key, assignment)) = value.split_once('=') else {
        return Err(WriteFailure::Core(Error::damaged("expected KEY=VALUE")));
    };
    if key.is_empty() || assignment.is_empty() {
        return Err(WriteFailure::Core(Error::damaged("expected KEY=VALUE")));
    }
    Ok((key, assignment))
}

fn parse_unsigned(value: &str) -> Result<u32, WriteFailure> {
    let parsed = if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else if let Some(binary) = value.strip_prefix("0b").or_else(|| value.strip_prefix("0B")) {
        u32::from_str_radix(binary, 2)
    } else if let Some(octal) = value.strip_prefix("0o").or_else(|| value.strip_prefix("0O")) {
        u32::from_str_radix(octal, 8)
    } else {
        value.parse::<u32>()
    };
    parsed.map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid unsigned integer '{value}'"))))
}

fn default_output_path(source: &Path) -> PathBuf {
    let directory = source.parent().unwrap_or_else(|| Path::new("."));
    let mut name = source
        .file_stem()
        .map_or_else(|| OsString::from("save"), OsString::from);
    name.push("_edited");
    if let Some(extension) = source.extension() {
        name.push(".");
        name.push(extension);
    }
    directory.join(name)
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::indexing_slicing)]
mod write_tests {
    use super::s2_legacy_write_error;
    use super::{allocate_object_id, writer, Save, WriteFailure};
    use crate::read::{read_info, read_inventory, s2_cli_warnings, s2_info_lines, s2_inventory_lines, s2_type_key};
    use crate::run;
    use sse_core::Error;
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sse-cli-write-{id}"));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create CLI test directory");
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn arguments(command: &str, source: &Path, value: &str, output: &Path, backups: &Path) -> Vec<String> {
        vec![
            command.to_owned(),
            source.display().to_string(),
            value.to_owned(),
            "--output".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]
    }

    #[test]
    fn set_money_exports_exact_fixture_and_keeps_the_source_and_original_backup() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let expected = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        fs::write(&source, source_bytes).expect("write source fixture");

        let result = run(&arguments("set-money", &source, "876543", &output, &backups));

        assert_eq!(result, 0);
        assert_eq!(fs::read(&source).expect("source remains readable"), source_bytes);
        assert_eq!(fs::read(&output).expect("export exists"), expected);
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list backups")
            .into_iter()
            .next()
            .expect("journal entry exists");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert_eq!(fs::read(entry.backup_path).expect("original backup"), source_bytes);
    }

    #[test]
    fn set_stack_exports_exact_fixture_for_decimal_and_hex_handles() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-source.sav");
        let expected = include_bytes!("../../../fixtures/synthetic/writer-stacks/xray-stack-soc-expected.sav");
        fs::write(&source, source_bytes).expect("write source fixture");
        let mut args = vec![
            "set-stack".to_owned(),
            source.display().to_string(),
            "0x1234".to_owned(),
            "44".to_owned(),
        ];
        args.extend([
            "-o".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);

        let result = run(&args);

        assert_eq!(result, 0);
        assert_eq!(fs::read(&source).expect("source remains readable"), source_bytes);
        assert_eq!(fs::read(&output).expect("export exists"), expected);
    }

    #[test]
    fn s2_set_money_exports_the_csharp_image_and_original_backup() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");
        fs::write(&source, source_bytes).expect("write S2 source fixture");

        let result = run(&arguments("set-money", &source, "876543", &output, &backups));

        assert_eq!(result, 0);
        assert_eq!(fs::read(&source).expect("source remains readable"), source_bytes);
        let actual = fs::read(&output).expect("S2 export exists");
        let verified = sse_s2::S2Save::from_bytes(&actual).expect("read back S2 export");
        assert_eq!(
            verified.container().image(),
            include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-expected.raw")
        );
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list S2 backups")
            .into_iter()
            .next()
            .expect("S2 export journal exists");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert_eq!(fs::read(entry.backup_path).expect("S2 source backup"), source_bytes);
    }

    #[test]
    fn s2_set_money_in_place_uses_verified_transaction_and_records_edit_summary() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");
        fs::write(&source, source_bytes).expect("write S2 source fixture");
        let args = vec![
            "set-money".to_owned(),
            source.display().to_string(),
            "876543".to_owned(),
            "--in-place".to_owned(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ];

        assert_eq!(run(&args), 0);

        let actual = fs::read(&source).expect("replaced S2 save");
        let verified = sse_s2::S2Save::from_bytes(&actual).expect("read back S2 save");
        assert_eq!(
            verified.container().image(),
            include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-expected.raw")
        );
        assert_eq!(verified.money(), 876_543);
        assert_eq!(
            verified.container().stored_crc32(),
            verified.container().computed_crc32()
        );
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list in-place S2 backups")
            .into_iter()
            .next()
            .expect("in-place transaction journal");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert!(entry.backup_path.to_string_lossy().contains("_ORIGINAL.sav"));
        assert_eq!(
            fs::read(&entry.backup_path).expect("original save backup"),
            source_bytes
        );
        let recovery_path = entry.journal_path.with_file_name(
            entry
                .journal_path
                .file_name()
                .expect("journal file name")
                .to_string_lossy()
                .replace("_ORIGINAL.json", "_EDITED.sav"),
        );
        assert_eq!(fs::read(recovery_path).expect("edited recovery save"), actual);
        let journal = fs::read_to_string(entry.journal_path).expect("read replacement journal");
        assert!(journal.contains("\"status\":\"verified\""));
        assert!(journal.contains("\"mode\":\"replace\",\"money\":876543,\"stack_count\":0"));
    }

    #[test]
    fn s2_set_money_writes_a_stored_kraken_block_when_compression_does_not_win() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let base = include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");
        let parsed = sse_s2::S2Save::from_bytes(base).expect("parse S2 money fixture");
        let mut image = parsed.container().image().to_vec();
        let original_len = image.len();
        image.resize(0x40000, 0);
        let mut state = 0x8c06_cc06_u32;
        for byte in image.iter_mut().skip(original_len) {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            *byte = u8::try_from(state & 0xff).unwrap_or_default();
        }

        let mut source_bytes = Vec::with_capacity(image.len().saturating_add(10));
        source_bytes.extend_from_slice(&u32::try_from(image.len()).unwrap_or_default().to_le_bytes());
        source_bytes.extend_from_slice(&[0xcc, 0x06]);
        source_bytes.extend_from_slice(&image);
        let source_crc = sse_codecs::crc32::crc32(&source_bytes);
        source_bytes.extend_from_slice(&source_crc.to_le_bytes());
        sse_s2::S2Save::from_bytes(&source_bytes).expect("read stored-block S2 source");
        fs::write(&source, &source_bytes).expect("write S2 source");

        let result = run(&arguments("set-money", &source, "876543", &output, &backups));

        assert_eq!(result, 0);
        let actual = fs::read(&output).expect("S2 export exists");
        assert_eq!(actual.get(4..6), Some(&[0xcc, 0x06][..]));
        let verified = sse_s2::S2Save::from_bytes(&actual).expect("read back S2 export");
        assert_eq!(verified.money(), 876_543);
        assert_eq!(
            verified.container().stored_crc32(),
            verified.container().computed_crc32()
        );
        assert!(s2_info_lines(&verified, &actual).iter().any(|line| line == "CRC: OK"));
        assert!(s2_info_lines(&verified, &actual)
            .iter()
            .any(|line| line == "Money: 876543"));
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list S2 backups")
            .into_iter()
            .next()
            .expect("S2 export journal exists");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert_eq!(fs::read(entry.backup_path).expect("S2 source backup"), source_bytes);
    }

    #[test]
    fn s2_set_stack_exports_the_csharp_image_and_original_backup() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav");
        fs::write(&source, source_bytes).expect("write S2 source fixture");
        let mut args = vec![
            "set-stack".to_owned(),
            source.display().to_string(),
            "0x30000001".to_owned(),
            "7".to_owned(),
        ];
        args.extend([
            "-o".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);

        let result = run(&args);

        assert_eq!(result, 0);
        assert_eq!(fs::read(&source).expect("source remains readable"), source_bytes);
        let actual = fs::read(&output).expect("S2 export exists");
        let verified = sse_s2::S2Save::from_bytes(&actual).expect("read back S2 export");
        assert_eq!(
            verified.container().image(),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-expected.raw")
        );
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list S2 backups")
            .into_iter()
            .next()
            .expect("S2 export journal exists");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert_eq!(fs::read(entry.backup_path).expect("S2 source backup"), source_bytes);
    }

    #[test]
    fn legacy_s2_write_refusal_uses_the_reference_text_and_exit_code() {
        let error = s2_legacy_write_error();

        assert_eq!(
            error.to_string(),
            "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again."
        );
        assert_eq!(error.exit_code() as u8, 3);
    }

    #[test]
    fn existing_output_uses_reference_io_exit_code_and_keeps_existing_bytes() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
        )
        .expect("write source fixture");
        fs::write(&output, b"keep existing output").expect("write existing output");

        let result = run(&arguments("set-money", &source, "876543", &output, &backups));

        assert_eq!(result, 5);
        assert_eq!(
            fs::read(&output).expect("existing output remains"),
            b"keep existing output"
        );
        assert!(!backups.exists());
    }

    #[test]
    fn info_and_inventory_accept_synthetic_s2_save() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/synthetic-s2.sav");
        let path = source.display().to_string();
        let bytes = fs::read(&source).expect("read synthetic S2 fixture");
        let save = sse_s2::S2Save::from_bytes(&bytes).expect("parse synthetic S2 fixture");

        assert_eq!(
            s2_info_lines(&save, &bytes).join("\n"),
            concat!(
                "CRC: OK\n",
                "Format: stalker2\n",
                "Packed: 360\n",
                "Raw: 350\n",
                "SHA256: 2fe435d0909e812c362c7577e5adaaef1390e22ea06a46571510530caae92408\n",
                "Money: 100\n",
                "Owned handles: 4\n",
                "Grid handles parsed/total: 2\n",
                "Grid cells: 2\n",
                "Inventory objects: 2\n",
                "Orphans: 2\n",
                "Warning: Handle 0x30000004: неизвестный orphan object kind=99, только read-only"
            )
        );
        assert_eq!(
            s2_inventory_lines(&save).join("\n"),
            concat!(
                "Format: stalker2\n",
                "POS        TYPE                 KEY                       COUNT   HANDLE\n",
                "?          Расходник            010203                          2  0x30000001\n",
                "?          Патроны              040506                          1  0x30000002"
            )
        );

        assert_eq!(run(&["info".to_owned(), path.clone()]), 0);
        assert_eq!(run(&["inventory".to_owned(), path]), 0);
    }

    #[test]
    fn s2_type_keys_use_lowercase_hex_like_the_reference() {
        assert_eq!(s2_type_key([0x05, 0x4c, 0x00]), "054c00");
    }

    #[test]
    fn s2_warning_lines_match_the_reference() {
        assert_eq!(
            s2_cli_warnings(
                true,
                &[
                    "Handle 0x30000001: неизвестный object kind=3, только read-only".to_owned(),
                    "Save uses the game 1.0.x layout".to_owned(),
                ],
                false,
            ),
            vec![
                "Warning: Handle 0x30000001: неизвестный object kind=3, только read-only".to_owned(),
                format!("Warning: {}", crate::S2_LEGACY_WARNING),
            ]
        );
    }

    #[test]
    fn s2_info_reports_unmatched_grid_cells_like_the_reference() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/writer-s2-stash/s2-stash-truncated.sav");
        let bytes = fs::read(&source).expect("read truncated S2 stash fixture");
        let save = sse_s2::S2Save::from_bytes(&bytes).expect("parse truncated S2 stash fixture");

        let lines = s2_info_lines(&save, &bytes);
        let unmatched = lines
            .iter()
            .position(|line| line == "Warning: Не удалось сопоставить ни одной grid cell с object record")
            .expect("report unmatched grid cells");
        let owned = lines
            .iter()
            .position(|line| line.starts_with("Warning: Owned handle "))
            .expect("report unresolved owned handles");

        assert!(
            unmatched < owned,
            "reference prints the grid warning before owned-handle warnings"
        );
    }

    #[test]
    fn non_save_image_reports_reference_unknown_format_error() {
        let temporary = TempDirectory::new();
        let image = temporary.0.join("thumbnail.png");
        let png_header = [137_u8, 80, 78, 71, 13, 10, 26, 10];
        fs::write(&image, png_header).expect("write synthetic thumbnail header");
        let path = image.display().to_string();

        let result = read_info(Some(&path));

        assert_eq!(
            result,
            Err(super::Error::damaged("Unknown or unsupported save format."))
        );
        assert_eq!(
            read_inventory(Some(&path)),
            Err(super::Error::damaged("Unknown or unsupported save format."))
        );
    }

    #[test]
    fn backup_cli_lists_and_restores_the_original_fixture_byte_for_byte() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let restored = saves.join("restored.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        fs::write(&source, source_bytes).expect("write source fixture");
        assert_eq!(run(&arguments("set-money", &source, "876543", &output, &backups)), 0);

        let entries = sse_storage::transaction::list_backups(&backups).expect("list generated backups");
        let entry = entries.first().expect("export produced a backup");
        assert_eq!(entry.status, sse_storage::transaction::BackupStatus::Verified);
        assert_eq!(
            run(&[
                "backups".to_owned(),
                "list".to_owned(),
                "--backup-dir".to_owned(),
                backups.display().to_string()
            ]),
            0
        );
        assert_eq!(
            run(&[
                "backups".to_owned(),
                "restore".to_owned(),
                entry.journal_path.display().to_string(),
                "--output".to_owned(),
                restored.display().to_string(),
            ]),
            0
        );
        assert_eq!(fs::read(&restored).expect("restored save"), source_bytes);
    }

    #[test]
    fn edit_composes_money_and_stack_and_bad_syntax_uses_exit_two() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        fs::write(&source, source_bytes).expect("write source fixture");
        let args = vec![
            "edit".to_owned(),
            source.display().to_string(),
            "--money".to_owned(),
            "876543".to_owned(),
            "--stack".to_owned(),
            "4660=44".to_owned(),
            "-o".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ];

        assert_eq!(run(&args), 0);
        let parsed = Save::read(&fs::read(&output).expect("combined export exists")).expect("read output");
        assert_eq!(parsed.money().expect("money"), 876_543);
        assert_eq!(
            parsed
                .inventory()
                .expect("inventory")
                .iter()
                .find(|item| item.handle == 0x1234)
                .and_then(|item| item.count),
            Some(44)
        );
        assert_eq!(run(&["set-stack".to_owned(), source.display().to_string()]), 2);
    }

    #[test]
    fn edit_refuses_unmatched_templates_and_unsupported_options() {
        let temporary = TempDirectory::new();
        let source = temporary.0.join("source.sav");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
        )
        .expect("write source fixture");
        let add = vec![
            "edit".to_owned(),
            source.display().to_string(),
            "--add".to_owned(),
            "bandage=1".to_owned(),
        ];
        let unsupported = vec![
            "edit".to_owned(),
            source.display().to_string(),
            "--move".to_owned(),
            "0x1234=0x0010".to_owned(),
        ];
        let attach = vec![
            "edit".to_owned(),
            source.display().to_string(),
            "--attach".to_owned(),
            "0x1234=0,0,1,1".to_owned(),
        ];
        let invalid_number = vec![
            "set-money".to_owned(),
            "missing.sav".to_owned(),
            "not-a-number".to_owned(),
        ];
        assert_eq!(run(&add), 3);
        assert_eq!(run(&unsupported), 3);
        assert_eq!(run(&attach), 3);
        assert_eq!(run(&invalid_number), 4);
    }

    #[test]
    fn explicit_add_template_must_match_the_requested_section() {
        let temporary = TempDirectory::new();
        let source = temporary.0.join("source.sav");
        let output = temporary.0.join("edited.sav");
        let backups = temporary.0.join("backups");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav"),
        )
        .expect("write X-Ray add fixture");

        let result = run(&[
            "edit".to_owned(),
            source.display().to_string(),
            "--add".to_owned(),
            "0x1234:exo_outfit=1".to_owned(),
            "--output".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);

        assert_eq!(result, 3);
        assert!(!output.exists());
    }

    #[test]
    fn repeated_money_or_stack_options_are_refused_not_dropped() {
        let temporary = TempDirectory::new();
        let source = temporary.0.join("source.sav");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
        )
        .expect("write source fixture");
        let both_money = vec![
            "set-money".to_owned(),
            source.display().to_string(),
            "100".to_owned(),
            "--money".to_owned(),
            "5".to_owned(),
        ];
        let both_stack = vec![
            "set-stack".to_owned(),
            source.display().to_string(),
            "0x1234".to_owned(),
            "9".to_owned(),
            "--stack".to_owned(),
            "0x1234=3".to_owned(),
        ];
        assert_eq!(run(&both_money), 2);
        assert_eq!(run(&both_stack), 2);
    }

    #[test]
    fn option_value_that_looks_like_an_option_is_refused() {
        let temporary = TempDirectory::new();
        let source = temporary.0.join("source.sav");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
        )
        .expect("write source fixture");
        let args = vec![
            "edit".to_owned(),
            source.display().to_string(),
            "--money".to_owned(),
            "5".to_owned(),
            "--backup-dir".to_owned(),
            "--in-place".to_owned(),
        ];
        assert_eq!(run(&args), 2);
    }

    #[test]
    fn object_id_allocator_skips_the_alife_sentinel() {
        let mut reserved = HashSet::new();
        assert!(matches!(allocate_object_id(&mut reserved, u16::MAX - 1), Ok(1)));
        assert!(!reserved.contains(&u16::MAX));
    }

    #[test]
    fn object_id_allocator_refuses_a_registry_filled_through_0xfffe() {
        let mut reserved = (1..u16::MAX).collect::<HashSet<_>>();
        let before = reserved.len();

        let error = allocate_object_id(&mut reserved, u16::MAX - 1)
            .expect_err("all non-sentinel object ids through 0xFFFE are occupied");

        assert!(matches!(
            error,
            WriteFailure::Core(Error::Refused(message)) if message.contains("no unused X-Ray object id")
        ));
        assert_eq!(reserved.len(), before);
        assert!(!reserved.contains(&u16::MAX));
    }

    #[test]
    fn edit_durability_and_placement_match_writer_fixtures_byte_for_byte() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let backups = temporary.0.join("backups");
        let durability_source = saves.join("durability.sav");
        let durability_output = saves.join("durability-edited.sav");
        let durability_bytes =
            include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        fs::write(&durability_source, durability_bytes).expect("write durability fixture");
        assert_eq!(
            run(&[
                "edit".to_owned(),
                durability_source.display().to_string(),
                "--durability".to_owned(),
                "0x3456=0.75".to_owned(),
                "--output".to_owned(),
                durability_output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            0
        );
        assert_eq!(
            fs::read(&durability_output).expect("durability output"),
            include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-cop-expected.sav")
        );

        let placement_source = saves.join("placement.sav");
        let placement_output = saves.join("placement-edited.sav");
        let placement_bytes =
            include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-source.sav");
        fs::write(&placement_source, placement_bytes).expect("write placement fixture");
        assert_eq!(
            run(&[
                "edit".to_owned(),
                placement_source.display().to_string(),
                "--placement".to_owned(),
                "0x3456=slot:3".to_owned(),
                "--output".to_owned(),
                placement_output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            0
        );
        assert_eq!(
            fs::read(&placement_output).expect("placement output"),
            include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-cop-expected.sav")
        );
    }

    #[test]
    fn edit_reads_upgrade_state_and_refuses_keys_missing_from_the_catalog() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cs-source.sav");
        fs::write(&source, source_bytes).expect("write upgrade fixture");
        let parsed = Save::read(source_bytes).expect("parse upgrade fixture");
        assert_eq!(
            writer::current_upgrades(&parsed, 0x3456).expect("read current upgrade vector"),
            vec!["up_a_wpn_test".to_owned(), "legacy_unknown".to_owned()]
        );

        assert_eq!(
            run(&[
                "edit".to_owned(),
                source.display().to_string(),
                "--upgrade".to_owned(),
                "0x3456=legacy_unknown,up_c_wpn_test".to_owned(),
                "--output".to_owned(),
                output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            3
        );
        assert!(!output.exists());
    }

    #[test]
    fn edit_add_clears_template_metadata_and_preserves_source() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav");
        fs::write(&source, source_bytes).expect("write add fixture");

        assert_eq!(
            run(&[
                "edit".to_owned(),
                source.display().to_string(),
                "--add".to_owned(),
                "0x1234:ammo_9x39_pab9=17".to_owned(),
                "--output".to_owned(),
                output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            0
        );
        assert_eq!(fs::read(&source).expect("source remains unchanged"), source_bytes);
        let output_bytes = fs::read(&output).expect("add output");
        let parsed = Save::read(&output_bytes).expect("edited save should read back");
        let added = parsed
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4661)
            .expect("new object should be present");
        assert_eq!(added.name, "ammo_9x39_pab9");
        assert_eq!(added.name_replace, "");
        assert_eq!(added.spawn_id, Some(u16::MAX));
        assert_eq!(added.story_id, Some(u32::MAX));
        assert_eq!(added.spawn_story_id, Some(u32::MAX));
        assert_eq!(parsed.custom_data(added), Some(&[][..]));
        let inventory = parsed.inventory().expect("edited inventory");
        let added_stack = inventory
            .iter()
            .find(|item| item.handle == 4661)
            .expect("new stack should be in the actor inventory");
        assert_eq!(added_stack.count, Some(17));
    }

    #[test]
    fn edit_add_without_template_selects_when_multiple_same_section_candidates_exist() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let fixture = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav");
        let initial = Save::read(fixture).expect("read add fixture");
        assert_eq!(
            super::automatic_add_template(&initial, "ammo_9x39_pab9"),
            Some(4660),
            "a sole same-section template remains the fallback even with metadata"
        );
        assert_eq!(super::automatic_add_template(&initial, "missing_section"), None);
        let seed = writer::apply(
            &initial,
            &writer::ChangeSet::new(vec![writer::Change::AddItem {
                template_object: 4660,
                item_key: "ammo_9x39_pab9".to_owned(),
                object_id: 4661,
                quantity: 1,
            }]),
        )
        .expect("seed a second same-section template");
        let seeded = Save::read(seed.as_slice()).expect("read seeded save");
        let quest_template = seeded
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4660)
            .expect("fixture's original candidate should exist");
        assert_ne!(quest_template.story_id, Some(u32::MAX));
        assert_ne!(quest_template.spawn_story_id, Some(u32::MAX));
        assert_ne!(seeded.custom_data(quest_template), Some(&[][..]));
        let safe_template = seeded
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4661)
            .expect("seeded candidate should exist");
        assert_eq!(safe_template.story_id, Some(u32::MAX));
        assert_eq!(safe_template.spawn_story_id, Some(u32::MAX));
        assert_eq!(seeded.custom_data(safe_template), Some(&[][..]));
        assert_eq!(
            seeded
                .registry_objects()
                .iter()
                .filter(|record| record.name.eq_ignore_ascii_case("ammo_9x39_pab9"))
                .count(),
            2,
            "test input must have multiple candidates for the requested section"
        );
        assert_eq!(
            super::automatic_add_template(&seeded, "ammo_9x39_pab9"),
            Some(4661),
            "prefer the same-section candidate without story metadata or custom data"
        );
        fs::write(&source, seed.as_slice()).expect("write seeded save");

        let result = run(&[
            "edit".to_owned(),
            source.display().to_string(),
            "--add".to_owned(),
            "ammo_9x39_pab9=7".to_owned(),
            "--output".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);

        assert_eq!(result, 0, "CLI should select a template when several match");
        let output_bytes = fs::read(&output).expect("read edited output");
        let output_save = Save::read(&output_bytes).expect("parse edited output");
        assert_eq!(
            output_save
                .registry_objects()
                .iter()
                .filter(|record| record.name.eq_ignore_ascii_case("ammo_9x39_pab9"))
                .count(),
            3,
            "CLI should add one object after selecting among the two candidates"
        );
        assert!(output_save
            .inventory()
            .expect("read edited inventory")
            .iter()
            .any(|item| item.count == Some(7)));
    }

    #[test]
    fn edit_stash_move_matches_writer_fixture_byte_for_byte() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-source.sav"),
        )
        .expect("write stash fixture");

        assert_eq!(
            run(&[
                "edit".to_owned(),
                source.display().to_string(),
                "--move".to_owned(),
                "0x2345=actor".to_owned(),
                "--output".to_owned(),
                output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            0
        );
        assert_eq!(
            fs::read(&output).expect("stash move output"),
            include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-cop-take.sav")
        );
    }

    #[test]
    fn edit_remove_refuses_story_linked_fixture_object() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-source.sav"),
        )
        .expect("write removal fixture");

        let result = run(&[
            "edit".to_owned(),
            source.display().to_string(),
            "--remove".to_owned(),
            "0x1234".to_owned(),
            "--output".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);
        assert_eq!(result, 3);
        assert!(!output.exists());
    }
}
