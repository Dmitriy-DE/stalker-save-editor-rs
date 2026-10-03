//! `stalker-save`: the command line. It is the parity surface: for the same input it prints what the C# command
//! line prints (see `tools/oracle.sh`), so every reader and writer is compared with the released editor.

use std::process::ExitCode;

mod fixes;
mod lint;
mod update;

const USAGE: &str = "Usage: stalker-save <version|fixes|update|lint|info|inventory|edit> ...\n\
Exit codes: 0 done, 2 wrong arguments, 3 refused (unsupported or unsafe), 4 unreadable or damaged input, 5 file or system error.";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.first().map(String::as_str) {
        Some("version") => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            ExitCode::from(sse_core::ExitCode::Done as u8)
        }
        Some("fixes") => {
            let rest = arguments.get(1..).unwrap_or_default();
            let code = fixes::run_fixes(rest);
            ExitCode::from(code as u8)
        }
        Some("lint") => {
            let rest = arguments.get(1..).unwrap_or_default();
            let code = lint::run_lint(rest);
            ExitCode::from(code as u8)
        }
        Some("update") => {
            let rest = arguments.get(1..).unwrap_or_default();
            let code = update::run_update(rest);
            ExitCode::from(code as u8)
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(sse_core::ExitCode::Usage as u8)
        }
    }
}
