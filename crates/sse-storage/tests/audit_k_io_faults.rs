#![allow(clippy::unwrap_used, clippy::expect_used, missing_docs)]
use sse_core::{Error, Result};
use sse_storage::transaction::{replace_transaction, replace_with_file_system, FileSystem, StdFileSystem};
use std::{
    cell::Cell,
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const OLD: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
const NEW: &[u8] = include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
struct ErrorFs {
    step: Cell<usize>,
    fail: usize,
}
impl ErrorFs {
    fn tick(&self) -> Result<()> {
        let step = self.step.get().checked_add(1).expect("operation counter overflow");
        self.step.set(step);
        if step == self.fail {
            Err(Error::System(format!("injected failure at operation {step}")))
        } else {
            Ok(())
        }
    }
}
impl FileSystem for ErrorFs {
    fn read_all(&self, p: &Path) -> Result<Vec<u8>> {
        self.tick()?;
        StdFileSystem.read_all(p)
    }
    fn is_symlink(&self, p: &Path) -> Result<bool> {
        self.tick()?;
        StdFileSystem.is_symlink(p)
    }
    fn create_dir_all(&self, p: &Path) -> Result<()> {
        self.tick()?;
        StdFileSystem.create_dir_all(p)
    }
    fn write_new(&self, p: &Path, b: &[u8]) -> Result<()> {
        self.tick()?;
        StdFileSystem.write_new(p, b)
    }
    fn copy_permissions(&self, s: &Path, d: &Path) -> Result<()> {
        self.tick()?;
        StdFileSystem.copy_permissions(s, d)
    }
    fn replace(&self, s: &Path, d: &Path) -> Result<()> {
        self.tick()?;
        StdFileSystem.replace(s, d)
    }
    fn delete_if_exists(&self, p: &Path) -> Result<()> {
        StdFileSystem.delete_if_exists(p)
    }
}
#[test]
fn each_io_failure_preserves_source_and_allows_retry() {
    for step in 1..=13 {
        let root = std::env::temp_dir().join(format!(
            "audit-k-io-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let saves = root.join("saves");
        fs::create_dir_all(&saves).unwrap();
        let source = saves.join("test.sav");
        fs::write(&source, OLD).unwrap();
        let hash = sse_codecs::sha256::sha256_hex(OLD);
        let backups = root.join("backups");
        let injector = ErrorFs {
            step: Cell::new(0),
            fail: step,
        };
        let result = replace_with_file_system(&injector, &source, &hash, NEW, &backups);
        assert!(result.is_err(), "step {step} unexpectedly succeeded");
        assert_eq!(fs::read(&source).unwrap(), OLD, "source changed at failed step {step}");
        let retry = replace_transaction(&source, &hash, NEW, &backups);
        assert!(retry.is_ok(), "retry failed after step {step}: {retry:?}");
        assert_eq!(fs::read(&source).unwrap(), NEW);
        fs::remove_dir_all(root).unwrap();
    }
}
