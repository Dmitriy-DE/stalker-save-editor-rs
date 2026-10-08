//! Contract tests for companion file and payload behavior.
use sse_companion::hook::{
    patch_bind_stalker, patch_exact_line, patch_main_menu, patch_quest_include, remove_bind_stalker, remove_exact_line,
    remove_main_menu, remove_quest_include,
};
use sse_companion::hotkeys::{
    replace_hotkey_layout, HotkeyAction, HotkeyBackend, HotkeyError, HotkeyGesture, HotkeyLayout, HotkeyMatcher,
};
use sse_companion::installer::{install_bundled, install_files, install_stalker2, uninstall, PayloadFile};
use sse_companion::protocol::CompanionClient;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn temp_dir(prefix: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_nanos());
    let sequence = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("{prefix}-{}-{now}-{sequence}", std::process::id()));
    assert!(fs::create_dir_all(&path).is_ok());
    path
}

#[test]
fn hook_patch_is_idempotent_and_exactly_reversible() -> Result<(), Box<dyn std::error::Error>> {
    let original = b"function actor:update()\r\n\tengine.update()\r\nend\r\n";
    let patched = patch_exact_line(original, b"\tengine.update()", b"\tcompanion.update()")?;
    assert!(patched
        .windows(b"\tcompanion.update()".len())
        .any(|window| window == b"\tcompanion.update()"));
    assert_eq!(
        patch_exact_line(&patched, b"\tengine.update()", b"\tcompanion.update()"),
        Ok(patched.clone())
    );
    assert_eq!(
        remove_exact_line(&patched, b"\tengine.update()", b"\tcompanion.update()"),
        Ok(original.to_vec())
    );
    Ok(())
}

#[test]
fn hook_patch_rejects_ambiguous_anchors_and_foreign_hook_occurrences() {
    assert!(patch_exact_line(b"a\na\n", b"a", b"hook").is_err());
    assert!(patch_exact_line(b"a\nother\nhook\n", b"a", b"hook").is_err());
}

#[test]
fn bind_stalker_hooks_follow_exact_source_lines_and_restore_original_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let original = b"function bind:update()\r\n\tobject_binder.update(self, delta)\r\nend\r\nfunction actor_binder:use_inventory_item(obj)\r\n\treturn true\r\nend\r\n";
    let game = sse_companion::bundled::Game::CallOfPripyat;
    let patched = patch_bind_stalker(original, game)?;
    assert!(patched
        .windows(b"\tif save_editor_companion then save_editor_companion.update() end".len())
        .any(|window| window == b"\tif save_editor_companion then save_editor_companion.update() end"));
    assert!(patched
        .windows(b"\tif save_editor_companion then save_editor_companion.on_use(obj) end".len())
        .any(|window| window == b"\tif save_editor_companion then save_editor_companion.on_use(obj) end"));
    assert_eq!(remove_bind_stalker(&patched, game)?, original);
    assert!(patch_bind_stalker(
        b"object_binder.update(self, delta)\nobject_binder.update(self, delta)\n",
        sse_companion::bundled::Game::ClearSky
    )
    .is_err());
    Ok(())
}

#[test]
fn quest_include_only_adds_or_removes_the_unique_final_line() -> Result<(), Box<dyn std::error::Error>> {
    let original = b"[section]\r\nvalue = true";
    let installed = patch_quest_include(original, true)?;
    assert!(installed.ends_with(b"#include \"save_editor_companion.ltx\"\r\n"));
    assert_eq!(patch_quest_include(&installed, true)?, installed);
    assert_eq!(remove_quest_include(&installed)?, b"[section]\r\nvalue = true\r\n");
    assert!(patch_quest_include(b"#include \"save_editor_companion.ltx\"\nother\n", true).is_err());
    Ok(())
}

