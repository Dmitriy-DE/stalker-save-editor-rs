//! S.T.A.L.K.E.R. 2 mod folder toggle engine (`~mods` <-> `~mods.disabled`).
//!
//! Enforces symlink/reparse-point protections and takes game installation path explicitly.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fs_util::check_no_links;
use sse_core::{Error, Result};

const PAKS_RELATIVE_PATH: &str = "Stalker2/Content/Paks";
const ACTIVE_MODS_FOLDER: &str = "~mods";
const DISABLED_MODS_FOLDER: &str = "~mods.disabled";

/// Current state of S.T.A.L.K.E.R. 2 mod folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModToggleStatus {
    /// Mods are currently enabled in `~mods`.
    Active {
        /// Number of `.pak` mod files.
        pak_count: usize,
        /// List of `.pak` file names.
        pak_names: Vec<String>,
    },
    /// Mods are currently disabled in `~mods.disabled`.
    Disabled {
        /// Number of `.pak` mod files.
        pak_count: usize,
        /// List of `.pak` file names.
        pak_names: Vec<String>,
    },
    /// Both folders exist simultaneously (conflict needing resolution).
    Conflicted {
        /// Number of active paks.
        active_count: usize,
        /// Number of disabled paks.
        disabled_count: usize,
    },
    /// Mod folder exists but contains no `.pak` files.
    Empty,
    /// Neither mod folder currently exists.
    NotFound,
    /// The target directory is not a recognized S.T.A.L.K.E.R. 2 installation.
    NotAnS2Installation,
}

/// Outcome of a mod toggle action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModToggleResult {
    /// Successfully enabled mods (renamed `~mods.disabled` to `~mods`).
    Enabled {
        /// Number of enabled mods.
        pak_count: usize,
    },
    /// Successfully disabled mods (renamed `~mods` to `~mods.disabled`).
    Disabled {
        /// Number of disabled mods.
        pak_count: usize,
    },
    /// Created a new empty `~mods` directory ready for mod placement.
    CreatedActiveDirectory,
}

/// Service managing the S.T.A.L.K.E.R. 2 `~mods` folder toggle.
pub struct Stalker2ModToggle;

impl Stalker2ModToggle {
    /// Resolves the base Paks directory for a S.T.A.L.K.E.R. 2 installation.
    #[must_use]
    pub fn resolve_paks_dir(game_directory: &Path) -> Option<PathBuf> {
        let candidate = game_directory.join(PAKS_RELATIVE_PATH);
        if candidate.is_dir() {
            Some(candidate)
        } else {
            None
        }
    }

    /// Queries the current status of mods in the S.T.A.L.K.E.R. 2 installation.
    ///
    /// # Errors
    /// Returns an error if directory validation fails or symlinks are detected.
    pub fn query_status(game_directory: &Path) -> Result<ModToggleStatus> {
        let paks_dir = match Self::resolve_paks_dir(game_directory) {
            Some(dir) => dir,
            None => return Ok(ModToggleStatus::NotAnS2Installation),
        };

        let active_path = paks_dir.join(ACTIVE_MODS_FOLDER);
        let disabled_path = paks_dir.join(DISABLED_MODS_FOLDER);

        if active_path.exists() {
            check_no_links(game_directory, &active_path)?;
        }
        if disabled_path.exists() {
            check_no_links(game_directory, &disabled_path)?;
        }

        let active_exists = active_path.is_dir();
        let disabled_exists = disabled_path.is_dir();

        if active_exists && disabled_exists {
            let active_paks = list_pak_files(&active_path)?;
            let disabled_paks = list_pak_files(&disabled_path)?;
            return Ok(ModToggleStatus::Conflicted {
                active_count: active_paks.len(),
                disabled_count: disabled_paks.len(),
            });
        }

        if active_exists {
            let paks = list_pak_files(&active_path)?;
            if paks.is_empty() {
                return Ok(ModToggleStatus::Empty);
            }
            return Ok(ModToggleStatus::Active {
                pak_count: paks.len(),
                pak_names: paks,
            });
        }

        if disabled_exists {
            let paks = list_pak_files(&disabled_path)?;
            if paks.is_empty() {
                return Ok(ModToggleStatus::Empty);
            }
            return Ok(ModToggleStatus::Disabled {
                pak_count: paks.len(),
                pak_names: paks,
            });
        }

        Ok(ModToggleStatus::NotFound)
    }

    /// Toggles the mods state (active <-> disabled) in the given game installation.
    ///
    /// # Errors
    /// Returns an error if the directory is not an S2 installation, if symlinks are detected,
    /// or if both active and disabled folders exist concurrently.
    /// Toggles `~mods` only when the game is not running, using the given probe.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the game is running or the process list cannot be read.
    pub fn toggle_while_not_running(
        game_directory: &Path,
        probe: &dyn crate::running_game::GameRunningProbe,
    ) -> Result<ModToggleResult> {
        crate::running_game::ensure_game_not_running(probe, crate::models::GameTarget::Stalker2)?;
        Self::toggle(game_directory)
    }

