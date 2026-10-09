//! Save preview thumbnails and S.T.A.L.K.E.R. 2 campaign metadata reader.
//!
//! Provides reading of X-Ray sidecar `.dds` preview textures and S2 campaign
//! and thumbnail data, with an LRU bounded memory cache.

use crate::dds::{DdsImage, RgbaImage};
use crate::file_tree::read_bounded_file;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_DDS_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB
const MAX_THUMBNAIL_BYTES: u64 = 4 * 1024 * 1024; // 4 MiB
const MAX_CAMPAIGN_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB
const RECORD_FIXED_SIZE: usize = 40;
const TICKS_AT_UNIX_EPOCH: i64 = 621_355_968_000_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;

/// Metadata for an S.T.A.L.K.E.R. 2 save slot parsed from `CampaignsSave.sav`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stalker2SlotMeta {
    /// 32-character hexadecimal GUID of the save slot.
    pub slot_guid: String,
    /// Internal location region string key.
    pub region_key: String,
    /// Active quest identifier string key.
    pub quest_key: String,
    /// Total play time in hours.
    pub play_hours: f64,
    /// UTC timestamp when the save was recorded.
    pub saved_at_utc: SystemTime,
}

impl Stalker2SlotMeta {
    /// Strips standard prefixes and suffixes from region key.
    ///
    /// Example: `sid_locations_region_iron_forest_name` -> `iron_forest`.
    #[must_use]
    pub fn region_slug(&self) -> &str {
        let mut s = self.region_key.as_str();
        if let Some(stripped) = s.strip_prefix("sid_locations_region_") {
            s = stripped;
        }
        if let Some(stripped) = s.strip_suffix("_name") {
            s = stripped;
        }
        s
    }
}

/// Reader for save preview thumbnails and S2 slot metadata.
pub struct SavePreviewReader;

impl SavePreviewReader {
    /// Extracts the DDS preview image located next to an X-Ray save.
    #[must_use]
    pub fn preview_xray(save_path: &Path) -> Option<RgbaImage> {
        let dds_path = save_path.with_extension("dds");
        let buffer = read_bounded_file(&dds_path, MAX_DDS_BYTES).ok()?;
        DdsImage::decode(&buffer).ok()
    }

    /// Extracts raw preview image bytes (JPEG) for an S.T.A.L.K.E.R. 2 save slot.
    #[must_use]
    pub fn preview_s2(save_path: &Path) -> Option<Vec<u8>> {
        let parent = save_path.parent()?;
        let root = parent.parent()?;
        let guid = slot_guid(save_path);
        let thumb_path = root.join("Thumbnails").join(format!("{guid}.sav"));

        let bytes = read_bounded_file(&thumb_path, MAX_THUMBNAIL_BYTES).ok()?;
        extract_jpeg_from_s2_data(&bytes)
    }

    /// Parses slot metadata for an S2 save from `CampaignsSave.sav`.
    #[must_use]
    pub fn s2_slot_meta(save_path: &Path) -> Option<Stalker2SlotMeta> {
        let parent = save_path.parent()?;
        let root = parent.parent()?;
        let index_path = root.join("CampaignsSave.sav");

        let bytes = read_bounded_file(&index_path, MAX_CAMPAIGN_BYTES).ok()?;
        let slots = parse_campaigns(&bytes);
        let guid = slot_guid(save_path);
        slots.get(&guid).cloned()
    }
}

fn extract_jpeg_from_s2_data(data: &[u8]) -> Option<Vec<u8>> {
    // If the data is already raw or contains a JPEG SOI marker near the start
    if let Some(pos) = find_jpeg_header(data) {
        if pos < 64 {
            let slice = data.get(pos..)?;
            return Some(slice.to_vec());
        }
    }
    None
}

fn find_jpeg_header(data: &[u8]) -> Option<usize> {
    const JPEG_SOI: [u8; 3] = [0xFF, 0xD8, 0xFF];
    data.windows(3).position(|w| w == JPEG_SOI)
}

