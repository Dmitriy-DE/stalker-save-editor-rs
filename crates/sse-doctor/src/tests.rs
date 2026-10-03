use crate::{
    analyze_crash_file, analyze_crash_file_with_dump_reader, analyze_crash_log, analyze_game_install,
    analyze_game_install_from_steam, analyze_quests, analyze_save, classify_game_build, discover_crash_logs,
    evaluate_quest_facts, parse_app_state, CrashAdvice, CrashDumpFacts, CrashDumpReader, CrashKind,
    CrashSignatureCatalog, GameBuildStatus, GameTarget, QuestNpcVitals, QuestTaskStatus, SaveDoctorStatus,
};

const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");

#[test]
fn documented_crash_signatures_match_case_insensitively_and_filter_by_game() {
    let line = "! [LUA][ERROR] ERROR: wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3";
    let matched = CrashSignatureCatalog::match_log(line, Some("cs"));

    assert_eq!(matched.map(|signature| signature.id), Some("cs.wrong-target-wild-napr"));
    assert_eq!(matched.map(|signature| signature.advice), Some(CrashAdvice::RepairSave));
    assert!(CrashSignatureCatalog::match_log(line, Some("soc")).is_none());
    assert_eq!(
        CrashSignatureCatalog::match_log(line, Some("Clear Sky")).map(|signature| signature.id),
        Some("cs.wrong-target-wild-napr")
    );
    assert!(CrashSignatureCatalog::match_log(line, Some("future game")).is_none());
}

#[test]
fn generic_installation_signatures_apply_to_any_game_without_matching_unrelated_text() {
    let matched = CrashSignatureCatalog::match_log("Can't find variable night_vision in [device_torch]", Some("soc"));

    assert_eq!(matched.map(|signature| signature.id), Some("any.missing-config-value"));
    assert_eq!(
        matched.map(|signature| signature.advice),
        Some(CrashAdvice::RepairInstallation)
    );
    assert!(CrashSignatureCatalog::match_log("generic engine error", Some("soc")).is_none());
}

#[test]
fn the_specific_missing_backpack_signature_wins_over_the_generic_installation_match() {
    let matched = CrashSignatureCatalog::match_log(
        "Can't find model file 'dynamics\\equipments\\item_rukzak.ogf'",
        Some("cs"),
    );

    assert_eq!(matched.map(|signature| signature.id), Some("cs.missing-backpack-model"));
}

#[test]
fn save_doctor_reports_structure_and_keeps_unvalidated_semantics_unknown() {
    let report = analyze_save(SYNTHETIC_XRAY_SAVE);

    assert_eq!(report.status, SaveDoctorStatus::Ok);
    assert_eq!(report.format_id, Some("stalker-soc"));
    assert!(report.findings.iter().any(|finding| finding.id == "structure"));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.id == "semantic-state" && finding.severity == SaveDoctorStatus::Unknown));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.id == "repair" && finding.repair.is_none()));
}

#[test]
fn unsupported_save_is_an_error_and_never_suggests_a_repair() {
    let report = analyze_save(b"not a supported save");

    assert_eq!(report.status, SaveDoctorStatus::Error);
    assert_eq!(report.format_id, None);
    assert!(report.findings.iter().any(|finding| finding.id == "structure"));
    assert!(report.findings.iter().all(|finding| finding.repair.is_none()));
}

#[test]
fn crash_analysis_classifies_lua_errors_and_links_only_documented_issues() {
    let analysis = analyze_crash_log(
        "! [LUA][ERROR] ERROR: wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3",
        Some("cs"),
    );

    assert_eq!(analysis.kind, CrashKind::LuaError);
    assert_eq!(
        analysis.known_issue.map(|issue| issue.id),
        Some("cs.wrong-target-wild-napr")
    );
    assert!(analyze_crash_log("FATAL ERROR\n[error]Expression : 0", Some("cs"))
        .known_issue
        .is_none());
}

#[test]
fn large_crash_file_reads_the_tail_and_matches_without_loading_the_whole_log() {
    use std::io::{Seek, SeekFrom, Write};

    let path = std::env::temp_dir().join(format!("sse-doctor-large-log-{}.log", std::process::id()));
    let setup = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&path)?;
        file.set_len(50 * 1024 * 1024)?;
        file.seek(SeekFrom::End(-64))?;
        file.write_all(b"Insufficient smart_terrain jobs")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_file(&path);
        return;
    }

    let analysis = analyze_crash_file(&path, Some("cs"));

    let _ = std::fs::remove_file(&path);
    assert!(analysis.is_ok());
    assert_eq!(
        analysis
            .ok()
            .and_then(|result| result.known_issue.map(|issue| issue.id)),
        Some("cs.insufficient-smart-jobs")
    );
}

