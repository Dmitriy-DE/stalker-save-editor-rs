//! Application self-updates, signature verification, streaming downloads, and platform installers.
//!
//! Owner: Gemini (G5).

pub mod detector;
pub mod fetch;
pub mod installer;
pub mod manifest;
pub mod platform;
pub mod service;
pub mod signature;

pub use detector::{UpdateInstallation, UpdateInstallationDetector, LINUX_PACKAGE_INSTALL_ROOT};
pub use fetch::{
    download_artifact, verify_existing_file, ContentRange, DefaultFetch, Fetch, FileFetch, MemoryFetch, Response,
};
pub use installer::{
    install_artifact, prepare_private_directory, MockProcessRunner, ProcessRunner, SystemProcessRunner,
    UpdateInstallResult, UpdateInstallState,
};
pub use manifest::{
    compare_versions, PrereleasePart, SemVer, UpdateArtifact, UpdateManifest, UpdateState, MAXIMUM_ARTIFACT_BYTES,
    MAXIMUM_MANIFEST_BYTES,
};
pub use service::{UpdateCheckResult, UpdateService, DEFAULT_MANIFEST_URL};
pub use signature::{verify_signature, MAXIMUM_SIGNATURE_BYTES, PUBLIC_KEY_PEM};
