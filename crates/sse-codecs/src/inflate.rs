//! Zlib/DEFLATE decoder used by PNG and save tooling.
//!
//! Stored, fixed-Huffman and dynamic-Huffman blocks are supported. The caller-provided output
//! limit is checked before every growth operation, and the zlib Adler-32 trailer is verified.

use sse_core::{Error, Result};

const MAX_CODES: usize = 288;
const MAX_BITS: usize = 15;

#[derive(Clone, Copy, Debug)]
struct HuffmanEntry {
    reversed_code: u16,
    length: u8,
    symbol: u16,
}

#[derive(Clone, Debug)]
struct Huffman {
    /// Sorted by (length, reversed code), so each length is one contiguous range.
    entries: Vec<HuffmanEntry>,
    /// Index range of the entries of each code length (`ranges[length]`).
    ranges: [(usize, usize); MAX_BITS + 1],
    maximum_length: u8,
}

impl Huffman {
    fn from_lengths(lengths: &[u8]) -> Result<Self> {
        if lengths.is_empty() || lengths.len() > MAX_CODES {
            return Err(Error::damaged("invalid DEFLATE Huffman alphabet size"));
        }
        let mut counts = [0_u16; MAX_BITS + 1];
        let mut used = 0_usize;
        for length in lengths.iter().copied() {
            if usize::from(length) > MAX_BITS {
                return Err(Error::damaged("DEFLATE Huffman code length exceeds 15"));
            }
            if length != 0 {
                let slot = counts
                    .get_mut(usize::from(length))
                    .ok_or_else(|| Error::damaged("Huffman count slot"))?;
                *slot = slot
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("Huffman code-count overflow"))?;
                used = used
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("Huffman used-count overflow"))?;
            }
        }
        if used == 0 {
            return Err(Error::damaged("empty DEFLATE Huffman tree"));
        }

        let mut left = 1_i32;
        let mut bits = 1_usize;
        while bits <= MAX_BITS {
            left = left
                .checked_mul(2)
                .and_then(|value| value.checked_sub(i32::from(*counts.get(bits).unwrap_or(&0))))
                .ok_or_else(|| Error::damaged("Huffman completeness overflow"))?;
            if left < 0 {
                return Err(Error::damaged("oversubscribed DEFLATE Huffman tree"));
            }
            bits = bits
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Huffman bit-length overflow"))?;
        }

        // RFC 1951 permits a one-symbol incomplete tree. Other incomplete trees are malformed.
        if left > 0 && used != 1 {
            return Err(Error::damaged("incomplete DEFLATE Huffman tree"));
        }

        let mut next_code = [0_u16; MAX_BITS + 1];
        let mut code = 0_u16;
        let mut current = 1_usize;
        while current <= MAX_BITS {
            let previous = current
                .checked_sub(1)
                .ok_or_else(|| Error::damaged("Huffman length underflow"))?;
            code = code
                .checked_add(*counts.get(previous).unwrap_or(&0))
                .and_then(|value| value.checked_shl(1))
                .ok_or_else(|| Error::damaged("Huffman canonical-code overflow"))?;
            if let Some(slot) = next_code.get_mut(current) {
                *slot = code;
            }
            current = current
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Huffman length overflow"))?;
        }

        let mut entries = Vec::with_capacity(used);
        let mut maximum_length = 0_u8;
        for (symbol, length) in lengths.iter().copied().enumerate() {
            if length == 0 {
                continue;
            }
            let slot = next_code
                .get_mut(usize::from(length))
                .ok_or_else(|| Error::damaged("Huffman canonical slot"))?;
            let canonical = *slot;
            *slot = slot
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Huffman code overflow"))?;
            entries.push(HuffmanEntry {
                reversed_code: reverse_low_bits(canonical, length),
                length,
                symbol: u16::try_from(symbol).map_err(|_| Error::damaged("Huffman symbol does not fit u16"))?,
            });
            maximum_length = maximum_length.max(length);
        }
        entries.sort_unstable_by_key(|entry| (entry.length, entry.reversed_code));
        let mut ranges = [(0_usize, 0_usize); MAX_BITS + 1];
        let mut start = 0_usize;
        for (length, range) in ranges.iter_mut().enumerate() {
            let count = entries
                .iter()
                .filter(|entry| usize::from(entry.length) == length)
                .count();
            let end = start.saturating_add(count);
            *range = (start, end);
            start = end;
        }
        Ok(Self {
            entries,
            ranges,
            maximum_length,
        })
    }

    fn decode(&self, bits: &mut BitReader<'_>) -> Result<u16> {
        let mut code = 0_u16;
        let mut length = 1_u8;
        while length <= self.maximum_length {
            let bit = u16::from(bits.read_bit()?);
            let shift = u32::from(
                length
                    .checked_sub(1)
                    .ok_or_else(|| Error::damaged("Huffman shift underflow"))?,
            );
            code |= bit
                .checked_shl(shift)
                .ok_or_else(|| Error::damaged("Huffman code shift overflow"))?;
            let (start, end) = self.ranges.get(usize::from(length)).copied().unwrap_or((0, 0));
            let candidates = self.entries.get(start..end).unwrap_or(&[]);
            if let Ok(index) = candidates.binary_search_by_key(&code, |entry| entry.reversed_code) {
                if let Some(entry) = candidates.get(index) {
                    return Ok(entry.symbol);
                }
            }
            length = length
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Huffman decode length overflow"))?;
        }
        Err(Error::damaged("invalid DEFLATE Huffman code"))
    }
}

