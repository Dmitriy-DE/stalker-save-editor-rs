//! DDS texture decoder supporting DXT1, DXT3, DXT5, and uncompressed 24/32-bit formats.
//!
//! Provides image cropping for extracting item icons from texture atlases.

use sse_core::fields::read_u32;
use sse_core::{Error, Result};

/// An RGBA8 image decoded from a DDS file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbaImage {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Pixel data in row-major RGBA order (4 bytes per pixel).
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    /// Creates a new RGBA image.
    #[must_use]
    pub fn new(width: usize, height: usize, pixels: Vec<u8>) -> Self {
        Self { width, height, pixels }
    }

    /// Crops a rectangular area from this image.
    ///
    /// Returns `None` if coordinates are outside image bounds or dimensions are 0.
    #[must_use]
    pub fn crop(&self, x: usize, y: usize, width: usize, height: usize) -> Option<Self> {
        if x >= self.width || y >= self.height || width == 0 || height == 0 {
            return None;
        }

        let w = width.min(self.width.saturating_sub(x));
        let h = height.min(self.height.saturating_sub(y));
        let total_bytes = w.checked_mul(h)?.checked_mul(4)?;
        let mut result = vec![0u8; total_bytes];

        for row in 0..h {
            let src_y = y.checked_add(row)?;
            let src_start = src_y.checked_mul(self.width)?.checked_add(x)?.checked_mul(4)?;
            let row_bytes = w.checked_mul(4)?;
            let src_end = src_start.checked_add(row_bytes)?;

            let src_slice = self.pixels.get(src_start..src_end)?;
            let dst_start = row.checked_mul(row_bytes)?;
            let dst_end = dst_start.checked_add(row_bytes)?;
            let dst_slice = result.get_mut(dst_start..dst_end)?;

            dst_slice.copy_from_slice(src_slice);
        }

        Some(Self::new(w, h, result))
    }
}

/// DDS decoder.
pub struct DdsImage;

impl DdsImage {
    const HEADER_SIZE: usize = 128;
    const MAXIMUM_PIXELS: u64 = 16_777_216;

    /// Decodes a DDS image from bytes.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] if image header or data is malformed or unsupported.
    pub fn decode(data: &[u8]) -> Result<RgbaImage> {
        if data.len() < Self::HEADER_SIZE || !data.starts_with(b"DDS ") {
            return Err(Error::damaged("Not a DDS image."));
        }

        let height_u32 = read_u32(data, 12)?;
        let width_u32 = read_u32(data, 16)?;
        if height_u32 == 0 || width_u32 == 0 {
            return Err(Error::damaged("DDS dimensions are invalid."));
        }

        let total_pixels = (u64::from(width_u32)).saturating_mul(u64::from(height_u32));
        if total_pixels > Self::MAXIMUM_PIXELS {
            return Err(Error::damaged("DDS dimensions are invalid."));
        }

        let width = usize::try_from(width_u32).map_err(|_| Error::damaged("DDS dimensions are invalid."))?;
        let height = usize::try_from(height_u32).map_err(|_| Error::damaged("DDS dimensions are invalid."))?;

        let pixel_flags = read_u32(data, 80)?;
        let payload = data.get(Self::HEADER_SIZE..).unwrap_or(&[]);

        if (pixel_flags & 0x4) != 0 {
            let fourcc = data.get(84..88).ok_or_else(|| Error::damaged("missing fourcc"))?;
            let pixels = decode_dxt(payload, width, height, fourcc)?;
            return Ok(RgbaImage::new(width, height, pixels));
        }

        if (pixel_flags & 0x40) != 0 {
            let pixels = decode_uncompressed(data, payload, width, height)?;
            return Ok(RgbaImage::new(width, height, pixels));
        }

        Err(Error::damaged("Unsupported DDS pixel format."))
    }
}