#[test]
fn menu_hook_tracks_the_unique_quit_block_and_ignores_comment_and_string_tokens(
) -> Result<(), Box<dyn std::error::Error>> {
    let original = b"function main_menu:OnKeyboard(dik, keyboard_action)\r\n\t-- if dik == DIK_Q then end\r\n\tlocal label = \"if dik == DIK_Q then end\"\r\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\r\n\t\tif dik == DIK_keys.DIK_Q then\r\n\t\t\treturn true\r\n\t\tend\r\n\tend\r\n\treturn false\r\nend\r\n";
    let patched = patch_main_menu(original)?;
    assert!(patched
        .windows(b"\tif save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end".len())
        .any(|window| window
            == b"\tif save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end"));
    assert_eq!(patch_main_menu(&patched)?, patched);
    assert_eq!(remove_main_menu(&patched)?, original);

    let without_quit = b"function main_menu:OnKeyboard(dik, keyboard_action)\n\tif keyboard_action == WINDOW_KEY_PRESSED then\n\t\tself:handle_key(dik)\n\tend\nend\n";
    let patched_ee = patch_main_menu(without_quit)?;
    assert!(patched_ee
        .windows(b"\t\tif save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end".len())
        .any(|window| window
            == b"\t\tif save_editor_companion_ui then save_editor_companion_ui.on_menu_key(dik, self) end"));
    assert_eq!(remove_main_menu(&patched_ee)?, without_quit);
    Ok(())
}

#[test]
fn menu_hook_ignores_levelled_lua_long_strings_and_comments() -> Result<(), Box<dyn std::error::Error>> {
    let original = b"function main_menu:OnKeyboard(dik, keyboard_action)\n\tlocal text = [==[\n\t\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\t\tend\n\t\t]=]\n\t\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\t\tend\n\t]==]\n\t--[=[\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\tend\n\t]=]\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\treturn true\n\tend\n\treturn false\nend\n";
    let patched = patch_main_menu(original)?;

    assert_eq!(remove_main_menu(&patched)?, original);
    Ok(())
}

#[test]
fn bundled_install_applies_hooks_and_uninstall_restores_source_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let bind_soc_cs = b"function bind:update()\r\n\tobject_binder.update(self, delta)\r\nend\r\nself.object:set_callback(callback.on_item_drop, self.on_item_drop, self)\r\n";
    let bind_cop = b"function bind:update()\r\n\tobject_binder.update(self, delta)\r\nend\r\nfunction actor_binder:use_inventory_item(obj)\r\n\treturn true\r\nend\r\n";
    let menu = b"function main_menu:OnKeyboard(dik, keyboard_action)\r\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\r\n\t\treturn true\r\n\tend\r\nend\r\n";
    let games = [
        (
            sse_companion::bundled::Game::ShadowOfChernobyl,
            "soc",
            bind_soc_cs.as_slice(),
            "gamedata/config/misc/save_editor_companion.ltx",
        ),
        (
            sse_companion::bundled::Game::ClearSky,
            "cs",
            bind_soc_cs.as_slice(),
            "gamedata/configs/misc/save_editor_companion.ltx",
        ),
        (
            sse_companion::bundled::Game::CallOfPripyat,
            "cop",
            bind_cop.as_slice(),
            "gamedata/configs/misc/save_editor_companion.ltx",
        ),
    ];
    for (game, game_id, bind_source, expected_config) in games {
        let root = temp_dir("sse-companion-bundled-hooks");
        let bind_path = root.join("gamedata/scripts/bind_stalker.script");
        let menu_path = root.join("gamedata/scripts/ui_main_menu.script");
        let quest_items_path = root.join(expected_config).with_file_name("quest_items.ltx");
        let original_quest_items = b"[section]\r\nvalue = true\r\n";
        fs::create_dir_all(bind_path.parent().ok_or("bind parent is missing")?)?;
        fs::create_dir_all(quest_items_path.parent().ok_or("quest items parent is missing")?)?;
        fs::write(&bind_path, bind_source)?;
        fs::write(&menu_path, menu)?;
        fs::write(&quest_items_path, original_quest_items)?;

        install_bundled(&root, game)?;

        assert!(root.join(expected_config).is_file(), "wrong config path for {game_id}");
        assert!(fs::read(&quest_items_path)?.ends_with(b"#include \"save_editor_companion.ltx\"\r\n"));
        let installed_bind = fs::read(&bind_path)?;
        let installed_menu = fs::read(&menu_path)?;
        assert!(installed_bind
            .windows(b"save_editor_companion then save_editor_companion.update()".len())
            .any(|window| window == b"save_editor_companion then save_editor_companion.update()"));
        assert!(installed_menu
            .windows(b"save_editor_companion_ui then save_editor_companion_ui.on_menu_key".len())
            .any(|window| window == b"save_editor_companion_ui then save_editor_companion_ui.on_menu_key"));
        assert!(uninstall(&root, game_id)?);
        assert_eq!(fs::read(&bind_path)?, bind_source);
        assert_eq!(fs::read(&menu_path)?, menu);
        assert_eq!(fs::read(&quest_items_path)?, original_quest_items);
        fs::remove_dir_all(root)?;
    }
    Ok(())
}

