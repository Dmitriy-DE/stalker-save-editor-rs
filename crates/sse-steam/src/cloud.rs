//! Fresh-hash guarded Steam and Auto-Cloud write transactions.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sse_codecs::sha256;

use crate::api::{SteamApi, SteamError, WriteFailure};

/// Maximum bytes accepted in a cloud file frame.
pub const MAX_CLOUD_FILE_BYTES: usize = 64 * 1024 * 1024;

/// Data prepared against a particular source cloud image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedEdit {
    /// SHA-256 of the source image from the read/analysis step.
    pub source_sha256: [u8; 32],
    /// Edited output bytes.
    pub output: Vec<u8>,
    /// SHA-256 of the edited bytes.
    pub output_sha256: [u8; 32],
}

impl PreparedEdit {
    /// Captures source and output hashes while taking one owned output buffer.
    #[must_use]
    pub fn new(source: &[u8], output: &[u8]) -> Self {
        Self {
            source_sha256: sha256::sha256(source),
            output: output.to_vec(),
            output_sha256: sha256::sha256(output),
        }
    }
}

/// Result of a Steam or Auto-Cloud write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    /// Final status.
    pub status: WriteStatus,
    /// Backup containing the source bytes.
    pub backup_path: PathBuf,
    /// Recovery copy containing intended output bytes.
    pub recovery_path: PathBuf,
    /// SHA-256 of the intended output.
    pub output_sha256: [u8; 32],
    /// Explanation when the write status is uncertain.
    pub reason: Option<String>,
}

/// Whether a write was verified or may have reached the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStatus {
    /// The result was read back and matches the requested bytes.
    Verified,
    /// A write was dispatched but completion could not be verified.
    Uncertain,
}

mod verifier_sealed {
    pub trait Sealed {}
}

/// Release-aware verifier required before any cloud write.
///
/// This trait is sealed so external callers cannot supply a verifier that accepts arbitrary
/// bytes. The only implementation currently available is fail-closed until the format readers
/// are integrated.
#[allow(private_bounds)]
pub trait SaveFormatVerifier: verifier_sealed::Sealed {
    /// Rejects bytes that are not a save for `app_id` and `remote_name`.
    fn verify(&mut self, app_id: u32, remote_name: &str, bytes: &[u8]) -> Result<(), SteamError>;
}

/// Fail-closed verifier used until C1/C3 reader crates are integrated.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSaveFormatVerifier;

impl verifier_sealed::Sealed for UnavailableSaveFormatVerifier {}

impl SaveFormatVerifier for UnavailableSaveFormatVerifier {
    fn verify(&mut self, _app_id: u32, _remote_name: &str, _bytes: &[u8]) -> Result<(), SteamError> {
        Err(SteamError::new(
            "save format verification awaits sse-xray/sse-s2 integration",
        ))
    }
}

/// Steam Remote Storage write transaction.
pub struct SteamCloudWriteTransaction;

impl SteamCloudWriteTransaction {
    /// Executes one fresh-hash, backup, write, persisted-check and read-back transaction.
    ///
    /// `write_enabled` must come from an explicit capability decision. An uncertain write is never retried.
    pub fn upload(
        api: &mut dyn SteamApi,
        verifier: &mut dyn SaveFormatVerifier,
        app_id: u32,
        remote_name: &str,
        prepared: &PreparedEdit,
        artifact_directory: &Path,
        write_enabled: bool,
    ) -> Result<WriteReceipt, SteamError> {
        if !write_enabled {
            return Err(SteamError::new("cloud writing is disabled before I/O"));
        }
        validate_size(prepared.output.len())?;
        if sha256::sha256(&prepared.output) != prepared.output_sha256 {
            return Err(SteamError::new("prepared output hash does not match its bytes"));
        }
        let remote_name = validate_remote_save_path(app_id, remote_name)?;
        let fresh = api.read_file(&remote_name)?;
        if sha256::sha256(&fresh) != prepared.source_sha256 {
            return Err(SteamError::new("cloud source changed after analysis"));
        }
        validate_size(fresh.len())?;
        verifier.verify(app_id, &remote_name, &fresh)?;
        verifier.verify(app_id, &remote_name, &prepared.output)?;
        let (backup_path, recovery_path) = write_artifacts(artifact_directory, &remote_name, &fresh, &prepared.output)?;

        match api.write_file(&remote_name, &prepared.output) {
            Err(WriteFailure::NotAttempted(error)) => {
                return Err(SteamError::new(format!("cloud write was not attempted: {error}")));
            }
            Err(WriteFailure::Uncertain(error)) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    prepared.output_sha256,
                    format!("Steam write result is uncertain: {error}"),
                ));
            }
            Ok(()) => {}
        }

        if let Err(error) = api.run_callbacks() {
            return Ok(uncertain_receipt(
                backup_path,
                recovery_path,
                prepared.output_sha256,
                format!("Steam callback dispatch failed after write: {error}"),
            ));
        }
        match api.file_persisted(&remote_name) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    prepared.output_sha256,
                    "Steam has not confirmed that the file is persisted".to_owned(),
                ));
            }
            Err(error) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    prepared.output_sha256,
                    format!("Steam persisted check failed: {error}"),
                ));
            }
        }
        let readback = match api.read_file(&remote_name) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    prepared.output_sha256,
                    format!("Steam read-back failed: {error}"),
                ));
            }
        };
        if readback.len() > MAX_CLOUD_FILE_BYTES {
            return Ok(uncertain_receipt(
                backup_path,
                recovery_path,
                prepared.output_sha256,
                "Steam read-back exceeds the configured size limit".to_owned(),
            ));
        }
        if sha256::sha256(&readback) != prepared.output_sha256 {
            return Ok(uncertain_receipt(
                backup_path,
                recovery_path,
                prepared.output_sha256,
                "Steam read-back hash does not match the output".to_owned(),
            ));
        }
        Ok(WriteReceipt {
            status: WriteStatus::Verified,
            backup_path,
            recovery_path,
            output_sha256: prepared.output_sha256,
            reason: None,
        })
    }
}

