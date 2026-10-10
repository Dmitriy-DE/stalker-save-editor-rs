//! PNG decoder to RGBA8.
//!
//! Chunk CRCs, zlib Adler-32, scanline filters and Adam7 interlacing are validated. Dimensions
//! are capped before allocation and inflated scanline size is computed before decompression.

use crate::inflate::inflate_zlib;
use sse_core::{Error, Result};

const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: usize = 16_777_216;
const MAX_CHUNK: usize = 32 * 1024 * 1024;
const MAX_IDAT_SIZE: usize = 32 * 1024 * 1024;

/// Decoded PNG pixels in row-major RGBA8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel: red, green, blue, alpha.
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
struct Header {
    width: u32,
    height: u32,
    bit_depth: u8,
    color_type: u8,
    interlace: u8,
}

#[derive(Clone, Copy, Debug)]
struct Transparency<'a> {
    data: &'a [u8],
}

/// Decodes a PNG image to RGBA8.
///
/// # Errors
/// Returns [`Error::Damaged`] for invalid PNG structure, CRCs, dimensions, colour/depth
/// combinations, filters, palette indices, zlib data or Adam7 scanline sizes.
pub fn decode(input: &[u8]) -> Result<Image> {
    if input.get(0..8) != Some(PNG_SIGNATURE.as_slice()) {
        return Err(Error::damaged("invalid PNG signature"));
    }
    let mut position = 8_usize;
    let mut header: Option<Header> = None;
    let mut palette: Option<&[u8]> = None;
    let mut transparency: Option<Transparency<'_>> = None;
    let mut idat = Vec::<u8>::new();
    let mut saw_idat = false;
    let mut ended_idat = false;
    let mut saw_iend = false;

    while position < input.len() {
        let length_u32 = read_be_u32(input, position)?;
        let length = usize::try_from(length_u32).map_err(|_| Error::damaged("PNG chunk length does not fit usize"))?;
        if length > MAX_CHUNK {
            return Err(Error::damaged("PNG chunk exceeds size limit"));
        }
        let type_offset = checked_add(position, 4)?;
        let chunk_type = checked_range(input, type_offset, 4)?;
        let data_offset = checked_add(type_offset, 4)?;
        let data = checked_range(input, data_offset, length)?;
        let crc_offset = checked_add(data_offset, length)?;
        let expected_crc = read_be_u32(input, crc_offset)?;
        let next = checked_add(crc_offset, 4)?;
        let actual_crc = crc32_parts(chunk_type, data);
        if actual_crc != expected_crc {
            return Err(Error::damaged("PNG chunk CRC mismatch"));
        }

        match chunk_type {
            b"IHDR" => {
                if header.is_some() || position != 8 || length != 13 {
                    return Err(Error::damaged("invalid PNG IHDR placement/length"));
                }
                header = Some(parse_header(data)?);
            }
            b"PLTE" => {
                if header.is_none() || saw_idat {
                    return Err(Error::damaged("invalid PNG PLTE placement"));
                }
                if data.is_empty() || data.len() > 768 || data.len().checked_rem(3) != Some(0) {
                    return Err(Error::damaged("invalid PNG palette length"));
                }
                palette = Some(data);
            }
            b"tRNS" => {
                let value = header.ok_or_else(|| Error::damaged("tRNS before IHDR"))?;
                if saw_idat || transparency.is_some() {
                    return Err(Error::damaged("invalid PNG tRNS placement"));
                }
                validate_transparency(value, data, palette)?;
                transparency = Some(Transparency { data });
            }
            b"IDAT" => {
                let value = header.ok_or_else(|| Error::damaged("IDAT before IHDR"))?;
                if ended_idat {
                    return Err(Error::damaged("PNG IDAT chunks are not consecutive"));
                }
                if value.color_type == 3 && palette.is_none() {
                    return Err(Error::damaged("indexed PNG has no PLTE"));
                }
                let wanted = idat
                    .len()
                    .checked_add(data.len())
                    .ok_or_else(|| Error::damaged("PNG IDAT size overflow"))?;
                if wanted > input.len() || wanted > MAX_IDAT_SIZE {
                    return Err(Error::damaged("PNG IDAT aggregate exceeds its size limit"));
                }
                idat.extend_from_slice(data);
                saw_idat = true;
            }
            b"IEND" => {
                if length != 0 || !saw_idat {
                    return Err(Error::damaged("invalid PNG IEND"));
                }
                saw_iend = true;
                position = next;
                break;
            }
            _ => {
                if saw_idat {
                    ended_idat = true;
                }
                let critical = chunk_type.first().copied().is_some_and(|byte| byte & 0x20 == 0);
                if critical {
                    return Err(Error::damaged("unknown critical PNG chunk"));
                }
            }
        }
        if saw_idat && chunk_type != b"IDAT" {
            ended_idat = true;
        }
        position = next;
    }

    if !saw_iend || position != input.len() {
        return Err(Error::damaged("PNG is missing final IEND or has trailing bytes"));
    }
    let header = header.ok_or_else(|| Error::damaged("PNG has no IHDR"))?;
    let pixels_count = usize::try_from(header.width)
        .ok()
        .and_then(|width| {
            usize::try_from(header.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| Error::damaged("PNG pixel-count overflow"))?;
    if pixels_count > MAX_PIXELS {
        return Err(Error::damaged("PNG pixel count exceeds limit"));
    }
    let filtered_size = filtered_size(header)?;
    let filtered = inflate_zlib(&idat, filtered_size)?;
    if filtered.len() != filtered_size {
        return Err(Error::damaged("PNG inflated scanline size mismatch"));
    }

    let pixel_bytes = pixels_count
        .checked_mul(4)
        .ok_or_else(|| Error::damaged("PNG RGBA size overflow"))?;
    let mut pixels = vec![0_u8; pixel_bytes];

    if header.interlace == 0 {
        decode_pass(
            &filtered,
            header,
            palette,
            transparency,
            Pass {
                x_start: 0,
                y_start: 0,
                x_step: 1,
                y_step: 1,
            },
            &mut pixels,
        )?;
    } else {
        const PASSES: [Pass; 7] = [
            Pass {
                x_start: 0,
                y_start: 0,
                x_step: 8,
                y_step: 8,
            },
            Pass {
                x_start: 4,
                y_start: 0,
                x_step: 8,
                y_step: 8,
            },
            Pass {
                x_start: 0,
                y_start: 4,
                x_step: 4,
                y_step: 8,
            },
            Pass {
                x_start: 2,
                y_start: 0,
                x_step: 4,
                y_step: 4,
            },
            Pass {
                x_start: 0,
                y_start: 2,
                x_step: 2,
                y_step: 4,
            },
            Pass {
                x_start: 1,
                y_start: 0,
                x_step: 2,
                y_step: 2,
            },
            Pass {
                x_start: 0,
                y_start: 1,
                x_step: 1,
                y_step: 2,
            },
        ];
        let mut offset = 0_usize;
        for pass in PASSES.iter().copied() {
            let (pass_width, pass_height) = pass_dimensions(header.width, header.height, pass)?;
            if pass_width == 0 || pass_height == 0 {
                continue;
            }
            let size = pass_size(header, pass_width, pass_height)?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::damaged("Adam7 pass range overflow"))?;
            let bytes = filtered
                .get(offset..end)
                .ok_or_else(|| Error::damaged("truncated Adam7 pass"))?;
            decode_pass(bytes, header, palette, transparency, pass, &mut pixels)?;
            offset = end;
        }
        if offset != filtered.len() {
            return Err(Error::damaged("Adam7 passes do not consume inflated data"));
        }
    }

    Ok(Image {
        width: header.width,
        height: header.height,
        pixels,
    })
}

fn parse_header(data: &[u8]) -> Result<Header> {
    let width = read_be_u32(data, 0)?;
    let height = read_be_u32(data, 4)?;
    let bit_depth = *data.get(8).ok_or_else(|| Error::damaged("short PNG IHDR bit depth"))?;
    let color_type = *data
        .get(9)
        .ok_or_else(|| Error::damaged("short PNG IHDR colour type"))?;
    let compression = *data
        .get(10)
        .ok_or_else(|| Error::damaged("short PNG IHDR compression"))?;
    let filter = *data.get(11).ok_or_else(|| Error::damaged("short PNG IHDR filter"))?;
    let interlace = *data.get(12).ok_or_else(|| Error::damaged("short PNG IHDR interlace"))?;
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::damaged("PNG dimensions are outside limits"));
    }
    if compression != 0 || filter != 0 || interlace > 1 {
        return Err(Error::damaged("unsupported PNG IHDR method"));
    }
    let valid_depth = match color_type {
        0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16),
        2 => matches!(bit_depth, 8 | 16),
        3 => matches!(bit_depth, 1 | 2 | 4 | 8),
        4 | 6 => matches!(bit_depth, 8 | 16),
        _ => false,
    };
    if !valid_depth {
        return Err(Error::damaged("invalid PNG colour type / bit depth"));
    }
    Ok(Header {
        width,
        height,
        bit_depth,
        color_type,
        interlace,
    })
}

