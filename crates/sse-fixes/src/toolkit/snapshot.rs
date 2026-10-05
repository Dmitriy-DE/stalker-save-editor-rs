//! Content-addressed snapshots and replayable rollback service.
//!
//! Captures and restores game fix states, companion mod status, and `user.ltx` settings.
//! All operations take the game installation path explicitly.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::catalog::GameFixCatalog;
use crate::engine::GameFixEngine;
use crate::fs_util::{check_no_links, AtomicFileWriter};
use crate::models::GameTarget;
use crate::toolkit::s2_mods::{ModToggleStatus, Stalker2ModToggle};
use crate::toolkit::user_ltx::ManagedUserLtxSettings;
use sse_core::{Error, Result};

const SNAPSHOTS_DIRECTORY: &str = ".sse/snapshots";

/// Record of an installed fix captured inside a snapshot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledFixSnapshot {
    /// Identifier of the fix.
    pub fix_id: String,
    /// Installed version.
    pub version: String,
}

/// A content-addressed point-in-time snapshot of the game environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolkitSnapshot {
    /// Unique SHA-256 identifier derived from snapshot contents.
    pub id: String,
    /// Optional user label or auto-generated name.
    pub label: String,
    /// Unix timestamp when the snapshot was taken.
    pub timestamp_epoch: u64,
    /// Game target.
    pub game: GameTarget,
    /// List of installed fixes at snapshot time.
    pub installed_fixes: Vec<InstalledFixSnapshot>,
    /// Managed `user.ltx` settings map at snapshot time.
    pub managed_user_ltx: BTreeMap<String, String>,
    /// Status of S.T.A.L.K.E.R. 2 mod folder (if applicable).
    pub s2_mods_state: Option<String>,
    /// Whether companion mod is installed.
    pub companion_installed: bool,
}

/// Outcome report of a snapshot restore operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotRestoreReport {
    /// Identifier of the restored snapshot.
    pub snapshot_id: String,
    /// Fixes that were installed during restore.
    pub installed_fixes: Vec<String>,
    /// Fixes that were uninstalled during restore.
    pub uninstalled_fixes: Vec<String>,
    /// Count of updated `user.ltx` settings.
    pub user_ltx_updates_count: usize,
    /// Whether S.T.A.L.K.E.R. 2 mods state was adjusted.
    pub s2_mods_toggled: bool,
}

/// Service orchestrating creation, listing, and restoration of toolkit snapshots.
pub struct ToolkitSnapshotService;

impl ToolkitSnapshotService {
    /// Resolves the snapshots directory for a game installation.
    #[must_use]
    pub fn snapshots_dir(game_directory: &Path) -> PathBuf {
        game_directory.join(SNAPSHOTS_DIRECTORY)
    }

    /// Creates a fresh content-addressed snapshot of the game's current configuration.
    ///
    /// # Errors
    /// Returns an error if querying installed state or writing the snapshot fails.
    pub fn create_snapshot(
        game_directory: &Path,
        game: GameTarget,
        engine: &GameFixEngine,
        label: Option<&str>,
    ) -> Result<ToolkitSnapshot> {
        let dir = Self::snapshots_dir(game_directory);
        check_no_links(game_directory, &dir)?;

        let installed_list = engine.list_installed(game_directory, None)?;
        let mut installed_fixes = Vec::new();
        for inst in installed_list {
            installed_fixes.push(InstalledFixSnapshot {
                fix_id: inst.id,
                version: inst.version,
            });
        }
        installed_fixes.sort_by(|a, b| a.fix_id.cmp(&b.fix_id));

        let managed_user_ltx = ManagedUserLtxSettings::read_managed_settings(game_directory)?;

        let s2_mods_state = if game == GameTarget::Stalker2 {
            match Stalker2ModToggle::query_status(game_directory)? {
                ModToggleStatus::Active { .. } => Some("active".to_string()),
                ModToggleStatus::Disabled { .. } => Some("disabled".to_string()),
                ModToggleStatus::Empty => Some("empty".to_string()),
                _ => None,
            }
        } else {
            None
        };

        let companion_installed = check_companion_installed(game, game_directory);

        let now_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let snapshot_label = label
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("Snapshot {}", now_epoch));

        let hash_input = format!(
            "{}:{}:{}:{:?}:{:?}:{:?}:{}",
            game_target_name(game),
            snapshot_label,
            now_epoch,
            installed_fixes,
            managed_user_ltx,
            s2_mods_state,
            companion_installed
        );
        let id = sse_codecs::sha256::sha256_hex(hash_input.as_bytes());

        let snapshot = ToolkitSnapshot {
            id: id.clone(),
            label: snapshot_label,
            timestamp_epoch: now_epoch,
            game,
            installed_fixes,
            managed_user_ltx,
            s2_mods_state,
            companion_installed,
        };

