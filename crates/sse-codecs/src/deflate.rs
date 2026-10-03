//! Dependency-free raw DEFLATE encoder with lazy LZ77 matching.
use std::{cmp::Reverse, collections::BinaryHeap};

use sse_core::{Error, Result};
/// Compression effort used by compress_raw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    /// Short hash chains and fixed Huffman codes.
    Fast,
    /// Deeper lazy matching and dynamic Huffman codes.
    Default,
}
struct Bits {
    out: Vec<u8>,
    bits: u64,
    count: u8,
}
impl Bits {
    fn new() -> Self {
        Self {
            out: Vec::new(),
            bits: 0,
            count: 0,
        }
    }
    fn put(&mut self, v: u32, n: u8) -> Result<()> {
        if n > 24 {
            return Err(Error::damaged("deflate bit width"));
        }
        self.bits |= u64::from(v).wrapping_shl(u32::from(self.count));
        self.count = self.count.saturating_add(n);
        while self.count >= 8 {
            self.out
                .push(u8::try_from(self.bits & 255).map_err(|_| Error::damaged("deflate byte"))?);
            self.bits >>= 8;
            self.count = self.count.saturating_sub(8);
        }
        Ok(())
    }
    fn finish(mut self) -> Vec<u8> {
        if self.count > 0 {
            self.out.push(u8::try_from(self.bits & 255).unwrap_or_default());
        }
        self.out
    }
}
fn rev(mut v: u16, n: u8) -> u16 {
    let mut r = 0u16;
    for _ in 0..n {
        r = r.wrapping_shl(1) | (v & 1);
        v = v.wrapping_shr(1);
    }
    r
}
fn fixed(sym: u16) -> (u16, u8) {
    match sym {
        0..=143 => (rev(sym.saturating_add(0x30), 8), 8),
        144..=255 => (rev(sym.saturating_sub(144).saturating_add(0x190), 9), 9),
        256..=279 => (rev(sym.saturating_sub(256), 7), 7),
        _ => (rev(sym.saturating_sub(280).saturating_add(0xc0), 8), 8),
    }
}
const LB: [usize; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LE: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DB: [usize; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DE: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];
fn emit_lit(w: &mut Bits, s: u16) -> Result<()> {
    let (c, n) = fixed(s);
    w.put(u32::from(c), n)
}
fn emit_match(w: &mut Bits, len: usize, dist: usize) -> Result<()> {
    let li = LB
        .iter()
        .enumerate()
        .rev()
        .find(|(_, b)| **b <= len)
        .map(|(i, _)| i)
        .ok_or_else(|| Error::damaged("deflate length"))?;
    emit_lit(
        w,
        u16::try_from(257_usize.saturating_add(li)).map_err(|_| Error::damaged("length symbol"))?,
    )?;
    let eb = *LE.get(li).ok_or_else(|| Error::damaged("length extra"))?;
    w.put(
        u32::try_from(len.saturating_sub(*LB.get(li).unwrap_or(&len))).map_err(|_| Error::damaged("length extra"))?,
        eb,
    )?;
    let di = DB
        .iter()
        .enumerate()
        .rev()
        .find(|(_, b)| **b <= dist)
        .map(|(i, _)| i)
        .ok_or_else(|| Error::damaged("distance"))?;
    w.put(
        u32::from(rev(
            u16::try_from(di).map_err(|_| Error::damaged("distance symbol"))?,
            5,
        )),
        5,
    )?;
    let de = *DE.get(di).ok_or_else(|| Error::damaged("distance extra"))?;
    w.put(
        u32::try_from(dist.saturating_sub(*DB.get(di).unwrap_or(&dist)))
            .map_err(|_| Error::damaged("distance extra"))?,
        de,
    )
}
fn hash3(d: &[u8], p: usize) -> Option<usize> {
    let a = usize::from(*d.get(p)?);
    let b = usize::from(*d.get(p.checked_add(1)?)?);
    let c = usize::from(*d.get(p.checked_add(2)?)?);
    Some(a.wrapping_mul(251).wrapping_add(b).wrapping_mul(251).wrapping_add(c) & 0x7fff)
}
fn best(d: &[u8], p: usize, head: &[Option<usize>], prev: &[Option<usize>], limit: usize) -> (usize, usize) {
    let Some(h) = hash3(d, p) else { return (0, 0) };
    let mut q = head.get(h).and_then(|x| *x);
    let mut best = (0, 0);
    let mut steps = 0usize;
    while let Some(pos) = q {
        if p <= pos || p.saturating_sub(pos) > 32768 {
            break;
        }
        let max = 258.min(d.len().saturating_sub(p));
        let mut n = 0usize;
        while n < max && d.get(pos.saturating_add(n)) == d.get(p.saturating_add(n)) {
            n = n.saturating_add(1);
        }
        if n >= 3 && n > best.0 {
            best = (n, p.saturating_sub(pos));
            if n == 258 {
                break;
            }
        }
        steps = steps.saturating_add(1);
        if steps >= limit {
            break;
        }
        q = prev.get(pos).and_then(|x| *x);
    }
    best
}

#[derive(Clone, Copy)]
enum Token {
    Lit(u8),
    Match { len: usize, dist: usize },
}

#[derive(Clone, Copy, Default)]
struct Code {
    bits: u16,
    len: u8,
}

fn length_symbol(len: usize) -> Result<(usize, u8, u32)> {
    let index = LB
        .iter()
        .enumerate()
        .rev()
        .find(|(_, base)| **base <= len)
        .map(|(index, _)| index)
        .ok_or_else(|| Error::damaged("deflate length"))?;
    let base = LB.get(index).copied().unwrap_or(len);
    Ok((
        257_usize.saturating_add(index),
        LE.get(index).copied().unwrap_or(0),
        u32::try_from(len.saturating_sub(base)).map_err(|_| Error::damaged("length extra"))?,
    ))
}

fn distance_symbol(dist: usize) -> Result<(usize, u8, u32)> {
    let index = DB
        .iter()
        .enumerate()
        .rev()
        .find(|(_, base)| **base <= dist)
        .map(|(index, _)| index)
        .ok_or_else(|| Error::damaged("deflate distance"))?;
    let base = DB.get(index).copied().unwrap_or(dist);
    Ok((
        index,
        DE.get(index).copied().unwrap_or(0),
        u32::try_from(dist.saturating_sub(base)).map_err(|_| Error::damaged("distance extra"))?,
    ))
}

fn tokenize(input: &[u8], level: Level) -> Vec<Token> {
    let mut head = vec![None; 32_768];
    let mut prev = vec![None; input.len()];
    let chain = if level == Level::Fast { 16 } else { 256 };
    let lazy = if level == Level::Fast { 8 } else { 96 };
    let mut out = Vec::with_capacity(input.len().saturating_div(2).saturating_add(1));
    let mut p = 0_usize;
    while p < input.len() {
        let current = best(input, p, &head, &prev, chain);
        if let Some(hash) = hash3(input, p) {
            if let Some(slot) = prev.get_mut(p) {
                *slot = head.get(hash).copied().flatten();
            }
            if let Some(slot) = head.get_mut(hash) {
                *slot = Some(p);
            }
        }
        let use_match = if current.0 >= 3 && p.saturating_add(1) < input.len() {
            let next = best(input, p.saturating_add(1), &head, &prev, lazy);
            next.0 <= current.0.saturating_add(1)
        } else {
            current.0 >= 3
        };
        if use_match {
            out.push(Token::Match {
                len: current.0,
                dist: current.1,
            });
            let end = p.saturating_add(current.0).min(input.len());
            let insertion_start = match level {
                Level::Fast => end.saturating_sub(1).max(p.saturating_add(1)),
                Level::Default => p.saturating_add(1),
            };
            for covered in insertion_start..end {
                if let Some(hash) = hash3(input, covered) {
                    if let Some(slot) = prev.get_mut(covered) {
                        *slot = head.get(hash).copied().flatten();
                    }
                    if let Some(slot) = head.get_mut(hash) {
                        *slot = Some(covered);
                    }
                }
            }
            p = end;
        } else {
            if let Some(byte) = input.get(p).copied() {
                out.push(Token::Lit(byte));
            }
            p = p.saturating_add(1);
        }
    }
    out
}

fn huffman_lengths(freq: &[u32], maximum: u8) -> Result<Vec<u8>> {
    let used: Vec<usize> = freq
        .iter()
        .enumerate()
        .filter_map(|(symbol, count)| (*count != 0).then_some(symbol))
        .collect();
    let mut result = vec![0_u8; freq.len()];
    if used.is_empty() {
        return Ok(result);
    }
    if used.len() == 1 {
        if let Some(slot) = result.get_mut(used.first().copied().unwrap_or(0)) {
            *slot = 1;
        }
        return Ok(result);
    }
    let mut parent: Vec<Option<usize>> = vec![None; used.len()];
    let mut heap: BinaryHeap<Reverse<(u32, usize, usize)>> = BinaryHeap::new();
    for (leaf, symbol) in used.iter().copied().enumerate() {
        heap.push(Reverse((freq.get(symbol).copied().unwrap_or(0), symbol, leaf)));
    }
    while heap.len() > 1 {
        let Some(Reverse((fa, sa, a))) = heap.pop() else { break };
        let Some(Reverse((fb, sb, b))) = heap.pop() else { break };
        let node = parent.len();
        parent.push(None);
        if let Some(slot) = parent.get_mut(a) {
            *slot = Some(node);
        }
        if let Some(slot) = parent.get_mut(b) {
            *slot = Some(node);
        }
        heap.push(Reverse((fa.saturating_add(fb), sa.min(sb), node)));
    }
    let mut raw = Vec::with_capacity(used.len());
    for leaf in 0..used.len() {
        let mut depth = 0_u8;
        let mut node = leaf;
        while let Some(next) = parent.get(node).copied().flatten() {
            depth = depth.saturating_add(1);
            node = next;
        }
        raw.push(depth.max(1));
    }
    if raw.iter().copied().max().unwrap_or(1) <= maximum {
        for (symbol, depth) in used.iter().copied().zip(raw) {
            if let Some(slot) = result.get_mut(symbol) {
                *slot = depth;
            }
        }
        return Ok(result);
    }
    let mut counts = vec![0_usize; usize::from(maximum).saturating_add(1)];
    let mut overflow = 0_usize;
    for depth in raw {
        let clipped = depth.min(maximum);
        if let Some(slot) = counts.get_mut(usize::from(clipped)) {
            *slot = slot.saturating_add(1);
        }
        if depth > maximum {
            overflow = overflow.saturating_add(1);
        }
    }
    while overflow > 0 {
        let mut bits = maximum.saturating_sub(1);
        while bits > 0 && counts.get(usize::from(bits)).copied().unwrap_or(0) == 0 {
            bits = bits.saturating_sub(1);
        }
        if bits == 0 {
            return Err(Error::damaged("cannot limit deflate Huffman tree"));
        }
        if let Some(slot) = counts.get_mut(usize::from(bits)) {
            *slot = slot.saturating_sub(1);
        }
        if let Some(slot) = counts.get_mut(usize::from(bits.saturating_add(1))) {
            *slot = slot.saturating_add(2);
        }
        if let Some(slot) = counts.get_mut(usize::from(maximum)) {
            *slot = slot.saturating_sub(1);
        }
        overflow = overflow.saturating_sub(overflow.min(2));
    }
    let mut ordered = used;
    ordered.sort_by_key(|symbol| (freq.get(*symbol).copied().unwrap_or(0), *symbol));
    let mut cursor = 0_usize;
    for bits in (1..=maximum).rev() {
        let amount = counts.get(usize::from(bits)).copied().unwrap_or(0);
        for _ in 0..amount {
            let symbol = ordered
                .get(cursor)
                .copied()
                .ok_or_else(|| Error::damaged("deflate Huffman assignment"))?;
            if let Some(slot) = result.get_mut(symbol) {
                *slot = bits;
            }
            cursor = cursor.saturating_add(1);
        }
    }
    if cursor != ordered.len() {
        return Err(Error::damaged("incomplete deflate Huffman assignment"));
    }
    Ok(result)
}

fn canonical(lengths: &[u8], maximum: u8) -> Result<Vec<Code>> {
    let mut counts = vec![0_u16; usize::from(maximum).saturating_add(1)];
    for length in lengths.iter().copied().filter(|length| *length != 0) {
        if length > maximum {
            return Err(Error::damaged("deflate Huffman length exceeds limit"));
        }
        if let Some(slot) = counts.get_mut(usize::from(length)) {
            *slot = slot.saturating_add(1);
        }
    }
    let mut next = vec![0_u16; counts.len()];
    let mut code = 0_u16;
    for bits in 1..=maximum {
        code = code
            .saturating_add(counts.get(usize::from(bits.saturating_sub(1))).copied().unwrap_or(0))
            .wrapping_shl(1);
        if let Some(slot) = next.get_mut(usize::from(bits)) {
            *slot = code;
        }
    }
    let mut result = vec![Code::default(); lengths.len()];
    for (symbol, length) in lengths.iter().copied().enumerate() {
        if length == 0 {
            continue;
        }
        let raw = next.get(usize::from(length)).copied().unwrap_or(0);
        if let Some(slot) = next.get_mut(usize::from(length)) {
            *slot = slot.saturating_add(1);
        }
        if let Some(slot) = result.get_mut(symbol) {
            *slot = Code {
                bits: rev(raw, length),
                len: length,
            };
        }
    }
    Ok(result)
}

fn emit_code(writer: &mut Bits, codes: &[Code], symbol: usize) -> Result<()> {
    let code = codes
        .get(symbol)
        .copied()
        .ok_or_else(|| Error::damaged("missing deflate Huffman symbol"))?;
    if code.len == 0 {
        return Err(Error::damaged("zero-length deflate Huffman symbol"));
    }
    writer.put(u32::from(code.bits), code.len)
}

fn emit_tokens(writer: &mut Bits, tokens: &[Token], literal: &[Code], distance: &[Code]) -> Result<()> {
    for token in tokens {
        match *token {
            Token::Lit(byte) => emit_code(writer, literal, usize::from(byte))?,
            Token::Match { len, dist } => {
                let (ls, le, lv) = length_symbol(len)?;
                emit_code(writer, literal, ls)?;
                writer.put(lv, le)?;
                let (ds, de, dv) = distance_symbol(dist)?;
                emit_code(writer, distance, ds)?;
                writer.put(dv, de)?;
            }
        }
    }
    emit_code(writer, literal, 256)
}

#[derive(Clone, Copy)]
struct LengthToken {
    symbol: usize,
    extra: u32,
    bits: u8,
}

fn encode_lengths(lengths: &[u8]) -> Vec<LengthToken> {
    let mut out = Vec::new();
    let mut p = 0_usize;
    while p < lengths.len() {
        let value = lengths.get(p).copied().unwrap_or(0);
        let mut run = 1_usize;
        while p.saturating_add(run) < lengths.len() && lengths.get(p.saturating_add(run)).copied() == Some(value) {
            run = run.saturating_add(1);
        }
        let mut left = run;
        if value == 0 {
            while left >= 11 {
                let n = left.min(138);
                out.push(LengthToken {
                    symbol: 18,
                    extra: u32::try_from(n.saturating_sub(11)).unwrap_or_default(),
                    bits: 7,
                });
                left = left.saturating_sub(n);
            }
            if left >= 3 {
                let n = left.min(10);
                out.push(LengthToken {
                    symbol: 17,
                    extra: u32::try_from(n.saturating_sub(3)).unwrap_or_default(),
                    bits: 3,
                });
                left = left.saturating_sub(n);
            }
            for _ in 0..left {
                out.push(LengthToken {
                    symbol: 0,
                    extra: 0,
                    bits: 0,
                });
            }
        } else {
            out.push(LengthToken {
                symbol: usize::from(value),
                extra: 0,
                bits: 0,
            });
            left = left.saturating_sub(1);
            while left >= 3 {
                let n = left.min(6);
                out.push(LengthToken {
                    symbol: 16,
                    extra: u32::try_from(n.saturating_sub(3)).unwrap_or_default(),
                    bits: 2,
                });
                left = left.saturating_sub(n);
            }
            for _ in 0..left {
                out.push(LengthToken {
                    symbol: usize::from(value),
                    extra: 0,
                    bits: 0,
                });
            }
        }
        p = p.saturating_add(run);
    }
    out
}

fn emit_dynamic(writer: &mut Bits, tokens: &[Token]) -> Result<()> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut lf = vec![0_u32; 286];
    let mut df = vec![0_u32; 30];
    if let Some(slot) = lf.get_mut(256) {
        *slot = 1;
    }
    for token in tokens {
        match *token {
            Token::Lit(byte) => {
                if let Some(slot) = lf.get_mut(usize::from(byte)) {
                    *slot = slot.saturating_add(1);
                }
            }
            Token::Match { len, dist } => {
                let (ls, _, _) = length_symbol(len)?;
                let (ds, _, _) = distance_symbol(dist)?;
                if let Some(slot) = lf.get_mut(ls) {
                    *slot = slot.saturating_add(1);
                }
                if let Some(slot) = df.get_mut(ds) {
                    *slot = slot.saturating_add(1);
                }
            }
        }
    }
    if df.iter().all(|value| *value == 0) {
        if let Some(slot) = df.first_mut() {
            *slot = 1;
        }
    }
    let ll = huffman_lengths(&lf, 15)?;
    let dl = huffman_lengths(&df, 15)?;
    let hlit = ll
        .iter()
        .rposition(|value| *value != 0)
        .map_or(257, |index| index.saturating_add(1).max(257))
        .min(286);
    let hdist = dl
        .iter()
        .rposition(|value| *value != 0)
        .map_or(1, |index| index.saturating_add(1).max(1))
        .min(30);
    let mut lengths = Vec::with_capacity(hlit.saturating_add(hdist));
    lengths.extend_from_slice(ll.get(..hlit).ok_or_else(|| Error::damaged("literal lengths"))?);
    lengths.extend_from_slice(dl.get(..hdist).ok_or_else(|| Error::damaged("distance lengths"))?);
    let encoded = encode_lengths(&lengths);
    let mut cf = vec![0_u32; 19];
    for item in &encoded {
        if let Some(slot) = cf.get_mut(item.symbol) {
            *slot = slot.saturating_add(1);
        }
    }
    let cl = huffman_lengths(&cf, 7)?;
    let hclen = ORDER
        .iter()
        .rposition(|symbol| cl.get(*symbol).copied().unwrap_or(0) != 0)
        .map_or(4, |index| index.saturating_add(1).max(4));
    let lc = canonical(&ll, 15)?;
    let dc = canonical(&dl, 15)?;
    let cc = canonical(&cl, 7)?;
    writer.put(1, 1)?;
    writer.put(2, 2)?;
    writer.put(
        u32::try_from(hlit.saturating_sub(257)).map_err(|_| Error::damaged("HLIT"))?,
        5,
    )?;
    writer.put(
        u32::try_from(hdist.saturating_sub(1)).map_err(|_| Error::damaged("HDIST"))?,
        5,
    )?;
    writer.put(
        u32::try_from(hclen.saturating_sub(4)).map_err(|_| Error::damaged("HCLEN"))?,
        4,
    )?;
    for symbol in ORDER.iter().copied().take(hclen) {
        writer.put(u32::from(cl.get(symbol).copied().unwrap_or(0)), 3)?;
    }
    for item in encoded {
        emit_code(writer, &cc, item.symbol)?;
        writer.put(item.extra, item.bits)?;
    }
    emit_tokens(writer, tokens, &lc, &dc)
}

