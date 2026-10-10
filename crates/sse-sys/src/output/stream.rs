//! Looping, stoppable playback of an in-memory track.
//!
//! The track is written to a [`ChunkSink`] a chunk at a time, from a dedicated thread, and wraps to its start
//! until [`LoopingPlayback::stop`] or drop. A stop takes effect after the chunk in progress, so the sink must
//! return from `write` in bounded time. Platform backends implement `ChunkSink`; this module has no audio
//! device of its own.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Frames written per chunk. At 48 kHz this is about 85 ms, which bounds the delay before a stop.
pub const CHUNK_FRAMES: usize = 4096;

/// Receives interleaved signed-16 samples in order, one chunk per call.
pub trait ChunkSink: Send + 'static {
    /// Writes one chunk. Returns `false` when the device is gone and playback must end.
    fn write(&mut self, samples: &[i16]) -> bool;
}

/// A track playing on its own thread until stopped.
pub struct LoopingPlayback {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LoopingPlayback {
    /// Starts looping `samples` (interleaved, `channels` per frame) into `sink`.
    ///
    /// Returns `None` for an empty track, a channel count other than 1 or 2, a sample count that is not a
    /// whole number of frames, or when the thread cannot be created. Nothing is played in those cases.
    #[must_use]
    pub fn start<S: ChunkSink>(samples: Vec<i16>, channels: u8, mut sink: S) -> Option<Self> {
        if samples.is_empty()
            || !(1..=2).contains(&channels)
            || samples.len().checked_rem(usize::from(channels)) != Some(0)
        {
            return None;
        }
        let step = CHUNK_FRAMES * usize::from(channels);
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = thread::Builder::new()
            .name("sse-ui-music".to_owned())
            .spawn(move || {
                let mut position: usize = 0;
                while !flag.load(Ordering::Acquire) {
                    let end = position.saturating_add(step).min(samples.len());
                    let Some(chunk) = samples.get(position..end) else { break };
                    if !sink.write(chunk) {
                        break;
                    }
                    position = if end >= samples.len() { 0 } else { end };
                }
            })
            .ok()?;
        Some(Self {
            stop,
            thread: Some(thread),
        })
    }

    /// Stops playback and waits for the thread to finish. Calling it again does nothing.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }

    /// Whether the playback thread is still writing.
    #[must_use]
    pub fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|thread| !thread.is_finished())
    }
}

impl Drop for LoopingPlayback {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Sends the length of each chunk to the test; `keep_going` decides whether the sink keeps accepting.
    struct Recorder {
        lengths: mpsc::Sender<usize>,
        keep_going: bool,
    }

    impl ChunkSink for Recorder {
        fn write(&mut self, samples: &[i16]) -> bool {
            let _ = self.lengths.send(samples.len());
            self.keep_going
        }
    }

    fn wait_for(lengths: &mpsc::Receiver<usize>, count: usize) -> Vec<usize> {
        (0..count)
            .map(|_| match lengths.recv_timeout(Duration::from_secs(5)) {
                Ok(length) => length,
                Err(error) => panic!("no chunk arrived: {error}"),
            })
            .collect()
    }

    #[test]
    fn a_track_longer_than_one_chunk_is_split_and_then_wraps() {
        let (sender, receiver) = mpsc::channel();
        // One mono frame more than a chunk: chunk, then the single leftover frame, then the wrap.
        let samples = vec![0_i16; CHUNK_FRAMES + 1];
        let mut playback = LoopingPlayback::start(
            samples,
            1,
            Recorder {
                lengths: sender,
                keep_going: true,
            },
        )
        .unwrap_or_else(|| panic!("valid track starts"));

        assert_eq!(wait_for(&receiver, 4), vec![CHUNK_FRAMES, 1, CHUNK_FRAMES, 1]);
        playback.stop();
        assert!(!playback.is_running());
    }

    #[test]
    fn stereo_chunks_hold_whole_frames() {
        let (sender, receiver) = mpsc::channel();
        let samples = vec![0_i16; 2 * (CHUNK_FRAMES + 3)];
        let mut playback = LoopingPlayback::start(
            samples,
            2,
            Recorder {
                lengths: sender,
                keep_going: true,
            },
        )
        .unwrap_or_else(|| panic!("valid stereo track starts"));

        assert_eq!(wait_for(&receiver, 2), vec![2 * CHUNK_FRAMES, 6]);
        playback.stop();
    }

    #[test]
    fn stop_ends_the_thread_and_later_calls_do_nothing() {
        let (sender, receiver) = mpsc::channel();
        let mut playback = LoopingPlayback::start(
            vec![0_i16; 8],
            1,
            Recorder {
                lengths: sender,
                keep_going: true,
            },
        )
        .unwrap_or_else(|| panic!("valid track starts"));
        let _ = wait_for(&receiver, 1);
        let started = Instant::now();
        playback.stop();
        playback.stop();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(!playback.is_running());
    }

    #[test]
    fn a_refused_chunk_ends_playback_without_further_writes() {
        let (sender, receiver) = mpsc::channel();
        let mut playback = LoopingPlayback::start(
            vec![0_i16; 8],
            1,
            Recorder {
                lengths: sender,
                keep_going: false,
            },
        )
        .unwrap_or_else(|| panic!("valid track starts"));
        assert_eq!(wait_for(&receiver, 1), vec![8]);
        playback.stop();
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn invalid_tracks_are_never_started() {
        let silent = || Recorder {
            lengths: mpsc::channel().0,
            keep_going: true,
        };
        assert!(LoopingPlayback::start(Vec::new(), 1, silent()).is_none());
        assert!(LoopingPlayback::start(vec![0; 4], 0, silent()).is_none());
        assert!(LoopingPlayback::start(vec![0; 4], 3, silent()).is_none());
        assert!(LoopingPlayback::start(vec![0; 3], 2, silent()).is_none());
    }
}
