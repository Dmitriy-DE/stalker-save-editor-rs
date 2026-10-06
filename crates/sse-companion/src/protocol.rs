//! Synchronous bounded file protocol. Callers should run this client on a worker thread.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Error, ErrorKind, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::MAX_COMPANION_FILE_BYTES;

const COMMAND_FILE: &str = "save_editor_cmd.txt";
const TEMP_COMMAND_FILE: &str = "save_editor_cmd.tmp";
const REPLY_FILE: &str = "save_editor_out.txt";
static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
static SEND_GATES: OnceLock<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

/// A validated companion reply status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyStatus {
    /// Command completed successfully.
    Ok,
    /// Command failed in the game.
    Error,
    /// The installed mod does not support the command.
    Unsupported,
}

impl ReplyStatus {
    /// Stable protocol token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Error => "error",
            Self::Unsupported => "unsupported",
        }
    }
}

/// A single reply line from the game mod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolReply {
    /// Request token echoed by the game.
    pub id: String,
    /// Result status.
    pub status: ReplyStatus,
    /// Remaining response text, if present.
    pub text: String,
}

/// Protocol I/O error.
#[derive(Debug)]
pub enum ProtocolError {
    /// A different command file is already pending.
    CommandPending,
    /// The request timed out or a reply was invalid.
    Invalid(String),
    /// Filesystem operation failed.
    Io(std::io::Error),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandPending => formatter.write_str("a companion command is already pending"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ProtocolError {}
impl From<std::io::Error> for ProtocolError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// One sender per exchange directory; concurrent client instances share their gate.
#[derive(Debug, Clone)]
pub struct CompanionClient {
    directory: PathBuf,
    send_gate: Arc<Mutex<()>>,
}

impl CompanionClient {
    /// Creates a client for a known mod exchange directory.
    #[must_use]
    pub fn new(directory: PathBuf) -> Self {
        let absolute = if directory.is_absolute() {
            directory
        } else {
            std::env::current_dir().map_or(directory.clone(), |cwd| cwd.join(directory))
        };
        let registry = SEND_GATES.get_or_init(|| Mutex::new(HashMap::new()));
        let gate = registry
            .lock()
            .ok()
            .map(|mut gates| {
                if let Some(existing) = gates.get(&absolute).and_then(Weak::upgrade) {
                    return existing;
                }
                let created = Arc::new(Mutex::new(()));
                let _ = gates.insert(absolute.clone(), Arc::downgrade(&created));
                created
            })
            .unwrap_or_else(|| Arc::new(Mutex::new(())));
        Self {
            directory: absolute,
            send_gate: gate,
        }
    }

    /// Sends a checked protocol v1 command and waits only for a reply bearing its fresh ID.
    pub fn send(&self, command: &str, arguments: &[&str], timeout: Duration) -> Result<ProtocolReply, ProtocolError> {
        if timeout.is_zero() {
            return Err(ProtocolError::Invalid("protocol timeout must be positive".to_owned()));
        }
        let formatted = format_command(command, arguments)?;
        let _gate = self
            .send_gate
            .lock()
            .map_err(|_| ProtocolError::Invalid("companion send gate was poisoned".to_owned()))?;
        fs::create_dir_all(&self.directory)?;
        let command_path = self.directory.join(COMMAND_FILE);
        let temporary_path = self.directory.join(TEMP_COMMAND_FILE);
        let reply_path = self.directory.join(REPLY_FILE);
        if command_path.exists() || temporary_path.exists() {
            return Err(ProtocolError::CommandPending);
        }
        let id = fresh_id();
        let line = format!("v1 {id} {formatted}\n");
        let mut temporary = OpenOptions::new().write(true).create_new(true).open(&temporary_path)?;
        if let Err(error) = temporary.write_all(line.as_bytes()).and_then(|()| temporary.sync_all()) {
            drop(temporary);
            let _ = fs::remove_file(&temporary_path);
            return Err(ProtocolError::Io(error));
        }
        drop(temporary);
        if let Err(error) = fs::rename(&temporary_path, &command_path) {
            let _ = fs::remove_file(&temporary_path);
            return Err(ProtocolError::Io(error));
        }

        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or_else(|| ProtocolError::Invalid("protocol timeout is too large".to_owned()))?;
        loop {
            if reply_path.exists() {
                if let Ok(bytes) = read_reply_bytes(&reply_path) {
                    let text = sse_content::decode_windows_1251(&bytes);
                    if reply_id(&text).as_deref() == Some(id.as_str()) {
                        let parsed = parse_reply(&text);
                        remove_owned_command(&command_path, line.as_bytes());
                        return parsed;
                    }
                }
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                remove_owned_command(&command_path, line.as_bytes());
                return Err(ProtocolError::Invalid(format!(
                    "companion protocol timed out for request {id}"
                )));
            }
            std::thread::sleep(remaining.min(Duration::from_millis(50)));
        }
    }

    fn send_ok(&self, command: &str, arguments: &[&str], timeout: Duration) -> Result<String, ProtocolError> {
        let reply = self.send(command, arguments, timeout)?;
        if reply.status == ReplyStatus::Ok {
            Ok(reply.text)
        } else {
            Err(ProtocolError::Invalid(format!(
                "{command} failed: {} {}",
                reply.status.as_str(),
                reply.text
            )))
        }
    }

    /// Pings the running game and returns the measured round-trip latency.
    pub fn ping(&self, timeout: Duration) -> Result<(Duration, String), ProtocolError> {
        let started = Instant::now();
        let text = self.send_ok("ping", &[], timeout)?;
        Ok((started.elapsed(), text))
    }

    /// Reads the player's live info payload.
    pub fn info(&self, timeout: Duration) -> Result<String, ProtocolError> {
        self.send_ok("info", &[], timeout)
    }

    /// Reads the live inventory payload.
    pub fn list_inventory(&self, timeout: Duration) -> Result<String, ProtocolError> {
        self.send_ok("list_inventory", &[], timeout)
    }

    /// Gives an item through the installed Companion protocol.
    pub fn give(&self, section: &str, count: u8, timeout: Duration) -> Result<String, ProtocolError> {
        let count = count.to_string();
        self.send_ok("give", &[section, count.as_str()], timeout)
    }

    /// Enables or disables the Companion side of global hotkeys.
    pub fn hotkeys(&self, enabled: bool, timeout: Duration) -> Result<String, ProtocolError> {
        self.send_ok("hotkeys", &[if enabled { "on" } else { "off" }], timeout)
    }

    /// Sends an experimental S.T.A.L.K.E.R. 2 god-mode command.
    pub fn s2_god(&self, enabled: bool, timeout: Duration) -> Result<String, ProtocolError> {
        self.send_ok("god", &[if enabled { "on" } else { "off" }], timeout)
    }

    /// Sends an experimental S.T.A.L.K.E.R. 2 noclip command.
    pub fn s2_noclip(&self, enabled: bool, timeout: Duration) -> Result<String, ProtocolError> {
        self.send_ok("noclip", &[if enabled { "on" } else { "off" }], timeout)
    }

    /// Sends an experimental S.T.A.L.K.E.R. 2 time-speed command.
    pub fn s2_time_speed(&self, speed: f32, timeout: Duration) -> Result<String, ProtocolError> {
        if !speed.is_finite() || !(0.0..=100.0).contains(&speed) {
            return Err(ProtocolError::Invalid("time speed is outside 0..100".to_owned()));
        }
        let speed = speed.to_string();
        self.send_ok("timespeed", &[speed.as_str()], timeout)
    }

    /// Exchange directory used by this client.
    #[must_use]
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

fn fresh_id() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |value| value.as_nanos());
    let counter = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{:x}{:x}{:x}", std::process::id(), timestamp, counter)
}

fn read_reply_bytes(path: &Path) -> Result<Vec<u8>, std::io::Error> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "companion reply is not a regular file",
        ));
    }
    let file = File::open(path)?;
    let mut bytes = Vec::new();
    file.take(
        u64::try_from(MAX_COMPANION_FILE_BYTES)
            .unwrap_or(u64::MAX)
            .saturating_add(1),
    )
    .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_COMPANION_FILE_BYTES {
        bytes.clear();
    }
    Ok(bytes)
}

