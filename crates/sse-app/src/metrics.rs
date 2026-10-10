//! Privacy-limited local performance metrics.

use crate::diagnostics::log_directory;
use sse_codecs::json::{Event, Reader};
use sse_core::{Error, Result};
use sse_sys::fetch::{Fetch, SystemFetch};
use std::collections::{BTreeMap, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

/// Destination reserved for the future aggregate metrics route on the existing Worker.
pub const METRICS_UPLOAD_ENDPOINT: &str = "https://save-editor-downloads.save-editor.workers.dev/metrics";
const MAX_METRICS_RESPONSE_BYTES: usize = 4 * 1024;
const MAX_METRICS_REQUEST_BYTES: usize = 16 * 1024;

const METRICS_FILE: &str = "metrics.jsonl";
const ROTATED_METRICS_FILE: &str = "metrics.jsonl.1";
const MAX_METRICS_BYTES: u64 = 1024 * 1024;
const MAX_SAMPLES: usize = 8192;
const METRICS_WRITER_QUEUE_CAPACITY: usize = 64;
const RECENT_SESSION_LIMIT: usize = 5;
const FORMATS: [&str; 8] = [
    "stalker-soc",
    "stalker-cs",
    "stalker-cop",
    "stalker-soc-ee",
    "stalker-cs-ee",
    "stalker-cop-ee",
    "stalker2",
    "unknown",
];
const SIZE_BUCKETS: [&str; 6] = [
    "0-64-KiB",
    "64-256-KiB",
    "256-KiB-1-MiB",
    "1-4-MiB",
    "4-16-MiB",
    "16-MiB+",
];

static SESSION: OnceLock<Mutex<Option<ActiveSession>>> = OnceLock::new();
static METRICS_WRITER: OnceLock<Option<SyncSender<WriterMessage>>> = OnceLock::new();
static DROPPED_METRICS_LINES: AtomicU64 = AtomicU64::new(0);

enum WriterMessage {
    Append(MetricsBatch),
    Flush(SyncSender<()>),
}

struct MetricsBatch {
    directory: std::path::PathBuf,
    lines: Vec<String>,
    dropped_before: u64,
}

struct ActiveSession {
    started: Instant,
    first_frame_recorded: bool,
    screen_switch_started: Option<Instant>,
    environment: Option<(u32, u32, u32)>,
    peak_memory_bytes: u64,
    last_memory_sample: Instant,
}

fn active_session() -> &'static Mutex<Option<ActiveSession>> {
    SESSION.get_or_init(|| Mutex::new(None))
}

fn lock_session() -> MutexGuard<'static, Option<ActiveSession>> {
    active_session()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts a local session and records only the operating-system category.
pub fn start_session() {
    let mut session = lock_session();
    if session.is_some() {
        return;
    }
    *session = Some(ActiveSession {
        started: Instant::now(),
        first_frame_recorded: false,
        screen_switch_started: None,
        environment: None,
        peak_memory_bytes: 0,
        last_memory_sample: Instant::now(),
    });
    append_record(&format!("{{\"event\":\"session_start\",\"os\":\"{}\"}}", os_category()));
}

/// Closes the active local session.
pub fn end_session() {
    record_memory_sample(true);
    let mut session = lock_session();
    if session.take().is_some() {
        append_record("{\"event\":\"session_end\"}");
    }
    drop(session);
    flush_metrics_writer();
}

/// Records the time from application startup to the first presented frame once per session.
pub fn record_first_frame() {
    let mut session = lock_session();
    let Some(active) = session.as_mut() else { return };
    if active.first_frame_recorded {
        return;
    }
    active.first_frame_recorded = true;
    let elapsed_ms = elapsed_millis(active.started.elapsed());
    append_record(&format!("{{\"event\":\"first_frame\",\"ms\":{elapsed_ms}}}"));
    drop(session);
    record_memory_sample(true);
}

/// Records the time spent building a newly selected screen.
pub fn record_screen_switch(elapsed: Duration) {
    let elapsed_ms = elapsed_millis(elapsed);
    record(&format!("{{\"event\":\"screen_switch\",\"ms\":{elapsed_ms}}}"));
}

/// Starts a screen-switch measurement that will finish after the new screen is presented.
pub fn begin_screen_switch() {
    let mut session = lock_session();
    let Some(active) = session.as_mut() else { return };
    active.screen_switch_started.get_or_insert_with(Instant::now);
}

/// Completes the current screen-switch measurement after a frame is presented.
pub fn record_screen_switch_presented() {
    let elapsed = {
        let mut session = lock_session();
        let Some(active) = session.as_mut() else { return };
        active.screen_switch_started.take().map(|started| started.elapsed())
    };
    if let Some(elapsed) = elapsed {
        record_screen_switch(elapsed);
    }
}

/// Records one presented frame while the user is scrolling.
pub fn record_scroll_frame(elapsed: Duration) {
    let elapsed_ms = elapsed_millis(elapsed);
    record(&format!("{{\"event\":\"scroll_frame\",\"ms\":{elapsed_ms}}}"));
}

/// Records a burst of scrolling frame measurements with one bounded background-writer enqueue.
pub fn record_scroll_frames(samples: &[Duration]) {
    if samples.is_empty() || lock_session().is_none() {
        return;
    }
    let lines = samples
        .iter()
        .map(|elapsed| format!("{{\"event\":\"scroll_frame\",\"ms\":{}}}", elapsed_millis(*elapsed)))
        .collect::<Vec<_>>();
    enqueue_records(lines);
}

/// Records a save read using a fixed format name and a broad size bucket.
pub fn record_save_read(format: &str, size_bytes: u64, elapsed: Duration) {
    record_save_operation("read", format, size_bytes, elapsed);
}

/// Records a save write using a fixed format name and a broad size bucket.
pub fn record_save_write(format: &str, size_bytes: u64, elapsed: Duration) {
    record_save_operation("write", format, size_bytes, elapsed);
}