fn validate_transparency(header: Header, data: &[u8], palette: Option<&[u8]>) -> Result<()> {
    match header.color_type {
        0 if data.len() == 2 => Ok(()),
        2 if data.len() == 6 => Ok(()),
        3 => {
            let palette = palette.ok_or_else(|| Error::damaged("indexed tRNS before PLTE"))?;
            let entries = palette
                .len()
                .checked_div(3)
                .ok_or_else(|| Error::damaged("palette entry-count error"))?;
            if data.len() > entries {
                Err(Error::damaged("indexed tRNS has more entries than PLTE"))
            } else {
                Ok(())
            }
        }
        4 | 6 => Err(Error::damaged("tRNS is forbidden for PNG with alpha channel")),
        _ => Err(Error::damaged("invalid PNG tRNS length")),
    }
}

#[derive(Clone, Copy)]
struct Pass {
    x_start: u32,
    y_start: u32,
    x_step: u32,
    y_step: u32,
}

fn filtered_size(header: Header) -> Result<usize> {
    if header.interlace == 0 {
        return pass_size(header, header.width, header.height);
    }
    const PASSES: [Pass; 7] = [
        Pass {
            x_start: 0,
            y_start: 0,
            x_step: 8,
            y_step: 8,
        },
        Pass {
            x_start: 4,
            y_start: 0,
            x_step: 8,
            y_step: 8,
        },
        Pass {
            x_start: 0,
            y_start: 4,
            x_step: 4,
            y_step: 8,
        },
        Pass {
            x_start: 2,
            y_start: 0,
            x_step: 4,
            y_step: 4,
        },
        Pass {
            x_start: 0,
            y_start: 2,
            x_step: 2,
            y_step: 4,
        },
        Pass {
            x_start: 1,
            y_start: 0,
            x_step: 2,
            y_step: 2,
        },
        Pass {
            x_start: 0,
            y_start: 1,
            x_step: 1,
            y_step: 2,
        },
    ];
    let mut total = 0_usize;
    for pass in PASSES.iter().copied() {
        let (width, height) = pass_dimensions(header.width, header.height, pass)?;
        if width != 0 && height != 0 {
            total = total
                .checked_add(pass_size(header, width, height)?)
                .ok_or_else(|| Error::damaged("PNG Adam7 filtered-size overflow"))?;
        }
    }
    Ok(total)
}

