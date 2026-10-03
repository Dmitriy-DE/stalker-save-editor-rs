//! Named profiles capturing game fix selections, companion status, and `user.ltx` overrides.
//!
//! Automatically takes a pre-switch snapshot before applying changes to guarantee rollback.
//! All operations take the game installation path explicitly.

use std::collections::BTreeMap;
use std::path::Path;

use crate::catalog::GameFixCatalog;
use crate::engine::GameFixEngine;
use crate::models::GameTarget;
use crate::toolkit::s2_mods::{ModToggleStatus, Stalker2ModToggle};
use crate::toolkit::snapshot::ToolkitSnapshotService;
use crate::toolkit::user_ltx::ManagedUserLtxSettings;
use sse_core::{Error, Result};

/// A named configuration profile for a specific game target.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolkitProfile {
    /// Unique profile name.
    pub name: String,
    /// Detailed description.
    pub description: String,
    /// Targeted game.
    pub game: GameTarget,
    /// Fix identifiers that must be installed when this profile is active.
    pub target_fix_ids: Vec<String>,
    /// Managed console settings overrides for `user.ltx`.
    pub user_ltx_overrides: BTreeMap<String, String>,
    /// Desired S.T.A.L.K.E.R. 2 mods state (if applicable).
    pub s2_mods_enabled: Option<bool>,
}

/// Result of applying a configuration profile to a game installation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileApplyResult {
    /// Applied profile name.
    pub profile_name: String,
    /// ID of the automatically captured snapshot created before making changes.
    pub pre_switch_snapshot_id: String,
    /// Fixes that were installed during profile application.
    pub installed_fixes: Vec<String>,
    /// Fixes that were uninstalled during profile application.
    pub uninstalled_fixes: Vec<String>,
    /// Number of updated `user.ltx` settings.
    pub user_ltx_changes: usize,
}

/// Service managing configuration profile application and exchange.
pub struct ToolkitProfileService;

impl ToolkitProfileService {
    /// Applies a configuration profile to the specified game installation.
    ///
    /// Automatically takes a pre-switch rollback snapshot before altering any files on disk.
    ///
    /// # Errors
    /// Returns an error if snapshot creation fails or fix installation/uninstallation fails.
    pub fn apply_profile(
        game_directory: &Path,
        profile: &ToolkitProfile,
        engine: &GameFixEngine,
        _catalog: &GameFixCatalog,
    ) -> Result<ProfileApplyResult> {
        // 1. Mandatory pre-switch snapshot
        let pre_snapshot = ToolkitSnapshotService::create_snapshot(
            game_directory,
            profile.game,
            engine,
            Some(&format!("Pre-switch snapshot before '{}'", profile.name)),
        )?;

        // 2. Reconcile installed fixes
        let currently_installed = engine.list_installed(game_directory, None)?;
        let current_ids: Vec<String> = currently_installed.into_iter().map(|f| f.id).collect();

        let mut uninstalled = Vec::new();
        for cur_id in &current_ids {
            if !profile.target_fix_ids.contains(cur_id) {
                engine.uninstall(cur_id, game_directory)?;
                uninstalled.push(cur_id.clone());
            }
        }

        let mut installed = Vec::new();
        for target_id in &profile.target_fix_ids {
            if !current_ids.contains(target_id) {
                if let Some(def) = GameFixCatalog::try_get(target_id) {
                    engine.install(def, game_directory)?;
                    installed.push(target_id.clone());
                } else {
                    return Err(Error::Refused(format!(
                        "Fix '{target_id}' specified in profile '{}' not found in catalog",
                        profile.name
                    )));
                }
            }
        }

        // 3. Apply user.ltx overrides
        let user_ltx_changes = if !profile.user_ltx_overrides.is_empty() {
            ManagedUserLtxSettings::update_managed_settings(game_directory, &profile.user_ltx_overrides)?
        } else {
            0
        };

        // 4. Align S.T.A.L.K.E.R. 2 mods state if requested
        if profile.game == GameTarget::Stalker2 {
            if let Some(want_enabled) = profile.s2_mods_enabled {
                let status = Stalker2ModToggle::query_status(game_directory)?;
                let needs_toggle = match status {
                    ModToggleStatus::Active { .. } => !want_enabled,
                    ModToggleStatus::Disabled { .. } => want_enabled,
                    ModToggleStatus::NotFound | ModToggleStatus::Empty => want_enabled,
                    _ => false,
                };
                if needs_toggle {
                    Stalker2ModToggle::toggle(game_directory)?;
                }
            }
        }

        Ok(ProfileApplyResult {
            profile_name: profile.name.clone(),
            pre_switch_snapshot_id: pre_snapshot.id,
            installed_fixes: installed,
            uninstalled_fixes: uninstalled,
            user_ltx_changes,
        })
    }