        let json_text = serialize_snapshot(&snapshot);
        let file_path = dir.join(format!("{id}.json"));
        AtomicFileWriter::write(&file_path, json_text.as_bytes(), true)?;

        Ok(snapshot)
    }

    /// Lists all snapshots available for the given game installation, newest first.
    ///
    /// # Errors
    /// Returns an error if reading the directory fails.
    pub fn list_snapshots(game_directory: &Path) -> Result<Vec<ToolkitSnapshot>> {
        let dir = Self::snapshots_dir(game_directory);
        if !dir.is_dir() {
            return Ok(Vec::new());
        }

        let mut results = Vec::new();
        let entries = fs::read_dir(&dir).map_err(Error::from)?;
        for entry in entries {
            let entry = entry.map_err(Error::from)?;
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(bytes) = fs::read(&path) {
                    if let Ok(snapshot) = deserialize_snapshot(&bytes) {
                        results.push(snapshot);
                    }
                }
            }
        }

        results.sort_by_key(|a| std::cmp::Reverse(a.timestamp_epoch));
        Ok(results)
    }

    /// Loads a specific snapshot by identifier.
    ///
    /// # Errors
    /// Returns an error if the snapshot does not exist or is corrupted.
    pub fn get_snapshot(game_directory: &Path, snapshot_id: &str) -> Result<ToolkitSnapshot> {
        let file_path = Self::snapshots_dir(game_directory).join(format!("{snapshot_id}.json"));
        if !file_path.is_file() {
            return Err(Error::Refused(format!("Snapshot '{snapshot_id}' not found")));
        }
        let bytes = fs::read(&file_path).map_err(Error::from)?;
        deserialize_snapshot(&bytes)
    }

    /// Deletes a stored snapshot by its content-addressed identifier.
    pub fn delete_snapshot(game_directory: &Path, snapshot_id: &str) -> Result<()> {
        if snapshot_id.len() != 64
            || !snapshot_id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error::Refused("Invalid toolkit snapshot id.".to_owned()));
        }
        let dir = Self::snapshots_dir(game_directory);
        check_no_links(game_directory, &dir)?;
        let path = dir.join(format!("{snapshot_id}.json"));
        check_no_links(game_directory, &path)?;
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::from(error)),
        }
    }

    /// Restores the game environment to the state captured in the specified snapshot.
    ///
    /// # Errors
    /// Returns an error if applying or rolling back fixes fails.
    pub fn restore_snapshot(
        game_directory: &Path,
        engine: &GameFixEngine,
        _catalog: &GameFixCatalog,
        snapshot_id: &str,
    ) -> Result<SnapshotRestoreReport> {
        let snapshot = Self::get_snapshot(game_directory, snapshot_id)?;

        // 1. Reconcile fixes
        let current_installed = engine.list_installed(game_directory, None)?;
        let current_ids: Vec<String> = current_installed.into_iter().map(|f| f.id).collect();
        let target_ids: Vec<String> = snapshot.installed_fixes.iter().map(|f| f.fix_id.clone()).collect();

        let mut uninstalled = Vec::new();
        for cur_id in &current_ids {
            if !target_ids.contains(cur_id) {
                engine.uninstall(cur_id, game_directory)?;
                uninstalled.push(cur_id.clone());
            }
        }

        let mut installed = Vec::new();
        for target_id in &target_ids {
            if !current_ids.contains(target_id) {
                if let Some(def) = GameFixCatalog::try_get(target_id) {
                    engine.install(def, game_directory)?;
                    installed.push(target_id.clone());
                } else {
                    return Err(Error::Refused(format!(
                        "Fix '{target_id}' required by snapshot is not available in catalog"
                    )));
                }
            }
        }

        // 2. Reconcile user.ltx
        let user_ltx_updates_count = if !snapshot.managed_user_ltx.is_empty() {
            ManagedUserLtxSettings::update_managed_settings(game_directory, &snapshot.managed_user_ltx)?
        } else {
            0
        };

        // 3. Reconcile S2 mods if applicable
        let mut s2_mods_toggled = false;
        if snapshot.game == GameTarget::Stalker2 {
            if let Some(ref target_s2_state) = snapshot.s2_mods_state {
                let current_s2_state = Stalker2ModToggle::query_status(game_directory)?;
                let needs_toggle = matches!(
                    (current_s2_state, target_s2_state.as_str()),
                    (ModToggleStatus::Active { .. }, "disabled") | (ModToggleStatus::Disabled { .. }, "active")
                );
                if needs_toggle {
                    Stalker2ModToggle::toggle(game_directory)?;
                    s2_mods_toggled = true;
                }
            }
        }

        Ok(SnapshotRestoreReport {
            snapshot_id: snapshot.id,
            installed_fixes: installed,
            uninstalled_fixes: uninstalled,
            user_ltx_updates_count,
            s2_mods_toggled,
        })
    }
}

