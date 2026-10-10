//! Main-menu music of the installed game, decided and decoded without touching the speakers.
//!
//! Nothing is redistributed: the track is read from the player's own install (`sounds/music/*.ogg` in the
//! game archives) when a game is known, decoded once, and handed to a [`MusicSink`]. Without an install,
//! without the file, or with a file that does not decode, the result is silence and never an error.

use sse_codecs::vorbis;

/// Upper bound on decoded samples per channel group: a little over ten minutes of 48 kHz stereo.
const MAXIMUM_TRACK_SAMPLES: usize = 48_000 * 60 * 12;

/// Music files of each game family, relative to the game data root. Shadow of Chornobyl stores its stereo
/// theme as two mono halves; they are interleaved into one stereo track.
#[must_use]
pub fn music_paths(family: &str) -> Option<&'static [&'static str]> {
    match family {
        "soc" => Some(&["sounds/music/wasteland2_l.ogg", "sounds/music/wasteland2_r.ogg"]),
        "clear_sky" => Some(&["sounds/music/wasteland2.ogg"]),
        "cop" => Some(&["sounds/music/menu.ogg"]),
        _ => None,
    }
}

/// Interleaves two mono sample streams into one stereo stream; the shorter one is padded with silence.
#[must_use]
pub fn interleave(left: &[i16], right: &[i16]) -> Vec<i16> {
    let frames = left.len().max(right.len());
    let mut out = Vec::with_capacity(frames.saturating_mul(2));
    for index in 0..frames {
        out.push(left.get(index).copied().unwrap_or(0));
        out.push(right.get(index).copied().unwrap_or(0));
    }
    out
}

/// A decoded track ready for playback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    /// Channel count (1 or 2).
    pub channels: u8,
    /// Sample rate in hertz.
    pub rate: u32,
    /// Interleaved signed 16-bit samples.
    pub samples: Vec<i16>,
}

/// Decodes a family's menu track from the bytes of its files. `read` returns the bytes of a relative path, or
/// `None` when the file is absent. Any missing or undecodable file yields `None`: silence, not an error.
#[must_use]
pub fn decode_track(family: &str, read: impl Fn(&str) -> Option<Vec<u8>>) -> Option<Track> {
    let paths = music_paths(family)?;
    let mut decoded = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = read(path)?;
        let pcm = vorbis::decode(&bytes, MAXIMUM_TRACK_SAMPLES).ok()?;
        decoded.push(pcm);
    }
    let first = decoded.first()?;
    let rate = first.rate;
    if decoded.iter().any(|pcm| pcm.rate != rate) {
        return None;
    }
    match decoded.as_slice() {
        [mono] if mono.channels == 1 => Some(Track {
            channels: 1,
            rate,
            samples: mono.samples.clone(),
        }),
        [stereo] if stereo.channels == 2 => Some(Track {
            channels: 2,
            rate,
            samples: stereo.samples.clone(),
        }),
        [left, right] if left.channels == 1 && right.channels == 1 => {
            let left: Vec<i16> = left.samples.clone();
            let right: Vec<i16> = right.samples.clone();
            Some(Track {
                channels: 2,
                rate,
                samples: interleave(&left, &right),
            })
        }
        _ => None,
    }
}

/// What the player should do right now, from the settings and the window state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicState {
    enabled: bool,
    volume: u8,
    focused: bool,
    track: bool,
}

impl Default for MusicState {
    fn default() -> Self {
        Self {
            enabled: false,
            volume: 80,
            focused: true,
            track: false,
        }
    }
}

impl MusicState {
    /// Music plays only when it is enabled, a track is available, and the window has focus.
    #[must_use]
    pub const fn should_play(&self) -> bool {
        self.enabled && self.focused && self.track
    }

    /// Volume in percent, clamped to 0..=100.
    #[must_use]
    pub const fn volume_percent(&self) -> u8 {
        if self.volume > 100 {
            100
        } else {
            self.volume
        }
    }

    /// Applies the settings toggle immediately.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Applies the settings volume immediately.
    pub fn set_volume(&mut self, volume: u8) {
        self.volume = volume.min(100);
    }

    /// Window focus: losing it stops the music, regaining it resumes.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    /// Records whether a track was loaded for the open save's game.
    pub fn set_track_available(&mut self, available: bool) {
        self.track = available;
    }
}