fn record_save_operation(operation: &str, format: &str, size_bytes: u64, elapsed: Duration) {
    let format = safe_format(format);
    let bucket = size_bucket(size_bytes);
    let elapsed_ms = elapsed_millis(elapsed);
    record(&format!(
        "{{\"event\":\"save_{operation}\",\"format\":\"{format}\",\"size_bucket\":\"{bucket}\",\"ms\":{elapsed_ms}}}"
    ));
}

/// Records save-directory discovery duration.
pub fn record_save_discovery(elapsed: Duration) {
    let elapsed_ms = elapsed_millis(elapsed);
    record(&format!("{{\"event\":\"save_discovery\",\"ms\":{elapsed_ms}}}"));
}

/// Records the current window dimensions and backing/UI scale when they change.
pub fn record_environment(width: u32, height: u32, scale: f32) {
    let scale_milli = if scale.is_finite() {
        (f64::from(scale.clamp(0.1, 10.0)) * 1000.0).round()
    } else {
        1000.0
    };
    let scale_milli = format!("{scale_milli:.0}").parse::<u32>().unwrap_or(1000);
    let mut session = lock_session();
    let Some(active) = session.as_mut() else { return };
    let environment = (width, height, scale_milli);
    if active.environment == Some(environment) {
        return;
    }
    active.environment = Some(environment);
    append_record(&format!(
        "{{\"event\":\"environment\",\"os\":\"{}\",\"width\":{width},\"height\":{height},\"scale_milli\":{scale_milli}}}",
        os_category()
    ));
}

/// Samples the operating-system high-water memory value at a bounded interval.
pub fn sample_peak_memory() {
    record_memory_sample(false);
}

fn record_memory_sample(force: bool) {
    let mut session = lock_session();
    let Some(active) = session.as_mut() else { return };
    if !force && active.last_memory_sample.elapsed() < Duration::from_secs(5) {
        return;
    }
    active.last_memory_sample = Instant::now();
    let Some(bytes) = peak_resident_memory_bytes() else {
        return;
    };
    if bytes <= active.peak_memory_bytes {
        return;
    }
    active.peak_memory_bytes = bytes;
    append_record(&format!("{{\"event\":\"peak_memory\",\"bytes\":{bytes}}}"));
}

/// Returns a high-water resident memory estimate where the operating system exposes one.
#[must_use]
pub fn peak_resident_memory_bytes() -> Option<u64> {
    sse_sys::process::peak_resident_memory_bytes()
}

fn record(line: &str) {
    if lock_session().is_some() {
        append_record(line);
    }
}

fn append_record(line: &str) {
    enqueue_records(vec![line.to_owned()]);
}

fn enqueue_records(lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    let directory = log_directory();
    let Some(sender) = metrics_writer() else {
        let dropped = u64::try_from(lines.len()).unwrap_or(u64::MAX);
        DROPPED_METRICS_LINES.fetch_add(dropped, Ordering::Relaxed);
        return;
    };
    let batch = MetricsBatch {
        directory,
        lines,
        dropped_before: DROPPED_METRICS_LINES.swap(0, Ordering::Relaxed),
    };
    match sender.try_send(WriterMessage::Append(batch)) {
        Ok(()) => {}
        Err(TrySendError::Full(WriterMessage::Append(batch))) => {
            record_dropped_batch(batch);
        }
        Err(TrySendError::Disconnected(WriterMessage::Append(batch))) => {
            record_dropped_batch(batch);
        }
        Err(TrySendError::Full(WriterMessage::Flush(_))) | Err(TrySendError::Disconnected(WriterMessage::Flush(_))) => {
        }
    }
}

fn metrics_writer() -> Option<&'static SyncSender<WriterMessage>> {
    METRICS_WRITER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel(METRICS_WRITER_QUEUE_CAPACITY);
            thread::Builder::new()
                .name("sse-metrics-writer".to_owned())
                .spawn(move || metrics_writer_loop(receiver))
                .ok()
                .map(|_| sender)
        })
        .as_ref()
}

fn metrics_writer_loop(receiver: Receiver<WriterMessage>) {
    while let Ok(message) = receiver.recv() {
        match message {
            WriterMessage::Append(batch) => append_metrics_batch(batch),
            WriterMessage::Flush(acknowledge) => {
                let _ = acknowledge.send(());
            }
        }
    }
}

fn flush_metrics_writer() {
    let Some(sender) = metrics_writer() else { return };
    let dropped = DROPPED_METRICS_LINES.swap(0, Ordering::Relaxed);
    if dropped != 0 {
        let batch = MetricsBatch {
            directory: log_directory(),
            lines: Vec::new(),
            dropped_before: dropped,
        };
        if let Err(error) = sender.send(WriterMessage::Append(batch)) {
            if let WriterMessage::Append(batch) = error.0 {
                DROPPED_METRICS_LINES.fetch_add(batch.dropped_before, Ordering::Relaxed);
            }
        }
    }
    let (acknowledge, flushed) = mpsc::sync_channel(0);
    if sender.send(WriterMessage::Flush(acknowledge)).is_ok() {
        let _ = flushed.recv();
    }
}

fn record_dropped_batch(batch: MetricsBatch) {
    let line_count = u64::try_from(batch.lines.len()).unwrap_or(u64::MAX);
    let dropped = batch.dropped_before.saturating_add(line_count);
    DROPPED_METRICS_LINES.fetch_add(dropped, Ordering::Relaxed);
}

