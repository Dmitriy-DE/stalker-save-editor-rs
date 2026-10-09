//! Inventory item icon service resolving shipped and installed game icons.
//!
//! Features:
//! - Shipped icon atlas with single-page on-demand decoding
//! - Built-in icon alias resolution mapping shared graphics
//! - Bounded negative cache (capped at 4096 entries)
//! - Low idle memory consumption (<= 4 MiB)

use crate::atlas::IconAtlas;
use crate::dds::RgbaImage;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Mutex;

const SHIPPED_ATLAS: &[u8] = include_bytes!("../data/icons.atlas");
const SHIPPED_ALIASES_JSON: &str = include_str!("../data/icon-aliases.json");
const MAXIMUM_CACHED_KEYS: usize = 4096;
const MAXIMUM_ICON_CACHE_BYTES: usize = 4 * 1024 * 1024;
const MAXIMUM_ICON_CACHE_ENTRIES: usize = 256;
const MAXIMUM_ICON_KEY_BYTES: usize = 512;

#[derive(Default)]
struct IconCache {
    entries: HashMap<String, RgbaImage>,
    order: VecDeque<String>,
    current_bytes: usize,
}

impl IconCache {
    fn get(&mut self, key: &str) -> Option<&RgbaImage> {
        if self.entries.contains_key(key) {
            if let Some(position) = self.order.iter().position(|cached| cached == key) {
                if let Some(promoted) = self.order.remove(position) {
                    self.order.push_back(promoted);
                }
            }
        }
        self.entries.get(key)
    }

    fn insert(&mut self, key: String, image: RgbaImage) {
        let image_bytes = image.pixels.len();
        if key.len() > MAXIMUM_ICON_KEY_BYTES || image_bytes > MAXIMUM_ICON_CACHE_BYTES {
            return;
        }

        if let Some(previous) = self.entries.remove(&key) {
            self.current_bytes = self.current_bytes.saturating_sub(previous.pixels.len());
            if let Some(position) = self.order.iter().position(|cached| cached == &key) {
                self.order.remove(position);
            }
        }

        while self.entries.len() >= MAXIMUM_ICON_CACHE_ENTRIES
            || self.current_bytes.saturating_add(image_bytes) > MAXIMUM_ICON_CACHE_BYTES
        {
            let Some(least_recently_used) = self.order.pop_front() else {
                break;
            };
            if let Some(removed) = self.entries.remove(&least_recently_used) {
                self.current_bytes = self.current_bytes.saturating_sub(removed.pixels.len());
            }
        }
        self.current_bytes = self.current_bytes.saturating_add(image_bytes);
        self.order.push_back(key.clone());
        self.entries.insert(key, image);
    }
}

/// Inventory item icon service.
pub struct ItemIconService {
    atlas: Option<IconAtlas<'static>>,
    aliases: HashMap<String, String>,
    decoded_page_cache: Mutex<Option<(usize, RgbaImage)>>,
    icon_cache: Mutex<IconCache>,
    miss_cache: Mutex<HashSet<String>>,
}

impl ItemIconService {
    /// Creates a new icon service backed by the shipped icon atlas.
    #[must_use]
    pub fn new() -> Self {
        let atlas = IconAtlas::parse(SHIPPED_ATLAS);
        let aliases = parse_aliases_json(SHIPPED_ALIASES_JSON);
        Self {
            atlas,
            aliases,
            decoded_page_cache: Mutex::new(None),
            icon_cache: Mutex::new(IconCache::default()),
            miss_cache: Mutex::new(HashSet::new()),
        }
    }

    /// Resolves the relative icon key for an item, applying alias mapping.
    #[must_use]
    pub fn icon_key(&self, release_id: &str, item_key: &str) -> Option<String> {
        if item_key.trim().is_empty() {
            return None;
        }

        let raw_key = if release_id.starts_with("stalker2") {
            if item_key.ends_with(".png") {
                item_key.to_string()
            } else {
                format!("s2/{item_key}.png")
            }
        } else if item_key.ends_with(".png") {
            item_key.to_string()
        } else {
            format!("xray/{item_key}.png")
        };

        if let Some(canonical) = self.aliases.get(&raw_key) {
            Some(canonical.clone())
        } else {
            Some(raw_key)
        }
    }

