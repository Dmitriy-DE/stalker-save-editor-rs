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
                artifact: (state == UpdateState::Available).then_some(artifact),
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

        if resp.status != 200 {
            return Err(CheckFailure::Unavailable(Error::Refused(format!(
                "Manifest server returned HTTP {}",
                resp.status
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

        if sig_resp.status != 200 {
            return Err(CheckFailure::Unavailable(Error::Refused(format!(
                "Signature server returned HTTP {}",
                sig_resp.status
            ))));
        }

        // 3. Verify signature
        verify_signature(&manifest_bytes, &sig_bytes).map_err(CheckFailure::Invalid)?;

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
        let artifact = UpdateArtifact {
            release_version: manifest.version.clone(),
            ..artifact
        };

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
        ensure_update_available(&self.current_version, artifact)?;
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
        ensure_update_available(&self.current_version, artifact)?;
        install_artifact(artifact, verified_archive, &self.installation, runner)
    }
}

/// Refuses an artifact unless the release it came from is strictly newer than the installed build.
fn ensure_update_available(current_version: &str, artifact: &UpdateArtifact) -> Result<()> {
    match compare_versions(current_version, &artifact.release_version)? {
        UpdateState::Available => Ok(()),
        _ => Err(Error::Refused(
            "The update package is not newer than the installed version".to_string(),
        )),
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
    use crate::fetch::{Fetch, Response};
    use crate::installer::ProcessRunner;
    use crate::manifest::{UpdateArtifact, UpdateState, MAXIMUM_MANIFEST_BYTES};
    use crate::signature::MAXIMUM_SIGNATURE_BYTES;
    use crate::test_support::MemoryFetch;
    use std::path::{Path, PathBuf};

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

    /// Counts download requests so a refused download can be shown to make no network call.
    #[derive(Default)]
    struct CountingFetch {
        calls: usize,
    }

    impl Fetch for CountingFetch {
        fn get(
            &mut self,
            _url: &str,
            _range_from: u64,
            _sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> sse_core::Result<Response> {
            self.calls = self.calls.saturating_add(1);
            Err(sse_core::Error::Refused("network must not be used".to_owned()))
        }

        fn get_with_response(
            &mut self,
            _url: &str,
            _range_from: u64,
            _on_response: &mut dyn FnMut(&Response) -> bool,
            _sink: &mut dyn FnMut(&[u8]) -> bool,
        ) -> sse_core::Result<Response> {
            self.calls = self.calls.saturating_add(1);
            Err(sse_core::Error::Refused("network must not be used".to_owned()))
        }
    }

    fn artifact_for_release(release_version: &str) -> UpdateArtifact {
        UpdateArtifact {
            target: "linux".to_owned(),
            architecture: "x86_64".to_owned(),
            kind: "portable".to_owned(),
            file: "save-editor.tar.gz".to_owned(),
            size: 1,
            sha256: "0".repeat(64),
            url: "https://updates.test/save-editor.tar.gz".to_owned(),
            release_version: release_version.to_owned(),
        }
    }

    #[test]
    fn download_refuses_a_release_that_is_not_newer() {
        let service = service();
        for release in ["1.0.0", "0.9.0"] {
            let mut fetch = CountingFetch::default();
            let artifact = artifact_for_release(release);
            let result = service.download(&mut fetch, &artifact, Path::new("/tmp/save-editor-download"), None);

            assert!(result.is_err(), "release {release} must be refused");
            assert_eq!(fetch.calls, 0, "no request for release {release}");
        }
    }

    #[test]
    fn install_refuses_a_release_that_is_not_newer() {
        let service = service();
        for release in ["1.0.0", "0.9.0"] {
            let mut runner = RecordingRunner::default();
            let artifact = artifact_for_release(release);
            let result = service.install(&artifact, Path::new("/tmp/save-editor-download.tar.gz"), &mut runner);

            assert!(result.is_err(), "release {release} must be refused");
            assert!(runner.program.is_none(), "no process for release {release}");
        }
    }

    #[derive(Default)]
    struct RecordingRunner {
        program: Option<String>,
    }

    impl ProcessRunner for RecordingRunner {
        fn run(&mut self, program: &str, _args: &[&str]) -> sse_core::Result<i32> {
            self.program = Some(program.to_owned());
            Ok(0)
        }
    }
}