fn reverse_low_bits(mut value: u16, length: u8) -> u16 {
    let mut out = 0_u16;
    let mut count = 0_u8;
    while count < length {
        out = out.checked_shl(1).unwrap_or_default() | (value & 1);
        value = value.checked_shr(1).unwrap_or_default();
        count = count.checked_add(1).unwrap_or(length);
    }
    out
}

struct BitReader<'a> {
    data: &'a [u8],
    byte_position: usize,
    bit_position: u8,
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_position: 0,
            bit_position: 0,
        }
    }

    fn read_bit(&mut self) -> Result<u8> {
        let byte = self
            .data
            .get(self.byte_position)
            .copied()
            .ok_or_else(|| Error::damaged("truncated DEFLATE bitstream"))?;
        let value = byte.checked_shr(u32::from(self.bit_position)).unwrap_or_default() & 1;
        self.bit_position = self
            .bit_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("DEFLATE bit-position overflow"))?;
        if self.bit_position == 8 {
            self.bit_position = 0;
            self.byte_position = self
                .byte_position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("DEFLATE byte-position overflow"))?;
        }
        Ok(value)
    }

    fn read_bits(&mut self, count: u8) -> Result<u32> {
        if count > 24 {
            return Err(Error::damaged("DEFLATE bit read is too wide"));
        }
        let mut value = 0_u32;
        let mut index = 0_u8;
        while index < count {
            let bit = u32::from(self.read_bit()?);
            value |= bit
                .checked_shl(u32::from(index))
                .ok_or_else(|| Error::damaged("DEFLATE bit shift overflow"))?;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("DEFLATE bit-count overflow"))?;
        }
        Ok(value)
    }

    fn align_byte(&mut self) -> Result<()> {
        if self.bit_position != 0 {
            self.bit_position = 0;
            self.byte_position = self
                .byte_position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("DEFLATE alignment overflow"))?;
        }
        if self.byte_position > self.data.len() {
            return Err(Error::damaged("DEFLATE alignment past end"));
        }
        Ok(())
    }

    fn take_bytes(&mut self, length: usize) -> Result<&'a [u8]> {
        if self.bit_position != 0 {
            return Err(Error::damaged("unaligned DEFLATE byte read"));
        }
        let end = self
            .byte_position
            .checked_add(length)
            .ok_or_else(|| Error::damaged("DEFLATE byte range overflow"))?;
        let value = self
            .data
            .get(self.byte_position..end)
            .ok_or_else(|| Error::damaged("truncated DEFLATE stored block"))?;
        self.byte_position = end;
        Ok(value)
    }

    fn consumed_all_bytes(&self) -> bool {
        self.byte_position == self.data.len() && self.bit_position == 0
    }
}

