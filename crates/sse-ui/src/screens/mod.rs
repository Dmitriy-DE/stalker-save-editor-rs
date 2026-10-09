//! Editor screens: the registry, the [`Screen`] contract and one file per work package.
//!
//! Each package owns one file and never edits the others:
//! `saves.rs` (S2), `history.rs` (S3), `games.rs` (S4), `services.rs` (S5), `app.rs` (S1, the shell's own screens).
//! A screen builds its widgets once, lazily, into the host panel the shell gives it, and reacts to messages.

use crate::event_loop::Message;
use crate::widget::{Tree, WidgetId};
use sse_core::{Error, Result, SaveBuffer};
use sse_storage::discovery::SaveDirectoryDiscoveryOptions;
use std::any::Any;
use std::sync::{Arc, Mutex};

/// A save copy prepared for download by the browser host.
pub struct BrowserDownload {
    /// Safe filename suggested to the browser.
    pub file_name: String,
    /// Verified save bytes.
    pub bytes: SaveBuffer,
}

#[derive(Default)]
struct BrowserFileState {
    open_requested: bool,
    download: Option<BrowserDownload>,
}

/// Transfers file-picker requests and edited save copies between the shared shell and browser host.
#[derive(Clone, Default)]
pub struct BrowserFileBridge {
    state: Arc<Mutex<BrowserFileState>>,
}

impl BrowserFileBridge {
    /// Creates an idle browser file-transfer bridge.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub(crate) fn request_open_file(&self) {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .open_requested = true;
    }

    /// Takes a pending request to show the browser's file picker.
    pub fn take_open_request(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut state.open_requested)
    }

    pub(crate) fn queue_download(&self, download: BrowserDownload) -> Result<()> {
        let mut state = self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.download.is_some() {
            return Err(Error::Refused("a browser download is already pending".to_owned()));
        }
        state.download = Some(download);
        Ok(())
    }

    /// Takes the next edited save copy that the browser should download.
    pub fn take_download(&self) -> Option<BrowserDownload> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .download
            .take()
    }
}

#[cfg(test)]
mod browser_file_bridge_tests {
    use super::{BrowserDownload, BrowserFileBridge};
    use sse_core::SaveBuffer;

    #[test]
    fn file_picker_requests_and_downloads_are_taken_once() -> sse_core::Result<()> {
        let bridge = BrowserFileBridge::new();
        assert!(!bridge.take_open_request());
        bridge.request_open_file();
        assert!(bridge.take_open_request());
        assert!(!bridge.take_open_request());

        bridge.queue_download(BrowserDownload {
            file_name: "copy.sav".to_owned(),
            bytes: SaveBuffer::from_vec(vec![1, 2, 3]),
        })?;
        let download = bridge
            .take_download()
            .ok_or_else(|| sse_core::Error::Refused("browser download was not queued".to_owned()))?;
        assert_eq!(download.file_name, "copy.sav");
        assert_eq!(download.bytes.as_slice(), [1, 2, 3]);
        assert!(bridge.take_download().is_none());
        Ok(())
    }
}

pub mod app;
#[cfg(feature = "native-ui")]
pub mod games;
pub mod history;
pub mod saves;
#[cfg(feature = "native-ui")]
pub mod services;
pub mod shell;
pub mod style;
mod wizard;

/// Discovery options shared by every save-library surface.
pub(crate) fn save_directory_discovery_options() -> SaveDirectoryDiscoveryOptions {
    let custom_save_directories = match sse_app::AppSettings::load(&sse_app::default_settings_path()) {
        Ok(settings) => settings.save_directories,
        Err(error) => {
            sse_app::diagnostics::warn(&format!(
                "settings file could not be loaded for save discovery: {error}"
            ));
            None
        }
    };
    SaveDirectoryDiscoveryOptions {
        custom_save_directories,
        ..SaveDirectoryDiscoveryOptions::default()
    }
}

#[cfg(test)]
pub(crate) fn task_registry_test_guard() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};

    // Background task names are process-global, so tests that inspect them must share this gate.
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Every screen of the editor, in sidebar order (same as the C# 1.3.1 sidebar).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ScreenId {
    /// Save overview.
    Overview,
    /// Inventory.
    Inventory,
    /// Factions.
    Factions,
    /// Stashes.
    Stashes,
    /// Level transitions.
    Transitions,
    /// Backups.
    Backups,
    /// Compare two saves.
    Compare,
    /// Save history.
    Timeline,
    /// Save doctor.
    SaveDoctor,
    /// Installed games.
    Games,
    /// Game fixes.
    GameFixes,
    /// Game doctor.
    GameDoctor,
    /// Game environment (toolkit).
    Environment,
    /// Companion mod.
    Companion,
    /// Achievements.
    Achievements,
    /// Steam Cloud.
    Cloud,
    /// Encyclopedia.
    Encyclopedia,
    /// Capabilities.
    Capabilities,
    /// Updates.
    Updates,
    /// Settings.
    Settings,
}

