//! Fresh-hash guarded Steam Cloud write transactions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sse_codecs::sha256;

use crate::api::{SteamApi, SteamError, WriteFailure, WriteStage};

/// Maximum bytes accepted in a cloud file frame.
pub const MAX_CLOUD_FILE_BYTES: usize = 64 * 1024 * 1024;
const PERSISTED_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PERSISTED_TIMEOUT: Duration = Duration::from_secs(120);

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
        Self::from_owned(source, output.to_vec())
    }

    /// Captures hashes while taking ownership of an already allocated output image.
    #[must_use]
    pub fn from_owned(source: &[u8], output: Vec<u8>) -> Self {
        Self {
            source_sha256: sha256::sha256(source),
            output_sha256: sha256::sha256(&output),
            output,
        }
    }

    /// Binds the output to a source hash captured before the user's write confirmation.
    #[must_use]
    pub fn from_source_sha256(source_sha256: [u8; 32], output: Vec<u8>) -> Self {
        Self {
            source_sha256,
            output_sha256: sha256::sha256(&output),
            output,
        }
    }
}

/// Result of a Steam Cloud write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WriteReceipt {
    /// Final status.
    pub status: WriteStatus,
    /// Write phase for an uncertain result; verified writes have no failure stage.
    pub stage: Option<WriteStage>,
    /// Backup containing the source bytes.
    pub backup_path: PathBuf,
    /// Recovery copy containing intended output bytes.
    pub recovery_path: PathBuf,
    /// Journal that makes the recovery copies visible in the application's backup list.
    pub journal_path: PathBuf,
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
/// bytes. Callers that cannot establish a supported release's format must use a fail-closed
/// verifier.
#[allow(private_bounds)]
pub trait SaveFormatVerifier: verifier_sealed::Sealed {
    /// Rejects bytes that are not a save for `app_id` and `remote_name`.
    fn verify(&mut self, app_id: u32, remote_name: &str, bytes: &[u8]) -> Result<(), SteamError>;
}

/// Fail-closed verifier for callers that cannot establish a supported save format.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnavailableSaveFormatVerifier;

impl verifier_sealed::Sealed for UnavailableSaveFormatVerifier {}

impl SaveFormatVerifier for UnavailableSaveFormatVerifier {
    fn verify(&mut self, _app_id: u32, _remote_name: &str, _bytes: &[u8]) -> Result<(), SteamError> {
        Err(SteamError::new(
            "save format verification is unavailable for this operation",
        ))
    }
}

/// Release-aware verifier for the six official X-Ray trilogy formats.
#[derive(Debug, Default)]
pub struct XRaySaveFormatVerifier {
    verified_images: HashSet<(u32, [u8; 32])>,
}

impl verifier_sealed::Sealed for XRaySaveFormatVerifier {}

impl SaveFormatVerifier for XRaySaveFormatVerifier {
    fn verify(&mut self, app_id: u32, _remote_name: &str, bytes: &[u8]) -> Result<(), SteamError> {
        let digest = sha256::sha256(bytes);
        if self.verified_images.contains(&(app_id, digest)) {
            return Ok(());
        }
        let expected = match app_id {
            4_500 => sse_xray::Format::Soc,
            20_510 => sse_xray::Format::Cs,
            41_700 => sse_xray::Format::Cop,
            2_427_410 => sse_xray::Format::SocEe,
            2_427_420 => sse_xray::Format::CsEe,
            2_427_430 => sse_xray::Format::CopEe,
            _ => {
                return Err(SteamError::new(
                    "RemoteStorage writes are limited to official X-Ray trilogy releases.",
                ));
            }
        };
        let save = sse_xray::Save::read(bytes)
            .map_err(|_| SteamError::new("RemoteStorage write payload is not a save for the selected release."))?;
        if save.format() != expected {
            return Err(SteamError::new(
                "RemoteStorage write payload is not a save for the selected release.",
            ));
        }
        self.verified_images.insert((app_id, digest));
        Ok(())
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
            return Err(SteamError::new("cloud writing is disabled before I/O").at_stage(WriteStage::BeforeWrite));
        }
        validate_size(prepared.output.len()).map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        if sha256::sha256(&prepared.output) != prepared.output_sha256 {
            return Err(
                SteamError::new("prepared output hash does not match its bytes").at_stage(WriteStage::BeforeWrite)
            );
        }
        let remote_name =
            validate_remote_save_path(app_id, remote_name).map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        let fresh = api
            .read_file(&remote_name)
            .map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        if sha256::sha256(&fresh) != prepared.source_sha256 {
            return Err(SteamError::new("cloud source changed after analysis").at_stage(WriteStage::BeforeWrite));
        }
        validate_size(fresh.len()).map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        verifier
            .verify(app_id, &remote_name, &fresh)
            .map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        verifier
            .verify(app_id, &remote_name, &prepared.output)
            .map_err(|error| error.at_stage(WriteStage::BeforeWrite))?;
        let artifacts = sse_storage::transaction::write_cloud_recovery_artifacts(
            artifact_directory,
            app_id,
            &remote_name,
            &fresh,
            &prepared.output,
        )
        .map_err(|error| SteamError::new(error.to_string()).at_stage(WriteStage::BeforeWrite))?;
        let backup_path = artifacts.backup_path;
        let recovery_path = artifacts.recovery_path;
        let journal_path = artifacts.journal_path;
        drop(fresh);