const LENGTH_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258,
];
const LENGTH_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DISTANCE_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145,
    8193, 12289, 16385, 24577,
];
const DISTANCE_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13,
];

/// Inflates a zlib stream with an enforced decompressed-size ceiling.
///
/// # Errors
/// Returns [`Error::Damaged`] for a malformed zlib/DEFLATE stream, invalid Huffman tree,
/// distance before the beginning of output, checksum mismatch, or when `maximum` would be
/// exceeded.
pub fn inflate_zlib(input: &[u8], maximum: usize) -> Result<Vec<u8>> {
    if input.len() < 6 {
        return Err(Error::damaged("zlib stream is too short"));
    }
    let cmf = *input.first().ok_or_else(|| Error::damaged("missing zlib CMF"))?;
    let flg = *input.get(1).ok_or_else(|| Error::damaged("missing zlib FLG"))?;
    if cmf & 0x0F != 8 || cmf.checked_shr(4).unwrap_or_default() > 7 {
        return Err(Error::damaged("unsupported zlib compression method/window"));
    }
    let header = u16::from(cmf)
        .checked_shl(8)
        .and_then(|value| value.checked_add(u16::from(flg)))
        .ok_or_else(|| Error::damaged("zlib header overflow"))?;
    if header.checked_rem(31) != Some(0) {
        return Err(Error::damaged("invalid zlib FCHECK"));
    }
    if flg & 0x20 != 0 {
        return Err(Error::damaged("zlib preset dictionary is unsupported"));
    }

    let trailer_start = input
        .len()
        .checked_sub(4)
        .ok_or_else(|| Error::damaged("zlib trailer underflow"))?;
    let deflate = input
        .get(2..trailer_start)
        .ok_or_else(|| Error::damaged("invalid zlib DEFLATE range"))?;
    let trailer = input
        .get(trailer_start..)
        .ok_or_else(|| Error::damaged("missing zlib Adler-32"))?;
    let expected_adler =
        u32::from_be_bytes(<[u8; 4]>::try_from(trailer).map_err(|_| Error::damaged("short zlib Adler-32"))?);

    let mut reader = BitReader::new(deflate);
    let mut output = Vec::new();
    let mut final_block = false;
    while !final_block {
        final_block = reader.read_bit()? != 0;
        let block_type = reader.read_bits(2)?;
        match block_type {
            0 => decode_stored(&mut reader, &mut output, maximum)?,
            1 => {
                let (literal, distance) = fixed_trees()?;
                decode_compressed(&mut reader, &literal, &distance, &mut output, maximum)?;
            }
            2 => {
                let (literal, distance) = dynamic_trees(&mut reader)?;
                decode_compressed(&mut reader, &literal, &distance, &mut output, maximum)?;
            }
            _ => return Err(Error::damaged("reserved DEFLATE block type")),
        }
    }
    reader.align_byte()?;
    if !reader.consumed_all_bytes() {
        return Err(Error::damaged("trailing bytes inside zlib DEFLATE payload"));
    }

    if adler32(&output) != expected_adler {
        return Err(Error::damaged("zlib Adler-32 mismatch"));
    }
    Ok(output)
}

/// Inflates a raw RFC 1951 DEFLATE stream with an enforced output ceiling.
pub fn inflate_raw(input: &[u8], maximum: usize) -> Result<Vec<u8>> {
    let mut reader = BitReader::new(input);
    let mut output = Vec::new();
    let mut final_block = false;
    while !final_block {
        final_block = reader.read_bit()? != 0;
        match reader.read_bits(2)? {
            0 => decode_stored(&mut reader, &mut output, maximum)?,
            1 => {
                let (literal, distance) = fixed_trees()?;
                decode_compressed(&mut reader, &literal, &distance, &mut output, maximum)?;
            }
            2 => {
                let (literal, distance) = dynamic_trees(&mut reader)?;
                decode_compressed(&mut reader, &literal, &distance, &mut output, maximum)?;
            }
            _ => return Err(Error::damaged("reserved DEFLATE block type")),
        }
    }
    Ok(output)
}

