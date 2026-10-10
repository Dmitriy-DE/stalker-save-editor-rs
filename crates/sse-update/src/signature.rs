//! ECDSA P-256 / SHA-256 update signature verification.

use sse_core::{Error, Result};

/// Maximum signature file size in bytes (fail closed if larger).
pub const MAXIMUM_SIGNATURE_BYTES: usize = 1024;

/// Shipped public key PEM embedded in the application.
pub const PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAEugfpzNXArEpRIoUsxrmG6KWIWMFc\n\
DuEkmd6oGnQq6qsZmILc2fYC0wfqEMk/NB88BSFAC1N6fmziJf11RVtlLQ==\n\
-----END PUBLIC KEY-----\n";

/// Verifies an ECDSA P-256 SHA-256 signature over manifest bytes.
///
/// # Errors
/// Returns an error if the signature exceeds [`MAXIMUM_SIGNATURE_BYTES`], cannot be base64-decoded,
/// or fails ECDSA verification against the publisher public key.
pub fn verify_signature(manifest_bytes: &[u8], signature_file_bytes: &[u8]) -> Result<()> {
    verify_signature_with_key(manifest_bytes, signature_file_bytes, PUBLIC_KEY_PEM)
}

/// Same check as [`verify_signature`] against the given public key PEM. Only the shipped key is trusted
/// outside tests; the release rehearsal uses a key generated for that run.
fn verify_signature_with_key(manifest_bytes: &[u8], signature_file_bytes: &[u8], public_key_pem: &str) -> Result<()> {
    if signature_file_bytes.len() > MAXIMUM_SIGNATURE_BYTES {
        return Err(Error::Refused(
            "Signature file exceeds maximum permitted size".to_string(),
        ));
    }

    let sig_text = core::str::from_utf8(signature_file_bytes)
        .map_err(|_| Error::damaged("Signature file is not valid UTF-8/ASCII"))?;

    let signature_raw = sse_codecs::base64::decode_standard_ignoring_ascii_whitespace(sig_text)?;

    let digest = sse_codecs::sha256::sha256(manifest_bytes);
    let public_key = sse_codecs::p256::PublicKey::from_pem(public_key_pem)?;

    if public_key.verify(&digest, &signature_raw) {
        Ok(())
    } else {
        Err(Error::Refused(
            "Update manifest signature is invalid; the update was not trusted".to_string(),
        ))
    }
}

#[cfg(test)]
mod release_rehearsal {
    //! Runs only in the release-rehearsal CI job (`cargo test -- --ignored`). The job makes a throwaway key and
    //! signs `latest.json` with it; the shipped key must refuse that manifest.
    use super::{verify_signature, verify_signature_with_key};
    use crate::manifest::{compare_versions, UpdateManifest, UpdateState};
    use std::path::PathBuf;

    fn rehearsal_dir() -> sse_core::Result<PathBuf> {
        std::env::var_os("SSE_REHEARSAL_DIR")
            .map(PathBuf::from)
            .ok_or_else(|| sse_core::Error::System("SSE_REHEARSAL_DIR is not set".to_string()))
    }

    #[test]
    #[ignore = "needs SSE_REHEARSAL_DIR from the release-rehearsal job"]
    fn release_manifest_is_checked_with_the_rehearsal_key() -> sse_core::Result<()> {
        let dir = rehearsal_dir()?;
        let read = |name: &str| std::fs::read(dir.join(name)).map_err(sse_core::Error::from);
        let manifest = read("latest.json")?;
        let signature = read("latest.json.sig")?;
        let public_key = String::from_utf8(read("rehearsal-public.pem")?)
            .map_err(|_| sse_core::Error::damaged("rehearsal public key is not text"))?;

        // 1. The signed manifest is accepted with the key that signed it.
        verify_signature_with_key(&manifest, &signature, &public_key)?;

        // 2. The shipped key does not trust a manifest signed by the rehearsal key.
        assert!(verify_signature(&manifest, &signature).is_err());

        // 3. One changed byte of the signature is refused.
        let mut bad_signature = signature.clone();
        let middle = bad_signature.len() / 2;
        if let Some(byte) = bad_signature.get_mut(middle) {
            *byte = if *byte == b'A' { b'B' } else { b'A' };
        }
        assert!(verify_signature_with_key(&manifest, &bad_signature, &public_key).is_err());

        // 4. One changed byte of the manifest is refused.
        let mut edited_manifest = manifest.clone();
        edited_manifest.push(b' ');
        assert!(verify_signature_with_key(&edited_manifest, &signature, &public_key).is_err());

        // 5. Each artifact the manifest lists matches its size and SHA-256; a changed copy is refused.
        let parsed = UpdateManifest::parse(&manifest)?;
        for key in ["windows-x86_64", "linux-x86_64", "linux-deb-amd64"] {
            let artifact = parsed
                .artifacts
                .get(key)
                .ok_or_else(|| sse_core::Error::damaged(format!("manifest has no {key}")))?;
            let path = dir.join(&artifact.file);
            crate::fetch::verify_existing_file(&path, artifact)?;
            let mut bytes = std::fs::read(&path).map_err(sse_core::Error::from)?;
            if let Some(first) = bytes.first_mut() {
                *first ^= 0x01;
            }
            let changed = dir.join(format!("changed-{}", artifact.file));
            std::fs::write(&changed, &bytes).map_err(sse_core::Error::from)?;
            assert!(
                crate::fetch::verify_existing_file(&changed, artifact).is_err(),
                "{key} changed copy accepted"
            );
        }

        // 6. An older version than the installed one is refused as a downgrade.
        assert_eq!(
            compare_versions("999.0.0", &parsed.version)?,
            UpdateState::DowngradeRefused
        );
        assert_eq!(compare_versions("0.0.1", &parsed.version)?, UpdateState::Available);
        Ok(())
    }
}
