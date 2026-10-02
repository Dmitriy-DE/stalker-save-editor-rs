//! CLI subcommand handler for `fixes`.

use sse_core::ExitCode;
use sse_fixes::{
    identify_game, GameFileExtractor, GameFixCatalog, GameFixEngine, GameFixMaturity, GameFixPreset, GameFixState,
    GameTarget,
};
use std::path::Path;

const FIXES_USAGE: &str = "Usage: fixes list [--game TARGET] [--json] | \
fixes state TARGET GAME_DIR [--json] | \
fixes status TARGET GAME_DIR [--json] | \
fixes preset <essential|recommended|all-safe> TARGET GAME_DIR [--json] | \
fixes apply-preset <essential|recommended|all-safe> TARGET GAME_DIR [--json] | \
fixes install ID GAME_DIR | \
fixes update ID GAME_DIR | \
fixes remove ID GAME_DIR | \
fixes extract TARGET GAME_DIR OUT_DIR [--archives-only] [PREFIX...]";

/// Executes the `fixes` subcommand.
pub fn run_fixes(args: &[String]) -> ExitCode {
    let Some(first) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };

    let rest = args.get(1..).unwrap_or_default();
    match first.as_str() {
        "list" => run_list(rest),
        "state" | "status" => run_state(rest),
        "preset" | "apply-preset" => run_preset(rest),
        "install" => run_install(rest),
        "update" => run_update(rest),
        "remove" => run_remove(rest),
        "extract" => run_extract(rest),
        _ => {
            eprintln!("{FIXES_USAGE}");
            ExitCode::Usage
        }
    }
}

