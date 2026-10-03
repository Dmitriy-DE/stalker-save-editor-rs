//! Binary library index format for fast, zero-open save list loading.
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

use crate::discovery::locator::{normalize_full_path, SaveDirectoryCandidate};
use crate::discovery::slot::{detect_format, SaveSlot};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const INDEX_MAGIC: &[u8; 4] = b"SSLI";
const INDEX_VERSION: u32 = 1;
const HEADER_SAMPLE_BYTES: usize = 4096;
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

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

/// Binary index of save slot metadata, avoiding file opens on warm starts.
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

        Some(Self { entries })
    }

    /// Serializes the library index to raw bytes.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        output.extend_from_slice(INDEX_MAGIC);
        output.extend_from_slice(&INDEX_VERSION.to_le_bytes());
        let count_u32 = u32::try_from(self.entries.len()).unwrap_or(u32::MAX);
        output.extend_from_slice(&count_u32.to_le_bytes());

        for entry in self.entries.values() {
            write_string(&mut output, &entry.path.to_string_lossy());
            write_string(&mut output, &entry.candidate_game_id);
            write_string(&mut output, &entry.candidate_release_id);
            output.extend_from_slice(&entry.size.to_le_bytes());
            output.extend_from_slice(&entry.mtime_secs.to_le_bytes());
            output.extend_from_slice(&entry.mtime_nanos.to_le_bytes());
            output.extend_from_slice(&entry.header_hash.to_le_bytes());
            write_optional_string(&mut output, entry.format_id.as_deref());
            write_optional_string(&mut output, entry.game_id.as_deref());
            write_optional_string(&mut output, entry.detection_error.as_deref());
        }

        output
    }

    /// Saves the library index atomically to a destination path.
    ///
    /// # Errors
    /// Returns [`io::Error`] if creating or replacing the file fails.
    pub fn save(&self, destination: &Path) -> io::Result<()> {
        let encoded = self.encode();
        let temp_path = destination.with_extension("tmp");
        {
            let mut file = File::create(&temp_path)?;
            file.write_all(&encoded)?;
            file.sync_all()?;
        }
        fs::rename(&temp_path, destination)
    }

    /// Looks up a cached entry by matching path, file size, and modification time.
    #[must_use]
    pub fn lookup(&self, path: &Path, size: u64, mtime: SystemTime) -> Option<&LibraryIndexEntry> {
        let (mtime_secs, mtime_nanos) = split_system_time(mtime);
        let entry = self.entries.get(path)?;
        if entry.size == size && entry.mtime_secs == mtime_secs && entry.mtime_nanos == mtime_nanos {
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
        let mut slots = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for candidate in candidates {
            let directory = normalize_full_path(&candidate.directory_path);
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };

            for entry in entries.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };

                let lower = name.to_ascii_lowercase();
                if lower == "campaignssave.sav"
                    || lower == "analyticsdata.sav"
                    || !(lower.ends_with(".sav") || lower.ends_with(".scop") || lower.ends_with(".scs"))
                {
                    continue;
                }

                let full_path = normalize_full_path(&path);
                if !seen.insert(full_path.clone()) {
                    continue;
                }

                let Ok(metadata) = fs::metadata(&full_path) else {
                    continue;
                };

                if !metadata.is_file() {
                    continue;
                }

                let size = metadata.len();
                let mtime = metadata.modified().unwrap_or(UNIX_EPOCH);

                if let Some(cached) = self.lookup(&full_path, size, mtime) {
                    slots.push(cached.to_save_slot());
                    continue;
                }

                // Cold read: read file header/bytes, detect format, record hash, update index
                let (format_id, game_id, detection_error, header_hash) = match fs::read(&full_path) {
                    Ok(bytes) => {
                        let (fid, gid, err) = detect_format(&bytes);
                        let hash = Self::compute_header_hash(&bytes);
                        (fid, gid, err, hash)
                    }
                    Err(err) => (None, None, Some(format!("IOException: {err}")), 0),
                };

                let (mtime_secs, mtime_nanos) = split_system_time(mtime);
                let entry = LibraryIndexEntry {
                    path: full_path.clone(),
                    candidate_game_id: candidate.game_id.clone(),
                    candidate_release_id: candidate.release_id.clone(),
                    size,
                    mtime_secs,
                    mtime_nanos,
                    header_hash,
                    format_id: format_id.clone(),
                    game_id: game_id.clone(),
                    detection_error: detection_error.clone(),
                };

                slots.push(entry.to_save_slot());
                self.insert(entry);
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

        slots
    }
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

fn write_string(output: &mut Vec<u8>, s: &str) {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len()).unwrap_or(u16::MAX);
    output.extend_from_slice(&len.to_le_bytes());
    let write_len = usize::from(len);
    if let Some(sub) = bytes.get(..write_len) {
        output.extend_from_slice(sub);
    }
}

fn write_optional_string(output: &mut Vec<u8>, s: Option<&str>) {
    match s {
        Some(val) => write_string(output, val),
        None => output.extend_from_slice(&0xFFFF_u16.to_le_bytes()),
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
