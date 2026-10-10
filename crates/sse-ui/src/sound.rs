//! Best-effort UI sounds loaded from the selected X-Ray game's own files.

use std::path::Path;

use sse_sys::output::{Output, SystemOutput};

/// Short UI feedback sound selected from the installed game's own assets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// Successful primary action.
    Select,
    /// Navigation or selection change.
    Switch,
    /// Refused or failed action.
    Decline,
}

#[derive(Clone, Debug)]
struct Clip {
    samples: Vec<i16>,
    channels: u8,
    rate: u32,
}

/// Decoded and cached game-native UI sound clips.
#[derive(Clone, Default)]
pub struct GameUiSounds {
    select: Option<Clip>,
    switch: Option<Clip>,
    decline: Option<Clip>,
}

impl GameUiSounds {
    /// Loads known menu cues from loose files or X-Ray archives without network access.
    #[must_use]
    pub fn load(game_id: &str, game_directory: &Path) -> Self {
        let Some(audio) = crate::game_audio::audio_game(game_id) else {
            return Self::default();
        };
        let wanted = |path: &str| {
            let lower = path.to_ascii_lowercase();
            lower.ends_with("menu_select.ogg")
                || lower.ends_with("menu_switch.ogg")
                || lower.ends_with("menu_decline.ogg")
        };
        let Some(tree) = crate::game_audio::read_game_files(audio, game_directory, wanted) else {
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

    /// Names of the cues the installed game supplied, in cue order.
    #[must_use]
    pub fn loaded_cues(&self) -> Vec<&'static str> {
        [
            (self.select.is_some(), "menu_select.ogg"),
            (self.switch.is_some(), "menu_switch.ogg"),
            (self.decline.is_some(), "menu_decline.ogg"),
        ]
        .into_iter()
        .filter_map(|(present, name)| present.then_some(name))
        .collect()
    }

    /// Plays a cached cue when the installed game supplied it.
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
    /// Reads the UI cues of a real install, read-only: `SSE_GAME_ID=cop SSE_GAME_DIR=/path cargo test -p sse-ui
    /// --lib real_install_cues -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs an installed game named by SSE_GAME_ID and SSE_GAME_DIR"]
    fn real_install_cues() {
        let (Ok(game), Ok(dir)) = (std::env::var("SSE_GAME_ID"), std::env::var("SSE_GAME_DIR")) else {
            panic!("set SSE_GAME_ID and SSE_GAME_DIR");
        };
        let sounds = GameUiSounds::load(&game, Path::new(&dir));
        eprintln!("CUES {game}: {:?}", sounds.loaded_cues());
    }

    #[test]
    fn unsupported_game_is_silent() {
        let sounds = GameUiSounds::load("stalker2", Path::new("."));
        sounds.play(Cue::Select, 1.0);
    }
}
