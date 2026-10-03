//! Dependency-free BC1, BC3, BC4, BC5 and BC7 block decoders to RGBA8.
//!
//! BC7 partition and fix-up data are the normative BPTC tables from Khronos.
//! Every decoder consumes exactly one 4x4 block and returns row-major RGBA8.

use sse_core::{Error, Result};

const PIXELS: usize = 16;
const RGBA_BYTES: usize = 64;
const WEIGHTS_2: [u8; 4] = [0, 21, 43, 64];
const WEIGHTS_3: [u8; 8] = [0, 9, 18, 27, 37, 46, 55, 64];
const WEIGHTS_4: [u8; 16] = [0, 4, 9, 13, 17, 21, 26, 30, 34, 38, 43, 47, 51, 55, 60, 64];

const PARTITION_2: [[u8; 16]; 64] = [
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1],
    [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 1, 1, 1],
    [0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1],
    [0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1, 1],
    [0, 1, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0],
    [0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0],
    [0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0],
    [0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 1],
    [0, 0, 1, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0],
    [0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 0, 0],
    [0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0],
    [0, 0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 1, 1, 0, 0],
    [0, 0, 0, 1, 0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0],
    [0, 1, 1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0],
    [0, 0, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0],
    [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
    [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1],
    [0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0],
    [0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 1, 0, 0, 0, 0, 1, 1, 1, 1, 0, 0],
    [0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0],
    [0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1],
    [0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1],
    [0, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 0],
    [0, 0, 0, 1, 0, 0, 1, 1, 1, 1, 0, 0, 1, 0, 0, 0],
    [0, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 0, 1, 1, 1, 1, 0, 1, 1, 1, 0, 0],
    [0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 1, 0],
    [0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 1, 1],
    [0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1],
    [0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0],
    [0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0],
    [0, 0, 0, 0, 0, 1, 0, 0, 1, 1, 1, 0, 0, 1, 0, 0],
    [0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 1],
    [0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0],
    [0, 0, 1, 1, 1, 0, 0, 1, 1, 1, 0, 0, 0, 1, 1, 0],
    [0, 1, 1, 0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 0, 1],
    [0, 1, 1, 0, 0, 0, 1, 1, 0, 0, 1, 1, 1, 0, 0, 1],
    [0, 1, 1, 1, 1, 1, 1, 0, 1, 0, 0, 0, 0, 0, 0, 1],
    [0, 0, 0, 1, 1, 0, 0, 0, 1, 1, 1, 0, 0, 1, 1, 1],
    [0, 0, 0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0],
    [0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0],
    [0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 1, 0, 1, 1, 1],
];
const PARTITION_3: [[u8; 16]; 64] = [
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 1, 2, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 2, 0, 0, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 1, 0, 1, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1, 1, 1, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1],
    [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2],
    [0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2, 0, 1, 1, 2],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 2, 2, 2, 0],
    [0, 0, 0, 1, 0, 0, 1, 1, 0, 1, 1, 2, 1, 1, 2, 2],
    [0, 1, 1, 1, 0, 0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0],
    [0, 0, 0, 0, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 1, 1, 1, 1],
    [0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2, 0, 2, 2, 2],
    [0, 0, 0, 1, 0, 0, 0, 1, 2, 2, 2, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2],
    [0, 0, 0, 0, 1, 1, 0, 0, 2, 2, 1, 0, 2, 2, 1, 0],
    [0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1, 0, 0, 0, 0],
    [0, 0, 1, 2, 0, 0, 1, 2, 1, 1, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1, 0, 1, 1, 0],
    [0, 0, 0, 0, 0, 1, 1, 0, 1, 2, 2, 1, 1, 2, 2, 1],
    [0, 0, 2, 2, 1, 1, 0, 2, 1, 1, 0, 2, 0, 0, 2, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 0, 0, 2, 2, 2, 2, 2],
    [0, 0, 1, 1, 0, 1, 2, 2, 0, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 0, 0, 2, 0, 0, 0, 2, 2, 1, 1, 2, 2, 2, 1],
    [0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 2, 2, 2],
    [0, 2, 2, 2, 0, 0, 2, 2, 0, 0, 1, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 0, 0, 1, 2, 0, 0, 2, 2, 0, 2, 2, 2],
    [0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0, 0, 1, 2, 0],
    [0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0],
    [0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0, 1, 2, 0],
    [0, 1, 2, 0, 2, 0, 1, 2, 1, 2, 0, 1, 0, 1, 2, 0],
    [0, 0, 1, 1, 2, 2, 0, 0, 1, 1, 2, 2, 0, 0, 1, 1],
    [0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 0, 0, 0, 0, 1, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 0, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2, 1, 1, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 2, 2, 0, 0, 1, 1],
    [0, 2, 2, 0, 1, 2, 2, 1, 0, 2, 2, 0, 1, 2, 2, 1],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 0, 1, 0, 1],
    [0, 0, 0, 0, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1, 2, 1],
    [0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 2, 2, 2, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 2, 2, 2, 0, 1, 1, 1],
    [0, 0, 0, 2, 1, 1, 1, 2, 0, 0, 0, 2, 1, 1, 1, 2],
    [0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 2, 2, 2, 0, 1, 1, 1, 0, 1, 1, 1, 0, 2, 2, 2],
    [0, 0, 0, 2, 1, 1, 1, 2, 1, 1, 1, 2, 0, 0, 0, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2, 2, 1, 1, 2],
    [0, 1, 1, 0, 0, 1, 1, 0, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 0, 2, 2, 0, 0, 1, 1, 0, 0, 1, 1, 0, 0, 2, 2],
    [0, 0, 2, 2, 1, 1, 2, 2, 1, 1, 2, 2, 0, 0, 2, 2],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 1, 1, 2],
    [0, 0, 0, 2, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 1],
    [0, 2, 2, 2, 1, 2, 2, 2, 0, 2, 2, 2, 1, 2, 2, 2],
    [0, 1, 0, 1, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2],
    [0, 1, 1, 1, 2, 0, 1, 1, 2, 2, 0, 1, 2, 2, 2, 0],
];
const ANCHOR_2: [u8; 64] = [
    15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 2, 8, 2, 2, 8, 8, 15, 2, 8, 2, 2, 8, 8, 2, 2,
    15, 15, 6, 8, 2, 8, 15, 15, 2, 8, 2, 2, 2, 15, 15, 6, 6, 2, 6, 8, 15, 15, 2, 2, 15, 15, 15, 15, 15, 2, 2, 15,
];
const ANCHOR_3_SECOND: [u8; 64] = [
    3, 3, 15, 15, 8, 3, 15, 15, 8, 8, 6, 6, 6, 5, 3, 3, 3, 3, 8, 15, 3, 3, 6, 10, 5, 8, 8, 6, 8, 5, 15, 15, 8, 15, 3,
    5, 6, 10, 8, 15, 15, 3, 15, 5, 15, 15, 15, 15, 3, 15, 5, 5, 5, 8, 5, 10, 5, 10, 8, 13, 15, 12, 3, 3,
];
const ANCHOR_3_THIRD: [u8; 64] = [
    15, 8, 8, 3, 15, 15, 3, 8, 15, 15, 15, 15, 15, 15, 15, 8, 15, 8, 15, 3, 15, 8, 15, 8, 3, 15, 6, 10, 15, 15, 10, 8,
    15, 3, 15, 10, 10, 8, 9, 10, 6, 15, 8, 15, 3, 6, 6, 8, 15, 3, 15, 15, 15, 15, 15, 15, 15, 15, 15, 15, 3, 15, 15, 8,
];

