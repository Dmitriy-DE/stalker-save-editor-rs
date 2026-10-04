//! Save slot discovery, format identification, and sorting.

use crate::discovery::locator::{normalize_full_path, resolve_entry_path, resolve_links, SaveDirectoryCandidate};
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

pub(crate) const HEADER_SAMPLE_BYTES: usize = 4096;

pub(crate) fn read_file_header(path: &std::path::Path, max_bytes: usize) -> io::Result<Vec<u8>> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0_u8; max_bytes];
    let mut bytes_read = 0_usize;
    while bytes_read < max_bytes {
        let chunk = match file.read(buffer.get_mut(bytes_read..).unwrap_or(&mut [])) {
            Ok(0) => break,
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        bytes_read = bytes_read.saturating_add(chunk);
    }
    buffer.truncate(bytes_read);
    Ok(buffer)
}

struct SlotTarget {
    path: PathBuf,
    candidate_game_id: String,
    candidate_release_id: String,
}

/// Service that discovers and identifies save slots in candidate directories.
pub struct SaveSlotDiscovery;

impl SaveSlotDiscovery {
    /// Scans candidate directories and returns discovered save slots.
    #[must_use]
    pub fn discover(candidates: &[SaveDirectoryCandidate]) -> SaveDiscoveryResult {
        let mut searched_paths = Vec::new();
        let mut searched_identities = HashSet::new();
        let mut seen_slots = HashSet::new();
        let mut found_files = Vec::new();

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

            searched_paths.push(identity.clone());

            let Ok(entries) = fs::read_dir(&identity) else {
                continue;
            };

            for entry in entries.flatten() {
                let file_path = resolve_entry_path(&identity, &entry);
                let Some(file_name) = file_path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if is_non_slot_file(file_name) || !has_save_extension(file_name) {
                    continue;
                }

                let path_key = file_path.to_string_lossy().to_string();
                if !seen_slots.insert(path_key) {
                    continue;
                }

                found_files.push(SlotTarget {
                    path: file_path,
                    candidate_game_id: candidate.game_id.clone(),
                    candidate_release_id: candidate.release_id.clone(),
                });
            }
        }

        if found_files.is_empty() {
            return SaveDiscoveryResult {
                slots: Vec::new(),
                searched_paths,
            };
        }

        let max_workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(1, 8);
        let num_workers = max_workers.min(found_files.len());
        let chunk_size = found_files
            .len()
            .checked_add(num_workers.saturating_sub(1))
            .and_then(|sum| sum.checked_div(num_workers))
            .unwrap_or(1)
            .max(1);

        let mut slots = Vec::with_capacity(found_files.len());

