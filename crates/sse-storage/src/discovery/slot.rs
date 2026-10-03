//! Save slot discovery, format identification, and sorting.

use crate::discovery::locator::{normalize_full_path, resolve_links, SaveDirectoryCandidate};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

const MAXIMUM_UNPACKED_SIZE: u64 = 536_870_912; // 512 MiB
const XRAY_MAGIC: u32 = 0xFFFF_FFFF;

/// Represents a single discovered save slot file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSlot {
    /// Full path to the save file.
    pub path: PathBuf,
    /// Candidate game identifier ("soc", "clear_sky", "cop", "stalker2").
    pub candidate_game_id: String,
    /// Candidate release identifier ("stalker-soc", "stalker2", etc.).
    pub candidate_release_id: String,
    /// File size in bytes.
    pub size: u64,
    /// Last write time of the save file.
    pub last_write_time_utc: SystemTime,
    /// Detected format identifier ("stalker-soc", "stalker-cs", "stalker2", etc.), or `None`.
    pub format_id: Option<String>,
    /// Detected game family ("soc", "clear_sky", "cop", "stalker2"), or `None`.
    pub game_id: Option<String>,
    /// Detection error message if the file is invalid or unreadable.
    pub detection_error: Option<String>,
}

/// Result of scanning directory candidates for save slots.
#[derive(Debug, Clone)]
pub struct SaveDiscoveryResult {
    /// Discovered save slots, sorted newest first.
    pub slots: Vec<SaveSlot>,
    /// Unique directory paths searched during discovery.
    pub searched_paths: Vec<PathBuf>,
}

/// Service that discovers and identifies save slots in candidate directories.
pub struct SaveSlotDiscovery;

impl SaveSlotDiscovery {
    /// Scans candidate directories and returns discovered save slots.
    #[must_use]
    pub fn discover(candidates: &[SaveDirectoryCandidate]) -> SaveDiscoveryResult {
        let mut searched_paths = Vec::new();
        let mut searched_identities = HashSet::new();
        let mut slots = Vec::new();
        let mut seen_slots = HashSet::new();

        for candidate in candidates {
            if candidate.directory_path.as_os_str().is_empty() {
                continue;
            }

            let directory = normalize_full_path(&candidate.directory_path);
            let identity = resolve_links(&directory);
            let identity_key = identity.to_string_lossy().to_string();

            if !searched_identities.insert(identity_key) {
                continue;
            }

            searched_paths.push(directory.clone());

            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };

            for entry in entries.flatten() {
                let file_path = entry.path();
                let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if is_non_slot_file(file_name) || !has_save_extension(file_name) {
                    continue;
                }

                let full_path = normalize_full_path(&file_path);
                let path_key = full_path.to_string_lossy().to_string();
                if !seen_slots.insert(path_key) {
                    continue;
                }

                let Ok(metadata) = fs::metadata(&full_path) else {
                    continue;
                };

                if !metadata.is_file() {
                    continue;
                }

                let size = metadata.len();
                let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

                if size > MAXIMUM_UNPACKED_SIZE {
                    slots.push(SaveSlot {
                        path: full_path,
                        candidate_game_id: candidate.game_id.clone(),
                        candidate_release_id: candidate.release_id.clone(),
                        size,
                        last_write_time_utc: mtime,
                        format_id: None,
                        game_id: None,
                        detection_error: Some("The file is larger than any save this version can read.".to_string()),
                    });
                    continue;
                }

                match fs::read(&full_path) {
                    Ok(bytes) => {
                        let (format_id, game_id, detection_error) = detect_format(&bytes);
                        slots.push(SaveSlot {
                            path: full_path,
                            candidate_game_id: candidate.game_id.clone(),
                            candidate_release_id: candidate.release_id.clone(),
                            size,
                            last_write_time_utc: mtime,
                            format_id,
                            game_id,
                            detection_error,
                        });
                    }
                    Err(err) => {
                        slots.push(SaveSlot {
                            path: full_path,
                            candidate_game_id: candidate.game_id.clone(),
                            candidate_release_id: candidate.release_id.clone(),
                            size,
                            last_write_time_utc: mtime,
                            format_id: None,
                            game_id: None,
                            detection_error: Some(format_io_error(&err)),
                        });
                    }
                }
            }
        }

        slots.sort_by(|left, right| {
            let mtime_order = right.last_write_time_utc.cmp(&left.last_write_time_utc);
            if mtime_order != std::cmp::Ordering::Equal {
                mtime_order
            } else {
                left.path.cmp(&right.path)
            }
        });

        SaveDiscoveryResult { slots, searched_paths }
    }
}

fn has_save_extension(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".sav") || lower.ends_with(".scop") || lower.ends_with(".scs")
}

fn is_non_slot_file(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower == "campaignssave.sav" || lower == "analyticsdata.sav"
}