/// Decodes one BC1/DXT1 block.
///
/// # Errors
/// Returns Error::Damaged unless the block is exactly 8 bytes.
pub fn decode_bc1(block: &[u8]) -> Result<[u8; RGBA_BYTES]> {
    decode_bc1_impl(block, false)
}

/// Decodes one BC3/DXT5 block.
///
/// # Errors
/// Returns Error::Damaged unless the block is exactly 16 bytes.
pub fn decode_bc3(block: &[u8]) -> Result<[u8; RGBA_BYTES]> {
    if block.len() != 16 {
        return Err(Error::damaged("BC3 block must be 16 bytes"));
    }
    let alpha = decode_bc4_values(block.get(..8).ok_or_else(|| Error::damaged("BC3 alpha block"))?)?;
    let mut rgba = decode_bc1_impl(block.get(8..).ok_or_else(|| Error::damaged("BC3 color block"))?, true)?;
    for (pixel, value) in alpha.iter().copied().enumerate() {
        let at = pixel
            .checked_mul(4)
            .and_then(|v| v.checked_add(3))
            .ok_or_else(|| Error::damaged("BC3 alpha offset"))?;
        *rgba
            .get_mut(at)
            .ok_or_else(|| Error::damaged("BC3 alpha destination"))? = value;
    }
    Ok(rgba)
}

