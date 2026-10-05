//! Safe transactional filesystem utilities and link traversal protection.

use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(test)]
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use sse_core::{Error, Result};

static COUNTER: AtomicU64 = AtomicU64::new(1);

#[cfg(test)]
thread_local! {
    static TEST_FAIL_WRITE_NUMBER: Cell<Option<usize>> = const { Cell::new(None) };
    static TEST_WRITE_NUMBER: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn fail_atomic_write_number_for_test(number: usize) {
    TEST_WRITE_NUMBER.with(|value| value.set(0));
    TEST_FAIL_WRITE_NUMBER.with(|value| value.set(Some(number)));
}

#[cfg(test)]
fn should_fail_atomic_write_for_test() -> bool {
    let number = TEST_WRITE_NUMBER.with(|value| {
        let next = value.get().saturating_add(1);
        value.set(next);
        next
    });
    TEST_FAIL_WRITE_NUMBER.with(|value| {
        if value.get() == Some(number) {
            value.set(None);
            true
        } else {
            false
        }
    })
}

/// Atomic file writer that creates a sibling temporary file, flushes it to disk,
/// and renames it over the target.
pub struct AtomicFileWriter;

impl AtomicFileWriter {
    /// Writes `bytes` to `path` atomically.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Refused`] on I/O error or if `overwrite` is false and target exists.
    pub fn write(path: &Path, bytes: &[u8], overwrite: bool) -> Result<()> {
        #[cfg(test)]
        if should_fail_atomic_write_for_test() {
            return Err(Error::System("injected atomic write failure".to_owned()));
        }

        if !overwrite && path.exists() {
            return Err(Error::Refused(format!(
                "Refusing to overwrite existing file: {}",
                path.display()
            )));
        }

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::System(e.to_string()))?;
        }

        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let temp_name = format!(
            "{}.tmp-{nanos:x}-{id:x}",
            path.file_name().unwrap_or_default().to_string_lossy()
        );
        let temp_path = path.with_file_name(temp_name);

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .map_err(|e| Error::System(e.to_string()))?;

        if file.write_all(bytes).is_err() || file.sync_all().is_err() {
            let _ = fs::remove_file(&temp_path);
            return Err(Error::System("Failed to write temporary file".to_string()));
        }
        drop(file);

        if fs::rename(&temp_path, path).is_err() {
            let _ = fs::remove_file(&temp_path);
            return Err(Error::System("Failed to rename temporary file".to_string()));
        }

        Ok(())
    }
}

/// Normalizes a game-relative path to use forward slashes and verifies safety.
///
/// # Errors
/// Returns [`Error::Damaged`] or [`Error::Refused`] if path is empty, rooted, traverses,
/// or touches application state folders.
pub fn normalize_relative_path(path: &str) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() || trimmed.contains(':') {
        return Err(Error::damaged("Path cannot be empty or contain drive colons"));
    }

    let normalized = trimmed.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(Error::damaged("Path cannot start with a slash"));
    }

    let segments: Vec<&str> = normalized.split('/').collect();
    if segments.is_empty() {
        return Err(Error::damaged("Path has no components"));
    }

    for segment in &segments {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return Err(Error::damaged("Path contains an empty or traversing component"));
        }
    }

    let first_segment = segments
        .first()
        .ok_or_else(|| Error::damaged("Path has no components after normalisation"))?;
    if first_segment.to_ascii_lowercase().starts_with(".save-editor-") {
        return Err(Error::damaged(
            "Game fixes cannot target application-managed state directories",
        ));
    }

    Ok(segments.join("/"))
}

/// Resolves a game-relative path against the root directory and checks against path traversal.
///
/// # Errors
/// Returns [`Error::Damaged`] or [`Error::System`] if the resolved path escapes root.
pub fn resolve_game_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative)?;
    let mut result = root.to_path_buf();
    for segment in normalized.split('/') {
        result.push(segment);
    }

    check_no_links(root, &result)?;
    Ok(result)
}

/// Resolves a state-relative path against the fix state directory.
///
/// # Errors
/// Returns [`Error::Damaged`] or [`Error::System`] if the path escapes.
pub fn resolve_state_path(state_dir: &Path, relative: &str) -> Result<PathBuf> {
    let normalized = normalize_relative_path(relative)?;
    let mut result = state_dir.to_path_buf();
    for segment in normalized.split('/') {
        result.push(segment);
    }

    check_no_links(state_dir, &result)?;
    Ok(result)
}

/// Checks that neither root nor any component leading to target is a symlink or reparse point.
///
/// # Errors
/// Returns [`Error::Refused`] if a symlink or reparse point is detected.
pub fn check_no_links(root: &Path, target: &Path) -> Result<()> {
    if is_symlink_or_reparse(root) {
        return Err(Error::Refused(
            "Refusing to operate through a linked directory".to_string(),
        ));
    }

    let Ok(rel) = target.strip_prefix(root) else {
        return Err(Error::damaged("Target path escapes root"));
    };

    let mut current = root.to_path_buf();
    for comp in rel.components() {
        current.push(comp);
        if is_symlink_or_reparse(&current) {
            return Err(Error::Refused(format!(
                "Refusing to operate through a link: {}",
                current.display()
            )));
        }
    }

    Ok(())
}

fn is_symlink_or_reparse(path: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    meta.file_type().is_symlink()
}
