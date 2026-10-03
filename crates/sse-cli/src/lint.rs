//! `stalker-save lint` command: runs static checks on game files.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use sse_content::{CompanionGame, GameFile, GameFileTree};
use sse_core::ExitCode;
use sse_lint::{LintEngine, LintOptions, LintSeverity};

const USAGE: &str = "Usage: stalker-save lint <game-folder> [--check <checker>] [--config <subdir>] [--json]";

/// Entry point for `stalker-save lint`.
#[must_use]
pub fn run_lint(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("{USAGE}");
        return ExitCode::Usage;
    }

    let mut game_folder: Option<PathBuf> = None;
    let mut check_filter: Option<String> = None;
    let mut config_subdir: Option<String> = None;
    let mut json_output = false;

    let mut i = 0;
    while i < args.len() {
        let arg = match args.get(i) {
            Some(a) => a.as_str(),
            None => break,
        };

        if arg == "--check" || arg == "--checker" {
            i = i.saturating_add(1);
            let Some(val) = args.get(i) else {
                eprintln!("{USAGE}");
                return ExitCode::Usage;
            };
            check_filter = Some(val.clone());
        } else if arg == "--config" {
            i = i.saturating_add(1);
            let Some(val) = args.get(i) else {
                eprintln!("{USAGE}");
                return ExitCode::Usage;
            };
            config_subdir = Some(val.clone());
        } else if arg == "--json" {
            json_output = true;
        } else if arg.starts_with('-') {
            eprintln!("Unknown option: {arg}\n{USAGE}");
            return ExitCode::Usage;
        } else if game_folder.is_none() {
            game_folder = Some(PathBuf::from(arg));
        } else {
            eprintln!("Unexpected argument: {arg}\n{USAGE}");
            return ExitCode::Usage;
        }

        i = i.saturating_add(1);
    }

    let Some(folder) = game_folder else {
        eprintln!("{USAGE}");
        return ExitCode::Usage;
    };

    if !folder.exists() {
        eprintln!("Game directory does not exist: {}", folder.display());
        return ExitCode::System;
    }

    // Try loading with CompanionGame / fsgame.ltx or fallback to directory scan
    let tree = match load_tree_for_lint(&folder) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to load game file tree: {e}");
            return ExitCode::Damaged;
        }
    };

    let options = LintOptions {
        single_checker: check_filter,
        config_subdir,
        max_files_globals: Some(1),
    };

    let engine = LintEngine::new(options);
    let report = engine.lint_tree(&tree);

    if json_output {
        print_json_report(&report);
    } else {
        for finding in &report.findings {
            println!("{}", finding.to_display_string());
        }

        let errors = report.count_by_severity(LintSeverity::Error);
        let warnings = report.count_by_severity(LintSeverity::Warning);
        println!(
            "== Lint completed in {} ms ({} files checked). Found {} issues ({} errors, {} warnings).",
            report.elapsed_ms,
            report.files_checked,
            report.findings.len(),
            errors,
            warnings
        );
    }

    if report.has_errors() {
        ExitCode::Refused
    } else {
        ExitCode::Done
    }
}

/// Loads a GameFileTree from a game directory or loose folder.
fn load_tree_for_lint(folder: &Path) -> sse_core::Result<GameFileTree> {
    // If fsgame.ltx exists, use CompanionGame loader
    if folder.join("fsgame.ltx").exists() {
        // Try ShadowOfChernobyl first, then ClearSky / CallOfPripyat
        let game = if folder.join("gamedata/configs").is_dir() || folder.join("configs").is_dir() {
            CompanionGame::CallOfPripyat
        } else {
            CompanionGame::ShadowOfChernobyl
        };

        if let Ok(tree) = GameFileTree::load_simple(game, folder, |_| true, false) {
            return Ok(tree);
        }
    }

    // Fallback: build loose tree from directory directly
    let mut files: HashMap<String, GameFile> = HashMap::new();
    let mut paths = Vec::new();
    collect_files_recursive(folder, &mut paths);

    for p in paths {
        if let Ok(rel) = p.strip_prefix(folder) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let file_path = p.clone();
            let game_file = GameFile::new(rel_str.clone(), "loose", move || {
                fs::read(&file_path).map_err(|e| sse_core::Error::System(e.to_string()))
            });
            files.insert(rel_str, game_file);
        }
    }

    Ok(GameFileTree {
        files,
        fingerprint: String::new(),
        has_loose_overlay: true,
        config_prefix: "configs/".to_string(),
        data_directory: Some(folder.to_path_buf()),
        issues: Vec::new(),
    })
}

fn collect_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(&path, out);
        } else if path.is_file() {
            out.push(path);
        }
    }
}

fn print_json_report(report: &sse_lint::LintReport) {
    println!("{{");
    println!("  \"filesChecked\": {},", report.files_checked);
    println!("  \"elapsedMs\": {},", report.elapsed_ms);
    println!("  \"totalFindings\": {},", report.findings.len());
    println!("  \"findings\": [");
    for (idx, finding) in report.findings.iter().enumerate() {
        let comma = if idx.saturating_add(1) < report.findings.len() {
            ","
        } else {
            ""
        };
        println!(
            "    {{\"checker\": \"{}\", \"file\": \"{}\", \"line\": {}, \"severity\": \"{}\", \"message\": \"{}\"}}{}",
            finding.checker,
            finding.file,
            finding.line,
            finding.severity.as_str(),
            finding.message.replace('\"', "\\\""),
            comma
        );
    }
    println!("  ]");
    println!("}}");
}
