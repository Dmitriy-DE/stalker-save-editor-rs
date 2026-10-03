//! Builds catalogs from an installed game's LTX sections and string tables.

use crate::models::{
    CatalogBundle, FactionCatalog, FactionDefinition, FactionRelation, ItemCatalog, ItemDefinition, UpgradeCatalog,
    UpgradeDefinition,
};
use sse_content::{LtxDocument, LtxSection};
use sse_core::Result;
use std::collections::{HashMap, HashSet};

/// Catalog builder for installed X-Ray game files.
pub struct InstalledGameCatalogBuilder;

impl InstalledGameCatalogBuilder {
    /// Builds a catalog bundle from parsed LTX sections and string tables.
    pub fn build(
        release_id: &str,
        sections: &HashMap<String, LtxSection>,
        strings: &HashMap<String, String>,
    ) -> Result<Option<CatalogBundle>> {
        let resolved = LtxDocument::resolve(sections);
        let items = build_items(&resolved, strings)?;
        if items.is_empty() {
            return Ok(None);
        }

        let item_keys: HashSet<String> = items.iter().map(|item| item.key.clone()).collect();
        let item_catalog = ItemCatalog::new(release_id.to_string(), items)?;
        let upgrades = build_upgrades(release_id, &resolved, strings, &item_keys)?;
        let factions = build_factions(release_id, &resolved, strings)?;

        Ok(Some(CatalogBundle::new(
            release_id.to_string(),
            item_catalog,
            factions,
            upgrades,
        )?))
    }

    /// Determines the item category from section name and LTX values.
    #[must_use]
    pub fn category(name: &str, values: &HashMap<String, String>) -> Option<&'static str> {
        let class_name = values
            .get("class")
            .map(|s| s.trim().to_ascii_uppercase())
            .unwrap_or_default();
        let lowered = name.to_ascii_lowercase();

        if class_name == "AMMO" || lowered.starts_with("ammo_") {
            return Some("ammo");
        }
        if values.contains_key("weapon_class")
            || values.contains_key("ammo_class")
            || lowered.starts_with("wpn_")
            || lowered.starts_with("weapon_")
        {
            return Some("weapon");
        }
        if class_name.starts_with("G_") || starts_with_any(&lowered, &["grenade", "rgd", "f1_"]) {
            return Some("grenade");
        }
        if class_name == "DETECTOR" || class_name == "DEVICE" || starts_with_any(&lowered, &["device_", "detector_"]) {
            return Some("device");
        }
        if matches!(class_name.as_str(), "E_STLK" | "EQU_STLK" | "E_HLMET" | "EQU_HLMT")
            || lowered.ends_with("_outfit")
            || starts_with_any(&lowered, &["outfit_", "scientific_", "helm_", "armor_"])
        {
            return Some("outfit");
        }
        if matches!(class_name.as_str(), "ARTEFACT" | "SCRPTART")
            || class_name.starts_with("AF_")
            || starts_with_any(&lowered, &["af_", "artifact_"])
        {
            return Some("artifact");
        }
        if matches!(
            class_name.as_str(),
            "II_FOOD" | "S_FOOD" | "II_MEDKI" | "II_BANDG" | "II_ANTIR" | "II_BOTTL"
        ) || starts_with_any(
            &lowered,
            &[
                "medkit", "bandage", "antirad", "drug_", "food_", "bread", "kolbasa", "vodka", "energy",
            ],
        ) {
            return Some("consumable");
        }
        if values.contains_key("inv_name") || values.contains_key("inv_name_short") {
            return Some("item");
        }

