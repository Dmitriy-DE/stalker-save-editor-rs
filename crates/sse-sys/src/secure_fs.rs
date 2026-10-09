//! Secure filesystem primitives for files that may be executed by privileged helpers.

use sse_core::{Error, Result};
use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ATOMIC_TEMP_ID: AtomicU64 = AtomicU64::new(1);

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

/// How an atomic write publishes its destination.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AtomicWriteMode {
    /// Create the destination only if it does not exist.
    CreateNew,
    /// Replace an existing destination, or create it without replacement if it is absent.
    CreateOrReplace,
}

/// Options for [`atomic_write`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtomicWriteOptions {
    mode: AtomicWriteMode,
    unix_mode: Option<u32>,
    sync_parent: bool,
}

struct StagedAtomicFile {
    path: std::path::PathBuf,
    file: Option<File>,
    cleanup_on_drop: bool,
}

impl Drop for StagedAtomicFile {
    fn drop(&mut self) {
        let path = &self.path;
        if self.cleanup_on_drop {
            drop_staged_file_before_cleanup(self.file.take(), || {
                let _ = std::fs::remove_file(path);
            });
        } else {
            drop(self.file.take());
        }
    }
}

impl AtomicWriteOptions {
    /// Creates a new destination and refuses to replace any existing path.
    #[must_use]
    pub const fn create_new() -> Self {
        Self {
            mode: AtomicWriteMode::CreateNew,
            unix_mode: None,
            sync_parent: true,
        }
    }

    /// Replaces an existing destination, or creates it if it is absent without replacing a path raced into place.
    #[must_use]
    pub const fn create_or_replace() -> Self {
        Self {
            mode: AtomicWriteMode::CreateOrReplace,
            unix_mode: None,
            sync_parent: true,
        }
    }

    /// Sets the permissions applied when creating the staged file on Unix.
    #[must_use]
    pub const fn with_unix_mode(mut self, mode: u32) -> Self {
        self.unix_mode = Some(mode);
        self
    }

    /// Leaves parent-directory synchronization to the caller after publication.
    #[must_use]
    pub const fn without_parent_sync(mut self) -> Self {
        self.sync_parent = false;
        self
    }
}

/// Writes and flushes a sibling temporary file, then atomically publishes it.
///
/// The destination's parent directory must already exist. On Unix the parent is synchronized after
/// publication unless [`AtomicWriteOptions::without_parent_sync`] is selected; on Windows the
/// underlying move uses `MOVEFILE_WRITE_THROUGH`.
///
/// # Errors
/// Returns the original filesystem error. If parent synchronization fails after publication, the error
/// states that the new file is already visible at the destination.
pub fn atomic_write(path: &Path, bytes: &[u8], options: AtomicWriteOptions) -> io::Result<()> {
    atomic_write_checked(path, bytes, options, |_| Ok(()))
}

/// Checked variant of [`atomic_write`] that validates the destination and staged path around publication.
///
/// The validator runs once before staging and again immediately before publication for both paths.
pub fn atomic_write_checked(
    path: &Path,
    bytes: &[u8],
    options: AtomicWriteOptions,
    validate_path: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    atomic_write_checked_with_writer(path, bytes, options, validate_path, |file, bytes| {
        file.write_all(bytes)?;
        file.sync_all()
    })
}

fn atomic_write_checked_with_writer(
    path: &Path,
    bytes: &[u8],
    options: AtomicWriteOptions,
    mut validate_path: impl FnMut(&Path) -> io::Result<()>,
    write_staged: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "atomic write destination has no file name"))?;
    validate_path(path)?;

    let mut staged = None;
    for _ in 0..128 {
        let id = NEXT_ATOMIC_TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(".");
        temporary_name.push(file_name);
        temporary_name.push(format!(".sse-tmp-{}-{id}", std::process::id()));
        let temporary = parent.join(temporary_name);
        validate_path(&temporary)?;

        let mut open_options = OpenOptions::new();
        open_options.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(mode) = options.unix_mode {
            use std::os::unix::fs::OpenOptionsExt;
            open_options.mode(mode);
        }

        match open_options.open(&temporary) {
            Ok(file) => {
                #[cfg(unix)]
                if let Some(mode) = options.unix_mode {
                    use std::os::unix::fs::PermissionsExt;
                    if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(mode)) {
                        drop(file);
                        let _ = std::fs::remove_file(&temporary);
                        return Err(error);
                    }
                }
                staged = Some(StagedAtomicFile {
                    path: temporary,
                    file: Some(file),
                    cleanup_on_drop: true,
                });
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    let Some(mut staged) = staged else {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique atomic-write temporary file",
        ));
    };

    (|| {
        let file = staged
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("atomic write staging file was already closed"))?;
        write_staged(file, bytes)?;
        drop(staged.file.take());

        validate_path(path)?;
        validate_path(&staged.path)?;
        let publish_result = match options.mode {
            AtomicWriteMode::CreateNew => publish_new(&staged.path, path),
            AtomicWriteMode::CreateOrReplace => match std::fs::symlink_metadata(path) {
                Ok(_) => replace_existing(&staged.path, path),
                Err(error) if error.kind() == io::ErrorKind::NotFound => publish_new(&staged.path, path),
                Err(error) => Err(error),
            },
        };
        publish_result?;
        staged.cleanup_on_drop = false;

        if options.sync_parent {
            if let Err(error) = sync_atomic_write_parent(parent) {
                return Err(io::Error::new(
                    error.kind(),
                    format!("atomic write was published, but syncing its parent directory failed: {error}"),
                ));
            }
        }
        Ok(())
    })()
}

