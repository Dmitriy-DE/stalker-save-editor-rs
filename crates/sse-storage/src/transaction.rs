//! Journaled local save replacement.

use sse_codecs::sha256;
use sse_core::{Error, Result};
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);
const MAXIMUM_JOURNAL_BYTES: u64 = 2 * 1024 * 1024;
const MAXIMUM_VERIFIED_BACKUP_SETS: usize = 100;

/// File operations used by the transaction; injectable implementations support failure testing.
pub trait FileSystem {
    /// Reads a complete file.
    fn read_all(&self, path: &Path) -> Result<Vec<u8>>;
    /// Reports whether a path itself is a symbolic link without following it.
    fn is_symlink(&self, path: &Path) -> Result<bool>;
    /// Creates a directory and its parents.
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    /// Creates a new file, writes all bytes, and flushes it durably.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()>;
    /// Copies the source file's permissions to a newly created replacement file.
    fn copy_permissions(&self, source: &Path, destination: &Path) -> Result<()>;
    /// Atomically moves one file over another and flushes the destination directory where supported.
    fn replace(&self, source: &Path, destination: &Path) -> Result<()>;
    /// Rechecks the destination hash immediately before replacing it.
    fn replace_if_sha256_matches(&self, source: &Path, destination: &Path, expected_sha256: &str) -> Result<()> {
        let current = self.read_all(destination)?;
        if sha256::sha256_hex(&current) != expected_sha256 {
            return Err(Error::Refused(
                "source changed immediately before replacement".to_owned(),
            ));
        }
        self.replace(source, destination)
    }
    /// Publishes a new path atomically without overwriting an existing destination.
    fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
        self.replace(source, destination)
    }
    /// Rechecks a source hash immediately before publishing a new output path.
    fn publish_new_if_source_sha256_matches(
        &self,
        staged_output: &Path,
        output_path: &Path,
        source_path: &Path,
        expected_source_sha256: &str,
    ) -> Result<()> {
        let current = self.read_all(source_path)?;
        if sha256::sha256_hex(&current) != expected_source_sha256 {
            return Err(Error::Refused(
                "source changed immediately before export publication".to_owned(),
            ));
        }
        self.publish_new(staged_output, output_path)
    }
    /// Removes a file if it exists.
    fn delete_if_exists(&self, path: &Path) -> Result<()>;
    /// Reports the platform read-only attribute or permission state used to reject unsafe replacement.
    fn is_readonly(&self, _path: &Path) -> Result<bool> {
        Ok(false)
    }
    /// Flushes a directory entry update where the platform supports directory synchronization.
    fn sync_directory(&self, _directory: &Path) -> Result<()> {
        Ok(())
    }
    /// Prunes old verified backup sets. The default is a no-op for in-memory filesystems; persistent
    /// implementations should override it if they manage the backup directory.
    fn prune_verified_backups(&self, _directory: &Path) -> Result<()> {
        Ok(())
    }
}

/// Standard-library filesystem operations for save transactions.
#[derive(Debug, Default)]
pub struct StdFileSystem;

impl FileSystem for StdFileSystem {
    fn read_all(&self, path: &Path) -> Result<Vec<u8>> {
        Ok(fs::read(path)?)
    }

    fn is_symlink(&self, path: &Path) -> Result<bool> {
        Ok(fs::symlink_metadata(path)?.file_type().is_symlink())
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        fs::create_dir_all(path)?;
        if let Some(parent) = path.parent() {
            sync_directory(Some(parent))?;
        }
        Ok(())
    }

    fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        sse_sys::secure_fs::atomic_write(
            path,
            bytes,
            sse_sys::secure_fs::AtomicWriteOptions::create_new()
                .with_unix_mode(0o600)
                .without_parent_sync(),
        )?;
        if let Some(parent) = path.parent() {
            if let Err(error) = sync_directory(Some(parent)) {
                let _ = fs::remove_file(path);
                return Err(error);
            }
        }
        Ok(())
    }

    fn copy_permissions(&self, source: &Path, destination: &Path) -> Result<()> {
        sse_sys::secure_fs::copy_dacl(source, destination)?;
        let _ = sse_sys::secure_fs::copy_owner_and_group(source, destination);
        fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
        Ok(())
    }

    fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
        sse_sys::secure_fs::replace_existing(source, destination)?;
        Ok(())
    }

    fn replace_if_sha256_matches(&self, source: &Path, destination: &Path, expected_sha256: &str) -> Result<()> {
        let current_sha256 = fresh_file_sha256(destination)?;
        if current_sha256 != expected_sha256 {
            return Err(Error::Refused(
                "source changed immediately before replacement".to_owned(),
            ));
        }
        self.replace(source, destination)
    }

    fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
        sse_sys::secure_fs::publish_new(source, destination)?;
        Ok(())
    }

    fn publish_new_if_source_sha256_matches(
        &self,
        staged_output: &Path,
        output_path: &Path,
        source_path: &Path,
        expected_source_sha256: &str,
    ) -> Result<()> {
        if fresh_file_sha256(source_path)? != expected_source_sha256 {
            return Err(Error::Refused(
                "source changed immediately before export publication".to_owned(),
            ));
        }
        self.publish_new(staged_output, output_path)
    }

    fn delete_if_exists(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn is_readonly(&self, path: &Path) -> Result<bool> {
        Ok(fs::metadata(path)?.permissions().readonly())
    }

    fn sync_directory(&self, directory: &Path) -> Result<()> {
        sync_directory(Some(directory))
    }

    fn prune_verified_backups(&self, directory: &Path) -> Result<()> {
        rotate_verified_backups(directory)
    }
}

/// Paths and output hash created by a successful replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementReceipt {
    /// Replaced save path.
    pub source_path: PathBuf,
    /// Durable original save copy.
    pub backup_path: PathBuf,
    /// Durable copy of the edited bytes for recovery.
    pub recovery_path: PathBuf,
    /// C#-compatible JSON journal path.
    pub journal_path: PathBuf,
    /// SHA-256 of the replacement bytes, in lowercase hexadecimal.
    pub output_sha256: String,
    /// Warning when optional old-backup rotation could not complete.
    pub maintenance_warning: Option<String>,
}

/// Edit summary stored in the C#-compatible export journal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EditSummary {
    /// Requested wallet value, when the export changes money.
    pub money: Option<u32>,
    /// Number of stack count changes in the edit set.
    pub stack_count: usize,
    /// Number of stash moves in the edit set.
    pub move_count: usize,
    /// Number of removed items in the edit set.
    pub detach_count: usize,
    /// Number of attached items in the edit set.
    pub attach_count: usize,
    /// Number of raw changes in the edit set.
    pub raw_count: usize,
    /// Number of item additions in the edit set.
    pub add_count: usize,
    /// Number of durability changes in the edit set.
    pub durability_count: usize,
    /// Number of upgrade changes in the edit set.
    pub upgrade_count: usize,
    /// Number of faction relation changes in the edit set.
    pub relation_count: usize,
    /// Whether the actor's player faction changed.
    pub player_faction: bool,
}

/// Paths and hash created by a safe export to a new file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReceipt {
    /// Newly published output path.
    pub output_path: PathBuf,
    /// Durable copy of the original source.
    pub backup_path: PathBuf,
    /// C#-compatible JSON journal path.
    pub journal_path: PathBuf,
    /// SHA-256 of the exported bytes.
    pub output_sha256: String,
    /// Warning when optional old-backup rotation could not complete.
    pub maintenance_warning: Option<String>,
}

/// Durable local recovery artifacts created before a Steam Cloud write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudRecoveryReceipt {
    /// Copy of the cloud bytes that were about to be replaced.
    pub backup_path: PathBuf,
    /// Copy of the bytes prepared for upload.
    pub recovery_path: PathBuf,
    /// Journal shown in the application's backup list.
    pub journal_path: PathBuf,
}

/// Result of an in-place backup restore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestoreReceipt {
    /// Save file that now contains the restored bytes.
    pub save_path: PathBuf,
    /// Safety backup created from the pre-restore save, when the file existed.
    pub safety_backup_path: Option<PathBuf>,
    /// Journal for the safety backup, when the file existed.
    pub safety_journal_path: Option<PathBuf>,
    /// Warning when optional old-backup rotation could not complete.
    pub maintenance_warning: Option<String>,
}

/// Backup listing status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupStatus {
    /// Journal and backup are valid, with no interrupted in-place write pending.
    Verified,
    /// Journal refers to a backup that is absent.
    Missing,
    /// The prepared journal and original backup are intact, but publication was interrupted.
    Interrupted,
    /// Journal or backup contents do not pass validation.
    Corrupt,
}

/// One journaled backup found in a backup directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupEntry {
    /// Journal path.
    pub journal_path: PathBuf,
    /// Original backup path declared by the journal.
    pub backup_path: PathBuf,
    /// Original source path declared by the journal.
    pub source_path: PathBuf,
    /// Expected SHA-256 from the journal.
    pub source_sha256: String,
    /// Verification result.
    pub status: BackupStatus,
    /// Operation recorded by the journal, when it could be read.
    pub operation_mode: Option<String>,
    /// Failure detail for missing or corrupt entries.
    pub error: Option<String>,
}