/// Decodes one unsigned BC4 block. Red carries BC4, green/blue are zero, alpha is 255.
///
/// # Errors
/// Returns Error::Damaged unless the block is exactly 8 bytes.
pub fn decode_bc4(block: &[u8]) -> Result<[u8; RGBA_BYTES]> {
    let red = decode_bc4_values(block)?;
    let mut rgba = [0_u8; RGBA_BYTES];
    for (pixel, value) in red.iter().copied().enumerate() {
        put_pixel(&mut rgba, pixel, [value, 0, 0, 255])?;
    }
    Ok(rgba)
}

/// Decodes one unsigned BC5 block. Red/green carry BC5, blue is zero, alpha is 255.
///
/// # Errors
/// Returns Error::Damaged unless the block is exactly 16 bytes.
pub fn decode_bc5(block: &[u8]) -> Result<[u8; RGBA_BYTES]> {
    if block.len() != 16 {
        return Err(Error::damaged("BC5 block must be 16 bytes"));
    }
    let red = decode_bc4_values(block.get(..8).ok_or_else(|| Error::damaged("BC5 red block"))?)?;
    let green = decode_bc4_values(block.get(8..).ok_or_else(|| Error::damaged("BC5 green block"))?)?;
    let mut rgba = [0_u8; RGBA_BYTES];
    for pixel in 0..PIXELS {
        put_pixel(
            &mut rgba,
            pixel,
            [
                *red.get(pixel).ok_or_else(|| Error::damaged("BC5 red texel"))?,
                *green.get(pixel).ok_or_else(|| Error::damaged("BC5 green texel"))?,
                0,
                255,
            ],
        )?;
    }
    Ok(rgba)
}

fn decode_bc1_impl(block: &[u8], force_four_color: bool) -> Result<[u8; RGBA_BYTES]> {
    if block.len() != 8 {
        return Err(Error::damaged("BC1 block must be 8 bytes"));
    }
    let c0 = read_u16(block, 0)?;
    let c1 = read_u16(block, 2)?;
    let p0 = rgb565(c0);
    let p1 = rgb565(c1);
    let mut palette = [[0_u8; 4]; 4];
    set_pixel4(&mut palette, 0, [p0.0, p0.1, p0.2, 255])?;
    set_pixel4(&mut palette, 1, [p1.0, p1.1, p1.2, 255])?;
    let first = *palette.first().ok_or_else(|| Error::damaged("BC1 palette first"))?;
    let second = *palette.get(1).ok_or_else(|| Error::damaged("BC1 palette second"))?;
    if force_four_color || c0 > c1 {
        set_pixel4(&mut palette, 2, mix(first, second, 1, 3))?;
        set_pixel4(&mut palette, 3, mix(first, second, 2, 3))?;
    } else {
        set_pixel4(&mut palette, 2, mix(first, second, 1, 2))?;
        set_pixel4(&mut palette, 3, [0, 0, 0, 0])?;
    }
    let indices = read_u32(block, 4)?;
    let mut out = [0_u8; RGBA_BYTES];
    for pixel in 0..PIXELS {
        let shift = u32::try_from(pixel.saturating_mul(2)).map_err(|_| Error::damaged("BC1 shift"))?;
        let index = usize::try_from(indices.checked_shr(shift).unwrap_or_default() & 3)
            .map_err(|_| Error::damaged("BC1 index"))?;
        put_pixel(
            &mut out,
            pixel,
            *palette.get(index).ok_or_else(|| Error::damaged("BC1 palette index"))?,
        )?;
    }
    Ok(out)
}

