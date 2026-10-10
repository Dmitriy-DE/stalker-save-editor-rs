//! Shipped game fixes catalogue.
//!
//! Embedded from `game-fixes.json` and parsed into memory. Enhanced Edition
//! variants are derived on initialization matching the reference implementation.

use std::collections::HashMap;
use std::sync::OnceLock;

use sse_catalog::{parse_json, JsonValue};
use sse_codecs::embedded_json::{self, JsonAssetCache};
use sse_core::{Error, Result};

use crate::models::{
    FileOverlayOperation, GameFixCategory, GameFixDefinition, GameFixImplementationType, GameFixMaturity,
    GameFixPreset, GameFixSaveCompatibility, GameFixVerificationState, GameTarget, SpawnEditKind, SpawnEditOperation,
    TextPatchOperation,
};

const CATALOG_DATA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/data_game-fixes.json.deflate"));
static ALL_FIXES: OnceLock<std::result::Result<Vec<GameFixDefinition>, String>> = OnceLock::new();
static CATALOG_DATA_JSON: JsonAssetCache = OnceLock::new();

/// Shipped game fixes catalogue.
pub struct GameFixCatalog;

impl GameFixCatalog {
    /// Current dataset version.
    pub const DATASET_VERSION: &'static str = "2026.10.1";
    /// Previous dataset version for migration checks.
    pub const PREVIOUS_DATASET_VERSION: &'static str = "2026.09.2";

    /// Returns all catalogue definitions, including generated Enhanced Edition variants.
    #[must_use]
    pub fn all() -> &'static [GameFixDefinition] {
        match ALL_FIXES.get_or_init(|| load_catalog().map_err(|e| e.to_string())) {
            Ok(fixes) => fixes.as_slice(),
            Err(_) => &[],
        }
    }

    /// Returns the reason the embedded catalog failed to load, or `None` when it loaded.
    ///
    /// When this is `Some`, [`GameFixCatalog::all`] is empty; callers that show the catalog should report this text.
    #[must_use]
    pub fn load_error() -> Option<&'static str> {
        ALL_FIXES
            .get_or_init(|| load_catalog().map_err(|e| e.to_string()))
            .as_ref()
            .err()
            .map(String::as_str)
    }

    /// Returns all fixes available for a game.
    #[must_use]
    pub fn for_game(game: GameTarget) -> Vec<&'static GameFixDefinition> {
        Self::all().iter().filter(|d| d.game == game).collect()
    }

    /// Looks up a fix by its identifier (e.g. `"cs.quest.dead-wild-napr"` or `"cs.quest.dead-wild-napr.ee"`).
    #[must_use]
    pub fn try_get(id: &str) -> Option<&'static GameFixDefinition> {
        Self::all().iter().find(|d| d.id == id)
    }

    /// Returns fixes matching a preset for a game.
    #[must_use]
    pub fn for_preset(game: GameTarget, preset: GameFixPreset) -> Vec<&'static GameFixDefinition> {
        if preset == GameFixPreset::Custom {
            return Vec::new();
        }
        Self::all()
            .iter()
            .filter(|d| d.game == game && Self::is_included_in_preset(d, preset))
            .collect()
    }

    /// Counts fixes in each category for a game.
    #[must_use]
    pub fn category_counts(game: GameTarget) -> HashMap<GameFixCategory, usize> {
        let mut counts = HashMap::new();
        for cat in GameFixCategory::ALL {
            counts.insert(cat, 0);
        }
        for fix in Self::for_game(game) {
            let entry: &mut usize = counts.entry(fix.category).or_insert(0);
            *entry = (*entry).saturating_add(1);
        }
        counts
    }

    /// Previous release's recommended preset counts for upgrade delta checks.
    #[must_use]
    pub const fn previous_preset_count(game: GameTarget, preset: GameFixPreset) -> usize {
        if matches!(preset, GameFixPreset::Recommended) {
            match game {
                GameTarget::ClearSky => 23,
                GameTarget::ShadowOfChernobyl => 15,
                GameTarget::CallOfPripyat => 10,
                _ => 0,
            }
        } else {
            0
        }
    }

    /// Checks if a definition qualifies for inclusion in a preset.
    #[must_use]
    pub fn is_included_in_preset(definition: &GameFixDefinition, preset: GameFixPreset) -> bool {
        if preset == GameFixPreset::Custom {
            return false;
        }
        if definition.maturity != GameFixMaturity::Validated || definition.category == GameFixCategory::Experimental {
            return false;
        }
        match preset {
            GameFixPreset::EssentialOnly => definition.category == GameFixCategory::Essential,
            GameFixPreset::Recommended => matches!(
                definition.category,
                GameFixCategory::Essential | GameFixCategory::Recommended
            ),
            GameFixPreset::AllSafeFixes => matches!(
                definition.category,
                GameFixCategory::Essential | GameFixCategory::Recommended | GameFixCategory::Community
            ),
            GameFixPreset::Custom => false,
        }
    }
}