#[test]
fn crash_log_discovery_is_bounded_and_returns_newest_matching_files() {
    let directory = std::env::temp_dir().join(format!("sse-doctor-discovery-{}", std::process::id()));
    let logs = directory.join("_appdata_").join("logs");
    let setup = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&logs)?;
        std::fs::write(logs.join("xray_old.log"), b"old")?;
        std::fs::write(logs.join("xray_new.log"), b"new")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_dir_all(&directory);
        return;
    }

    let found = discover_crash_logs(&directory, 1);

    let _ = std::fs::remove_dir_all(&directory);
    assert!(found.is_ok());
    assert_eq!(
        found
            .ok()
            .and_then(|files| files.first().map(|file| file.name.as_str().to_owned())),
        Some("xray_new.log".to_owned())
    );
}

#[test]
fn crash_log_discovery_refuses_an_unbounded_result_limit() {
    let directory = std::env::temp_dir().join(format!("sse-doctor-empty-{}", std::process::id()));

    let result = discover_crash_logs(&directory, 0);

    assert!(matches!(result, Err(sse_core::Error::Refused(_))));
}

#[test]
fn crash_file_reader_refuses_minidump_bytes_until_a_dump_reader_is_connected() {
    let path = std::env::temp_dir().join(format!("sse-doctor-dump-{}.mdmp", std::process::id()));
    let write = std::fs::write(&path, b"MDMP\0\0\0\0");
    assert!(write.is_ok());
    if write.is_err() {
        return;
    }

    let result = analyze_crash_file(&path, Some("cs"));

    let _ = std::fs::remove_file(path);
    assert!(matches!(result, Err(sse_core::Error::Refused(_))));
}

#[test]
fn crash_file_reader_uses_the_dump_reader_trait_and_preserves_module_offsets() {
    struct SyntheticDumpReader;

    impl CrashDumpReader for SyntheticDumpReader {
        fn read_minidump(&self, _bytes: &[u8]) -> sse_core::Result<CrashDumpFacts> {
            Ok(CrashDumpFacts {
                message: "FATAL ERROR: access violation".to_owned(),
                faulting_module_offset: Some("xrRender_R1.dll+0x879a4".to_owned()),
            })
        }
    }

    let path = std::env::temp_dir().join(format!("sse-doctor-reader-{}.mdmp", std::process::id()));
    let write = std::fs::write(&path, b"MDMPsynthetic");
    assert!(write.is_ok());
    if write.is_err() {
        return;
    }

    let analysis = analyze_crash_file_with_dump_reader(&path, Some("cs"), Some(&SyntheticDumpReader));

    let _ = std::fs::remove_file(path);
    assert!(analysis.is_ok());
    assert_eq!(
        analysis.ok().map(|result| result.faulting_module_offset),
        Some(Some("xrRender_R1.dll+0x879a4".to_owned()))
    );
}

#[test]
fn minidump_reader_handles_truncations_and_deterministic_bit_flips() {
    struct SyntheticDumpReader;

    impl CrashDumpReader for SyntheticDumpReader {
        fn read_minidump(&self, _bytes: &[u8]) -> sse_core::Result<CrashDumpFacts> {
            Ok(CrashDumpFacts {
                message: "synthetic dump text".to_owned(),
                faulting_module_offset: None,
            })
        }
    }

    let path = std::env::temp_dir().join(format!("sse-doctor-mutations-{}.mdmp", std::process::id()));
    let source = b"MDMPsynthetic bytes";
    for length in 0..=source.len() {
        let Some(prefix) = source.get(..length) else {
            continue;
        };
        if std::fs::write(&path, prefix).is_err() {
            let _ = std::fs::remove_file(path);
            return;
        }
        assert!(analyze_crash_file_with_dump_reader(&path, Some("cs"), Some(&SyntheticDumpReader)).is_ok());
    }
    for index in 0..source.len() {
        for bit in 0..8_u32 {
            let mut changed = source.to_vec();
            if let Some(byte) = changed.get_mut(index) {
                *byte ^= 1_u8 << bit;
            }
            if std::fs::write(&path, changed).is_err() {
                let _ = std::fs::remove_file(path);
                return;
            }
            assert!(analyze_crash_file_with_dump_reader(&path, Some("cs"), Some(&SyntheticDumpReader)).is_ok());
        }
    }
    let _ = std::fs::remove_file(path);
}

