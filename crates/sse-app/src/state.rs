//! Pure application state model with change notifications.
//!
//! Tracks:
//! - Selected game / release
//! - Active save file path
//! - Active screen ID
//! - Recent saves list (bounded, deduplicated, most-recent first)
//! - Draft edit tracking (per save SHA-256)
//!
//! Emits `AppEvent` notifications when state mutations occur, allowing UI or other
//! observers to stay synchronized without tight coupling or dependencies on `sse-ui`.

use sse_core::Result;
use sse_storage::drafts::{DraftJournal, DraftPlan};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};

/// Maximum number of recent saves kept in history.
pub const MAX_RECENT_SAVES: usize = 20;

/// Events broadcast when application state changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// Selected game family or release changed.
    SelectedGameChanged(Option<String>),
    /// Active save path changed.
    CurrentSaveChanged(Option<PathBuf>),
    /// Current screen changed.
    ActiveScreenChanged(String),
    /// Recent saves list changed.
    RecentSavesChanged(Vec<PathBuf>),
    /// A draft was modified or saved for a save with the given SHA-256.
    DraftChanged {
        /// Lowercase SHA-256 of the source save.
        source_sha256: String,
        /// Whether the draft currently contains unsaved modifications.
        has_changes: bool,
    },
    /// A draft was cleared or reset.
    DraftDiscarded {
        /// Lowercase SHA-256 of the source save.
        source_sha256: String,
    },
}

/// Read-only snapshot of the current application state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStateSnapshot {
    /// Currently selected game identifier (e.g. "stalker-cop", "stalker2").
    pub selected_game: Option<String>,
    /// Currently loaded save file path.
    pub current_save: Option<PathBuf>,
    /// Currently displayed screen identifier (e.g. "overview", "settings").
    pub active_screen: String,
    /// Recent saves list, ordered from most recent to oldest.
    pub recent_saves: Vec<PathBuf>,
    /// Set of save SHA-256 hashes that currently have active drafts.
    pub active_draft_hashes: Vec<String>,
}

/// Central application state container.
pub struct AppState {
    selected_game: Option<String>,
    game_dir: Option<PathBuf>,
    current_save: Option<PathBuf>,
    current_save_sha256: Option<String>,
    invalid_numeric_input: bool,
    active_screen: String,
    recent_saves: Vec<PathBuf>,
    drafts: HashMap<String, DraftJournal>,
    event_sender: Sender<AppEvent>,
    event_receiver: Receiver<AppEvent>,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

impl AppState {
    /// Creates a new default application state.
    #[must_use]
    pub fn new() -> Self {
        let (event_sender, event_receiver) = mpsc::channel();
        Self {
            selected_game: None,
            game_dir: None,
            current_save: None,
            current_save_sha256: None,
            invalid_numeric_input: false,
            active_screen: "overview".to_owned(),
            recent_saves: Vec::new(),
            drafts: HashMap::new(),
            event_sender,
            event_receiver,
        }
    }

    /// Returns a subscriber channel sender for external listeners if needed.
    #[must_use]
    pub fn event_sender(&self) -> Sender<AppEvent> {
        self.event_sender.clone()
    }

    /// Polls pending state events without blocking.
    #[must_use]
    pub fn poll_events(&self) -> Vec<AppEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.event_receiver.try_recv() {
            events.push(ev);
        }
        events
    }

    /// Takes a read-only snapshot of the state.
    #[must_use]
    pub fn snapshot(&self) -> AppStateSnapshot {
        let mut draft_hashes: Vec<String> = self.drafts.keys().cloned().collect();
        draft_hashes.sort();
        AppStateSnapshot {
            selected_game: self.selected_game.clone(),
            current_save: self.current_save.clone(),
            active_screen: self.active_screen.clone(),
            recent_saves: self.recent_saves.clone(),
            active_draft_hashes: draft_hashes,
        }
    }