        None
    }

    /// Determines the engine serialization family.
    #[must_use]
    pub fn serialization_family(name: &str, values: &HashMap<String, String>, category: Option<&str>) -> &'static str {
        let lowered = name.to_ascii_lowercase();
        let class_name = values
            .get("class")
            .map(|s| s.trim().to_ascii_uppercase())
            .unwrap_or_default();

        if category == Some("ammo") || class_name == "AMMO" {
            return "ammo";
        }
        if lowered == "device_torch" {
            return "torch";
        }
        if lowered == "device_pda" {
            return "pda";
        }
        if starts_with_any(&lowered, &["detector_", "device_detector"]) {
            return "detector";
        }
        if category == Some("weapon") || starts_with_any(&lowered, &["wpn_", "weapon_"]) {
            if class_name == "WP_KNIFE" || lowered.ends_with("_knife") {
                return "weapon";
            }
            if matches!(
                class_name.as_str(),
                "WP_BM16" | "WP_RG6" | "WP_SHOTG" | "WP_SPAS12" | "WP_TOZ34"
            ) {
                return "weapon_shotgun";
            }
            if matches!(class_name.as_str(), "WP_AK74" | "WP_FN2000" | "WP_GROZA") {
                return "weapon_wgl";
            }
            if class_name.starts_with("WP_") {
                return "weapon_magazined";
            }
        }
        if category == Some("outfit") || matches!(class_name.as_str(), "E_STLK" | "E_SCI" | "E_MILIT" | "E_EXO") {
            return "outfit";
        }
        if matches!(class_name.as_str(), "II_PDA" | "IITEM_PDA") {
            return "pda";
        }
        if matches!(class_name.as_str(), "II_DOCUMENT" | "IITEM_DOCUMENT") {
            return "document";
        }

        "base"
    }
}

fn build_items(
    resolved: &[(LtxSection, HashMap<String, String>)],
    strings: &HashMap<String, String>,
) -> Result<Vec<ItemDefinition>> {
    let mut items = Vec::new();
    for (section, values) in resolved {
        let name = &section.name;
        let cat = InstalledGameCatalogBuilder::category(name, values);
        if cat.is_none() || name.starts_with('$') || name.to_ascii_lowercase().ends_with("_hud") {
            continue;
        }

        let name_key = section
            .values
            .get("inv_name_short")
            .or_else(|| section.values.get("inv_name"))
            .or_else(|| values.get("inv_name_short"))
            .or_else(|| values.get("inv_name"))
            .filter(|s| !s.is_empty());

        let max_stack = parse_int(values.get("box_size").map(String::as_str))
            .or_else(|| parse_int(values.get("inv_max_count").map(String::as_str)));

        let display_name = name_key.and_then(|k| strings.get(k)).cloned();
        let unit_weight = parse_double(values.get("inv_weight").map(String::as_str)).filter(|&w| w >= 0.0);
        let width = parse_non_negative_u32(values.get("inv_grid_width").map(String::as_str));
        let height = parse_non_negative_u32(values.get("inv_grid_height").map(String::as_str));
        let max_stack_u32 = max_stack.and_then(|v| if v >= 0 { u32::try_from(v).ok() } else { None });
        let slots = parse_slots(values.get("inv_grid_slot").map(String::as_str));
        let source = format!("{}#{}", section.source, name);
        let serialization_family =
            Some(InstalledGameCatalogBuilder::serialization_family(name, values, cat).to_string());
        let icon_x = parse_non_negative_u32(values.get("inv_grid_x").map(String::as_str));
        let icon_y = parse_non_negative_u32(values.get("inv_grid_y").map(String::as_str));
        let icon_texture = values
            .get("icons_texture")
            .cloned()
            .or_else(|| Some("ui_icon_equipment".to_string()));
        let class_name = values.get("class").cloned();
        let cost = parse_non_negative_u32(values.get("cost").map(String::as_str));

        let def = ItemDefinition::new(
            name.clone(),
            display_name,
            cat.map(|s| s.to_string()),
            unit_weight,
            width,
            height,
            max_stack_u32,
            slots,
            source,
            serialization_family,
            icon_x,
            icon_y,
            icon_texture,
            class_name,
            name_key.cloned(),
            None,
            cost,
        )?;
        items.push(def);
    }

    Ok(items)
}

