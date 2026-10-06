//! Bounded JSON-lines protocol used by the native Steam worker process.

use std::io::{BufRead, Write};

use sse_codecs::json::{Event, NumberExt, Reader};

use crate::api::{Achievement, CloudFile, WriteStage};
use crate::cloud::MAX_CLOUD_FILE_BYTES;

/// Maximum UTF-8 JSON header accepted from either side of the worker pipe.
pub const MAX_HEADER_BYTES: usize = 1024 * 1024;
const MAX_JSON_NESTING: usize = 64;

/// One native RemoteStorage operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeRequest {
    /// Lists files for one Steam app.
    List {
        /// Steam application identifier.
        app_id: u32,
    },
    /// Reads one remote file.
    Read {
        /// Steam application identifier.
        app_id: u32,
        /// Exact remote filename.
        file_name: String,
    },
    /// Writes one verified X-Ray save image.
    Write {
        /// Steam application identifier.
        app_id: u32,
        /// Exact remote filename.
        file_name: String,
        /// Bounded length of the raw payload.
        size: usize,
        /// Save image bytes; empty until read from the worker pipe.
        bytes: Vec<u8>,
    },
}

/// A worker response with a raw byte body only for file reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeResponse {
    /// A sorted RemoteStorage listing.
    Files(Vec<CloudFile>),
    /// Raw file bytes follow the JSON header.
    Data(Vec<u8>),
    /// A write was accepted by Steam.
    Ok,
    /// Readiness line emitted by a long-running session worker.
    Ready,
    /// PascalCase achievement list from native-op.
    Achievements(Vec<Achievement>),
    /// PascalCase achievement mutation result from native-op.
    Achievement(Achievement),
    /// A bounded operation failed.
    Error {
        /// Human-readable reason.
        message: String,
        /// Present for save writes.
        stage: Option<WriteStage>,
    },
}

/// JSON-line parse, framing, or size error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtocolError {
    /// Human-readable error.
    pub message: String,
}