    /// Returns the currently selected game.
    #[must_use]
    pub fn selected_game(&self) -> Option<&str> {
        self.selected_game.as_deref()
    }

    /// Sets the selected game identifier, emitting `SelectedGameChanged` if different.
    pub fn set_selected_game(&mut self, game: Option<String>) {
        if self.selected_game != game {
            self.selected_game = game.clone();
            let _ = self.event_sender.send(AppEvent::SelectedGameChanged(game));
        }
    }

    /// Install directory of the selected game, set by the games overview.
    #[must_use]
    pub fn game_dir(&self) -> Option<&Path> {
        self.game_dir.as_deref()
    }

    /// Sets the install directory of the selected game.
    pub fn set_game_dir(&mut self, dir: Option<PathBuf>) {
        self.game_dir = dir;
    }

    /// Returns the current save path.
    #[must_use]
    pub fn current_save(&self) -> Option<&Path> {
        self.current_save.as_deref()
    }

    /// SHA-256 of the bytes currently loaded from `current_save`.
    #[must_use]
    pub fn current_save_sha256(&self) -> Option<&str> {
        self.current_save_sha256.as_deref()
    }

    /// Whether an active numeric editor contains a value that cannot be saved.
    #[must_use]
    pub const fn has_invalid_numeric_input(&self) -> bool {
        self.invalid_numeric_input
    }

    /// Updates the validation state of the active numeric editor.
    pub fn set_invalid_numeric_input(&mut self, invalid: bool) {
        self.invalid_numeric_input = invalid;
    }

    /// Sets the active save file path, updating recent saves and emitting `CurrentSaveChanged`.
    pub fn set_current_save(&mut self, save_path: Option<PathBuf>) {
        if self.current_save != save_path {
            self.current_save = save_path.clone();
            self.current_save_sha256 = None;
            self.invalid_numeric_input = false;
            if let Some(ref path) = save_path {
                self.record_recent_save(path.clone());
            }
            let _ = self.event_sender.send(AppEvent::CurrentSaveChanged(save_path));
        }
    }

    /// Sets the current save path and the source bytes' SHA-256 as one loaded-save identity.
    pub fn set_current_save_identity(&mut self, save_path: PathBuf, source_sha256: String) {
        self.set_current_save(Some(save_path));
        self.current_save_sha256 = Some(source_sha256);
        self.invalid_numeric_input = false;
    }

    /// Returns the active screen identifier.
    #[must_use]
    pub fn active_screen(&self) -> &str {
        &self.active_screen
    }

    /// Changes the active screen, emitting `ActiveScreenChanged` if different.
    pub fn set_active_screen(&mut self, screen: impl Into<String>) {
        let screen = screen.into();
        if self.active_screen != screen {
            self.active_screen = screen.clone();
            let _ = self.event_sender.send(AppEvent::ActiveScreenChanged(screen));
        }
    }

    /// Returns the list of recent save file paths.
    #[must_use]
    pub fn recent_saves(&self) -> &[PathBuf] {
        &self.recent_saves
    }

    /// Adds a save path to the recent list (most recent first, deduplicated, capped at `MAX_RECENT_SAVES`).
    pub fn record_recent_save(&mut self, path: PathBuf) {
        self.recent_saves.retain(|p| p != &path);
        self.recent_saves.insert(0, path);
        if self.recent_saves.len() > MAX_RECENT_SAVES {
            self.recent_saves.truncate(MAX_RECENT_SAVES);
        }
        let _ = self
            .event_sender
            .send(AppEvent::RecentSavesChanged(self.recent_saves.clone()));
    }

    /// Clears the recent saves list.
    pub fn clear_recent_saves(&mut self) {
        if !self.recent_saves.is_empty() {
            self.recent_saves.clear();
            let _ = self.event_sender.send(AppEvent::RecentSavesChanged(Vec::new()));
        }
    }

    /// Checks if a draft exists for the given save SHA-256.
    #[must_use]
    pub fn has_draft(&self, source_sha256: &str) -> bool {
        self.drafts
            .get(source_sha256)
            .and_then(DraftJournal::current)
            .is_some_and(plan_has_changes)
    }