fn format_io_error(err: &io::Error) -> String {
    format!("IOException: {err}")
}

/// Detects the format and game family of save file bytes.
#[must_use]
pub fn detect_format(bytes: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(format_id) = detect_xray(bytes) {
        let game_id = family_for_format(&format_id);
        return (Some(format_id), game_id, None);
    }

    if detect_stalker2(bytes) {
        return (Some("stalker2".to_string()), Some("stalker2".to_string()), None);
    }

    (
        None,
        None,
        Some("The file is not a save format this version can read.".to_string()),
    )
}

fn detect_xray(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 12 {
        return None;
    }

    let magic = read_u32_le(bytes, 0)?;
    let container_version = read_u32_le(bytes, 4)?;
    let unpacked_size_u32 = read_u32_le(bytes, 8)?;

    if magic != XRAY_MAGIC {
        return None;
    }

    if container_version != 3 && container_version != 5 && container_version != 6 {
        return None;
    }

    if unpacked_size_u32 == 0 || u64::from(unpacked_size_u32) > MAXIMUM_UNPACKED_SIZE {
        return None;
    }

    let unpacked_size = usize::try_from(unpacked_size_u32).ok()?;
    let payload = bytes.get(12..)?;
    let raw = sse_codecs::lzo1x::decompress(payload, unpacked_size).ok()?;

    let (alife_version, object_data) = parse_xray_chunks(&raw)?;

    // Check Original formats
    if container_version == 3 && alife_version == 3 {
        return Some("stalker-soc".to_string());
    }
    if container_version == 5 && alife_version == 5 {
        return Some("stalker-cs".to_string());
    }
    if container_version == 6 && alife_version == 6 {
        return Some("stalker-cop".to_string());
    }

    // Check Enhanced formats
    if container_version == 3 && alife_version == 51 {
        return Some("stalker-soc-ee".to_string());
    }
    if container_version == 6 && alife_version == 54 {
        let has_marsh = contains_subslice(object_data, b"marsh");
        let has_zaton = contains_subslice(object_data, b"zaton");
        if has_marsh && !has_zaton {
            return Some("stalker-cs-ee".to_string());
        }
        if has_zaton && !has_marsh {
            return Some("stalker-cop-ee".to_string());
        }
    }

    None
}

fn parse_xray_chunks(raw: &[u8]) -> Option<(u32, &[u8])> {
    let mut offset = 0_usize;
    let mut alife_version: Option<u32> = None;
    let mut object_data: Option<&[u8]> = None;

    while offset.checked_add(8)? <= raw.len() {
        let chunk_type = read_u32_le(raw, offset)?;
        let chunk_size_u32 = read_u32_le(raw, offset.checked_add(4)?)?;
        let chunk_size = usize::try_from(chunk_size_u32).ok()?;
        let data_start = offset.checked_add(8)?;
        let data_end = data_start.checked_add(chunk_size)?;
        let chunk_data = raw.get(data_start..data_end)?;

        if chunk_type == 0 {
            // ALIFE header chunk must be 4 bytes
            if chunk_data.len() == 4 {
                alife_version = read_u32_le(chunk_data, 0);
            }
        } else if chunk_type == 2 {
            object_data = Some(chunk_data);
        }

        offset = data_end;
    }

    match (alife_version, object_data) {
        (Some(av), Some(od)) => Some((av, od)),
        _ => None,
    }
}

fn detect_stalker2(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }

    let unpacked_size = match read_u32_le(bytes, 0) {
        Some(v) => v,
        None => return false,
    };

    if unpacked_size == 0 || u64::from(unpacked_size) > MAXIMUM_UNPACKED_SIZE {
        return false;
    }

    let checksum_offset = match bytes.len().checked_sub(4) {
        Some(pos) => pos,
        None => return false,
    };

    let stored_crc = match read_u32_le(bytes, checksum_offset) {
        Some(v) => v,
        None => return false,
    };

    let body = match bytes.get(..checksum_offset) {
        Some(slice) => slice,
        None => return false,
    };

    let computed_crc = sse_codecs::crc32::crc32(body);
    stored_crc == computed_crc
}

fn family_for_format(format_id: &str) -> Option<String> {
    match format_id {
        "stalker-soc" | "stalker-soc-ee" => Some("soc".to_string()),
        "stalker-cs" | "stalker-cs-ee" => Some("clear_sky".to_string()),
        "stalker-cop" | "stalker-cop-ee" => Some("cop".to_string()),
        "stalker2" => Some("stalker2".to_string()),
        _ => None,
    }
}

fn read_u32_le(data: &[u8], offset: usize) -> Option<u32> {
    let slice = data.get(offset..offset.checked_add(4)?)?;
    let arr: [u8; 4] = slice.try_into().ok()?;
    Some(u32::from_le_bytes(arr))
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|window| window == needle)
}
