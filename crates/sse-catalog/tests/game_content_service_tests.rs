//! Integration tests for game content service and catalog builder.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{GameContentService, InstalledGameCatalogBuilder};
use sse_content::CompanionGame;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[test]
fn maps_categories_and_serializer_families_like_the_oracle() {
    let cases = [
        ("wpn_ak74", "WP_AK74", "weapon", "weapon_wgl"),
        ("wpn_bm16", "WP_BM16", "weapon", "weapon_shotgun"),
        ("wpn_pm", "WP_PM", "weapon", "weapon_magazined"),
        ("ammo_9x18_fmj", "AMMO", "ammo", "ammo"),
        ("device_torch", "TORCH_S", "device", "torch"),
        ("stalker_outfit", "E_STLK", "outfit", "outfit"),
        ("medkit", "II_MEDKI", "consumable", "base"),
    ];

    for (name, class_name, expected_category, expected_family) in cases {
        let mut values = HashMap::new();
        values.insert("class".to_string(), class_name.to_string());
        values.insert("inv_name".to_string(), "x".to_string());

        let category = InstalledGameCatalogBuilder::category(name, &values);
        assert_eq!(category, Some(expected_category), "Failed category for {name}");

        let family = InstalledGameCatalogBuilder::serialization_family(name, &values, category);
        assert_eq!(family, expected_family, "Failed family for {name}");
    }
}

