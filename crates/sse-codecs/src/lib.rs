//! Codecs and parsers written in this repository. No third-party code.

/// Block-compressed texture decoders (BC1/3/4/5/7).
pub mod bc;
/// LZO1X as the X-Ray engine uses it for saves.
pub mod lzo1x;

/// Independent safe Rust Kraken decoder retained for cross-checking the main implementation.
pub mod kraken_c3a;
mod kraken_c3a_entropy;
mod kraken_c3a_lz;

/// X-Ray archive LZHUF decoding and header descrambling.
pub mod lzhuf;

/// Strict RFC 4648 standard Base64 decoding.
pub mod base64;
/// IEEE CRC-32 used by archive and container formats.
pub mod crc32;
/// Raw DEFLATE compressor.
pub mod deflate;
/// Bounded raw-DEFLATE decoding and caching for built-in JSON assets.
pub mod embedded_json;
/// Safe OpenType/TrueType parsing and glyph coverage rasterisation.
pub mod font;
/// Zlib/DEFLATE decoder used by PNG and package tooling.
pub mod inflate;
/// Strict streaming RFC 8259 JSON reader and writer.
pub mod json;
/// Windows minidump reader for crash diagnostics.
pub mod minidump;
/// Ogg page and packet framing.
pub mod ogg;
/// ECDSA P-256 update-signature verification.
pub mod p256;
/// Valve text KeyValues reader used for Steam library discovery.
pub mod vdf;
/// Vorbis-I audio decoder.
pub mod vorbis;
/// ZIP reader and reproducible writer.
pub mod zip;

/// Baseline JPEG decoder with libjpeg-compatible IDCT and upsampling.
pub mod jpeg;
/// PNG decoder to RGBA8.
pub mod png;
/// Minimal deterministic RGBA8 PNG encoder for golden tests.
pub mod png_encode;
/// SHA-256 (FIPS 180-4).
pub mod sha256;

/// Oodle Kraken decompressor used by STALKER 2 saves.
pub mod kraken;
/// Safe Kraken mode-1 LZ and Huffman encoder paired with [`kraken`].
pub mod kraken_encode;

const MAX_DECLARED_EXPANSION_RATIO: usize = 4096;
const DECLARED_EXPANSION_SLACK: usize = 64 * 1024;

/// Rejects hostile declared decompressed sizes before callers allocate the output buffer.
///
/// The bound is deliberately generous for real save compression while preventing tiny inputs from
/// requesting hundreds of MiB. It must be checked before allocation.
///
/// # Errors
/// Returns `Error::Damaged` when the declaration is not proportional to the packed stream.
pub fn validate_declared_output_size(stream_len: usize, declared_size: usize, codec: &str) -> sse_core::Result<()> {
    let proportional = stream_len
        .checked_mul(MAX_DECLARED_EXPANSION_RATIO)
        .and_then(|value| value.checked_add(DECLARED_EXPANSION_SLACK))
        .unwrap_or(usize::MAX);
    if declared_size > proportional {
        return Err(sse_core::Error::damaged(format!(
            "{codec} declared output size is disproportionate to the packed stream"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod allocation_guard_tests {
    use super::validate_declared_output_size;

    #[test]
    fn tiny_stream_cannot_claim_256_mib() {
        assert!(validate_declared_output_size(100, 256 * 1024 * 1024, "test").is_err());
    }

    #[test]
    fn ordinary_compression_ratio_is_accepted() {
        assert!(validate_declared_output_size(1024, 1024 * 1024, "test").is_ok());
    }
}

#[cfg(test)]
mod embedded_json_tests {
    use super::embedded_json::decode_json_asset;

    #[test]
    fn empty_json_asset_inflates_from_its_size_header() {
        let mut asset = 0_u32.to_le_bytes().to_vec();
        asset.extend_from_slice(&[0x03, 0x00]);

        assert_eq!(decode_json_asset(&asset).unwrap_or_default(), b"");
    }

    #[test]
    fn embedded_json_rejects_truncated_size_header() {
        assert!(decode_json_asset(&[1, 2, 3]).is_err());
    }

    #[test]
    fn embedded_json_rejects_declared_size_mismatch() {
        let mut asset = 1_u32.to_le_bytes().to_vec();
        asset.extend_from_slice(&[0x03, 0x00]);

        assert!(decode_json_asset(&asset).is_err());
    }
}