/// Exports a prepared save to a new file, records the original and verifies the durable output.
pub fn export_transaction(
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    output_path: &Path,
    backup_directory: &Path,
    summary: EditSummary,
) -> Result<ExportReceipt> {
    if replacement.is_empty() {
        return Err(Error::Refused("replacement save is empty".to_owned()));
    }
    if expected_source_sha256.len() != 64 || !expected_source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Refused("prepared source hash is not a SHA-256 value".to_owned()));
    }
    let source_path = absolute_path(source_path)?;
    let output_path = absolute_path(output_path)?;
    let backup_directory = absolute_path(backup_directory)?;
    if source_path == output_path {
        return Err(Error::Refused(
            "export output cannot be the selected source save".to_owned(),
        ));
    }
    if fs::symlink_metadata(&source_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(Error::Refused(
            "writing through a symbolic-link save is refused".to_owned(),
        ));
    }
    match fs::symlink_metadata(&output_path) {
        Ok(_) => {
            return Err(Error::System(format!(
                "export output already exists: {}",
                output_path.display()
            )));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let source_directory = source_path
        .parent()
        .ok_or_else(|| Error::Refused("source save has no parent directory".to_owned()))?;
    let output_directory = output_path
        .parent()
        .ok_or_else(|| Error::Refused("export output has no parent directory".to_owned()))?;
    if !output_directory.is_dir() {
        return Err(Error::Refused(format!(
            "export output directory does not exist: {}",
            output_directory.display()
        )));
    }
    ensure_backup_directory_outside(source_directory, &backup_directory)?;

    let source_bytes = fs::read(&source_path)?;
    let source_sha256 = sha256::sha256_hex(&source_bytes);
    if source_sha256 != expected_source_sha256 {
        return Err(Error::Refused(format!(
            "Source changed since analysis: expected {expected_source_sha256}, found {source_sha256}."
        )));
    }
    let output_sha256 = sha256::sha256_hex(replacement);
    let token = transaction_token();
    let stem = artifact_stem_prefix(&source_path);
    let artifact_stem = format!("{stem}_{token}");
    let backup_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.sav"));
    let journal_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.json"));
    let output_name = output_path.file_name().unwrap_or_else(|| OsStr::new("save.sav"));
    let temporary_output = temporary_sibling(output_directory, output_name, &token, ".tmp");
    let temporary_journal = backup_directory.join(format!(".{artifact_stem}.json.tmp"));
    StdFileSystem.create_dir_all(&backup_directory)?;
    ensure_backup_directory_outside(source_directory, &backup_directory)?;
    let mut published = false;
    let mut backup_created = false;
    let mut journal_created = false;
    let mut temporary_output_created = false;
    let mut temporary_journal_created = false;
    let transaction = (|| {
        StdFileSystem.write_new(&backup_path, &source_bytes)?;
        backup_created = true;
        StdFileSystem.write_new(&temporary_output, replacement)?;
        temporary_output_created = true;
        let created_at = timestamp_utc()?;
        let prepared_journal = serialize_export_journal(&ExportJournal {
            status: "prepared",
            created_at: &created_at,
            source_path: &source_path,
            source_sha256: &source_sha256,
            output_path: &output_path,
            output_sha256: &output_sha256,
            backup_path: &backup_path,
            summary,
        });
        StdFileSystem.write_new(&journal_path, &prepared_journal)?;
        journal_created = true;
        StdFileSystem.publish_new_if_source_sha256_matches(
            &temporary_output,
            &output_path,
            &source_path,
            expected_source_sha256,
        )?;
        published = true;
        temporary_output_created = false;
        sync_directory(Some(output_directory))?;
        let read_back = fs::read(&output_path)?;
        if read_back != replacement || sha256::sha256_hex(&read_back) != output_sha256 {
            return Err(Error::System(
                "export output read-back did not match the prepared bytes".to_owned(),
            ));
        }
        let verified_journal = serialize_export_journal(&ExportJournal {
            status: "verified",
            created_at: &created_at,
            source_path: &source_path,
            source_sha256: &source_sha256,
            output_path: &output_path,
            output_sha256: &output_sha256,
            backup_path: &backup_path,
            summary,
        });
        StdFileSystem.write_new(&temporary_journal, &verified_journal)?;
        temporary_journal_created = true;
        StdFileSystem.replace(&temporary_journal, &journal_path)?;
        temporary_journal_created = false;
        sync_directory(Some(&backup_directory))?;
        Ok(())
    })();
    if let Err(error) = transaction {
        let mut remove_attempt_artifacts = !published;
        let mut cleanup_error = None;
        if published {
            match fs::read(&output_path) {
                Ok(bytes) if bytes == replacement => match remove_file_if_exists(&output_path) {
                    Ok(()) => {
                        remove_attempt_artifacts = true;
                        if let Err(sync_error) = sync_directory(Some(output_directory)) {
                            cleanup_error = Some(sync_error);
                        }
                    }
                    Err(remove_error) => cleanup_error = Some(remove_error),
                },
                _ => {}
            }
        }
        if temporary_output_created {
            if let Err(remove_error) = remove_file_if_exists(&temporary_output) {
                cleanup_error.get_or_insert(remove_error);
            }
        }
        if temporary_journal_created {
            if let Err(remove_error) = remove_file_if_exists(&temporary_journal) {
                cleanup_error.get_or_insert(remove_error);
            }
        }
        if remove_attempt_artifacts {
            if journal_created {
                if let Err(remove_error) = remove_file_if_exists(&journal_path) {
                    cleanup_error.get_or_insert(remove_error);
                }
            }
            if backup_created {
                if let Err(remove_error) = remove_file_if_exists(&backup_path) {
                    cleanup_error.get_or_insert(remove_error);
                }
            }
            if let Err(sync_error) = sync_directory(Some(&backup_directory)) {
                cleanup_error.get_or_insert(sync_error);
            }
        }
        if let Some(cleanup_error) = cleanup_error {
            return Err(Error::System(format!(
                "export failed ({error}); cleanup was incomplete ({cleanup_error})"
            )));
        }
        return Err(error);
    }
    let maintenance_warning = rotate_verified_backups(&backup_directory)
        .err()
        .map(|error| error.to_string());
    Ok(ExportReceipt {
        output_path,
        backup_path,
        journal_path,
        output_sha256,
        maintenance_warning,
    })
}

/// Writes durable cloud preimage and upload-recovery copies under the configured backup folder.
///
/// The journal's source path points to a new-copy destination, so backup restoration cannot
/// replace a local save or write back to Steam Cloud.
pub fn write_cloud_recovery_artifacts(
    directory: &Path,
    app_id: u32,
    remote_name: &str,
    cloud_bytes: &[u8],
    upload_bytes: &[u8],
) -> Result<CloudRecoveryReceipt> {
    if app_id == 0 || remote_name.trim().is_empty() {
        return Err(Error::Refused("Steam Cloud recovery identity is incomplete".to_owned()));
    }
    if cloud_bytes.is_empty() || upload_bytes.is_empty() {
        return Err(Error::Refused(
            "Steam Cloud recovery artifacts must not be empty".to_owned(),
        ));
    }
    let directory = absolute_path(directory)?;
    let remote_leaf = remote_name
        .rsplit(['/', '\\'])
        .next()
        .filter(|leaf| !leaf.is_empty())
        .ok_or_else(|| Error::Refused("Steam Cloud filename is empty".to_owned()))?;
    let safe_name = safe_cloud_artifact_stem(remote_leaf);
    let artifact_stem = format!("steam-cloud-{app_id}-{safe_name}_{}", transaction_token());
    let backup_path = directory.join(format!("{artifact_stem}_ORIGINAL.sav"));
    let recovery_path = directory.join(format!("{artifact_stem}_EDITED.sav"));
    let journal_path = directory.join(format!("{artifact_stem}_ORIGINAL.json"));
    let source_path = directory
        .join("cloud_recoveries")
        .join(app_id.to_string())
        .join(format!("{safe_name}.sav"));
    let created_at = timestamp_utc()?;
    let source_sha256 = sha256::sha256_hex(cloud_bytes);
    let output_sha256 = sha256::sha256_hex(upload_bytes);
    let journal = serialize_journal(&Journal {
        status: "verified",
        created_at: &created_at,
        source_path: &source_path,
        source_sha256: &source_sha256,
        output_path: &recovery_path,
        output_sha256: &output_sha256,
        backup_path: &backup_path,
        recovery_path: &recovery_path,
        operation: JournalOperation::SteamCloud,
    });

    StdFileSystem.create_dir_all(&directory)?;
    let source_directory = source_path
        .parent()
        .ok_or_else(|| Error::Refused("Steam Cloud recovery path has no parent directory".to_owned()))?;
    StdFileSystem.create_dir_all(source_directory)?;

    let mut backup_created = false;
    let mut recovery_created = false;
    let mut journal_created = false;
    let write_result = (|| {
        StdFileSystem.write_new(&backup_path, cloud_bytes)?;
        backup_created = true;
        StdFileSystem.write_new(&recovery_path, upload_bytes)?;
        recovery_created = true;
        StdFileSystem.write_new(&journal_path, &journal)?;
        journal_created = true;
        Ok(())
    })();
    if let Err(error) = write_result {
        let mut cleanup_error = None;
        for (path, created) in [
            (&journal_path, journal_created),
            (&recovery_path, recovery_created),
            (&backup_path, backup_created),
        ] {
            if created {
                if let Err(remove_error) = StdFileSystem.delete_if_exists(path) {
                    cleanup_error.get_or_insert(remove_error);
                }
            }
        }
        if let Err(sync_error) = StdFileSystem.sync_directory(&directory) {
            cleanup_error.get_or_insert(sync_error);
        }
        if let Some(cleanup_error) = cleanup_error {
            return Err(Error::System(format!(
                "Steam Cloud recovery write failed ({error}); cleanup was incomplete ({cleanup_error})"
            )));
        }
        return Err(error);
    }

    Ok(CloudRecoveryReceipt {
        backup_path,
        recovery_path,
        journal_path,
    })
}

/// Lists and verifies one directory of C#-compatible backup journals.
pub fn list_backups(directory: &Path) -> Result<Vec<BackupEntry>> {
    let directory = absolute_path(directory)?;
    match fs::symlink_metadata(&directory) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "json") {
            paths.push(path);
        }
    }
    paths.sort();
    let mut entries = Vec::with_capacity(paths.len());
    let mut referenced = HashSet::with_capacity(paths.len());
    for journal_path in paths {
        let entry = inspect_backup(&directory, &journal_path);
        referenced.insert(entry.backup_path.clone());
        entries.push(entry);
    }
    for item in fs::read_dir(&directory)? {
        let item = item?;
        let backup_path = item.path();
        if backup_path
            .file_name()
            .is_some_and(|name| has_native_ascii_suffix(name, "_ORIGINAL.sav"))
            && !referenced.contains(&backup_path)
        {
            entries.push(BackupEntry {
                journal_path: backup_path.with_extension("json"),
                backup_path,
                source_path: PathBuf::new(),
                source_sha256: String::new(),
                status: BackupStatus::Corrupt,
                operation_mode: None,
                error: Some("journal for this backup is missing".to_owned()),
            });
        }
    }
    entries.sort_by(|left, right| right.journal_path.cmp(&left.journal_path));
    Ok(entries)
}

fn rotate_verified_backups(directory: &Path) -> Result<()> {
    let directory = absolute_path(directory)?;
    let entries = list_backups(&directory)?;
    let mut verified = entries
        .into_iter()
        .filter(|entry| entry.status == BackupStatus::Verified)
        .map(|entry| {
            let bytes = fs::read(&entry.journal_path)?;
            let fields = parse_top_fields(&bytes)?;
            let created_at = field_string(&fields, "created_at")?;
            Ok((created_at, entry, fields))
        })
        .collect::<Result<Vec<_>>>()?;
    verified.sort_by(|left, right| right.0.cmp(&left.0));
    if verified.len() <= MAXIMUM_VERIFIED_BACKUP_SETS {
        return Ok(());
    }

    let mut removed_any = false;
    let mut kept_unverified = 0_usize;
    for (_, entry, fields) in verified.into_iter().skip(MAXIMUM_VERIFIED_BACKUP_SETS) {
        if entry.journal_path.parent() != Some(directory.as_path())
            || entry.backup_path.parent() != Some(directory.as_path())
        {
            return Err(Error::Refused(
                "refusing to rotate backup files outside the selected directory".to_owned(),
            ));
        }
        let recovery_path = if fields.contains_key("recovery_path") || fields.contains_key("recovery_path_native") {
            Some(path_field(&fields, "recovery_path", "recovery_path_native")?)
        } else {
            None
        };
        if let Some(recovery_path) = recovery_path.as_ref() {
            if recovery_path.parent() != Some(directory.as_path()) {
                return Err(Error::Refused(
                    "refusing to rotate a backup with an external recovery path".to_owned(),
                ));
            }
            let output_sha256 = field_string(&fields, "output_sha256")?;
            if !valid_sha256(&output_sha256) || cached_file_sha256(recovery_path)? != output_sha256 {
                // Keep this set and move on, so the older sets are still removed.
                kept_unverified = kept_unverified.saturating_add(1);
                continue;
            }
        }

        let mut cleanup_error = None;
        if let Some(recovery_path) = recovery_path.as_ref() {
            match remove_file_if_exists(recovery_path) {
                Ok(()) => {
                    invalidate_backup_hash_cache(recovery_path);
                    removed_any = true;
                }
                Err(error) => cleanup_error = Some(error),
            }
        }
        if cleanup_error.is_none() {
            match remove_file_if_exists(&entry.backup_path) {
                Ok(()) => {
                    invalidate_backup_hash_cache(&entry.backup_path);
                    removed_any = true;
                }
                Err(error) => cleanup_error = Some(error),
            }
        }
        if cleanup_error.is_none() {
            match remove_file_if_exists(&entry.journal_path) {
                Ok(()) => removed_any = true,
                Err(error) => cleanup_error = Some(error),
            }
        }
        if let Some(error) = cleanup_error {
            if removed_any {
                sync_directory(Some(&directory))?;
            }
            return Err(error);
        }
    }
    if removed_any {
        sync_directory(Some(&directory))?;
    }
    if kept_unverified > 0 {
        return Err(Error::Refused(format!(
            "kept {kept_unverified} backup set(s) whose recovery copy does not match its journal"
        )));
    }
    Ok(())
}

/// Restores a verified journaled backup to a new path without replacing the source save.
pub fn restore_backup(journal_path: &Path, output_path: &Path) -> Result<PathBuf> {
    restore_backup_with_file_system(&StdFileSystem, journal_path, output_path)
}

fn restore_backup_with_file_system(
    files: &impl FileSystem,
    journal_path: &Path,
    output_path: &Path,
) -> Result<PathBuf> {
    let journal_path = absolute_path(journal_path)?;
    let output_path = absolute_path(output_path)?;
    let directory = journal_path
        .parent()
        .ok_or_else(|| Error::Refused("backup journal has no parent directory".to_owned()))?;
    let entry = inspect_backup(directory, &journal_path);
    if !matches!(entry.status, BackupStatus::Verified | BackupStatus::Interrupted) {
        return Err(restore_not_restorable(&entry));
    }
    if output_path == entry.backup_path {
        return Err(Error::Refused("Restore output cannot be the backup file.".to_owned()));
    }
    if fs::symlink_metadata(&output_path).is_ok() {
        return Err(Error::System(format!(
            "Restore output already exists: {}.",
            output_path.display()
        )));
    }
    let output_directory = output_path
        .parent()
        .ok_or_else(|| Error::Refused("Restore output has no parent directory.".to_owned()))?;
    if !output_directory.is_dir() {
        return Err(Error::Refused(format!(
            "Restore output directory does not exist: {}.",
            output_directory.display()
        )));
    }
    let bytes = fs::read(&entry.backup_path)?;
    if sha256::sha256_hex(&bytes) != entry.source_sha256 {
        return Err(Error::Refused("Backup changed after its last verification.".to_owned()));
    }
    let token = transaction_token();
    let output_name = output_path.file_name().unwrap_or_else(|| OsStr::new("save.sav"));
    let temporary = temporary_sibling(output_directory, output_name, &token, ".tmp");
    files.write_new(&temporary, &bytes)?;
    let publish = files.publish_new(&temporary, &output_path);
    cleanup(files, &temporary);
    publish?;
    if let Err(error) = files.sync_directory(output_directory) {
        return Err(Error::System(format!(
            "Restored output was published at {} but directory sync failed: {error}",
            output_path.display()
        )));
    }
    let read_back = files.read_all(&output_path).map_err(|error| {
        Error::System(format!(
            "Restored output was published at {} but read-back failed: {error}",
            output_path.display()
        ))
    })?;
    if read_back != bytes || sha256::sha256_hex(&read_back) != entry.source_sha256 {
        return Err(Error::System(format!(
            "Restored output at {} did not match its expected bytes.",
            output_path.display()
        )));
    }
    Ok(output_path)
}

