//! C#-compatible draft history and persistence regressions.

#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::indexing_slicing)]

use sse_storage::drafts::{AddRequest, DraftJournal, DraftPlacement, DraftPlan, DraftStore, StashPut};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-draft-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("temporary directory should be created");
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn current(journal: &DraftJournal) -> &DraftPlan {
    journal.current().expect("journal should have a current plan")
}

#[test]
fn reads_the_python_schema_one_fixture_and_preserves_its_edits() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0bb85823656a0280d6add2df2d0c7bdc80bc064eb9344eaafc3b5a73804cc3a0";
    fs::write(
        store.path_for(source_sha256).expect("valid source hash"),
        include_bytes!("../../../fixtures/synthetic/drafts/python-v1-draft.json"),
    )
    .expect("fixture should be copied into the isolated draft directory");

    let journal = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft should load");

    assert_eq!(journal.index(), 2);
    assert_eq!(journal.plans().len(), 3);
    assert_eq!(current(&journal).money, Some(900_000));
    assert_eq!(current(&journal).stack_counts.get(&0x1234), Some(&44));
    assert_eq!(current(&journal).detach_handles, [0x2345]);
    assert_eq!(
        current(&journal).adds,
        [AddRequest::new("wpn_test", 2, "inventory").expect("valid add")]
    );
    assert!(journal.can_apply_current());
}

#[test]
fn basic_edits_use_the_schema_two_shape_readable_by_the_reference() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid plan");
    plan.money = Some(12_345);
    plan.stack_counts.insert(0x1234, 8);
    let journal = DraftJournal::new(vec![plan], 0).expect("valid journal");

    store.save(journal).expect("draft should save");
    let bytes = fs::read(store.path_for(source_sha256).expect("valid hash")).expect("draft should exist");
    let serialized = std::str::from_utf8(&bytes).expect("draft JSON should be UTF-8");

    assert!(serialized.contains("\"schema\":2"));
    assert!(!serialized.contains("\"durability\""));
    assert!(!serialized.contains("\"placements\""));
    assert!(!serialized.contains("\"upgrades\""));
    assert!(!serialized.contains("\"s2StashTakes\""));
    assert!(store.load(source_sha256).expect("draft read should succeed").is_some());
}

