//! Named profiles capturing game fix selections, companion status, and `user.ltx` overrides.
//!
//! Automatically takes a pre-switch snapshot before applying changes to guarantee rollback.
//! All operations take the game installation path explicitly.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::catalog::GameFixCatalog;
use crate::engine::GameFixEngine;
use crate::models::{GameFixState, GameTarget};
use crate::toolkit::s2_mods::{ModToggleStatus, Stalker2ModToggle};
use crate::toolkit::snapshot::ToolkitSnapshotService;
use crate::toolkit::user_ltx::ManagedUserLtxSettings;
use sse_core::{Error, Result};

static NEXT_PROFILE_ID: AtomicU64 = AtomicU64::new(1);
const PROFILE_ID_HEX_LEN: usize = 32;

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

/// A persisted toolkit profile and its storage identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredToolkitProfile {
    /// Lowercase 32-hex identifier used as the profile file name.
    pub id: String,
    /// Persisted managed-state profile.
    pub profile: ToolkitProfile,
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
    /// Lists persisted profiles from `<data>/profiles/<id>.json`.
    pub fn list_profiles(data_directory: &Path) -> Result<Vec<StoredToolkitProfile>> {
        let directory = data_directory.join("profiles");
        if !directory.exists() {
            return Ok(Vec::new());
        }
        let mut profiles = Vec::new();
        for entry in fs::read_dir(&directory).map_err(|error| Error::System(error.to_string()))? {
            let entry = entry.map_err(|error| Error::System(error.to_string()))?;
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(id) = file_name.strip_suffix(".json") else {
                continue;
            };
            if !valid_profile_id(id)
                || !entry
                    .file_type()
                    .map_err(|error| Error::System(error.to_string()))?
                    .is_file()
            {
                continue;
            }
            let bytes = fs::read(&path).map_err(|error| Error::System(error.to_string()))?;
            profiles.push(StoredToolkitProfile {
                id: id.to_owned(),
                profile: Self::deserialize_profile(&bytes)?,
            });
        }
        profiles.sort_by(|left, right| left.profile.name.cmp(&right.profile.name).then(left.id.cmp(&right.id)));
        Ok(profiles)
    }

    /// Saves a profile after verifying that every installed Game Fix is still intact.
    pub fn save_profile(
        data_directory: &Path,
        game_directory: &Path,
        profile: &ToolkitProfile,
        engine: &GameFixEngine,
    ) -> Result<String> {
        ensure_xray_profile(profile.game)?;
        for installed in engine.list_installed(game_directory, None)? {
            if installed.state == GameFixState::Modified {
                return Err(Error::Refused(format!(
                    "Game Fix {} has drifted; resolve it before saving a profile.",
                    installed.id
                )));
            }
        }
        let directory = data_directory.join("profiles");
        prepare_private_profile_directory(&directory)?;
        let id = fresh_profile_id();
        let path = directory.join(format!("{id}.json"));
        write_private_atomic(&path, Self::serialize_profile(profile).as_bytes())?;
        Ok(id)
    }

    /// Deletes one persisted profile without following arbitrary path components.
    pub fn delete_profile(data_directory: &Path, id: &str) -> Result<()> {
        if !valid_profile_id(id) {
            return Err(Error::Refused("Invalid toolkit profile id.".to_owned()));
        }
        let path = data_directory.join("profiles").join(format!("{id}.json"));
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::System(error.to_string())),
        }
    }

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
        ensure_xray_profile(profile.game)?;
        // Refuse before any change: a profile that cannot be fully applied must not uninstall fixes first.
        for target_id in &profile.target_fix_ids {
            if GameFixCatalog::try_get(target_id).is_none() {
                return Err(Error::Refused(format!(
                    "Fix '{target_id}' specified in profile '{}' not found in catalog",
                    profile.name
                )));
            }
        }
        ManagedUserLtxSettings::validate_managed_settings(&profile.user_ltx_overrides)?;

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
        out.push_str(&format!("  \"name\": \"{}\",\n", json_escape(&profile.name)));
        out.push_str(&format!(
            "  \"description\": \"{}\",\n",
            json_escape(&profile.description)
        ));
        out.push_str(&format!("  \"game\": \"{}\",\n", game_target_str(profile.game)));
        out.push_str("  \"target_fix_ids\": [\n");
        for (i, id) in profile.target_fix_ids.iter().enumerate() {
            let comma = if i.saturating_add(1) == profile.target_fix_ids.len() {
                ""
            } else {
                ","
            };
            out.push_str(&format!("    \"{}\"{}\n", json_escape(id), comma));
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
            out.push_str(&format!(
                "    \"{}\": \"{}\"{}\n",
                json_escape(k),
                json_escape(v),
                comma
            ));
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

pub(super) fn json_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            ch if ch.is_control() => escaped.push_str(&format!("\\u{:04x}", u32::from(ch))),
            ch => escaped.push(ch),
        }
    }
    escaped
}

