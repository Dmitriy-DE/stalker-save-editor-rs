//! Contract and hostile-input coverage for the C5 Steam package.

use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use sse_steam::achievements::{AchievementConfirmation, AchievementService};
use sse_steam::api::{CloudFile, ScriptedSteamApi, SteamApi, WriteStage};
use sse_steam::autocloud::AutoCloudSteamApi;
use sse_steam::cloud::{
    validate_remote_save_path, write_auto_cloud, PreparedEdit, SteamCloudWriteTransaction,
    UnavailableSaveFormatVerifier, MAX_CLOUD_FILE_BYTES,
};
use sse_steam::discovery::{
    auto_cloud_path, find_auto_cloud_root, list_auto_cloud_files, parse_library_paths, read_auto_cloud_file,
    STALKER_2_APP_ID,
};
use sse_steam::protocol::{
    decode_request, decode_response_body, encode_frame, encode_response, handle_request, read_frame, serve_one,
    Request, Response, MAX_FRAME_BYTES,
};
use sse_steam::worker::{classify_worker_args, WorkerArgs};

fn temp_dir(label: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let path = std::env::temp_dir().join(format!("sse-steam-{label}-{}-{nanos}", std::process::id()));
    assert!(fs::create_dir_all(&path).is_ok());
    path
}

fn symlink_file(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
}

#[test]
fn parses_current_and_legacy_steam_library_vdf_paths() {
    let parsed = parse_library_paths(include_str!("../../../fixtures/synthetic/steam-vdf/libraryfolders.vdf"));
    assert!(parsed.is_ok());
    let Ok(paths) = parsed else { return };
    assert!(paths
        .iter()
        .any(|path| path.to_string_lossy() == "D:\\Games\\SteamLibrary"));
    let legacy = parse_library_paths("\"libraryfolders\" { \"0\" \"C:\\\\Steam\" }");
    assert!(legacy.is_ok_and(|entries| entries.iter().any(|path| path.to_string_lossy() == "C:\\Steam")));
}

#[test]
fn vdf_reader_handles_truncation_bit_flips_hostile_lengths_and_fixed_mutations() {
    let fixture = include_str!("../../../fixtures/synthetic/steam-vdf/libraryfolders.vdf");
    for end in 0..fixture.len() {
        let prefix = fixture.get(..end).unwrap_or_default();
        assert!(std::panic::catch_unwind(|| sse_codecs::vdf::parse(prefix)).is_ok());
    }
    for bit_index in 0..fixture.len().saturating_mul(8) {
        let mut changed = fixture.as_bytes().to_vec();
        if let Some(byte) = changed.get_mut(bit_index / 8) {
            *byte ^= 1_u8.checked_shl(u32::try_from(bit_index % 8).unwrap_or(0)).unwrap_or(0);
        }
        let input = String::from_utf8_lossy(&changed).into_owned();
        assert!(std::panic::catch_unwind(|| sse_codecs::vdf::parse(&input)).is_ok());
    }
    let oversized = " ".repeat(16 * 1024 * 1024 + 1);
    assert!(sse_codecs::vdf::parse(&oversized).is_err());
    let mut seed = 0x51ea_u64;
    for _ in 0..256 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut changed = fixture.as_bytes().to_vec();
        let position = usize::try_from(seed).unwrap_or(0) % changed.len();
        if let Some(byte) = changed.get_mut(position) {
            *byte ^= u8::try_from((seed >> 32) & 0xff).unwrap_or(0);
        }
        let input = String::from_utf8_lossy(&changed).into_owned();
        assert!(std::panic::catch_unwind(|| sse_codecs::vdf::parse(&input)).is_ok());
    }
}

#[test]
fn cloud_write_allowlist_matches_release_paths_and_rejects_traversal() {
    assert_eq!(
        validate_remote_save_path(4500, "_appdata_/savedgames/slot.sav")
            .ok()
            .as_deref(),
        Some("_appdata_/savedgames/slot.sav")
    );
    assert!(validate_remote_save_path(4500, "_appdata_/savedgames/../slot.sav").is_err());
    assert!(validate_remote_save_path(1643320, "Stalker2/Saved/slot.sav").is_err());
    assert!(validate_remote_save_path(41700, "_appdata_/savedgames/slot.scop").is_ok());
    assert!(validate_remote_save_path(20510, "_appdata_/savedgames/slot.scop").is_err());
}

