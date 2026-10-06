//! Local diagnostics: rotating redacted log, crash marker, and an exportable gzip bundle.
//!
//! K10 intentionally does not implement report upload. Diagnostics stay on disk until the user
//! explicitly exports the bundle.

use crate::paths::default_data_directory;
use sse_codecs::{
    crc32::crc32,
    deflate::{compress_raw, Level},
    zip::{self, Entry},
};
use sse_core::{Error, Result};
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
const PART_BYTES: usize = 256 * 1024;
const ROTATIONS: usize = 3;
const MAX_DIAGNOSTIC_GAME_LOG_BYTES: usize = 1024 * 1024;
const MAX_DIAGNOSTIC_GAME_LOG_FILES: usize = 64;

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
    format!(
        "Version: {}\nOS: {} {}\nError:\n{}\n\nStack:\n{}\n\nApplication log:\n{}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        redact_paths(&redact(error_text)),
        redact_paths(&redact(stack)),
        redact_paths(&redact(log)),
    )
}

/// Builds an automatic report from the previous-run crash marker.
#[must_use]
pub fn pending_automatic_error_report() -> Option<String> {
    pending_crash().map(|crash| automatic_error_report(&crash, "captured in crash marker"))
}

/// Saves the user-approved automatic report locally.
///
/// The receiver endpoint is intentionally not contacted until the owner configures the HTTPS service.
///
/// # Errors
/// Returns an error when the local report cannot be written.
pub fn save_automatic_error_report(report: &str) -> Result<PathBuf> {
    let directory = log_directory();
    fs::create_dir_all(&directory)?;
    let path = directory.join(AUTOMATIC_REPORT_FILE);
    fs::write(&path, redact_paths(&redact(report)))?;
    Ok(path)
}

/// Compile-time HTTPS receiver configured by the owner, if one is available.
#[must_use]
pub fn automatic_report_endpoint() -> Option<&'static str> {
    option_env!("SSE_REPORT_ENDPOINT").filter(|value| value.starts_with("https://"))
}

fn redact_paths(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    for segment in text.split_inclusive(char::is_whitespace) {
        let token = segment.trim_end_matches(char::is_whitespace);
        let whitespace = segment.get(token.len()..).unwrap_or_default();
        let looks_like_path = token.contains('/') || token.contains('\\') || token.as_bytes().get(1) == Some(&b':');
        if looks_like_path {
            output.push_str("<path>");
        } else {
            output.push_str(token);
        }
        output.push_str(whitespace);
    }
    output
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
    if let Ok(text) = std::str::from_utf8(&data) {
        let redacted = redact(text);
        Some(tail_utf8(&redacted, maximum).as_bytes().to_vec())
    } else {
        Some(data)
    }
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

    static TEST_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    fn warning_lines_are_written_with_redacted_paths() -> Result<()> {
        let _guard = TEST_GATE
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
        let _guard = TEST_GATE
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
    fn bundle_is_valid_gzip_with_redacted_payload() -> Result<()> {
        let _guard = TEST_GATE
            .lock()
            .map_err(|_| Error::System("diagnostics test gate poisoned".to_owned()))?;
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
        assert!(entries.iter().any(|entry| entry.name.ends_with("/crash.dmp")));
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