/// Receives the decisions. The real implementation is a streaming output that can loop and stop; it is not
/// part of this module.
pub trait MusicSink {
    /// Starts looping `track` at `volume` percent.
    fn start(&mut self, track: &Track, volume: u8);
    /// Stops playback started by [`MusicSink::start`].
    fn stop(&mut self);
}

/// Brings the sink in line with the state, starting or stopping only on a change.
#[derive(Debug, Default)]
pub struct MusicPlayer {
    playing: bool,
}

impl MusicPlayer {
    /// Applies `state` to `sink`; `track` is the loaded track when one exists.
    pub fn sync(&mut self, state: &MusicState, track: Option<&Track>, sink: &mut dyn MusicSink) {
        match (state.should_play(), track) {
            (true, Some(track)) if !self.playing => {
                sink.start(track, state.volume_percent());
                self.playing = true;
            }
            (false, _) | (true, None) if self.playing => {
                sink.stop();
                self.playing = false;
            }
            _ => {}
        }
    }

    /// Whether the sink currently plays.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        events: Vec<&'static str>,
        volumes: Vec<u8>,
    }

    impl MusicSink for Recorder {
        fn start(&mut self, _track: &Track, volume: u8) {
            self.events.push("start");
            self.volumes.push(volume);
        }

        fn stop(&mut self) {
            self.events.push("stop");
        }
    }

    fn track() -> Track {
        Track {
            channels: 2,
            rate: 44_100,
            samples: vec![0; 4],
        }
    }

    #[test]
    fn each_family_names_its_own_menu_track() {
        assert_eq!(music_paths("cop"), Some(&["sounds/music/menu.ogg"][..]));
        assert_eq!(music_paths("clear_sky"), Some(&["sounds/music/wasteland2.ogg"][..]));
        assert_eq!(music_paths("soc").map(<[&str]>::len), Some(2));
        assert_eq!(music_paths("s2"), None, "the second game has no music in this table");
    }

    #[test]
    fn missing_or_broken_files_give_silence_not_an_error() {
        assert_eq!(decode_track("cop", |_| None), None);
        assert_eq!(decode_track("cop", |_| Some(b"not ogg".to_vec())), None);
        assert_eq!(
            decode_track("soc", |path| (path.ends_with("_l.ogg")).then(|| b"x".to_vec())),
            None
        );
        assert_eq!(decode_track("unknown", |_| Some(Vec::new())), None);
    }

    #[test]
    fn two_mono_halves_interleave_and_pad_the_shorter() {
        assert_eq!(interleave(&[1, 2], &[3]), vec![1, 3, 2, 0]);
        assert_eq!(interleave(&[], &[]), Vec::<i16>::new());
    }

    #[test]
    fn toggle_and_focus_decide_whether_music_plays() {
        let mut state = MusicState::default();
        assert!(!state.should_play(), "music is off by default");
        state.set_track_available(true);
        assert!(!state.should_play());
        state.set_enabled(true);
        assert!(state.should_play());
        state.set_focused(false);
        assert!(!state.should_play(), "losing focus stops the music");
        state.set_focused(true);
        assert!(state.should_play(), "regaining focus resumes it");
    }

    #[test]
    fn volume_is_clamped_and_applied_at_start() {
        let mut state = MusicState::default();
        state.set_volume(250);
        assert_eq!(state.volume_percent(), 100);
        state.set_enabled(true);
        state.set_track_available(true);
        let mut player = MusicPlayer::default();
        let mut sink = Recorder::default();
        player.sync(&state, Some(&track()), &mut sink);
        assert_eq!(sink.volumes, vec![100]);
    }

    #[test]
    fn the_player_starts_once_and_stops_on_change() {
        let mut state = MusicState::default();
        state.set_track_available(true);
        state.set_enabled(true);
        let mut player = MusicPlayer::default();
        let mut sink = Recorder::default();
        player.sync(&state, Some(&track()), &mut sink);
        player.sync(&state, Some(&track()), &mut sink);
        assert_eq!(sink.events, vec!["start"], "a second sync must not restart the track");
        state.set_focused(false);
        player.sync(&state, Some(&track()), &mut sink);
        state.set_enabled(false);
        player.sync(&state, Some(&track()), &mut sink);
        assert_eq!(sink.events, vec!["start", "stop"]);
        assert!(!player.is_playing());
    }

    #[test]
    fn no_track_means_no_start_and_no_error() {
        let mut state = MusicState::default();
        state.set_enabled(true);
        let mut player = MusicPlayer::default();
        let mut sink = Recorder::default();
        player.sync(&state, None, &mut sink);
        assert!(sink.events.is_empty());
    }
}