        match api.write_file(&remote_name, &prepared.output) {
            Err(WriteFailure::NotAttempted(error)) => {
                return Err(SteamError::new(format!(
                    "cloud write was not attempted: {error}; recovery artifacts: {}, {}, {}",
                    backup_path.display(),
                    recovery_path.display(),
                    journal_path.display(),
                ))
                .at_stage(WriteStage::BeforeWrite));
            }
            Err(WriteFailure::Rejected(error)) => {
                return Err(SteamError::new(format!(
                    "Steam RemoteStorage rejected the write: {error}; recovery artifacts: {}, {}, {}",
                    backup_path.display(),
                    recovery_path.display(),
                    journal_path.display(),
                ))
                .at_stage(WriteStage::WriteRejected));
            }
            Err(WriteFailure::Uncertain(error)) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    journal_path,
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
                journal_path,
                prepared.output_sha256,
                format!("Steam callback dispatch failed after write: {error}"),
            ));
        }
        match wait_until_persisted(api, &remote_name) {
            Ok(true) => {}
            Ok(false) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    journal_path,
                    prepared.output_sha256,
                    "Steam has not confirmed that the file is persisted".to_owned(),
                ));
            }
            Err(error) => {
                return Ok(uncertain_receipt(
                    backup_path,
                    recovery_path,
                    journal_path,
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
                    journal_path,
                    prepared.output_sha256,
                    format!("Steam read-back failed: {error}"),
                ));
            }
        };
        if readback.len() > MAX_CLOUD_FILE_BYTES {
            return Ok(uncertain_receipt(
                backup_path,
                recovery_path,
                journal_path,
                prepared.output_sha256,
                "Steam read-back exceeds the configured size limit".to_owned(),
            ));
        }
        if sha256::sha256(&readback) != prepared.output_sha256 {
            return Ok(uncertain_receipt(
                backup_path,
                recovery_path,
                journal_path,
                prepared.output_sha256,
                "Steam read-back hash does not match the output".to_owned(),
            ));
        }
        Ok(WriteReceipt {
            status: WriteStatus::Verified,
            stage: None,
            backup_path,
            recovery_path,
            journal_path,
            output_sha256: prepared.output_sha256,
            reason: None,
        })
    }
}

