//! Local diagnostics: rotating redacted logs, crash markers, and user-approved report exports.

use crate::paths::default_data_directory;
use sse_codecs::{
    crc32::crc32,
    deflate::{compress_raw, Level},
    zip::{self, Entry},
};
use sse_core::{Error, Result};
use sse_sys::fetch::{Fetch, SystemFetch};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE: &str = "save-editor.log";
const CRASH_FILE: &str = "last-crash.txt";
const AUTOMATIC_REPORT_FILE: &str = "automatic-error-report.txt";
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_CRASH_BYTES: usize = 64 * 1024;
const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;
const MAX_AUTOMATIC_REPORT_BYTES: usize = 2 * 1024 * 1024;
const MAX_AUTOMATIC_REPORT_CHARS: usize = 8_000;
const MAX_REPORT_RESPONSE_BYTES: usize = 16 * 1024;
const DEFAULT_REPORT_ENDPOINT: &str = "https://save-editor-downloads.save-editor.workers.dev/diagnostics";
const PART_BYTES: usize = 256 * 1024;
const ROTATIONS: usize = 3;
const MAX_DIAGNOSTIC_GAME_LOG_BYTES: usize = 1024 * 1024;
const MAX_DIAGNOSTIC_GAME_LOG_FILES: usize = 64;

static LOG_GATE: Mutex<()> = Mutex::new(());
static LOG_DIRECTORY: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();
#[cfg(test)]
pub(crate) static LOG_DIRECTORY_TEST_GATE: Mutex<()> = Mutex::new(());

/// Returns the directory containing the application log and crash marker.
#[must_use]
pub fn log_directory() -> PathBuf {
    let configured = LOG_DIRECTORY
        .get_or_init(|| Mutex::new(None))
        .lock()
        .ok()
        .and_then(|value| value.clone());
    configured.unwrap_or_else(|| default_data_directory().join("logs"))
}

/// Overrides the diagnostics directory. Passing None restores the normal application path.
pub fn configure_log_directory(directory: Option<PathBuf>) {
    if let Ok(mut configured) = LOG_DIRECTORY.get_or_init(|| Mutex::new(None)).lock() {
        *configured = directory;
    }
}

/// Writes one redacted informational line. Logging errors never escape to the caller.
pub fn info(message: &str) {
    write_log("INFO", message);
}

/// Writes one redacted warning line. Logging errors never escape to the caller.
pub fn warn(message: &str) {
    write_log("WARN", message);
}

/// Writes one redacted error line. Logging errors never escape to the caller.
pub fn error(message: &str) {
    write_log("ERROR", message);
}

/// Records a save write that completed and passed its durable read-back verification.
pub fn save_write_succeeded(path: &Path) {
    info(&format!("save write succeeded: {}", path.display()));
}

/// Records a save write that failed, including the operation path and error.
pub fn save_write_failed(path: &Path, reason: &str) {
    error(&format!("save write failed: {}: {reason}", path.display()));
}

/// Records a save write cancelled before the operation completed.
pub fn save_write_cancelled(path: &Path) {
    warn(&format!("save write cancelled: {}", path.display()));
}

fn write_log(level: &str, message: &str) {
    let Ok(_guard) = LOG_GATE.lock() else { return };
    let directory = log_directory();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let path = directory.join(LOG_FILE);
    let line = format!("{} {level} {}\n", timestamp(), redact(message));
    let line_bytes = u64::try_from(line.len()).unwrap_or(u64::MAX);
    if fs::metadata(&path)
        .ok()
        .is_some_and(|metadata| metadata.len().saturating_add(line_bytes) > MAX_LOG_BYTES)
    {
        rotate(&path);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}

fn rotate(path: &Path) {
    for index in (1..ROTATIONS).rev() {
        let source = PathBuf::from(format!("{}.{}", path.display(), index));
        let destination = PathBuf::from(format!("{}.{}", path.display(), index.saturating_add(1)));
        if source.exists() {
            let _ = fs::rename(source, destination);
        }
    }
    if path.exists() {
        let _ = fs::rename(path, PathBuf::from(format!("{}.1", path.display())));
    }
}

fn timestamp() -> String {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format_timestamp_utc(duration)
}

fn format_timestamp_utc(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    let mut remaining_days = seconds / 86_400;
    let seconds_in_day = seconds % 86_400;
    let mut year = 1970_u64;
    loop {
        if year > 9999 {
            // Keep ordinary log dates in the four-digit ISO form; preserve out-of-range epoch values.
            return format!("unix+{seconds}s.{:03}Z", duration.subsec_millis());
        }
        let leap = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
        let year_days = if leap { 366 } else { 365 };
        if remaining_days < year_days {
            break;
        }
        let Some(days_after_year) = remaining_days.checked_sub(year_days) else {
            return "invalid-time".to_owned();
        };
        let Some(next_year) = year.checked_add(1) else {
            return "invalid-time".to_owned();
        };
        remaining_days = days_after_year;
        year = next_year;
    }

    let leap = year % 400 == 0 || (year % 4 == 0 && year % 100 != 0);
    let mut month = 1_u64;
    loop {
        let month_days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 if leap => 29,
            2 => 28,
            _ => return "invalid-time".to_owned(),
        };
        if remaining_days < month_days {
            break;
        }
        let Some(days_after_month) = remaining_days.checked_sub(month_days) else {
            return "invalid-time".to_owned();
        };
        let Some(next_month) = month.checked_add(1) else {
            return "invalid-time".to_owned();
        };
        remaining_days = days_after_month;
        month = next_month;
    }
    let Some(day) = remaining_days.checked_add(1) else {
        return "invalid-time".to_owned();
    };

    let hour = seconds_in_day / 3_600;
    let minute = seconds_in_day % 3_600 / 60;
    let second = seconds_in_day % 60;
    format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{:03}Z",
        duration.subsec_millis()
    )
}

/// Redacts home-directory user names, Wine user names, SteamID64 values and Steam userdata IDs.
#[must_use]
pub fn redact(text: &str) -> String {
    let mut value = text.to_owned();
    for home in [std::env::var("HOME").ok(), std::env::var("USERPROFILE").ok()]
        .into_iter()
        .flatten()
        .filter(|home| home.len() > 1)
    {
        value = replace_ascii_case_insensitive(&value, &home, "<home>");
    }
    value = redact_after_marker(&value, "/var/home/", "/var/<home>");
    value = redact_after_marker(&value, "/home/", "<home>");
    value = redact_after_marker(&value, "/Users/", "<home>");
    value = redact_windows_user_paths(&value);
    value = redact_after_marker(&value, "/media/", "/media/<user>");
    value = redact_after_marker(&value, "drive_c/users/", "drive_c/users/<user>");
    value = redact_after_marker(&value, "drive_c\\users\\", "drive_c\\users\\<user>");
    value = redact_userdata(&value);
    value = redact_steam_ids(&value);
    redact_unc_paths(&value)
}