fn restore_not_restorable(entry: &BackupEntry) -> Error {
    let status = match entry.status {
        BackupStatus::Verified => "Verified",
        BackupStatus::Missing => "Missing",
        BackupStatus::Interrupted => "Interrupted",
        BackupStatus::Corrupt => "Corrupt",
    };
    let detail = match entry.error.as_deref().unwrap_or("unknown backup error") {
        "backup file is missing" => "Backup file is missing.".to_owned(),
        "backup SHA-256 does not match the journal" => "Backup SHA256 does not match the journal.".to_owned(),
        "journal status is not verified" => "Journal status is not verified.".to_owned(),
        "journal operation is not an object" => "Journal operation must be an object.".to_owned(),
        detail => {
            let mut characters = detail.chars();
            let first = characters
                .next()
                .map_or_else(String::new, |value| value.to_uppercase().collect());
            let mut sentence = first + characters.as_str();
            if !sentence.ends_with('.') {
                sentence.push('.');
            }
            sentence
        }
    };
    Error::Refused(format!("Backup is not restorable ({status}): {detail}"))
}

/// Restores a verified replace/restore backup over its source after checking the current output hash.
///
/// When the source exists, a verified safety backup and `restore` journal are written before the
/// replacement. If read-back validation fails, the pre-restore bytes are restored before returning.
/// A missing source is restored as a new file and has no safety backup.
pub fn restore_in_place(journal_path: &Path) -> Result<RestoreReceipt> {
    let journal_path = absolute_path(journal_path)?;
    let directory = journal_path
        .parent()
        .ok_or_else(|| Error::Refused("backup journal has no parent directory".to_owned()))?;
    let entry = inspect_backup(directory, &journal_path);
    if !matches!(entry.status, BackupStatus::Verified | BackupStatus::Interrupted) {
        return Err(restore_not_restorable(&entry));
    }
    let metadata = read_restore_metadata(&journal_path)?;
    if !matches!(metadata.operation_mode.as_str(), "replace" | "restore") {
        return Err(Error::Refused(
            "Journal does not describe an in-place save replacement.".to_owned(),
        ));
    }
    if metadata.output_path != entry.source_path {
        return Err(Error::Refused(
            "Journal output path does not match its source save.".to_owned(),
        ));
    }
    let source_path = absolute_path(&entry.source_path)?;
    let source_metadata = match fs::symlink_metadata(&source_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(Error::Refused("Restoring a symbolic-link save is refused.".to_owned()));
        }
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if source_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.permissions().readonly())
    {
        return Err(Error::Refused(
            "The save is read-only; clear its read-only attribute before restoring.".to_owned(),
        ));
    }
    let source_directory = source_path
        .parent()
        .ok_or_else(|| Error::Refused("Source save has no parent directory.".to_owned()))?;
    if !source_directory.is_dir() {
        return Err(Error::Refused(format!(
            "Source save directory does not exist: {}.",
            source_directory.display()
        )));
    }
    let restore_bytes = fs::read(&entry.backup_path)?;
    if sha256::sha256_hex(&restore_bytes) != entry.source_sha256 {
        return Err(Error::Refused("Backup changed after its last verification.".to_owned()));
    }
    if source_metadata.is_none() {
        let restored_path = restore_backup(&journal_path, &source_path)?;
        let maintenance_warning = cleanup_interrupted_temps(&entry, &metadata)
            .err()
            .map(|error| error.to_string());
        return Ok(RestoreReceipt {
            save_path: restored_path,
            safety_backup_path: None,
            safety_journal_path: None,
            maintenance_warning,
        });
    }
    let current = fs::read(&source_path)?;
    let current_sha256 = sha256::sha256_hex(&current);
    if current_sha256 == entry.source_sha256 {
        let maintenance_warning = cleanup_interrupted_temps(&entry, &metadata)
            .err()
            .map(|error| error.to_string());
        return Ok(RestoreReceipt {
            save_path: source_path,
            safety_backup_path: None,
            safety_journal_path: None,
            maintenance_warning,
        });
    }
    if current_sha256 != metadata.output_sha256 {
        return Err(Error::Refused(
            "Current save matches neither the original nor journaled output; refusing to overwrite it.".to_owned(),
        ));
    }
    if directory.starts_with(source_directory) {
        return Err(Error::Refused(
            "Backup directory must be outside the save directory. (Parameter 'backupDirectory')".to_owned(),
        ));
    }
    let restore_from = entry.backup_path.clone();
    let (receipt, ()) = replace_with_file_system_and_verifier_operation(
        &StdFileSystem,
        ReplacementRequest::new(&source_path, &metadata.output_sha256, &restore_bytes, directory),
        JournalOperation::Restore(&restore_from),
        |read_back| {
            if read_back == restore_bytes && sha256::sha256_hex(read_back) == entry.source_sha256 {
                Ok(())
            } else {
                Err(Error::System(
                    "Restored save read-back did not match its expected bytes.".to_owned(),
                ))
            }
        },
    )?;
    let mut maintenance_warning = receipt.maintenance_warning;
    if let Err(error) = cleanup_interrupted_temps(&entry, &metadata) {
        append_warning(&mut maintenance_warning, error.to_string());
    }
    if let Err(error) = rotate_verified_backups(directory) {
        append_warning(&mut maintenance_warning, error.to_string());
    }
    Ok(RestoreReceipt {
        save_path: receipt.source_path,
        safety_backup_path: Some(receipt.backup_path),
        safety_journal_path: Some(receipt.journal_path),
        maintenance_warning,
    })
}

fn append_warning(warning: &mut Option<String>, addition: String) {
    match warning {
        Some(warning) => {
            warning.push_str("; ");
            warning.push_str(&addition);
        }
        None => *warning = Some(addition),
    }
}

fn cleanup_interrupted_temps(entry: &BackupEntry, metadata: &RestoreMetadata) -> Result<()> {
    if entry.status != BackupStatus::Interrupted {
        return Ok(());
    }
    let Some(journal_name) = entry.journal_path.file_name().and_then(OsStr::to_str) else {
        return Ok(());
    };
    let Some(artifact_stem) = journal_name.strip_suffix("_ORIGINAL.json") else {
        return Ok(());
    };
    let Some((_, token)) = artifact_stem.rsplit_once('_') else {
        return Ok(());
    };
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Ok(());
    }

    let source_path = absolute_path(&entry.source_path)?;
    let output_path = absolute_path(&metadata.output_path)?;
    let mut changed_directories = HashSet::new();
    let mut errors = Vec::new();

    if let (Some(directory), Some(file_name)) = (output_path.parent(), output_path.file_name()) {
        let temporary = temporary_sibling(directory, file_name, token, ".tmp");
        match remove_matching_temp(&temporary, &metadata.output_sha256) {
            Ok(true) => {
                changed_directories.insert(directory.to_path_buf());
            }
            Ok(false) => {}
            Err(error) => errors.push(error.to_string()),
        }
    }

    if matches!(metadata.operation_mode.as_str(), "replace" | "restore") {
        if let (Some(directory), Some(file_name)) = (source_path.parent(), source_path.file_name()) {
            let temporary = temporary_sibling(directory, file_name, token, ".rollback.tmp");
            match remove_matching_temp(&temporary, &entry.source_sha256) {
                Ok(true) => {
                    changed_directories.insert(directory.to_path_buf());
                }
                Ok(false) => {}
                Err(error) => errors.push(error.to_string()),
            }
        }
    }

    if let Some(directory) = entry.journal_path.parent() {
        let temporary_journal = directory.join(format!(".{artifact_stem}.json.tmp"));
        match fs::symlink_metadata(&temporary_journal) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!("cannot inspect {}: {error}", temporary_journal.display())),
            Ok(_) => {
                let candidate = inspect_backup(directory, &temporary_journal);
                let candidate_metadata = read_restore_metadata(&temporary_journal);
                match candidate_metadata {
                    Ok(candidate_metadata)
                        if candidate.status == BackupStatus::Verified
                            && candidate.source_path == entry.source_path
                            && candidate.source_sha256 == entry.source_sha256
                            && candidate.backup_path == entry.backup_path
                            && candidate_metadata.output_path == metadata.output_path
                            && candidate_metadata.output_sha256 == metadata.output_sha256
                            && candidate_metadata.operation_mode == metadata.operation_mode =>
                    {
                        match remove_file_if_exists(&temporary_journal) {
                            Ok(()) => {
                                invalidate_backup_hash_cache(&temporary_journal);
                                changed_directories.insert(directory.to_path_buf());
                            }
                            Err(error) => errors.push(format!(
                                "cannot remove verified temporary journal {}: {error}",
                                temporary_journal.display()
                            )),
                        }
                    }
                    Ok(_) => errors.push(format!(
                        "left temporary journal {} because it does not match the verified interrupted transaction",
                        temporary_journal.display()
                    )),
                    Err(error) => errors.push(format!(
                        "left temporary journal {} because it could not be verified: {error}",
                        temporary_journal.display()
                    )),
                }
            }
        }
    }

    for directory in changed_directories {
        if let Err(error) = sync_directory(Some(&directory)) {
            errors.push(format!(
                "cannot sync {} after temporary cleanup: {error}",
                directory.display()
            ));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(Error::System(errors.join("; ")))
    }
}

fn remove_matching_temp(path: &Path, expected_sha256: &str) -> Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if !metadata.file_type().is_file() {
        return Err(Error::Refused(format!(
            "left temporary path {} because it is not a regular file",
            path.display()
        )));
    }
    let bytes = fs::read(path)?;
    if sha256::sha256_hex(&bytes) != expected_sha256 {
        return Err(Error::Refused(format!(
            "left temporary file {} because its SHA-256 does not match the interrupted transaction",
            path.display()
        )));
    }
    remove_file_if_exists(path)?;
    invalidate_backup_hash_cache(path);
    Ok(true)
}

/// All inputs required to replace a save in one transaction.
#[derive(Clone, Copy)]
pub struct ReplacementRequest<'a> {
    /// Save path to replace.
    pub source_path: &'a Path,
    /// SHA-256 captured when the edit began.
    pub expected_source_sha256: &'a str,
    /// Complete replacement save bytes.
    pub replacement: &'a [u8],
    /// Directory that stores verified backups and transaction journals.
    pub backup_directory: &'a Path,
    /// Summary recorded in the transaction journal.
    pub summary: EditSummary,
}

impl<'a> ReplacementRequest<'a> {
    /// Creates a replacement request with an empty edit summary.
    #[must_use]
    pub fn new(
        source_path: &'a Path,
        expected_source_sha256: &'a str,
        replacement: &'a [u8],
        backup_directory: &'a Path,
    ) -> Self {
        Self {
            source_path,
            expected_source_sha256,
            replacement,
            backup_directory,
            summary: EditSummary::default(),
        }
    }

    /// Adds the edit summary written to the transaction journal.
    #[must_use]
    pub fn with_summary(mut self, summary: EditSummary) -> Self {
        self.summary = summary;
        self
    }
}

/// Replaces a save after fresh-hash validation, semantic preflight and durable read-back verification.
///
/// The same request and checks are used by production callers and injected filesystem tests.
pub fn replace_transaction<P, V>(
    files: &impl FileSystem,
    request: ReplacementRequest<'_>,
    preflight: impl FnOnce(&[u8], &[u8]) -> Result<P>,
    verify_readback: impl FnOnce(&[u8]) -> Result<V>,
) -> Result<(ReplacementReceipt, P, V)> {
    let (mut receipt, preflight, verified) = replace_with_file_system_and_operation_and_checks(
        files,
        request,
        JournalOperation::Replace(request.summary),
        preflight,
        verify_readback,
    )?;
    receipt.maintenance_warning = files
        .prune_verified_backups(request.backup_directory)
        .err()
        .map(|error| error.to_string());
    Ok((receipt, preflight, verified))
}

