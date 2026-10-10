//! Single-owner settings writer. All UI mutations are serialized in one worker; later patches win.

use crate::{default_settings_path, AppSettings};
use sse_core::{Error, Result};
use std::path::PathBuf;
use std::sync::{mpsc, OnceLock};

/// One ordered mutation applied by the settings writer.
#[derive(Clone, Debug)]
pub enum SettingsPatch {
    /// Replace the complete settings snapshot.
    Replace(AppSettings),
    /// Replace only the visual theme identifier.
    Theme(String),
    /// Replace only the accent identifier.
    Accent(String),
    /// Replace only the UI scale percentage.
    Scale(u32),
    /// Replace the configured save discovery directories.
    SaveDirectories(Vec<PathBuf>),
    /// Replace whether the navigation sidebar is collapsed.
    NavigationCollapsed(bool),
    /// Mark the reports notice as seen and optionally change report sending.
    ReportsNotice {
        /// New report-sending value, or None to keep the current value.
        send_reports: Option<bool>,
    },
    /// Set the interface language code; `None` returns to system detection.
    Language(Option<String>),
    /// Set whether sound effects are enabled.
    SoundEnabled(bool),
    /// Set whether menu music is enabled.
    MusicEnabled(bool),
    /// Set the menu music volume in percent.
    MusicVolume(u32),
    /// Set the sound volume in percent.
    SoundVolume(u32),
    /// Set the backup directory override; `None` restores the default.
    BackupDirectory(Option<PathBuf>),
    /// Set whether crash and telemetry reports may be sent.
    SendReports(bool),
    /// Set whether aggregate performance metrics may be uploaded.
    MetricsConsent(bool),
    /// Add directories to the save discovery list; paths already listed (ignoring case and spaces) are skipped.
    AddSaveDirectories(Vec<PathBuf>),
}

struct Request {
    patch: SettingsPatch,
    done: mpsc::Sender<Result<()>>,
}
static WRITER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();

fn sender() -> &'static mpsc::Sender<Request> {
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Request>();
        let _writer = std::thread::Builder::new()
            .name("settings-writer".to_owned())
            .spawn(move || run_writer(default_settings_path(), rx));
        tx
    })
}

fn run_writer(path: PathBuf, rx: mpsc::Receiver<Request>) {
    if let Some(directory) = path.parent() {
        crate::settings::remove_stale_settings_temporaries(directory, std::time::SystemTime::now());
    }
    let mut current = match AppSettings::load(&path) {
        Ok(settings) => Some(settings),
        Err(error) => {
            crate::diagnostics::warn(&format!("settings file could not be loaded: {error}"));
            None
        }
    };

    while let Ok(request) = rx.recv() {
        let unreadable = current.is_none();
        let next = match request.patch {
            SettingsPatch::Replace(settings) => Ok(settings),
            patch => {
                let mut updated = current.clone().unwrap_or_default();
                apply_patch(&mut updated, patch);
                Ok(updated)
            }
        };

        let result = match next {
            Ok(updated) => {
                let kept = if unreadable {
                    preserve_unreadable_copy(&path).map(|_| ())
                } else {
                    Ok(())
                };
                kept.and_then(|()| updated.save(&path).map(|()| updated))
                    .map(|saved| current = Some(saved))
            }
            Err(error) => Err(error),
        };
        let _ = request.done.send(result);
    }
}

const BROKEN_PREFIX: &str = "settings.json.broken-";

/// Keeps a copy of an unreadable settings file before the first write replaces it.
/// One copy is kept per broken content: an identical earlier copy is reused.
fn preserve_unreadable_copy(path: &std::path::Path) -> Result<PathBuf> {
    let bytes = std::fs::read(path)?;
    let directory = path
        .parent()
        .ok_or_else(|| Error::Refused("settings path has no directory".to_owned()))?;
    for entry in std::fs::read_dir(directory)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(BROKEN_PREFIX) && std::fs::read(entry.path())? == bytes {
            return Ok(entry.path());
        }
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| Error::Refused(format!("clock before 1970: {error}")))?
        .as_millis();
    let copy = directory.join(format!("{BROKEN_PREFIX}{stamp}"));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&copy)?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    Ok(copy)
}

