//! Integration tests for the game environment toolkit module.
//!
//! Tests managed `user.ltx`, S2 mod folder toggle, snapshots, profiles, and installation audits.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_fixes::models::*;
use sse_fixes::toolkit::*;
use sse_fixes::GameFixEngine;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static TOOLKIT_FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);

struct ToolkitTestFixture {
    root: PathBuf,
}

impl ToolkitTestFixture {
    fn new(game: GameTarget, build_id: &str) -> Self {
        let counter = TOOLKIT_FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "sse-toolkit-fixture-{}-{}-{}",
            std::process::id(),
            counter,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        // Write marker file
        if game.is_xray() {
            let fsgame = "; fsgame marker\n$game_data$ = false| false| $fs_root$| gamedata\\\n$app_data_root$ = true| false| $fs_root$| _appdata_\\\n";
            fs::write(temp_dir.join("fsgame.ltx"), fsgame).unwrap();
        } else {
            let paks = temp_dir.join("Stalker2").join("Content").join("Paks");
            fs::create_dir_all(paks).unwrap();
        }

        if let Some(app_id) = game.steam_app_id() {
            let acf = format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{}\"\n\t\"buildid\"\t\t\"{}\"\n}}\n",
                app_id, build_id
            );
            fs::write(temp_dir.join(format!("appmanifest_{app_id}.acf")), acf).unwrap();
        }

        Self { root: temp_dir }
    }

    fn write_file(&self, relative_path: &str, content: &[u8]) {
        let path = self.root.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn read_file_string(&self, relative_path: &str) -> String {
        fs::read_to_string(self.root.join(relative_path)).unwrap()
    }
}

impl Drop for ToolkitTestFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_synthetic_fix(id: &str, file: &str, orig: &str, repl: &str, build_id: &str) -> GameFixDefinition {
    GameFixDefinition {
        id: id.to_string(),
        game: GameTarget::ClearSky,
        version: "1.0.0".to_string(),
        title: format!("Synthetic fix {id}"),
        supported_steam_build_ids: vec![build_id.to_string()],
        category: GameFixCategory::Essential,
        maturity: GameFixMaturity::Validated,
        depends_on: vec![],
        conflicts_with: vec![],
        text_patches: vec![TextPatchOperation {
            relative_path: file.to_string(),
            expected_text: orig.to_string(),
            replacement_text: repl.to_string(),
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

#[test]
fn user_ltx_reads_and_updates_preserving_comments_and_order() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let initial_ltx = b"; User console settings\r\n\
g_fov 67.5\r\n\
hud_crosshair on\r\n\
; unmanaged key below\r\n\
bind forward kW\r\n\
mouse_sens 0.12\r\n";

    fixture.write_file("_appdata_/user.ltx", initial_ltx);

    let read = ManagedUserLtxSettings::read_managed_settings(&fixture.root).unwrap();
    assert_eq!(read.get("g_fov").map(String::as_str), Some("67.5"));
    assert_eq!(read.get("hud_crosshair").map(String::as_str), Some("on"));
    assert_eq!(read.get("mouse_sens").map(String::as_str), Some("0.12"));
    assert_eq!(read.get("bind"), None);

    let mut updates = BTreeMap::new();
    updates.insert("g_fov".to_string(), "85.0".to_string());
    updates.insert("rs_v_sync".to_string(), "on".to_string()); // new key

    let changed = ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).unwrap();
    assert_eq!(changed, 2);

    let updated_text = fixture.read_file_string("_appdata_/user.ltx");
    assert!(updated_text.contains("g_fov 85.0"));
    assert!(updated_text.contains("bind forward kW"));
    assert!(updated_text.contains("rs_v_sync on"));
    assert!(updated_text.contains("; User console settings"));

    // Check drift detection
    let mut baseline = BTreeMap::new();
    baseline.insert("g_fov".to_string(), "85.0".to_string());
    baseline.insert("rs_v_sync".to_string(), "on".to_string());
    baseline.insert("hud_crosshair".to_string(), "on".to_string());

    let report = ManagedUserLtxSettings::detect_drift(&fixture.root, &baseline).unwrap();
    assert!(report.is_clean());
}

#[test]
fn user_ltx_managed_keys_are_case_insensitive() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    fixture.write_file("_appdata_/user.ltx", b"G_FOV 67.5\r\nHUD_CROSSHAIR on\r\n");

    let read = ManagedUserLtxSettings::read_managed_settings(&fixture.root).unwrap();
    assert_eq!(read.get("g_fov").map(String::as_str), Some("67.5"));
    assert_eq!(read.get("hud_crosshair").map(String::as_str), Some("on"));

    let updates = BTreeMap::from([
        ("g_fov".to_string(), "85.0".to_string()),
        ("hud_crosshair".to_string(), "off".to_string()),
    ]);
    assert_eq!(
        ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).unwrap(),
        2
    );