/// Sidebar section; each one opens its screens as tabs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// СОХРАНЕНИЯ.
    Saves,
    /// ИГРЫ.
    Games,
    /// ЭНЦИКЛОПЕДИЯ.
    Encyclopedia,
    /// НАСТРОЙКИ.
    Settings,
}

impl Group {
    /// All sections in sidebar order.
    pub const ALL: [Self; 4] = [Self::Saves, Self::Games, Self::Encyclopedia, Self::Settings];

    /// Sidebar caption.
    #[must_use]
    pub fn caption(self) -> &'static str {
        match self {
            Self::Saves => crate::strings::t("СОХРАНЕНИЯ"),
            Self::Games => crate::strings::t("ИГРЫ"),
            Self::Encyclopedia => crate::strings::t("ЭНЦИКЛОПЕДИЯ"),
            Self::Settings => crate::strings::t("НАСТРОЙКИ"),
        }
    }
}

impl ScreenId {
    /// All screens in sidebar order for a native build.
    #[cfg(feature = "native-ui")]
    pub const ALL: [Self; 20] = [
        Self::Overview,
        Self::Inventory,
        Self::Factions,
        Self::Stashes,
        Self::Transitions,
        Self::Backups,
        Self::Compare,
        Self::Timeline,
        Self::SaveDoctor,
        Self::Games,
        Self::GameFixes,
        Self::GameDoctor,
        Self::Environment,
        Self::Companion,
        Self::Achievements,
        Self::Cloud,
        Self::Encyclopedia,
        Self::Capabilities,
        Self::Updates,
        Self::Settings,
    ];

    /// Screens available in the browser build, which has no native game services.
    #[cfg(not(feature = "native-ui"))]
    pub const ALL: [Self; 11] = [
        Self::Overview,
        Self::Inventory,
        Self::Factions,
        Self::Stashes,
        Self::Transitions,
        Self::Backups,
        Self::Compare,
        Self::Timeline,
        Self::SaveDoctor,
        Self::Capabilities,
        Self::Settings,
    ];

    /// Sidebar and header title.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Overview => crate::strings::t("ОБЗОР"),
            Self::Inventory => crate::strings::t("ИНВЕНТАРЬ"),
            Self::Factions => crate::strings::t("ФРАКЦИИ"),
            Self::Stashes => crate::strings::t("ТАЙНИКИ"),
            Self::Transitions => crate::strings::t("ПЕРЕХОДЫ"),
            Self::Backups => crate::strings::t("БЭКАПЫ"),
            Self::Compare => crate::strings::t("СРАВНЕНИЕ"),
            Self::Timeline => crate::strings::t("ИСТОРИЯ СОХРАНЕНИЙ"),
            Self::SaveDoctor => crate::strings::t("ДОКТОР СОХРАНЕНИЯ"),
            Self::Games => crate::strings::t("ОБЗОР ИГР"),
            Self::GameFixes => crate::strings::t("ИСПРАВЛЕНИЯ ИГРЫ"),
            Self::GameDoctor => crate::strings::t("ДОКТОР ИГРЫ"),
            Self::Environment => crate::strings::t("СРЕДА ИГРЫ"),
            Self::Companion => crate::strings::t("КОМПАНЬОН"),
            Self::Achievements => crate::strings::t("ДОСТИЖЕНИЯ"),
            Self::Cloud => crate::strings::t("ОБЛАКО"),
            Self::Encyclopedia => crate::strings::t("ЭНЦИКЛОПЕДИЯ"),
            Self::Capabilities => crate::strings::t("ВОЗМОЖНОСТИ"),
            Self::Updates => crate::strings::t("ОБНОВЛЕНИЯ"),
            Self::Settings => crate::strings::t("НАСТРОЙКИ"),
        }
    }

    /// Sidebar group.
    #[must_use]
    pub const fn group(self) -> Group {
        match self {
            Self::Overview
            | Self::Inventory
            | Self::Factions
            | Self::Stashes
            | Self::Transitions
            | Self::Backups
            | Self::Compare
            | Self::Timeline
            | Self::SaveDoctor => Group::Saves,
            Self::Games
            | Self::GameFixes
            | Self::GameDoctor
            | Self::Environment
            | Self::Companion
            | Self::Achievements => Group::Games,
            Self::Cloud => Group::Saves,
            Self::Encyclopedia => Group::Encyclopedia,
            Self::Capabilities | Self::Updates | Self::Settings => Group::Settings,
        }
    }

    /// Work package that owns the screen (file in this directory).
    #[must_use]
    pub const fn package(self) -> &'static str {
        match self {
            Self::Overview | Self::Inventory | Self::Factions | Self::Stashes | Self::Transitions => "S2",
            Self::Backups | Self::Compare | Self::Timeline | Self::SaveDoctor => "S3",
            Self::Games | Self::GameFixes | Self::GameDoctor | Self::Environment | Self::Encyclopedia => "S4",
            Self::Companion | Self::Achievements | Self::Cloud | Self::Updates => "S5",
            Self::Capabilities | Self::Settings => "S1",
        }
    }
}