#[test]
fn bundled_installer_follows_the_game_config_fsgame_alias() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-config-alias");
    let bind_path = root.join("gamedata/scripts/bind_stalker.script");
    let menu_path = root.join("gamedata/scripts/ui_main_menu.script");
    let quest_items_path = root.join("gamedata/custom_config/misc/quest_items.ltx");
    let original_quest_items = b"[section]\nvalue = true\n";
    fs::create_dir_all(bind_path.parent().ok_or("bind parent is missing")?)?;
    fs::create_dir_all(quest_items_path.parent().ok_or("quest items parent is missing")?)?;
    fs::write(
        root.join("fsgame.ltx"),
        "$game_data$ = false | true | $fs_root$ | gamedata\\\n\
         $game_config$ = true | false | $game_data$ | custom_config\\\n",
    )?;
    let original_bind = b"function bind:update()\n\tobject_binder.update(self, delta)\nend\nself.object:set_callback(callback.on_item_drop, self.on_item_drop, self)\n";
    fs::write(&bind_path, original_bind)?;
    fs::write(
        &menu_path,
        b"function main_menu:OnKeyboard(dik, keyboard_action)\n\tif keyboard_action == ui_events.WINDOW_KEY_PRESSED then\n\t\treturn true\n\tend\nend\n",
    )?;
    fs::write(&quest_items_path, original_quest_items)?;

    install_bundled(&root, sse_companion::bundled::Game::ClearSky)?;

    assert!(root
        .join("gamedata/custom_config/misc/save_editor_companion.ltx")
        .is_file());
    assert!(fs::read(&quest_items_path)?.ends_with(b"#include \"save_editor_companion.ltx\"\n"));
    assert!(!root.join("gamedata/configs/misc/save_editor_companion.ltx").exists());
    assert!(uninstall(&root, "cs")?);
    assert_eq!(fs::read(&bind_path)?, original_bind);
    assert_eq!(fs::read(&quest_items_path)?, original_quest_items);
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn hotkey_layout_roundtrips_and_rejects_duplicate_gestures() -> Result<(), Box<dyn std::error::Error>> {
    let layout = HotkeyLayout::parse("heal=Ctrl+H\nmark=Alt+M\n")?;
    assert_eq!(layout.to_text(), "heal=Ctrl+H\nmark=Alt+M\n");
    assert_eq!(layout.binding(HotkeyAction::Heal).map(|gesture| gesture.key), Some('H'));
    assert!(HotkeyLayout::parse("heal=Ctrl+H\nmark=Ctrl+H\n").is_err());
    assert!(HotkeyLayout::parse("heal=Ctrl+F13\n").is_err());
    Ok(())
}