        if num_workers <= 1 {
            for target in &found_files {
                slots.push(scan_single_slot(
                    &target.path,
                    &target.candidate_game_id,
                    &target.candidate_release_id,
                ));
            }
        } else {
            let chunks: Vec<&[SlotTarget]> = found_files.chunks(chunk_size).collect();
            std::thread::scope(|s| {
                let mut handles = Vec::with_capacity(chunks.len());
                for chunk in chunks {
                    handles.push(s.spawn(move || {
                        let mut local_slots = Vec::with_capacity(chunk.len());
                        for target in chunk {
                            local_slots.push(scan_single_slot(
                                &target.path,
                                &target.candidate_game_id,
                                &target.candidate_release_id,
                            ));
                        }
                        local_slots
                    }));
                }
                for handle in handles {
                    if let Ok(mut batch) = handle.join() {
                        slots.append(&mut batch);
                    }
                }
            });
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

fn scan_single_slot(path: &std::path::Path, candidate_game_id: &str, candidate_release_id: &str) -> SaveSlot {
    let Ok(metadata) = fs::metadata(path) else {
        return SaveSlot {
            path: path.to_path_buf(),
            candidate_game_id: candidate_game_id.to_string(),
            candidate_release_id: candidate_release_id.to_string(),
            size: 0,
            last_write_time_utc: SystemTime::UNIX_EPOCH,
            format_id: None,
            game_id: None,
            detection_error: Some("Unable to read file metadata".to_string()),
        };
    };

    let size = metadata.len();
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

    if size > MAXIMUM_UNPACKED_SIZE {
        return SaveSlot {
            path: path.to_path_buf(),
            candidate_game_id: candidate_game_id.to_string(),
            candidate_release_id: candidate_release_id.to_string(),
            size,
            last_write_time_utc: mtime,
            format_id: None,
            game_id: None,
            detection_error: Some("The file is larger than any save this version can read.".to_string()),
        };
    }

    match read_file_header(path, HEADER_SAMPLE_BYTES) {
        Ok(header_bytes) => {
            let (format_id, game_id, detection_error) =
                detect_format_with_context(&header_bytes, Some(path), Some(candidate_release_id));
            SaveSlot {
                path: path.to_path_buf(),
                candidate_game_id: candidate_game_id.to_string(),
                candidate_release_id: candidate_release_id.to_string(),
                size,
                last_write_time_utc: mtime,
                format_id,
                game_id,
                detection_error,
            }
        }
        Err(err) => SaveSlot {
            path: path.to_path_buf(),
            candidate_game_id: candidate_game_id.to_string(),
            candidate_release_id: candidate_release_id.to_string(),
            size,
            last_write_time_utc: mtime,
            format_id: None,
            game_id: None,
            detection_error: Some(format_io_error(&err)),
        },
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
    detect_format_with_context(bytes, None, None)
}

pub(crate) fn detect_format_with_context(
    bytes: &[u8],
    path: Option<&std::path::Path>,
    candidate_release_id: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(format_id) = detect_xray(bytes, path, candidate_release_id) {
        let game_id = family_for_format(&format_id);
        return (Some(format_id), game_id, None);
    }

    if detect_stalker2(bytes, path, candidate_release_id) {
        return (Some("stalker2".to_string()), Some("stalker2".to_string()), None);
    }

    (
        None,
        None,
        Some("The file is not a save format this version can read.".to_string()),
    )
}

fn detect_xray(bytes: &[u8], path: Option<&std::path::Path>, candidate_release_id: Option<&str>) -> Option<String> {
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

    let payload = bytes.get(12..)?;
    let fast_alife = extract_alife_version(payload);

    let alife_version = if let Some(av) = fast_alife {
        av
    } else {
        let unpacked_size = usize::try_from(unpacked_size_u32).ok()?;
        let raw = sse_codecs::lzo1x::decompress(payload, unpacked_size).ok()?;
        let (av, _) = parse_xray_chunks(&raw)?;
        av
    };

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
        let file_name = path.and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("");
        let lower = file_name.to_ascii_lowercase();

        if lower.ends_with(".scop") || candidate_release_id == Some("stalker-cop-ee") {
            return Some("stalker-cop-ee".to_string());
        }
        if lower.ends_with(".scs") || candidate_release_id == Some("stalker-cs-ee") {
            return Some("stalker-cs-ee".to_string());
        }

        // If whole file is available and decompresses, check object chunk
        if let Ok(unpacked_size) = usize::try_from(unpacked_size_u32) {
            if let Ok(raw) = sse_codecs::lzo1x::decompress(payload, unpacked_size) {
                if let Some((_, object_data)) = parse_xray_chunks(&raw) {
                    let has_marsh = contains_subslice(object_data, b"marsh");
                    let has_zaton = contains_subslice(object_data, b"zaton");
                    if has_marsh && !has_zaton {
                        return Some("stalker-cs-ee".to_string());
                    }
                    if has_zaton && !has_marsh {
                        return Some("stalker-cop-ee".to_string());
                    }
                }
            }
        }

        // Default to stalker-cop-ee for Enhanced Edition version 6/54
        return Some("stalker-cop-ee".to_string());
    }

    None
}

fn extract_alife_version(payload: &[u8]) -> Option<u32> {
    let mut pos = 0_usize;
    if payload.len() >= 5 && payload.first().copied() == Some(17) {
        pos = pos.saturating_add(2);
    }
    let cmd = *payload.get(pos)?;
    pos = pos.saturating_add(1);
    let lit_len = if cmd > 17 {
        usize::from(cmd.saturating_sub(17))
    } else if cmd < 16 {
        if cmd == 0 {
            while payload.get(pos).copied() == Some(0) {
                pos = pos.saturating_add(1);
            }
            let next_byte = *payload.get(pos)?;
            pos = pos.saturating_add(1);
            15_usize.saturating_add(usize::from(next_byte))
        } else {
            usize::from(cmd)
        }
    } else {
        return None;
    };

    let literals = payload.get(pos..pos.checked_add(lit_len)?)?;
    if literals.len() < 12 {
        return None;
    }
    let chunk_type = read_u32_le(literals, 0)?;
    let chunk_size = read_u32_le(literals, 4)?;
    if chunk_type == 0 && chunk_size == 4 {
        read_u32_le(literals, 8)
    } else {
        None
    }
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

fn detect_stalker2(bytes: &[u8], path: Option<&std::path::Path>, candidate_release_id: Option<&str>) -> bool {
    if bytes.len() < 8 {
        return false;
    }

    let magic = match read_u32_le(bytes, 0) {
        Some(m) => m,
        None => return false,
    };
    if magic == XRAY_MAGIC {
        return false;
    }

    let unpacked_size = magic;
    if unpacked_size == 0 || u64::from(unpacked_size) > MAXIMUM_UNPACKED_SIZE {
        return false;
    }

    if bytes.len() >= 8 {
        if let Some(pos) = bytes.len().checked_sub(4) {
            if let Some(stored_crc) = read_u32_le(bytes, pos) {
                if let Some(body) = bytes.get(..pos) {
                    if sse_codecs::crc32::crc32(body) == stored_crc {
                        return true;
                    }
                }
            }
        }
    }

    let file_name = path.and_then(|p| p.file_name()).and_then(|n| n.to_str()).unwrap_or("");
    let lower = file_name.to_ascii_lowercase();

    if candidate_release_id == Some("stalker2") || lower.ends_with(".sav") {
        if let Some(first_stream_byte) = bytes.get(4) {
            if *first_stream_byte != 0 {
                return true;
            }
        }
    }

    false
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
