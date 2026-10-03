//! Non-blocking output of short interleaved signed-16 PCM UI sounds.

use std::thread;

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

#[allow(clippy::cast_possible_truncation)]
fn scaled_sample(sample: i16, volume: f32) -> i16 {
    let value = f32::from(sample) * volume;
    value.round().clamp(f32::from(i16::MIN), f32::from(i16::MAX)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
