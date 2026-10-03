//! Journaled local save replacement.

use sse_codecs::sha256;
use sse_core::{Error, Result};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TRANSACTION_ID: AtomicU64 = AtomicU64::new(1);

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