fn pass_dimensions(width: u32, height: u32, pass: Pass) -> Result<(u32, u32)> {
    if pass.x_step == 0 || pass.y_step == 0 {
        return Err(Error::damaged("Adam7 pass has zero step"));
    }
    let pass_width = if width <= pass.x_start {
        0
    } else {
        div_ceil_u32(
            width
                .checked_sub(pass.x_start)
                .ok_or_else(|| Error::damaged("Adam7 width underflow"))?,
            pass.x_step,
        )?
    };
    let pass_height = if height <= pass.y_start {
        0
    } else {
        div_ceil_u32(
            height
                .checked_sub(pass.y_start)
                .ok_or_else(|| Error::damaged("Adam7 height underflow"))?,
            pass.y_step,
        )?
    };
    Ok((pass_width, pass_height))
}

fn div_ceil_u32(value: u32, divisor: u32) -> Result<u32> {
    if divisor == 0 {
        return Err(Error::damaged("division by zero"));
    }
    value
        .checked_add(
            divisor
                .checked_sub(1)
                .ok_or_else(|| Error::damaged("divisor underflow"))?,
        )
        .and_then(|adjusted| adjusted.checked_div(divisor))
        .ok_or_else(|| Error::damaged("ceil division overflow"))
}

fn channels(color_type: u8) -> Result<u8> {
    match color_type {
        0 => Ok(1),
        2 => Ok(3),
        3 => Ok(1),
        4 => Ok(2),
        6 => Ok(4),
        _ => Err(Error::damaged("invalid PNG colour type")),
    }
}

fn bits_per_pixel(header: Header) -> Result<u32> {
    u32::from(channels(header.color_type)?)
        .checked_mul(u32::from(header.bit_depth))
        .ok_or_else(|| Error::damaged("PNG bits-per-pixel overflow"))
}

fn row_bytes(header: Header, width: u32) -> Result<usize> {
    let bits = bits_per_pixel(header)?
        .checked_mul(width)
        .ok_or_else(|| Error::damaged("PNG row bit-count overflow"))?;
    let bytes = bits
        .checked_add(7)
        .and_then(|value| value.checked_div(8))
        .ok_or_else(|| Error::damaged("PNG row byte-count overflow"))?;
    usize::try_from(bytes).map_err(|_| Error::damaged("PNG row size does not fit usize"))
}

fn filter_bpp(header: Header) -> Result<usize> {
    let bits = bits_per_pixel(header)?;
    let bytes = bits
        .checked_add(7)
        .and_then(|value| value.checked_div(8))
        .ok_or_else(|| Error::damaged("PNG filter bytes-per-pixel overflow"))?;
    usize::try_from(bytes.max(1)).map_err(|_| Error::damaged("PNG filter bpp does not fit usize"))
}

