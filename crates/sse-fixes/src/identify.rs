//! Structural game installation check and Steam build identification.

use std::fs;
use std::path::Path;

use crate::models::GameTarget;

/// Identifies if a directory is a valid installation for `target` and detects its Steam build ID.
#[must_use]
pub fn identify_game(target: GameTarget, game_directory: &Path) -> (bool, Option<String>) {
    if !game_directory.is_dir() {
        return (false, None);
    }

    let is_installation = has_expected_marker(target, game_directory);
    let build_id = try_read_steam_build_id(target, game_directory);

    (is_installation, build_id)
}

fn has_expected_marker(target: GameTarget, game_directory: &Path) -> bool {
    if !target.is_xray() {
        return game_directory.join("Stalker2").join("Content").join("Paks").is_dir();
    }

    let markers = match target {
        GameTarget::ShadowOfChernobyl | GameTarget::ShadowOfChernobylEnhancedEdition => {
            &["fsgame.ltx", "fsgame_soc.ltx"][..]
        }
        GameTarget::ClearSky | GameTarget::ClearSkyEnhancedEdition => &["fsgame.ltx", "fsgame_cs.ltx"][..],
        GameTarget::CallOfPripyat | GameTarget::CallOfPripyatEnhancedEdition => &["fsgame.ltx", "fsgame_cop.ltx"][..],
        GameTarget::Stalker2 => &[][..],
    };

    markers.iter().any(|marker| game_directory.join(marker).is_file())
}

fn try_read_steam_build_id(target: GameTarget, game_directory: &Path) -> Option<String> {
    let app_id = target.steam_app_id()?;
    let manifest_name = format!("appmanifest_{app_id}.acf");

    // Try standard Steam directory layouts:
    // 1. game_dir/../../appmanifest_<app_id>.acf (inside steamapps)
    // 2. game_dir/../../../steamapps/appmanifest_<app_id>.acf
    // 3. game_dir/appmanifest_<app_id>.acf
    let candidates = [
        game_directory.join("..").join("..").join(&manifest_name),
        game_directory
            .join("..")
            .join("..")
            .join("..")
            .join("steamapps")
            .join(&manifest_name),
        game_directory.join(&manifest_name),
    ];

    for path in &candidates {
        if let Ok(content) = fs::read_to_string(path) {
            if let Some(build_id) = parse_vdf_build_id(&content, app_id) {
                return Some(build_id);
            }
        }
    }

    None
}

fn parse_vdf_build_id(vdf: &str, expected_app_id: u32) -> Option<String> {
    // Read the structure, not a token stream: only the top-level AppState fields count.
    let document = sse_codecs::vdf::parse(vdf).ok()?;
    let app_state = document.get_object("AppState")?;
    if app_state.get_string("appid")? != expected_app_id.to_string() {
        return None;
    }
    app_state
        .get_string("buildid")
        .map(str::to_owned)
        .filter(|build| !build.trim().is_empty())
}

#[cfg(test)]
mod acf_buildid_tests {
    use super::parse_vdf_build_id;

    #[test]
    fn buildid_comes_from_app_state_not_from_a_value_that_spells_it() {
        let acf = r#""AppState"
{
    "appid"     "1643320"
    "buildid"   "12345678"
    "name"      "buildid"
    "UserConfig"
    {
        "buildid"   "999999"
    }
}
"#;
        assert_eq!(parse_vdf_build_id(acf, 1_643_320).as_deref(), Some("12345678"));
    }

    #[test]
    fn manifest_for_another_app_gives_no_build_id() {
        let acf = r#""AppState" { "appid" "4500" "buildid" "7" }"#;
        assert_eq!(parse_vdf_build_id(acf, 1_643_320), None);
    }
}
