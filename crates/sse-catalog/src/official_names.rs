//! Official names catalog reading official game strings from string tables.

use sse_core::{Error, Result};
use std::sync::OnceLock;

use crate::value::{parse_json, JsonValue};
use sse_codecs::embedded_json::{self, JsonAssetCache};

const EMBEDDED_NAMES_RAW: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/data_catalog_names.json.deflate"));

static EMBEDDED_OFFICIAL_NAMES: OnceLock<OfficialNamesCatalog> = OnceLock::new();
static EMBEDDED_NAMES_JSON: JsonAssetCache = OnceLock::new();

/// Shipped names for trilogy items, stashes, levels, factions, and upgrades.
pub struct OfficialNamesCatalog {
    data: JsonValue,
}

impl OfficialNamesCatalog {
    /// Loads an official names catalog from raw JSON bytes.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed payload.
    pub fn load(payload: &[u8]) -> Result<Self> {
        let json_str = std::str::from_utf8(payload)
            .map_err(|e| Error::damaged(format!("Invalid UTF-8 in official names JSON: {e}")))?;
        let value = parse_json(json_str)?;

        let releases = value
            .get("releases")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| Error::damaged("Official names document does not contain releases."))?;

        for (rel_name, rel_val) in releases {
            let kinds = rel_val
                .as_object()
                .ok_or_else(|| Error::damaged(format!("Invalid official names family '{rel_name}'.")))?;
            for (kind_name, kind_val) in kinds {
                let entries = kind_val
                    .as_object()
                    .ok_or_else(|| Error::damaged(format!("Invalid official names kind '{kind_name}'.")))?;
                for (entry_name, entry_val) in entries {
                    let languages = entry_val
                        .as_object()
                        .ok_or_else(|| Error::damaged(format!("Invalid official names entry '{entry_name}'.")))?;
                    for (_lang_name, lang_val) in languages {
                        if lang_val.as_str().is_none() {
                            return Err(Error::damaged(format!(
                                "Invalid official name translation '{entry_name}'."
                            )));
                        }
                    }
                }
            }
        }

        Ok(Self { data: value })
    }

    /// Loads the embedded official names catalog.
    #[must_use]
    pub fn load_embedded() -> &'static Self {
        EMBEDDED_OFFICIAL_NAMES.get_or_init(|| {
            embedded_json::get_json(EMBEDDED_NAMES_RAW, &EMBEDDED_NAMES_JSON)
                .and_then(Self::load)
                .unwrap_or(Self { data: JsonValue::Null })
        })
    }

    /// Resolves an official name with language fallback (requested -> "en" -> "ru").
    #[must_use]
    pub fn resolve(
        &self,
        release_id: Option<&str>,
        kind: &str,
        key: Option<&str>,
        language: Option<&str>,
    ) -> Option<String> {
        let family = Self::release_family(release_id)?;
        let key = key?.trim();
        if key.is_empty() {
            return None;
        }

        let requested = match language {
            Some(l) if !l.trim().is_empty() => l.trim(),
            _ => "en",
        };

        if let Some(name) = self.lookup_in_language(family, kind, key, requested) {
            return Some(name);
        }
        if requested != "en" {
            if let Some(name) = self.lookup_in_language(family, kind, key, "en") {
                return Some(name);
            }
        }
        if requested != "ru" {
            if let Some(name) = self.lookup_in_language(family, kind, key, "ru") {
                return Some(name);
            }
        }

        None
    }

    fn lookup_in_language(&self, family: &str, kind: &str, key: &str, lang: &str) -> Option<String> {
        let releases = self.data.get("releases")?;
        let fam_val = releases.get(family)?;
        let kind_val = fam_val.get(kind)?;
        let entry_val = kind_val.get(key)?;
        let lang_clean = lang.replace('-', "_");

        let text = entry_val
            .get(lang)
            .or_else(|| entry_val.get(&lang_clean))
            .and_then(JsonValue::as_str)
            .filter(|s| !s.is_empty())?;

        Some(text.to_string())
    }

    /// Maps release ID to trilogy family ("soc", "clear_sky", "cop") or None for S2/unsupported.
    #[must_use]
    pub fn release_family(release_id: Option<&str>) -> Option<&'static str> {
        let value = release_id.unwrap_or("").trim().to_ascii_lowercase();
        if value.starts_with("stalker2") {
            return None;
        }
        if value.contains("cop") || value.contains("pripyat") || value.contains("prypiat") {
            return Some("cop");
        }
        if value.contains("-cs") || value.contains("clear") || value == "cs" {
            return Some("clear_sky");
        }
        if value.contains("soc") || value.contains("shadow") {
            return Some("soc");
        }
        match value.as_str() {
            "soc" => Some("soc"),
            "clear_sky" => Some("clear_sky"),
            "cop" => Some("cop"),
            _ => None,
        }
    }
}
