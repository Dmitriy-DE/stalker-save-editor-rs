//! Inventory item icon service resolving shipped and installed game icons.
//!
//! Features:
//! - Shipped icon atlas with single-page on-demand decoding
//! - Built-in icon alias resolution mapping shared graphics
//! - Bounded negative cache (capped at 4096 entries)
//! - Low idle memory consumption (<= 4 MiB)

use crate::atlas::IconAtlas;
use crate::dds::RgbaImage;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

const SHIPPED_ATLAS: &[u8] = include_bytes!("../data/icons.atlas");
const SHIPPED_ALIASES_JSON: &str = include_str!("../data/icon-aliases.json");
const MAXIMUM_CACHED_KEYS: usize = 4096;

/// Inventory item icon service.
pub struct ItemIconService {
    atlas: Option<IconAtlas<'static>>,
    aliases: HashMap<String, String>,
    decoded_page_cache: Mutex<Option<(usize, RgbaImage)>>,
    icon_cache: Mutex<HashMap<String, RgbaImage>>,
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
            icon_cache: Mutex::new(HashMap::new()),
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
        let key = self.icon_key(release_id, item_key)?;

        // Check icon cache
        if let Ok(cache) = self.icon_cache.lock() {
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
        let icons = self.icon_cache.lock().map_or(0, |c| c.len());
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
