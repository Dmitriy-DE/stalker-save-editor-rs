//! Non-blocking output of short interleaved signed-16 PCM UI sounds.

use std::thread;

pub mod stream;

pub use stream::{chunk_duration, ChunkSink, LoopingPlayback, CHUNK_FRAMES};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// A best-effort sink for short UI sounds.
///
/// Implementations return from [`Output::play`] without waiting for playback. A missing or
/// unavailable audio service is intentionally silent rather than an application error.
pub trait Output {
    /// Queues interleaved signed-16 PCM for playback.
    fn play(&mut self, pcm: &[i16], channels: u8, rate: u32, volume: f32);
}

/// The native output backend for the current operating system.
#[derive(Debug, Default)]
pub struct SystemOutput;

impl Output for SystemOutput {
    fn play(&mut self, pcm: &[i16], channels: u8, rate: u32, volume: f32) {
        if pcm.is_empty() || channels == 0 || channels > 2 || !(8_000..=768_000).contains(&rate) {
            return;
        }
        let owned = pcm.to_vec();
        let volume = volume.clamp(0.0, 1.0);
        let _ = thread::Builder::new().name("sse-ui-sound".to_owned()).spawn(move || {
            #[cfg(target_os = "linux")]
            linux::play(owned, channels, rate, volume);
            #[cfg(target_os = "windows")]
            windows::play(owned, channels, rate, volume);
            #[cfg(target_os = "macos")]
            macos::play(owned, channels, rate, volume);
            #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
            let _ = (owned, channels, rate, volume);
        });
    }
}

/// Starts looping `samples` (interleaved signed-16 PCM) until the returned handle is stopped or dropped.
///
/// On Linux the track streams into one PulseAudio playback stream that is removed on stop. Elsewhere it is
/// written as repeated one-shot buffers, paced to their length, so a stop takes effect within one chunk.
/// Returns `None` when the audio service is unavailable or the track has an invalid shape.
#[must_use]
pub fn start_music(samples: Vec<i16>, channels: u8, rate: u32, volume: f32) -> Option<LoopingPlayback> {
    if !(8_000..=768_000).contains(&rate) {
        return None;
    }
    let volume = volume.clamp(0.0, 1.0);
    #[cfg(target_os = "linux")]
    {
        let sink = linux::MusicStream::open(channels, rate, volume).ok()?;
        LoopingPlayback::start(samples, channels, sink)
    }
    #[cfg(not(target_os = "linux"))]
    {
        LoopingPlayback::start(samples, channels, RepeatedPlay { channels, rate, volume })
    }
}

/// Repeats one-shot buffers through the native output on platforms without a streaming backend here.
#[cfg(not(target_os = "linux"))]
struct RepeatedPlay {
    channels: u8,
    rate: u32,
    volume: f32,
}

#[cfg(not(target_os = "linux"))]
impl ChunkSink for RepeatedPlay {
    fn write(&mut self, samples: &[i16]) -> bool {
        SystemOutput.play(samples, self.channels, self.rate, self.volume);
        let frames = samples.len().checked_div(usize::from(self.channels)).unwrap_or(0);
        thread::sleep(chunk_duration(frames, self.rate));
        true
    }
}

#[allow(clippy::cast_possible_truncation)]
#[cfg(any(not(target_arch = "wasm32"), test))]
fn scaled_sample(sample: i16, volume: f32) -> i16 {
    let value = f32::from(sample) * volume;
    value.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sample_scaling_clamps_and_rounds() {
        assert_eq!(scaled_sample(10_000, 0.5), 5_000);
        assert_eq!(scaled_sample(-10_000, 0.5), -5_000);
        assert_eq!(scaled_sample(i16::MAX, 2.0), i16::MAX);
    }

    #[test]
    fn invalid_shapes_are_silent() {
        let mut output = SystemOutput;
        output.play(&[1, 2], 0, 48_000, 1.0);
        output.play(&[1, 2], 2, 1, 1.0);
    }

    /// Plays a tone on the live PulseAudio server for about three seconds. Run by hand with an audio server
    /// present; it proves the stream is created, keeps running and stops.
    #[test]
    #[ignore = "needs a live audio server; run by hand"]
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    fn live_tone_streams_and_stops() {
        let pcm: Vec<i16> = (0..48_000_u32 * 3)
            .map(|index| {
                let phase = f64::from(index) * 2.0 * std::f64::consts::PI * 440.0 / 48_000.0;
                (phase.sin() * 6_000.0).round() as i16
            })
            .collect();
        let Some(mut playback) = start_music(pcm, 1, 48_000, 0.3) else {
            panic!("no audio stream could be opened");
        };
        std::thread::sleep(Duration::from_secs(2));
        assert!(playback.is_running(), "the stream stopped on its own");
        playback.stop();
        assert!(!playback.is_running());
    }
}
