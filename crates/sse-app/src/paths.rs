//! Default application filesystem paths.
//!
//! Follows the C# editor conventions:
//! - Data directory from `STALKER_SAVE_EDITOR_DATA` env var if present.
//! - Fallback:
//!   - Windows: `%LOCALAPPDATA%/StalkerSaveEditor`
//!   - Linux/other: `~/.local/share/StalkerSaveEditor` or `~/.StalkerSaveEditor`
//! - Settings file: `<DataDirectory>/settings.json`

use std::env;
use std::path::PathBuf;

/// Returns the default root directory where the application keeps its own data
/// (never the game's folders).
#[must_use]
pub fn default_data_directory() -> PathBuf {
    if let Ok(custom) = env::var("STALKER_SAVE_EDITOR_DATA") {
        if !custom.trim().is_empty() {
            return PathBuf::from(custom);
        }
    }

    #[cfg(windows)]
    {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            if !local_app_data.trim().is_empty() {
                return PathBuf::from(local_app_data).join("StalkerSaveEditor");
            }
        }
        if let Ok(user_profile) = env::var("USERPROFILE") {
            if !user_profile.trim().is_empty() {
                return PathBuf::from(user_profile)
                    .join("AppData")
                    .join("Local")
                    .join("StalkerSaveEditor");
            }
        }
    }

    #[cfg(not(windows))]
    {
        if let Ok(xdg_data_home) = env::var("XDG_DATA_HOME") {
            if !xdg_data_home.trim().is_empty() {
                return PathBuf::from(xdg_data_home).join("StalkerSaveEditor");
            }
        }
        if let Ok(home) = env::var("HOME") {
            if !home.trim().is_empty() {
                return PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("StalkerSaveEditor");
            }
        }
    }

    PathBuf::from("StalkerSaveEditor")
}

/// Returns the default path to `settings.json`.
#[must_use]
pub fn default_settings_path() -> PathBuf {
    default_data_directory().join("settings.json")
}
