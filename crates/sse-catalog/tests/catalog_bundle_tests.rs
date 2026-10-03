//! Integration tests for catalog bundle reading and writing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{CatalogBundleReader, CatalogBundleWriter};

#[test]
fn embedded_bundle_loads_all_python_generated_release_metadata() {
    let embedded = CatalogBundleReader::load_embedded();

    assert_eq!(embedded.len(), 3);
    assert!(embedded.contains_key("stalker-cop"));
    assert!(embedded.contains_key("stalker-cs"));
    assert!(embedded.contains_key("stalker-soc"));

    let cop = &embedded["stalker-cop"];
    let cs = &embedded["stalker-cs"];
    let soc = &embedded["stalker-soc"];

    assert_eq!(cop.items.items().len(), 434);
    assert_eq!(cs.items.items().len(), 417);
    assert_eq!(soc.items.items().len(), 389);

    assert_eq!(
        cop.items
            .resolve("ammo_11.43x23_hydro")
            .and_then(|i| i.display_name.as_deref()),
        Some(".45Гидро")
    );
}

#[test]
fn embedded_bundle_is_loaded_once_and_reused() {
    let first = CatalogBundleReader::load_embedded();
    let second = CatalogBundleReader::load_embedded();

    assert_eq!(first as *const _, second as *const _);
}

#[test]
fn catalog_lookup_is_exact_and_ambiguous_display_names_stay_unresolved() {
    let synthetic_json = br#"{
      "schema_version": 1,
      "releases": {
        "stalker-cop": {
          "items": [
            {"key": "scope_exact", "display_name": "Shared label", "category": "weapon", "unit_weight": 1.25, "slots": null, "max_stack": null, "serialization_family": "WEAPON", "icon_x": 1, "icon_y": 2, "icon_texture": "ui\\scope"},
            {"key": "item_second", "display_name": "Shared label", "category": null, "slots": [], "max_stack": 3}
          ],
          "factions": [
            {"key": "actor", "display_name": "Actor", "numeric_id": 0, "source": "fixture#communities"},
            {"key": "stalker", "display_name": "Stalker", "numeric_id": 1, "source": "fixture#communities"}
          ],
          "relation_addresses": [{"source": "actor", "target": "stalker", "row": 0, "column": 1, "value": 25}],
          "upgrades": [{"key": "upgrade_exact", "display_name": "Scope upgrade", "item_key": "scope_exact", "applicable_item_keys": [], "source": "fixture#upgrade"}]
        }
      }
    }"#;

    let bundles = CatalogBundleReader::load(synthetic_json).unwrap();
    assert_eq!(bundles.len(), 1);
    let parsed = &bundles["stalker-cop"];

    assert!(parsed.items.resolve("scope_exact").is_some());
    assert!(parsed.items.resolve("Scope_Exact").is_none());
    assert!(parsed.items.resolve_display_name("Shared label").is_none());
    assert_eq!(
        parsed
            .items
            .resolve_key_or_display_name("  scope_exact  ")
            .map(|i| i.key.as_str()),
        Some("scope_exact")
    );
    assert!(parsed.items.resolve_key_or_display_name("unknown key").is_none());

    let factions = parsed.factions.as_ref().unwrap();
    assert!(factions.resolve_numeric(500).is_none());
    assert_eq!(factions.relation_address("actor", "stalker").unwrap(), (0, 1));
    assert_eq!(factions.default_relation("actor", "stalker").unwrap(), Some(25));

    let upgrades = parsed.upgrades.as_ref().unwrap();
    let for_scope = upgrades.for_item("scope_exact");
    assert_eq!(for_scope.len(), 1);
    assert_eq!(for_scope[0].key, "upgrade_exact");

    assert!(parsed.items.resolve("stalker2_only_item").is_none());
}

#[test]
fn rejects_unsupported_or_malformed_bundle_payloads() {
    let payloads = [
        r#"{"schema_version":2,"releases":{}}"#,
        r#"{"schema_version":1,"releases":{"unknown-release":{"items":[]}}}"#,
        r#"{"schema_version":1,"releases":{"stalker-cop":{"items":[{"key":"x"},{"key":"x"}]}}}"#,
    ];

    for json in payloads {
        assert!(
            CatalogBundleReader::load(json.as_bytes()).is_err(),
            "Expected failure for: {json}"
        );
    }
}

#[test]
fn rejects_foreign_faction_and_upgrade_release_ids() {
    let foreign_faction = r#"{"schema_version":1,"releases":{"stalker-cop":{"items":[],"factions":[{"key":"actor","source":"fixture","release_id":"stalker-cs"}]}}}"#;
    let foreign_upgrade = r#"{"schema_version":1,"releases":{"stalker-cop":{"items":[],"upgrades":[{"key":"up","source":"fixture","release_id":"stalker-cs"}]}}}"#;

    assert!(CatalogBundleReader::load(foreign_faction.as_bytes()).is_err());
    assert!(CatalogBundleReader::load(foreign_upgrade.as_bytes()).is_err());
}

#[test]
fn round_trip_writer_reproduces_bundle() {
    let embedded = CatalogBundleReader::load_embedded();
    let cop = &embedded["stalker-cop"];

    let written = CatalogBundleWriter::write(&[cop]).unwrap();
    let reloaded = CatalogBundleReader::load(&written).unwrap();

    let reloaded_cop = &reloaded["stalker-cop"];
    assert_eq!(reloaded_cop.items.items().len(), cop.items.items().len());
    assert_eq!(reloaded_cop.items.resolve("wpn_pm").unwrap().key, "wpn_pm");
}
