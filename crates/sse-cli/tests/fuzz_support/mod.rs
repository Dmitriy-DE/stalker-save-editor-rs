//! Shared deterministic, bounded mutation harness for binary-format regression fuzz tests.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const MAX_MUTATED_INPUT: usize = 1024 * 1024;
const MAX_PEAK_RSS_KIB: u64 = 256 * 1024;
const PER_INPUT_LIMIT: Duration = Duration::from_secs(1);

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0.wrapping_shl(13);
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0.wrapping_shl(17);
        self.0
    }
}

fn mutate(seed: &[u8], random: &mut XorShift) -> Vec<u8> {
    let mut result = seed.to_vec();
    let change_count = usize::try_from((random.next() & 3).wrapping_add(1)).unwrap_or(1);
    for _ in 0..change_count {
        match random.next() & 7 {
            0 | 1 => {
                if !result.is_empty() {
                    let length = u64::try_from(result.len()).unwrap_or(u64::MAX);
                    let offset =
                        usize::try_from(random.next().checked_rem(length).unwrap_or_default()).unwrap_or_default();
                    if let Some(byte) = result.get_mut(offset) {
                        *byte ^= u8::try_from(random.next() & 0xff).unwrap_or_default();
                    }
                }
            }
            2 => {
                let length = u64::try_from(result.len().saturating_add(1)).unwrap_or(u64::MAX);
                let new_length =
                    usize::try_from(random.next().checked_rem(length).unwrap_or_default()).unwrap_or_default();
                result.truncate(new_length);
            }
            3 => {
                if result.len() >= 4 {
                    let hostile_length = match random.next() & 3 {
                        0 => 0_u32,
                        1 => u32::MAX,
                        2 => 0x1000_0001,
                        _ => 0x7fff_ffff,
                    };
                    if let Some(header) = result.get_mut(..4) {
                        header.copy_from_slice(&hostile_length.to_le_bytes());
                    }
                }
            }
            4 => {
                if result.len() < MAX_MUTATED_INPUT {
                    result.push(u8::try_from(random.next() & 0xff).unwrap_or_default());
                }
            }
            5 => result.reverse(),
            _ => {
                if !result.is_empty() {
                    let length = u64::try_from(result.len()).unwrap_or(u64::MAX);
                    let offset =
                        usize::try_from(random.next().checked_rem(length).unwrap_or_default()).unwrap_or_default();
                    if let Some(byte) = result.get_mut(offset) {
                        *byte = 0xff;
                    }
                }
            }
        }
    }
    result
}

pub fn run<F>(seed: &[u8], iterations: usize, random_seed: u64, parser: F)
where
    F: FnMut(&[u8]) + Send + 'static,
{
    let total_started = Instant::now();
    assert!(!seed.is_empty(), "fuzz corpus seed is empty");
    assert!(seed.len() <= MAX_MUTATED_INPUT, "fuzz corpus seed exceeds its cap");

    let (job_sender, job_receiver) = mpsc::channel::<(usize, Vec<u8>)>();
    let (result_sender, result_receiver) = mpsc::channel::<(usize, bool, Duration)>();
    let worker = thread::spawn(move || {
        let mut parser = parser;
        while let Ok((iteration, input)) = job_receiver.recv() {
            let started = Instant::now();
            let completed = catch_unwind(AssertUnwindSafe(|| parser(&input))).is_ok();
            let elapsed = started.elapsed();
            if result_sender.send((iteration, completed, elapsed)).is_err() {
                break;
            }
        }
    });

    let mut random = XorShift(random_seed.max(1));
    let mut slowest_input = Duration::ZERO;
    let mut peak_rss = None;
    for iteration in 0..iterations {
        let input = mutate(seed, &mut random);
        assert!(input.len() <= MAX_MUTATED_INPUT, "mutator exceeded its input cap");
        assert!(job_sender.send((iteration, input)).is_ok(), "fuzz worker stopped early");
        let Ok((completed_iteration, completed, elapsed)) = result_receiver.recv_timeout(PER_INPUT_LIMIT) else {
            panic!("format parser exceeded the one-second per-input limit at mutation {iteration}");
        };
        assert_eq!(
            completed_iteration, iteration,
            "fuzz worker returned an out-of-order result"
        );
        assert!(
            completed,
            "format parser panicked at deterministic mutation {iteration}"
        );
        assert!(
            elapsed < PER_INPUT_LIMIT,
            "format parser exceeded the one-second per-input limit"
        );
        slowest_input = slowest_input.max(elapsed);
        if iteration % 64 == 0 {
            peak_rss = assert_peak_rss_below_limit();
        }
    }
    drop(job_sender);
    assert!(worker.join().is_ok(), "fuzz worker panicked");
    peak_rss = assert_peak_rss_below_limit().or(peak_rss);
    eprintln!(
        "{iterations} mutations: {:.3}s total; slowest parser call {:.3}ms; peak RSS {} KiB",
        total_started.elapsed().as_secs_f64(),
        slowest_input.as_secs_f64() * 1000.0,
        peak_rss.map_or_else(|| "unavailable".to_owned(), |value| value.to_string())
    );
}

#[cfg(target_os = "linux")]
fn assert_peak_rss_below_limit() -> Option<u64> {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        panic!("Linux peak-RSS counter is unavailable");
    };
    let peak_rss_kib = status
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmHWM:")
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse::<u64>().ok())
        })
        .unwrap_or(u64::MAX);
    assert!(
        peak_rss_kib <= MAX_PEAK_RSS_KIB,
        "peak RSS exceeded 256 MiB: {peak_rss_kib} KiB"
    );
    Some(peak_rss_kib)
}

#[cfg(not(target_os = "linux"))]
fn assert_peak_rss_below_limit() -> Option<u64> {
    None
}
