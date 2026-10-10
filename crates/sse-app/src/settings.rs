//! Application settings compatible with C# `AppSettings.cs`.
//!
//! Stored in `settings.json` formatted as indented UTF-8 JSON with snake_case keys.
//! Missing files use defaults; unreadable or damaged files return an error.
//! Saving is atomic and durable: writes to a unique temporary file, flushes/syncs,
//! renames over the destination, and syncs the parent directory.

use sse_codecs::json::{Event, NumberExt, Reader, Writer};
use sse_core::{Error, Result};
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

// A first-run settings save must not race another in-process writer for the absent destination.
static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

/// User preferences matching `src/StalkerSaveEditor.Desktop/Services/AppSettings.cs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSettings {
    /// Save directories override; `None` means detect automatically.
    pub save_directories: Option<Vec<PathBuf>>,
    /// Backup directory override; `None` means default `<DataDirectory>/backups`.
    pub backup_directory: Option<PathBuf>,
    /// Interface language code (e.g. "ru", "en"); `None` means detect system language.
    pub language: Option<String>,
    /// Whether sound effects are enabled. Default is `true`.
    pub sound_enabled: bool,
    /// Sound volume in percent (0..100). Default is `80`.
    pub sound_volume: u32,
    /// Whether music playback is enabled. Default is `false`.
    pub music_enabled: bool,
    /// Selected visual theme ID. Default is `"zone"`.
    pub theme_id: String,
    /// Selected visual accent ID. Default is `"amber"`.
    pub accent_id: String,
    /// UI scale percent (0 means "fit screen automatically"). Default is `0`.
    pub ui_scale_percent: u32,
    /// Navigation sidebar collapsed state; `None` until user folds or unfolds it.
    pub navigation_collapsed: Option<bool>,
    /// Whether telemetry/crash reports are allowed. Default is `false` until the user opts in.
    pub send_reports: bool,
    /// Whether aggregate performance metrics may be uploaded. Default is `false`.
    pub send_metrics: bool,
    /// Whether the user has seen the first-run notice about reports. Default is `false`.
    pub reports_notice_shown: bool,
    /// Timestamp of last sent report in UTC (ISO 8601 string, e.g. "2026-10-03T12:00:00Z").
    pub last_report_utc: Option<String>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            save_directories: None,
            backup_directory: None,
            language: None,
            sound_enabled: true,
            sound_volume: 80,
            music_enabled: false,
            theme_id: "zone".to_owned(),
            accent_id: "amber".to_owned(),
            ui_scale_percent: 0,
            navigation_collapsed: None,
            send_reports: false,
            send_metrics: false,
            reports_notice_shown: false,
            last_report_utc: None,
        }
    }
}

impl AppSettings {
    /// Creates a new `AppSettings` with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads settings from `path`. A missing file uses defaults; read or parse errors are preserved.
    ///
    /// # Errors
    /// Returns an error when an existing settings file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read(path) {
            Ok(bytes) => Self::from_json_slice(&bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }

