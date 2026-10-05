use crate::{
    analyze_crash_file, analyze_crash_file_with_dump_reader, analyze_crash_log, analyze_game_install,
    analyze_game_install_from_steam, analyze_quests, analyze_quests_from_save, analyze_save, classify_game_build,
    discover_crash_logs, evaluate_quest_facts, parse_app_state, prepare_quest_repair, verify_quest_repair, CrashAdvice,
    CrashDumpFacts, CrashDumpReader, CrashKind, CrashSignatureCatalog, GameBuildStatus, GameTarget, QuestNpcVitals,
    QuestTaskStatus, SaveDoctorStatus,
};

const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../fixtures/synthetic/xray-soc.sav");

#[test]
fn crash_signature_catalog_contains_every_reference_entry() {
    let signatures = CrashSignatureCatalog::all();
    assert_eq!(
        signatures.len(),
        47,
        "the checked-in C# 1.3.1 source has 47 catalog entries"
    );
    assert!(signatures.iter().any(|signature| signature.game == "cs"));
    assert!(signatures.iter().any(|signature| signature.game == "soc"));
    assert!(signatures.iter().any(|signature| signature.game == "any"));
    for (index, signature) in signatures.iter().enumerate() {
        assert!(signatures.iter().take(index).all(|other| other.id != signature.id));
        assert!(!signature.pattern.is_empty());
    }
}

#[test]
fn every_reference_regex_branch_compiles_and_matches_its_synthetic_example() {
    let automaton = super::AUTOMATON.get_or_init(super::build_automaton);
    assert_eq!(automaton.programs.len(), CrashSignatureCatalog::all().len());
    for (signature_index, program) in automaton.programs.iter().enumerate() {
        let Some(signature) = CrashSignatureCatalog::all().get(signature_index) else {
            return;
        };
        assert!(
            !program.variants.is_empty(),
            "signature {signature_index} did not compile"
        );
        for (variant_index, variant) in program.variants.iter().enumerate() {
            assert!(
                !variant.anchor.is_empty(),
                "signature {signature_index} has no literal prefix"
            );
            assert!(
                variant.matches_at(&variant.example, 0),
                "signature {signature_index} pattern did not match its generated branch example"
            );
            let Some(state) = variant.anchor.iter().try_fold(0_u32, |state, byte| {
                automaton
                    .nodes
                    .get(super::state_index(state))
                    .and_then(|node| node.transitions.get(usize::from(*byte)))
                    .copied()
            }) else {
                panic!("signature {} has no automaton path", signature.id);
            };
            assert!(
                automaton.nodes.get(super::state_index(state)).is_some_and(|node| {
                    node.outputs.iter().any(|output| {
                        output.signature_index == signature_index && output.variant_index == variant_index
                    })
                }),
                "signature {} was not reachable from its automaton anchor",
                signature.id
            );
        }
    }
}