fn replace_with_file_system_and_verifier_operation<T>(
    files: &impl FileSystem,
    request: ReplacementRequest<'_>,
    operation: JournalOperation<'_>,
    verify_readback: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<(ReplacementReceipt, T)> {
    replace_with_file_system_and_operation_and_checks(files, request, operation, |_, _| Ok(()), verify_readback)
        .map(|(receipt, (), verified)| (receipt, verified))
}

fn replace_with_file_system_and_operation_and_checks<P, V>(
    files: &impl FileSystem,
    request: ReplacementRequest<'_>,
    operation: JournalOperation<'_>,
    preflight: impl FnOnce(&[u8], &[u8]) -> Result<P>,
    verify_readback: impl FnOnce(&[u8]) -> Result<V>,
) -> Result<(ReplacementReceipt, P, V)> {
    let ReplacementRequest {
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
        summary: _,
    } = request;
    if replacement.is_empty() {
        return Err(Error::Refused("replacement save is empty".to_owned()));
    }
    if expected_source_sha256.len() != 64 || !expected_source_sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(Error::Refused("prepared source hash is not a SHA-256 value".to_owned()));
    }
    let source_path = absolute_path(source_path)?;
    let backup_directory = absolute_path(backup_directory)?;
    let source_directory = source_path
        .parent()
        .ok_or_else(|| Error::Refused("source save has no parent directory".to_owned()))?;
    if files.is_symlink(&source_path)? {
        return Err(Error::Refused(
            "The save is a symbolic link; open the file it points to instead.".to_owned(),
        ));
    }
    if files.is_readonly(&source_path)? {
        return Err(Error::Refused(
            "The save is read-only; clear its read-only attribute before editing.".to_owned(),
        ));
    }
    ensure_backup_directory_outside(source_directory, &backup_directory)?;

    let source_bytes = files.read_all(&source_path)?;
    let source_sha256 = sha256::sha256_hex(&source_bytes);
    if source_sha256 != expected_source_sha256 {
        return Err(Error::Refused(format!(
            "source changed since preparation: expected {expected_source_sha256}, found {source_sha256}"
        )));
    }
    let preflight_value = preflight(&source_bytes, replacement)?;
    let output_sha256 = sha256::sha256_hex(replacement);
    let created_at = timestamp_utc()?;
    let token = transaction_token();
    let stem = artifact_stem_prefix(&source_path);
    let artifact_stem = format!("{stem}_{}_{}", file_timestamp(&created_at)?, token);
    let backup_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.sav"));
    let recovery_path = backup_directory.join(format!("{artifact_stem}_EDITED.sav"));
    let journal_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.json"));
    let file_name = source_path.file_name().unwrap_or_else(|| OsStr::new("save.sav"));
    let temporary_output = temporary_sibling(source_directory, file_name, &token, ".tmp");
    let temporary_journal = backup_directory.join(format!(".{artifact_stem}.json.tmp"));
    let temporary_rollback = temporary_sibling(source_directory, file_name, &token, ".rollback.tmp");
    files.create_dir_all(&backup_directory)?;
    ensure_backup_directory_outside(source_directory, &backup_directory)?;

    let mut source_replaced = false;
    let mut backup_created = false;
    let mut recovery_created = false;
    let mut journal_created = false;
    let transaction = (|| {
        files.write_new(&backup_path, &source_bytes)?;
        backup_created = true;
        files.write_new(&recovery_path, replacement)?;
        recovery_created = true;
        files.write_new(
            &journal_path,
            &serialize_journal(&Journal {
                status: "prepared",
                created_at: &created_at,
                source_path: &source_path,
                source_sha256: &source_sha256,
                output_path: &source_path,
                output_sha256: &output_sha256,
                backup_path: &backup_path,
                recovery_path: &recovery_path,
                operation,
            }),
        )?;
        journal_created = true;
        files.write_new(&temporary_output, replacement)?;

        files.copy_permissions(&source_path, &temporary_output)?;

        // The source is checked again immediately before the rename, after metadata copying, by `replace_if_sha256_matches`.
        files.replace_if_sha256_matches(&temporary_output, &source_path, expected_source_sha256)?;
        source_replaced = true;
        files.sync_directory(source_directory)?;

        let read_back = files.read_all(&source_path)?;
        let read_back_hash = sha256::sha256_hex(&read_back);
        if read_back_hash != output_sha256 || read_back != replacement {
            return Err(Error::System(
                "replacement read-back did not match the prepared bytes".to_owned(),
            ));
        }
        let verified_value = verify_readback(&read_back)?;

        files.write_new(
            &temporary_journal,
            &serialize_journal(&Journal {
                status: "verified",
                created_at: &created_at,
                source_path: &source_path,
                source_sha256: &source_sha256,
                output_path: &source_path,
                output_sha256: &output_sha256,
                backup_path: &backup_path,
                recovery_path: &recovery_path,
                operation,
            }),
        )?;
        files.replace(&temporary_journal, &journal_path)?;
        files.sync_directory(&backup_directory)?;
        Ok(verified_value)
    })();

    let verified_value = match transaction {
        Ok(value) => value,
        Err(error) => {
            if source_replaced {
                let rollback = (|| {
                    let current = files.read_all(&source_path)?;
                    if sha256::sha256_hex(&current) != output_sha256 {
                        return Err(Error::Refused(
                            "rollback refused because the source changed after replacement".to_owned(),
                        ));
                    }
                    files.write_new(&temporary_rollback, &source_bytes)?;
                    files.copy_permissions(&source_path, &temporary_rollback)?;
                    let current = files.read_all(&source_path)?;
                    if sha256::sha256_hex(&current) != output_sha256 {
                        return Err(Error::Refused(
                            "rollback refused because the source changed before restoration".to_owned(),
                        ));
                    }
                    files.replace_if_sha256_matches(&temporary_rollback, &source_path, &output_sha256)?;
                    files.sync_directory(source_directory)
                })();
                if let Err(rollback_error) = rollback {
                    cleanup(files, &temporary_output);
                    cleanup(files, &temporary_journal);
                    cleanup(files, &temporary_rollback);
                    return Err(Error::System(format!(
                        "replacement failed ({error}); restoring the original also failed ({rollback_error})"
                    )));
                }
                match files.read_all(&source_path) {
                    Ok(restored) if restored == source_bytes && sha256::sha256_hex(&restored) == source_sha256 => {}
                    Ok(_) => {
                        cleanup(files, &temporary_output);
                        cleanup(files, &temporary_journal);
                        cleanup(files, &temporary_rollback);
                        return Err(Error::System(format!(
                            "replacement failed ({error}); restoring the original did not pass read-back verification"
                        )));
                    }
                    Err(rollback_error) => {
                        cleanup(files, &temporary_output);
                        cleanup(files, &temporary_journal);
                        cleanup(files, &temporary_rollback);
                        return Err(Error::System(format!(
                        "replacement failed ({error}); restoring the original could not be verified ({rollback_error})"
                    )));
                    }
                }
            }
            cleanup(files, &temporary_output);
            cleanup(files, &temporary_journal);
            cleanup(files, &temporary_rollback);
            if backup_created {
                cleanup(files, &backup_path);
            }
            if recovery_created {
                cleanup(files, &recovery_path);
            }
            if journal_created {
                cleanup(files, &journal_path);
            }
            return Err(error);
        }
    };

    cleanup(files, &temporary_output);
    cleanup(files, &temporary_journal);
    cleanup(files, &temporary_rollback);
    Ok((
        ReplacementReceipt {
            source_path,
            backup_path,
            recovery_path,
            journal_path,
            output_sha256,
            maintenance_warning: None,
        },
        preflight_value,
        verified_value,
    ))
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

fn artifact_stem_prefix(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save".into());
    let mut prefix = String::new();
    for character in stem.chars() {
        if prefix.len().saturating_add(character.len_utf8()) > 80 {
            break;
        }
        prefix.push(character);
    }
    if prefix.is_empty() {
        "save".to_owned()
    } else {
        prefix
    }
}

fn safe_cloud_artifact_stem(remote_leaf: &str) -> String {
    let safe = remote_leaf
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(64)
        .collect::<String>()
        .trim_matches(['.', '_'])
        .to_owned();
    if safe.is_empty() {
        "cloud-save".to_owned()
    } else {
        safe
    }
}

fn temporary_sibling(directory: &Path, file_name: &OsStr, token: &str, suffix: &str) -> PathBuf {
    let mut temporary_name = OsString::from(".");
    temporary_name.push(file_name);
    temporary_name.push(".");
    temporary_name.push(token);
    temporary_name.push(suffix);
    directory.join(temporary_name)
}

fn has_native_ascii_suffix(value: &OsStr, suffix: &str) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().ends_with(suffix.as_bytes())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let value = value.encode_wide().collect::<Vec<_>>();
        let suffix = suffix.encode_utf16().collect::<Vec<_>>();
        value.ends_with(&suffix)
    }
    #[cfg(not(any(unix, windows)))]
    {
        value.to_str().is_some_and(|value| value.ends_with(suffix))
    }
}

fn ensure_backup_directory_outside(source_directory: &Path, backup_directory: &Path) -> Result<()> {
    let source_directory = canonicalize_path_with_missing_tail(source_directory)?;
    let backup_directory = canonicalize_path_with_missing_tail(backup_directory)?;
    if path_starts_with(&backup_directory, &source_directory, cfg!(windows)) {
        return Err(Error::Refused(
            "Backup directory must be outside the selected save directory.".to_owned(),
        ));
    }
    Ok(())
}

fn canonicalize_path_with_missing_tail(path: &Path) -> Result<PathBuf> {
    let absolute = absolute_path(path)?;
    let mut resolved = PathBuf::new();
    let mut has_missing_tail = false;

    for component in absolute.components() {
        match component {
            Component::Prefix(prefix) => resolved.push(prefix.as_os_str()),
            Component::RootDir => resolved.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if resolved.parent().is_some() {
                    resolved.pop();
                }
                if has_missing_tail {
                    match fs::canonicalize(&resolved) {
                        Ok(canonical) => {
                            resolved = canonical;
                            has_missing_tail = false;
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                        Err(error) => return Err(error.into()),
                    }
                }
            }
            Component::Normal(name) => {
                resolved.push(name);
                if !has_missing_tail {
                    match fs::canonicalize(&resolved) {
                        Ok(canonical) => resolved = canonical,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            if fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                                return Err(Error::Refused(
                                    "Backup path contains an unresolved symbolic link.".to_owned(),
                                ));
                            }
                            has_missing_tail = true;
                        }
                        Err(error) => return Err(error.into()),
                    }
                }
            }
        }
    }

    Ok(resolved)
}

fn path_starts_with(path: &Path, prefix: &Path, case_insensitive: bool) -> bool {
    let mut path_components = path.components();
    prefix.components().all(|prefix_component| {
        path_components
            .next()
            .is_some_and(|path_component| path_components_equal(path_component, prefix_component, case_insensitive))
    })
}

fn path_components_equal(left: Component<'_>, right: Component<'_>, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.as_os_str().to_string_lossy().to_lowercase() == right.as_os_str().to_string_lossy().to_lowercase()
    } else {
        left == right
    }
}

fn transaction_token() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    let counter = NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
    let counter = u32::try_from(counter & u64::from(u32::MAX)).unwrap_or_default();
    format!("{nanos:016x}{:08x}{counter:08x}", std::process::id())
}

fn file_timestamp(created_at: &str) -> Result<String> {
    let utc = created_at
        .split_once('+')
        .map(|(timestamp, _)| timestamp)
        .ok_or_else(|| Error::System("journal timestamp has no UTC offset".to_owned()))?;
    let mut result = utc
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == 'T')
        .collect::<String>();
    if result.len() != 22 {
        return Err(Error::System("journal timestamp has an invalid format".to_owned()));
    }
    result.push('Z');
    Ok(result)
}

struct Journal<'a> {
    status: &'a str,
    created_at: &'a str,
    source_path: &'a Path,
    source_sha256: &'a str,
    output_path: &'a Path,
    output_sha256: &'a str,
    backup_path: &'a Path,
    recovery_path: &'a Path,
    operation: JournalOperation<'a>,
}

#[derive(Clone, Copy)]
enum JournalOperation<'a> {
    Replace(EditSummary),
    Restore(&'a Path),
    SteamCloud,
}

struct ExportJournal<'a> {
    status: &'a str,
    created_at: &'a str,
    source_path: &'a Path,
    source_sha256: &'a str,
    output_path: &'a Path,
    output_sha256: &'a str,
    backup_path: &'a Path,
    summary: EditSummary,
}

fn native_path_bytes(path: &Path) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().flat_map(u16::to_le_bytes).collect()
    }
    #[cfg(not(any(unix, windows)))]
    {
        path.to_string_lossy().as_bytes().to_vec()
    }
}

fn native_path_encoding() -> &'static str {
    if cfg!(windows) {
        "w"
    } else {
        "u"
    }
}

fn encode_native_path(path: &Path) -> String {
    let bytes = native_path_bytes(path);
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(2).saturating_add(2));
    encoded.push_str(native_path_encoding());
    encoded.push(':');
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn quote_native_path(path: &Path) -> String {
    json_escape(&encode_native_path(path))
}