    let updated_text = fixture.read_file_string("_appdata_/user.ltx");
    assert_eq!(
        updated_text.lines().filter(|line| line.starts_with("G_FOV ")).count(),
        1
    );
    assert_eq!(
        updated_text
            .lines()
            .filter(|line| line.starts_with("HUD_CROSSHAIR "))
            .count(),
        1
    );
    assert!(updated_text.contains("G_FOV 85.0"));
    assert!(updated_text.contains("HUD_CROSSHAIR off"));

    let baseline = BTreeMap::from([
        ("g_fov".to_string(), "85.0".to_string()),
        ("hud_crosshair".to_string(), "off".to_string()),
    ]);
    assert!(ManagedUserLtxSettings::detect_drift(&fixture.root, &baseline)
        .unwrap()
        .is_clean());
}

#[test]
fn user_ltx_read_refuses_managed_keys_duplicated_with_different_casing() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    fixture.write_file("_appdata_/user.ltx", b"G_FOV 67.5\ng_fov 70.0\n");

    let error = ManagedUserLtxSettings::read_managed_settings(&fixture.root).unwrap_err();

    assert!(error.to_string().contains("more than one g_fov"));
}

#[test]
fn user_ltx_update_refuses_managed_keys_duplicated_with_different_casing() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let original = b"G_FOV 67.5\ng_fov 70.0\n";
    fixture.write_file("_appdata_/user.ltx", original);
    let updates = BTreeMap::from([("g_fov".to_string(), "85.0".to_string())]);

    let error = ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).unwrap_err();

    assert!(error.to_string().contains("more than one g_fov"));
    assert_eq!(fs::read(fixture.root.join("_appdata_/user.ltx")).unwrap(), original);
}

#[test]
fn user_ltx_rejects_duplicate_managed_keys_with_different_casing() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let original = b"G_FOV 67.5\r\n";
    fixture.write_file("_appdata_/user.ltx", original);
    let updates = BTreeMap::from([
        ("G_FOV".to_string(), "80.0".to_string()),
        ("g_fov".to_string(), "90.0".to_string()),
    ]);

    let error = ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).unwrap_err();

    assert!(error.to_string().contains("provided more than once"));
    assert_eq!(fs::read(fixture.root.join("_appdata_/user.ltx")).unwrap(), original);
}

#[test]
fn fsgame_appdata_root_rejects_parent_path_escape() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let game = fixture.root.join("game");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&game).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("user.ltx"), b"g_fov 70.0\n").unwrap();

    let separator = std::path::MAIN_SEPARATOR;
    let outside_name = outside.file_name().unwrap().to_string_lossy();
    let escape = format!("..{separator}{outside_name}{separator}");
    fs::write(
        game.join("fsgame.ltx"),
        format!("$app_data_root$ = true| false| $fs_root$| {escape}\n"),
    )
    .unwrap();

    let resolved = ManagedUserLtxSettings::resolve_path(&game).unwrap();

    assert_eq!(resolved, game.join("_appdata_").join("user.ltx"));
    assert!(ManagedUserLtxSettings::read_managed_settings(&game).unwrap().is_empty());
}

#[test]
fn fsgame_appdata_root_accepts_an_in_game_custom_directory() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let game = fixture.root.join("game");
    let appdata = game.join("user-data");
    fs::create_dir_all(&appdata).unwrap();
    fs::write(appdata.join("user.ltx"), b"g_fov 72.0\n").unwrap();
    fs::write(
        game.join("fsgame.ltx"),
        "$app_data_root$ = true| false| $fs_root$| user-data/\n",
    )
    .unwrap();

    assert_eq!(
        ManagedUserLtxSettings::resolve_path(&game),
        Some(fs::canonicalize(appdata.join("user.ltx")).unwrap())
    );
    assert_eq!(
        ManagedUserLtxSettings::read_managed_settings(&game)
            .unwrap()
            .get("g_fov")
            .map(String::as_str),
        Some("72.0")
    );
}