#[test]
fn autoc_cloud_path_rejects_traversal_and_allows_stalker2_paths() {
    let root = temp_dir("autocloud");
    let path = auto_cloud_path(&root, "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav");
    assert!(path
        .as_ref()
        .is_ok_and(|candidate| candidate.ends_with("Stalker2/Saved/STEAM/SaveGames/Data/slot.sav")));
    assert!(auto_cloud_path(&root, "Stalker2/../outside.sav").is_err());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn discovers_only_the_injected_proton_autocloud_root() {
    let library = temp_dir("proton");
    let root = library.join("steamapps/compatdata/1643320/pfx/drive_c/users/tester/AppData/Local");
    assert!(fs::create_dir_all(root.join("Stalker2")).is_ok());
    let expected_root = root.canonicalize();
    assert!(expected_root.is_ok());
    let found = find_auto_cloud_root(STALKER_2_APP_ID, None, [library.clone()]);
    assert_eq!(found, expected_root.ok());
    assert!(find_auto_cloud_root(4500, None, [library.clone()]).is_none());
    let _ = fs::remove_dir_all(library);
}

#[test]
fn local_autocloud_api_lists_and_reads_only_from_the_selected_steam_library() {
    let library = temp_dir("autocloud-api");
    let auto_cloud_root = library.join("steamapps/compatdata/1643320/pfx/drive_c/users/tester/AppData/Local");
    let save_path = auto_cloud_root.join("Stalker2/Saved/STEAM/SaveGames/Data/slot.sav");
    assert!(save_path
        .parent()
        .is_some_and(|parent| fs::create_dir_all(parent).is_ok()));
    assert!(fs::write(&save_path, b"synthetic S2 Auto-Cloud save").is_ok());

    let mut api = AutoCloudSteamApi::with_roots([library.clone()], None);
    assert!(api.initialize(STALKER_2_APP_ID).is_ok());
    assert!(api.run_callbacks().is_ok());
    let listed = api.list_files();
    assert!(listed.is_ok());
    assert!(listed.is_ok_and(|files| {
        files
            .iter()
            .any(|file| file.name == "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav")
    }));
    assert_eq!(
        api.read_file("Stalker2/Saved/STEAM/SaveGames/Data/slot.sav").ok(),
        Some(b"synthetic S2 Auto-Cloud save".to_vec())
    );
    let response = handle_request(
        &mut api,
        Request::Read {
            app_id: STALKER_2_APP_ID,
            remote_name: "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav".into(),
        },
    );
    assert!(response.ok);
    assert_eq!(response.payload, b"synthetic S2 Auto-Cloud save");
    assert!(api.initialize(4500).is_err());

    let _ = fs::remove_dir_all(library);
}

#[test]
fn auto_cloud_reader_lists_and_reads_bounded_files_under_the_stalker2_tree() {
    let root = temp_dir("autocloud-read");
    let game_root = root.join("Stalker2/Saved/STEAM/SaveGames/Data");
    assert!(fs::create_dir_all(&game_root).is_ok());
    let save_path = game_root.join("slot.sav");
    assert!(fs::write(&save_path, b"synthetic cloud save").is_ok());
    let outside = root.join("outside.sav");
    assert!(fs::write(&outside, b"outside synthetic file").is_ok());
    let symlink = game_root.join("linked.sav");
    let symlink_created = symlink_file(&outside, &symlink).is_ok();
    let oversized_path = game_root.join("oversized.sav");
    let oversized = fs::File::create(&oversized_path);
    assert!(oversized.is_ok_and(|file| file
        .set_len(u64::try_from(MAX_CLOUD_FILE_BYTES).unwrap_or(0) + 1)
        .is_ok()));

    let listed = list_auto_cloud_files(&root);
    assert!(listed.is_ok());
    let Ok(listed) = listed else { return };
    assert_eq!(listed.len(), 2);
    assert!(listed
        .iter()
        .any(|file| file.name == "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav"));
    assert_eq!(
        read_auto_cloud_file(&root, "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav").ok(),
        Some(b"synthetic cloud save".to_vec())
    );
    assert!(read_auto_cloud_file(&root, "Stalker2/../outside.sav").is_err());
    assert!(read_auto_cloud_file(&root, "Stalker2/Saved/STEAM/SaveGames/Data/oversized.sav").is_err());
    if symlink_created {
        assert!(list_auto_cloud_files(&root).is_ok_and(|files| files.len() == 2));
        assert!(read_auto_cloud_file(&root, "Stalker2/Saved/STEAM/SaveGames/Data/linked.sav").is_err());
    }

    let _ = fs::remove_dir_all(root);
}

#[test]
#[ignore = "manual Release Auto-Cloud read throughput measurement"]
fn release_autocloud_read_1_mib_throughput_measurement() {
    let root = temp_dir("autocloud-read-bench");
    let game_root = root.join("Stalker2/Saved/STEAM/SaveGames/Data");
    assert!(fs::create_dir_all(&game_root).is_ok());
    let path = game_root.join("benchmark.sav");
    let payload = vec![0x5a; 1024 * 1024];
    assert!(fs::write(&path, &payload).is_ok());
    drop(payload);

    let iterations = 100_u32;
    let started = Instant::now();
    for _ in 0..iterations {
        let result = read_auto_cloud_file(&root, "Stalker2/Saved/STEAM/SaveGames/Data/benchmark.sav");
        assert!(result.is_ok_and(|bytes| bytes.len() == 1024 * 1024 && bytes.first() == Some(&0x5a)));
    }
    let elapsed = started.elapsed();
    println!(
        "Auto-Cloud synthetic 1 MiB read: {iterations} reads in {elapsed:?}; {:.3} us/read; one payload Vec/read, 1 MiB bound exercised",
        elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations)
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn binary_frames_round_trip_and_reject_hostile_lengths() {
    let request = Request::List {
        app_id: STALKER_2_APP_ID,
    };
    let encoded = encode_frame(&request);
    assert!(encoded.is_ok());
    let Ok(frame) = encoded else { return };
    assert_eq!(decode_request(&frame).ok(), Some(request.clone()));
    let write_request = Request::Write {
        app_id: 4500,
        remote_name: "_appdata_/savedgames/slot.sav".into(),
        expected_source_sha256: [7_u8; 32],
        artifact_directory: PathBuf::from("artifacts"),
        output: b"edited bytes".to_vec(),
    };
    assert_eq!(
        encode_frame(&write_request)
            .ok()
            .and_then(|bytes| decode_request(&bytes).ok()),
        Some(write_request)
    );
    assert!(usize::try_from(u32::MAX).is_ok_and(|maximum| MAX_FRAME_BYTES < maximum));
    let too_large = MAX_FRAME_BYTES
        .checked_add(1)
        .and_then(|length| u32::try_from(length).ok());
    assert!(too_large.is_some_and(|length| read_frame(&mut length.to_le_bytes().as_slice()).is_err()));
    assert!(decode_request(&[1, 2, 3]).is_err());
    for end in 0..frame.len() {
        assert!(decode_request(frame.get(..end).unwrap_or_default()).is_err());
    }
    for bit_index in 0..frame.len().saturating_mul(8) {
        let mut changed = frame.clone();
        if let Some(byte) = changed.get_mut(bit_index / 8) {
            *byte ^= 1_u8.checked_shl(u32::try_from(bit_index % 8).unwrap_or(0)).unwrap_or(0);
        }
        assert!(std::panic::catch_unwind(|| decode_request(&changed)).is_ok());
    }
    let mut seed = 0x5eed_u64;
    for _ in 0..256 {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut changed = frame.clone();
        if let Some(byte) = changed.get_mut(usize::try_from(seed).unwrap_or(0) % frame.len()) {
            *byte ^= u8::try_from((seed >> 32) & 0xff).unwrap_or(0);
        }
        assert!(std::panic::catch_unwind(|| decode_request(&changed)).is_ok());
    }
}

#[test]
fn worker_serves_one_binary_frame_and_replies_without_json() {
    let mut api = ScriptedSteamApi::default();
    api.files
        .insert("_appdata_/savedgames/slot.sav".into(), b"source bytes".to_vec());
    let request = Request::Read {
        app_id: 4500,
        remote_name: "_appdata_/savedgames/slot.sav".into(),
    };
    let encoded = encode_frame(&request);
    assert!(encoded.is_ok());
    let Ok(encoded) = encoded else { return };
    let mut input = encoded.as_slice();
    let mut output = Vec::new();
    assert!(serve_one(&mut api, &mut input, &mut output).is_ok());
    let mut response_frame = output.as_slice();
    let response_body = read_frame(&mut response_frame);
    assert!(response_body.is_ok());
    let Ok(response_body) = response_body else { return };
    let response = decode_response_body(&response_body);
    assert!(response.is_ok_and(|reply| reply.ok && reply.payload == b"source bytes"));
}

#[test]
fn worker_write_fails_closed_without_a_release_format_reader() {
    let mut api = ScriptedSteamApi::default();
    let remote_name = "_appdata_/savedgames/slot.sav";
    api.files.insert(remote_name.into(), b"source bytes".to_vec());
    let response = handle_request(
        &mut api,
        Request::Write {
            app_id: 4500,
            remote_name: remote_name.into(),
            expected_source_sha256: sse_codecs::sha256::sha256(b"source bytes"),
            artifact_directory: PathBuf::from("unused-artifacts"),
            output: b"edited bytes".to_vec(),
        },
    );
    assert!(!response.ok);
    assert_eq!(response.stage, Some(WriteStage::BeforeWrite));
    assert_eq!(api.write_count, 0);
}

#[test]
fn worker_response_preserves_the_cloud_write_stage() {
    for stage in [
        None,
        Some(WriteStage::BeforeWrite),
        Some(WriteStage::WriteRejected),
        Some(WriteStage::AfterWrite),
    ] {
        let response = Response {
            ok: stage.is_none(),
            stage,
            payload: b"worker result".to_vec(),
        };
        let encoded = encode_response(&response);
        assert!(encoded.is_ok());
        let Ok(encoded) = encoded else { return };
        let decoded_frame = read_frame(&mut encoded.as_slice());
        assert!(decoded_frame.is_ok());
        let Ok(decoded_frame) = decoded_frame else { return };
        assert_eq!(decode_response_body(&decoded_frame), Ok(response));
    }
}

#[test]
fn worker_response_rejects_unknown_write_stage_values() {
    assert!(decode_response_body(&[1, 1, 4]).is_err());
}

#[test]
fn public_cloud_write_api_fails_closed_until_a_release_reader_is_integrated() {
    let artifacts = temp_dir("transaction");
    let mut api = ScriptedSteamApi::default();
    let remote_name = "_appdata_/savedgames/save.sav";
    api.files.insert(remote_name.into(), b"fresh source".to_vec());
    let prepared = PreparedEdit::new(b"fresh source", b"edited save");
    let mut verifier = UnavailableSaveFormatVerifier;
    let result =
        SteamCloudWriteTransaction::upload(&mut api, &mut verifier, 4500, remote_name, &prepared, &artifacts, true);
    assert!(result.is_err());
    assert_eq!(api.write_count, 0);
    assert!(fs::read_dir(&artifacts).is_ok_and(|entries| entries.count() == 0));
    let _ = fs::remove_dir_all(artifacts);
}

#[test]
fn public_cloud_write_api_cannot_be_enabled_by_a_boolean() {
    let artifacts = temp_dir("verified");
    let mut api = ScriptedSteamApi::default();
    let remote_name = "_appdata_/savedgames/save.sav";
    api.files.insert(remote_name.into(), b"fresh source".to_vec());
    let prepared = PreparedEdit::new(b"fresh source", b"edited save");
    let mut verifier = UnavailableSaveFormatVerifier;
    let result =
        SteamCloudWriteTransaction::upload(&mut api, &mut verifier, 4500, remote_name, &prepared, &artifacts, true);
    assert!(result.is_err());
    assert_eq!(api.write_count, 0);
    assert!(fs::read_dir(&artifacts).is_ok_and(|entries| entries.count() == 0));
    let _ = fs::remove_dir_all(artifacts);
}

#[test]
fn auto_cloud_writer_is_disabled_until_path_operations_are_race_safe() {
    let root = temp_dir("auto-cloud-write");
    let game_root = root.join("Stalker2/Saved/STEAM/SaveGames/Data");
    assert!(fs::create_dir_all(&game_root).is_ok());
    let target = game_root.join("slot.sav");
    assert!(fs::write(&target, b"cloud original").is_ok());
    let artifacts = root.join("artifacts");
    let prepared = PreparedEdit::new(b"cloud original", b"cloud edited");
    let mut verifier = UnavailableSaveFormatVerifier;
    let result = write_auto_cloud(
        &root,
        "Stalker2/Saved/STEAM/SaveGames/Data/slot.sav",
        &prepared,
        &artifacts,
        true,
        &mut verifier,
    );
    assert!(result.is_err());
    assert!(fs::read(&target).is_ok_and(|bytes| bytes == b"cloud original"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn stale_cloud_source_is_refused_before_any_write() {
    let artifacts = temp_dir("stale");
    let mut api = ScriptedSteamApi::default();
    let remote_name = "_appdata_/savedgames/save.sav";
    api.files.insert(remote_name.into(), b"newer source".to_vec());
    let prepared = PreparedEdit::new(b"old source", b"edited save");
    let mut verifier = UnavailableSaveFormatVerifier;
    assert!(SteamCloudWriteTransaction::upload(
        &mut api,
        &mut verifier,
        4500,
        remote_name,
        &prepared,
        &artifacts,
        true,
    )
    .is_err());
    assert_eq!(api.write_count, 0);
    let _ = fs::remove_dir_all(artifacts);
}

#[test]
fn achievement_mutation_requires_an_explicit_confirmation_token() {
    let mut api = ScriptedSteamApi::default();
    api.achievements.insert("ACH_TEST".into(), false);
    let service = AchievementService;
    assert!(service
        .set(&mut api, 4500, "ACH_TEST", AchievementConfirmation::Declined)
        .is_err());
    assert_eq!(api.achievements.get("ACH_TEST").copied(), Some(false));
    assert!(service
        .set(&mut api, 4500, "ACH_TEST", AchievementConfirmation::Confirmed)
        .is_ok());
    assert_eq!(api.achievements.get("ACH_TEST").copied(), Some(true));
    assert!(service
        .clear(&mut api, 4500, "ACH_TEST", AchievementConfirmation::Confirmed)
        .is_ok());
    assert_eq!(api.achievements.get("ACH_TEST").copied(), Some(false));
    assert!(service
        .clear(&mut api, 9999, "ACH_TEST", AchievementConfirmation::Confirmed)
        .is_err());
}

#[test]
fn worker_arguments_are_classified_before_normal_cli_routing() {
    assert_eq!(classify_worker_args(&["--steam-worker".into()]), WorkerArgs::Worker);
    assert_eq!(classify_worker_args(&["--steam-workre".into()]), WorkerArgs::UsageError);
    assert_eq!(classify_worker_args(&["version".into()]), WorkerArgs::NormalCli);
}

#[test]
fn cloud_listing_is_provided_by_the_api_trait() {
    let mut api = ScriptedSteamApi::default();
    api.listed_files.push(CloudFile {
        name: "save.sav".into(),
        size: 12,
        timestamp: 4,
    });
    assert!(api.list_files().is_ok_and(|files| files.len() == 1));
}