fn decode_stored(reader: &mut BitReader<'_>, output: &mut Vec<u8>, maximum: usize) -> Result<()> {
    reader.align_byte()?;
    let header = reader.take_bytes(4)?;
    let len = u16::from_le_bytes(
        <[u8; 2]>::try_from(header.get(0..2).ok_or_else(|| Error::damaged("short stored LEN"))?)
            .map_err(|_| Error::damaged("short stored LEN"))?,
    );
    let nlen = u16::from_le_bytes(
        <[u8; 2]>::try_from(header.get(2..4).ok_or_else(|| Error::damaged("short stored NLEN"))?)
            .map_err(|_| Error::damaged("short stored NLEN"))?,
    );
    if len != !nlen {
        return Err(Error::damaged("DEFLATE stored LEN/NLEN mismatch"));
    }
    let length = usize::from(len);
    ensure_growth(output.len(), length, maximum)?;
    let bytes = reader.take_bytes(length)?;
    output.extend_from_slice(bytes);
    Ok(())
}

fn fixed_trees() -> Result<(Huffman, Huffman)> {
    let mut literal_lengths = vec![0_u8; 288];
    let mut symbol = 0_usize;
    while symbol <= 143 {
        set_length(&mut literal_lengths, symbol, 8)?;
        symbol = symbol
            .checked_add(1)
            .ok_or_else(|| Error::damaged("fixed symbol overflow"))?;
    }
    while symbol <= 255 {
        set_length(&mut literal_lengths, symbol, 9)?;
        symbol = symbol
            .checked_add(1)
            .ok_or_else(|| Error::damaged("fixed symbol overflow"))?;
    }
    while symbol <= 279 {
        set_length(&mut literal_lengths, symbol, 7)?;
        symbol = symbol
            .checked_add(1)
            .ok_or_else(|| Error::damaged("fixed symbol overflow"))?;
    }
    while symbol <= 287 {
        set_length(&mut literal_lengths, symbol, 8)?;
        symbol = symbol
            .checked_add(1)
            .ok_or_else(|| Error::damaged("fixed symbol overflow"))?;
    }
    let distance_lengths = vec![5_u8; 32];
    Ok((
        Huffman::from_lengths(&literal_lengths)?,
        Huffman::from_lengths(&distance_lengths)?,
    ))
}

fn set_length(lengths: &mut [u8], index: usize, value: u8) -> Result<()> {
    let slot = lengths
        .get_mut(index)
        .ok_or_else(|| Error::damaged("Huffman symbol outside alphabet"))?;
    *slot = value;
    Ok(())
}

