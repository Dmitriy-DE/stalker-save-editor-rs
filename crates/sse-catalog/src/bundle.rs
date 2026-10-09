//! Catalog bundle reader and writer.
//!
//! Loads bundled JSON release catalogs and parses/formats dynamic bundles
//! using standard library JSON.

use sse_core::{Error, Result};
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::models::{
    CatalogBundle, FactionCatalog, FactionDefinition, FactionRelation, ItemCatalog, ItemDefinition, UpgradeCatalog,
    UpgradeDefinition,
};
use crate::value::{parse_json, JsonValue};
use sse_codecs::embedded_json::{self, JsonAssetCache};

const EMBEDDED_CATALOGS_RAW: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/data_catalogs.json.deflate"));

static EMBEDDED_BUNDLES: OnceLock<HashMap<String, CatalogBundle>> = OnceLock::new();
static EMBEDDED_CATALOGS_JSON: JsonAssetCache = OnceLock::new();

/// Reader for bundled JSON catalogs and dynamic bundles.
pub struct CatalogBundleReader;

impl CatalogBundleReader {
    /// Loads all embedded catalogs.
    #[must_use]
    pub fn load_embedded() -> &'static HashMap<String, CatalogBundle> {
        EMBEDDED_BUNDLES.get_or_init(|| {
            embedded_json::get_json(EMBEDDED_CATALOGS_RAW, &EMBEDDED_CATALOGS_JSON)
                .and_then(Self::load)
                .unwrap_or_default()
        })
    }

    /// Loads all catalog bundles from a JSON payload.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on malformed payload or unsupported version.
    pub fn load(payload: &[u8]) -> Result<HashMap<String, CatalogBundle>> {
        let json_str =
            std::str::from_utf8(payload).map_err(|e| Error::damaged(format!("Invalid UTF-8 in bundle: {e}")))?;
        let root = parse_json(json_str)?;

        let version = root
            .get("schema_version")
            .and_then(JsonValue::as_u64)
            .ok_or_else(|| Error::damaged("Missing schema_version"))?;
        if version != 1 {
            return Err(Error::damaged(format!("Unsupported schema_version {version}")));
        }

        let releases_obj = root
            .get("releases")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| Error::damaged("Missing releases object"))?;

        let mut bundles = HashMap::new();
        for (rel_id, rel_val) in releases_obj {
            if rel_id != "stalker-cop" && rel_id != "stalker-cs" && rel_id != "stalker-soc" {
                return Err(Error::damaged(format!("Unknown release id '{rel_id}' in bundle")));
            }

            let items_arr = rel_val
                .get("items")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| Error::damaged(format!("Missing items array for release '{rel_id}'")))?;

            let mut items = Vec::with_capacity(items_arr.len());
            for item_val in items_arr {
                let key = item_val
                    .get("key")
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| Error::damaged("Item entry missing 'key'"))?;

                let slots = item_val
                    .get("slots")
                    .and_then(JsonValue::as_array)
                    .map(|arr| arr.iter().filter_map(JsonValue::as_str).map(String::from).collect())
                    .unwrap_or_default();

                let item_def = ItemDefinition::new(
                    key.to_string(),
                    item_val
                        .get("display_name")
                        .and_then(JsonValue::as_str)
                        .map(String::from),
                    item_val.get("category").and_then(JsonValue::as_str).map(String::from),
                    item_val.get("unit_weight").and_then(JsonValue::as_f64),
                    item_val
                        .get("width")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    item_val
                        .get("height")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    item_val
                        .get("max_stack")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    slots,
                    item_val
                        .get("source")
                        .and_then(JsonValue::as_str)
                        .filter(|s| !s.trim().is_empty())
                        .unwrap_or("generated-official-metadata")
                        .to_string(),
                    item_val
                        .get("serialization_family")
                        .and_then(JsonValue::as_str)
                        .map(String::from),
                    item_val
                        .get("icon_x")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    item_val
                        .get("icon_y")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                    item_val
                        .get("icon_texture")
                        .and_then(JsonValue::as_str)
                        .map(String::from),
                    item_val.get("class_name").and_then(JsonValue::as_str).map(String::from),
                    item_val
                        .get("display_name_key")
                        .and_then(JsonValue::as_str)
                        .map(String::from),
                    None,
                    item_val
                        .get("cost")
                        .and_then(JsonValue::as_u64)
                        .and_then(|n| u32::try_from(n).ok()),
                )?;
                items.push(item_def);
            }
            let item_catalog = ItemCatalog::new(rel_id.clone(), items)?;

            let factions_catalog = if let Some(f_arr) = rel_val.get("factions").and_then(JsonValue::as_array) {
                let mut f_list = Vec::with_capacity(f_arr.len());
                for f_val in f_arr {
                    let key = f_val
                        .get("key")
                        .and_then(JsonValue::as_str)
                        .ok_or_else(|| Error::damaged("Faction missing 'key'"))?;
                    let r_id = f_val.get("release_id").and_then(JsonValue::as_str).unwrap_or(rel_id);
                    if r_id != rel_id {
                        return Err(Error::damaged(format!(
                            "Faction '{key}' belongs to '{r_id}' but placed in '{rel_id}'"
                        )));
                    }
                    let f_def = FactionDefinition::new(
                        key.to_string(),
                        f_val.get("display_name").and_then(JsonValue::as_str).map(String::from),
                        f_val
                            .get("source")
                            .and_then(JsonValue::as_str)
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or("generated-official-metadata")
                            .to_string(),
                        rel_id.clone(),
                        f_val
                            .get("numeric_id")
                            .and_then(JsonValue::as_i64)
                            .and_then(|n| i32::try_from(n).ok()),
                    )?;
                    f_list.push(f_def);
                }

                let mut rel_list = Vec::new();
                if let Some(r_arr) = rel_val.get("relation_addresses").and_then(JsonValue::as_array) {
                    for r_val in r_arr {
                        let source = r_val
                            .get("source")
                            .and_then(JsonValue::as_str)
                            .ok_or_else(|| Error::damaged("Missing relation source"))?
                            .to_string();
                        let target = r_val
                            .get("target")
                            .and_then(JsonValue::as_str)
                            .ok_or_else(|| Error::damaged("Missing relation target"))?
                            .to_string();
                        let value = r_val
                            .get("value")
                            .and_then(JsonValue::as_i64)
                            .and_then(|n| i32::try_from(n).ok())
                            .unwrap_or(0);
                        rel_list.push(FactionRelation { source, target, value });
                    }
                }

                let gw_min = rel_val
                    .get("goodwill_min")
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| i32::try_from(n).ok());
                let gw_max = rel_val
                    .get("goodwill_max")
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| i32::try_from(n).ok());
                let neutral = rel_val
                    .get("attitude_neutral_threshold")
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| i32::try_from(n).ok());
                let friend = rel_val
                    .get("attitude_friend_threshold")
                    .and_then(JsonValue::as_i64)
                    .and_then(|n| i32::try_from(n).ok());

                Some(FactionCatalog::new(
                    rel_id.clone(),
                    f_list,
                    rel_list,
                    gw_min,
                    gw_max,
                    neutral,
                    friend,
                )?)
            } else {
                None
            };

            let upgrades_catalog = if let Some(u_arr) = rel_val.get("upgrades").and_then(JsonValue::as_array) {
                let mut u_list = Vec::with_capacity(u_arr.len());
                for u_val in u_arr {
                    let key = u_val
                        .get("key")
                        .and_then(JsonValue::as_str)
                        .ok_or_else(|| Error::damaged("Upgrade missing 'key'"))?;
                    let r_id = u_val.get("release_id").and_then(JsonValue::as_str).unwrap_or(rel_id);
                    if r_id != rel_id {
                        return Err(Error::damaged(format!(
                            "Upgrade '{key}' belongs to '{r_id}' but placed in '{rel_id}'"
                        )));
                    }
                    let applicable = u_val
                        .get("applicable_item_keys")
                        .and_then(JsonValue::as_array)
                        .map(|arr| arr.iter().filter_map(JsonValue::as_str).map(String::from).collect())
                        .unwrap_or_default();

                    let u_def = UpgradeDefinition::new(
                        key.to_string(),
                        u_val.get("display_name").and_then(JsonValue::as_str).map(String::from),
                        u_val.get("category").and_then(JsonValue::as_str).map(String::from),
                        u_val.get("item_key").and_then(JsonValue::as_str).map(String::from),
                        u_val
                            .get("source")
                            .and_then(JsonValue::as_str)
                            .filter(|s| !s.trim().is_empty())
                            .unwrap_or("generated-official-metadata")
                            .to_string(),
                        rel_id.clone(),
                        u_val.get("section").and_then(JsonValue::as_str).map(String::from),
                        u_val.get("property").and_then(JsonValue::as_str).map(String::from),
                        u_val.get("icon").and_then(JsonValue::as_str).map(String::from),
                        applicable,
                    )?;
                    u_list.push(u_def);
                }
                Some(UpgradeCatalog::new(rel_id.clone(), u_list)?)
            } else {
                None
            };

            let bundle = CatalogBundle::new(rel_id.clone(), item_catalog, factions_catalog, upgrades_catalog)?;
            bundles.insert(rel_id.clone(), bundle);
        }

        borrow_series_names(bundles)
    }
}

