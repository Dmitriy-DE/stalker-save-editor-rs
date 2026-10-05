//! Versioned little-endian length-prefixed binary worker protocol.

use std::io::{Read, Write};
use std::ops::Range;
use std::path::PathBuf;

use crate::achievements::AchievementConfirmation;
use crate::api::{Achievement, CloudFile, SteamApi, SteamError, WriteStage};
use crate::cloud::{PreparedEdit, SteamCloudWriteTransaction, UnavailableSaveFormatVerifier, WriteStatus};

/// Maximum body size accepted by the worker protocol.
pub const MAX_FRAME_BYTES: usize = 72 * 1024 * 1024;
const PROTOCOL_VERSION: u8 = 1;

/// One worker request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// Enumerates Steam Remote Storage files.
    List {
        /// Steam application id.
        app_id: u32,
    },
    /// Downloads a Steam Remote Storage file.
    Read {
        /// Steam application id.
        app_id: u32,
        /// Remote filename.
        remote_name: String,
    },
    /// Executes a guarded cloud write transaction.
    Write {
        /// Steam application id.
        app_id: u32,
        /// Remote filename.
        remote_name: String,
        /// Hash observed when the edit was prepared.
        expected_source_sha256: [u8; 32],
        /// Directory for exclusive backup and recovery artifacts.
        artifact_directory: PathBuf,
        /// Edited save bytes.
        output: Vec<u8>,
    },
    /// Reads achievements for one app.
    ListAchievements {
        /// Steam application id.
        app_id: u32,
    },
    /// Sets an achievement after an explicit confirmation.
    SetAchievement {
        /// Steam application id.
        app_id: u32,
        /// Achievement API name.
        name: String,
        /// True only after an explicit confirmation.
        confirmed: bool,
    },
    /// Clears an achievement after an explicit confirmation.
    ClearAchievement {
        /// Steam application id.
        app_id: u32,
        /// Achievement API name.
        name: String,
        /// True only after an explicit confirmation.
        confirmed: bool,
    },
}

/// One worker response payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// False means the request failed; `payload` then contains a UTF-8 error message.
    pub ok: bool,
    /// Cloud-write transaction phase, when the response belongs to a write failure or uncertainty.
    pub stage: Option<WriteStage>,
    /// Operation data in the operation-specific binary layout.
    pub payload: Vec<u8>,
}

/// Encodes a request as a 4-byte little-endian length followed by a binary body.
pub fn encode_frame(request: &Request) -> Result<Vec<u8>, ProtocolError> {
    let mut frame = vec![0_u8; 4];
    frame.push(PROTOCOL_VERSION);
    match request {
        Request::List { app_id } => {
            frame.push(1);
            put_u32(&mut frame, *app_id);
        }
        Request::Read { app_id, remote_name } => {
            frame.push(2);
            put_u32(&mut frame, *app_id);
            put_string(&mut frame, remote_name)?;
        }
        Request::Write {
            app_id,
            remote_name,
            expected_source_sha256,
            artifact_directory,
            output,
        } => {
            if output.len() > crate::cloud::MAX_CLOUD_FILE_BYTES {
                return Err(ProtocolError::new("cloud payload exceeds the configured limit"));
            }
            frame.push(3);
            put_u32(&mut frame, *app_id);
            put_string(&mut frame, remote_name)?;
            frame.extend_from_slice(expected_source_sha256);
            put_string(&mut frame, &artifact_directory.to_string_lossy())?;
            put_bytes(&mut frame, output)?;
        }
        Request::ListAchievements { app_id } => {
            frame.push(4);
            put_u32(&mut frame, *app_id);
        }
        Request::SetAchievement {
            app_id,
            name,
            confirmed,
        } => {
            frame.push(5);
            put_u32(&mut frame, *app_id);
            put_string(&mut frame, name)?;
            frame.push(u8::from(*confirmed));
        }
        Request::ClearAchievement {
            app_id,
            name,
            confirmed,
        } => {
            frame.push(6);
            put_u32(&mut frame, *app_id);
            put_string(&mut frame, name)?;
            frame.push(u8::from(*confirmed));
        }
    }
    finish_frame(frame, "worker request")
}

