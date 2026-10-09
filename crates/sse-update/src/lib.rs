//! Application self-updates, signature verification, streaming downloads, and platform installers.
//!
//! Owner: Gemini (G5).

extern crate self as sse_update;

#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod test_support;

pub mod detector;
pub mod fetch;
pub mod installer;
pub mod manifest;
pub mod platform;
pub mod service;
pub mod signature;

pub use detector::{UpdateInstallation, UpdateInstallationDetector, LINUX_PACKAGE_INSTALL_ROOT};
pub use fetch::{download_artifact, verify_existing_file, ContentRange, DefaultFetch, Fetch, Response};
pub use installer::{
    install_artifact, prepare_private_directory, ProcessRunner, SystemProcessRunner, UpdateInstallResult,
    UpdateInstallState,
};
pub use manifest::{
    compare_versions, PrereleasePart, SemVer, UpdateArtifact, UpdateManifest, UpdateState, MAXIMUM_ARTIFACT_BYTES,
    MAXIMUM_MANIFEST_BYTES,
};
pub use service::{UpdateCheckResult, UpdateService, DEFAULT_MANIFEST_URL};
pub use signature::{verify_signature, MAXIMUM_SIGNATURE_BYTES, PUBLIC_KEY_PEM};