/// Checks the C# release-specific Remote Storage path allow-list.
pub fn validate_remote_save_path(app_id: u32, remote_name: &str) -> Result<String, SteamError> {
    let normalized = remote_name.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').collect();
    if normalized.trim().is_empty()
        || segments.iter().any(|segment| {
            segment.is_empty()
                || *segment == "."
                || *segment == ".."
                || segment.contains(':')
                || segment.chars().any(char::is_control)
        })
    {
        return Err(SteamError::new("Steam save path is invalid"));
    }
    let (prefix, extensions): (&str, &[&str]) = match app_id {
        4_500 | 20_510 => ("_appdata_/savedgames/", &[".sav"]),
        41_700 => ("_appdata_/savedgames/", &[".sav", ".scop"]),
        2_427_410 => ("STALKER Shadow of Chornobyl - EE/STEAM/savedgames/", &[".sav"]),
        2_427_420 => ("STALKER Clear Sky - EE/STEAM/savedgames/", &[".sav", ".scop", ".scs"]),
        2_427_430 => (
            "STALKER Call of Prypiat - EE/STEAM/savedgames/",
            &[".sav", ".scop", ".scs"],
        ),
        _ => return Err(SteamError::new("Steam save writes are unsupported for this app id")),
    };
    if !normalized
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    {
        return Err(SteamError::new(
            "Steam save path is outside the selected release prefix",
        ));
    }
    let leaf = normalized
        .get(prefix.len()..)
        .ok_or_else(|| SteamError::new("Steam save path prefix range"))?;
    if leaf.is_empty()
        || leaf.contains('/')
        || !extensions
            .iter()
            .any(|extension| leaf.to_ascii_lowercase().ends_with(extension))
    {
        return Err(SteamError::new("Steam save filename or extension is not allowed"));
    }
    Ok(normalized)
}

/// Writes an S.T.A.L.K.E.R. 2 Auto-Cloud file through the same guarded local transaction.
pub fn write_auto_cloud(
    root: &Path,
    remote_name: &str,
    prepared: &PreparedEdit,
    artifact_directory: &Path,
    write_enabled: bool,
    verifier: &mut dyn SaveFormatVerifier,
) -> Result<WriteReceipt, SteamError> {
    let _ = (root, remote_name, prepared, artifact_directory, write_enabled, verifier);
    Err(SteamError::new(
        "Auto-Cloud writes are disabled until descriptor-relative path operations are available",
    ))
}

fn validate_size(size: usize) -> Result<(), SteamError> {
    if size == 0 || size > MAX_CLOUD_FILE_BYTES {
        return Err(SteamError::new("cloud file size is outside the supported range"));
    }
    Ok(())
}

fn write_artifacts(
    directory: &Path,
    remote_name: &str,
    source: &[u8],
    output: &[u8],
) -> Result<(PathBuf, PathBuf), SteamError> {
    fs::create_dir_all(directory).map_err(|error| SteamError::new(error.to_string()))?;
    let first = unique_artifact_path(directory, &format!("{}-original", safe_stem(remote_name)))?;
    let second = unique_artifact_path(directory, &format!("{}-edited", safe_stem(remote_name)))?;
    write_exclusive(&first, source)?;
    if let Err(error) = write_exclusive(&second, output) {
        let _ = fs::remove_file(&first);
        return Err(error);
    }
    Ok((first, second))
}

fn unique_artifact_path(directory: &Path, stem: &str) -> Result<PathBuf, SteamError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    Ok(directory.join(format!("{stem}-{}-{timestamp}.sav", std::process::id())))
}

fn safe_stem(remote_name: &str) -> String {
    remote_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("cloud-save")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .take(80)
        .collect()
}

fn write_exclusive(path: &Path, data: &[u8]) -> Result<(), SteamError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| SteamError::new(error.to_string()))?;
    file.write_all(data)
        .map_err(|error| SteamError::new(error.to_string()))?;
    file.sync_all().map_err(|error| SteamError::new(error.to_string()))
}

fn uncertain_receipt(
    backup_path: PathBuf,
    recovery_path: PathBuf,
    output_sha256: [u8; 32],
    reason: String,
) -> WriteReceipt {
    WriteReceipt {
        status: WriteStatus::Uncertain,
        backup_path,
        recovery_path,
        output_sha256,
        reason: Some(reason),
    }
}