fn decode_native_path(encoded: &str) -> Result<PathBuf> {
    let (encoding, hex) = encoded
        .split_once(':')
        .ok_or_else(|| Error::Refused("journal native path encoding is malformed".to_owned()))?;
    let bytes = decode_hex(hex)?;
    #[cfg(unix)]
    if encoding == "u" {
        use std::os::unix::ffi::OsStringExt;
        return Ok(PathBuf::from(OsString::from_vec(bytes)));
    }
    #[cfg(windows)]
    if encoding == "w" {
        use std::os::windows::ffi::OsStringExt;
        if bytes.len() % 2 != 0 {
            return Err(Error::Refused("journal Windows path has an odd byte count".to_owned()));
        }
        let wide = bytes
            .chunks_exact(2)
            .map(|pair| {
                let pair: [u8; 2] = pair
                    .try_into()
                    .map_err(|_| Error::Refused("journal Windows path has an odd byte count".to_owned()))?;
                Ok(u16::from_le_bytes(pair))
            })
            .collect::<Result<Vec<_>>>()?;
        return Ok(PathBuf::from(OsString::from_wide(&wide)));
    }
    #[cfg(not(any(unix, windows)))]
    if encoding == "u" {
        return String::from_utf8(bytes)
            .map(PathBuf::from)
            .map_err(|_| Error::Refused("journal path is not valid UTF-8".to_owned()));
    }
    Err(Error::Refused(format!(
        "journal path encoding {encoding} is unsupported on this platform"
    )))
}

fn decode_hex(hex: &str) -> Result<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return Err(Error::Refused("journal native path hex has an odd length".to_owned()));
    }
    let mut bytes = Vec::with_capacity(hex.len() / 2);
    for pair in hex.as_bytes().chunks_exact(2) {
        let [high, low] = pair else {
            return Err(Error::Refused("journal native path hex pair is malformed".to_owned()));
        };
        let high = hex_value(*high)?;
        let low = hex_value(*low)?;
        bytes.push((high << 4) | low);
    }
    Ok(bytes)
}

fn hex_value(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => byte
            .checked_sub(b'0')
            .ok_or_else(|| Error::Refused("journal native path digit is malformed".to_owned())),
        b'a'..=b'f' => byte
            .checked_sub(b'a')
            .and_then(|value| value.checked_add(10))
            .ok_or_else(|| Error::Refused("journal native path digit is malformed".to_owned())),
        b'A'..=b'F' => byte
            .checked_sub(b'A')
            .and_then(|value| value.checked_add(10))
            .ok_or_else(|| Error::Refused("journal native path digit is malformed".to_owned())),
        _ => Err(Error::Refused("journal native path contains invalid hex".to_owned())),
    }
}

fn path_field(fields: &HashMap<String, TopValue>, display_name: &str, native_name: &str) -> Result<PathBuf> {
    match fields.get(native_name) {
        Some(TopValue::String(value)) => decode_native_path(value),
        Some(_) => Err(Error::Refused(format!("journal field {native_name} is not text"))),
        None => Ok(PathBuf::from(field_string(fields, display_name)?)),
    }
}

#[derive(Clone, PartialEq, Eq)]
struct FileFingerprint {
    length: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_seconds: i64,
    #[cfg(unix)]
    changed_nanoseconds: i64,
    #[cfg(windows)]
    creation_time: u64,
    #[cfg(windows)]
    last_write_time: u64,
}

struct CachedBackupHash {
    fingerprint: FileFingerprint,
    sha256: String,
}

static BACKUP_HASH_CACHE: OnceLock<Mutex<HashMap<PathBuf, CachedBackupHash>>> = OnceLock::new();
const MAXIMUM_CACHED_BACKUP_HASHES: usize = 512;

fn file_fingerprint(metadata: &fs::Metadata) -> FileFingerprint {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        FileFingerprint {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            device: metadata.dev(),
            inode: metadata.ino(),
            changed_seconds: metadata.ctime(),
            changed_nanoseconds: metadata.ctime_nsec(),
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        FileFingerprint {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            creation_time: metadata.creation_time(),
            last_write_time: metadata.last_write_time(),
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        FileFingerprint {
            length: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

fn regular_file_fingerprint(path: &Path) -> std::io::Result<FileFingerprint> {
    let before = fs::symlink_metadata(path)?;
    if before.file_type().is_symlink() || !before.is_file() {
        return Err(std::io::Error::other("path is not a regular file"));
    }
    Ok(file_fingerprint(&before))
}

fn hash_file_with_fingerprint(path: &Path, fingerprint: &FileFingerprint) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = sha256::Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        let Some(chunk) = buffer.get(..length) else {
            return Err(std::io::Error::other("file read exceeded its buffer"));
        };
        hasher.update(chunk);
    }
    let after = fs::symlink_metadata(path)?;
    if after.file_type().is_symlink() || !after.is_file() || file_fingerprint(&after) != *fingerprint {
        return Err(std::io::Error::other("file changed while it was being verified"));
    }
    Ok(hasher.finalize_hex())
}

fn fresh_file_sha256(path: &Path) -> std::io::Result<String> {
    let fingerprint = regular_file_fingerprint(path)?;
    hash_file_with_fingerprint(path, &fingerprint)
}

fn cached_file_sha256(path: &Path) -> std::io::Result<String> {
    let fingerprint = regular_file_fingerprint(path)?;
    let cache = BACKUP_HASH_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(cached) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(path)
        .filter(|cached| cached.fingerprint == fingerprint)
    {
        return Ok(cached.sha256.clone());
    }

    let digest = hash_file_with_fingerprint(path, &fingerprint)?;
    let mut cache = cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if cache.len() >= MAXIMUM_CACHED_BACKUP_HASHES {
        cache.clear();
    }
    cache.insert(
        path.to_path_buf(),
        CachedBackupHash {
            fingerprint,
            sha256: digest.clone(),
        },
    );
    Ok(digest)
}

fn invalidate_backup_hash_cache(path: &Path) {
    if let Some(cache) = BACKUP_HASH_CACHE.get() {
        cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(path);
    }
}

fn serialize_export_journal(journal: &ExportJournal<'_>) -> Vec<u8> {
    use std::fmt::Write as _;

    let created_at = json_escape(journal.created_at);
    let source_path = json_escape(&journal.source_path.to_string_lossy());
    let output_path = json_escape(&journal.output_path.to_string_lossy());
    let backup_path = json_escape(&journal.backup_path.to_string_lossy());
    let source_path_native = quote_native_path(journal.source_path);
    let output_path_native = quote_native_path(journal.output_path);
    let backup_path_native = quote_native_path(journal.backup_path);
    let money = journal
        .summary
        .money
        .map_or_else(|| "null".to_owned(), |value| value.to_string());
    let mut encoded = String::new();
    let _ = write!(
        encoded,
        "{{\"version\":1,\"status\":\"{}\",\"created_at\":\"{created_at}\",\"source_path\":\"{source_path}\",\"source_path_native\":\"{source_path_native}\",\"source_sha256\":\"{}\",\"output_path\":\"{output_path}\",\"output_path_native\":\"{output_path_native}\",\"output_sha256\":\"{}\",\"backup_path\":\"{backup_path}\",\"backup_path_native\":\"{backup_path_native}\",\"operation\":{{\"mode\":\"export\",\"money\":{money},\"stack_count\":{},\"move_count\":{},\"detach_count\":{},\"attach_count\":{},\"raw_count\":{},\"add_count\":{},\"durability_count\":{},\"upgrade_count\":{},\"relation_count\":{},\"player_faction\":{}",
        journal.status,
        journal.source_sha256,
        journal.output_sha256,
        journal.summary.stack_count,
        journal.summary.move_count,
        journal.summary.detach_count,
        journal.summary.attach_count,
        journal.summary.raw_count,
        journal.summary.add_count,
        journal.summary.durability_count,
        journal.summary.upgrade_count,
        journal.summary.relation_count,
        journal.summary.player_faction,
    );
    encoded.push_str("}}");
    encoded.into_bytes()
}

struct ParsedBackupJournal {
    source_path: PathBuf,
    output_path: PathBuf,
    output_sha256: String,
    backup_path: PathBuf,
    source_sha256: String,
    journal_status: String,
    operation_mode: Option<String>,
}

fn inspect_backup(directory: &Path, journal_path: &Path) -> BackupEntry {
    let corrupt = |message: String,
                   backup_path: Option<PathBuf>,
                   source_path: Option<PathBuf>,
                   sha256: Option<String>| BackupEntry {
        journal_path: journal_path.to_path_buf(),
        backup_path: backup_path.unwrap_or_else(|| journal_path.with_extension("sav")),
        source_path: source_path.unwrap_or_default(),
        source_sha256: sha256.unwrap_or_default(),
        status: BackupStatus::Corrupt,
        operation_mode: None,
        error: Some(message),
    };
    if fs::symlink_metadata(journal_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return corrupt("journal is a symbolic link".to_owned(), None, None, None);
    }
    if fs::metadata(journal_path).is_ok_and(|metadata| metadata.len() > MAXIMUM_JOURNAL_BYTES) {
        return corrupt("journal exceeds the 2 MiB size limit".to_owned(), None, None, None);
    }
    let bytes = match fs::read(journal_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return corrupt("journal is missing".to_owned(), None, None, None)
        }
        Err(error) => return corrupt(format!("journal cannot be read: {error}"), None, None, None),
    };
    let fields = match parse_top_fields(&bytes) {
        Ok(fields) => fields,
        Err(error) => return corrupt(error.to_string(), None, None, None),
    };
    let parsed = (|| -> Result<ParsedBackupJournal> {
        if field_u64(&fields, "version")? != 1 {
            return Err(Error::Refused("unsupported journal version".to_owned()));
        }
        let created_at = field_string(&fields, "created_at")?;
        let output_path = path_field(&fields, "output_path", "output_path_native")?;
        let output_sha256 = field_string(&fields, "output_sha256")?;
        if created_at.is_empty() || output_path.as_os_str().is_empty() || !valid_sha256(&output_sha256) {
            return Err(Error::Refused(
                "journal creation or output fields are malformed".to_owned(),
            ));
        }
        if !matches!(fields.get("operation"), Some(TopValue::Object)) {
            return Err(Error::Refused("journal operation is not an object".to_owned()));
        }
        let operation_mode = parse_operation_mode(&bytes).ok();
        let journal_status = field_string(&fields, "status")?;
        if !matches!(journal_status.as_str(), "verified" | "prepared") {
            return Err(Error::Refused("journal status is not recognized".to_owned()));
        }
        let source_path = path_field(&fields, "source_path", "source_path_native")?;
        let source_sha256 = field_string(&fields, "source_sha256")?;
        if !valid_sha256(&source_sha256) {
            return Err(Error::Refused("journal source SHA-256 is malformed".to_owned()));
        }
        let backup_value = path_field(&fields, "backup_path", "backup_path_native")?;
        let backup_path = if backup_value.is_absolute() {
            backup_value
        } else {
            directory.join(backup_value)
        };
        if backup_path.parent() != Some(directory) {
            return Err(Error::Refused(
                "journal backup path is outside its backup directory".to_owned(),
            ));
        }
        Ok(ParsedBackupJournal {
            source_path,
            output_path,
            output_sha256,
            backup_path,
            source_sha256,
            journal_status,
            operation_mode,
        })
    })();
    let ParsedBackupJournal {
        source_path,
        output_path,
        output_sha256,
        backup_path,
        source_sha256,
        journal_status,
        operation_mode,
    } = match parsed {
        Ok(values) => values,
        Err(error) => return corrupt(error.to_string(), None, None, None),
    };
    if fs::symlink_metadata(&backup_path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return corrupt(
            "backup is a symbolic link".to_owned(),
            Some(backup_path),
            Some(source_path),
            Some(source_sha256),
        );
    }
    let actual = match cached_file_sha256(&backup_path) {
        Ok(actual) => actual,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return BackupEntry {
                journal_path: journal_path.to_path_buf(),
                backup_path,
                source_path,
                source_sha256,
                status: BackupStatus::Missing,
                operation_mode,
                error: Some("backup file is missing".to_owned()),
            }
        }
        Err(error) => {
            return corrupt(
                format!("backup cannot be read: {error}"),
                Some(backup_path),
                Some(source_path),
                Some(source_sha256),
            )
        }
    };
    if actual != source_sha256 {
        return corrupt(
            "backup SHA-256 does not match the journal".to_owned(),
            Some(backup_path),
            Some(source_path),
            Some(source_sha256),
        );
    }
    let status = if journal_status == "prepared"
        && !prepared_write_is_resolved(
            &bytes,
            journal_path,
            &source_path,
            &output_path,
            &source_sha256,
            &output_sha256,
        ) {
        BackupStatus::Interrupted
    } else {
        BackupStatus::Verified
    };
    BackupEntry {
        journal_path: journal_path.to_path_buf(),
        backup_path,
        source_path,
        source_sha256,
        status,
        operation_mode,
        error: (status == BackupStatus::Interrupted)
            .then(|| "prepared transaction needs recovery; the original backup passed SHA-256 verification".to_owned()),
    }
}