#[test]
fn minidump_reader_refuses_a_hostile_length_before_allocating_the_dump() {
    use std::io::Write;

    let path = std::env::temp_dir().join(format!("sse-doctor-oversize-{}.mdmp", std::process::id()));
    let setup = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&path)?;
        file.set_len(64 * 1024 * 1024 + 1)?;
        file.write_all(b"MDMP")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_file(path);
        return;
    }

    let result = analyze_crash_file(&path, Some("cs"));

    let _ = std::fs::remove_file(path);
    assert!(matches!(result, Err(sse_core::Error::Refused(_))));
}

#[test]
fn local_vdf_parser_rejects_truncations_duplicate_fields_and_fixed_seed_mutations() {
    let source = "\"AppState\" { \"appid\" \"20510\" \"installdir\" \"Clear Sky\" \"buildid\" \"11450472\" }";
    assert!(parse_app_state(source).is_some());
    assert!(parse_app_state("\"AppState\" { \"appid\" \"20510\" \"appid\" \"41700\" }").is_none());

    for length in 0..source.len() {
        let Some(prefix) = source.get(..length) else {
            continue;
        };
        let _ = parse_app_state(prefix);
    }
    for seed in 0..256_usize {
        let mut bytes = source.as_bytes().to_vec();
        let slot = seed % bytes.len();
        if let Some(byte) = bytes.get_mut(slot) {
            *byte ^= u8::try_from(seed).unwrap_or(0);
        }
        let text = String::from_utf8_lossy(&bytes);
        let _ = parse_app_state(&text);
    }
}

#[test]
fn quest_rules_report_dead_npcs_without_flags_and_keep_unknown_facts_unresolved() {
    let npc_vitals = [QuestNpcVitals {
        section: "esc_wolf".to_owned(),
        is_dead: true,
    }];
    let known_info = Vec::<String>::new();
    let states = evaluate_quest_facts("stalker-cs", Some(&known_info), &npc_vitals);

    let wolf = states.iter().find(|state| state.id == "cs.wolf-dead");
    assert_eq!(wolf.map(|state| state.status), Some(QuestTaskStatus::Broken));
    assert_eq!(wolf.map(|state| state.missing_info), Some(Some("esc_wolf_dead")));
    assert!(wolf.is_some_and(|state| state.needs_preventing_fix));

    let missing_npc = evaluate_quest_facts("stalker-cs", Some(&known_info), &[]);
    assert!(missing_npc.iter().all(|state| state.status == QuestTaskStatus::Unknown));
}

#[test]
fn crash_text_reader_survives_truncations_and_deterministic_mutations() {
    let source = b"[LUA][ERROR] wrong target for storyline quest: logic@work5,gar_smart_terrain_6_3";
    for length in 0..=source.len() {
        let prefix = source.get(..length).map_or(&[][..], |prefix| prefix);
        let text = String::from_utf8_lossy(prefix);
        let _ = analyze_crash_log(&text, Some("cs"));
    }
    for seed in 0..256_usize {
        let mut mutated = source.to_vec();
        let slot = seed % source.len();
        if let Some(byte) = mutated.get_mut(slot) {
            *byte ^= u8::try_from(seed).unwrap_or(0);
        }
        let text = String::from_utf8_lossy(&mutated);
        let _ = analyze_crash_log(&text, Some("cs"));
    }
}

#[test]
fn game_build_fingerprints_are_scoped_to_each_supported_release() {
    assert_eq!(
        classify_game_build(GameTarget::Soc, Some("11567845")).status,
        GameBuildStatus::Verified
    );
    assert_eq!(
        classify_game_build(GameTarget::SocEe, Some("11567845")).status,
        GameBuildStatus::Unknown
    );
    assert_eq!(
        classify_game_build(GameTarget::CopEe, Some("24067133")).status,
        GameBuildStatus::Verified
    );
    assert_eq!(
        classify_game_build(GameTarget::Cs, Some("future-build")).status,
        GameBuildStatus::Unknown
    );
    assert_eq!(
        classify_game_build(GameTarget::Cs, None).status,
        GameBuildStatus::NotInstalled
    );
}

#[test]
fn game_doctor_checks_the_selected_install_marker_without_classifying_loose_files() {
    let directory = std::env::temp_dir().join(format!("sse-doctor-game-{}", std::process::id()));
    let setup = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join("fsgame_cs.ltx"), b"$game_data$ = true")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_dir_all(&directory);
        return;
    }

    let report = analyze_game_install(GameTarget::Cs, &directory, None);

    let _ = std::fs::remove_dir_all(&directory);
    assert_eq!(report.status, SaveDoctorStatus::Ok);
    assert!(report.marker_found);
    assert_eq!(report.build.status, GameBuildStatus::NotInstalled);
}

