//! `stalker-save`: the command line. It is the parity surface: for the same input it prints what the C# command
//! line prints (see `tools/oracle.sh`), so every reader and writer is compared with the released editor.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sse_core::{Error, ExitCode as CommandExitCode, SaveBuffer};
use sse_xray::writer::{self, Change, ChangeSet, Placement};
use sse_xray::Save;

mod audit;
mod fixes;
mod lint;
mod update;

const USAGE: &str = "Usage: stalker-save <version|info|inventory|set-money|set-stack|edit|doctor|backups|fixes|update|lint|audit> ...\n\
Write commands accept --in-place for journaled replacement; do not combine it with --output.\n\
Exit codes: 0 done, 2 wrong arguments, 3 refused (unsupported or unsafe), 4 unreadable or damaged input, 5 file or system error.";
const S2_LEGACY_WARNING: &str = "Сохранение записано игрой версии 1.0.x: показаны деньги и предметы в сетке рюкзака; надетое снаряжение и состояние предметов не читаются, правка недоступна.";
const S2_LEGACY_EDIT_REFUSAL: &str = "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again.";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if let Some(worker_exit) = sse_steam::worker::run_if_worker(&arguments) {
        return worker_exit;
    }
    ExitCode::from(run(&arguments))
}

fn run(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("version") if arguments.len() == 1 => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            sse_core::ExitCode::Done as u8
        }
        Some("info") if arguments.len() == 2 => report(read_info(arguments.get(1))),
        Some("inventory")
            if arguments.len() == 2
                || (arguments.len() == 3 && arguments.get(2).map(String::as_str) == Some("--all")) =>
        {
            report(read_inventory(arguments.get(1)))
        }
        Some("audit") if arguments.len() == 2 => report(audit::run(arguments.get(1).map(String::as_str))),
        Some("audit") => {
            eprintln!("Error: audit requires a file containing one save path per line. {USAGE}");
            sse_core::ExitCode::Usage as u8
        }
        Some("doctor")
            if (arguments.len() == 3 || (arguments.len() == 4 && is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("save") =>
        {
            report(doctor_save(arguments.get(2), is_json(arguments)))
        }
        Some("doctor") if arguments.len() >= 3 && arguments.get(1).map(String::as_str) == Some("crash") => {
            let Some((game, json)) = parse_crash_options(arguments) else {
                eprintln!("{USAGE}");
                return sse_core::ExitCode::Usage as u8;
            };
            report(doctor_crash(arguments.get(2), game, json))
        }
        Some("doctor")
            if (arguments.len() == 4 || (arguments.len() == 5 && is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("game") =>
        {
            report(doctor_game(arguments.get(2), arguments.get(3), is_json(arguments)))
        }
        Some("doctor")
            if (arguments.len() == 3 || (arguments.len() == 4 && is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("quests") =>
        {
            report(doctor_quests(arguments.get(2), is_json(arguments)))
        }
        Some("set-money" | "set-stack" | "edit") => run_write(arguments),
        Some("backups") => run_backups(arguments.get(1..).unwrap_or_default()),
        Some("info" | "inventory") => {
            eprintln!("Error: Invalid arguments. {USAGE}");
            sse_core::ExitCode::Usage as u8
        }
        Some("fixes") => fixes::run_fixes(arguments.get(1..).unwrap_or_default()) as u8,
        Some("lint") => lint::run_lint(arguments.get(1..).unwrap_or_default()) as u8,
        Some("update") => update::run_update(arguments.get(1..).unwrap_or_default()) as u8,
        Some(_) => {
            eprintln!("{USAGE}");
            sse_core::ExitCode::Usage as u8
        }
        None => {
            eprintln!("{USAGE}");
            sse_core::ExitCode::Usage as u8
        }
    }
}

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

fn run_write(arguments: &[String]) -> u8 {
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
        "set-money" => options.money = positional_money,
        "set-stack" => {
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
            None => {
                let matching = save
                    .registry_objects()
                    .iter()
                    .filter(|record| {
                        record.name.eq_ignore_ascii_case(&addition.item_key)
                            || record.name_replace.eq_ignore_ascii_case(&addition.item_key)
                    })
                    .map(|record| record.object_id)
                    .collect::<Vec<_>>();
                match matching.as_slice() {
                    [handle] => *handle,
                    [] => {
                        return Err(WriteFailure::Core(Error::Refused(format!(
                            "no confirmed registry template matches item key '{}' (use TEMPLATE:KEY=COUNT)",
                            addition.item_key
                        ))));
                    }
                    _ => {
                        return Err(WriteFailure::Core(Error::Refused(format!(
                            "item key '{}' matches multiple registry templates; specify TEMPLATE:KEY=COUNT",
                            addition.item_key
                        ))));
                    }
                }
            }
        };
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
        .unwrap_or_else(default_backup_directory);
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
        let (receipt, ()) = sse_storage::transaction::replace_transaction_with_summary_and_verifier(
            source_path,
            expected_source_sha256,
            replacement,
            backup_directory,
            summary,
            verify_readback,
        )?;
        return Ok(PublishedWrite {
            output_path: receipt.source_path,
            backup_path: receipt.backup_path,
            output_sha256: receipt.output_sha256,
            size: replacement.len(),
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
        .unwrap_or_else(default_backup_directory);
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
        if candidate != 0 && reserved.insert(candidate) {
            return Ok(candidate);
        }
    }
    Err(WriteFailure::Core(Error::Refused(
        "no unused X-Ray object id is available".to_owned(),
    )))
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

fn default_backup_directory() -> PathBuf {
    #[cfg(target_os = "windows")]
    let data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local")));
    #[cfg(target_os = "macos")]
    let data = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    data.unwrap_or_else(|| std::env::temp_dir().join("StalkerSaveEditorData"))
        .join("StalkerSaveEditor")
        .join("backups")
}

fn report(result: sse_core::Result<()>) -> u8 {
    match result {
        Ok(()) => sse_core::ExitCode::Done as u8,
        Err(error) => {
            eprintln!("Error: {error}");
            error.exit_code() as u8
        }
    }
}

fn read_info(path: Option<&String>) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    if let Ok(save) = sse_s2::S2Save::from_bytes(packed.as_slice()) {
        print_lines(s2_info_lines(&save, packed.as_slice()));
        return Ok(());
    }
    let save = read_xray_or_unsupported(packed.as_slice())?;
    println!("Integrity: X-Ray LZO/container OK");
    println!("Format: {}", save.format().id());
    println!("Packed: {}", packed.len());
    println!("Raw: {}", save.raw_size());
    println!("SHA256: {}", sse_codecs::sha256::sha256_hex(packed.as_slice()));
    println!("Money: {}", save.money()?);
    println!("Inventory objects: {}", save.inventory()?.len());
    Ok(())
}

fn read_inventory(path: Option<&String>) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    if let Ok(save) = sse_s2::S2Save::from_bytes(packed.as_slice()) {
        print_lines(s2_inventory_lines(&save));
        return Ok(());
    }
    let save = read_xray_or_unsupported(packed.as_slice())?;
    println!("Format: {}", save.format().id());
    println!("POS        TYPE                 KEY                       COUNT   HANDLE");
    for item in save.inventory()? {
        let position = item.placement.as_deref().unwrap_or("inventory");
        let count = item
            .count
            .map_or_else(|| "unknown".to_owned(), |value| value.to_string());
        println!(
            "{position:<10} {:<20} {:<25} {count:>7}  0x{:04X}",
            item.category, item.section, item.handle
        );
    }
    Ok(())
}

fn read_xray_or_unsupported(packed: &[u8]) -> sse_core::Result<Save> {
    Save::read(packed).map_err(|error| {
        if packed.starts_with(&u32::MAX.to_le_bytes()) {
            error
        } else {
            Error::damaged("Unknown or unsupported save format.")
        }
    })
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        println!("{line}");
    }
}

fn s2_info_lines(save: &sse_s2::S2Save, packed: &[u8]) -> Vec<String> {
    let index = save.index();
    let items = save.items();
    let orphans = save.orphans();
    let mut lines = vec![
        "CRC: OK".to_owned(),
        "Format: stalker2".to_owned(),
        format!("Packed: {}", save.container().packed_size()),
        format!("Raw: {}", save.container().image().len()),
        format!("SHA256: {}", sse_codecs::sha256::sha256_hex(packed)),
        format!("Money: {}", save.money()),
        format!("Owned handles: {}", index.owned_handles().len()),
        format!("Grid handles parsed/total: {}", index.grid_handle_count()),
        format!("Grid cells: {}", index.grid_cells().len()),
        format!("Inventory objects: {}", items.len()),
        format!("Orphans: {}", orphans.len()),
    ];
    let unmatched_grid_cells = items.is_empty() && index.grid_handle_count() > 0;
    lines.extend(s2_cli_warnings(
        index.is_legacy(),
        save.warnings(),
        unmatched_grid_cells,
    ));
    lines
}

fn s2_cli_warnings(is_legacy: bool, warnings: &[String], unmatched_grid_cells: bool) -> Vec<String> {
    // C# prints every reader warning and puts the 1.0.x layout warning, in Russian, last.
    let mut lines: Vec<String> = warnings
        .iter()
        .filter(|warning| !warning.contains("1.0.x"))
        .map(|warning| format!("Warning: {warning}"))
        .collect();
    if unmatched_grid_cells {
        let insert_at = lines
            .iter()
            .position(|line| line.starts_with("Warning: Owned handle "))
            .unwrap_or(lines.len());
        lines.insert(
            insert_at,
            "Warning: Не удалось сопоставить ни одной grid cell с object record".to_owned(),
        );
    }
    if is_legacy || warnings.iter().any(|warning| warning.contains("1.0.x")) {
        lines.push(format!("Warning: {S2_LEGACY_WARNING}"));
    }
    lines
}

fn s2_inventory_lines(save: &sse_s2::S2Save) -> Vec<String> {
    let mut lines = vec![
        "Format: stalker2".to_owned(),
        "POS        TYPE                 KEY                       COUNT   HANDLE".to_owned(),
    ];
    lines.extend(save.items().into_iter().map(|item| {
        let position = if item.x.is_some() {
            "?"
        } else if matches!(item.kind_code, 6 | 8 | 10 | 11) {
            "у персонажа"
        } else {
            "экипировано"
        };
        let key = s2_type_key(item.type_key);
        let category = s2_category_name(item.kind_code, item.display_name.as_deref());
        format!(
            "{position:<10} {category:<20} {key:<25} {:>7}  0x{:08X}",
            item.count, item.handle
        )
    }));
    lines
}

fn s2_type_key(key: [u8; 3]) -> String {
    format!("{:02x}{:02x}{:02x}", key[0], key[1], key[2])
}

fn s2_category_name(kind: u8, display_name: Option<&str>) -> String {
    let normalized = display_name.unwrap_or_default().trim().to_lowercase();
    if normalized.starts_with("nvg_")
        || normalized.starts_with("binocular")
        || normalized.starts_with("пнв")
        || normalized.starts_with("бинокль")
        || normalized.starts_with("бинокл")
    {
        return "Устройство".to_owned();
    }
    if normalized.contains("_upgrade_") || normalized.contains("_attachment_") {
        return "Модуль/улучшение".to_owned();
    }
    if normalized.ends_with("_armor") || normalized.ends_with("_helmet") {
        return "Броня/экипировка".to_owned();
    }
    if normalized.contains("_armor_") || normalized.contains("_helmet_") || normalized.starts_with("gunbucket_") {
        return "Разное".to_owned();
    }

    match kind {
        0 => "Оружие".to_owned(),
        1 => "Броня/экипировка".to_owned(),
        2 => "Артефакт".to_owned(),
        4 => "Расходник".to_owned(),
        5 => "Патроны".to_owned(),
        6 => "Детектор".to_owned(),
        7 => "Гранаты".to_owned(),
        8 => "Разное".to_owned(),
        10 => "ПНВ".to_owned(),
        11 => "Бинокль".to_owned(),
        _ => format!("Тип {kind}"),
    }
}

fn run_backups(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("list") => {
            let mut directory = default_backup_directory();
            let mut options = arguments.iter().skip(1);
            while let Some(option) = options.next() {
                if option != "--backup-dir" {
                    eprintln!("Error: Unknown backups list option: {option}.");
                    return CommandExitCode::Usage as u8;
                }
                let Some(value) = options.next() else {
                    eprintln!("Error: Missing value after --backup-dir.");
                    return CommandExitCode::Usage as u8;
                };
                directory = PathBuf::from(value);
            }
            match sse_storage::transaction::list_backups(&directory) {
                Ok(entries) => {
                    for entry in entries {
                        println!(
                            "{:?}  {}  <-  {}",
                            entry.status,
                            entry.backup_path.display(),
                            entry.source_path.display()
                        );
                        if let Some(error) = entry.error {
                            println!("  Error: {error}");
                        }
                    }
                    CommandExitCode::Done as u8
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    error.exit_code() as u8
                }
            }
        }
        Some("restore") if arguments.len() == 4 && arguments.get(2).map(String::as_str) == Some("--output") => {
            let Some(journal) = arguments.get(1) else {
                eprintln!("Error: Missing backup journal path.");
                return CommandExitCode::Usage as u8;
            };
            let Some(output) = arguments.get(3) else {
                eprintln!("Error: Missing restore output path.");
                return CommandExitCode::Usage as u8;
            };
            match sse_storage::transaction::restore_backup(Path::new(journal), Path::new(output)) {
                Ok(path) => {
                    println!("Restored: {}", path.display());
                    CommandExitCode::Done as u8
                }
                Err(error) => {
                    eprintln!("Error: {error}");
                    error.exit_code() as u8
                }
            }
        }
        _ => {
            eprintln!("Error: Usage: stalker-save backups <list [--backup-dir DIR]|restore JOURNAL --output SAVE>");
            CommandExitCode::Usage as u8
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{run, USAGE};

    #[test]
    fn audit_is_listed_and_requires_a_path_list_argument() {
        assert!(USAGE.contains("audit"));
        assert_eq!(run(&["audit".to_owned()]), 2);
    }
}

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::indexing_slicing)]
mod write_tests {
    use super::s2_legacy_write_error;
    use super::{
        read_info, read_inventory, run, s2_cli_warnings, s2_info_lines, s2_inventory_lines, s2_type_key, writer, Save,
    };
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
                format!("Warning: {}", super::S2_LEGACY_WARNING),
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
    fn edit_add_matches_writer_fixture_byte_for_byte() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("source.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        fs::write(
            &source,
            include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav"),
        )
        .expect("write add fixture");

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
        assert_eq!(
            fs::read(&output).expect("add output"),
            include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-expected.sav")
        );
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
    fn edit_remove_matches_writer_fixture_byte_for_byte() {
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

        assert_eq!(
            run(&[
                "edit".to_owned(),
                source.display().to_string(),
                "--remove".to_owned(),
                "0x1234".to_owned(),
                "--output".to_owned(),
                output.display().to_string(),
                "--backup-dir".to_owned(),
                backups.display().to_string(),
            ]),
            0
        );
        assert_eq!(
            fs::read(&output).expect("removal output"),
            include_bytes!("../../../fixtures/synthetic/writer-delete/xray-delete-soc-expected.sav")
        );
    }
}

fn is_json(arguments: &[String]) -> bool {
    arguments.last().map(String::as_str) == Some("--json")
}

fn parse_crash_options(arguments: &[String]) -> Option<(Option<&String>, bool)> {
    let mut game = None;
    let mut json = false;
    let mut cursor = 3_usize;
    while cursor < arguments.len() {
        match arguments.get(cursor)?.as_str() {
            "--json" => {
                json = true;
                cursor = cursor.checked_add(1)?;
            }
            "--game" => {
                let value_at = cursor.checked_add(1)?;
                let value = arguments.get(value_at)?;
                if value.starts_with("--") {
                    return None;
                }
                game = Some(value);
                cursor = cursor.checked_add(2)?;
            }
            _ => return None,
        }
    }
    Some((game, json))
}

fn doctor_save(path: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    let report = sse_doctor::analyze_save(packed.as_slice());
    if json {
        println!("{}", json_save_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged("save structure could not be validated"));
        }
        return Ok(());
    }
    println!("Save Doctor: {:?}", report.status);
    println!("Format: {}", report.format_id.unwrap_or("unknown"));
    println!(
        "Objects: {}",
        report
            .object_count
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    println!(
        "Inventory objects: {}",
        report
            .inventory_count
            .map_or_else(|| "unknown".to_owned(), |count| count.to_string())
    );
    for finding in &report.findings {
        println!(
            "{:?} [{}] {}: {}",
            finding.severity, finding.id, finding.looked_at, finding.found
        );
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        let detail = report.findings.first().map_or_else(
            || "save structure could not be validated".to_owned(),
            |finding| finding.found.clone(),
        );
        return Err(Error::damaged(detail));
    }
    Ok(())
}

fn doctor_crash(path: Option<&String>, game: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing crash-log path"))?;
    let game = game.map(String::as_str);
    let analysis = sse_doctor::analyze_crash_file(Path::new(path), game)?;
    if json {
        println!("{}", json_crash_analysis(&analysis)?);
        return Ok(());
    }
    println!("Crash kind: {:?}", analysis.kind);
    println!("Summary: {}", analysis.summary);
    if let Some(issue) = analysis.known_issue {
        println!("Known issue: {} — {}", issue.id, issue.title);
        println!("Advice: {:?}", issue.advice);
    } else {
        println!("Known issue: none");
    }
    Ok(())
}

fn doctor_game(target: Option<&String>, directory: Option<&String>, json: bool) -> sse_core::Result<()> {
    let target = target
        .and_then(|id| sse_doctor::GameTarget::parse(id))
        .ok_or_else(|| Error::Refused("unknown game target".to_owned()))?;
    let directory = directory.ok_or_else(|| Error::damaged("missing game directory"))?;
    let report = sse_doctor::analyze_game_install_from_steam(target, Path::new(directory));
    if json {
        println!("{}", json_game_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged(
                "selected directory does not match the requested game target",
            ));
        }
        return Ok(());
    }
    println!("Game Doctor: {:?}", report.status);
    println!("Target: {}", target.id());
    println!("Directory: {}", report.directory.display());
    println!("Game marker: {}", report.marker_found);
    println!("Build fingerprint: {:?}", report.build.status);
    for finding in &report.findings {
        println!(
            "{:?} [{}] {}: {}",
            finding.severity, finding.id, finding.looked_at, finding.found
        );
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        return Err(Error::damaged(
            "selected directory does not match the requested game target",
        ));
    }
    Ok(())
}

fn doctor_quests(path: Option<&String>, json: bool) -> sse_core::Result<()> {
    let path = path.ok_or_else(|| Error::damaged("missing save path"))?;
    let packed = SaveBuffer::read(Path::new(path))?;
    let report = sse_doctor::analyze_quests(packed.as_slice());
    if json {
        println!("{}", json_quest_report(&report)?);
        if report.status == sse_doctor::SaveDoctorStatus::Error {
            return Err(Error::damaged(report.summary));
        }
        return Ok(());
    }
    println!("Quest Doctor: {:?}", report.status);
    println!("Format: {}", report.format_id.unwrap_or("unknown"));
    println!("Summary: {}", report.summary);
    for state in &report.states {
        println!("{:?} [{}] {}: {}", state.status, state.id, state.title, state.detail);
    }
    if report.status == sse_doctor::SaveDoctorStatus::Error {
        return Err(Error::damaged(report.summary));
    }
    Ok(())
}

#[cfg(test)]
fn json_string(value: &str) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.string(value)?;
    finish_json(writer)
}

