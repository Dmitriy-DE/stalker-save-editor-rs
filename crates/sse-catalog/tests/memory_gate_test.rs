//! Integration tests verifying catalog memory gate budget (< 15 MiB RSS).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_catalog::{CatalogBundleReader, I18nService, OfficialNamesCatalog, Stalker2ArmorUpgrades, Stalker2ItemCatalog};

fn get_process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let parts: Vec<&str> = statm.split_whitespace().collect();
        let resident_pages: u64 = parts.get(1)?.parse().ok()?;
        let page_size = 4096u64;
        Some(resident_pages * page_size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[test]
fn loading_every_shipped_catalogue_keeps_memory_under_15_mib() {
    let bundles = CatalogBundleReader::load_embedded();
    assert_eq!(bundles.len(), 3);

    let names = OfficialNamesCatalog::load_embedded();
    assert_eq!(
        names.resolve(Some("stalker-cop"), "items", Some("wpn_ak74"), Some("ru")),
        Some("АКМ-74/2".to_string())
    );

    let s2_items = Stalker2ItemCatalog::load_embedded();
    assert!(s2_items.count() > 1000);
    assert_eq!(s2_items.name(Some("A012A"), "ru"), Some("12/76 мм жекан"));

    let upgrades_count = Stalker2ArmorUpgrades::count();
    assert!(upgrades_count > 0);

    let service = I18nService::instance();
    service.set_language("ru");
    assert_eq!(service.tr("ИНВЕНТАРЬ", &[]), "ИНВЕНТАРЬ");

    if let Some(rss) = get_process_rss_bytes() {
        let limit = 15 * 1024 * 1024; // 15 MiB
        assert!(
            rss <= limit,
            "Process RSS {rss} bytes ({:.2} MiB) exceeded 15 MiB limit",
            rss as f64 / (1024.0 * 1024.0)
        );
    }
}
