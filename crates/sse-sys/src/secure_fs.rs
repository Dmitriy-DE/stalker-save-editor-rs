//! Secure filesystem primitives for files that may be executed by privileged helpers.

use sse_core::{Error, Result};
use std::fs::File;
#[cfg(unix)]
use std::fs::OpenOptions;
use std::io;
use std::path::Path;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
const LINUX_O_NOFOLLOW: Option<i32> = Some(0o400_000);
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
const LINUX_O_NOFOLLOW: Option<i32> = Some(0o100_000);
#[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
const LINUX_O_NOFOLLOW: Option<i32> = None;

/// Opens a regular file owned by the current user.
///
/// On Linux the final path component is opened with `O_NOFOLLOW`, so a symlink
/// cannot be substituted between validation and opening.
pub fn open_owned_regular(path: &Path) -> Result<File> {
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

        let Some(no_follow_flag) = LINUX_O_NOFOLLOW else {
            return Err(Error::Refused(
                "Secure file opening is unavailable on this Linux architecture".to_owned(),
            ));
        };
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(no_follow_flag)
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

/// Atomically moves a staged file into a new path without replacing an existing file.
pub fn publish_new(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        publish_linux::rename_no_replace(source, destination)
    }
    #[cfg(target_os = "macos")]
    {
        publish_macos::rename_no_replace(source, destination)
    }
    #[cfg(windows)]
    {
        publish_windows::rename_no_replace(source, destination)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        std::fs::hard_link(source, destination)?;
        std::fs::remove_file(source)
    }
}

/// Atomically replaces an existing file with a staged file.
pub fn replace_existing(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        publish_windows::replace_existing(source, destination)
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(source, destination)
    }
}

#[cfg(target_os = "linux")]
mod publish_linux {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const AT_FDCWD: i32 = -100;
    const RENAME_NOREPLACE: u32 = 1;

    unsafe extern "C" {
        fn renameat2(
            old_directory: i32,
            old_path: *const i8,
            new_directory: i32,
            new_path: *const i8,
            flags: u32,
        ) -> i32;
    }

    pub(super) fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
        let source = CString::new(source.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
        let destination = CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL"))?;
        // SAFETY: both pointers are NUL-terminated, AT_FDCWD resolves the absolute paths, and flags request no replacement.
        let status = unsafe {
            renameat2(
                AT_FDCWD,
                source.as_ptr(),
                AT_FDCWD,
                destination.as_ptr(),
                RENAME_NOREPLACE,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(target_os = "macos")]
mod publish_macos {
    use std::ffi::CString;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::path::Path;

    const RENAME_EXCL: u32 = 0x0000_0004;

    unsafe extern "C" {
        fn renamex_np(old_path: *const i8, new_path: *const i8, flags: u32) -> i32;
    }

    pub(super) fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
        let source = CString::new(source.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "source path contains NUL"))?;
        let destination = CString::new(destination.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "destination path contains NUL"))?;
        // SAFETY: both pointers are NUL-terminated and RENAME_EXCL prevents replacing a destination.
        let status = unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), RENAME_EXCL) };
        if status == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }
}

#[cfg(windows)]
mod publish_windows {
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }

    pub(super) fn rename_no_replace(source: &Path, destination: &Path) -> io::Result<()> {
        let source = wide(source);
        let destination = wide(destination);
        // SAFETY: both paths are NUL-terminated UTF-16 buffers. Omitting REPLACE_EXISTING preserves an existing target.
        let status = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), MOVEFILE_WRITE_THROUGH) };
        if status != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    pub(super) fn replace_existing(source: &Path, destination: &Path) -> io::Result<()> {
        let source = wide(source);
        let destination = wide(destination);
        let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
        // SAFETY: both paths are NUL-terminated UTF-16 buffers. The flags request replacement and wait for the move to finish.
        let status = unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), flags) };
        if status != 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    fn wide(path: &Path) -> Vec<u16> {
        let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
        value.push(0);
        value
    }
}