fn enhanced_edition_of(game: GameTarget) -> Option<GameTarget> {
    match game {
        GameTarget::ShadowOfChernobyl => Some(GameTarget::ShadowOfChernobylEnhancedEdition),
        GameTarget::ClearSky => Some(GameTarget::ClearSkyEnhancedEdition),
        GameTarget::CallOfPripyat => Some(GameTarget::CallOfPripyatEnhancedEdition),
        _ => None,
    }
}

fn enhanced_edition_build(game: GameTarget) -> &'static str {
    match game {
        GameTarget::ShadowOfChernobyl => "24067120",
        GameTarget::ClearSky => "24067129",
        GameTarget::CallOfPripyat => "24067133",
        _ => "",
    }
}

fn load_catalog() -> Result<Vec<GameFixDefinition>> {
    let json_bytes = embedded_json::get_json(CATALOG_DATA, &CATALOG_DATA_JSON)?;
    let json_text = std::str::from_utf8(json_bytes).map_err(|_| Error::damaged("Invalid catalog JSON"))?;
    let parsed = parse_json(json_text).map_err(|_| Error::damaged("Invalid catalog JSON"))?;
    let root = parsed
        .as_object()
        .ok_or_else(|| Error::damaged("Catalog JSON root is not an object"))?;

    let defs_json = root
        .iter()
        .find(|(k, _)| k.as_str() == "definitions")
        .and_then(|(_, v)| v.as_array())
        .ok_or_else(|| Error::damaged("Catalog definitions missing"))?;

    let ee_hashes_json = root
        .iter()
        .find(|(k, _)| k.as_str() == "enhancedEditionSha256")
        .and_then(|(_, v)| v.as_object())
        .ok_or_else(|| Error::damaged("Catalog enhancedEditionSha256 missing"))?;

    let mut ee_sha256_map: HashMap<&str, Vec<&str>> = HashMap::new();
    for (k, v) in ee_hashes_json {
        if let Some(arr) = v.as_array() {
            let hashes: Vec<&str> = arr.iter().filter_map(|h| h.as_str()).collect();
            ee_sha256_map.insert(k.as_str(), hashes);
        }
    }

    let mut definitions = Vec::with_capacity(defs_json.len());
    for item in defs_json {
        let def = parse_definition(item)?;
        definitions.push(def);
    }

    // Verify uniqueness of IDs
    let mut ids = std::collections::HashSet::new();
    for def in &definitions {
        if !ids.insert(&def.id) {
            return Err(Error::damaged(format!("Duplicate fix ID: {}", def.id)));
        }
    }

    // Generate Enhanced Edition variants
    let mut ee_variants = Vec::new();
    for fix in &definitions {
        let Some(hashes) = ee_sha256_map.get(fix.id.as_str()) else {
            continue;
        };

        let shared: Vec<_> = fix.text_patches.iter().filter(|p| !p.retail_only).cloned().collect();

        let expected_hash_count = shared.len().checked_add(fix.spawn_edits.len()).unwrap_or(0);
        if hashes.len() != expected_hash_count {
            return Err(Error::damaged(format!("EE hash count mismatch for {}", fix.id)));
        }

        let Some(ee_game) = enhanced_edition_of(fix.game) else {
            return Err(Error::damaged(format!("No EE target for {:?}", fix.game)));
        };
        let ee_build = enhanced_edition_build(fix.game);

        let mut ee_text_patches = Vec::with_capacity(shared.len());
        for (i, mut patch) in shared.into_iter().enumerate() {
            if let Some(h) = hashes.get(i) {
                patch.expected_file_sha256 = Some((*h).to_string());
            }
            ee_text_patches.push(patch);
        }

        let mut ee_spawn_edits = Vec::with_capacity(fix.spawn_edits.len());
        for (i, mut edit) in fix.spawn_edits.clone().into_iter().enumerate() {
            let hash_idx = ee_text_patches.len().saturating_add(i);
            if let Some(h) = hashes.get(hash_idx) {
                edit.expected_file_sha256 = Some((*h).to_string());
            }
            ee_spawn_edits.push(edit);
        }

        let ee_fix = GameFixDefinition {
            id: format!("{}.ee", fix.id),
            game: ee_game,
            version: fix.version.clone(),
            title: fix.title.clone(),
            supported_steam_build_ids: vec![ee_build.to_string()],
            category: fix.category,
            maturity: fix.maturity,
            depends_on: fix.depends_on.iter().map(|id| format!("{id}.ee")).collect(),
            conflicts_with: fix
                .conflicts_with
                .iter()
                .map(|id| format!("{id}.ee"))
                .collect(),
            text_patches: ee_text_patches,
            source: format!("{} Enhanced Edition variant: same anchors, EE file hashes.", fix.source),
            problem: fix.problem.clone(),
            description: fix.description.clone(),
            implementation: fix.implementation,
            requires_new_game: fix.requires_new_game,
            save_compatibility: fix.save_compatibility,
            verification_state: fix.verification_state,
            detection_method: format!(
                "Steam build {ee_build}, exact archived EE source-file SHA-256 per operation, and unique exact text anchors."
            ),
            references: fix.references.clone(),
            overlays: fix.overlays.clone(),
            spawn_edits: ee_spawn_edits,
        };

        ee_variants.push(ee_fix);
    }

    definitions.extend(ee_variants);
    Ok(definitions)
}