fn decode_uncompressed(header: &[u8], payload: &[u8], width: usize, height: usize) -> Result<Vec<u8>> {
    let bits = read_u32(header, 88)?;
    if bits != 24 && bits != 32 {
        return Err(Error::damaged("Unsupported uncompressed DDS pixel size."));
    }

    let bytes_per_pixel = (bits / 8) as usize;
    let minimum_pitch = width
        .checked_mul(bytes_per_pixel)
        .ok_or_else(|| Error::damaged("DDS pitch calculation overflow"))?;

    let mut stored_pitch = read_u32(header, 20)? as usize;
    if stored_pitch == 0 {
        stored_pitch = minimum_pitch;
    }

    if stored_pitch > i32::MAX as usize || stored_pitch < minimum_pitch {
        return Err(Error::damaged("DDS pitch is invalid."));
    }

    let needed_len = (height.saturating_sub(1))
        .checked_mul(stored_pitch)
        .and_then(|p| p.checked_add(minimum_pitch))
        .ok_or_else(|| Error::damaged("DDS payload size overflow"))?;

    if needed_len > payload.len() {
        return Err(Error::damaged("DDS pixel data is truncated."));
    }

    let red_mask = read_u32(header, 92)?;
    let green_mask = read_u32(header, 96)?;
    let blue_mask = read_u32(header, 100)?;
    let alpha_mask = read_u32(header, 104)?;

    for &mask in &[red_mask, green_mask, blue_mask, alpha_mask] {
        if mask != 0 {
            let ones = mask.count_ones();
            let tz = mask.trailing_zeros();
            let shifted = mask >> tz;
            let expected = if ones >= 32 {
                0xFFFF_FFFFu32
            } else {
                (1u32 << ones).wrapping_sub(1)
            };
            if ones > 16 || shifted != expected {
                return Err(Error::damaged("Unsupported DDS channel mask."));
            }
        }
    }

    let total_bytes = width
        .checked_mul(height)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| Error::damaged("Image size overflow"))?;
    let mut rgba = vec![0u8; total_bytes];
    let channels = [
        Channel::new(red_mask),
        Channel::new(green_mask),
        Channel::new(blue_mask),
        Channel::new(alpha_mask),
    ];

    for y in 0..height {
        let row_start = y
            .checked_mul(stored_pitch)
            .ok_or_else(|| Error::damaged("pitch overflow"))?;
        for x in 0..width {
            let pixel_start = row_start
                .checked_add(
                    x.checked_mul(bytes_per_pixel)
                        .ok_or_else(|| Error::damaged("pixel overflow"))?,
                )
                .ok_or_else(|| Error::damaged("pixel overflow"))?;

            let pixel_bytes = payload
                .get(
                    pixel_start
                        ..pixel_start
                            .checked_add(bytes_per_pixel)
                            .ok_or_else(|| Error::damaged("overflow"))?,
                )
                .ok_or_else(|| Error::damaged("DDS pixel data is truncated."))?;

            // 24-bit pixels are zero-extended; both widths are at most four bytes.
            let mut value_bytes = [0u8; 4];
            if let Some(dst) = value_bytes.get_mut(..bytes_per_pixel) {
                dst.copy_from_slice(pixel_bytes);
            }
            let value = u32::from_le_bytes(value_bytes);

            let out_idx = y
                .checked_mul(width)
                .and_then(|p| p.checked_add(x))
                .and_then(|p| p.checked_mul(4))
                .ok_or_else(|| Error::damaged("index overflow"))?;

            let r = channels[0].extract(value);
            let g = channels[1].extract(value);
            let b = channels[2].extract(value);
            let a = if alpha_mask != 0 {
                channels[3].extract(value)
            } else {
                255
            };

            if let Some(slice) = rgba.get_mut(out_idx..out_idx.saturating_add(4)) {
                slice.copy_from_slice(&[r, g, b, a]);
            }
        }
    }

    Ok(rgba)
}