fn finish_json(writer: sse_codecs::json::Writer) -> sse_core::Result<String> {
    String::from_utf8(writer.finish()?).map_err(|_| Error::damaged("JSON writer returned invalid UTF-8"))
}

fn json_write_string(writer: &mut sse_codecs::json::Writer, value: &str) -> sse_core::Result<()> {
    writer.string(value)
}

fn json_write_optional_string(writer: &mut sse_codecs::json::Writer, value: Option<&str>) -> sse_core::Result<()> {
    match value {
        Some(value) => json_write_string(writer, value),
        None => writer.null(),
    }
}

fn json_write_optional_count(writer: &mut sse_codecs::json::Writer, value: Option<usize>) -> sse_core::Result<()> {
    match value {
        Some(value) => writer.u64(u64::try_from(value).map_err(|_| Error::damaged("JSON count does not fit u64"))?),
        None => writer.null(),
    }
}

fn json_write_optional_i32(writer: &mut sse_codecs::json::Writer, value: Option<i32>) -> sse_core::Result<()> {
    match value {
        Some(value) => writer.i64(i64::from(value)),
        None => writer.null(),
    }
}

fn json_write_change(writer: &mut sse_codecs::json::Writer, change: &Change) -> sse_core::Result<()> {
    writer.object_start()?;
    match change {
        Change::SetMoney {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setMoney")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.u64(u64::from(*old_value))?;
            writer.key("newValue")?;
            writer.u64(u64::from(*new_value))?;
        }
        Change::SetStack {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setStack")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.u64(u64::from(*old_value))?;
            writer.key("newValue")?;
            writer.u64(u64::from(*new_value))?;
        }
        Change::SetDurability {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setDurability")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.number(&old_value.to_string())?;
            writer.key("newValue")?;
            writer.number(&new_value.to_string())?;
        }
        Change::SetPlacement {
            target_object,
            destination,
        } => {
            writer.key("kind")?;
            writer.string("setPlacement")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("destination")?;
            match destination {
                Placement::Ruck => writer.string("ruck")?,
                Placement::Belt => writer.string("belt")?,
                Placement::Slot(slot) => writer.string(&format!("slot:{slot}"))?,
            }
        }
        Change::MoveItem {
            target_object,
            old_parent,
            new_parent,
        } => {
            writer.key("kind")?;
            writer.string("moveItem")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldParent")?;
            writer.u64(u64::from(*old_parent))?;
            writer.key("newParent")?;
            writer.u64(u64::from(*new_parent))?;
        }
        Change::RemoveItem { target_object } => {
            writer.key("kind")?;
            writer.string("removeItem")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
        }
        Change::AddItem {
            template_object,
            item_key,
            object_id,
            quantity,
        } => {
            writer.key("kind")?;
            writer.string("addItem")?;
            writer.key("templateObject")?;
            writer.u64(u64::from(*template_object))?;
            writer.key("itemKey")?;
            json_write_string(writer, item_key)?;
            writer.key("objectId")?;
            writer.u64(u64::from(*object_id))?;
            writer.key("quantity")?;
            writer.u64(u64::from(*quantity))?;
        }
        Change::SetPlayerFaction {
            target_object,
            old_value,
            faction_key,
        } => {
            writer.key("kind")?;
            writer.string("setPlayerFaction")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.i64(i64::from(*old_value))?;
            writer.key("factionKey")?;
            json_write_string(writer, faction_key)?;
        }
        Change::SetFactionRelation {
            target_object,
            faction_key,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setFactionRelation")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("factionKey")?;
            json_write_string(writer, faction_key)?;
            writer.key("oldValue")?;
            json_write_optional_i32(writer, *old_value)?;
            writer.key("newValue")?;
            writer.i64(i64::from(*new_value))?;
        }
        Change::SetUpgrades {
            target_object,
            old_value,
            new_value,
        } => {
            writer.key("kind")?;
            writer.string("setUpgrades")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("oldValue")?;
            writer.array_start()?;
            for value in old_value {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
            writer.key("newValue")?;
            writer.array_start()?;
            for value in new_value {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
        }
        Change::AddInfoPortions {
            target_object,
            info_portions,
        } => {
            writer.key("kind")?;
            writer.string("addInfoPortions")?;
            writer.key("targetObject")?;
            writer.u64(u64::from(*target_object))?;
            writer.key("infoPortions")?;
            writer.array_start()?;
            for value in info_portions {
                json_write_string(writer, value)?;
            }
            writer.array_end()?;
        }
        Change::RelocateActor { destination_changer } => {
            writer.key("kind")?;
            writer.string("relocateActor")?;
            writer.key("destinationChanger")?;
            writer.u64(u64::from(*destination_changer))?;
        }
    }
    writer.object_end()
}

fn json_write_findings(
    writer: &mut sse_codecs::json::Writer,
    findings: &[sse_doctor::RuleFinding],
) -> sse_core::Result<()> {
    writer.array_start()?;
    for finding in findings {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(writer, finding.id)?;
        writer.key("severity")?;
        json_write_string(writer, &format!("{:?}", finding.severity).to_lowercase())?;
        writer.key("lookedAt")?;
        json_write_string(writer, finding.looked_at)?;
        writer.key("found")?;
        json_write_string(writer, &finding.found)?;
        writer.key("repair")?;
        if let Some(change_set) = &finding.repair {
            writer.array_start()?;
            for change in change_set.changes() {
                json_write_change(writer, change)?;
            }
            writer.array_end()?;
        } else {
            writer.null()?;
        }
        writer.object_end()?;
    }
    writer.array_end()
}

fn json_save_report(report: &sse_doctor::SaveDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("formatId")?;
    json_write_optional_string(&mut writer, report.format_id)?;
    writer.key("objectCount")?;
    json_write_optional_count(&mut writer, report.object_count)?;
    writer.key("inventoryCount")?;
    json_write_optional_count(&mut writer, report.inventory_count)?;
    writer.key("findings")?;
    json_write_findings(&mut writer, &report.findings)?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_quest_report(report: &sse_doctor::QuestDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("formatId")?;
    json_write_optional_string(&mut writer, report.format_id)?;
    writer.key("questStatesAvailable")?;
    writer.bool(report.quest_states_available)?;
    writer.key("summary")?;
    json_write_string(&mut writer, &report.summary)?;
    writer.key("states")?;
    writer.array_start()?;
    for state in &report.states {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(&mut writer, state.id)?;
        writer.key("title")?;
        json_write_string(&mut writer, state.title)?;
        writer.key("status")?;
        json_write_string(&mut writer, &format!("{:?}", state.status).to_lowercase())?;
        writer.key("reason")?;
        json_write_string(&mut writer, state.reason)?;
        writer.key("missingInfo")?;
        json_write_optional_string(&mut writer, state.missing_info)?;
        writer.key("preventingFixId")?;
        json_write_optional_string(&mut writer, state.preventing_fix_id)?;
        writer.key("needsPreventingFix")?;
        writer.bool(state.needs_preventing_fix)?;
        writer.key("detail")?;
        json_write_string(&mut writer, state.detail)?;
        writer.key("references")?;
        writer.array_start()?;
        for reference in state.references {
            json_write_string(&mut writer, reference)?;
        }
        writer.array_end()?;
        writer.object_end()?;
    }
    writer.array_end()?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_game_report(report: &sse_doctor::GameDoctorReport) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.status).to_lowercase())?;
    writer.key("target")?;
    json_write_string(&mut writer, report.target.id())?;
    writer.key("directory")?;
    json_write_string(&mut writer, &report.directory.display().to_string())?;
    writer.key("markerFound")?;
    writer.bool(report.marker_found)?;
    writer.key("build")?;
    writer.object_start()?;
    writer.key("buildId")?;
    json_write_optional_string(&mut writer, report.build.build_id.as_deref())?;
    writer.key("status")?;
    json_write_string(&mut writer, &format!("{:?}", report.build.status).to_lowercase())?;
    writer.object_end()?;
    writer.key("findings")?;
    json_write_findings(&mut writer, &report.findings)?;
    writer.object_end()?;
    finish_json(writer)
}

fn json_crash_analysis(analysis: &sse_doctor::CrashLogAnalysis) -> sse_core::Result<String> {
    let mut writer = sse_codecs::json::Writer::compact();
    writer.object_start()?;
    writer.key("kind")?;
    json_write_string(&mut writer, &format!("{:?}", analysis.kind).to_lowercase())?;
    writer.key("summary")?;
    json_write_string(&mut writer, &analysis.summary)?;
    writer.key("knownIssue")?;
    if let Some(issue) = analysis.known_issue {
        writer.object_start()?;
        writer.key("id")?;
        json_write_string(&mut writer, issue.id)?;
        writer.key("title")?;
        json_write_string(&mut writer, issue.title)?;
        writer.key("game")?;
        json_write_string(&mut writer, issue.game)?;
        writer.key("advice")?;
        json_write_string(&mut writer, &format!("{:?}", issue.advice).to_lowercase())?;
        writer.object_end()?;
    } else {
        writer.null()?;
    }
    writer.key("faultingModuleOffset")?;
    json_write_optional_string(&mut writer, analysis.faulting_module_offset.as_deref())?;
    writer.object_end()?;
    finish_json(writer)
}

#[cfg(test)]
mod doctor_tests {
    use super::{json_string, run};

    const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");

    #[test]
    fn doctor_save_accepts_a_supported_synthetic_save() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-{}.sav", std::process::id()));
        let write = std::fs::write(&path, SYNTHETIC_XRAY_SAVE);
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "save".to_owned(),
            path.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let mut json_arguments = arguments.clone();
        json_arguments.push("--json".to_owned());
        let json_exit_code = run(&json_arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
        assert_eq!(json_exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_crash_accepts_an_explicit_synthetic_log() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-{}.log", std::process::id()));
        let write = std::fs::write(
            &path,
            "! [LUA][ERROR] ERROR: wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3",
        );
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "crash".to_owned(),
            path.to_string_lossy().into_owned(),
            "--game".to_owned(),
            "cs".to_owned(),
        ];

        let exit_code = run(&arguments);

        let mut json_arguments = arguments.clone();
        json_arguments.push("--json".to_owned());
        let json_exit_code = run(&json_arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
        assert_eq!(json_exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_crash_rejects_unknown_options() {
        let arguments = vec![
            "doctor".to_owned(),
            "crash".to_owned(),
            "synthetic.log".to_owned(),
            "--unknown".to_owned(),
        ];

        assert_eq!(run(&arguments), sse_core::ExitCode::Usage as u8);
    }

    #[test]
    fn doctor_game_checks_the_explicit_target_and_directory() {
        let directory = std::env::temp_dir().join(format!("sse-cli-doctor-game-{}", std::process::id()));
        let setup = (|| -> std::io::Result<()> {
            std::fs::create_dir_all(&directory)?;
            std::fs::write(directory.join("fsgame_cs.ltx"), b"$game_data$ = true")
        })();
        assert!(setup.is_ok());
        if setup.is_err() {
            let _ = std::fs::remove_dir_all(&directory);
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "game".to_owned(),
            "cs".to_owned(),
            directory.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn doctor_quests_keeps_unreadable_quest_state_unknown() {
        let path = std::env::temp_dir().join(format!("sse-cli-doctor-quests-{}.sav", std::process::id()));
        let write = std::fs::write(&path, SYNTHETIC_XRAY_SAVE);
        assert!(write.is_ok());
        if write.is_err() {
            return;
        }
        let arguments = vec![
            "doctor".to_owned(),
            "quests".to_owned(),
            path.to_string_lossy().into_owned(),
        ];

        let exit_code = run(&arguments);

        let _ = std::fs::remove_file(path);
        assert_eq!(exit_code, sse_core::ExitCode::Done as u8);
    }

    #[test]
    fn json_string_escapes_quotes_slashes_and_controls() {
        assert_eq!(
            json_string("a\n\"b\\c\u{0001}").unwrap_or_else(|error| format!("JSON encoding error: {error}")),
            "\"a\\n\\\"b\\\\c\\u0001\""
        );
    }

    #[test]
    fn save_doctor_json_contains_status_counts_and_rule_evidence() {
        let report = sse_doctor::analyze_save(SYNTHETIC_XRAY_SAVE);
        let output = super::json_save_report(&report).unwrap_or_else(|error| format!("JSON report error: {error}"));

        assert!(output.starts_with('{'));
        assert!(output.ends_with('}'));
        assert!(output.contains("\"status\":\"ok\""));
        assert!(output.contains("\"formatId\":\"stalker-soc\""));
        assert!(output.contains("\"id\":\"semantic-state\""));
        assert!(output.contains("\"repair\":null"));
    }
}
