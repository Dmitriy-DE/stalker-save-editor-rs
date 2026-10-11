//! The faction number in each release catalog must be the number a save stores for that community: its position
//! in the game's `communities` list.
//!
//! The engine reads `communities = name, team, name, team, ...` into a table indexed by position and stores that
//! position (`CHARACTER_COMMUNITY_INDEX`) in saves: the actor's faction, the key of every relation record and the
//! rows of `[communities_relations]`. The number after each name is the community's team. Clear Sky and Call of
//! Pripyat number teams 0, 1, 2, ... so the two coincide; Shadow of Chernobyl skips teams 3 and 4, so from `stalker`
//! on the position is not the team.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use sse_catalog::CatalogBundleReader;

/// `communities = name, team, ...` from `config/creatures/game_relations.ltx`, section `[game_relations]`, in the
/// game's order.
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
    for (position, (key, _team)) in game.iter().enumerate() {
        let position = i32::try_from(position).expect("community position");
        let found = factions
            .resolve_numeric(position)
            .unwrap_or_else(|| panic!("{release}: game community {key} at {position} is missing from the catalog"));
        assert_eq!(found.key, *key, "{release}: position {position} must be {key}");
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
fn clear_sky_and_call_of_pripyat_teams_equal_positions() {
    for game in [CLEAR_SKY, CALL_OF_PRIPYAT] {
        for (position, (key, team)) in game.iter().enumerate() {
            assert_eq!(i32::try_from(position).ok(), Some(*team), "{key}");
        }
    }
}

#[test]
fn shadow_of_chernobyl_relation_keys_are_positions_not_teams() {
    // Real ToT saves hold actor relation records with keys 3, 7, 8 and 9 (values 0, 0, 50..200, 50). No community
    // has team 3. By position they are stalker, ecolog, dolg and freedom, and dolg, freedom and ecolog are exactly
    // the communities whose goodwill the game's task rewards change (`reward_relation` in task_manager.ltx).
    let bundle = CatalogBundleReader::load_embedded()
        .get("stalker-soc")
        .expect("ToT catalog");
    let factions = bundle.factions.as_ref().expect("faction catalog");
    let key = |id| factions.resolve_numeric(id).map(|faction| faction.key.as_str());
    assert_eq!(key(3), Some("stalker"));
    assert_eq!(key(7), Some("ecolog"));
    assert_eq!(key(8), Some("dolg"));
    assert_eq!(key(9), Some("freedom"));
    // Fifteen communities: positions 0 to 14. Anything above is not a community of the unmodified game.
    assert_eq!(key(14), Some("arena_enemy"));
    assert_eq!(key(15), None);
    assert_eq!(key(21), None);
}