fn dynamic_trees(reader: &mut BitReader<'_>) -> Result<(Huffman, Huffman)> {
    let hlit = usize::try_from(reader.read_bits(5)?)
        .ok()
        .and_then(|value| value.checked_add(257))
        .ok_or_else(|| Error::damaged("HLIT overflow"))?;
    let hdist = usize::try_from(reader.read_bits(5)?)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| Error::damaged("HDIST overflow"))?;
    let hclen = usize::try_from(reader.read_bits(4)?)
        .ok()
        .and_then(|value| value.checked_add(4))
        .ok_or_else(|| Error::damaged("HCLEN overflow"))?;
    if hlit > 286 || hdist > 32 {
        return Err(Error::damaged("dynamic DEFLATE alphabet is too large"));
    }

    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut code_lengths = [0_u8; 19];
    let mut index = 0_usize;
    while index < hclen {
        let target = *ORDER
            .get(index)
            .ok_or_else(|| Error::damaged("code-length order index"))?;
        let value = u8::try_from(reader.read_bits(3)?).map_err(|_| Error::damaged("code-length value overflow"))?;
        let slot = code_lengths
            .get_mut(target)
            .ok_or_else(|| Error::damaged("code-length slot"))?;
        *slot = value;
        index = index
            .checked_add(1)
            .ok_or_else(|| Error::damaged("HCLEN index overflow"))?;
    }
    let code_tree = Huffman::from_lengths(&code_lengths)?;
    let total = hlit
        .checked_add(hdist)
        .ok_or_else(|| Error::damaged("dynamic code-count overflow"))?;
    let mut lengths = Vec::with_capacity(total);
    while lengths.len() < total {
        let symbol = code_tree.decode(reader)?;
        match symbol {
            0..=15 => {
                lengths.push(u8::try_from(symbol).map_err(|_| Error::damaged("code length does not fit u8"))?);
            }
            16 => {
                let previous = lengths
                    .last()
                    .copied()
                    .ok_or_else(|| Error::damaged("repeat code 16 has no previous length"))?;
                let repeat = usize::try_from(reader.read_bits(2)?)
                    .ok()
                    .and_then(|value| value.checked_add(3))
                    .ok_or_else(|| Error::damaged("repeat-16 count overflow"))?;
                repeat_length(&mut lengths, total, previous, repeat)?;
            }
            17 => {
                let repeat = usize::try_from(reader.read_bits(3)?)
                    .ok()
                    .and_then(|value| value.checked_add(3))
                    .ok_or_else(|| Error::damaged("repeat-17 count overflow"))?;
                repeat_length(&mut lengths, total, 0, repeat)?;
            }
            18 => {
                let repeat = usize::try_from(reader.read_bits(7)?)
                    .ok()
                    .and_then(|value| value.checked_add(11))
                    .ok_or_else(|| Error::damaged("repeat-18 count overflow"))?;
                repeat_length(&mut lengths, total, 0, repeat)?;
            }
            _ => return Err(Error::damaged("invalid code-length alphabet symbol")),
        }
    }

    let literal_lengths = lengths
        .get(0..hlit)
        .ok_or_else(|| Error::damaged("literal-code range"))?;
    if literal_lengths.get(256).copied().unwrap_or_default() == 0 {
        return Err(Error::damaged("dynamic tree has no end-of-block code"));
    }
    let distance_lengths = lengths
        .get(hlit..)
        .ok_or_else(|| Error::damaged("distance-code range"))?;
    let literal = Huffman::from_lengths(literal_lengths)?;
    let distance = if distance_lengths.iter().all(|value| *value == 0) {
        Huffman {
            entries: Vec::new(),
            ranges: [(0, 0); MAX_BITS + 1],
            maximum_length: 0,
        }
    } else {
        Huffman::from_lengths(distance_lengths)?
    };
    Ok((literal, distance))
}

fn repeat_length(lengths: &mut Vec<u8>, total: usize, value: u8, repeat: usize) -> Result<()> {
    let wanted = lengths
        .len()
        .checked_add(repeat)
        .ok_or_else(|| Error::damaged("code-length repeat overflow"))?;
    if wanted > total {
        return Err(Error::damaged("code-length repeat exceeds alphabet"));
    }
    let mut count = 0_usize;
    while count < repeat {
        lengths.push(value);
        count = count
            .checked_add(1)
            .ok_or_else(|| Error::damaged("code-length repeat counter overflow"))?;
    }
    Ok(())
}

