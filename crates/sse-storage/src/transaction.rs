//! Journaled local save replacement.

use sse_codecs::sha256;
use sse_core::{Error, Result};
use std::collections::HashMap;
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);
const MAXIMUM_JOURNAL_BYTES: u64 = 2 * 1024 * 1024;

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
    /// Publishes a new path atomically without overwriting an existing destination.
    fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
        self.replace(source, destination)
    }
    /// Removes a file if it exists.
    fn delete_if_exists(&self, path: &Path) -> Result<()>;
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
        sync_directory(path.parent());
        Ok(())
    }

    fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let mut created = false;
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options.open(path)?;
            created = true;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
            }
            file.write_all(bytes)?;
            file.sync_all()?;
            sync_directory(path.parent());
            Ok(())
        })();
        if result.is_err() && created {
            let _ = fs::remove_file(path);
        }
        result
    }

    fn copy_permissions(&self, source: &Path, destination: &Path) -> Result<()> {
        fs::set_permissions(destination, fs::metadata(source)?.permissions())?;
        Ok(())
    }

    fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
        fs::rename(source, destination)?;
        sync_directory(destination.parent());
        Ok(())
    }

    fn publish_new(&self, source: &Path, destination: &Path) -> Result<()> {
        fs::hard_link(source, destination)?;
        let _ = fs::remove_file(source);
        sync_directory(destination.parent());
        Ok(())
    }

    fn delete_if_exists(&self, path: &Path) -> Result<()> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
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
}

/// Backup listing status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupStatus {
    /// Journal and backup exist and the source hash matches.
    Verified,
    /// Journal refers to a backup that is absent.
    Missing,
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
    if fs::symlink_metadata(&output_path).is_ok() {
        return Err(Error::System(format!(
            "export output already exists: {}",
            output_path.display()
        )));
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
    if backup_directory.starts_with(source_directory) {
        return Err(Error::Refused(
            "Backup directory must be outside the selected save directory.".to_owned(),
        ));
    }

    let source_bytes = fs::read(&source_path)?;
    let source_sha256 = sha256::sha256_hex(&source_bytes);
    if source_sha256 != expected_source_sha256 {
        return Err(Error::Refused(format!(
            "Source changed since analysis: expected {expected_source_sha256}, found {source_sha256}."
        )));
    }
    let output_sha256 = sha256::sha256_hex(replacement);
    let token = transaction_token();
    let stem = source_path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save".into());
    let artifact_stem = format!("{stem}_{token}");
    let backup_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.sav"));
    let journal_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.json"));
    let output_name = output_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save.sav".into());
    let temporary_output = output_directory.join(format!(".{output_name}.{token}.tmp"));
    let temporary_journal = backup_directory.join(format!(".{artifact_stem}.json.tmp"));
    fs::create_dir_all(&backup_directory)?;
    let mut published = false;
    let transaction = (|| {
        StdFileSystem.write_new(&backup_path, &source_bytes)?;
        StdFileSystem.write_new(&temporary_output, replacement)?;
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
        let fresh_source = fs::read(&source_path)?;
        if sha256::sha256_hex(&fresh_source) != expected_source_sha256 {
            return Err(Error::Refused(
                "source changed immediately before export publication".to_owned(),
            ));
        }
        StdFileSystem.publish_new(&temporary_output, &output_path)?;
        published = true;
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
        StdFileSystem.replace(&temporary_journal, &journal_path)?;
        Ok(())
    })();
    cleanup(&StdFileSystem, &temporary_output);
    cleanup(&StdFileSystem, &temporary_journal);
    if let Err(error) = transaction {
        if published && fs::read(&output_path).is_ok_and(|bytes| bytes == replacement) {
            let _ = fs::remove_file(&output_path);
        }
        return Err(error);
    }
    Ok(ExportReceipt {
        output_path,
        backup_path,
        journal_path,
        output_sha256,
    })
}