/// Parses `CampaignsSave.sav` binary payload into slot metadata records.
#[must_use]
pub fn parse_campaigns(raw: &[u8]) -> HashMap<String, Stalker2SlotMeta> {
    let mut result = HashMap::new();
    let end = raw.windows(12).position(|w| w == b"Achievements").unwrap_or(raw.len());

    let search_limit = raw.len().min(128);
    let name_slice = raw.get(13..search_limit);
    let name_end = match name_slice.and_then(|s| s.iter().position(|&b| b == 0)) {
        Some(pos) => pos,
        None => return result,
    };

    let mut position = 13_usize.saturating_add(name_end).saturating_add(1);
    let mut strings: Vec<String> = Vec::new();

    while position.checked_add(RECORD_FIXED_SIZE).is_some_and(|p| p <= end) {
        let Some((meta, next_pos)) = parse_campaign_record(raw, position, end, &mut strings) else {
            break;
        };
        result.insert(meta.slot_guid.clone(), meta);
        position = next_pos;
    }

    result
}

fn parse_campaign_record(
    raw: &[u8],
    position: usize,
    _end: usize,
    strings: &mut Vec<String>,
) -> Option<(Stalker2SlotMeta, usize)> {
    let mut guid_str = String::with_capacity(32);
    for part in 0..4_usize {
        let offset = position.checked_add(4)?.checked_add(part.checked_mul(4)?)?;
        let slice = raw.get(offset..offset.checked_add(4)?)?;
        let arr: [u8; 4] = slice.try_into().ok()?;
        let val = u32::from_le_bytes(arr);
        guid_str.push_str(&format!("{val:08X}"));
    }

    let ticks_slice = raw.get(position.checked_add(28)?..position.checked_add(36)?)?;
    let ticks_arr: [u8; 8] = ticks_slice.try_into().ok()?;
    let ticks = i64::from_le_bytes(ticks_arr);

    let seconds_slice = raw.get(position.checked_add(36)?..position.checked_add(40)?)?;
    let seconds_arr: [u8; 4] = seconds_slice.try_into().ok()?;
    let seconds = f32::from_le_bytes(seconds_arr);

    let mut offset = position.checked_add(RECORD_FIXED_SIZE)?;
    let region = read_campaign_string(raw, &mut offset, strings)?;
    let quest = read_campaign_string(raw, &mut offset, strings)?;

    if ticks <= 0 || !(0.0..1e8).contains(&seconds) {
        return None;
    }

    let saved_at_utc = ticks_to_system_time(ticks);
    let play_hours = f64::from(seconds) / 3600.0;
    let next_pos = offset.saturating_add(6);

    Some((
        Stalker2SlotMeta {
            slot_guid: guid_str,
            region_key: region,
            quest_key: quest,
            play_hours,
            saved_at_utc,
        },
        next_pos,
    ))
}

fn read_campaign_string(data: &[u8], offset: &mut usize, strings: &mut Vec<String>) -> Option<String> {
    let index_slice = data.get(*offset..offset.checked_add(2)?)?;
    let index = u16::from_le_bytes(index_slice.try_into().ok()?);
    *offset = offset.checked_add(2)?;

    let index_usize = usize::from(index);
    if index_usize == strings.len() {
        let len_slice = data.get(*offset..offset.checked_add(2)?)?;
        let len = u16::from_le_bytes(len_slice.try_into().ok()?);
        *offset = offset.checked_add(2)?;

        let len_usize = usize::from(len);
        let text_bytes = data.get(*offset..offset.checked_add(len_usize)?)?;
        *offset = offset.checked_add(len_usize)?;

        // Ensure valid ASCII
        if text_bytes.iter().any(|&b| b > 0x7F) {
            return None;
        }
        let text = String::from_utf8(text_bytes.to_vec()).ok()?;
        strings.push(text.clone());
        Some(text)
    } else {
        strings.get(index_usize).cloned()
    }
}

fn ticks_to_system_time(ticks: i64) -> SystemTime {
    if ticks <= TICKS_AT_UNIX_EPOCH {
        return UNIX_EPOCH;
    }
    let diff_ticks = ticks.saturating_sub(TICKS_AT_UNIX_EPOCH);
    let seconds = diff_ticks / TICKS_PER_SECOND;
    let subsec_ticks = diff_ticks % TICKS_PER_SECOND;
    let nanos = u32::try_from(subsec_ticks.saturating_mul(100)).unwrap_or(0);

    let secs_u64 = u64::try_from(seconds).unwrap_or(0);
    UNIX_EPOCH
        .checked_add(Duration::new(secs_u64, nanos))
        .unwrap_or(UNIX_EPOCH)
}