    /// Returns a reference to the active draft plan for a save, if any.
    #[must_use]
    pub fn draft(&self, source_sha256: &str) -> Option<&DraftPlan> {
        self.drafts.get(source_sha256).and_then(DraftJournal::current)
    }

    /// Returns the complete undo/redo journal for one source save.
    #[must_use]
    pub fn draft_journal(&self, source_sha256: &str) -> Option<&DraftJournal> {
        self.drafts.get(source_sha256)
    }

    /// Updates or replaces the draft plan for a save, emitting `DraftChanged`.
    pub fn set_draft(&mut self, plan: DraftPlan) {
        let journal = DraftJournal::new(vec![plan], 0);
        if let Ok(journal) = journal {
            self.set_draft_journal(journal);
        }
    }

    /// Replaces the in-memory undo journal for one save.
    pub fn set_draft_journal(&mut self, journal: DraftJournal) {
        let Some(plan) = journal.current() else {
            return;
        };
        let source_sha256 = plan.source_sha256.clone();
        let has_changes = plan_has_changes(plan);
        self.drafts.insert(source_sha256.clone(), journal);
        let _ = self.event_sender.send(AppEvent::DraftChanged {
            source_sha256,
            has_changes,
        });
    }

    /// Records a new plan in the bounded undo journal for its source save.
    pub fn record_draft(&mut self, plan: DraftPlan) -> Result<()> {
        let source_sha256 = plan.source_sha256.clone();
        let journal = if let Some(journal) = self.drafts.get(&source_sha256).cloned() {
            journal.record(plan, false)?
        } else {
            DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?.record(plan, false)?
        };
        self.set_draft_journal(journal);
        Ok(())
    }

    /// Whether an earlier draft snapshot exists for this source save.
    #[must_use]
    pub fn can_undo_draft(&self, source_sha256: &str) -> bool {
        self.drafts.get(source_sha256).is_some_and(DraftJournal::can_undo)
    }

    /// Whether a later draft snapshot exists for this source save.
    #[must_use]
    pub fn can_redo_draft(&self, source_sha256: &str) -> bool {
        self.drafts.get(source_sha256).is_some_and(DraftJournal::can_redo)
    }

    /// Moves to the preceding plan snapshot, emitting `DraftChanged`.
    pub fn undo_draft(&mut self, source_sha256: &str) -> Result<()> {
        if let Some(journal) = self.drafts.get(source_sha256).cloned() {
            self.set_draft_journal(journal.undo());
        }
        Ok(())
    }

    /// Moves to the next plan snapshot, emitting `DraftChanged`.
    pub fn redo_draft(&mut self, source_sha256: &str) -> Result<()> {
        if let Some(journal) = self.drafts.get(source_sha256).cloned() {
            self.set_draft_journal(journal.redo());
        }
        Ok(())
    }

    /// Removes a draft for the given save SHA-256, emitting `DraftDiscarded`.
    pub fn discard_draft(&mut self, source_sha256: &str) -> Option<DraftPlan> {
        let removed = self.drafts.remove(source_sha256);
        if let Some(plan) = removed.as_ref().and_then(DraftJournal::current) {
            let _ = self.event_sender.send(AppEvent::DraftDiscarded {
                source_sha256: source_sha256.to_owned(),
            });
            return Some(plan.clone());
        }
        None
    }
}

fn plan_has_changes(plan: &DraftPlan) -> bool {
    plan.money.is_some()
        || !plan.stack_counts.is_empty()
        || !plan.durability.is_empty()
        || !plan.placements.is_empty()
        || !plan.upgrades.is_empty()
        || !plan.detach_handles.is_empty()
        || !plan.adds.is_empty()
        || !plan.stash_takes.is_empty()
        || !plan.stash_puts.is_empty()
        || plan.unmapped_legacy_plan.is_some()
}