    /// Parses settings from a JSON byte slice.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed.
    pub fn from_json_slice(bytes: &[u8]) -> Result<Self> {
        let mut reader = Reader::new(bytes);
        let first = reader.next_event()?.ok_or_else(|| Error::damaged("empty JSON"))?;
        if first != Event::ObjectStart {
            return Err(Error::damaged("settings root must be an object"));
        }

        let mut settings = Self::default();

        loop {
            let event = reader
                .next_event()?
                .ok_or_else(|| Error::damaged("unexpected end of settings object"))?;
            match event {
                Event::ObjectEnd => break,
                Event::Key(key) => {
                    let key_str = key.as_str();
                    match key_str {
                        "save_directories" => {
                            settings.save_directories = parse_optional_path_array(&mut reader)?;
                        }
                        "backup_directory" => {
                            settings.backup_directory = parse_optional_path(&mut reader)?;
                        }
                        "language" => {
                            settings.language = parse_optional_string(&mut reader)?;
                        }
                        "sound_enabled" => {
                            if let Some(val) = parse_optional_bool(&mut reader)? {
                                settings.sound_enabled = val;
                            }
                        }
                        "sound_volume" => {
                            if let Some(val) = parse_optional_u32(&mut reader)? {
                                settings.sound_volume = val.min(100);
                            }
                        }
                        "music_enabled" => {
                            if let Some(val) = parse_optional_bool(&mut reader)? {
                                settings.music_enabled = val;
                            }
                        }
                        "theme_id" => {
                            if let Some(val) = parse_optional_string(&mut reader)? {
                                if !val.is_empty() {
                                    settings.theme_id = val;
                                }
                            }
                        }
                        "accent_id" => {
                            if let Some(val) = parse_optional_string(&mut reader)? {
                                if !val.is_empty() {
                                    settings.accent_id = val;
                                }
                            }
                        }
                        "ui_scale_percent" => {
                            if let Some(val) = parse_optional_u32(&mut reader)? {
                                settings.ui_scale_percent = val;
                            }
                        }
                        "navigation_collapsed" => {
                            settings.navigation_collapsed = parse_optional_bool(&mut reader)?;
                        }
                        "send_reports" => {
                            if let Some(val) = parse_optional_bool(&mut reader)? {
                                settings.send_reports = val;
                            }
                        }
                        "send_metrics" => {
                            if let Some(val) = parse_optional_bool(&mut reader)? {
                                settings.send_metrics = val;
                            }
                        }
                        "reports_notice_shown" => {
                            if let Some(val) = parse_optional_bool(&mut reader)? {
                                settings.reports_notice_shown = val;
                            }
                        }
                        "last_report_utc" => {
                            settings.last_report_utc = parse_optional_string(&mut reader)?;
                        }
                        _ => {
                            // Forward compatibility: skip unknown properties
                            reader.skip_value()?;
                        }
                    }
                }
                _ => return Err(Error::damaged("expected object key or end")),
            }
        }

        if reader.next_event()?.is_some() {
            return Err(Error::damaged("trailing events after settings object"));
        }

        Ok(settings)
    }

    /// Serializes settings to JSON bytes in indented format matching C# editor output.
    ///
    /// # Errors
    /// Returns an error if writing fails.
    pub fn to_json_bytes(&self) -> Result<Vec<u8>> {
        let mut writer = Writer::indented();
        writer.object_start()?;

        writer.key("save_directories")?;
        if let Some(dirs) = &self.save_directories {
            writer.array_start()?;
            for dir in dirs {
                writer.string(path_to_json_string(dir)?)?;
            }
            writer.array_end()?;
        } else {
            writer.null()?;
        }

        writer.key("backup_directory")?;
        if let Some(dir) = &self.backup_directory {
            writer.string(path_to_json_string(dir)?)?;
        } else {
            writer.null()?;
        }

        writer.key("language")?;
        if let Some(lang) = &self.language {
            writer.string(lang)?;
        } else {
            writer.null()?;
        }

        writer.key("sound_enabled")?;
        writer.bool(self.sound_enabled)?;

        writer.key("sound_volume")?;
        writer.u64(u64::from(self.sound_volume))?;

        writer.key("music_enabled")?;
        writer.bool(self.music_enabled)?;

        writer.key("theme_id")?;
        writer.string(&self.theme_id)?;

        writer.key("accent_id")?;
        writer.string(&self.accent_id)?;

        writer.key("ui_scale_percent")?;
        writer.u64(u64::from(self.ui_scale_percent))?;

        writer.key("navigation_collapsed")?;
        if let Some(collapsed) = self.navigation_collapsed {
            writer.bool(collapsed)?;
        } else {
            writer.null()?;
        }

        writer.key("send_reports")?;
        writer.bool(self.send_reports)?;

        writer.key("send_metrics")?;
        writer.bool(self.send_metrics)?;

        writer.key("reports_notice_shown")?;
        writer.bool(self.reports_notice_shown)?;

        writer.key("last_report_utc")?;
        if let Some(ts) = &self.last_report_utc {
            writer.string(ts)?;
        } else {
            writer.null()?;
        }

        writer.object_end()?;
        let mut bytes = writer.finish()?;
        // Append trailing newline for POSIX/editor standard
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Atomically and durably saves settings to the specified path.
    ///
    /// Writes to a temporary file in the destination directory, flushes it to disk, atomically
    /// replaces the destination, and flushes the directory.
    ///
    /// # Errors
    /// Returns an error if filesystem operations or serialization fail.
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let bytes = self.to_json_bytes()?;
        let _write_guard = SETTINGS_WRITE_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sse_sys::secure_fs::atomic_write_checked(
            path,
            &bytes,
            sse_sys::secure_fs::AtomicWriteOptions::create_or_replace()
                .with_unix_mode(0o600)
                .without_parent_sync(),
            |candidate| match fs::symlink_metadata(candidate) {
                Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "target path is a symbolic link",
                )),
                Ok(_) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(error),
            },
        )
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied
                && error.to_string() == "target path is a symbolic link"
            {
                Error::Refused("target path is a symbolic link".to_owned())
            } else {
                Error::from(error)
            }
        })?;
        sync_directory(parent);
        Ok(())
    }
}