#[test]
fn hotkey_gesture_errors_match_csharp_and_empty_segments_are_ignored() {
    for (input, expected) in [
        ("H", "Hotkey 'H' must include a modifier and one letter."),
        ("Super+H", "Unknown hotkey modifier 'Super'."),
        ("Ctrl+Ctrl+H", "Hotkey 'Ctrl+Ctrl+H' repeats modifier 'Ctrl'."),
        ("Ctrl+F13", "Hotkey 'Ctrl+F13' must end in one A-Z letter."),
        ("Bogus+1", "Unknown hotkey modifier 'Bogus'."),
        ("  H ", "Hotkey '  H ' must include a modifier and one letter."),
    ] {
        assert_eq!(
            HotkeyGesture::parse(input).map_err(|error| error.to_string()),
            Err(expected.to_owned())
        );
    }
    assert_eq!(
        HotkeyGesture::parse("Ctrl++A").map(|gesture| gesture.to_string()),
        Ok("Ctrl+A".to_owned())
    );
}

#[derive(Default)]
struct TestHotkeyBackend {
    registrations: Vec<(HotkeyAction, HotkeyGesture)>,
    fail_once: Option<(HotkeyAction, HotkeyGesture)>,
}

impl HotkeyBackend for TestHotkeyBackend {
    fn register(&mut self, action: HotkeyAction, gesture: HotkeyGesture) -> Result<(), HotkeyError> {
        if self.fail_once == Some((action, gesture)) {
            self.fail_once = None;
            return Err(HotkeyError::new("simulated registration failure"));
        }
        self.registrations.push((action, gesture));
        Ok(())
    }

    fn unregister(&mut self, action: HotkeyAction, gesture: HotkeyGesture) -> Result<(), HotkeyError> {
        let Some(index) = self
            .registrations
            .iter()
            .position(|registered| *registered == (action, gesture))
        else {
            return Err(HotkeyError::new("simulated registration was not active"));
        };
        self.registrations.remove(index);
        Ok(())
    }
}

#[test]
fn hotkey_layout_replacement_rolls_back_after_a_registration_failure() -> Result<(), Box<dyn std::error::Error>> {
    let old = HotkeyLayout::parse("heal=Ctrl+H\nrepair_equipped=Alt+R\n")?;
    let new = HotkeyLayout::parse("heal=Ctrl+J\nrepair_equipped=Alt+R\n")?;
    let ctrl_h = HotkeyGesture::parse("Ctrl+H")?;
    let ctrl_j = HotkeyGesture::parse("Ctrl+J")?;
    let alt_r = HotkeyGesture::parse("Alt+R")?;
    let mut backend = TestHotkeyBackend {
        registrations: vec![(HotkeyAction::Heal, ctrl_h), (HotkeyAction::RepairEquipped, alt_r)],
        fail_once: Some((HotkeyAction::Heal, ctrl_j)),
    };

    assert!(replace_hotkey_layout(&mut backend, &old, &new).is_err());
    assert_eq!(
        backend.registrations,
        vec![(HotkeyAction::RepairEquipped, alt_r), (HotkeyAction::Heal, ctrl_h),]
    );
    assert!(replace_hotkey_layout(&mut backend, &old, &new).is_ok());
    assert_eq!(
        backend.registrations,
        vec![(HotkeyAction::RepairEquipped, alt_r), (HotkeyAction::Heal, ctrl_j),]
    );
    Ok(())
}

#[test]
fn hotkey_layout_persistence_is_bounded_and_replaces_existing_file() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-hotkeys");
    let path = root.join("hotkeys.txt");
    fs::write(&path, "old layout\n")?;
    let layout = HotkeyLayout::parse("heal=Ctrl+H\nrepair_equipped=Alt+R\n")?;
    layout.save(&path)?;
    assert_eq!(HotkeyLayout::load(&path).to_text(), layout.to_text());
    fs::write(&path, vec![b'a'; 64 * 1024 + 1])?;
    assert_eq!(HotkeyLayout::load(&path).to_text(), HotkeyLayout::default().to_text());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn game_window_matcher_ignores_non_game_windows() {
    let matcher = HotkeyMatcher;
    assert!(matcher.matches("xrEngine.exe", "MozillaWindowClass"));
    assert!(matcher.matches("XR_3DA", "Shadow of Chernobyl"));
    assert!(!matcher.matches("firefox", "MozillaWindowClass"));
    assert!(!matcher.matches("code", "VS Code"));
}

