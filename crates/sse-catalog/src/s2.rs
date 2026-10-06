//! S.T.A.L.K.E.R. 2 item and upgrade catalogs.

use sse_core::{Error, Result};
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::value::{parse_json, JsonValue};

const EMBEDDED_S2_ITEMS_RAW: &[u8] = include_bytes!("../data/s2_items.json");
const EMBEDDED_S2_UPGRADES_RAW: &[u8] = include_bytes!("../data/s2_upgrades.json");

static EMBEDDED_S2_ITEMS: OnceLock<Stalker2ItemCatalog> = OnceLock::new();
static EMBEDDED_S2_ARMOR_MAP: OnceLock<HashMap<String, Stalker2ArmorUpgrade>> = OnceLock::new();

/// Item entry in the S.T.A.L.K.E.R. 2 catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stalker2ItemEntry {
    /// Item SID.
    pub sid: String,
    /// Official names by language.
    pub names: Vec<(String, String)>,
    /// Official descriptions by language.
    pub descriptions: Vec<(String, String)>,
    /// Relative icon file path (e.g. "s2/A012A.png").
    pub icon: Option<String>,
    /// Parent variant SID if any.
    pub variant_of: Option<String>,
}

const FAMILY_ICONS: &[(&str, &str)] = &[
    ("pda", "s2/KozimkovPDA.png"),
    ("blueprint_", "s2/Blueprint_Gvintar_Upgrade_1.png"),
];

/// Catalog of S.T.A.L.K.E.R. 2 items.
pub struct Stalker2ItemCatalog {
    data: JsonValue,
}

impl Stalker2ItemCatalog {
    /// Loads the S2 item catalog from JSON bytes.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed JSON payload.
    pub fn load(payload: &[u8]) -> Result<Self> {
        let json_str =
            std::str::from_utf8(payload).map_err(|e| Error::damaged(format!("Invalid UTF-8 in S2 items JSON: {e}")))?;
        let value = parse_json(json_str)?;

        if value.get("items").and_then(JsonValue::as_object).is_none() {
            return Err(Error::damaged("S2 item catalog has no items object."));
        }

        Ok(Self { data: value })
    }