/// Messages of the editor event loop.
pub enum AppMessage {
    /// Seconds since start, once per second (status line, caret blink, relative times).
    Tick(u64),
    /// Ask the shell to open its shared save picker and return after loading.
    OpenSavePicker {
        /// Screen to show after the selected save finishes loading.
        return_to: ScreenId,
    },
    /// Open the backup history screen after startup detects an interrupted save write.
    OpenBackups,
    /// Open a registered application screen from an in-screen action.
    OpenScreen(ScreenId),
    /// Open the linked game fix in the Game Fixes screen without installing it.
    OpenGameFix {
        /// Canonical game identifier from the fix catalog.
        game_id: String,
        /// Identifier of the catalog fix to select.
        fix_id: String,
    },
    /// Result of background work for one screen; the screen downcasts the payload it sent itself.
    ToScreen(ScreenId, Box<dyn Any + Send>),
    /// Global draft command from the shell, dispatched to the selected-save editor screen.
    EditorAction(EditorAction),
    /// UI sound clips decoded from the selected game's files by a worker.
    SoundLoaded(String, Box<crate::sound::GameUiSounds>),
    /// Durable settings-write result delivered by the settings writer.
    SettingsWriteFinished(std::result::Result<(), String>),
}

/// Commands available from the shared editor toolbar and keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    /// Revert to the preceding draft snapshot.
    Undo,
    /// Reapply the next draft snapshot.
    Redo,
    /// Discard edits and restore the source save values.
    Reset,
    /// Validate and write the current draft with a backup.
    Save,
}

impl std::fmt::Debug for AppMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tick(seconds) => write!(f, "Tick({seconds})"),
            Self::OpenSavePicker { return_to } => write!(f, "OpenSavePicker({return_to:?})"),
            Self::OpenBackups => f.write_str("OpenBackups"),
            Self::OpenScreen(id) => write!(f, "OpenScreen({id:?})"),
            Self::OpenGameFix { game_id, fix_id } => write!(f, "OpenGameFix({game_id}, {fix_id})"),
            Self::ToScreen(id, _) => write!(f, "ToScreen({id:?})"),
            Self::EditorAction(action) => write!(f, "EditorAction({action:?})"),
            Self::SoundLoaded(game, _) => write!(f, "SoundLoaded({game})"),
            Self::SettingsWriteFinished(result) => write!(f, "SettingsWriteFinished({})", result.is_ok()),
        }
    }
}

/// Submits a settings mutation and reports its durable-write result without blocking the UI when a proxy exists.
pub(crate) fn submit_settings_write(
    patch: sse_app::settings_writer::SettingsPatch,
    proxy: Option<crate::event_loop::Proxy<AppMessage>>,
) -> Result<()> {
    let result = sse_app::settings_writer::submit(patch);
    let Some(proxy) = proxy else {
        return result
            .recv()
            .map_err(|error| sse_core::Error::Refused(format!("settings writer stopped: {error}")))?;
    };

    std::thread::Builder::new()
        .name("settings-write-result".to_owned())
        .spawn(move || {
            let result = match result.recv() {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(format!("settings writer stopped: {error}")),
            };
            if let Err(error) = &result {
                sse_app::diagnostics::error(&format!("settings write failed: {error}"));
            }
            let _ = proxy.send(AppMessage::SettingsWriteFinished(result));
        })
        .map_err(|error| sse_core::Error::Refused(format!("could not start settings result worker: {error}")))?;
    Ok(())
}

