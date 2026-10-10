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
use crate::models::{GameFixDefinition, GameFixState, GameTarget};
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

        let (installed, uninstalled) = reconcile_fixes(
            engine,
            game_directory,
            &current_ids,
            &profile.target_fix_ids,
            GameFixCatalog::try_get,
            |id| {
                format!(
                    "Fix '{id}' specified in profile '{}' not found in catalog",
                    profile.name
                )
            },
        )?;

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
                    engine.ensure_game_not_running_for(GameTarget::Stalker2)?;
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

        let game = game_target_from_field(&val)?;

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

/// Reads the `game` field of a stored profile or snapshot. A missing field means SoC (legacy files).
///
/// # Errors
/// Returns [`Error::Damaged`] for a non-string or unknown game name instead of silently choosing another game.
pub(crate) fn game_target_from_field(value: &sse_catalog::JsonValue) -> Result<GameTarget> {
    let name = match value.get("game") {
        None => "soc",
        Some(field) => field
            .as_str()
            .ok_or_else(|| Error::damaged("Profile or snapshot 'game' must be a string"))?,
    };
    game_target_from_name(name).ok_or_else(|| Error::damaged(format!("Unknown game '{name}'")))
}

fn game_target_from_name(name: &str) -> Option<GameTarget> {
    match name {
        "soc" => Some(GameTarget::ShadowOfChernobyl),
        "cs" => Some(GameTarget::ClearSky),
        "cop" => Some(GameTarget::CallOfPripyat),
        "soc_ee" => Some(GameTarget::ShadowOfChernobylEnhancedEdition),
        "cs_ee" => Some(GameTarget::ClearSkyEnhancedEdition),
        "cop_ee" => Some(GameTarget::CallOfPripyatEnhancedEdition),
        "s2" => Some(GameTarget::Stalker2),
        _ => None,
    }
}

/// Moves the installed fixes from `current_ids` to `target_ids`.
///
/// New fixes are installed first. Fixes that are no longer wanted are removed only after every install
/// succeeded. If any step fails, the switch is undone: fixes removed by this call are installed again from
/// `definition_of`, and fixes installed by this call are removed, so the game is left as it was before the call.
/// Returns `(installed, uninstalled)`.
///
/// # Errors
/// Returns the failing step's error, with any failed undo step reported with it.
pub(crate) fn reconcile_fixes<'d>(
    engine: &GameFixEngine,
    game_directory: &Path,
    current_ids: &[String],
    target_ids: &[String],
    definition_of: impl Fn(&str) -> Option<&'d GameFixDefinition>,
    missing_message: impl Fn(&str) -> String,
) -> Result<(Vec<String>, Vec<String>)> {
    let mut installed: Vec<String> = Vec::new();
    for target_id in target_ids {
        if current_ids.contains(target_id) {
            continue;
        }
        let Some(definition) = definition_of(target_id) else {
            return Err(Error::Refused(missing_message(target_id)));
        };
        if let Err(error) = engine.install(definition, game_directory) {
            let undo_failures = undo_fix_switch(engine, game_directory, &installed, &[], &definition_of);
            return Err(fix_switch_error(error, undo_failures));
        }
        installed.push(target_id.clone());
    }

    let mut uninstalled: Vec<String> = Vec::new();
    for cur_id in current_ids {
        if target_ids.contains(cur_id) {
            continue;
        }
        if let Err(error) = engine.uninstall(cur_id, game_directory) {
            let undo_failures = undo_fix_switch(engine, game_directory, &installed, &uninstalled, &definition_of);
            return Err(fix_switch_error(error, undo_failures));
        }
        uninstalled.push(cur_id.clone());
    }
    Ok((installed, uninstalled))
}

/// Undoes a partly applied fix switch: removes `installed` (newest first), then reinstalls `uninstalled`
/// (newest first). Returns one message per step that could not be undone.
fn undo_fix_switch<'d>(
    engine: &GameFixEngine,
    game_directory: &Path,
    installed: &[String],
    uninstalled: &[String],
    definition_of: &impl Fn(&str) -> Option<&'d GameFixDefinition>,
) -> Vec<String> {
    let mut failures = Vec::new();
    for id in installed.iter().rev() {
        if let Err(undo_error) = engine.uninstall(id, game_directory) {
            failures.push(format!("{id} is still installed ({undo_error})"));
        }
    }
    for id in uninstalled.iter().rev() {
        let Some(definition) = definition_of(id) else {
            failures.push(format!("{id} was removed and is not in the catalog to restore it"));
            continue;
        };
        if let Err(undo_error) = engine.install(definition, game_directory) {
            failures.push(format!("{id} was removed and could not be restored ({undo_error})"));
        }
    }
    failures
}

fn fix_switch_error(error: Error, undo_failures: Vec<String>) -> Error {
    if undo_failures.is_empty() {
        return error;
    }
    Error::System(format!(
        "Fix switch failed: {error}; undo incomplete: {}",
        undo_failures.join("; ")
    ))
}

pub(super) fn game_target_str(target: GameTarget) -> &'static str {
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

