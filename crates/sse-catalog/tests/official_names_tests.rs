//! Integration tests for official names catalog and language resolution.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::OfficialNamesCatalog;

#[test]
fn resolves_release_scoped_translations_and_language_fallbacks() {
    let names = OfficialNamesCatalog::load_embedded();

    assert_eq!(
        names.resolve(Some("stalker-cs-ee"), "items", Some("wpn_ak74"), Some("ru")),
        Some("АКМ-74/2".to_string())
    );
    assert_eq!(
        names.resolve(Some("stalker-cop"), "items", Some("ammo_9x39_pab9"), Some("pt_BR")),
        Some("9x39 mm SP-5".to_string())
    );
    assert_eq!(
        names.resolve(Some("stalker-cop"), "factions", Some("actor"), Some("unknown")),
        Some("Free Stalker".to_string())
    );
    assert_eq!(
        names.resolve(Some("stalker2"), "items", Some("wpn_ak74"), Some("ru")),
        None
    );
    assert_eq!(
        names.resolve(Some("stalker-soc"), "items", Some("missing_key"), Some("ru")),
        None
    );
    assert_eq!(names.resolve(Some("stalker-cop"), "items", None, Some("ru")), None);
}

#[test]
fn maps_only_supported_trilogy_release_names() {
    let cases = [
        ("stalker-soc-ee", Some("soc")),
        ("stalker-cs-ee", Some("clear_sky")),
        ("stalker-cop-ee", Some("cop")),
        ("Shadow of Chernobyl", Some("soc")),
        ("Call of Pripyat", Some("cop")),
        ("stalker2", None),
        ("unknown-game", None),
    ];

    for (release_id, family) in cases {
        assert_eq!(
            OfficialNamesCatalog::release_family(Some(release_id)),
            family,
            "Failed for release: {release_id}"
        );
    }
}
