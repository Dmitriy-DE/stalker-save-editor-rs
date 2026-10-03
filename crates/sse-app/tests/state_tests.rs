//! Tests for AppState mutations, draft tracking, and AppEvent emission.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_app::state::{AppEvent, AppState, MAX_RECENT_SAVES};
use sse_storage::drafts::DraftPlan;
use std::path::PathBuf;

#[test]
fn app_state_initial_defaults() {
    let state = AppState::new();
    let snapshot = state.snapshot();

    assert_eq!(snapshot.selected_game, None);
    assert_eq!(snapshot.current_save, None);
    assert_eq!(snapshot.active_screen, "overview");
    assert!(snapshot.recent_saves.is_empty());
    assert!(snapshot.active_draft_hashes.is_empty());
}

#[test]
fn app_state_mutations_emit_events() {
    let mut state = AppState::new();

    state.set_selected_game(Some("stalker-cop".to_owned()));
    state.set_active_screen("inventory");
    state.set_current_save(Some(PathBuf::from("/saves/test.sav")));

    let events = state.poll_events();
    assert_eq!(events.len(), 4); // selected_game, active_screen, recent_saves (from current_save), current_save

    assert!(events.contains(&AppEvent::SelectedGameChanged(Some("stalker-cop".to_owned()))));
    assert!(events.contains(&AppEvent::ActiveScreenChanged("inventory".to_owned())));
    assert!(events.contains(&AppEvent::CurrentSaveChanged(Some(PathBuf::from("/saves/test.sav")))));
}

#[test]
fn recent_saves_capped_and_deduplicated() {
    let mut state = AppState::new();

    for i in 0..25 {
        state.record_recent_save(PathBuf::from(format!("/saves/save_{i}.sav")));
    }

    assert_eq!(state.recent_saves().len(), MAX_RECENT_SAVES);
    // Most recent should be at the front
    assert_eq!(state.recent_saves()[0], PathBuf::from("/saves/save_24.sav"));

    // Deduplication moves to front
    state.record_recent_save(PathBuf::from("/saves/save_10.sav"));
    assert_eq!(state.recent_saves().len(), MAX_RECENT_SAVES);
    assert_eq!(state.recent_saves()[0], PathBuf::from("/saves/save_10.sav"));
}

#[test]
fn draft_tracking_and_events() {
    let mut state = AppState::new();
    let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    let empty_plan = DraftPlan::empty(sha256).expect("empty plan");
    state.set_draft(empty_plan);

    assert!(state.has_draft(sha256));
    let events = state.poll_events();
    assert_eq!(
        events,
        vec![AppEvent::DraftChanged {
            source_sha256: sha256.to_owned(),
            has_changes: false,
        }]
    );

    let discarded = state.discard_draft(sha256);
    assert!(discarded.is_some());
    assert!(!state.has_draft(sha256));

    let events = state.poll_events();
    assert_eq!(
        events,
        vec![AppEvent::DraftDiscarded {
            source_sha256: sha256.to_owned(),
        }]
    );
}