#[cfg(windows)]
#[test]
fn fsgame_appdata_root_rejects_absolute_path_escape() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let game = fixture.root.join("game");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&game).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("user.ltx"), b"g_fov 70.0\n").unwrap();
    let outside_path = outside.to_string_lossy();
    fs::write(
        game.join("fsgame.ltx"),
        format!("$app_data_root$ = true| false| $fs_root$| {outside_path}\\\n"),
    )
    .unwrap();

    let resolved = ManagedUserLtxSettings::resolve_path(&game).unwrap();

    assert_eq!(resolved, game.join("_appdata_").join("user.ltx"));
    assert!(ManagedUserLtxSettings::read_managed_settings(&game).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn fsgame_appdata_root_rejects_symlink_escape() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let game = fixture.root.join("game");
    let outside = fixture.root.join("outside");
    fs::create_dir_all(&game).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("user.ltx"), b"g_fov 70.0\n").unwrap();
    std::os::unix::fs::symlink(&outside, game.join("custom-data")).unwrap();
    fs::write(
        game.join("fsgame.ltx"),
        "$app_data_root$ = true| false| $fs_root$| custom-data/\n",
    )
    .unwrap();

    let resolved = ManagedUserLtxSettings::resolve_path(&game).unwrap();

    assert_eq!(resolved, game.join("_appdata_").join("user.ltx"));
    assert!(ManagedUserLtxSettings::read_managed_settings(&game).unwrap().is_empty());
}

#[test]
fn user_ltx_validates_bounds_and_rejects_unmanaged_keys() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    fixture.write_file("_appdata_/user.ltx", b"g_fov 70.0\r\n");

    let mut invalid_fov = BTreeMap::new();
    invalid_fov.insert("g_fov".to_string(), "150.0".to_string()); // Max is 110.0
    let err = ManagedUserLtxSettings::update_managed_settings(&fixture.root, &invalid_fov).unwrap_err();
    assert!(err.to_string().contains("out of bounds"));

    let mut unmanaged = BTreeMap::new();
    unmanaged.insert("unknown_cheat_cmd".to_string(), "1".to_string());
    let err2 = ManagedUserLtxSettings::update_managed_settings(&fixture.root, &unmanaged).unwrap_err();
    assert!(err2.to_string().contains("not in the managed allow-list"));
}

#[test]
fn stalker2_mod_toggle_safely_renames_and_detects_status() {
    let fixture = ToolkitTestFixture::new(GameTarget::Stalker2, "1643320");
    let paks_dir = fixture.root.join("Stalker2").join("Content").join("Paks");

    // Initially not found
    let status = Stalker2ModToggle::query_status(&fixture.root).unwrap();
    assert_eq!(status, ModToggleStatus::NotFound);

    // Toggle creates empty ~mods
    let toggle1 = Stalker2ModToggle::toggle(&fixture.root).unwrap();
    assert_eq!(toggle1, ModToggleResult::CreatedActiveDirectory);
    assert_eq!(
        Stalker2ModToggle::query_status(&fixture.root).unwrap(),
        ModToggleStatus::Empty
    );

    // Add a pak file into ~mods
    let active_dir = paks_dir.join("~mods");
    fs::write(active_dir.join("UltraGraphics_P.pak"), b"pak data").unwrap();

    let status_active = Stalker2ModToggle::query_status(&fixture.root).unwrap();
    match status_active {
        ModToggleStatus::Active {
            pak_count,
            ref pak_names,
        } => {
            assert_eq!(pak_count, 1);
            assert_eq!(pak_names, &["UltraGraphics_P.pak"]);
        }
        _ => panic!("Expected Active status"),
    }

    // Toggle to disabled
    let toggle2 = Stalker2ModToggle::toggle(&fixture.root).unwrap();
    assert_eq!(toggle2, ModToggleResult::Disabled { pak_count: 1 });
    assert!(!active_dir.exists());
    assert!(paks_dir.join("~mods.disabled").exists());

    // Toggle back to enabled
    let toggle3 = Stalker2ModToggle::toggle(&fixture.root).unwrap();
    assert_eq!(toggle3, ModToggleResult::Enabled { pak_count: 1 });
    assert!(active_dir.exists());
    assert!(!paks_dir.join("~mods.disabled").exists());
}