/// Reads and bounds one frame body, rejecting attacker-controlled sizes before allocation.
pub fn read_frame(reader: &mut impl Read) -> Result<Vec<u8>, ProtocolError> {
    let mut prefix = [0_u8; 4];
    reader
        .read_exact(&mut prefix)
        .map_err(|error| ProtocolError::new(error.to_string()))?;
    let length = usize::try_from(u32::from_le_bytes(prefix))
        .map_err(|_| ProtocolError::new("worker frame length is not representable"))?;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(ProtocolError::new("worker frame length is outside the supported range"));
    }
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .map_err(|error| ProtocolError::new(error.to_string()))?;
    Ok(body)
}

/// Decodes exactly one length-prefixed worker request.
pub fn decode_request(frame: &[u8]) -> Result<Request, ProtocolError> {
    let mut cursor = frame;
    let body = read_frame(&mut cursor)?;
    if !cursor.is_empty() {
        return Err(ProtocolError::new("trailing bytes after worker request"));
    }
    decode_body(body)
}

/// Encodes a response frame using the same length prefix.
pub fn encode_response(response: &Response) -> Result<Vec<u8>, ProtocolError> {
    let body_length = response.payload.len().saturating_add(3);
    if body_length > MAX_FRAME_BYTES {
        return Err(ProtocolError::new("worker response exceeds the frame limit"));
    }
    let mut frame = Vec::with_capacity(body_length.saturating_add(4));
    frame.extend_from_slice(&[0_u8; 4]);
    frame.push(PROTOCOL_VERSION);
    frame.push(u8::from(response.ok));
    frame.push(encode_stage(response.stage));
    frame.extend_from_slice(&response.payload);
    finish_frame(frame, "worker response")
}

/// Decodes a response frame after its bounded body has been read.
pub fn decode_response_body(body: &[u8]) -> Result<Response, ProtocolError> {
    let mut cursor = Cursor::new(body);
    if cursor.u8()? != PROTOCOL_VERSION {
        return Err(ProtocolError::new("unsupported Steam worker response version"));
    }
    let ok = cursor.boolean()?;
    let stage = decode_stage(cursor.u8()?)?;
    let remaining = cursor.remaining();
    let payload = cursor.take(remaining)?.to_vec();
    Ok(Response { ok, stage, payload })
}

/// Decodes one request, runs it through the API and returns an operation-specific response.
pub fn handle_request(api: &mut dyn SteamApi, request: Request) -> Response {
    let is_write = matches!(&request, Request::Write { .. });
    match handle_request_inner(api, request) {
        Ok(payload) => {
            let stage = if is_write && payload.first() == Some(&1) {
                Some(WriteStage::AfterWrite)
            } else {
                None
            };
            Response {
                ok: true,
                stage,
                payload,
            }
        }
        Err(error) => Response {
            ok: false,
            stage: error.stage,
            payload: error.to_string().into_bytes(),
        },
    }
}

/// Reads one frame, runs the request, and writes one response frame.
pub fn serve_one(api: &mut dyn SteamApi, input: &mut impl Read, output: &mut impl Write) -> Result<(), ProtocolError> {
    let body = read_frame(input)?;
    let request = decode_body(body)?;
    let response = handle_request(api, request);
    write_response(output, &response)
}

fn write_response(output: &mut impl Write, response: &Response) -> Result<(), ProtocolError> {
    let body_length = response.payload.len().saturating_add(3);
    if body_length > MAX_FRAME_BYTES {
        return Err(ProtocolError::new("worker response exceeds the frame limit"));
    }
    let length = u32::try_from(body_length).map_err(|_| ProtocolError::new("worker response length overflows u32"))?;
    output
        .write_all(&length.to_le_bytes())
        .map_err(|error| ProtocolError::new(error.to_string()))?;
    output
        .write_all(&[PROTOCOL_VERSION, u8::from(response.ok), encode_stage(response.stage)])
        .map_err(|error| ProtocolError::new(error.to_string()))?;
    output
        .write_all(&response.payload)
        .map_err(|error| ProtocolError::new(error.to_string()))
}

fn encode_stage(stage: Option<WriteStage>) -> u8 {
    match stage {
        None => 0,
        Some(WriteStage::BeforeWrite) => 1,
        Some(WriteStage::WriteRejected) => 2,
        Some(WriteStage::AfterWrite) => 3,
    }
}

fn decode_stage(value: u8) -> Result<Option<WriteStage>, ProtocolError> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(WriteStage::BeforeWrite)),
        2 => Ok(Some(WriteStage::WriteRejected)),
        3 => Ok(Some(WriteStage::AfterWrite)),
        _ => Err(ProtocolError::new("unknown Steam write stage")),
    }
}