/// Temporary files older than this are left by an interrupted save and may be removed at startup.
pub const STALE_SETTINGS_TEMPORARY_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Removes `settings.json` temporary files that an interrupted save left behind.
///
/// Only files named like a settings temporary and last changed at least [`STALE_SETTINGS_TEMPORARY_AGE`]
/// before `now` are removed, so a save that is running now is never touched. Returns how many were removed.
pub fn remove_stale_settings_temporaries(directory: &Path, now: std::time::SystemTime) -> usize {
    let Ok(entries) = fs::read_dir(directory) else {
        return 0;
    };
    let mut removed: usize = 0;
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !is_settings_temporary_name(name) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let age = now.duration_since(modified).unwrap_or_default();
        if age >= STALE_SETTINGS_TEMPORARY_AGE && fs::remove_file(entry.path()).is_ok() {
            removed = removed.saturating_add(1);
        }
    }
    removed
}

/// Names used by atomic settings saves: the current `.sse-tmp-` form and the older `.tmp` form.
fn is_settings_temporary_name(name: &str) -> bool {
    name.starts_with(".settings.json.") && (name.contains(".sse-tmp-") || name.ends_with(".tmp"))
}

fn path_to_json_string(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| Error::Refused("settings path is not valid UTF-8".to_owned()))
}

fn parse_optional_string(reader: &mut Reader<'_>) -> Result<Option<String>> {
    let event = reader.next_event()?.ok_or_else(|| Error::damaged("expected value"))?;
    match event {
        Event::String(t) => Ok(Some(t.into_owned())),
        Event::Null => Ok(None),
        _ => Err(Error::damaged("expected string or null")),
    }
}

fn parse_optional_path(reader: &mut Reader<'_>) -> Result<Option<PathBuf>> {
    parse_optional_string(reader).map(|opt| opt.map(PathBuf::from))
}

fn parse_optional_bool(reader: &mut Reader<'_>) -> Result<Option<bool>> {
    let event = reader.next_event()?.ok_or_else(|| Error::damaged("expected value"))?;
    match event {
        Event::Bool(b) => Ok(Some(b)),
        Event::Null => Ok(None),
        _ => Err(Error::damaged("expected bool or null")),
    }
}

fn parse_optional_u32(reader: &mut Reader<'_>) -> Result<Option<u32>> {
    let event = reader.next_event()?.ok_or_else(|| Error::damaged("expected value"))?;
    match event {
        Event::Number(num_str) => {
            let u = num_str
                .as_u64()
                .ok_or_else(|| Error::damaged("invalid unsigned number"))?;
            let val = u32::try_from(u).map_err(|_| Error::damaged("number exceeds u32 range"))?;
            Ok(Some(val))
        }
        Event::Null => Ok(None),
        _ => Err(Error::damaged("expected number or null")),
    }
}

fn parse_optional_path_array(reader: &mut Reader<'_>) -> Result<Option<Vec<PathBuf>>> {
    let event = reader
        .next_event()?
        .ok_or_else(|| Error::damaged("expected array or null"))?;
    match event {
        Event::Null => Ok(None),
        Event::ArrayStart => {
            let mut list = Vec::new();
            loop {
                let elem = reader.next_event()?.ok_or_else(|| Error::damaged("truncated array"))?;
                match elem {
                    Event::ArrayEnd => break,
                    Event::String(t) => {
                        list.push(PathBuf::from(t.into_owned()));
                    }
                    _ => return Err(Error::damaged("expected string inside path array")),
                }
            }
            Ok(Some(list))
        }
        _ => Err(Error::damaged("expected array or null")),
    }
}

fn sync_directory(directory: &Path) {
    #[cfg(unix)]
    match File::open(directory) {
        Ok(handle) => {
            if let Err(error) = handle.sync_all() {
                crate::diagnostics::warn(&format!("failed to sync settings directory after rename: {error}"));
            }
        }
        Err(error) => {
            crate::diagnostics::warn(&format!(
                "failed to open settings directory for sync after rename: {error}"
            ));
        }
    }
    #[cfg(not(unix))]
    let _ = directory;
}