fn prepared_write_is_resolved(
    journal: &[u8],
    journal_path: &Path,
    source_path: &Path,
    output_path: &Path,
    source_sha256: &str,
    output_sha256: &str,
) -> bool {
    let Ok(operation_mode) = parse_operation_mode(journal) else {
        return false;
    };
    if !matches!(operation_mode.as_str(), "replace" | "restore") {
        return false;
    }
    let Ok(source_path) = absolute_path(source_path) else {
        return false;
    };
    let Ok(output_path) = absolute_path(output_path) else {
        return false;
    };
    if source_path != output_path {
        return false;
    }
    let Ok(current_sha256) = fresh_file_sha256(&source_path) else {
        return false;
    };
    let resolved_hash =
        current_sha256 == source_sha256 || (operation_mode == "restore" && current_sha256 == output_sha256);
    resolved_hash && !prepared_transaction_has_temporary_artifacts(journal_path, &source_path, &output_path)
}

fn prepared_transaction_has_temporary_artifacts(journal_path: &Path, source_path: &Path, output_path: &Path) -> bool {
    let Some(journal_name) = journal_path.file_name().and_then(OsStr::to_str) else {
        return true;
    };
    let Some(artifact_stem) = journal_name.strip_suffix("_ORIGINAL.json") else {
        return true;
    };
    let Some((_, token)) = artifact_stem.rsplit_once('_') else {
        return true;
    };
    if token.len() != 32 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return true;
    }

    let Some(source_directory) = source_path.parent() else {
        return true;
    };
    let Some(source_name) = source_path.file_name() else {
        return true;
    };
    let Some(output_directory) = output_path.parent() else {
        return true;
    };
    let Some(output_name) = output_path.file_name() else {
        return true;
    };
    let Some(journal_directory) = journal_path.parent() else {
        return true;
    };

    let temporaries = [
        temporary_sibling(output_directory, output_name, token, ".tmp"),
        temporary_sibling(source_directory, source_name, token, ".rollback.tmp"),
        journal_directory.join(format!(".{artifact_stem}.json.tmp")),
    ];
    temporaries.iter().any(|path| match fs::symlink_metadata(path) {
        Ok(_) => true,
        Err(error) => error.kind() != std::io::ErrorKind::NotFound,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TopValue {
    String(String),
    Number(String),
    Object,
    Array,
    Other,
}

struct RestoreMetadata {
    output_path: PathBuf,
    output_sha256: String,
    operation_mode: String,
}

fn parse_top_fields(bytes: &[u8]) -> Result<HashMap<String, TopValue>> {
    use sse_codecs::json::{Event, Reader, Text};

    let mut reader = Reader::new(bytes);
    if !matches!(reader.next_event()?, Some(Event::ObjectStart)) {
        return Err(Error::Refused("journal root is not a JSON object".to_owned()));
    }
    let mut fields = HashMap::new();
    loop {
        match reader
            .next_event()?
            .ok_or_else(|| Error::Refused("journal object is truncated".to_owned()))?
        {
            Event::ObjectEnd => break,
            Event::Key(key) => {
                let key = match key {
                    Text::Borrowed(value) => value.to_owned(),
                    Text::Owned(value) => value,
                };
                let value = reader
                    .next_event()?
                    .ok_or_else(|| Error::Refused("journal value is missing".to_owned()))?;
                let parsed = match value {
                    Event::String(value) => TopValue::String(value.into_owned()),
                    Event::Number(value) => TopValue::Number(value.to_owned()),
                    Event::ObjectStart => {
                        skip_json_container(&mut reader)?;
                        TopValue::Object
                    }
                    Event::ArrayStart => {
                        skip_json_container(&mut reader)?;
                        TopValue::Array
                    }
                    Event::Bool(_) | Event::Null => TopValue::Other,
                    Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => {
                        return Err(Error::Refused("journal has an invalid top-level value".to_owned()));
                    }
                };
                if fields.insert(key.clone(), parsed).is_some() {
                    return Err(Error::Refused(format!("journal repeats the {key} field")));
                }
            }
            _ => return Err(Error::Refused("journal has an invalid top-level member".to_owned())),
        }
    }
    if reader.next_event()?.is_some() {
        return Err(Error::Refused("journal has trailing data".to_owned()));
    }
    Ok(fields)
}

fn skip_json_container(reader: &mut sse_codecs::json::Reader<'_>) -> Result<()> {
    use sse_codecs::json::Event;

    let mut depth = 1_usize;
    while depth != 0 {
        match reader
            .next_event()?
            .ok_or_else(|| Error::Refused("journal container is truncated".to_owned()))?
        {
            Event::ObjectStart | Event::ArrayStart => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| Error::Refused("journal nesting overflow".to_owned()))?;
            }
            Event::ObjectEnd | Event::ArrayEnd => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::Refused("journal nesting underflow".to_owned()))?;
            }
            Event::Key(_) | Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => {}
        }
    }
    Ok(())
}

fn field_string(fields: &HashMap<String, TopValue>, name: &str) -> Result<String> {
    match fields.get(name) {
        Some(TopValue::String(value)) => Ok(value.clone()),
        _ => Err(Error::Refused(format!("journal field {name} is missing or not text"))),
    }
}

fn field_u64(fields: &HashMap<String, TopValue>, name: &str) -> Result<u64> {
    match fields.get(name) {
        Some(TopValue::Number(value)) => value
            .parse::<u64>()
            .map_err(|_| Error::Refused(format!("journal field {name} is not an unsigned integer"))),
        _ => Err(Error::Refused(format!(
            "journal field {name} is missing or not an integer"
        ))),
    }
}

fn read_restore_metadata(journal_path: &Path) -> Result<RestoreMetadata> {
    let bytes = fs::read(journal_path)?;
    let fields = parse_top_fields(&bytes)?;
    let output_path = path_field(&fields, "output_path", "output_path_native")?;
    let output_sha256 = field_string(&fields, "output_sha256")?;
    if !valid_sha256(&output_sha256) {
        return Err(Error::Refused("journal output SHA-256 is malformed".to_owned()));
    }
    let operation_mode = parse_operation_mode(&bytes)?;
    Ok(RestoreMetadata {
        output_path,
        output_sha256,
        operation_mode,
    })
}

fn parse_operation_mode(bytes: &[u8]) -> Result<String> {
    use sse_codecs::json::{Event, Reader};

    let mut reader = Reader::new(bytes);
    if !matches!(reader.next_event()?, Some(Event::ObjectStart)) {
        return Err(Error::Refused("journal root is not a JSON object".to_owned()));
    }
    loop {
        let event = reader
            .next_event()?
            .ok_or_else(|| Error::Refused("journal object is truncated".to_owned()))?;
        let Event::Key(key) = event else {
            if event == Event::ObjectEnd {
                return Err(Error::Refused("journal operation is missing".to_owned()));
            }
            return Err(Error::Refused("journal has an invalid top-level member".to_owned()));
        };
        let key = key.into_owned();
        let value = reader
            .next_event()?
            .ok_or_else(|| Error::Refused("journal value is missing".to_owned()))?;
        if key == "operation" {
            if value != Event::ObjectStart {
                return Err(Error::Refused("journal operation is not an object".to_owned()));
            }
            let mut mode = None;
            loop {
                match reader
                    .next_event()?
                    .ok_or_else(|| Error::Refused("journal operation is truncated".to_owned()))?
                {
                    Event::ObjectEnd => break,
                    Event::Key(name) => {
                        let name = name.into_owned();
                        let value = reader
                            .next_event()?
                            .ok_or_else(|| Error::Refused("journal operation value is missing".to_owned()))?;
                        if name == "mode" {
                            if mode.is_some() {
                                return Err(Error::Refused("journal operation repeats the mode field".to_owned()));
                            }
                            let Event::String(value) = value else {
                                return Err(Error::Refused("journal operation mode is not text".to_owned()));
                            };
                            mode = Some(value.into_owned());
                        } else {
                            skip_json_value(&mut reader, value)?;
                        }
                    }
                    _ => return Err(Error::Refused("journal operation has an invalid member".to_owned())),
                }
            }
            return mode.ok_or_else(|| Error::Refused("journal operation mode is missing".to_owned()));
        }
        skip_json_value(&mut reader, value)?;
    }
}

fn skip_json_value(reader: &mut sse_codecs::json::Reader<'_>, value: sse_codecs::json::Event<'_>) -> Result<()> {
    use sse_codecs::json::Event;

    match value {
        Event::ObjectStart | Event::ArrayStart => skip_json_container(reader),
        Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => Ok(()),
        Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => {
            Err(Error::Refused("journal has an invalid JSON value".to_owned()))
        }
    }
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn serialize_journal(journal: &Journal<'_>) -> Vec<u8> {
    let source_path = json_escape(&journal.source_path.to_string_lossy());
    let output_path = json_escape(&journal.output_path.to_string_lossy());
    let backup_path = json_escape(&journal.backup_path.to_string_lossy());
    let recovery_path = json_escape(&journal.recovery_path.to_string_lossy());
    let source_path_native = quote_native_path(journal.source_path);
    let output_path_native = quote_native_path(journal.output_path);
    let backup_path_native = quote_native_path(journal.backup_path);
    let recovery_path_native = quote_native_path(journal.recovery_path);
    let created_at = json_escape(journal.created_at);
    let operation = match journal.operation {
        JournalOperation::Replace(summary) => {
            let money = summary
                .money
                .map_or_else(|| "null".to_owned(), |value| value.to_string());
            format!(
                "{{\"mode\":\"replace\",\"money\":{money},\"stack_count\":{},\"move_count\":{},\"detach_count\":{},\"attach_count\":{},\"raw_count\":{},\"add_count\":{},\"durability_count\":{},\"upgrade_count\":{},\"relation_count\":{},\"player_faction\":{}}}",
                summary.stack_count,
                summary.move_count,
                summary.detach_count,
                summary.attach_count,
                summary.raw_count,
                summary.add_count,
                summary.durability_count,
                summary.upgrade_count,
                summary.relation_count,
                summary.player_faction,
            )
        }
        JournalOperation::Restore(restore_from) => {
            let restore_from_display = json_escape(&restore_from.to_string_lossy());
            let restore_from_native = quote_native_path(restore_from);
            format!(
                "{{\"mode\":\"restore\",\"restore_from\":\"{restore_from_display}\",\"restore_from_native\":\"{restore_from_native}\",\"money\":null,\"stack_count\":0,\"move_count\":0,\"detach_count\":0,\"attach_count\":0,\"raw_count\":0,\"add_count\":0,\"durability_count\":0,\"upgrade_count\":0,\"relation_count\":0,\"player_faction\":false}}"
            )
        }
        JournalOperation::SteamCloud => "{\"mode\":\"steam-cloud\"}".to_owned(),
    };
    format!(
        "{{\"version\":1,\"status\":\"{}\",\"created_at\":\"{created_at}\",\"source_path\":\"{source_path}\",\"source_path_native\":\"{source_path_native}\",\"source_sha256\":\"{}\",\"output_path\":\"{output_path}\",\"output_path_native\":\"{output_path_native}\",\"output_sha256\":\"{}\",\"backup_path\":\"{backup_path}\",\"backup_path_native\":\"{backup_path_native}\",\"recovery_path\":\"{recovery_path}\",\"recovery_path_native\":\"{recovery_path_native}\",\"operation\":{operation}}}",
        journal.status, journal.source_sha256, journal.output_sha256
    )
    .into_bytes()
}

fn json_escape(value: &str) -> String {
    use std::fmt::Write as _;

    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            value if value <= '\u{001F}' => {
                let _ = write!(escaped, "\\u{:04x}", u32::from(value));
            }
            value => escaped.push(value),
        }
    }
    escaped
}

