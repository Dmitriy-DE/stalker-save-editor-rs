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
pub fn running_processes() -> io::Result<Vec<String>> {
    running_processes_impl()
}

#[cfg(target_os = "windows")]
fn known_folder_impl(id: KnownFolder) -> Option<PathBuf> {
    use crate::win32_ffi::{CoTaskMemFree, Guid, SHGetKnownFolderPath};
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

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
    let guid = match id {
        KnownFolder::Documents => &DOCUMENTS,
        KnownFolder::SavedGames => &SAVED_GAMES,
    };
    let mut raw = std::ptr::null_mut::<u16>();
    // SAFETY: guid points to a valid KNOWNFOLDERID, token is null for the current user,
    // and raw is a valid out-pointer. On success shell32 allocates raw with the COM task allocator.
    let status = unsafe { SHGetKnownFolderPath(guid, 0, std::ptr::null_mut(), &mut raw) };
    if status < 0 || raw.is_null() {
        return None;
    }
    let mut len = 0_usize;
    // SAFETY: SHGetKnownFolderPath returned a NUL-terminated UTF-16 string.
    unsafe {
        while *raw.add(len) != 0 {
            len = len.checked_add(1)?;
        }
    }
    // SAFETY: raw is live for len UTF-16 code units and remains allocated until CoTaskMemFree below.
    let value = unsafe { std::slice::from_raw_parts(raw, len) };
    let path = PathBuf::from(OsString::from_wide(value));
    // SAFETY: raw was allocated by SHGetKnownFolderPath with the COM task allocator and is freed exactly once.
    unsafe { CoTaskMemFree(raw.cast()) };
    Some(path)
}

#[cfg(not(target_os = "windows"))]
const fn known_folder_impl(_id: KnownFolder) -> Option<PathBuf> {
    None
}

#[cfg(target_os = "windows")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    use crate::win32_ffi::{
        CloseHandle, CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, ProcessEntry32W,
    };
    use std::ffi::c_void;

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    let invalid = (-1_isize) as *mut c_void;
    // SAFETY: flags request a process snapshot and process_id is ignored for TH32CS_SNAPPROCESS.
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
    // SAFETY: snapshot is valid and entry has the documented size and writable storage.
    let mut ok = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while ok {
        let len = entry
            .executable
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(entry.executable.len());
        if let Some(units) = entry.executable.get(..len) {
            if let Ok(name) = String::from_utf16(units) {
                if !name.is_empty() {
                    names.push(name);
                }
            }
        }
        // SAFETY: snapshot remains live and entry is reusable writable PROCESSENTRY32W storage.
        ok = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: snapshot is an owned kernel handle returned by CreateToolhelp32Snapshot and is closed exactly once.
    unsafe {
        let _ = CloseHandle(snapshot);
    }
    Ok(names)
}

#[cfg(target_os = "linux")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let file_name = entry.file_name();
        if !file_name.to_string_lossy().bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let comm = entry.path().join("comm");
        if let Ok(name) = std::fs::read_to_string(comm) {
            let name = name.trim();
            if !name.is_empty() {
                names.push(name.to_owned());
            }
        }
    }
    Ok(names)
}

#[cfg(target_os = "macos")]
fn running_processes_impl() -> io::Result<Vec<String>> {
    let output = std::process::Command::new("ps").args(["-axo", "comm="]).output()?;
    if !output.status.success() {
        return Err(io::Error::other("ps process enumeration failed"));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(text
        .lines()
        .filter_map(|line| {
            let path = std::path::Path::new(line.trim());
            path.file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
        })
        .collect())
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn running_processes_impl() -> io::Result<Vec<String>> {
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::{known_folder, running_processes, KnownFolder};

    #[test]
    fn process_enumeration_is_available() {
        let result = running_processes();
        assert!(result.is_ok(), "process enumeration failed: {result:?}");
    }

    #[test]
    fn known_folder_api_accepts_supported_ids() {
        let _ = known_folder(KnownFolder::Documents);
        let _ = known_folder(KnownFolder::SavedGames);
    }
}
