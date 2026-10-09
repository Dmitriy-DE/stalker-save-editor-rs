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

// Arbitrary byte filenames are supported on Linux filesystems; macOS rejects them as invalid UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn discovery_keeps_distinct_non_utf8_save_paths_distinct() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = TempDir::new("non-utf8-discovery");
    let left_dir = temp.path.join(OsString::from_vec(b"saves-\xFF".to_vec()));
    let right_dir = temp.path.join(OsString::from_vec(b"saves-\xFE".to_vec()));
    fs::create_dir_all(&left_dir).expect("create left non-UTF-8 directory");
    fs::create_dir_all(&right_dir).expect("create right non-UTF-8 directory");
    fs::write(left_dir.join("slot.sav"), [1, 2, 3, 4, 0]).expect("write left save");
    fs::write(right_dir.join("slot.sav"), [5, 6, 7, 8, 0]).expect("write right save");
    let candidates = vec![
        SaveDirectoryCandidate::new("cop", "stalker-cop", &left_dir),
        SaveDirectoryCandidate::new("cop", "stalker-cop", &right_dir),
    ];

    let result = SaveSlotDiscovery::discover(&candidates);
    assert_eq!(result.slots.len(), 2);
}

#[test]
fn configured_save_directories_are_included_in_candidates() {
    let temp = TempDir::new("configured-saves");
    let configured = temp.path.join("custom save folder");
    fs::create_dir_all(&configured).expect("create configured save directory");
    let save_path = configured.join("custom.sav");
    fs::write(&save_path, b"synthetic save candidate").expect("write synthetic save");
    let options = SaveDirectoryDiscoveryOptions {
        platform: Some(SaveDiscoveryPlatform::Linux),
        home_directory: Some(temp.path.join("home")),
        environment: Some(std::collections::HashMap::new()),
        steam_roots: Some(Vec::new()),
        custom_save_directories: Some(vec![configured.clone()]),
        ..SaveDirectoryDiscoveryOptions::default()
    };

    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&options));

    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.directory_path == canonicalize_temp_root(&configured)),
        "a configured directory must be searched even if it is outside standard save roots"
    );
    let discovered = SaveSlotDiscovery::discover(&candidates);
    assert!(discovered.searched_paths.contains(&canonicalize_temp_root(&configured)));
    assert!(discovered.slots.iter().any(|slot| slot.path == save_path));
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
        custom_save_directories: None,
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
        custom_save_directories: None,
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
        custom_save_directories: None,
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

    let encoded = index.encode().expect("encode index");
    let decoded = LibraryIndex::decode(&encoded).expect("decode index");
    assert_eq!(decoded.len(), 1);

    let looked_up = decoded
        .lookup(
            &entry.path,
            1024,
            UNIX_EPOCH + Duration::new(1_700_000_000, 500),
            entry.header_hash,
        )
        .expect("lookup");
    assert_eq!(looked_up, &entry);

    // Corrupted bytes return None safely
    assert!(LibraryIndex::decode(b"corrupt header data").is_none());
}

#[test]
fn library_index_rejects_trailing_bytes_after_declared_entries() {
    let mut index = LibraryIndex::new();
    index.insert(LibraryIndexEntry {
        path: PathBuf::from("/path/to/save.sav"),
        candidate_game_id: "cop".to_string(),
        candidate_release_id: "stalker-cop".to_string(),
        size: 1,
        mtime_secs: 0,
        mtime_nanos: 0,
        header_hash: 0,
        format_id: None,
        game_id: None,
        detection_error: None,
    });
    let mut encoded = index.encode().expect("encode index");
    encoded.extend_from_slice(b"unclaimed tail");
    assert!(LibraryIndex::decode(&encoded).is_none());
}

#[test]
fn library_index_rejects_hostile_entry_count_before_allocating() {
    let mut encoded = Vec::from(*b"SSLI");
    encoded.extend_from_slice(&1_u32.to_le_bytes());
    encoded.extend_from_slice(&u32::MAX.to_le_bytes());

    assert!(LibraryIndex::decode(&encoded).is_none());
}