#[test]
fn installer_keeps_original_file_and_uninstall_restores_exact_bytes() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-install");
    let original = b"user-owned hook bytes\r\n";
    fs::create_dir_all(root.join("gamedata/scripts"))?;
    fs::write(root.join("gamedata/scripts/bind_stalker.script"), original)?;
    let payloads = [PayloadFile::new("gamedata/scripts/menu.script", b"new".to_vec())];

    install_files(&root, "soc", "v1", &payloads)?;
    assert_eq!(fs::read(root.join("gamedata/scripts/bind_stalker.script"))?, original);
    assert_eq!(fs::read(root.join("gamedata/scripts/menu.script"))?, b"new");
    uninstall(&root, "soc")?;

    assert_eq!(fs::read(root.join("gamedata/scripts/bind_stalker.script"))?, original);
    assert!(!root.join("gamedata/scripts/menu.script").exists());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn installer_refuses_to_back_up_an_existing_file_over_the_configured_limit() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-large-original");
    let target = root.join("gamedata/oversized.bin");
    fs::create_dir_all(target.parent().ok_or("target parent is missing")?)?;
    let original = vec![0x5a; sse_companion::MAX_COMPANION_FILE_BYTES.saturating_add(1)];
    fs::write(&target, &original)?;

    assert!(install_files(
        &root,
        "soc",
        "v1",
        &[PayloadFile::new("gamedata/oversized.bin", b"new".to_vec())]
    )
    .is_err());
    assert_eq!(fs::read(&target)?, original);
    assert!(!root.join(".save-editor-companion").exists());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn installer_refuses_to_remove_a_file_changed_after_install() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-conflict");
    assert!(install_files(&root, "cs", "v1", &[PayloadFile::new("gamedata/a", b"owned".to_vec())]).is_ok());
    fs::write(root.join("gamedata/a"), b"user edit")?;
    assert!(uninstall(&root, "cs").is_err());
    assert!(fs::read(root.join("gamedata/a")).is_ok_and(|bytes| bytes == b"user edit"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn update_restores_stale_owned_files_before_publishing_the_new_manifest() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-update");
    fs::create_dir_all(root.join("gamedata/scripts"))?;
    fs::write(root.join("gamedata/scripts/stale.script"), b"original stale bytes")?;
    install_files(
        &root,
        "soc",
        "v1",
        &[
            PayloadFile::new("gamedata/scripts/stale.script", b"old payload".to_vec()),
            PayloadFile::new("gamedata/scripts/kept.script", b"old kept".to_vec()),
        ],
    )?;
    install_files(
        &root,
        "soc",
        "v2",
        &[PayloadFile::new("gamedata/scripts/kept.script", b"new kept".to_vec())],
    )?;

    assert_eq!(
        fs::read(root.join("gamedata/scripts/stale.script"))?,
        b"original stale bytes"
    );
    assert!(!root
        .join(".save-editor-companion/backups/gamedata/scripts/stale.script.original")
        .exists());
    assert_eq!(fs::read(root.join("gamedata/scripts/kept.script"))?, b"new kept");
    assert!(uninstall(&root, "soc")?);
    assert_eq!(
        fs::read(root.join("gamedata/scripts/stale.script"))?,
        b"original stale bytes"
    );
    assert!(!root.join("gamedata/scripts/kept.script").exists());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn protocol_client_waits_for_the_matching_reply_id() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-protocol");
    let command_path = root.join("save_editor_cmd.txt");
    let reply_path = root.join("save_editor_out.txt");
    fs::write(&reply_path, "v1 stale-id ok old\n")?;
    let client = CompanionClient::new(root.clone());
    let worker_path = command_path.clone();
    let worker_reply = reply_path.clone();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        let request = fs::read_to_string(&worker_path);
        let id = request
            .as_ref()
            .ok()
            .and_then(|text| text.split_whitespace().nth(1))
            .unwrap_or("missing-id");
        let _ = fs::remove_file(worker_path);
        thread::sleep(Duration::from_millis(15));
        let _ = fs::write(worker_reply, format!("v1 {id} ok ready\n"));
    });

    let (latency, reply) = client.ping(Duration::from_secs(2))?;
    assert!(worker.join().is_ok());
    assert!(latency <= Duration::from_secs(2));
    assert_eq!(reply, "ready");
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn protocol_client_clears_a_stale_command_before_sending_the_next_request() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-stale-protocol");
    let command_path = root.join("save_editor_cmd.txt");
    let reply_path = root.join("save_editor_out.txt");
    fs::write(&command_path, "v1 1-1-1 give medkit 1\n")?;
    let worker_path = command_path.clone();
    let worker_reply = reply_path.clone();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if let Ok(request) = fs::read_to_string(&worker_path) {
                if !request.contains("1-1-1") {
                    let id = request.split_whitespace().nth(1).unwrap_or("missing-id");
                    let _ = fs::remove_file(&worker_path);
                    let _ = fs::write(worker_reply, format!("v1 {id} ok fresh\n"));
                    return Some(id.to_owned());
                }
            }
            thread::sleep(Duration::from_millis(2));
        }
        None
    });

    let reply = CompanionClient::new(root.clone()).ping(Duration::from_secs(2));
    let received_id = worker.join().map_err(|_| "protocol worker panicked")?;
    assert!(received_id.is_some(), "client did not replace the stale command");
    assert_eq!(reply?.1, "fresh");
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn protocol_client_clears_a_command_with_a_future_timestamp() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-future-protocol");
    let command_path = root.join("save_editor_cmd.txt");
    let future = SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_secs()
        .saturating_add(31);
    let stale_id = format!("{future}-{:x}-1", std::process::id());
    fs::write(&command_path, format!("v1 {stale_id} ping\n"))?;

    let error = CompanionClient::new(root.clone())
        .ping(Duration::from_millis(2))
        .err()
        .ok_or("future-dated command must not block new requests")?;

    assert!(error.to_string().contains("outcome is unknown"));
    assert!(!command_path.exists(), "the client should remove its timed-out request");
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn protocol_allow_list_includes_the_implemented_s2_position_command() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-pos-protocol");
    let command_path = root.join("save_editor_cmd.txt");
    let reply_path = root.join("save_editor_out.txt");
    let worker_path = command_path.clone();
    let worker_reply = reply_path.clone();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !worker_path.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(2));
        }
        let request = fs::read_to_string(&worker_path).ok()?;
        if !request.split_whitespace().any(|field| field == "pos") {
            return None;
        }
        let id = request.split_whitespace().nth(1)?;
        let _ = fs::remove_file(worker_path);
        let _ = fs::write(worker_reply, format!("v1 {id} ok 1,2,3\n"));
        Some(())
    });

    let reply = CompanionClient::new(root.clone()).send("pos", &[], Duration::from_secs(2));
    assert!(worker.join().map_err(|_| "protocol worker panicked")?.is_some());
    assert_eq!(reply?.text, "1,2,3");
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn protocol_timeout_warns_that_command_outcome_is_unknown() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-timeout-protocol");
    let error = CompanionClient::new(root.clone())
        .ping(Duration::from_millis(1))
        .err()
        .ok_or("no running game should time out")?;
    assert!(error.to_string().contains("outcome is unknown"));
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn companion_lua_command_reads_are_bounded_and_s2_console_failures_are_reported() {
    let s2 = include_str!("../assets/companion/s2/SaveEditorCompanion/Scripts/main.lua");
    let xray = include_str!("../assets/companion/gamedata/scripts/save_editor_companion.script");
    for source in [s2, xray] {
        assert!(source.contains("MAX_COMMAND_BYTES = 1024 * 1024"));
        assert!(source.contains("f:read(MAX_COMMAND_BYTES + 1)"));
        assert!(!source.contains("f:read(\"*all\")"));
        assert!(!source.contains("f:read(\"*l\")"));
        assert!(source.contains("command_timestamp(id)"));
    }
    assert!(s2.matches("if not execute_console_command").count() >= 6);
    assert!(xray.contains("save_editor_level_changer_pending.txt"));
    assert!(xray.contains("recover_persisted_level_changer"));
}