#[cfg(unix)]
#[test]
fn persisted_draft_permissions_are_restricted_at_creation() {
    use std::os::unix::fs::PermissionsExt;

    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid plan");
    plan.money = Some(5);
    store
        .save(DraftJournal::new(vec![plan], 0).expect("valid journal"))
        .expect("draft should persist");

    let path = store.path_for(source_sha256).expect("valid source hash");
    assert_eq!(
        fs::metadata(path).expect("draft metadata").permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn schema_three_roundtrips_all_edits_and_supports_undo_redo_and_branching() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let empty = DraftPlan::empty(source_sha256).expect("valid empty plan");
    let mut first = DraftPlan::empty(source_sha256).expect("valid plan");
    first.money = Some(100);
    let mut second = DraftPlan::empty(source_sha256).expect("valid plan");
    second.money = Some(200);
    second.stack_counts.insert(0x1234, 8);
    second.detach_handles.push(0x2345);
    second
        .adds
        .push(AddRequest::new("wpn_test", 2, "inventory").expect("valid add"));
    second.stash_takes.push(0x3456);
    second.s2_stash_takes.push(0x789a_bcde);
    second
        .stash_puts
        .push(StashPut::new(0x4567, 0x5678).expect("valid stash transfer"));
    second.durability.insert(0x6789, 75);
    second.placements.insert(0x6789, DraftPlacement::Belt);
    second.upgrades.insert(0x6789, vec!["wpn_upgrade_scope_1".to_owned()]);
    let journal = DraftJournal::new(vec![empty, first, second], 2).expect("valid journal");

    let saved = store.save(journal).expect("draft should save");
    let bytes = fs::read(store.path_for(source_sha256).expect("valid hash")).expect("draft file should exist");
    let serialized = std::str::from_utf8(&bytes).expect("draft JSON should be UTF-8");
    assert!(serialized.contains("\"schema\":3"));
    assert!(serialized.contains("\"s2StashTakes\":[2023406814]"));
    assert!(serialized.contains("\"source_sha256\""));
    assert!(serialized.contains("\"sourceSha256\""));
    assert_eq!(current(&saved).money, Some(200));
    assert_eq!(current(&saved).durability.get(&0x6789), Some(&75));
    assert_eq!(current(&saved).placements.get(&0x6789), Some(&DraftPlacement::Belt));
    assert_eq!(
        current(&saved).upgrades.get(&0x6789),
        Some(&vec!["wpn_upgrade_scope_1".to_owned()])
    );

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft should load");
    assert_eq!(current(&restored), current(&saved));
    assert_eq!(current(&restored).s2_stash_takes, [0x789a_bcde]);
    assert_eq!(current(&restored.undo()).money, Some(100));
    assert_eq!(current(&restored.undo().redo()), current(&restored));
    assert_eq!(current(&restored).durability.get(&0x6789), Some(&75));
    assert_eq!(current(&restored).placements.get(&0x6789), Some(&DraftPlacement::Belt));
    assert_eq!(
        current(&restored).upgrades.get(&0x6789),
        Some(&vec!["wpn_upgrade_scope_1".to_owned()])
    );
    let mut branch_plan = DraftPlan::empty(source_sha256).expect("valid plan with money 300");
    branch_plan.money = Some(300);
    let branched = restored
        .undo()
        .record(branch_plan, false)
        .expect("branch should be recorded");
    assert_eq!(current(&branched).money, Some(300));
    assert!(!branched.can_redo());
}

#[test]
fn schema_three_roundtrips_s2_stash_handles_without_truncation() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid empty plan");
    plan.s2_stash_takes.push(0x1234_5678);
    let journal = DraftJournal::new(vec![plan], 0).expect("valid S2 transfer draft");

    let saved = store.save(journal).expect("S2 transfer draft should persist");
    let bytes = fs::read(store.path_for(source_sha256).expect("valid hash")).expect("draft should exist");
    let serialized = std::str::from_utf8(&bytes).expect("draft JSON should be UTF-8");
    assert!(serialized.contains("\"schema\":3"));
    assert!(serialized.contains("\"s2StashTakes\":[305419896]"));

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("S2 transfer draft should load");
    assert_eq!(current(&saved).s2_stash_takes, [0x1234_5678]);
    assert_eq!(current(&restored).s2_stash_takes, [0x1234_5678]);
    assert_eq!(current(&restored), current(&saved));
}

#[test]
fn schema_five_roundtrips_faction_relation_and_relocation_edits() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid plan");
    plan.faction_relations.insert("freedom".to_owned(), 1_000);
    plan.relocate_to = Some(0x1234);
    let journal = DraftJournal::new(vec![plan], 0).expect("valid journal");

    store.save(journal).expect("faction relation draft should save");
    let bytes = fs::read(store.path_for(source_sha256).expect("valid hash")).expect("draft should exist");
    let serialized = std::str::from_utf8(&bytes).expect("draft JSON should be UTF-8");
    assert!(serialized.contains("\"schema\":5"));
    assert!(serialized.contains("\"factionRelations\":{\"freedom\":1000}"));
    assert!(serialized.contains("\"relocateTo\":4660"));

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft should load");
    assert_eq!(current(&restored).faction_relations.get("freedom"), Some(&1_000));
    assert_eq!(current(&restored).relocate_to, Some(0x1234));
}

#[test]
fn schema_three_loads_durability_placement_and_upgrade_edits() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let bytes = format!(
        "{{\"index\":0,\"plans\":[{{\"sourceSha256\":\"{source_sha256}\",\"money\":null,\"stackCounts\":{{}},\"detachHandles\":[],\"adds\":[],\"stashTakes\":[],\"s2StashTakes\":[305419896],\"stashPuts\":[],\"durability\":{{\"4660\":75}},\"placements\":{{\"4660\":\"belt\"}},\"upgrades\":{{\"4660\":[\"wpn_upgrade_scope_1\"]}},\"unmappedLegacyPlan\":null}}],\"schema\":3,\"source_sha256\":\"{source_sha256}\"}}"
    );
    fs::write(store.path_for(source_sha256).expect("valid source hash"), bytes).expect("draft JSON should be written");

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft with all supported inventory edits should load");

    assert!(restored.can_apply_current());
    assert_eq!(current(&restored).s2_stash_takes, [0x1234_5678]);
    assert_eq!(current(&restored).durability.get(&4660), Some(&75));
    assert_eq!(current(&restored).placements.get(&4660), Some(&DraftPlacement::Belt));
    assert_eq!(
        current(&restored).upgrades.get(&4660),
        Some(&vec!["wpn_upgrade_scope_1".to_owned()])
    );
}