    /// Renames `~mods` between active and disabled. Callers that can run while the game is open must use
    /// [`Stalker2ModToggle::toggle_while_not_running`] instead.
    pub fn toggle(game_directory: &Path) -> Result<ModToggleResult> {
        let paks_dir = Self::resolve_paks_dir(game_directory).ok_or_else(|| {
            Error::Refused(
                "Game directory is not a valid S.T.A.L.K.E.R. 2 installation (missing Stalker2/Content/Paks)"
                    .to_string(),
            )
        })?;

        refuse_paks_outside_game(game_directory, &paks_dir)?;

        let active_path = paks_dir.join(ACTIVE_MODS_FOLDER);
        let disabled_path = paks_dir.join(DISABLED_MODS_FOLDER);

        if active_path.exists() {
            check_no_links(game_directory, &active_path)?;
        }
        if disabled_path.exists() {
            check_no_links(game_directory, &disabled_path)?;
        }

        let active_exists = active_path.is_dir();
        let disabled_exists = disabled_path.is_dir();

        if active_exists && disabled_exists {
            return Err(Error::Refused(
                "Both ~mods and ~mods.disabled exist simultaneously. Please resolve manually before toggling."
                    .to_string(),
            ));
        }

        if active_exists {
            let paks = list_pak_files(&active_path)?;
            let pak_count = paks.len();
            fs::rename(&active_path, &disabled_path).map_err(Error::from)?;
            Ok(ModToggleResult::Disabled { pak_count })
        } else if disabled_exists {
            let paks = list_pak_files(&disabled_path)?;
            let pak_count = paks.len();
            fs::rename(&disabled_path, &active_path).map_err(Error::from)?;
            Ok(ModToggleResult::Enabled { pak_count })
        } else {
            fs::create_dir_all(&active_path).map_err(Error::from)?;
            Ok(ModToggleResult::CreatedActiveDirectory)
        }
    }
}

/// Refuses a `Paks` directory that a link on the way from the chosen game folder moves outside that folder.
///
/// Without this a link such as `Stalker2/Content/Paks -> /elsewhere` would make the toggle create or rename
/// `~mods` in a directory the user never selected.
fn refuse_paks_outside_game(game_directory: &Path, paks_dir: &Path) -> Result<()> {
    let game = fs::canonicalize(game_directory).map_err(Error::from)?;
    let paks = fs::canonicalize(paks_dir).map_err(Error::from)?;
    if paks.starts_with(&game) {
        Ok(())
    } else {
        Err(Error::Refused(format!(
            "Refusing to toggle mods through a link that leaves the game folder: {}",
            paks_dir.display()
        )))
    }
}

fn list_pak_files(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let entries = fs::read_dir(dir).map_err(Error::from)?;
    for entry in entries {
        let entry = entry.map_err(Error::from)?;
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                if file_name.ends_with(".pak") || file_name.ends_with(".PAK") {
                    names.push(file_name.to_string());
                }
            }
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(all(test, unix))]
mod link_tests {
    use super::{ModToggleResult, Stalker2ModToggle};
    use sse_core::{Error, Result};
    use std::fs;
    use std::path::PathBuf;

    fn temp(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("sse-s2-mods-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn toggle_refuses_a_paks_link_that_leaves_the_game_folder() -> Result<()> {
        let root = temp("escape");
        let game = root.join("game");
        let outside = root.join("outside");
        fs::create_dir_all(game.join("Stalker2/Content"))?;
        fs::create_dir_all(&outside)?;
        std::os::unix::fs::symlink(&outside, game.join("Stalker2/Content/Paks"))?;

        let result = Stalker2ModToggle::toggle(&game);

        assert!(matches!(result, Err(Error::Refused(_))), "{result:?}");
        assert!(
            !outside.join("~mods").exists(),
            "nothing may be created outside the game"
        );

        // Existing folders behind the link are not renamed either.
        fs::create_dir(outside.join("~mods"))?;
        assert!(matches!(Stalker2ModToggle::toggle(&game), Err(Error::Refused(_))));
        assert!(outside.join("~mods").is_dir());
        assert!(!outside.join("~mods.disabled").exists());
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn toggle_still_works_inside_a_real_game_folder() -> Result<()> {
        let root = temp("inside");
        let game = root.join("game");
        fs::create_dir_all(game.join("Stalker2/Content/Paks"))?;

        assert_eq!(
            Stalker2ModToggle::toggle(&game)?,
            ModToggleResult::CreatedActiveDirectory
        );
        assert!(game.join("Stalker2/Content/Paks/~mods").is_dir());
        assert_eq!(
            Stalker2ModToggle::toggle(&game)?,
            ModToggleResult::Disabled { pak_count: 0 }
        );
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
