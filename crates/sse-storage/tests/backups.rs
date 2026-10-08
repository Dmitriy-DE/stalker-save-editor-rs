//! Backup listing, C# journal shape, and restore verification.

#![allow(clippy::arithmetic_side_effects, clippy::expect_used, clippy::indexing_slicing)]

use sse_storage::transaction::{
    export_transaction, list_backups, replace_transaction, restore_backup, restore_in_place, BackupStatus, EditSummary,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TemporaryDirectory(PathBuf);

impl TemporaryDirectory {
    fn new() -> Self {
        let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-backup-test-{}-{id}", std::process::id()));
        fs::create_dir_all(&path).expect("temporary directory should be created");
        Self(path)
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn export_fixture(root: &std::path::Path) -> (Vec<u8>, Vec<u8>, std::path::PathBuf, std::path::PathBuf) {
    let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav").to_vec();
    let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav").to_vec();
    fs::create_dir_all(root.join("saves")).expect("save directory should be created");
    fs::create_dir_all(root.join("exports")).expect("export directory should be created");
    let source = root.join("saves/source.sav");
    let output = root.join("exports/edited.sav");
    fs::write(&source, &original).expect("source fixture should be written");
    let summary = EditSummary {
        money: Some(900_000),
        stack_count: 2,
        ..EditSummary::default()
    };
    export_transaction(
        &source,
        &sse_codecs::sha256::sha256_hex(&original),
        &replacement,
        &output,
        &root.join("backups"),
        summary,
    )
    .expect("fixture export should succeed");
    (original, replacement, source, root.join("backups"))
}

#[test]
fn exported_journal_lists_verified_and_restores_original_bytes_to_a_new_path() {
    let directory = TemporaryDirectory::new();
    let (original, replacement, source, backup_directory) = export_fixture(&directory.0);

    let entries = list_backups(&backup_directory).expect("backup listing should succeed");

    let entry = entries.first().expect("export should be listed");
    assert_eq!(entry.status, BackupStatus::Verified);
    assert_eq!(fs::read(&source).expect("source should still exist"), original);
    assert_eq!(
        fs::read(directory.0.join("exports/edited.sav")).expect("output should exist"),
        replacement
    );
    let journal = fs::read_to_string(&entry.journal_path).expect("journal should be readable");
    for field in [
        "\"version\":1",
        "\"status\":\"verified\"",
        "\"source_path\"",
        "\"source_sha256\"",
        "\"output_path\"",
        "\"output_sha256\"",
        "\"backup_path\"",
        "\"operation\"",
        "\"mode\":\"export\"",
        "\"money\":900000",
        "\"stack_count\":2",
    ] {
        assert!(journal.contains(field), "journal must contain {field}");
    }

    let restored = directory.0.join("restored.sav");
    restore_backup(&entry.journal_path, &restored).expect("verified backup should restore");
    assert_eq!(fs::read(restored).expect("restored save should be readable"), original);
}

#[cfg(unix)]
#[test]
fn backup_listing_does_not_treat_a_broken_directory_symlink_as_empty() {
    use std::os::unix::fs::symlink;

    let directory = TemporaryDirectory::new();
    let backup_directory = directory.0.join("backups");
    symlink(directory.0.join("missing-target"), &backup_directory)
        .expect("broken backup-directory symlink should be created");

    assert!(
        list_backups(&backup_directory).is_err(),
        "an unusable backup directory must not appear to contain no backups"
    );
}

#[test]
fn recovering_an_interrupted_write_clears_its_pending_recovery_status() {
    let directory = TemporaryDirectory::new();
    let save_directory = directory.0.join("saves");
    let backup_directory = directory.0.join("backups");
    fs::create_dir_all(&save_directory).expect("save directory should be created");
    let source = save_directory.join("source.sav");
    let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
    let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
    fs::write(&source, original).expect("source should be written");
    let receipt = replace_transaction(
        &source,
        &sse_codecs::sha256::sha256_hex(original),
        replacement,
        &backup_directory,
    )
    .expect("replacement should succeed");
    let journal = fs::read_to_string(&receipt.journal_path).expect("journal should be readable");
    fs::write(
        &receipt.journal_path,
        journal.replace("\"status\":\"verified\"", "\"status\":\"prepared\""),
    )
    .expect("journal should be marked interrupted");
    assert!(list_backups(&backup_directory)
        .expect("interrupted backup should list")
        .iter()
        .any(|entry| entry.status == BackupStatus::Interrupted));

    restore_in_place(&receipt.journal_path).expect("interrupted save should restore");

    assert_eq!(fs::read(&source).expect("restored source should read"), original);
    assert!(list_backups(&backup_directory)
        .expect("recovered backup should list")
        .iter()
        .all(|entry| entry.status != BackupStatus::Interrupted));
}

#[cfg(target_os = "linux")]
#[test]
fn export_journal_keeps_non_unicode_source_and_output_paths_lossless() {
    use std::os::unix::ffi::OsStringExt;

    let directory = TemporaryDirectory::new();
    let saves = directory.0.join("saves");
    let exports = directory.0.join("exports");
    let backups = directory.0.join("backups");
    fs::create_dir_all(&saves).expect("save directory should be created");
    fs::create_dir_all(&exports).expect("export directory should be created");
    let source = saves.join(std::ffi::OsString::from_vec(b"source-\xff.sav".to_vec()));
    let output = exports.join(std::ffi::OsString::from_vec(b"edited-\xfe.sav".to_vec()));
    let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
    let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
    fs::write(&source, original).expect("source should be written");

    export_transaction(
        &source,
        &sse_codecs::sha256::sha256_hex(original),
        replacement,
        &output,
        &backups,
        EditSummary::default(),
    )
    .expect("export should support non-Unicode paths");

    let entry = list_backups(&backups).expect("backup should list").remove(0);
    assert_eq!(entry.source_path, source);
    assert_eq!(entry.status, BackupStatus::Verified);
    assert_eq!(fs::read(&output).expect("output should exist"), replacement);
}

#[cfg(target_os = "linux")]
#[test]
fn failed_export_does_not_leave_an_orphan_backup() {
    let directory = TemporaryDirectory::new();
    let saves = directory.0.join("saves");
    let backups = directory.0.join("backups");
    fs::create_dir_all(&saves).expect("save directory should be created");
    let source = saves.join("source.sav");
    let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
    let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
    fs::write(&source, original).expect("source fixture should be written");

    let result = export_transaction(
        &source,
        &sse_codecs::sha256::sha256_hex(original),
        replacement,
        &PathBuf::from("/proc/sse-export-must-not-be-created.sav"),
        &backups,
        EditSummary::default(),
    );

    assert!(result.is_err(), "publishing under /proc should fail");
    assert!(
        list_backups(&backups)
            .expect("backup listing should succeed after a failed export")
            .is_empty(),
        "failed export must not leave an orphan reported as a corrupt backup"
    );
}

#[test]
fn in_place_backup_rotation_keeps_the_one_hundred_newest_verified_sets() {
    let directory = TemporaryDirectory::new();
    let save_directory = directory.0.join("saves");
    let backup_directory = directory.0.join("backups");
    fs::create_dir_all(&save_directory).expect("save directory should be created");
    let source = save_directory.join("source.sav");
    let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
    let edited = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
    fs::write(&source, original).expect("source should be written");

    for _ in 0..102 {
        let current = fs::read(&source).expect("current source should be readable");
        let (expected, replacement) = if current == original {
            (original.as_slice(), edited.as_slice())
        } else {
            (edited.as_slice(), original.as_slice())
        };
        replace_transaction(
            &source,
            &sse_codecs::sha256::sha256_hex(expected),
            replacement,
            &backup_directory,
        )
        .expect("in-place save should create a verified backup");
    }

    let entries = list_backups(&backup_directory).expect("backup listing should succeed");
    assert_eq!(entries.len(), 100);
    assert!(entries.iter().all(|entry| entry.status == BackupStatus::Verified));
}

#[test]
fn missing_and_corrupt_backup_journals_are_listed_but_never_restored() {
    let directory = TemporaryDirectory::new();
    let (_, _, _, backup_directory) = export_fixture(&directory.0);
    let verified = list_backups(&backup_directory).expect("backup listing should succeed");
    let entry = verified.first().expect("export should be listed").clone();
    fs::remove_file(&entry.backup_path).expect("backup should be removable for this test");

    let missing = list_backups(&backup_directory).expect("missing backup should be listable");
    assert_eq!(missing[0].status, BackupStatus::Missing);
    assert!(restore_backup(&entry.journal_path, &directory.0.join("missing.sav")).is_err());

    fs::write(&entry.backup_path, b"tampered backup").expect("corrupt backup should be written");
    let corrupt = list_backups(&backup_directory).expect("corrupt backup should be listable");
    assert_eq!(corrupt[0].status, BackupStatus::Corrupt);
    assert!(restore_backup(&entry.journal_path, &directory.0.join("corrupt.sav")).is_err());
}

#[test]
fn an_unjournaled_backup_is_reported_as_corrupt() {
    let directory = TemporaryDirectory::new();
    let backup_directory = directory.0.join("backups");
    fs::create_dir_all(&backup_directory).expect("backup directory should be created");
    fs::write(backup_directory.join("orphan_ORIGINAL.sav"), b"synthetic orphan")
        .expect("orphan backup should be written");

    let entries = list_backups(&backup_directory).expect("orphan should be listable");

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].status, BackupStatus::Corrupt);
    assert!(entries[0].error.as_deref().unwrap_or_default().contains("journal"));
}

#[test]
fn truncated_hostile_and_mutated_journals_fail_closed() {
    let directory = TemporaryDirectory::new();
    let (_, _, _, backup_directory) = export_fixture(&directory.0);
    let entries = list_backups(&backup_directory).expect("journal should list");
    let journal_path = entries
        .first()
        .expect("export should have a journal")
        .journal_path
        .clone();
    let original = fs::read(&journal_path).expect("journal should be readable");

    for end in 0..original.len() {
        fs::write(&journal_path, &original[..end]).expect("truncated journal should be written");
        let listed = list_backups(&backup_directory).expect("truncated journal should fail closed");
        assert_eq!(listed[0].status, BackupStatus::Corrupt);
    }

    let mut seed = 0x85eb_ca6b_u32;
    for _ in 0..256 {
        let mut mutated = original.clone();
        seed ^= seed.wrapping_shl(13);
        seed ^= seed.wrapping_shr(17);
        seed ^= seed.wrapping_shl(5);
        let index = (seed as usize) % mutated.len();
        mutated[index] ^= 1 << (seed % 8);
        fs::write(&journal_path, mutated).expect("mutated journal should be written");
        assert!(list_backups(&backup_directory).is_ok());
    }

    fs::write(&journal_path, vec![b' '; 2 * 1024 * 1024 + 1]).expect("oversized journal should be written");
    let listed = list_backups(&backup_directory).expect("oversized journal should fail closed");
    assert_eq!(listed[0].status, BackupStatus::Corrupt);
}
