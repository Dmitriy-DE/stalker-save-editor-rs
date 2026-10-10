//! Safe operating-system discovery helpers used by save locators and write guards.

use std::io;
use std::path::PathBuf;

/// Per-user folders needed by game discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownFolder {
    /// The user's redirected Documents folder.
    Documents,
    /// The Windows Saved Games known folder.
    SavedGames,
}

/// Resolves a platform known folder.
///
/// Windows uses `SHGetKnownFolderPath`, so redirected folders are respected.
/// Other platforms return `None`; their game locators use platform-native roots.
#[must_use]
pub fn known_folder(id: KnownFolder) -> Option<PathBuf> {
    known_folder_impl(id)
}

/// Returns executable names for currently running processes.
///
/// # Errors
/// Returns an operating-system error if the process list cannot be queried.
pub fn running_processes() -> io::Result<Vec<String>> {
    let mut names = running_processes_impl()?;
    names.retain(|name| !name.is_empty());
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

#[cfg(target_os = "windows")]
fn known_folder_impl(id: KnownFolder) -> Option<PathBuf> {
    use crate::win32_ffi::{CoInitializeEx, CoTaskMemFree, CoUninitialize, Guid, SHGetKnownFolderPath};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    const COINIT_APARTMENTTHREADED: u32 = 0x0000_0002;
    const RPC_E_CHANGED_MODE: i32 = -2_147_417_850;
    const DOCUMENTS: Guid = Guid {
        a: 0xFDD3_9AD0,
        b: 0x238F,
        c: 0x46AF,
        d: [0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7],
    };
    const SAVED_GAMES: Guid = Guid {
        a: 0x4C5C_32FF,
        b: 0xBB9D,
        c: 0x43B0,
        d: [0xB5, 0xB4, 0x2D, 0x72, 0xE5, 0x4E, 0xAA, 0xA4],
    };

    // SAFETY: null reserved pointer and a documented apartment flag satisfy
    // CoInitializeEx; this call affects only the current thread.
    let com_status = unsafe { CoInitializeEx(std::ptr::null_mut(), COINIT_APARTMENTTHREADED) };
    if com_status < 0 && com_status != RPC_E_CHANGED_MODE {
        return None;
    }
    let must_uninitialize = com_status >= 0;
    let guid = match id {
        KnownFolder::Documents => &DOCUMENTS,
        KnownFolder::SavedGames => &SAVED_GAMES,
    };
    let mut raw = std::ptr::null_mut::<u16>();
    // SAFETY: guid points to a valid KNOWNFOLDERID, token is null for the current
    // user, and raw is a valid out-pointer.
    let status = unsafe { SHGetKnownFolderPath(guid, 0, std::ptr::null_mut(), &mut raw) };
    let result = if status >= 0 && !raw.is_null() {
        let mut len = 0_usize;
        // SAFETY: successful SHGetKnownFolderPath returns a NUL-terminated UTF-16
        // allocation that remains live until CoTaskMemFree below.
        unsafe {
            while *raw.add(len) != 0 {
                len = len.saturating_add(1);
            }
        }
        // SAFETY: len was measured inside the returned UTF-16 allocation.
        let value = unsafe { std::slice::from_raw_parts(raw, len) };
        Some(PathBuf::from(OsString::from_wide(value)))
    } else {
        None
    };
    if !raw.is_null() {
        // SAFETY: raw was allocated by SHGetKnownFolderPath with the COM task
        // allocator and has not been freed.
        unsafe { CoTaskMemFree(raw.cast()) };
    }
    if must_uninitialize {
        // SAFETY: this thread successfully initialized COM above, so this balances
        // that initialization exactly once.
        unsafe { CoUninitialize() };
    }
    result
}

#[cfg(not(target_os = "windows"))]
const fn known_folder_impl(_id: KnownFolder) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "windows")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    use crate::win32_ffi::{CloseHandle, CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, ProcessEntry32W};
    use std::ffi::c_void;

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    let invalid = (-1_isize) as *mut c_void;
    // SAFETY: the flag requests a read-only process snapshot; process_id is
    // ignored for TH32CS_SNAPPROCESS.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == invalid {
        return Err(io::Error::last_os_error());
    }
    let mut entry = ProcessEntry32W {
        size: u32::try_from(std::mem::size_of::<ProcessEntry32W>())
            .map_err(|_| io::Error::other("PROCESSENTRY32W size overflow"))?,
        usage: 0,
        process_id: 0,
        default_heap_id: 0,
        module_id: 0,
        threads: 0,
        parent_process_id: 0,
        priority_class_base: 0,
        flags: 0,
        executable: [0; 260],
    };
    let mut names = Vec::new();
    // ERROR_NO_MORE_FILES marks the normal end of the list. Any other failure must not look like an
    // empty list, because an empty list means "game not running" to the process guard.
    const ERROR_NO_MORE_FILES: i32 = 18;
    let mut enumeration_error: Option<io::Error> = None;
    // SAFETY: snapshot is valid and entry has the documented size and writable
    // storage required by Process32FirstW.
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    if !ok {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_NO_MORE_FILES) {
            enumeration_error = Some(error);
        }
    }
    while ok {
        let len = entry
            .executable
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.executable.len());
        if let Some(units) = entry.executable.get(..len) {
            let name = String::from_utf16_lossy(units);
            if !name.is_empty() {
                names.push(name);
            }
        }
        // SAFETY: snapshot remains open and entry remains valid writable
        // PROCESSENTRY32W storage.
        ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
        if !ok {
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_NO_MORE_FILES) {
                enumeration_error = Some(error);
            }
        }
    }
    // SAFETY: snapshot is the owned kernel handle returned above and is closed
    // exactly once after enumeration.
    let closed = unsafe { CloseHandle(snapshot) };
    if closed == 0 {
        return Err(io::Error::last_os_error());
    }
    if let Some(error) = enumeration_error {
        return Err(error);
    }
    Ok(names)
}

#[cfg(all(test, target_os = "windows"))]
mod windows_enumeration_tests {
    #[test]
    fn running_processes_lists_the_current_process() {
        let names = super::running_processes().unwrap();
        let current = std::env::current_exe().unwrap();
        let file_name = current.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            names.iter().any(|name| name.eq_ignore_ascii_case(&file_name)),
            "{names:?}"
        );
    }
}

#[cfg(target_os = "linux")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir("/proc")?.flatten() {
        let file_name = entry.file_name();
        let pid = file_name.to_string_lossy();
        if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Ok(name) = std::fs::read_to_string(entry.path().join("comm")) else {
            continue;
        };
        let name = name.trim();
        if !name.is_empty() {
            names.push(name.to_owned());
        }
    }
    Ok(names)
}

#[cfg(target_os = "macos")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    let output = std::process::Command::new("/bin/ps").args(["-axo", "comm="]).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!("ps exited with {}", output.status)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            std::path::Path::new(line.trim())
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .filter(|name| !name.is_empty())
        })
        .collect())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn running_processes_impl() -> io::Result<Vec<String>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "process enumeration is unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::{known_folder, running_processes, KnownFolder};

    #[test]
    fn process_enumeration_is_available() {
        let result = running_processes();
        assert!(result.is_ok(), "process enumeration failed: {result:?}");
        if let Ok(names) = result {
            assert!(!names.is_empty());
            assert!(names.iter().all(|name| !name.is_empty()));
        }
    }

    #[test]
    fn known_folder_api_accepts_supported_ids() {
        let _ = known_folder(KnownFolder::Documents);
        let _ = known_folder(KnownFolder::SavedGames);
    }
}