fn pass_size(header: Header, width: u32, height: u32) -> Result<usize> {
    let row = row_bytes(header, width)?;
    let with_filter = row
        .checked_add(1)
        .ok_or_else(|| Error::damaged("PNG filtered row size overflow"))?;
    with_filter
        .checked_mul(usize::try_from(height).map_err(|_| Error::damaged("PNG pass height does not fit usize"))?)
        .ok_or_else(|| Error::damaged("PNG pass size overflow"))
}

fn decode_pass(
    filtered: &[u8],
    header: Header,
    palette: Option<&[u8]>,
    transparency: Option<Transparency<'_>>,
    pass: Pass,
    output: &mut [u8],
) -> Result<()> {
    let (width, height) = pass_dimensions(header.width, header.height, pass)?;
    if width == 0 || height == 0 {
        if filtered.is_empty() {
            return Ok(());
        }
        return Err(Error::damaged("empty PNG pass has bytes"));
    }
    let row_len = row_bytes(header, width)?;
    let bpp = filter_bpp(header)?;
    let expected = pass_size(header, width, height)?;
    if filtered.len() != expected {
        return Err(Error::damaged("PNG pass byte count mismatch"));
    }

    let mut previous = vec![0_u8; row_len];
    let mut current = vec![0_u8; row_len];
    let mut source = 0_usize;
    let mut row = 0_u32;
    while row < height {
        let filter = *filtered
            .get(source)
            .ok_or_else(|| Error::damaged("missing PNG filter byte"))?;
        source = source
            .checked_add(1)
            .ok_or_else(|| Error::damaged("PNG source offset overflow"))?;
        let end = source
            .checked_add(row_len)
            .ok_or_else(|| Error::damaged("PNG row source overflow"))?;
        let encoded = filtered
            .get(source..end)
            .ok_or_else(|| Error::damaged("truncated PNG row"))?;
        current.copy_from_slice(encoded);
        unfilter(filter, &mut current, &previous, bpp)?;
        write_pixels(
            &current,
            width,
            row,
            PixelWrite {
                header,
                palette,
                transparency,
                pass,
            },
            output,
        )?;
        core::mem::swap(&mut previous, &mut current);
        source = end;
        row = row
            .checked_add(1)
            .ok_or_else(|| Error::damaged("PNG row counter overflow"))?;
    }
    Ok(())
}

fn unfilter(filter: u8, row: &mut [u8], previous: &[u8], bpp: usize) -> Result<()> {
    if previous.len() != row.len() || bpp == 0 {
        return Err(Error::damaged("invalid PNG filter row state"));
    }
    let mut index = 0_usize;
    while index < row.len() {
        let raw = *row.get(index).ok_or_else(|| Error::damaged("PNG filter row index"))?;
        let left = index
            .checked_sub(bpp)
            .and_then(|position| row.get(position).copied())
            .unwrap_or(0);
        let up = previous.get(index).copied().unwrap_or(0);
        let up_left = index
            .checked_sub(bpp)
            .and_then(|position| previous.get(position).copied())
            .unwrap_or(0);
        let predictor = match filter {
            0 => 0,
            1 => left,
            2 => up,
            3 => {
                let sum = u16::from(left)
                    .checked_add(u16::from(up))
                    .ok_or_else(|| Error::damaged("PNG average predictor overflow"))?;
                u8::try_from(sum.checked_div(2).unwrap_or_default())
                    .map_err(|_| Error::damaged("PNG average predictor does not fit byte"))?
            }
            4 => paeth(left, up, up_left)?,
            _ => return Err(Error::damaged("unknown PNG scanline filter")),
        };
        let value = if filter == 0 { raw } else { raw.wrapping_add(predictor) };
        let slot = row
            .get_mut(index)
            .ok_or_else(|| Error::damaged("PNG filter output index"))?;
        *slot = value;
        index = index
            .checked_add(1)
            .ok_or_else(|| Error::damaged("PNG filter index overflow"))?;
    }
    Ok(())
}

fn paeth(a: u8, b: u8, c: u8) -> Result<u8> {
    let ai = i32::from(a);
    let bi = i32::from(b);
    let ci = i32::from(c);
    let p = ai
        .checked_add(bi)
        .and_then(|value| value.checked_sub(ci))
        .ok_or_else(|| Error::damaged("PNG Paeth predictor overflow"))?;
    let pa = p.checked_sub(ai).unwrap_or_default().abs();
    let pb = p.checked_sub(bi).unwrap_or_default().abs();
    let pc = p.checked_sub(ci).unwrap_or_default().abs();
    if pa <= pb && pa <= pc {
        Ok(a)
    } else if pb <= pc {
        Ok(b)
    } else {
        Ok(c)
    }
}