fn decode_bc4_values(block: &[u8]) -> Result<[u8; PIXELS]> {
    if block.len() != 8 {
        return Err(Error::damaged("BC4 block must be 8 bytes"));
    }
    let a0 = *block.first().ok_or_else(|| Error::damaged("BC4 endpoint 0"))?;
    let a1 = *block.get(1).ok_or_else(|| Error::damaged("BC4 endpoint 1"))?;
    let mut palette = [0_u8; 8];
    *palette.get_mut(0).ok_or_else(|| Error::damaged("BC4 palette"))? = a0;
    *palette.get_mut(1).ok_or_else(|| Error::damaged("BC4 palette"))? = a1;
    if a0 > a1 {
        for i in 1_u16..=6 {
            let v = u16::from(a0)
                .saturating_mul(7_u16.saturating_sub(i))
                .saturating_add(u16::from(a1).saturating_mul(i))
                .checked_div(7)
                .unwrap_or_default();
            *palette
                .get_mut(usize::from(i.saturating_add(1)))
                .ok_or_else(|| Error::damaged("BC4 palette"))? = u8::try_from(v).unwrap_or_default();
        }
    } else {
        for i in 1_u16..=4 {
            let v = u16::from(a0)
                .saturating_mul(5_u16.saturating_sub(i))
                .saturating_add(u16::from(a1).saturating_mul(i))
                .checked_div(5)
                .unwrap_or_default();
            *palette
                .get_mut(usize::from(i.saturating_add(1)))
                .ok_or_else(|| Error::damaged("BC4 palette"))? = u8::try_from(v).unwrap_or_default();
        }
        *palette.get_mut(6).ok_or_else(|| Error::damaged("BC4 palette"))? = 0;
        *palette.get_mut(7).ok_or_else(|| Error::damaged("BC4 palette"))? = 255;
    }
    let mut packed = 0_u64;
    for i in 0..6_usize {
        let byte = u64::from(
            *block
                .get(i.saturating_add(2))
                .ok_or_else(|| Error::damaged("BC4 index byte"))?,
        );
        packed |= byte
            .checked_shl(u32::try_from(i.saturating_mul(8)).unwrap_or_default())
            .unwrap_or_default();
    }
    let mut out = [0_u8; PIXELS];
    for pixel in 0..PIXELS {
        let index = usize::try_from(
            packed
                .checked_shr(u32::try_from(pixel.saturating_mul(3)).unwrap_or_default())
                .unwrap_or_default()
                & 7,
        )
        .map_err(|_| Error::damaged("BC4 index"))?;
        *out.get_mut(pixel).ok_or_else(|| Error::damaged("BC4 output"))? =
            *palette.get(index).ok_or_else(|| Error::damaged("BC4 palette index"))?;
    }
    Ok(out)
}