fn timestamp_utc() -> Result<String> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::System("system clock is before the Unix epoch".to_owned()))?;
    let seconds = duration.as_secs();
    let mut remaining_days = seconds / 86_400;
    let seconds_in_day = seconds.checked_rem(86_400).unwrap_or_default();
    let mut year = 1970_u64;
    loop {
        if year > 9999 {
            return Err(Error::System(
                "system clock is outside the journal date range".to_owned(),
            ));
        }
        let leap =
            year.checked_rem(400) == Some(0) || (year.checked_rem(4) == Some(0) && year.checked_rem(100) != Some(0));
        let year_days = if leap { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        remaining_days = remaining_days
            .checked_sub(year_days)
            .ok_or_else(|| Error::System("journal date calculation underflow".to_owned()))?;
        year = year
            .checked_add(1)
            .ok_or_else(|| Error::System("journal year overflow".to_owned()))?;
    }
    let leap = year.checked_rem(400) == Some(0) || (year.checked_rem(4) == Some(0) && year.checked_rem(100) != Some(0));
    let mut month = 1_u64;
    loop {
        let month_days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return Err(Error::System("journal month calculation overflow".to_owned())),
        };
        if remaining_days < month_days {
            break;
        }
        remaining_days = remaining_days
            .checked_sub(month_days)
            .ok_or_else(|| Error::System("journal date calculation underflow".to_owned()))?;
        month = month
            .checked_add(1)
            .ok_or_else(|| Error::System("journal month overflow".to_owned()))?;
    }
    let day = remaining_days
        .checked_add(1)
        .ok_or_else(|| Error::System("journal day overflow".to_owned()))?;
    let hour = seconds_in_day / 3_600;
    let minute = seconds_in_day.checked_rem(3_600).unwrap_or_default() / 60;
    let second = seconds_in_day.checked_rem(60).unwrap_or_default();
    let fraction = duration.subsec_nanos() / 100;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{fraction:07}+00:00"
    ))
}