#[test]
fn snapshot_service_creates_content_addressed_record_and_restores() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let script = "gamedata/scripts/m_quest.script";
    fixture.write_file(script, b"v = 1\n");
    fixture.write_file("_appdata_/user.ltx", b"g_fov 70.0\r\n");

    let fix = make_synthetic_fix("cs.test.snap", script, "v = 1\n", "v = 2\n", "11450472");
    let engine = GameFixEngine::with_synthetic(true);
    let catalog = sse_fixes::GameFixCatalog;

    // Take initial snapshot before fix
    let snap_initial =
        ToolkitSnapshotService::create_snapshot(&fixture.root, GameTarget::ClearSky, &engine, Some("Baseline"))
            .unwrap();
    assert!(!snap_initial.id.is_empty());
    assert!(snap_initial.installed_fixes.is_empty());

    // Install fix and update user.ltx
    engine.install(&fix, &fixture.root).unwrap();
    let mut ltx_up = BTreeMap::new();
    ltx_up.insert("g_fov".to_string(), "90.0".to_string());
    ManagedUserLtxSettings::update_managed_settings(&fixture.root, &ltx_up).unwrap();

    // Verify modified state
    assert_eq!(fixture.read_file_string(script), "v = 2\n");
    assert_eq!(engine.list_installed(&fixture.root, None).unwrap().len(), 1);

    // Take second snapshot
    let snap_after =
        ToolkitSnapshotService::create_snapshot(&fixture.root, GameTarget::ClearSky, &engine, Some("After Fix"))
            .unwrap();
    assert_eq!(snap_after.installed_fixes.len(), 1);

    // List snapshots
    let all_snaps = ToolkitSnapshotService::list_snapshots(&fixture.root).unwrap();
    assert_eq!(all_snaps.len(), 2);

    // Restore initial snapshot
    let report = ToolkitSnapshotService::restore_snapshot(&fixture.root, &engine, &catalog, &snap_initial.id).unwrap();
    assert_eq!(report.uninstalled_fixes, vec!["cs.test.snap"]);
    assert_eq!(fixture.read_file_string(script), "v = 1\n");
    assert_eq!(engine.list_installed(&fixture.root, None).unwrap().len(), 0);
    assert_eq!(
        ManagedUserLtxSettings::read_managed_settings(&fixture.root)
            .unwrap()
            .get("g_fov")
            .map(String::as_str),
        Some("70.0")
    );
}

#[test]
fn profile_service_applies_profile_with_automatic_rollback_snapshot() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let script = "gamedata/scripts/profile.script";
    fixture.write_file(script, b"val = 10\n");

    let _fix = make_synthetic_fix("cs.test.prof", script, "val = 10\n", "val = 20\n", "11450472");
    let engine = GameFixEngine::with_synthetic(true);
    let catalog = sse_fixes::GameFixCatalog;

    let mut overrides = BTreeMap::new();
    overrides.insert("mouse_sens".to_string(), "0.25".to_string());

    let profile = ToolkitProfile {
        name: "Pro Gaming".to_string(),
        description: "Tweaked settings".to_string(),
        game: GameTarget::ClearSky,
        target_fix_ids: vec![],
        user_ltx_overrides: overrides,
        s2_mods_enabled: None,
    };

    let result = ToolkitProfileService::apply_profile(&fixture.root, &profile, &engine, &catalog).unwrap();

    assert_eq!(result.profile_name, "Pro Gaming");
    assert!(!result.pre_switch_snapshot_id.is_empty());

    // Verify snapshot file was created
    let pre_snap = ToolkitSnapshotService::get_snapshot(&fixture.root, &result.pre_switch_snapshot_id).unwrap();
    assert!(pre_snap.label.contains("Pre-switch snapshot"));

    // Profile JSON serialization round-trip
    let json_text = ToolkitProfileService::serialize_profile(&profile);
    let deserialized = ToolkitProfileService::deserialize_profile(json_text.as_bytes()).unwrap();
    assert_eq!(profile, deserialized);
}

