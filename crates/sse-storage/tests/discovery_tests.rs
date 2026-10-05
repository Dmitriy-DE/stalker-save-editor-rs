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
        let path = canonicalize_temp_root(&path);
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn canonicalize_temp_root(path: &Path) -> PathBuf {
    fs::canonicalize(path).expect("failed to canonicalize temp dir")
}

#[cfg(unix)]
#[test]
fn temporary_test_roots_canonicalize_symlink_aliases() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("canonical-root");
    let alias = temp.path.join("alias");
    symlink(&temp.path, &alias).expect("create directory alias");
    assert_eq!(canonicalize_temp_root(&alias), temp.path);
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
        known_documents_directory: None,
        known_saved_games_directory: None,
        environment: None,
        steam_roots: Some(vec![steam_root]),
    };

    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&options));

    let canonical_install = fs::canonicalize(&install).expect("canonical install path");
    let expected_custom = canonical_install.join("custom-user-data").join("saves");
    assert!(
        candidates
            .iter()
            .any(|c| c.release_id == "stalker-cop" && c.directory_path == expected_custom),
        "should find fsgame override path"
    );

    let expected_appdata = canonical_install.join("_appdata_").join("savedgames");
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
fn finds_windows_save_paths_from_redirected_known_folders_and_onedrive_roots() {
    let temp = TempDir::new("locator-known-folders");
    let profile = temp.path.join("Profile");
    let documents = temp.path.join("Redirected").join("MyDocs");
    let saved_games = temp.path.join("Redirected").join("MySavedGames");
    let one_drive = temp.path.join("OneDrive");
    let one_drive_consumer = temp.path.join("OneDriveConsumer");
    let fallback_documents = profile.join("Documents");
    let soc_folder = "stalker-shoc";
    let cs_folder = "Stalker-STCS";
    let cop_folder = "S.T.A.L.K.E.R. - Call of Pripyat";
    let ee_folder = "STALKER Shadow of Chornobyl - EE";

    for path in [
        documents.join(soc_folder).join("savedgames"),
        one_drive.join("Documents").join(cs_folder).join("savedgames"),
        one_drive_consumer.join("Documents").join(cop_folder).join("savedgames"),
        saved_games.join(ee_folder).join("STEAM").join("savedgames"),
        fallback_documents.join("Stalker-SHOC").join("savedgames"),
    ] {
        fs::create_dir_all(path).expect("create synthetic save directory");
    }

    let environment = std::collections::HashMap::from([
        ("HOME".to_owned(), profile.to_string_lossy().into_owned()),
        ("USERPROFILE".to_owned(), profile.to_string_lossy().into_owned()),
        ("OneDrive".to_owned(), one_drive.to_string_lossy().into_owned()),
        (
            "OneDriveConsumer".to_owned(),
            one_drive_consumer.to_string_lossy().into_owned(),
        ),
    ]);
    let options = SaveDirectoryDiscoveryOptions {
        platform: Some(SaveDiscoveryPlatform::Windows),
        home_directory: Some(profile.clone()),
        user_profile_directory: Some(profile),
        public_directory: None,
        local_app_data_directory: Some(temp.path.join("LocalAppData")),
        environment: Some(environment),
        steam_roots: Some(Vec::new()),
        known_documents_directory: Some(documents.clone()),
        known_saved_games_directory: Some(saved_games.clone()),
    };

    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&options));
    for (release_id, directory) in [
        ("stalker-soc", documents.join(soc_folder).join("savedgames")),
        (
            "stalker-cs",
            one_drive.join("Documents").join(cs_folder).join("savedgames"),
        ),
        (
            "stalker-cop",
            one_drive_consumer.join("Documents").join(cop_folder).join("savedgames"),
        ),
        (
            "stalker-soc-ee",
            saved_games.join(ee_folder).join("STEAM").join("savedgames"),
        ),
        (
            "stalker-soc",
            fallback_documents.join("Stalker-SHOC").join("savedgames"),
        ),
    ] {
        assert!(
            candidates
                .iter()
                .any(|candidate| candidate.release_id == release_id && candidate.directory_path == directory),
            "missing candidate for {release_id} at {}",
            directory.display()
        );
    }
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
        known_documents_directory: None,
        known_saved_games_directory: None,
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
    let canonical_saves = fs::canonicalize(&saves).expect("canonical saves path");

    assert_eq!(result.slots.len(), 2);
    assert_eq!(
        result.slots[0].path,
        fs::canonicalize(&newer).expect("canonical newer save")
    );
    assert_eq!(
        result.slots[1].path,
        fs::canonicalize(&older).expect("canonical older save")
    );
    assert!(result.slots[0].format_id.is_none());
    assert!(result.slots[0].detection_error.is_some());
    assert_eq!(result.searched_paths, vec![canonical_saves]);
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
fn library_index_warm_scan_performance_333_saves_under_150ms() {
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

    // Warm scan: leave headroom for noisy Windows CI runners.
    let start = Instant::now();
    let warm_slots = index.scan_with_index(&candidates);
    let elapsed = start.elapsed();

    assert_eq!(warm_slots.len(), 333);
    assert!(
        elapsed < Duration::from_millis(150),
        "Warm scan took {:?}, must be < 150ms",
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
            let canonical_save = fs::canonicalize(&save_file).expect("canonical save path");
            assert_eq!(result.slots.len(), 1, "Should deduplicate symlinked save directory");
            assert_eq!(result.slots[0].path, canonical_save, "Path should be canonical");

            let mut index = LibraryIndex::new();
            let index_slots = index.scan_with_index(&candidates);
            assert_eq!(
                index_slots.len(),
                1,
                "scan_with_index should deduplicate symlinked save directory"
            );
            assert_eq!(
                index_slots[0].path,
                fs::canonicalize(&save_file).expect("canonical indexed save path"),
                "Indexed path should be canonical"
            );
        }
    }
}

