//! Tests for save directory locator, slot discovery, and library index.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use sse_storage::discovery::{
    LibraryIndex, LibraryIndexEntry, SaveDirectoryCandidate, SaveDirectoryDiscoveryOptions, SaveDirectoryLocator,
    SaveDiscoveryPlatform, SaveSlotDiscovery,
};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("sse-test-{name}-{count}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn finds_windows_game_save_paths_from_steam_manifests_and_fsgame_override() {
    let temp = TempDir::new("locator-win");
    let home = temp.path.join("User");
    let steam_root = temp.path.join("Steam");
    let library = temp.path.join("Library");

    fs::create_dir_all(&home).expect("mkdir home");
    fs::create_dir_all(steam_root.join("steamapps")).expect("mkdir steamapps");
    fs::create_dir_all(library.join("steamapps")).expect("mkdir lib steamapps");

    let library_vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    let vdf_content = format!(
        "\"libraryfolders\" {{\n  \"0\" {{ \"path\" \"{}\" }}\n  \"1\" {{ \"path\" \"{}\" }}\n}}\n",
        steam_root.display().to_string().replace('\\', "\\\\"),
        library.display().to_string().replace('\\', "\\\\"),
    );
    fs::write(&library_vdf, vdf_content).expect("write libraryfolders.vdf");

    let install = library.join("steamapps").join("common").join("Custom COP Install");
    fs::create_dir_all(&install).expect("mkdir install");

    let acf = library.join("steamapps").join("appmanifest_41700.acf");
    fs::write(
        &acf,
        "\"AppState\" { \"appid\" \"41700\" \"installdir\" \"Custom COP Install\" }\n",
    )
    .expect("write acf");

    let fsgame = install.join("fsgame.ltx");
    fs::write(
        &fsgame,
        "$app_data_root$ = true| false| $fs_root$| custom-user-data\\\n$game_saves$ = true| false| $app_data_root$| saves\\\n",
    )
    .expect("write fsgame.ltx");

    let options = SaveDirectoryDiscoveryOptions {
        platform: Some(SaveDiscoveryPlatform::Windows),
        home_directory: Some(home.clone()),
        user_profile_directory: Some(home.clone()),
        public_directory: Some(temp.path.join("Public")),
        local_app_data_directory: Some(home.join("AppData").join("Local")),
        environment: None,
        steam_roots: Some(vec![steam_root]),
    };

    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&options));

    let expected_custom = install.join("custom-user-data").join("saves");
    assert!(
        candidates
            .iter()
            .any(|c| c.release_id == "stalker-cop" && c.directory_path == expected_custom),
        "should find fsgame override path"
    );

    let expected_appdata = install.join("_appdata_").join("savedgames");
    assert!(
        candidates
            .iter()
            .any(|c| c.release_id == "stalker-cop" && c.directory_path == expected_appdata),
        "should find fallback appdata path"
    );

    assert!(
        candidates
            .iter()
            .any(|c| c.directory_path.to_string_lossy().contains("Documents")),
        "should find localized Documents paths"
    );
}

#[test]
fn finds_proton_save_paths_in_a_secondary_library_even_without_a_game_manifest() {
    let temp = TempDir::new("locator-proton");
    let home = temp.path.join("home");
    let steam_root = temp.path.join("Steam");
    let library = temp.path.join("Secondary Library");

    fs::create_dir_all(&home).expect("mkdir home");
    fs::create_dir_all(steam_root.join("steamapps")).expect("mkdir steamapps");

    let library_vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    let vdf_content = format!(
        "\"libraryfolders\" {{\n  \"0\" {{ \"path\" \"{}\" }}\n  \"1\" {{ \"path\" \"{}\" }}\n}}\n",
        steam_root.display().to_string().replace('\\', "\\\\"),
        library.display().to_string().replace('\\', "\\\\"),
    );
    fs::write(&library_vdf, vdf_content).expect("write libraryfolders.vdf");

    let cop_compat = library
        .join("steamapps")
        .join("compatdata")
        .join("41700")
        .join("pfx")
        .join("drive_c")
        .join("users")
        .join("ProtonProfile");
    fs::create_dir_all(&cop_compat).expect("mkdir cop compat");

    let s2_compat = library
        .join("steamapps")
        .join("compatdata")
        .join("1643320")
        .join("pfx")
        .join("drive_c")
        .join("users")
        .join("ProtonProfile");
    fs::create_dir_all(&s2_compat).expect("mkdir s2 compat");

    let options = SaveDirectoryDiscoveryOptions {
        platform: Some(SaveDiscoveryPlatform::Linux),
        home_directory: Some(home),
        user_profile_directory: None,
        public_directory: None,
        local_app_data_directory: None,
        environment: None,
        steam_roots: Some(vec![steam_root]),
    };

    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&options));

    let cop_expected = Path::new("compatdata")
        .join("41700")
        .join("pfx")
        .join("drive_c")
        .join("users")
        .join("ProtonProfile")
        .join("Documents")
        .join("Stalker-COP")
        .join("savedgames");

    assert!(
        candidates.iter().any(|c| c.release_id == "stalker-cop"
            && c.directory_path
                .to_string_lossy()
                .contains(&cop_expected.to_string_lossy().to_string())),
        "should find proton CoP candidate"
    );

    let s2_expected = Path::new("compatdata")
        .join("1643320")
        .join("pfx")
        .join("drive_c")
        .join("users")
        .join("ProtonProfile")
        .join("Local Settings")
        .join("Application Data")
        .join("Stalker2")
        .join("Saved")
        .join("STEAM")
        .join("SaveGames");

    assert!(
        candidates.iter().any(|c| c.release_id == "stalker2"
            && c.directory_path
                .to_string_lossy()
                .contains(&s2_expected.to_string_lossy().to_string())),
        "should find proton S2 candidate"
    );
}

