//! Little-endian field reads with bounds checks, shared by the save readers and writers.

use crate::{Error, Result};

/// Reads a little-endian `u16` at `offset`.
///
/// # Errors
/// Returns `Error::Damaged` when the field is not inside `bytes`.
pub fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| Error::damaged("field offset overflows"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("field is outside the buffer"))?;
    let value = <[u8; 2]>::try_from(value).map_err(|_| Error::damaged("field has the wrong width"))?;
    Ok(u16::from_le_bytes(value))
}

/// Reads a little-endian `u32` at `offset`.
///
/// # Errors
/// Returns `Error::Damaged` when the field is not inside `bytes`.
pub fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| Error::damaged("field offset overflows"))?;
    let value = bytes
        .get(offset..end)
        .ok_or_else(|| Error::damaged("field is outside the buffer"))?;
    let value = <[u8; 4]>::try_from(value).map_err(|_| Error::damaged("field has the wrong width"))?;
    Ok(u32::from_le_bytes(value))
}
