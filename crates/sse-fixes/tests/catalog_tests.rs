//! Game fix catalog integration tests.

// Tests use unwrap/expect to fail fast on assertion errors, and index into known-length
// collections. These patterns are intentional in test code.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_fixes::*;

#[test]
fn lookup_presets_and_counts_cover_exactly_what_a_game_lists_including_enhanced_editions() {
    let targets = [
        GameTarget::ShadowOfChernobyl,
        GameTarget::ClearSky,
        GameTarget::CallOfPripyat,
        GameTarget::ShadowOfChernobylEnhancedEdition,
        GameTarget::ClearSkyEnhancedEdition,
        GameTarget::CallOfPripyatEnhancedEdition,
        GameTarget::Stalker2,
    ];

    for &game in &targets {
        let listed = GameFixCatalog::for_game(game);
        for fix in &listed {
            let found = GameFixCatalog::try_get(&fix.id);
            assert!(found.is_some(), "Fix id: {}", fix.id);
            assert_eq!(found.unwrap().game, game);
        }

        let cat_counts = GameFixCatalog::category_counts(game);
        let sum_counts: usize = cat_counts.values().copied().sum();
        assert_eq!(listed.len(), sum_counts);

        let recommended = GameFixCatalog::for_preset(game, GameFixPreset::Recommended);
        for fix in &recommended {
            assert!(listed.iter().any(|item| item.id == fix.id));
        }

        let mut expected_recommended: Vec<&str> = listed
            .iter()
            .filter(|fix| {
                fix.maturity == GameFixMaturity::Validated
                    && (fix.category == GameFixCategory::Essential || fix.category == GameFixCategory::Recommended)
            })
            .map(|fix| fix.id.as_str())
            .collect();
        expected_recommended.sort();

        let mut actual_recommended: Vec<&str> = recommended.iter().map(|fix| fix.id.as_str()).collect();
        actual_recommended.sort();

        assert_eq!(expected_recommended, actual_recommended);
    }

    assert!(!GameFixCatalog::for_preset(GameTarget::ClearSkyEnhancedEdition, GameFixPreset::Recommended).is_empty());
}

#[test]
fn all_spawn_fix_is_structured_and_pinned_to_the_retail_file() {
    let all = GameFixCatalog::all();
    let fix = all
        .iter()
        .find(|c| c.id == "cs.crash.all-spawn-errors")
        .expect("cs.crash.all-spawn-errors should exist");

    assert_eq!(fix.implementation, GameFixImplementationType::Structured);
    assert!(fix.text_patches.is_empty());
    assert_eq!(fix.spawn_edits.len(), 43);

    for edit in &fix.spawn_edits {
        assert_eq!(edit.relative_path, "gamedata/spawns/all.spawn");
        assert_eq!(edit.expected_file_sha256.as_ref().unwrap().len(), 64);
        assert!(!edit.expected.is_empty());
    }

    let cordon = fix
        .spawn_edits
        .iter()
        .find(|edit| edit.target == "esc_smart_terrain_3_7_walker_1_walk")
        .expect("cordon waypoint edit should exist");
    assert_eq!(cordon.level_vertex_id, Some(135001));

    let mil = fix
        .spawn_edits
        .iter()
        .find(|edit| edit.target == "mil_smart_terrain_2_1")
        .expect("military edit should exist");
    assert!(mil.replacement.as_ref().unwrap().contains("squad_capacity = 1"));

    assert!(!all.iter().any(|c| c.id == "cs.crash.all-spawn-errors.ee"));
}