#[cfg(test)]
mod reconcile_tests {
    use super::reconcile_fixes;
    use crate::engine::GameFixEngine;
    use crate::models::{
        GameFixCategory, GameFixDefinition, GameFixImplementationType, GameFixMaturity, GameFixSaveCompatibility,
        GameFixVerificationState, GameTarget, TextPatchOperation,
    };
    use std::collections::HashMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(1);

    fn game_root() -> PathBuf {
        std::env::temp_dir().join(format!(
            "sse-reconcile-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    /// A ClearSky install folder with the X-Ray marker and an app manifest on a supported build.
    fn make_game(root: &Path) -> sse_core::Result<()> {
        fs::create_dir_all(root.join("gamedata/configs")).map_err(sse_core::Error::from)?;
        fs::write(
            root.join("fsgame.ltx"),
            "; fsgame marker\n$game_data$ = false| false| $fs_root$| gamedata\\\n",
        )
        .map_err(sse_core::Error::from)?;
        let app_id = GameTarget::ClearSky.steam_app_id().unwrap_or(0);
        let acf = format!("\"AppState\"\n{{\n\t\"appid\"\t\t\"{app_id}\"\n\t\"buildid\"\t\t\"11450472\"\n}}\n");
        fs::write(root.join(format!("appmanifest_{app_id}.acf")), acf).map_err(sse_core::Error::from)?;
        for name in ["a", "b", "c"] {
            fs::write(root.join(format!("gamedata/configs/{name}.ltx")), format!("old-{name}"))
                .map_err(sse_core::Error::from)?;
        }
        Ok(())
    }

    fn fix(id: &str, name: &str) -> GameFixDefinition {
        GameFixDefinition {
            id: id.to_string(),
            game: GameTarget::ClearSky,
            version: "1.0.0".to_string(),
            title: format!("Synthetic fix {id}"),
            supported_steam_build_ids: vec!["11450472".to_string()],
            category: GameFixCategory::Essential,
            maturity: GameFixMaturity::Validated,
            depends_on: vec![],
            conflicts_with: vec![],
            text_patches: vec![TextPatchOperation {
                relative_path: format!("gamedata/configs/{name}.ltx"),
                expected_text: format!("old-{name}"),
                replacement_text: format!("new-{name}"),
                code_page: 28591,
                expected_file_sha256: None,
                retail_only: false,
            }],
            source: "test".to_string(),
            problem: "test".to_string(),
            description: "test".to_string(),
            implementation: GameFixImplementationType::ExactTextReplacement,
            requires_new_game: false,
            save_compatibility: GameFixSaveCompatibility::ExistingSaves,
            verification_state: GameFixVerificationState::RetailFilesVerified,
            detection_method: "test".to_string(),
            references: vec![],
            overlays: vec![],
            spawn_edits: vec![],
        }
    }

    fn installed_ids(engine: &GameFixEngine, root: &Path) -> sse_core::Result<Vec<String>> {
        let mut ids: Vec<String> = engine.list_installed(root, None)?.into_iter().map(|f| f.id).collect();
        ids.sort();
        Ok(ids)
    }

    #[test]
    fn a_failed_removal_puts_back_the_fixes_it_already_removed() -> sse_core::Result<()> {
        let root = game_root();
        make_game(&root)?;
        let engine = GameFixEngine::with_synthetic(true);
        let definitions: HashMap<String, GameFixDefinition> =
            [fix("test.a", "a"), fix("test.b", "b"), fix("test.c", "c")]
                .into_iter()
                .map(|d| (d.id.clone(), d))
                .collect();
        let lookup = |id: &str| definitions.get(id);

        // Installed before the switch: A and B. The switch wants only C.
        reconcile_fixes(
            &engine,
            &root,
            &[],
            &["test.a".to_string(), "test.b".to_string()],
            lookup,
            |id| id.to_string(),
        )?;
        // Someone edits B's file after install: removing B will be refused.
        fs::write(root.join("gamedata/configs/b.ltx"), "edited by hand").map_err(sse_core::Error::from)?;

        let current = vec!["test.a".to_string(), "test.b".to_string()];
        let Err(error) = reconcile_fixes(&engine, &root, &current, &["test.c".to_string()], lookup, |id| {
            id.to_string()
        }) else {
            return Err(sse_core::Error::System("removing B must fail".to_string()));
        };

        let message = error.to_string();
        assert!(
            !message.contains("undo incomplete"),
            "undo should be complete: {message}"
        );
        // Back to the starting point: A and B installed, C not installed, A's file restored to its installed text.
        assert_eq!(
            installed_ids(&engine, &root)?,
            vec!["test.a".to_string(), "test.b".to_string()]
        );
        assert_eq!(
            fs::read_to_string(root.join("gamedata/configs/a.ltx")).map_err(sse_core::Error::from)?,
            "new-a"
        );
        assert_eq!(
            fs::read_to_string(root.join("gamedata/configs/c.ltx")).map_err(sse_core::Error::from)?,
            "old-c"
        );
        assert_eq!(
            fs::read_to_string(root.join("gamedata/configs/b.ltx")).map_err(sse_core::Error::from)?,
            "edited by hand"
        );
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }
}
