//! Single-owner settings writer. All UI mutations are serialized in one worker; later patches win.

use crate::{default_settings_path, AppSettings};
use sse_core::{Error, Result};
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
    /// Mark the reports notice as seen and optionally change report sending.
    ReportsNotice {
        /// New report-sending value, or None to keep the current value.
        send_reports: Option<bool>,
    },
}

struct Request {
    patch: SettingsPatch,
    done: mpsc::Sender<Result<()>>,
}
static WRITER: OnceLock<mpsc::Sender<Request>> = OnceLock::new();

fn sender() -> &'static mpsc::Sender<Request> {
    WRITER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Request>();
        std::thread::Builder::new()
            .name("settings-writer".to_owned())
            .spawn(move || {
                let path = default_settings_path();
                let mut current = AppSettings::load(&path);
                while let Ok(request) = rx.recv() {
                    match request.patch {
                        SettingsPatch::Replace(value) => current = value,
                        SettingsPatch::Theme(value) => current.theme_id = value,
                        SettingsPatch::Accent(value) => current.accent_id = value,
                        SettingsPatch::Scale(value) => current.ui_scale_percent = value,
                        SettingsPatch::ReportsNotice { send_reports } => {
                            current.reports_notice_shown = true;
                            if let Some(value) = send_reports {
                                current.send_reports = value;
                            }
                        }
                    }
                    let _ = request.done.send(current.save(&path));
                }
            })
            .expect("settings writer thread");
        tx
    })
}

/// Queues one settings mutation and returns a receiver for its durable-write result.\n#[must_use]\npub fn submit(patch: SettingsPatch) -> mpsc::Receiver<Result<()>> {
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
    use super::*;
    #[test]
    fn patch_variants_are_cloneable() {
        let _ = SettingsPatch::Scale(125).clone();
    }
}