/// Extracts uppercase GUID stem for save slot files.
#[must_use]
pub fn slot_guid(save_path: &Path) -> String {
    let stem = save_path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let len = stem.len().min(32);
    stem.get(..len).map(|s| s.to_ascii_uppercase()).unwrap_or_default()
}

/// LRU cache for decoded save preview images with a stated memory budget.
pub struct PreviewCache {
    max_bytes: usize,
    current_bytes: usize,
    entries: HashMap<PathBuf, RgbaImage>,
    order: VecDeque<PathBuf>,
}

impl PreviewCache {
    /// Creates a preview cache with a specified maximum memory budget in bytes.
    #[must_use]
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            current_bytes: 0,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// Current memory usage of cached images in bytes.
    #[must_use]
    pub fn current_memory_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Maximum memory limit in bytes.
    #[must_use]
    pub fn max_memory_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Retrieves an image from cache, promoting it to most recently used.
    pub fn get(&mut self, path: &Path) -> Option<&RgbaImage> {
        if self.entries.contains_key(path) {
            // Promote in LRU queue
            if let Some(pos) = self.order.iter().position(|p| p == path) {
                if let Some(p) = self.order.remove(pos) {
                    self.order.push_back(p);
                }
            }
            self.entries.get(path)
        } else {
            None
        }
    }

    /// Inserts an image into the cache, evicting least recently used entries if needed.
    pub fn insert(&mut self, path: PathBuf, image: RgbaImage) {
        let image_bytes = image.pixels.len();
        if image_bytes > self.max_bytes {
            return;
        }

        // If updating existing entry, remove old size
        if let Some(old) = self.entries.remove(&path) {
            self.current_bytes = self.current_bytes.saturating_sub(old.pixels.len());
            self.order.retain(|p| p != &path);
        }

        while self.current_bytes.saturating_add(image_bytes) > self.max_bytes {
            if let Some(old_path) = self.order.pop_front() {
                if let Some(removed) = self.entries.remove(&old_path) {
                    self.current_bytes = self.current_bytes.saturating_sub(removed.pixels.len());
                }
            } else {
                break;
            }
        }

        self.current_bytes = self.current_bytes.saturating_add(image_bytes);
        self.order.push_back(path.clone());
        self.entries.insert(path, image);
    }

    /// Clears all entries from the cache.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.current_bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::read_bounded_file;
    use std::io::{self, Read};

    struct CountingReader {
        remaining: usize,
        bytes_read: usize,
    }

    impl Read for CountingReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let count = buffer.len().min(self.remaining);
            for byte in buffer.iter_mut().take(count) {
                *byte = b'x';
            }
            self.remaining = self.remaining.saturating_sub(count);
            self.bytes_read = self.bytes_read.saturating_add(count);
            Ok(count)
        }
    }

    #[test]
    fn bounded_preview_reader_stops_after_one_byte_over_the_limit() {
        let mut reader = CountingReader {
            remaining: 100,
            bytes_read: 0,
        };

        let result = crate::file_tree::read_bounded_bytes(&mut reader, 8);

        assert!(matches!(result, Err(sse_core::Error::Refused(_))));
        assert_eq!(reader.bytes_read, 9);
    }

    #[test]
    fn bounded_reader_accepts_input_at_the_exact_limit() {
        let input = b"12345678";

        let result = crate::file_tree::read_bounded_bytes(input.as_slice(), input.len() as u64);

        assert_eq!(result.as_deref(), Ok(input.as_slice()));
    }

    #[test]
    fn bounded_preview_file_rejects_an_oversized_open_file() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("sse-preview-bound-{unique}.sav"));
        assert!(
            std::fs::write(&path, b"123456789").is_ok(),
            "could not create preview fixture"
        );

        let result = read_bounded_file(&path, 8);
        let removed = std::fs::remove_file(path);

        assert!(matches!(result, Err(sse_core::Error::Refused(_))));
        assert!(removed.is_ok(), "could not remove preview fixture");
    }

    #[test]
    fn bounded_file_accepts_input_at_the_exact_limit() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("sse-preview-exact-bound-{unique}.sav"));
        assert!(
            std::fs::write(&path, b"12345678").is_ok(),
            "could not create preview fixture"
        );

        let result = read_bounded_file(&path, 8);
        let removed = std::fs::remove_file(path);

        assert_eq!(result.as_deref(), Ok(b"12345678".as_slice()));
        assert!(removed.is_ok(), "could not remove preview fixture");
    }
}