    /// Loads the icon for a release and item key, returning an RGBA image if found.
    #[must_use]
    pub fn load(&self, release_id: &str, item_key: &str) -> Option<RgbaImage> {
        if item_key.len() > MAXIMUM_ICON_KEY_BYTES {
            return None;
        }
        let key = self.icon_key(release_id, item_key)?;
        if key.len() > MAXIMUM_ICON_KEY_BYTES {
            return None;
        }

        // Check icon cache
        if let Ok(mut cache) = self.icon_cache.lock() {
            if let Some(img) = cache.get(&key) {
                return Some(img.clone());
            }
        }

        // Check miss cache
        if let Ok(misses) = self.miss_cache.lock() {
            if misses.contains(&key) {
                return None;
            }
        }

        let atlas = self.atlas.as_ref()?;
        let entry = match atlas.get_entry(&key) {
            Some(e) => e,
            None => {
                self.record_miss(key);
                return None;
            }
        };

        let page_index = usize::from(entry.page_index);
        let mut page_cache_guard = self.decoded_page_cache.lock().ok()?;

        let need_decode = match page_cache_guard.as_ref() {
            Some((idx, _)) => *idx != page_index,
            None => true,
        };

        if need_decode {
            let decoded_page = atlas.decode_page(page_index)?;
            *page_cache_guard = Some((page_index, decoded_page));
        }

        let (_, page_img) = page_cache_guard.as_ref()?;
        let cropped = atlas.crop_icon(entry, page_img)?;

        // Cache icon
        if let Ok(mut icon_cache) = self.icon_cache.lock() {
            icon_cache.insert(key, cropped.clone());
        }

        Some(cropped)
    }

    /// Returns the number of cached keys (both loaded and missed).
    #[must_use]
    pub fn cached_key_count(&self) -> usize {
        let icons = self.icon_cache.lock().map_or(0, |cache| cache.entries.len());
        let misses = self.miss_cache.lock().map_or(0, |m| m.len());
        icons.saturating_add(misses)
    }

    fn record_miss(&self, key: String) {
        if let Ok(mut misses) = self.miss_cache.lock() {
            if misses.len() >= MAXIMUM_CACHED_KEYS {
                misses.clear();
            }
            misses.insert(key);
        }
    }
}

