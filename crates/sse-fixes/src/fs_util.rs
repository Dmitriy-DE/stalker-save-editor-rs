//! Safe transactional filesystem utilities and link traversal protection.

#[cfg(test)]
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

use sse_core::{Error, Result};

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

        if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|e| Error::System(e.to_string()))?;
        }

        let options = if overwrite {
            sse_sys::secure_fs::AtomicWriteOptions::create_or_replace()
        } else {
            sse_sys::secure_fs::AtomicWriteOptions::create_new()
        }
        .without_parent_sync();

        match sse_sys::secure_fs::atomic_write(path, bytes, options) {
            Ok(()) => Ok(()),
            Err(_error) if !overwrite && fs::symlink_metadata(path).is_ok() => Err(Error::Refused(format!(
                "Refusing to overwrite existing file: {}",
                path.display()
            ))),
            Err(error) => Err(Error::System(error.to_string())),
        }
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
    if meta.file_type().is_symlink() {
        return true;
    }
    // Junctions and other mount points are reparse points but not symlinks, so `is_symlink` misses them.
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(all(test, windows))]
mod reparse_point_tests {
    use super::is_symlink_or_reparse;
    use std::fs;

    #[test]
    fn a_junction_is_treated_as_a_link_but_a_plain_directory_is_not() -> std::io::Result<()> {
        let root = std::env::temp_dir().join(format!("sse-junction-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let target = root.join("target");
        let junction = root.join("junction");
        fs::create_dir_all(&target)?;
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .status()?;
        let plain_is_link = is_symlink_or_reparse(&target);
        let junction_is_link = status.success() && is_symlink_or_reparse(&junction);
        let _ = fs::remove_dir_all(&root);
        assert!(status.success(), "mklink /J must create the junction for this test");
        assert!(!plain_is_link);
        assert!(junction_is_link);
        Ok(())
    }
}

#[cfg(test)]
mod atomic_writer_tests {
    use super::AtomicFileWriter;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn no_overwrite_keeps_existing_bytes_and_staging_clean() -> std::io::Result<()> {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-fixes-atomic-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("existing.json");
        std::fs::write(&destination, b"original")?;

        assert!(AtomicFileWriter::write(&destination, b"replacement", false).is_err());
        assert_eq!(std::fs::read(&destination)?, b"original");
        assert_eq!(std::fs::read_dir(&directory)?.count(), 1);

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