#[derive(Clone, Copy)]
struct PixelWrite<'a> {
    header: Header,
    palette: Option<&'a [u8]>,
    transparency: Option<Transparency<'a>>,
    pass: Pass,
}

fn write_pixels(
    row_bytes: &[u8],
    pass_width: u32,
    pass_row: u32,
    context: PixelWrite<'_>,
    output: &mut [u8],
) -> Result<()> {
    let header = context.header;
    let pass = context.pass;
    let channel_count = usize::from(channels(header.color_type)?);
    let mut x = 0_u32;
    while x < pass_width {
        let sample_base = usize::try_from(x)
            .ok()
            .and_then(|value| value.checked_mul(channel_count))
            .ok_or_else(|| Error::damaged("PNG sample index overflow"))?;
        let rgba = pixel_rgba(row_bytes, sample_base, header, context.palette, context.transparency)?;
        let destination_x = pass
            .x_start
            .checked_add(
                x.checked_mul(pass.x_step)
                    .ok_or_else(|| Error::damaged("Adam7 x overflow"))?,
            )
            .ok_or_else(|| Error::damaged("Adam7 destination x overflow"))?;
        let destination_y = pass
            .y_start
            .checked_add(
                pass_row
                    .checked_mul(pass.y_step)
                    .ok_or_else(|| Error::damaged("Adam7 y overflow"))?,
            )
            .ok_or_else(|| Error::damaged("Adam7 destination y overflow"))?;
        write_rgba(output, header.width, destination_x, destination_y, rgba)?;
        x = x
            .checked_add(1)
            .ok_or_else(|| Error::damaged("PNG pixel counter overflow"))?;
    }
    Ok(())
}

fn pixel_rgba(
    row: &[u8],
    sample_base: usize,
    header: Header,
    palette: Option<&[u8]>,
    transparency: Option<Transparency<'_>>,
) -> Result<[u8; 4]> {
    match header.color_type {
        0 => {
            let gray_raw = sample(row, sample_base, header.bit_depth)?;
            let gray = scale_sample(gray_raw, header.bit_depth)?;
            let alpha = if let Some(value) = transparency {
                let transparent = read_be_u16(value.data, 0)?;
                if gray_raw == transparent {
                    0
                } else {
                    255
                }
            } else {
                255
            };
            Ok([gray, gray, gray, alpha])
        }
        2 => {
            let r_raw = sample(row, sample_base, header.bit_depth)?;
            let g_index = sample_base
                .checked_add(1)
                .ok_or_else(|| Error::damaged("PNG green sample index overflow"))?;
            let b_index = sample_base
                .checked_add(2)
                .ok_or_else(|| Error::damaged("PNG blue sample index overflow"))?;
            let g_raw = sample(row, g_index, header.bit_depth)?;
            let b_raw = sample(row, b_index, header.bit_depth)?;
            let r = scale_sample(r_raw, header.bit_depth)?;
            let g = scale_sample(g_raw, header.bit_depth)?;
            let b = scale_sample(b_raw, header.bit_depth)?;
            let alpha = if let Some(value) = transparency {
                let tr = read_be_u16(value.data, 0)?;
                let tg = read_be_u16(value.data, 2)?;
                let tb = read_be_u16(value.data, 4)?;
                if r_raw == tr && g_raw == tg && b_raw == tb {
                    0
                } else {
                    255
                }
            } else {
                255
            };
            Ok([r, g, b, alpha])
        }
        3 => {
            let index = usize::from(
                u8::try_from(sample(row, sample_base, header.bit_depth)?)
                    .map_err(|_| Error::damaged("palette index does not fit byte"))?,
            );
            let palette = palette.ok_or_else(|| Error::damaged("indexed PNG has no palette"))?;
            let base = index
                .checked_mul(3)
                .ok_or_else(|| Error::damaged("palette offset overflow"))?;
            let r = *palette
                .get(base)
                .ok_or_else(|| Error::damaged("palette index outside PLTE"))?;
            let g = *palette
                .get(checked_add(base, 1)?)
                .ok_or_else(|| Error::damaged("palette index outside PLTE"))?;
            let b = *palette
                .get(checked_add(base, 2)?)
                .ok_or_else(|| Error::damaged("palette index outside PLTE"))?;
            let alpha = transparency
                .and_then(|value| value.data.get(index).copied())
                .unwrap_or(255);
            Ok([r, g, b, alpha])
        }
        4 => {
            let gray_raw = sample(row, sample_base, header.bit_depth)?;
            let alpha_index = checked_add(sample_base, 1)?;
            let alpha_raw = sample(row, alpha_index, header.bit_depth)?;
            let gray = scale_sample(gray_raw, header.bit_depth)?;
            let alpha = scale_sample(alpha_raw, header.bit_depth)?;
            Ok([gray, gray, gray, alpha])
        }
        6 => {
            let r = scale_sample(sample(row, sample_base, header.bit_depth)?, header.bit_depth)?;
            let g = scale_sample(
                sample(row, checked_add(sample_base, 1)?, header.bit_depth)?,
                header.bit_depth,
            )?;
            let b = scale_sample(
                sample(row, checked_add(sample_base, 2)?, header.bit_depth)?,
                header.bit_depth,
            )?;
            let a = scale_sample(
                sample(row, checked_add(sample_base, 3)?, header.bit_depth)?,
                header.bit_depth,
            )?;
            Ok([r, g, b, a])
        }
        _ => Err(Error::damaged("invalid PNG colour type")),
    }
}