fn redact_windows_user_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        let root_separator = index.saturating_add(2);
        let users_start = index.saturating_add(3);
        let user_separator = index.saturating_add(8);
        let is_user_root = bytes.get(index).is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(index.saturating_add(1)) == Some(&b':')
            && bytes
                .get(root_separator)
                .is_some_and(|byte| matches!(*byte, b'/' | b'\\'))
            && ascii_bytes_match(bytes, users_start, b"Users")
            && bytes
                .get(user_separator)
                .is_some_and(|byte| matches!(*byte, b'/' | b'\\'));
        if !is_user_root {
            index = index.saturating_add(1);
            continue;
        }
        if let Some(prefix) = text.get(cursor..index) {
            out.push_str(prefix);
        }
        out.push_str("<home>");
        let mut end = user_separator.saturating_add(1);
        while let Some(byte) = bytes.get(end).copied() {
            if matches!(byte, b'/' | b'\\' | b'\n' | b'\r' | b'"' | b'\'' | b';') {
                break;
            }
            end = end.saturating_add(1);
        }
        cursor = end;
        index = end;
    }
    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }
    out
}

fn replace_ascii_case_insensitive(text: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return text.to_owned();
    }
    let lower_text = text.to_ascii_lowercase();
    let lower_needle = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while let Some(relative) = lower_text.get(cursor..).and_then(|tail| tail.find(&lower_needle)) {
        let start = cursor.saturating_add(relative);
        let end = start.saturating_add(needle.len());
        if let Some(prefix) = text.get(cursor..start) {
            out.push_str(prefix);
        }
        out.push_str(replacement);
        cursor = end;
    }
    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }
    out
}

fn redact_after_marker(text: &str, marker: &str, replacement: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let marker_lower = marker.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    while let Some(relative) = lower.get(cursor..).and_then(|tail| tail.find(&marker_lower)) {
        let start = cursor.saturating_add(relative);
        if let Some(prefix) = text.get(cursor..start) {
            out.push_str(prefix);
        }
        out.push_str(replacement);
        let mut end = start.saturating_add(marker.len());
        while let Some(byte) = text.as_bytes().get(end).copied() {
            if byte == b'/' || byte == b'\\' {
                break;
            }
            if byte == b'\n' || byte == b'\r' || byte == b'"' || byte == b'\'' || byte == b';' {
                break;
            }
            end = end.saturating_add(1);
        }
        cursor = end;
    }
    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }
    out
}

fn redact_unc_paths(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        let next = index.saturating_add(1);
        let is_unc_separator = matches!(bytes.get(index), Some(b'/' | b'\\')) && bytes.get(index) == bytes.get(next);
        let starts_at_boundary = index == 0
            || bytes
                .get(index.saturating_sub(1))
                .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'=' | b'(' | b'[' | b'"' | b'\''));
        if is_unc_separator && starts_at_boundary {
            if let Some(prefix) = text.get(cursor..index) {
                out.push_str(prefix);
            }
            out.push_str("<path>");
            let mut end = index;
            while let Some(byte) = bytes.get(end).copied() {
                if byte == b'\n' || byte == b'\r' || byte == b';' || byte == b'"' || byte == b'\'' {
                    break;
                }
                end = end.saturating_add(1);
            }
            cursor = end;
            index = end;
        } else {
            index = index.saturating_add(1);
        }
    }
    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }
    out
}

fn redact_userdata(text: &str) -> String {
    let mut value = text.to_owned();
    for marker in ["userdata/", "userdata\\"] {
        let lower = value.to_ascii_lowercase();
        let mut out = String::with_capacity(value.len());
        let mut cursor = 0_usize;
        while let Some(relative) = lower.get(cursor..).and_then(|tail| tail.find(marker)) {
            let start = cursor.saturating_add(relative);
            let digits = start.saturating_add(marker.len());
            let mut end = digits;
            while value.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                end = end.saturating_add(1);
            }
            if end == digits {
                cursor = digits;
                continue;
            }
            if let Some(prefix) = value.get(cursor..start) {
                out.push_str(prefix);
            }
            out.push_str("userdata/<id>");
            cursor = end;
        }
        if let Some(tail) = value.get(cursor..) {
            out.push_str(tail);
        }
        value = out;
    }
    value
}

fn redact_steam_ids(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0_usize;
    let mut index = 0_usize;
    while index < bytes.len() {
        let steam_id_end = steam_id2_end(bytes, index).or_else(|| steam_id3_end(bytes, index));
        if let Some(end) = steam_id_end {
            if let Some(prefix) = text.get(cursor..index) {
                out.push_str(prefix);
            }
            out.push_str("<steamid>");
            cursor = end;
            index = end;
            continue;
        }
        if bytes.get(index).is_some_and(u8::is_ascii_digit) {
            let start = index;
            while bytes.get(index).is_some_and(u8::is_ascii_digit) {
                index = index.saturating_add(1);
            }
            let token = text.get(start..index).unwrap_or("");
            if token.len() == 17 && token.starts_with("7656119") {
                if let Some(prefix) = text.get(cursor..start) {
                    out.push_str(prefix);
                }
                out.push_str("<steamid>");
                cursor = index;
            }
        } else {
            index = index.saturating_add(1);
        }
    }
    if let Some(tail) = text.get(cursor..) {
        out.push_str(tail);
    }
    out
}

fn steam_id2_end(bytes: &[u8], start: usize) -> Option<usize> {
    if !ascii_bytes_match(bytes, start, b"STEAM_") {
        return None;
    }
    let mut end = start.checked_add(b"STEAM_".len())?;
    if !scan_ascii_digits(bytes, &mut end) || bytes.get(end) != Some(&b':') {
        return None;
    }
    end = end.saturating_add(1);
    if !scan_ascii_digits(bytes, &mut end) || bytes.get(end) != Some(&b':') {
        return None;
    }
    end = end.saturating_add(1);
    scan_ascii_digits(bytes, &mut end).then_some(end)
}

fn steam_id3_end(bytes: &[u8], start: usize) -> Option<usize> {
    if bytes.get(start) != Some(&b'[') || !ascii_bytes_match(bytes, start.saturating_add(1), b"U:") {
        return None;
    }
    let mut end = start.checked_add(3)?;
    if !scan_ascii_digits(bytes, &mut end) || bytes.get(end) != Some(&b':') {
        return None;
    }
    end = end.saturating_add(1);
    if !scan_ascii_digits(bytes, &mut end) || bytes.get(end) != Some(&b']') {
        return None;
    }
    end.checked_add(1)
}

fn ascii_bytes_match(bytes: &[u8], start: usize, expected: &[u8]) -> bool {
    let Some(end) = start.checked_add(expected.len()) else {
        return false;
    };
    bytes.get(start..end).is_some_and(|actual| {
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| actual.eq_ignore_ascii_case(expected))
    })
}

fn scan_ascii_digits(bytes: &[u8], index: &mut usize) -> bool {
    let start = *index;
    while bytes.get(*index).is_some_and(u8::is_ascii_digit) {
        *index = index.saturating_add(1);
    }
    *index > start
}

/// Installs a panic hook that records a redacted crash marker before delegating to the previous hook.
pub fn install_crash_reporter() {
    let _ = PANIC_HOOK.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic| {
            let location = panic.location().map_or_else(
                || "unknown location".to_owned(),
                |location| format!("{}:{}", location.file(), location.line()),
            );
            let payload = panic
                .payload()
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| panic.payload().downcast_ref::<String>().map(String::as_str))
                .unwrap_or("non-string panic");
            let backtrace = std::backtrace::Backtrace::force_capture();
            record_crash(
                "Unhandled panic",
                &format!("{location}: {payload}\nBacktrace:\n{backtrace}"),
            );
            previous(panic);
        }));
    });
}