fn finish_frame(mut frame: Vec<u8>, label: &str) -> Result<Vec<u8>, ProtocolError> {
    let body_length = frame
        .len()
        .checked_sub(4)
        .ok_or_else(|| ProtocolError::new("internal worker frame has no prefix"))?;
    if body_length == 0 || body_length > MAX_FRAME_BYTES {
        return Err(ProtocolError::new(format!("{label} exceeds the frame limit")));
    }
    let length = u32::try_from(body_length).map_err(|_| ProtocolError::new("worker frame length overflows u32"))?;
    let prefix = frame
        .get_mut(..4)
        .ok_or_else(|| ProtocolError::new("internal worker frame prefix range"))?;
    prefix.copy_from_slice(&length.to_le_bytes());
    Ok(frame)
}

/// Protocol parse/encode error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError {
    /// Explanation of the rejected frame.
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

fn handle_request_inner(api: &mut dyn SteamApi, request: Request) -> Result<Vec<u8>, SteamError> {
    match request {
        Request::List { app_id } => {
            api.initialize(app_id)?;
            api.run_callbacks()?;
            encode_cloud_files(api.list_files()?).map_err(|error| SteamError::new(error.message))
        }
        Request::Read { app_id, remote_name } => {
            api.initialize(app_id)?;
            api.run_callbacks()?;
            let bytes = api.read_file(&remote_name)?;
            if bytes.len() > crate::cloud::MAX_CLOUD_FILE_BYTES {
                return Err(SteamError::new("downloaded cloud file exceeds the configured limit"));
            }
            Ok(bytes)
        }
        Request::Write {
            app_id,
            remote_name,
            expected_source_sha256,
            artifact_directory,
            output,
        } => {
            api.initialize(app_id)?;
            let prepared = PreparedEdit {
                source_sha256: expected_source_sha256,
                output_sha256: sse_codecs::sha256::sha256(&output),
                output,
            };
            let mut verifier = UnavailableSaveFormatVerifier;
            let receipt = SteamCloudWriteTransaction::upload(
                api,
                &mut verifier,
                app_id,
                &remote_name,
                &prepared,
                &artifact_directory,
                true,
            )?;
            let mut payload = vec![match receipt.status {
                WriteStatus::Verified => 0,
                WriteStatus::Uncertain => 1,
            }];
            payload.extend_from_slice(&receipt.output_sha256);
            put_string(&mut payload, &receipt.backup_path.to_string_lossy())
                .map_err(|error| SteamError::new(error.message))?;
            put_string(&mut payload, &receipt.recovery_path.to_string_lossy())
                .map_err(|error| SteamError::new(error.message))?;
            put_string(&mut payload, receipt.reason.as_deref().unwrap_or(""))
                .map_err(|error| SteamError::new(error.message))?;
            Ok(payload)
        }
        Request::ListAchievements { app_id } => {
            api.initialize(app_id)?;
            api.run_callbacks()?;
            encode_achievements(api.achievements()?).map_err(|error| SteamError::new(error.message))
        }
        Request::SetAchievement {
            app_id,
            name,
            confirmed,
        } => {
            crate::achievements::AchievementService.set(
                api,
                app_id,
                &name,
                if confirmed {
                    AchievementConfirmation::Confirmed
                } else {
                    AchievementConfirmation::Declined
                },
            )?;
            Ok(Vec::new())
        }
        Request::ClearAchievement {
            app_id,
            name,
            confirmed,
        } => {
            crate::achievements::AchievementService.clear(
                api,
                app_id,
                &name,
                if confirmed {
                    AchievementConfirmation::Confirmed
                } else {
                    AchievementConfirmation::Declined
                },
            )?;
            Ok(Vec::new())
        }
    }
}