    /// Serializes a profile to RFC 8259 JSON format.
    #[must_use]
    pub fn serialize_profile(profile: &ToolkitProfile) -> String {
        let mut out = String::with_capacity(512);
        out.push_str("{\n");
        out.push_str(&format!("  \"name\": \"{}\",\n", profile.name));
        out.push_str(&format!("  \"description\": \"{}\",\n", profile.description));
        out.push_str(&format!("  \"game\": \"{}\",\n", game_target_str(profile.game)));
        out.push_str("  \"target_fix_ids\": [\n");
        for (i, id) in profile.target_fix_ids.iter().enumerate() {
            let comma = if i.saturating_add(1) == profile.target_fix_ids.len() {
                ""
            } else {
                ","
            };
            out.push_str(&format!("    \"{}\"{}\n", id, comma));
        }
        out.push_str("  ],\n");
        out.push_str("  \"user_ltx_overrides\": {\n");
        let mut overrides: Vec<_> = profile.user_ltx_overrides.iter().collect();
        overrides.sort_by(|a, b| a.0.cmp(b.0));
        for (i, (k, v)) in overrides.iter().enumerate() {
            let comma = if i.saturating_add(1) == overrides.len() {
                ""
            } else {
                ","
            };
            out.push_str(&format!("    \"{}\": \"{}\"{}\n", k, v, comma));
        }
        out.push_str("  }");
        if let Some(s2_mods) = profile.s2_mods_enabled {
            out.push_str(&format!(",\n  \"s2_mods_enabled\": {}\n", s2_mods));
        } else {
            out.push('\n');
        }
        out.push_str("}\n");
        out
    }

    /// Deserializes a profile from JSON bytes.
    ///
    /// # Errors
    /// Returns an error if the JSON is malformed or missing required keys.
    pub fn deserialize_profile(bytes: &[u8]) -> Result<ToolkitProfile> {
        let text = std::str::from_utf8(bytes).map_err(|_| Error::damaged("Invalid UTF-8 in profile JSON"))?;
        let val = sse_catalog::parse_json(text).map_err(|e| Error::damaged(format!("{e}")))?;

        let name = val
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::damaged("Missing profile 'name'"))?
            .to_string();

        let description = val
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let game_str = val.get("game").and_then(|v| v.as_str()).unwrap_or("soc");

        let game = match game_str {
            "cs" => GameTarget::ClearSky,
            "cop" => GameTarget::CallOfPripyat,
            "soc_ee" => GameTarget::ShadowOfChernobylEnhancedEdition,
            "cs_ee" => GameTarget::ClearSkyEnhancedEdition,
            "cop_ee" => GameTarget::CallOfPripyatEnhancedEdition,
            "s2" => GameTarget::Stalker2,
            _ => GameTarget::ShadowOfChernobyl,
        };

        let mut target_fix_ids = Vec::new();
        if let Some(ids_arr) = val.get("target_fix_ids").and_then(|v| v.as_array()) {
            for item in ids_arr {
                if let Some(s) = item.as_str() {
                    target_fix_ids.push(s.to_string());
                }
            }
        }

        let mut user_ltx_overrides = BTreeMap::new();
        if let Some(obj) = val.get("user_ltx_overrides").and_then(|v| v.as_object()) {
            for (k, v) in obj {
                if let Some(val_str) = v.as_str() {
                    user_ltx_overrides.insert(k.clone(), val_str.to_string());
                }
            }
        }

        let s2_mods_enabled = val.get("s2_mods_enabled").and_then(|v| v.as_bool());

        Ok(ToolkitProfile {
            name,
            description,
            game,
            target_fix_ids,
            user_ltx_overrides,
            s2_mods_enabled,
        })
    }
}

fn game_target_str(target: GameTarget) -> &'static str {
    match target {
        GameTarget::ShadowOfChernobyl => "soc",
        GameTarget::ClearSky => "cs",
        GameTarget::CallOfPripyat => "cop",
        GameTarget::ShadowOfChernobylEnhancedEdition => "soc_ee",
        GameTarget::ClearSkyEnhancedEdition => "cs_ee",
        GameTarget::CallOfPripyatEnhancedEdition => "cop_ee",
        GameTarget::Stalker2 => "s2",
    }
}
