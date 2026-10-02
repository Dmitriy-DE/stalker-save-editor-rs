//! Integration tests for i18n translation service and completeness checker.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{I18nCompletenessChecker, I18nService, SUPPORTED_LANGUAGES};

#[test]
fn every_interface_language_loads_its_translations() {
    let service = I18nService::instance();

    for &(code, _) in SUPPORTED_LANGUAGES {
        if code != "ru" {
            let catalog = service.get_catalog(code).unwrap();
            assert!(
                catalog.len() > 1000,
                "{code} catalog is missing or has only {} entries",
                catalog.len()
            );
        }
    }

    assert_eq!(service.tr_in(Some("en"), "ИНВЕНТАРЬ", &[]), "INVENTORY");
}

#[test]
fn completeness_checker_validates_coverage_and_placeholders() {
    let result = I18nCompletenessChecker::validate(None);

    assert!(
        result.success,
        "Completeness checker failed with errors:\n{}",
        result.errors.join("\n")
    );
    assert!(result.total_messages > 1000);
    assert_eq!(result.checked_locales, 14);
}