fn decode_body(mut body: Vec<u8>) -> Result<Request, ProtocolError> {
    let (mut request, output_range) = {
        let mut cursor = Cursor::new(&body);
        if cursor.u8()? != PROTOCOL_VERSION {
            return Err(ProtocolError::new("unsupported Steam worker protocol version"));
        }
        let opcode = cursor.u8()?;
        let app_id = cursor.u32()?;
        let mut output_range = None;
        let request = match opcode {
            1 => Request::List { app_id },
            2 => Request::Read {
                app_id,
                remote_name: cursor.string()?,
            },
            3 => {
                let remote_name = cursor.string()?;
                let expected_source_sha256 = cursor.array32()?;
                let artifact_directory = PathBuf::from(cursor.string()?);
                output_range = Some(cursor.byte_range()?);
                Request::Write {
                    app_id,
                    remote_name,
                    expected_source_sha256,
                    artifact_directory,
                    output: Vec::new(),
                }
            }
            4 => Request::ListAchievements { app_id },
            5 => Request::SetAchievement {
                app_id,
                name: cursor.string()?,
                confirmed: cursor.boolean()?,
            },
            6 => Request::ClearAchievement {
                app_id,
                name: cursor.string()?,
                confirmed: cursor.boolean()?,
            },
            _ => return Err(ProtocolError::new("unknown Steam worker opcode")),
        };
        if cursor.remaining() != 0 {
            return Err(ProtocolError::new("trailing bytes in Steam worker request"));
        }
        (request, output_range)
    };
    if let (Request::Write { output, .. }, Some(range)) = (&mut request, output_range) {
        let output_length = range
            .end
            .checked_sub(range.start)
            .ok_or_else(|| ProtocolError::new("worker payload range is invalid"))?;
        body.copy_within(range, 0);
        body.truncate(output_length);
        *output = body;
    }
    Ok(request)
}

fn encode_cloud_files(files: Vec<CloudFile>) -> Result<Vec<u8>, ProtocolError> {
    let mut output = Vec::new();
    let count = u32::try_from(files.len()).map_err(|_| ProtocolError::new("too many cloud files"))?;
    put_u32(&mut output, count);
    for file in files {
        put_string(&mut output, &file.name)?;
        put_u64(&mut output, file.size);
        output.extend_from_slice(&file.timestamp.to_le_bytes());
    }
    Ok(output)
}

fn encode_achievements(entries: Vec<Achievement>) -> Result<Vec<u8>, ProtocolError> {
    let mut output = Vec::new();
    let count = u32::try_from(entries.len()).map_err(|_| ProtocolError::new("too many achievements"))?;
    put_u32(&mut output, count);
    for entry in entries {
        put_string(&mut output, &entry.name)?;
        put_string(&mut output, &entry.display_name)?;
        put_string(&mut output, &entry.description)?;
        output.push(u8::from(entry.hidden));
        output.push(u8::from(entry.achieved));
        put_u32(&mut output, entry.unlock_time);
    }
    Ok(output)
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_string(output: &mut Vec<u8>, value: &str) -> Result<(), ProtocolError> {
    let length = u16::try_from(value.len()).map_err(|_| ProtocolError::new("worker string exceeds 65535 bytes"))?;
    output.extend_from_slice(&length.to_le_bytes());
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_bytes(output: &mut Vec<u8>, value: &[u8]) -> Result<(), ProtocolError> {
    let length = u32::try_from(value.len()).map_err(|_| ProtocolError::new("worker binary payload exceeds u32"))?;
    put_u32(output, length);
    output.extend_from_slice(value);
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| ProtocolError::new("worker cursor overflow"))?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| ProtocolError::new("truncated Steam worker request"))?;
        self.position = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, ProtocolError> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| ProtocolError::new("truncated Steam worker request"))
    }

    fn boolean(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtocolError::new("invalid Steam worker boolean")),
        }
    }

    fn u32(&mut self) -> Result<u32, ProtocolError> {
        let mut bytes = [0_u8; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    fn array32(&mut self) -> Result<[u8; 32], ProtocolError> {
        let mut bytes = [0_u8; 32];
        bytes.copy_from_slice(self.take(32)?);
        Ok(bytes)
    }

    fn string(&mut self) -> Result<String, ProtocolError> {
        let mut bytes = [0_u8; 2];
        bytes.copy_from_slice(self.take(2)?);
        let length = usize::from(u16::from_le_bytes(bytes));
        String::from_utf8(self.take(length)?.to_vec())
            .map_err(|_| ProtocolError::new("Steam worker string is not UTF-8"))
    }

    fn byte_range(&mut self) -> Result<Range<usize>, ProtocolError> {
        let length =
            usize::try_from(self.u32()?).map_err(|_| ProtocolError::new("worker byte length is not representable"))?;
        if length > crate::cloud::MAX_CLOUD_FILE_BYTES {
            return Err(ProtocolError::new("cloud payload exceeds the configured limit"));
        }
        let start = self.position;
        let _ = self.take(length)?;
        Ok(start..self.position)
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }
}