fn parse_definition(val: &JsonValue) -> Result<GameFixDefinition> {
    let obj = val
        .as_object()
        .ok_or_else(|| Error::damaged("Fix definition is not an object"))?;

    let get_str = |key: &str| -> Result<String> {
        obj.iter()
            .find(|(k, _)| k.as_str() == key)
            .and_then(|(_, v)| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| Error::damaged(format!("Missing string field '{key}'")))
    };

    let get_str_opt = |key: &str| -> Option<String> {
        obj.iter()
            .find(|(k, _)| k.as_str() == key)
            .and_then(|(_, v)| v.as_str())
            .map(|s| s.to_string())
    };

    let get_bool_opt = |key: &str| -> bool {
        obj.iter()
            .find(|(k, _)| k.as_str() == key)
            .and_then(|(_, v)| v.as_bool())
            .unwrap_or(false)
    };

    let get_str_list = |key: &str| -> Vec<String> {
        obj.iter()
            .find(|(k, _)| k.as_str() == key)
            .and_then(|(_, v)| v.as_array())
            .map(|arr| arr.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect())
            .unwrap_or_default()
    };

    let id = get_str("id")?;
    let game_str = get_str("game")?;
    let game =
        GameTarget::parse(&game_str).ok_or_else(|| Error::damaged(format!("Unknown game target '{game_str}'")))?;
    let version = get_str("version")?;
    let title = get_str("title")?;
    let supported_steam_build_ids = get_str_list("supportedSteamBuildIds");
    let category_str = get_str("category")?;
    let category = GameFixCategory::parse(&category_str)
        .ok_or_else(|| Error::damaged(format!("Unknown category '{category_str}'")))?;
    let maturity_str = get_str("maturity")?;
    let maturity = GameFixMaturity::parse(&maturity_str)
        .ok_or_else(|| Error::damaged(format!("Unknown maturity '{maturity_str}'")))?;
    let depends_on = get_str_list("dependsOn");
    let conflicts_with = get_str_list("conflictsWith");
    let source = get_str("source")?;
    let problem = get_str_opt("problem").unwrap_or_default();
    let description = get_str_opt("description").unwrap_or_default();

    let implementation = obj
        .iter()
        .find(|(k, _)| k.as_str() == "implementation")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixImplementationType::parse)
        .unwrap_or(GameFixImplementationType::ExactTextReplacement);

    let requires_new_game = get_bool_opt("requiresNewGame");

    let save_compatibility = obj
        .iter()
        .find(|(k, _)| k.as_str() == "saveCompatibility")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixSaveCompatibility::parse)
        .unwrap_or(GameFixSaveCompatibility::Unknown);

    let verification_state = obj
        .iter()
        .find(|(k, _)| k.as_str() == "verificationState")
        .and_then(|(_, v)| v.as_str())
        .and_then(GameFixVerificationState::parse)
        .unwrap_or(GameFixVerificationState::Research);

    let detection_method = get_str_opt("detectionMethod").unwrap_or_default();
    let references = get_str_list("references");

    let mut text_patches = Vec::new();
    if let Some(patches) = obj
        .iter()
        .find(|(k, _)| k.as_str() == "textPatches")
        .and_then(|(_, v)| v.as_array())
    {
        for patch in patches {
            let p_obj = patch
                .as_object()
                .ok_or_else(|| Error::damaged("Text patch is not an object"))?;
            let rel = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "relativePath")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Text patch missing relativePath"))?
                .to_string();
            let exp = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "expectedText")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Text patch missing expectedText"))?
                .to_string();
            let rep = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "replacementText")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Text patch missing replacementText"))?
                .to_string();
            let sha = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "expectedFileSha256")
                .and_then(|(_, v)| v.as_str())
                .map(|s| s.to_string());
            let cp = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "codePage")
                .and_then(|(_, v)| v.as_u64())
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(28591);
            let retail_only = p_obj
                .iter()
                .find(|(k, _)| k.as_str() == "retailOnly")
                .and_then(|(_, v)| v.as_bool())
                .unwrap_or(false);

            text_patches.push(TextPatchOperation {
                relative_path: rel,
                expected_text: exp,
                replacement_text: rep,
                expected_file_sha256: sha,
                code_page: cp,
                retail_only,
            });
        }
    }

    let mut overlays = Vec::new();
    if let Some(overlays_json) = obj
        .iter()
        .find(|(k, _)| k.as_str() == "overlays")
        .and_then(|(_, v)| v.as_array())
    {
        for overlay in overlays_json {
            let o_obj = overlay
                .as_object()
                .ok_or_else(|| Error::damaged("Overlay is not an object"))?;
            let rel = o_obj
                .iter()
                .find(|(k, _)| k.as_str() == "relativePath")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Overlay missing relativePath"))?
                .to_string();
            let content_sha = o_obj
                .iter()
                .find(|(k, _)| k.as_str() == "contentSha256")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Overlay missing contentSha256"))?
                .to_string();
            let exp_sha = o_obj
                .iter()
                .find(|(k, _)| k.as_str() == "expectedFileSha256")
                .and_then(|(_, v)| v.as_str())
                .map(|s| s.to_string());

            overlays.push(FileOverlayOperation {
                relative_path: rel,
                content_sha256: content_sha,
                expected_file_sha256: exp_sha,
            });
        }
    }

    let mut spawn_edits = Vec::new();
    if let Some(edits_json) = obj
        .iter()
        .find(|(k, _)| k.as_str() == "spawnEdits")
        .and_then(|(_, v)| v.as_array())
    {
        for edit in edits_json {
            let e_obj = edit
                .as_object()
                .ok_or_else(|| Error::damaged("Spawn edit is not an object"))?;
            let rel = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "relativePath")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Spawn edit missing relativePath"))?
                .to_string();
            let kind_str = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "kind")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Spawn edit missing kind"))?;
            let kind = SpawnEditKind::parse(kind_str)
                .ok_or_else(|| Error::damaged(format!("Unknown spawn edit kind '{kind_str}'")))?;
            let target = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "target")
                .and_then(|(_, v)| v.as_str())
                .ok_or_else(|| Error::damaged("Spawn edit missing target"))?
                .to_string();
            let exp_sha = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "expectedFileSha256")
                .and_then(|(_, v)| v.as_str())
                .map(|s| s.to_string());
            let point = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "point")
                .and_then(|(_, v)| v.as_u64())
                .and_then(|v| usize::try_from(v).ok())
                .unwrap_or(0);
            let expected = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "expected")
                .and_then(|(_, v)| v.as_str())
                .unwrap_or_default()
                .to_string();
            let replacement = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "replacement")
                .and_then(|(_, v)| v.as_str())
                .map(|s| s.to_string());

            let position = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "position")
                .and_then(|(_, v)| v.as_array())
                .and_then(|arr| {
                    if arr.len() == 3 {
                        let x = arr
                            .first()
                            .and_then(JsonValue::as_f64)
                            .and_then(|f| format!("{f}").parse::<f32>().ok())?;
                        let y = arr
                            .get(1)
                            .and_then(JsonValue::as_f64)
                            .and_then(|f| format!("{f}").parse::<f32>().ok())?;
                        let z = arr
                            .get(2)
                            .and_then(JsonValue::as_f64)
                            .and_then(|f| format!("{f}").parse::<f32>().ok())?;
                        Some([x, y, z])
                    } else {
                        None
                    }
                });

            let level_vertex_id = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "levelVertexId")
                .and_then(|(_, v)| v.as_u64())
                .and_then(|v| u32::try_from(v).ok());

            let game_vertex_id = e_obj
                .iter()
                .find(|(k, _)| k.as_str() == "gameVertexId")
                .and_then(|(_, v)| v.as_u64())
                .and_then(|v| u16::try_from(v).ok());

            spawn_edits.push(SpawnEditOperation {
                relative_path: rel,
                kind,
                target,
                expected_file_sha256: exp_sha,
                point,
                expected,
                replacement,
                position,
                level_vertex_id,
                game_vertex_id,
            });
        }
    }

    Ok(GameFixDefinition {
        id,
        game,
        version,
        title,
        supported_steam_build_ids,
        category,
        maturity,
        depends_on,
        conflicts_with,
        text_patches,
        source,
        problem,
        description,
        implementation,
        requires_new_game,
        save_compatibility,
        verification_state,
        detection_method,
        references,
        overlays,
        spawn_edits,
    })
}
