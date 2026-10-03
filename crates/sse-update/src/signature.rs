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

    let signature_raw = decode_base64_trimmed(sig_text)?;

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

fn base64_value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => Ok(byte.saturating_sub(b'A')),
        b'a'..=b'z' => Ok(byte.saturating_sub(b'a').saturating_add(26)),
        b'0'..=b'9' => Ok(byte.saturating_sub(b'0').saturating_add(52)),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::damaged("invalid base64 character in signature")),
    }
}

fn decode_base64_trimmed(text: &str) -> Result<Vec<u8>> {
    // Filter out whitespace
    let mut clean = Vec::new();
    for b in text.as_bytes() {
        if !b.is_ascii_whitespace() {
            clean.push(*b);
        }
    }

    if clean.is_empty() || clean.len().checked_rem(4) != Some(0) {
        return Err(Error::damaged("invalid base64 signature length"));
    }

    let estimated_cap = clean
        .len()
        .checked_div(4)
        .and_then(|v| v.checked_mul(3))
        .ok_or_else(|| Error::damaged("base64 capacity overflow"))?;
    let mut output = Vec::with_capacity(estimated_cap);

    let mut index = 0_usize;
    while index < clean.len() {
        let a = base64_value(*clean.get(index).ok_or_else(|| Error::damaged("base64 a"))?)?;
        let b_idx = index.checked_add(1).ok_or_else(|| Error::damaged("overflow"))?;
        let c_idx = index.checked_add(2).ok_or_else(|| Error::damaged("overflow"))?;
        let d_idx = index.checked_add(3).ok_or_else(|| Error::damaged("overflow"))?;

        let b = base64_value(*clean.get(b_idx).ok_or_else(|| Error::damaged("base64 b"))?)?;
        let c_byte = *clean.get(c_idx).ok_or_else(|| Error::damaged("base64 c"))?;
        let d_byte = *clean.get(d_idx).ok_or_else(|| Error::damaged("base64 d"))?;

        let c = if c_byte == b'=' { 0 } else { base64_value(c_byte)? };
        let d = if d_byte == b'=' { 0 } else { base64_value(d_byte)? };

        let first = a
            .checked_shl(2)
            .and_then(|v| v.checked_add(b.checked_shr(4).unwrap_or_default()))
            .ok_or_else(|| Error::damaged("base64 first byte overflow"))?;
        output.push(first);

        if c_byte != b'=' {
            let second = (b & 0x0F)
                .checked_shl(4)
                .and_then(|v| v.checked_add(c.checked_shr(2).unwrap_or_default()))
                .ok_or_else(|| Error::damaged("base64 second byte overflow"))?;
            output.push(second);
        }

        if d_byte != b'=' {
            let third = (c & 0x03)
                .checked_shl(6)
                .and_then(|v| v.checked_add(d))
                .ok_or_else(|| Error::damaged("base64 third byte overflow"))?;
            output.push(third);
        }

        index = index.checked_add(4).ok_or_else(|| Error::damaged("overflow"))?;
    }

    Ok(output)
}
