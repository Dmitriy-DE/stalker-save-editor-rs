//! `stalker-save`: the command line. It is the parity surface: for the same input it prints what the C# command
//! line prints (see `tools/oracle.sh`), so every reader and writer is compared with the released editor.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use sse_core::{Error, ExitCode as CommandExitCode, SaveBuffer};
use sse_xray::writer::{self, Change, ChangeSet};
use sse_xray::Save;

mod fixes;
mod lint;
mod update;

const USAGE: &str = "Usage: stalker-save <version|info|inventory|set-money|set-stack|edit|fixes|update|lint> ...\n\
Exit codes: 0 done, 2 wrong arguments, 3 refused (unsupported or unsafe), 4 unreadable or damaged input, 5 file or system error.";

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
        Some("set-money" | "set-stack" | "edit") => run_write(arguments),
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
    has_unverified_change_kind: bool,
    unsupported_operations: Vec<String>,
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
            if options.money.is_none() && options.stacks.is_empty() && !options.has_unverified_change_kind {
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
    if options.has_unverified_change_kind {
        return Err(WriteFailure::Core(Error::Refused(
            "Edit kinds other than money and stack counts are not supported because EditService.VerifyReadBack does not verify them completely."
                .to_owned(),
        )));
    }
    let stack_count = options.stacks.len();
    let mut changes = Vec::with_capacity(stack_count.saturating_add(usize::from(options.money.is_some())));
    let mut stack_output = Vec::with_capacity(stack_count);
    if let Some(new_value) = options.money {
        changes.push(Change::SetMoney {
            target_object: save.actor_id(),
            old_value: save.money()?,
            new_value,
        });
    }
    if !options.stacks.is_empty() {
        let inventory = save.inventory()?;
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
                let (key, quantity) = parse_string_assignment(value)?;
                let _quantity = parse_unsigned(quantity)?;
                if key.is_empty() {
                    return Err(WriteFailure::Core(Error::damaged("item key must not be empty")));
                }
                options.has_unverified_change_kind = true;
            }
            "--durability" => {
                let (handle, condition) = parse_string_assignment(value)?;
                let _handle = parse_unsigned(handle)?;
                let _condition = condition
                    .parse::<f64>()
                    .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid durability '{condition}'"))))?;
                options.has_unverified_change_kind = true;
            }
            "--upgrade" => {
                let (handle, _) = parse_string_assignment(value)?;
                let _handle = parse_unsigned(handle)?;
                options.has_unverified_change_kind = true;
            }
            "--placement" => {
                let (handle, placement) = parse_string_assignment(value)?;
                let _handle = parse_unsigned(handle)?;
                if let Some((kind, slot)) = placement.split_once(':') {
                    if kind == "slot" {
                        let _slot = slot
                            .parse::<i32>()
                            .map_err(|_| WriteFailure::Core(Error::damaged(format!("invalid slot '{slot}'"))))?;
                    }
                }
                options.has_unverified_change_kind = true;
            }
            "--move" | "--detach" | "--attach" | "--raw" => {
                options.unsupported_operations.push(option.to_owned());
            }
            _ => return Err(WriteFailure::Usage(format!("Unknown option: {option}."))),
        }
    }
    Ok(())
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
    use super::{run, Save};
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
            "--move".to_owned(),
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
}
