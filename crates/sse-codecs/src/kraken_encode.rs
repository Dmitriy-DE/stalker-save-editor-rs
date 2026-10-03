//! Kraken encoder for S.T.A.L.K.E.R. 2 save streams.
//!
//! The LZ parser uses bounded hash chains. Tables are emitted as Kraken entropy
//! arrays; incompressible sub-blocks fall back to the format's raw LZ chunk.

use sse_core::{Error, Result};

const BLOCK: usize = 0x40000;
const SUB_BLOCK: usize = 0x20000;
const HASH_BITS: usize = 16;
const HASH_SIZE: usize = 1 << HASH_BITS;
const CHAIN_LIMIT: usize = 64;
const MIN_MATCH: usize = 4;

#[derive(Clone, Copy, Debug)]
struct Match {
    len: usize,
    distance: usize,
}

#[derive(Default)]
struct Bits {
    bits: Vec<bool>,
}

impl Bits {
    fn put(&mut self, value: u32, count: u32) {
        for shift in (0..count).rev() {
            self.bits.push(value & (1_u32 << shift) != 0);
        }
    }

    fn gamma(&mut self, value: u32) -> Result<()> {
        if value == 0 {
            return Err(Error::damaged("Kraken gamma zero"));
        }
        let n = 31_u32.saturating_sub(value.leading_zeros());
        for _ in 0..n {
            self.bits.push(false);
        }
        self.put(value, n.saturating_add(1));
        Ok(())
    }

    fn length(&mut self, value: u32) -> Result<()> {
        let biased = value
            .checked_add(64)
            .ok_or_else(|| Error::damaged("Kraken length overflow"))?;
        let log = 31_u32.saturating_sub(biased.leading_zeros());
        if !(6..=18).contains(&log) {
            return Err(Error::Refused(
                "Kraken extended length outside encoder range".to_owned(),
            ));
        }
        let n = log.saturating_sub(6);
        for _ in 0..n {
            self.bits.push(false);
        }
        self.put(biased, n.saturating_add(7));
        Ok(())
    }

    fn bytes(&self) -> Vec<u8> {
        let mut out = vec![0_u8; self.bits.len().saturating_add(7) / 8];
        for (index, bit) in self.bits.iter().copied().enumerate() {
            if bit {
                let byte = index / 8;
                let shift = 7_usize.saturating_sub(index % 8);
                if let Some(slot) = out.get_mut(byte) {
                    *slot |= 1_u8 << shift;
                }
            }
        }
        out
    }
}

fn hash4(data: &[u8], at: usize) -> Option<usize> {
    let a = u32::from(*data.get(at)?);
    let b = u32::from(*data.get(at.checked_add(1)?)?);
    let c = u32::from(*data.get(at.checked_add(2)?)?);
    let d = u32::from(*data.get(at.checked_add(3)?)?);
    let h = a
        .wrapping_mul(0x1e35_a7bd)
        .wrapping_add(b.wrapping_mul(0x9e37_79b1))
        .wrapping_add(c.wrapping_mul(0x85eb_ca6b))
        .wrapping_add(d);
    usize::try_from(h >> (32 - HASH_BITS)).ok()
}

fn best_match(data: &[u8], at: usize, end: usize, head: &[Option<usize>], prev: &[Option<usize>]) -> Match {
    let Some(hash) = hash4(data, at) else {
        return Match { len: 0, distance: 0 };
    };
    let mut candidate = head.get(hash).copied().flatten();
    let mut best = Match { len: 0, distance: 0 };
    let mut steps = 0usize;
    while let Some(pos) = candidate {
        if pos >= at {
            break;
        }
        let distance = at.saturating_sub(pos);
        let max = end.saturating_sub(at);
        let mut len = 0usize;
        while len < max && data.get(pos.saturating_add(len)) == data.get(at.saturating_add(len)) {
            len = len.saturating_add(1);
        }
        if len > best.len {
            best = Match { len, distance };
            if len == max {
                break;
            }
        }
        steps = steps.saturating_add(1);
        if steps >= CHAIN_LIMIT {
            break;
        }
        candidate = prev.get(pos).copied().flatten();
    }
    best
}

fn insert(data: &[u8], at: usize, head: &mut [Option<usize>], prev: &mut [Option<usize>]) {
    if let Some(hash) = hash4(data, at) {
        if let Some(slot) = prev.get_mut(at) {
            *slot = head.get(hash).copied().flatten();
        }
        if let Some(slot) = head.get_mut(hash) {
            *slot = Some(at);
        }
    }
}