#[test]
fn reference_regexes_reject_malformed_variable_fields() {
    assert!(CrashSignatureCatalog::match_log(
        "sim_combat.script:not-a-line: attempt to index field 'actor' (a nil value)",
        Some("cs")
    )
    .is_none());
    assert!(CrashSignatureCatalog::match_log(
        "There is no squad [red_pursuit_bounty_hunters_squad_x] in sim_board",
        Some("cs")
    )
    .is_none());
    assert!(CrashSignatureCatalog::match_log("entity not found. id_parent=x id_entity=2", Some("soc")).is_none());
}

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
fn every_documented_csharp_catalog_example_resolves_to_the_same_signature() {
    let examples = [
        (
            "[error]Arguments     : LUA error: ...\\sim_combat.script:419: attempt to index field 'actor' (a nil value)",
            Some("cs"),
            "cs.sim-combat-actor-nil",
        ),
        (
            "smart_terrain.script:483: Insufficient smart_terrain jobs val_smart_terrain_9_6",
            Some("cs"),
            "cs.insufficient-smart-jobs",
        ),
        (
            "ERROR: cant find animation for slot 8",
            Some("cs"),
            "cs.hospital-jump-down-animation",
        ),
        (
            "LUA error: ... clear sky\\gamedata\\scripts\\sim_squad_generic.script:1184: attempt to index field '?' (a nil value)",
            Some("cs"),
            "cs.squad-hint-unknown-target",
        ),
        (
            "LUA error: xr_logic: pstor_load_all: not registered type N 147 encountered",
            Some("cs"),
            "cs.pstor-unknown-type",
        ),
        (
            "[error]Description   : entity not found. id_parent=1350 id_entity=1312 frame=11471",
            Some("soc"),
            "soc.entity-not-found",
        ),
        (
            "- Critical: SMapLocation binded to non-existent object id=4242",
            Some("soc"),
            "soc.map-location-dead-object",
        ),
        (
            "Can't find model file 'monsters\\up_monsters\\pseudodog_noah.ogf'.",
            Some("cop"),
            "any.missing-model",
        ),
        (
            "Can't open section 'wpn_pm_actor'",
            Some("cop"),
            "any.missing-section",
        ),
        (
            "Can't find variable night_vision in [device_torch]",
            Some("soc"),
            "any.missing-config-value",
        ),
        (
            "[error]Arguments : string table xml file not found string_table_includes.xml",
            Some("soc"),
            "any.missing-string-table",
        ),
        (
            "[error]Expression    : hFile>0\n[error]Function      : FileDownload",
            Some("cs"),
            "any.config-not-opened",
        ),
    ];

    for (log, game, expected_id) in examples {
        let signatures = CrashSignatureCatalog::all();
        let signature_index = signatures.iter().position(|signature| signature.id == expected_id);
        assert!(
            signature_index.is_some(),
            "documented C# signature exists in the Rust catalog"
        );
        let signature_index = signature_index.unwrap_or_default();
        let automaton = super::AUTOMATON.get_or_init(super::build_automaton);
        let regex_matches = automaton.programs.get(signature_index).is_some_and(|program| {
            program.variants.iter().any(|variant| {
                log.as_bytes()
                    .windows(variant.anchor.len())
                    .enumerate()
                    .any(|(start, window)| {
                        window.eq_ignore_ascii_case(&variant.anchor) && variant.matches_at(log.as_bytes(), start)
                    })
            })
        });
        assert!(regex_matches, "reference regex did not match: {expected_id}: {log}");
        assert_eq!(
            CrashSignatureCatalog::match_log(log, game).map(|signature| signature.id),
            Some(expected_id),
            "game {game:?}: {log}"
        );
        assert_eq!(
            CrashSignatureCatalog::match_log(log, None).map(|signature| signature.id),
            Some(expected_id),
            "game agnostic: {log}"
        );
    }
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

    let result = analyze_crash_file_with_dump_reader(&path, Some("cs"), None);

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
fn shared_vdf_parser_handles_truncations_duplicate_fields_and_fixed_seed_mutations() {
    let source = "\"AppState\" { \"appid\" \"20510\" \"installdir\" \"Clear Sky\" \"buildid\" \"11450472\" }";
    assert!(parse_app_state(source).is_some());
    let duplicate = parse_app_state("\"AppState\" { \"appid\" \"20510\" \"appid\" \"41700\" }");
    assert_eq!(
        duplicate.as_ref().and_then(|fields| fields.app_id.as_deref()),
        Some("41700")
    );

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
fn quest_doctor_keeps_unmatched_npc_facts_unknown() -> sse_core::Result<()> {
    let report = analyze_quests(SYNTHETIC_XRAY_SAVE);

    assert_eq!(report.status, SaveDoctorStatus::Unknown);
    assert!(report.quest_states_available);
    assert_eq!(report.format_id, Some("stalker-soc"));
    assert!(report
        .states
        .iter()
        .all(|state| state.status == QuestTaskStatus::Unknown));
    assert!(report.states.iter().all(|state| state.missing_info.is_none()));
    assert!(prepare_quest_repair(SYNTHETIC_XRAY_SAVE)?.is_none());
    verify_quest_repair(SYNTHETIC_XRAY_SAVE)?;
    Ok(())
}

#[test]
fn quest_doctor_can_evaluate_an_already_parsed_save() -> sse_core::Result<()> {
    let parsed = sse_xray::Save::read(SYNTHETIC_XRAY_SAVE)?;
    let from_bytes = analyze_quests(SYNTHETIC_XRAY_SAVE);
    let from_index = analyze_quests_from_save(&parsed);

    assert_eq!(from_index.format_id, from_bytes.format_id);
    assert_eq!(from_index.status, from_bytes.status);
    assert_eq!(from_index.states, from_bytes.states);
    Ok(())
}

#[test]
fn default_crash_reader_extracts_exception_and_faulting_module_from_a_minidump() {
    fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
        if let Some(target) = bytes.get_mut(offset..offset.saturating_add(4)) {
            target.copy_from_slice(&value.to_le_bytes());
        }
    }
    fn put_u64(bytes: &mut [u8], offset: usize, value: u64) {
        if let Some(target) = bytes.get_mut(offset..offset.saturating_add(8)) {
            target.copy_from_slice(&value.to_le_bytes());
        }
    }
    fn write_entry(bytes: &mut [u8], offset: usize, stream: u32, size: u32, rva: u32) {
        put_u32(bytes, offset, stream);
        put_u32(bytes, offset.saturating_add(4), size);
        put_u32(bytes, offset.saturating_add(8), rva);
    }

    let module_name = r"S:\game\bin\xrCore.dll".encode_utf16().collect::<Vec<_>>();
    let module_bytes = module_name
        .iter()
        .flat_map(|character| character.to_le_bytes())
        .collect::<Vec<_>>();
    let name_offset = 32 + 24 + 112 + 168;
    let mut dump = vec![0_u8; name_offset + 4 + module_bytes.len()];
    put_u32(&mut dump, 0, 0x504D_444D);
    put_u32(&mut dump, 8, 2);
    put_u32(&mut dump, 12, 32);
    write_entry(&mut dump, 32, 4, 112, 56);
    write_entry(&mut dump, 44, 6, 168, 168);
    put_u32(&mut dump, 56, 1);
    put_u64(&mut dump, 60, 0x1_0000_0000);
    put_u32(&mut dump, 68, 0x20_0000);
    put_u32(&mut dump, 80, u32::try_from(name_offset).unwrap_or_default());
    put_u32(&mut dump, 176, 0x8000_0003);
    put_u64(&mut dump, 192, 0x1_0001_B944);
    put_u32(
        &mut dump,
        name_offset,
        u32::try_from(module_bytes.len()).unwrap_or_default(),
    );
    if let Some(target) = dump.get_mut(name_offset.saturating_add(4)..) {
        target.copy_from_slice(&module_bytes);
    }

    let path = std::env::temp_dir().join(format!("sse-doctor-default-dump-{}.mdmp", std::process::id()));
    if let Err(error) = std::fs::write(&path, dump) {
        panic!("write synthetic dump: {error}");
    }
    let analysis = analyze_crash_file(&path, Some("cs"));
    let _ = std::fs::remove_file(&path);
    let analysis = match analysis {
        Ok(value) => value,
        Err(error) => panic!("default minidump reader should parse the fixture: {error}"),
    };
    assert_eq!(analysis.faulting_module_offset.as_deref(), Some("xrCore.dll+0x1B944"));
    assert!(analysis.summary.contains("0x80000003"));
}
