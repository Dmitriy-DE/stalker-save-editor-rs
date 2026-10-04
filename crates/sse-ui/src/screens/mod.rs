//! Editor screens: the registry, the [`Screen`] contract and one file per work package.
//!
//! Each package owns one file and never edits the others:
//! `saves.rs` (S2), `history.rs` (S3), `games.rs` (S4), `services.rs` (S5), `app.rs` (S1, the shell's own screens).
//! A screen builds its widgets once, lazily, into the host panel the shell gives it, and reacts to messages.

use crate::event_loop::Message;
use crate::widget::{Tree, WidgetId};
use sse_core::Result;
use std::any::Any;

pub mod app;
pub mod games;
pub mod history;
pub mod saves;
pub mod services;
pub mod shell;
pub mod style;
mod wizard;

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

/// Sidebar group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    /// СОХРАНЕНИЯ.
    Saves,
    /// ИГРЫ.
    Games,
    /// ИНСТРУМЕНТЫ.
    Tools,
}

impl Group {
    /// Sidebar caption.
    #[must_use]
    pub const fn caption(self) -> &'static str {
        match self {
            Self::Saves => "СОХРАНЕНИЯ",
            Self::Games => "ИГРЫ",
            Self::Tools => "ИНСТРУМЕНТЫ",
        }
    }
}

impl ScreenId {
    /// All screens in sidebar order.
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

    /// Sidebar and header title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Overview => "ОБЗОР",
            Self::Inventory => "ИНВЕНТАРЬ",
            Self::Factions => "ФРАКЦИИ",
            Self::Stashes => "ТАЙНИКИ",
            Self::Transitions => "ПЕРЕХОДЫ",
            Self::Backups => "БЭКАПЫ",
            Self::Compare => "СРАВНЕНИЕ",
            Self::Timeline => "ИСТОРИЯ СОХРАНЕНИЙ",
            Self::SaveDoctor => "ДОКТОР СОХРАНЕНИЯ",
            Self::Games => "ОБЗОР ИГР",
            Self::GameFixes => "ИСПРАВЛЕНИЯ ИГРЫ",
            Self::GameDoctor => "ДОКТОР ИГРЫ",
            Self::Environment => "СРЕДА ИГРЫ",
            Self::Companion => "КОМПАНЬОН",
            Self::Achievements => "ДОСТИЖЕНИЯ",
            Self::Cloud => "ОБЛАКО",
            Self::Encyclopedia => "ЭНЦИКЛОПЕДИЯ",
            Self::Capabilities => "ВОЗМОЖНОСТИ",
            Self::Updates => "ОБНОВЛЕНИЯ",
            Self::Settings => "НАСТРОЙКИ",
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
            Self::Cloud | Self::Encyclopedia | Self::Capabilities | Self::Updates | Self::Settings => Group::Tools,
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
    /// Result of background work for one screen; the screen downcasts the payload it sent itself.
    ToScreen(ScreenId, Box<dyn Any + Send>),
    /// Global draft command from the shell, dispatched to the selected-save editor screen.
    EditorAction(EditorAction),
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
            Self::ToScreen(id, _) => write!(f, "ToScreen({id:?})"),
            Self::EditorAction(action) => write!(f, "EditorAction({action:?})"),
        }
    }
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
    let mut screens: Vec<Box<dyn Screen>> = Vec::new();
    let save_workspace = saves::Workspace::default();
    screens.extend(saves::screens_with_workspace(save_workspace.clone()));
    screens.extend(history::screens_with_workspace(save_workspace));
    screens.extend(games::screens());
    screens.extend(services::screens());
    screens.extend(app::screens());
    screens.sort_by_key(|screen| screen.id());
    screens
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
            &format!("Экран в работе ({})", self.id.package()),
            style::Text::Body,
        )?;
        Ok(())
    }
}
