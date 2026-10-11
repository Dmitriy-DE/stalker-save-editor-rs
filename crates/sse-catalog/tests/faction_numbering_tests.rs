//! The faction numbers in each release catalog must be the numbers the game writes in its `communities` list.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use sse_catalog::CatalogBundleReader;

/// `communities = name, id, ...` from `config/creatures/game_relations.ltx`, section `[game_relations]`.
const SHADOW_OF_CHERNOBYL: &[(&str, i32)] = &[
    ("actor", 0),
    ("actor_dolg", 1),
    ("actor_freedom", 2),
    ("stalker", 5),
    ("monolith", 6),
    ("military", 7),
    ("killer", 8),
    ("ecolog", 9),
    ("dolg", 10),
    ("freedom", 11),
    ("bandit", 12),
    ("zombied", 13),
    ("stranger", 14),
    ("trader", 15),
    ("arena_enemy", 16),
];

const CLEAR_SKY: &[(&str, i32)] = &[
    ("actor", 0),
    ("actor_bandit", 1),
    ("actor_dolg", 2),
    ("actor_freedom", 3),
    ("actor_stalker", 4),
    ("arena_enemy", 5),
    ("bandit", 6),
    ("csky", 7),
    ("dolg", 8),
    ("ecolog", 9),
    ("freedom", 10),
    ("killer", 11),
    ("army", 12),
    ("monolith", 13),
    ("monster", 14),
    ("stalker", 15),
    ("trader", 16),
    ("zombied", 17),
    ("actor_csky", 18),
    ("renegade", 19),
];

const CALL_OF_PRIPYAT: &[(&str, i32)] = &[
    ("actor", 0),
    ("bandit", 1),
    ("dolg", 2),
    ("ecolog", 3),
    ("freedom", 4),
    ("killer", 5),
    ("army", 6),
    ("monolith", 7),
    ("monster", 8),
    ("stalker", 9),
    ("zombied", 10),
];

fn assert_matches_game(release: &str, game: &[(&str, i32)]) {
    let bundle = CatalogBundleReader::load_embedded()
        .get(release)
        .expect("release catalog is embedded");
    let factions = bundle.factions.as_ref().expect("release has a faction catalog");
    assert_eq!(factions.factions().len(), game.len(), "{release}: number of factions");
    for (key, id) in game {
        let found = factions
            .resolve_numeric(*id)
            .unwrap_or_else(|| panic!("{release}: game community {key} = {id} is missing from the catalog"));
        assert_eq!(found.key, *key, "{release}: id {id} must be {key}");
    }
}

#[test]
fn shadow_of_chernobyl_faction_numbers_match_the_game() {
    assert_matches_game("stalker-soc", SHADOW_OF_CHERNOBYL);
}

#[test]
fn clear_sky_faction_numbers_match_the_game() {
    assert_matches_game("stalker-cs", CLEAR_SKY);
}

#[test]
fn call_of_pripyat_faction_numbers_match_the_game() {
    assert_matches_game("stalker-cop", CALL_OF_PRIPYAT);
}

#[test]
fn ids_three_and_twenty_one_are_not_game_communities() {
    // Entries with id 3 appear in real ToT saves; the game's list has no such community, so no key may be assigned.
    let bundle = CatalogBundleReader::load_embedded()
        .get("stalker-soc")
        .expect("ToT catalog");
    let factions = bundle.factions.as_ref().expect("faction catalog");
    assert!(factions.resolve_numeric(3).is_none());
    assert!(factions.resolve_numeric(21).is_none());
}
