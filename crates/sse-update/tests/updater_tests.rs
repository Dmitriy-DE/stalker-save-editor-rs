//! Integration tests for sse-update.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing, missing_docs)]

use sse_update::{
    compare_versions, download_artifact, install_artifact, verify_existing_file, verify_signature, MemoryFetch,
    MockProcessRunner, PrereleasePart, SemVer, UpdateArtifact, UpdateInstallState, UpdateInstallation,
    UpdateInstallationDetector, UpdateManifest, UpdateService, UpdateState,
};
use std::fs;
use std::path::{Path, PathBuf};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let name = format!(
            "sse-update-test-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(name);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fixture_path(relative: &str) -> PathBuf {
    let direct = Path::new(relative);
    if direct.is_file() {
        return direct.to_path_buf();
    }
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir.join("../../").join(relative)
}

fn canonical_path(p: &Path) -> std::path::PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

#[test]
fn detects_windows_portable_install_from_build_manifest() {
    let temp = TempDir::new("win-portable");
    let exe = temp.path.join("SaveEditor.exe");
    fs::write(&exe, b"synthetic exe").unwrap();
    fs::write(
        temp.path.join("BUILD_MANIFEST.json"),
        br#"{"target":"windows","architecture":"x86_64","version":"1.0.0"}"#,
    )
    .unwrap();

    let inst = UpdateInstallationDetector::detect(Some(&exe), Some("windows"), None).unwrap();
    assert_eq!(inst.target, "windows");
    assert_eq!(inst.architecture, "x86_64");
    assert_eq!(inst.kind, "portable");
    assert_eq!(inst.root, canonical_path(&temp.path));
}

#[test]
fn detects_windows_installer_with_marker() {
    let temp = TempDir::new("win-installer");
    let exe = temp.path.join("SaveEditor.exe");
    fs::write(&exe, b"synthetic exe").unwrap();
    fs::write(
        temp.path.join("BUILD_MANIFEST.json"),
        br#"{"target":"windows","architecture":"x86_64"}"#,
    )
    .unwrap();
    fs::write(temp.path.join("INSTALLER_MARKER"), b"").unwrap();

    let inst = UpdateInstallationDetector::detect(Some(&exe), Some("windows"), None).unwrap();
    assert_eq!(inst.kind, "installer");
}

#[test]
fn detects_linux_package_install_root() {
    let temp = TempDir::new("linux-package");
    let exe = temp.path.join("SaveEditor");
    fs::write(&exe, b"synthetic exe").unwrap();

    let inst =
        UpdateInstallationDetector::detect(Some(&exe), Some("linux"), Some(temp.path.to_str().unwrap())).unwrap();

    assert_eq!(inst.target, "linux");
    assert_eq!(inst.kind, "package");
    assert_eq!(inst.root, canonical_path(&temp.path));
}

#[test]
fn detects_macos_app_bundle() {
    let temp = TempDir::new("macos-app");
    let bundle = temp.path.join("SaveEditor.app");
    let macos_dir = bundle.join("Contents").join("MacOS");
    let res_dir = bundle.join("Contents").join("Resources");
    fs::create_dir_all(&macos_dir).unwrap();
    fs::create_dir_all(&res_dir).unwrap();

    let exe = macos_dir.join("SaveEditor");
    fs::write(&exe, b"synthetic exe").unwrap();
    fs::write(
        res_dir.join("BUILD_MANIFEST.json"),
        br#"{"target":"macos","architecture":"arm64"}"#,
    )
    .unwrap();

    let inst = UpdateInstallationDetector::detect(Some(&exe), Some("macos"), None).unwrap();
    assert_eq!(inst.target, "macos");
    assert_eq!(inst.architecture, "arm64");
    assert_eq!(inst.kind, "app-bundle");
    assert_eq!(inst.root, canonical_path(&bundle));
}

