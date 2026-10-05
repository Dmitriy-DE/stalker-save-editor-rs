//! Local diagnostics: rotating redacted log, crash marker, and an exportable gzip bundle.
//!
//! K10 intentionally does not implement report upload. Diagnostics stay on disk until the user
//! explicitly exports the bundle.

use crate::paths::default_data_directory;
use sse_codecs::{crc32::crc32, deflate::{compress_raw, Level}};
use sse_core::{Error, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const LOG_FILE: &str = "save-editor.log";
const CRASH_FILE: &str = "last-crash.txt";
const MAX_LOG_BYTES: u64 = 1024 * 1024;
const MAX_CRASH_BYTES: usize = 64 * 1024;
const MAX_BUNDLE_BYTES: usize = 2 * 1024 * 1024;
const PART_BYTES: usize = 256 * 1024;
const ROTATIONS: usize = 3;

static LOG_GATE: Mutex<()> = Mutex::new(());
static LOG_DIRECTORY: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static PANIC_HOOK: OnceLock<()> = OnceLock::new();

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
    format!("{}.{:03}Z", duration.as_secs(), duration.subsec_millis())
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
    value = redact_after_marker(&value, "/home/", "<home>");
    value = redact_after_marker(&value, "/Users/", "<home>");
    value = redact_after_marker(&value, "C:\\Users\\", "<home>");
    value = redact_after_marker(&value, "drive_c/users/", "drive_c/users/<user>");
    value = redact_after_marker(&value, "drive_c\\users\\", "drive_c\\users\\<user>");
    value = redact_userdata(&value);
    redact_steam_ids(&value)
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
            if byte == b'/' || byte == b'\\' || byte.is_ascii_whitespace() {
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
            record_crash("Unhandled panic", &format!("{location}: {payload}"));
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

/// Writes a diagnostics bundle to a user-selected path.
///
/// # Errors
/// Returns an error if bundle creation or file writing fails.
pub fn save_diagnostics_bundle(path: &Path, environment_report: Option<&str>) -> Result<()> {
    if path.as_os_str().is_empty() {
        return Err(Error::Refused("diagnostics path is empty".to_owned()));
    }
    let bundle = diagnostics_bundle(environment_report)?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bundle)?;
    Ok(())
}

/// Returns a small local-only environment report suitable for an exported diagnostics bundle.
#[must_use]
pub fn environment_report() -> String {
    format!(
        "OS: {}\nArchitecture: {}\nData directory: {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH,
        redact(&default_data_directory().to_string_lossy())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sse_codecs::inflate::inflate_raw;

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
    fn bundle_is_valid_gzip_with_redacted_payload() -> Result<()> {
        let directory = std::env::temp_dir().join(format!("sse-diagnostics-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        configure_log_directory(Some(directory.clone()));
        info("home /home/alice and steam 76561198012345678");
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
        assert!(text.contains("<steamid>"));
        assert!(!text.contains("alice"));
        configure_log_directory(None);
        let _ = fs::remove_dir_all(directory);
        Ok(())
    }
}