impl ProtocolError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProtocolError {}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Null,
    Bool(bool),
    String(String),
    Number(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

/// Reads one newline-terminated JSON header without growing beyond the protocol limit.
pub fn read_header(reader: &mut impl BufRead) -> Result<Vec<u8>, ProtocolError> {
    let mut bytes = Vec::new();
    loop {
        let available = reader
            .fill_buf()
            .map_err(|error| ProtocolError::new(error.to_string()))?;
        if available.is_empty() {
            return Err(ProtocolError::new("Steam worker closed the pipe before a JSON header"));
        }
        let line_end = available.iter().position(|byte| *byte == b'\n');
        let take = line_end.unwrap_or(available.len());
        let next_len = bytes
            .len()
            .checked_add(take)
            .ok_or_else(|| ProtocolError::new("Steam worker header length overflow"))?;
        if next_len > MAX_HEADER_BYTES {
            return Err(ProtocolError::new("Steam worker header exceeded the size limit"));
        }
        bytes
            .try_reserve(take)
            .map_err(|error| ProtocolError::new(format!("Steam worker header allocation failed: {error}")))?;
        let prefix = available
            .get(..take)
            .ok_or_else(|| ProtocolError::new("Steam worker header range is invalid"))?;
        bytes.extend_from_slice(prefix);
        let consumed = take
            .checked_add(usize::from(line_end.is_some()))
            .ok_or_else(|| ProtocolError::new("Steam worker header length overflow"))?;
        reader.consume(consumed);
        if line_end.is_some() {
            return Ok(bytes);
        }
    }
}

/// Parses one request line. JSON keys are case-insensitive and app id/size may be strings.
pub fn parse_request(bytes: &[u8]) -> Result<NativeRequest, ProtocolError> {
    let root = parse_json(bytes)?;
    let object = as_object(&root, "request must be a JSON object")?;
    let operation = field(object, "operation")?
        .and_then(as_string)
        .ok_or_else(|| ProtocolError::new("Steam worker request has no operation"))?;
    let app_id = field(object, "appId")?
        .and_then(as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| ProtocolError::new("Steam worker appId must be a positive integer"))?;
    match operation.to_ascii_lowercase().as_str() {
        "list" => Ok(NativeRequest::List { app_id }),
        "read" => {
            let file_name = required_string(object, "fileName")?;
            Ok(NativeRequest::Read { app_id, file_name })
        }
        "write" => {
            let file_name = required_string(object, "fileName")?;
            let size = field(object, "size")?
                .and_then(as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .ok_or_else(|| ProtocolError::new("Steam worker write size is invalid"))?;
            if size == 0 || size > MAX_CLOUD_FILE_BYTES {
                return Err(ProtocolError::new(
                    "RemoteStorage write size is outside the supported range.",
                ));
            }
            Ok(NativeRequest::Write {
                app_id,
                file_name,
                size,
                bytes: Vec::new(),
            })
        }
        _ => Err(ProtocolError::new(
            "Only Steam RemoteStorage list, read, and save-write are supported.",
        )),
    }
}

/// Returns whether a valid JSON header identifies a write operation, even when another field is invalid.
pub fn is_write_request_header(bytes: &[u8]) -> bool {
    let Ok(root) = parse_json(bytes) else {
        return false;
    };
    let Ok(object) = as_object(&root, "request must be a JSON object") else {
        return false;
    };
    let Ok(Some(operation)) = field(object, "operation") else {
        return false;
    };
    as_string(operation).is_some_and(|operation| operation.eq_ignore_ascii_case("write"))
}

/// Reads a request and its optional write body, returning the same owned buffer for write bytes.
pub fn read_request(reader: &mut impl BufRead) -> Result<NativeRequest, ProtocolError> {
    let header = read_header(reader)?;
    let mut request = parse_request(&header)?;
    read_request_body(reader, &mut request)?;
    finish_request(reader)?;
    Ok(request)
}

/// Reads the raw body for an already parsed write request.
pub fn read_request_body(reader: &mut impl BufRead, request: &mut NativeRequest) -> Result<(), ProtocolError> {
    if let NativeRequest::Write { size, bytes, .. } = request {
        if *size == 0 || *size > MAX_CLOUD_FILE_BYTES {
            return Err(ProtocolError::new(
                "RemoteStorage write size is outside the supported range.",
            ));
        }
        bytes
            .try_reserve_exact(*size)
            .map_err(|error| ProtocolError::new(format!("Steam worker write allocation failed: {error}")))?;
        bytes.resize(*size, 0);
        reader
            .read_exact(bytes)
            .map_err(|error| ProtocolError::new(format!("Steam worker write body is truncated: {error}")))?;
    }
    Ok(())
}

/// Requires EOF after the single request and its optional raw body.
pub fn finish_request(reader: &mut impl BufRead) -> Result<(), ProtocolError> {
    let mut trailing = [0_u8; 1];
    match reader.read(&mut trailing) {
        Ok(0) => Ok(()),
        Ok(_) => Err(ProtocolError::new("trailing bytes after Steam worker request")),
        Err(error) => Err(ProtocolError::new(error.to_string())),
    }
}

/// Encodes the JSON header for a request. Write data is sent separately and is not copied here.
pub fn encode_request_header(request: &NativeRequest) -> Result<Vec<u8>, ProtocolError> {
    let (operation, app_id, file_name, size) = match request {
        NativeRequest::List { app_id } => ("list", *app_id, None, None),
        NativeRequest::Read { app_id, file_name } => ("read", *app_id, Some(file_name.as_str()), None),
        NativeRequest::Write {
            app_id,
            file_name,
            size,
            bytes,
        } => {
            if *size == 0 || *size > MAX_CLOUD_FILE_BYTES || bytes.len() != *size {
                return Err(ProtocolError::new(
                    "RemoteStorage write size is outside the supported range.",
                ));
            }
            ("write", *app_id, Some(file_name.as_str()), Some(*size))
        }
    };
    if app_id == 0 {
        return Err(ProtocolError::new("Steam worker appId must be a positive integer"));
    }
    let mut line = String::new();
    line.push_str("{\"operation\":");
    push_json_string(&mut line, operation)?;
    line.push_str(",\"appId\":");
    line.push_str(&app_id.to_string());
    line.push_str(",\"fileName\":");
    if let Some(file_name) = file_name {
        push_json_string(&mut line, file_name)?;
    } else {
        line.push_str("null");
    }
    if let Some(size) = size {
        line.push_str(",\"size\":");
        line.push_str(&size.to_string());
    }
    line.push_str("}\n");
    bound_header(line)
}

/// Reads one response header and exactly its optional data body.
pub fn read_response(reader: &mut impl BufRead) -> Result<NativeResponse, ProtocolError> {
    let header = read_header(reader)?;
    let value = parse_json(&header)?;
    let object = as_object(&value, "Steam worker response must be a JSON object")?;
    let kind = field(object, "type")?
        .and_then(as_string)
        .ok_or_else(|| ProtocolError::new("Steam worker response has no type"))?;
    let response = match kind.to_ascii_lowercase().as_str() {
        "files" => {
            let items = field(object, "files")?
                .and_then(as_array)
                .ok_or_else(|| ProtocolError::new("Steam worker files response has no files array"))?;
            let mut files = Vec::new();
            files
                .try_reserve(items.len())
                .map_err(|error| ProtocolError::new(format!("Steam file list allocation failed: {error}")))?;
            for item in items {
                files.push(parse_cloud_file(item)?);
            }
            Ok(NativeResponse::Files(files))
        }
        "data" => {
            let size = field(object, "size")?
                .and_then(as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value <= MAX_CLOUD_FILE_BYTES)
                .ok_or_else(|| ProtocolError::new("Steam worker returned an invalid file size."))?;
            let mut bytes = Vec::new();
            bytes
                .try_reserve_exact(size)
                .map_err(|error| ProtocolError::new(format!("Steam file allocation failed: {error}")))?;
            bytes.resize(size, 0);
            reader
                .read_exact(&mut bytes)
                .map_err(|error| ProtocolError::new(format!("Steam worker data body is truncated: {error}")))?;
            Ok(NativeResponse::Data(bytes))
        }
        "ok" => Ok(NativeResponse::Ok),
        "ready" => Ok(NativeResponse::Ready),
        "achievements" => {
            let items = field(object, "items")?
                .and_then(as_array)
                .ok_or_else(|| ProtocolError::new("Steam achievement response has no items array"))?;
            let mut achievements = Vec::new();
            achievements
                .try_reserve(items.len())
                .map_err(|error| ProtocolError::new(format!("Steam achievement allocation failed: {error}")))?;
            for item in items {
                achievements.push(parse_achievement(item)?);
            }
            Ok(NativeResponse::Achievements(achievements))
        }
        "achievement" => {
            let item =
                field(object, "item")?.ok_or_else(|| ProtocolError::new("Steam achievement response has no item"))?;
            Ok(NativeResponse::Achievement(parse_achievement(item)?))
        }
        "error" => {
            let message = field(object, "message")?
                .and_then(as_string)
                .ok_or_else(|| ProtocolError::new("Steam worker error response has no message"))?;
            let stage = match field(object, "stage")? {
                None | Some(Value::Null) => None,
                Some(value) => Some(
                    match as_string(value)
                        .ok_or_else(|| ProtocolError::new("Steam worker error stage is invalid"))?
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "before_write" => WriteStage::BeforeWrite,
                        "write_rejected" => WriteStage::WriteRejected,
                        "after_write" => WriteStage::AfterWrite,
                        _ => return Err(ProtocolError::new("Steam worker error stage is unknown")),
                    },
                ),
            };
            Ok(NativeResponse::Error { message, stage })
        }
        _ => Err(ProtocolError::new(
            "Steam worker returned an unexpected list/read/write response.",
        )),
    }?;
    let mut trailing = [0_u8; 1];
    match reader.read(&mut trailing) {
        Ok(0) => Ok(response),
        Ok(_) => Err(ProtocolError::new("trailing bytes after Steam worker response")),
        Err(error) => Err(ProtocolError::new(error.to_string())),
    }
}

/// Writes a files JSON header bounded to one MiB.
pub fn write_files_response(output: &mut impl Write, files: &[CloudFile]) -> Result<(), ProtocolError> {
    let mut header = String::new();
    header.push_str("{\"type\":\"files\",\"files\":[");
    for (index, file) in files.iter().enumerate() {
        if index != 0 {
            header.push(',');
        }
        header.push_str("{\"name\":");
        push_json_string(&mut header, &file.name)?;
        header.push_str(",\"size\":");
        header.push_str(&file.size.to_string());
        header.push_str(",\"timestamp\":");
        header.push_str(&file.timestamp.to_string());
        header.push_str(",\"isPersisted\":");
        header.push_str(if file.persisted { "true" } else { "false" });
        header.push_str(",\"exists\":");
        header.push_str(if file.exists { "true" } else { "false" });
        header.push('}');
        if header.len() > MAX_HEADER_BYTES {
            return Err(ProtocolError::new(
                "Steam worker response header exceeded the size limit.",
            ));
        }
    }
    header.push_str("]}\n");
    write_header(output, header)
}

/// Writes a raw file read header followed by the unchanged data bytes.
pub fn write_data_response(output: &mut impl Write, bytes: &[u8]) -> Result<(), ProtocolError> {
    if bytes.len() > MAX_CLOUD_FILE_BYTES {
        return Err(ProtocolError::new("Steam worker returned an invalid file size."));
    }
    let header = format!("{{\"type\":\"data\",\"size\":{}}}\n", bytes.len());
    write_header(output, header)?;
    output
        .write_all(bytes)
        .map_err(|error| ProtocolError::new(error.to_string()))
}

/// Writes a successful write response.
pub fn write_ok_response(output: &mut impl Write) -> Result<(), ProtocolError> {
    write_header(output, "{\"type\":\"ok\"}\n".to_owned())
}

/// Writes an error response, including the transaction stage for cloud writes.
pub fn write_error_response(
    output: &mut impl Write,
    message: &str,
    stage: Option<WriteStage>,
) -> Result<(), ProtocolError> {
    let mut header = String::from("{\"type\":\"error\",\"message\":");
    push_json_string(&mut header, message)?;
    if let Some(stage) = stage {
        header.push_str(",\"stage\":");
        push_json_string(&mut header, stage.as_protocol_value())?;
    }
    header.push_str("}\n");
    write_header(output, header)
}

/// Encodes a PascalCase achievements response compatible with ACCEPTANCE Part IV §4.5.
pub fn encode_achievements(entries: &[Achievement]) -> Result<Vec<u8>, ProtocolError> {
    let mut header = String::from("{\"type\":\"Achievements\",\"items\":[");
    for (index, entry) in entries.iter().enumerate() {
        if index != 0 {
            header.push(',');
        }
        push_achievement(&mut header, entry)?;
        if header.len() > MAX_HEADER_BYTES {
            return Err(ProtocolError::new(
                "Steam worker response header exceeded the size limit.",
            ));
        }
    }
    header.push_str("]}\n");
    bound_header(header)
}

/// Encodes one PascalCase achievement mutation response.
pub fn encode_achievement(entry: &Achievement) -> Result<Vec<u8>, ProtocolError> {
    let mut header = String::from("{\"type\":\"Achievement\",\"item\":");
    push_achievement(&mut header, entry)?;
    header.push_str("}\n");
    bound_header(header)
}

/// Encodes a PascalCase error response for the native-op worker.
pub fn encode_operation_error(message: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut header = String::from("{\"type\":\"Error\",\"message\":");
    push_json_string(&mut header, message)?;
    header.push_str("}\n");
    bound_header(header)
}

/// Encodes a lowercase session-worker error response.
pub fn encode_session_error(message: &str) -> Result<Vec<u8>, ProtocolError> {
    let mut header = String::from("{\"type\":\"error\",\"message\":");
    push_json_string(&mut header, message)?;
    header.push_str("}\n");
    bound_header(header)
}

fn push_achievement(output: &mut String, entry: &Achievement) -> Result<(), ProtocolError> {
    output.push_str("{\"apiName\":");
    push_json_string(output, &entry.name)?;
    output.push_str(",\"name\":");
    push_json_string(output, &entry.display_name)?;
    output.push_str(",\"description\":");
    push_json_string(output, &entry.description)?;
    output.push_str(",\"achieved\":");
    output.push_str(if entry.achieved { "true" } else { "false" });
    output.push_str(",\"unlockTime\":");
    output.push_str(&entry.unlock_time.to_string());
    output.push_str(",\"hidden\":");
    output.push_str(if entry.hidden { "true" } else { "false" });
    output.push('}');
    Ok(())
}

fn write_header(output: &mut impl Write, header: String) -> Result<(), ProtocolError> {
    let bytes = bound_header(header)?;
    output
        .write_all(&bytes)
        .map_err(|error| ProtocolError::new(error.to_string()))
}

fn bound_header(mut header: String) -> Result<Vec<u8>, ProtocolError> {
    if !header.ends_with('\n') || header.len() > MAX_HEADER_BYTES.saturating_add(1) {
        return Err(ProtocolError::new("Steam worker header exceeded the size limit"));
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(header.len())
        .map_err(|error| ProtocolError::new(format!("Steam worker header allocation failed: {error}")))?;
    bytes.extend_from_slice(header.as_bytes());
    header.clear();
    Ok(bytes)
}

fn parse_cloud_file(value: &Value) -> Result<CloudFile, ProtocolError> {
    let object = as_object(value, "Steam worker file entry must be an object")?;
    let name = required_string(object, "name")?;
    let size = field(object, "size")?
        .and_then(as_u64)
        .ok_or_else(|| ProtocolError::new("Steam worker file size is invalid"))?;
    let timestamp = field(object, "timestamp")?
        .and_then(as_i64)
        .ok_or_else(|| ProtocolError::new("Steam worker file timestamp is invalid"))?;
    let persisted = field(object, "isPersisted")?
        .and_then(as_bool)
        .ok_or_else(|| ProtocolError::new("Steam worker file isPersisted is invalid"))?;
    let exists = field(object, "exists")?
        .and_then(as_bool)
        .ok_or_else(|| ProtocolError::new("Steam worker file exists is invalid"))?;
    Ok(CloudFile {
        name,
        size,
        timestamp,
        persisted,
        exists,
    })
}

fn parse_achievement(value: &Value) -> Result<Achievement, ProtocolError> {
    let object = as_object(value, "Steam achievement entry must be an object")?;
    let name = required_string(object, "apiName")?;
    let display_name = required_string(object, "name")?;
    let description = required_string(object, "description")?;
    let achieved = field(object, "achieved")?
        .and_then(as_bool)
        .ok_or_else(|| ProtocolError::new("Steam achievement achieved state is invalid"))?;
    let unlock_time = field(object, "unlockTime")?
        .and_then(as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| ProtocolError::new("Steam achievement unlock time is invalid"))?;
    let hidden = field(object, "hidden")?
        .and_then(as_bool)
        .ok_or_else(|| ProtocolError::new("Steam achievement hidden state is invalid"))?;
    Ok(Achievement {
        name,
        display_name,
        description,
        hidden,
        achieved,
        unlock_time,
    })
}

fn required_string(object: &[(String, Value)], name: &str) -> Result<String, ProtocolError> {
    field(object, name)?
        .and_then(as_string)
        .ok_or_else(|| ProtocolError::new(format!("Steam worker field {name} must be a string")))
}

fn field<'a>(object: &'a [(String, Value)], name: &str) -> Result<Option<&'a Value>, ProtocolError> {
    let mut matches = object
        .iter()
        .filter(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(ProtocolError::new(format!("Steam worker field {name} is duplicated")));
    }
    Ok(first)
}