fn check_companion_installed(game: GameTarget, game_directory: &Path) -> bool {
    if !game.is_xray() {
        return false;
    }
    // Check for companion Lua marker script
    game_directory
        .join("gamedata")
        .join("scripts")
        .join("sse_companion.script")
        .is_file()
}

fn game_target_name(target: GameTarget) -> &'static str {
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

fn serialize_snapshot(snapshot: &ToolkitSnapshot) -> String {
    let mut out = String::with_capacity(1024);
    out.push_str("{\n");
    out.push_str(&format!("  \"id\": \"{}\",\n", snapshot.id));
    out.push_str(&format!("  \"label\": \"{}\",\n", snapshot.label));
    out.push_str(&format!("  \"timestamp_epoch\": {},\n", snapshot.timestamp_epoch));
    out.push_str(&format!("  \"game\": \"{}\",\n", game_target_name(snapshot.game)));
    out.push_str("  \"installed_fixes\": [\n");
    for (i, f) in snapshot.installed_fixes.iter().enumerate() {
        let comma = if i.saturating_add(1) == snapshot.installed_fixes.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!(
            "    {{\"fix_id\": \"{}\", \"version\": \"{}\"}}{}\n",
            f.fix_id, f.version, comma
        ));
    }
    out.push_str("  ],\n");
    out.push_str("  \"managed_user_ltx\": {\n");
    let mut ltx_entries: Vec<_> = snapshot.managed_user_ltx.iter().collect();
    ltx_entries.sort_by(|a, b| a.0.cmp(b.0));
    for (i, (k, v)) in ltx_entries.iter().enumerate() {
        let comma = if i.saturating_add(1) == ltx_entries.len() {
            ""
        } else {
            ","
        };
        out.push_str(&format!("    \"{}\": \"{}\"{}\n", k, v, comma));
    }
    out.push_str("  },\n");
    if let Some(ref s2) = snapshot.s2_mods_state {
        out.push_str(&format!("  \"s2_mods_state\": \"{}\",\n", s2));
    }
    out.push_str(&format!(
        "  \"companion_installed\": {}\n",
        snapshot.companion_installed
    ));
    out.push_str("}\n");
    out
}

fn deserialize_snapshot(bytes: &[u8]) -> Result<ToolkitSnapshot> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::damaged("Invalid UTF-8 in snapshot JSON"))?;
    let json_val = sse_catalog::parse_json(text).map_err(|e| Error::damaged(format!("{e}")))?;

    let id = json_val
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::damaged("Missing 'id' in snapshot JSON"))?
        .to_string();

    let label = json_val.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();

    let timestamp_epoch = json_val.get("timestamp_epoch").and_then(|v| v.as_u64()).unwrap_or(0);

    let game_str = json_val.get("game").and_then(|v| v.as_str()).unwrap_or("soc");

    let game = match game_str {
        "cs" => GameTarget::ClearSky,
        "cop" => GameTarget::CallOfPripyat,
        "soc_ee" => GameTarget::ShadowOfChernobylEnhancedEdition,
        "cs_ee" => GameTarget::ClearSkyEnhancedEdition,
        "cop_ee" => GameTarget::CallOfPripyatEnhancedEdition,
        "s2" => GameTarget::Stalker2,
        _ => GameTarget::ShadowOfChernobyl,
    };

    let mut installed_fixes = Vec::new();
    if let Some(fixes_arr) = json_val.get("installed_fixes").and_then(|v| v.as_array()) {
        for item in fixes_arr {
            if let (Some(fix_id), Some(ver)) = (
                item.get("fix_id").and_then(|v| v.as_str()),
                item.get("version").and_then(|v| v.as_str()),
            ) {
                installed_fixes.push(InstalledFixSnapshot {
                    fix_id: fix_id.to_string(),
                    version: ver.to_string(),
                });
            }
        }
    }

    let mut managed_user_ltx = BTreeMap::new();
    if let Some(ltx_obj) = json_val.get("managed_user_ltx").and_then(|v| v.as_object()) {
        for (k, v) in ltx_obj {
            if let Some(val_str) = v.as_str() {
                managed_user_ltx.insert(k.clone(), val_str.to_string());
            }
        }
    }

    let s2_mods_state = json_val
        .get("s2_mods_state")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);

    let companion_installed = json_val
        .get("companion_installed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    Ok(ToolkitSnapshot {
        id,
        label,
        timestamp_epoch,
        game,
        installed_fixes,
        managed_user_ltx,
        s2_mods_state,
        companion_installed,
    })
}
