//! High-level update service coordinating check, download, and install.

use crate::detector::UpdateInstallation;
use crate::fetch::{download_artifact, Fetch};
use crate::installer::{install_artifact, ProcessRunner, UpdateInstallResult};
use crate::manifest::{compare_versions, UpdateArtifact, UpdateManifest, UpdateState, MAXIMUM_MANIFEST_BYTES};
use crate::signature::{verify_signature, MAXIMUM_SIGNATURE_BYTES};
use sse_core::{Error, Result};
use std::path::{Path, PathBuf};

/// Default live update manifest endpoint.
pub const DEFAULT_MANIFEST_URL: &str = "https://save-editor-downloads.save-editor.workers.dev/latest.json";

/// Check result containing detected status and manifest metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateCheckResult {
    /// State of the check.
    pub state: UpdateState,
    /// Parsed manifest, if check was successful.
    pub manifest: Option<UpdateManifest>,
    /// Selected artifact matching the current installation, if available.
    pub artifact: Option<UpdateArtifact>,
    /// Diagnostic error message, if check failed.
    pub error: Option<String>,
}

/// The coordinating service for self-updates.
#[derive(Clone, Debug)]
pub struct UpdateService {
    current_version: String,
    installation: UpdateInstallation,
    manifest_url: String,
    public_key_pem: Option<String>,
}

enum CheckFailure {
    Unavailable(Error),
    Invalid(Error),
}

impl UpdateService {
    /// Creates a new update service with the default manifest URL and embedded public key.
    #[must_use]
    pub fn new(current_version: impl Into<String>, installation: UpdateInstallation) -> Self {
        Self {
            current_version: current_version.into(),
            installation,
            manifest_url: DEFAULT_MANIFEST_URL.to_string(),
            public_key_pem: None,
        }
    }

    /// Sets a custom manifest URL (e.g. for testing).
    #[must_use]
    pub fn with_manifest_url(mut self, url: impl Into<String>) -> Self {
        self.manifest_url = url.into();
        self
    }

    /// Returns the current installed version.
    #[must_use]
    pub fn current_version(&self) -> &str {
        &self.current_version
    }

    /// Returns the target installation description.
    #[must_use]
    pub fn installation(&self) -> &UpdateInstallation {
        &self.installation
    }

    /// Performs an update check against the configured manifest URL.
    pub fn check(&self, fetch: &mut dyn Fetch) -> UpdateCheckResult {
        match self.check_inner(fetch) {
            Ok((state, manifest, artifact)) => UpdateCheckResult {
                state,
                manifest: Some(manifest),
                artifact: Some(artifact),
                error: None,
            },
            Err(CheckFailure::Unavailable(error)) => failed_check(UpdateState::Unavailable, error),
            Err(CheckFailure::Invalid(error)) => failed_check(UpdateState::Invalid, error),
        }
    }

    fn check_inner(
        &self,
        fetch: &mut dyn Fetch,
    ) -> core::result::Result<(UpdateState, UpdateManifest, UpdateArtifact), CheckFailure> {
        // 1. Fetch latest.json
        let mut manifest_bytes = Vec::new();
        let mut manifest_too_large = false;
        let manifest_response = fetch.get(&self.manifest_url, 0, &mut |chunk| {
            if manifest_bytes.len().saturating_add(chunk.len()) > MAXIMUM_MANIFEST_BYTES {
                manifest_too_large = true;
                return false;
            }
            manifest_bytes.extend_from_slice(chunk);
            true
        });
        let resp = match manifest_response {
            Ok(response) => response,
            Err(_error) if manifest_too_large => {
                return Err(CheckFailure::Invalid(Error::Refused(
                    "Manifest payload exceeds maximum permitted size".to_string(),
                )));
            }
            Err(error) => return Err(CheckFailure::Unavailable(error)),
        };
        if manifest_too_large {
            return Err(CheckFailure::Invalid(Error::Refused(
                "Manifest payload exceeds maximum permitted size".to_string(),
            )));
        }

        if resp.status_code != 200 {
            return Err(CheckFailure::Unavailable(Error::Refused(format!(
                "Manifest server returned HTTP {}",
                resp.status_code
            ))));
        }

        // 2. Fetch latest.json.sig
        let sig_url = format!("{}.sig", self.manifest_url);
        let mut sig_bytes = Vec::new();
        let mut signature_too_large = false;
        let signature_response = fetch.get(&sig_url, 0, &mut |chunk| {
            if sig_bytes.len().saturating_add(chunk.len()) > MAXIMUM_SIGNATURE_BYTES {
                signature_too_large = true;
                return false;
            }
            sig_bytes.extend_from_slice(chunk);
            true
        });
        let sig_resp = match signature_response {
            Ok(response) => response,
            Err(_error) if signature_too_large => {
                return Err(CheckFailure::Invalid(Error::Refused(
                    "Signature file exceeds maximum permitted size".to_string(),
                )));
            }
            Err(error) => return Err(CheckFailure::Unavailable(error)),
        };
        if signature_too_large {
            return Err(CheckFailure::Invalid(Error::Refused(
                "Signature file exceeds maximum permitted size".to_string(),
            )));
        }

        if sig_resp.status_code != 200 {
            return Err(CheckFailure::Unavailable(Error::Refused(format!(
                "Signature server returned HTTP {}",
                sig_resp.status_code
            ))));
        }

        // 3. Verify signature
        verify_signature(&manifest_bytes, &sig_bytes, self.public_key_pem.as_deref()).map_err(CheckFailure::Invalid)?;

        // 4. Parse manifest
        let manifest = UpdateManifest::parse(&manifest_bytes).map_err(CheckFailure::Invalid)?;

        // 5. Select artifact for current installation
        let artifact_kind = crate::platform::artifact_kind_for(&self.installation.target, &self.installation.kind);
        let artifact = manifest
            .select(
                &self.installation.target,
                &self.installation.architecture,
                artifact_kind,
            )
            .map_err(CheckFailure::Unavailable)?
            .clone();

        // 6. Compare versions with downgrade protection
        let state = compare_versions(&self.current_version, &manifest.version).map_err(CheckFailure::Invalid)?;

        Ok((state, manifest, artifact))
    }