#[test]
fn library_index_save_refuses_strings_longer_than_its_wire_length() {
    let temp = TempDir::new("index-long-string");
    let mut index = LibraryIndex::new();
    index.insert(LibraryIndexEntry {
        path: PathBuf::from("/path/to/save.sav"),
        candidate_game_id: "cop".to_string(),
        candidate_release_id: "r".repeat(usize::from(u16::MAX) + 1),
        size: 1,
        mtime_secs: 0,
        mtime_nanos: 0,
        header_hash: 0,
        format_id: None,
        game_id: None,
        detection_error: None,
    });

    let result = index.save(&temp.path.join("oversized.index"));
    assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::InvalidInput));

    let mut sentinel_collision = LibraryIndex::new();
    sentinel_collision.insert(LibraryIndexEntry {
        path: PathBuf::from("/path/to/save.sav"),
        candidate_game_id: "cop".to_string(),
        candidate_release_id: "stalker-cop".to_string(),
        size: 1,
        mtime_secs: 0,
        mtime_nanos: 0,
        header_hash: 0,
        format_id: None,
        game_id: None,
        detection_error: Some("e".repeat(usize::from(u16::MAX))),
    });
    let result = sentinel_collision.save(&temp.path.join("sentinel-collision.index"));
    assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::InvalidInput));
}

#[cfg(unix)]
#[test]
fn library_index_save_refuses_non_utf8_paths_instead_of_lossy_encoding() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let temp = TempDir::new("index-non-utf8-path");
    let mut index = LibraryIndex::new();
    index.insert(LibraryIndexEntry {
        path: PathBuf::from(OsString::from_vec(vec![b'/', b's', b'a', b'v', b'e', 0xFF])),
        candidate_game_id: "cop".to_string(),
        candidate_release_id: "stalker-cop".to_string(),
        size: 1,
        mtime_secs: 0,
        mtime_nanos: 0,
        header_hash: 0,
        format_id: None,
        game_id: None,
        detection_error: None,
    });

    let result = index.save(&temp.path.join("non-utf8.index"));
    assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::InvalidInput));
}

#[test]
fn library_index_parallel_saves_do_not_collide_on_temporary_files() {
    use std::sync::{Arc, Barrier};

    let temp = TempDir::new("index-parallel-save");
    let large_message = "x".repeat(60_000);
    let make_index = |path: &str| {
        let mut index = LibraryIndex::new();
        for number in 0..16 {
            index.insert(LibraryIndexEntry {
                path: PathBuf::from(format!("/{path}/{number}.sav")),
                candidate_game_id: "cop".to_string(),
                candidate_release_id: "stalker-cop".to_string(),
                size: 1,
                mtime_secs: 0,
                mtime_nanos: 0,
                header_hash: 0,
                format_id: None,
                game_id: None,
                detection_error: Some(large_message.clone()),
            });
        }
        index
    };

    for attempt in 0..8 {
        let left_index = make_index("left");
        let right_index = make_index("right");
        let left_path = temp.path.join(format!("library-{attempt}.left"));
        let right_path = temp.path.join(format!("library-{attempt}.right"));
        let barrier = Arc::new(Barrier::new(2));
        let left_barrier = Arc::clone(&barrier);
        let right_barrier = Arc::clone(&barrier);
        let left = std::thread::spawn(move || {
            left_barrier.wait();
            left_index.save(&left_path)
        });
        let right = std::thread::spawn(move || {
            right_barrier.wait();
            right_index.save(&right_path)
        });

        let left_result = left.join().expect("left save thread");
        assert!(left_result.is_ok(), "left index save failed: {left_result:?}");
        let right_result = right.join().expect("right save thread");
        assert!(right_result.is_ok(), "right index save failed: {right_result:?}");
        assert!(LibraryIndex::load(&temp.path.join(format!("library-{attempt}.left"))).is_some());
        assert!(LibraryIndex::load(&temp.path.join(format!("library-{attempt}.right"))).is_some());
    }
}