fn decode_dxt(payload: &[u8], width: usize, height: usize, fourcc: &[u8]) -> Result<Vec<u8>> {
    let kind = if fourcc == b"DXT1" {
        1
    } else if fourcc == b"DXT3" {
        3
    } else if fourcc == b"DXT5" {
        5
    } else {
        return Err(Error::damaged("Unsupported DDS compression."));
    };

    let block_size = if kind == 1 { 8 } else { 16 };
    let total_bytes = width
        .checked_mul(height)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| Error::damaged("size overflow"))?;
    let mut rgba = vec![0u8; total_bytes];

    let mut colors = [0u32; 4];
    let mut alphas = [255u8; 16];
    let mut alpha_values = [0u8; 8];
    let mut offset = 0usize;

    let mut block_y = 0usize;
    while block_y < height {
        let mut block_x = 0usize;
        while block_x < width {
            let next_offset = offset
                .checked_add(block_size)
                .ok_or_else(|| Error::damaged("offset overflow"))?;
            let block = payload
                .get(offset..next_offset)
                .ok_or_else(|| Error::damaged("DDS block data is truncated."))?;
            offset = next_offset;

            let color_bits: u32;

            if kind == 1 {
                decode_colors(block, true, &mut colors)?;
                let cb_bytes = block.get(4..8).ok_or_else(|| Error::damaged("block truncated"))?;
                color_bits = u32::from_le_bytes(cb_bytes.try_into().unwrap_or([0; 4]));
                alphas.fill(255);
            } else if kind == 3 {
                let color_block = block.get(8..16).ok_or_else(|| Error::damaged("block truncated"))?;
                decode_colors(color_block, false, &mut colors)?;

                let alpha_bytes = block.get(0..8).ok_or_else(|| Error::damaged("block truncated"))?;
                let alpha_bits = u64::from_le_bytes(alpha_bytes.try_into().unwrap_or([0; 8]));
                for (idx, a_slot) in alphas.iter_mut().enumerate() {
                    let shift = u32::try_from(idx).unwrap_or(0).wrapping_mul(4);
                    let raw4 = u8::try_from((alpha_bits >> shift) & 0xF).unwrap_or(0);
                    *a_slot = raw4.wrapping_mul(17);
                }

                let cb_bytes = block.get(12..16).ok_or_else(|| Error::damaged("block truncated"))?;
                color_bits = u32::from_le_bytes(cb_bytes.try_into().unwrap_or([0; 4]));
            } else {
                let color_block = block.get(8..16).ok_or_else(|| Error::damaged("block truncated"))?;
                decode_colors(color_block, false, &mut colors)?;

                let a0 = u32::from(*block.first().ok_or_else(|| Error::damaged("block truncated"))?);
                let a1 = u32::from(*block.get(1).ok_or_else(|| Error::damaged("block truncated"))?);

                alpha_values[0] = u8::try_from(a0).unwrap_or(0);
                alpha_values[1] = u8::try_from(a1).unwrap_or(0);

                if a0 > a1 {
                    for (step, slot) in (1u32..).zip(&mut alpha_values[2..]) {
                        let numer = (7u32.saturating_sub(step))
                            .wrapping_mul(a0)
                            .wrapping_add(step.wrapping_mul(a1));
                        *slot = numer.checked_div(7).and_then(|v| u8::try_from(v).ok()).unwrap_or(0);
                    }
                } else {
                    for (step, slot) in (1u32..).zip(&mut alpha_values[2..6]) {
                        let numer = (5u32.saturating_sub(step))
                            .wrapping_mul(a0)
                            .wrapping_add(step.wrapping_mul(a1));
                        *slot = numer.checked_div(5).and_then(|v| u8::try_from(v).ok()).unwrap_or(0);
                    }
                    alpha_values[6] = 0;
                    alpha_values[7] = 255;
                }

                let mut alpha_bits = 0u64;
                for idx in 0..6usize {
                    let b = u64::from(
                        *block
                            .get(2usize.wrapping_add(idx))
                            .ok_or_else(|| Error::damaged("block truncated"))?,
                    );
                    let shift = u32::try_from(idx).unwrap_or(0).wrapping_mul(8);
                    alpha_bits |= b << shift;
                }

                for (idx, a_slot) in alphas.iter_mut().enumerate() {
                    let shift = u32::try_from(idx).unwrap_or(0).wrapping_mul(3);
                    let a_idx = usize::try_from((alpha_bits >> shift) & 0x7).unwrap_or(0);
                    *a_slot = alpha_values.get(a_idx).copied().unwrap_or(255);
                }

                let cb_bytes = block.get(12..16).ok_or_else(|| Error::damaged("block truncated"))?;
                color_bits = u32::from_le_bytes(cb_bytes.try_into().unwrap_or([0; 4]));
            }

            for local_y in 0..4usize {
                for local_x in 0..4usize {
                    let px = block_x.saturating_add(local_x);
                    let py = block_y.saturating_add(local_y);
                    if px >= width || py >= height {
                        continue;
                    }

                    let local_idx = local_y.wrapping_mul(4).wrapping_add(local_x);
                    let shift = u32::try_from(local_idx).unwrap_or(0).wrapping_mul(2);
                    let color_idx = usize::try_from((color_bits >> shift) & 0x3).unwrap_or(0);
                    let color = colors.get(color_idx).copied().unwrap_or(0);

                    let out_pixel = py
                        .checked_mul(width)
                        .and_then(|p| p.checked_add(px))
                        .and_then(|p| p.checked_mul(4))
                        .ok_or_else(|| Error::damaged("pixel index overflow"))?;

                    let r = u8::try_from((color >> 16) & 0xFF).unwrap_or(0);
                    let g = u8::try_from((color >> 8) & 0xFF).unwrap_or(0);
                    let b = u8::try_from(color & 0xFF).unwrap_or(0);
                    let a = if color == 0xFF00_0000 {
                        0
                    } else {
                        alphas.get(local_idx).copied().unwrap_or(255)
                    };

                    if let Some(slice) = rgba.get_mut(out_pixel..out_pixel.saturating_add(4)) {
                        slice.copy_from_slice(&[r, g, b, a]);
                    }
                }
            }

            block_x = block_x.saturating_add(4);
        }
        block_y = block_y.saturating_add(4);
    }

    Ok(rgba)
}

