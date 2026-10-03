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
    let mut actual_app_id: Option<String> = None;
    let mut build_id: Option<String> = None;

    let tokens = tokenize_vdf(vdf);
    let mut iter = tokens.iter().peekable();

    while let Some(key) = iter.next() {
        if key.eq_ignore_ascii_case("appid") {
            if let Some(val) = iter.next() {
                actual_app_id = Some(val.clone());
            }
        } else if key.eq_ignore_ascii_case("buildid") {
            if let Some(val) = iter.next() {
                build_id = Some(val.clone());
            }
        }
    }

    let expected_str = expected_app_id.to_string();
    if let Some(ref aid) = actual_app_id {
        if aid != &expected_str {
            return None;
        }
    }

    build_id.filter(|s| !s.trim().is_empty())
}

fn tokenize_vdf(s: &str) -> Vec<String> {
    // Collect quoted tokens from a VDF/ACF file using iterator-based parsing.
    // Each quoted string between `"..."` is one token; non-quoted content is ignored.
    let mut tokens = Vec::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '"' {
            // Consume until the closing quote (or end of string).
            let token: String = chars.by_ref().take_while(|&ch| ch != '"').collect();
            tokens.push(token);
        }
    }

    tokens
}