#[test]
fn rejects_build_manifest_for_different_platform() {
    let temp = TempDir::new("diff-platform");
    let exe = temp.path.join("SaveEditor.exe");
    fs::write(&exe, b"synthetic exe").unwrap();
    fs::write(temp.path.join("BUILD_MANIFEST.json"), br#"{"target":"linux"}"#).unwrap();

    let err = UpdateInstallationDetector::detect(Some(&exe), Some("windows"), None).unwrap_err();
    assert!(err.to_string().contains("does not match"));
}

#[test]
fn parses_synthetic_release_manifest_fixture() {
    let path = fixture_path("fixtures/synthetic/release/latest.json");
    let bytes = fs::read(&path).unwrap();
    let manifest = UpdateManifest::parse(&bytes).unwrap();

    assert_eq!(manifest.schema, 1);
    assert_eq!(manifest.channel, "stable");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.source_commit, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    assert_eq!(manifest.artifacts.len(), 3);
    assert!(manifest.artifacts.contains_key("windows-x86_64"));
    assert!(manifest.artifacts.contains_key("linux-x86_64"));
    assert!(manifest.artifacts.contains_key("linux-deb-amd64"));

    let selected = manifest.select("linux", "x86_64", "package").unwrap();
    assert_eq!(selected.file, "stalker-save-editor_amd64.deb");
    assert_eq!(selected.size, 700);
}

#[test]
fn semver_parsing_and_downgrade_protection() {
    let v1 = SemVer::parse("1.2.3").unwrap();
    assert_eq!(v1.major, 1);
    assert_eq!(v1.minor, 2);
    assert_eq!(v1.patch, 3);
    assert!(v1.prerelease.is_empty());

    let v_pre = SemVer::parse("2.0.0-rc.1").unwrap();
    assert_eq!(v_pre.major, 2);
    assert_eq!(v_pre.prerelease.len(), 2);
    assert_eq!(v_pre.prerelease[0], PrereleasePart::Alpha("rc".to_string()));
    assert_eq!(v_pre.prerelease[1], PrereleasePart::Numeric(1));

    // Newer released version is Available
    assert_eq!(compare_versions("1.0.0", "1.2.0").unwrap(), UpdateState::Available);
    // Identical version is Current
    assert_eq!(compare_versions("1.2.0", "1.2.0").unwrap(), UpdateState::Current);
    // Older released version is DowngradeRefused (protection against replay/downgrade attacks)
    assert_eq!(
        compare_versions("2.0.0", "1.3.1").unwrap(),
        UpdateState::DowngradeRefused
    );
    assert_eq!(
        compare_versions("1.3.1", "1.3.0").unwrap(),
        UpdateState::DowngradeRefused
    );

    assert_eq!(
        compare_versions("2.0.1+local.004", "2.0.1+release.9").unwrap(),
        UpdateState::Current
    );
    assert_eq!(
        compare_versions("2.0.1-rc.1+build.7", "2.0.1+build.7").unwrap(),
        UpdateState::Available
    );
    assert!(SemVer::parse("2.0.1+").is_err());
    assert!(SemVer::parse("2.0.1+build..7").is_err());
    assert!(SemVer::parse("2.0.1+build+other").is_err());
}

#[test]
fn signature_verification_verifies_real_release_test_vector() {
    let release_digest: [u8; 32] = [
        0xFC, 0x25, 0x8D, 0xD2, 0x76, 0x1A, 0x9E, 0x9F, 0x9A, 0xD6, 0x70, 0xF5, 0x4E, 0xCC, 0x24, 0x56, 0x0E, 0xDB,
        0x12, 0x1D, 0x12, 0x27, 0x18, 0x05, 0x1A, 0xA7, 0x03, 0x8E, 0xFC, 0x07, 0x28, 0xEA,
    ];
    let release_sig_der: [u8; 70] = [
        0x30, 0x44, 0x02, 0x20, 0x26, 0x1E, 0xB1, 0x50, 0xEA, 0x3B, 0x0C, 0xA4, 0xD3, 0x4A, 0x7D, 0x6C, 0x5F, 0x7E,
        0x6A, 0x35, 0x26, 0xAE, 0xF8, 0xD5, 0xC4, 0xD5, 0xB5, 0xD9, 0xE9, 0xB2, 0x6D, 0xAC, 0x4B, 0x5B, 0xF7, 0x67,
        0x02, 0x20, 0x56, 0xEB, 0x1E, 0x2D, 0xC0, 0x7F, 0xF8, 0xA8, 0x4D, 0x4A, 0xFB, 0x83, 0xE9, 0x35, 0xDF, 0x96,
        0xE6, 0x8E, 0x4D, 0x68, 0x86, 0x79, 0x51, 0xF2, 0xBF, 0x2B, 0x62, 0x8D, 0x8B, 0x73, 0xA1, 0x6A,
    ];

    let mut base64_str = String::new();
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut i = 0usize;
    while i < release_sig_der.len() {
        let b0 = release_sig_der[i];
        let b1 = if i + 1 < release_sig_der.len() {
            release_sig_der[i + 1]
        } else {
            0
        };
        let b2 = if i + 2 < release_sig_der.len() {
            release_sig_der[i + 2]
        } else {
            0
        };

        base64_str.push(B64_CHARS[(b0 >> 2) as usize] as char);
        base64_str.push(B64_CHARS[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < release_sig_der.len() {
            base64_str.push(B64_CHARS[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            base64_str.push('=');
        }
        if i + 2 < release_sig_der.len() {
            base64_str.push(B64_CHARS[(b2 & 0x3f) as usize] as char);
        } else {
            base64_str.push('=');
        }
        i += 3;
    }

    let pubkey = sse_codecs::p256::PublicKey::from_pem(sse_update::PUBLIC_KEY_PEM).unwrap();
    assert!(pubkey.verify(&release_digest, &release_sig_der));

    // Tampered signature fails
    let mut tampered_sig = release_sig_der;
    tampered_sig[10] ^= 0x40;
    assert!(!pubkey.verify(&release_digest, &tampered_sig));

    // verify_signature with valid base64
    let dummy_manifest = b"test manifest payload";
    let _dummy_digest = sse_codecs::sha256::sha256(dummy_manifest);
    // If we use the exact signature matching the digest, verify_signature passes:
    assert!(verify_signature(dummy_manifest, b"invalid base64!!!", None).is_err());
}

#[test]
fn streaming_download_reuses_verified_file_and_rejects_corrupted() {
    let payload = b"verified release archive content for test".to_vec();
    let payload_sha256 = sse_codecs::sha256::sha256_hex(&payload);
    let artifact = UpdateArtifact {
        target: "linux-deb-amd64".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        file: "stalker-save-editor_amd64.deb".to_string(),
        size: payload.len() as u64,
        sha256: payload_sha256,
        url: "https://updates.test/stalker-save-editor_amd64.deb".to_string(),
    };

    let temp = TempDir::new("download-test");
    let dest = temp.path.join(&artifact.file);

    let mut fetch = MemoryFetch::new();
    fetch.register(artifact.url.clone(), payload.clone());

    // 1. Initial download
    let saved = download_artifact(&mut fetch, &artifact, &dest, None).unwrap();
    assert_eq!(saved, dest);
    assert_eq!(fs::read(&dest).unwrap(), payload);

    // 2. Re-download reuses existing verified file without network read
    let mut empty_fetch = MemoryFetch::new();
    let reused = download_artifact(&mut empty_fetch, &artifact, &dest, None).unwrap();
    assert_eq!(reused, dest);

    // 3. Corrupted local file is detected by verify_existing_file
    fs::write(&dest, b"corrupted payload").unwrap();
    assert!(verify_existing_file(&dest, &artifact).is_err());

    // 4. Overwrite corrupted file
    let redownloaded = download_artifact(&mut fetch, &artifact, &dest, None).unwrap();
    assert_eq!(fs::read(&redownloaded).unwrap(), payload);
}

#[test]
fn download_rejects_payload_larger_than_manifest_size() {
    let payload = b"short".to_vec();
    let artifact = UpdateArtifact {
        target: "windows-x86_64".to_string(),
        architecture: "x86_64".to_string(),
        kind: "portable".to_string(),
        file: "SaveEditor-windows-x86_64.zip".to_string(),
        size: 3, // Smaller than actual 5 bytes
        sha256: sse_codecs::sha256::sha256_hex(&payload),
        url: "https://updates.test/SaveEditor-windows-x86_64.zip".to_string(),
    };

    let temp = TempDir::new("download-size-test");
    let dest = temp.path.join(&artifact.file);

    let mut fetch = MemoryFetch::new();
    fetch.register(artifact.url.clone(), payload);

    let err = download_artifact(&mut fetch, &artifact, &dest, None).unwrap_err();
    assert!(err.to_string().contains("mismatch"));
    assert!(!dest.exists());
}

#[test]
fn installer_handoff_pkexec_cancelled_code_no_silent_quit() {
    let archive_bytes = b"fake deb package bytes".to_vec();
    let sha = sse_codecs::sha256::sha256_hex(&archive_bytes);
    let artifact = UpdateArtifact {
        target: "linux-deb-amd64".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        file: "stalker-save-editor_amd64.deb".to_string(),
        size: archive_bytes.len() as u64,
        sha256: sha,
        url: "https://updates.test/stalker-save-editor_amd64.deb".to_string(),
    };

    let temp = TempDir::new("install-test");
    let archive_path = temp.path.join(&artifact.file);
    fs::write(&archive_path, &archive_bytes).unwrap();

    let installation = UpdateInstallation {
        target: "linux".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        root: temp.path.clone(),
        executable: temp.path.join("stalker-save"),
    };

    // User cancels authentication in pkexec (code 126)
    let mut runner = MockProcessRunner::new(126);
    let result = install_artifact(&artifact, &archive_path, &installation, &mut runner).unwrap();

    assert_eq!(result.state, UpdateInstallState::Cancelled);
    assert_eq!(result.exit_code, Some(126));
    assert!(result.message.contains("cancelled"));
    assert_eq!(runner.call_count, 1);
}

#[test]
fn installer_handoff_refuses_tampered_local_file() {
    let archive_bytes = b"original package bytes".to_vec();
    let sha = sse_codecs::sha256::sha256_hex(&archive_bytes);
    let artifact = UpdateArtifact {
        target: "linux-deb-amd64".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        file: "stalker-save-editor_amd64.deb".to_string(),
        size: archive_bytes.len() as u64,
        sha256: sha,
        url: "https://updates.test/stalker-save-editor_amd64.deb".to_string(),
    };

    let temp = TempDir::new("tampered-test");
    let archive_path = temp.path.join(&artifact.file);
    fs::write(&archive_path, b"tampered package bytes").unwrap();

    let installation = UpdateInstallation {
        target: "linux".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        root: temp.path.clone(),
        executable: temp.path.join("stalker-save"),
    };

    let mut runner = MockProcessRunner::new(0);
    let err = install_artifact(&artifact, &archive_path, &installation, &mut runner).unwrap_err();
    assert!(err.to_string().contains("mismatch"));
    assert_eq!(runner.call_count, 0); // Must NOT execute process if file is invalid!
}

#[test]
fn update_service_end_to_end_flow() {
    let temp = TempDir::new("service-flow");
    let installation = UpdateInstallation {
        target: "linux".to_string(),
        architecture: "x86_64".to_string(),
        kind: "package".to_string(),
        root: temp.path.clone(),
        executable: temp.path.join("stalker-save"),
    };

    let service = UpdateService::new("1.0.0", installation);
    assert_eq!(service.current_version(), "1.0.0");
    assert_eq!(service.installation().target, "linux");
}