#[test]
fn library_index_rechecks_header_when_size_and_mtime_match() {
    let temp = TempDir::new("index-header-hash");
    let saves_dir = temp.path.join("saves");
    fs::create_dir_all(&saves_dir).expect("mkdir saves");
    let save_path = saves_dir.join("slot.sav");
    let mut changed = include_bytes!("../../../fixtures/synthetic/xray-soc.sav").to_vec();
    fs::write(&save_path, &changed).expect("write valid synthetic save");
    let candidate = SaveDirectoryCandidate::new("soc", "stalker-soc", &saves_dir);
    let candidates = vec![candidate];
    let mut index = LibraryIndex::new();

    let first = index.scan_with_index(&candidates);
    assert_eq!(
        first.first().and_then(|slot| slot.format_id.as_deref()),
        Some("stalker-soc")
    );
    let original_mtime = fs::metadata(&save_path)
        .expect("metadata before edit")
        .modified()
        .expect("mtime before edit");

    changed[4] = 5;
    fs::write(&save_path, &changed).expect("write same-size changed header");
    File::options()
        .write(true)
        .open(&save_path)
        .expect("open changed save")
        .set_times(std::fs::FileTimes::new().set_modified(original_mtime))
        .expect("restore original mtime");

    let second = index.scan_with_index(&candidates);
    assert_eq!(second.first().and_then(|slot| slot.format_id.as_deref()), None);
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

#[cfg(unix)]
#[test]
fn lists_a_save_once_when_its_folder_is_reachable_through_a_link() {
    use std::os::unix::fs::symlink;

    let temp = TempDir::new("slot-link");
    let real = temp.path.join("AppData").join("Local").join("savedgames");
    fs::create_dir_all(&real).expect("mkdir real");

    let save_file = real.join("quick.scop");
    fs::write(&save_file, b"test content").expect("write quick.scop");

    let link_parent = temp.path.join("Local Settings");
    fs::create_dir_all(&link_parent).expect("mkdir link parent");

    let link = link_parent.join("Application Data");
    symlink(temp.path.join("AppData").join("Local"), &link).expect("create directory symlink");
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
fn incomplete_enhanced_header_is_not_classified_from_path_or_candidate() {
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
    assert_eq!(slot.format_id, None);
    assert_eq!(slot.game_id, None);
    assert!(slot.detection_error.is_some());
}

#[test]
fn enhanced_format_detection_follows_content_over_path_and_candidate() {
    let temp = TempDir::new("ee-content-detection");
    let save_path = temp.path.join("cop_save.scop");
    fs::write(
        &save_path,
        include_bytes!("../../../fixtures/synthetic/xray-clear-sky-ee.sav"),
    )
    .expect("write synthetic Clear Sky EE save");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &temp.path)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    let slot = &result.slots[0];
    assert_eq!(slot.format_id.as_deref(), Some("stalker-cs-ee"));
    assert_eq!(slot.game_id.as_deref(), Some("clear_sky"));
    assert!(slot.detection_error.is_none());
}

#[test]
fn enhanced_format_detection_reads_past_the_header_sample() {
    let temp = TempDir::new("ee-large-content-detection");
    let save_path = temp.path.join("large_save.scop");
    let bytes = with_large_unknown_xray_chunk(include_bytes!(
        "../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav"
    ));
    assert!(bytes.len() > 4096, "fixture must exceed the discovery header sample");
    fs::write(&save_path, bytes).expect("write expanded synthetic save");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &temp.path)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    let slot = &result.slots[0];
    assert_eq!(slot.format_id.as_deref(), Some("stalker-cop-ee"));
    assert_eq!(slot.game_id.as_deref(), Some("cop"));
    assert!(slot.detection_error.is_none());
}

#[test]
fn library_index_cold_scan_reads_past_the_header_sample_for_ee_detection() {
    let temp = TempDir::new("ee-large-index-detection");
    let save_path = temp.path.join("large_save.scop");
    let bytes = with_large_unknown_xray_chunk(include_bytes!(
        "../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav"
    ));
    fs::write(&save_path, bytes).expect("write expanded synthetic save");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &temp.path)];
    let mut index = LibraryIndex::new();
    let slots = index.scan_with_index(&candidates);

    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].format_id.as_deref(), Some("stalker-cop-ee"));
    assert_eq!(slots[0].game_id.as_deref(), Some("cop"));
    assert!(slots[0].detection_error.is_none());
}