fn cleanup(files: &impl FileSystem, path: &Path) {
    let _ = files.delete_if_exists(path);
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn sync_directory(directory: Option<&Path>) -> Result<()> {
    #[cfg(unix)]
    if let Some(directory) = directory {
        File::open(directory)?.sync_all()?;
    }
    #[cfg(not(unix))]
    let _ = directory;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{restore_backup_with_file_system, FileSystem, ReplacementReceipt, ReplacementRequest, StdFileSystem};
    use sse_core::{Error, Result};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn replace_with_file_system(
        files: &impl FileSystem,
        source: &Path,
        expected_sha256: &str,
        replacement: &[u8],
        backups: &Path,
    ) -> Result<ReplacementReceipt> {
        replace_with_file_system_and_verifier(files, source, expected_sha256, replacement, backups, |_| Ok(()))
            .map(|(receipt, ())| receipt)
    }

    fn replace_with_file_system_and_verifier<T>(
        files: &impl FileSystem,
        source: &Path,
        expected_sha256: &str,
        replacement: &[u8],
        backups: &Path,
        verify_readback: impl FnOnce(&[u8]) -> Result<T>,
    ) -> Result<(ReplacementReceipt, T)> {
        let (receipt, (), verified) = super::replace_transaction(
            files,
            ReplacementRequest::new(source, expected_sha256, replacement, backups),
            |_, _| Ok(()),
            verify_readback,
        )?;
        Ok((receipt, verified))
    }

    fn replace_std_transaction(
        source: &Path,
        expected_sha256: &str,
        replacement: &[u8],
        backups: &Path,
    ) -> Result<ReplacementReceipt> {
        replace_with_file_system(&StdFileSystem, source, expected_sha256, replacement, backups)
    }

    #[cfg(unix)]
    #[test]
    fn unix_native_path_encoding_roundtrips_non_utf8_bytes_without_filesystem_access() -> TestResult {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(std::ffi::OsString::from_vec(b"slot-\xff.sav".to_vec()));
        let encoded = super::encode_native_path(&path);

        assert_eq!(encoded, "u:736c6f742dff2e736176");
        assert_eq!(super::decode_native_path(&encoded)?, path);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn export_journal_paths_roundtrip_non_utf8_without_filesystem_access() -> TestResult {
        use std::os::unix::ffi::OsStringExt;

        let source = PathBuf::from(std::ffi::OsString::from_vec(b"source-\xff.sav".to_vec()));
        let output = PathBuf::from(std::ffi::OsString::from_vec(b"edited-\xfe.sav".to_vec()));
        let backup = PathBuf::from(std::ffi::OsString::from_vec(b"backup-\x80.sav".to_vec()));
        let sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let encoded = super::serialize_export_journal(&super::ExportJournal {
            status: "verified",
            created_at: "2026-10-08T00:00:00.0000000+00:00",
            source_path: &source,
            source_sha256: sha256,
            output_path: &output,
            output_sha256: sha256,
            backup_path: &backup,
            summary: super::EditSummary::default(),
        });
        let fields = super::parse_top_fields(&encoded)?;

        assert_eq!(super::path_field(&fields, "source_path", "source_path_native")?, source);
        assert_eq!(super::path_field(&fields, "output_path", "output_path_native")?, output);
        assert_eq!(super::path_field(&fields, "backup_path", "backup_path_native")?, backup);
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn windows_native_path_decoder_preserves_utf16_code_units() -> TestResult {
        use std::os::windows::ffi::OsStrExt;

        let decoded = super::decode_native_path("w:41003dd800de")?;
        let code_units = decoded.as_os_str().encode_wide().collect::<Vec<_>>();

        assert_eq!(code_units, [0x0041_u16, 0xd83d, 0xde00]);
        assert!(super::decode_native_path("w:41").is_err());
        Ok(())
    }

    #[test]
    fn replaces_and_records_a_synthetic_save() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let receipt = replace_with_file_system(&fs, &source, &source_hash, output_bytes, &backup_directory)?;

        assert!(receipt.maintenance_warning.is_none());
        assert_eq!(fs.bytes(&source).as_deref(), Some(output_bytes.as_slice()));
        assert_eq!(fs.bytes(&receipt.backup_path).as_deref(), Some(source_bytes.as_slice()));
        assert_eq!(fs.backup_prune_calls(), 1);
        let journal = fs
            .bytes(&receipt.journal_path)
            .ok_or_else(|| std::io::Error::other("journal should exist"))?;
        let journal = std::str::from_utf8(&journal)?;
        assert!(journal.contains("\"status\":\"verified\""));
        assert!(journal.contains(&format!("\"source_sha256\":\"{source_hash}\"")));
        assert_eq!(fs.operation_count(), 12);
        Ok(())
    }

    #[test]
    fn a_stale_source_is_refused_before_any_file_is_written() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        let wrong_hash = "00".repeat(32);

        assert!(replace_with_file_system(&fs, &source, &wrong_hash, output_bytes, &backup_directory).is_err());
        assert_eq!(fs.bytes(&source).as_deref(), Some(source_bytes.as_slice()));
        assert_eq!(fs.operation_count(), 2);
        Ok(())
    }

    #[test]
    fn read_only_sources_are_refused_before_transaction_artifacts_are_created() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        fs.readonly.set(true);

        let result = replace_with_file_system(
            &fs,
            &source,
            &sse_codecs::sha256::sha256_hex(source_bytes),
            output_bytes,
            &backup_directory,
        );

        assert!(matches!(result, Err(Error::Refused(_))));
        assert_eq!(fs.files.borrow().len(), 1);
        assert_eq!(fs.bytes(&source).as_deref(), Some(source_bytes.as_slice()));
        Ok(())
    }

    #[test]
    fn every_transaction_step_failure_keeps_the_source_bytes() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();

        for failure_step in 1..=12 {
            let fs = MemoryFs::new(&source, source_bytes, Some(failure_step));
            assert!(replace_with_file_system(&fs, &source, &source_hash, output_bytes, &backup_directory).is_err());
            assert_eq!(
                fs.bytes(&source).as_deref(),
                Some(source_bytes.as_slice()),
                "step {failure_step}"
            );
            assert_eq!(fs.files.borrow().len(), 1, "failed artifacts at step {failure_step}");
        }
        Ok(())
    }

    #[test]
    fn preflight_rejection_happens_before_any_transaction_artifact() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        let readback_called = Cell::new(false);

        let request = super::ReplacementRequest::new(&source, &source_hash, replacement, &backup_directory)
            .with_summary(super::EditSummary {
                money: Some(123_456),
                ..super::EditSummary::default()
            });
        let result = super::replace_transaction(
            &fs,
            request,
            |original, prepared| {
                assert_eq!(original, source_bytes);
                assert_eq!(prepared, replacement);
                Err::<(), Error>(Error::damaged("injected preflight verification failure"))
            },
            |_| {
                readback_called.set(true);
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!readback_called.get());
        assert_eq!(fs.bytes(&source).as_deref(), Some(source_bytes.as_slice()));
        assert_eq!(fs.files.borrow().len(), 1);
        assert_eq!(fs.operation_count(), 2);
        Ok(())
    }

    #[test]
    fn replacement_journal_records_the_full_edit_summary() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        let summary = super::EditSummary {
            money: Some(12_345),
            stack_count: 2,
            move_count: 3,
            detach_count: 4,
            attach_count: 5,
            raw_count: 6,
            add_count: 7,
            durability_count: 8,
            upgrade_count: 9,
            relation_count: 10,
            player_faction: true,
        };

        let (receipt, (), ()) = super::replace_transaction(
            &fs,
            super::ReplacementRequest::new(&source, &source_hash, replacement, &backup_directory).with_summary(summary),
            |_, _| Ok(()),
            |_| Ok(()),
        )?;

        let journal = fs
            .bytes(&receipt.journal_path)
            .ok_or_else(|| std::io::Error::other("journal should exist"))?;
        let journal = std::str::from_utf8(&journal)?;
        for field in [
            "\"money\":12345",
            "\"stack_count\":2",
            "\"move_count\":3",
            "\"detach_count\":4",
            "\"attach_count\":5",
            "\"raw_count\":6",
            "\"add_count\":7",
            "\"durability_count\":8",
            "\"upgrade_count\":9",
            "\"relation_count\":10",
            "\"player_faction\":true",
        ] {
            assert!(journal.contains(field), "journal must contain {field}");
        }
        Ok(())
    }

    #[test]
    fn semantic_readback_failure_rolls_back_and_cleans_artifacts() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);

        let result = replace_with_file_system_and_verifier(
            &fs,
            &source,
            &source_hash,
            output_bytes,
            &backup_directory,
            |read_back| {
                assert_eq!(read_back, output_bytes);
                Err::<(), Error>(Error::damaged("injected semantic verification failure"))
            },
        );

        assert!(result.is_err());
        assert_eq!(fs.bytes(&source).as_deref(), Some(source_bytes.as_slice()));
        assert_eq!(fs.files.borrow().len(), 1);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replacement_preserves_unix_mode_and_owner_bits() -> TestResult {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        let unique = format!("sse-storage-permissions-{}", std::process::id());
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&backups)?;
        let source = saves.join("save.sav");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, source_bytes)?;
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o640))?;
        let source_metadata = std::fs::metadata(&source)?;
        let source_uid = source_metadata.uid();
        let source_gid = source_metadata.gid();
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let failed =
            replace_with_file_system_and_verifier(&StdFileSystem, &source, &source_hash, replacement, &backups, |_| {
                Err::<(), Error>(Error::damaged("injected semantic verification failure"))
            });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&source)?, source_bytes);
        let original_mode = std::fs::metadata(&source)?.permissions().mode() & 0o777;
        assert_eq!(original_mode, 0o640);
        let original_metadata = std::fs::metadata(&source)?;
        assert_eq!(original_metadata.uid(), source_uid);
        assert_eq!(original_metadata.gid(), source_gid);
        assert!(std::fs::read_dir(&backups)?.next().is_none());
        assert!(std::fs::read_dir(&saves)?.all(|entry| { entry.is_ok_and(|entry| entry.file_name() == "save.sav") }));

        let receipt = replace_std_transaction(&source, &source_hash, replacement, &backups)?;

        let mode = std::fs::metadata(&source)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        let replaced_metadata = std::fs::metadata(&source)?;
        assert_eq!(replaced_metadata.uid(), source_uid);
        assert_eq!(replaced_metadata.gid(), source_gid);
        assert_eq!(std::fs::read(&source)?, replacement);
        for artifact in [&receipt.backup_path, &receipt.recovery_path, &receipt.journal_path] {
            assert_eq!(std::fs::metadata(artifact)?.permissions().mode() & 0o777, 0o600);
        }
        assert!(receipt
            .backup_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("_ORIGINAL.sav")));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn directory_sync_errors_are_returned() -> TestResult {
        let path = std::env::temp_dir().join(format!("sse-missing-sync-{}", std::process::id()));
        assert!(StdFileSystem.sync_directory(&path).is_err());
        Ok(())
    }

    struct FailDirectorySync;

    impl FileSystem for FailDirectorySync {
        fn read_all(&self, path: &Path) -> Result<Vec<u8>> {
            StdFileSystem.read_all(path)
        }

        fn is_symlink(&self, path: &Path) -> Result<bool> {
            StdFileSystem.is_symlink(path)
        }

        fn create_dir_all(&self, path: &Path) -> Result<()> {
            StdFileSystem.create_dir_all(path)
        }

        fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
            StdFileSystem.write_new(path, bytes)
        }

        fn copy_permissions(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.copy_permissions(source, destination)
        }

        fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.replace(source, destination)
        }

        fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.publish_new(source, destination)
        }

        fn delete_if_exists(&self, path: &Path) -> Result<()> {
            StdFileSystem.delete_if_exists(path)
        }

        fn sync_directory(&self, _directory: &Path) -> Result<()> {
            Err(Error::System("injected directory sync failure".to_owned()))
        }
    }

    #[test]
    fn restore_output_sync_failure_reports_the_published_path() -> TestResult {
        let unique = format!(
            "sse-storage-restore-output-sync-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        let exports = root.join("exports");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&exports)?;
        let source = saves.join("save.sav");
        let output = exports.join("restored.sav");
        let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let edited = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, original)?;
        let receipt = replace_std_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;

        let result = super::restore_backup_with_file_system(&FailDirectorySync, &receipt.journal_path, &output);

        let error = result
            .err()
            .ok_or_else(|| std::io::Error::other("injected post-publication sync failure should be returned"))?;
        assert!(error.to_string().contains(&output.display().to_string()));
        assert_eq!(std::fs::read(&output)?, original);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    struct RestoreOutputChangesAfterReadback {
        output: PathBuf,
        readback_seen: Cell<bool>,
    }

    impl FileSystem for RestoreOutputChangesAfterReadback {
        fn read_all(&self, path: &Path) -> Result<Vec<u8>> {
            if path == self.output && !self.readback_seen.replace(true) {
                std::fs::write(path, b"external replacement observed during restore")?;
                let observed = StdFileSystem.read_all(path)?;
                std::fs::write(
                    path,
                    include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
                )?;
                return Ok(observed);
            }
            StdFileSystem.read_all(path)
        }

        fn is_symlink(&self, path: &Path) -> Result<bool> {
            StdFileSystem.is_symlink(path)
        }

        fn create_dir_all(&self, path: &Path) -> Result<()> {
            StdFileSystem.create_dir_all(path)
        }

        fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
            StdFileSystem.write_new(path, bytes)
        }

        fn copy_permissions(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.copy_permissions(source, destination)
        }

        fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.replace(source, destination)
        }

        fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
            StdFileSystem.publish_new(source, destination)
        }

        fn delete_if_exists(&self, path: &Path) -> Result<()> {
            StdFileSystem.delete_if_exists(path)
        }
    }

    #[test]
    fn restore_readback_mismatch_must_not_delete_a_replaced_output() -> TestResult {
        let unique = format!(
            "sse-storage-restore-mismatch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        let exports = root.join("exports");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&exports)?;
        let source = saves.join("save.sav");
        let output = exports.join("restored.sav");
        let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let edited = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, original)?;
        let receipt = replace_std_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;
        let files = RestoreOutputChangesAfterReadback {
            output: output.clone(),
            readback_seen: Cell::new(false),
        };

        let result = restore_backup_with_file_system(&files, &receipt.journal_path, &output);

        assert!(result.is_err(), "the mismatching read-back must still be reported");
        assert_eq!(std::fs::read(&output)?, original);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn in_place_restore_checks_current_hash_and_creates_a_reversible_safety_backup() -> TestResult {
        let unique = format!(
            "sse-storage-restore-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&backups)?;
        let source = saves.join("save.sav");
        let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let edited = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, original)?;
        let receipt = replace_std_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;

        let restored = super::restore_in_place(&receipt.journal_path)?;

        assert_eq!(std::fs::read(&source)?, original);
        let safety_backup = restored
            .safety_backup_path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("restore should create a safety backup"))?;
        let safety_journal = restored
            .safety_journal_path
            .as_ref()
            .ok_or_else(|| std::io::Error::other("restore should create a safety journal"))?;
        assert_eq!(std::fs::read(safety_backup)?, edited);
        let journal = std::fs::read_to_string(safety_journal)?;
        assert!(journal.contains("\"mode\":\"restore\""));
        assert!(journal.contains("\"restore_from\":"));

        super::restore_in_place(safety_journal)?;
        assert_eq!(std::fs::read(&source)?, edited);
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn cloud_recovery_refuses_a_backup_folder_that_is_a_file_and_writes_nothing() -> TestResult {
        let root = std::env::temp_dir().join(format!("sse-cloud-refuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let blocker = root.join("not-a-folder");
        std::fs::write(&blocker, b"plain file")?;

        let result = super::write_cloud_recovery_artifacts(&blocker, 4500, "quicksave.sav", b"cloud", b"edited");

        assert!(result.is_err());
        assert_eq!(std::fs::read(&blocker)?, b"plain file");
        assert_eq!(
            std::fs::read_dir(&root)?.count(),
            1,
            "no artifact may be created beside the blocking file"
        );
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn cloud_recovery_artifacts_are_visible_and_restore_only_to_a_new_copy() -> TestResult {
        let unique = format!(
            "sse-storage-cloud-backup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let original = b"cloud original save bytes";
        let edited = b"edited local save bytes";
        let receipt =
            super::write_cloud_recovery_artifacts(&root, 4500, "_appdata_/savedgames/quicksave.sav", original, edited)?;

        let entries = super::list_backups(&root)?;
        let entry = entries
            .iter()
            .find(|entry| entry.journal_path == receipt.journal_path)
            .ok_or_else(|| std::io::Error::other("Steam Cloud backup should be visible"))?;
        assert_eq!(entry.status, super::BackupStatus::Verified);
        assert_eq!(entry.operation_mode.as_deref(), Some("steam-cloud"));
        assert_eq!(std::fs::read(&receipt.backup_path)?, original);
        assert_eq!(std::fs::read(&receipt.recovery_path)?, edited);

        let restored_directory = root.join("restored");
        std::fs::create_dir_all(&restored_directory)?;
        let restored = restored_directory.join("quicksave.sav");
        super::restore_backup(&receipt.journal_path, &restored)?;
        assert_eq!(std::fs::read(restored)?, original);
        assert!(super::restore_in_place(&receipt.journal_path).is_err());

        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn in_place_restore_refuses_changed_source_and_restores_a_missing_source() -> TestResult {
        let unique = format!(
            "sse-storage-restore-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&backups)?;
        let source = saves.join("save.sav");
        let original = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let edited = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, original)?;
        let receipt = replace_std_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;
        std::fs::write(&source, b"changed after save")?;
        let Err(error) = super::restore_in_place(&receipt.journal_path) else {
            return Err("changed source must be refused".into());
        };
        assert_eq!(
            error.to_string(),
            "Current save matches neither the original nor journaled output; refusing to overwrite it."
        );
        assert_eq!(std::fs::read(&source)?, b"changed after save");

        std::fs::write(&source, edited)?;
        std::fs::remove_file(&source)?;
        let restored = super::restore_in_place(&receipt.journal_path)?;
        assert_eq!(std::fs::read(&source)?, original);
        assert!(restored.safety_backup_path.is_none());
        assert!(restored.safety_journal_path.is_none());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn replacing_a_symbolic_link_is_refused_without_touching_its_target() -> TestResult {
        let unique = format!("sse-storage-symlink-{}", std::process::id());
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("saves");
        let backups = root.join("backups");
        std::fs::create_dir_all(&saves)?;
        std::fs::create_dir_all(&backups)?;
        let target = saves.join("real.sav");
        let link = saves.join("linked.sav");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&target, source_bytes)?;
        std::os::unix::fs::symlink(&target, &link)?;
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let result = replace_std_transaction(&link, &source_hash, replacement, &backups);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&target)?, source_bytes);
        assert!(std::fs::read_dir(&backups)?.next().is_none());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn backup_directory_symlink_alias_inside_save_folder_is_refused() -> TestResult {
        let unique = format!(
            "sse-storage-backup-alias-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let save_directory = root.join("real/saves");
        let alias = root.join("alias");
        std::fs::create_dir_all(&save_directory)?;
        std::os::unix::fs::symlink(root.join("real"), &alias)?;
        let source = save_directory.join("save.sav");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, source_bytes)?;
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let result = replace_std_transaction(&source, &source_hash, replacement, &alias.join("saves/backups"));

        assert!(result.is_err());
        assert_eq!(std::fs::read(&source)?, source_bytes);
        assert!(!save_directory.join("backups").exists());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn backup_path_containment_compares_windows_components_without_case() {
        let source_directory = Path::new("C:/Users/Player/Saves");
        let backup_directory = Path::new("c:/users/player/saves/Backups");

        assert!(super::path_starts_with(backup_directory, source_directory, true));
        assert!(!super::path_starts_with(
            Path::new("C:/Users/Player/Saves-old"),
            source_directory,
            true
        ));
        assert!(!super::path_starts_with(backup_directory, source_directory, false));
    }

    #[cfg(windows)]
    #[test]
    fn backup_directory_inside_save_folder_is_refused_when_windows_case_differs() -> TestResult {
        let unique = format!(
            "sse-storage-backup-case-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        let saves = root.join("Saves");
        std::fs::create_dir_all(&saves)?;
        let source = saves.join("save.sav");
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        std::fs::write(&source, source_bytes)?;
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let result = replace_std_transaction(&source, &source_hash, replacement, &root.join("saves/backups"));

        assert!(result.is_err());
        assert_eq!(std::fs::read(&source)?, source_bytes);
        assert!(!saves.join("backups").exists());
        std::fs::remove_dir_all(root)?;
        Ok(())
    }

    fn fake_paths() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join("sse-storage-fake");
        (root.join("saves/save.sav"), root.join("backups"))
    }

    struct MemoryFs {
        files: RefCell<HashMap<PathBuf, Vec<u8>>>,
        operations: Cell<usize>,
        fail_at: Option<usize>,
        failed: Cell<bool>,
        readonly: Cell<bool>,
        backup_prune_calls: Cell<usize>,
    }

    impl MemoryFs {
        fn new(source: &Path, bytes: &[u8], fail_at: Option<usize>) -> Self {
            Self {
                files: RefCell::new(HashMap::from([(source.to_path_buf(), bytes.to_vec())])),
                operations: Cell::new(0),
                fail_at,
                failed: Cell::new(false),
                readonly: Cell::new(false),
                backup_prune_calls: Cell::new(0),
            }
        }

        fn bytes(&self, path: &Path) -> Option<Vec<u8>> {
            self.files.borrow().get(path).cloned()
        }

        fn operation_count(&self) -> usize {
            self.operations.get()
        }

        fn backup_prune_calls(&self) -> usize {
            self.backup_prune_calls.get()
        }

        fn tick(&self) -> Result<()> {
            let operation = self.operations.get().saturating_add(1);
            self.operations.set(operation);
            if self.fail_at == Some(operation) && !self.failed.replace(true) {
                return Err(Error::System(format!("injected filesystem failure at {operation}")));
            }
            Ok(())
        }
    }

    impl FileSystem for MemoryFs {
        fn read_all(&self, path: &Path) -> Result<Vec<u8>> {
            self.tick()?;
            self.files
                .borrow()
                .get(path)
                .cloned()
                .ok_or_else(|| Error::System(format!("missing synthetic file {}", path.display())))
        }

        fn is_symlink(&self, _path: &Path) -> Result<bool> {
            self.tick()?;
            Ok(false)
        }

        fn create_dir_all(&self, _path: &Path) -> Result<()> {
            self.tick()
        }

        fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
            self.tick()?;
            let mut files = self.files.borrow_mut();
            if files.contains_key(path) {
                return Err(Error::System(format!(
                    "synthetic file already exists: {}",
                    path.display()
                )));
            }
            files.insert(path.to_path_buf(), bytes.to_vec());
            Ok(())
        }

        fn copy_permissions(&self, _source: &Path, _destination: &Path) -> Result<()> {
            self.tick()
        }

        fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
            self.tick()?;
            let mut files = self.files.borrow_mut();
            let bytes = files
                .remove(source)
                .ok_or_else(|| Error::System(format!("missing synthetic temp {}", source.display())))?;
            files.insert(destination.to_path_buf(), bytes);
            Ok(())
        }

        fn replace_if_sha256_matches(&self, source: &Path, destination: &Path, expected_sha256: &str) -> Result<()> {
            self.tick()?;
            let mut files = self.files.borrow_mut();
            let current = files
                .get(destination)
                .ok_or_else(|| Error::System(format!("missing synthetic source {}", destination.display())))?;
            if sse_codecs::sha256::sha256_hex(current) != expected_sha256 {
                return Err(Error::Refused(
                    "source changed immediately before replacement".to_owned(),
                ));
            }
            let bytes = files
                .remove(source)
                .ok_or_else(|| Error::System(format!("missing synthetic temp {}", source.display())))?;
            files.insert(destination.to_path_buf(), bytes);
            Ok(())
        }

        fn delete_if_exists(&self, path: &Path) -> Result<()> {
            self.files.borrow_mut().remove(path);
            Ok(())
        }

        fn is_readonly(&self, _path: &Path) -> Result<bool> {
            Ok(self.readonly.get())
        }

        fn prune_verified_backups(&self, _directory: &Path) -> Result<()> {
            self.backup_prune_calls
                .set(self.backup_prune_calls.get().saturating_add(1));
            Ok(())
        }
    }
}
