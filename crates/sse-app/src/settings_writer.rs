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
    /// Set the sound volume in percent.
    SoundVolume(u32),
    /// Set the backup directory override; `None` restores the default.
    BackupDirectory(Option<PathBuf>),
    /// Set whether crash and telemetry reports may be sent.
    SendReports(bool),
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
    let mut current = match AppSettings::load(&path) {
        Ok(settings) => Some(settings),
        Err(error) => {
            crate::diagnostics::warn(&format!("settings file could not be loaded: {error}"));
            None
        }
    };

    while let Ok(request) = rx.recv() {
        let next = match request.patch {
            SettingsPatch::Replace(settings) => Ok(settings),
            patch => match current.as_ref() {
                Some(settings) => {
                    let mut updated = settings.clone();
                    apply_patch(&mut updated, patch);
                    Ok(updated)
                }
                None => Err(Error::Refused(
                    "settings file is unreadable; an explicit replacement is required before applying patches"
                        .to_owned(),
                )),
            },
        };

        let result = match next {
            Ok(updated) => match updated.save(&path) {
                Ok(()) => {
                    current = Some(updated);
                    Ok(())
                }
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        let _ = request.done.send(result);
    }
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
        SettingsPatch::SoundVolume(value) => settings.sound_volume = value,
        SettingsPatch::BackupDirectory(value) => settings.backup_directory = value,
        SettingsPatch::SendReports(value) => settings.send_reports = value,
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
        drop(tx);
        worker.join().expect("settings writer thread exits");

        let saved = AppSettings::load(&path).expect("load settings");
        assert_eq!(saved.language.as_deref(), Some("en"));
        assert_eq!(saved.sound_volume, 40);
        assert_eq!(saved.save_directories, Some(vec![PathBuf::from("saves-a")]));
        let _ = fs::remove_dir_all(directory);
    }

    #[test]
    fn corrupt_settings_are_preserved_until_explicit_replacement() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse_settings_writer_{unique}"));
        fs::create_dir_all(&directory).expect("create test directory");
        let path = directory.join("settings.json");
        let original = b"{ damaged settings";
        fs::write(&path, original).expect("write damaged settings");

        let (tx, rx) = mpsc::channel();
        let worker_path = path.clone();
        let worker = std::thread::spawn(move || run_writer(worker_path, rx));

        let (done_tx, done_rx) = mpsc::channel();
        tx.send(Request {
            patch: SettingsPatch::Theme("new-theme".to_owned()),
            done: done_tx,
        })
        .expect("send incremental patch");
        assert!(done_rx.recv().expect("receive patch result").is_err());
        assert_eq!(fs::read(&path).expect("read damaged settings"), original);

        let replacement = AppSettings {
            theme_id: "replacement".to_owned(),
            ..AppSettings::default()
        };
        let (done_tx, done_rx) = mpsc::channel();
        tx.send(Request {
            patch: SettingsPatch::Replace(replacement.clone()),
            done: done_tx,
        })
        .expect("send explicit replacement");
        done_rx
            .recv()
            .expect("receive replacement result")
            .expect("explicit replacement writes settings");

        drop(tx);
        worker.join().expect("settings writer thread exits");
        assert_eq!(AppSettings::load(&path).expect("load replacement"), replacement);
        let _ = fs::remove_dir_all(directory);
    }
}