#[test]
fn enhanced_mod_without_a_level_marker_is_not_classified_from_path() {
    let temp = TempDir::new("ee-mod-detection");
    let save_path = temp.path.join("modded_save.scop");
    let bytes = without_xray_level_marker(include_bytes!(
        "../../../fixtures/synthetic/xray-call-of-pripyat-ee.sav"
    ));
    fs::write(&save_path, bytes).expect("write synthetic mod save");

    let candidates = vec![SaveDirectoryCandidate::new("cop", "stalker-cop-ee", &temp.path)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    let slot = &result.slots[0];
    assert_eq!(slot.format_id, None);
    assert_eq!(slot.game_id, None);
    assert!(slot.detection_error.is_some());
}

fn with_large_unknown_xray_chunk(packed: &[u8]) -> Vec<u8> {
    let declared_size = u32::from_le_bytes(packed[8..12].try_into().expect("X-Ray header size"));
    let unpacked_size = usize::try_from(declared_size).expect("fixture size should fit usize");
    let mut raw =
        sse_codecs::lzo1x::decompress(&packed[12..], unpacked_size).expect("synthetic X-Ray fixture should decompress");
    raw.extend_from_slice(&0xCAFE_BABEu32.to_le_bytes());
    raw.extend_from_slice(&8192_u32.to_le_bytes());
    let mut seed = 0xA341_316C_u32;
    for _ in 0..8192 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        raw.push((seed >> 24) as u8);
    }

    let compressed = sse_codecs::lzo1x::compress(&raw);
    let mut output = packed[..12].to_vec();
    output[8..12].copy_from_slice(
        &u32::try_from(raw.len())
            .expect("expanded synthetic fixture should fit the header")
            .to_le_bytes(),
    );
    output.extend_from_slice(&compressed);
    output
}

fn without_xray_level_marker(packed: &[u8]) -> Vec<u8> {
    let declared_size = u32::from_le_bytes(packed[8..12].try_into().expect("X-Ray header size"));
    let unpacked_size = usize::try_from(declared_size).expect("fixture size should fit usize");
    let raw =
        sse_codecs::lzo1x::decompress(&packed[12..], unpacked_size).expect("synthetic X-Ray fixture should decompress");
    let mut raw = raw;
    let marker = raw
        .windows(5)
        .position(|window| window == b"zaton")
        .expect("synthetic CoP EE fixture should have a level marker");
    raw[marker..marker.saturating_add(5)].copy_from_slice(b"other");
    let compressed = sse_codecs::lzo1x::compress(&raw);
    let mut output = packed[..12].to_vec();
    output.extend_from_slice(&compressed);
    output
}

#[test]
fn real_system_discovery_verifies_g4b_fixes_if_present() {
    let Some(s7_path) = std::env::var_os("SSE_G4B_TEST_SAVE").map(PathBuf::from) else {
        return;
    };
    assert!(s7_path.is_file(), "SSE_G4B_TEST_SAVE must name a save file");
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

#[test]
fn foreign_sav_with_nonzero_fifth_byte_is_not_identified_as_stalker2() {
    let temp = TempDir::new("foreign-sav");
    let saves = temp.path.join("saves");
    fs::create_dir_all(&saves).expect("create saves dir");
    // A plausible unpacked size, a non-zero fifth byte, and no container trailer that matches.
    let mut bytes = vec![0x10, 0x00, 0x00, 0x00, 0x01];
    bytes.extend_from_slice(&[0x5a; 59]);
    fs::write(saves.join("foreign.sav"), &bytes).expect("write foreign save");

    let candidates = vec![SaveDirectoryCandidate::new("stalker2", "stalker2", &saves)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), 1);
    assert!(result.slots[0].format_id.is_none(), "{:?}", result.slots[0].format_id);
    assert!(result.slots[0].game_id.is_none());
}

#[test]
fn every_stalker2_fixture_is_identified_by_its_container() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic");
    let mut sources = Vec::new();
    let mut pending = vec![fixtures];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).expect("read fixtures") {
            let path = entry.expect("fixture entry").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "sav")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().contains("s2"))
            {
                sources.push(path);
            }
        }
    }
    assert!(!sources.is_empty(), "the repository should carry S2 fixtures");

    let temp = TempDir::new("s2-fixtures");
    let saves = temp.path.join("saves");
    fs::create_dir_all(&saves).expect("create saves dir");
    for source in &sources {
        let name = source.file_name().expect("fixture name");
        fs::copy(source, saves.join(name)).expect("copy fixture");
    }
    let candidates = vec![SaveDirectoryCandidate::new("stalker2", "stalker2", &saves)];
    let result = SaveSlotDiscovery::discover(&candidates);

    assert_eq!(result.slots.len(), sources.len());
    for slot in &result.slots {
        assert_eq!(slot.format_id.as_deref(), Some("stalker2"), "{}", slot.path.display());
    }
}