impl Default for ItemIconService {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_aliases_json(json_str: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let trimmed = json_str.trim().trim_start_matches('{').trim_end_matches('}');
    for line in trimmed.lines() {
        let line_clean = line.trim().trim_end_matches(',');
        if let Some((k, v)) = line_clean.split_once(':') {
            let key = k.trim().trim_matches('"').to_string();
            let val = v.trim().trim_matches('"').to_string();
            if !key.is_empty() && !val.is_empty() {
                map.insert(key, val);
            }
        }
    }
    map
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::unwrap_used
    )]

    use super::{IconCache, ItemIconService, MAXIMUM_ICON_CACHE_BYTES, MAXIMUM_ICON_CACHE_ENTRIES, SHIPPED_ATLAS};
    use crate::dds::RgbaImage;

    fn page_zero_entries() -> Vec<(String, usize)> {
        let count_bytes: [u8; 4] = SHIPPED_ATLAS.get(10..14).unwrap().try_into().unwrap();
        let entry_count = usize::try_from(u32::from_le_bytes(count_bytes)).unwrap();
        let mut offset = 14_usize;
        let mut entries = Vec::new();

        for _ in 0..entry_count {
            let length_bytes: [u8; 2] = SHIPPED_ATLAS.get(offset..offset + 2).unwrap().try_into().unwrap();
            let name_len = usize::from(u16::from_le_bytes(length_bytes));
            offset += 2;
            let name_end = offset + name_len;
            let name = String::from_utf8(SHIPPED_ATLAS.get(offset..name_end).unwrap().to_vec()).unwrap();
            offset = name_end;

            let entry_bytes = SHIPPED_ATLAS.get(offset..offset + 10).unwrap();
            let page = u16::from_le_bytes(entry_bytes.get(..2).unwrap().try_into().unwrap());
            let width = usize::from(u16::from_le_bytes(entry_bytes.get(6..8).unwrap().try_into().unwrap()));
            let height = usize::from(u16::from_le_bytes(entry_bytes.get(8..10).unwrap().try_into().unwrap()));
            offset += 10;

            if page == 0 {
                entries.push((name, width * height * 4));
            }
        }

        entries
    }

    fn load_icon(service: &ItemIconService, name: &str) {
        let (release, key) = if let Some(key) = name.strip_prefix("s2/") {
            ("stalker2", key)
        } else {
            ("stalker-cop", name.strip_prefix("xray/").unwrap())
        };
        let item_key = key.strip_suffix(".png").unwrap();
        assert!(
            service.load(release, item_key).is_some(),
            "missing shipped icon: {name}"
        );
    }

    #[test]
    fn successful_icons_stay_within_the_positive_cache_byte_budget_using_lru() {
        let entries = page_zero_entries();
        let mut prefix_bytes = 0_usize;
        let mut prefix_len = 0_usize;
        while let Some((_, bytes)) = entries.get(prefix_len) {
            if prefix_bytes.saturating_add(*bytes) > MAXIMUM_ICON_CACHE_BYTES {
                break;
            }
            prefix_bytes = prefix_bytes.saturating_add(*bytes);
            prefix_len = prefix_len.saturating_add(1);
        }
        let (crossing_name, crossing_bytes) = entries.get(prefix_len).unwrap();
        assert!(prefix_bytes <= MAXIMUM_ICON_CACHE_BYTES);
        assert!(prefix_bytes.saturating_add(*crossing_bytes) > MAXIMUM_ICON_CACHE_BYTES);

        let service = ItemIconService::new();
        for (name, _) in entries.iter().take(prefix_len) {
            load_icon(&service, name);
        }
        let first_name = entries.first().unwrap().0.clone();
        let second_name = entries.get(1).unwrap().0.clone();
        load_icon(&service, &first_name);
        load_icon(&service, crossing_name);

        let cache = service.icon_cache.lock().unwrap();
        let cached_bytes = cache
            .entries
            .values()
            .fold(0_usize, |total, image| total.saturating_add(image.pixels.len()));
        assert!(cached_bytes <= MAXIMUM_ICON_CACHE_BYTES);
        assert!(
            cache.entries.contains_key(&first_name),
            "a recently used icon should remain cached"
        );
        assert!(
            !cache.entries.contains_key(&second_name),
            "the least recently used icon should be evicted"
        );
    }

    #[test]
    fn oversized_icon_keys_are_not_retained_as_misses() {
        let service = ItemIconService::new();
        let key = "x".repeat(513);

        assert!(service.load("stalker-cop", &key).is_none());
        assert_eq!(service.cached_key_count(), 0);
    }

    #[test]
    fn successful_icon_cache_stays_within_its_entry_limit() {
        let mut cache = IconCache::default();
        for index in 0..=MAXIMUM_ICON_CACHE_ENTRIES {
            cache.insert(format!("icon-{index}"), RgbaImage::new(1, 1, vec![0; 4]));
        }

        assert_eq!(cache.entries.len(), MAXIMUM_ICON_CACHE_ENTRIES);
        assert!(!cache.entries.contains_key("icon-0"));
        assert!(cache
            .entries
            .contains_key(&format!("icon-{MAXIMUM_ICON_CACHE_ENTRIES}")));
        assert!(cache.current_bytes <= MAXIMUM_ICON_CACHE_BYTES);
    }
}
