//! Bounded raw-DEFLATE storage for the built-in game-fixes JSON asset.

use sse_codecs::inflate::inflate_raw;
use sse_core::{Error, Result};
use std::sync::OnceLock;

pub(crate) type JsonAssetCache = OnceLock<std::result::Result<Vec<u8>, String>>;

pub(crate) fn get_json(asset: &'static [u8], cache: &'static JsonAssetCache) -> Result<&'static [u8]> {
    match cache.get_or_init(|| decode_json_asset(asset).map_err(|error| error.to_string())) {
        Ok(decoded) => Ok(decoded.as_slice()),
        Err(message) => Err(Error::damaged(message.clone())),
    }
}

pub(crate) fn decode_json_asset(asset: &[u8]) -> Result<Vec<u8>> {
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