fn emit_fixed(writer: &mut Bits, tokens: &[Token]) -> Result<()> {
    writer.put(1, 1)?;
    writer.put(1, 2)?;
    for token in tokens {
        match *token {
            Token::Lit(byte) => emit_lit(writer, u16::from(byte))?,
            Token::Match { len, dist } => emit_match(writer, len, dist)?,
        }
    }
    emit_lit(writer, 256)
}

/// Compresses to a raw RFC 1951 stream with bounded lazy hash-chain matching.
///
/// Fast emits fixed Huffman codes. Default emits length-limited dynamic
/// Huffman codes, including the RFC 1951 code-length RLE alphabet.
pub fn compress_raw(input: &[u8], level: Level) -> Result<Vec<u8>> {
    let tokens = tokenize(input, level);
    let mut writer = Bits::new();
    match level {
        Level::Fast => emit_fixed(&mut writer, &tokens)?,
        Level::Default => emit_dynamic(&mut writer, &tokens)?,
    }
    Ok(writer.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inflate::inflate_raw;

    #[test]
    fn empty_is_valid_shape() {
        let v = compress_raw(b"", Level::Default).unwrap_or_default();
        assert!(!v.is_empty());
        assert_eq!(inflate_raw(&v, 0).unwrap_or_default(), b"");
    }
    #[test]
    fn repetitive_compresses() {
        let d = vec![b'a'; 10000];
        let v = compress_raw(&d, Level::Default).unwrap_or_default();
        assert!(v.len() < 200);
        assert_eq!(inflate_raw(&v, d.len()).unwrap_or_default(), d);
    }

    #[test]
    fn ten_mib_repetitive_meets_ten_mib_per_second_floor() {
        let data = vec![b'a'; 10_485_760];
        let started = std::time::Instant::now();
        let encoded = compress_raw(&data, Level::Default).unwrap_or_default();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "10 MiB compression missed the 10 MiB/s task floor"
        );
        assert!(encoded.len() < 20_000);
        assert_eq!(inflate_raw(&encoded, data.len()).unwrap_or_default(), data);
    }

    #[test]
    fn default_is_within_five_percent_of_zlib_6_on_text_fixture() {
        const PARAGRAPH: &[u8] =
            b"Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n";
        let mut data = Vec::with_capacity(PARAGRAPH.len().saturating_mul(10_000));
        for _ in 0..10_000 {
            data.extend_from_slice(PARAGRAPH);
        }
        // Python zlib 1.2.x, level 6, raw RFC 1951 (wbits=-15) produces
        // 4,322 bytes for this exact 1,240,000-byte fixture.
        const ZLIB_6_RAW: usize = 4_322;
        let encoded = compress_raw(&data, Level::Default).unwrap_or_default();
        assert!(
            encoded.len().saturating_mul(100) <= ZLIB_6_RAW.saturating_mul(105),
            "ours={} zlib6={ZLIB_6_RAW}",
            encoded.len()
        );
        assert_eq!(inflate_raw(&encoded, data.len()).unwrap_or_default(), data);
    }
    #[test]
    fn default_writes_dynamic_huffman() {
        let d = b"dynamic huffman repeated repeated repeated repeated text";
        let v = compress_raw(d, Level::Default).unwrap_or_default();
        assert_eq!(v.first().copied().unwrap_or(0) & 7, 5);
        assert_eq!(inflate_raw(&v, d.len()).unwrap_or_default(), d);
    }
}