/// Records a redacted crash marker capped at 64 KiB and also appends it to the application log.
pub fn record_crash(context: &str, detail: &str) {
    let text = redact(&format!("{} {context}\n{detail}\n", timestamp()));
    error(&format!("{context}: {detail}"));
    let directory = log_directory();
    if fs::create_dir_all(&directory).is_err() {
        return;
    }
    let bytes = text.as_bytes();
    let kept = bytes.get(..bytes.len().min(MAX_CRASH_BYTES)).unwrap_or(bytes);
    let _ = fs::write(directory.join(CRASH_FILE), kept);
}

/// Returns the previous-run crash marker, if present.
#[must_use]
pub fn pending_crash() -> Option<String> {
    fs::read_to_string(log_directory().join(CRASH_FILE)).ok()
}

/// Removes the previous-run crash marker.
pub fn dismiss_crash() {
    let _ = fs::remove_file(log_directory().join(CRASH_FILE));
}

/// Builds the exact automatic error-report payload shown to the user before consent.
///
/// The payload contains only application version, OS/architecture, the supplied error,
/// a redacted stack trace, and a redacted tail of the application log. Path-like tokens
/// are removed after normal diagnostics redaction. Save contents and game logs are never read.
#[must_use]
pub fn automatic_error_report(error_text: &str, stack: &str) -> String {
    let log = fs::read_to_string(log_directory().join(LOG_FILE)).unwrap_or_default();
    let log = tail_utf8(&log, 16 * 1024);
    let report = format!(
        "Version: {}\nOS: {} {}\nError:\n{}\n\nStack:\n{}\n\nApplication log:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        redact_paths(&redact(error_text)),
        redact_paths(&redact(stack)),
        redact_paths(&redact(log)),
    );
    truncate_chars(&report, MAX_AUTOMATIC_REPORT_CHARS).to_owned()
}

fn truncate_chars(value: &str, maximum: usize) -> &str {
    value
        .char_indices()
        .nth(maximum)
        .map_or(value, |(index, _)| value.get(..index).unwrap_or(value))
}

/// Builds an automatic report from the previous-run crash marker.
#[must_use]
pub fn pending_automatic_error_report() -> Option<String> {
    pending_crash().map(|crash| automatic_error_report(&crash, "captured in crash marker"))
}

/// Saves the user-approved automatic report locally.
///
/// The report is written to the application data directory before any upload is attempted.
///
/// # Errors
/// Returns an error when the local report cannot be written.
pub fn save_automatic_error_report(report: &str) -> Result<PathBuf> {
    let directory = log_directory();
    fs::create_dir_all(&directory)?;
    let path = directory.join(AUTOMATIC_REPORT_FILE);
    // Atomic replace: an interrupted write must not leave a truncated report to be shown as pending.
    sse_sys::secure_fs::atomic_write(
        &path,
        redact_paths(&redact(report)).as_bytes(),
        sse_sys::secure_fs::AtomicWriteOptions::create_or_replace(),
    )?;
    Ok(path)
}

/// HTTPS receiver configured at compile time, or the existing diagnostics Worker by default.
#[must_use]
pub fn automatic_report_endpoint() -> &'static str {
    option_env!("SSE_REPORT_ENDPOINT")
        .filter(|value| valid_https_endpoint(value))
        .unwrap_or(DEFAULT_REPORT_ENDPOINT)
}

/// Sends a user-approved automatic error report as a bounded gzip POST.
///
/// The caller must first save the plain-text report locally and obtain the user's explicit
/// confirmation in the UI. The system transport accepts only HTTPS and never follows redirects.
///
/// # Errors
/// Returns an error for compression or transport failures, a non-201 response, an invalid
/// response body, or a missing/invalid report identifier.
pub fn upload_automatic_error_report(report: &str) -> Result<String> {
    let endpoint = automatic_report_endpoint();
    let mut fetch = SystemFetch {
        max_bytes: u64::try_from(MAX_REPORT_RESPONSE_BYTES).unwrap_or(u64::MAX),
        ..SystemFetch::default()
    };
    upload_automatic_error_report_with(&mut fetch, endpoint, report)
}

fn upload_automatic_error_report_with(fetch: &mut dyn Fetch, endpoint: &str, report: &str) -> Result<String> {
    if !valid_https_endpoint(endpoint) {
        return Err(Error::Refused("only HTTPS report endpoints are allowed".to_owned()));
    }
    let body = automatic_report_gzip(report)?;
    let mut response_body = Vec::new();
    let response = fetch.post(endpoint, "application/gzip", &body, &mut |chunk| {
        let Some(next_len) = response_body.len().checked_add(chunk.len()) else {
            return false;
        };
        if next_len > MAX_REPORT_RESPONSE_BYTES {
            return false;
        }
        response_body.extend_from_slice(chunk);
        true
    })?;
    if response.final_url != endpoint || !valid_https_endpoint(&response.final_url) {
        return Err(Error::Refused("report endpoint redirected".to_owned()));
    }
    match response.status {
        201 => parse_report_id(&response_body),
        429 => Err(Error::Refused(
            "report service rate limit reached (HTTP 429)".to_owned(),
        )),
        status => Err(Error::System(format!("report service returned HTTP {status}"))),
    }
}

fn valid_https_endpoint(value: &str) -> bool {
    value.starts_with("https://")
        && value.len() > "https://".len()
        && value
            .bytes()
            .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
}

fn automatic_report_gzip(report: &str) -> Result<Vec<u8>> {
    let sanitized = redact_paths(&redact(report));
    if sanitized.len() > MAX_AUTOMATIC_REPORT_BYTES {
        return Err(Error::Refused("automatic report exceeds size limit".to_owned()));
    }
    let raw = sanitized.as_bytes();
    let compressed = compress_raw(raw, Level::Default)?;
    let total = compressed
        .len()
        .checked_add(18)
        .ok_or_else(|| Error::Refused("automatic report size overflow".to_owned()))?;
    if total > MAX_AUTOMATIC_REPORT_BYTES {
        return Err(Error::Refused("compressed report exceeds 2 MiB".to_owned()));
    }
    let mut gzip = Vec::with_capacity(total);
    gzip.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255]);
    gzip.extend_from_slice(&compressed);
    gzip.extend_from_slice(&crc32(raw).to_le_bytes());
    gzip.extend_from_slice(&u32::try_from(raw.len()).unwrap_or(u32::MAX).to_le_bytes());
    Ok(gzip)
}

fn parse_report_id(payload: &[u8]) -> Result<String> {
    use sse_codecs::json::{Event, Reader};

    let mut reader = Reader::new(payload);
    if !matches!(reader.next_event()?, Some(Event::ObjectStart)) {
        return Err(Error::damaged("report service response is not a JSON object"));
    }
    let mut report_id: Option<String> = None;
    loop {
        match reader
            .next_event()?
            .ok_or_else(|| Error::damaged("truncated report service response"))?
        {
            Event::ObjectEnd => break,
            Event::Key(key) => {
                let value = reader
                    .next_event()?
                    .ok_or_else(|| Error::damaged("missing report service field value"))?;
                if key.as_str() == "report_id" {
                    if report_id.is_some() {
                        return Err(Error::damaged("duplicate report_id in service response"));
                    }
                    let Event::String(value) = value else {
                        return Err(Error::damaged("report_id is not a string"));
                    };
                    report_id = Some(value.into_owned());
                } else {
                    skip_json_value(&mut reader, value)?;
                }
            }
            _ => return Err(Error::damaged("unexpected report service response field")),
        }
    }
    if reader.next_event()?.is_some() {
        return Err(Error::damaged("trailing data in report service response"));
    }
    let report_id = report_id.ok_or_else(|| Error::damaged("report service omitted report_id"))?;
    if report_id.is_empty()
        || report_id.len() > 128
        || !report_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(Error::damaged("invalid report_id from service"));
    }
    Ok(report_id)
}

