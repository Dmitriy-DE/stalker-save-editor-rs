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

const USAGE: &str = "Usage: stalker-save <version|info|inventory|set-money|set-stack|edit|backups|fixes|update|lint|audit> ...\n\
Exit codes: 0 done, 2 wrong arguments, 3 refused (unsupported or unsafe), 4 unreadable or damaged input, 5 file or system error.";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
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

    let source = SaveBuffer::read(&path)?;
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
        for handle in options.stack_order {
            let new_value = options
                .stacks
                .get(&handle)
                .copied()
                .ok_or_else(|| WriteFailure::Core(Error::damaged("missing parsed stack option")))?;
            let target_object = u16::try_from(handle).map_err(|_| {
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
    let output_path = options.output.unwrap_or_else(|| default_output_path(&path));
    let backup_directory = options.backup_directory.unwrap_or_else(default_backup_directory);
    let receipt = sse_storage::transaction::export_transaction(
        &path,
        &source_sha256,
        output.as_slice(),
        &output_path,
        &backup_directory,
        sse_storage::transaction::EditSummary {
            money: options.money,
            stack_count,
            move_count: options.moves.len(),
            detach_count: options.removals.len(),
            add_count: options.additions.len(),
            durability_count: options.durability.len(),
            upgrade_count: options.upgrades.len(),
            ..sse_storage::transaction::EditSummary::default()
        },
    )?;
    let read_back = std::fs::read(&receipt.output_path)?;
    if read_back.as_slice() != output.as_slice() || sse_codecs::sha256::sha256_hex(&read_back) != receipt.output_sha256
    {
        return Err(WriteFailure::Core(Error::System(
            "export read-back did not match the prepared save".to_owned(),
        )));
    }
    println!("Output: {}", receipt.output_path.display());
    println!("Size: {}", read_back.len());
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
    let save = Save::read(packed.as_slice())?;
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
    let save = Save::read(packed.as_slice())?;
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
    lines.extend(save.warnings().iter().map(|warning| format!("Warning: {warning}")));
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
        let key = format!(
            "{:02X}{:02X}{:02X}",
            item.type_key[0], item.type_key[1], item.type_key[2]
        );
        let category = s2_category_name(item.kind_code, item.display_name.as_deref());
        format!(
            "{position:<10} {category:<20} {key:<25} {:>7}  0x{:08X}",
            item.count, item.handle
        )
    }));
    lines
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
    use super::{run, s2_info_lines, s2_inventory_lines, writer, Save};
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