    /// Loads the embedded S2 item catalog.
    #[must_use]
    pub fn load_embedded() -> &'static Self {
        EMBEDDED_S2_ITEMS.get_or_init(|| Self::load(EMBEDDED_S2_ITEMS_RAW).unwrap_or(Self { data: JsonValue::Null }))
    }

    /// Number of items in catalog.
    #[must_use]
    pub fn count(&self) -> usize {
        self.data
            .get("items")
            .and_then(JsonValue::as_object)
            .map(|o| o.len())
            .unwrap_or(0)
    }

    /// Resolves canonical SID handling case folding, `_Player` suffix and `GuardGun` prefix.
    #[must_use]
    pub fn canonical_sid<'a>(&'a self, sid: Option<&str>) -> Option<&'a str> {
        let value = sid?.trim();
        if value.is_empty() {
            return None;
        }

        let mut candidates = Vec::with_capacity(3);
        candidates.push(value.to_string());

        if let Some(stripped) = value.strip_suffix("_Player") {
            candidates.push(stripped.to_string());
        }
        if let Some(rest) = value.strip_prefix("GuardGun") {
            candidates.push(format!("Gun{rest}"));
        }

        let items = self.data.get("items")?.as_object()?;

        for cand in &candidates {
            if let Some((exact_key, _)) = items.iter().find(|(k, _)| k == cand) {
                return Some(exact_key.as_str());
            }
        }
        for cand in &candidates {
            if let Some((key, _)) = items.iter().find(|(k, _)| k.eq_ignore_ascii_case(cand)) {
                return Some(key.as_str());
            }
        }

        None
    }

    /// Resolves an entry by its save SID.
    #[must_use]
    pub fn resolve(&self, sid: Option<&str>) -> Option<Stalker2ItemEntry> {
        let key = self.canonical_sid(sid)?;
        let item_val = self.data.get("items")?.get(key)?;

        let names = parse_strings(item_val.get("names"));
        let descriptions = parse_strings(item_val.get("descriptions"));
        let icon = item_val
            .get("icon")
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from);
        let variant_of = item_val
            .get("variant_of")
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from);

        Some(Stalker2ItemEntry {
            sid: key.to_string(),
            names,
            descriptions,
            icon,
            variant_of,
        })
    }

    /// Resolves item display name with language fallback (requested -> "en").
    #[must_use]
    pub fn name(&self, sid: Option<&str>, language: &str) -> Option<&str> {
        let key = self.canonical_sid(sid)?;
        let item = self.data.get("items")?.get(key)?;
        let names = item.get("names")?;
        names
            .get(language)
            .or_else(|| names.get("en"))
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Resolves item description with language fallback (requested -> "en").
    #[must_use]
    pub fn description(&self, sid: Option<&str>, language: &str) -> Option<&str> {
        let key = self.canonical_sid(sid)?;
        let item = self.data.get("items")?.get(key)?;
        let descriptions = item.get("descriptions")?;
        descriptions
            .get(language)
            .or_else(|| descriptions.get("en"))
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Resolves icon relative path, or family fallback.
    #[must_use]
    pub fn icon(&self, sid: Option<&str>) -> Option<&str> {
        if let Some(key) = self.canonical_sid(sid) {
            if let Some(item) = self.data.get("items").and_then(|it| it.get(key)) {
                if let Some(ic) = item.get("icon").and_then(JsonValue::as_str).filter(|s| !s.is_empty()) {
                    return Some(ic);
                }
            }
        }
        let sid = sid?.trim();
        if sid.is_empty() {
            return None;
        }
        let folded = sid.to_ascii_lowercase();
        for &(marker, icon) in FAMILY_ICONS {
            if folded.contains(marker) {
                return Some(icon);
            }
        }
        None
    }
}

fn parse_strings(val: Option<&JsonValue>) -> Vec<(String, String)> {
    let mut list = Vec::new();
    if let Some(entries) = val.and_then(JsonValue::as_object) {
        list.reserve(entries.len());
        for (k, v) in entries {
            if let Some(s) = v.as_str() {
                if !s.is_empty() {
                    list.push((k.clone(), s.to_string()));
                }
            }
        }
    }
    list
}

/// S2 armor upgrade metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stalker2ArmorUpgrade {
    /// Effect key (e.g. "psy", "chem").
    pub effect: String,
    /// Tier level (1, 2, 3).
    pub tier: i32,
}

/// S2 armor and weapon upgrade lookup.
pub struct Stalker2ArmorUpgrades;

impl Stalker2ArmorUpgrades {
    fn load_armors() -> HashMap<String, Stalker2ArmorUpgrade> {
        let json_str = std::str::from_utf8(EMBEDDED_S2_UPGRADES_RAW).unwrap_or("");
        let value = parse_json(json_str).unwrap_or(JsonValue::Null);

        let mut armors = HashMap::new();
        if let Some(upgrades) = value.get("upgrades").and_then(JsonValue::as_object) {
            for (sid, up_val) in upgrades {
                let effect = up_val
                    .get("effect")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("")
                    .to_string();
                let tier = up_val
                    .get("tier")
                    .and_then(JsonValue::as_i64)
                    .and_then(|v| i32::try_from(v).ok())
                    .unwrap_or(0);
                armors.insert(sid.clone(), Stalker2ArmorUpgrade { effect, tier });
            }
        }
        armors
    }

    /// Finds an armor upgrade by its save prototype SID.
    #[must_use]
    pub fn find_armor(sid: &str) -> Option<&'static Stalker2ArmorUpgrade> {
        let map = EMBEDDED_S2_ARMOR_MAP.get_or_init(Self::load_armors);
        map.get(sid)
    }

    /// Number of registered armor upgrades.
    #[must_use]
    pub fn count() -> usize {
        if let Some(map) = EMBEDDED_S2_ARMOR_MAP.get() {
            return map.len();
        }
        670
    }
}
