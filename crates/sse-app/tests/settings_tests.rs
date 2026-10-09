//! Tests for AppSettings load, save, atomic write, error handling, and JSON compatibility.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_app::settings::AppSettings;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;
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
fn default_settings_start_with_reports_disabled() {
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
    assert!(!settings.send_reports);
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

    let reloaded = AppSettings::load(&settings_path).expect("saved settings should load");
    assert_eq!(reloaded.language, Some("ru".to_owned()));
    assert_eq!(reloaded.sound_volume, 90);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_settings_writers_leave_one_valid_file_and_no_staging_files() {
    let directory = temp_test_dir("concurrent_atomic_save");
    let settings_path = directory.join("settings.json");
    let writer_count = 8;
    let barrier = Arc::new(Barrier::new(writer_count));
    let writers = (0..writer_count)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            let settings_path = settings_path.clone();
            thread::spawn(move || {
                let settings = AppSettings {
                    language: Some(format!("writer-{index}")),
                    theme_id: format!("theme-{index}"),
                    ..AppSettings::default()
                };
                barrier.wait();
                settings.save(&settings_path).expect("concurrent save should succeed");
            })
        })
        .collect::<Vec<_>>();

    for writer in writers {
        writer.join().expect("settings writer should not panic");
    }

    let loaded = AppSettings::load(&settings_path).expect("concurrent result should be valid JSON");
    assert!(loaded
        .theme_id
        .strip_prefix("theme-")
        .and_then(|index| index.parse::<usize>().ok())
        .is_some_and(|index| index < writer_count));
    let entries = fs::read_dir(&directory)
        .expect("read settings directory")
        .collect::<std::io::Result<Vec<_>>>()
        .expect("read directory entries");
    assert_eq!(entries.len(), 1, "only settings.json should remain");
    assert_eq!(entries[0].file_name(), "settings.json");

    fs::remove_dir_all(directory).expect("remove temporary test directory");
}

#[cfg(unix)]
#[test]
fn atomic_save_refuses_symlink_destination_without_changing_its_target() -> std::io::Result<()> {
    use std::os::unix::fs::symlink;

    let dir = temp_test_dir("symlink_settings");
    let original = dir.join("original.json");
    let destination = dir.join("settings.json");
    fs::write(&original, b"original")?;
    symlink(&original, &destination)?;

    let error = AppSettings::default()
        .save(&destination)
        .expect_err("settings must not replace a symlink");

    assert!(matches!(error, sse_core::Error::Refused(_)));
    assert_eq!(fs::read(&original)?, b"original");
    assert!(fs::symlink_metadata(&destination)?.file_type().is_symlink());
    assert_eq!(fs::read_dir(&dir)?.count(), 2);
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn atomic_settings_file_has_owner_only_permissions() -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_test_dir("settings_permissions");
    let destination = dir.join("settings.json");
    AppSettings::default()
        .save(&destination)
        .expect("settings save should succeed");

    assert_eq!(fs::metadata(&destination)?.permissions().mode() & 0o777, 0o600);
    fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn load_missing_file_returns_defaults() {
    let dir = temp_test_dir("missing");
    let non_existent = dir.join("does_not_exist.json");

    let settings = AppSettings::load(&non_existent).expect("missing settings use defaults");
    assert_eq!(settings, AppSettings::default());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn load_corrupt_file_returns_error_without_changing_the_file() {
    let dir = temp_test_dir("corrupt");
    let corrupt_path = dir.join("settings.json");

    let original = b"{ not valid json at all ::: ";
    fs::write(&corrupt_path, original).expect("write");
    assert!(AppSettings::load(&corrupt_path).is_err());
    assert_eq!(fs::read(&corrupt_path).expect("read original"), original);

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

#[test]
fn reads_csharp_snake_case_settings_snapshot() {
    let csharp = br#"{
  "save_directories": [
    "C:\\\\Games\\\\Saves",
    "/home/user/.local/share/saves"
  ],
  "backup_directory": "/tmp/backups",
  "language": "uk",
  "sound_enabled": false,
  "sound_volume": 35,
  "music_enabled": true,
  "theme_id": "zone",
  "accent_id": "amber",
  "ui_scale_percent": 125,
  "navigation_collapsed": false,
  "send_reports": false,
  "reports_notice_shown": true,
  "last_report_utc": "2026-10-05T18:30:00Z"
}"#;
    let settings = AppSettings::from_json_slice(csharp).expect("C# settings snapshot should parse");
    assert_eq!(settings.save_directories.as_ref().map(Vec::len), Some(2));
    assert_eq!(settings.backup_directory, Some(PathBuf::from("/tmp/backups")));
    assert_eq!(settings.language.as_deref(), Some("uk"));
    assert!(!settings.sound_enabled);
    assert_eq!(settings.sound_volume, 35);
    assert!(settings.music_enabled);
    assert_eq!(settings.ui_scale_percent, 125);
    assert_eq!(settings.navigation_collapsed, Some(false));
    assert!(!settings.send_reports);
    assert!(settings.reports_notice_shown);
    assert_eq!(settings.last_report_utc.as_deref(), Some("2026-10-05T18:30:00Z"));
}

#[cfg(unix)]
#[test]
fn serialization_refuses_non_utf8_paths_instead_of_replacing_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let non_utf8_path = PathBuf::from(OsString::from_vec(b"/tmp/saves-\xff".to_vec()));
    let save_directories = AppSettings {
        save_directories: Some(vec![non_utf8_path.clone()]),
        ..AppSettings::default()
    };
    let backup_directory = AppSettings {
        backup_directory: Some(non_utf8_path),
        ..AppSettings::default()
    };

    assert!(save_directories.to_json_bytes().is_err());
    assert!(backup_directory.to_json_bytes().is_err());
}

#[test]
fn integration_test_process_isolates_settings_and_logs_without_environment_override() {
    const CHILD_MARKER: &str = "SSE_TEST_DATA_ISOLATION_CHILD";
    if std::env::var_os(CHILD_MARKER).is_some() {
        let data_directory = sse_app::paths::default_data_directory();
        assert!(data_directory.starts_with(std::env::temp_dir()));
        assert_eq!(sse_app::diagnostics::log_directory(), data_directory.join("logs"));
        return;
    }

    let output = Command::new(std::env::current_exe().expect("test executable path"))
        .args([
            "--exact",
            "integration_test_process_isolates_settings_and_logs_without_environment_override",
            "--nocapture",
        ])
        .env_remove("STALKER_SAVE_EDITOR_DATA")
        .env(CHILD_MARKER, "1")
        .output()
        .expect("isolated child test should start");

    assert!(
        output.status.success(),
        "child test failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
