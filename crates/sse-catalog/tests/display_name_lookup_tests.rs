//! Display-name lookups: ASCII and Unicode case-insensitive matching, trimming and ambiguity.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use sse_catalog::models::{ItemCatalog, ItemDefinition};

fn item(key: &str, name: &str) -> ItemDefinition {
    ItemDefinition::new(
        key.to_string(),
        Some(name.to_string()),
        None,
        None,
        None,
        None,
        None,
        Vec::new(),
        "test".to_string(),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}

fn catalog() -> ItemCatalog {
    ItemCatalog::new(
        "stalker-cop".to_string(),
        vec![
            item("ak", "АКМ-74/2"),
            item("medkit", "Medkit"),
            item("twin_a", "Twin Name"),
            item("twin_b", "twin name"),
        ],
    )
    .unwrap()
}

#[test]
fn cyrillic_names_match_regardless_of_case_and_surrounding_spaces() {
    let catalog = catalog();
    assert_eq!(
        catalog.resolve_display_name("акм-74/2").map(|i| i.key.as_str()),
        Some("ak")
    );
    assert_eq!(
        catalog.resolve_display_name("  АКМ-74/2 ").map(|i| i.key.as_str()),
        Some("ak")
    );
}

#[test]
fn ascii_names_match_case_insensitively() {
    let catalog = catalog();
    assert_eq!(
        catalog.resolve_display_name("MEDKIT").map(|i| i.key.as_str()),
        Some("medkit")
    );
}

#[test]
fn ambiguous_or_unknown_names_do_not_resolve() {
    let catalog = catalog();
    assert!(catalog.resolve_display_name("TWIN NAME").is_none());
    assert!(catalog.resolve_display_name("unknown").is_none());
    assert!(catalog.resolve_display_name("   ").is_none());
}