#[test]
fn sorts_slots_newest_first_and_marks_unrecognized_files_without_guessing_from_folder() {
    let temp = TempDir::new("slot-sort");
    let saves = temp.path.join("saves");
    fs::create_dir_all(&saves).expect("mkdir saves");

    let older = saves.join("older.scop");
    let newer = saves.join("newer.scs");

    fs::write(&older, b"older fake save").expect("write older");
    std::thread::sleep(Duration::from_millis(50));
    fs::write(&newer, b"newer fake save").expect("write newer");

    let candidates = vec![SaveDirectoryCandidate::new("stalker2", "stalker2", &saves)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 2);
    assert_eq!(result.slots[0].path, newer);
    assert_eq!(result.slots[1].path, older);
    assert!(result.slots[0].format_id.is_none());
    assert!(result.slots[0].detection_error.is_some());
    assert_eq!(result.searched_paths, vec![saves]);
}

#[test]
fn includes_supported_foreign_extensions_but_excludes_sidecars_and_stalker2_metadata() {
    let temp = TempDir::new("slot-extensions");
    let saves = temp.path.join("saves");
    fs::create_dir_all(&saves).expect("mkdir saves");

    for name in &[
        "slot.scop",
        "slot.scs",
        "thumb.dds",
        "meta.info",
        "campaignssave.sav",
        "analyticsdata.sav",
    ] {
        fs::write(saves.join(name), [1, 2, 3]).expect("write file");
    }

    let candidates = vec![SaveDirectoryCandidate::new("soc", "stalker-soc", &saves)];
    let result = SaveSlotDiscovery::discover(&candidates);

    let mut found_names: Vec<String> = result
        .slots
        .iter()
        .map(|s| s.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    found_names.sort();

    assert_eq!(found_names, vec!["slot.scop", "slot.scs"]);
}

#[test]
fn explains_why_a_file_is_not_readable() {
    let temp = TempDir::new("slot-unreadable");
    fs::write(temp.path.join("broken.sav"), [1, 2, 3, 4]).expect("write broken");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop", &temp.path)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    let slot = &result.slots[0];
    assert!(slot.format_id.is_none());
    assert!(slot.detection_error.is_some());
}

#[test]
fn library_index_encode_decode_round_trip_and_corrupt_recovery() {
    let mut index = LibraryIndex::new();
    let entry = LibraryIndexEntry {
        path: PathBuf::from("/path/to/save.sav"),
        candidate_game_id: "cop".to_string(),
        candidate_release_id: "stalker-cop".to_string(),
        size: 1024,
        mtime_secs: 1_700_000_000,
        mtime_nanos: 500,
        header_hash: 0x1234_5678_9ABC_DEF0,
        format_id: Some("stalker-cop".to_string()),
        game_id: Some("cop".to_string()),
        detection_error: None,
    };
    index.insert(entry.clone());

    let encoded = index.encode();
    let decoded = LibraryIndex::decode(&encoded).expect("decode index");
    assert_eq!(decoded.len(), 1);

    let looked_up = decoded
        .lookup(&entry.path, 1024, UNIX_EPOCH + Duration::new(1_700_000_000, 500))
        .expect("lookup");
    assert_eq!(looked_up, &entry);

    // Corrupted bytes return None safely
    assert!(LibraryIndex::decode(b"corrupt header data").is_none());
}

#[test]
fn library_index_warm_scan_performance_333_saves_under_100ms() {
    let temp = TempDir::new("index-perf");
    let saves_dir = temp.path.join("saves");
    fs::create_dir_all(&saves_dir).expect("mkdir saves");

    let mut index = LibraryIndex::new();
    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop", &saves_dir)];

    // Create 333 dummy saves
    for i in 0..333 {
        let path = saves_dir.join(format!("save_{i:04}.scop"));
        let mut file = File::create(&path).expect("create file");
        file.write_all(b"dummy save content").expect("write");
    }

    // Cold scan
    let _ = index.scan_with_index(&candidates);
    assert_eq!(index.len(), 333);

    // Warm scan: measured against TASKS.md requirement (<= 100 ms)
    let start = Instant::now();
    let warm_slots = index.scan_with_index(&candidates);
    let elapsed = start.elapsed();

    assert_eq!(warm_slots.len(), 333);
    assert!(
        elapsed < Duration::from_millis(100),
        "Warm scan took {:?}, must be <= 100ms",
        elapsed
    );
}

#[test]
fn lists_a_save_once_when_its_folder_is_reachable_through_a_link() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let temp = TempDir::new("slot-link");
        let real = temp.path.join("AppData").join("Local").join("savedgames");
        fs::create_dir_all(&real).expect("mkdir real");

        let save_file = real.join("quick.scop");
        fs::write(&save_file, b"test content").expect("write quick.scop");

        let link_parent = temp.path.join("Local Settings");
        fs::create_dir_all(&link_parent).expect("mkdir link parent");

        let link = link_parent.join("Application Data");
        if symlink(temp.path.join("AppData").join("Local"), &link).is_ok() {
            let candidates = vec![
                SaveDirectoryCandidate::new("cop", "stalker-cop", &real),
                SaveDirectoryCandidate::new("cop", "stalker-cop", link.join("savedgames")),
            ];

            let result = SaveSlotDiscovery::discover(&candidates);
            assert_eq!(result.slots.len(), 1, "Should deduplicate symlinked save directory");
        }
    }
}
