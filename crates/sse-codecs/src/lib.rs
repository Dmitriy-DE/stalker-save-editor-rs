//! Codecs and parsers written in this repository. No third-party code.

/// Block-compressed texture decoders (BC1/3/4/5/7).
pub mod bc;
/// LZO1X as the X-Ray engine uses it for saves.
pub mod lzo1x;

/// X-Ray archive LZHUF decoding and header descrambling.
pub mod lzhuf;

/// IEEE CRC-32 used by archive and container formats.
pub mod crc32;
/// Raw DEFLATE compressor.
pub mod deflate;
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

/// PNG decoder to RGBA8.
pub mod png;
/// SHA-256 (FIPS 180-4).
pub mod sha256;

/// Oodle Kraken decompressor used by STALKER 2 saves.
pub mod kraken;