fn as_object<'a>(value: &'a Value, error: &str) -> Result<&'a [(String, Value)], ProtocolError> {
    match value {
        Value::Object(items) => Ok(items),
        _ => Err(ProtocolError::new(error)),
    }
}

fn as_array(value: &Value) -> Option<&[Value]> {
    match value {
        Value::Array(items) => Some(items),
        _ => None,
    }
}

fn as_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn as_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(value) => Some(*value),
        _ => None,
    }
}

fn as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(value) => value.as_str().as_u64(),
        Value::String(value) => value.as_str().as_u64(),
        _ => None,
    }
}

fn as_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(value) => value.as_str().as_i64(),
        Value::String(value) => value.as_str().as_i64(),
        _ => None,
    }
}

fn push_json_string(output: &mut String, value: &str) -> Result<(), ProtocolError> {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\u{08}' => output.push_str("\\b"),
            '\u{0c}' => output.push_str("\\f"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                write!(output, "\\u{:04x}", u32::from(character))
                    .map_err(|error| ProtocolError::new(error.to_string()))?;
            }
            character => output.push(character),
        }
        if output.len() > MAX_HEADER_BYTES {
            return Err(ProtocolError::new("Steam worker header exceeded the size limit"));
        }
    }
    output.push('"');
    Ok(())
}

