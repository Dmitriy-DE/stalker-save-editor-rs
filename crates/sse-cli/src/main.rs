//! `stalker-save`: the command line. It is the parity surface: for the same input it prints what the C# command
//! line prints (see `tools/oracle.sh`), so every reader and writer is compared with the released editor.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sse_core::{Error, ExitCode as CommandExitCode, SaveBuffer};
use sse_xray::writer::{self, Change, ChangeSet, Placement};
use sse_xray::Save;

mod fixes;
mod lint;
mod update;

const USAGE: &str = "Usage: stalker-save <version|info|inventory|set-money|set-stack|edit|backups|fixes|update|lint> ...\n\
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

#[derive(Debug)]
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
    additions: Vec<(String, u32)>,
    durability: Vec<(u32, f32)>,
    upgrades: Vec<(u32, Vec<String>)>,
    placements: Vec<(u32, Placement)>,
    moves: Vec<(u32, MoveDestination)>,
    unsupported_operations: Vec<String>,
}

#[derive(Clone, Copy)]
enum MoveDestination {
    Inventory,
    Stash(u16),
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
    let mut changes = Vec::with_capacity(
        stack_count
            .saturating_add(usize::from(options.money.is_some()))
            .saturating_add(options.durability.len())
            .saturating_add(options.upgrades.len())
            .saturating_add(options.placements.len())
            .saturating_add(options.moves.len())
            .saturating_add(options.additions.len()),
    );
    let mut stack_output = Vec::with_capacity(stack_count);
    let inventory = if stack_count > 0 || !options.durability.is_empty() || !options.placements.is_empty() {
        Some(save.inventory()?)
    } else {
        None
    };
    if let Some(new_value) = options.money {
        changes.push(Change::SetMoney {
            target_object: save.actor_id(),
            old_value: save.money()?,
            new_value,
        });
    }
    if stack_count > 0 {
        for handle in options.stack_order.iter().copied() {
            let new_value = options
                .stacks
                .get(&handle)
                .copied()
                .ok_or_else(|| WriteFailure::Core(Error::damaged("missing parsed stack option")))?;
            let target_object = object_id(handle)?;
            let old_value = inventory
                .as_deref()
                .unwrap_or(&[])
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

    for (handle, new_value) in &options.durability {
        let target_object = object_id(*handle)?;
        let old_value = inventory
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .find(|item| item.handle == target_object)
            .and_then(|item| item.condition)
            .ok_or_else(|| {
                WriteFailure::Core(Error::Refused(format!(
                    "object 0x{target_object:04X} has no confirmed durability field"
                )))
            })?;
        changes.push(Change::SetDurability {
            target_object,
            old_value,
            new_value: *new_value,
        });
    }

    for (handle, new_value) in &options.upgrades {
        let target_object = object_id(*handle)?;
        changes.push(Change::SetUpgrades {
            target_object,
            old_value: writer::current_upgrades(&save, target_object)?,
            new_value: new_value.clone(),
        });
    }

    for (handle, destination) in &options.placements {
        changes.push(Change::SetPlacement {
            target_object: object_id(*handle)?,
            destination: *destination,
        });
    }

    for (handle, destination) in &options.moves {
        let target_object = object_id(*handle)?;
        let record = save
            .registry_objects()
            .iter()
            .find(|record| record.object_id == target_object)
            .ok_or_else(|| WriteFailure::Core(Error::Refused(format!("object 0x{target_object:04X} is missing"))))?;
        let new_parent = match destination {
            MoveDestination::Inventory => save.actor_id(),
            MoveDestination::Stash(stash) => *stash,
        };
        changes.push(Change::MoveItem {
            target_object,
            old_parent: record.parent_id,
            new_parent,
        });
    }

    changes.extend(writer::prepare_add_item_changes(&save, &options.additions)?);
    let add_count = changes
        .iter()
        .filter(|change| matches!(change, Change::AddItem { .. }))
        .count();

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
            add_count,
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
    for (key, quantity) in &options.additions {
        println!("Add {key} x{quantity}");
    }
    for (handle, condition) in &options.durability {
        println!("Durability 0x{handle:04X}: {}", format_condition(*condition));
    }
    for (handle, upgrades) in &options.upgrades {
        println!("Upgrades 0x{handle:04X}: {}", upgrades.join(","));
    }
    for (handle, placement) in &options.placements {
        println!("Placement 0x{handle:04X}: {}", placement_name(*placement));
    }
    for (handle, destination) in &options.moves {
        match destination {
            MoveDestination::Inventory => println!("Move 0x{handle:04X}: inventory"),
            MoveDestination::Stash(stash) => println!("Move 0x{handle:04X}: stash 0x{stash:04X}"),
        }
    }
    Ok(())
}

fn object_id(handle: u32) -> Result<u16, WriteFailure> {
    u16::try_from(handle).map_err(|_| {
        WriteFailure::Core(Error::Refused(format!(
            "X-Ray item handle 0x{handle:X} exceeds 16 bits"
        )))
    })
}

fn format_condition(condition: f32) -> String {
    format!("{condition:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

fn placement_name(placement: Placement) -> String {
    match placement {
        Placement::Ruck => "ruck".to_owned(),
        Placement::Belt => "belt".to_owned(),
        Placement::Slot(slot) => format!("slot:{slot}"),
    }
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
                let (key, quantity) = parse_string_assignment(value)?;
                if key.is_empty() {
                    return Err(WriteFailure::Core(Error::damaged("item key must not be empty")));
                }
                options.additions.push((key.to_owned(), parse_unsigned(quantity)?));
            }
            "--durability" => {
                let (handle, condition) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.durability.iter().any(|(existing, _)| *existing == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate durability handle 0x{handle:X}")));
                }
                let condition = condition
                    .parse::<f32>()
                    .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid durability '{condition}'"))))?;
                options.durability.push((handle, condition));
            }
            "--upgrade" => {
                let (handle, upgrade_list) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.upgrades.iter().any(|(existing, _)| *existing == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate upgrade handle 0x{handle:X}")));
                }
                let upgrades = upgrade_list.split(',').map(str::to_owned).collect::<Vec<_>>();
                if upgrades.iter().any(String::is_empty) {
                    return Err(WriteFailure::Core(Error::damaged("upgrade keys must not be empty")));
                }
                options.upgrades.push((handle, upgrades));
            }
            "--placement" => {
                let (handle, placement) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.placements.iter().any(|(existing, _)| *existing == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate placement handle 0x{handle:X}")));
                }
                options.placements.push((handle, parse_placement(placement)?));
            }
            "--move" => {
                let (handle, destination) = parse_string_assignment(value)?;
                let handle = parse_unsigned(handle)?;
                if options.moves.iter().any(|(existing, _)| *existing == handle) {
                    return Err(WriteFailure::Usage(format!("duplicate move handle 0x{handle:X}")));
                }
                options.moves.push((handle, parse_move_destination(destination)?));
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
            let slot = value
                .strip_prefix("slot:")
                .ok_or_else(|| WriteFailure::Core(Error::damaged(format!("invalid placement '{value}'"))))?;
            let parsed = parse_unsigned(slot)?;
            let parsed = u8::try_from(parsed)
                .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid placement slot '{slot}'"))))?;
            Ok(Placement::Slot(parsed))
        }
    }
}

fn parse_move_destination(value: &str) -> Result<MoveDestination, WriteFailure> {
    if value.eq_ignore_ascii_case("inventory") {
        return Ok(MoveDestination::Inventory);
    }
    let stash_id = value
        .get(..6)
        .filter(|prefix| prefix.eq_ignore_ascii_case("stash:"))
        .and_then(|_| value.get(6..))
        .ok_or_else(|| WriteFailure::Core(Error::damaged(format!("invalid move destination '{value}'"))))?;
    let stash_id = parse_unsigned(stash_id)?;
    let stash_id = u16::try_from(stash_id)
        .map_err(|_| WriteFailure::Core(Error::Refused("stash object id exceeds 16 bits".to_owned())))?;
    Ok(MoveDestination::Stash(stash_id))
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

fn run_backups(arguments: &[String]) -> u8 {
    match arguments {
        [command] if command == "list" => list_backup_entries(&default_backup_directory()),
        [command, option, directory] if command == "list" && (option == "--directory" || option == "--backup-dir") => {
            list_backup_entries(Path::new(directory))
        }
        [command, journal, option, output] if command == "restore" && option == "--output" => {
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
            eprintln!(
                "Error: Invalid backups arguments. Usage: stalker-save backups list [--directory DIR] | backups restore JOURNAL --output PATH"
            );
            CommandExitCode::Usage as u8
        }
    }
}

fn list_backup_entries(directory: &Path) -> u8 {
    match sse_storage::transaction::list_backups(directory) {
        Ok(entries) => {
            if entries.is_empty() {
                println!("No backups found.");
            }
            for entry in entries {
                let status = match entry.status {
                    sse_storage::transaction::BackupStatus::Verified => "verified",
                    sse_storage::transaction::BackupStatus::Missing => "missing",
                    sse_storage::transaction::BackupStatus::Corrupt => "corrupt",
                };
                println!("{status}: {}", entry.journal_path.display());
                println!("  source: {}", entry.source_path.display());
                println!("  backup: {}", entry.backup_path.display());
                if !entry.source_sha256.is_empty() {
                    println!("  sha256: {}", entry.source_sha256);
                }
                if let Some(error) = entry.error {
                    println!("  detail: {error}");
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

#[cfg(test)]
#[allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::indexing_slicing)]
mod write_tests {
    use super::{parse_write_options, run, MoveDestination, Placement, Save, WriteOptions};
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
    fn edit_refuses_unverified_kinds_and_cli_errors_keep_reference_exit_classes() {
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
            "missing.sav".to_owned(),
            "--detach".to_owned(),
            "1234".to_owned(),
        ];
        let invalid_number = vec![
            "set-money".to_owned(),
            "missing.sav".to_owned(),
            "not-a-number".to_owned(),
        ];
        assert_eq!(run(&add), 3);
        assert_eq!(run(&unsupported), 3);
        assert_eq!(run(&invalid_number), 4);
    }

    #[test]
    fn edit_exports_fixture_verified_durability_and_placement_changes() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");

        let durability_source = saves.join("durability.sav");
        let durability_bytes =
            include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-source.sav");
        let durability_expected =
            include_bytes!("../../../fixtures/synthetic/writer-durability/xray-durability-soc-expected.sav");
        fs::write(&durability_source, durability_bytes).expect("write durability fixture");
        assert_eq!(
            run(&edit_arguments(
                &durability_source,
                &output,
                &backups,
                &[("--durability", "13398=0.75")],
            )),
            0
        );
        assert_eq!(fs::read(&output).expect("durability output"), durability_expected);

        fs::remove_file(&output).expect("remove previous export");
        let placement_source = saves.join("placement.sav");
        let placement_bytes =
            include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-source.sav");
        let placement_expected =
            include_bytes!("../../../fixtures/synthetic/writer-placement/xray-placement-soc-expected.sav");
        fs::write(&placement_source, placement_bytes).expect("write placement fixture");
        assert_eq!(
            run(&edit_arguments(
                &placement_source,
                &output,
                &backups,
                &[("--placement", "13398=slot:3")],
            )),
            0
        );
        assert_eq!(fs::read(&output).expect("placement output"), placement_expected);
    }

    #[test]
    fn edit_exports_fixture_verified_stash_moves_and_additions() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");

        let move_source = saves.join("stashes.sav");
        let move_bytes = include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-source.sav");
        let move_expected = include_bytes!("../../../fixtures/synthetic/xray-stashes/xray-stash-soc-take.sav");
        fs::write(&move_source, move_bytes).expect("write stash fixture");
        assert_eq!(
            run(&edit_arguments(
                &move_source,
                &output,
                &backups,
                &[("--move", "9029=inventory")],
            )),
            0
        );
        assert_eq!(fs::read(&output).expect("stash transfer output"), move_expected);

        fs::remove_file(&output).expect("remove previous export");
        let add_source = saves.join("addition.sav");
        let add_bytes = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-source.sav");
        let add_expected = include_bytes!("../../../fixtures/synthetic/writer-add/xray-add-soc-ammo-expected.sav");
        fs::write(&add_source, add_bytes).expect("write addition fixture");
        assert_eq!(
            run(&edit_arguments(
                &add_source,
                &output,
                &backups,
                &[("--add", "ammo_9x39_pab9=17")],
            )),
            0
        );
        assert_eq!(fs::read(&output).expect("addition output"), add_expected);
    }

    #[test]
    fn edit_reads_and_preserves_confirmed_upgrade_vectors() {
        let temporary = TempDirectory::new();
        let saves = temporary.0.join("saves");
        fs::create_dir(&saves).expect("create save directory");
        let source = saves.join("upgrades.sav");
        let output = saves.join("edited.sav");
        let backups = temporary.0.join("backups");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-upgrades/xray-upgrades-cop-source.sav");
        fs::write(&source, source_bytes).expect("write upgrades fixture");
        let save = Save::read(source_bytes).expect("read upgrades fixture");
        let current = sse_xray::writer::current_upgrades(&save, 13398).expect("read current upgrades");
        let value = format!("13398={}", current.join(","));
        let args = edit_arguments(&source, &output, &backups, &[("--upgrade", &value)]);

        assert_eq!(run(&args), 0);
        let written = Save::read(&fs::read(&output).expect("read upgrade output")).expect("parse output");
        assert_eq!(
            sse_xray::writer::current_upgrades(&written, 13398).expect("read written upgrades"),
            current
        );
    }

    #[test]
    fn backup_commands_list_and_restore_only_verified_entries() {
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

        let list = vec![
            "backups".to_owned(),
            "list".to_owned(),
            "--directory".to_owned(),
            backups.display().to_string(),
        ];
        assert_eq!(run(&list), 0);
        let entry = sse_storage::transaction::list_backups(&backups)
            .expect("list verified backup")
            .into_iter()
            .next()
            .expect("backup entry exists");
        let restore = vec![
            "backups".to_owned(),
            "restore".to_owned(),
            entry.journal_path.display().to_string(),
            "--output".to_owned(),
            restored.display().to_string(),
        ];
        assert_eq!(run(&restore), 0);
        assert_eq!(fs::read(&restored).expect("restored backup"), source_bytes);
    }

    #[test]
    fn edit_parses_all_supported_mutation_options() {
        let arguments = vec![
            "--add".to_owned(),
            "ammo_9x39_pab9=17".to_owned(),
            "--durability".to_owned(),
            "13398=0.75".to_owned(),
            "--upgrade".to_owned(),
            "13398=up_one,up_two".to_owned(),
            "--placement".to_owned(),
            "13398=slot:3".to_owned(),
            "--move".to_owned(),
            "9029=stash:0x10".to_owned(),
        ];
        let mut options = WriteOptions::default();
        parse_write_options(&arguments, 0, &mut options).expect("parse edit options");
        assert_eq!(options.additions, [("ammo_9x39_pab9".to_owned(), 17)]);
        assert_eq!(options.durability, [(13398, 0.75)]);
        assert_eq!(
            options.upgrades,
            [(13398, vec!["up_one".to_owned(), "up_two".to_owned()])]
        );
        assert_eq!(options.placements, [(13398, Placement::Slot(3))]);
        assert!(matches!(options.moves.as_slice(), [(9029, MoveDestination::Stash(16))]));
    }

    fn edit_arguments(source: &Path, output: &Path, backups: &Path, changes: &[(&str, &str)]) -> Vec<String> {
        let mut arguments = vec!["edit".to_owned(), source.display().to_string()];
        for (option, value) in changes {
            arguments.push((*option).to_owned());
            arguments.push((*value).to_owned());
        }
        arguments.extend([
            "--output".to_owned(),
            output.display().to_string(),
            "--backup-dir".to_owned(),
            backups.display().to_string(),
        ]);
        arguments
    }
}
