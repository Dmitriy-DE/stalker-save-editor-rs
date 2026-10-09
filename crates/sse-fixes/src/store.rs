//! Content-addressed store for fix-pack whole-file overlays.

use std::fs;
use std::path::{Path, PathBuf};

use sse_codecs::sha256::sha256_hex;
use sse_core::Result;

use crate::fs_util::AtomicFileWriter;

/// Content-addressed cache of fix-pack files named by SHA-256.
pub struct GameFixContentStore;

impl GameFixContentStore {
    /// Default fix-pack store directory in user's data folder.
    #[must_use]
    pub fn default_directory() -> PathBuf {
        if is_test_or_example_executable() {
            return test_data_root().join("content").join("fixpacks");
        }
        if let Ok(path) = std::env::var("STALKER_SAVE_EDITOR_DATA") {
            if !path.trim().is_empty() {
                return PathBuf::from(path).join("content").join("fixpacks");
            }
        }
        if let Ok(path) = std::env::var("XDG_DATA_HOME") {
            if !path.is_empty() {
                return PathBuf::from(path)
                    .join("stalker-save-editor")
                    .join("content")
                    .join("fixpacks");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() {
                return PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("stalker-save-editor")
                    .join("content")
                    .join("fixpacks");
            }
        }
        std::env::temp_dir()
            .join("stalker-save-editor")
            .join("content")
            .join("fixpacks")
    }

    /// Reads a file from the content store and verifies its hash.
    #[must_use]
    pub fn read(store_dir: &Path, sha256: &str) -> Option<Vec<u8>> {
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }

        let path = store_dir.join(sha256);
        let bytes = fs::read(&path).ok()?;
        let actual_hash = sha256_hex(&bytes);
        if actual_hash.eq_ignore_ascii_case(sha256) {
            Some(bytes)
        } else {
            None
        }
    }

    /// Adds bytes to the content store under their SHA-256 hash.
    ///
    /// # Errors
    /// Returns [`Error::System`] on I/O failure.
    pub fn add(store_dir: &Path, bytes: &[u8]) -> Result<String> {
        let hash = sha256_hex(bytes);
        let path = store_dir.join(&hash);
        if !path.exists() {
            AtomicFileWriter::write(&path, bytes, false)?;
        }
        Ok(hash)
    }

    /// Returns true if the store contains a valid file for `sha256`.
    #[must_use]
    pub fn contains(store_dir: &Path, sha256: &str) -> bool {
        Self::read(store_dir, sha256).is_some()
    }
}

fn is_test_or_example_executable() -> bool {
    cfg!(test)
        || std::env::current_exe()
            .ok()
            .and_then(|executable| executable.parent().map(Path::to_path_buf))
            .and_then(|parent| parent.file_name().map(|name| name.to_owned()))
            .is_some_and(|name| name == "deps" || name == "examples")
}

fn test_data_root() -> PathBuf {
    if let Some(custom) = std::env::var_os("STALKER_SAVE_EDITOR_DATA")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        if let (Ok(temporary), Ok(custom_path)) = (fs::canonicalize(std::env::temp_dir()), fs::canonicalize(&custom)) {
            if custom_path.starts_with(temporary) {
                return custom;
            }
        }
    }
    std::env::temp_dir().join(format!("stalker-save-editor-test-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::GameFixContentStore;

    #[test]
    fn test_executable_fixpack_store_is_temporary() {
        assert!(
            GameFixContentStore::default_directory().starts_with(std::env::temp_dir()),
            "test fixpack data must stay under the temporary directory"
        );
    }
}