fn raw_entropy(data: &[u8]) -> Result<Vec<u8>> {
    if data.len() > 0x3ffff {
        return Err(Error::Refused("Kraken entropy array exceeds 18-bit size".to_owned()));
    }
    let mut out = Vec::with_capacity(data.len().saturating_add(3));
    if data.len() <= 0x0fff {
        let size = u16::try_from(data.len()).map_err(|_| Error::damaged("entropy size"))?;
        out.extend_from_slice(&(0x8000_u16 | size).to_be_bytes());
    } else {
        let size = u32::try_from(data.len()).map_err(|_| Error::damaged("entropy size"))?;
        out.push(u8::try_from((size >> 16) & 0xff).unwrap_or_default());
        out.push(u8::try_from((size >> 8) & 0xff).unwrap_or_default());
        out.push(u8::try_from(size & 0xff).unwrap_or_default());
    }
    out.extend_from_slice(data);
    Ok(out)
}

fn distance_code(distance: usize) -> Result<(u8, u32, u32)> {
    let q = u32::try_from(distance)
        .map_err(|_| Error::Refused("Kraken match distance exceeds u32".to_owned()))?
        .checked_add(8)
        .ok_or_else(|| Error::damaged("Kraken distance overflow"))?;
    let log = 31_u32.saturating_sub(q.leading_zeros());
    if log < 3 {
        return Err(Error::damaged("Kraken distance below eight"));
    }
    let width = log.saturating_sub(3);
    if width > 26 {
        return Err(Error::Refused("Kraken match distance exceeds scale-1 range".to_owned()));
    }
    let high = q >> width;
    if !(8..=15).contains(&high) {
        return Err(Error::damaged("Kraken distance high bits"));
    }
    let low = if width == 0 { 0 } else { q & 1_u32.checked_shl(width).unwrap_or_default().saturating_sub(1) };
    let packed = width
        .checked_mul(8)
        .and_then(|v| v.checked_add(high.saturating_sub(8)))
        .and_then(|v| u8::try_from(v).ok())
        .ok_or_else(|| Error::damaged("Kraken packed distance"))?;
    Ok((packed, low, width))
}

fn encode_subblock(data: &[u8], start: usize, end: usize) -> Result<Vec<u8>> {
    let raw = data
        .get(start..end)
        .ok_or_else(|| Error::damaged("Kraken sub-block range"))?;
    if raw.len() < 16 {
        return raw_chunk(raw);
    }

    let mut head = vec![None; HASH_SIZE];
    let mut prev = vec![None; end];
    let history = start.saturating_sub(SUB_BLOCK);
    for at in history..start {
        insert(data, at, &mut head, &mut prev);
    }

    let mut literals = Vec::new();
    let mut commands = Vec::new();
    let mut offsets = Vec::new();
    let mut lengths = Vec::new();
    let mut extended = Vec::new();
    let mut at = start;
    let first = if start == 0 { 8 } else { 0 };
    at = at.saturating_add(first);
    let mut literal_start = at;

    while at < end {
        let found = best_match(data, at, end, &head, &prev);
        if found.len < MIN_MATCH {
            insert(data, at, &mut head, &mut prev);
            at = at.saturating_add(1);
            continue;
        }
        let literal_len = at.saturating_sub(literal_start);
        let match_len = found.len;
        let lit_code = if literal_len < 3 {
            u8::try_from(literal_len).unwrap_or_default()
        } else {
            lengths.push(length_token(literal_len, &mut extended)?);
            3
        };
        let match_code = if match_len <= 16 {
            u8::try_from(match_len.saturating_sub(2)).unwrap_or_default()
        } else {
            lengths.push(length_token(match_len.saturating_sub(14), &mut extended)?);
            15
        };
        literals.extend_from_slice(
            data.get(literal_start..at)
                .ok_or_else(|| Error::damaged("Kraken literal range"))?,
        );
        commands.push(0xc0 | (match_code << 2) | lit_code);
        offsets.push(found.distance);

        let match_end = at.saturating_add(match_len).min(end);
        for pos in at..match_end {
            insert(data, pos, &mut head, &mut prev);
        }
        at = match_end;
        literal_start = at;
    }
    literals.extend_from_slice(
        data.get(literal_start..end)
            .ok_or_else(|| Error::damaged("Kraken final literals"))?,
    );

    if commands.is_empty() {
        return raw_chunk(raw);
    }

    let mut packed_offsets = Vec::with_capacity(offsets.len());
    let mut forward = Bits::default();
    let mut backward = Bits::default();
    backward.gamma(
        u32::try_from(extended.len())
            .map_err(|_| Error::damaged("Kraken extension count"))?
            .saturating_add(1),
    )?;
    for (index, distance) in offsets.iter().copied().enumerate() {
        let (packed, low, width) = distance_code(distance)?;
        packed_offsets.push(packed);
        if index % 2 == 0 {
            forward.put(low, width);
        } else {
            backward.put(low, width);
        }
    }
    for (index, value) in extended.iter().copied().enumerate() {
        if index % 2 == 0 {
            forward.length(value)?;
        } else {
            backward.length(value)?;
        }
    }
    let mut bitstream = forward.bytes();
    let mut back_bytes = backward.bytes();
    back_bytes.reverse();
    bitstream.extend_from_slice(&back_bytes);
    while bitstream.len() < 8 {
        bitstream.insert(bitstream.len() / 2, 0);
    }

    let mut payload = Vec::new();
    if start == 0 {
        payload.extend_from_slice(data.get(..8).ok_or_else(|| Error::damaged("Kraken first literals"))?);
    }
    payload.extend_from_slice(&raw_entropy(&literals)?);
    payload.extend_from_slice(&raw_entropy(&commands)?);
    payload.push(128);
    payload.extend_from_slice(&raw_entropy(&packed_offsets)?);
    payload.extend_from_slice(&raw_entropy(&lengths)?);
    payload.extend_from_slice(&bitstream);

    if payload.len() >= raw.len() {
        return raw_chunk(raw);
    }
    let mut out = Vec::with_capacity(payload.len().saturating_add(3));
    let size = u32::try_from(payload.len()).map_err(|_| Error::damaged("Kraken LZ size"))?;
    let header = 0x800000_u32 | (1_u32 << 19) | size;
    put_u24(&mut out, header);
    out.extend_from_slice(&payload);
    Ok(out)
}