#[test]
fn cold_scan_performance_333_saves_under_200ms() {
    let temp = TempDir::new("cold-perf");
    let saves_dir = temp.path.join("saves");
    fs::create_dir_all(&saves_dir).expect("mkdir saves");

    for i in 0..333 {
        let path = saves_dir.join(format!("save_{i:04}.scop"));
        let mut file = File::create(&path).expect("create file");
        file.write_all(b"dummy save content").expect("write");
    }

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &saves_dir)];

    let start_discover = Instant::now();
    let result = SaveSlotDiscovery::discover(&candidates);
    let discover_elapsed = start_discover.elapsed();

    assert_eq!(result.slots.len(), 333);
    assert!(
        discover_elapsed < Duration::from_millis(200),
        "Cold discover took {:?}, must be <= 200ms",
        discover_elapsed
    );

    let mut index = LibraryIndex::new();
    let start_index = Instant::now();
    let index_slots = index.scan_with_index(&candidates);
    let index_elapsed = start_index.elapsed();

    assert_eq!(index_slots.len(), 333);
    assert!(
        index_elapsed < Duration::from_millis(200),
        "Cold index scan took {:?}, must be <= 200ms",
        index_elapsed
    );
}

#[test]
fn cop_ee_scop_detected_as_cop_ee_from_header() {
    let temp = TempDir::new("cop-ee-test");
    let save_path = temp.path.join("test_save.scop");

    // X-Ray header: magic=0xFFFFFFFF, container_version=6, unpacked_size=1024
    let mut data = Vec::new();
    data.extend_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    data.extend_from_slice(&6_u32.to_le_bytes());
    data.extend_from_slice(&1024_u32.to_le_bytes());
    // LZO literal run: command=14, then ALIFE chunk (type=0, size=4, alife_version=54)
    data.push(14);
    data.extend_from_slice(&0_u32.to_le_bytes());
    data.extend_from_slice(&4_u32.to_le_bytes());
    data.extend_from_slice(&54_u32.to_le_bytes());
    data.extend_from_slice(&[0_u8; 100]); // padding

    fs::write(&save_path, &data).expect("write test scop");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &temp.path)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    let slot = &result.slots[0];
    assert_eq!(slot.format_id.as_deref(), Some("stalker-cop-ee"));
    assert_eq!(slot.game_id.as_deref(), Some("cop"));
    assert!(slot.detection_error.is_none());
}

#[test]
fn real_system_discovery_verifies_g4b_fixes_if_present() {
    let s7_path = PathBuf::from("/home/dmytro/.local/share/Steam/steamapps/compatdata/2427430/pfx/drive_c/users/steamuser/Saved Games/STALKER Call of Prypiat - EE/STEAM/savedgames/s7.scop");
    if s7_path.exists() {
        let candidates = SaveDirectoryLocator::find_candidate_directories(None);
        let start_cold = Instant::now();
        let result = SaveSlotDiscovery::discover(&candidates);
        let cold_time = start_cold.elapsed();

        println!(
            "Real system cold discover: {} slots in {:?}",
            result.slots.len(),
            cold_time
        );
        assert!(
            cold_time < Duration::from_millis(500),
            "Cold discover must be fast (< 500ms)"
        );

        let s7_slot = result.slots.iter().find(|s| s.path == s7_path);
        assert!(s7_slot.is_some(), "s7.scop must be discovered under its canonical path");
        let s7 = s7_slot.unwrap();
        assert_eq!(s7.format_id.as_deref(), Some("stalker-cop-ee"));

        // Verify none of the discovered slots use the symlink ~/.steam/steam prefix
        assert!(
            result
                .slots
                .iter()
                .all(|s| !s.path.to_string_lossy().contains("/.steam/steam/")),
            "No saves should be listed under ~/.steam/steam symlink"
        );

        let mut index = LibraryIndex::new();
        let start_index = Instant::now();
        let index_slots = index.scan_with_index(&candidates);
        let index_time = start_index.elapsed();

        println!(
            "Real system cold index: {} slots in {:?}",
            index_slots.len(),
            index_time
        );
        assert!(
            index_time < Duration::from_millis(500),
            "Cold index must be fast (< 500ms)"
        );

        // Verify index does not duplicate saves (exact count match with discover)
        assert_eq!(
            index_slots.len(),
            result.slots.len(),
            "LibraryIndex should have no duplicates (not 2x)"
        );
    }
}
