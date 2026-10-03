//! Game file extraction utility for research and fix development.

use std::fs;
use std::path::Path;

use sse_content::file_tree::{CompanionGame, GameFileTree};
use sse_core::{Error, Result};

use crate::models::GameTarget;

/// Game file extractor.
pub struct GameFileExtractor;

impl GameFileExtractor {
    /// Extracts files from the game archives and loose directories according to LocatorAPI rules.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Refused`] on unsupported targets or I/O failure.
    pub fn extract(
        target: GameTarget,
        game_directory: &Path,
        output_directory: &Path,
        prefixes: &[&str],
        archives_only: bool,
    ) -> Result<(usize, Vec<String>)> {
        let (game, fsgame) = match target {
            GameTarget::ShadowOfChernobyl => (CompanionGame::ShadowOfChernobyl, "fsgame.ltx"),
            GameTarget::ClearSky => (CompanionGame::ClearSky, "fsgame.ltx"),
            GameTarget::CallOfPripyat => (CompanionGame::CallOfPripyat, "fsgame.ltx"),
            GameTarget::ShadowOfChernobylEnhancedEdition => (CompanionGame::ShadowOfChernobyl, "fsgame_soc.ltx"),
            GameTarget::ClearSkyEnhancedEdition => (CompanionGame::ClearSky, "fsgame_cs.ltx"),
            GameTarget::CallOfPripyatEnhancedEdition => (CompanionGame::CallOfPripyat, "fsgame_cop.ltx"),
            GameTarget::Stalker2 => {
                return Err(Error::Refused("S2 has no X-Ray archives".to_string()));
            }
        };

        let wanted_prefixes: Vec<String> = prefixes
            .iter()
            .map(|p| p.replace('\\', "/").trim_start_matches('/').to_string())
            .collect();

        let wanted_filter = |path: &str| {
            if wanted_prefixes.is_empty() {
                true
            } else {
                wanted_prefixes
                    .iter()
                    .any(|prefix| path.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
            }
        };

        let tree = GameFileTree::load(
            game,
            game_directory,
            wanted_filter,
            Some(&[fsgame]),
            !archives_only,
            true,
            archives_only,
            None,
            None,
        )?;

        let mut issues = tree.issues;
        let mut written: usize = 0;
        let out_canonical = output_directory
            .canonicalize()
            .unwrap_or_else(|_| output_directory.to_path_buf());

        for (relative, file) in &tree.files {
            let mut destination = out_canonical.clone();
            for seg in relative.split('/') {
                destination.push(seg);
            }

            if !destination.starts_with(&out_canonical) {
                issues.push(format!("skipped a path outside the output folder: {relative}"));
                continue;
            }

            match file.read() {
                Ok(bytes) => {
                    if let Some(parent) = destination.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if fs::write(&destination, bytes).is_ok() {
                        written = written.saturating_add(1);
                    } else {
                        issues.push(format!("failed to write: {relative}"));
                    }
                }
                Err(err) => {
                    issues.push(format!("{relative}: {err:?}"));
                }
            }
        }

        Ok((written, issues))
    }
}