fn decode_compressed(
    reader: &mut BitReader<'_>,
    literal: &Huffman,
    distance: &Huffman,
    output: &mut Vec<u8>,
    maximum: usize,
) -> Result<()> {
    loop {
        let symbol = literal.decode(reader)?;
        match symbol {
            0..=255 => {
                ensure_growth(output.len(), 1, maximum)?;
                output.push(u8::try_from(symbol).map_err(|_| Error::damaged("literal symbol does not fit byte"))?);
            }
            256 => return Ok(()),
            257..=285 => {
                let length_index = usize::from(
                    symbol
                        .checked_sub(257)
                        .ok_or_else(|| Error::damaged("length symbol underflow"))?,
                );
                let base = usize::from(
                    *LENGTH_BASE
                        .get(length_index)
                        .ok_or_else(|| Error::damaged("invalid DEFLATE length symbol"))?,
                );
                let extra_bits = *LENGTH_EXTRA
                    .get(length_index)
                    .ok_or_else(|| Error::damaged("missing length extra-bit count"))?;
                let extra = usize::try_from(reader.read_bits(extra_bits)?)
                    .map_err(|_| Error::damaged("length extra bits do not fit usize"))?;
                let length = base
                    .checked_add(extra)
                    .ok_or_else(|| Error::damaged("DEFLATE match length overflow"))?;

                let distance_symbol = usize::from(distance.decode(reader)?);
                if distance_symbol >= 30 {
                    return Err(Error::damaged("invalid DEFLATE distance symbol"));
                }
                let distance_base = usize::try_from(
                    *DISTANCE_BASE
                        .get(distance_symbol)
                        .ok_or_else(|| Error::damaged("missing distance base"))?,
                )
                .map_err(|_| Error::damaged("distance base does not fit usize"))?;
                let distance_extra_bits = *DISTANCE_EXTRA
                    .get(distance_symbol)
                    .ok_or_else(|| Error::damaged("missing distance extra-bit count"))?;
                let distance_extra = usize::try_from(reader.read_bits(distance_extra_bits)?)
                    .map_err(|_| Error::damaged("distance extra bits do not fit usize"))?;
                let distance_value = distance_base
                    .checked_add(distance_extra)
                    .ok_or_else(|| Error::damaged("DEFLATE distance overflow"))?;
                if distance_value == 0 || distance_value > output.len() {
                    return Err(Error::damaged("DEFLATE match points before output"));
                }
                ensure_growth(output.len(), length, maximum)?;
                let mut copied = 0_usize;
                while copied < length {
                    let source = output
                        .len()
                        .checked_sub(distance_value)
                        .ok_or_else(|| Error::damaged("DEFLATE match source underflow"))?;
                    let byte = *output
                        .get(source)
                        .ok_or_else(|| Error::damaged("DEFLATE match source outside output"))?;
                    output.push(byte);
                    copied = copied
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("DEFLATE match counter overflow"))?;
                }
            }
            _ => return Err(Error::damaged("reserved DEFLATE literal/length symbol")),
        }
    }
}

fn ensure_growth(current: usize, additional: usize, maximum: usize) -> Result<()> {
    let wanted = current
        .checked_add(additional)
        .ok_or_else(|| Error::damaged("inflated output size overflow"))?;
    if wanted > maximum {
        return Err(Error::damaged("inflated output exceeds configured limit"));
    }
    Ok(())
}

/// Adler-32 as used by zlib. The modulus is applied once per 5552-byte block, the largest block for
/// which the running sums cannot overflow `u32`, instead of once per byte.
#[must_use]
pub fn adler32(data: &[u8]) -> u32 {
    const MODULUS: u32 = 65_521;
    const BLOCK: usize = 5_552;
    let mut a = 1_u32;
    let mut b = 0_u32;
    for block in data.chunks(BLOCK) {
        for byte in block {
            a = a.wrapping_add(u32::from(*byte));
            b = b.wrapping_add(a);
        }
        a %= MODULUS;
        b %= MODULUS;
    }
    b.checked_shl(16).unwrap_or_default() | a
}

#[cfg(test)]
mod tests {
    use super::adler32;

    #[test]
    fn adler32_matches_the_standard_vectors() {
        assert_eq!(adler32(b""), 1);
        assert_eq!(adler32(b"Wikipedia"), 0x11E6_0398);
    }

    #[test]
    fn adler32_block_boundaries_match_a_per_byte_reference() {
        fn reference(data: &[u8]) -> u32 {
            let (mut a, mut b) = (1_u64, 0_u64);
            for byte in data {
                a = (a + u64::from(*byte)) % 65_521;
                b = (b + a) % 65_521;
            }
            u32::try_from((b << 16) | a).unwrap_or(0)
        }
        let data: Vec<u8> = (0..20_000_u32)
            .map(|i| u8::try_from(i.wrapping_mul(31) % 251).unwrap_or(0))
            .collect();
        for len in [0_usize, 1, 5_551, 5_552, 5_553, 11_104, 20_000] {
            let prefix = data.get(..len).unwrap_or(&[]);
            assert_eq!(adler32(prefix), reference(prefix), "length {len}");
        }
    }

