//! Bounded raw-DEFLATE storage for built-in JSON assets.

use crate::inflate::inflate_raw;
use sse_core::{Error, Result};
use std::sync::OnceLock;

/// One-time cache for the result of decoding a built-in JSON asset.
pub type JsonAssetCache = OnceLock<std::result::Result<Vec<u8>, String>>;

/// Decodes an asset once and returns its cached bytes.
pub fn get_json(asset: &'static [u8], cache: &'static JsonAssetCache) -> Result<&'static [u8]> {
    match cache.get_or_init(|| decode_json_asset(asset).map_err(|error| error.to_string())) {
        Ok(decoded) => Ok(decoded.as_slice()),
        Err(message) => Err(Error::damaged(message.clone())),
    }
}

/// Decodes a four-byte little-endian output-size header followed by raw DEFLATE data.
pub fn decode_json_asset(asset: &[u8]) -> Result<Vec<u8>> {
    let declared_length = asset
        .get(..4)
        .and_then(|header| <[u8; 4]>::try_from(header).ok())
        .map(u32::from_le_bytes)
        .ok_or_else(|| Error::damaged("Embedded JSON asset has no size header."))?;
    let maximum =
        usize::try_from(declared_length).map_err(|_| Error::damaged("Embedded JSON asset size is invalid."))?;
    let compressed = asset
        .get(4..)
        .ok_or_else(|| Error::damaged("Embedded JSON asset has no DEFLATE stream."))?;
    let decoded = inflate_raw(compressed, maximum)?;
    if decoded.len() != maximum {
        return Err(Error::damaged("Embedded JSON asset size does not match its header."));
    }
    Ok(decoded)
}
