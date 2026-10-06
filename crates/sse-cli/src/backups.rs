use std::path::Path;
use std::path::PathBuf;

use sse_core::ExitCode as CommandExitCode;

use crate::configured_backup_directory;

pub(super) fn run_backups(arguments: &[String]) -> u8 {
    match arguments.first().map(String::as_str) {
        Some("list") => {
            let mut directory = configured_backup_directory();
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
