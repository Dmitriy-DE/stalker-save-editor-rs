//! Cost of the reads the «Среда игры» screen makes on its UI thread each time it is shown (R4-052).
//!
//! The test builds a realistic store, times each read and prints the figures (`cargo test -- --nocapture`).
//! The budget is generous on purpose: it only catches a regression of an order of magnitude, not normal jitter.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use sse_fixes::models::GameTarget;
use sse_fixes::toolkit::{ManagedUserLtxSettings, ToolkitProfile, ToolkitProfileService, ToolkitSnapshotService};

const COUNT: usize = 100;
const LTX_LINES: usize = 40;
const BUDGET: Duration = Duration::from_millis(500);

fn root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sse-env-reads-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_snapshots(game: &std::path::Path) {
    let dir = game.join(".sse").join("snapshots");
    fs::create_dir_all(&dir).unwrap();
    for i in 0..COUNT {
        let id = format!("{i:064x}");
        let json = format!(
            "{{\n  \"id\": \"{id}\",\n  \"label\": \"Снапшот {i}\",\n  \"timestamp_epoch\": {i},\n  \"game\": \"cs\",\n  \"installed_fixes\": [\n    {{\"fix_id\": \"fix-a\", \"version\": \"1.0\"}},\n    {{\"fix_id\": \"fix-b\", \"version\": \"2.1\"}}\n  ],\n  \"managed_user_ltx\": {{\n    \"ui_mode\": \"1\"\n  }},\n  \"s2_mods_state\": null,\n  \"companion_installed\": false\n}}\n"
        );
        fs::write(dir.join(format!("{id}.json")), json).unwrap();
    }
}

fn write_profiles(data: &std::path::Path) {
    let dir = data.join("profiles");
    fs::create_dir_all(&dir).unwrap();
    let profile = ToolkitProfile {
        name: "Профиль".to_owned(),
        description: String::new(),
        game: GameTarget::ClearSky,
        target_fix_ids: vec!["fix-a".to_owned(), "fix-b".to_owned()],
        user_ltx_overrides: BTreeMap::new(),
        s2_mods_enabled: None,
    };
    let text = ToolkitProfileService::serialize_profile(&profile);
    for i in 0..COUNT {
        fs::write(dir.join(format!("{i:032x}.json")), &text).unwrap();
    }
}

fn write_user_ltx(game: &std::path::Path) {
    let dir = game.join("appdata");
    fs::create_dir_all(&dir).unwrap();
    let mut text = String::new();
    for i in 0..LTX_LINES {
        text.push_str(&format!("set_ui_option_{i} = {i}\n"));
    }
    text.push_str("g_fov = 67.5\n");
    fs::write(dir.join("user.ltx"), text).unwrap();
}

#[test]
fn environment_screen_reads_stay_within_budget() {
    let game = root("game");
    let data = root("data");
    write_snapshots(&game);
    write_profiles(&data);
    write_user_ltx(&game);

    let started = Instant::now();
    let snapshots = ToolkitSnapshotService::list_snapshots(&game).unwrap();
    let snapshots_time = started.elapsed();

    let started = Instant::now();
    let profiles = ToolkitProfileService::list_profiles(&data).unwrap();
    let profiles_time = started.elapsed();

    let started = Instant::now();
    let settings = ManagedUserLtxSettings::read_managed_settings(&game).unwrap();
    let settings_time = started.elapsed();

    assert_eq!(snapshots.len(), COUNT);
    assert_eq!(profiles.len(), COUNT);
    assert!(settings.contains_key("g_fov"));

    eprintln!(
        "R4-052 measure: {COUNT} snapshots {snapshots_time:?}; {COUNT} profiles {profiles_time:?}; \
         user.ltx ({LTX_LINES} lines) {settings_time:?}"
    );
    let total = snapshots_time + profiles_time + settings_time;
    assert!(total < BUDGET, "UI-thread reads took {total:?}, budget {BUDGET:?}");

    let _ = fs::remove_dir_all(&game);
    let _ = fs::remove_dir_all(&data);
}