/// Lists and verifies one directory of C#-compatible backup journals.
pub fn list_backups(directory: &Path) -> Result<Vec<BackupEntry>> {
    let directory = absolute_path(directory)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut paths = fs::read_dir(&directory)?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .collect::<Vec<_>>();
    paths.sort();
    let mut entries = Vec::with_capacity(paths.len());
    let mut referenced = Vec::with_capacity(paths.len());
    for journal_path in paths {
        let entry = inspect_backup(&directory, &journal_path);
        referenced.push(entry.backup_path.clone());
        entries.push(entry);
    }
    for item in fs::read_dir(&directory)? {
        let item = item?;
        let backup_path = item.path();
        if backup_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().ends_with("_ORIGINAL.sav"))
            && !referenced.contains(&backup_path)
        {
            entries.push(BackupEntry {
                journal_path: backup_path.with_extension("json"),
                backup_path,
                source_path: PathBuf::new(),
                source_sha256: String::new(),
                status: BackupStatus::Corrupt,
                error: Some("journal for this backup is missing".to_owned()),
            });
        }
    }
    entries.sort_by(|left, right| right.journal_path.cmp(&left.journal_path));
    Ok(entries)
}

/// Restores a verified journaled backup to a new path without replacing the source save.
pub fn restore_backup(journal_path: &Path, output_path: &Path) -> Result<PathBuf> {
    let journal_path = absolute_path(journal_path)?;
    let output_path = absolute_path(output_path)?;
    let directory = journal_path
        .parent()
        .ok_or_else(|| Error::Refused("backup journal has no parent directory".to_owned()))?;
    let entry = inspect_backup(directory, &journal_path);
    if entry.status != BackupStatus::Verified {
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
    let output_name = output_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save.sav".into());
    let temporary = output_directory.join(format!(".{output_name}.{token}.tmp"));
    StdFileSystem.write_new(&temporary, &bytes)?;
    let publish = StdFileSystem.publish_new(&temporary, &output_path);
    cleanup(&StdFileSystem, &temporary);
    publish?;
    let read_back = fs::read(&output_path)?;
    if read_back != bytes || sha256::sha256_hex(&read_back) != entry.source_sha256 {
        if fs::read(&output_path).is_ok_and(|published| published == bytes) {
            let _ = StdFileSystem.delete_if_exists(&output_path);
        }
        return Err(Error::System(
            "Restored output read-back did not match its expected bytes.".to_owned(),
        ));
    }
    Ok(output_path)
}

fn restore_not_restorable(entry: &BackupEntry) -> Error {
    let status = match entry.status {
        BackupStatus::Verified => "Verified",
        BackupStatus::Missing => "Missing",
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
    if entry.status != BackupStatus::Verified {
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
        return Ok(RestoreReceipt {
            save_path: restored_path,
            safety_backup_path: None,
            safety_journal_path: None,
        });
    }
    let current = fs::read(&source_path)?;
    if sha256::sha256_hex(&current) != metadata.output_sha256 {
        return Err(Error::Refused(
            "Current save changed after the journaled replacement; refusing to overwrite it.".to_owned(),
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
        &source_path,
        &metadata.output_sha256,
        &restore_bytes,
        directory,
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
    Ok(RestoreReceipt {
        save_path: receipt.source_path,
        safety_backup_path: Some(receipt.backup_path),
        safety_journal_path: Some(receipt.journal_path),
    })
}

/// Replaces a save only when its fresh SHA-256 still matches the prepared source hash.
pub fn replace_transaction(
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
) -> Result<ReplacementReceipt> {
    replace_transaction_with_verifier(
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
        |_| Ok(()),
    )
    .map(|(receipt, ())| receipt)
}

/// Replaces a save and runs a format-aware check against durable read-back bytes before commit.
pub fn replace_transaction_with_verifier<T>(
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
    verify_readback: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<(ReplacementReceipt, T)> {
    replace_with_file_system_and_verifier(
        &StdFileSystem,
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
        verify_readback,
    )
}

/// Testable version of [`replace_transaction`].
pub fn replace_with_file_system(
    files: &impl FileSystem,
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
) -> Result<ReplacementReceipt> {
    replace_with_file_system_and_verifier(
        files,
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
        |_| Ok(()),
    )
    .map(|(receipt, ())| receipt)
}

/// Testable replacement with a read-back verifier whose output is returned after commit.
pub fn replace_with_file_system_and_verifier<T>(
    files: &impl FileSystem,
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
    verify_readback: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<(ReplacementReceipt, T)> {
    replace_with_file_system_and_verifier_operation(
        files,
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
        JournalOperation::Replace,
        verify_readback,
    )
}

fn replace_with_file_system_and_verifier_operation<T>(
    files: &impl FileSystem,
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
    operation: JournalOperation<'_>,
    verify_readback: impl FnOnce(&[u8]) -> Result<T>,
) -> Result<(ReplacementReceipt, T)> {
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
    if backup_directory.starts_with(source_directory) {
        return Err(Error::Refused(
            "Backup directory must be outside the selected save directory.".to_owned(),
        ));
    }
    if files.is_symlink(&source_path)? {
        return Err(Error::Refused(
            "The save is a symbolic link; open the file it points to instead.".to_owned(),
        ));
    }

    let source_bytes = files.read_all(&source_path)?;
    let source_sha256 = sha256::sha256_hex(&source_bytes);
    if source_sha256 != expected_source_sha256 {
        return Err(Error::Refused(format!(
            "source changed since preparation: expected {expected_source_sha256}, found {source_sha256}"
        )));
    }
    let output_sha256 = sha256::sha256_hex(replacement);
    let created_at = timestamp_utc()?;
    let token = transaction_token();
    let stem = source_path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save".into());
    let stem = stem.chars().take(64).collect::<String>();
    let artifact_stem = format!("{stem}_{}_{}", file_timestamp(&created_at)?, token);
    let backup_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.sav"));
    let recovery_path = backup_directory.join(format!("{artifact_stem}_EDITED.sav"));
    let journal_path = backup_directory.join(format!("{artifact_stem}_ORIGINAL.json"));
    let file_name = source_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save.sav".into());
    let temporary_output = source_directory.join(format!(".{file_name}.{token}.tmp"));
    let temporary_journal = backup_directory.join(format!(".{artifact_stem}.json.tmp"));
    let temporary_rollback = source_directory.join(format!(".{file_name}.{token}.rollback.tmp"));
    files.create_dir_all(&backup_directory)?;

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

        let current_source = files.read_all(&source_path)?;
        let current_hash = sha256::sha256_hex(&current_source);
        if current_hash != expected_source_sha256 {
            return Err(Error::Refused(format!(
                "Source changed before replacement: expected {expected_source_sha256}, found {current_hash}."
            )));
        }

        files.copy_permissions(&source_path, &temporary_output)?;
        source_replaced = true;
        files.replace(&temporary_output, &source_path)?;

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
        Ok(verified_value)
    })();

    let verified_value = match transaction {
        Ok(value) => value,
        Err(error) => {
            if source_replaced {
                let rollback = files
                    .write_new(&temporary_rollback, &source_bytes)
                    .and_then(|()| files.copy_permissions(&source_path, &temporary_rollback))
                    .and_then(|()| files.replace(&temporary_rollback, &source_path));
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
        },
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
    Replace,
    Restore(&'a Path),
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

fn serialize_export_journal(journal: &ExportJournal<'_>) -> Vec<u8> {
    use std::fmt::Write as _;

    let created_at = json_escape(journal.created_at);
    let source_path = json_escape(&journal.source_path.to_string_lossy());
    let output_path = json_escape(&journal.output_path.to_string_lossy());
    let backup_path = json_escape(&journal.backup_path.to_string_lossy());
    let money = journal
        .summary
        .money
        .map_or_else(|| "null".to_owned(), |value| value.to_string());
    let mut encoded = String::new();
    let _ = write!(
        encoded,
        "{{\"version\":1,\"status\":\"{}\",\"created_at\":\"{created_at}\",\"source_path\":\"{source_path}\",\"source_sha256\":\"{}\",\"output_path\":\"{output_path}\",\"output_sha256\":\"{}\",\"backup_path\":\"{backup_path}\",\"operation\":{{\"mode\":\"export\",\"money\":{money},\"stack_count\":{},\"move_count\":{},\"detach_count\":{},\"attach_count\":{},\"raw_count\":{},\"add_count\":{},\"durability_count\":{},\"upgrade_count\":{},\"relation_count\":{},\"player_faction\":{}",
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
    let parsed = (|| -> Result<(PathBuf, PathBuf, String)> {
        if field_u64(&fields, "version")? != 1 {
            return Err(Error::Refused("unsupported journal version".to_owned()));
        }
        let created_at = field_string(&fields, "created_at")?;
        let output_path = field_string(&fields, "output_path")?;
        let output_sha256 = field_string(&fields, "output_sha256")?;
        if created_at.is_empty() || output_path.is_empty() || !valid_sha256(&output_sha256) {
            return Err(Error::Refused(
                "journal creation or output fields are malformed".to_owned(),
            ));
        }
        if !matches!(fields.get("operation"), Some(TopValue::Object)) {
            return Err(Error::Refused("journal operation is not an object".to_owned()));
        }
        if field_string(&fields, "status")? != "verified" {
            return Err(Error::Refused("journal status is not verified".to_owned()));
        }
        let source_path = PathBuf::from(field_string(&fields, "source_path")?);
        let source_sha256 = field_string(&fields, "source_sha256")?;
        if !valid_sha256(&source_sha256) {
            return Err(Error::Refused("journal source SHA-256 is malformed".to_owned()));
        }
        let backup_value = PathBuf::from(field_string(&fields, "backup_path")?);
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
        Ok((source_path, backup_path, source_sha256))
    })();
    let (source_path, backup_path, source_sha256) = match parsed {
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
    let backup_bytes = match fs::read(&backup_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return BackupEntry {
                journal_path: journal_path.to_path_buf(),
                backup_path,
                source_path,
                source_sha256,
                status: BackupStatus::Missing,
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
    let actual = sha256::sha256_hex(&backup_bytes);
    if actual != source_sha256 {
        return corrupt(
            "backup SHA-256 does not match the journal".to_owned(),
            Some(backup_path),
            Some(source_path),
            Some(source_sha256),
        );
    }
    BackupEntry {
        journal_path: journal_path.to_path_buf(),
        backup_path,
        source_path,
        source_sha256,
        status: BackupStatus::Verified,
        error: None,
    }
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
    let output_path = PathBuf::from(field_string(&fields, "output_path")?);
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
    let created_at = json_escape(journal.created_at);
    let operation = match journal.operation {
        JournalOperation::Replace => "{\"mode\":\"replace\",\"money\":null,\"stack_count\":0}".to_owned(),
        JournalOperation::Restore(restore_from) => format!(
            "{{\"mode\":\"restore\",\"restore_from\":\"{}\",\"money\":null,\"stack_count\":0}}",
            json_escape(&restore_from.to_string_lossy())
        ),
    };
    format!(
        "{{\"version\":1,\"status\":\"{}\",\"created_at\":\"{created_at}\",\"source_path\":\"{source_path}\",\"source_sha256\":\"{}\",\"output_path\":\"{output_path}\",\"output_sha256\":\"{}\",\"backup_path\":\"{backup_path}\",\"recovery_path\":\"{recovery_path}\",\"operation\":{operation}}}",
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

fn sync_directory(directory: Option<&Path>) {
    #[cfg(unix)]
    if let Some(directory) = directory {
        if let Ok(handle) = File::open(directory) {
            let _ = handle.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = directory;
}

#[cfg(test)]
mod tests {
    use super::{replace_with_file_system, FileSystem};
    use sse_core::{Error, Result};
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn replaces_and_records_a_synthetic_save() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let receipt = replace_with_file_system(&fs, &source, &source_hash, output_bytes, &backup_directory)?;

        assert_eq!(fs.bytes(&source).as_deref(), Some(output_bytes.as_slice()));
        assert_eq!(fs.bytes(&receipt.backup_path).as_deref(), Some(source_bytes.as_slice()));
        let journal = fs
            .bytes(&receipt.journal_path)
            .ok_or_else(|| std::io::Error::other("journal should exist"))?;
        let journal = std::str::from_utf8(&journal)?;
        assert!(journal.contains("\"status\":\"verified\""));
        assert!(journal.contains(&format!("\"source_sha256\":\"{source_hash}\"")));
        assert_eq!(fs.operation_count(), 13);
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
    fn every_transaction_step_failure_keeps_the_source_bytes() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();

        for failure_step in 1..=13 {
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
    fn semantic_readback_failure_rolls_back_and_cleans_artifacts() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();
        let fs = MemoryFs::new(&source, source_bytes, None);

        let result = super::replace_with_file_system_and_verifier(
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
    fn replacement_preserves_unix_mode_bits() -> TestResult {
        use std::os::unix::fs::PermissionsExt;

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
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);

        let failed = super::replace_transaction_with_verifier(&source, &source_hash, replacement, &backups, |_| {
            Err::<(), Error>(Error::damaged("injected semantic verification failure"))
        });
        assert!(failed.is_err());
        assert_eq!(std::fs::read(&source)?, source_bytes);
        let original_mode = std::fs::metadata(&source)?.permissions().mode() & 0o777;
        assert_eq!(original_mode, 0o640);
        assert!(std::fs::read_dir(&backups)?.next().is_none());
        assert!(std::fs::read_dir(&saves)?.all(|entry| { entry.is_ok_and(|entry| entry.file_name() == "save.sav") }));

        let receipt = super::replace_transaction(&source, &source_hash, replacement, &backups)?;

        let mode = std::fs::metadata(&source)?.permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
        assert_eq!(std::fs::read(&source)?, replacement);
        assert!(receipt
            .backup_path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().contains("_ORIGINAL.sav")));
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
        let receipt = super::replace_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;

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
        let receipt = super::replace_transaction(&source, &sse_codecs::sha256::sha256_hex(original), edited, &backups)?;
        std::fs::write(&source, b"changed after save")?;
        let Err(error) = super::restore_in_place(&receipt.journal_path) else {
            return Err("changed source must be refused".into());
        };
        assert_eq!(
            error.to_string(),
            "Current save changed after the journaled replacement; refusing to overwrite it."
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

        let result = super::replace_transaction(&link, &source_hash, replacement, &backups);

        assert!(result.is_err());
        assert_eq!(std::fs::read(&target)?, source_bytes);
        assert!(std::fs::read_dir(&backups)?.next().is_none());
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
    }

    impl MemoryFs {
        fn new(source: &Path, bytes: &[u8], fail_at: Option<usize>) -> Self {
            Self {
                files: RefCell::new(HashMap::from([(source.to_path_buf(), bytes.to_vec())])),
                operations: Cell::new(0),
                fail_at,
                failed: Cell::new(false),
            }
        }

        fn bytes(&self, path: &Path) -> Option<Vec<u8>> {
            self.files.borrow().get(path).cloned()
        }

        fn operation_count(&self) -> usize {
            self.operations.get()
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

        fn delete_if_exists(&self, path: &Path) -> Result<()> {
            self.files.borrow_mut().remove(path);
            Ok(())
        }
    }
}
