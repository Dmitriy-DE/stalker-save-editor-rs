//! Catalog keys whose sections a game ships but never loads.

/// Sections of Clear Sky's `configs/misc/unique_items.ltx`.
///
/// The file is a leftover of Shadow of Chernobyl: it sits in `configs.db` of Clear Sky and of its Enhanced Edition,
/// but no config includes it (`system.ltx` and the files it pulls in never name it), so the running game has none
/// of these sections. An object of such a section cannot be created by the game, and a save holding one would not
/// load.
const CLEAR_SKY_UNLOADED: [&str; 19] = [
    "outfit_bandit_m1",
    "outfit_dolg_m1",
    "outfit_exo_m1",
    "outfit_novice_m1",
    "outfit_specnaz_m1",
    "outfit_stalker_m1",
    "outfit_stalker_m2",
    "outfit_svoboda_m1",
    "wpn_ak74_arena",
    "wpn_ak74u_arena",
    "wpn_bm16_arena",
    "wpn_fn2000_arena",
    "wpn_g36_arena",
    "wpn_groza_arena",
    "wpn_mp5_arena",
    "wpn_pm_arena",
    "wpn_spas12_arena",
    "wpn_toz34_arena",
    "wpn_val_arena",
];

/// Whether the game of `format_id` (a release or save format id) loads the section `key`.
///
/// `false` only for keys known to be shipped without being loaded; such an item is not offered for adding. Keys
/// the catalog does not know are not judged here.
#[must_use]
pub fn is_loaded_by_game(format_id: &str, key: &str) -> bool {
    match crate::bundle::release_for_format(format_id) {
        "stalker-cs" => !CLEAR_SKY_UNLOADED.contains(&key),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::{is_loaded_by_game, CLEAR_SKY_UNLOADED};
    use crate::CatalogBundleReader;

    #[test]
    fn every_unloaded_clear_sky_key_is_a_catalog_item() {
        let bundles = CatalogBundleReader::load_embedded();
        let clear_sky = bundles.get("stalker-cs").map(|bundle| bundle.items.items());
        for key in CLEAR_SKY_UNLOADED {
            assert!(
                clear_sky.is_some_and(|items| items.iter().any(|item| item.key == key)),
                "{key} is no longer in the Clear Sky catalog"
            );
            assert!(!is_loaded_by_game("stalker-cs", key), "{key}");
            assert!(!is_loaded_by_game("stalker-cs-ee", key), "{key}");
        }
    }

    #[test]
    fn the_same_sections_are_real_items_of_shadow_of_chernobyl() {
        // Shadow of Chernobyl includes unique_items.ltx; there the unique outfits are loaded and obtainable.
        assert!(is_loaded_by_game("stalker-soc", "outfit_bandit_m1"));
        assert!(is_loaded_by_game("stalker-soc-ee", "wpn_pm_arena"));
        assert!(is_loaded_by_game("stalker-cs", "wpn_pm"));
        assert!(is_loaded_by_game("stalker-cop", "wpn_pm_arena"));
    }
}