#[test]
fn install_audit_classifies_and_cleans_up_orphaned_files() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let engine = GameFixEngine::with_synthetic(true);

    // 1. Write an orphaned backup file
    fixture.write_file("gamedata/scripts/stray.script.sse-backup", b"old backup bytes");
    // 2. Write a custom user mod file
    fixture.write_file("gamedata/scripts/custom_mod.script", b"custom mod logic");

    let audit = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();

    assert_eq!(audit.orphaned_count, 1);
    assert_eq!(audit.custom_mod_count, 1);
    assert!(!audit.is_clean());

    // Clean up orphans
    let cleaned = ToolkitInstallAudit::cleanup_orphans(&fixture.root, &audit).unwrap();
    assert_eq!(cleaned, 1);

    // Verify custom mod remains untouched
    assert!(fixture.root.join("gamedata/scripts/custom_mod.script").is_file());
    assert!(!fixture.root.join("gamedata/scripts/stray.script.sse-backup").exists());

    // Re-audit is now clean of orphans
    let post_audit = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    assert_eq!(post_audit.orphaned_count, 0);
    assert_eq!(post_audit.custom_mod_count, 1);
    assert!(post_audit.is_clean());
}

#[test]
fn user_ltx_refuses_non_finite_float_values() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    fixture.write_file("_appdata_/user.ltx", b"g_fov 70.0\r\n");

    for bad in ["NaN", "nan", "inf", "-inf"] {
        let mut updates = BTreeMap::new();
        updates.insert("g_fov".to_string(), bad.to_string());
        assert!(
            ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).is_err(),
            "value {bad} must be refused"
        );
    }
    assert_eq!(
        fs::read(fixture.root.join("_appdata_/user.ltx")).unwrap(),
        b"g_fov 70.0\r\n"
    );
}

#[test]
fn user_ltx_refuses_to_rewrite_file_when_text_is_not_cp1251() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let original = "; note \u{1F600}\r\ng_fov 70.0\r\n";
    fixture.write_file("_appdata_/user.ltx", original.as_bytes());

    let mut updates = BTreeMap::new();
    updates.insert("g_fov".to_string(), "80.0".to_string());
    assert!(ManagedUserLtxSettings::update_managed_settings(&fixture.root, &updates).is_err());
    assert_eq!(
        fs::read(fixture.root.join("_appdata_/user.ltx")).unwrap(),
        original.as_bytes()
    );
}

#[test]
fn cleanup_orphans_keeps_suffix_named_user_file_recoverable() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let engine = GameFixEngine::with_synthetic(true);
    fixture.write_file("gamedata/textures/texture.dds.sse-orig", b"user copy");

    let audit = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    assert_eq!(audit.orphaned_count, 1);
    let cleaned = ToolkitInstallAudit::cleanup_orphans(&fixture.root, &audit).unwrap();
    assert_eq!(cleaned, 1);

    assert!(!fixture.root.join("gamedata/textures/texture.dds.sse-orig").exists());
    let kept = fixture
        .root
        .join(".save-editor-quarantine/gamedata/textures/texture.dds.sse-orig");
    assert_eq!(fs::read(kept).unwrap(), b"user copy");
}

#[cfg(unix)]
#[test]
fn install_audit_does_not_follow_symlink_cycles_in_gamedata() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let engine = GameFixEngine::with_synthetic(true);
    fixture.write_file("gamedata/scripts/custom_mod.script", b"custom mod logic");
    std::os::unix::fs::symlink("..", fixture.root.join("gamedata/scripts/loop")).unwrap();

    let audit = ToolkitInstallAudit::audit_installation(&fixture.root, GameTarget::ClearSky, &engine).unwrap();
    assert_eq!(audit.custom_mod_count, 1);
}