#[test]
fn shipped_catalogue_contains_archive_verified_clear_sky_fixes_and_populates_safe_presets() {
    let fixes = GameFixCatalog::for_game(GameTarget::ClearSky);
    assert_eq!(fixes.len(), 75);

    let fix = fixes
        .iter()
        .find(|c| c.id == "cs.quest.dead-wild-napr")
        .expect("cs.quest.dead-wild-napr should exist");

    assert_eq!(fix.id, "cs.quest.dead-wild-napr");
    assert_eq!(fix.category, GameFixCategory::Essential);
    assert_eq!(fix.maturity, GameFixMaturity::Validated);
    assert_eq!(fix.verification_state, GameFixVerificationState::RetailFilesVerified);
    assert_eq!(fix.supported_steam_build_ids, vec!["11450472"]);
    assert_eq!(fix.implementation, GameFixImplementationType::ExactTextReplacement);
    assert_eq!(fix.text_patches.len(), 1);
    assert_eq!(fix.text_patches[0].expected_file_sha256.as_ref().unwrap().len(), 64);

    for candidate in &fixes {
        assert_eq!(
            candidate.verification_state,
            GameFixVerificationState::RetailFilesVerified
        );
        assert_eq!(candidate.maturity, GameFixMaturity::Validated);
        assert_eq!(candidate.supported_steam_build_ids, vec!["11450472"]);
        for op in &candidate.text_patches {
            assert_eq!(op.expected_file_sha256.as_ref().unwrap().len(), 64);
        }
    }

    assert!(!GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::EssentialOnly).is_empty());
    assert!(!GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::Recommended).is_empty());
    assert_eq!(
        GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::Recommended).len(),
        73
    );
    assert!(
        GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::Recommended)
            .iter()
            .any(|c| c.id == fix.id)
    );
    assert!(
        !GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::Recommended)
            .iter()
            .any(|c| c.category == GameFixCategory::Community)
    );

    assert!(GameFixCatalog::for_game(GameTarget::Stalker2).is_empty());
    assert!(!GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::AllSafeFixes).is_empty());
    assert!(GameFixCatalog::for_preset(GameTarget::ClearSky, GameFixPreset::Custom).is_empty());
}

#[test]
fn community_dialogue_and_sniper_fixes_target_their_exact_terminal_sections() {
    let dialog2_fix = GameFixCatalog::try_get("cs.dialog.escape-2-level-changers").unwrap();
    let dialog2 = dialog2_fix
        .text_patches
        .iter()
        .find(|p| p.relative_path.ends_with("esc_pda_dialog_2.ltx") && p.expected_text.starts_with("[sr_idle"))
        .unwrap();
    assert_eq!(dialog2.expected_text, "[sr_idle@7]\r\n");
    assert_eq!(
        dialog2.replacement_text,
        "[sr_idle@7]\r\non_signal = sound_end | nil %=enable_level_changer(443) =enable_level_changer(445)%\r\n"
    );

    let dialog4_fix = GameFixCatalog::try_get("cs.dialog.escape-4-level-changer").unwrap();
    let dialog4 = dialog4_fix
        .text_patches
        .iter()
        .find(|p| p.relative_path.ends_with("esc_pda_dialog_4.ltx") && p.expected_text.starts_with("[sr_idle"))
        .unwrap();
    assert_eq!(dialog4.expected_text, "[sr_idle@14]\r\n");
    assert_eq!(
        dialog4.replacement_text,
        "[sr_idle@14]\r\non_signal = sound_end | nil %=enable_level_changer(444)%\r\n"
    );

    let sniper_patches = &GameFixCatalog::try_get("cs.quest.verified-hospital-sniper-danger-keys")
        .unwrap()
        .text_patches;
    assert_eq!(sniper_patches.len(), 2);
    assert!(sniper_patches
        .iter()
        .any(|p| p.expected_text.starts_with("[danger_condition]\r\n")));
    assert!(sniper_patches
        .iter()
        .any(|p| p.expected_text.starts_with("[danger_condition@2]\r\n")));
}