    /// Downloads the artifact into `destination`, verifying hash and size in one pass.
    ///
    /// # Errors
    /// Returns an error on network or verification failure.
    pub fn download(
        &self,
        fetch: &mut dyn Fetch,
        artifact: &UpdateArtifact,
        destination: &Path,
        progress: Option<&mut dyn FnMut(u64, u64)>,
    ) -> Result<PathBuf> {
        download_artifact(fetch, artifact, destination, progress)
    }

    /// Hands off the downloaded artifact to the system installer.
    ///
    /// # Errors
    /// Returns an error if verification fails or execution cannot proceed.
    pub fn install(
        &self,
        artifact: &UpdateArtifact,
        verified_archive: &Path,
        runner: &mut dyn ProcessRunner,
    ) -> Result<UpdateInstallResult> {
        install_artifact(artifact, verified_archive, &self.installation, runner)
    }
}

fn failed_check(state: UpdateState, error: Error) -> UpdateCheckResult {
    UpdateCheckResult {
        state,
        manifest: None,
        artifact: None,
        error: Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{UpdateService, DEFAULT_MANIFEST_URL};
    use crate::detector::UpdateInstallation;
    use crate::fetch::MemoryFetch;
    use crate::manifest::{UpdateState, MAXIMUM_MANIFEST_BYTES};
    use crate::signature::MAXIMUM_SIGNATURE_BYTES;
    use std::path::PathBuf;

    fn service() -> UpdateService {
        UpdateService::new(
            "1.0.0",
            UpdateInstallation {
                target: "linux".to_owned(),
                architecture: "x86_64".to_owned(),
                kind: "portable".to_owned(),
                root: PathBuf::from("/tmp/save-editor-test"),
                executable: PathBuf::from("/tmp/save-editor-test/stalker-save"),
            },
        )
    }

    #[test]
    fn transport_failure_is_unavailable() {
        let result = service().check(&mut MemoryFetch::new());

        assert_eq!(result.state, UpdateState::Unavailable);
        assert!(result.error.is_some());
        assert!(result.manifest.is_none());
        assert!(result.artifact.is_none());
    }

    #[test]
    fn signature_transport_failure_is_unavailable() {
        let mut fetch = MemoryFetch::new();
        fetch.register(DEFAULT_MANIFEST_URL, b"manifest bytes".to_vec());

        let result = service().check(&mut fetch);

        assert_eq!(result.state, UpdateState::Unavailable);
        assert!(result.error.is_some());
    }

    #[test]
    fn oversized_manifest_is_invalid() {
        let mut fetch = MemoryFetch::new();
        fetch.register(DEFAULT_MANIFEST_URL, vec![b'x'; MAXIMUM_MANIFEST_BYTES + 1]);

        let result = service().check(&mut fetch);

        assert_eq!(result.state, UpdateState::Invalid);
        assert!(result.error.is_some());
    }

    #[test]
    fn oversized_signature_is_invalid() {
        let mut fetch = MemoryFetch::new();
        fetch.register(DEFAULT_MANIFEST_URL, b"manifest bytes".to_vec());
        fetch.register(
            format!("{DEFAULT_MANIFEST_URL}.sig"),
            vec![b'x'; MAXIMUM_SIGNATURE_BYTES + 1],
        );

        let result = service().check(&mut fetch);

        assert_eq!(result.state, UpdateState::Invalid);
        assert!(result.error.is_some());
    }

    #[test]
    fn untrusted_manifest_is_invalid() {
        let mut fetch = MemoryFetch::new();
        fetch.register(DEFAULT_MANIFEST_URL, b"not a signed manifest".to_vec());
        fetch.register(format!("{DEFAULT_MANIFEST_URL}.sig"), b"AAAA".to_vec());

        let result = service().check(&mut fetch);

        assert_eq!(result.state, UpdateState::Invalid);
        assert!(result.error.is_some());
        assert!(result.manifest.is_none());
        assert!(result.artifact.is_none());
    }
}