fn sync_atomic_write_parent(parent: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = parent;
    Ok(())
}

fn drop_staged_file_before_cleanup<T>(file: Option<T>, cleanup: impl FnOnce()) {
    drop(file);
    cleanup();
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
    use super::atomic_write_checked_with_writer;
    use super::{atomic_write, publish_new, AtomicWriteOptions};
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

    #[test]
    fn atomic_write_create_new_never_replaces_an_existing_destination() -> std::io::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-write-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("state.json");
        std::fs::write(&destination, b"existing")?;

        let result = atomic_write(&destination, b"replacement", AtomicWriteOptions::create_new());
        assert_eq!(
            result.map_err(|error| error.kind()),
            Err(std::io::ErrorKind::AlreadyExists)
        );
        assert_eq!(std::fs::read(&destination)?, b"existing");

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn atomic_write_creates_or_replaces_the_destination() -> std::io::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-replace-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("settings.json");
        std::fs::write(&destination, b"old")?;

        atomic_write(&destination, b"new", AtomicWriteOptions::create_or_replace())?;
        assert_eq!(std::fs::read(&destination)?, b"new");
        assert_eq!(
            std::fs::read_dir(&directory)?.count(),
            1,
            "staging file should be removed"
        );

        let new_destination = directory.join("first-run.json");
        atomic_write(&new_destination, b"created", AtomicWriteOptions::create_or_replace())?;
        assert_eq!(std::fs::read(&new_destination)?, b"created");

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn atomic_write_can_leave_parent_synchronization_to_the_caller() -> std::io::Result<()> {
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-no-parent-sync-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("state.json");

        atomic_write(
            &destination,
            b"state",
            AtomicWriteOptions::create_new().without_parent_sync(),
        )?;
        assert_eq!(std::fs::read(&destination)?, b"state");
        assert_eq!(std::fs::read_dir(&directory)?.count(), 1);

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn concurrent_create_new_writers_have_one_winner() -> std::io::Result<()> {
        use std::sync::{Arc, Barrier};
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-race-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("first-run.json");
        let barrier = Arc::new(Barrier::new(3));
        let mut writers = Vec::new();
        for bytes in [b"writer-a".as_slice(), b"writer-b".as_slice()] {
            let destination = destination.clone();
            let barrier = Arc::clone(&barrier);
            writers.push(std::thread::spawn(move || {
                barrier.wait();
                atomic_write(&destination, bytes, AtomicWriteOptions::create_new())
            }));
        }
        barrier.wait();
        let results = writers
            .into_iter()
            .map(|writer| {
                writer
                    .join()
                    .unwrap_or_else(|_| Err(std::io::Error::other("writer thread panicked")))
            })
            .collect::<Vec<_>>();

        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(results.iter().filter(|result| result.is_err()).count(), 1);
        let final_bytes = std::fs::read(&destination)?;
        assert!(final_bytes == b"writer-a" || final_bytes == b"writer-b");

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_applies_requested_unix_permissions() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-mode-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("private.json");

        atomic_write(
            &destination,
            b"private",
            AtomicWriteOptions::create_new().with_unix_mode(0o600),
        )?;
        assert_eq!(std::fs::metadata(&destination)?.permissions().mode() & 0o777, 0o600);

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn checked_atomic_write_removes_staging_file_when_validation_fails() -> std::io::Result<()> {
        use super::atomic_write_checked;
        use std::cell::Cell;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-checked-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("managed.txt");
        std::fs::write(&destination, b"original")?;
        let validations = Cell::new(0_usize);

        let result = atomic_write_checked(
            &destination,
            b"replacement",
            AtomicWriteOptions::create_or_replace(),
            |_| {
                let count = validations.get().saturating_add(1);
                validations.set(count);
                if count == 4 {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected validation failure",
                    ))
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(
            result.map_err(|error| error.kind()),
            Err(std::io::ErrorKind::PermissionDenied)
        );
        assert_eq!(std::fs::read(&destination)?, b"original");
        assert_eq!(
            std::fs::read_dir(&directory)?.count(),
            1,
            "failed staging file should be removed"
        );

        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn atomic_write_removes_staging_file_after_write_failure() -> std::io::Result<()> {
        use std::io::Write;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-atomic-write-failure-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let destination = directory.join("settings.json");
        std::fs::write(&destination, b"original")?;

        let result = atomic_write_checked_with_writer(
            &destination,
            b"replacement",
            AtomicWriteOptions::create_or_replace(),
            |_| Ok(()),
            |file, _| {
                file.write_all(b"partial")?;
                Err(std::io::Error::other("injected write failure"))
            },
        );

        assert_eq!(result.map_err(|error| error.kind()), Err(std::io::ErrorKind::Other));
        assert_eq!(std::fs::read(&destination)?, b"original");
        assert_eq!(std::fs::read_dir(&directory)?.count(), 1);
        std::fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn atomic_staging_cleanup_closes_the_file_before_removing_it() {
        use super::drop_staged_file_before_cleanup;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Arc;

        struct DropMarker(Arc<AtomicBool>);

        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }

        let file_closed = Arc::new(AtomicBool::new(false));
        let cleanup_saw_closed_file = Arc::new(AtomicBool::new(false));
        let cleanup_flag = Arc::clone(&cleanup_saw_closed_file);
        let closed_flag = Arc::clone(&file_closed);

        drop_staged_file_before_cleanup(Some(DropMarker(Arc::clone(&file_closed))), move || {
            cleanup_flag.store(closed_flag.load(Ordering::SeqCst), Ordering::SeqCst)
        });

        assert!(cleanup_saw_closed_file.load(Ordering::SeqCst));
    }
}