/// Copies the discretionary ACL from a save onto its staged replacement.
///
/// ACL-copy failures are fatal because they can change who may access the replacement.
pub fn copy_dacl(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        windows_security::copy_dacl(source, destination)
    }
    #[cfg(not(windows))]
    {
        let _ = (source, destination);
        Ok(())
    }
}

/// Attempts to copy native owner and group metadata from a save onto its staged replacement.
///
/// Failure is best effort: ordinary users generally cannot assign another file's owner.
pub fn copy_owner_and_group(source: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;

        let source_metadata = std::fs::metadata(source)?;
        let destination_file = OpenOptions::new().write(true).open(destination)?;
        let destination_metadata = destination_file.metadata()?;
        if source_metadata.uid() == destination_metadata.uid() && source_metadata.gid() == destination_metadata.gid() {
            return Ok(());
        }

        unsafe extern "C" {
            fn fchown(fd: i32, owner: u32, group: u32) -> i32;
        }
        // SAFETY: the descriptor belongs to the open destination file; fchown reads only its fd and scalar IDs.
        let result = unsafe {
            fchown(
                destination_file.as_raw_fd(),
                source_metadata.uid(),
                source_metadata.gid(),
            )
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    #[cfg(windows)]
    {
        let _ = windows_security::copy_owner_group(source, destination);
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (source, destination);
        Ok(())
    }
}

#[cfg(windows)]
mod windows_security {
    use std::ffi::c_void;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    const SE_FILE_OBJECT: u32 = 1;
    const OWNER_SECURITY_INFORMATION: u32 = 0x0000_0001;
    const GROUP_SECURITY_INFORMATION: u32 = 0x0000_0002;
    const DACL_SECURITY_INFORMATION: u32 = 0x0000_0004;

    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn GetNamedSecurityInfoW(
            object_name: *mut u16,
            object_type: u32,
            security_info: u32,
            owner: *mut *mut c_void,
            group: *mut *mut c_void,
            dacl: *mut *mut c_void,
            sacl: *mut *mut c_void,
            descriptor: *mut *mut c_void,
        ) -> u32;
        fn SetNamedSecurityInfoW(
            object_name: *mut u16,
            object_type: u32,
            security_info: u32,
            owner: *mut c_void,
            group: *mut c_void,
            dacl: *mut c_void,
            sacl: *mut c_void,
        ) -> u32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }

    pub(super) fn copy_dacl(source: &Path, destination: &Path) -> io::Result<()> {
        let mut source_name = wide(source);
        let mut destination_name = wide(destination);
        let security_info = DACL_SECURITY_INFORMATION;
        let mut dacl = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: all output pointers refer to initialized local pointer slots; source_name is NUL-terminated.
        let read_status = unsafe {
            GetNamedSecurityInfoW(
                source_name.as_mut_ptr(),
                SE_FILE_OBJECT,
                security_info,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut dacl,
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if read_status != 0 {
            if !descriptor.is_null() {
                // SAFETY: a non-null descriptor returned by GetNamedSecurityInfoW is owned by the caller.
                let _ = unsafe { LocalFree(descriptor) };
            }
            return Err(io::Error::other(format!(
                "GetNamedSecurityInfoW failed with Windows error {read_status}"
            )));
        }
        if descriptor.is_null() {
            return Err(io::Error::other(
                "GetNamedSecurityInfoW returned no security descriptor",
            ));
        }

        // SAFETY: successful GetNamedSecurityInfoW allocated descriptor and returned valid owner/group/DACL pointers into it.
        let write_status = unsafe {
            SetNamedSecurityInfoW(
                destination_name.as_mut_ptr(),
                SE_FILE_OBJECT,
                security_info,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                dacl,
                std::ptr::null_mut(),
            )
        };
        // SAFETY: descriptor is the allocation returned by GetNamedSecurityInfoW and has not been freed yet.
        let _ = unsafe { LocalFree(descriptor) };
        if write_status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "SetNamedSecurityInfoW failed with Windows error {write_status}"
            )))
        }
    }

    pub(super) fn copy_owner_group(source: &Path, destination: &Path) -> io::Result<()> {
        let mut source_name = wide(source);
        let mut destination_name = wide(destination);
        let security_info = OWNER_SECURITY_INFORMATION | GROUP_SECURITY_INFORMATION;
        let mut owner = std::ptr::null_mut();
        let mut group = std::ptr::null_mut();
        let mut descriptor = std::ptr::null_mut();
        // SAFETY: all output pointers refer to initialized local pointer slots; source_name is NUL-terminated.
        let read_status = unsafe {
            GetNamedSecurityInfoW(
                source_name.as_mut_ptr(),
                SE_FILE_OBJECT,
                security_info,
                &mut owner,
                &mut group,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut descriptor,
            )
        };
        if read_status != 0 {
            if !descriptor.is_null() {
                // SAFETY: a non-null descriptor returned by GetNamedSecurityInfoW is owned by the caller.
                let _ = unsafe { LocalFree(descriptor) };
            }
            return Err(io::Error::other(format!(
                "GetNamedSecurityInfoW failed with Windows error {read_status}"
            )));
        }
        if descriptor.is_null() {
            return Err(io::Error::other(
                "GetNamedSecurityInfoW returned no security descriptor",
            ));
        }
        // SAFETY: successful GetNamedSecurityInfoW allocated descriptor and returned valid owner/group pointers into it.
        let write_status = unsafe {
            SetNamedSecurityInfoW(
                destination_name.as_mut_ptr(),
                SE_FILE_OBJECT,
                security_info,
                owner,
                group,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        // SAFETY: descriptor is the allocation returned by GetNamedSecurityInfoW and has not been freed yet.
        let _ = unsafe { LocalFree(descriptor) };
        if write_status == 0 {
            Ok(())
        } else {
            Err(io::Error::other(format!(
                "SetNamedSecurityInfoW failed with Windows error {write_status}"
            )))
        }
    }

    fn wide(path: &Path) -> Vec<u16> {
        let mut value = path.as_os_str().encode_wide().collect::<Vec<_>>();
        value.push(0);
        value
    }
}

#[cfg(target_os = "linux")]
fn current_effective_uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    // SAFETY: geteuid takes no arguments and has no preconditions.
    unsafe { geteuid() }
}