fn decode_colors(block: &[u8], allow_transparent: bool, colors: &mut [u32; 4]) -> Result<()> {
    let b0 = block.get(0..2).ok_or_else(|| Error::damaged("color truncated"))?;
    let b1 = block.get(2..4).ok_or_else(|| Error::damaged("color truncated"))?;
    let first = u16::from_le_bytes(b0.try_into().unwrap_or([0; 2]));
    let second = u16::from_le_bytes(b1.try_into().unwrap_or([0; 2]));

    let (r0, g0, b0) = rgb565(first);
    let (r1, g1, b1) = rgb565(second);

    colors[0] = pack_rgb(r0, g0, b0);
    colors[1] = pack_rgb(r1, g1, b1);

    if first > second || !allow_transparent {
        let r2 = (2u32.wrapping_mul(r0).wrapping_add(r1)).checked_div(3).unwrap_or(0);
        let g2 = (2u32.wrapping_mul(g0).wrapping_add(g1)).checked_div(3).unwrap_or(0);
        let b2 = (2u32.wrapping_mul(b0).wrapping_add(b1)).checked_div(3).unwrap_or(0);
        colors[2] = pack_rgb(r2, g2, b2);

        let r3 = (r0.wrapping_add(2u32.wrapping_mul(r1))).checked_div(3).unwrap_or(0);
        let g3 = (g0.wrapping_add(2u32.wrapping_mul(g1))).checked_div(3).unwrap_or(0);
        let b3 = (b0.wrapping_add(2u32.wrapping_mul(b1))).checked_div(3).unwrap_or(0);
        colors[3] = pack_rgb(r3, g3, b3);
    } else {
        let r2 = r0.wrapping_add(r1).checked_div(2).unwrap_or(0);
        let g2 = g0.wrapping_add(g1).checked_div(2).unwrap_or(0);
        let b2 = b0.wrapping_add(b1).checked_div(2).unwrap_or(0);
        colors[2] = pack_rgb(r2, g2, b2);
        colors[3] = 0xFF00_0000; // Marker for transparent black
    }

    Ok(())
}