fn skip_json_value(reader: &mut sse_codecs::json::Reader<'_>, first: sse_codecs::json::Event<'_>) -> Result<()> {
    use sse_codecs::json::Event;

    let mut depth = usize::from(matches!(first, Event::ObjectStart | Event::ArrayStart));
    while depth > 0 {
        match reader
            .next_event()?
            .ok_or_else(|| Error::damaged("truncated report service JSON value"))?
        {
            Event::ObjectStart | Event::ArrayStart => {
                depth = depth
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("report service JSON nesting overflow"))?;
            }
            Event::ObjectEnd | Event::ArrayEnd => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::damaged("invalid report service JSON nesting"))?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn redact_paths(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut cursor = 0_usize;
    while let Some(start) = next_path_start(bytes, cursor) {
        if let Some(prefix) = text.get(cursor..start) {
            output.push_str(prefix);
        }
        output.push_str("<path>");
        let end = path_end(bytes, start);
        cursor = end.max(start.saturating_add(1));
    }
    if let Some(tail) = text.get(cursor..) {
        output.push_str(tail);
    }
    output
}

fn next_path_start(bytes: &[u8], from: usize) -> Option<usize> {
    for index in from..bytes.len() {
        let drive_path = bytes.get(index).is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(index.saturating_add(1)) == Some(&b':')
            && bytes
                .get(index.saturating_add(2))
                .is_some_and(|byte| matches!(*byte, b'/' | b'\\'));
        if drive_path {
            return Some(index);
        }
        if matches!(bytes.get(index), Some(b'/' | b'\\')) {
            let mut start = index;
            while start > from {
                let previous = bytes.get(start.saturating_sub(1)).copied().unwrap_or_default();
                if previous.is_ascii_whitespace()
                    || matches!(previous, b'=' | b'(' | b'[' | b'"' | b'\'' | b';' | b',' | b':')
                {
                    break;
                }
                start = start.saturating_sub(1);
            }
            return Some(start);
        }
    }
    None
}

fn path_end(bytes: &[u8], start: usize) -> usize {
    let mut end = start;
    while let Some(byte) = bytes.get(end).copied() {
        if matches!(byte, b'\n' | b'\r' | b';' | b'"' | b'\'' | b')' | b']' | b'}') {
            break;
        }
        if byte.is_ascii_whitespace() && has_filename_extension(bytes, end) {
            break;
        }
        if byte == b':'
            && end > start.saturating_add(2)
            && bytes.get(end.saturating_add(1)).is_some_and(u8::is_ascii_whitespace)
        {
            break;
        }
        end = end.saturating_add(1);
    }
    end
}

fn has_filename_extension(bytes: &[u8], end: usize) -> bool {
    let tail_start = end.saturating_sub(10);
    let tail = bytes.get(tail_start..end).unwrap_or_default();
    let Some(dot) = tail.iter().rposition(|byte| *byte == b'.') else {
        return false;
    };
    let extension = tail.get(dot.saturating_add(1)..).unwrap_or_default();
    const KNOWN_EXTENSIONS: [&[u8]; 33] = [
        b"sav", b"sav2", b"scop", b"dat", b"txt", b"log", b"json", b"cfg", b"ini", b"db", b"zip", b"dmp", b"mdmp",
        b"exe", b"dll", b"bin", b"bak", b"tmp", b"toml", b"xml", b"yaml", b"yml", b"lua", b"script", b"png", b"jpg",
        b"jpeg", b"svg", b"acf", b"vdf", b"rs", b"sh", b"lock",
    ];
    KNOWN_EXTENSIONS
        .iter()
        .any(|known| extension.eq_ignore_ascii_case(known))
}

/// Creates a redacted gzip diagnostics bundle without sending it anywhere.
///
/// # Errors
/// Returns an error if DEFLATE compression fails.
pub fn diagnostics_bundle(environment_report: Option<&str>) -> Result<Vec<u8>> {
    let mut text = String::new();
    append_part(
        &mut text,
        "",
        &format!(
            "S.T.A.L.K.E.R. Save Editor {}, {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        MAX_BUNDLE_BYTES,
    );
    if let Some(crash) = pending_crash() {
        append_part(&mut text, "crash", &redact(&crash), PART_BYTES);
    }
    if let Some(report) = environment_report.filter(|report| !report.trim().is_empty()) {
        append_part(&mut text, "environment", &redact(report), PART_BYTES);
    }
    let directory = log_directory();
    for name in [LOG_FILE, "save-editor.log.1", "save-editor.log.2", "save-editor.log.3"] {
        if text.len() >= MAX_BUNDLE_BYTES {
            break;
        }
        if let Ok(log) = fs::read_to_string(directory.join(name)) {
            append_part(&mut text, name, &redact(&log), MAX_BUNDLE_BYTES);
        }
    }
    let raw = text.as_bytes();
    let compressed = compress_raw(raw, Level::Default)?;
    let mut gzip = Vec::with_capacity(compressed.len().saturating_add(18));
    gzip.extend_from_slice(&[0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 255]);
    gzip.extend_from_slice(&compressed);
    gzip.extend_from_slice(&crc32(raw).to_le_bytes());
    let size = u32::try_from(raw.len()).unwrap_or(u32::MAX);
    gzip.extend_from_slice(&size.to_le_bytes());
    Ok(gzip)
}

fn append_part(output: &mut String, title: &str, value: &str, part_limit: usize) {
    if value.is_empty() || output.len() >= MAX_BUNDLE_BYTES {
        return;
    }
    let header = if title.is_empty() {
        String::new()
    } else {
        format!("--- {title} ---\n")
    };
    let available = MAX_BUNDLE_BYTES
        .saturating_sub(output.len())
        .saturating_sub(header.len())
        .saturating_sub(1)
        .min(part_limit);
    if available == 0 {
        return;
    }
    output.push_str(&header);
    output.push_str(tail_utf8(value, available));
    output.push('\n');
}

fn tail_utf8(value: &str, maximum_bytes: usize) -> &str {
    if value.len() <= maximum_bytes {
        return value;
    }
    let mut start = value.len().saturating_sub(maximum_bytes);
    while start < value.len() && !value.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    value.get(start..).unwrap_or("")
}

/// Returns a small local-only environment report suitable for an exported diagnostics bundle.
#[must_use]
pub fn environment_report() -> String {
    format!(
        "Version: {}\nOS: {}\nArchitecture: {}\nData directory: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        redact(&default_data_directory().to_string_lossy())
    )
}

/// One discovered game installation to include in a local diagnostics report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticGame {
    /// Display name of the game.
    pub title: String,
    /// Installation directory; the report includes only its redacted form.
    pub install_directory: PathBuf,
    /// Whether this is S.T.A.L.K.E.R. 2, whose log paths are under `Saved`.
    pub is_stalker2: bool,
}

/// Builds a local ZIP report containing application diagnostics and discovered game paths.
/// Game logs and crash files are read only when `include_game_logs` is true.
///
/// # Errors
/// Returns an error if the ZIP cannot be encoded.
pub fn diagnostics_zip(games: &[DiagnosticGame], include_game_logs: bool) -> Result<Vec<u8>> {
    diagnostics_zip_at(&log_directory(), games, include_game_logs)
}

/// Writes a local diagnostics ZIP to the selected path. This function never sends report data.
///
/// # Errors
/// Returns an error if the archive cannot be built or written.
pub fn save_diagnostics_zip(path: &Path, games: &[DiagnosticGame], include_game_logs: bool) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::Refused("diagnostics path is empty".to_owned()));
    }
    let archive = diagnostics_zip(games, include_game_logs)?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, archive)?;
    Ok(())
}

