//! Binary library index format for fast, header-revalidated save list loading.
//!
//! Layout:
//! - Magic: `b"SSLI"` (4 bytes)
//! - Format Version: `u32` (1)
//! - Number of entries: `u32`
//! - Repeated entry records:
//!   - `path`: length-prefixed UTF-8 string (`u16` length + bytes)
//!   - `candidate_game_id`: length-prefixed string
//!   - `candidate_release_id`: length-prefixed string
//!   - `size`: `u64`
//!   - `mtime_secs`: `i64`
//!   - `mtime_nanos`: `u32`
//!   - `header_hash`: `u64`
//!   - `format_id`: optional length-prefixed string (`0xFFFF` if `None`)
//!   - `game_id`: optional length-prefixed string (`0xFFFF` if `None`)
//!   - `detection_error`: optional length-prefixed string (`0xFFFF` if `None`)

use crate::discovery::locator::{normalize_full_path, resolve_entry_path, resolve_links, SaveDirectoryCandidate};
use crate::discovery::slot::{has_save_extension, is_non_slot_file, sort_newest_first, SaveSlot};
use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INDEX_MAGIC: &[u8; 4] = b"SSLI";
const INDEX_VERSION: u32 = 1;
const HEADER_SAMPLE_BYTES: usize = 4096;
const MINIMUM_INDEX_ENTRY_BYTES: usize = 40;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
/// Process-wide counter for temporary index file names; uniqueness is what matters. It holds no index data.
static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Single entry cached inside the library index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryIndexEntry {
    /// Full path to the save file.
    pub path: PathBuf,
    /// Candidate game identifier.
    pub candidate_game_id: String,
    /// Candidate release identifier.
    pub candidate_release_id: String,
    /// File size in bytes.
    pub size: u64,
    /// File modification time in seconds since Unix epoch.
    pub mtime_secs: i64,
    /// Fractional modification time nanoseconds.
    pub mtime_nanos: u32,
    /// 64-bit FNV-1a hash of the first 4 KiB header.
    pub header_hash: u64,
    /// Detected format identifier.
    pub format_id: Option<String>,
    /// Detected game family.
    pub game_id: Option<String>,
    /// Detection error message.
    pub detection_error: Option<String>,
}

impl LibraryIndexEntry {
    /// Converts this cached index entry into a `SaveSlot`.
    #[must_use]
    pub fn to_save_slot(&self) -> SaveSlot {
        let duration = if let Ok(secs) = u64::try_from(self.mtime_secs) {
            Duration::new(secs, self.mtime_nanos)
        } else {
            Duration::ZERO
        };
        let mtime = UNIX_EPOCH.checked_add(duration).unwrap_or(UNIX_EPOCH);

        SaveSlot {
            path: self.path.clone(),
            candidate_game_id: self.candidate_game_id.clone(),
            candidate_release_id: self.candidate_release_id.clone(),
            size: self.size,
            last_write_time_utc: mtime,
            format_id: self.format_id.clone(),
            game_id: self.game_id.clone(),
            detection_error: self.detection_error.clone(),
        }
    }
}

/// Binary index of save slot metadata, validating a small header sample on warm starts.
#[derive(Debug, Clone, Default)]
pub struct LibraryIndex {
    entries: HashMap<PathBuf, LibraryIndexEntry>,
}

impl LibraryIndex {
    /// Creates an empty library index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Number of entries in the index.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Checks whether the index is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Attempts to load and decode a library index from disk.
    ///
    /// Returns `None` if the file is missing, damaged, or of an unsupported version,
    /// allowing the caller to safely rebuild it without failing.
    #[must_use]
    pub fn load(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        Self::decode(&bytes)
    }