fn parse_json(bytes: &[u8]) -> Result<Value, ProtocolError> {
    if bytes.len() > MAX_HEADER_BYTES {
        return Err(ProtocolError::new("Steam worker header exceeded the size limit"));
    }
    let mut reader = Reader::new(bytes);
    let event = reader
        .next_event()
        .map_err(|error| ProtocolError::new(error.to_string()))?
        .ok_or_else(|| ProtocolError::new("Steam worker JSON value is missing"))?;
    let value = parse_event(&mut reader, event, 0)?;
    if reader
        .next_event()
        .map_err(|error| ProtocolError::new(error.to_string()))?
        .is_some()
    {
        return Err(ProtocolError::new("trailing data after Steam worker JSON"));
    }
    Ok(value)
}

fn parse_event(reader: &mut Reader<'_>, event: Event<'_>, depth: usize) -> Result<Value, ProtocolError> {
    match event {
        Event::Null => Ok(Value::Null),
        Event::Bool(value) => Ok(Value::Bool(value)),
        Event::String(value) => Ok(Value::String(value.into_owned())),
        Event::Number(value) => Ok(Value::Number(value.to_owned())),
        Event::ArrayStart => {
            let child_depth = next_json_depth(depth)?;
            let mut values = Vec::new();
            loop {
                match reader
                    .next_event()
                    .map_err(|error| ProtocolError::new(error.to_string()))?
                    .ok_or_else(|| ProtocolError::new("truncated Steam worker JSON array"))?
                {
                    Event::ArrayEnd => return Ok(Value::Array(values)),
                    event => {
                        let value = parse_event(reader, event, child_depth)?;
                        values
                            .try_reserve(1)
                            .map_err(|error| ProtocolError::new(format!("JSON array allocation failed: {error}")))?;
                        values.push(value);
                    }
                }
            }
        }
        Event::ObjectStart => {
            let child_depth = next_json_depth(depth)?;
            let mut values = Vec::new();
            loop {
                match reader
                    .next_event()
                    .map_err(|error| ProtocolError::new(error.to_string()))?
                    .ok_or_else(|| ProtocolError::new("truncated Steam worker JSON object"))?
                {
                    Event::ObjectEnd => return Ok(Value::Object(values)),
                    Event::Key(key) => {
                        let key = key.into_owned();
                        let event = reader
                            .next_event()
                            .map_err(|error| ProtocolError::new(error.to_string()))?
                            .ok_or_else(|| ProtocolError::new("Steam worker JSON object value is missing"))?;
                        let value = parse_event(reader, event, child_depth)?;
                        values
                            .try_reserve(1)
                            .map_err(|error| ProtocolError::new(format!("JSON object allocation failed: {error}")))?;
                        values.push((key, value));
                    }
                    _ => return Err(ProtocolError::new("invalid Steam worker JSON object")),
                }
            }
        }
        Event::Key(_) | Event::ObjectEnd | Event::ArrayEnd => {
            Err(ProtocolError::new("invalid Steam worker JSON value"))
        }
    }
}

