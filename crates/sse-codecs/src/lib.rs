//! Codecs written in this repository: LZO1X (X1); to come: LZHUF, CRC32, SHA-256, Kraken, JSON, VDF, P-256, inflate.

/// LZO1X as the X-Ray engine uses it for saves.
pub mod lzo1x;

/// Windows minidump reader for crash diagnostics.
pub mod minidump;