/// Serializer for catalog bundles into JSON format.
pub struct CatalogBundleWriter;

impl CatalogBundleWriter {
    /// Serializes game catalogs into a JSON byte vector.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] on serialization error.
    pub fn write(catalogs: &[&CatalogBundle]) -> Result<Vec<u8>> {
        let mut releases_entries = Vec::new();

        for cat in catalogs {
            let mut bundle_entries = Vec::new();

            // Items
            let mut items_arr = Vec::new();
            for item in cat.items.items() {
                let mut item_obj = Vec::new();
                item_obj.push(("key".to_string(), JsonValue::String(item.key.clone())));
                if let Some(dn) = &item.display_name {
                    item_obj.push(("display_name".to_string(), JsonValue::String(dn.clone())));
                }
                if let Some(c) = &item.category {
                    item_obj.push(("category".to_string(), JsonValue::String(c.clone())));
                }
                if let Some(w) = item.unit_weight {
                    item_obj.push(("unit_weight".to_string(), JsonValue::Number(w)));
                }
                if let Some(c) = item.cost {
                    item_obj.push(("cost".to_string(), JsonValue::Number(f64::from(c))));
                }
                if let Some(w) = item.width {
                    item_obj.push(("width".to_string(), JsonValue::Number(f64::from(w))));
                }
                if let Some(h) = item.height {
                    item_obj.push(("height".to_string(), JsonValue::Number(f64::from(h))));
                }
                if let Some(ms) = item.max_stack {
                    item_obj.push(("max_stack".to_string(), JsonValue::Number(f64::from(ms))));
                }
                let slots_arr: Vec<JsonValue> = item.slots.iter().map(|s| JsonValue::String(s.clone())).collect();
                item_obj.push(("slots".to_string(), JsonValue::Array(slots_arr)));
                let source = if item.source.trim().is_empty() {
                    "generated-official-metadata".to_string()
                } else {
                    item.source.clone()
                };
                item_obj.push(("source".to_string(), JsonValue::String(source)));
                if let Some(fam) = &item.serialization_family {
                    item_obj.push(("serialization_family".to_string(), JsonValue::String(fam.clone())));
                }
                if let Some(x) = item.icon_x {
                    item_obj.push(("icon_x".to_string(), JsonValue::Number(f64::from(x))));
                }
                if let Some(y) = item.icon_y {
                    item_obj.push(("icon_y".to_string(), JsonValue::Number(f64::from(y))));
                }
                if let Some(tex) = &item.icon_texture {
                    item_obj.push(("icon_texture".to_string(), JsonValue::String(tex.clone())));
                }
                if let Some(cls) = &item.class_name {
                    item_obj.push(("class_name".to_string(), JsonValue::String(cls.clone())));
                }
                if let Some(dnk) = &item.display_name_key {
                    item_obj.push(("display_name_key".to_string(), JsonValue::String(dnk.clone())));
                }
                items_arr.push(JsonValue::Object(item_obj));
            }
            bundle_entries.push(("items".to_string(), JsonValue::Array(items_arr)));

            // Factions
            if let Some(factions) = &cat.factions {
                let mut f_arr = Vec::new();
                for f in factions.factions() {
                    let mut f_obj = Vec::new();
                    f_obj.push(("key".to_string(), JsonValue::String(f.key.clone())));
                    if let Some(dn) = &f.display_name {
                        f_obj.push(("display_name".to_string(), JsonValue::String(dn.clone())));
                    }
                    f_obj.push(("release_id".to_string(), JsonValue::String(cat.release_id.clone())));
                    let source = if f.source.trim().is_empty() {
                        "generated-official-metadata".to_string()
                    } else {
                        f.source.clone()
                    };
                    f_obj.push(("source".to_string(), JsonValue::String(source)));
                    if let Some(num_id) = f.numeric_id {
                        f_obj.push(("numeric_id".to_string(), JsonValue::Number(f64::from(num_id))));
                    }
                    f_arr.push(JsonValue::Object(f_obj));
                }
                bundle_entries.push(("factions".to_string(), JsonValue::Array(f_arr)));

                if let Ok(addrs) = factions.relation_addresses() {
                    let mut rel_arr = Vec::new();
                    for r in addrs {
                        let r_obj = vec![
                            ("source".to_string(), JsonValue::String(r.source)),
                            ("target".to_string(), JsonValue::String(r.target)),
                            ("row".to_string(), JsonValue::Number(f64::from(r.row))),
                            ("column".to_string(), JsonValue::Number(f64::from(r.column))),
                            ("value".to_string(), JsonValue::Number(f64::from(r.value))),
                        ];
                        rel_arr.push(JsonValue::Object(r_obj));
                    }
                    bundle_entries.push(("relation_addresses".to_string(), JsonValue::Array(rel_arr)));
                }

                if let Some(min) = factions.goodwill_min() {
                    bundle_entries.push(("goodwill_min".to_string(), JsonValue::Number(f64::from(min))));
                }
                if let Some(max) = factions.goodwill_max() {
                    bundle_entries.push(("goodwill_max".to_string(), JsonValue::Number(f64::from(max))));
                }
                if let Some(neutral) = factions.attitude_neutral_threshold() {
                    bundle_entries.push((
                        "attitude_neutral_threshold".to_string(),
                        JsonValue::Number(f64::from(neutral)),
                    ));
                }
                if let Some(friend) = factions.attitude_friend_threshold() {
                    bundle_entries.push((
                        "attitude_friend_threshold".to_string(),
                        JsonValue::Number(f64::from(friend)),
                    ));
                }
            }

            // Upgrades
            if let Some(upgrades) = &cat.upgrades {
                let mut u_arr = Vec::new();
                for u in upgrades.upgrades() {
                    let mut u_obj = Vec::new();
                    u_obj.push(("key".to_string(), JsonValue::String(u.key.clone())));
                    if let Some(dn) = &u.display_name {
                        u_obj.push(("display_name".to_string(), JsonValue::String(dn.clone())));
                    }
                    if let Some(c) = &u.category {
                        u_obj.push(("category".to_string(), JsonValue::String(c.clone())));
                    }
                    if let Some(ik) = &u.item_key {
                        u_obj.push(("item_key".to_string(), JsonValue::String(ik.clone())));
                    }
                    u_obj.push(("release_id".to_string(), JsonValue::String(cat.release_id.clone())));
                    let source = if u.source.trim().is_empty() {
                        "generated-official-metadata".to_string()
                    } else {
                        u.source.clone()
                    };
                    u_obj.push(("source".to_string(), JsonValue::String(source)));
                    if let Some(sec) = &u.section {
                        u_obj.push(("section".to_string(), JsonValue::String(sec.clone())));
                    }
                    if let Some(prop) = &u.property_name {
                        u_obj.push(("property".to_string(), JsonValue::String(prop.clone())));
                    }
                    if let Some(icon) = &u.icon {
                        u_obj.push(("icon".to_string(), JsonValue::String(icon.clone())));
                    }
                    let app_arr: Vec<JsonValue> = u
                        .applicable_item_keys
                        .iter()
                        .map(|k| JsonValue::String(k.clone()))
                        .collect();
                    u_obj.push(("applicable_item_keys".to_string(), JsonValue::Array(app_arr)));
                    u_arr.push(JsonValue::Object(u_obj));
                }
                bundle_entries.push(("upgrades".to_string(), JsonValue::Array(u_arr)));
            }

            releases_entries.push((cat.release_id.clone(), JsonValue::Object(bundle_entries)));
        }

        let root_entries = vec![
            ("schema_version".to_string(), JsonValue::Number(1.0)),
            ("releases".to_string(), JsonValue::Object(releases_entries)),
        ];

        let root = JsonValue::Object(root_entries);
        let serialized = root.to_string();
        Ok(serialized.into_bytes())
    }
}