#[test]
fn protocol_rejects_bad_weather_arguments_and_oversized_request_before_writing(
) -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-protocol-validation");
    let client = CompanionClient::new(root.clone());
    assert!(client
        .send("weather", &["rain", "later"], Duration::from_millis(5))
        .is_err());
    assert!(client
        .send("give", &[&"x".repeat(1024 * 1024)], Duration::from_millis(5))
        .is_err());
    assert!(!root.join("save_editor_cmd.tmp").exists());
    assert!(!root.join("save_editor_cmd.txt").exists());
    let _ = fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn bundled_text_payload_matches_xray_windows_1251_contract() -> Result<(), Box<dyn std::error::Error>> {
    for game in [
        sse_companion::bundled::Game::ShadowOfChernobyl,
        sse_companion::bundled::Game::ClearSky,
        sse_companion::bundled::Game::CallOfPripyat,
    ] {
        let files = sse_companion::bundled::payloads(game)?;
        assert!(files
            .iter()
            .any(|file| file.relative_path.ends_with("save_editor_companion.script")));
        assert!(files.iter().any(|file| file.relative_path.ends_with(".dds")));
        for file in files.iter().filter(|file| {
            file.relative_path.ends_with(".script")
                || file.relative_path.ends_with(".xml")
                || file.relative_path.ends_with(".ltx")
        }) {
            assert!(!sse_content::decode_windows_1251(&file.bytes).contains('\u{fffd}'));
        }
    }
    Ok(())
}

