//! Codecs and parsers written in this repository. No third-party code.

/// LZO1X as the X-Ray engine uses it for saves.
pub mod lzo1x;

/// X-Ray archive LZHUF decoding and header descrambling.
pub mod lzhuf;

/// IEEE CRC-32 used by archive and container formats.
pub mod crc32;
/// Safe OpenType/TrueType parsing and glyph coverage rasterisation.
pub mod font;
/// Zlib/DEFLATE decoder used by PNG and package tooling.
pub mod inflate;
/// Strict streaming RFC 8259 JSON reader and writer.
pub mod json;
/// Windows minidump reader for crash diagnostics.
pub mod minidump;
/// ECDSA P-256 update-signature verification.
pub mod p256;
/// Valve text KeyValues reader used for Steam library discovery.
pub mod vdf;

/// PNG decoder to RGBA8.
pub mod png;