fn build_upgrades(
    release_id: &str,
    resolved: &[(LtxSection, HashMap<String, String>)],
    strings: &HashMap<String, String>,
    item_keys: &HashSet<String>,
) -> Result<Option<UpgradeCatalog>> {
    let mut aliases = Vec::new();
    for (section, _) in resolved {
        if section.name.eq_ignore_ascii_case("upgraded_inventory") {
            for entry in &section.entries {
                if !aliases.contains(entry) {
                    aliases.push(entry.clone());
                }
            }
        }
    }
    aliases.sort();

    let mut upgrades = Vec::new();
    for (section, values) in resolved {
        if !section.name.to_ascii_lowercase().starts_with("up_") || !values.contains_key("section") {
            continue;
        }

        let source_norm = section.source.replace('\\', "/").to_ascii_lowercase();
        let candidate = upgrade_item_key(&source_norm);
        let item_key = candidate
            .as_deref()
            .filter(|&c| item_keys.contains(c))
            .map(|s| s.to_string());

        let applicable: Vec<String> = match candidate {
            Some(ref cand) => {
                let prefix = format!("{cand}_");
                aliases
                    .iter()
                    .filter(|alias| *alias == cand || alias.starts_with(&prefix))
                    .cloned()
                    .collect()
            }
            None => Vec::new(),
        };

        let name_key = values.get("name").map(String::as_str);
        let display_name = name_key
            .and_then(|k| strings.get(k).map(|s| s.as_str()).or(Some(k)))
            .map(|s| s.to_string());

        let cat = if source_norm.contains("/weapons/upgrades/") {
            Some("weapon".to_string())
        } else if source_norm.contains("/outfit_upgrades/") {
            Some("outfit".to_string())
        } else {
            None
        };

        let def = UpgradeDefinition::new(
            section.name.clone(),
            display_name,
            cat,
            item_key,
            format!("{}#{}", section.source, section.name),
            release_id.to_string(),
            values.get("section").cloned(),
            values.get("property").cloned(),
            values.get("icon").cloned(),
            applicable,
        )?;
        upgrades.push(def);
    }

    if upgrades.is_empty() {
        Ok(None)
    } else {
        Ok(Some(UpgradeCatalog::new(release_id.to_string(), upgrades)?))
    }
}

fn build_factions(
    release_id: &str,
    resolved: &[(LtxSection, HashMap<String, String>)],
    strings: &HashMap<String, String>,
) -> Result<Option<FactionCatalog>> {
    let mut communities: Vec<(String, i32)> = Vec::new();
    let mut community_source = String::new();
    let mut game_relations: Option<&HashMap<String, String>> = None;
    let mut relation_rows: Option<&HashMap<String, String>> = None;
    let mut action_points: Option<&HashMap<String, String>> = None;

    for (section, values) in resolved {
        match section.name.to_ascii_lowercase().as_str() {
            "game_relations" => {
                if communities.is_empty() {
                    if let Some(comm_str) = values.get("communities") {
                        communities = parse_community_pairs(comm_str);
                        if !communities.is_empty() {
                            community_source = format!("{}#communities", section.source);
                        }
                    }
                }
                if game_relations.is_none() {
                    game_relations = Some(values);
                }
            }
            "communities_relations" if relation_rows.is_none() => {
                relation_rows = Some(values);
            }
            "action_points" if action_points.is_none() => {
                action_points = Some(values);
            }
            _ => {}
        }
    }

    if communities.is_empty() {
        return Ok(None);
    }

    let mut factions = Vec::with_capacity(communities.len());
    for (key, id) in &communities {
        let display_name = strings.get(key).cloned();
        let def = FactionDefinition::new(
            key.clone(),
            display_name,
            community_source.clone(),
            release_id.to_string(),
            Some(*id),
        )?;
        factions.push(def);
    }

    let mut relations = Vec::new();
    for (key, _) in &communities {
        let row = relation_rows
            .and_then(|r| r.get(key))
            .map(|s| parse_int_list(s))
            .unwrap_or_default();

        for (column, &val) in row.iter().enumerate() {
            if column < communities.len() {
                if let Some(target) = communities.get(column) {
                    relations.push(FactionRelation {
                        source: key.clone(),
                        target: target.0.clone(),
                        value: val,
                    });
                }
            }
        }
    }

    let limits = action_points
        .and_then(|ap| ap.get("community_goodwill_limits"))
        .map(|s| parse_int_list(s))
        .unwrap_or_default();

    let goodwill_min = if limits.len() >= 2 {
        limits.first().copied()
    } else {
        None
    };
    let goodwill_max = if limits.len() >= 2 {
        limits.get(1).copied()
    } else {
        None
    };
    let attitude_neutral = game_relations
        .and_then(|gr| {
            gr.get("attitude_neutal_threshold")
                .or_else(|| gr.get("attitude_neutral_threshold"))
        })
        .and_then(|s| parse_int(Some(s)));
    let attitude_friend = game_relations
        .and_then(|gr| gr.get("attitude_friend_threshold"))
        .and_then(|s| parse_int(Some(s)));

    Ok(Some(FactionCatalog::new(
        release_id.to_string(),
        factions,
        relations,
        goodwill_min,
        goodwill_max,
        attitude_neutral,
        attitude_friend,
    )?))
}