fn reply_id(reply: &str) -> Option<String> {
    let mut fields = reply.split_whitespace();
    if fields.next()? != "v1" {
        return None;
    }
    let id = fields.next()?;
    valid_id(id).then(|| id.to_owned())
}

fn parse_reply(reply: &str) -> Result<ProtocolReply, ProtocolError> {
    let line = reply.trim_end_matches(['\r', '\n']);
    if line.contains(['\r', '\n']) {
        return Err(ProtocolError::Invalid("reply must be exactly one line".to_owned()));
    }
    let mut fields = line.splitn(4, ' ').filter(|field| !field.is_empty());
    if fields.next() != Some("v1") {
        return Err(ProtocolError::Invalid("reply does not match protocol v1".to_owned()));
    }
    let id = fields
        .next()
        .ok_or_else(|| ProtocolError::Invalid("reply has no ID".to_owned()))?;
    if !valid_id(id) {
        return Err(ProtocolError::Invalid("reply ID is invalid".to_owned()));
    }
    let status = match fields.next() {
        Some("ok") => ReplyStatus::Ok,
        Some("error") => ReplyStatus::Error,
        Some("unsupported") => ReplyStatus::Unsupported,
        _ => return Err(ProtocolError::Invalid("reply status is unknown".to_owned())),
    };
    Ok(ProtocolReply {
        id: id.to_owned(),
        status,
        text: fields.next().unwrap_or_default().to_owned(),
    })
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn format_command(command: &str, arguments: &[&str]) -> Result<String, ProtocolError> {
    let bounds = match command {
        "ping" | "info" | "heal" | "repair_equipped" | "list_inventory" | "mark" | "jump_last" => (0, 0),
        "give" => (1, 2),
        "money" | "god" | "noclip" | "timespeed" | "hotkeys" => (1, 1),
        "teleport" => (3, 3),
        "weather" => (0, 2),
        "quicksave" => (0, 1),
        _ => {
            return Err(ProtocolError::Invalid(
                "command is not in companion protocol v1".to_owned(),
            ))
        }
    };
    if arguments.len() < bounds.0
        || arguments.len() > bounds.1
        || arguments.iter().any(|value| {
            value.is_empty() || value.chars().any(char::is_whitespace) || value.chars().any(char::is_control)
        })
    {
        return Err(ProtocolError::Invalid("command arguments are invalid".to_owned()));
    }
    match command {
        "god" | "noclip" | "hotkeys" if !matches!(arguments.first(), Some(&"on" | &"off")) => {
            return Err(ProtocolError::Invalid("command expects on or off".to_owned()))
        }
        "money" if arguments.first().is_some_and(|value| value.parse::<i32>().is_err()) => {
            return Err(ProtocolError::Invalid("money delta is invalid".to_owned()))
        }
        "weather" if arguments.get(1).is_some_and(|value| *value != "now") => {
            return Err(ProtocolError::Invalid(
                "weather's second argument must be now".to_owned(),
            ))
        }
        "give"
            if arguments
                .get(1)
                .is_some_and(|value| value.parse::<u8>().map_or(true, |count| !(1..=100).contains(&count))) =>
        {
            return Err(ProtocolError::Invalid("give count is outside 1..100".to_owned()))
        }
        "teleport"
            if arguments
                .iter()
                .any(|value| value.parse::<f32>().map_or(true, |number| !number.is_finite())) =>
        {
            return Err(ProtocolError::Invalid(
                "teleport coordinates must be finite numbers".to_owned(),
            ))
        }
        "timespeed"
            if arguments.first().is_some_and(|value| {
                value
                    .parse::<f32>()
                    .map_or(true, |number| !number.is_finite() || !(0.0..=100.0).contains(&number))
            }) =>
        {
            return Err(ProtocolError::Invalid("time speed is outside 0..100".to_owned()))
        }
        _ => {}
    }
    if arguments.is_empty() {
        if command.len() > MAX_COMPANION_FILE_BYTES.saturating_sub(32) {
            return Err(ProtocolError::Invalid(
                "command exceeds the protocol size limit".to_owned(),
            ));
        }
        Ok(command.to_owned())
    } else {
        let formatted = format!("{command} {}", arguments.join(" "));
        if formatted.len() > MAX_COMPANION_FILE_BYTES.saturating_sub(32) {
            return Err(ProtocolError::Invalid(
                "command exceeds the protocol size limit".to_owned(),
            ));
        }
        Ok(formatted)
    }
}

fn remove_owned_command(path: &Path, expected: &[u8]) {
    if fs::read(path).is_ok_and(|bytes| bytes == expected) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Test-only temporary files use expect to make fixture setup failures explicit.
mod reader_tests {
    use super::{parse_reply, read_reply_bytes, reply_id};
    use crate::MAX_COMPANION_FILE_BYTES;
    use std::fs;
    use std::io::ErrorKind;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn temporary_file() -> PathBuf {
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("sse-c6-reply-{}-{index}", std::process::id()))
    }

    #[test]
    fn reply_reader_rejects_truncation_before_status_is_complete() {
        let fixture = b"v1 request_123 ok complete\n";
        for end in 0..16 {
            let prefix = fixture.get(..end).unwrap_or_default();
            let text = std::str::from_utf8(prefix).unwrap_or_default();
            assert!(parse_reply(text).is_err(), "accepted truncated header at byte {end}");
        }
    }

    #[test]
    fn deterministic_reply_bit_flips_cannot_retain_the_original_correlation_id() {
        let fixture = b"v1 request_123 ok complete\n";
        let masks = [1_u8, 2, 4, 8, 16, 32, 64, 128];
        let mut valid_mutations = 0_u32;
        for index in 3..14 {
            for mask in masks {
                let mut changed = fixture.to_vec();
                if let Some(byte) = changed.get_mut(index) {
                    *byte ^= mask;
                }
                if let Ok(text) = std::str::from_utf8(&changed) {
                    if let Ok(parsed) = parse_reply(text) {
                        valid_mutations = valid_mutations.saturating_add(1);
                        assert_ne!(reply_id(text).as_deref(), Some("request_123"));
                        assert_eq!(reply_id(text).as_deref(), Some(parsed.id.as_str()));
                    }
                }
            }
        }
        assert!(valid_mutations > 0);
    }

    #[test]
    fn reply_reader_discards_a_hostile_oversized_file() {
        let path = temporary_file();
        let hostile = vec![b'x'; MAX_COMPANION_FILE_BYTES.saturating_add(1)];
        fs::write(&path, hostile).expect("write oversized reply fixture");
        assert!(read_reply_bytes(&path).expect("read bounded reply").is_empty());
        let _ = fs::remove_file(path);
    }

    #[test]
    fn reply_reader_rejects_non_regular_paths_before_opening() {
        let path = temporary_file();
        fs::create_dir_all(&path).expect("create directory reply path");
        let error = read_reply_bytes(&path).expect_err("directory is not a reply file");
        assert_eq!(error.kind(), ErrorKind::InvalidData);
        let _ = fs::remove_dir_all(path);
    }
}
