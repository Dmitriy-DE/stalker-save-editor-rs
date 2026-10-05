//! Application settings compatible with C# `AppSettings.cs`.
//!
//! Stored in `settings.json` formatted as indented UTF-8 JSON with snake_case keys.
//! Reading missing or damaged files falls back to defaults without panicking.
//! Saving is atomic and durable: writes to a unique temporary file, flushes/syncs,
//! renames over the destination, and syncs the parent directory.

use sse_codecs::json::{Event, NumberExt, Reader, Writer};
use sse_core::{Error, Result};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

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
    /// Whether telemetry/crash reports are allowed. Default is `false` until explicit first-run consent.
    pub send_reports: bool,
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

    /// Loads settings from `path`. If the file does not exist, cannot be read,
    /// or contains damaged JSON, returns default settings without failing.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(_) => return Self::default(),
        };
        Self::from_json_slice(&bytes).unwrap_or_default()
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
                writer.string(&dir.to_string_lossy())?;
            }
            writer.array_end()?;
        } else {
            writer.null()?;
        }

        writer.key("backup_directory")?;
        if let Some(dir) = &self.backup_directory {
            writer.string(&dir.to_string_lossy())?;
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
    /// Writes to a temporary file (`.<filename>.<pid>-<id>.tmp`) in the destination directory,
    /// flushes to disk (`sync_all`), renames over destination, and flushes the directory.
    ///
    /// # Errors
    /// Returns an error if filesystem operations or serialization fail.
    pub fn save(&self, path: &Path) -> Result<()> {
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent)?;

        let bytes = self.to_json_bytes()?;
        let id = NEXT_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("settings.json");
        let temp_path = parent.join(format!(".{file_name}.{}-{id}.tmp", std::process::id()));

        let write_result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options.open(&temp_path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&temp_path, fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);

            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                return Err(Error::Refused("target path is a symbolic link".to_owned()));
            }

            fs::rename(&temp_path, path)?;
            sync_directory(parent);
            Ok(())
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }

        write_result
    }
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
    if let Ok(handle) = File::open(directory) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = directory;
}
