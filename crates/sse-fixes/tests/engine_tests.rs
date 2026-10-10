//! Integration tests for the GameFixEngine and AllSpawnEditor.

// Tests use unwrap/expect to fail fast and index into known-length collections.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_fixes::*;
use std::fs;
use std::path::PathBuf;

use std::sync::atomic::{AtomicU64, Ordering};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(1);

struct TestFixture {
    root: PathBuf,
}

impl TestFixture {
    fn new(game: GameTarget, build_id: &str) -> Self {
        let counter = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_dir = std::env::temp_dir().join(format!(
            "sse-test-fixture-{}-{}-{}",
            std::process::id(),
            counter,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        // Write fsgame.ltx marker for CS / SoC / CoP
        let fsgame_content = "; fsgame marker\n$game_data$ = false| false| $fs_root$| gamedata\\\n";
        fs::write(temp_dir.join("fsgame.ltx"), fsgame_content).unwrap();

        // Write Steam appmanifest
        if let Some(app_id) = game.steam_app_id() {
            let acf_content = format!(
                "\"AppState\"\n{{\n\t\"appid\"\t\t\"{}\"\n\t\"buildid\"\t\t\"{}\"\n}}\n",
                app_id, build_id
            );
            fs::write(temp_dir.join(format!("appmanifest_{}.acf", app_id)), acf_content).unwrap();
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

    fn read_file(&self, relative_path: &str) -> Vec<u8> {
        fs::read(self.root.join(relative_path)).unwrap()
    }

    fn read_file_string(&self, relative_path: &str) -> String {
        fs::read_to_string(self.root.join(relative_path)).unwrap()
    }
}

impl Drop for TestFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn make_test_definition(
    id: &str,
    relative_path: &str,
    expected: &str,
    replacement: &str,
    build_id: &str,
) -> GameFixDefinition {
    GameFixDefinition {
        id: id.to_string(),
        game: GameTarget::ClearSky,
        version: "1.0.0".to_string(),
        title: "Test Fix".to_string(),
        supported_steam_build_ids: vec![build_id.to_string()],
        category: GameFixCategory::Essential,
        maturity: GameFixMaturity::Validated,
        depends_on: vec![],
        conflicts_with: vec![],
        text_patches: vec![TextPatchOperation {
            relative_path: relative_path.to_string(),
            expected_text: expected.to_string(),
            replacement_text: replacement.to_string(),
            code_page: 28591,
            expected_file_sha256: None,
            retail_only: false,
        }],
        source: "unit test".to_string(),
        problem: "problem".to_string(),
        description: "description".to_string(),
        implementation: GameFixImplementationType::ExactTextReplacement,
        requires_new_game: false,
        save_compatibility: GameFixSaveCompatibility::ExistingSaves,
        verification_state: GameFixVerificationState::RetailFilesVerified,
        detection_method: "method".to_string(),
        references: vec![],
        overlays: vec![],
        spawn_edits: vec![],
    }
}

/// Minimal synthetic Clear Sky `all.spawn` binary builder.
mod synthetic_all_spawn {
    fn chunk(id: u32, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + data.len());
        out.extend_from_slice(&id.to_le_bytes());
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    fn prefix16(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(2 + data.len());
        out.extend_from_slice(&(data.len() as u16).to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    pub fn build(custom_data: &str, path_name: &str, point_name: &str, level_vertex: u32) -> Vec<u8> {
        // State chunk
        let mut state = Vec::new();
        state.extend_from_slice(&0u16.to_le_bytes()); // graph
        state.extend_from_slice(&0.0f32.to_le_bytes()); // distance
        state.extend_from_slice(&0u32.to_le_bytes()); // direct control
        state.extend_from_slice(&0u32.to_le_bytes()); // node
        state.extend_from_slice(&0u32.to_le_bytes()); // flags
        state.extend_from_slice(custom_data.as_bytes());
        state.push(0); // null terminator
        state.extend_from_slice(&u32::MAX.to_le_bytes());
        state.extend_from_slice(&u32::MAX.to_le_bytes());
        state.push(42); // byte 42

        // Packet
        let mut packet = Vec::new();
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.extend_from_slice(b"stalker\0npc_1\0");
        packet.push(0);
        packet.push(0xFE);
        packet.extend_from_slice(&[0u8; 24]);
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.extend_from_slice(&u16::MAX.to_le_bytes());
        packet.extend_from_slice(&u16::MAX.to_le_bytes());
        packet.extend_from_slice(&0x21u16.to_le_bytes());
        packet.extend_from_slice(&124u16.to_le_bytes());
        packet.extend_from_slice(&0xFFFFu16.to_le_bytes());
        packet.extend_from_slice(&8u16.to_le_bytes());
        packet.extend_from_slice(&0u16.to_le_bytes());
        packet.extend_from_slice(&1u16.to_le_bytes());
        packet.extend_from_slice(&((state.len() + 2) as u16).to_le_bytes());
        packet.extend_from_slice(&state);

        let spawn_packet = prefix16(&packet);
        let mut obj_inner = chunk(0, &spawn_packet);
        obj_inner.extend_from_slice(&chunk(1, &[0, 0]));
        let mut obj = chunk(0, &0u16.to_le_bytes());
        obj.extend_from_slice(&chunk(1, &obj_inner));

        let mut objects = chunk(0, &1u32.to_le_bytes());
        objects.extend_from_slice(&chunk(1, &chunk(0, &obj)));
        objects.extend_from_slice(&chunk(2, &[]));

        // Patrol point
        let mut point = Vec::new();
        point.extend_from_slice(point_name.as_bytes());
        point.push(0);
        point.extend_from_slice(&1.0f32.to_le_bytes());
        point.extend_from_slice(&2.0f32.to_le_bytes());
        point.extend_from_slice(&3.0f32.to_le_bytes());
        point.extend_from_slice(&0u32.to_le_bytes());
        point.extend_from_slice(&level_vertex.to_le_bytes());
        point.extend_from_slice(&9u16.to_le_bytes());

        let mut vertex = chunk(0, &0u32.to_le_bytes());
        vertex.extend_from_slice(&chunk(1, &point));

        let mut graph = chunk(0, &1u32.to_le_bytes());
        graph.extend_from_slice(&chunk(1, &chunk(0, &vertex)));
        graph.extend_from_slice(&chunk(2, &[]));

        let mut path_data = Vec::new();
        path_data.extend_from_slice(path_name.as_bytes());
        path_data.push(0);
        let mut path = chunk(0, &path_data);
        path.extend_from_slice(&chunk(1, &graph));

        let mut patrols = chunk(0, &1u32.to_le_bytes());
        patrols.extend_from_slice(&chunk(1, &chunk(0, &path)));

        let mut out = chunk(0, &[0u8; 44]);
        out.extend_from_slice(&chunk(1, &objects));
        out.extend_from_slice(&chunk(3, &patrols));
        out
    }
}

#[test]
fn install_is_idempotent_and_uninstall_restores_the_exact_original_bytes() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    let original = "local state = 1\n";
    fixture.write_file(relative_path, original.as_bytes());

    let definition = make_test_definition(
        "cs.test.safe",
        relative_path,
        "local state = 1\n",
        "local state = 2\n",
        "11450472",
    );

    let engine = GameFixEngine::with_synthetic(true);

    let installed = engine.install(&definition, &fixture.root).unwrap();
    assert!(installed.changed);
    assert_eq!(fixture.read_file_string(relative_path), "local state = 2\n");
    assert_eq!(
        engine.get_status(&definition, &fixture.root).unwrap(),
        GameFixState::Installed
    );

    // Repeated install should not change
    let repeated = engine.install(&definition, &fixture.root).unwrap();
    assert!(!repeated.changed);
    assert_eq!(repeated.state, GameFixState::Installed);

    // Status and list installed should report it installed
    let installed_list = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(installed_list.len(), 1);
    assert_eq!(installed_list[0].id, definition.id);
    assert_eq!(
        engine.get_status(&definition, &fixture.root).unwrap(),
        GameFixState::Installed
    );

    // Uninstall
    let removed = engine.uninstall(&definition.id, &fixture.root).unwrap();
    assert!(removed.changed);
    assert_eq!(fixture.read_file_string(relative_path), original);
    assert_eq!(
        engine.get_status(&definition, &fixture.root).unwrap(),
        GameFixState::Removed
    );
}

#[test]
fn spawn_edits_rename_a_waypoint_and_change_custom_data_and_uninstall_restores_exact_bytes() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let original = synthetic_all_spawn::build("[logic]\nactive = walker\n", "path_walk", "name00a=guard", 5);
    let path = "gamedata/spawns/all.spawn";
    fixture.write_file(path, &original);

    let mut definition = make_test_definition("cs.test.spawn", path, "", "", "11450472");
    definition.text_patches.clear();
    definition.implementation = GameFixImplementationType::Structured;
    definition.spawn_edits = vec![
        SpawnEditOperation {
            relative_path: path.to_string(),
            kind: SpawnEditKind::PatrolPoint,
            target: "path_walk".to_string(),
            point: 0,
            expected: "name00a=guard".to_string(),
            replacement: Some("name00|a=guard".to_string()),
            position: None,
            level_vertex_id: Some(7),
            game_vertex_id: None,
            expected_file_sha256: None,
        },
        SpawnEditOperation {
            relative_path: path.to_string(),
            kind: SpawnEditKind::CustomData,
            target: "npc_1".to_string(),
            point: 0,
            expected: "[logic]\nactive = walker\n".to_string(),
            replacement: Some("[logic]\nactive = walker@longer\n".to_string()),
            position: None,
            level_vertex_id: None,
            game_vertex_id: None,
            expected_file_sha256: None,
        },
    ];

    let engine = GameFixEngine::with_synthetic(true);
    let install_res = engine.install(&definition, &fixture.root).unwrap();
    assert!(install_res.changed);

    let patched = fixture.read_file(path);
    let expected_patched =
        synthetic_all_spawn::build("[logic]\nactive = walker@longer\n", "path_walk", "name00|a=guard", 7);
    assert_eq!(patched, expected_patched);

    let uninstalled = engine.uninstall(&definition.id, &fixture.root).unwrap();
    assert_eq!(uninstalled.state, GameFixState::Removed);
    assert_eq!(fixture.read_file(path), original);
}

#[test]
fn missing_or_ambiguous_text_anchor_is_rejected_without_partial_writes() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    let original = "local state = 1\nlocal state = 1\n";
    fixture.write_file(relative_path, original.as_bytes());

    let definition = make_test_definition(
        "cs.test.anchor",
        relative_path,
        "local state = 1\n",
        "local state = 2\n",
        "11450472",
    );

    let engine = GameFixEngine::with_synthetic(true);
    let result = engine.install(&definition, &fixture.root);
    assert!(result.is_err());
    assert_eq!(fixture.read_file_string(relative_path), original);
}

#[test]
fn rollback_restores_all_files_when_a_later_patch_fails() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let file1 = "gamedata/scripts/first.script";
    let file2 = "gamedata/scripts/second.script";
    fixture.write_file(file1, b"first = 1\n");
    fixture.write_file(file2, b"second = 1\n");

    let mut definition = make_test_definition("cs.test.rollback", file1, "first = 1\n", "first = 2\n", "11450472");
    definition.text_patches.push(TextPatchOperation {
        relative_path: file2.to_string(),
        expected_text: "nonexistent = 1\n".to_string(),
        replacement_text: "second = 2\n".to_string(),
        code_page: 28591,
        expected_file_sha256: None,
        retail_only: false,
    });

    let engine = GameFixEngine::with_synthetic(true);
    let err = engine.install(&definition, &fixture.root);
    assert!(err.is_err());

    // File 1 must have been rolled back to original
    assert_eq!(fixture.read_file_string(file1), "first = 1\n");
    assert_eq!(fixture.read_file_string(file2), "second = 1\n");
    assert!(engine.list_installed(&fixture.root, None).unwrap().is_empty());
}

#[test]
fn text_patch_round_trips_explicit_cp1251_text_and_restores_original_bytes() {
    let fixture = TestFixture::new(GameTarget::CallOfPripyat, "11450453");
    let relative_path = "gamedata/configs/text/rus/st_dialogs.xml";
    let original = "<text>Неправильное описание артефакта</text>\r\n";
    let replacement = "<text>Исправленное описание артефакта</text>\r\n";

    let original_bytes = encode_patch_text(original, 1251).unwrap();
    let replacement_bytes = encode_patch_text(replacement, 1251).unwrap();

    fixture.write_file(relative_path, &original_bytes);

    let mut definition = make_test_definition("cop.test.cp1251", relative_path, original, replacement, "11450453");
    definition.game = GameTarget::CallOfPripyat;
    definition.text_patches[0].code_page = 1251;

    let engine = GameFixEngine::with_synthetic(true);
    let res = engine.install(&definition, &fixture.root).unwrap();
    assert_eq!(res.state, GameFixState::Installed);
    assert_eq!(fixture.read_file(relative_path), replacement_bytes);

    let uninst = engine.uninstall(&definition.id, &fixture.root).unwrap();
    assert_eq!(uninst.state, GameFixState::Removed);
    assert_eq!(fixture.read_file(relative_path), original_bytes);
}

#[test]
fn unsupported_build_is_rejected_before_any_file_or_manifest_change() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    fixture.write_file(relative_path, b"local state = 1\n");

    let definition = make_test_definition(
        "cs.test.build",
        relative_path,
        "local state = 1\n",
        "local state = 2\n",
        "99999999",
    );

    let engine = GameFixEngine::with_synthetic(true);
    let result = engine.install(&definition, &fixture.root);
    assert!(result.is_err());
    assert_eq!(fixture.read_file_string(relative_path), "local state = 1\n");
    assert_eq!(
        engine.get_status(&definition, &fixture.root).unwrap(),
        GameFixState::NotInstalled
    );
}

#[test]
fn interrupted_transaction_is_recovered_to_original_state() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    let original = "local state = 1\n";
    fixture.write_file(relative_path, original.as_bytes());