fn sample(row: &[u8], index: usize, depth: u8) -> Result<u16> {
    match depth {
        1 | 2 | 4 => {
            let bits = usize::from(depth);
            let bit_offset = index
                .checked_mul(bits)
                .ok_or_else(|| Error::damaged("packed PNG sample offset overflow"))?;
            let byte_index = bit_offset
                .checked_div(8)
                .ok_or_else(|| Error::damaged("packed PNG sample division"))?;
            let within = bit_offset
                .checked_rem(8)
                .ok_or_else(|| Error::damaged("packed PNG sample remainder"))?;
            let shift = 8_usize
                .checked_sub(bits)
                .and_then(|value| value.checked_sub(within))
                .ok_or_else(|| Error::damaged("packed PNG sample crosses byte"))?;
            let byte = *row
                .get(byte_index)
                .ok_or_else(|| Error::damaged("packed PNG sample outside row"))?;
            let mask = 1_u16
                .checked_shl(u32::from(depth))
                .and_then(|value| value.checked_sub(1))
                .ok_or_else(|| Error::damaged("PNG sample mask overflow"))?;
            Ok(u16::from(byte)
                .checked_shr(u32::try_from(shift).map_err(|_| Error::damaged("PNG sample shift"))?)
                .unwrap_or_default()
                & mask)
        }
        8 => row
            .get(index)
            .copied()
            .map(u16::from)
            .ok_or_else(|| Error::damaged("PNG sample outside row")),
        16 => {
            let offset = index
                .checked_mul(2)
                .ok_or_else(|| Error::damaged("16-bit PNG sample offset overflow"))?;
            read_be_u16(row, offset)
        }
        _ => Err(Error::damaged("unsupported PNG sample depth")),
    }
}

fn scale_sample(value: u16, depth: u8) -> Result<u8> {
    match depth {
        16 => u8::try_from(value.checked_shr(8).unwrap_or_default())
            .map_err(|_| Error::damaged("16-bit PNG sample reduction overflow")),
        8 => u8::try_from(value).map_err(|_| Error::damaged("8-bit PNG sample overflow")),
        1 | 2 | 4 => {
            let maximum = 1_u32
                .checked_shl(u32::from(depth))
                .and_then(|value| value.checked_sub(1))
                .ok_or_else(|| Error::damaged("PNG sample maximum overflow"))?;
            let scaled = u32::from(value)
                .checked_mul(255)
                .and_then(|value| value.checked_div(maximum))
                .ok_or_else(|| Error::damaged("PNG sample scaling overflow"))?;
            u8::try_from(scaled).map_err(|_| Error::damaged("scaled PNG sample does not fit byte"))
        }
        _ => Err(Error::damaged("unsupported PNG sample depth")),
    }
}

fn write_rgba(output: &mut [u8], width: u32, x: u32, y: u32, rgba: [u8; 4]) -> Result<()> {
    if x >= width {
        return Err(Error::damaged("PNG destination x outside image"));
    }
    let pixel = usize::try_from(y)
        .ok()
        .and_then(|row| usize::try_from(width).ok().and_then(|w| row.checked_mul(w)))
        .and_then(|row| usize::try_from(x).ok().and_then(|column| row.checked_add(column)))
        .ok_or_else(|| Error::damaged("PNG destination pixel offset overflow"))?;
    let offset = pixel
        .checked_mul(4)
        .ok_or_else(|| Error::damaged("PNG RGBA offset overflow"))?;
    let target = output
        .get_mut(offset..checked_add(offset, 4)?)
        .ok_or_else(|| Error::damaged("PNG destination outside output"))?;
    target.copy_from_slice(&rgba);
    Ok(())
}

fn read_be_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = checked_range(data, offset, 2)?;
    let array = <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged("short PNG u16"))?;
    Ok(u16::from_be_bytes(array))
}

fn read_be_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = checked_range(data, offset, 4)?;
    let array = <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("short PNG u32"))?;
    Ok(u32::from_be_bytes(array))
}

fn checked_add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| Error::damaged("PNG offset overflow"))
}

