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
pub fn verify_signature(
    manifest_bytes: &[u8],
    signature_file_bytes: &[u8],
    public_key_pem: Option<&str>,
) -> Result<()> {
    if signature_file_bytes.len() > MAXIMUM_SIGNATURE_BYTES {
        return Err(Error::Refused(
            "Signature file exceeds maximum permitted size".to_string(),
        ));
    }

    let sig_text = core::str::from_utf8(signature_file_bytes)
        .map_err(|_| Error::damaged("Signature file is not valid UTF-8/ASCII"))?;

    let signature_raw = sse_codecs::base64::decode_standard_ignoring_ascii_whitespace(sig_text)?;

    let digest = sse_codecs::sha256::sha256(manifest_bytes);
    let pem = public_key_pem.unwrap_or(PUBLIC_KEY_PEM);
    let public_key = sse_codecs::p256::PublicKey::from_pem(pem)?;

    if public_key.verify(&digest, &signature_raw) {
        Ok(())
    } else {
        Err(Error::Refused(
            "Update manifest signature is invalid; the update was not trusted".to_string(),
        ))
    }
}