    // Create an interrupted transaction directory
    let state_dir = fixture.root.join(".save-editor-game-fixes").join("cs.test.interrupted");
    let backup_dir = state_dir.join("backups");
    fs::create_dir_all(&backup_dir).unwrap();

    let backup_file = backup_dir.join("file-0000.before");
    fs::write(&backup_file, original.as_bytes()).unwrap();

    let before_sha = sse_codecs::sha256::sha256_hex(original.as_bytes());
    let after_bytes = b"local state = corrupted\n";
    let after_sha = sse_codecs::sha256::sha256_hex(after_bytes);

    let journal_json = format!(
        r#"{{
  "schemaVersion": 1,
  "kind": "install",
  "freshState": true,
  "files": [
    {{
      "relativePath": "gamedata/scripts/task.script",
      "beforeSha256": "{before_sha}",
      "afterSha256": "{after_sha}",
      "backupPath": "backups/file-0000.before",
      "targetExistedBefore": true
    }}
  ]
}}"#
    );
    fs::write(state_dir.join("transaction.json"), journal_json).unwrap();

    // Now modify the game file to simulate partial write
    fixture.write_file(relative_path, after_bytes);

    let engine = GameFixEngine::with_synthetic(true);
    let recovered = engine.recover_interrupted(&fixture.root).unwrap();
    assert_eq!(recovered, vec!["cs.test.interrupted"]);

    // File should be restored from backup!
    assert_eq!(fixture.read_file_string(relative_path), original);
    assert!(!state_dir.exists());
}