fn wait_until_persisted(api: &mut dyn SteamApi, remote_name: &str) -> Result<bool, SteamError> {
    let started = Instant::now();
    loop {
        if api.file_persisted(remote_name)? {
            return Ok(true);
        }
        let elapsed = started.elapsed();
        if elapsed >= PERSISTED_TIMEOUT {
            return Ok(false);
        }
        std::thread::sleep(PERSISTED_POLL_INTERVAL.min(PERSISTED_TIMEOUT.saturating_sub(elapsed)));
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

fn validate_size(size: usize) -> Result<(), SteamError> {
    if size == 0 || size > MAX_CLOUD_FILE_BYTES {
        return Err(SteamError::new("cloud file size is outside the supported range"));
    }
    Ok(())
}

fn uncertain_receipt(
    backup_path: PathBuf,
    recovery_path: PathBuf,
    journal_path: PathBuf,
    output_sha256: [u8; 32],
    reason: String,
) -> WriteReceipt {
    WriteReceipt {
        status: WriteStatus::Uncertain,
        stage: Some(WriteStage::AfterWrite),
        backup_path,
        recovery_path,
        journal_path,
        output_sha256,
        reason: Some(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        verifier_sealed, PreparedEdit, SaveFormatVerifier, SteamCloudWriteTransaction, SteamError, WriteStatus,
    };
    use crate::api::{ScriptedSteamApi, WriteBehavior, WriteStage};
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct AcceptFixtureBytes;

    impl verifier_sealed::Sealed for AcceptFixtureBytes {}

    impl SaveFormatVerifier for AcceptFixtureBytes {
        fn verify(&mut self, _app_id: u32, _remote_name: &str, _bytes: &[u8]) -> Result<(), SteamError> {
            Ok(())
        }
    }

    fn artifacts_directory() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!("sse-steam-stage-{}-{nanos}-{sequence}", std::process::id()));
        assert!(std::fs::create_dir_all(&directory).is_ok());
        directory
    }

    fn upload(api: &mut ScriptedSteamApi, artifacts: &Path) -> Result<super::WriteReceipt, SteamError> {
        let source = b"source fixture";
        let prepared = PreparedEdit::new(source, b"edited fixture");
        let mut verifier = AcceptFixtureBytes;
        SteamCloudWriteTransaction::upload(
            api,
            &mut verifier,
            4500,
            "_appdata_/savedgames/slot.sav",
            &prepared,
            artifacts,
            true,
        )
    }

    #[test]
    fn explicit_filewrite_rejection_is_classified_before_uncertain_results() {
        let artifacts = artifacts_directory();
        let mut api = ScriptedSteamApi::default();
        api.files
            .insert("_appdata_/savedgames/slot.sav".into(), b"source fixture".to_vec());
        api.write_behavior = WriteBehavior::RejectWrite("FileWrite returned false".to_owned());

        let result = upload(&mut api, &artifacts);
        assert!(result.is_err_and(|error| {
            error.stage == Some(WriteStage::WriteRejected)
                && error.message.contains("recovery artifacts")
                && error.message.contains("_ORIGINAL.json")
        }));
        assert_eq!(api.write_count, 0);
        assert!(std::fs::remove_dir_all(artifacts).is_ok());
    }

    #[test]
    fn upload_refuses_a_cloud_version_changed_after_confirmation() {
        let artifacts = artifacts_directory();
        let confirmed_source = b"cloud version shown for confirmation";
        let prepared =
            PreparedEdit::from_source_sha256(sse_codecs::sha256::sha256(confirmed_source), b"edited fixture".to_vec());
        let mut api = ScriptedSteamApi::default();
        api.files.insert(
            "_appdata_/savedgames/slot.sav".into(),
            b"newer cloud version from another device".to_vec(),
        );
        let mut verifier = AcceptFixtureBytes;

        let result = SteamCloudWriteTransaction::upload(
            &mut api,
            &mut verifier,
            4500,
            "_appdata_/savedgames/slot.sav",
            &prepared,
            &artifacts,
            true,
        );

        assert!(result.is_err_and(|error| {
            error.stage == Some(WriteStage::BeforeWrite) && error.message.contains("cloud source changed")
        }));
        assert_eq!(api.write_count, 0);
        assert!(std::fs::read_dir(&artifacts).is_ok_and(|mut files| files.next().is_none()));
        assert!(std::fs::remove_dir_all(artifacts).is_ok());
    }

    #[test]
    fn lost_write_response_keeps_an_after_write_stage() {
        let artifacts = artifacts_directory();
        let mut api = ScriptedSteamApi::default();
        api.files
            .insert("_appdata_/savedgames/slot.sav".into(), b"source fixture".to_vec());
        api.write_behavior = WriteBehavior::FailAfterWrite("response lost".to_owned());

        let result = upload(&mut api, &artifacts);
        assert!(result.is_ok_and(|receipt| {
            receipt.status == WriteStatus::Uncertain && receipt.stage == Some(WriteStage::AfterWrite)
        }));
        assert_eq!(api.write_count, 1);
        assert!(std::fs::remove_dir_all(artifacts).is_ok());
    }
}