fn next_json_depth(depth: usize) -> Result<usize, ProtocolError> {
    let next = depth
        .checked_add(1)
        .ok_or_else(|| ProtocolError::new("Steam worker JSON nesting overflow"))?;
    if next > MAX_JSON_NESTING {
        return Err(ProtocolError::new("Steam worker JSON nesting exceeds the limit"));
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        encode_achievement, encode_achievements, encode_request_header, parse_request, read_header, read_request,
        read_response, write_data_response, NativeRequest, NativeResponse, MAX_HEADER_BYTES,
    };
    use crate::api::{Achievement, CloudFile, WriteStage};

    #[test]
    fn request_headers_round_trip_and_accept_csharp_numeric_strings() {
        let request = parse_request(
            r#"{"Operation":"READ","APPID":"41700","FileName":"_appdata_/savedgames/квест.sav"}"#.as_bytes(),
        );
        assert_eq!(
            request,
            Ok(NativeRequest::Read {
                app_id: 41_700,
                file_name: "_appdata_/savedgames/квест.sav".to_owned(),
            })
        );
        let write = NativeRequest::Write {
            app_id: 4500,
            file_name: "_appdata_/savedgames/a.sav".to_owned(),
            size: 3,
            bytes: b"abc".to_vec(),
        };
        let header = encode_request_header(&write);
        assert!(header.is_ok());
        assert!(header.is_ok_and(|bytes| {
            bytes.starts_with(br#"{"operation":"write","appId":4500"#) && bytes.ends_with(b"\"size\":3}\n")
        }));
    }

    #[test]
    fn files_and_data_responses_keep_metadata_and_raw_bytes() {
        let files = vec![CloudFile {
            name: "slot\"1.sav".to_owned(),
            size: 7,
            timestamp: -2,
            persisted: true,
            exists: true,
        }];
        let mut output = Vec::new();
        assert!(super::write_files_response(&mut output, &files).is_ok());
        let parsed = read_response(&mut Cursor::new(output));
        assert_eq!(parsed, Ok(NativeResponse::Files(files)));

        let mut output = Vec::new();
        assert!(write_data_response(&mut output, b"\0save").is_ok());
        assert_eq!(
            read_response(&mut Cursor::new(output)),
            Ok(NativeResponse::Data(b"\0save".to_vec()))
        );
        assert!(read_response(&mut Cursor::new(b"{\"type\":\"ok\"}\nextra".to_vec())).is_err());
    }

    #[test]
    fn write_error_stages_and_pascal_case_achievements_round_trip() {
        let mut output = Vec::new();
        assert!(super::write_error_response(&mut output, "rejected", Some(WriteStage::WriteRejected)).is_ok());
        assert_eq!(
            read_response(&mut Cursor::new(output)),
            Ok(NativeResponse::Error {
                message: "rejected".to_owned(),
                stage: Some(WriteStage::WriteRejected),
            })
        );

        let entry = Achievement {
            name: "API_A".to_owned(),
            display_name: "Achievement".to_owned(),
            description: "Description".to_owned(),
            hidden: true,
            achieved: false,
            unlock_time: 0,
        };
        let encoded = encode_achievements(std::slice::from_ref(&entry));
        assert!(encoded.as_ref().is_ok_and(|bytes| {
            read_response(&mut Cursor::new(bytes.clone())) == Ok(NativeResponse::Achievements(vec![entry.clone()]))
        }));
        assert!(encoded.is_ok_and(|bytes| {
            let text = String::from_utf8(bytes).unwrap_or_default();
            text.contains("\"type\":\"Achievements\"")
                && text.contains("\"apiName\":\"API_A\"")
                && text.contains("\"unlockTime\":0")
        }));
        let encoded = encode_achievement(&entry);
        assert!(encoded.as_ref().is_ok_and(|bytes| {
            read_response(&mut Cursor::new(bytes.clone())) == Ok(NativeResponse::Achievement(entry.clone()))
        }));
        assert!(encoded.is_ok_and(|bytes| {
            String::from_utf8(bytes).is_ok_and(|text| text.contains("\"type\":\"Achievement\""))
        }));
    }

    #[test]
    fn bounded_header_rejects_hostile_length_and_truncation() {
        let oversized = vec![b'x'; MAX_HEADER_BYTES + 1];
        let mut cursor = Cursor::new(oversized);
        assert!(read_header(&mut cursor).is_err());
        assert!(read_header(&mut Cursor::new(b"{\"type\":\"ok\"}".to_vec())).is_err());

        let hostile = format!(
            "{{\"operation\":\"write\",\"appId\":4500,\"fileName\":\"a.sav\",\"size\":{}}}",
            u64::MAX
        );
        assert!(parse_request(hostile.as_bytes()).is_err());
        assert!(read_request(&mut Cursor::new(
            b"{\"operation\":\"write\",\"appId\":4500,\"fileName\":\"a.sav\",\"size\":3}\nab".to_vec()
        ))
        .is_err());
    }

    #[test]
    fn deeply_nested_json_is_rejected_without_recursing_unboundedly() {
        let nested = format!(
            "{{\"operation\":\"list\",\"appId\":41700,\"ignored\":{}null{}}}",
            "[".repeat(128),
            "]".repeat(128)
        );
        assert!(parse_request(nested.as_bytes()).is_err());
        assert!(std::panic::catch_unwind(|| parse_request(nested.as_bytes())).is_ok());
    }

    #[test]
    fn deterministic_json_mutations_never_panic() {
        let fixture = br#"{"operation":"list","appId":41700,"fileName":null}"#;
        for end in 0..fixture.len() {
            if let Some(prefix) = fixture.get(..end) {
                let _ = std::panic::catch_unwind(|| parse_request(prefix));
            }
        }
        for bit in 0..fixture.len().saturating_mul(8) {
            let mut changed = fixture.to_vec();
            if let Some(byte) = changed.get_mut(bit / 8) {
                *byte ^= 1_u8.checked_shl(u32::try_from(bit % 8).unwrap_or(0)).unwrap_or(0);
            }
            let _ = std::panic::catch_unwind(|| parse_request(&changed));
        }
        let mut seed = 0x4b35_u64;
        for _ in 0..2_000 {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            let mut changed = fixture.to_vec();
            let index = usize::try_from(seed).unwrap_or(0) % changed.len();
            if let Some(byte) = changed.get_mut(index) {
                *byte ^= u8::try_from(seed >> 32).unwrap_or(0);
            }
            assert!(std::panic::catch_unwind(|| parse_request(&changed)).is_ok());
        }
    }
}