fn append_metrics_batch(batch: MetricsBatch) {
    let MetricsBatch {
        directory,
        lines,
        dropped_before,
    } = batch;
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join(METRICS_FILE);
    let rotated = directory.join(ROTATED_METRICS_FILE);
    let mut payload = String::new();
    if dropped_before != 0 {
        payload.push_str(&format!(
            "{{\"event\":\"dropped_metrics\",\"count\":{dropped_before}}}\n"
        ));
    }
    for line in lines {
        payload.push_str(&line);
        payload.push('\n');
    }
    let payload_bytes = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    if payload_bytes == 0 || payload_bytes > MAX_METRICS_BYTES {
        return;
    }
    let current_bytes = match fs::metadata(&path) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => return,
    };
    let rotated_now = current_bytes.saturating_add(payload_bytes) > MAX_METRICS_BYTES;
    if rotated_now {
        let continuation_bytes = u64::try_from(b"{\"event\":\"session_continue\"}\n".len()).unwrap_or(u64::MAX);
        if payload_bytes.saturating_add(continuation_bytes) > MAX_METRICS_BYTES {
            return;
        }
        match fs::remove_file(&rotated) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return,
        }
        match fs::rename(&path, &rotated) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return,
        }
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        if rotated_now {
            let _ = file.write_all(b"{\"event\":\"session_continue\"}\n");
        }
        let _ = file.write_all(payload.as_bytes());
    }
}

fn elapsed_millis(elapsed: Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn os_category() -> &'static str {
    match std::env::consts::OS {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "macos",
        _ => "other",
    }
}

fn safe_format(format: &str) -> &'static str {
    FORMATS
        .iter()
        .copied()
        .find(|candidate| *candidate == format)
        .unwrap_or("unknown")
}

fn safe_size_bucket(bucket: &str) -> &'static str {
    SIZE_BUCKETS
        .iter()
        .copied()
        .find(|candidate| *candidate == bucket)
        .unwrap_or(SIZE_BUCKETS[5])
}

fn size_bucket(bytes: u64) -> &'static str {
    match bytes {
        0..=65_535 => SIZE_BUCKETS[0],
        65_536..=262_143 => SIZE_BUCKETS[1],
        262_144..=1_048_575 => SIZE_BUCKETS[2],
        1_048_576..=4_194_303 => SIZE_BUCKETS[3],
        4_194_304..=16_777_215 => SIZE_BUCKETS[4],
        _ => SIZE_BUCKETS[5],
    }
}

/// One percentile triplet in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DurationPercentiles {
    /// 50th percentile.
    pub p50_ms: u64,
    /// 95th percentile.
    pub p95_ms: u64,
    /// Maximum sample.
    pub max_ms: u64,
}

/// One save operation aggregate grouped by format and source size.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveOperationSummary {
    /// Operation name: `read` or `write`.
    pub operation: &'static str,
    /// Fixed game-format category.
    pub format: &'static str,
    /// Broad source-size bucket.
    pub size_bucket: &'static str,
    /// Number of measured operations.
    pub count: u64,
    /// Integer average duration in milliseconds.
    pub average_ms: u64,
    /// Maximum duration in milliseconds.
    pub max_ms: u64,
}

/// Redacted metrics summary for one application session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionSummary {
    /// Whether the application recorded a normal end-of-session marker.
    pub ended: bool,
    /// Operating-system category, when recorded.
    pub operating_system: Option<&'static str>,
    /// Last recorded window resolution in physical pixels.
    pub resolution: Option<(u32, u32)>,
    /// Last recorded combined scale, expressed in percent.
    pub scale_percent: Option<u32>,
    /// Time from startup until the first frame, in milliseconds.
    pub first_frame_ms: Option<u64>,
    /// Screen-switch duration percentiles.
    pub screen_switch: Option<DurationPercentiles>,
    /// Scroll-frame duration percentiles.
    pub scrolling_frame: Option<DurationPercentiles>,
    /// Last measured save-discovery duration in milliseconds.
    pub save_discovery_ms: Option<u64>,
    /// Highest resident-memory value observed in bytes, when supported.
    pub peak_memory_bytes: Option<u64>,
    /// Save read/write totals by operation, format and size bucket.
    pub save_operations: Vec<SaveOperationSummary>,
    screen_switch_samples: VecDeque<u64>,
    scroll_samples: VecDeque<u64>,
    save_groups: BTreeMap<(&'static str, &'static str, &'static str), (u64, u64, u64)>,
}

impl SessionSummary {
    fn add_sample(samples: &mut VecDeque<u64>, value: u64) {
        if samples.len() == MAX_SAMPLES {
            samples.pop_front();
        }
        samples.push_back(value);
    }

    fn finish(&mut self) {
        self.screen_switch = percentiles(self.screen_switch_samples.make_contiguous());
        self.scrolling_frame = percentiles(self.scroll_samples.make_contiguous());
        self.save_operations = self
            .save_groups
            .iter()
            .map(
                |((operation, format, bucket), (count, total, maximum))| SaveOperationSummary {
                    operation,
                    format,
                    size_bucket: bucket,
                    count: *count,
                    average_ms: safe_average(*total, *count),
                    max_ms: *maximum,
                },
            )
            .collect();
    }
}

/// Returns up to five latest session summaries, newest first.
#[must_use]
pub fn recent_sessions() -> Vec<SessionSummary> {
    let directory = log_directory();
    let mut sessions = Vec::new();
    let mut current = None;
    read_metrics_file(&directory.join(ROTATED_METRICS_FILE), &mut sessions, &mut current);
    read_metrics_file(&directory.join(METRICS_FILE), &mut sessions, &mut current);
    if let Some(mut summary) = current {
        summary.finish();
        sessions.push(summary);
    }
    sessions.reverse();
    sessions.truncate(RECENT_SESSION_LIMIT);
    sessions
}

