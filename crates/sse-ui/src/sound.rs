//! Best-effort UI sounds loaded from the selected X-Ray game's own files.

use std::path::Path;

use sse_content::{CompanionGame, GameFileTree};
use sse_sys::output::{Output, SystemOutput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    Select,
    Switch,
    Decline,
}

#[derive(Clone, Debug)]
struct Clip {
    samples: Vec<i16>,
    channels: u8,
    rate: u32,
}

#[derive(Clone, Default)]
pub struct GameUiSounds {
    select: Option<Clip>,
    switch: Option<Clip>,
    decline: Option<Clip>,
}

impl GameUiSounds {
    #[must_use]
    pub fn load(game_id: &str, game_directory: &Path) -> Self {
        let game = match game_id {
            "soc" | "stalker-soc" | "stalker-soc-ee" => CompanionGame::ShadowOfChernobyl,
            "cs" | "clear_sky" | "stalker-cs" | "stalker-cs-ee" => CompanionGame::ClearSky,
            "cop" | "stalker-cop" | "stalker-cop-ee" => CompanionGame::CallOfPripyat,
            _ => return Self::default(),
        };
        let wanted = |path: &str| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with("menu_select.ogg")
                || lower.ends_with("menu_switch.ogg")
                || lower.ends_with("menu_decline.ogg")
        };
        let Ok(tree) = GameFileTree::load_simple(game, game_directory, wanted, true) else {
            return Self::default();
        };
        let mut sounds = Self::default();
        for (path, file) in tree.files {
            let lower = path.to_ascii_lowercase();
            let Ok(bytes) = file.read() else { continue };
            let Ok(pcm) = sse_codecs::vorbis::decode(&bytes, 2_000_000) else {
                continue;
            };
            let clip = Clip {
                samples: pcm.samples,
                channels: pcm.channels,
                rate: pcm.rate,
            };
            if lower.ends_with("menu_select.ogg") {
                sounds.select = Some(clip);
            } else if lower.ends_with("menu_switch.ogg") {
                sounds.switch = Some(clip);
            } else if lower.ends_with("menu_decline.ogg") {
                sounds.decline = Some(clip);
            }
        }
        sounds
    }

    pub fn play(&self, cue: Cue, volume: f32) {
        let clip = match cue {
            Cue::Select => self.select.as_ref(),
            Cue::Switch => self.switch.as_ref(),
            Cue::Decline => self.decline.as_ref(),
        };
        if let Some(clip) = clip {
            let mut output = SystemOutput;
            output.play(&clip.samples, clip.channels, clip.rate, volume);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsupported_game_is_silent() {
        let sounds = GameUiSounds::load("stalker2", Path::new("."));
        sounds.play(Cue::Select, 1.0);
    }
}