fn ensure_xray_profile(game: GameTarget) -> Result<()> {
    if game == GameTarget::Stalker2 {
        return Err(Error::Refused(
            "Toolkit profiles currently support X-Ray managed providers only.".to_owned(),
        ));
    }
    Ok(())
}

fn valid_profile_id(id: &str) -> bool {
    id.len() == PROFILE_ID_HEX_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn fresh_profile_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let counter = u128::from(NEXT_PROFILE_ID.fetch_add(1, Ordering::Relaxed));
    format!("{:032x}", nanos ^ counter)
}

fn prepare_private_profile_directory(directory: &Path) -> Result<()> {
    fs::create_dir_all(directory).map_err(|error| Error::System(error.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|error| Error::System(error.to_string()))?;
    }
    Ok(())
}

fn write_private_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::damaged("Profile path has no parent"))?;
    let temp = parent.join(format!(
        ".profile-{}-{}.tmp",
        std::process::id(),
        NEXT_PROFILE_ID.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|error| Error::System(error.to_string()))?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(&temp);
        return Err(Error::System(error.to_string()));
    }
    drop(file);
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(Error::System(error.to_string()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .map_err(|error| Error::System(error.to_string()))?;
    }
    Ok(())
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

#[cfg(test)]
mod storage_tests {
    use super::{ToolkitProfile, ToolkitProfileService};
    use crate::{GameFixEngine, GameTarget};
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "sse-profile-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn profile(game: GameTarget) -> ToolkitProfile {
        ToolkitProfile {
            name: "Тест".to_owned(),
            description: String::new(),
            game,
            target_fix_ids: Vec::new(),
            user_ltx_overrides: BTreeMap::new(),
            s2_mods_enabled: None,
        }
    }

    #[test]
    fn profile_store_round_trips_and_deletes_private_file() -> sse_core::Result<()> {
        let root = temp_root();
        let game = root.join("game");
        fs::create_dir_all(&game).map_err(sse_core::Error::from)?;
        let engine = GameFixEngine::new();
        let id = ToolkitProfileService::save_profile(&root, &game, &profile(GameTarget::CallOfPripyat), &engine)?;
        let stored = ToolkitProfileService::list_profiles(&root)?;
        assert_eq!(stored.len(), 1);
        assert_eq!(stored.first().map(|item| item.id.as_str()), Some(id.as_str()));
        assert_eq!(stored.first().map(|item| item.profile.name.as_str()), Some("Тест"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.join("profiles"))
                    .map_err(sse_core::Error::from)?
                    .permissions()
                    .mode()
                    & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(root.join("profiles").join(format!("{id}.json")))
                    .map_err(sse_core::Error::from)?
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        ToolkitProfileService::delete_profile(&root, &id)?;
        assert!(ToolkitProfileService::list_profiles(&root)?.is_empty());
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn stalker2_profile_save_is_refused_with_acceptance_text() {
        let root = temp_root();
        let game = root.join("game");
        let _ = fs::create_dir_all(&game);
        let result =
            ToolkitProfileService::save_profile(&root, &game, &profile(GameTarget::Stalker2), &GameFixEngine::new());
        assert_eq!(
            result.err().map(|error| error.to_string()).as_deref(),
            Some("Toolkit profiles currently support X-Ray managed providers only.")
        );
        let _ = fs::remove_dir_all(root);
    }
}
