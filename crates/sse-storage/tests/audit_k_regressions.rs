#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, missing_docs)]
use sse_core::{Error, Result};
use sse_storage::transaction::*;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const OLD: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
const NEW: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
const EXTERNAL: &[u8] = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "audit-k-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(p.join("saves")).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn rollback_must_preserve_a_new_external_save() {
    let t = Temp::new();
    let source = t.0.join("saves/test.sav");
    fs::write(&source, OLD).unwrap();
    let result = replace_transaction_with_verifier(
        &source,
        &sse_codecs::sha256::sha256_hex(OLD),
        NEW,
        &t.0.join("backups"),
        |_| {
            fs::write(&source, EXTERNAL)?;
            Err::<(), Error>(Error::System("injected readback failure after external save".into()))
        },
    );
    assert!(result.is_err());
    assert!(
        fs::read(&source).unwrap() == EXTERNAL,
        "rollback destroyed the newer external save"
    );
}
struct SourceChangesDuringPermissionCopy;
impl FileSystem for SourceChangesDuringPermissionCopy {
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
        StdFileSystem.copy_permissions(source, destination)?;
        fs::write(source, EXTERNAL)?;
        Ok(())
    }
    fn replace(&self, source: &Path, destination: &Path) -> Result<()> {
        StdFileSystem.replace(source, destination)
    }
    fn delete_if_exists(&self, path: &Path) -> Result<()> {
        StdFileSystem.delete_if_exists(path)
    }
}
#[test]
fn source_changed_during_staging_must_not_be_overwritten() {
    let t = Temp::new();
    let source = t.0.join("saves/test.sav");
    fs::write(&source, OLD).unwrap();
    let result = replace_with_file_system(
        &SourceChangesDuringPermissionCopy,
        &source,
        &sse_codecs::sha256::sha256_hex(OLD),
        NEW,
        &t.0.join("backups"),
    );
    assert!(result.is_err(), "a changed source must abort the transaction");
    assert_eq!(fs::read(&source).unwrap(), EXTERNAL);
    assert!(list_backups(&t.0.join("backups")).unwrap().is_empty());
}
#[test]
fn valid_unicode_filename_must_be_saveable() {
    let t = Temp::new();
    let source = t.0.join("saves").join(format!("{}.sav", "界".repeat(60)));
    fs::write(&source, OLD).unwrap();
    let result = replace_transaction(&source, &sse_codecs::sha256::sha256_hex(OLD), NEW, &t.0.join("backups"));
    assert!(result.is_ok(), "valid 184-byte filename cannot be saved: {result:?}");
}
#[cfg(unix)]
#[test]
fn non_utf8_source_path_must_restore_to_the_original_path() {
    use std::os::unix::ffi::OsStringExt;
    let t = Temp::new();
    let source =
        t.0.join("saves")
            .join(std::ffi::OsString::from_vec(b"slot-\xff.sav".to_vec()));
    fs::write(&source, OLD).unwrap();
    let receipt =
        replace_transaction(&source, &sse_codecs::sha256::sha256_hex(OLD), NEW, &t.0.join("backups")).unwrap();
    let restored = restore_in_place(&receipt.journal_path).unwrap();
    assert_eq!(
        restored.save_path, source,
        "journal changed the source path through lossy UTF-8 conversion"
    );
    assert_eq!(fs::read(&source).unwrap(), OLD);
}
struct CrashFs {
    step: std::cell::Cell<usize>,
    stop: usize,
}
impl CrashFs {
    fn tick(&self) {
        let step = self.step.get().checked_add(1).expect("operation counter overflow");
        self.step.set(step);
        if step == self.stop {
            std::process::exit(86);
        }
    }
}
impl FileSystem for CrashFs {
    fn read_all(&self, p: &Path) -> Result<Vec<u8>> {
        self.tick();
        StdFileSystem.read_all(p)
    }
    fn is_symlink(&self, p: &Path) -> Result<bool> {
        self.tick();
        StdFileSystem.is_symlink(p)
    }
    fn create_dir_all(&self, p: &Path) -> Result<()> {
        self.tick();
        StdFileSystem.create_dir_all(p)
    }
    fn write_new(&self, p: &Path, b: &[u8]) -> Result<()> {
        self.tick();
        StdFileSystem.write_new(p, b)
    }
    fn copy_permissions(&self, s: &Path, d: &Path) -> Result<()> {
        self.tick();
        StdFileSystem.copy_permissions(s, d)
    }
    fn replace(&self, s: &Path, d: &Path) -> Result<()> {
        self.tick();
        StdFileSystem.replace(s, d)
    }
    fn delete_if_exists(&self, p: &Path) -> Result<()> {
        StdFileSystem.delete_if_exists(p)
    }
}
#[test]
#[ignore]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("AUDIT_K_CRASH_ROOT").unwrap());
    let stop = std::env::var("AUDIT_K_CRASH_STEP").unwrap().parse().unwrap();
    let files = CrashFs {
        step: std::cell::Cell::new(0),
        stop,
    };
    let _ = replace_with_file_system(
        &files,
        &root.join("saves/test.sav"),
        &sse_codecs::sha256::sha256_hex(OLD),
        NEW,
        &root.join("backups"),
    );
}
#[test]
fn process_interruptions_preserve_whole_source_at_every_step() {
    for step in 1..=13 {
        let t = Temp::new();
        let source = t.0.join("saves/test.sav");
        fs::write(&source, OLD).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "crash_child", "--ignored"])
            .env("AUDIT_K_CRASH_ROOT", &t.0)
            .env("AUDIT_K_CRASH_STEP", step.to_string())
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        let bytes = fs::read(&source).unwrap();
        assert!(bytes == OLD || bytes == NEW, "torn source after step {step}");
        let entries = list_backups(&t.0.join("backups")).unwrap();
        eprintln!(
            "crash before operation {step}: original={} entries={:?}",
            bytes == OLD,
            entries.iter().map(|e| (&e.status, &e.error)).collect::<Vec<_>>()
        );
    }
}
#[test]
fn interrupted_replacement_must_allow_recovery_of_intact_backup() {
    let t = Temp::new();
    let source = t.0.join("saves/test.sav");
    fs::write(&source, OLD).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "crash_child", "--ignored"])
        .env("AUDIT_K_CRASH_ROOT", &t.0)
        .env("AUDIT_K_CRASH_STEP", "13")
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(86));
    assert_eq!(fs::read(&source).unwrap(), NEW);
    let temporary = fs::read_dir(t.0.join("backups"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().ends_with(".json.tmp"))
        })
        .expect("interrupted replacement should leave its temporary verified journal");
    let journal = fs::read_dir(t.0.join("backups"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let result = restore_in_place(&journal);
    assert!(
        result.is_ok(),
        "intact backup cannot be recovered after interruption: {result:?}"
    );
    assert_eq!(fs::read(&source).unwrap(), OLD);
    assert!(
        !temporary.exists(),
        "explicit recovery must remove its hash-verified stale temporary"
    );
}