fn checked_range(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = checked_add(offset, length)?;
    data.get(offset..end)
        .ok_or_else(|| Error::damaged("PNG range outside input"))
}

fn crc32_parts(first: &[u8], second: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in first.iter().chain(second.iter()).copied() {
        crc ^= u32::from(byte);
        let mut bit = 0_u8;
        while bit < 8 {
            let mask = if crc & 1 != 0 { 0xEDB8_8320 } else { 0 };
            crc = crc.checked_shr(1).unwrap_or_default() ^ mask;
            bit = bit.checked_add(1).unwrap_or(8);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{decode, paeth, unfilter};
    use sse_core::Error;

    // 1x1 RGBA pixel (12, 34, 56, 78), emitted with Python stdlib zlib and hand-built PNG chunks.
    const TINY_RGBA: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4, 0x89, 0x00, 0x00, 0x00,
        0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xE0, 0x51, 0xB2, 0xF0, 0x03, 0x00, 0x01, 0x59, 0x00, 0xB5,
        0x46, 0xCB, 0x2A, 0x07, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    // Reference PNGs and expected RGBA pixels generated with Python's stdlib zlib and struct
    // (generator kept outside the repo); the expected pixels were computed without this decoder.
    const GRAY8_ALL_FILTERS: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x03, 0x00, 0x00, 0x00, 0x05, 0x08, 0x00, 0x00, 0x00, 0x00, 0xA5, 0x1A, 0x09, 0x7E, 0x00, 0x00, 0x00,
        0x1A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x50, 0xF5, 0x62, 0xE4, 0x56, 0x55, 0x65, 0xE2, 0xE6,
        0xE6, 0x66, 0x16, 0x93, 0x90, 0x60, 0x01, 0xD2, 0x00, 0x0F, 0xA2, 0x01, 0x57, 0xC8, 0x57, 0xBD, 0x61, 0x00,
        0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    const GRAY8_ALL_FILTERS_EXPECTED: &[u8] = &[
        0x00, 0x00, 0x00, 0xFF, 0x25, 0x25, 0x25, 0xFF, 0x4A, 0x4A, 0x4A, 0xFF, 0x0B, 0x0B, 0x0B, 0xFF, 0x30, 0x30,
        0x30, 0xFF, 0x55, 0x55, 0x55, 0xFF, 0x16, 0x16, 0x16, 0xFF, 0x3B, 0x3B, 0x3B, 0xFF, 0x60, 0x60, 0x60, 0xFF,
        0x21, 0x21, 0x21, 0xFF, 0x46, 0x46, 0x46, 0xFF, 0x6B, 0x6B, 0x6B, 0xFF, 0x2C, 0x2C, 0x2C, 0xFF, 0x51, 0x51,
        0x51, 0xFF, 0x76, 0x76, 0x76, 0xFF,
    ];

    const RGB16: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x10, 0x02, 0x00, 0x00, 0x00, 0x46, 0x73, 0xFD, 0x33, 0x00, 0x00, 0x00,
        0x16, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x10, 0x32, 0x59, 0x7D, 0x96, 0x9F, 0x9F, 0xE1, 0x3F, 0x03,
        0x43, 0x43, 0xFD, 0x7F, 0x00, 0x1D, 0x39, 0x04, 0xDA, 0x21, 0x54, 0x8D, 0x6A, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    const RGB16_EXPECTED: &[u8] = &[0x12, 0xAB, 0x0F, 0xFF, 0xFF, 0x00, 0x7F, 0xFF];

    const PALETTE_TRNS: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x03, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x00, 0x00, 0x00, 0x66, 0x8E, 0xFC, 0x27, 0x00, 0x00, 0x00,
        0x09, 0x50, 0x4C, 0x54, 0x45, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x2D, 0x4A, 0xCD, 0x8A,
        0x00, 0x00, 0x00, 0x03, 0x74, 0x52, 0x4E, 0x53, 0x00, 0xFF, 0x80, 0x84, 0xEA, 0xBA, 0x8C, 0x00, 0x00, 0x00,
        0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x90, 0x00, 0x00, 0x00, 0x1A, 0x00, 0x19, 0x2D, 0x88, 0xF4,
        0x36, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    const PALETTE_TRNS_EXPECTED: &[u8] = &[0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x00, 0xFF, 0x00, 0x00, 0xFF, 0x80];

    const ADAM7_GRAY8: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x03, 0x00, 0x00, 0x00, 0x03, 0x08, 0x00, 0x00, 0x00, 0x01, 0x04, 0x44, 0xDA, 0xF5, 0x00, 0x00, 0x00,
        0x17, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x60, 0x60, 0x62, 0x10, 0x11, 0x63, 0x60, 0x64, 0x10,
        0x65, 0xE0, 0xE2, 0xE6, 0x01, 0x00, 0x02, 0x65, 0x00, 0x64, 0x64, 0x87, 0xE5, 0xED, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    const ADAM7_GRAY8_EXPECTED: &[u8] = &[
        0x00, 0x00, 0x00, 0xFF, 0x01, 0x01, 0x01, 0xFF, 0x02, 0x02, 0x02, 0xFF, 0x0A, 0x0A, 0x0A, 0xFF, 0x0B, 0x0B,
        0x0B, 0xFF, 0x0C, 0x0C, 0x0C, 0xFF, 0x14, 0x14, 0x14, 0xFF, 0x15, 0x15, 0x15, 0xFF, 0x16, 0x16, 0x16, 0xFF,
    ];

    const BAD_IDAT_CRC: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x00, 0x00, 0x00, 0x00, 0x3A, 0x7E, 0x9B, 0x55, 0x00, 0x00, 0x00,
        0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x60, 0x07, 0x00, 0x00, 0x09, 0x00, 0x08, 0xDF, 0xDC, 0x3C,
        0x73, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn grayscale_8bit_decodes_every_filter_type() {
        let image = decode(GRAY8_ALL_FILTERS).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!((image.width, image.height), (3, 5));
        assert_eq!(image.pixels, GRAY8_ALL_FILTERS_EXPECTED);
    }

    #[test]
    fn sixteen_bit_rgb_keeps_the_high_byte_of_each_sample() {
        let image = decode(RGB16).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!((image.width, image.height), (1, 2));
        assert_eq!(image.pixels, RGB16_EXPECTED);
    }

    #[test]
    fn palette_with_trns_alpha_decodes_each_index() {
        let image = decode(PALETTE_TRNS).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(image.pixels, PALETTE_TRNS_EXPECTED);
    }

    #[test]
    fn adam7_interlaced_image_is_reassembled_in_place() {
        let image = decode(ADAM7_GRAY8).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!((image.width, image.height), (3, 3));
        assert_eq!(image.pixels, ADAM7_GRAY8_EXPECTED);
    }

    #[test]
    fn a_bad_idat_chunk_crc_is_refused() {
        assert!(matches!(decode(BAD_IDAT_CRC), Err(Error::Damaged(_))));
    }

    #[test]
    fn tiny_rgba_decodes() {
        let image = decode(TINY_RGBA).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(image.width, 1);
        assert_eq!(image.height, 1);
        assert_eq!(image.pixels, [12, 34, 56, 78]);
    }

    #[test]
    fn paeth_known_values() {
        assert_eq!(paeth(10, 20, 30), Ok(10));
        assert_eq!(paeth(100, 80, 90), Ok(90));
    }

    #[test]
    fn all_five_filters_restore_rows() {
        let previous = [10_u8, 20, 30, 40];
        let expected = [20_u8, 30, 40, 50];

        let mut none = expected;
        assert_eq!(unfilter(0, &mut none, &previous, 1), Ok(()));
        assert_eq!(none, expected);

        let mut sub = [20_u8, 10, 10, 10];
        assert_eq!(unfilter(1, &mut sub, &previous, 1), Ok(()));
        assert_eq!(sub, expected);

        let mut up = [10_u8, 10, 10, 10];
        assert_eq!(unfilter(2, &mut up, &previous, 1), Ok(()));
        assert_eq!(up, expected);

        let mut average = [15_u8, 10, 10, 10];
        assert_eq!(unfilter(3, &mut average, &previous, 1), Ok(()));
        assert_eq!(average, expected);

        // Encode expected row against Paeth predictors.
        let mut encoded = [0_u8; 4];
        let mut index = 0_usize;
        while index < 4 {
            let left = index.checked_sub(1).and_then(|i| expected.get(i).copied()).unwrap_or(0);
            let up = previous.get(index).copied().unwrap_or(0);
            let up_left = index.checked_sub(1).and_then(|i| previous.get(i).copied()).unwrap_or(0);
            let predictor = paeth(left, up, up_left).unwrap_or(0);
            if let (Some(slot), Some(value)) = (encoded.get_mut(index), expected.get(index).copied()) {
                *slot = value.wrapping_sub(predictor);
            }
            index = index.checked_add(1).unwrap_or(4);
        }
        assert_eq!(unfilter(4, &mut encoded, &previous, 1), Ok(()));
        assert_eq!(encoded, expected);
    }

    #[test]
    fn truncations_are_errors() {
        let mut length = 0_usize;
        while length < TINY_RGBA.len() {
            let prefix = TINY_RGBA.get(..length).unwrap_or_default();
            assert!(matches!(decode(prefix), Err(Error::Damaged(_))));
            length = length.checked_add(1).unwrap_or(TINY_RGBA.len());
        }
    }
}