fn diagnostics_zip_at(log_directory: &Path, games: &[DiagnosticGame], include_game_logs: bool) -> Result<Vec<u8>> {
    let mut entries = vec![Entry {
        name: "report/environment.txt".to_owned(),
        data: redact(&environment_report()).into_bytes(),
    }];
    let mut game_report = String::from("Found game installations:\n");
    let mut ordered_games: Vec<&DiagnosticGame> = games.iter().collect();
    ordered_games.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.install_directory.cmp(&right.install_directory))
    });
    for game in &ordered_games {
        game_report.push_str(&single_line(&redact(&game.title)));
        game_report.push('\t');
        game_report.push_str(&single_line(&redact(&game.install_directory.to_string_lossy())));
        game_report.push('\n');
    }
    entries.push(Entry {
        name: "report/games.txt".to_owned(),
        data: game_report.into_bytes(),
    });

    for name in [
        CRASH_FILE,
        LOG_FILE,
        "save-editor.log.1",
        "save-editor.log.2",
        "save-editor.log.3",
        "metrics.jsonl",
        "metrics.jsonl.1",
    ] {
        if let Some(data) = read_diagnostic_file(&log_directory.join(name), log_directory, PART_BYTES) {
            let archive_name = if name == CRASH_FILE {
                "application/last-crash.txt".to_owned()
            } else {
                format!("application/{name}")
            };
            entries.push(Entry {
                name: archive_name,
                data,
            });
        }
    }

    if include_game_logs {
        let mut total_game_bytes = 0_usize;
        let mut file_count = 0_usize;
        'games: for (game_index, game) in ordered_games.iter().enumerate() {
            for (archive_name, path) in game_log_paths(game, game_index) {
                if file_count >= MAX_DIAGNOSTIC_GAME_LOG_FILES || total_game_bytes >= MAX_DIAGNOSTIC_GAME_LOG_BYTES {
                    break 'games;
                }
                let remaining = MAX_DIAGNOSTIC_GAME_LOG_BYTES.saturating_sub(total_game_bytes);
                let maximum = remaining.min(PART_BYTES);
                let Some(data) = read_diagnostic_file(&path, &game.install_directory, maximum) else {
                    continue;
                };
                if data.is_empty() {
                    continue;
                }
                total_game_bytes = total_game_bytes.saturating_add(data.len());
                file_count = file_count.saturating_add(1);
                entries.push(Entry {
                    name: archive_name,
                    data,
                });
            }
        }
    }

    zip::write(&entries, Level::Default)
}

fn game_log_paths(game: &DiagnosticGame, game_index: usize) -> Vec<(String, PathBuf)> {
    let Ok(root) = fs::canonicalize(&game.install_directory) else {
        return Vec::new();
    };
    let locations: &[(&str, &str)] = if game.is_stalker2 {
        &[("Saved/Logs", "logs"), ("Saved/Crashes", "crashes")]
    } else {
        &[("appdata/logs", "xray-logs"), ("logs", "xray-logs")]
    };
    let mut found = Vec::new();
    for (relative, category) in locations {
        let directory = game.install_directory.join(relative);
        let Ok(metadata) = fs::symlink_metadata(&directory) else {
            continue;
        };
        if !metadata.file_type().is_dir() {
            continue;
        }
        let Ok(directory) = fs::canonicalize(directory) else {
            continue;
        };
        if !directory.starts_with(&root) {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| {
                let file_type = entry.file_type().ok()?;
                if file_type.is_file() {
                    Some(entry.path())
                } else {
                    None
                }
            })
            .collect();
        files.sort();
        for path in files {
            let Some(name) = path.file_name().map(|name| name.to_string_lossy().into_owned()) else {
                continue;
            };
            if !game.is_stalker2 && !is_xray_log_name(&name) {
                continue;
            }
            if game.is_stalker2 && !is_stalker2_log_name(&name, category) {
                continue;
            }
            found.push((
                format!("game-logs/{game_index:02}/{category}/{}", safe_archive_segment(&name)),
                path,
            ));
        }
    }
    found
}

fn is_xray_log_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("xray_") && lower.ends_with(".log")
}

fn is_stalker2_log_name(name: &str, category: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if category == "crashes" {
        matches!(lower.rsplit('.').next(), Some("log" | "txt" | "dmp" | "mdmp" | "crash"))
    } else {
        matches!(lower.rsplit('.').next(), Some("log" | "txt"))
    }
}

fn safe_archive_segment(value: &str) -> String {
    let segment: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if segment.is_empty() || segment == "." || segment == ".." {
        "log.bin".to_owned()
    } else {
        segment
    }
}

fn read_diagnostic_file(path: &Path, root: &Path, maximum: usize) -> Option<Vec<u8>> {
    if maximum == 0 {
        return None;
    }
    let root = fs::canonicalize(root).ok()?;
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.file_type().is_file() {
        return None;
    }
    let path = fs::canonicalize(path).ok()?;
    if !path.starts_with(&root) {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let maximum_u64 = u64::try_from(maximum).ok()?;
    file.seek(SeekFrom::Start(length.saturating_sub(maximum_u64))).ok()?;
    let mut data = Vec::with_capacity(usize::try_from(length.min(maximum_u64)).ok()?);
    file.take(maximum_u64).read_to_end(&mut data).ok()?;
    // Binary files (minidumps, raw memory) can carry paths and secrets the redaction cannot see, so they
    // are left out of the archive. A NUL byte marks binary; X-Ray logs are Windows-1251 text and are kept.
    if data.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(&data) {
        Ok(text) => text.to_owned(),
        Err(_) => decode_windows_1251_text(&data),
    };
    let redacted = redact(&text);
    Some(tail_utf8(&redacted, maximum).as_bytes().to_vec())
}

/// Windows-1251 code points for bytes 0x80..=0xFF (the same table `sse-content` uses for X-Ray text).
const CP1251_HIGH: [u16; 128] = [
    1026, 1027, 8218, 1107, 8222, 8230, 8224, 8225, 8364, 8240, 1033, 8249, 1034, 1036, 1035, 1039, 1106, 8216, 8217,
    8220, 8221, 8226, 8211, 8212, 65533, 8482, 1113, 8250, 1114, 1116, 1115, 1119, 160, 1038, 1118, 1032, 164, 1168,
    166, 167, 1025, 169, 1028, 171, 172, 173, 174, 1031, 176, 177, 1030, 1110, 1169, 181, 182, 183, 1105, 8470, 1108,
    187, 1112, 1029, 1109, 1111, 1040, 1041, 1042, 1043, 1044, 1045, 1046, 1047, 1048, 1049, 1050, 1051, 1052, 1053,
    1054, 1055, 1056, 1057, 1058, 1059, 1060, 1061, 1062, 1063, 1064, 1065, 1066, 1067, 1068, 1069, 1070, 1071, 1072,
    1073, 1074, 1075, 1076, 1077, 1078, 1079, 1080, 1081, 1082, 1083, 1084, 1085, 1086, 1087, 1088, 1089, 1090, 1091,
    1092, 1093, 1094, 1095, 1096, 1097, 1098, 1099, 1100, 1101, 1102, 1103,
];

fn decode_windows_1251_text(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&byte| {
            if byte < 0x80 {
                char::from(byte)
            } else {
                let code = CP1251_HIGH
                    .get(usize::from(byte.saturating_sub(0x80)))
                    .copied()
                    .unwrap_or(65533);
                char::from_u32(u32::from(code)).unwrap_or('\u{FFFD}')
            }
        })
        .collect()
}