#[test]
fn schema_four_drafts_remain_readable_after_schema_three_rollout() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let bytes = format!(
        "{{\"index\":0,\"plans\":[{{\"sourceSha256\":\"{source_sha256}\",\"money\":null,\"stackCounts\":{{}},\"detachHandles\":[],\"adds\":[],\"stashTakes\":[],\"s2StashTakes\":[305419896],\"stashPuts\":[],\"durability\":{{}},\"placements\":{{}},\"upgrades\":{{}},\"unmappedLegacyPlan\":null}}],\"schema\":4,\"source_sha256\":\"{source_sha256}\"}}"
    );
    fs::write(store.path_for(source_sha256).expect("valid hash"), bytes).expect("schema-four draft should be written");

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("previous schema-four draft should remain readable");

    assert_eq!(current(&restored).s2_stash_takes, [0x1234_5678]);
}

#[test]
fn loads_schema_two_drafts_without_extended_fields() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let bytes = format!(
        "{{\"index\":0,\"plans\":[{{\"sourceSha256\":\"{source_sha256}\",\"money\":8765,\"stackCounts\":{{\"4660\":12}},\"detachHandles\":[],\"adds\":[],\"stashTakes\":[],\"stashPuts\":[],\"unmappedLegacyPlan\":null}}],\"schema\":2,\"source_sha256\":\"{source_sha256}\"}}"
    );
    fs::write(store.path_for(source_sha256).expect("valid source hash"), bytes).expect("draft JSON should be written");

    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("schema-two draft should remain readable");

    assert_eq!(current(&restored).money, Some(8765));
    assert_eq!(current(&restored).stack_counts.get(&4660), Some(&12));
    assert!(current(&restored).durability.is_empty());
}

#[test]
fn persists_a_draft_that_contains_only_durability_placement_and_upgrades() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid empty plan");
    plan.durability.insert(0x1234, 75);
    plan.placements.insert(0x1234, DraftPlacement::Slot(4));
    plan.upgrades.insert(0x1234, vec!["wpn_upgrade_scope_1".to_owned()]);
    let journal = DraftJournal::new(vec![plan], 0).expect("valid edit journal");

    store.save(journal).expect("draft should persist");
    let restored = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft with only non-count edits should load");

    assert_eq!(current(&restored).durability.get(&0x1234), Some(&75));
    assert_eq!(
        current(&restored).placements.get(&0x1234),
        Some(&DraftPlacement::Slot(4))
    );
    assert_eq!(
        current(&restored).upgrades.get(&0x1234),
        Some(&vec!["wpn_upgrade_scope_1".to_owned()])
    );
}

#[test]
fn rejects_durability_outside_zero_to_one_hundred_percent() {
    let source_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid empty plan");
    plan.durability.insert(0x1234, 101);

    assert!(DraftJournal::new(vec![plan], 0).is_err());
}

#[test]
fn rejects_slot_zero_in_draft_placement() {
    let source_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut plan = DraftPlan::empty(source_sha256).expect("valid empty plan");
    plan.placements.insert(0x1234, DraftPlacement::Slot(0));

    assert!(DraftJournal::new(vec![plan], 0).is_err());
}

#[test]
fn rejects_invalid_or_repeated_s2_stash_handles() {
    let source_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut zero = DraftPlan::empty(source_sha256).expect("valid empty plan");
    zero.s2_stash_takes.push(0);
    assert!(DraftJournal::new(vec![zero], 0).is_err());

    let mut duplicate = DraftPlan::empty(source_sha256).expect("valid empty plan");
    duplicate.s2_stash_takes.extend([0x1234_5678, 0x1234_5678]);
    assert!(DraftJournal::new(vec![duplicate], 0).is_err());
}