/// Everything a screen may touch while handling a message.
pub struct Context<'a> {
    /// Widget tree.
    pub tree: &'a mut Tree,
    /// Sender for background work: `proxy.send(AppMessage::ToScreen(id, Box::new(result)))` from a worker thread.
    pub proxy: Option<&'a crate::event_loop::Proxy<AppMessage>>,
    /// Status line text to show, if the screen wants to say something.
    pub status: Option<String>,
    /// Shared application state: selected game and its install directory, current save, recent saves, drafts.
    pub app: &'a mut sse_app::state::AppState,
}

/// One editor screen.
pub trait Screen {
    /// Which screen this is.
    fn id(&self) -> ScreenId;

    /// One-line description under the title.
    fn subtitle(&self) -> &str;

    /// Builds the widgets into `host` (a stretched column). Called once, the first time the screen is shown.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()>;

    /// The screen became visible (refresh data here).
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    fn shown(&mut self, _cx: &mut Context<'_>) -> Result<()> {
        Ok(())
    }

    /// Opens a save supplied by a developer-only headless workflow, when supported.
    ///
    /// # Errors
    /// Returns an error when the requested file cannot be read or parsed.
    fn open_save(&mut self, _cx: &mut Context<'_>, _path: &std::path::Path) -> Result<bool> {
        Ok(false)
    }

    /// Opens a save selected by the browser without writing it to a local path.
    ///
    /// # Errors
    /// Returns an error when the imported bytes are rejected or cannot be parsed.
    fn open_browser_file(
        &mut self,
        _cx: &mut Context<'_>,
        _file_name: &str,
        _bytes: Vec<u8>,
        _last_modified_ms: u64,
    ) -> Result<bool> {
        Ok(false)
    }

    /// A message while the screen is built. `clicked` is a widget clicked anywhere; ignore what is not yours.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    fn message(
        &mut self,
        _cx: &mut Context<'_>,
        _message: &Message<AppMessage>,
        _clicked: Option<WidgetId>,
    ) -> Result<()> {
        Ok(())
    }
}

/// All screens, in [`ScreenId::ALL`] order.
#[must_use]
pub fn registry() -> Vec<Box<dyn Screen>> {
    registry_with_save_workspace(saves::Workspace::default())
}

pub(crate) fn registry_with_save_workspace(save_workspace: saves::Workspace) -> Vec<Box<dyn Screen>> {
    let mut screens: Vec<Box<dyn Screen>> = Vec::new();
    screens.extend(saves::screens_with_workspace(save_workspace.clone()));
    screens.extend(history::screens_with_workspace(save_workspace.clone()));
    #[cfg(feature = "native-ui")]
    screens.extend(games::screens());
    #[cfg(feature = "native-ui")]
    screens.extend(services::screens(save_workspace.clone()));
    screens.extend(app::screens_with_workspace(save_workspace));
    screens.sort_by_key(|screen| screen.id());
    screens
}

#[cfg(all(test, not(feature = "native-ui")))]
mod browser_registry_tests {
    use super::{registry, ScreenId};

    #[test]
    fn browser_registry_excludes_native_only_screens() {
        let ids = registry().into_iter().map(|screen| screen.id()).collect::<Vec<_>>();
        assert_eq!(ids.len(), ScreenId::ALL.len());
        assert!(ScreenId::ALL.iter().all(|id| ids.contains(id)));
        assert!(ids.iter().all(|id| {
            !matches!(
                *id,
                ScreenId::Games
                    | ScreenId::GameFixes
                    | ScreenId::GameDoctor
                    | ScreenId::Environment
                    | ScreenId::Companion
                    | ScreenId::Achievements
                    | ScreenId::Cloud
                    | ScreenId::Encyclopedia
                    | ScreenId::Updates
            )
        }));
    }
}

/// A screen that only says which package will fill it.
pub struct Placeholder {
    id: ScreenId,
    subtitle: &'static str,
}

impl Placeholder {
    /// Creates a placeholder.
    #[must_use]
    pub const fn new(id: ScreenId, subtitle: &'static str) -> Self {
        Self { id, subtitle }
    }
}

impl Screen for Placeholder {
    fn id(&self) -> ScreenId {
        self.id
    }

    fn subtitle(&self) -> &str {
        self.subtitle
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            card,
            &crate::strings::tr_in(
                Some(crate::strings::current_language()),
                "Экран в работе ({0})",
                &[&self.id.package()],
            ),
            style::Text::Body,
        )?;
        Ok(())
    }
}
