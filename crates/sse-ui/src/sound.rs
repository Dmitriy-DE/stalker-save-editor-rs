//! Best-effort UI sounds loaded from the selected X-Ray game's own files.

use std::path::Path;

use sse_content::GameFile;

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
    /// The file name the clip came from, one of the candidates of its cue.
    source: &'static str,
}

/// Files that may stand in for each cue, in order of preference. The menu files exist in the Enhanced Editions
/// and some originals; the inventory sounds are in every release, so an original install still gets cues.
const SELECT: &[&str] = &["menu_select.ogg", "inv_slot.ogg"];
const SWITCH: &[&str] = &["menu_switch.ogg", "inv_belt.ogg"];
const DECLINE: &[&str] = &[
    "menu_decline.ogg",
    "inv_close.ogg",
    "inv_properties.ogg",
    "inv_ruck.ogg",
];

/// Decoded and cached game-native UI sound clips.
#[derive(Clone, Default)]
pub struct GameUiSounds {
    select: Option<Clip>,
    switch: Option<Clip>,
    decline: Option<Clip>,
}

/// The first candidate file present in `names` (lower-cased paths), with the path that matched it.
fn pick<'a>(names: &[&'a str], candidates: &[&'static str]) -> Option<(&'static str, &'a str)> {
    candidates.iter().find_map(|candidate| {
        names
            .iter()
            .copied()
            .find(|name| *name == *candidate || name.ends_with(&format!("/{candidate}")))
            .map(|name| (*candidate, name))
    })
}

impl GameUiSounds {
    /// Loads cues from loose files or X-Ray archives without network access.
    #[must_use]
    pub fn load(game_id: &str, game_directory: &Path) -> Self {
        // Original (non-Enhanced) releases ship no menu_select/switch/decline and so stay silent: the C# 1.3.1 editor
        // played copies bundled with the app instead; those are not copied here, as they are the game's own files.
        let Some(audio) = crate::game_audio::audio_game(game_id) else {
            return Self::default();
        };
        let wanted = |path: &str| {
            let lower = path.to_ascii_lowercase();
            SELECT
                .iter()
                .chain(SWITCH)
                .chain(DECLINE)
                .any(|name| lower == *name || lower.ends_with(&format!("/{name}")))
        };
        let Some(tree) = crate::game_audio::read_game_files(audio, game_directory, wanted) else {
            return Self::default();
        };
        let files: std::collections::BTreeMap<String, &GameFile> = tree
            .files
            .iter()
            .map(|(path, file)| (path.to_ascii_lowercase(), file))
            .collect();
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        let clip = |candidates: &[&'static str]| -> Option<Clip> {
            let (source, name) = pick(&names, candidates)?;
            let bytes = files.get(name)?.read().ok()?;
            let pcm = sse_codecs::vorbis::decode(&bytes, 2_000_000).ok()?;
            Some(Clip {
                samples: pcm.samples,
                channels: pcm.channels,
                rate: pcm.rate,
                source,
            })
        };
        Self {
            select: clip(SELECT),
            switch: clip(SWITCH),
            decline: clip(DECLINE),
        }
    }

    /// The file each loaded cue came from, in cue order (select, switch, decline).
    #[must_use]
    pub fn loaded_cues(&self) -> Vec<&'static str> {
        [&self.select, &self.switch, &self.decline]
            .into_iter()
            .filter_map(|clip| clip.as_ref().map(|clip| clip.source))
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
    fn a_menu_file_beats_the_inventory_fallback_and_a_missing_one_falls_through() {
        let menu = ["sounds/interface/menu_select.ogg", "sounds/interface/inv_slot.ogg"];
        assert_eq!(
            pick(&menu, SELECT),
            Some(("menu_select.ogg", "sounds/interface/menu_select.ogg"))
        );
        let original = ["sounds/interface/inv_slot.ogg", "sounds/interface/inv_belt.ogg"];
        assert_eq!(
            pick(&original, SELECT),
            Some(("inv_slot.ogg", "sounds/interface/inv_slot.ogg"))
        );
        assert_eq!(
            pick(&original, SWITCH),
            Some(("inv_belt.ogg", "sounds/interface/inv_belt.ogg"))
        );
        // Decline falls through its list: no close sound, so properties.
        let no_close = ["sounds/interface/inv_properties.ogg", "sounds/interface/inv_ruck.ogg"];
        assert_eq!(
            pick(&no_close, DECLINE),
            Some(("inv_properties.ogg", "sounds/interface/inv_properties.ogg"))
        );
    }

    #[test]
    fn a_name_that_only_ends_the_same_way_is_not_a_cue() {
        let names = ["sounds/interface/xinv_slot.ogg", "sounds/interface/inv_slot.ogg.bak"];
        assert_eq!(pick(&names, SELECT), None);
        assert_eq!(pick(&[], DECLINE), None);
    }

    #[test]
    fn unsupported_game_is_silent() {
        let sounds = GameUiSounds::load("stalker2", Path::new("."));
        sounds.play(Cue::Select, 1.0);
    }
}