#[test]
fn update_reapplies_new_version_and_updates_manifest() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    fixture.write_file(relative_path, b"local state = 1\n");

    let first = make_test_definition(
        "cs.test.update",
        relative_path,
        "local state = 1\n",
        "local state = 2\n",
        "11450472",
    );
    let mut second = make_test_definition(
        "cs.test.update",
        relative_path,
        "local state = 1\n",
        "local state = 3\n",
        "11450472",
    );
    second.version = "2.0.0".to_string();

    let engine = GameFixEngine::with_synthetic(true);
    let installed = engine.install(&first, &fixture.root).unwrap();
    assert!(installed.changed);
    assert_eq!(fixture.read_file_string(relative_path), "local state = 2\n");

    let updated = engine.update(&second, &fixture.root).unwrap();
    assert!(updated.changed);
    assert_eq!(fixture.read_file_string(relative_path), "local state = 3\n");

    let list = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].version, "2.0.0");

    let removed = engine.uninstall(&second.id, &fixture.root).unwrap();
    assert!(removed.changed);
    assert_eq!(fixture.read_file_string(relative_path), "local state = 1\n");
}

#[test]
fn uninstall_refuses_to_restore_over_a_file_modified_after_install() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/task.script";
    fixture.write_file(relative_path, b"local state = 1\n");

    let definition = make_test_definition(
        "cs.test.drift",
        relative_path,
        "local state = 1\n",
        "local state = 2\n",
        "11450472",
    );
    let engine = GameFixEngine::with_synthetic(true);

    engine.install(&definition, &fixture.root).unwrap();

    // User or external tool changes file
    fixture.write_file(relative_path, b"external modification\n");

    let err = engine.uninstall(&definition.id, &fixture.root);
    assert!(err.is_err());
    assert_eq!(
        engine.get_status(&definition, &fixture.root).unwrap(),
        GameFixState::Modified
    );
}

