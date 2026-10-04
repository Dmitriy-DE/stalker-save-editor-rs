//! Tests for AppSettings load, save, atomic write, error handling, and JSON compatibility.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_app::settings::AppSettings;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!("sse_app_test_{name}_{nanos}"));
    let _ = fs::create_dir_all(&path);
    path
}

#[test]
fn default_settings_matches_csharp() {
    let settings = AppSettings::default();
    assert_eq!(settings.save_directories, None);
    assert_eq!(settings.backup_directory, None);
    assert_eq!(settings.language, None);
    assert!(settings.sound_enabled);
    assert_eq!(settings.sound_volume, 80);
    assert!(!settings.music_enabled);
    assert_eq!(settings.theme_id, "zone");
    assert_eq!(settings.accent_id, "amber");
    assert_eq!(settings.ui_scale_percent, 0);
    assert_eq!(settings.navigation_collapsed, None);
    assert!(settings.send_reports);
    assert!(!settings.reports_notice_shown);
    assert_eq!(settings.last_report_utc, None);
}

#[test]
fn round_trip_settings_json() {
    let original = AppSettings {
        save_directories: Some(vec![PathBuf::from("/saves/1"), PathBuf::from("/saves/2")]),
        backup_directory: Some(PathBuf::from("/backups")),
        language: Some("uk".to_owned()),
        sound_enabled: false,
        sound_volume: 45,
        music_enabled: true,
        theme_id: "stalker".to_owned(),
        accent_id: "emerald".to_owned(),
        ui_scale_percent: 125,
        navigation_collapsed: Some(true),
        send_reports: false,
        reports_notice_shown: true,
        last_report_utc: Some("2026-10-04T00:30:00Z".to_owned()),
    };

    let bytes = original.to_json_bytes().expect("serialization should succeed");
    let json_str = std::str::from_utf8(&bytes).expect("must be UTF-8");

    // Check snake_case keys in JSON
    assert!(json_str.contains("\"save_directories\":"));
    assert!(json_str.contains("\"backup_directory\":"));
    assert!(json_str.contains("\"sound_enabled\": false"));
    assert!(json_str.contains("\"sound_volume\": 45"));
    assert!(json_str.contains("\"music_enabled\": true"));
    assert!(json_str.contains("\"theme_id\": \"stalker\""));
    assert!(json_str.contains("\"accent_id\": \"emerald\""));
    assert!(json_str.contains("\"ui_scale_percent\": 125"));
    assert!(json_str.contains("\"navigation_collapsed\": true"));
    assert!(json_str.contains("\"send_reports\": false"));
    assert!(json_str.contains("\"reports_notice_shown\": true"));
    assert!(json_str.contains("\"last_report_utc\": \"2026-10-04T00:30:00Z\""));

    let loaded = AppSettings::from_json_slice(&bytes).expect("deserialization should succeed");
    assert_eq!(loaded, original);
}

#[test]
fn atomic_save_and_load_file() {
    let dir = temp_test_dir("atomic_save");
    let settings_path = dir.join("settings.json");

    let settings = AppSettings {
        language: Some("ru".to_owned()),
        sound_volume: 90,
        theme_id: "zone".to_owned(),
        ..AppSettings::default()
    };

    settings.save(&settings_path).expect("save should succeed");
    assert!(settings_path.exists());

    // Check no temp files left behind
    for entry in fs::read_dir(&dir).expect("read_dir") {
        let entry = entry.expect("entry");
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(!name.starts_with(".settings.json.") || !name.ends_with(".tmp"));
    }

    let reloaded = AppSettings::load(&settings_path);
    assert_eq!(reloaded.language, Some("ru".to_owned()));
    assert_eq!(reloaded.sound_volume, 90);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn load_missing_file_returns_defaults() {
    let dir = temp_test_dir("missing");
    let non_existent = dir.join("does_not_exist.json");

    let settings = AppSettings::load(&non_existent);
    assert_eq!(settings, AppSettings::default());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn load_corrupt_file_returns_defaults_without_panic() {
    let dir = temp_test_dir("corrupt");
    let corrupt_path = dir.join("settings.json");

    fs::write(&corrupt_path, b"{ not valid json at all ::: ").expect("write");
    let settings = AppSettings::load(&corrupt_path);
    assert_eq!(settings, AppSettings::default());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn forward_compatibility_unknown_fields_ignored() {
    let json_with_future_fields = br#"{
        "sound_volume": 65,
        "theme_id": "zone",
        "some_future_option": 12345,
        "nested_unknown": { "a": [1, 2, 3] }
    }"#;

    let settings =
        AppSettings::from_json_slice(json_with_future_fields).expect("should skip unknown fields without error");
    assert_eq!(settings.sound_volume, 65);
    assert_eq!(settings.theme_id, "zone");
}
