//! Failed profile switches and failed restores: the game must come back to the state before the change.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::GameFixEngine;
use crate::models::{
    GameFixCategory, GameFixDefinition, GameFixImplementationType, GameFixMaturity, GameFixSaveCompatibility,
    GameFixVerificationState, GameTarget, TextPatchOperation,
};
use crate::toolkit::profile::{ToolkitProfile, ToolkitProfileService};
use crate::toolkit::snapshot::ToolkitSnapshotService;

struct Install {
    root: PathBuf,
}

impl Install {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("sse-rollback-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("_appdata_")).unwrap();
        fs::create_dir_all(root.join("gamedata/scripts")).unwrap();
        // The same markers the toolkit tests write for a Clear Sky install.
        fs::write(
            root.join("fsgame.ltx"),
            "; fsgame marker\n$game_data$ = false| false| $fs_root$| gamedata\\\n$app_data_root$ = true| false| $fs_root$| _appdata_\\\n",
        )
        .unwrap();
        fs::write(
            root.join("appmanifest_20510.acf"),
            "\"AppState\"\n{\n\t\"appid\"\t\t\"20510\"\n\t\"buildid\"\t\t\"11450472\"\n}\n",
        )
        .unwrap();
        fs::write(root.join("gamedata/scripts/keep.script"), b"val = 10\n").unwrap();
        Self { root }
    }
}

impl Drop for Install {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn synthetic_fix() -> GameFixDefinition {
    GameFixDefinition {
        id: "cs.test.rollback".to_string(),
        game: GameTarget::ClearSky,
        version: "1.0.0".to_string(),
        title: "Synthetic rollback fix".to_string(),
        supported_steam_build_ids: vec!["11450472".to_string()],
        category: GameFixCategory::Essential,
        maturity: GameFixMaturity::Validated,
        depends_on: vec![],
        conflicts_with: vec![],
        text_patches: vec![TextPatchOperation {
            relative_path: "gamedata/scripts/keep.script".to_string(),
            expected_text: "val = 10\n".to_string(),
            replacement_text: "val = 20\n".to_string(),
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

/// The game files as they are on disk, excluding the snapshot folder that the switch itself creates.
fn game_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let relative = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            if relative == ".sse" {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn installed_ids(engine: &GameFixEngine, root: &Path) -> Vec<String> {
    engine
        .list_installed(root, None)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect()
}

fn profile(overrides: &[(&str, &str)]) -> ToolkitProfile {
    let user_ltx_overrides: BTreeMap<String, String> =
        overrides.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    ToolkitProfile {
        name: "Rollback".to_string(),
        description: String::new(),
        game: GameTarget::ClearSky,
        target_fix_ids: Vec::new(),
        user_ltx_overrides,
        s2_mods_enabled: None,
    }
}

/// Puts a folder where `user.ltx` must be written: any write to that path fails on every platform.
fn block_settings(root: &Path) {
    let path = root.join("_appdata_").join("user.ltx");
    let _ = fs::remove_file(&path);
    fs::create_dir_all(&path).unwrap();
}

fn unblock_settings(root: &Path) {
    let _ = fs::remove_dir(root.join("_appdata_").join("user.ltx"));
}

#[test]
fn failed_settings_step_restores_the_installed_fix_and_the_files() {
    let install = Install::new("success");
    let engine = GameFixEngine::with_synthetic(true);
    let fix = synthetic_fix();
    engine.install(&fix, &install.root).unwrap();
    // No user.ltx yet: the snapshot holds no managed values, and the settings folder is read-only, so creating
    // user.ltx in the settings step fails.
    let before = game_files(&install.root);
    let definitions = [fix.clone()];
    let resolve = |id: &str| definitions.iter().find(|d| d.id == id);

    block_settings(&install.root);
    let result =
        ToolkitProfileService::apply_profile_with(&install.root, &profile(&[("mouse_sens", "2")]), &engine, &resolve);
    unblock_settings(&install.root);
    let message = result.unwrap_err().to_string();

    assert!(
        message.contains("was not applied and the game was returned"),
        "{message}"
    );
    assert_eq!(
        installed_ids(&engine, &install.root),
        vec!["cs.test.rollback".to_string()]
    );
    assert_eq!(
        game_files(&install.root),
        before,
        "the game files must match the state before the switch"
    );
}

#[test]
fn failed_rollback_says_what_was_applied_and_where_the_snapshot_is() {
    let install = Install::new("rollback-fails");
    let engine = GameFixEngine::with_synthetic(true);
    let fix = synthetic_fix();
    engine.install(&fix, &install.root).unwrap();
    let definitions = [fix.clone()];
    let resolve = |id: &str| definitions.iter().find(|d| d.id == id);
    // A rollback to a snapshot that does not exist cannot run: the error must name the snapshot and the folder.
    let error = sse_core::Error::Refused("settings step failed".to_string());
    let message =
        ToolkitSnapshotService::rollback_failed(&install.root, &engine, &resolve, "no-such-snapshot", error, "Profile")
            .to_string();

    assert!(message.contains("also failed"), "{message}");
    assert!(message.contains("no-such-snapshot"), "{message}");
    assert!(message.contains("restore it by hand"), "{message}");
    assert!(message.contains(".sse"), "the snapshot folder must be named: {message}");
    // The failed rollback does not change the installed fix.
    assert_eq!(
        installed_ids(&engine, &install.root),
        vec!["cs.test.rollback".to_string()]
    );
}

#[test]
fn failed_restore_puts_back_the_state_before_the_restore() {
    let install = Install::new("restore");
    let engine = GameFixEngine::with_synthetic(true);
    let fix = synthetic_fix();
    // The snapshot holds the fix installed and a managed value of 1.
    engine.install(&fix, &install.root).unwrap();
    fs::write(install.root.join("_appdata_/user.ltx"), b"mouse_sens = 1\n").unwrap();
    let snapshot =
        ToolkitSnapshotService::create_snapshot(&install.root, GameTarget::ClearSky, &engine, Some("keep")).unwrap();
    // The state now: the fix is removed and no user.ltx file exists. The restore must write the managed value
    // back, and the settings path is blocked, so that write fails. The rollback then has nothing to write.
    engine.uninstall(&fix.id, &install.root).unwrap();
    fs::remove_file(install.root.join("_appdata_/user.ltx")).unwrap();
    block_settings(&install.root);
    let before = game_files(&install.root);
    let definitions = [fix.clone()];
    let resolve = |id: &str| definitions.iter().find(|d| d.id == id);

    let result = ToolkitSnapshotService::restore_snapshot_with(&install.root, &engine, &resolve, &snapshot.id);
    unblock_settings(&install.root);

    assert!(result.is_err(), "the blocked settings path must fail the restore");
    assert_eq!(
        game_files(&install.root),
        before,
        "the failed restore must put back the state before it"
    );
}
