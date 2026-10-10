//! Save slot discovery, format identification, and sorting.

use crate::discovery::locator::{normalize_full_path, resolve_entry_path, resolve_links, SaveDirectoryCandidate};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::SystemTime;

const MAXIMUM_FILE_SIZE: u64 = 536_870_912; // 512 MiB, the largest file discovery reads
const XRAY_MAGIC: u32 = 0xFFFF_FFFF;
/// Serializes reads of whole Enhanced Edition files during discovery, so at most one full file is held in memory at a
/// time. The lock guards no data.
static FULL_XRAY_EE_DETECTION_LOCK: Mutex<()> = Mutex::new(());

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
    /// One description per scan worker that failed; the save files it covered are not in `slots`.
    pub worker_failures: Vec<String>,
}

/// Adds one worker's batch to `out`, or records that the worker failed.
///
/// A failed worker's saves are missing from the result, so the failure is kept for the caller to report.
pub(crate) fn absorb_worker_result<T>(
    out: &mut Vec<T>,
    failures: &mut Vec<String>,
    expected: usize,
    result: std::thread::Result<Vec<T>>,
) {
    match result {
        Ok(mut batch) => out.append(&mut batch),
        Err(_) => failures.push(format!(
            "a save scan worker failed; {expected} save file(s) were not identified"
        )),
    }
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

fn with_full_xray_ee_detection<T>(operation: impl FnOnce() -> T) -> T {
    let _guard = FULL_XRAY_EE_DETECTION_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    operation()
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
            if !searched_identities.insert(identity.clone()) {
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

                if !seen_slots.insert(file_path.clone()) {
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
                worker_failures: Vec::new(),
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
        let mut worker_failures = Vec::new();

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
                    let expected = chunk.len();
                    handles.push((
                        expected,
                        s.spawn(move || {
                            let mut local_slots = Vec::with_capacity(chunk.len());
                            for target in chunk {
                                local_slots.push(scan_single_slot(
                                    &target.path,
                                    &target.candidate_game_id,
                                    &target.candidate_release_id,
                                ));
                            }
                            local_slots
                        }),
                    ));
                }
                for (expected, handle) in handles {
                    absorb_worker_result(&mut slots, &mut worker_failures, expected, handle.join());
                }
            });
        }

        sort_newest_first(&mut slots);

        SaveDiscoveryResult {
            slots,
            searched_paths,
            worker_failures,
        }
    }
}