    use super::{inflate_zlib, BitReader, Huffman};
    use sse_core::Error;

    // zlib streams generated once with Python's stdlib zlib at level 0 / Z_FIXED / default.
    const STORED: &[u8] = &[
        0x78, 0x01, 0x01, 0x0B, 0x00, 0xF4, 0xFF, 0x73, 0x74, 0x6F, 0x72, 0x65, 0x64, 0x2D, 0x64, 0x61, 0x74, 0x61,
        0x1A, 0xF3, 0x04, 0x59,
    ];

    const FIXED: &[u8] = &[
        0x78, 0x01, 0x73, 0x74, 0x0A, 0x72, 0x74, 0x76, 0x74, 0x71, 0x04, 0x52, 0xBA, 0x8E, 0x23, 0x80, 0xCD, 0xC0,
        0xC8, 0xC4, 0xCC, 0xC2, 0xCA, 0xC6, 0xCE, 0xC1, 0xC9, 0xC5, 0xCD, 0xC3, 0xCB, 0xC7, 0x2F, 0x20, 0x28, 0x24,
        0x2C, 0x22, 0x2A, 0x26, 0x2E, 0x21, 0x29, 0x25, 0x2D, 0x23, 0x2B, 0x27, 0x0F, 0x00, 0x68, 0x93, 0x40, 0x85,
    ];

    const DYNAMIC: &[u8] = &[
        0x78, 0x9C, 0xED, 0xCA, 0x49, 0x16, 0xC1, 0x50, 0x10, 0x86, 0xD1, 0xAD, 0xD4, 0x12, 0x04, 0xD1, 0x0C, 0x2D,
        0x05, 0x09, 0x89, 0xEE, 0x11, 0x89, 0x6E, 0xF5, 0x1C, 0x7B, 0x78, 0xB3, 0xFB, 0x8F, 0xBE, 0x3A, 0x75, 0xFB,
        0xA6, 0x8E, 0xDB, 0xD0, 0x6E, 0x8F, 0xB1, 0xE9, 0xD2, 0xF3, 0x12, 0xBB, 0xF4, 0x8A, 0xC3, 0x70, 0xBE, 0xDE,
        0x23, 0x3D, 0xEA, 0x2E, 0xFA, 0xDF, 0xFB, 0xB4, 0xFE, 0xBC, 0xA3, 0x4A, 0xFB, 0xFF, 0xC1, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB,
        0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x2C, 0xCB, 0xB2, 0x39, 0xED,
        0xCA, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xB2, 0x6F, 0x54, 0x8C, 0x27, 0xD3, 0x72, 0x36,
        0x5F, 0x2C, 0x95, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0x4A, 0x29, 0xA5, 0x94, 0x52, 0xB9, 0xEA, 0x0B, 0x7F,
        0x50, 0xDC, 0xBE,
    ];

    #[test]
    fn stored_block_decodes() {
        assert_eq!(inflate_zlib(STORED, 64), Ok(b"stored-data".to_vec()));
    }

    #[test]
    fn fixed_huffman_block_decodes() {
        let mut expected = Vec::new();
        let mut count = 0_usize;
        while count < 20 {
            expected.extend_from_slice(b"ABRACADABRA-");
            count = count.checked_add(1).unwrap_or(20);
        }
        let mut byte = 0_u8;
        while byte < 32 {
            expected.push(byte);
            byte = byte.checked_add(1).unwrap_or(32);
        }
        assert_eq!(inflate_zlib(FIXED, expected.len()), Ok(expected));
    }