#[cfg(test)]
mod tests {
    use super::publish_new;
    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    use super::{open_owned_regular, LINUX_O_NOFOLLOW};

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    #[test]
    fn x86_64_uses_the_linux_o_nofollow_value() {
        assert_eq!(LINUX_O_NOFOLLOW, Some(0o400_000));
    }

    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    #[test]
    fn aarch64_uses_the_linux_o_nofollow_value() {
        assert_eq!(LINUX_O_NOFOLLOW, Some(0o100_000));
    }

    #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
    #[test]
    fn refuses_a_symlink_as_the_final_file_component() -> std::io::Result<()> {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-secure-fs-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let target = directory.join("target");
        let link = directory.join("link");
        std::fs::write(&target, b"fixture")?;
        symlink(&target, &link)?;

        let result = open_owned_regular(&link);
        std::fs::remove_dir_all(directory)?;
        assert!(result.is_err(), "final-component symlink unexpectedly opened");
        Ok(())
    }

    #[test]
    fn publishes_without_hard_links_and_never_replaces_an_existing_file() -> std::io::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-publish-new-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let source = directory.join("staged");
        let destination = directory.join("published");
        std::fs::write(&source, b"staged bytes")?;

        publish_new(&source, &destination)?;
        assert!(!source.exists());
        assert_eq!(std::fs::read(&destination)?, b"staged bytes");

        std::fs::write(&source, b"replacement bytes")?;
        assert!(publish_new(&source, &destination).is_err());
        assert_eq!(std::fs::read(&source)?, b"replacement bytes");
        assert_eq!(std::fs::read(&destination)?, b"staged bytes");
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }
}
