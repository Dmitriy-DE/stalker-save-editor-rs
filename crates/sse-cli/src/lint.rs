//! `stalker-save lint` command: runs static checks on game files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sse_content::{collect_files_recursive, CompanionGame, EntryDecoder, GameFile, GameFileTree, HeaderDecoder};
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

/// Builds archive decoders (LZHUF header + LZO1x entries) matching what X-Ray uses.
///
/// The header table in X-Ray `.db` archives is either:
///   - stored raw (chunk type 1 without the compressed flag), or
///   - LZHUF-compressed, optionally preceded by the regional scrambler.
///
/// Entry data is always LZO1x-compressed when `compressed_size != uncompressed_size`.
fn make_archive_decoders() -> (HeaderDecoder, EntryDecoder) {
    let header_decoder: HeaderDecoder = Arc::new(|data: &[u8]| -> sse_core::Result<Vec<Vec<u8>>> {
        let mut candidates: Vec<Vec<u8>> = Vec::new();

        // Try plain LZHUF first (CoP resources/*.db and others)
        if let Ok(decoded) = sse_codecs::lzhuf::decode(data) {
            candidates.push(decoded);
        }

        // Try scramble-then-LZHUF (SoC gamedata.db*, CS gamedata)
        for world_wide in [true, false] {
            let descrambled = sse_codecs::lzhuf::descramble(data, world_wide);
            if let Ok(decoded) = sse_codecs::lzhuf::decode(&descrambled) {
                candidates.push(decoded);
            }
        }

        if candidates.is_empty() {
            Err(sse_core::Error::damaged("X-Ray archive header could not be decoded"))
        } else {
            Ok(candidates)
        }
    });

    let entry_decoder: EntryDecoder = Arc::new(|data: &[u8], expected_size: usize| -> sse_core::Result<Vec<u8>> {
        sse_codecs::lzo1x::decompress(data, expected_size)
    });

    (header_decoder, entry_decoder)
}

/// Loads a [`GameFileTree`] from a game directory.
///
/// If `fsgame.ltx` is present the full G1 file-tree loader is used so both
/// `.db` archives and loose `gamedata/` files are visible (archives first,
/// loose overlay on top), exactly as the X-Ray engine sees them.  Archive
/// headers are decoded with LZHUF (with regional descrambling if needed);
/// entry bodies are LZO1x-decompressed on demand (`defer_archive_content = true`).
///
/// When `fsgame.ltx` is absent the directory is walked recursively as a
/// loose-only tree (mod directories, extracted archives, CI fixtures).
fn load_tree_for_lint(folder: &Path) -> sse_core::Result<GameFileTree> {
    if folder.join("fsgame.ltx").exists() {
        let (header_decoder, entry_decoder) = make_archive_decoders();

        // Detect game family by the config sub-directory name.
        // SoC uses config/, CS and CoP use configs/.
        let game = if folder.join("gamedata/config").is_dir() {
            CompanionGame::ShadowOfChernobyl
        } else {
            CompanionGame::CallOfPripyat
        };

        let is_wanted = |p: &str| {
            let lower = p.to_ascii_lowercase();
            lower.ends_with(".ltx") || lower.ends_with(".xml") || lower.ends_with(".script")
        };

        if let Ok(tree) = GameFileTree::load(
            game,
            folder,
            is_wanted,
            None,
            true,
            true, // defer: decompress entries on demand to avoid loading all ~2000 files upfront
            false,
            Some(header_decoder),
            Some(entry_decoder),
        ) {
            return Ok(tree);
        }
    }

    // Fallback: loose directory walk (mod folders, CI fixture trees, etc.)
    let mut files: HashMap<String, GameFile> = HashMap::new();
    let paths = collect_files_recursive(folder);

    for p in paths {
        if let Ok(rel) = p.strip_prefix(folder) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let game_file = GameFile::from_path(rel_str.clone(), "loose", p);
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

fn print_json_report(report: &sse_lint::LintReport) {
    println!("{{");
    println!("  \"filesChecked\": {},", report.files_checked);
    println!("  \"elapsedMs\": {},", report.elapsed_ms);
    println!("  \"findings\": [");
    let mut first = true;
    for finding in &report.findings {
        if !first {
            println!(",");
        }
        first = false;
        let sev = match finding.severity {
            LintSeverity::Error => "error",
            LintSeverity::Warning => "warning",
            LintSeverity::Info => "info",
        };
        print!(
            "    {{\"checker\":\"{}\",\"file\":\"{}\",\"line\":{},\"severity\":\"{}\",\"message\":\"{}\"}}",
            finding.checker,
            finding.file.replace('"', "\\\""),
            finding.line,
            sev,
            finding.message.replace('"', "\\\"")
        );
    }
    if !report.findings.is_empty() {
        println!();
    }
    println!("  ]");
    println!("}}");
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::load_tree_for_lint;
    use std::fs;
    use std::path::PathBuf;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let path = std::env::temp_dir().join(format!("sse-lint-content-{unique}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    #[test]
    fn loose_lint_fallback_does_not_follow_directory_symlinks() {
        let root = TempDir::new();
        let config_dir = root.0.join("configs");
        fs::create_dir_all(&config_dir).unwrap();
        fs::write(config_dir.join("system.ltx"), b"[system]\n").unwrap();
        std::os::unix::fs::symlink(&config_dir, root.0.join("config_alias")).unwrap();

        let tree = load_tree_for_lint(&root.0).unwrap();

        assert!(tree.files.contains_key("configs/system.ltx"));
        assert!(!tree.files.contains_key("config_alias/system.ltx"));
    }

    #[cfg(unix)]
    #[test]
    fn loose_lint_fallback_keeps_paths_relative_to_a_symlink_root() {
        let root = TempDir::new();
        let install_dir = root.0.join("install");
        fs::create_dir_all(install_dir.join("configs")).unwrap();
        fs::write(install_dir.join("configs/system.ltx"), b"[system]\n").unwrap();
        let install_link = root.0.join("install-link");
        std::os::unix::fs::symlink(&install_dir, &install_link).unwrap();

        let tree = load_tree_for_lint(&install_link).unwrap();

        assert!(tree.files.contains_key("configs/system.ltx"));
    }
}
