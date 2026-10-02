//! Codecs written in this repository: LZO1X (X1); to come: LZHUF, CRC32, SHA-256, Kraken, JSON, VDF, P-256, inflate.

/// LZO1X as the X-Ray engine uses it for saves.
pub mod lzo1x;

/// X-Ray archive LZHUF decoding and header descrambling.
pub mod lzhuf;

/// IEEE CRC-32 used by archive and container formats.
pub mod crc32;
/// Valve text KeyValues reader used for Steam library discovery.
pub mod vdf;
