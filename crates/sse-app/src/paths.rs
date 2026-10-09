//! Default application filesystem paths.
//!
//! Follows the C# editor conventions:
//! - Data directory from `STALKER_SAVE_EDITOR_DATA` env var if present.
//! - Fallback:
//!   - Windows: `%LOCALAPPDATA%/StalkerSaveEditor`
//!   - Linux/other: `~/.local/share/StalkerSaveEditor` or `~/.StalkerSaveEditor`
//! - Settings file: `<DataDirectory>/settings.json`

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Returns the default root directory where the application keeps its own data
/// (never the game's folders).
#[must_use]
pub fn default_data_directory() -> PathBuf {
    if is_test_or_example_executable() {
        if let Some(custom) = env::var_os("STALKER_SAVE_EDITOR_DATA")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().to_string_lossy().trim().is_empty())
            .filter(|path| is_under_temporary_directory(path))
        {
            return custom;
        }
        return env::temp_dir().join(format!("stalker-save-editor-test-{}", std::process::id()));
    }

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

fn is_test_or_example_executable() -> bool {
    cfg!(test)
        || env::current_exe()
            .ok()
            .and_then(|executable| executable.parent().map(Path::to_path_buf))
            .and_then(|parent| parent.file_name().map(|name| name.to_owned()))
            .is_some_and(|name| name == "deps" || name == "examples")
}

fn is_under_temporary_directory(path: &Path) -> bool {
    let Ok(temporary_directory) = fs::canonicalize(env::temp_dir()) else {
        return false;
    };
    let Ok(path) = fs::canonicalize(path) else {
        return false;
    };
    path.starts_with(temporary_directory)
}

/// Returns the configured backup directory, or `<DataDirectory>/backups`.
#[must_use]
pub fn backup_directory(settings: &crate::settings::AppSettings) -> PathBuf {
    settings
        .backup_directory
        .as_ref()
        .filter(|directory| !directory.as_os_str().is_empty())
        .cloned()
        .unwrap_or_else(|| default_data_directory().join("backups"))
}

/// Returns the default path to `settings.json`.
#[must_use]
pub fn default_settings_path() -> PathBuf {
    default_data_directory().join("settings.json")
}

/// Returns the private directory used for downloaded update packages.
#[must_use]
pub fn update_download_directory() -> PathBuf {
    default_data_directory().join("updates")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_directory_uses_settings_override() {
        let settings = crate::settings::AppSettings {
            backup_directory: Some(PathBuf::from("custom-backups")),
            ..crate::settings::AppSettings::default()
        };

        assert_eq!(backup_directory(&settings), PathBuf::from("custom-backups"));
    }

    #[test]
    fn backup_directory_defaults_under_application_data_directory() {
        let settings = crate::settings::AppSettings::default();

        assert_eq!(backup_directory(&settings), default_data_directory().join("backups"));
    }

    #[test]
    fn empty_backup_override_uses_application_data_directory() {
        let settings = crate::settings::AppSettings {
            backup_directory: Some(PathBuf::new()),
            ..crate::settings::AppSettings::default()
        };

        assert_eq!(backup_directory(&settings), default_data_directory().join("backups"));
    }

    #[test]
    fn test_executables_default_app_data_and_logs_to_temporary_root() -> sse_core::Result<()> {
        let _guard = crate::diagnostics::LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| sse_core::Error::System("log directory test gate poisoned".to_owned()))?;
        let data_directory = default_data_directory();
        let data_is_in_temp_dir = env::temp_dir()
            .canonicalize()
            .ok()
            .zip(data_directory.parent().and_then(|parent| fs::canonicalize(parent).ok()))
            .zip(data_directory.file_name())
            .is_some_and(|((temporary_directory, data_parent), data_name)| {
                data_parent.join(data_name).starts_with(temporary_directory)
            });
        assert!(
            data_is_in_temp_dir,
            "test data must stay under the temporary directory: {}",
            data_directory.display()
        );
        assert_eq!(crate::diagnostics::log_directory(), data_directory.join("logs"));
        Ok(())
    }
}