fn upgrade_item_key(source: &str) -> Option<String> {
    let file_name = match source.rfind('/') {
        Some(idx) => source.get(idx.saturating_add(1)..).unwrap_or(""),
        None => source,
    };
    let stem = file_name.strip_suffix("_up.ltx")?;

    if source.contains("/weapons/upgrades/") && stem.starts_with("w_") {
        if stem.len() > 2 {
            return stem.get(2..).map(|rest| format!("wpn_{rest}"));
        }
        return None;
    }
    if source.contains("/outfit_upgrades/") {
        if stem.starts_with("o_") && stem.len() > 2 {
            return stem.get(2..).map(|rest| rest.to_string());
        }
        if !stem.is_empty() {
            return Some(stem.to_string());
        }
    }
    None
}

fn starts_with_any(val: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| val.starts_with(prefix))
}

fn parse_int(val: Option<&str>) -> Option<i32> {
    let text = val?.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(hex_part) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        return i32::from_str_radix(hex_part, 16).ok();
    }
    if let Ok(num) = text.parse::<i32>() {
        return Some(num);
    }
    let normalized = text.replace(',', ".");
    if let Some(int_part) = normalized.split('.').next() {
        if let Ok(num) = int_part.trim().parse::<i32>() {
            return Some(num);
        }
    }
    None
}

fn parse_non_negative_u32(val: Option<&str>) -> Option<u32> {
    let int_val = parse_int(val)?;
    if int_val >= 0 {
        u32::try_from(int_val).ok()
    } else {
        None
    }
}

fn parse_double(val: Option<&str>) -> Option<f64> {
    let text = val?.trim().replace(',', ".");
    let parsed = text.parse::<f64>().ok()?;
    if parsed.is_finite() {
        Some(parsed)
    } else {
        None
    }
}

fn parse_slots(val: Option<&str>) -> Vec<String> {
    val.map(|s| {
        s.split([',', ';', ' '])
            .map(|tok| tok.trim().to_string())
            .filter(|tok| !tok.is_empty())
            .collect()
    })
    .unwrap_or_default()
}

fn parse_community_pairs(val: &str) -> Vec<(String, i32)> {
    let tokens: Vec<&str> = val.split(',').map(str::trim).filter(|t| !t.is_empty()).collect();
    if tokens.len() < 2 || tokens.len() % 2 != 0 {
        return Vec::new();
    }
    let mut pairs = Vec::with_capacity(tokens.len() / 2);
    let mut i = 0;
    while i < tokens.len() {
        if let (Some(&name), Some(&id_str)) = (tokens.get(i), tokens.get(i.saturating_add(1))) {
            if let Some(id) = parse_int(Some(id_str)) {
                if id >= 0 {
                    pairs.push((name.to_string(), id));
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            }
        }
        i = i.saturating_add(2);
    }
    pairs
}

fn parse_int_list(val: &str) -> Vec<i32> {
    let mut result = Vec::new();
    for tok in val.split(',') {
        match parse_int(Some(tok)) {
            Some(n) => result.push(n),
            None => return Vec::new(),
        }
    }
    result
}
