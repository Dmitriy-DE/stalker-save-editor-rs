//! Integration tests for S.T.A.L.K.E.R. 2 item catalog and armor upgrades.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{Stalker2ArmorUpgrades, Stalker2ItemCatalog};

#[test]
fn embedded_catalog_has_official_names_and_icons() {
    let catalog = Stalker2ItemCatalog::load_embedded();

    assert!(catalog.count() > 1000);
    assert_eq!(catalog.name(Some("A012A"), "ru"), Some("12/76 мм жекан"));
    assert_eq!(catalog.name(Some("A012A"), "en"), Some("12x76mm Slug"));
    assert_eq!(catalog.icon(Some("A012A")), Some("s2/A012A.png"));
    assert!(catalog.description(Some("A012A"), "ru").is_some());
}

#[test]
fn non_russian_ui_falls_back_to_english_not_cyrillic() {
    let catalog = Stalker2ItemCatalog::load_embedded();
    assert_eq!(catalog.name(Some("A012A"), "de"), Some("12x76mm Slug"));
}

#[test]
fn canonicalizes_save_sids() {
    let catalog = Stalker2ItemCatalog::load_embedded();

    assert_eq!(catalog.canonical_sid(Some("a012a")), Some("A012A"));
    assert_eq!(catalog.canonical_sid(Some("A012A_Player")), Some("A012A"));
}

#[test]
fn unknown_sid_has_no_name_but_family_icon_fallback_works() {
    let catalog = Stalker2ItemCatalog::load_embedded();

    assert!(catalog.name(Some("NoSuchItem_12345"), "ru").is_none());
    assert!(catalog.icon(Some("NoSuchItem_12345")).is_none());
    assert_eq!(
        catalog.icon(Some("SomeUnknownQuest_PDA_Garpia")),
        Some("s2/KozimkovPDA.png")
    );
}

#[test]
fn rejects_a_payload_without_items() {
    assert!(Stalker2ItemCatalog::load(br#"{"schema_version":2}"#).is_err());
}

#[test]
fn armor_and_weapon_upgrades_are_resolvable() {
    assert!(Stalker2ArmorUpgrades::count() > 100);

    let armor_up = Stalker2ArmorUpgrades::find_armor("Anomaly_Scientific_Armor_MaxDurability_Left_1_1");
    assert!(armor_up.is_some());
    let up = armor_up.unwrap();
    assert_eq!(up.effect, "durable");
    assert_eq!(up.tier, 1);
}
