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
    /// Creates a directory and its parents.
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    /// Creates a new file, writes all bytes, and flushes it durably.
    fn write_new(&self, path: &Path, bytes: &[u8]) -> Result<()>;
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
            "backup directory must be outside the source directory".to_owned(),
        ));
    }

    let source_bytes = fs::read(&source_path)?;
    let source_sha256 = sha256::sha256_hex(&source_bytes);
    if source_sha256 != expected_source_sha256 {
        return Err(Error::Refused(format!(
            "source changed since preparation: expected {expected_source_sha256}, found {source_sha256}"
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
    if fs::symlink_metadata(&output_path).is_ok() {
        return Err(Error::System(format!(
            "restore output already exists: {}",
            output_path.display()
        )));
    }
    let directory = journal_path
        .parent()
        .ok_or_else(|| Error::Refused("backup journal has no parent directory".to_owned()))?;
    let entry = inspect_backup(directory, &journal_path);
    if entry.status != BackupStatus::Verified {
        return Err(Error::Refused(format!(
            "backup is not restorable: {}",
            entry.error.unwrap_or_else(|| "unverified backup".to_owned())
        )));
    }
    if output_path == entry.backup_path {
        return Err(Error::Refused("restore output cannot be the backup file".to_owned()));
    }
    let output_directory = output_path
        .parent()
        .ok_or_else(|| Error::Refused("restore output has no parent directory".to_owned()))?;
    if !output_directory.is_dir() {
        return Err(Error::Refused(format!(
            "restore output directory does not exist: {}",
            output_directory.display()
        )));
    }
    let bytes = fs::read(&entry.backup_path)?;
    if sha256::sha256_hex(&bytes) != entry.source_sha256 {
        return Err(Error::Refused("backup changed after verification".to_owned()));
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
        return Err(Error::System(
            "restored output read-back did not match the backup".to_owned(),
        ));
    }
    Ok(output_path)
}

/// Replaces a save only when its fresh SHA-256 still matches the prepared source hash.
pub fn replace_transaction(
    source_path: &Path,
    expected_source_sha256: &str,
    replacement: &[u8],
    backup_directory: &Path,
) -> Result<ReplacementReceipt> {
    replace_with_file_system(
        &StdFileSystem,
        source_path,
        expected_source_sha256,
        replacement,
        backup_directory,
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
            "backup directory must be outside the source directory".to_owned(),
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
    let token = transaction_token();
    let stem = source_path
        .file_stem()
        .map(|value| value.to_string_lossy())
        .unwrap_or_else(|| "save".into());
    let artifact_stem = format!("{stem}_{}", token);
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
    let transaction = (|| {
        files.write_new(&backup_path, &source_bytes)?;
        files.write_new(&recovery_path, replacement)?;
        let prepared_at = timestamp_utc()?;
        files.write_new(
            &journal_path,
            &serialize_journal(&Journal {
                status: "prepared",
                created_at: &prepared_at,
                source_path: &source_path,
                source_sha256: &source_sha256,
                output_path: &source_path,
                output_sha256: &output_sha256,
                backup_path: &backup_path,
                recovery_path: &recovery_path,
            }),
        )?;
        files.write_new(&temporary_output, replacement)?;

        let current_source = files.read_all(&source_path)?;
        let current_hash = sha256::sha256_hex(&current_source);
        if current_hash != expected_source_sha256 {
            return Err(Error::Refused(
                "source changed immediately before replacement".to_owned(),
            ));
        }

        files.replace(&temporary_output, &source_path)?;
        source_replaced = true;

        let read_back = files.read_all(&source_path)?;
        let read_back_hash = sha256::sha256_hex(&read_back);
        if read_back_hash != output_sha256 || read_back != replacement {
            return Err(Error::System(
                "replacement read-back did not match the prepared bytes".to_owned(),
            ));
        }

        let verified_at = timestamp_utc()?;
        files.write_new(
            &temporary_journal,
            &serialize_journal(&Journal {
                status: "verified",
                created_at: &verified_at,
                source_path: &source_path,
                source_sha256: &source_sha256,
                output_path: &source_path,
                output_sha256: &output_sha256,
                backup_path: &backup_path,
                recovery_path: &recovery_path,
            }),
        )?;
        files.replace(&temporary_journal, &journal_path)?;
        Ok(())
    })();

    if let Err(error) = transaction {
        if source_replaced {
            let rollback = files
                .write_new(&temporary_rollback, &source_bytes)
                .and_then(|()| files.replace(&temporary_rollback, &source_path));
            if let Err(rollback_error) = rollback {
                cleanup(files, &temporary_output);
                cleanup(files, &temporary_journal);
                cleanup(files, &temporary_rollback);
                return Err(Error::System(format!(
                    "replacement failed ({error}); restoring the original also failed ({rollback_error})"
                )));
            }
        }
        cleanup(files, &temporary_output);
        cleanup(files, &temporary_journal);
        cleanup(files, &temporary_rollback);
        return Err(error);
    }

    cleanup(files, &temporary_output);
    cleanup(files, &temporary_journal);
    cleanup(files, &temporary_rollback);
    Ok(ReplacementReceipt {
        source_path,
        backup_path,
        recovery_path,
        journal_path,
        output_sha256,
    })
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
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let counter = NEXT_TRANSACTION_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:032x}-{:08x}-{counter:016x}", std::process::id())
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

fn valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn serialize_journal(journal: &Journal<'_>) -> Vec<u8> {
    let source_path = json_escape(&journal.source_path.to_string_lossy());
    let output_path = json_escape(&journal.output_path.to_string_lossy());
    let backup_path = json_escape(&journal.backup_path.to_string_lossy());
    let recovery_path = json_escape(&journal.recovery_path.to_string_lossy());
    let created_at = json_escape(journal.created_at);
    format!(
        "{{\"version\":1,\"status\":\"{}\",\"created_at\":\"{created_at}\",\"source_path\":\"{source_path}\",\"source_sha256\":\"{}\",\"output_path\":\"{output_path}\",\"output_sha256\":\"{}\",\"backup_path\":\"{backup_path}\",\"recovery_path\":\"{recovery_path}\",\"operation\":{{\"mode\":\"replace\",\"money\":null,\"stack_count\":0}}}}",
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
        assert_eq!(fs.operation_count(), 11);
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
        assert_eq!(fs.operation_count(), 1);
        Ok(())
    }

    #[test]
    fn every_transaction_step_failure_keeps_the_source_bytes() -> TestResult {
        let source_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let output_bytes = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        let source_hash = sse_codecs::sha256::sha256_hex(source_bytes);
        let (source, backup_directory) = fake_paths();

        for failure_step in 1..=11 {
            let fs = MemoryFs::new(&source, source_bytes, Some(failure_step));
            assert!(replace_with_file_system(&fs, &source, &source_hash, output_bytes, &backup_directory).is_err());
            assert_eq!(
                fs.bytes(&source).as_deref(),
                Some(source_bytes.as_slice()),
                "step {failure_step}"
            );
        }
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