#[test]
fn no_game_file_is_claimed_by_two_fixes() {
    use std::collections::HashMap;

    for &game in &[
        GameTarget::ShadowOfChernobyl,
        GameTarget::ClearSky,
        GameTarget::CallOfPripyat,
        GameTarget::ShadowOfChernobylEnhancedEdition,
        GameTarget::ClearSkyEnhancedEdition,
        GameTarget::CallOfPripyatEnhancedEdition,
    ] {
        let fixes = GameFixCatalog::for_game(game);
        let mut path_owners: HashMap<String, Vec<&str>> = HashMap::new();

        for fix in &fixes {
            for path in GameFixEngine::managed_paths(fix) {
                path_owners.entry(path.to_lowercase()).or_default().push(&fix.id);
            }
        }

        for (path, owners) in path_owners {
            let mut unique_owners = owners.clone();
            unique_owners.sort();
            unique_owners.dedup();
            assert_eq!(
                unique_owners.len(),
                1,
                "Game {:?}, path {} claimed by multiple fixes: {:?}",
                game,
                path,
                unique_owners
            );
        }
    }
}

#[test]
fn a_retail_only_patch_is_left_out_of_the_enhanced_edition_variant() {
    let retail = GameFixCatalog::try_get("cs.crash.treasure-given-twice").unwrap();
    let enhanced = GameFixCatalog::try_get("cs.crash.treasure-given-twice.ee").unwrap();

    assert!(retail.text_patches.iter().any(|p| p.retail_only));
    assert!(!enhanced.text_patches.iter().any(|p| p.retail_only));
    assert_eq!(
        retail.text_patches.iter().filter(|p| !p.retail_only).count(),
        enhanced.text_patches.len()
    );
}

#[test]
fn shipped_soc_catalogue_contains_only_retail_verified_zrp_bug_fixes() {
    let fixes = GameFixCatalog::for_game(GameTarget::ShadowOfChernobyl);
    assert_eq!(fixes.len(), 35);

    for fix in &fixes {
        assert_eq!(fix.supported_steam_build_ids, vec!["11567845"]);
        assert_eq!(fix.maturity, GameFixMaturity::Validated);
        assert_eq!(fix.verification_state, GameFixVerificationState::RetailFilesVerified);
        assert!(fix.source.to_lowercase().contains("zrp") || fix.source.contains("static check"));
        assert!(fix
            .references
            .iter()
            .any(|r| { r.contains("metacognix.com/stlkrsoc") || r.contains("tools/lua_globals.py") }));
        for p in &fix.text_patches {
            assert_eq!(p.expected_file_sha256.as_ref().unwrap().len(), 64);
        }
    }

    assert!(GameFixCatalog::for_preset(GameTarget::ShadowOfChernobyl, GameFixPreset::Recommended).len() >= 15);
}