#[test]
fn keeps_untouched_state_and_only_the_latest_one_hundred_edits() {
    let source_sha256 = "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let mut journal =
        DraftJournal::new(vec![DraftPlan::empty(source_sha256).expect("valid plan")], 0).expect("valid journal");
    for money in 1..=150 {
        let mut plan = DraftPlan::empty(source_sha256).expect("valid plan");
        plan.money = Some(money);
        journal = journal.record(plan, false).expect("matching edit should be recorded");
    }

    assert_eq!(journal.plans().len(), 101);
    assert_eq!(journal.plans()[0].money, None);
    assert_eq!(journal.plans()[1].money, Some(51));
    assert_eq!(current(&journal).money, Some(150));
}

#[test]
fn rejects_invalid_source_hashes_and_does_not_load_for_a_different_save() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    assert!(store.path_for("../outside").is_err());
    let original = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let changed = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut plan = DraftPlan::empty(original).expect("valid hash");
    plan.money = Some(5);
    store
        .save(DraftJournal::new(vec![plan], 0).expect("valid journal"))
        .expect("save should succeed");
    assert!(store.load(changed).expect("read should succeed").is_none());
}

#[test]
fn identical_save_bytes_at_different_paths_have_separate_drafts() -> sse_core::Result<()> {
    let directory = TemporaryDirectory::new();
    let source_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let first = DraftStore::for_source(&directory.0, directory.0.join("slot-a.sav"));
    let second = DraftStore::for_source(&directory.0, directory.0.join("slot-b.sav"));
    let mut first_plan = DraftPlan::empty(source_sha256).expect("valid plan");
    first_plan.money = Some(111);
    let mut second_plan = DraftPlan::empty(source_sha256).expect("valid plan");
    second_plan.money = Some(222);

    first
        .save(DraftJournal::new(vec![first_plan], 0).expect("valid journal"))
        .expect("first path draft should save");
    second
        .save(DraftJournal::new(vec![second_plan], 0).expect("valid journal"))
        .expect("second path draft should save");

    assert_ne!(first.path_for(source_sha256)?, second.path_for(source_sha256)?);
    assert_eq!(
        first
            .load(source_sha256)?
            .expect("first draft should exist")
            .current()
            .expect("first current plan should exist")
            .money,
        Some(111)
    );
    assert_eq!(
        second
            .load(source_sha256)?
            .expect("second draft should exist")
            .current()
            .expect("second current plan should exist")
            .money,
        Some(222)
    );
    Ok(())
}

#[test]
fn preserves_unmapped_legacy_edits_and_refuses_to_branch_over_them() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0bb85823656a0280d6add2df2d0c7bdc80bc064eb9344eaafc3b5a73804cc3a0";
    let legacy = std::str::from_utf8(include_bytes!(
        "../../../fixtures/synthetic/drafts/python-v1-draft.json"
    ))
    .expect("legacy fixture should be UTF-8");
    let unmapped = legacy.replace(
        "\"durability\":[]",
        "\"durability\":[[9029,0.5]],\"future_operation\":{\"opaque\":true}",
    );
    fs::write(store.path_for(source_sha256).expect("valid source hash"), &unmapped)
        .expect("legacy draft should be written");

    let journal = store
        .load(source_sha256)
        .expect("draft read should succeed")
        .expect("draft should load");

    assert!(!journal.can_apply_current());
    assert!(current(&journal).unmapped_legacy_plan.is_some());
    let mut next = DraftPlan::empty(source_sha256).expect("valid plan");
    next.money = Some(901_000);
    assert!(journal.record(next.clone(), false).is_err());
    let explicitly_discarded = journal
        .record(next, true)
        .expect("explicit discard should allow a new plan");
    assert!(explicitly_discarded.can_apply_current());

    store.save(journal).expect("unmapped legacy data should persist");
    let restored = store
        .load(source_sha256)
        .expect("saved draft should read")
        .expect("saved draft should remain");
    assert!(!restored.can_apply_current());
    assert!(current(&restored).unmapped_legacy_plan.is_some());
}