fn pack_rgb(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

fn rgb565(val: u16) -> (u32, u32, u32) {
    let r = ((u32::from(val >> 11)) & 0x1F)
        .wrapping_mul(255)
        .checked_div(31)
        .unwrap_or(0);
    let g = ((u32::from(val >> 5)) & 0x3F)
        .wrapping_mul(255)
        .checked_div(63)
        .unwrap_or(0);
    let b = (u32::from(val) & 0x1F).wrapping_mul(255).checked_div(31).unwrap_or(0);
    (r, g, b)
}

/// One colour channel of an uncompressed mask, with its shift and scale worked out once per image.
#[derive(Clone, Copy)]
struct Channel {
    mask: u32,
    shift: u32,
    maximum: u32,
    /// Output for every raw value when the channel is at most 8 bits wide; empty otherwise.
    table: Option<[u8; 256]>,
}

impl Channel {
    fn new(mask: u32) -> Self {
        let width_bits = mask.count_ones();
        let maximum = if width_bits >= 32 {
            0xFFFF_FFFFu32
        } else {
            (1u32 << width_bits).wrapping_sub(1)
        };
        let shift = mask.trailing_zeros();
        let table = (width_bits <= 8 && maximum != 0).then(|| {
            let mut table = [0u8; 256];
            for (raw, slot) in (0u32..).zip(table.iter_mut()) {
                *slot = scale(raw, maximum);
            }
            table
        });
        Self {
            mask,
            shift,
            maximum,
            table,
        }
    }

    /// Same result as [`extract_channel`], without recomputing the mask per pixel.
    fn extract(self, value: u32) -> u8 {
        if self.mask == 0 || self.maximum == 0 {
            return 0;
        }
        let raw = (value & self.mask) >> self.shift;
        match &self.table {
            Some(table) => table.get(usize::try_from(raw).unwrap_or(0)).copied().unwrap_or(0),
            None => scale(raw, self.maximum),
        }
    }
}

/// Scales a raw channel value to 0..=255 with rounding.
fn scale(raw: u32, maximum: u32) -> u8 {
    let half_max = maximum / 2;
    let numer = raw.wrapping_mul(255).wrapping_add(half_max);
    u8::try_from(numer.checked_div(maximum).unwrap_or(0)).unwrap_or(0)
}

#[cfg(test)]
fn extract_channel(value: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let width_bits = mask.count_ones();
    let raw = (value & mask) >> shift;
    let maximum = if width_bits >= 32 {
        0xFFFF_FFFFu32
    } else {
        (1u32 << width_bits).wrapping_sub(1)
    };
    if maximum == 0 {
        return 0;
    }
    let half_max = maximum.checked_div(2).unwrap_or(0);
    let numer = raw.wrapping_mul(255).wrapping_add(half_max);
    let div = numer.checked_div(maximum).unwrap_or(0);
    u8::try_from(div).unwrap_or(0)
}

#[cfg(test)]
mod channel_table_tests {
    use super::{extract_channel, Channel};

    #[test]
    fn table_and_division_agree_for_every_raw_value() {
        // Masks at 1..=8 bits use the lookup table; 9..=16 bits use the division path.
        for width in 1_u32..=16 {
            for shift in [0_u32, 3] {
                let mask = ((1_u32 << width) - 1) << shift;
                let channel = Channel::new(mask);
                let limit = 1_u32 << width;
                for raw in 0..limit {
                    let value = raw << shift;
                    assert_eq!(
                        channel.extract(value),
                        extract_channel(value, mask),
                        "width {width} shift {shift} raw {raw}"
                    );
                }
            }
        }
    }

    #[test]
    fn zero_mask_is_zero() {
        assert_eq!(Channel::new(0).extract(0xFFFF_FFFF), 0);
    }
}