#[test]
fn game_doctor_reads_build_ids_only_from_a_matching_synthetic_steam_manifest() {
    let library = std::env::temp_dir().join(format!("sse-doctor-steam-{}", std::process::id()));
    let game = library.join("steamapps").join("common").join("Clear Sky");
    let setup = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&game)?;
        std::fs::write(game.join("fsgame_cs.ltx"), b"$game_data$ = true")?;
        std::fs::write(
            library.join("steamapps").join("appmanifest_20510.acf"),
            "\"AppState\" { \"appid\" \"20510\" \"installdir\" \"Clear Sky\" \"buildid\" \"11450472\" }",
        )
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_dir_all(&library);
        return;
    }

    let report = analyze_game_install_from_steam(GameTarget::Cs, &game);

    let _ = std::fs::remove_dir_all(&library);
    assert_eq!(report.status, SaveDoctorStatus::Ok);
    assert_eq!(report.build.build_id.as_deref(), Some("11450472"));
    assert_eq!(report.build.status, GameBuildStatus::Verified);
}

#[test]
fn game_doctor_refuses_an_oversized_synthetic_steam_manifest() {
    use std::io::Write;

    let library = std::env::temp_dir().join(format!("sse-doctor-steam-large-{}", std::process::id()));
    let game = library.join("steamapps").join("common").join("Clear Sky");
    let manifest = library.join("steamapps").join("appmanifest_20510.acf");
    let setup = (|| -> std::io::Result<()> {
        std::fs::create_dir_all(&game)?;
        std::fs::write(game.join("fsgame_cs.ltx"), b"$game_data$ = true")?;
        let mut file = std::fs::File::create(&manifest)?;
        file.set_len(1024 * 1024 + 1)?;
        file.write_all(b"\"AppState\"")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_dir_all(&library);
        return;
    }

    let report = analyze_game_install_from_steam(GameTarget::Cs, &game);

    let _ = std::fs::remove_dir_all(&library);
    assert_eq!(report.build.status, GameBuildStatus::NotInstalled);
    assert_eq!(report.build.build_id, None);
}

#[test]
#[ignore = "manual Release throughput and bounded-tail measurement"]
fn release_crash_log_50_mib_tail_throughput_measurement() {
    use std::io::{Seek, SeekFrom, Write};

    let path = std::env::temp_dir().join(format!("sse-doctor-bench-{}.log", std::process::id()));
    let setup = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&path)?;
        file.set_len(50 * 1024 * 1024)?;
        file.seek(SeekFrom::End(-128))?;
        file.write_all(b"! [LUA][ERROR] ERROR: Insufficient smart_terrain jobs")
    })();
    assert!(setup.is_ok());
    if setup.is_err() {
        let _ = std::fs::remove_file(&path);
        return;
    }
    let warmup = analyze_crash_file(&path, Some("cs"));
    assert!(warmup.is_ok());
    let iterations = 1_000_u32;
    let started = std::time::Instant::now();
    let mut matched = 0_u32;
    for _ in 0..iterations {
        let analysis = analyze_crash_file(&path, Some("cs"));
        assert!(analysis.is_ok());
        if analysis
            .ok()
            .and_then(|value| value.known_issue)
            .is_some_and(|issue| issue.id == "cs.insufficient-smart-jobs")
        {
            matched = matched.saturating_add(1);
        }
    }
    let elapsed = started.elapsed();
    let _ = std::fs::remove_file(path);
    println!(
        "S2 50 MiB crash-log file tail: {iterations} analyses in {elapsed:?}; {:.3} us/analysis; matched {matched}; read bound 256 KiB",
        elapsed.as_secs_f64() * 1_000_000.0 / f64::from(iterations)
    );
    assert_eq!(matched, iterations);
}

#[test]
fn quest_doctor_keeps_save_states_unknown_when_the_reader_exposes_no_quest_fields() {
    let report = analyze_quests(SYNTHETIC_XRAY_SAVE);

    assert_eq!(report.status, SaveDoctorStatus::Unknown);
    assert!(report.quest_states_available);
    assert_eq!(report.format_id, Some("stalker-soc"));
    assert!(report
        .states
        .iter()
        .all(|state| state.status == QuestTaskStatus::Unknown));
    assert!(report.states.iter().all(|state| state.missing_info.is_none()));
}