#[test]
fn preset_batch_applies_multiple_fixes() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let file1 = "gamedata/scripts/first.script";
    let file2 = "gamedata/scripts/second.script";
    fixture.write_file(file1, b"first = 1\n");
    fixture.write_file(file2, b"second = 1\n");

    let fix1 = make_test_definition("cs.test.p1", file1, "first = 1\n", "first = 2\n", "11450472");
    let fix2 = make_test_definition("cs.test.p2", file2, "second = 1\n", "second = 2\n", "11450472");

    let engine = GameFixEngine::with_synthetic(true);
    let result = engine
        .apply_fixes(
            GameTarget::ClearSky,
            GameFixPreset::Recommended,
            &[&fix1, &fix2],
            &fixture.root,
        )
        .unwrap();

    assert_eq!(result.selected_fix_count, 2);
    assert_eq!(result.installed_fix_ids.len(), 2);
    assert!(result.already_installed_fix_ids.is_empty());
    assert!(result.changed());

    assert_eq!(fixture.read_file_string(file1), "first = 2\n");
    assert_eq!(fixture.read_file_string(file2), "second = 2\n");

    let installed = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(installed.len(), 2);
}

#[test]
fn all_spawn_refuses_waypoint_name_outside_latin1_instead_of_writing_question_marks() {
    let original = synthetic_all_spawn::build("[logic]\nactive = walker\n", "path_walk", "name00a=guard", 5);
    let edit = SpawnEditOperation {
        relative_path: "gamedata/spawns/all.spawn".to_string(),
        kind: SpawnEditKind::PatrolPoint,
        target: "path_walk".to_string(),
        expected_file_sha256: None,
        point: 0,
        expected: "name00a=guard".to_string(),
        replacement: Some("Точка=guard".to_string()),
        position: None,
        level_vertex_id: Some(7),
        game_vertex_id: None,
    };
    assert!(AllSpawnEditor::apply(&original, &[edit]).is_err());
}

