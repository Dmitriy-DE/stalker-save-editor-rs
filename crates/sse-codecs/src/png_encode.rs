//! Minimal deterministic RGBA8 PNG encoder for golden images.
//! Uses zlib with RFC 1951 stored blocks: deliberately simple, dependency-free and lossless.

use sse_core::{Error, Result};

const SIG: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

/// Encode an RGBA8 image as a non-interlaced PNG.
pub fn encode_rgba8(width: u32, height: u32, pixels: &[u8]) -> Result<Vec<u8>> {
    if width == 0 || height == 0 {
        return Err(Error::Refused("PNG dimensions must be non-zero".to_owned()));
    }
    let row = usize::try_from(width)
        .ok()
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| Error::damaged("PNG row overflow"))?;
    let expected = row
        .checked_mul(usize::try_from(height).map_err(|_| Error::damaged("PNG height"))?)
        .ok_or_else(|| Error::damaged("PNG image overflow"))?;
    if pixels.len() != expected {
        return Err(Error::Refused("RGBA buffer size does not match dimensions".to_owned()));
    }
    let scan_cap = expected
        .checked_add(usize::try_from(height).unwrap_or(0))
        .ok_or_else(|| Error::damaged("PNG scanline overflow"))?;
    let mut scan = Vec::with_capacity(scan_cap);
    for y in 0..usize::try_from(height).map_err(|_| Error::damaged("PNG height"))? {
        scan.push(0);
        let start = y.checked_mul(row).ok_or_else(|| Error::damaged("PNG row offset"))?;
        let end = start.checked_add(row).ok_or_else(|| Error::damaged("PNG row end"))?;
        scan.extend_from_slice(pixels.get(start..end).ok_or_else(|| Error::damaged("PNG pixels"))?);
    }

    let mut z = Vec::with_capacity(scan.len().saturating_add(scan.len() / 65_535 * 5).saturating_add(16));
    z.extend_from_slice(&[0x78, 0x01]);
    let mut pos = 0usize;
    if scan.is_empty() {
        z.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while pos < scan.len() {
        let n = (scan.len() - pos).min(65_535);
        z.push(if pos + n == scan.len() { 1 } else { 0 });
        let n16 = u16::try_from(n).map_err(|_| Error::damaged("PNG stored block"))?;
        z.extend_from_slice(&n16.to_le_bytes());
        z.extend_from_slice(&(!n16).to_le_bytes());
        z.extend_from_slice(
            scan.get(pos..pos + n)
                .ok_or_else(|| Error::damaged("PNG stored data"))?,
        );
        pos = pos.checked_add(n).ok_or_else(|| Error::damaged("PNG stored offset"))?;
    }
    z.extend_from_slice(&adler32(&scan).to_be_bytes());

    let mut out = Vec::with_capacity(z.len().saturating_add(57));
    out.extend_from_slice(SIG);
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(&mut out, b"IHDR", &ihdr)?;
    chunk(&mut out, b"IDAT", &z)?;
    chunk(&mut out, b"IEND", &[])?;
    Ok(out)
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) -> Result<()> {
    let len = u32::try_from(data.len()).map_err(|_| Error::damaged("PNG chunk too large"))?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = 0xffff_ffffu32;
    for b in kind.iter().chain(data) {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    out.extend_from_slice(&(!crc).to_be_bytes());
    Ok(())
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for x in data {
        a = (a + u32::from(*x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png;

    #[test]
    fn round_trip() {
        let p = [255, 0, 0, 255, 0, 255, 0, 128];
        let e = encode_rgba8(2, 1, &p).unwrap();
        let d = png::decode(&e).unwrap();
        assert_eq!((d.width, d.height, d.pixels), (2, 1, p.to_vec()));
    }

    #[test]
    fn rejects_bad_length() {
        assert!(encode_rgba8(2, 2, &[0; 4]).is_err());
    }
}