/// Decodes all eight standard BC7 modes.
///
/// # Errors
/// Returns Error::Damaged for malformed input or the reserved all-zero mode prefix.
pub fn decode_bc7(block: &[u8]) -> Result<[u8; RGBA_BYTES]> {
    if block.len() != 16 {
        return Err(Error::damaged("BC7 block must be 16 bytes"));
    }
    let bytes = <[u8; 16]>::try_from(block).map_err(|_| Error::damaged("BC7 block width"))?;
    let mut bits = Bits::new(u128::from_le_bytes(bytes));
    let mut mode = None;
    for candidate in 0_u8..8 {
        if bits.read(1)? != 0 {
            mode = Some(candidate);
            break;
        }
    }
    let mode = mode.ok_or_else(|| Error::damaged("reserved BC7 mode"))?;
    let info = Mode::for_number(mode);
    let partition = usize::try_from(bits.read(info.partition_bits)?).map_err(|_| Error::damaged("BC7 partition"))?;
    let rotation = u8::try_from(bits.read(info.rotation_bits)?).map_err(|_| Error::damaged("BC7 rotation"))?;
    let selector = u8::try_from(bits.read(info.selector_bits)?).map_err(|_| Error::damaged("BC7 selector"))?;
    let endpoints = usize::from(info.subsets).saturating_mul(2);
    let mut raw = [[0_u16; 4]; 6];
    let mut expanded = [[255_u8; 4]; 6];
    for channel in 0..3_usize {
        for endpoint in 0..endpoints {
            *raw.get_mut(endpoint)
                .and_then(|v| v.get_mut(channel))
                .ok_or_else(|| Error::damaged("BC7 color endpoint slot"))? =
                u16::try_from(bits.read(info.color_bits)?).map_err(|_| Error::damaged("BC7 color endpoint"))?;
        }
    }
    if info.alpha_bits != 0 {
        for endpoint in 0..endpoints {
            *raw.get_mut(endpoint)
                .and_then(|v| v.get_mut(3))
                .ok_or_else(|| Error::damaged("BC7 alpha endpoint slot"))? =
                u16::try_from(bits.read(info.alpha_bits)?).map_err(|_| Error::damaged("BC7 alpha endpoint"))?;
        }
    }
    let mut pbits = [0_u8; 6];
    match info.pbits {
        PBits::None => {}
        PBits::PerSubset => {
            for subset in 0..usize::from(info.subsets) {
                let p = u8::try_from(bits.read(1)?).map_err(|_| Error::damaged("BC7 shared pbit"))?;
                let first = subset.saturating_mul(2);
                *pbits.get_mut(first).ok_or_else(|| Error::damaged("BC7 pbit"))? = p;
                *pbits
                    .get_mut(first.saturating_add(1))
                    .ok_or_else(|| Error::damaged("BC7 pbit"))? = p;
            }
        }
        PBits::PerEndpoint => {
            for endpoint in 0..endpoints {
                *pbits.get_mut(endpoint).ok_or_else(|| Error::damaged("BC7 pbit"))? =
                    u8::try_from(bits.read(1)?).map_err(|_| Error::damaged("BC7 pbit"))?;
            }
        }
    }
    for endpoint in 0..endpoints {
        for channel in 0..4_usize {
            if channel == 3 && info.alpha_bits == 0 {
                *expanded
                    .get_mut(endpoint)
                    .and_then(|v| v.get_mut(channel))
                    .ok_or_else(|| Error::damaged("BC7 endpoint"))? = 255;
                continue;
            }
            let base = if channel == 3 { info.alpha_bits } else { info.color_bits };
            let raw_value = *raw
                .get(endpoint)
                .and_then(|v| v.get(channel))
                .ok_or_else(|| Error::damaged("BC7 raw endpoint"))?;
            let (value, width) = match info.pbits {
                PBits::None => (raw_value, base),
                _ => (
                    raw_value.checked_shl(1).unwrap_or_default()
                        | u16::from(*pbits.get(endpoint).ok_or_else(|| Error::damaged("BC7 pbit"))?),
                    base.saturating_add(1),
                ),
            };
            *expanded
                .get_mut(endpoint)
                .and_then(|v| v.get_mut(channel))
                .ok_or_else(|| Error::damaged("BC7 endpoint"))? = expand(value, width)?;
        }
    }
    let mut primary = [0_u8; PIXELS];
    let mut secondary = [0_u8; PIXELS];
    read_indices(&mut bits, &mut primary, info.primary_bits, info.subsets, partition)?;
    if info.secondary_bits != 0 {
        read_indices(&mut bits, &mut secondary, info.secondary_bits, info.subsets, partition)?;
    }
    if bits.position != 128 {
        return Err(Error::damaged("BC7 bit accounting did not end at 128"));
    }
    let mut out = [0_u8; RGBA_BYTES];
    for pixel in 0..PIXELS {
        let subset = subset_for(info.subsets, partition, pixel)?;
        let e0 = subset.saturating_mul(2);
        let e1 = e0.saturating_add(1);
        let pi = *primary.get(pixel).ok_or_else(|| Error::damaged("BC7 primary index"))?;
        let si = *secondary
            .get(pixel)
            .ok_or_else(|| Error::damaged("BC7 secondary index"))?;
        let (ci, cb, ai, ab) = if info.secondary_bits == 0 {
            (pi, info.primary_bits, pi, info.primary_bits)
        } else if info.selector_bits != 0 && selector != 0 {
            (si, info.secondary_bits, pi, info.primary_bits)
        } else {
            (pi, info.primary_bits, si, info.secondary_bits)
        };
        let mut rgba = [0_u8; 4];
        for channel in 0..3_usize {
            let a = *expanded
                .get(e0)
                .and_then(|v| v.get(channel))
                .ok_or_else(|| Error::damaged("BC7 endpoint 0"))?;
            let b = *expanded
                .get(e1)
                .and_then(|v| v.get(channel))
                .ok_or_else(|| Error::damaged("BC7 endpoint 1"))?;
            *rgba.get_mut(channel).ok_or_else(|| Error::damaged("BC7 channel"))? = interp(a, b, ci, cb)?;
        }
        *rgba.get_mut(3).ok_or_else(|| Error::damaged("BC7 alpha"))? = if info.alpha_bits == 0 {
            255
        } else {
            interp(
                *expanded
                    .get(e0)
                    .and_then(|v| v.get(3))
                    .ok_or_else(|| Error::damaged("BC7 alpha 0"))?,
                *expanded
                    .get(e1)
                    .and_then(|v| v.get(3))
                    .ok_or_else(|| Error::damaged("BC7 alpha 1"))?,
                ai,
                ab,
            )?
        };
        if rotation != 0 {
            let c = usize::from(rotation.saturating_sub(1));
            if c > 2 {
                return Err(Error::damaged("BC7 rotation"));
            }
            rgba.swap(c, 3);
        }
        put_pixel(&mut out, pixel, rgba)?;
    }
    Ok(out)
}