#[test]
fn oversized_history_is_compacted_and_oversized_current_plan_is_refused() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut plans = vec![DraftPlan::empty(source_sha256).expect("valid initial plan")];
    for state in 0..20 {
        let mut plan = DraftPlan::empty(source_sha256).expect("valid plan");
        for item in 0..3_000 {
            plan.adds.push(
                AddRequest::new(
                    format!("synthetic_item_{state:02}_{item:04}_long_catalog_identifier"),
                    1,
                    "inventory",
                )
                .expect("valid add"),
            );
        }
        plans.push(plan);
    }
    let journal = DraftJournal::new(plans, 20).expect("valid journal");

    let compacted = store
        .save(journal)
        .expect("large history should compact to current state");
    assert_eq!(compacted.plans().len(), 2);
    assert_eq!(current(&compacted).adds.len(), 3_000);
    assert!(store.path_for(source_sha256).expect("valid hash").exists());

    let oversized_sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let mut oversized = DraftPlan::empty(oversized_sha).expect("valid plan");
    for item in 0..40_000 {
        oversized.adds.push(
            AddRequest::new(
                format!("synthetic_item_{item:05}_long_catalog_identifier"),
                1,
                "inventory",
            )
            .expect("valid add"),
        );
    }
    let journal = DraftJournal::new(vec![oversized], 0).expect("valid journal");
    assert!(store.save(journal).is_err());
    assert!(!store.path_for(oversized_sha).expect("valid hash").exists());
}

#[test]
fn existing_corrupt_or_oversized_drafts_are_reported_instead_of_treated_as_absent() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0bb85823656a0280d6add2df2d0c7bdc80bc064eb9344eaafc3b5a73804cc3a0";
    let path = store.path_for(source_sha256).expect("valid source hash");
    let fixture = include_bytes!("../../../fixtures/synthetic/drafts/python-v1-draft.json");

    for end in 0..fixture.len() {
        fs::write(&path, &fixture[..end]).expect("truncated draft should be written");
        assert!(
            store.load(source_sha256).is_err(),
            "truncation at byte {end} was hidden"
        );
    }

    let mut seed = 0x9e37_79b9_u32;
    for _ in 0..256 {
        let mut mutated = fixture.to_vec();
        seed ^= seed.wrapping_shl(13);
        seed ^= seed.wrapping_shr(17);
        seed ^= seed.wrapping_shl(5);
        let index = (seed as usize) % mutated.len();
        mutated[index] ^= 1 << (seed % 8);
        fs::write(&path, mutated).expect("mutated draft should be written");
        assert!(
            !matches!(store.load(source_sha256), Ok(None)),
            "existing mutated draft was treated as missing"
        );
    }

    fs::write(&path, vec![0_u8; 2 * 1024 * 1024 + 1]).expect("oversized draft should be written");
    assert!(store.load(source_sha256).is_err(), "oversized draft was hidden");
}

#[test]
fn save_does_not_overwrite_a_corrupt_existing_draft() {
    let directory = TemporaryDirectory::new();
    let store = DraftStore::new(&directory.0);
    let source_sha256 = "0bb85823656a0280d6add2df2d0c7bdc80bc064eb9344eaafc3b5a73804cc3a0";
    let path = store.path_for(source_sha256).expect("valid source hash");
    let original = b"{not a draft";
    fs::write(&path, original).expect("corrupt draft should be written");
    let journal =
        DraftJournal::new(vec![DraftPlan::empty(source_sha256).expect("valid plan")], 0).expect("valid journal");

    assert!(store.save(journal).is_err());
    assert_eq!(fs::read(path).expect("original must remain"), original);
}

#[test]
fn draft_plans_refuse_detach_and_stack_handles_outside_the_item_range() {
    let source = "0".repeat(63) + "1";
    let mut detach = DraftPlan::empty(&source).expect("plan");
    detach.detach_handles = vec![0];
    assert!(DraftJournal::new(vec![detach], 0).is_err());

    let mut max_detach = DraftPlan::empty(&source).expect("plan");
    max_detach.detach_handles = vec![u16::MAX];
    assert!(DraftJournal::new(vec![max_detach], 0).is_err());

    let mut stack = DraftPlan::empty(&source).expect("plan");
    stack.stack_counts.insert(0, 2);
    assert!(DraftJournal::new(vec![stack], 0).is_err());
}
