//! Reads sound files from an installed X-Ray game: its archives first, loose files overriding them.
//!
//! Every caller goes through [`read_game_files`], so the archive decoders and the index-file names are
//! defined once. Nothing is bundled and nothing is written.

use std::path::Path;
use std::sync::Arc;

use sse_content::{CompanionGame, EntryDecoder, GameFileTree};

/// The game a UI id names, with its archive index file names (Enhanced Editions name theirs after the game).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioGame {
    /// Short family id used for file lookup: `soc`, `clear_sky` or `cop`.
    pub family: &'static str,
    /// The companion game that owns the archive layout.
    pub game: CompanionGame,
    /// Index file names to try, in order.
    pub fsgame_names: &'static [&'static str],
}

/// Maps a UI game id to its audio game; `None` for games without X-Ray sound files here.
#[must_use]
pub fn audio_game(game_id: &str) -> Option<AudioGame> {
    match game_id {
        "soc" | "stalker-soc" | "stalker-soc-ee" => Some(AudioGame {
            family: "soc",
            game: CompanionGame::ShadowOfChernobyl,
            fsgame_names: &["fsgame_soc.ltx", "fsgame.ltx"],
        }),
        "cs" | "clear_sky" | "stalker-cs" | "stalker-cs-ee" => Some(AudioGame {
            family: "clear_sky",
            game: CompanionGame::ClearSky,
            fsgame_names: &["fsgame_cs.ltx", "fsgame.ltx"],
        }),
        "cop" | "stalker-cop" | "stalker-cop-ee" => Some(AudioGame {
            family: "cop",
            game: CompanionGame::CallOfPripyat,
            fsgame_names: &["fsgame_cop.ltx", "fsgame.ltx"],
        }),
        _ => None,
    }
}

/// Reads the files of `game_directory` whose lower-cased paths satisfy `wanted`.
///
/// Archives are compressed, so the X-Ray header decoder and LZO for entries are supplied here. Returns `None`
/// when the game is unknown or the index file is missing; files that fail to open are skipped by the loader.
#[must_use]
pub fn read_game_files(audio: AudioGame, game_directory: &Path, wanted: impl Fn(&str) -> bool) -> Option<GameFileTree> {
    let entry_decoder: EntryDecoder =
        Arc::new(|data: &[u8], expected: usize| sse_codecs::lzo1x::decompress(data, expected));
    GameFileTree::load(
        audio.game,
        game_directory,
        wanted,
        Some(audio.fsgame_names),
        true,
        true,
        false,
        Some(sse_content::xray_header_decoder()),
        Some(entry_decoder),
    )
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhanced_editions_share_the_family_of_their_game() {
        assert_eq!(audio_game("stalker-soc-ee").map(|g| g.family), Some("soc"));
        assert_eq!(audio_game("cs").map(|g| g.family), Some("clear_sky"));
        assert_eq!(audio_game("stalker2"), None);
    }
}