#[test]
fn all_spawn_refuses_custom_data_outside_latin1_instead_of_writing_question_marks() {
    let original = synthetic_all_spawn::build("[logic]\nactive = walker\n", "path_walk", "name00a=guard", 5);
    let edit = SpawnEditOperation {
        relative_path: "gamedata/spawns/all.spawn".to_string(),
        kind: SpawnEditKind::CustomData,
        target: "npc_1".to_string(),
        expected_file_sha256: None,
        point: 0,
        expected: "[logic]\nactive = walker\n".to_string(),
        replacement: Some("[logic]\nactive = Ёжик\n".to_string()),
        position: None,
        level_vertex_id: None,
        game_vertex_id: None,
    };
    assert!(AllSpawnEditor::apply(&original, &[edit]).is_err());
}

// A quote is not a legal file name on Windows, so this path-based check runs on Unix only.
#[cfg(unix)]
#[test]
fn manifest_with_quote_in_relative_path_is_written_as_valid_json() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/q\"uote.script";
    fixture.write_file(relative_path, b"quote = 1\n");
    let definition = make_test_definition("cs.test.quote", relative_path, "quote = 1\n", "quote = 2\n", "11450472");

    let engine = GameFixEngine::with_synthetic(true);
    engine.install(&definition, &fixture.root).unwrap();

    let installed = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "cs.test.quote");
}