#[test]
fn shipped_cop_catalogue_contains_retail_verified_fixes_and_includes_community_in_all_safe() {
    let fixes = GameFixCatalog::for_game(GameTarget::CallOfPripyat);
    assert_eq!(fixes.len(), 36);

    for fix in &fixes {
        assert_eq!(fix.supported_steam_build_ids, vec!["11450453"]);
        assert_eq!(fix.maturity, GameFixMaturity::Validated);
        assert_eq!(fix.verification_state, GameFixVerificationState::RetailFilesVerified);
        for p in &fix.text_patches {
            assert_eq!(p.expected_file_sha256.as_ref().unwrap().len(), 64);
        }
    }

    let recommended = GameFixCatalog::for_preset(GameTarget::CallOfPripyat, GameFixPreset::Recommended);
    assert_eq!(recommended.len(), 22);

    assert!(recommended.iter().any(|f| f.id == "cop.weapon.spas12-sight-alignment"));
    assert!(recommended.iter().any(|f| f.id == "cop.weapon.val-sight-alignment"));
    assert!(recommended.iter().any(|f| f.id == "cop.dialog.correct-anomaly-name"));
    assert!(recommended
        .iter()
        .any(|f| f.id == "cop.dialog.gonta-after-soroka-recovered"));
    assert!(recommended
        .iter()
        .any(|f| f.id == "cop.quest.memory-module-unlock-attribution"));
    assert!(recommended.iter().any(|f| f.id == "cop.prp.crow-counter-guard"));
    assert!(recommended.iter().any(|f| f.id == "cop.prp.x8-burer-health-guard"));
    assert!(recommended.iter().any(|f| f.id == "cop.prp.jupiter-scanner-task-guard"));
    assert!(recommended
        .iter()
        .any(|f| f.id == "cop.prp.altered-insulator-door-gate"));
    assert!(recommended.iter().any(|f| f.id == "cop.prp.sky-stretching-fix"));
    assert!(!recommended.iter().any(|f| f.id == "cop.prp.knife-hit-reach"));

    assert!(
        GameFixCatalog::for_preset(GameTarget::CallOfPripyat, GameFixPreset::AllSafeFixes)
            .iter()
            .any(|f| f.category == GameFixCategory::Community)
    );

    assert_eq!(
        GameFixCatalog::previous_preset_count(GameTarget::CallOfPripyat, GameFixPreset::Recommended),
        10
    );

    let russian_patches: Vec<&TextPatchOperation> = fixes
        .iter()
        .flat_map(|f| &f.text_patches)
        .filter(|p| p.relative_path.contains("/text/rus/"))
        .collect();
    assert!(!russian_patches.is_empty());
    for p in russian_patches {
        assert_eq!(p.code_page, 1251);
    }
}

#[test]
fn each_edition_gets_the_variants_verified_against_its_files() {
    let cases = [
        (GameTarget::ShadowOfChernobylEnhancedEdition, "24067120", 20),
        (GameTarget::ClearSkyEnhancedEdition, "24067129", 51),
        (GameTarget::CallOfPripyatEnhancedEdition, "24067133", 25),
    ];

    for (game, build, count) in cases {
        let fixes = GameFixCatalog::for_game(game);
        assert_eq!(fixes.len(), count, "Game {:?}", game);
        for fix in &fixes {
            assert!(fix.id.ends_with(".ee"), "Fix: {}", fix.id);
            assert_eq!(fix.supported_steam_build_ids, vec![build]);
            for patch in &fix.text_patches {
                assert_eq!(
                    patch.expected_file_sha256.as_ref().unwrap().len(),
                    64,
                    "Fix: {}",
                    fix.id
                );
            }
        }
    }
}

#[test]
fn a_variant_keeps_the_retail_anchors_and_changes_only_the_hashes() {
    let retail = GameFixCatalog::try_get("soc.quest.petruha-report-once").unwrap();
    let ee = GameFixCatalog::try_get("soc.quest.petruha-report-once.ee").unwrap();

    assert_eq!(ee.game, GameTarget::ShadowOfChernobylEnhancedEdition);
    assert_eq!(retail.text_patches.len(), ee.text_patches.len());
    for (r, e) in retail.text_patches.iter().zip(&ee.text_patches) {
        assert_eq!(r.relative_path, e.relative_path);
        assert_eq!(r.expected_text, e.expected_text);
        assert_eq!(r.replacement_text, e.replacement_text);
    }
    assert_ne!(
        retail.text_patches[0].expected_file_sha256,
        ee.text_patches[0].expected_file_sha256
    );
}

#[test]
fn fixes_already_fixed_in_enhanced_edition_get_no_variant() {
    assert!(GameFixCatalog::try_get("cs.ai.snork-aggression-key.ee").is_none());
    assert!(GameFixCatalog::try_get("cop.prp.knife-hit-reach.ee").is_none());

    let all = GameFixCatalog::all();
    let mut ids: Vec<&str> = all.iter().map(|f| f.id.as_str()).collect();
    let total = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(total, ids.len());
}

#[test]
fn embedded_catalog_reports_successful_load() {
    assert!(GameFixCatalog::load_error().is_none());
    assert!(!GameFixCatalog::all().is_empty());
}
