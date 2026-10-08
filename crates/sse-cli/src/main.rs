//! `stalker-save`: the command line. It is the parity surface: for the same input it prints what the C# command
//! line prints (see `tools/oracle.sh`), so every reader and writer is compared with the released editor.

use std::path::PathBuf;
use std::process::ExitCode;

mod audit;
mod backups;
mod doctor;
mod fixes;
mod lint;
mod read;
mod update;
mod write;

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
        Some("info") if arguments.len() == 2 => report(read::read_info(arguments.get(1))),
        Some("inventory")
            if arguments.len() == 2
                || (arguments.len() == 3 && arguments.get(2).map(String::as_str) == Some("--all")) =>
        {
            report(read::read_inventory(arguments.get(1)))
        }
        Some("audit") if arguments.len() == 2 => report(audit::run(arguments.get(1).map(String::as_str))),
        Some("audit") => {
            eprintln!("Error: audit requires a file containing one save path per line. {USAGE}");
            sse_core::ExitCode::Usage as u8
        }
        Some("doctor")
            if (arguments.len() == 3 || (arguments.len() == 4 && doctor::is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("save") =>
        {
            report(doctor::doctor_save(arguments.get(2), doctor::is_json(arguments)))
        }
        Some("doctor") if arguments.len() >= 3 && arguments.get(1).map(String::as_str) == Some("crash") => {
            let Some((game, json)) = doctor::parse_crash_options(arguments) else {
                eprintln!("{USAGE}");
                return sse_core::ExitCode::Usage as u8;
            };
            report(doctor::doctor_crash(arguments.get(2), game, json))
        }
        Some("doctor")
            if (arguments.len() == 4 || (arguments.len() == 5 && doctor::is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("game") =>
        {
            report(doctor::doctor_game(
                arguments.get(2),
                arguments.get(3),
                doctor::is_json(arguments),
            ))
        }
        Some("doctor")
            if (arguments.len() == 3 || (arguments.len() == 4 && doctor::is_json(arguments)))
                && arguments.get(1).map(String::as_str) == Some("quests") =>
        {
            report(doctor::doctor_quests(arguments.get(2), doctor::is_json(arguments)))
        }
        Some("set-money" | "set-stack" | "edit") => write::run_write(arguments),
        Some("backups") => backups::run_backups(arguments.get(1..).unwrap_or_default()),
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

fn configured_backup_directory() -> PathBuf {
    let settings = match sse_app::AppSettings::load(&sse_app::default_settings_path()) {
        Ok(settings) => settings,
        Err(error) => {
            eprintln!("Warning: settings could not be loaded; using the default backup directory: {error}");
            sse_app::diagnostics::warn(&format!("settings file could not be loaded by the CLI: {error}"));
            sse_app::AppSettings::default()
        }
    };
    sse_app::paths::backup_directory(&settings)
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

#[cfg(test)]
mod tests {
    use super::{run, USAGE};

    #[test]
    fn audit_is_listed_and_requires_a_path_list_argument() {
        assert!(USAGE.contains("audit"));
        assert_eq!(run(&["audit".to_owned()]), 2);
    }
}