fn length_token(value: usize, extended: &mut Vec<u32>) -> Result<u8> {
    if value < 3 {
        return Err(Error::damaged("Kraken length token below three"));
    }
    if value <= 257 {
        return u8::try_from(value.saturating_sub(3)).map_err(|_| Error::damaged("Kraken length byte"));
    }
    extended.push(
        u32::try_from(value.saturating_sub(258))
            .map_err(|_| Error::Refused("Kraken extended length exceeds u32".to_owned()))?,
    );
    Ok(255)
}

fn raw_chunk(raw: &[u8]) -> Result<Vec<u8>> {
    let size = u32::try_from(raw.len()).map_err(|_| Error::damaged("Kraken raw chunk size"))?;
    let mut out = Vec::with_capacity(raw.len().saturating_add(3));
    put_u24(&mut out, 0x800000 | size);
    out.extend_from_slice(raw);
    Ok(out)
}

fn put_u24(out: &mut Vec<u8>, value: u32) {
    out.push(u8::try_from((value >> 16) & 0xff).unwrap_or_default());
    out.push(u8::try_from((value >> 8) & 0xff).unwrap_or_default());
    out.push(u8::try_from(value & 0xff).unwrap_or_default());
}

/// Compress bytes to a Kraken stream accepted by this crate's decoder.
pub fn compress(input: &[u8]) -> Result<Vec<u8>> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let mut block_start = 0usize;
    while block_start < input.len() {
        let block_end = block_start.saturating_add(BLOCK).min(input.len());
        out.extend_from_slice(&[0x0c, 6]);
        let mut payload = Vec::new();
        let mut sub = block_start;
        while sub < block_end {
            let end = sub.saturating_add(SUB_BLOCK).min(block_end);
            payload.extend_from_slice(&encode_subblock(input, sub, end)?);
            sub = end;
        }
        let raw_len = block_end.saturating_sub(block_start);
        if payload.len() >= raw_len {
            out.truncate(out.len().saturating_sub(2));
            out.extend_from_slice(&[0x4c, 6]);
            out.extend_from_slice(
                input
                    .get(block_start..block_end)
                    .ok_or_else(|| Error::damaged("Kraken raw block"))?,
            );
        } else {
            let q =
                u32::try_from(payload.len().saturating_sub(1)).map_err(|_| Error::damaged("Kraken quantum size"))?;
            put_u24(&mut out, q);
            out.extend_from_slice(&payload);
        }
        block_start = block_end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kraken::decompress_into;
    use std::fs;
    use std::path::Path;

    fn round_trip(data: &[u8]) {
        let packed = compress(data).unwrap_or_else(|error| panic!("{error:?}"));
        let mut decoded = vec![0_u8; data.len()];
        decompress_into(&packed, &mut decoded).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(decoded, data);
    }

    #[test]
    fn basic_round_trips() {
        round_trip(b"x");
        round_trip(&vec![7; 100_000]);
        round_trip(b"The Zone gives and the Zone takes. The Zone gives and the Zone takes.");
    }

    #[test]
    fn manifest_raw_vectors_round_trip() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("kraken");
        let manifest = fs::read_to_string(root.join("manifest.json")).unwrap_or_else(|error| panic!("{error:?}"));
        let mut seen = std::collections::BTreeSet::new();
        for line in manifest.lines() {
            let Some(raw) = line.trim().strip_prefix("\"raw\": \"") else {
                continue;
            };
            let Some(name) = raw.strip_suffix("\",") else {
                continue;
            };
            if seen.insert(name.to_owned()) {
                round_trip(&fs::read(root.join(name)).unwrap_or_else(|error| panic!("{error:?}")));
            }
        }
    }
}
