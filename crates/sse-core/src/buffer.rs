use std::path::Path;
use std::sync::Arc;

use crate::{Error, Result};

/// The bytes of one save image, owned once and shared by reference.
///
/// The C# editor copied a save at almost every boundary (reader snapshot, writer input, stage hand-off, result):
/// hundreds of megabytes for a large save. Here a file is read into one buffer; readers borrow it, a writer
/// produces one new buffer, and cloning a `SaveBuffer` only bumps a reference count.
#[derive(Debug, Clone)]
pub struct SaveBuffer {
    bytes: Arc<[u8]>,
}

impl SaveBuffer {
    /// Largest file the editor reads as a save.
    pub const MAXIMUM_FILE_BYTES: u64 = 512 * 1024 * 1024;

    /// Takes ownership of bytes the caller has just produced; nothing is copied.
    #[must_use]
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::from(bytes),
        }
    }

    /// Reads a file in one piece, refusing anything larger than [`Self::MAXIMUM_FILE_BYTES`].
    ///
    /// # Errors
    /// [`Error::System`] when the file cannot be read, [`Error::Refused`] when it is too large.
    pub fn read(path: &Path) -> Result<Self> {
        let length = std::fs::metadata(path)?.len();
        if length > Self::MAXIMUM_FILE_BYTES {
            return Err(Error::Refused(format!(
                "{} is larger than a save can be ({length} bytes)",
                path.display()
            )));
        }
        Ok(Self::from_vec(std::fs::read(path)?))
    }

    /// The bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    /// Length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// True for an empty buffer.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::SaveBuffer;

    #[test]
    fn a_clone_shares_the_same_bytes() {
        let first = SaveBuffer::from_vec(vec![1, 2, 3]);
        let second = first.clone();
        assert!(std::ptr::eq(first.as_slice().as_ptr(), second.as_slice().as_ptr()));
        assert_eq!(second.len(), 3);
        assert!(!second.is_empty());
    }
}