fn single_line(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if matches!(character, '\r' | '\n' | '\t') {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sse_codecs::inflate::inflate_raw;
    use sse_sys::fetch::Response;

    #[test]
    fn windows_1251_xray_log_is_kept_and_transcoded_to_utf8() -> Result<()> {
        let root = std::env::temp_dir().join(format!("sse-diag-cp1251-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        let path = root.join("xray_1.log");
        // "Ошибка" in Windows-1251 followed by ASCII; not valid UTF-8.
        fs::write(&path, [0xce_u8, 0xf8, 0xe8, 0xe1, 0xea, 0xe0, b' ', b'x', b'\n'])?;

        let kept = read_diagnostic_file(&path, &root, PART_BYTES);
        let _ = fs::remove_dir_all(&root);
        let text = String::from_utf8(kept.unwrap_or_default()).unwrap_or_default();
        assert!(text.contains("Ошибка x"), "{text:?}");
        Ok(())
    }

    #[test]
    fn binary_game_dumps_are_left_out_of_the_diagnostics_archive() -> Result<()> {
        let root = std::env::temp_dir().join(format!("sse-diag-binary-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let game_logs = root.join("game/Saved/Crashes");
        fs::create_dir_all(&game_logs)?;
        fs::write(
            game_logs.join("crash.dmp"),
            [0x4d_u8, 0x44, 0x4d, 0x50, 0xff, 0xfe, 0x00, 0x80],
        )?;
        fs::write(game_logs.join("crash.log"), b"Error: C:\\Users\\alice\\x\n")?;
        let games = vec![DiagnosticGame {
            title: "S.T.A.L.K.E.R. 2".to_owned(),
            install_directory: root.join("game"),
            is_stalker2: true,
        }];
        let archive = diagnostics_zip_at(&root.join("logs"), &games, true);
        let _ = fs::remove_dir_all(&root);
        let bytes = archive?;
        let names = String::from_utf8_lossy(&bytes);
        assert!(!names.contains("crash.dmp"), "binary dump must not be archived");
        Ok(())
    }

    #[test]
    fn automatic_report_is_saved_atomically_with_no_leftover_file() -> Result<()> {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
        let directory = std::env::temp_dir().join(format!("sse-report-atomic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        configure_log_directory(Some(directory.clone()));

        let first = save_automatic_error_report("Error: first report");
        let first_text = first.and_then(|path| Ok(fs::read_to_string(path)?));
        let second = save_automatic_error_report("Error: second report");
        let second_text = second.and_then(|path| Ok(fs::read_to_string(path)?));
        configure_log_directory(None);
        let names: Vec<String> = fs::read_dir(&directory)?
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned()))
            .collect();
        let _ = fs::remove_dir_all(&directory);
        assert!(first_text?.contains("first report"));
        assert!(second_text?.contains("second report"));
        assert_eq!(names, vec![AUTOMATIC_REPORT_FILE.to_owned()]);
        Ok(())
    }

    #[test]
    fn diagnostic_timestamps_use_readable_utc_calendar_time() {
        assert_eq!(
            format_timestamp_utc(std::time::Duration::from_millis(0)),
            "1970-01-01T00:00:00.000Z"
        );
        assert_eq!(
            format_timestamp_utc(std::time::Duration::from_millis(951_827_696_789)),
            "2000-02-29T12:34:56.789Z"
        );
    }

    #[test]
    fn save_write_outcomes_are_logged_with_readable_time_and_redacted_paths() -> Result<()> {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
        let directory = std::env::temp_dir().join(format!("sse-save-write-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        configure_log_directory(Some(directory.clone()));

        save_write_succeeded(Path::new("/home/alice/saves/working.sav"));
        save_write_failed(Path::new("/home/alice/saves/broken.sav"), "permission denied");

        let log = fs::read_to_string(directory.join(LOG_FILE));
        configure_log_directory(None);
        let _ = fs::remove_dir_all(&directory);
        let log = log?;
        let lines: Vec<_> = log.lines().collect();
        assert_eq!(lines.len(), 2, "{log:?}");
        let success = lines.first().copied().unwrap_or_default();
        let failure = lines.get(1).copied().unwrap_or_default();
        assert!(
            success.contains(" INFO save write succeeded: <home>/saves/working.sav"),
            "{log:?}"
        );
        assert!(
            failure.contains(" ERROR save write failed: <home>/saves/broken.sav: permission denied"),
            "{log:?}"
        );
        for line in lines {
            let Some((timestamp, _)) = line.split_once(' ') else {
                return Err(Error::System(format!("log line has no timestamp: {line:?}")));
            };
            let valid_timestamp = timestamp.len() == 24
                && timestamp.bytes().enumerate().all(|(index, byte)| match index {
                    4 | 7 => byte == b'-',
                    10 => byte == b'T',
                    13 | 16 => byte == b':',
                    19 => byte == b'.',
                    23 => byte == b'Z',
                    _ => byte.is_ascii_digit(),
                });
            assert!(valid_timestamp, "{timestamp:?}");
            assert!(!line.contains("alice"), "{line:?}");
        }
        Ok(())
    }

    struct FakeResponder {
        status: u16,
        response: Vec<u8>,
        request_body: Vec<u8>,
        request_url: String,
        content_type: String,
    }

    impl Fetch for FakeResponder {
        fn get(&mut self, _url: &str, _range_from: u64, _sink: &mut dyn FnMut(&[u8]) -> bool) -> Result<Response> {
            Err(Error::Refused("fake responder only accepts POST".to_owned()))
        }

        fn post(
            &mut self,
            url: &str,
            content_type: &str,
            body: &[u8],
            sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> Result<Response> {
            self.request_url = url.to_owned();
            self.content_type = content_type.to_owned();
            self.request_body.extend_from_slice(body);
            if !sink(&self.response) {
                return Err(Error::Refused("fake response rejected by sink".to_owned()));
            }
            Ok(Response {
                status: self.status,
                content_length: Some(u64::try_from(self.response.len()).unwrap_or(u64::MAX)),
                content_range: None,
                final_url: url.to_owned(),
            })
        }
    }

    #[test]
    fn redacts_home_steam_and_wine_identifiers() {
        let source = "/home/alice/game C:\\Users\\Alice\\save drive_c/users/steamuser/AppData userdata/123456/remote 76561198012345678";
        let redacted = redact(source);
        assert!(!redacted.contains("alice"));
        assert!(!redacted.contains("Alice"));
        assert!(!redacted.contains("steamuser"));
        assert!(!redacted.contains("123456"));
        assert!(!redacted.contains("76561198012345678"));
        assert!(redacted.contains("<home>"));
        assert!(redacted.contains("<steamid>"));
        assert!(redacted.contains("userdata/<id>"));
    }

    #[test]
    fn redacts_steam_id_text_formats() {
        let source = "steam2=STEAM_0:1:123456 steam3=[U:1:7654321]";
        let redacted = redact(source);

        assert!(
            !redacted.contains("STEAM_0:1:123456"),
            "leaked Steam2 ID in {redacted:?}"
        );
        assert!(!redacted.contains("[U:1:7654321]"), "leaked Steam3 ID in {redacted:?}");
        assert_eq!(redacted.matches("<steamid>").count(), 2, "{redacted:?}");
    }

    #[test]
    fn redacts_user_components_with_spaces() {
        let source =
            r#"windows=C:\Users\Alice Example\Documents\save.sav; media=/media/Bob Example/Saves folder/save.sav"#;
        let redacted = redact(source);

        for private_fragment in ["Alice", "Example", "Bob"] {
            assert!(
                !redacted.contains(private_fragment),
                "leaked {private_fragment:?} in {redacted:?}"
            );
        }
    }

    #[test]
    fn preserves_absolute_var_home_path_shape_without_leaking_user() {
        let redacted = redact("/var/home/Alice Example/saves/quicksave.sav");

        assert_eq!(redacted, "/var/<home>/saves/quicksave.sav");
    }

    #[test]
    fn redacts_run_media_and_non_c_drive_user_components() {
        let source = r"mounted=/run/media/Alice Smith/Games/slot.sav; drive=D:\Users\Carol Example\Documents\save.sav";
        let redacted = redact(source);

        for private_fragment in ["Alice", "Smith", "Carol", "Example"] {
            assert!(
                !redacted.contains(private_fragment),
                "leaked {private_fragment:?} in {redacted:?}"
            );
        }
    }

    #[test]
    fn redacts_unc_paths_with_spaced_components() {
        let source = r#"unc=\\archive01\Profiles\Carol Example\Documents\save.sav; status=failed"#;
        let redacted = redact(source);

        assert!(!redacted.contains("archive01"), "leaked UNC host in {redacted:?}");
        assert!(!redacted.contains("Carol Example"), "leaked UNC user in {redacted:?}");
    }

    #[test]
    fn automatic_report_endpoint_defaults_to_the_https_worker() {
        assert_eq!(automatic_report_endpoint(), DEFAULT_REPORT_ENDPOINT);
    }

    #[test]
    fn automatic_report_is_bounded_to_the_preview_length() {
        let long_error = "я".repeat(9_000);
        let report = automatic_error_report(&long_error, "");

        assert!(
            report.chars().count() <= 8_000,
            "report length: {}",
            report.chars().count()
        );
    }

    #[test]
    fn automatic_report_redacts_path_components_after_spaces() {
        let source = r"failed D:\Users\Alice Example Folder\OneDrive - Private Company\Save With Spaces.sav; code=1";
        let report = redact_paths(source);

        for private_fragment in [
            "Alice",
            "Example",
            "Folder",
            "OneDrive",
            "Private",
            "Company",
            "Save",
            "Spaces.sav",
        ] {
            assert!(
                !report.contains(private_fragment),
                "leaked {private_fragment:?} in {report:?}"
            );
        }
        assert!(report.contains("<path>; code=1"), "{report:?}");
    }

    #[test]
    fn automatic_report_does_not_leak_after_a_dotted_directory_name() {
        let report = redact_paths("failed /home/Alice/OneDrive 2.0 Company; code=1");

        assert!(!report.contains("Company"), "leaked folder name in {report:?}");
        assert!(report.contains("<path>; code=1"), "{report:?}");
    }

    #[test]
    fn warning_lines_are_written_with_redacted_paths() -> Result<()> {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
        let directory = std::env::temp_dir().join(format!("sse-warning-log-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        configure_log_directory(Some(directory.clone()));

        let result = {
            warn("failed to read /home/alice/steamapps/appmanifest_123.acf: permission denied");
            fs::read_to_string(directory.join(LOG_FILE))
        };
        configure_log_directory(None);
        let _ = fs::remove_dir_all(&directory);
        let log = result?;
        assert!(log.contains(" WARN "));
        assert!(log.contains("<home>/steamapps/appmanifest_123.acf"));
        assert!(!log.contains("alice"));
        Ok(())
    }

    #[test]
    fn automatic_report_contains_only_redacted_diagnostics() -> Result<()> {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
        let directory = std::env::temp_dir().join(format!("sse-auto-report-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        configure_log_directory(Some(directory.clone()));
        info("opened /home/alice/secret.sav for 76561198012345678");
        let report = automatic_error_report(
            "failed C:\\Users\\Alice\\save.sav",
            "frame at /Users/alice/project/src/main.rs:10",
        );
        assert!(report.contains("Version:"));
        assert!(report.contains("OS:"));
        assert!(report.contains("<path>"));
        assert!(report.contains("<steamid>"));
        assert!(!report.contains("Alice"));
        assert!(!report.contains("alice"));
        assert!(!report.contains(".sav"));
        let saved = save_automatic_error_report(&report)?;
        assert!(saved.exists());
        configure_log_directory(None);
        let _ = fs::remove_dir_all(directory);
        Ok(())
    }

    #[test]
    fn report_upload_uses_gzip_and_reads_report_id_from_fake_responder() -> Result<()> {
        let url = "https://example.test/diagnostics";
        let mut fake = FakeResponder {
            status: 201,
            response: br#"{"meta":{"accepted":true},"report_id":"diag_123"}"#.to_vec(),
            request_body: Vec::new(),
            request_url: String::new(),
            content_type: String::new(),
        };

        let report_id = upload_automatic_error_report_with(&mut fake, url, "diagnostic message")?;

        assert_eq!(report_id, "diag_123");
        assert_eq!(fake.request_url, url);
        assert_eq!(fake.content_type, "application/gzip");
        assert_eq!(fake.request_body.get(..3), Some(&[0x1f, 0x8b, 8][..]));
        let deflate_end = fake.request_body.len().saturating_sub(8);
        let raw_size = fake
            .request_body
            .get(deflate_end.saturating_add(4)..)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
            .unwrap_or_default();
        let payload = inflate_raw(
            fake.request_body.get(10..deflate_end).unwrap_or_default(),
            usize::try_from(raw_size).unwrap_or_default(),
        )?;
        assert_eq!(payload, b"diagnostic message");
        assert!(
            u64::try_from(fake.request_body.len()).unwrap_or(u64::MAX)
                <= u64::try_from(MAX_AUTOMATIC_REPORT_BYTES).unwrap_or(u64::MAX)
        );
        Ok(())
    }

    #[test]
    fn report_upload_surfaces_worker_rate_limit_without_following_redirects() {
        let url = "https://example.test/diagnostics";
        let mut fake = FakeResponder {
            status: 429,
            response: Vec::new(),
            request_body: Vec::new(),
            request_url: String::new(),
            content_type: String::new(),
        };

        let result = upload_automatic_error_report_with(&mut fake, url, "diagnostic message");

        assert!(matches!(result, Err(Error::Refused(message)) if message.contains("429")));
        assert_eq!(fake.request_url, url);
    }

    #[test]
    fn bundle_is_valid_gzip_with_redacted_payload() -> Result<()> {
        let _guard = LOG_DIRECTORY_TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
        let directory = std::env::temp_dir().join(format!("sse-diagnostics-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        configure_log_directory(Some(directory.clone()));
        info("home /home/alice/secret.sav and steam 76561198012345678");
        record_crash("test", "/Users/alice/crash");
        let gzip = diagnostics_bundle(Some("C:\\Users\\Alice\\environment"))?;
        assert_eq!(gzip.get(..3), Some(&[0x1f, 0x8b, 8][..]));
        let deflate_end = gzip.len().saturating_sub(8);
        let raw_size = gzip
            .get(deflate_end.saturating_add(4)..)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
            .unwrap_or_default();
        let payload = inflate_raw(
            gzip.get(10..deflate_end).unwrap_or_default(),
            usize::try_from(raw_size).unwrap_or_default(),
        )?;
        let text = String::from_utf8_lossy(&payload);
        assert!(text.contains("<home>"));
        assert!(text.contains("<steamid>"), "{text}");
        assert!(!text.contains("alice"));
        configure_log_directory(None);
        let _ = fs::remove_dir_all(directory);
        Ok(())
    }

    #[test]
    fn diagnostic_zip_redacts_metadata_and_omits_game_logs_without_consent() -> Result<()> {
        let root = std::env::temp_dir().join(format!("sse-diagnostic-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let app_logs = root.join("app-logs");
        let xray = root.join("games").join("xray");
        let stalker2 = root.join("games").join("stalker2");
        fs::create_dir_all(app_logs.join("game"))?;
        fs::create_dir_all(xray.join("appdata").join("logs"))?;
        fs::create_dir_all(stalker2.join("Saved").join("Logs"))?;
        fs::create_dir_all(stalker2.join("Saved").join("Crashes"))?;
        fs::write(
            app_logs.join("save-editor.log"),
            "startup C:\\Users\\Alice\\Documents\\secret.sav\n",
        )?;
        fs::write(
            app_logs.join("metrics.jsonl"),
            "{\"event\":\"save_read\",\"format\":\"stalker-cop\",\"size_bucket\":\"64-256-KiB\",\"ms\":12}\n",
        )?;
        fs::write(
            app_logs.join("metrics.jsonl.1"),
            "{\"event\":\"first_frame\",\"ms\":91}\n",
        )?;
        fs::write(
            xray.join("appdata").join("logs").join("xray_1.log"),
            "xray diagnostic\n",
        )?;
        fs::write(
            stalker2.join("Saved").join("Logs").join("Stalker2.log"),
            "game diagnostic\n",
        )?;
        fs::write(
            stalker2.join("Saved").join("Crashes").join("crash.txt"),
            "crash diagnostic\n",
        )?;
        let games = [
            DiagnosticGame {
                title: "Shadow of Chernobyl".to_owned(),
                install_directory: xray,
                is_stalker2: false,
            },
            DiagnosticGame {
                title: "Heart of Chornobyl".to_owned(),
                install_directory: stalker2,
                is_stalker2: true,
            },
            DiagnosticGame {
                title: "Clear Sky".to_owned(),
                install_directory: PathBuf::from(r"C:\Users\Alice\Games\Clear Sky"),
                is_stalker2: false,
            },
        ];

        let archive = diagnostics_zip_at(&app_logs, &games, false)?;
        let entries = sse_codecs::zip::read(&archive, 4 * 1024 * 1024)?;
        let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();
        assert!(names.contains(&"report/environment.txt"));
        assert!(names.contains(&"report/games.txt"));
        assert!(names.contains(&"application/save-editor.log"));
        assert!(names.contains(&"application/metrics.jsonl"));
        assert!(names.contains(&"application/metrics.jsonl.1"));
        assert!(!names.iter().any(|name| name.starts_with("game-logs/")));
        let environment = entries
            .iter()
            .find(|entry| entry.name == "report/environment.txt")
            .map(|entry| String::from_utf8_lossy(&entry.data))
            .ok_or_else(|| Error::damaged("environment report missing from diagnostic ZIP"))?;
        assert!(environment.contains(&format!("Version: {}", env!("CARGO_PKG_VERSION"))));
        let application_log = entries
            .iter()
            .find(|entry| entry.name == "application/save-editor.log")
            .map(|entry| String::from_utf8_lossy(&entry.data))
            .ok_or_else(|| Error::damaged("application log missing from diagnostic ZIP"))?;
        assert!(application_log.contains("<home>"));
        assert!(!application_log.contains("Alice"));
        let game_list = entries
            .iter()
            .find(|entry| entry.name == "report/games.txt")
            .map(|entry| String::from_utf8_lossy(&entry.data))
            .ok_or_else(|| Error::damaged("game list missing from diagnostic ZIP"))?;
        assert!(game_list.contains("<home>"));
        assert!(!game_list.contains("Alice"));
        let _ = fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn diagnostic_zip_adds_bounded_xray_and_stalker2_logs_after_consent() -> Result<()> {
        let root = std::env::temp_dir().join(format!("sse-diagnostic-logs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let app_logs = root.join("app-logs");
        let xray = root.join("xray");
        let stalker2 = root.join("stalker2");
        fs::create_dir_all(&app_logs)?;
        fs::create_dir_all(xray.join("logs"))?;
        fs::create_dir_all(stalker2.join("Saved").join("Logs"))?;
        fs::create_dir_all(stalker2.join("Saved").join("Crashes"))?;
        fs::write(
            xray.join("logs").join("xray_0.log"),
            "xray report from C:\\Users\\Alice\\mod\n",
        )?;
        fs::write(stalker2.join("Saved").join("Logs").join("game.log"), "S2 game log\n")?;
        fs::write(
            stalker2.join("Saved").join("Crashes").join("crash.dmp"),
            [0_u8, 1, 2, 3],
        )?;
        let games = [
            DiagnosticGame {
                title: "Shadow of Chernobyl".to_owned(),
                install_directory: xray,
                is_stalker2: false,
            },
            DiagnosticGame {
                title: "Heart of Chornobyl".to_owned(),
                install_directory: stalker2,
                is_stalker2: true,
            },
        ];

        let archive = diagnostics_zip_at(&app_logs, &games, true)?;
        let entries = sse_codecs::zip::read(&archive, 4 * 1024 * 1024)?;
        assert!(entries.iter().any(|entry| entry.name.ends_with("/xray_0.log")));
        assert!(entries.iter().any(|entry| entry.name.ends_with("/game.log")));
        // Binary minidumps are never copied raw into the archive (see `read_diagnostic_file`).
        assert!(!entries.iter().any(|entry| entry.name.ends_with("/crash.dmp")));
        let xray_log = entries
            .iter()
            .find(|entry| entry.name.ends_with("/xray_0.log"))
            .map(|entry| String::from_utf8_lossy(&entry.data))
            .ok_or_else(|| Error::damaged("X-Ray log missing from diagnostic ZIP"))?;
        assert!(xray_log.contains("<home>"));
        assert!(!xray_log.contains("Alice"));
        assert!(entries.iter().all(|entry| entry.data.len() <= PART_BYTES));
        let _ = fs::remove_dir_all(root);
        Ok(())
    }
}