    /// Decodes a library index from raw bytes.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 12 || bytes.get(..4)? != INDEX_MAGIC {
            return None;
        }

        let version = read_u32_le(bytes, 4)?;
        if version != INDEX_VERSION {
            return None;
        }

        let count_u32 = read_u32_le(bytes, 8)?;
        let count = usize::try_from(count_u32).ok()?;
        if count > bytes.len().saturating_sub(12) / MINIMUM_INDEX_ENTRY_BYTES {
            return None;
        }

        let mut offset = 12_usize;
        let mut entries = HashMap::with_capacity(count);

        for _ in 0..count {
            let path_str = read_string(bytes, &mut offset)?;
            let path = PathBuf::from(path_str);
            let candidate_game_id = read_string(bytes, &mut offset)?;
            let candidate_release_id = read_string(bytes, &mut offset)?;
            let size = read_u64_le(bytes, &mut offset)?;
            let mtime_secs = read_i64_le(bytes, &mut offset)?;
            let mtime_nanos = read_u32_le_offset(bytes, &mut offset)?;
            let header_hash = read_u64_le(bytes, &mut offset)?;
            let format_id = read_optional_string(bytes, &mut offset)?;
            let game_id = read_optional_string(bytes, &mut offset)?;
            let detection_error = read_optional_string(bytes, &mut offset)?;

            entries.insert(
                path.clone(),
                LibraryIndexEntry {
                    path,
                    candidate_game_id,
                    candidate_release_id,
                    size,
                    mtime_secs,
                    mtime_nanos,
                    header_hash,
                    format_id,
                    game_id,
                    detection_error,
                },
            );
        }

        if offset != bytes.len() {
            return None;
        }

        Some(Self { entries })
    }

    /// Serializes the library index to raw bytes.
    ///
    /// # Errors
    /// Returns [`io::ErrorKind::InvalidInput`] if the entry count or any string does not fit the wire format.
    pub fn encode(&self) -> io::Result<Vec<u8>> {
        let mut output = Vec::new();
        output.extend_from_slice(INDEX_MAGIC);
        output.extend_from_slice(&INDEX_VERSION.to_le_bytes());
        let count_u32 = u32::try_from(self.entries.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "too many library index entries"))?;
        output.extend_from_slice(&count_u32.to_le_bytes());

        for entry in self.entries.values() {
            let path = entry
                .path
                .to_str()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "library index path is not valid UTF-8"))?;
            write_string(&mut output, path)?;
            write_string(&mut output, &entry.candidate_game_id)?;
            write_string(&mut output, &entry.candidate_release_id)?;
            output.extend_from_slice(&entry.size.to_le_bytes());
            output.extend_from_slice(&entry.mtime_secs.to_le_bytes());
            output.extend_from_slice(&entry.mtime_nanos.to_le_bytes());
            output.extend_from_slice(&entry.header_hash.to_le_bytes());
            write_optional_string(&mut output, entry.format_id.as_deref())?;
            write_optional_string(&mut output, entry.game_id.as_deref())?;
            write_optional_string(&mut output, entry.detection_error.as_deref())?;
        }

        Ok(output)
    }

    /// Saves the library index atomically to a destination path.
    ///
    /// # Errors
    /// Returns [`io::Error`] if creating or replacing the file fails.
    pub fn save(&self, destination: &Path) -> io::Result<()> {
        let encoded = self.encode()?;
        let (temp_path, mut file) = create_unique_temp_file(destination)?;
        let write_result = file.write_all(&encoded).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temp_path, destination) {
            let _ = fs::remove_file(&temp_path);
            return Err(error);
        }
        Ok(())
    }

    /// Looks up a cached entry by matching path, file size, modification time, and header hash.
    #[must_use]
    pub fn lookup(&self, path: &Path, size: u64, mtime: SystemTime, header_hash: u64) -> Option<&LibraryIndexEntry> {
        let (mtime_secs, mtime_nanos) = split_system_time(mtime);
        let entry = self.entries.get(path)?;
        if entry.size == size
            && entry.mtime_secs == mtime_secs
            && entry.mtime_nanos == mtime_nanos
            && entry.header_hash == header_hash
        {
            Some(entry)
        } else {
            None
        }
    }

    /// Inserts or replaces an index entry.
    pub fn insert(&mut self, entry: LibraryIndexEntry) {
        self.entries.insert(entry.path.clone(), entry);
    }

    /// Computes the 64-bit FNV-1a hash of a file's initial header bytes.
    #[must_use]
    pub fn compute_header_hash(bytes: &[u8]) -> u64 {
        let sample_len = bytes.len().min(HEADER_SAMPLE_BYTES);
        let sample = bytes.get(..sample_len).unwrap_or(&[]);
        let mut hash = FNV_OFFSET_BASIS;
        for &byte in sample {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    /// Discovers save slots using warm index revalidation when possible.
    #[must_use]
    pub fn scan_with_index(&mut self, candidates: &[SaveDirectoryCandidate]) -> Vec<SaveSlot> {
        let mut searched_identities = std::collections::HashSet::new();
        let mut seen = std::collections::HashSet::new();
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

            let Ok(entries) = fs::read_dir(&identity) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = resolve_entry_path(&identity, &entry);
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                if is_non_slot_file(name) || !has_save_extension(name) {
                    continue;
                }

                if !seen.insert(path.clone()) {
                    continue;
                }

                found_files.push((path, candidate.game_id.clone(), candidate.release_id.clone()));
            }
        }

        // Entries for saves that were deleted or moved must not be written back into the index file.
        self.entries.retain(|path, _| seen.contains(path));

        let mut slots = Vec::with_capacity(found_files.len());
        let mut cold_targets = Vec::new();

        for (path, cand_game, cand_rel) in &found_files {
            let Ok(metadata) = fs::metadata(path) else {
                continue;
            };
            if !metadata.is_file() {
                continue;
            }
            let size = metadata.len();
            let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);

            let current_header_hash = crate::discovery::slot::read_file_header(path, HEADER_SAMPLE_BYTES)
                .ok()
                .map(|header| Self::compute_header_hash(&header));
            if let Some(cached) = current_header_hash.and_then(|hash| self.lookup(path, size, mtime, hash)) {
                slots.push(cached.to_save_slot());
            } else {
                cold_targets.push(ColdTarget {
                    path: path.clone(),
                    candidate_game_id: cand_game.clone(),
                    candidate_release_id: cand_rel.clone(),
                    size,
                    mtime,
                });
            }
        }

        if !cold_targets.is_empty() {
            let max_workers = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(1, 8);
            let num_workers = max_workers.min(cold_targets.len());
            let chunk_size = cold_targets
                .len()
                .checked_add(num_workers.saturating_sub(1))
                .and_then(|sum| sum.checked_div(num_workers))
                .unwrap_or(1)
                .max(1);

            let mut new_entries = Vec::with_capacity(cold_targets.len());

            if num_workers <= 1 {
                for target in &cold_targets {
                    new_entries.push(scan_single_index_entry(
                        &target.path,
                        &target.candidate_game_id,
                        &target.candidate_release_id,
                        target.size,
                        target.mtime,
                    ));
                }
            } else {
                let chunks: Vec<&[ColdTarget]> = cold_targets.chunks(chunk_size).collect();
                std::thread::scope(|s| {
                    let mut handles = Vec::with_capacity(chunks.len());
                    for chunk in chunks {
                        handles.push(s.spawn(move || {
                            let mut local = Vec::with_capacity(chunk.len());
                            for target in chunk {
                                local.push(scan_single_index_entry(
                                    &target.path,
                                    &target.candidate_game_id,
                                    &target.candidate_release_id,
                                    target.size,
                                    target.mtime,
                                ));
                            }
                            local
                        }));
                    }
                    for handle in handles {
                        if let Ok(mut batch) = handle.join() {
                            new_entries.append(&mut batch);
                        }
                    }
                });
            }

            for entry in new_entries {
                slots.push(entry.to_save_slot());
                self.insert(entry);
            }
        }

        sort_newest_first(&mut slots);

        slots
    }
}