fn run_list(args: &[String]) -> ExitCode {
    let mut selected_target: Option<GameTarget> = None;
    let mut json = false;

    let mut i = 0usize;
    while i < args.len() {
        let Some(arg) = args.get(i) else {
            break;
        };
        match arg.as_str() {
            "--json" => {
                json = true;
                i = i.saturating_add(1);
            }
            "--game" => {
                let next_idx = i.saturating_add(1);
                let Some(val) = args.get(next_idx) else {
                    eprintln!("{FIXES_USAGE}");
                    return ExitCode::Usage;
                };
                match GameTarget::parse(val) {
                    Some(t) => {
                        selected_target = Some(t);
                        i = i.saturating_add(2);
                    }
                    None => {
                        eprintln!("Unknown game target: {val}");
                        return ExitCode::Usage;
                    }
                }
            }
            _ => {
                eprintln!("{FIXES_USAGE}");
                return ExitCode::Usage;
            }
        }
    }

    let all_fixes = GameFixCatalog::all();
    let filtered: Vec<_> = all_fixes
        .iter()
        .filter(|f| selected_target.is_none_or(|t| f.game == t))
        .collect();

    if json {
        let mut out = String::from("[\n");
        for (idx, f) in filtered.iter().enumerate() {
            let managed_paths = GameFixEngine::managed_paths(f);
            out.push_str("  {\n");
            out.push_str(&format!("    \"id\": \"{}\",\n", f.id));
            out.push_str(&format!("    \"game\": \"{}\",\n", f.game.id()));
            out.push_str(&format!("    \"version\": \"{}\",\n", f.version));
            out.push_str(&format!("    \"title\": \"{}\",\n", escape_json(&f.title)));
            out.push_str(&format!("    \"category\": \"{}\",\n", f.category.as_str()));
            out.push_str(&format!("    \"maturity\": \"{}\",\n", f.maturity.as_str()));

            out.push_str("    \"supportedSteamBuildIds\": [");
            for (bi, b) in f.supported_steam_build_ids.iter().enumerate() {
                out.push_str(&format!(
                    "\"{}\"{}",
                    b,
                    if bi.saturating_add(1) < f.supported_steam_build_ids.len() {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            out.push_str("],\n");

            out.push_str("    \"dependsOn\": [");
            for (di, d) in f.depends_on.iter().enumerate() {
                out.push_str(&format!(
                    "\"{}\"{}",
                    d,
                    if di.saturating_add(1) < f.depends_on.len() {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            out.push_str("],\n");

            out.push_str("    \"conflictsWith\": [");
            for (ci, c) in f.conflicts_with.iter().enumerate() {
                out.push_str(&format!(
                    "\"{}\"{}",
                    c,
                    if ci.saturating_add(1) < f.conflicts_with.len() {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            out.push_str("],\n");

            out.push_str("    \"managedPaths\": [");
            for (pi, p) in managed_paths.iter().enumerate() {
                out.push_str(&format!(
                    "\"{}\"{}",
                    p,
                    if pi.saturating_add(1) < managed_paths.len() {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            out.push_str("]\n");

            out.push_str(&format!(
                "  }}{}",
                if idx.saturating_add(1) < filtered.len() {
                    ",\n"
                } else {
                    "\n"
                }
            ));
        }
        out.push_str("]\n");
        print!("{out}");
    } else if filtered.is_empty() {
        println!("No evidence-validated game-file fixes are currently catalogued.");
    } else {
        for f in filtered {
            println!("{} [{}] {}: {}", f.id, f.game.id(), f.category.as_str(), f.title);
        }
    }

    ExitCode::Done
}

fn run_state(args: &[String]) -> ExitCode {
    if args.is_empty() || args.len() > 3 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(target_str) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };

    let target = match GameTarget::parse(target_str) {
        Some(t) => t,
        None => {
            eprintln!("Unknown game target: {target_str}");
            return ExitCode::Usage;
        }
    };

    let Some(dir_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let json = if args.len() == 3 {
        if args.get(2).map(String::as_str) == Some("--json") {
            true
        } else {
            eprintln!("{FIXES_USAGE}");
            return ExitCode::Usage;
        }
    } else {
        false
    };

    let (_is_game, steam_build_id) = identify_game(target, game_dir);
    let engine = GameFixEngine::new();
    let installed_fixes = match engine.list_installed(game_dir, None) {
        Ok(list) => list,
        Err(e) => {
            eprintln!("Error checking installed fixes: {e}");
            return map_error_to_exit_code(&e);
        }
    };

    let catalogue = GameFixCatalog::for_game(target);
    let recommended = GameFixCatalog::for_preset(target, GameFixPreset::Recommended);

    let compatible_recommended: Vec<_> = if !recommended.is_empty() {
        if let Some(ref build) = steam_build_id {
            if recommended
                .iter()
                .all(|d| d.supported_steam_build_ids.iter().any(|b| b == build))
            {
                recommended.clone()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    let status_note: Option<&'static str> = if catalogue.is_empty() {
        Some("No evidence-validated game-file fixes are currently catalogued.")
    } else if recommended.is_empty() {
        Some("Catalogued fixes are experimental or research-only and are not included in safe presets.")
    } else if compatible_recommended.is_empty() {
        Some("The detected build is not supported by the complete safe preset.")
    } else {
        None
    };

    if json {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"game\": \"{}\",\n", target.id()));
        match &steam_build_id {
            Some(b) => out.push_str(&format!("  \"steamBuildId\": \"{}\",\n", b)),
            None => out.push_str("  \"steamBuildId\": null,\n"),
        }

        out.push_str("  \"installedFixes\": [\n");
        for (i, f) in installed_fixes.iter().enumerate() {
            out.push_str("    {\n");
            out.push_str(&format!("      \"id\": \"{}\",\n", f.id));
            out.push_str(&format!("      \"version\": \"{}\",\n", f.version));
            out.push_str(&format!("      \"state\": \"{}\"\n", f.state.as_str()));
            out.push_str(&format!(
                "    }}{}",
                if i.saturating_add(1) < installed_fixes.len() {
                    ",\n"
                } else {
                    "\n"
                }
            ));
        }
        out.push_str("  ],\n");

        out.push_str("  \"availableFixes\": [\n");
        for (i, f) in catalogue.iter().enumerate() {
            let state = installed_fixes
                .iter()
                .find(|inf| inf.id == f.id)
                .map_or(GameFixState::NotInstalled, |inf| inf.state);

            out.push_str("    {\n");
            out.push_str(&format!("      \"id\": \"{}\",\n", f.id));
            out.push_str(&format!("      \"title\": \"{}\",\n", escape_json(&f.title)));
            out.push_str(&format!("      \"category\": \"{}\",\n", f.category.as_str()));
            out.push_str(&format!("      \"maturity\": \"{}\",\n", f.maturity.as_str()));
            out.push_str("      \"supportedSteamBuildIds\": [");
            for (bi, b) in f.supported_steam_build_ids.iter().enumerate() {
                out.push_str(&format!(
                    "\"{}\"{}",
                    b,
                    if bi.saturating_add(1) < f.supported_steam_build_ids.len() {
                        ", "
                    } else {
                        ""
                    }
                ));
            }
            out.push_str("],\n");
            out.push_str(&format!("      \"state\": \"{}\"\n", state.as_str()));
            out.push_str(&format!(
                "    }}{}",
                if i.saturating_add(1) < catalogue.len() {
                    ",\n"
                } else {
                    "\n"
                }
            ));
        }
        out.push_str("  ],\n");

        out.push_str("  \"compatibleRecommendedFixIds\": [");
        for (i, f) in compatible_recommended.iter().enumerate() {
            out.push_str(&format!(
                "\"{}\"{}",
                f.id,
                if i.saturating_add(1) < compatible_recommended.len() {
                    ", "
                } else {
                    ""
                }
            ));
        }
        out.push_str("],\n");

        out.push_str(&format!(
            "  \"datasetVersion\": \"{}\",\n",
            GameFixCatalog::DATASET_VERSION
        ));
        match status_note {
            Some(note) => out.push_str(&format!("  \"statusNote\": \"{}\"\n", escape_json(note))),
            None => out.push_str("  \"statusNote\": null\n"),
        }
        out.push_str("}\n");
        print!("{out}");
    } else {
        let build_str = steam_build_id.as_deref().unwrap_or("unknown");
        let experimental_count = catalogue
            .iter()
            .filter(|f| f.maturity == GameFixMaturity::Experimental)
            .count();
        println!(
            "{}; Steam build {}; {} catalogued; {} compatible safe recommendations; {} installed; {} experimental.",
            target.title(),
            build_str,
            catalogue.len(),
            compatible_recommended.len(),
            installed_fixes.len(),
            experimental_count
        );
    }

    ExitCode::Done
}

fn run_preset(args: &[String]) -> ExitCode {
    if args.len() < 3 || args.len() > 4 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(preset_str) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let preset = match GameFixPreset::parse(preset_str) {
        Some(p) => p,
        None => {
            eprintln!("Preset must be essential, recommended, or all-safe. Custom selections use individual fix install/remove commands.");
            return ExitCode::Usage;
        }
    };

    let Some(target_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let target = match GameTarget::parse(target_str) {
        Some(t) => t,
        None => {
            eprintln!("Unknown game target: {target_str}");
            return ExitCode::Usage;
        }
    };

    let Some(dir_str) = args.get(2) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let json = if args.len() == 4 {
        if args.get(3).map(String::as_str) == Some("--json") {
            true
        } else {
            eprintln!("{FIXES_USAGE}");
            return ExitCode::Usage;
        }
    } else {
        false
    };

    let selected = GameFixCatalog::for_preset(target, preset);
    let engine = GameFixEngine::new();

    let result = match engine.apply_fixes(target, preset, &selected, game_dir) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("Error applying preset: {e}");
            return map_error_to_exit_code(&e);
        }
    };

    if json {
        let mut out = String::new();
        out.push_str("{\n");
        out.push_str(&format!("  \"game\": \"{}\",\n", target.id()));
        out.push_str(&format!("  \"preset\": \"{}\",\n", result.preset.as_str()));
        out.push_str(&format!("  \"selectedFixCount\": {},\n", result.selected_fix_count));

        out.push_str("  \"installedFixIds\": [");
        for (i, id) in result.installed_fix_ids.iter().enumerate() {
            out.push_str(&format!(
                "\"{}\"{}",
                id,
                if i.saturating_add(1) < result.installed_fix_ids.len() {
                    ", "
                } else {
                    ""
                }
            ));
        }
        out.push_str("],\n");

        out.push_str("  \"alreadyInstalledFixIds\": [");
        for (i, id) in result.already_installed_fix_ids.iter().enumerate() {
            out.push_str(&format!(
                "\"{}\"{}",
                id,
                if i.saturating_add(1) < result.already_installed_fix_ids.len() {
                    ", "
                } else {
                    ""
                }
            ));
        }
        out.push_str("],\n");

        out.push_str(&format!("  \"changed\": {}\n", result.changed()));
        out.push_str("}\n");
        print!("{out}");
    } else {
        println!(
            "{}: installed {} of {} safe fix(es); {} already current.",
            result.preset.as_str(),
            result.installed_fix_ids.len(),
            result.selected_fix_count,
            result.already_installed_fix_ids.len()
        );
    }

    ExitCode::Done
}

fn run_install(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(fix_id) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let Some(dir_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let definition = match GameFixCatalog::try_get(fix_id) {
        Some(def) => def,
        None => {
            eprintln!("Fix is not present in the evidence-validated catalogue: {fix_id}");
            return ExitCode::Refused;
        }
    };

    let engine = GameFixEngine::new();
    match engine.install(definition, game_dir) {
        Ok(result) => {
            let note = if result.changed {
                " (changed)"
            } else {
                " (already current)"
            };
            println!("{}: {}{}", result.state.as_str(), definition.id, note);
            ExitCode::Done
        }
        Err(e) => {
            eprintln!("Install failed: {e}");
            map_error_to_exit_code(&e)
        }
    }
}

fn run_update(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(fix_id) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let Some(dir_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let definition = match GameFixCatalog::try_get(fix_id) {
        Some(def) => def,
        None => {
            eprintln!("Fix is not present in the evidence-validated catalogue: {fix_id}");
            return ExitCode::Refused;
        }
    };

    let engine = GameFixEngine::new();
    match engine.update(definition, game_dir) {
        Ok(result) => {
            let note = if result.changed {
                " (updated)"
            } else {
                " (already current)"
            };
            println!("{}: {}{}", result.state.as_str(), definition.id, note);
            ExitCode::Done
        }
        Err(e) => {
            eprintln!("Update failed: {e}");
            map_error_to_exit_code(&e)
        }
    }
}

fn run_remove(args: &[String]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(fix_id) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let Some(dir_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let engine = GameFixEngine::new();
    match engine.uninstall(fix_id, game_dir) {
        Ok(result) => {
            let note = if result.changed { " (restored)" } else { " (unchanged)" };
            println!("{}: {}{}", result.state.as_str(), fix_id, note);
            ExitCode::Done
        }
        Err(e) => {
            eprintln!("Remove failed: {e}");
            map_error_to_exit_code(&e)
        }
    }
}

fn run_extract(args: &[String]) -> ExitCode {
    if args.len() < 3 {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    }

    let Some(target_str) = args.first() else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let target = match GameTarget::parse(target_str) {
        Some(t) => t,
        None => {
            eprintln!("Unknown game target: {target_str}");
            return ExitCode::Usage;
        }
    };

    let Some(dir_str) = args.get(1) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let game_dir = Path::new(dir_str);

    let Some(out_str) = args.get(2) else {
        eprintln!("{FIXES_USAGE}");
        return ExitCode::Usage;
    };
    let out_dir = Path::new(out_str);

    let mut archives_only = false;
    let mut prefixes = Vec::new();

    let rest = args.get(3..).unwrap_or_default();
    for arg in rest {
        if arg == "--archives-only" {
            archives_only = true;
        } else {
            prefixes.push(arg.as_str());
        }
    }

    match GameFileExtractor::extract(target, game_dir, out_dir, &prefixes, archives_only) {
        Ok((files, issues)) => {
            for issue in issues {
                eprintln!("Warning: {issue}");
            }
            println!("{} files -> {}", files, out_dir.display());
            ExitCode::Done
        }
        Err(e) => {
            eprintln!("Extraction failed: {e}");
            map_error_to_exit_code(&e)
        }
    }
}

fn map_error_to_exit_code(err: &sse_core::Error) -> ExitCode {
    match err {
        sse_core::Error::Refused(_) => ExitCode::Refused,
        sse_core::Error::Damaged(_) => ExitCode::Damaged,
        sse_core::Error::System(_) => ExitCode::System,
    }
}

fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}
