//! CLI backup routing checks using only temporary settings and synthetic save fixtures.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new() -> std::io::Result<Self> {
        let id = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("sse-cli-backup-path-{}-{id}", std::process::id()));
        fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        std::mem::drop(fs::remove_dir_all(&self.0));
    }
}

fn run_money_edit(root: &Path, data_directory: &Path, suffix: &str) -> std::io::Result<()> {
    let saves = root.join("saves");
    fs::create_dir_all(&saves)?;
    let source = saves.join(format!("source-{suffix}.sav"));
    let output = saves.join(format!("edited-{suffix}.sav"));
    fs::write(
        &source,
        include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav"),
    )?;
    let result = Command::new(env!("CARGO_BIN_EXE_stalker-save"))
        .current_dir(root)
        .env("STALKER_SAVE_EDITOR_DATA", data_directory)
        .arg("set-money")
        .arg(&source)
        .arg("876543")
        .arg("--output")
        .arg(&output)
        .output()?;

    assert!(
        result.status.success(),
        "CLI failed: {}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file(), "CLI output save is missing");
    Ok(())
}

fn journal_count(directory: &Path) -> std::io::Result<usize> {
    let mut count = 0_usize;
    for entry in fs::read_dir(directory)? {
        if entry?.file_name().to_string_lossy().ends_with("_ORIGINAL.json") {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

#[test]
fn cli_uses_settings_override_and_stalker_data_directory_for_backups() -> std::io::Result<()> {
    let temporary = TempDirectory::new()?;

    let override_data = temporary.0.join("override-data");
    fs::create_dir_all(&override_data)?;
    fs::write(
        override_data.join("settings.json"),
        br#"{"backup_directory":"configured-backups"}"#,
    )?;
    run_money_edit(&temporary.0, &override_data, "override")?;
    assert_eq!(journal_count(&temporary.0.join("configured-backups"))?, 1);
    assert!(!override_data.join("backups").exists());

    let default_data = temporary.0.join("default-data");
    fs::create_dir_all(&default_data)?;
    fs::write(default_data.join("settings.json"), b"{}")?;
    run_money_edit(&temporary.0, &default_data, "default")?;
    assert_eq!(journal_count(&default_data.join("backups"))?, 1);
    Ok(())
}
