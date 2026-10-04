//! Secure filesystem primitives for files that may be executed by privileged helpers.

use sse_core::{Error, Result};
use std::fs::{File, OpenOptions};
use std::path::Path;

/// Opens a regular file owned by the current user.
///
/// On Linux the final path component is opened with `O_NOFOLLOW`, so a symlink
/// cannot be substituted between validation and opening.
pub fn open_owned_regular(path: &Path) -> Result<File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

        const O_NOFOLLOW: i32 = 0o400_000;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(O_NOFOLLOW)
            .open(path)
            .map_err(Error::from)?;
        let metadata = file.metadata().map_err(Error::from)?;
        if !metadata.file_type().is_file() {
            return Err(Error::Refused("Update package is not a regular file".to_owned()));
        }
        if metadata.uid() != current_effective_uid() {
            return Err(Error::Refused(
                "Update package is not owned by the current user".to_owned(),
            ));
        }
        Ok(file)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let file = File::open(path).map_err(Error::from)?;
        if !file.metadata().map_err(Error::from)?.file_type().is_file() {
            return Err(Error::Refused("Update package is not a regular file".to_owned()));
        }
        Ok(file)
    }
}

/// Verifies that a directory is owned by the current user on Linux.
pub fn verify_directory_owner(path: &Path) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::MetadataExt;

        let metadata = std::fs::symlink_metadata(path).map_err(Error::from)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Refused("Update directory is not a real directory".to_owned()));
        }
        if metadata.uid() != current_effective_uid() {
            return Err(Error::Refused(
                "Update directory is not owned by the current user".to_owned(),
            ));
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        if !std::fs::metadata(path).map_err(Error::from)?.is_dir() {
            return Err(Error::Refused("Update directory is not a directory".to_owned()));
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn current_effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid takes no arguments and has no preconditions.
    unsafe { geteuid() }
}