#[derive(Clone, Copy)]
enum PBits {
    None,
    PerSubset,
    PerEndpoint,
}
#[derive(Clone, Copy)]
struct Mode {
    subsets: u8,
    partition_bits: u8,
    rotation_bits: u8,
    selector_bits: u8,
    color_bits: u8,
    alpha_bits: u8,
    pbits: PBits,
    primary_bits: u8,
    secondary_bits: u8,
}
impl Mode {
    const fn for_number(m: u8) -> Self {
        match m {
            0 => Self {
                subsets: 3,
                partition_bits: 4,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 4,
                alpha_bits: 0,
                pbits: PBits::PerEndpoint,
                primary_bits: 3,
                secondary_bits: 0,
            },
            1 => Self {
                subsets: 2,
                partition_bits: 6,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 6,
                alpha_bits: 0,
                pbits: PBits::PerSubset,
                primary_bits: 3,
                secondary_bits: 0,
            },
            2 => Self {
                subsets: 3,
                partition_bits: 6,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 5,
                alpha_bits: 0,
                pbits: PBits::None,
                primary_bits: 2,
                secondary_bits: 0,
            },
            3 => Self {
                subsets: 2,
                partition_bits: 6,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 7,
                alpha_bits: 0,
                pbits: PBits::PerEndpoint,
                primary_bits: 2,
                secondary_bits: 0,
            },
            4 => Self {
                subsets: 1,
                partition_bits: 0,
                rotation_bits: 2,
                selector_bits: 1,
                color_bits: 5,
                alpha_bits: 6,
                pbits: PBits::None,
                primary_bits: 2,
                secondary_bits: 3,
            },
            5 => Self {
                subsets: 1,
                partition_bits: 0,
                rotation_bits: 2,
                selector_bits: 0,
                color_bits: 7,
                alpha_bits: 8,
                pbits: PBits::None,
                primary_bits: 2,
                secondary_bits: 2,
            },
            6 => Self {
                subsets: 1,
                partition_bits: 0,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 7,
                alpha_bits: 7,
                pbits: PBits::PerEndpoint,
                primary_bits: 4,
                secondary_bits: 0,
            },
            _ => Self {
                subsets: 2,
                partition_bits: 6,
                rotation_bits: 0,
                selector_bits: 0,
                color_bits: 5,
                alpha_bits: 5,
                pbits: PBits::PerEndpoint,
                primary_bits: 2,
                secondary_bits: 0,
            },
        }
    }
}
struct Bits {
    value: u128,
    position: u8,
}
impl Bits {
    const fn new(value: u128) -> Self {
        Self { value, position: 0 }
    }
    fn read(&mut self, count: u8) -> Result<u128> {
        if count == 0 {
            return Ok(0);
        }
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| Error::damaged("BC7 bit overflow"))?;
        if end > 128 {
            return Err(Error::damaged("truncated BC7 block"));
        }
        let mask = 1_u128
            .checked_shl(u32::from(count))
            .unwrap_or_default()
            .saturating_sub(1);
        let value = self.value.checked_shr(u32::from(self.position)).unwrap_or_default() & mask;
        self.position = end;
        Ok(value)
    }
}
fn read_indices(bits: &mut Bits, out: &mut [u8; PIXELS], width: u8, subsets: u8, partition: usize) -> Result<()> {
    for pixel in 0..PIXELS {
        let subset = subset_for(subsets, partition, pixel)?;
        let anchor = anchor_for(subsets, partition, subset)?;
        let actual = if pixel == anchor {
            width.saturating_sub(1)
        } else {
            width
        };
        *out.get_mut(pixel).ok_or_else(|| Error::damaged("BC7 index output"))? =
            u8::try_from(bits.read(actual)?).map_err(|_| Error::damaged("BC7 index"))?;
    }
    Ok(())
}
fn subset_for(subsets: u8, partition: usize, pixel: usize) -> Result<usize> {
    match subsets {
        1 => Ok(0),
        2 => Ok(usize::from(
            *PARTITION_2
                .get(partition)
                .and_then(|r| r.get(pixel))
                .ok_or_else(|| Error::damaged("BC7 partition2"))?,
        )),
        3 => Ok(usize::from(
            *PARTITION_3
                .get(partition)
                .and_then(|r| r.get(pixel))
                .ok_or_else(|| Error::damaged("BC7 partition3"))?,
        )),
        _ => Err(Error::damaged("BC7 subset count")),
    }
}
fn anchor_for(subsets: u8, partition: usize, subset: usize) -> Result<usize> {
    match (subsets, subset) {
        (_, 0) => Ok(0),
        (2, 1) => Ok(usize::from(
            *ANCHOR_2.get(partition).ok_or_else(|| Error::damaged("BC7 anchor2"))?,
        )),
        (3, 1) => Ok(usize::from(
            *ANCHOR_3_SECOND
                .get(partition)
                .ok_or_else(|| Error::damaged("BC7 anchor31"))?,
        )),
        (3, 2) => Ok(usize::from(
            *ANCHOR_3_THIRD
                .get(partition)
                .ok_or_else(|| Error::damaged("BC7 anchor32"))?,
        )),
        _ => Err(Error::damaged("BC7 anchor subset")),
    }
}
fn expand(value: u16, bits: u8) -> Result<u8> {
    if bits == 0 || bits > 8 {
        return Err(Error::damaged("BC7 endpoint precision"));
    }
    let max = 1_u16.checked_shl(u32::from(bits)).unwrap_or_default().saturating_sub(1);
    let v = u32::from(value)
        .saturating_mul(255)
        .saturating_add(u32::from(max).checked_div(2).unwrap_or_default())
        .checked_div(u32::from(max))
        .unwrap_or_default();
    u8::try_from(v).map_err(|_| Error::damaged("BC7 endpoint expansion"))
}
fn interp(a: u8, b: u8, index: u8, bits: u8) -> Result<u8> {
    let weights: &[u8] = match bits {
        2 => &WEIGHTS_2,
        3 => &WEIGHTS_3,
        4 => &WEIGHTS_4,
        _ => return Err(Error::damaged("BC7 index precision")),
    };
    let w = u16::from(
        *weights
            .get(usize::from(index))
            .ok_or_else(|| Error::damaged("BC7 weight"))?,
    );
    let v = u16::from(a)
        .saturating_mul(64_u16.saturating_sub(w))
        .saturating_add(u16::from(b).saturating_mul(w))
        .saturating_add(32)
        .checked_shr(6)
        .unwrap_or_default();
    u8::try_from(v).map_err(|_| Error::damaged("BC7 interpolation"))
}
fn rgb565(v: u16) -> (u8, u8, u8) {
    let r = u8::try_from((v >> 11) & 31).unwrap_or_default();
    let g = u8::try_from((v >> 5) & 63).unwrap_or_default();
    let b = u8::try_from(v & 31).unwrap_or_default();
    (r << 3 | r >> 2, g << 2 | g >> 4, b << 3 | b >> 2)
}
fn mix(a: [u8; 4], b: [u8; 4], bw: u16, d: u16) -> [u8; 4] {
    std::array::from_fn(|c| {
        u8::try_from(
            u16::from(*a.get(c).unwrap_or(&0))
                .saturating_mul(d.saturating_sub(bw))
                .saturating_add(u16::from(*b.get(c).unwrap_or(&0)).saturating_mul(bw))
                .checked_div(d)
                .unwrap_or_default(),
        )
        .unwrap_or_default()
    })
}
fn set_pixel4(out: &mut [[u8; 4]; 4], at: usize, value: [u8; 4]) -> Result<()> {
    *out.get_mut(at).ok_or_else(|| Error::damaged("BC palette slot"))? = value;
    Ok(())
}
fn put_pixel(out: &mut [u8; RGBA_BYTES], pixel: usize, rgba: [u8; 4]) -> Result<()> {
    let start = pixel.checked_mul(4).ok_or_else(|| Error::damaged("BC pixel offset"))?;
    let end = start.checked_add(4).ok_or_else(|| Error::damaged("BC pixel end"))?;
    out.get_mut(start..end)
        .ok_or_else(|| Error::damaged("BC pixel destination"))?
        .copy_from_slice(&rgba);
    Ok(())
}
fn read_u16(b: &[u8], at: usize) -> Result<u16> {
    let e = at.checked_add(2).ok_or_else(|| Error::damaged("BC u16 offset"))?;
    Ok(u16::from_le_bytes(
        <[u8; 2]>::try_from(b.get(at..e).ok_or_else(|| Error::damaged("BC u16"))?)
            .map_err(|_| Error::damaged("BC u16 width"))?,
    ))
}
fn read_u32(b: &[u8], at: usize) -> Result<u32> {
    let e = at.checked_add(4).ok_or_else(|| Error::damaged("BC u32 offset"))?;
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(b.get(at..e).ok_or_else(|| Error::damaged("BC u32"))?)
            .map_err(|_| Error::damaged("BC u32 width"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bc1_hand_calculated_palette() {
        let out = decode_bc1(&[0x00, 0xF8, 0xE0, 0x07, 0xE4, 0xE4, 0xE4, 0xE4]).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            out.get(0..16),
            Some(&[255, 0, 0, 255, 0, 255, 0, 255, 170, 85, 0, 255, 85, 170, 0, 255][..])
        );
    }
    #[test]
    fn bc3_hand_calculated_alpha() {
        let block = [255, 0, 0, 0, 0, 0, 0, 0, 0x00, 0xF8, 0x00, 0xF8, 0, 0, 0, 0];
        let out = decode_bc3(&block).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(out.get(0..4), Some(&[255, 0, 0, 255][..]));
    }
    #[test]
    fn bc4_hand_calculated_endpoint() {
        let out = decode_bc4(&[255, 0, 0, 0, 0, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(out.get(0..4), Some(&[255, 0, 0, 255][..]));
    }
    #[test]
    fn bc5_hand_calculated_channels() {
        let out =
            decode_bc5(&[255, 0, 0, 0, 0, 0, 0, 0, 128, 64, 0, 0, 0, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(out.get(0..4), Some(&[255, 128, 0, 255][..]));
    }
    #[test]
    fn bc7_mode6_hand_built_endpoint_zero() {
        let mut w = TestBits::default();
        for _ in 0..6 {
            w.push(0, 1);
        }
        w.push(1, 1);
        for v in [127_u128, 0, 0, 0, 0, 127, 127, 127] {
            w.push(v, 7);
        }
        w.push(1, 1);
        w.push(1, 1);
        w.push(0, 3);
        for _ in 1..16 {
            w.push(0, 4);
        }
        assert_eq!(w.position, 128);
        let out = decode_bc7(&w.value.to_le_bytes()).unwrap_or_else(|e| panic!("{e:?}"));
        for pixel in 0..16_usize {
            let at = pixel.saturating_mul(4);
            assert_eq!(out.get(at..at.saturating_add(4)), Some(&[255, 0, 0, 255][..]));
        }
    }
    #[derive(Default)]
    struct TestBits {
        value: u128,
        position: u8,
    }
    impl TestBits {
        fn push(&mut self, value: u128, count: u8) {
            self.value |= value.checked_shl(u32::from(self.position)).unwrap_or_default();
            self.position = self.position.saturating_add(count);
        }
    }
}