#[test]
fn icon_cache_names_differ_for_keys_that_sanitise_to_the_same_text() {
    let colliding_keys = ["weapon/a", "weapon:a", "weapon?a", "weapon_a"];
    let mut names = Vec::new();

    for key in colliding_keys {
        let name = GameContentService::icon_cache_file_name(key);
        assert!(name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'));
        names.push(name);
    }

    let mut deduplicated = names.clone();
    deduplicated.sort();
    deduplicated.dedup();
    assert_eq!(deduplicated.len(), names.len());
    assert_eq!(
        GameContentService::icon_cache_file_name("weapon/a"),
        GameContentService::icon_cache_file_name("weapon/a")
    );
}

#[test]
fn builds_and_caches_a_catalog_with_items_upgrades_factions_and_icons() {
    let temp_root = std::env::temp_dir().join(format!("se-content-{}", std::process::id()));
    let game_dir = temp_root.join("game");
    let cache_dir = temp_root.join("cache");

    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    fs::write(
        game_dir.join("fsgame.ltx"),
        "$game_data$ = false| true| $fs_root$| gamedata\\\n\
         $game_config$ = true| false| $game_data$| configs\\\n\
         $arch_dir_resources$ = false| false| $fs_root$| resources\\\n",
    )
    .unwrap();

    write_file(
        &game_dir,
        "gamedata/configs/system.ltx",
        "#include \"misc\\items.ltx\"\n\
         #include \"creatures\\game_relations.ltx\"\n\
         #include \"misc\\inventory_upgrades.ltx\"\n",
    );

    write_file(
        &game_dir,
        "gamedata/configs/misc/items.ltx",
        "[medkit]\n\
         class = II_MEDKI\n\
         inv_name = st_medkit\n\
         inv_weight = 0.5\n\
         cost = 1250\n\
         inv_grid_width = 1\n\
         inv_grid_height = 1\n\
         inv_grid_x = 1\n\
         inv_grid_y = 0\n\
         [wpn_pm]\n\
         class = WP_PM\n\
         inv_name = st_pm\n\
         inv_grid_width = 2\n\
         inv_grid_height = 1\n\
         inv_grid_x = 0\n\
         inv_grid_y = 1\n\
         [wpn_pm_hud]\n\
         class = WP_PM\n\
         inv_name = st_pm\n",
    );

    write_file(
        &game_dir,
        "gamedata/configs/misc/inventory_upgrades.ltx",
        "#include \"..\\weapons\\upgrades\\w_pm_up.ltx\"\n\
         [upgraded_inventory]\n\
         wpn_pm\n",
    );

    write_file(
        &game_dir,
        "gamedata/configs/weapons/upgrades/w_pm_up.ltx",
        "[up_a_pm]\n\
         section = up_sect_a_pm\n\
         name = st_up_a_pm\n",
    );

    write_file(
        &game_dir,
        "gamedata/configs/creatures/game_relations.ltx",
        "[game_relations]\n\
         communities = stalker, 0, bandit, 1\n\
         [communities_relations]\n\
         stalker = 0, -1000\n\
         bandit = -1000, 0\n\
         [action_points]\n\
         community_goodwill_limits = -5000, 5000\n",
    );

    write_file(
        &game_dir,
        "gamedata/configs/text/rus/st_items.xml",
        "<string_table>\
           <string id=\"st_medkit\"><text>Аптечка</text></string>\
           <string id=\"st_pm\"><text>ПМ</text></string>\
           <string id=\"st_up_a_pm\"><text>Ствол</text></string>\
           <string id=\"stalker\"><text>Одиночки</text></string>\
         </string_table>",
    );

    let dds = solid_dxt1(100, 100, false);
    let dds_dir = game_dir.join("gamedata/textures/ui");
    fs::create_dir_all(&dds_dir).unwrap();
    fs::write(dds_dir.join("ui_icon_equipment.dds"), dds).unwrap();

    let first = GameContentService::load(CompanionGame::CallOfPripyat, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();

    assert!(!first.status().from_cache);
    assert_eq!(first.status().item_count, 2);
    assert!(first.bundle().items.resolve("wpn_pm_hud").is_none());
    assert_eq!(
        first.bundle().items.resolve("medkit").unwrap().display_name.as_deref(),
        Some("Аптечка")
    );
    assert_eq!(first.bundle().items.resolve("medkit").unwrap().cost, Some(1250));
    assert_eq!(
        first
            .bundle()
            .items
            .resolve("wpn_pm")
            .unwrap()
            .serialization_family
            .as_deref(),
        Some("weapon_magazined")
    );
    assert_eq!(
        first
            .bundle()
            .upgrades
            .as_ref()
            .unwrap()
            .resolve("up_a_pm")
            .unwrap()
            .item_key
            .as_deref(),
        Some("wpn_pm")
    );
    assert_eq!(
        first
            .bundle()
            .factions
            .as_ref()
            .unwrap()
            .default_relation("stalker", "bandit")
            .unwrap(),
        Some(-1000)
    );
    assert_eq!(first.bundle().factions.as_ref().unwrap().goodwill_max(), Some(5000));

    let png = first.icon_png("wpn_pm").unwrap();
    assert_eq!(png[0], 0x89);
    let width_bytes = [png[16], png[17], png[18], png[19]];
    let height_bytes = [png[20], png[21], png[22], png[23]];
    assert_eq!(u32::from_be_bytes(width_bytes), 100);
    assert_eq!(u32::from_be_bytes(height_bytes), 50);

    let second = GameContentService::load(CompanionGame::CallOfPripyat, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();
    assert!(second.status().from_cache);
    assert_eq!(
        second.bundle().items.resolve("wpn_pm").unwrap().display_name.as_deref(),
        Some("ПМ")
    );
    assert_eq!(second.bundle().items.resolve("wpn_pm").unwrap().width, Some(2));

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn changing_a_game_file_invalidates_the_cache() {
    let temp_root = std::env::temp_dir().join(format!("se-content-inv-{}", std::process::id()));
    let game_dir = temp_root.join("game");
    let cache_dir = temp_root.join("cache");

    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    fs::write(
        game_dir.join("fsgame.ltx"),
        "$game_data$ = false| true| $fs_root$| gamedata\\\n\
         $game_config$ = true| false| $game_data$| configs\\\n\
         $arch_dir$ = false| false| $fs_root$\n",
    )
    .unwrap();

    write_file(
        &game_dir,
        "gamedata/configs/system.ltx",
        "[medkit]\nclass = II_MEDKI\ninv_name = a\n",
    );

    let first = GameContentService::load(CompanionGame::CallOfPripyat, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();
    assert!(!first.status().from_cache);

    let second = GameContentService::load(CompanionGame::CallOfPripyat, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();
    assert!(second.status().from_cache);

    // Modify file
    write_file(
        &game_dir,
        "gamedata/configs/system.ltx",
        "[medkit]\nclass = II_MEDKI\ninv_name = b\n[bandage]\nclass = II_BANDG\ninv_name = c\n",
    );

    let rebuilt = GameContentService::load(CompanionGame::CallOfPripyat, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();
    assert!(!rebuilt.status().from_cache);
    assert_eq!(rebuilt.status().item_count, 2);

    let _ = fs::remove_dir_all(&temp_root);
}

#[test]
fn detects_a_known_community_mod_by_its_files() {
    let temp_root = std::env::temp_dir().join(format!("se-content-mod-{}", std::process::id()));
    let game_dir = temp_root.join("game");
    let cache_dir = temp_root.join("cache");

    let _ = fs::remove_dir_all(&temp_root);
    fs::create_dir_all(&game_dir).unwrap();
    fs::create_dir_all(&cache_dir).unwrap();

    fs::write(
        game_dir.join("fsgame.ltx"),
        "$game_data$ = false| true| $fs_root$| gamedata\\\n\
         $game_config$ = true| false| $game_data$| configs\\\n\
         $arch_dir$ = false| false| $fs_root$\n",
    )
    .unwrap();

    write_file(
        &game_dir,
        "gamedata/configs/system.ltx",
        "[medkit]\nclass = II_MEDKI\ninv_name = a\n",
    );
    write_file(&game_dir, "gamedata/OGSM_CS_info.ltx", "; mod");

    let loaded = GameContentService::load(CompanionGame::ClearSky, &game_dir, &cache_dir, "ru")
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status().mod_name.as_deref(), Some("OGSM"));

    let _ = fs::remove_dir_all(&temp_root);
}

fn write_file(root: &Path, rel: &str, content: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, content).unwrap();
}

fn solid_dxt1(width: usize, height: usize, transparent: bool) -> Vec<u8> {
    let blocks = width.div_ceil(4) * height.div_ceil(4);
    let mut data = vec![0u8; 128 + blocks * 8];
    data[..4].copy_from_slice(b"DDS ");
    data[4..8].copy_from_slice(&124u32.to_le_bytes());
    data[12..16].copy_from_slice(&(height as u32).to_le_bytes());
    data[16..20].copy_from_slice(&(width as u32).to_le_bytes());
    data[80..84].copy_from_slice(&0x4u32.to_le_bytes());
    data[84..88].copy_from_slice(b"DXT1");

    for block in 0..blocks {
        let offset = 128 + block * 8;
        if transparent {
            data[offset..offset + 2].copy_from_slice(&0x0000u16.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&0xF800u16.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&0xFFFFFFFFu32.to_le_bytes());
        } else {
            data[offset..offset + 2].copy_from_slice(&0xF800u16.to_le_bytes());
            data[offset + 2..offset + 4].copy_from_slice(&0x0000u16.to_le_bytes());
            data[offset + 4..offset + 8].copy_from_slice(&0x00000000u32.to_le_bytes());
        }
    }
    data
}