struct ColdTarget {
    path: PathBuf,
    candidate_game_id: String,
    candidate_release_id: String,
    size: u64,
    mtime: SystemTime,
}

fn scan_single_index_entry(
    path: &Path,
    candidate_game_id: &str,
    candidate_release_id: &str,
    size: u64,
    mtime: SystemTime,
) -> LibraryIndexEntry {
    let (format_id, game_id, detection_error, header_hash) =
        match crate::discovery::slot::read_file_header(path, HEADER_SAMPLE_BYTES) {
            Ok(header) => {
                let (fid, gid, err) = crate::discovery::slot::detect_format_for_file(path, &header, size)
                    .unwrap_or_else(|error| (None, None, Some(format!("IOException: {error}"))));
                let hash = LibraryIndex::compute_header_hash(&header);
                (fid, gid, err, hash)
            }
            Err(err) => (None, None, Some(format!("IOException: {err}")), 0),
        };

    let (mtime_secs, mtime_nanos) = split_system_time(mtime);
    LibraryIndexEntry {
        path: path.to_path_buf(),
        candidate_game_id: candidate_game_id.to_string(),
        candidate_release_id: candidate_release_id.to_string(),
        size,
        mtime_secs,
        mtime_nanos,
        header_hash,
        format_id,
        game_id,
        detection_error,
    }
}

