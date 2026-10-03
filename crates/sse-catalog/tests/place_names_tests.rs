//! Integration tests for place names formatting and localization.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{I18nService, PlaceNames};

#[test]
fn a_level_is_named_as_the_games_name_it() {
    I18nService::instance().set_language("ru");

    let cases = [
        ("stalker-soc", "L02_Garbage", "Свалка"),
        ("stalker-cop", "jupiter_underground", "Путепровод «Припять-1»"),
        ("stalker-cs", "marsh", "Болота"),
        ("stalker-cs", "agroprom_underground", "Подземелье НИИ Агропром"),
        ("stalker-cop", "l01_escape", "Кордон"),
        ("stalker-cop", "some_mod_level", "some_mod_level"),
    ];

    for (release, level, expected) in cases {
        assert_eq!(
            PlaceNames::level(Some(release), Some(level)),
            expected,
            "Failed for level {level}"
        );
    }
}

#[test]
fn a_stash_without_an_official_name_is_described_by_its_kind() {
    I18nService::instance().set_language("ru");

    let cases = [
        ("stalker-cop", "zat_a2_actor_treasure", "Личный ящик на «Скадовске»"),
        ("stalker-cop", "jup_b202_actor_treasure", "Личный ящик на «Янове»"),
        ("stalker-cop", "mod_x_actor_treasure", "Личный ящик"),
        ("stalker-soc", "level_prefix_inventory_box", "Ящик"),
        ("stalker-soc", "level_prefix_inventory_box_0000", "Ящик № 1"),
        ("stalker-soc", "bar_inventory_box_0012", "Ящик № 13"),
        ("stalker-cs", "esc_smart_terrain_5_7_box", "Ящик лагеря 5-7"),
        ("stalker-cop", "mod_treasure_weapon", "Тайник"),
        ("stalker-cop", "zat_b12_container", "Контейнер «zat_b12_container»"),
    ];

    for (release, box_id, expected) in cases {
        assert_eq!(
            PlaceNames::stash(Some(release), Some(box_id), 7),
            expected,
            "Failed for stash {box_id}"
        );
    }
}

#[test]
fn a_clear_sky_stash_has_the_name_of_its_treasure() {
    I18nService::instance().set_language("ru");

    let name = PlaceNames::stash(Some("stalker-cs"), Some("mar_treasure_1"), 7);
    assert!(!name.contains("mar_treasure"));
    assert!(!name.contains("Тайник №"));
    assert_eq!(
        name,
        PlaceNames::stash(Some("stalker-cs-ee"), Some("MAR_TREASURE_1"), 7)
    );
}

#[test]
fn an_unnamed_box_is_named_by_its_handle() {
    I18nService::instance().set_language("ru");
    assert_eq!(PlaceNames::stash(Some("stalker-cop"), Some(""), 42), "Тайник 0x002A");
}

#[test]
fn the_level_of_an_object_follows_from_its_prefix() {
    I18nService::instance().set_language("ru");

    let cases = [
        ("zat_b12_container", Some("Затон")),
        ("pripyat_level_changer_0000", Some("Припять")),
        ("pas_b400_level_changer", Some("Путепровод «Припять-1»")),
        ("exit_to_garbage_01", None),
    ];

    for (name, expected) in cases {
        assert_eq!(
            PlaceNames::level_of_object(Some("stalker-cop"), Some(name)).as_deref(),
            expected,
            "Failed for object {name}"
        );
    }
}