fn apply_patch(settings: &mut AppSettings, patch: SettingsPatch) {
    match patch {
        SettingsPatch::Replace(value) => *settings = value,
        SettingsPatch::Theme(value) => settings.theme_id = value,
        SettingsPatch::Accent(value) => settings.accent_id = value,
        SettingsPatch::Scale(value) => settings.ui_scale_percent = value,
        SettingsPatch::SaveDirectories(value) => settings.save_directories = Some(value),
        SettingsPatch::NavigationCollapsed(value) => settings.navigation_collapsed = Some(value),
        SettingsPatch::ReportsNotice { send_reports } => {
            settings.reports_notice_shown = true;
            if let Some(value) = send_reports {
                settings.send_reports = value;
            }
        }
        SettingsPatch::Language(value) => settings.language = value,
        SettingsPatch::SoundEnabled(value) => settings.sound_enabled = value,
        SettingsPatch::MusicEnabled(value) => settings.music_enabled = value,
        SettingsPatch::MusicVolume(value) => settings.music_volume = value.min(100),
        SettingsPatch::SoundVolume(value) => settings.sound_volume = value,
        SettingsPatch::BackupDirectory(value) => settings.backup_directory = value,
        SettingsPatch::SendReports(value) => settings.send_reports = value,
        SettingsPatch::MetricsConsent(value) => settings.send_metrics = value,
        SettingsPatch::AddSaveDirectories(paths) => {
            let directories = settings.save_directories.get_or_insert_with(Vec::new);
            for path in paths {
                let key = directory_key(&path);
                if !directories.iter().any(|item| directory_key(item) == key) {
                    directories.push(path);
                }
            }
        }
    }
}

fn directory_key(path: &std::path::Path) -> String {
    path.to_string_lossy().trim().to_lowercase()
}

/// Queues one settings mutation and returns a receiver for its durable-write result.
#[must_use]
pub fn submit(patch: SettingsPatch) -> mpsc::Receiver<Result<()>> {
    let (done_tx, done_rx) = mpsc::channel();
    if sender().send(Request { patch, done: done_tx }).is_err() {
        let (fallback_tx, fallback_rx) = mpsc::channel();
        let _ = fallback_tx.send(Err(Error::Refused("settings writer stopped".to_owned())));
        return fallback_rx;
    }
    done_rx
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn patch_variants_are_cloneable() {
        let _ = SettingsPatch::Scale(125).clone();
    }

    #[test]
    fn field_patches_keep_changes_made_by_other_screens() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse_settings_fields_{unique}"));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("settings.json");
        AppSettings::default().save(&path).expect("write initial settings");

        let (tx, rx) = mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || run_writer(worker_path, rx));
        let send = |patch: SettingsPatch| {
            let (done_tx, done_rx) = mpsc::channel();
            tx.send(Request { patch, done: done_tx }).expect("send patch");
            done_rx.recv().expect("receive result").expect("patch writes settings");
        };

        // The setup wizard adds a folder; the settings screen, opened earlier, then changes the language.
        send(SettingsPatch::AddSaveDirectories(vec![PathBuf::from("saves-a")]));
        send(SettingsPatch::AddSaveDirectories(vec![PathBuf::from("SAVES-A ")]));
        send(SettingsPatch::Language(Some("en".to_owned())));
        send(SettingsPatch::SoundVolume(40));
        send(SettingsPatch::MetricsConsent(true));
        drop(tx);
        worker.join().expect("settings writer thread exits");

        let saved = AppSettings::load(&path).expect("load settings");
        assert_eq!(saved.language.as_deref(), Some("en"));
        assert_eq!(saved.sound_volume, 40);
        assert!(saved.send_metrics);
        assert_eq!(saved.save_directories, Some(vec![PathBuf::from("saves-a")]));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn unreadable_settings_are_copied_before_the_first_write() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse_settings_broken_{unique}"));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("settings.json");
        let original = b"{ damaged settings";
        fs::write(&path, original).expect("write damaged settings");

        let (tx, rx) = mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || run_writer(worker_path, rx));
        let send = |patch: SettingsPatch| {
            let (done_tx, done_rx) = mpsc::channel();
            tx.send(Request { patch, done: done_tx }).expect("send patch");
            done_rx.recv().expect("receive result")
        };
        send(SettingsPatch::Theme("new-theme".to_owned())).expect("patch writes settings");
        send(SettingsPatch::Language(Some("en".to_owned()))).expect("second patch writes settings");
        drop(tx);
        worker.join().expect("settings writer thread exits");

        let copies: Vec<PathBuf> = fs::read_dir(&directory)
            .expect("list test directory")
            .flatten()
            .map(|entry| entry.path())
            .filter(|candidate| candidate.to_string_lossy().contains("settings.json.broken-"))
            .collect();
        assert_eq!(copies.len(), 1, "one copy per broken file");
        let copy = copies.first().expect("copy exists");
        assert_eq!(fs::read(copy).expect("read copy"), original);
        let saved = AppSettings::load(&path).expect("settings file is valid after the write");
        assert_eq!(saved.theme_id, "new-theme");
        assert_eq!(saved.language.as_deref(), Some("en"));
        let _ = fs::remove_dir_all(directory);
    }
}