#[test]
fn manifest_with_quote_in_version_is_written_as_valid_json() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/version.script";
    fixture.write_file(relative_path, b"version = 1\n");
    let mut definition = make_test_definition(
        "cs.test.version",
        relative_path,
        "version = 1\n",
        "version = 2\n",
        "11450472",
    );
    definition.version = "1.0\"quote".to_string();

    let engine = GameFixEngine::with_synthetic(true);
    engine.install(&definition, &fixture.root).unwrap();

    let installed = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "cs.test.version");
}

struct StubProbe(Result<bool, &'static str>);

impl sse_fixes::running_game::GameRunningProbe for StubProbe {
    fn is_game_running(&self, _game: GameTarget) -> sse_core::Result<bool> {
        match self.0 {
            Ok(running) => Ok(running),
            Err(message) => Err(sse_core::Error::System(message.to_string())),
        }
    }
}

#[test]
fn install_is_refused_while_the_game_is_running_and_leaves_files_untouched() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/running.script";
    fixture.write_file(relative_path, b"run = 1\n");
    let definition = make_test_definition("cs.test.running", relative_path, "run = 1\n", "run = 2\n", "11450472");

    let engine = GameFixEngine::with_synthetic(true).with_process_probe(std::sync::Arc::new(StubProbe(Ok(true))));
    assert!(engine.install(&definition, &fixture.root).is_err());
    assert_eq!(fixture.read_file_string(relative_path), "run = 1\n");
    assert!(engine.list_installed(&fixture.root, None).unwrap().is_empty());
}

#[test]
fn uninstall_is_refused_while_the_game_is_running_and_keeps_the_fix() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/keep.script";
    fixture.write_file(relative_path, b"keep = 1\n");
    let definition = make_test_definition("cs.test.keep", relative_path, "keep = 1\n", "keep = 2\n", "11450472");

    let installer = GameFixEngine::with_synthetic(true);
    installer.install(&definition, &fixture.root).unwrap();

    let engine = GameFixEngine::with_synthetic(true).with_process_probe(std::sync::Arc::new(StubProbe(Ok(true))));
    assert!(engine.uninstall("cs.test.keep", &fixture.root).is_err());
    assert_eq!(fixture.read_file_string(relative_path), "keep = 2\n");
    assert_eq!(engine.list_installed(&fixture.root, None).unwrap().len(), 1);
}

