//! Counts embedded item keys with no display name after reading installed game content (read-only, SSE_GAME_*_DIR).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::collections::HashMap;
use std::path::Path;

use sse_catalog::{CatalogBundleReader, GameContentService, SaveNaming};
use sse_content::CompanionGame;

#[test]
#[ignore = "reads installed game archives (read-only) from SSE_GAME_*_DIR"]
fn installed_item_name_coverage() {
    let cache = std::env::temp_dir().join(format!("sse-name-probe-{}", std::process::id()));
    let games = [
        (CompanionGame::ShadowOfChernobyl, "stalker-soc", "SSE_GAME_SOC_DIR"),
        (CompanionGame::ClearSky, "stalker-cs", "SSE_GAME_CS_DIR"),
        (CompanionGame::CallOfPripyat, "stalker-cop", "SSE_GAME_COP_DIR"),
    ];
    for (game, release, env) in games {
        let Ok(dir) = std::env::var(env) else { continue };
        let loaded = GameContentService::load(game, Path::new(&dir), &cache, "ru").expect("load installed content");
        let installed: HashMap<String, String> = match loaded.as_ref() {
            Some(content) => content
                .bundle()
                .items
                .items()
                .iter()
                .filter_map(|item| item.display_name.clone().map(|name| (item.key.clone(), name)))
                .collect(),
            None => HashMap::new(),
        };
        let issues = loaded.as_ref().map(|c| c.status().issues.clone()).unwrap_or_default();
        let embedded = CatalogBundleReader::load_embedded()
            .get(release)
            .expect("embedded bundle");
        let mut missing = Vec::new();
        for item in embedded.items.items() {
            let name = SaveNaming::item_name(release, &item.key, installed.get(&item.key).map(String::as_str));
            if name == item.key {
                missing.push(item.key.clone());
            }
        }
        for key in [
            "ammo_10.9x32.5",
            "dolg_exo_outfit",
            "matras",
            "monolith_exo_outfit",
            "notepad",
        ] {
            let in_embedded = embedded.items.items().iter().any(|item| item.key == key);
            let installed_name = installed.get(key).cloned();
            let name = SaveNaming::item_name(release, key, installed_name.as_deref());
            println!("KEY {release} {key} embedded={in_embedded} installed={installed_name:?} resolved={name:?}");
        }
        println!(
            "RESULT {release} items={} installed_names={} missing={} keys={missing:?} issues={:?}",
            embedded.items.items().len(),
            installed.len(),
            missing.len(),
            issues
        );
    }
    let _ = std::fs::remove_dir_all(&cache);
}