fn is_original_trilogy(release_id: &str) -> bool {
    matches!(release_id, "stalker-soc" | "stalker-cs" | "stalker-cop")
}

fn is_russian(value: Option<&str>) -> bool {
    value.is_some_and(|text| text.chars().any(|c| matches!(c, 'А'..='Я' | 'а'..='я' | 'Ё' | 'ё')))
}

fn borrow_series_names(mut loaded: HashMap<String, CatalogBundle>) -> Result<HashMap<String, CatalogBundle>> {
    let trilogy_ids: Vec<String> = loaded.keys().filter(|k| is_original_trilogy(k)).cloned().collect();

    for release_id in &trilogy_ids {
        let mut item_replacements = Vec::new();
        if let Some(bundle) = loaded.get(release_id) {
            for (idx, item) in bundle.items.items().iter().enumerate() {
                let keep = is_russian(item.display_name.as_deref())
                    || item.key.to_ascii_lowercase().starts_with("wpn_")
                    || item.key.to_ascii_lowercase().starts_with("helm_")
                    || item.key.to_ascii_lowercase().starts_with("mp_")
                    || item.key.to_ascii_lowercase().ends_with("_outfit")
                    || item
                        .serialization_family
                        .as_ref()
                        .is_some_and(|fam| fam.starts_with("weapon") || fam.starts_with("outfit"));

                if !keep {
                    let donor_name = trilogy_ids
                        .iter()
                        .filter(|other| *other != release_id)
                        .find_map(|other| {
                            loaded
                                .get(other)?
                                .items
                                .resolve(&item.key)
                                .and_then(|d| d.display_name.as_deref())
                                .filter(|name| is_russian(Some(name)))
                        });
                    if let Some(dn) = donor_name {
                        item_replacements.push((idx, dn.to_string()));
                    }
                }
            }
        }

        if let Some(bundle) = loaded.get_mut(release_id) {
            for (idx, name) in item_replacements {
                if let Some(item) = bundle.items.items_mut().get_mut(idx) {
                    item.display_name = Some(name);
                }
            }
        }

        let mut faction_replacements = Vec::new();
        if let Some(bundle) = loaded.get(release_id) {
            if let Some(ref factions) = bundle.factions {
                for (idx, faction) in factions.factions().iter().enumerate() {
                    if faction.key != "actor" && !is_russian(faction.display_name.as_deref()) {
                        let donor_name = trilogy_ids
                            .iter()
                            .filter(|other| *other != release_id)
                            .find_map(|other| {
                                loaded
                                    .get(other)?
                                    .factions
                                    .as_ref()?
                                    .resolve(&faction.key)
                                    .ok()?
                                    .display_name
                                    .as_deref()
                                    .filter(|name| is_russian(Some(name)))
                            });
                        if let Some(dn) = donor_name {
                            faction_replacements.push((idx, dn.to_string()));
                        }
                    }
                }
            }
        }

        if let Some(bundle) = loaded.get_mut(release_id) {
            if let Some(ref mut factions) = bundle.factions {
                for (idx, name) in faction_replacements {
                    if let Some(faction) = factions.factions_mut().get_mut(idx) {
                        faction.display_name = Some(name);
                    }
                }
            }
        }
    }

    Ok(loaded)
}