#[test]
fn enhanced_edition_staging_writes_archive_and_reference_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let parent = temp_dir("sse-companion-ee");
    let output = parent.join("package");
    let started = Instant::now();
    sse_companion::bundled::stage_enhanced_edition(
        &output,
        sse_companion::bundled::Game::CallOfPripyat,
        "1.0.0",
        "Save Editor Team",
        "Save Editor Companion",
        "Companion mod",
    )?;
    let staging_elapsed = started.elapsed();
    let metadata = fs::read_to_string(output.join("desc.json"))?;
    assert!(metadata.contains("\"game\": \"cop\""));
    assert!(metadata.contains("\"package_file\": \"save_editor_companion_cop.xrp\""));
    assert!(metadata.contains("\"game_code\": \"COP\""));
    let archive_bytes = fs::read(output.join("save_editor_companion_cop.xrp"))?;
    eprintln!(
        "EE staging sample: elapsed_us={}, archive_bytes={}",
        staging_elapsed.as_micros(),
        archive_bytes.len()
    );
    let archive = sse_content::XRayArchive::from_slice(&archive_bytes, None, None)?;
    assert!(archive
        .entries()
        .iter()
        .any(|entry| entry.name == "gamedata/scripts/save_editor_companion.script"));
    for expected in sse_companion::bundled::payloads(sse_companion::bundled::Game::CallOfPripyat)? {
        assert_eq!(archive.read_file(&expected.relative_path)?, expected.bytes);
    }
    assert!(sse_companion::bundled::stage_enhanced_edition(
        &output,
        sse_companion::bundled::Game::CallOfPripyat,
        "1.0.0",
        "author",
        "title",
        "desc",
    )
    .is_err());
    let _ = fs::remove_dir_all(parent);
    Ok(())
}

#[test]
fn s2_installer_places_owned_mod_in_ue4ss_folder_and_uninstalls_it() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_dir("sse-companion-s2");
    install_stalker2(&root)?;
    let mod_dir = root.join("SaveEditorCompanion");
    assert!(mod_dir.join("Scripts/main.lua").is_file());
    let marker = fs::read_to_string(mod_dir.join("save_editor_install.json"))?;
    assert!(marker.contains(&sse_companion::bundled::stalker2_build()));
    assert!(uninstall(&root, "s2")?);
    assert!(!mod_dir.exists());
    let _ = fs::remove_dir_all(root);
    Ok(())
}