    #[test]
    fn dynamic_huffman_block_decodes() {
        let mut expected = Vec::new();
        let mut count = 0_usize;
        while count < 1_000 {
            expected.extend_from_slice(b"the quick brown fox jumps over the lazy dog ");
            count = count.checked_add(1).unwrap_or(1_000);
        }
        expected.extend(core::iter::repeat_n(b'A', 5_000));
        let mut repeats = 0_usize;
        while repeats < 500 {
            expected.extend_from_slice(b"0123456789");
            repeats = repeats.checked_add(1).unwrap_or(500);
        }
        assert_eq!(inflate_zlib(DYNAMIC, expected.len()), Ok(expected));
    }

    #[test]
    fn output_limit_stops_before_growth() {
        assert!(matches!(inflate_zlib(STORED, 10), Err(Error::Damaged(_))));
    }

    #[test]
    fn bad_adler_is_rejected() {
        let mut stream = STORED.to_vec();
        if let Some(last) = stream.last_mut() {
            *last ^= 1;
        }
        assert!(matches!(inflate_zlib(&stream, 64), Err(Error::Damaged(_))));
    }

    #[test]
    fn every_truncation_is_an_error() {
        let mut length = 0_usize;
        while length < STORED.len() {
            let prefix = STORED.get(..length).unwrap_or_default();
            assert!(inflate_zlib(prefix, 64).is_err());
            length = length.checked_add(1).unwrap_or(STORED.len());
        }
    }

    #[test]
    fn huffman_refuses_an_oversubscribed_tree() {
        // Three 1-bit codes cannot fit in a prefix code: only two 1-bit words exist.
        assert!(matches!(Huffman::from_lengths(&[1, 1, 1]), Err(Error::Damaged(_))));
    }

    #[test]
    fn huffman_refuses_an_incomplete_tree_with_several_symbols() {
        // Two 2-bit codes leave half of the code space unused; RFC 1951 only allows that for one symbol.
        assert!(matches!(Huffman::from_lengths(&[2, 2]), Err(Error::Damaged(_))));
    }

    #[test]
    fn huffman_accepts_a_single_one_bit_symbol() {
        assert!(Huffman::from_lengths(&[1]).is_ok());
    }

    #[test]
    fn huffman_refuses_an_empty_tree_and_long_codes() {
        assert!(matches!(Huffman::from_lengths(&[0, 0, 0]), Err(Error::Damaged(_))));
        assert!(matches!(Huffman::from_lengths(&[16]), Err(Error::Damaged(_))));
    }

    #[test]
    fn huffman_decodes_the_rfc1951_canonical_example() {
        // RFC 1951 section 3.2.2 example: A..H with lengths 3,3,3,3,3,2,4,4 give
        // F=00, A=010, B=011, C=100, D=101, E=110, G=1110, H=1111 (codes as written MSB first).
        let lengths = [3_u8, 3, 3, 3, 3, 2, 4, 4];
        let table = Huffman::from_lengths(&lengths).unwrap_or_else(|error| panic!("{error:?}"));
        let codes: [(&str, usize); 8] = [
            ("010", 0),
            ("011", 1),
            ("100", 2),
            ("101", 3),
            ("110", 4),
            ("00", 5),
            ("1110", 6),
            ("1111", 7),
        ];
        // Pack the codes into a DEFLATE-style stream: each code is sent most significant bit first.
        let mut bits = Vec::new();
        let mut expected = Vec::new();
        for _ in 0..3 {
            for (code, symbol) in codes.iter().rev() {
                bits.extend(code.chars().map(|c| c == '1'));
                expected.push(*symbol);
            }
        }
        let mut bytes = vec![0_u8; bits.len().div_ceil(8)];
        for (index, bit) in bits.iter().enumerate() {
            if *bit {
                if let Some(byte) = bytes.get_mut(index / 8) {
                    *byte |= 1 << (index % 8);
                }
            }
        }
        let mut reader = BitReader::new(&bytes);
        for symbol in expected {
            let decoded = table.decode(&mut reader).unwrap_or_else(|error| panic!("{error:?}"));
            assert_eq!(usize::from(decoded), symbol);
        }
    }
}
