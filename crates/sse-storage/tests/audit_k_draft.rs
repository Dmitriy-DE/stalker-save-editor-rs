#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]

use sse_storage::drafts::{DraftJournal, DraftPlan, DraftStore};
use std::fs;

#[test]
fn unreadable_existing_draft_is_not_treated_as_absent() {
    let root = std::env::temp_dir().join(format!("sse-audit-k-draft-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let sha = "a".repeat(64);
    let store = DraftStore::new(&root);
    let initial = DraftPlan::empty(&sha).unwrap();
    let mut edited = initial.clone();
    edited.money = Some(123);
    store
        .save(DraftJournal::new(vec![initial, edited], 1).unwrap())
        .unwrap();
    let path = store.path_for(&sha).unwrap();
    let original = fs::read(&path).unwrap();
    let preserved = root.join("preserved.json");
    fs::rename(&path, &preserved).unwrap();
    fs::create_dir(&path).unwrap();
    let result = store.load(&sha);
    assert!(result.is_err(), "an unreadable existing draft must stop the load");
    let replacement = DraftJournal::new(vec![DraftPlan::empty(&sha).unwrap()], 0).unwrap();
    assert!(
        store.save(replacement).is_err(),
        "saving must not replace an unreadable draft"
    );
    assert!(path.is_dir(), "the unreadable draft path must remain untouched");
    assert_eq!(fs::read(&preserved).unwrap(), original);
    fs::remove_dir_all(root).unwrap();
}