fn read_metrics_file(path: &Path, sessions: &mut Vec<SessionSummary>, current: &mut Option<SessionSummary>) {
    let Ok(contents) = fs::read_to_string(path) else { return };
    for line in contents.lines() {
        let Some(fields) = parse_fields(line) else { continue };
        let Some(event) = fields.get("event").copied() else {
            continue;
        };
        if event == "session_start" {
            if let Some(mut previous) = current.take() {
                previous.finish();
                sessions.push(previous);
            }
            *current = Some(SessionSummary {
                operating_system: fields.get("os").and_then(|value| safe_os(value)),
                ..SessionSummary::default()
            });
            continue;
        }
        if current.is_none() {
            *current = Some(SessionSummary::default());
        }
        let Some(summary) = current.as_mut() else { continue };
        match event {
            "session_continue" => {}
            "session_end" => {
                summary.ended = true;
                summary.finish();
                if let Some(closed) = current.take() {
                    sessions.push(closed);
                }
            }
            "first_frame" => summary.first_frame_ms = number(&fields, "ms"),
            "screen_switch" => {
                if let Some(value) = number(&fields, "ms") {
                    SessionSummary::add_sample(&mut summary.screen_switch_samples, value);
                }
            }
            "scroll_frame" => {
                if let Some(value) = number(&fields, "ms") {
                    SessionSummary::add_sample(&mut summary.scroll_samples, value);
                }
            }
            "save_read" | "save_write" => add_save_operation(summary, event, &fields),
            "save_discovery" => summary.save_discovery_ms = number(&fields, "ms"),
            "peak_memory" => summary.peak_memory_bytes = number(&fields, "bytes"),
            "environment" => {
                if let (Some(width), Some(height), Some(scale_milli)) = (
                    number(&fields, "width"),
                    number(&fields, "height"),
                    number(&fields, "scale_milli"),
                ) {
                    if width <= 32_768 && height <= 32_768 && scale_milli <= 10_000 {
                        if let (Ok(width), Ok(height), Ok(scale_percent)) = (
                            u32::try_from(width),
                            u32::try_from(height),
                            u32::try_from(scale_milli.saturating_add(5) / 10),
                        ) {
                            summary.resolution = Some((width, height));
                            summary.scale_percent = Some(scale_percent);
                            summary.operating_system = fields
                                .get("os")
                                .and_then(|value| safe_os(value))
                                .or(summary.operating_system);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn add_save_operation(summary: &mut SessionSummary, event: &str, fields: &BTreeMap<&str, &str>) {
    let operation = if event == "save_read" { "read" } else { "write" };
    let format = fields.get("format").map_or("unknown", |value| safe_format(value));
    let bucket = fields
        .get("size_bucket")
        .map_or(SIZE_BUCKETS[5], |value| safe_size_bucket(value));
    let Some(milliseconds) = number(fields, "ms") else {
        return;
    };
    let aggregate = summary.save_groups.entry((operation, format, bucket)).or_default();
    aggregate.0 = aggregate.0.saturating_add(1);
    aggregate.1 = aggregate.1.saturating_add(milliseconds);
    aggregate.2 = aggregate.2.max(milliseconds);
}

fn parse_fields(line: &str) -> Option<BTreeMap<&str, &str>> {
    let body = line.trim().strip_prefix('{')?.strip_suffix('}')?;
    let mut fields = BTreeMap::new();
    for entry in body.split(',') {
        let (key, value) = entry.split_once(':')?;
        let key = key.trim().strip_prefix('"')?.strip_suffix('"')?;
        let value = value.trim().trim_matches('"');
        fields.insert(key, value);
    }
    Some(fields)
}

fn number(fields: &BTreeMap<&str, &str>, key: &str) -> Option<u64> {
    fields.get(key)?.parse::<u64>().ok()
}

fn safe_os(value: &str) -> Option<&'static str> {
    match value {
        "linux" => Some("linux"),
        "windows" => Some("windows"),
        "macos" => Some("macos"),
        "other" => Some("other"),
        _ => None,
    }
}

fn percentiles(samples: &[u64]) -> Option<DurationPercentiles> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let middle = sorted.len() / 2;
    let p50_ms = if sorted.len() % 2 == 0 {
        let lower = sorted.get(middle.saturating_sub(1)).copied().unwrap_or_default();
        let upper = sorted.get(middle).copied().unwrap_or_default();
        lower.saturating_add(upper.saturating_sub(lower) / 2)
    } else {
        sorted.get(middle).copied().unwrap_or_default()
    };
    let p95_rank = sorted.len().saturating_mul(95).saturating_add(99) / 100;
    let p95_index = p95_rank.saturating_sub(1).min(sorted.len().saturating_sub(1));
    Some(DurationPercentiles {
        p50_ms,
        p95_ms: sorted.get(p95_index).copied().unwrap_or_default(),
        max_ms: sorted.last().copied().unwrap_or_default(),
    })
}

/// Builds the exact aggregate request body preview. No request is sent by this function.
#[must_use]
pub fn upload_preview() -> String {
    let sessions = recent_sessions();
    let session_count = sessions.len();
    let first_frame = average(sessions.iter().filter_map(|session| session.first_frame_ms));
    let switch = percentiles(
        &sessions
            .iter()
            .flat_map(|session| session.screen_switch_samples.iter().copied())
            .collect::<Vec<_>>(),
    );
    let scroll = percentiles(
        &sessions
            .iter()
            .flat_map(|session| session.scroll_samples.iter().copied())
            .collect::<Vec<_>>(),
    );
    let discovery = average(sessions.iter().filter_map(|session| session.save_discovery_ms));
    let peak_memory_mib = sessions
        .iter()
        .filter_map(|session| session.peak_memory_bytes)
        .max()
        .map(|bytes| bytes / (1024 * 1024));
    let mut save_groups: BTreeMap<(&str, &str, &str), (u64, u64, u64)> = BTreeMap::new();
    for operation in sessions.iter().flat_map(|session| session.save_operations.iter()) {
        let group = save_groups
            .entry((operation.operation, operation.format, operation.size_bucket))
            .or_default();
        group.0 = group.0.saturating_add(operation.count);
        group.1 = group
            .1
            .saturating_add(operation.average_ms.saturating_mul(operation.count));
        group.2 = group.2.max(operation.max_ms);
    }
    let mut output = format!(
        "{{\"schema\":1,\"sessions\":{session_count},\"first_frame_avg_ms\":{},\"screen_switch_ms\":{{\"p50\":{},\"p95\":{},\"max\":{}}},\"scroll_frame_ms\":{{\"p50\":{},\"p95\":{},\"max\":{}}},\"save_discovery_avg_ms\":{},\"peak_memory_mib\":{},\"save_operations\":[",
        json_number(first_frame),
        triplet_value(switch, |triplet| triplet.p50_ms),
        triplet_value(switch, |triplet| triplet.p95_ms),
        triplet_value(switch, |triplet| triplet.max_ms),
        triplet_value(scroll, |triplet| triplet.p50_ms),
        triplet_value(scroll, |triplet| triplet.p95_ms),
        triplet_value(scroll, |triplet| triplet.max_ms),
        json_number(discovery),
        json_number(peak_memory_mib),
    );
    let mut first_group = true;
    for ((operation, format, bucket), (count, total, maximum)) in save_groups {
        let average = safe_average(total, count);
        if !first_group {
            output.push(',');
        }
        first_group = false;
        output.push_str(&format!(
            "{{\"operation\":\"{operation}\",\"format\":\"{format}\",\"size_bucket\":\"{bucket}\",\"count\":{count},\"average_ms\":{average},\"max_ms\":{maximum}}}"
        ));
    }
    output.push_str("]}");
    output
}

/// Sends the exact JSON preview shown to the user. The UI must have separate consent and must
/// display this payload before invoking the function. Requests are one-shot and never retried.
pub fn upload_aggregate_preview(consented: bool, preview: &str) -> Result<()> {
    let mut fetch = SystemFetch {
        max_bytes: u64::try_from(MAX_METRICS_RESPONSE_BYTES).unwrap_or(u64::MAX),
        ..SystemFetch::default()
    };
    upload_aggregate_preview_with(&mut fetch, METRICS_UPLOAD_ENDPOINT, consented, preview)
}

fn upload_aggregate_preview_with(fetch: &mut dyn Fetch, endpoint: &str, consented: bool, preview: &str) -> Result<()> {
    if !consented {
        return Err(Error::Refused(
            "aggregate metrics upload requires separate consent".to_owned(),
        ));
    }
    if !valid_https_endpoint(endpoint) {
        return Err(Error::Refused("only HTTPS metrics endpoints are allowed".to_owned()));
    }
    if preview.is_empty() || preview.len() > MAX_METRICS_REQUEST_BYTES {
        return Err(Error::Refused(
            "aggregate metrics payload is empty or too large".to_owned(),
        ));
    }
    validate_metrics_payload(preview)?;
    let mut response_body = Vec::new();
    let response = fetch.post(endpoint, "application/json", preview.as_bytes(), &mut |chunk| {
        let Some(next_len) = response_body.len().checked_add(chunk.len()) else {
            return false;
        };
        if next_len > MAX_METRICS_RESPONSE_BYTES {
            return false;
        }
        response_body.extend_from_slice(chunk);
        true
    })?;
    if response.final_url != endpoint || !valid_https_endpoint(&response.final_url) {
        return Err(Error::Refused("metrics endpoint redirected".to_owned()));
    }
    match response.status {
        201 => validate_report_id_response(&response_body),
        status @ (400 | 405 | 413 | 415 | 429 | 503) => Err(Error::Refused(format!(
            "metrics service rejected the request (HTTP {status})"
        ))),
        status => Err(Error::System(format!("metrics service returned HTTP {status}"))),
    }
}

fn validate_metrics_payload(preview: &str) -> Result<()> {
    #[derive(Clone, Copy)]
    enum RootField {
        Other,
        Schema,
        Sessions,
    }

    let invalid = || Error::Refused("metrics payload must contain schema 1 and sessions 0..1000".to_owned());
    let mut reader = Reader::new(preview.as_bytes());
    if !matches!(reader.next_event()?, Some(Event::ObjectStart)) {
        return Err(invalid());
    }

    let mut depth = 1_usize;
    let mut field = None;
    let mut schema_seen = false;
    let mut sessions_seen = false;
    let mut schema_valid = false;
    let mut sessions_valid = false;

    while let Some(event) = reader.next_event()? {
        match event {
            Event::Key(key) if depth == 1 => {
                field = Some(match key.as_str() {
                    "schema" => {
                        if schema_seen {
                            return Err(invalid());
                        }
                        schema_seen = true;
                        RootField::Schema
                    }
                    "sessions" => {
                        if sessions_seen {
                            return Err(invalid());
                        }
                        sessions_seen = true;
                        RootField::Sessions
                    }
                    _ => RootField::Other,
                });
            }
            Event::ObjectStart | Event::ArrayStart => {
                if depth == 1 && matches!(field, Some(RootField::Schema | RootField::Sessions)) {
                    return Err(invalid());
                }
                if depth == 1 {
                    field = None;
                }
                depth = depth.checked_add(1).ok_or_else(invalid)?;
            }
            Event::ObjectEnd | Event::ArrayEnd => {
                depth = depth.checked_sub(1).ok_or_else(invalid)?;
            }
            Event::Number(value) if depth == 1 => match field.take() {
                Some(RootField::Schema) => {
                    if value != "1" {
                        return Err(invalid());
                    }
                    schema_valid = true;
                }
                Some(RootField::Sessions) => {
                    let sessions = value.parse::<u64>().map_err(|_| invalid())?;
                    if sessions > 1000 {
                        return Err(invalid());
                    }
                    sessions_valid = true;
                }
                Some(RootField::Other) | None => {}
            },
            Event::String(_) | Event::Bool(_) | Event::Null if depth == 1 => {
                if matches!(field.take(), Some(RootField::Schema | RootField::Sessions)) {
                    return Err(invalid());
                }
            }
            Event::Key(_) | Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => {}
        }
    }

    if depth != 0 || !schema_valid || !sessions_valid {
        return Err(invalid());
    }
    Ok(())
}

fn validate_report_id_response(response_body: &[u8]) -> Result<()> {
    let invalid = || Error::Refused("metrics service returned an invalid report_id response".to_owned());
    let mut reader = Reader::new(response_body);
    if !matches!(reader.next_event()?, Some(Event::ObjectStart)) {
        return Err(invalid());
    }

    let mut depth = 1_usize;
    let mut report_id_field = false;
    let mut report_id_seen = false;
    let mut report_id_valid = false;
    while let Some(event) = reader.next_event()? {
        match event {
            Event::Key(key) if depth == 1 => {
                report_id_field = key.as_str() == "report_id";
                if report_id_field {
                    if report_id_seen {
                        return Err(invalid());
                    }
                    report_id_seen = true;
                }
            }
            Event::ObjectStart | Event::ArrayStart => {
                if depth == 1 && report_id_field {
                    return Err(invalid());
                }
                if depth == 1 {
                    report_id_field = false;
                }
                depth = depth.checked_add(1).ok_or_else(invalid)?;
            }
            Event::ObjectEnd | Event::ArrayEnd => {
                depth = depth.checked_sub(1).ok_or_else(invalid)?;
            }
            Event::String(value) if depth == 1 => {
                if report_id_field {
                    if value.as_str().trim().is_empty() {
                        return Err(invalid());
                    }
                    report_id_valid = true;
                }
                report_id_field = false;
            }
            Event::Number(_) | Event::Bool(_) | Event::Null if depth == 1 => {
                if report_id_field {
                    return Err(invalid());
                }
                report_id_field = false;
            }
            Event::Key(_) | Event::String(_) | Event::Number(_) | Event::Bool(_) | Event::Null => {}
        }
    }

    if depth != 0 || !report_id_seen || !report_id_valid {
        return Err(invalid());
    }
    Ok(())
}

fn valid_https_endpoint(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() > "https://".len()
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
}

fn average(values: impl Iterator<Item = u64>) -> Option<u64> {
    let (count, total) = values.fold((0_u64, 0_u64), |(count, total), value| {
        (count.saturating_add(1), total.saturating_add(value))
    });
    (count > 0).then(|| safe_average(total, count))
}

fn safe_average(total: u64, count: u64) -> u64 {
    total.checked_div(count).unwrap_or_default()
}

fn json_number(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_owned(), |number| number.to_string())
}

fn triplet_value(percentiles: Option<DurationPercentiles>, select: impl FnOnce(DurationPercentiles) -> u64) -> String {
    percentiles.map_or_else(|| "null".to_owned(), |value| select(value).to_string())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        append_record, begin_screen_switch, end_session, flush_metrics_writer, percentiles, recent_sessions,
        record_environment, record_first_frame, record_save_discovery, record_save_read, record_save_write,
        record_screen_switch, record_screen_switch_presented, record_scroll_frames, size_bucket, start_session,
        upload_aggregate_preview, upload_aggregate_preview_with, upload_preview, Duration, SessionSummary, VecDeque,
        MAX_METRICS_BYTES, MAX_SAMPLES, METRICS_FILE, METRICS_UPLOAD_ENDPOINT, ROTATED_METRICS_FILE,
    };
    use crate::diagnostics::{configure_log_directory, LOG_DIRECTORY_TEST_GATE};
    use std::fs;
    use std::io::Write;
    use std::time::SystemTime;

    struct FakePost {
        status: u16,
        final_url: String,
        response_body: Vec<u8>,
        calls: usize,
        posted_url: String,
        content_type: String,
        request_body: Vec<u8>,
    }

    impl FakePost {
        fn new(status: u16, final_url: &str) -> Self {
            Self {
                status,
                final_url: final_url.to_owned(),
                response_body: Vec::new(),
                calls: 0,
                posted_url: String::new(),
                content_type: String::new(),
                request_body: Vec::new(),
            }
        }

        fn with_response_body(mut self, body: &str) -> Self {
            self.response_body = body.as_bytes().to_vec();
            self
        }
    }

    impl sse_sys::fetch::Fetch for FakePost {
        fn get(
            &mut self,
            _url: &str,
            _range_from: u64,
            _sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> sse_core::Result<sse_sys::fetch::Response> {
            Err(sse_core::Error::Refused("unexpected GET".to_owned()))
        }

        fn post(
            &mut self,
            url: &str,
            content_type: &str,
            body: &[u8],
            sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> sse_core::Result<sse_sys::fetch::Response> {
            self.calls = self.calls.saturating_add(1);
            self.posted_url = url.to_owned();
            self.content_type = content_type.to_owned();
            self.request_body = body.to_vec();
            if !sink(&self.response_body) {
                return Err(sse_core::Error::Refused("response body rejected".to_owned()));
            }
            Ok(sse_sys::fetch::Response {
                status: self.status,
                content_length: Some(u64::try_from(self.response_body.len()).unwrap_or(u64::MAX)),
                content_range: None,
                final_url: self.final_url.clone(),
            })
        }
    }

    #[test]
    fn save_sizes_are_grouped_without_file_identity() {
        assert_eq!(size_bucket(0), "0-64-KiB");
        assert_eq!(size_bucket(65_536), "64-256-KiB");
        assert_eq!(size_bucket(262_144), "256-KiB-1-MiB");
        assert_eq!(size_bucket(1_048_576), "1-4-MiB");
        assert_eq!(size_bucket(4_194_304), "4-16-MiB");
        assert_eq!(size_bucket(16_777_216), "16-MiB+");
    }

    #[test]
    fn percentiles_are_stable_for_even_and_odd_samples() {
        assert_eq!(
            percentiles(&[9, 1, 5]).map(|p| (p.p50_ms, p.p95_ms, p.max_ms)),
            Some((5, 9, 9))
        );
        assert_eq!(
            percentiles(&[10, 20, 30, 40]).map(|p| (p.p50_ms, p.p95_ms, p.max_ms)),
            Some((25, 40, 40))
        );
        assert_eq!(percentiles(&[]), None);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn aggregate_upload_is_separately_consented() {
        assert!(METRICS_UPLOAD_ENDPOINT.ends_with("/metrics"));
        assert!(upload_preview().contains("\"schema\":1"));
    }

    #[test]
    fn aggregate_upload_sends_the_exact_consented_preview_once() {
        let preview = "{\"schema\":1,\"sessions\":0}";
        let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");

        upload_aggregate_preview_with(&mut fetch, METRICS_UPLOAD_ENDPOINT, true, preview)
            .expect("accepted aggregate upload");

        assert_eq!(fetch.calls, 1);
        assert_eq!(fetch.posted_url, METRICS_UPLOAD_ENDPOINT);
        assert_eq!(fetch.content_type, "application/json");
        assert_eq!(fetch.request_body, preview.as_bytes());
    }

    #[test]
    fn aggregate_upload_refuses_missing_consent_without_network_access() {
        let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
        let result = upload_aggregate_preview_with(
            &mut fetch,
            METRICS_UPLOAD_ENDPOINT,
            false,
            "{\"schema\":1,\"sessions\":0}",
        );
        assert!(result.is_err());
        assert_eq!(fetch.calls, 0);
        assert!(upload_aggregate_preview(false, "{\"schema\":1,\"sessions\":0}").is_err());
    }

    #[test]
    fn aggregate_upload_rejects_redirects_and_rate_limits_without_retrying() {
        let preview = "{\"schema\":1,\"sessions\":0}";
        let mut redirected =
            FakePost::new(201, "https://example.invalid/metrics").with_response_body("{\"report_id\":\"r-123\"}");
        assert!(upload_aggregate_preview_with(&mut redirected, METRICS_UPLOAD_ENDPOINT, true, preview,).is_err());
        assert_eq!(redirected.calls, 1);

        let mut rate_limited =
            FakePost::new(429, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
        assert!(upload_aggregate_preview_with(&mut rate_limited, METRICS_UPLOAD_ENDPOINT, true, preview,).is_err());
        assert_eq!(rate_limited.calls, 1);
    }

    #[test]
    fn aggregate_upload_rejects_non_https_endpoints_and_oversized_previews() {
        let preview = "{\"schema\":1,\"sessions\":0}";
        let mut fetch =
            FakePost::new(201, "http://example.invalid/metrics").with_response_body("{\"report_id\":\"r-123\"}");
        assert!(upload_aggregate_preview_with(&mut fetch, "http://example.invalid/metrics", true, preview,).is_err());
        assert_eq!(fetch.calls, 0);

        let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
        let oversized = "x".repeat(16 * 1024 + 1);
        assert!(upload_aggregate_preview_with(&mut fetch, METRICS_UPLOAD_ENDPOINT, true, &oversized).is_err());
        assert_eq!(fetch.calls, 0);
    }

    #[test]
    fn aggregate_upload_validates_schema_and_session_count_before_network_access() {
        let invalid_previews = [
            "[]",
            "{\"schema\":1}",
            "{\"sessions\":0}",
            "{\"schema\":2,\"sessions\":0}",
            "{\"schema\":1.0,\"sessions\":0}",
            "{\"schema\":1,\"sessions\":-1}",
            "{\"schema\":1,\"sessions\":1001}",
            "{\"schema\":1,\"sessions\":1.0}",
            "{\"schema\":1,\"sessions\":true}",
            "{\"schema\":1,\"schema\":1,\"sessions\":0}",
            "{\"schema\":1,\"sessions\":0,\"sessions\":0}",
            "{\"nested\":{\"schema\":1,\"sessions\":0}}",
        ];

        for preview in invalid_previews {
            let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
            assert!(
                upload_aggregate_preview_with(&mut fetch, METRICS_UPLOAD_ENDPOINT, true, preview).is_err(),
                "accepted invalid metrics payload: {preview}"
            );
            assert_eq!(fetch.calls, 0, "sent invalid metrics payload: {preview}");
        }
    }

    #[test]
    fn aggregate_upload_accepts_exactly_sixteen_kibibytes() {
        let prefix = "{\"schema\":1,\"sessions\":1000,\"padding\":\"";
        let suffix = "\"}";
        let padding = "x".repeat(16 * 1024 - prefix.len() - suffix.len());
        let preview = format!("{prefix}{padding}{suffix}");
        assert_eq!(preview.len(), 16 * 1024);

        let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
        upload_aggregate_preview_with(&mut fetch, METRICS_UPLOAD_ENDPOINT, true, &preview)
            .expect("accept a valid payload at the size limit");
        assert_eq!(fetch.calls, 1);
    }

    #[test]
    fn aggregate_upload_accepts_only_http_201_and_never_retries_terminal_statuses() {
        for status in [200, 202, 400, 405, 413, 415, 429, 503] {
            let mut fetch =
                FakePost::new(status, METRICS_UPLOAD_ENDPOINT).with_response_body("{\"report_id\":\"r-123\"}");
            assert!(
                upload_aggregate_preview_with(
                    &mut fetch,
                    METRICS_UPLOAD_ENDPOINT,
                    true,
                    "{\"schema\":1,\"sessions\":0}",
                )
                .is_err(),
                "accepted unexpected HTTP status {status}"
            );
            assert_eq!(fetch.calls, 1, "retried HTTP status {status}");
        }
    }

    #[test]
    fn aggregate_upload_requires_a_valid_report_id_response() {
        for response_body in [
            "",
            "not-json",
            "{}",
            "{\"report_id\":\"\"}",
            "{\"report_id\":4}",
            "{\"report_id\":\"one\",\"report_id\":\"two\"}",
        ] {
            let mut fetch = FakePost::new(201, METRICS_UPLOAD_ENDPOINT).with_response_body(response_body);
            assert!(
                upload_aggregate_preview_with(
                    &mut fetch,
                    METRICS_UPLOAD_ENDPOINT,
                    true,
                    "{\"schema\":1,\"sessions\":0}",
                )
                .is_err(),
                "accepted invalid response body: {response_body}"
            );
            assert_eq!(fetch.calls, 1);
        }
    }

    #[test]
    fn local_metrics_are_redacted_summarized_and_previewed_without_sending() {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-metrics-test-{stamp}"));
        fs::create_dir_all(&directory).expect("create temporary metrics directory");
        configure_log_directory(Some(directory.clone()));

        start_session();
        record_first_frame();
        record_screen_switch(Duration::from_millis(7));
        record_screen_switch(Duration::from_millis(3));
        record_scroll_frames(&[Duration::from_millis(11), Duration::from_millis(14)]);
        record_save_read("stalker-cop", 131_072, Duration::from_millis(12));
        record_save_read("/home/private/person/save.sav", 131_072, Duration::from_millis(13));
        record_save_write("stalker-cop", 131_072, Duration::from_millis(9));
        record_save_discovery(Duration::from_millis(42));
        record_environment(1280, 800, 1.25);
        flush_metrics_writer();
        let metrics_path = directory.join(METRICS_FILE);
        let current_bytes = fs::metadata(&metrics_path)
            .expect("metrics writer flushed startup events")
            .len();
        let padding_bytes = MAX_METRICS_BYTES.saturating_sub(current_bytes);
        let mut padding = vec![b'x'; usize::try_from(padding_bytes).expect("metrics limit fits usize")];
        let mut current_file = fs::OpenOptions::new()
            .append(true)
            .open(metrics_path)
            .expect("open metrics file");
        current_file.write_all(&padding).expect("force metrics rotation");
        padding.clear();
        record_scroll_frames(&[Duration::from_millis(15)]);
        end_session();

        let metrics = format!(
            "{}{}",
            fs::read_to_string(directory.join(ROTATED_METRICS_FILE)).expect("read rotated metrics file"),
            fs::read_to_string(directory.join(METRICS_FILE)).expect("read metrics file")
        );
        assert!(!metrics.contains("/home/private"));
        assert!(!metrics.contains("person"));
        assert!(!metrics.contains("save.sav"));
        assert!(metrics.contains("\"format\":\"unknown\""));

        let summaries = recent_sessions();
        assert_eq!(summaries.len(), 1);
        let summary = summaries.first().expect("one recorded session remains in the summary");
        assert!(summary.ended);
        assert_eq!(summary.screen_switch.map(|value| value.p50_ms), Some(5));
        assert_eq!(summary.scrolling_frame.map(|value| value.max_ms), Some(15));
        assert_eq!(summary.save_discovery_ms, Some(42));
        assert_eq!(summary.resolution, Some((1280, 800)));
        assert_eq!(summary.scale_percent, Some(125));
        if cfg!(target_os = "linux") {
            assert!(summary.peak_memory_bytes.is_some());
        }
        assert_eq!(summary.save_operations.len(), 3);

        let preview = upload_preview();
        let mut reader = sse_codecs::json::Reader::new(preview.as_bytes());
        while reader.next_event().expect("valid aggregate JSON").is_some() {}
        assert!(preview.contains("\"format\":\"stalker-cop\""));
        assert!(preview.contains("\"format\":\"unknown\""));
        assert!(!preview.contains("/home/private"));
        assert!(!preview.contains("save.sav"));

        configure_log_directory(None);
        fs::remove_dir_all(directory).expect("remove temporary metrics directory");
    }

    #[test]
    fn bounded_sample_window_discards_oldest_without_shifting_the_rest() {
        let mut samples = VecDeque::new();
        for value in 0..=u64::try_from(MAX_SAMPLES).expect("sample limit fits u64") {
            SessionSummary::add_sample(&mut samples, value);
        }
        assert_eq!(samples.len(), MAX_SAMPLES);
        assert_eq!(samples.front(), Some(&1));
        assert_eq!(
            samples.back(),
            Some(&u64::try_from(MAX_SAMPLES).expect("sample limit fits u64"))
        );
    }

    #[test]
    fn screen_switch_measurement_finishes_after_a_presented_frame() {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-metrics-screen-switch-{stamp}"));
        fs::create_dir_all(&directory).expect("create temporary metrics directory");
        configure_log_directory(Some(directory.clone()));

        start_session();
        begin_screen_switch();
        std::thread::sleep(Duration::from_millis(5));
        record_screen_switch_presented();
        end_session();

        let summary = recent_sessions().into_iter().next().expect("session recorded");
        assert!(summary.screen_switch.is_some_and(|samples| samples.max_ms >= 1));
        configure_log_directory(None);
        fs::remove_dir_all(directory).expect("remove temporary metrics directory");
    }

    #[test]
    fn metrics_file_rotates_at_one_mibibyte() {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-metrics-rotation-{stamp}"));
        fs::create_dir_all(&directory).expect("create temporary metrics directory");
        configure_log_directory(Some(directory.clone()));
        fs::write(
            directory.join(METRICS_FILE),
            vec![b'x'; usize::try_from(MAX_METRICS_BYTES).expect("metrics limit fits usize")],
        )
        .expect("write oversized metrics file");

        append_record("{\"event\":\"session_start\",\"os\":\"linux\"}");
        flush_metrics_writer();

        assert_eq!(
            fs::metadata(directory.join(ROTATED_METRICS_FILE))
                .expect("rotated file exists")
                .len(),
            MAX_METRICS_BYTES
        );
        assert!(
            fs::metadata(directory.join(METRICS_FILE))
                .expect("new metrics file exists")
                .len()
                < 100
        );
        configure_log_directory(None);
        fs::remove_dir_all(directory).expect("remove temporary metrics directory");
    }

    #[test]
    fn failed_rotation_never_appends_past_the_one_mibibyte_limit() {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stamp = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("sse-metrics-rotation-error-{stamp}"));
        fs::create_dir_all(&directory).expect("create temporary metrics directory");
        configure_log_directory(Some(directory.clone()));
        let current_path = directory.join(METRICS_FILE);
        fs::write(
            &current_path,
            vec![b'x'; usize::try_from(MAX_METRICS_BYTES.saturating_sub(8)).expect("file limit fits usize")],
        )
        .expect("write metrics file near the limit");
        let rotated_path = directory.join(ROTATED_METRICS_FILE);
        fs::create_dir(&rotated_path).expect("create an unremovable rotation target");
        fs::write(rotated_path.join("blocker"), b"keep").expect("block rotation target removal");

        append_record("{\"event\":\"screen_switch\",\"ms\":9}");
        flush_metrics_writer();

        assert_eq!(
            fs::metadata(current_path).expect("current log remains").len(),
            MAX_METRICS_BYTES - 8
        );
        configure_log_directory(None);
        fs::remove_dir_all(directory).expect("remove temporary metrics directory");
    }
}
