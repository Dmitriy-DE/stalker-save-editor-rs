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