#[test]
fn install_is_refused_when_the_process_list_cannot_be_read() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let relative_path = "gamedata/scripts/unknown.script";
    fixture.write_file(relative_path, b"unknown = 1\n");
    let definition = make_test_definition(
        "cs.test.unknown",
        relative_path,
        "unknown = 1\n",
        "unknown = 2\n",
        "11450472",
    );

    let engine =
        GameFixEngine::with_synthetic(true).with_process_probe(std::sync::Arc::new(StubProbe(Err("access denied"))));
    assert!(engine.install(&definition, &fixture.root).is_err());
    assert_eq!(fixture.read_file_string(relative_path), "unknown = 1\n");
}

#[test]
fn game_process_names_match_their_game_only() {
    use sse_fixes::running_game::game_process_matches;
    assert!(game_process_matches(
        GameTarget::ShadowOfChernobyl,
        "C:\\Games\\XR_3DA.exe"
    ));
    assert!(game_process_matches(GameTarget::ClearSky, "xrEngine.exe"));
    assert!(game_process_matches(
        GameTarget::CallOfPripyatEnhancedEdition,
        "xrengine.exe"
    ));
    assert!(game_process_matches(
        GameTarget::Stalker2,
        "Stalker2-Win64-Shipping.exe"
    ));
    assert!(!game_process_matches(GameTarget::ClearSky, "XR_3DA.exe"));
    assert!(!game_process_matches(GameTarget::Stalker2, "xrEngine.exe"));
}

#[test]
fn native_linux_game_process_names_match_their_game() {
    use sse_fixes::running_game::game_process_matches;
    // Native Linux builds run without the .exe suffix; the name is the engine executable's name.
    assert!(game_process_matches(GameTarget::ShadowOfChernobyl, "xr_3da"));
    assert!(game_process_matches(GameTarget::ClearSky, "/opt/stalker/xrEngine"));
    assert!(!game_process_matches(GameTarget::ClearSky, "xr_3da"));
}

/// Reports a running game from the `running_from`-th check on (zero-based), so a fault can land on a later step.
struct RunningFromCheck {
    checks: std::sync::atomic::AtomicUsize,
    running_from: usize,
}

impl sse_fixes::running_game::GameRunningProbe for RunningFromCheck {
    fn is_game_running(&self, _game: GameTarget) -> sse_core::Result<bool> {
        let index = self.checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(index >= self.running_from)
    }
}

#[test]
fn failed_preset_reports_fixes_that_could_not_be_rolled_back() {
    let fixture = TestFixture::new(GameTarget::ClearSky, "11450472");
    let file1 = "gamedata/scripts/first.script";
    let file2 = "gamedata/scripts/second.script";
    fixture.write_file(file1, b"first = 1\n");
    fixture.write_file(file2, b"second = 1\n");

    let fix1 = make_test_definition("cs.test.p1", file1, "first = 1\n", "first = 2\n", "11450472");
    let fix2 = make_test_definition("cs.test.p2", file2, "second = 1\n", "second = 2\n", "11450472");

    // Check 0 lets fix 1 install; check 1 (fix 2's install) and check 2 (rollback of fix 1) see a running game.
    let probe = RunningFromCheck {
        checks: std::sync::atomic::AtomicUsize::new(0),
        running_from: 1,
    };
    let engine = GameFixEngine::with_synthetic(true).with_process_probe(std::sync::Arc::new(probe));
    let error = engine
        .apply_fixes(
            GameTarget::ClearSky,
            GameFixPreset::Recommended,
            &[&fix1, &fix2],
            &fixture.root,
        )
        .unwrap_err()
        .to_string();

    assert!(error.contains("still installed"), "{error}");
    assert!(error.contains("cs.test.p1"), "{error}");
    let installed = engine.list_installed(&fixture.root, None).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].id, "cs.test.p1");
}