/// Orders slots newest first; equal times are ordered by path.
pub(crate) fn sort_newest_first(slots: &mut [SaveSlot]) {
    slots.sort_by(|left, right| {
        let mtime_order = right.last_write_time_utc.cmp(&left.last_write_time_utc);
        if mtime_order != std::cmp::Ordering::Equal {
            mtime_order
        } else {
            left.path.cmp(&right.path)
        }
    });
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

    if size > MAXIMUM_FILE_SIZE {
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

    let detection = read_file_header(path, HEADER_SAMPLE_BYTES)
        .and_then(|header_bytes| detect_format_for_file(path, &header_bytes, size));

    match detection {
        Ok((format_id, game_id, detection_error)) => SaveSlot {
            path: path.to_path_buf(),
            candidate_game_id: candidate_game_id.to_string(),
            candidate_release_id: candidate_release_id.to_string(),
            size,
            last_write_time_utc: mtime,
            format_id,
            game_id,
            detection_error,
        },
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

fn requires_full_xray_ee_bytes(bytes: &[u8]) -> bool {
    if bytes.len() < 12 || read_u32_le(bytes, 0) != Some(XRAY_MAGIC) || read_u32_le(bytes, 4) != Some(6) {
        return false;
    }
    bytes
        .get(12..)
        .and_then(extract_alife_version)
        .is_some_and(|version| version == 54)
}

pub(crate) fn detect_format_for_file(
    path: &std::path::Path,
    header_bytes: &[u8],
    size: u64,
) -> io::Result<(Option<String>, Option<String>, Option<String>)> {
    if size > MAXIMUM_FILE_SIZE {
        return Ok((
            None,
            None,
            Some("The file is larger than any save this version can read.".to_owned()),
        ));
    }
    if requires_full_xray_ee_bytes(header_bytes) {
        let full_size = usize::try_from(size)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "save size does not fit this platform"))?;
        if full_size > header_bytes.len() {
            return with_full_xray_ee_detection(|| {
                read_file_header(path, full_size).map(|full_bytes| detect_format_with_context(&full_bytes, Some(path)))
            });
        }
    }
    Ok(detect_format_with_context(header_bytes, Some(path)))
}

pub(crate) fn has_save_extension(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower.ends_with(".sav") || lower.ends_with(".scop") || lower.ends_with(".scs")
}

pub(crate) fn is_non_slot_file(file_name: &str) -> bool {
    let lower = file_name.to_ascii_lowercase();
    lower == "campaignssave.sav" || lower == "analyticsdata.sav"
}

fn format_io_error(err: &io::Error) -> String {
    format!("IOException: {err}")
}

/// Detects the format and game family of save file bytes.
#[must_use]
pub fn detect_format(bytes: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    detect_format_with_context(bytes, None)
}

pub(crate) fn detect_format_with_context(
    bytes: &[u8],
    path: Option<&std::path::Path>,
) -> (Option<String>, Option<String>, Option<String>) {
    if let Some(format_id) = detect_xray(bytes) {
        let game_id = family_for_format(&format_id);
        return (Some(format_id), game_id, None);
    }

    if detect_stalker2(bytes, path) {
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

    if unpacked_size_u32 == 0
        || usize::try_from(unpacked_size_u32).is_ok_and(|size| size > sse_core::limits::MAXIMUM_UNPACKED_BYTES)
    {
        return None;
    }

    let payload = bytes.get(12..)?;
    let mut unpacked = None;
    let alife_version = if let Some(version) = extract_alife_version(payload) {
        version
    } else {
        let unpacked_size = usize::try_from(unpacked_size_u32).ok()?;
        let raw = sse_codecs::lzo1x::decompress(payload, unpacked_size).ok()?;
        let (version, _) = parse_xray_chunks(&raw)?;
        unpacked = Some(raw);
        version
    };

    // The version rule and the Enhanced Edition markers live in sse-xray; only the object chunk is read here.
    if container_version == 6 && alife_version == 54 {
        let raw = match unpacked {
            Some(raw) => raw,
            None => {
                let unpacked_size = usize::try_from(unpacked_size_u32).ok()?;
                sse_codecs::lzo1x::decompress(payload, unpacked_size).ok()?
            }
        };
        let (confirmed_version, object_data) = parse_xray_chunks(&raw)?;
        if confirmed_version != alife_version {
            return None;
        }
        return sse_xray::format_id_for_versions(container_version, alife_version, object_data).map(str::to_owned);
    }
    sse_xray::format_id_for_versions(container_version, alife_version, &[]).map(str::to_owned)
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

/// Largest S2 save that discovery identifies by its container trailer; a larger file is not identified here.
const S2_CONTAINER_CHECK_LIMIT: u64 = 64 * 1024 * 1024;

fn detect_stalker2(bytes: &[u8], path: Option<&std::path::Path>) -> bool {
    match path {
        Some(path) => detect_stalker2_file(path),
        None => s2_container_is_valid(bytes),
    }
}

/// Reads the whole file, because the trailer check covers all of it; a 4 KiB sample cannot prove a container.
fn detect_stalker2_file(path: &std::path::Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let size = metadata.len();
    if !(8..=S2_CONTAINER_CHECK_LIMIT).contains(&size) {
        return false;
    }
    let Ok(size) = usize::try_from(size) else {
        return false;
    };
    read_file_header(path, size).is_ok_and(|bytes| s2_container_is_valid(&bytes))
}

/// The checks `sse-s2` makes before it decodes a container: the unpacked size is in range, and the CRC-32 trailer
/// matches the rest of the bytes. `bytes` must be the whole container.
fn s2_container_is_valid(bytes: &[u8]) -> bool {
    if bytes.len() < 8 {
        return false;
    }
    let Some(trailer_offset) = bytes.len().checked_sub(4) else {
        return false;
    };
    let (Some(unpacked_size), Some(stored_crc)) = (read_u32_le(bytes, 0), read_u32_le(bytes, trailer_offset)) else {
        return false;
    };
    if unpacked_size == 0
        || usize::try_from(unpacked_size).is_ok_and(|size| size > sse_core::limits::MAXIMUM_UNPACKED_BYTES)
    {
        return false;
    }
    bytes
        .get(..trailer_offset)
        .is_some_and(|body| sse_codecs::crc32::crc32(body) == stored_crc)
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

#[cfg(test)]
mod tests {
    use super::with_full_xray_ee_detection;
    use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc, Barrier};
    use std::time::Duration;

    #[test]
    fn full_xray_ee_detection_is_serialized() {
        const WORKERS: usize = 8;
        let start = Arc::new(Barrier::new(WORKERS));
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        std::thread::scope(|scope| {
            for _ in 0..WORKERS {
                let start = Arc::clone(&start);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                scope.spawn(move || {
                    start.wait();
                    with_full_xray_ee_detection(|| {
                        let current = active.fetch_add(1, Ordering::SeqCst).saturating_add(1);
                        peak.fetch_max(current, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(2));
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                });
            }
        });

        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}

#[cfg(test)]
mod worker_failure_tests {
    use super::absorb_worker_result;

    #[test]
    fn a_failed_worker_is_reported_and_its_batch_is_not_invented() {
        let mut out: Vec<u8> = vec![1];
        let mut failures = Vec::new();
        absorb_worker_result(&mut out, &mut failures, 3, Ok(vec![2, 3]));
        assert_eq!(out, vec![1, 2, 3]);
        assert!(failures.is_empty());

        let panicked: std::thread::Result<Vec<u8>> = Err(Box::new("worker panicked"));
        absorb_worker_result(&mut out, &mut failures, 4, panicked);
        assert_eq!(out, vec![1, 2, 3]);
        assert_eq!(failures.len(), 1);
        let message = failures.first().map_or("", String::as_str);
        assert!(message.contains("4 save file(s)"), "{message}");
    }
}