#[test]
fn profile_with_unknown_fix_leaves_installed_fixes_untouched() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let script = "gamedata/scripts/profile_keep.script";
    fixture.write_file(script, b"val = 10\n");
    let engine = GameFixEngine::with_synthetic(true);
    let keep = make_synthetic_fix("cs.test.keep", script, "val = 10\n", "val = 20\n", "11450472");
    engine.install(&keep, &fixture.root).unwrap();

    let profile = ToolkitProfile {
        name: "Unknown fix".to_string(),
        description: String::new(),
        game: GameTarget::ClearSky,
        target_fix_ids: vec!["cs.not.in.catalog".to_string()],
        user_ltx_overrides: BTreeMap::new(),
        s2_mods_enabled: None,
    };
    let catalog = sse_fixes::GameFixCatalog;
    assert!(ToolkitProfileService::apply_profile(&fixture.root, &profile, &engine, &catalog).is_err());

    let ids: Vec<String> = engine
        .list_installed(&fixture.root, None)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec!["cs.test.keep".to_string()]);
}

#[test]
fn profile_with_refused_setting_leaves_installed_fixes_untouched() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let script = "gamedata/scripts/profile_keep2.script";
    fixture.write_file(script, b"val = 10\n");
    let engine = GameFixEngine::with_synthetic(true);
    let keep = make_synthetic_fix("cs.test.keep2", script, "val = 10\n", "val = 20\n", "11450472");
    engine.install(&keep, &fixture.root).unwrap();

    let mut overrides = BTreeMap::new();
    overrides.insert("unknown_cheat_cmd".to_string(), "1".to_string());
    let profile = ToolkitProfile {
        name: "Bad setting".to_string(),
        description: String::new(),
        game: GameTarget::ClearSky,
        target_fix_ids: vec![],
        user_ltx_overrides: overrides,
        s2_mods_enabled: None,
    };
    let catalog = sse_fixes::GameFixCatalog;
    assert!(ToolkitProfileService::apply_profile(&fixture.root, &profile, &engine, &catalog).is_err());

    let ids: Vec<String> = engine
        .list_installed(&fixture.root, None)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec!["cs.test.keep2".to_string()]);
}

#[test]
fn profile_rejects_unknown_game_instead_of_defaulting_to_shadow() {
    let text = br#"{"name":"Typo","game":"clearsky"}"#;
    assert!(ToolkitProfileService::deserialize_profile(text).is_err());
}

struct S2RunningProbe;

impl sse_fixes::running_game::GameRunningProbe for S2RunningProbe {
    fn is_game_running(&self, _game: GameTarget) -> sse_core::Result<bool> {
        Ok(true)
    }
}

#[test]
fn stalker2_mod_toggle_is_refused_while_the_game_is_running() {
    let fixture = ToolkitTestFixture::new(GameTarget::Stalker2, "1643320");
    let paks_dir = fixture.root.join("Stalker2").join("Content").join("Paks");

    fs::create_dir_all(paks_dir.join("~mods")).unwrap();
    fs::write(paks_dir.join("~mods").join("a.pak"), b"mod").unwrap();

    let result = Stalker2ModToggle::toggle_while_not_running(&fixture.root, &S2RunningProbe);
    assert!(result.is_err());
    assert!(paks_dir.join("~mods").join("a.pak").is_file());
    assert!(!paks_dir.join("~mods.disabled").exists());
}

#[test]
fn failed_profile_switch_keeps_the_previously_installed_fix() {
    let fixture = ToolkitTestFixture::new(GameTarget::ClearSky, "11450472");
    let script = "gamedata/scripts/keep_switch.script";
    fixture.write_file(script, b"val = 10\n");
    let engine = GameFixEngine::with_synthetic(true);
    let keep = make_synthetic_fix("cs.test.keep4", script, "val = 10\n", "val = 20\n", "11450472");
    engine.install(&keep, &fixture.root).unwrap();

    // A catalog fix cannot install on this fixture (no game files), so the switch must fail.
    let catalog_id = sse_fixes::GameFixCatalog::for_game(GameTarget::ClearSky)
        .first()
        .map(|definition| definition.id.clone())
        .expect("ClearSky catalog has fixes");
    let profile = ToolkitProfile {
        name: "Switch".to_string(),
        description: String::new(),
        game: GameTarget::ClearSky,
        target_fix_ids: vec![catalog_id],
        user_ltx_overrides: BTreeMap::new(),
        s2_mods_enabled: None,
    };
    let catalog = sse_fixes::GameFixCatalog;
    assert!(ToolkitProfileService::apply_profile(&fixture.root, &profile, &engine, &catalog).is_err());

    let ids: Vec<String> = engine
        .list_installed(&fixture.root, None)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(ids, vec!["cs.test.keep4".to_string()]);
}