fn create_unique_temp_file(destination: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = destination.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "library index destination has no file name",
        )
    })?;
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    for _ in 0..128 {
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut temp_name = OsString::from(".");
        temp_name.push(file_name);
        temp_name.push(format!(".{}.{}.tmp", std::process::id(), counter));
        let temp_path = parent.join(temp_name);
        match OpenOptions::new().write(true).create_new(true).open(&temp_path) {
            Ok(file) => return Ok((temp_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique library index temporary file",
    ))
}

fn split_system_time(time: SystemTime) -> (i64, u32) {
    match time.duration_since(UNIX_EPOCH) {
        Ok(dur) => (dur.as_secs().try_into().unwrap_or(i64::MAX), dur.subsec_nanos()),
        Err(err) => {
            let dur = err.duration();
            let secs: i64 = dur.as_secs().try_into().unwrap_or(i64::MAX);
            (secs.saturating_neg(), dur.subsec_nanos())
        }
    }
}

fn write_string(output: &mut Vec<u8>, s: &str) -> io::Result<()> {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "library index string exceeds 65535 bytes"))?;
    output.extend_from_slice(&len.to_le_bytes());
    output.extend_from_slice(bytes);
    Ok(())
}

fn write_optional_string(output: &mut Vec<u8>, s: Option<&str>) -> io::Result<()> {
    match s {
        Some(val) if val.len() == usize::from(u16::MAX) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "library index optional string collides with the absent-value marker",
        )),
        Some(val) => write_string(output, val),
        None => {
            output.extend_from_slice(&0xFFFF_u16.to_le_bytes());
            Ok(())
        }
    }
}

fn read_string(bytes: &[u8], offset: &mut usize) -> Option<String> {
    let len_slice = bytes.get(*offset..offset.checked_add(2)?)?;
    let len = u16::from_le_bytes(len_slice.try_into().ok()?);
    *offset = offset.checked_add(2)?;

    let str_len = usize::from(len);
    let str_bytes = bytes.get(*offset..offset.checked_add(str_len)?)?;
    *offset = offset.checked_add(str_len)?;

    String::from_utf8(str_bytes.to_vec()).ok()
}

fn read_optional_string(bytes: &[u8], offset: &mut usize) -> Option<Option<String>> {
    let len_slice = bytes.get(*offset..offset.checked_add(2)?)?;
    let len = u16::from_le_bytes(len_slice.try_into().ok()?);
    if len == 0xFFFF {
        *offset = offset.checked_add(2)?;
        return Some(None);
    }
    let s = read_string(bytes, offset)?;
    Some(Some(s))
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset.checked_add(4)?)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_u32_le_offset(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let slice = bytes.get(*offset..offset.checked_add(4)?)?;
    *offset = offset.checked_add(4)?;
    Some(u32::from_le_bytes(slice.try_into().ok()?))
}

fn read_u64_le(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let slice = bytes.get(*offset..offset.checked_add(8)?)?;
    *offset = offset.checked_add(8)?;
    Some(u64::from_le_bytes(slice.try_into().ok()?))
}

fn read_i64_le(bytes: &[u8], offset: &mut usize) -> Option<i64> {
    let slice = bytes.get(*offset..offset.checked_add(8)?)?;
    *offset = offset.checked_add(8)?;
    Some(i64::from_le_bytes(slice.try_into().ok()?))
}
