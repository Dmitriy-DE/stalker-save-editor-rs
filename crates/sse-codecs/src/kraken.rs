//! Safe, dependency-free Kraken decoder for the subset used by game-save streams.
//!
//! This implementation is intentionally independent from the pointer-oriented reference decoder. It keeps
//! all source/destination positions as checked slice offsets, bounds every table and recursion level, and
//! refuses non-Kraken codecs explicitly.

use sse_core::{Error, Result};

const KRAKEN: u8 = 6;
const MERMAID: u8 = 10;
const LZNA: u8 = 5;
const BITKNIT: u8 = 11;
const LEVIATHAN: u8 = 12;
const BLOCK: usize = 0x40000;
const SUB_BLOCK: usize = 0x20000;
const MAX_SCRATCH: usize = 0x80000;
const MAX_RECURSION: usize = 16;
const MAX_ENTROPY_ARRAYS: usize = 63;
const MAX_HUFF_SYMBOLS: usize = 256;
const HUFF_BITS: usize = 11;
const HUFF_LUT: usize = 1 << HUFF_BITS;
const MAX_LEN_EXTENSIONS: usize = 512;

/// Decompresses one Kraken stream into an exactly-sized output slice.
///
/// # Errors
/// Returns [`Error::Damaged`] for malformed/truncated Kraken input and [`Error::Refused`] for Oodle codecs
/// other than Kraken or for checksum-protected blocks whose checksum algorithm is not part of this package.
pub fn decompress_into(source: &[u8], output: &mut [u8]) -> Result<()> {
    let mut src = 0_usize;
    let mut dst = 0_usize;
    let mut header = BlockHeader::default();
    while dst < output.len() {
        if dst.checked_rem(BLOCK).unwrap_or_default() == 0 {
            header = parse_block_header(source, &mut src)?;
            match header.decoder_type {
                KRAKEN => {}
                MERMAID => return Err(Error::Refused("Mermaid stream is not Kraken".to_owned())),
                LZNA => return Err(Error::Refused("LZNA stream is not Kraken".to_owned())),
                BITKNIT => return Err(Error::Refused("Bitknit stream is not Kraken".to_owned())),
                LEVIATHAN => return Err(Error::Refused("Leviathan stream is not Kraken".to_owned())),
                other => return Err(Error::Refused(format!("unsupported Oodle decoder type {other}"))),
            }
            if header.use_checksums {
                return Err(Error::Refused(
                    "checksum-protected Kraken blocks are refused: the supplied reference leaves the checksum algorithm unimplemented"
                        .to_owned(),
                ));
            }
        }
        let amount = output.len().saturating_sub(dst).min(BLOCK);
        let dst_end = dst
            .checked_add(amount)
            .ok_or_else(|| Error::damaged("Kraken output position overflow"))?;
        if header.uncompressed {
            let src_end = src
                .checked_add(amount)
                .ok_or_else(|| Error::damaged("uncompressed Kraken source overflow"))?;
            let input = source
                .get(src..src_end)
                .ok_or_else(|| Error::damaged("truncated uncompressed Kraken block"))?;
            output
                .get_mut(dst..dst_end)
                .ok_or_else(|| Error::damaged("Kraken output range outside buffer"))?
                .copy_from_slice(input);
            src = src_end;
            dst = dst_end;
            continue;
        }

        let quantum = parse_quantum_header(source, &mut src, header.use_checksums)?;
        if quantum.compressed_size == 0 {
            if quantum.whole_match_distance != 0 {
                let distance = usize::try_from(quantum.whole_match_distance)
                    .map_err(|_| Error::damaged("whole-match distance does not fit usize"))?;
                copy_match(output, dst, amount, distance)?;
            } else {
                output
                    .get_mut(dst..dst_end)
                    .ok_or_else(|| Error::damaged("memset quantum outside output"))?
                    .fill(quantum.special_byte);
            }
            dst = dst_end;
            continue;
        }
        let compressed = usize::try_from(quantum.compressed_size)
            .map_err(|_| Error::damaged("quantum compressed size does not fit usize"))?;
        if compressed > amount {
            return Err(Error::damaged(
                "Kraken quantum compressed size exceeds raw quantum size",
            ));
        }
        let src_end = src
            .checked_add(compressed)
            .ok_or_else(|| Error::damaged("quantum source range overflow"))?;
        let payload = source
            .get(src..src_end)
            .ok_or_else(|| Error::damaged("truncated Kraken quantum"))?;
        if compressed == amount {
            output
                .get_mut(dst..dst_end)
                .ok_or_else(|| Error::damaged("quantum output outside destination"))?
                .copy_from_slice(payload);
        } else {
            decode_quantum(payload, output, dst, dst_end)?;
        }
        src = src_end;
        dst = dst_end;
    }
    if src != source.len() {
        return Err(Error::damaged(format!(
            "{} trailing bytes after Kraken stream",
            source.len().saturating_sub(src)
        )));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Default)]
struct BlockHeader {
    decoder_type: u8,
    uncompressed: bool,
    use_checksums: bool,
}

#[derive(Clone, Copy, Debug, Default)]
struct QuantumHeader {
    compressed_size: u32,
    special_byte: u8,
    whole_match_distance: u32,
}

fn parse_block_header(source: &[u8], at: &mut usize) -> Result<BlockHeader> {
    let first = take_u8(source, at)?;
    if first & 0x0f != 0x0c || (first >> 4) & 0x03 != 0 {
        return Err(Error::damaged("invalid Kraken block header"));
    }
    let second = take_u8(source, at)?;
    let decoder_type = second & 0x7f;
    if !matches!(decoder_type, KRAKEN | MERMAID | LZNA | BITKNIT | LEVIATHAN) {
        return Err(Error::damaged("unknown Oodle decoder type in block header"));
    }
    Ok(BlockHeader {
        decoder_type,
        uncompressed: first & 0x40 != 0,
        use_checksums: second & 0x80 != 0,
    })
}

fn parse_quantum_header(source: &[u8], at: &mut usize, use_checksum: bool) -> Result<QuantumHeader> {
    let a = u32::from(take_u8(source, at)?);
    let b = u32::from(take_u8(source, at)?);
    let c = u32::from(take_u8(source, at)?);
    let value = a.checked_shl(16).unwrap_or_default() | b.checked_shl(8).unwrap_or_default() | c;
    let size = value & 0x3ffff;
    if size != 0x3ffff {
        let compressed_size = size
            .checked_add(1)
            .ok_or_else(|| Error::damaged("quantum size overflow"))?;
        if use_checksum {
            let _checksum = take_u24_be(source, at)?;
        }
        return Ok(QuantumHeader {
            compressed_size,
            special_byte: 0,
            whole_match_distance: 0,
        });
    }
    let special = value >> 18;
    if special == 1 {
        let special_byte = take_u8(source, at)?;
        return Ok(QuantumHeader {
            compressed_size: 0,
            special_byte,
            whole_match_distance: 0,
        });
    }
    Err(Error::damaged("unsupported Kraken special quantum header"))
}

fn decode_quantum(source: &[u8], output: &mut [u8], mut dst: usize, dst_end: usize) -> Result<()> {
    let mut src = 0_usize;
    while dst < dst_end {
        let raw_size = dst_end.saturating_sub(dst).min(SUB_BLOCK);
        let raw_end = dst
            .checked_add(raw_size)
            .ok_or_else(|| Error::damaged("Kraken sub-block output overflow"))?;
        if source.len().saturating_sub(src) < 3 {
            return Err(Error::damaged("truncated Kraken sub-block header"));
        }
        let chunk_header = peek_u24_be(source, src)?;
        if chunk_header & 0x800000 == 0 {
            let destination = output
                .get_mut(dst..raw_end)
                .ok_or_else(|| Error::damaged("entropy-only output range invalid"))?;
            let used = decode_entropy_from(
                source
                    .get(src..)
                    .ok_or_else(|| Error::damaged("entropy source range invalid"))?,
                destination,
                0,
            )?;
            if destination.len() != raw_size {
                return Err(Error::damaged("entropy-only Kraken block decoded wrong size"));
            }
            src = src
                .checked_add(used)
                .ok_or_else(|| Error::damaged("entropy source position overflow"))?;
        } else {
            src = src
                .checked_add(3)
                .ok_or_else(|| Error::damaged("LZ source position overflow"))?;
            let compressed = usize::try_from(chunk_header & 0x7ffff)
                .map_err(|_| Error::damaged("LZ chunk size conversion failed"))?;
            let mode =
                u8::try_from((chunk_header >> 19) & 0x0f).map_err(|_| Error::damaged("LZ mode conversion failed"))?;
            let src_end = src
                .checked_add(compressed)
                .ok_or_else(|| Error::damaged("LZ payload range overflow"))?;
            let payload = source
                .get(src..src_end)
                .ok_or_else(|| Error::damaged("truncated Kraken LZ payload"))?;
            if compressed < raw_size {
                let offset = dst;
                let table = read_lz_table(mode, payload, output, dst, raw_end, offset)?;
                process_lz_runs(mode, table, output, dst, raw_end, offset)?;
            } else if compressed == raw_size && mode == 0 {
                output
                    .get_mut(dst..raw_end)
                    .ok_or_else(|| Error::damaged("raw LZ chunk output invalid"))?
                    .copy_from_slice(payload);
            } else {
                return Err(Error::damaged("invalid Kraken LZ chunk size/mode"));
            }
            src = src_end;
        }
        dst = raw_end;
    }
    if src != source.len() {
        return Err(Error::damaged(
            "Kraken quantum did not consume its complete compressed payload",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct EntropyHeader {
    kind: u8,
    payload_start: usize,
    payload_size: usize,
    output_size: usize,
    total_size: usize,
}

fn entropy_header(source: &[u8], capacity: usize) -> Result<EntropyHeader> {
    if source.len() < 2 {
        return Err(Error::damaged("truncated entropy block header"));
    }
    let first = source
        .first()
        .copied()
        .ok_or_else(|| Error::damaged("missing entropy header byte"))?;
    let kind = (first >> 4) & 7;
    if kind == 0 {
        let (payload_start, payload_size) = if first >= 0x80 {
            let second = source
                .get(1)
                .copied()
                .ok_or_else(|| Error::damaged("short raw entropy header"))?;
            let packed = u16::from(first).checked_shl(8).unwrap_or_default() | u16::from(second);
            (2_usize, usize::from(packed & 0x0fff))
        } else {
            if source.len() < 3 {
                return Err(Error::damaged("short long raw entropy header"));
            }
            let packed = peek_u24_be(source, 0)?;
            if packed & !0x3ffff != 0 {
                return Err(Error::damaged("reserved raw entropy header bits are set"));
            }
            (
                3_usize,
                usize::try_from(packed).map_err(|_| Error::damaged("raw entropy size conversion failed"))?,
            )
        };
        if payload_size > capacity {
            return Err(Error::damaged("raw entropy output exceeds capacity"));
        }
        let total_size = payload_start
            .checked_add(payload_size)
            .ok_or_else(|| Error::damaged("raw entropy block size overflow"))?;
        if total_size > source.len() {
            return Err(Error::damaged("truncated raw entropy payload"));
        }
        return Ok(EntropyHeader {
            kind,
            payload_start,
            payload_size,
            output_size: payload_size,
            total_size,
        });
    }
    if kind >= 6 {
        return Err(Error::damaged("invalid entropy codec type"));
    }
    let (payload_start, payload_size, output_size) = if first >= 0x80 {
        if source.len() < 3 {
            return Err(Error::damaged("short compact entropy header"));
        }
        let bits = peek_u24_be(source, 0)?;
        let src_size =
            usize::try_from(bits & 0x3ff).map_err(|_| Error::damaged("entropy src size conversion failed"))?;
        let delta =
            usize::try_from((bits >> 10) & 0x3ff).map_err(|_| Error::damaged("entropy delta conversion failed"))?;
        let dst_size = src_size
            .checked_add(delta)
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| Error::damaged("entropy output size overflow"))?;
        (3_usize, src_size, dst_size)
    } else {
        if source.len() < 5 {
            return Err(Error::damaged("short extended entropy header"));
        }
        let word = u32::from(source.get(1).copied().unwrap_or_default())
            .checked_shl(24)
            .unwrap_or_default()
            | u32::from(source.get(2).copied().unwrap_or_default())
                .checked_shl(16)
                .unwrap_or_default()
            | u32::from(source.get(3).copied().unwrap_or_default())
                .checked_shl(8)
                .unwrap_or_default()
            | u32::from(source.get(4).copied().unwrap_or_default());
        let src_size =
            usize::try_from(word & 0x3ffff).map_err(|_| Error::damaged("entropy src size conversion failed"))?;
        let high = u32::from(first).checked_shl(14).unwrap_or_default();
        let dst_bits = ((word >> 18) | high) & 0x3ffff;
        let dst_size = usize::try_from(dst_bits)
            .ok()
            .and_then(|v| v.checked_add(1))
            .ok_or_else(|| Error::damaged("entropy dst size overflow"))?;
        if src_size >= dst_size {
            return Err(Error::damaged(
                "compressed entropy size is not smaller than decoded size",
            ));
        }
        (5_usize, src_size, dst_size)
    };
    if output_size > capacity {
        return Err(Error::damaged("entropy output exceeds capacity"));
    }
    let total_size = payload_start
        .checked_add(payload_size)
        .ok_or_else(|| Error::damaged("entropy block size overflow"))?;
    if total_size > source.len() {
        return Err(Error::damaged("truncated entropy payload"));
    }
    Ok(EntropyHeader {
        kind,
        payload_start,
        payload_size,
        output_size,
        total_size,
    })
}

fn decode_entropy_from(source: &[u8], output: &mut [u8], depth: usize) -> Result<usize> {
    let header = entropy_header(source, output.len())?;
    if header.output_size != output.len() {
        return Err(Error::damaged("entropy block decoded size does not match destination"));
    }
    let payload_end = header
        .payload_start
        .checked_add(header.payload_size)
        .ok_or_else(|| Error::damaged("entropy payload end overflow"))?;
    let payload = source
        .get(header.payload_start..payload_end)
        .ok_or_else(|| Error::damaged("entropy payload outside source"))?;
    match header.kind {
        0 => output.copy_from_slice(payload),
        1 => decode_tans(payload, output)?,
        2 => decode_huffman(payload, output, 1)?,
        3 => decode_rle(
            payload,
            output,
            depth
                .checked_add(1)
                .ok_or_else(|| Error::damaged("RLE recursion overflow"))?,
        )?,
        4 => decode_huffman(payload, output, 2)?,
        5 => decode_recursive(
            payload,
            output,
            depth
                .checked_add(1)
                .ok_or_else(|| Error::damaged("recursive entropy depth overflow"))?,
        )?,
        _ => return Err(Error::damaged("unknown entropy codec")),
    }
    Ok(header.total_size)
}

fn decode_entropy_owned(source: &[u8], capacity: usize, depth: usize) -> Result<(Vec<u8>, usize)> {
    let header = entropy_header(source, capacity)?;
    if header.output_size > MAX_SCRATCH {
        return Err(Error::Refused(
            "single entropy scratch array exceeds 512 KiB".to_owned(),
        ));
    }
    let mut output = vec![0_u8; header.output_size];
    let used = decode_entropy_from(source, &mut output, depth)?;
    Ok((output, used))
}

#[derive(Clone, Debug)]
struct HeaderBits<'a> {
    data: &'a [u8],
    pos: usize,
    bits: u32,
    bitpos: i32,
}

impl<'a> HeaderBits<'a> {
    fn new(data: &'a [u8]) -> Result<Self> {
        let mut this = Self {
            data,
            pos: 0,
            bits: 0,
            bitpos: 24,
        };
        this.refill()?;
        Ok(this)
    }

    fn refill(&mut self) -> Result<()> {
        if self.bitpos > 24 {
            return Err(Error::damaged("bit reader refill with invalid bit position"));
        }
        while self.bitpos > 0 {
            let byte = self
                .data
                .get(self.pos)
                .copied()
                .ok_or_else(|| Error::damaged("truncated bitstream"))?;
            let shift = u32::try_from(self.bitpos).map_err(|_| Error::damaged("negative bit position"))?;
            self.bits |= u32::from(byte).checked_shl(shift).unwrap_or_default();
            self.bitpos = self
                .bitpos
                .checked_sub(8)
                .ok_or_else(|| Error::damaged("bit position underflow"))?;
            self.pos = self
                .pos
                .checked_add(1)
                .ok_or_else(|| Error::damaged("bitstream position overflow"))?;
        }
        Ok(())
    }

    fn read_bit_no_refill(&mut self) -> Result<u32> {
        let value = self.bits >> 31;
        self.bits = self.bits.checked_shl(1).unwrap_or_default();
        self.bitpos = self
            .bitpos
            .checked_add(1)
            .ok_or_else(|| Error::damaged("bit position overflow"))?;
        Ok(value)
    }

    fn read_bit(&mut self) -> Result<u32> {
        self.refill()?;
        self.read_bit_no_refill()
    }

    fn read_bits_no_refill(&mut self, count: u32) -> Result<u32> {
        if count == 0 || count > 31 {
            return Err(Error::damaged("invalid bit count"));
        }
        let shift = 32_u32
            .checked_sub(count)
            .ok_or_else(|| Error::damaged("bit shift underflow"))?;
        let value = self.bits >> shift;
        self.bits = self.bits.checked_shl(count).unwrap_or_default();
        self.bitpos = self
            .bitpos
            .checked_add(i32::try_from(count).map_err(|_| Error::damaged("bit count conversion failed"))?)
            .ok_or_else(|| Error::damaged("bit position overflow"))?;
        Ok(value)
    }

    fn read_bits_zero(&mut self, count: u32) -> Result<u32> {
        if count == 0 {
            return Ok(0);
        }
        self.read_bits_no_refill(count)
    }

    fn leading_zeros(&self) -> u32 {
        self.bits.leading_zeros()
    }

    fn rice_reader(&self) -> Result<RiceBits<'a>> {
        let delta = 24_i32
            .checked_sub(self.bitpos)
            .ok_or_else(|| Error::damaged("Rice pointer delta overflow"))?;
        let rounded = delta
            .checked_add(7)
            .ok_or_else(|| Error::damaged("Rice pointer rounding overflow"))?
            >> 3;
        let rounded = usize::try_from(rounded.max(0)).map_err(|_| Error::damaged("Rice pointer conversion failed"))?;
        let byte = self
            .pos
            .checked_sub(rounded)
            .ok_or_else(|| Error::damaged("Rice pointer before input"))?;
        let bitpos = u8::try_from(
            (self
                .bitpos
                .checked_sub(24)
                .ok_or_else(|| Error::damaged("Rice bitpos underflow"))?
                & 7)
            .max(0),
        )
        .map_err(|_| Error::damaged("Rice bitpos conversion failed"))?;
        Ok(RiceBits {
            data: self.data,
            byte,
            bitpos,
        })
    }

    fn reset_from_rice(&mut self, rice: &RiceBits<'a>) -> Result<()> {
        self.pos = rice.byte;
        self.bits = 0;
        self.bitpos = 24;
        self.refill()?;
        let shift = u32::from(rice.bitpos);
        self.bits = self.bits.checked_shl(shift).unwrap_or_default();
        self.bitpos = self
            .bitpos
            .checked_add(i32::from(rice.bitpos))
            .ok_or_else(|| Error::damaged("bitpos reset overflow"))?;
        Ok(())
    }

    fn data_pointer(&self) -> Result<usize> {
        let delta = 24_i32
            .checked_sub(self.bitpos)
            .ok_or_else(|| Error::damaged("bit pointer delta overflow"))?;
        let bytes = usize::try_from((delta / 8).max(0)).map_err(|_| Error::damaged("bit pointer conversion failed"))?;
        self.pos
            .checked_sub(bytes)
            .ok_or_else(|| Error::damaged("bit pointer before source"))
    }
}

#[derive(Clone, Debug)]
struct RiceBits<'a> {
    data: &'a [u8],
    byte: usize,
    bitpos: u8,
}

impl<'a> RiceBits<'a> {
    fn read_bit(&mut self) -> Result<u8> {
        let byte = self
            .data
            .get(self.byte)
            .copied()
            .ok_or_else(|| Error::damaged("truncated Golomb-Rice stream"))?;
        let shift = 7_u8.saturating_sub(self.bitpos);
        let value = (byte >> shift) & 1;
        self.bitpos = self.bitpos.saturating_add(1);
        if self.bitpos == 8 {
            self.bitpos = 0;
            self.byte = self
                .byte
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Rice byte position overflow"))?;
        }
        Ok(value)
    }

    fn read_bits(&mut self, count: u8) -> Result<u32> {
        let mut value = 0_u32;
        for _ in 0..count {
            value = value.checked_shl(1).unwrap_or_default() | u32::from(self.read_bit()?);
        }
        Ok(value)
    }
}

fn decode_rice_lengths(reader: &mut RiceBits<'_>, count: usize) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(count);
    let mut zeros = 0_u16;
    while out.len() < count {
        if reader.read_bit()? == 0 {
            zeros = zeros
                .checked_add(1)
                .ok_or_else(|| Error::damaged("Golomb-Rice zero run overflow"))?;
            if zeros > u16::from(u8::MAX) {
                return Err(Error::damaged("Golomb-Rice run exceeds u8"));
            }
        } else {
            out.push(u8::try_from(zeros).map_err(|_| Error::damaged("Rice length conversion failed"))?);
            zeros = 0;
        }
    }
    Ok(out)
}

fn decode_rice_bits(reader: &mut RiceBits<'_>, values: &mut [u8], bitcount: u8) -> Result<()> {
    if bitcount > 3 {
        return Err(Error::damaged("Golomb-Rice extra bit count exceeds 3"));
    }
    for value in values {
        let extra = reader.read_bits(bitcount)?;
        let shifted = u32::from(*value).checked_shl(u32::from(bitcount)).unwrap_or_default();
        *value = u8::try_from(shifted | extra).map_err(|_| Error::damaged("Golomb-Rice value exceeds byte"))?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
struct HuffRange {
    symbol: u16,
    count: u16,
}

fn read_fluff(bits: &mut HeaderBits<'_>, num_symbols: usize) -> Result<usize> {
    if num_symbols == 256 {
        return Ok(0);
    }
    let mut x = 257_usize.saturating_sub(num_symbols);
    x = x.min(num_symbols);
    x = x
        .checked_mul(2)
        .ok_or_else(|| Error::damaged("Huffman fluff range overflow"))?;
    if x == 0 {
        return Err(Error::damaged("invalid Huffman symbol count"));
    }
    let y = usize::try_from(usize::BITS.saturating_sub((x.saturating_sub(1)).leading_zeros()))
        .map_err(|_| Error::damaged("Huffman fluff bit count conversion failed"))?;
    let value =
        bits.read_bits_no_refill(u32::try_from(y).map_err(|_| Error::damaged("Huffman fluff width overflow"))?)?;
    let z = (1_usize
        .checked_shl(u32::try_from(y).map_err(|_| Error::damaged("Huffman fluff shift width overflow"))?)
        .unwrap_or_default())
    .saturating_sub(x);
    let value_usize = usize::try_from(value).map_err(|_| Error::damaged("Huffman fluff value conversion failed"))?;
    if value_usize >> 1 >= z {
        Ok(value_usize.saturating_sub(z))
    } else {
        // The reference consumes one fewer bit in this branch. Reconstruct that by rewinding one logical bit.
        bits.bits = bits.bits.rotate_right(1);
        bits.bitpos = bits.bitpos.saturating_sub(1);
        Ok(value_usize >> 1)
    }
}

fn huff_convert_ranges(
    bits: &mut HeaderBits<'_>,
    num_symbols: usize,
    fluff: usize,
    symlen: &[u8],
) -> Result<Vec<HuffRange>> {
    let num_ranges = fluff >> 1;
    let mut sym_idx = 0_usize;
    let mut len_at = 0_usize;
    if fluff & 1 != 0 {
        bits.refill()?;
        let v = u32::from(
            *symlen
                .get(len_at)
                .ok_or_else(|| Error::damaged("missing Huffman range length"))?,
        );
        len_at = len_at.saturating_add(1);
        if v >= 8 {
            return Err(Error::damaged("Huffman range space width is too large"));
        }
        let base = 1_usize
            .checked_shl(v.saturating_add(1))
            .unwrap_or_default()
            .saturating_sub(1);
        sym_idx = usize::try_from(bits.read_bits_no_refill(v.saturating_add(1))?)
            .map_err(|_| Error::damaged("Huffman range value conversion failed"))?
            .saturating_add(base);
    }
    let mut ranges = Vec::with_capacity(num_ranges.saturating_add(1));
    let mut used = 0_usize;
    for _ in 0..num_ranges {
        bits.refill()?;
        let v_num = u32::from(
            *symlen
                .get(len_at)
                .ok_or_else(|| Error::damaged("missing Huffman range count width"))?,
        );
        len_at = len_at.saturating_add(1);
        if v_num >= 9 {
            return Err(Error::damaged("Huffman range count width is too large"));
        }
        let count = usize::try_from(bits.read_bits_zero(v_num)?)
            .map_err(|_| Error::damaged("Huffman range count conversion failed"))?
            .saturating_add(1_usize.checked_shl(v_num).unwrap_or_default());
        let v_space = u32::from(
            *symlen
                .get(len_at)
                .ok_or_else(|| Error::damaged("missing Huffman range space width"))?,
        );
        len_at = len_at.saturating_add(1);
        if v_space >= 8 {
            return Err(Error::damaged("Huffman range space width is too large"));
        }
        let space = usize::try_from(bits.read_bits_no_refill(v_space.saturating_add(1))?)
            .map_err(|_| Error::damaged("Huffman range space conversion failed"))?
            .saturating_add(
                1_usize
                    .checked_shl(v_space.saturating_add(1))
                    .unwrap_or_default()
                    .saturating_sub(1),
            );
        ranges.push(HuffRange {
            symbol: u16::try_from(sym_idx).map_err(|_| Error::damaged("Huffman symbol index exceeds u16"))?,
            count: u16::try_from(count).map_err(|_| Error::damaged("Huffman range count exceeds u16"))?,
        });
        used = used
            .checked_add(count)
            .ok_or_else(|| Error::damaged("Huffman used-symbol count overflow"))?;
        sym_idx = sym_idx
            .checked_add(count)
            .and_then(|v| v.checked_add(space))
            .ok_or_else(|| Error::damaged("Huffman range symbol overflow"))?;
    }
    if sym_idx >= 256 || used >= num_symbols || sym_idx.saturating_add(num_symbols.saturating_sub(used)) > 256 {
        return Err(Error::damaged("invalid Huffman symbol ranges"));
    }
    ranges.push(HuffRange {
        symbol: u16::try_from(sym_idx).map_err(|_| Error::damaged("Huffman final symbol index exceeds u16"))?,
        count: u16::try_from(num_symbols.saturating_sub(used))
            .map_err(|_| Error::damaged("Huffman final range count exceeds u16"))?,
    });
    Ok(ranges)
}

fn read_huff_lengths_old(bits: &mut HeaderBits<'_>, syms: &mut Vec<u8>, counts: &mut [usize; 12]) -> Result<usize> {
    if bits.read_bit_no_refill()? != 0 {
        let forced = bits.read_bits_no_refill(2)?;
        let mut symbol = 0_usize;
        let mut num_symbols = 0_usize;
        let mut average_x4 = 32_i32;
        let threshold_shift = 20_u32 >> forced;
        let threshold = 1_u32
            .checked_shl(31_u32.saturating_sub(threshold_shift))
            .unwrap_or_default();
        let mut skip_zeros = bits.read_bit()? != 0;
        loop {
            if !skip_zeros {
                if bits.bits & 0xff00_0000 == 0 {
                    return Err(Error::damaged("invalid old Huffman zero run gamma"));
                }
                let lz = bits.leading_zeros();
                let width = lz
                    .saturating_add(1)
                    .checked_mul(2)
                    .ok_or_else(|| Error::damaged("Huffman gamma width overflow"))?;
                let raw = bits.read_bits_no_refill(width)?;
                let add = usize::try_from(raw.saturating_sub(1))
                    .map_err(|_| Error::damaged("Huffman zero run conversion failed"))?;
                symbol = symbol
                    .checked_add(add)
                    .ok_or_else(|| Error::damaged("Huffman symbol overflow"))?;
                if symbol >= 256 {
                    break;
                }
            }
            bits.refill()?;
            if bits.bits & 0xff00_0000 == 0 {
                return Err(Error::damaged("invalid old Huffman symbol-run gamma"));
            }
            let lz = bits.leading_zeros();
            let width = lz
                .saturating_add(1)
                .checked_mul(2)
                .ok_or_else(|| Error::damaged("Huffman gamma width overflow"))?;
            let raw = bits.read_bits_no_refill(width)?;
            let run =
                usize::try_from(raw.saturating_sub(1)).map_err(|_| Error::damaged("Huffman run conversion failed"))?;
            if symbol.saturating_add(run) > 256 {
                return Err(Error::damaged("Huffman symbol run exceeds alphabet"));
            }
            bits.refill()?;
            num_symbols = num_symbols
                .checked_add(run)
                .ok_or_else(|| Error::damaged("Huffman symbol count overflow"))?;
            for _ in 0..run {
                if bits.bits < threshold {
                    return Err(Error::damaged("old Huffman code-length gamma too large"));
                }
                let zeros = bits.leading_zeros();
                let read = zeros
                    .checked_add(forced)
                    .and_then(|v| v.checked_add(1))
                    .ok_or_else(|| Error::damaged("old Huffman code width overflow"))?;
                let raw_value = i32::try_from(bits.read_bits_no_refill(read)?)
                    .map_err(|_| Error::damaged("old Huffman code value conversion failed"))?;
                let zeros_i =
                    i32::try_from(zeros).map_err(|_| Error::damaged("old Huffman zero count conversion failed"))?;
                let forced_i =
                    i32::try_from(forced).map_err(|_| Error::damaged("old Huffman forced width conversion failed"))?;
                let adjustment = zeros_i
                    .saturating_sub(1)
                    .checked_shl(u32::try_from(forced_i.max(0)).unwrap_or_default())
                    .unwrap_or_default();
                let value = raw_value.saturating_add(adjustment);
                let zig = (value & 1).wrapping_neg() ^ (value >> 1);
                let code_len = zig.saturating_add((average_x4.saturating_add(2)) >> 2);
                if !(1..=11).contains(&code_len) {
                    return Err(Error::damaged("old Huffman code length outside 1..11"));
                }
                average_x4 = code_len.saturating_add((3_i32.saturating_mul(average_x4).saturating_add(2)) >> 2);
                bits.refill()?;
                let code_index =
                    usize::try_from(code_len).map_err(|_| Error::damaged("Huffman code length conversion failed"))?;
                *counts
                    .get_mut(code_index)
                    .ok_or_else(|| Error::damaged("Huffman code-length bucket outside table"))? =
                    counts.get(code_index).copied().unwrap_or_default().saturating_add(1);
                syms.push(u8::try_from(symbol).map_err(|_| Error::damaged("Huffman symbol exceeds byte"))?);
                symbol = symbol.saturating_add(1);
            }
            if symbol == 256 {
                break;
            }
            skip_zeros = false;
        }
        if symbol != 256 || num_symbols < 2 {
            return Err(Error::damaged("invalid old Huffman symbol coverage"));
        }
        Ok(num_symbols)
    } else {
        let num_symbols = usize::try_from(bits.read_bits_no_refill(8)?)
            .map_err(|_| Error::damaged("Huffman symbol count conversion failed"))?;
        if num_symbols == 0 {
            return Err(Error::damaged("zero sparse Huffman symbols"));
        }
        if num_symbols == 1 {
            syms.push(
                u8::try_from(bits.read_bits_no_refill(8)?)
                    .map_err(|_| Error::damaged("Huffman symbol exceeds byte"))?,
            );
            return Ok(1);
        }
        let width = bits.read_bits_no_refill(3)?;
        if width > 4 {
            return Err(Error::damaged("sparse Huffman code-length width exceeds 4"));
        }
        for _ in 0..num_symbols {
            bits.refill()?;
            let symbol = u8::try_from(bits.read_bits_no_refill(8)?)
                .map_err(|_| Error::damaged("Huffman symbol exceeds byte"))?;
            let code_len = bits.read_bits_zero(width)?.saturating_add(1);
            if code_len > 11 {
                return Err(Error::damaged("sparse Huffman code length exceeds 11"));
            }
            let index =
                usize::try_from(code_len).map_err(|_| Error::damaged("Huffman code length conversion failed"))?;
            *counts
                .get_mut(index)
                .ok_or_else(|| Error::damaged("Huffman length bucket missing"))? =
                counts.get(index).copied().unwrap_or_default().saturating_add(1);
            syms.push(symbol);
        }
        Ok(num_symbols)
    }
}

fn read_huff_lengths_new(bits: &mut HeaderBits<'_>, syms: &mut Vec<u8>, counts: &mut [usize; 12]) -> Result<usize> {
    let forced_bits = u8::try_from(bits.read_bits_no_refill(2)?)
        .map_err(|_| Error::damaged("new Huffman forced width conversion failed"))?;
    let num_symbols = usize::try_from(bits.read_bits_no_refill(8)?)
        .map_err(|_| Error::damaged("new Huffman symbol count conversion failed"))?
        .saturating_add(1);
    let fluff = read_fluff(bits, num_symbols)?;
    let total = num_symbols
        .checked_add(fluff)
        .ok_or_else(|| Error::damaged("new Huffman Rice value count overflow"))?;
    if total > 512 {
        return Err(Error::damaged("new Huffman Rice table exceeds 512 entries"));
    }
    let mut rice = bits.rice_reader()?;
    let mut code_len = decode_rice_lengths(&mut rice, total)?;
    decode_rice_bits(
        &mut rice,
        code_len
            .get_mut(..num_symbols)
            .ok_or_else(|| Error::damaged("new Huffman code length range invalid"))?,
        forced_bits,
    )?;
    bits.reset_from_rice(&rice)?;

    let mut running_sum = 0x1e_i32;
    for slot in code_len.iter_mut().take(num_symbols) {
        let raw = i32::from(*slot);
        let delta = (raw & 1).wrapping_neg() ^ (raw >> 1);
        let actual = delta.saturating_add(running_sum >> 2).saturating_add(1);
        if !(1..=11).contains(&actual) {
            return Err(Error::damaged("new Huffman code length outside 1..11"));
        }
        *slot = u8::try_from(actual).map_err(|_| Error::damaged("new Huffman code length conversion failed"))?;
        running_sum = running_sum.saturating_add(delta);
    }
    let range_lengths = code_len
        .get(num_symbols..)
        .ok_or_else(|| Error::damaged("new Huffman range lengths missing"))?;
    let ranges = huff_convert_ranges(bits, num_symbols, fluff, range_lengths)?;
    let mut code_at = 0_usize;
    for range in ranges {
        let mut symbol = usize::from(range.symbol);
        for _ in 0..usize::from(range.count) {
            let code = *code_len
                .get(code_at)
                .ok_or_else(|| Error::damaged("new Huffman code length missing"))?;
            code_at = code_at.saturating_add(1);
            let index = usize::from(code);
            *counts
                .get_mut(index)
                .ok_or_else(|| Error::damaged("Huffman length bucket missing"))? =
                counts.get(index).copied().unwrap_or_default().saturating_add(1);
            syms.push(u8::try_from(symbol).map_err(|_| Error::damaged("new Huffman symbol exceeds byte"))?);
            symbol = symbol.saturating_add(1);
        }
    }
    if syms.len() != num_symbols {
        return Err(Error::damaged("new Huffman range coverage mismatch"));
    }
    Ok(num_symbols)
}

fn make_huff_lut(counts: &[usize; 12], syms: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut lengths = vec![0_u8; HUFF_LUT];
    let mut symbols = vec![0_u8; HUFF_LUT];
    let mut source = 0_usize;
    let mut slot = 0_usize;
    for length in 1_usize..=HUFF_BITS {
        let count = counts.get(length).copied().unwrap_or_default();
        let repeat_shift = u32::try_from(HUFF_BITS.saturating_sub(length))
            .map_err(|_| Error::damaged("Huffman repeat shift conversion failed"))?;
        let repeat = 1_usize.checked_shl(repeat_shift).unwrap_or_default();
        for _ in 0..count {
            let symbol = *syms
                .get(source)
                .ok_or_else(|| Error::damaged("Huffman symbol table shorter than code counts"))?;
            source = source.saturating_add(1);
            for _ in 0..repeat {
                let reversed = reverse_11(slot)?;
                *lengths
                    .get_mut(reversed)
                    .ok_or_else(|| Error::damaged("Huffman LUT length slot outside table"))? =
                    u8::try_from(length).map_err(|_| Error::damaged("Huffman length exceeds byte"))?;
                *symbols
                    .get_mut(reversed)
                    .ok_or_else(|| Error::damaged("Huffman LUT symbol slot outside table"))? = symbol;
                slot = slot
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("Huffman LUT slot overflow"))?;
            }
        }
    }
    if slot != HUFF_LUT || source != syms.len() {
        return Err(Error::damaged("Huffman tree is not complete"));
    }
    Ok((lengths, symbols))
}

fn reverse_11(value: usize) -> Result<usize> {
    let value = u16::try_from(value).map_err(|_| Error::damaged("Huffman index exceeds 11 bits"))?;
    Ok(usize::from(value.reverse_bits() >> 5))
}

#[derive(Clone, Debug)]
struct LsbForward<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}
impl<'a> LsbForward<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte: 0, bit: 0 }
    }
    fn peek(&self, count: u8) -> Result<u32> {
        let mut clone = self.clone();
        clone.read(count, true)
    }
    fn consume(&mut self, count: u8) -> Result<()> {
        let _ = self.read(count, false)?;
        Ok(())
    }
    fn read(&mut self, count: u8, padded: bool) -> Result<u32> {
        let mut value = 0_u32;
        for shift in 0..count {
            let bit_value = match self.data.get(self.byte).copied() {
                Some(byte) => (byte >> self.bit) & 1,
                None if padded => 0,
                None => return Err(Error::damaged("forward bitstream exhausted")),
            };
            value |= u32::from(bit_value).checked_shl(u32::from(shift)).unwrap_or_default();
            self.bit = self.bit.saturating_add(1);
            if self.bit == 8 {
                self.bit = 0;
                self.byte = self
                    .byte
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("forward bit position overflow"))?;
            }
        }
        Ok(value)
    }
    fn consumed_ceil(&self) -> usize {
        self.byte.saturating_add(usize::from(self.bit != 0))
    }
}

#[derive(Clone, Debug)]
struct LsbBackward<'a> {
    data: &'a [u8],
    byte_from_end: usize,
    bit: u8,
}
impl<'a> LsbBackward<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_from_end: 0,
            bit: 0,
        }
    }
    fn peek(&self, count: u8) -> Result<u32> {
        let mut c = self.clone();
        c.read(count, true)
    }
    fn consume(&mut self, count: u8) -> Result<()> {
        let _ = self.read(count, false)?;
        Ok(())
    }
    fn read(&mut self, count: u8, padded: bool) -> Result<u32> {
        let mut value = 0_u32;
        for shift in 0..count {
            let index = self.data.len().checked_sub(self.byte_from_end.saturating_add(1));
            let bit_value = match index.and_then(|i| self.data.get(i)).copied() {
                Some(byte) => (byte >> self.bit) & 1,
                None if padded => 0,
                None => return Err(Error::damaged("backward bitstream exhausted")),
            };
            value |= u32::from(bit_value).checked_shl(u32::from(shift)).unwrap_or_default();
            self.bit = self.bit.saturating_add(1);
            if self.bit == 8 {
                self.bit = 0;
                self.byte_from_end = self
                    .byte_from_end
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("backward bit position overflow"))?;
            }
        }
        Ok(value)
    }
    fn consumed_ceil(&self) -> usize {
        self.byte_from_end.saturating_add(usize::from(self.bit != 0))
    }
}

fn huff_symbol_forward(reader: &mut LsbForward<'_>, lengths: &[u8], symbols: &[u8]) -> Result<u8> {
    let index =
        usize::try_from(reader.peek(11)? & 0x7ff).map_err(|_| Error::damaged("Huffman LUT index conversion failed"))?;
    let len = *lengths
        .get(index)
        .ok_or_else(|| Error::damaged("Huffman LUT index outside table"))?;
    if len == 0 {
        return Err(Error::damaged("zero-length Huffman code"));
    }
    let symbol = *symbols
        .get(index)
        .ok_or_else(|| Error::damaged("Huffman symbol LUT index outside table"))?;
    reader.consume(len)?;
    Ok(symbol)
}
fn huff_symbol_backward(reader: &mut LsbBackward<'_>, lengths: &[u8], symbols: &[u8]) -> Result<u8> {
    let index =
        usize::try_from(reader.peek(11)? & 0x7ff).map_err(|_| Error::damaged("Huffman LUT index conversion failed"))?;
    let len = *lengths
        .get(index)
        .ok_or_else(|| Error::damaged("Huffman LUT index outside table"))?;
    if len == 0 {
        return Err(Error::damaged("zero-length Huffman code"));
    }
    let symbol = *symbols
        .get(index)
        .ok_or_else(|| Error::damaged("Huffman symbol LUT index outside table"))?;
    reader.consume(len)?;
    Ok(symbol)
}

fn decode_huff_three(stream_a: &[u8], shared: &[u8], output: &mut [u8], lengths: &[u8], symbols: &[u8]) -> Result<()> {
    let mut a = LsbForward::new(stream_a);
    let mut m = LsbForward::new(shared);
    let mut b = LsbBackward::new(shared);
    let mut at = 0_usize;
    while at < output.len() {
        if let Some(slot) = output.get_mut(at) {
            *slot = huff_symbol_forward(&mut a, lengths, symbols)?;
        }
        at = at.saturating_add(1);
        if at >= output.len() {
            break;
        }
        if let Some(slot) = output.get_mut(at) {
            *slot = huff_symbol_backward(&mut b, lengths, symbols)?;
        }
        at = at.saturating_add(1);
        if at >= output.len() {
            break;
        }
        if let Some(slot) = output.get_mut(at) {
            *slot = huff_symbol_forward(&mut m, lengths, symbols)?;
        }
        at = at.saturating_add(1);
    }
    if a.consumed_ceil() != stream_a.len() {
        return Err(Error::damaged("Huffman forward stream did not end at split"));
    }
    if m.consumed_ceil().saturating_add(b.consumed_ceil()) != shared.len() {
        return Err(Error::damaged("Huffman middle/backward streams did not meet"));
    }
    Ok(())
}

fn decode_huffman(source: &[u8], output: &mut [u8], kind: u8) -> Result<()> {
    let mut bits = HeaderBits::new(source)?;
    let mut counts = [0_usize; 12];
    let mut syms = Vec::with_capacity(MAX_HUFF_SYMBOLS);
    let first = bits.read_bit_no_refill()?;
    let num = if first == 0 {
        read_huff_lengths_old(&mut bits, &mut syms, &mut counts)?
    } else if bits.read_bit_no_refill()? == 0 {
        read_huff_lengths_new(&mut bits, &mut syms, &mut counts)?
    } else {
        return Err(Error::damaged("reserved Huffman code-length coding"));
    };
    if num == 0 {
        return Err(Error::damaged("Huffman table has no symbols"));
    }
    let start = bits.data_pointer()?;
    if num == 1 {
        let symbol = *syms
            .first()
            .ok_or_else(|| Error::damaged("single-symbol Huffman table is empty"))?;
        output.fill(symbol);
        return Ok(());
    }
    let (lengths, symbols) = make_huff_lut(&counts, &syms)?;
    if kind == 1 {
        let split_end = start
            .checked_add(2)
            .ok_or_else(|| Error::damaged("Huffman split header overflow"))?;
        let split = usize::from(read_u16_le(
            source
                .get(start..split_end)
                .ok_or_else(|| Error::damaged("short Huffman split"))?,
        )?);
        let data_start = split_end;
        let mid = data_start
            .checked_add(split)
            .ok_or_else(|| Error::damaged("Huffman split overflow"))?;
        if mid > source.len() {
            return Err(Error::damaged("Huffman split outside source"));
        }
        decode_huff_three(
            source
                .get(data_start..mid)
                .ok_or_else(|| Error::damaged("Huffman first stream invalid"))?,
            source
                .get(mid..)
                .ok_or_else(|| Error::damaged("Huffman shared stream invalid"))?,
            output,
            &lengths,
            &symbols,
        )?;
    } else {
        let split_mid_end = start
            .checked_add(3)
            .ok_or_else(|| Error::damaged("Huffman middle split overflow"))?;
        let split_mid = usize::try_from(read_u24_le(
            source
                .get(start..split_mid_end)
                .ok_or_else(|| Error::damaged("short Huffman middle split"))?,
        )?)
        .map_err(|_| Error::damaged("Huffman middle split conversion failed"))?;
        let first_base = split_mid_end;
        let middle = first_base
            .checked_add(split_mid)
            .ok_or_else(|| Error::damaged("Huffman middle position overflow"))?;
        if middle > source.len() {
            return Err(Error::damaged("Huffman middle split outside source"));
        }
        let left_header_end = first_base
            .checked_add(2)
            .ok_or_else(|| Error::damaged("Huffman left split header overflow"))?;
        let left = usize::from(read_u16_le(
            source
                .get(first_base..left_header_end)
                .ok_or_else(|| Error::damaged("short Huffman left split"))?,
        )?);
        let left_start = left_header_end;
        let left_mid = left_start
            .checked_add(left)
            .ok_or_else(|| Error::damaged("Huffman left split overflow"))?;
        if left_mid > middle {
            return Err(Error::damaged("Huffman left split crosses middle"));
        }
        let half = output.len().saturating_add(1) / 2;
        decode_huff_three(
            source
                .get(left_start..left_mid)
                .ok_or_else(|| Error::damaged("Huffman first-half forward stream invalid"))?,
            source
                .get(left_mid..middle)
                .ok_or_else(|| Error::damaged("Huffman first-half shared stream invalid"))?,
            output
                .get_mut(..half)
                .ok_or_else(|| Error::damaged("Huffman first-half output invalid"))?,
            &lengths,
            &symbols,
        )?;
        let right_header_end = middle
            .checked_add(2)
            .ok_or_else(|| Error::damaged("Huffman right split header overflow"))?;
        let right = usize::from(read_u16_le(
            source
                .get(middle..right_header_end)
                .ok_or_else(|| Error::damaged("short Huffman right split"))?,
        )?);
        let right_start = right_header_end;
        let right_mid = right_start
            .checked_add(right)
            .ok_or_else(|| Error::damaged("Huffman right split overflow"))?;
        if right_mid > source.len() {
            return Err(Error::damaged("Huffman right split outside source"));
        }
        decode_huff_three(
            source
                .get(right_start..right_mid)
                .ok_or_else(|| Error::damaged("Huffman second-half forward stream invalid"))?,
            source
                .get(right_mid..)
                .ok_or_else(|| Error::damaged("Huffman second-half shared stream invalid"))?,
            output
                .get_mut(half..)
                .ok_or_else(|| Error::damaged("Huffman second-half output invalid"))?,
            &lengths,
            &symbols,
        )?;
    }
    Ok(())
}

fn decode_rle(source: &[u8], output: &mut [u8], depth: usize) -> Result<()> {
    if depth > MAX_RECURSION {
        return Err(Error::damaged("RLE recursion limit exceeded"));
    }
    if source.len() == 1 {
        output.fill(source.first().copied().unwrap_or_default());
        return Ok(());
    }
    if source.is_empty() {
        return Err(Error::damaged("empty RLE payload"));
    }
    let mut commands: Vec<u8>;
    let (front, mut back): (&[u8], usize);
    if source.first().copied().unwrap_or_default() != 0 {
        let (decoded, used) = decode_entropy_owned(source, MAX_SCRATCH, depth)?;
        let tail = source
            .get(used..)
            .ok_or_else(|| Error::damaged("RLE command tail invalid"))?;
        let total = decoded
            .len()
            .checked_add(tail.len())
            .ok_or_else(|| Error::damaged("RLE command buffer size overflow"))?;
        if total > MAX_SCRATCH {
            return Err(Error::Refused("RLE command scratch exceeds 512 KiB".to_owned()));
        }
        commands = decoded;
        commands.extend_from_slice(tail);
        front = commands.as_slice();
        back = front.len();
    } else {
        commands = Vec::new();
        front = source
            .get(1..)
            .ok_or_else(|| Error::damaged("RLE source missing commands"))?;
        back = front.len();
    }
    let mut front_at = 0_usize;
    let mut dst = 0_usize;
    let mut rle_byte = 0_u8;
    while front_at < back {
        let cmd = *front
            .get(back.saturating_sub(1))
            .ok_or_else(|| Error::damaged("RLE command end underflow"))?;
        if u32::from(cmd).saturating_sub(1) >= 0x2f {
            back = back.saturating_sub(1);
            let copy = usize::from((!cmd) & 0x0f);
            let run = usize::from(cmd >> 4);
            rle_copy_run(front, &mut front_at, back, output, &mut dst, copy, rle_byte, run)?;
        } else if cmd >= 0x10 {
            if back < 2 {
                return Err(Error::damaged("truncated two-byte RLE command"));
            }
            let pair = front
                .get(back.saturating_sub(2)..back)
                .ok_or_else(|| Error::damaged("RLE two-byte command outside buffer"))?;
            let data = read_u16_le(pair)?.saturating_sub(4096);
            back = back.saturating_sub(2);
            let copy = usize::from(data & 0x3f);
            let run = usize::from(data >> 6);
            rle_copy_run(front, &mut front_at, back, output, &mut dst, copy, rle_byte, run)?;
        } else if cmd == 1 {
            rle_byte = *front
                .get(front_at)
                .ok_or_else(|| Error::damaged("RLE literal byte missing"))?;
            front_at = front_at.saturating_add(1);
            back = back.saturating_sub(1);
        } else if cmd >= 9 {
            if back < 2 {
                return Err(Error::damaged("truncated long RLE command"));
            }
            let value = read_u16_le(
                front
                    .get(back.saturating_sub(2)..back)
                    .ok_or_else(|| Error::damaged("RLE long command outside buffer"))?,
            )?;
            let units = value
                .checked_sub(0x08ff)
                .ok_or_else(|| Error::damaged("invalid long RLE command"))?;
            let run = usize::from(units)
                .checked_mul(128)
                .ok_or_else(|| Error::damaged("RLE run length overflow"))?;
            back = back.saturating_sub(2);
            fill_run(output, &mut dst, rle_byte, run)?;
        } else {
            if back < 2 {
                return Err(Error::damaged("truncated long-copy RLE command"));
            }
            let value = read_u16_le(
                front
                    .get(back.saturating_sub(2)..back)
                    .ok_or_else(|| Error::damaged("RLE long-copy command outside buffer"))?,
            )?;
            let units = value
                .checked_sub(511)
                .ok_or_else(|| Error::damaged("invalid long-copy RLE command"))?;
            let copy = usize::from(units)
                .checked_mul(64)
                .ok_or_else(|| Error::damaged("RLE copy length overflow"))?;
            back = back.saturating_sub(2);
            copy_literals(front, &mut front_at, back, output, &mut dst, copy)?;
        }
    }
    if front_at != back || dst != output.len() {
        return Err(Error::damaged("RLE command/data streams did not end together"));
    }
    drop(commands);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn rle_copy_run(
    input: &[u8],
    front: &mut usize,
    back: usize,
    output: &mut [u8],
    dst: &mut usize,
    copy: usize,
    value: u8,
    run: usize,
) -> Result<()> {
    copy_literals(input, front, back, output, dst, copy)?;
    fill_run(output, dst, value, run)
}
fn copy_literals(
    input: &[u8],
    front: &mut usize,
    back: usize,
    output: &mut [u8],
    dst: &mut usize,
    count: usize,
) -> Result<()> {
    let input_end = front
        .checked_add(count)
        .ok_or_else(|| Error::damaged("RLE literal end overflow"))?;
    let output_end = dst
        .checked_add(count)
        .ok_or_else(|| Error::damaged("RLE output end overflow"))?;
    if input_end > back {
        return Err(Error::damaged("RLE literal stream crosses command stream"));
    }
    let src = input
        .get(*front..input_end)
        .ok_or_else(|| Error::damaged("RLE literal range invalid"))?;
    output
        .get_mut(*dst..output_end)
        .ok_or_else(|| Error::damaged("RLE output range invalid"))?
        .copy_from_slice(src);
    *front = input_end;
    *dst = output_end;
    Ok(())
}
fn fill_run(output: &mut [u8], dst: &mut usize, value: u8, count: usize) -> Result<()> {
    let end = dst
        .checked_add(count)
        .ok_or_else(|| Error::damaged("RLE run end overflow"))?;
    output
        .get_mut(*dst..end)
        .ok_or_else(|| Error::damaged("RLE run exceeds destination"))?
        .fill(value);
    *dst = end;
    Ok(())
}

fn decode_recursive(source: &[u8], output: &mut [u8], depth: usize) -> Result<()> {
    if depth > MAX_RECURSION {
        return Err(Error::damaged("recursive entropy depth limit exceeded"));
    }
    if source.len() < 2 {
        return Err(Error::damaged("short recursive entropy payload"));
    }
    let first = source.first().copied().unwrap_or_default();
    let count = usize::from(first & 0x7f);
    if count < 2 {
        return Err(Error::damaged("recursive entropy requires at least two arrays"));
    }
    if first & 0x80 == 0 {
        let mut src = 1_usize;
        let mut dst = 0_usize;
        for _ in 0..count {
            let remaining = output.len().saturating_sub(dst);
            let header = entropy_header(
                source
                    .get(src..)
                    .ok_or_else(|| Error::damaged("recursive entropy source invalid"))?,
                remaining,
            )?;
            let end = dst
                .checked_add(header.output_size)
                .ok_or_else(|| Error::damaged("recursive entropy output overflow"))?;
            let used = decode_entropy_from(
                source
                    .get(src..)
                    .ok_or_else(|| Error::damaged("recursive source invalid"))?,
                output
                    .get_mut(dst..end)
                    .ok_or_else(|| Error::damaged("recursive output invalid"))?,
                depth,
            )?;
            src = src
                .checked_add(used)
                .ok_or_else(|| Error::damaged("recursive source position overflow"))?;
            dst = end;
        }
        if dst != output.len() || src != source.len() {
            return Err(Error::damaged(
                "recursive entropy streams did not consume exact buffers",
            ));
        }
    } else {
        let (arrays, used, total) = decode_multi_array(source, 1, output.len(), depth)?;
        if used != source.len() || total != output.len() {
            return Err(Error::damaged("multi-array recursive entropy size mismatch"));
        }
        let first_array = arrays
            .first()
            .ok_or_else(|| Error::damaged("multi-array returned no arrays"))?;
        output.copy_from_slice(first_array);
    }
    Ok(())
}

fn decode_multi_array(
    source: &[u8],
    array_count: usize,
    capacity: usize,
    depth: usize,
) -> Result<(Vec<Vec<u8>>, usize, usize)> {
    if source.len() < 4 {
        return Err(Error::damaged("short multi-array header"));
    }
    let marker = source.first().copied().unwrap_or_default();
    if marker & 0x80 == 0 {
        return Err(Error::damaged("multi-array marker missing high bit"));
    }
    let num_arrays = usize::from(marker & 0x3f);
    let mut src = 1_usize;
    if num_arrays == 0 {
        let mut arrays = Vec::with_capacity(array_count);
        let mut total = 0_usize;
        for _ in 0..array_count {
            let (data, used) = decode_entropy_owned(
                source
                    .get(src..)
                    .ok_or_else(|| Error::damaged("multi-array source invalid"))?,
                capacity.saturating_sub(total),
                depth,
            )?;
            src = src
                .checked_add(used)
                .ok_or_else(|| Error::damaged("multi-array source overflow"))?;
            total = total
                .checked_add(data.len())
                .ok_or_else(|| Error::damaged("multi-array total overflow"))?;
            arrays.push(data);
        }
        return Ok((arrays, src, total));
    }
    if num_arrays > MAX_ENTROPY_ARRAYS {
        return Err(Error::damaged("too many entropy arrays"));
    }
    let mut entropy = Vec::with_capacity(num_arrays);
    let mut total = 0_usize;
    for _ in 0..num_arrays {
        let (data, used) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("multi-array entropy source invalid"))?,
            capacity.saturating_sub(total),
            depth,
        )?;
        src = src
            .checked_add(used)
            .ok_or_else(|| Error::damaged("multi-array entropy source overflow"))?;
        total = total
            .checked_add(data.len())
            .ok_or_else(|| Error::damaged("multi-array total size overflow"))?;
        entropy.push((data, 0_usize));
    }
    let q_end = src
        .checked_add(2)
        .ok_or_else(|| Error::damaged("multi-array Q range overflow"))?;
    let q = read_u16_le(
        source
            .get(src..q_end)
            .ok_or_else(|| Error::damaged("short multi-array Q"))?,
    )?;
    src = q_end;
    let indexes_header = entropy_header(
        source
            .get(src..)
            .ok_or_else(|| Error::damaged("multi-array indexes header missing"))?,
        total,
    )?;
    let num_indexes = indexes_header.output_size;
    if num_indexes < array_count.saturating_add(1) {
        return Err(Error::damaged("multi-array index stream too short"));
    }
    let num_lens_default = num_indexes.saturating_sub(array_count);
    let mut interval_indexes;
    let interval_lenlog2;
    let num_lens;
    if q & 0x8000 != 0 {
        let (packed, used) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("multi-array packed indexes source invalid"))?,
            num_indexes,
            depth,
        )?;
        if packed.len() != num_indexes {
            return Err(Error::damaged("multi-array packed index count mismatch"));
        }
        src = src
            .checked_add(used)
            .ok_or_else(|| Error::damaged("multi-array source overflow"))?;
        interval_indexes = Vec::with_capacity(num_indexes);
        let mut lengths = Vec::with_capacity(num_indexes);
        for byte in packed {
            lengths.push(byte >> 4);
            interval_indexes.push(byte & 0x0f);
        }
        interval_lenlog2 = lengths;
        num_lens = num_indexes;
    } else {
        let (indexes, used) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("multi-array indexes source invalid"))?,
            num_indexes,
            depth,
        )?;
        if indexes.len() != num_indexes {
            return Err(Error::damaged("multi-array index count mismatch"));
        }
        src = src
            .checked_add(used)
            .ok_or_else(|| Error::damaged("multi-array source overflow"))?;
        let (lengths, used2) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("multi-array length-log source invalid"))?,
            num_lens_default,
            depth,
        )?;
        if lengths.len() != num_lens_default || lengths.iter().any(|v| *v > 16) {
            return Err(Error::damaged("invalid multi-array interval length logs"));
        }
        src = src
            .checked_add(used2)
            .ok_or_else(|| Error::damaged("multi-array source overflow"))?;
        interval_indexes = indexes;
        interval_lenlog2 = lengths;
        num_lens = num_lens_default;
    }
    let var_len = usize::from(q & 0x3fff);
    let var_end = src
        .checked_add(var_len)
        .ok_or_else(|| Error::damaged("multi-array varbits range overflow"))?;
    let varbits = source
        .get(src..var_end)
        .ok_or_else(|| Error::damaged("truncated multi-array varbits"))?;
    let mut forward = MsbForward::new(varbits);
    let mut backward = MsbBackward::new(varbits);
    let mut intervals = Vec::with_capacity(num_lens);
    for index in 0..num_lens {
        let bits = *interval_lenlog2
            .get(index)
            .ok_or_else(|| Error::damaged("multi-array length-log index outside stream"))?;
        let value = if index % 2 == 0 {
            forward.read(bits)?
        } else {
            backward.read(bits)?
        };
        intervals.push(value);
    }
    if forward.consumed_ceil().saturating_add(backward.consumed_ceil()) > varbits.len() {
        return Err(Error::damaged("multi-array varbits streams crossed"));
    }
    if interval_indexes.last().copied().unwrap_or(1) != 0 {
        return Err(Error::damaged("multi-array index stream lacks final separator"));
    }
    let mut outputs = Vec::with_capacity(array_count);
    let mut index_at = 0_usize;
    let mut len_at = 0_usize;
    let increment = usize::from(q & 0x8000 != 0);
    for _ in 0..array_count {
        let mut array = Vec::new();
        loop {
            let source_index = *interval_indexes
                .get(index_at)
                .ok_or_else(|| Error::damaged("multi-array index stream exhausted"))?;
            index_at = index_at.saturating_add(1);
            if source_index == 0 {
                break;
            }
            let entropy_index = usize::from(source_index).saturating_sub(1);
            let length = usize::try_from(
                *intervals
                    .get(len_at)
                    .ok_or_else(|| Error::damaged("multi-array interval length stream exhausted"))?,
            )
            .map_err(|_| Error::damaged("multi-array interval length conversion failed"))?;
            len_at = len_at.saturating_add(1);
            let (data, position) = entropy
                .get_mut(entropy_index)
                .ok_or_else(|| Error::damaged("multi-array source index outside entropy arrays"))?;
            let end = position
                .checked_add(length)
                .ok_or_else(|| Error::damaged("multi-array entropy slice overflow"))?;
            let slice = data
                .get(*position..end)
                .ok_or_else(|| Error::damaged("multi-array interval exceeds entropy source"))?;
            array.extend_from_slice(slice);
            *position = end;
            if array.len() > capacity {
                return Err(Error::damaged("multi-array output exceeds capacity"));
            }
        }
        len_at = len_at.saturating_add(increment);
        outputs.push(array);
    }
    if index_at != interval_indexes.len() || len_at != num_lens {
        return Err(Error::damaged("multi-array index/length streams not fully consumed"));
    }
    if entropy.iter().any(|(data, pos)| *pos != data.len()) {
        return Err(Error::damaged("multi-array entropy source not fully consumed"));
    }
    let out_total = outputs.iter().try_fold(0_usize, |acc, a| {
        acc.checked_add(a.len())
            .ok_or_else(|| Error::damaged("multi-array output total overflow"))
    })?;
    Ok((outputs, var_end, out_total))
}

#[derive(Clone, Debug)]
struct MsbForward<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}
impl<'a> MsbForward<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, byte: 0, bit: 0 }
    }
    fn read(&mut self, count: u8) -> Result<u32> {
        let mut v = 0_u32;
        for _ in 0..count {
            let b = *self
                .data
                .get(self.byte)
                .ok_or_else(|| Error::damaged("forward MSB bitstream exhausted"))?;
            let one = (b >> (7_u8.saturating_sub(self.bit))) & 1;
            v = v.checked_shl(1).unwrap_or_default() | u32::from(one);
            self.bit = self.bit.saturating_add(1);
            if self.bit == 8 {
                self.bit = 0;
                self.byte = self
                    .byte
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("forward MSB bit position overflow"))?;
            }
        }
        Ok(v)
    }
    fn consumed_ceil(&self) -> usize {
        self.byte.saturating_add(usize::from(self.bit != 0))
    }
}
#[derive(Clone, Debug)]
struct MsbBackward<'a> {
    data: &'a [u8],
    from_end: usize,
    bit: u8,
}
impl<'a> MsbBackward<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            from_end: 0,
            bit: 0,
        }
    }
    fn read(&mut self, count: u8) -> Result<u32> {
        let mut v = 0_u32;
        for _ in 0..count {
            let idx = self
                .data
                .len()
                .checked_sub(self.from_end.saturating_add(1))
                .ok_or_else(|| Error::damaged("backward MSB bitstream exhausted"))?;
            let b = *self
                .data
                .get(idx)
                .ok_or_else(|| Error::damaged("backward MSB byte outside stream"))?;
            let one = (b >> (7_u8.saturating_sub(self.bit))) & 1;
            v = v.checked_shl(1).unwrap_or_default() | u32::from(one);
            self.bit = self.bit.saturating_add(1);
            if self.bit == 8 {
                self.bit = 0;
                self.from_end = self
                    .from_end
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("backward MSB bit position overflow"))?;
            }
        }
        Ok(v)
    }
    fn consumed_ceil(&self) -> usize {
        self.from_end.saturating_add(usize::from(self.bit != 0))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct TansWeight {
    symbol: u8,
    weight: u16,
}
#[derive(Clone, Copy, Debug, Default)]
struct TansEntry {
    x: u16,
    bits: u8,
    symbol: u8,
    w: u16,
}

fn decode_tans(source: &[u8], output: &mut [u8]) -> Result<()> {
    if source.len() < 8 || output.len() < 5 {
        return Err(Error::damaged("tANS block too short"));
    }
    let mut bits = HeaderBits::new(source)?;
    if bits.read_bit_no_refill()? != 0 {
        return Err(Error::damaged("reserved tANS bit is set"));
    }
    let l_bits = u8::try_from(bits.read_bits_no_refill(2)?)
        .map_err(|_| Error::damaged("tANS L width conversion failed"))?
        .saturating_add(8);
    let weights = decode_tans_table(&mut bits, l_bits)?;
    let start = bits.data_pointer()?;
    if start >= source.len() {
        return Err(Error::damaged("tANS table consumes complete payload"));
    }
    let lut = init_tans_lut(&weights, l_bits)?;
    let encoded = source
        .get(start..)
        .ok_or_else(|| Error::damaged("tANS encoded region invalid"))?;
    let mut forward = LsbForward::new(encoded);
    let mut backward = LsbBackward::new(encoded);
    let mask = (1_u32.checked_shl(u32::from(l_bits)).unwrap_or_default()).saturating_sub(1);
    let mut states = [0_u32; 5];
    *states
        .get_mut(0)
        .ok_or_else(|| Error::damaged("tANS state slot missing"))? = forward.read(l_bits, false)? & mask;
    *states
        .get_mut(1)
        .ok_or_else(|| Error::damaged("tANS state slot missing"))? = backward.read(l_bits, false)? & mask;
    *states
        .get_mut(2)
        .ok_or_else(|| Error::damaged("tANS state slot missing"))? = forward.read(l_bits, false)? & mask;
    *states
        .get_mut(3)
        .ok_or_else(|| Error::damaged("tANS state slot missing"))? = backward.read(l_bits, false)? & mask;
    *states
        .get_mut(4)
        .ok_or_else(|| Error::damaged("tANS state slot missing"))? = forward.read(l_bits, false)? & mask;
    let body_len = output.len().saturating_sub(5);
    let mut dst = 0_usize;
    let pattern = [
        (0_usize, true),
        (1, true),
        (2, true),
        (3, true),
        (4, true),
        (0, false),
        (1, false),
        (2, false),
        (3, false),
        (4, false),
    ];
    let mut round = 0_usize;
    while dst < body_len {
        let (state_index, is_forward) = *pattern
            .get(round.checked_rem(pattern.len()).unwrap_or_default())
            .ok_or_else(|| Error::damaged("tANS schedule index invalid"))?;
        let state = usize::try_from(
            *states
                .get(state_index)
                .ok_or_else(|| Error::damaged("tANS state index invalid"))?,
        )
        .map_err(|_| Error::damaged("tANS state conversion failed"))?;
        let entry = *lut.get(state).ok_or_else(|| Error::damaged("tANS state outside LUT"))?;
        if let Some(slot) = output.get_mut(dst) {
            *slot = entry.symbol;
        }
        dst = dst.saturating_add(1);
        let extra = if is_forward {
            forward.read(entry.bits, false)?
        } else {
            backward.read(entry.bits, false)?
        };
        let next = u32::from(entry.w)
            .checked_add(extra)
            .ok_or_else(|| Error::damaged("tANS next state overflow"))?;
        *states
            .get_mut(state_index)
            .ok_or_else(|| Error::damaged("tANS state slot missing"))? = next;
        round = round.saturating_add(1);
    }
    if forward.consumed_ceil().saturating_add(backward.consumed_ceil()) > encoded.len() {
        return Err(Error::damaged("tANS forward/backward streams crossed"));
    }
    for (i, state) in states.iter().copied().enumerate() {
        if state > 255 {
            return Err(Error::damaged("tANS terminal state exceeds byte"));
        }
        let out_index = body_len
            .checked_add(i)
            .ok_or_else(|| Error::damaged("tANS terminal output position overflow"))?;
        if let Some(slot) = output.get_mut(out_index) {
            *slot = u8::try_from(state).map_err(|_| Error::damaged("tANS terminal state conversion failed"))?;
        }
    }
    Ok(())
}

fn decode_tans_table(bits: &mut HeaderBits<'_>, l_bits: u8) -> Result<Vec<TansWeight>> {
    bits.refill()?;
    let l = 1_usize.checked_shl(u32::from(l_bits)).unwrap_or_default();
    let mut result = Vec::new();
    if bits.read_bit_no_refill()? != 0 {
        let q = u8::try_from(bits.read_bits_no_refill(3)?).map_err(|_| Error::damaged("tANS Q conversion failed"))?;
        let num = usize::try_from(bits.read_bits_no_refill(8)?)
            .map_err(|_| Error::damaged("tANS symbol count conversion failed"))?
            .saturating_add(1);
        if num < 2 {
            return Err(Error::damaged("tANS table needs at least two symbols"));
        }
        let fluff = read_fluff(bits, num)?;
        let total = num
            .checked_add(fluff)
            .ok_or_else(|| Error::damaged("tANS Rice count overflow"))?;
        let mut rice_reader = bits.rice_reader()?;
        let rice = decode_rice_lengths(&mut rice_reader, total)?;
        bits.reset_from_rice(&rice_reader)?;
        let ranges = huff_convert_ranges(
            bits,
            num,
            fluff,
            rice.get(num..)
                .ok_or_else(|| Error::damaged("tANS range Rice data missing"))?,
        )?;
        bits.refill()?;
        let mut rice_at = 0_usize;
        let mut average = 6_i32;
        let mut sum = 0_usize;
        for range in ranges {
            let mut symbol = usize::from(range.symbol);
            for _ in 0..usize::from(range.count) {
                bits.refill()?;
                let extra = q.saturating_add(
                    *rice
                        .get(rice_at)
                        .ok_or_else(|| Error::damaged("tANS symbol Rice data missing"))?,
                );
                rice_at = rice_at.saturating_add(1);
                if extra > 15 {
                    return Err(Error::damaged("tANS extra width exceeds 15"));
                }
                let base = 1_i32
                    .checked_shl(u32::from(extra))
                    .unwrap_or_default()
                    .saturating_sub(1_i32.checked_shl(u32::from(q)).unwrap_or_default());
                let mut value = i32::try_from(bits.read_bits_zero(u32::from(extra))?)
                    .map_err(|_| Error::damaged("tANS weight bits conversion failed"))?
                    .saturating_add(base);
                let avg4 = average >> 2;
                let mut limit = avg4.saturating_mul(2);
                if value <= limit {
                    value = avg4.saturating_add((value & 1).wrapping_neg() ^ (value >> 1));
                }
                if limit > value {
                    limit = value;
                }
                value = value.saturating_add(1);
                average = average.saturating_add(limit.saturating_sub(avg4));
                if value <= 0 {
                    return Err(Error::damaged("tANS non-positive weight"));
                }
                let weight = u16::try_from(value).map_err(|_| Error::damaged("tANS weight exceeds u16"))?;
                result.push(TansWeight {
                    symbol: u8::try_from(symbol).map_err(|_| Error::damaged("tANS symbol exceeds byte"))?,
                    weight,
                });
                sum = sum
                    .checked_add(usize::from(weight))
                    .ok_or_else(|| Error::damaged("tANS weight sum overflow"))?;
                symbol = symbol.saturating_add(1);
            }
        }
        if sum != l {
            return Err(Error::damaged("tANS weights do not sum to table size"));
        }
    } else {
        let count = usize::try_from(bits.read_bits_no_refill(3)?)
            .map_err(|_| Error::damaged("tANS sparse count conversion failed"))?
            .saturating_add(1);
        let bits_per_sym = u8::BITS.saturating_sub(l_bits.leading_zeros()).max(1);
        let max_delta = u8::try_from(bits.read_bits_no_refill(bits_per_sym)?)
            .map_err(|_| Error::damaged("tANS max delta conversion failed"))?;
        if max_delta == 0 || max_delta > l_bits {
            return Err(Error::damaged("invalid tANS max delta width"));
        }
        let mut seen = [false; 256];
        let mut weight = 0_u16;
        let mut total = 0_usize;
        for _ in 0..count {
            bits.refill()?;
            let symbol = u8::try_from(bits.read_bits_no_refill(8)?)
                .map_err(|_| Error::damaged("tANS symbol conversion failed"))?;
            if *seen.get(usize::from(symbol)).unwrap_or(&false) {
                return Err(Error::damaged("duplicate tANS symbol"));
            }
            let delta = u16::try_from(bits.read_bits_no_refill(u32::from(max_delta))?)
                .map_err(|_| Error::damaged("tANS weight delta conversion failed"))?;
            weight = weight
                .checked_add(delta)
                .ok_or_else(|| Error::damaged("tANS weight overflow"))?;
            if weight == 0 {
                return Err(Error::damaged("zero tANS weight"));
            }
            if let Some(slot) = seen.get_mut(usize::from(symbol)) {
                *slot = true;
            }
            result.push(TansWeight { symbol, weight });
            total = total
                .checked_add(usize::from(weight))
                .ok_or_else(|| Error::damaged("tANS total weight overflow"))?;
        }
        bits.refill()?;
        let symbol = u8::try_from(bits.read_bits_no_refill(8)?)
            .map_err(|_| Error::damaged("tANS final symbol conversion failed"))?;
        if *seen.get(usize::from(symbol)).unwrap_or(&false) {
            return Err(Error::damaged("duplicate final tANS symbol"));
        }
        let remaining = l.saturating_sub(total);
        if remaining < usize::from(weight) || remaining <= 1 {
            return Err(Error::damaged("invalid final tANS weight"));
        }
        result.push(TansWeight {
            symbol,
            weight: u16::try_from(remaining).map_err(|_| Error::damaged("final tANS weight exceeds u16"))?,
        });
        result.sort_by_key(|item| (item.weight > 1, item.symbol, item.weight));
    }
    Ok(result)
}

fn init_tans_lut(weights: &[TansWeight], l_bits: u8) -> Result<Vec<TansEntry>> {
    let l = 1_usize.checked_shl(u32::from(l_bits)).unwrap_or_default();
    let mut lut = vec![TansEntry::default(); l];
    let mut singles = Vec::new();
    let mut multis = Vec::new();
    for item in weights {
        if item.weight == 1 {
            singles.push(*item);
        } else {
            multis.push(*item);
        }
    }
    let single_count = singles.len();
    if single_count > l {
        return Err(Error::damaged("too many tANS singletons"));
    }
    let slots = l.saturating_sub(single_count);
    let base = slots / 4;
    let extra = slots % 4;
    let mut pointers = [0_usize; 4];
    let mut cursor = 0_usize;
    for i in 0..4 {
        *pointers
            .get_mut(i)
            .ok_or_else(|| Error::damaged("tANS pointer slot missing"))? = cursor;
        cursor = cursor.saturating_add(base).saturating_add(usize::from(i < extra));
    }
    for (i, item) in singles.iter().enumerate() {
        let index = slots
            .checked_add(i)
            .ok_or_else(|| Error::damaged("tANS singleton index overflow"))?;
        *lut.get_mut(index)
            .ok_or_else(|| Error::damaged("tANS singleton index outside LUT"))? = TansEntry {
            x: u16::try_from(l.saturating_sub(1)).map_err(|_| Error::damaged("tANS x exceeds u16"))?,
            bits: l_bits,
            symbol: item.symbol,
            w: 0,
        };
    }
    let mut weights_sum = 0_usize;
    for item in multis {
        let weight = usize::from(item.weight);
        if weight == 0 {
            return Err(Error::damaged("zero tANS weight"));
        }
        if weight > 4 {
            let sym_bits = usize::BITS.saturating_sub(weight.leading_zeros()).saturating_sub(1);
            let mut z = i32::from(l_bits).saturating_sub(
                i32::try_from(sym_bits).map_err(|_| Error::damaged("tANS sym bits conversion failed"))?,
            );
            let mut what = 1_usize
                .checked_shl(u32::try_from(z.max(0)).unwrap_or_default())
                .unwrap_or_default();
            let mut x = (1_usize.checked_shl(sym_bits.saturating_add(1)).unwrap_or_default()).saturating_sub(weight);
            let mut w = (l.saturating_sub(1))
                & (weight
                    .checked_shl(u32::try_from(z.max(0)).unwrap_or_default())
                    .unwrap_or_default());
            for lane in 0..4 {
                let y = (weight.saturating_add(weights_sum.wrapping_sub(lane).wrapping_sub(1) & 3)) / 4;
                let first = x.min(y);
                for _ in 0..first {
                    let p = *pointers
                        .get(lane)
                        .ok_or_else(|| Error::damaged("tANS lane pointer missing"))?;
                    *lut.get_mut(p)
                        .ok_or_else(|| Error::damaged("tANS lane write outside LUT"))? = TansEntry {
                        x: u16::try_from(what.saturating_sub(1)).map_err(|_| Error::damaged("tANS x exceeds u16"))?,
                        bits: u8::try_from(z.max(0)).map_err(|_| Error::damaged("tANS bits conversion failed"))?,
                        symbol: item.symbol,
                        w: u16::try_from(w).map_err(|_| Error::damaged("tANS w exceeds u16"))?,
                    };
                    *pointers
                        .get_mut(lane)
                        .ok_or_else(|| Error::damaged("tANS lane pointer missing"))? = p.saturating_add(1);
                    w = w.saturating_add(what);
                }
                if x >= y {
                    x = x.saturating_sub(y);
                } else {
                    let rest = y.saturating_sub(x);
                    z = z.saturating_sub(1);
                    what >>= 1;
                    w = 0;
                    for _ in 0..rest {
                        let p = *pointers
                            .get(lane)
                            .ok_or_else(|| Error::damaged("tANS lane pointer missing"))?;
                        *lut.get_mut(p)
                            .ok_or_else(|| Error::damaged("tANS lane write outside LUT"))? = TansEntry {
                            x: u16::try_from(what.saturating_sub(1))
                                .map_err(|_| Error::damaged("tANS x exceeds u16"))?,
                            bits: u8::try_from(z.max(0)).map_err(|_| Error::damaged("tANS bits conversion failed"))?,
                            symbol: item.symbol,
                            w: u16::try_from(w).map_err(|_| Error::damaged("tANS w exceeds u16"))?,
                        };
                        *pointers
                            .get_mut(lane)
                            .ok_or_else(|| Error::damaged("tANS lane pointer missing"))? = p.saturating_add(1);
                        w = w.saturating_add(what);
                    }
                    x = weight;
                }
            }
        } else {
            let mut remaining = weight;
            let mut logical_weight = weight;
            let mut lane_bits = ((1_usize
                .checked_shl(
                    u32::try_from(weight).map_err(|_| Error::damaged("tANS small-weight shift conversion failed"))?,
                )
                .unwrap_or_default())
            .saturating_sub(1))
            .checked_shl(u32::try_from(weights_sum & 3).unwrap_or_default())
            .unwrap_or_default();
            while remaining != 0 {
                let lane = usize::try_from(lane_bits.trailing_zeros())
                    .map_err(|_| Error::damaged("tANS lane index conversion failed"))?
                    & 3;
                lane_bits &= lane_bits.saturating_sub(1);
                let p = *pointers
                    .get(lane)
                    .ok_or_else(|| Error::damaged("tANS lane pointer missing"))?;
                let weight_bits = usize::BITS
                    .saturating_sub(logical_weight.leading_zeros())
                    .saturating_sub(1);
                let bits = i32::from(l_bits).saturating_sub(
                    i32::try_from(weight_bits)
                        .map_err(|_| Error::damaged("tANS small weight bit conversion failed"))?,
                );
                let step = 1_usize
                    .checked_shl(u32::try_from(bits.max(0)).unwrap_or_default())
                    .unwrap_or_default();
                *lut.get_mut(p)
                    .ok_or_else(|| Error::damaged("tANS small-weight write outside LUT"))? = TansEntry {
                    x: u16::try_from(step.saturating_sub(1)).map_err(|_| Error::damaged("tANS x exceeds u16"))?,
                    bits: u8::try_from(bits.max(0)).map_err(|_| Error::damaged("tANS bits conversion failed"))?,
                    symbol: item.symbol,
                    w: u16::try_from(
                        (l.saturating_sub(1))
                            & (logical_weight
                                .checked_shl(u32::try_from(bits.max(0)).unwrap_or_default())
                                .unwrap_or_default()),
                    )
                    .map_err(|_| Error::damaged("tANS w exceeds u16"))?,
                };
                *pointers
                    .get_mut(lane)
                    .ok_or_else(|| Error::damaged("tANS lane pointer missing"))? = p.saturating_add(1);
                logical_weight = logical_weight.saturating_add(1);
                remaining = remaining.saturating_sub(1);
            }
        }
        weights_sum = weights_sum.saturating_add(weight);
    }
    if lut
        .iter()
        .any(|entry| entry.bits == 0 && entry.x == 0 && entry.w == 0 && entry.symbol == 0)
        && weights.iter().all(|item| item.symbol != 0)
    {
        return Err(Error::damaged("tANS LUT contains uninitialised entries"));
    }
    Ok(lut)
}

#[derive(Clone, Debug)]
struct OffsetBits<'a> {
    data: &'a [u8],
    pos: i64,
    bits: u32,
    bitpos: i32,
    backward: bool,
}
impl<'a> OffsetBits<'a> {
    fn forward(data: &'a [u8]) -> Result<Self> {
        let mut s = Self {
            data,
            pos: 0,
            bits: 0,
            bitpos: 24,
            backward: false,
        };
        s.refill()?;
        Ok(s)
    }
    fn backward(data: &'a [u8]) -> Result<Self> {
        let mut s = Self {
            data,
            pos: i64::try_from(data.len()).map_err(|_| Error::damaged("offset source length exceeds i64"))?,
            bits: 0,
            bitpos: 24,
            backward: true,
        };
        s.refill()?;
        Ok(s)
    }
    fn refill(&mut self) -> Result<()> {
        if self.bitpos > 24 {
            return Err(Error::damaged("offset reader bit position exceeds refill bound"));
        }
        let data_len =
            i64::try_from(self.data.len()).map_err(|_| Error::damaged("offset source length exceeds i64"))?;
        while self.bitpos > 0 {
            let byte = if self.backward {
                self.pos = self
                    .pos
                    .checked_sub(1)
                    .ok_or_else(|| Error::damaged("backward offset position underflow"))?;
                if self.pos >= 0 && self.pos < data_len {
                    let index = usize::try_from(self.pos)
                        .map_err(|_| Error::damaged("backward offset index conversion failed"))?;
                    self.data.get(index).copied().unwrap_or_default()
                } else {
                    0
                }
            } else {
                let byte = if self.pos >= 0 && self.pos < data_len {
                    let index = usize::try_from(self.pos)
                        .map_err(|_| Error::damaged("forward offset index conversion failed"))?;
                    self.data.get(index).copied().unwrap_or_default()
                } else {
                    0
                };
                self.pos = self
                    .pos
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("forward offset position overflow"))?;
                byte
            };
            let shift = u32::try_from(self.bitpos).map_err(|_| Error::damaged("offset bit shift conversion failed"))?;
            self.bits |= u32::from(byte).checked_shl(shift).unwrap_or_default();
            self.bitpos = self
                .bitpos
                .checked_sub(8)
                .ok_or_else(|| Error::damaged("offset bit position underflow"))?;
        }
        Ok(())
    }
    fn read_zero(&mut self, n: u32) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        if n > 24 {
            return Err(Error::damaged("offset bit read exceeds 24"));
        }
        let shift = 32_u32
            .checked_sub(n)
            .ok_or_else(|| Error::damaged("offset bit shift underflow"))?;
        let value = self.bits >> shift;
        self.bits = self.bits.checked_shl(n).unwrap_or_default();
        self.bitpos = self
            .bitpos
            .checked_add(i32::try_from(n).map_err(|_| Error::damaged("offset bit count conversion failed"))?)
            .ok_or_else(|| Error::damaged("offset bit position overflow"))?;
        Ok(value)
    }
    fn read_more(&mut self, n: u32) -> Result<u32> {
        let value = if n <= 24 {
            self.read_zero(n)?
        } else {
            let high = self
                .read_zero(24)?
                .checked_shl(n.saturating_sub(24))
                .unwrap_or_default();
            self.refill()?;
            high | self.read_zero(n.saturating_sub(24))?
        };
        self.refill()?;
        Ok(value)
    }
    fn read_distance(&mut self, v: u8) -> Result<u32> {
        let n = if v < 0xf0 {
            u32::from(v >> 4).saturating_add(4)
        } else {
            u32::from(v.saturating_sub(0xf0)).saturating_add(4)
        };
        if n >= 31 {
            return Err(Error::damaged("distance bit count too large"));
        }
        let w = (self.bits | 1).rotate_left(n);
        self.bitpos = self
            .bitpos
            .checked_add(i32::try_from(n).map_err(|_| Error::damaged("distance bit count conversion failed"))?)
            .ok_or_else(|| Error::damaged("distance bitpos overflow"))?;
        let mask = (2_u32.checked_shl(n).unwrap_or_default()).saturating_sub(1);
        self.bits = w & !mask;
        let mut result = if v < 0xf0 {
            (w & mask)
                .checked_shl(4)
                .unwrap_or_default()
                .saturating_add(u32::from(v & 0x0f))
                .wrapping_sub(248)
        } else {
            8_322_816_u32.saturating_add((w & mask).checked_shl(12).unwrap_or_default())
        };
        self.refill()?;
        if v >= 0xf0 {
            result = result.saturating_add(self.read_zero(12)?);
            self.refill()?;
        }
        Ok(result)
    }
    fn read_length(&mut self) -> Result<u32> {
        let n = self.bits.leading_zeros();
        if n > 12 {
            return Err(Error::damaged("length code leading-zero count exceeds 12"));
        }
        self.bitpos = self
            .bitpos
            .checked_add(i32::try_from(n).map_err(|_| Error::damaged("length zero count conversion failed"))?)
            .ok_or_else(|| Error::damaged("length bitpos overflow"))?;
        self.bits = self.bits.checked_shl(n).unwrap_or_default();
        self.refill()?;
        let total = n.saturating_add(7);
        let value = self
            .read_zero(total)?
            .checked_sub(64)
            .ok_or_else(|| Error::damaged("length code below bias"))?;
        self.refill()?;
        Ok(value)
    }
    fn effective_pos(&self) -> Result<i64> {
        let residual = i64::from((24_i32.saturating_sub(self.bitpos)).max(0) >> 3);
        if self.backward {
            self.pos
                .checked_add(residual)
                .ok_or_else(|| Error::damaged("backward effective offset overflow"))
        } else {
            self.pos
                .checked_sub(residual)
                .ok_or_else(|| Error::damaged("forward effective offset underflow"))
        }
    }
}

#[derive(Debug)]
struct LzTable {
    literals: Vec<u8>,
    commands: Vec<u8>,
    offsets: Vec<i32>,
    lengths: Vec<u32>,
}

fn read_lz_table(
    mode: u8,
    source: &[u8],
    output: &mut [u8],
    dst_start: usize,
    dst_end: usize,
    offset: usize,
) -> Result<LzTable> {
    if mode > 1 {
        return Err(Error::damaged("Kraken LZ mode exceeds 1"));
    }
    if source.len() < 13 {
        return Err(Error::damaged("Kraken LZ table payload too short"));
    }
    let mut src = 0_usize;
    if offset == 0 {
        let first = source
            .get(..8)
            .ok_or_else(|| Error::damaged("missing first 8 LZ bytes"))?;
        let end = dst_start
            .checked_add(8)
            .ok_or_else(|| Error::damaged("initial LZ output overflow"))?;
        output
            .get_mut(dst_start..end)
            .ok_or_else(|| Error::damaged("initial LZ output outside destination"))?
            .copy_from_slice(first);
        src = 8;
    }
    if source.get(src).copied().unwrap_or_default() & 0x80 != 0 {
        return Err(Error::Refused(
            "Kraken excess-byte LZ table mode is not used by supported saves".to_owned(),
        ));
    }
    let capacity = dst_end.saturating_sub(dst_start);
    let (literals, used1) = decode_entropy_owned(
        source
            .get(src..)
            .ok_or_else(|| Error::damaged("LZ literal entropy source invalid"))?,
        capacity,
        1,
    )?;
    src = src
        .checked_add(used1)
        .ok_or_else(|| Error::damaged("LZ literal source overflow"))?;
    let (commands, used2) = decode_entropy_owned(
        source
            .get(src..)
            .ok_or_else(|| Error::damaged("LZ command entropy source invalid"))?,
        capacity,
        1,
    )?;
    src = src
        .checked_add(used2)
        .ok_or_else(|| Error::damaged("LZ command source overflow"))?;
    if source.len().saturating_sub(src) < 3 {
        return Err(Error::damaged("short LZ offset section"));
    }
    let mut scale = 0_u8;
    let mut packed_extra: Option<Vec<u8>> = None;
    let packed_offsets;
    if source.get(src).copied().unwrap_or_default() & 0x80 != 0 {
        scale = source.get(src).copied().unwrap_or_default().saturating_sub(127);
        src = src.saturating_add(1);
        let (p, u) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("LZ packed offset source invalid"))?,
            commands.len(),
            1,
        )?;
        src = src
            .checked_add(u)
            .ok_or_else(|| Error::damaged("LZ packed offset source overflow"))?;
        if scale != 1 {
            let (extra, u2) = decode_entropy_owned(
                source
                    .get(src..)
                    .ok_or_else(|| Error::damaged("LZ packed offset-extra source invalid"))?,
                p.len(),
                1,
            )?;
            if extra.len() != p.len() {
                return Err(Error::damaged("LZ offset-extra count mismatch"));
            }
            src = src
                .checked_add(u2)
                .ok_or_else(|| Error::damaged("LZ offset-extra source overflow"))?;
            packed_extra = Some(extra);
        }
        packed_offsets = p;
    } else {
        let (p, u) = decode_entropy_owned(
            source
                .get(src..)
                .ok_or_else(|| Error::damaged("LZ packed offset source invalid"))?,
            commands.len(),
            1,
        )?;
        src = src
            .checked_add(u)
            .ok_or_else(|| Error::damaged("LZ packed offset source overflow"))?;
        packed_offsets = p;
    }
    let len_capacity = capacity / 4;
    let (packed_lengths, u3) = decode_entropy_owned(
        source
            .get(src..)
            .ok_or_else(|| Error::damaged("LZ packed length source invalid"))?,
        len_capacity,
        1,
    )?;
    src = src
        .checked_add(u3)
        .ok_or_else(|| Error::damaged("LZ packed length source overflow"))?;
    if packed_offsets.len().saturating_mul(4) > MAX_SCRATCH || packed_lengths.len().saturating_mul(4) > MAX_SCRATCH {
        return Err(Error::Refused(
            "Kraken LZ expanded table exceeds scratch limit".to_owned(),
        ));
    }
    let (offsets, lengths) = unpack_offsets(
        source
            .get(src..)
            .ok_or_else(|| Error::damaged("LZ offset bitstream invalid"))?,
        &packed_offsets,
        packed_extra.as_deref(),
        scale,
        &packed_lengths,
    )?;
    Ok(LzTable {
        literals,
        commands,
        offsets,
        lengths,
    })
}

fn unpack_offsets(
    bits_source: &[u8],
    packed_offsets: &[u8],
    extra: Option<&[u8]>,
    scale: u8,
    packed_lengths: &[u8],
) -> Result<(Vec<i32>, Vec<u32>)> {
    let mut forward = OffsetBits::forward(bits_source)?;
    let mut backward = OffsetBits::backward(bits_source)?;
    if backward.bits < 0x2000 {
        return Err(Error::damaged("LZ backward offset stream has invalid length prefix"));
    }
    let n = backward.bits.leading_zeros();
    if n > 31 {
        return Err(Error::damaged("invalid LZ length-count prefix"));
    }
    backward.bitpos = backward
        .bitpos
        .checked_add(i32::try_from(n).map_err(|_| Error::damaged("LZ length-count prefix conversion failed"))?)
        .ok_or_else(|| Error::damaged("LZ backward bitpos overflow"))?;
    backward.bits = backward.bits.checked_shl(n).unwrap_or_default();
    backward.refill()?;
    let width = n.saturating_add(1);
    let ext_count = backward
        .read_zero(width)?
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("invalid LZ extended-length count"))?;
    backward.refill()?;
    let ext_count =
        usize::try_from(ext_count).map_err(|_| Error::damaged("LZ extended-length count conversion failed"))?;
    if ext_count > MAX_LEN_EXTENSIONS {
        return Err(Error::damaged("too many extended LZ lengths"));
    }
    let mut offsets = Vec::with_capacity(packed_offsets.len());
    for (index, cmd) in packed_offsets.iter().copied().enumerate() {
        let reader = if index % 2 == 0 { &mut forward } else { &mut backward };
        let offset = if scale == 0 {
            let distance = reader.read_distance(cmd)?;
            i32::try_from(distance)
                .map_err(|_| Error::damaged("LZ distance exceeds i32"))?
                .checked_neg()
                .ok_or_else(|| Error::damaged("LZ distance negation overflow"))?
        } else {
            let width = u32::from(cmd >> 3);
            if width > 26 {
                return Err(Error::damaged("extended LZ offset width exceeds 26"));
            }
            let head = u32::from(8_u8.saturating_add(cmd & 7))
                .checked_shl(width)
                .unwrap_or_default();
            let distance = head | reader.read_more(width)?;
            let base = 8_i64.saturating_sub(i64::from(distance));
            let scaled = if scale == 1 {
                base
            } else {
                let low = i64::from(
                    extra
                        .and_then(|v| v.get(index))
                        .copied()
                        .ok_or_else(|| Error::damaged("missing LZ offset low-byte stream"))?,
                );
                base.saturating_mul(i64::from(scale)).saturating_sub(low)
            };
            i32::try_from(scaled).map_err(|_| Error::damaged("scaled LZ offset exceeds i32"))?
        };
        offsets.push(offset);
    }
    let mut extended = Vec::with_capacity(ext_count);
    for index in 0..ext_count {
        let value = if index % 2 == 0 {
            forward.read_length()?
        } else {
            backward.read_length()?
        };
        extended.push(value);
    }
    if forward.effective_pos()? != backward.effective_pos()? {
        return Err(Error::damaged("LZ forward/backward offset streams did not meet"));
    }
    let mut ext_at = 0_usize;
    let mut lengths = Vec::with_capacity(packed_lengths.len());
    for byte in packed_lengths {
        let value = if *byte == 255 {
            let extra_value = *extended
                .get(ext_at)
                .ok_or_else(|| Error::damaged("missing extended LZ length"))?;
            ext_at = ext_at.saturating_add(1);
            extra_value.saturating_add(255)
        } else {
            u32::from(*byte)
        };
        lengths.push(value.saturating_add(3));
    }
    if ext_at != extended.len() {
        return Err(Error::damaged("unused extended LZ lengths"));
    }
    Ok((offsets, lengths))
}

fn process_lz_runs(
    mode: u8,
    table: LzTable,
    output: &mut [u8],
    block_start: usize,
    block_end: usize,
    history_offset: usize,
) -> Result<()> {
    if mode > 1 {
        return Err(Error::damaged("invalid Kraken LZ mode"));
    }
    let mut dst = block_start;
    if history_offset == 0 {
        dst = dst
            .checked_add(8)
            .ok_or_else(|| Error::damaged("LZ initial output position overflow"))?;
    }
    let history_start = block_start.saturating_sub(history_offset);
    let mut literal_at = 0_usize;
    let mut offset_at = 0_usize;
    let mut length_at = 0_usize;
    let mut recent = [0_i32; 7];
    for index in 3..=5 {
        if let Some(slot) = recent.get_mut(index) {
            *slot = -8;
        }
    }
    let mut last_offset = -8_i32;
    for command in table.commands {
        let mut litlen = usize::from(command & 3);
        let offset_index = usize::from(command >> 6);
        let match_code = usize::from((command >> 2) & 0x0f);
        if litlen == 3 {
            litlen = usize::try_from(
                *table
                    .lengths
                    .get(length_at)
                    .ok_or_else(|| Error::damaged("missing long literal length"))?,
            )
            .map_err(|_| Error::damaged("literal length conversion failed"))?;
            length_at = length_at.saturating_add(1);
        }
        let next_new = *table.offsets.get(offset_at).unwrap_or(&0);
        if let Some(slot) = recent.get_mut(6) {
            *slot = next_new;
        }
        copy_lz_literals(
            mode,
            &table.literals,
            &mut literal_at,
            output,
            &mut dst,
            block_end,
            history_start,
            last_offset,
            litlen,
        )?;
        let recent_index = offset_index.saturating_add(3);
        let chosen = *recent
            .get(recent_index)
            .ok_or_else(|| Error::damaged("LZ recent-offset index outside table"))?;
        for index in (1_usize..=3).rev() {
            let from = index.saturating_sub(1);
            let value = *recent
                .get(from.saturating_add(3))
                .ok_or_else(|| Error::damaged("LZ recent-offset source missing"))?;
            if let Some(slot) = recent.get_mut(index.saturating_add(3)) {
                *slot = value;
            }
        }
        if let Some(slot) = recent.get_mut(3) {
            *slot = chosen;
        }
        last_offset = chosen;
        if offset_index == 3 {
            offset_at = offset_at.saturating_add(1);
        }
        let match_len = if match_code != 15 {
            match_code.saturating_add(2)
        } else {
            let extra = usize::try_from(
                *table
                    .lengths
                    .get(length_at)
                    .ok_or_else(|| Error::damaged("missing long match length"))?,
            )
            .map_err(|_| Error::damaged("match length conversion failed"))?;
            length_at = length_at.saturating_add(1);
            14_usize.saturating_add(extra)
        };
        copy_match_signed(output, &mut dst, block_end, history_start, chosen, match_len)?;
    }
    if offset_at != table.offsets.len() || length_at != table.lengths.len() {
        return Err(Error::damaged("Kraken LZ offset/length streams not fully consumed"));
    }
    let final_len = block_end.saturating_sub(dst);
    if table.literals.len().saturating_sub(literal_at) != final_len {
        return Err(Error::damaged("Kraken LZ final literal count mismatch"));
    }
    copy_lz_literals(
        mode,
        &table.literals,
        &mut literal_at,
        output,
        &mut dst,
        block_end,
        history_start,
        last_offset,
        final_len,
    )?;
    if dst != block_end || literal_at != table.literals.len() {
        return Err(Error::damaged("Kraken LZ did not fill block exactly"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn copy_lz_literals(
    mode: u8,
    literals: &[u8],
    literal_at: &mut usize,
    output: &mut [u8],
    dst: &mut usize,
    block_end: usize,
    history_start: usize,
    last_offset: i32,
    count: usize,
) -> Result<()> {
    let lit_end = literal_at
        .checked_add(count)
        .ok_or_else(|| Error::damaged("literal stream position overflow"))?;
    let src = literals
        .get(*literal_at..lit_end)
        .ok_or_else(|| Error::damaged("literal stream exhausted"))?;
    let out_end = dst
        .checked_add(count)
        .ok_or_else(|| Error::damaged("literal output position overflow"))?;
    if out_end > block_end {
        return Err(Error::damaged("literal run exceeds output block"));
    }
    if mode == 1 {
        output
            .get_mut(*dst..out_end)
            .ok_or_else(|| Error::damaged("literal output range invalid"))?
            .copy_from_slice(src);
    } else {
        for (index, value) in src.iter().copied().enumerate() {
            let pos = dst
                .checked_add(index)
                .ok_or_else(|| Error::damaged("literal delta output position overflow"))?;
            let reference = add_signed(pos, last_offset)?;
            if reference < history_start || reference >= pos {
                return Err(Error::damaged("delta literal reference outside history"));
            }
            let prior = *output
                .get(reference)
                .ok_or_else(|| Error::damaged("delta literal reference outside output"))?;
            if let Some(slot) = output.get_mut(pos) {
                *slot = value.wrapping_add(prior);
            }
        }
    }
    *literal_at = lit_end;
    *dst = out_end;
    Ok(())
}

fn copy_match_signed(
    output: &mut [u8],
    dst: &mut usize,
    block_end: usize,
    history_start: usize,
    offset: i32,
    count: usize,
) -> Result<()> {
    let end = dst
        .checked_add(count)
        .ok_or_else(|| Error::damaged("match output end overflow"))?;
    if end > block_end {
        return Err(Error::damaged("match exceeds output block"));
    }
    for index in 0..count {
        let out = dst
            .checked_add(index)
            .ok_or_else(|| Error::damaged("match output position overflow"))?;
        let reference = add_signed(out, offset)?;
        if reference < history_start || reference >= out {
            return Err(Error::damaged("match offset outside available history"));
        }
        let value = *output
            .get(reference)
            .ok_or_else(|| Error::damaged("match source outside output"))?;
        if let Some(slot) = output.get_mut(out) {
            *slot = value;
        }
    }
    *dst = end;
    Ok(())
}

fn add_signed(base: usize, delta: i32) -> Result<usize> {
    if delta < 0 {
        base.checked_sub(
            usize::try_from(delta.unsigned_abs()).map_err(|_| Error::damaged("negative offset conversion failed"))?,
        )
        .ok_or_else(|| Error::damaged("negative offset before output start"))
    } else {
        base.checked_add(
            usize::try_from(u32::try_from(delta).map_err(|_| Error::damaged("positive offset conversion failed"))?)
                .map_err(|_| Error::damaged("positive offset usize conversion failed"))?,
        )
        .ok_or_else(|| Error::damaged("positive offset overflow"))
    }
}
fn copy_match(output: &mut [u8], dst: usize, count: usize, distance: usize) -> Result<()> {
    if distance == 0 || distance > dst {
        return Err(Error::damaged("whole-match distance outside output history"));
    }
    let end = dst
        .checked_add(count)
        .ok_or_else(|| Error::damaged("whole-match end overflow"))?;
    if end > output.len() {
        return Err(Error::damaged("whole-match exceeds output"));
    }
    for index in 0..count {
        let out = dst
            .checked_add(index)
            .ok_or_else(|| Error::damaged("whole-match output position overflow"))?;
        let from = out
            .checked_sub(distance)
            .ok_or_else(|| Error::damaged("whole-match source before output"))?;
        let value = *output
            .get(from)
            .ok_or_else(|| Error::damaged("whole-match source outside output"))?;
        if let Some(slot) = output.get_mut(out) {
            *slot = value;
        }
    }
    Ok(())
}

fn take_u8(source: &[u8], at: &mut usize) -> Result<u8> {
    let value = *source
        .get(*at)
        .ok_or_else(|| Error::damaged("unexpected end of Kraken source"))?;
    *at = at
        .checked_add(1)
        .ok_or_else(|| Error::damaged("Kraken source position overflow"))?;
    Ok(value)
}
fn take_u24_be(source: &[u8], at: &mut usize) -> Result<u32> {
    let end = at
        .checked_add(3)
        .ok_or_else(|| Error::damaged("u24 source range overflow"))?;
    let value = peek_u24_be(source, *at)?;
    *at = end;
    Ok(value)
}
fn peek_u24_be(source: &[u8], at: usize) -> Result<u32> {
    let end = at.checked_add(3).ok_or_else(|| Error::damaged("u24 range overflow"))?;
    let s = source.get(at..end).ok_or_else(|| Error::damaged("truncated u24"))?;
    Ok(u32::from(s.first().copied().unwrap_or_default())
        .checked_shl(16)
        .unwrap_or_default()
        | u32::from(s.get(1).copied().unwrap_or_default())
            .checked_shl(8)
            .unwrap_or_default()
        | u32::from(s.get(2).copied().unwrap_or_default()))
}
fn read_u16_le(bytes: &[u8]) -> Result<u16> {
    Ok(u16::from_le_bytes(
        <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged("expected two bytes"))?,
    ))
}
fn read_u24_le(bytes: &[u8]) -> Result<u32> {
    if bytes.len() != 3 {
        return Err(Error::damaged("expected three bytes"));
    }
    Ok(u32::from(bytes.first().copied().unwrap_or_default())
        | u32::from(bytes.get(1).copied().unwrap_or_default())
            .checked_shl(8)
            .unwrap_or_default()
        | u32::from(bytes.get(2).copied().unwrap_or_default())
            .checked_shl(16)
            .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[derive(Debug)]
    struct Fixture {
        name: String,
        raw: String,
        size: usize,
    }

    fn fixture_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("kraken")
    }

    fn quoted_value(block: &str, key: &str) -> Option<String> {
        let marker = format!("\"{key}\"");
        let at = block.find(&marker)?;
        let tail = block.get(at.checked_add(marker.len())?..)?;
        let colon = tail.find(':')?;
        let value = tail.get(colon.checked_add(1)?..)?.trim_start();
        let value = value.strip_prefix('"')?;
        let end = value.find('"')?;
        Some(value.get(..end)?.to_owned())
    }

    fn usize_value(block: &str, key: &str) -> Option<usize> {
        let marker = format!("\"{key}\"");
        let at = block.find(&marker)?;
        let tail = block.get(at.checked_add(marker.len())?..)?;
        let colon = tail.find(':')?;
        let value = tail.get(colon.checked_add(1)?..)?.trim_start();
        let end = value.find(|ch: char| !ch.is_ascii_digit()).unwrap_or(value.len());
        value.get(..end)?.parse::<usize>().ok()
    }

    fn parse_manifest(text: &str) -> Vec<Fixture> {
        let mut fixtures = Vec::new();
        for block in text.split('{').skip(1) {
            let Some(body) = block.split('}').next() else {
                continue;
            };
            let (Some(name), Some(raw), Some(size)) = (
                quoted_value(body, "name"),
                quoted_value(body, "raw"),
                usize_value(body, "size"),
            ) else {
                continue;
            };
            fixtures.push(Fixture { name, raw, size });
        }
        fixtures
    }

    #[test]
    fn manifest_vectors_match_reference_byte_for_byte() {
        let root = fixture_root();
        let manifest = fs::read_to_string(root.join("manifest.json"))
            .unwrap_or_else(|error| panic!("read Kraken manifest: {error}"));
        let fixtures = parse_manifest(&manifest);
        assert_eq!(fixtures.len(), 23, "manifest must contain the 23 C++ reference vectors");

        for fixture in fixtures {
            let packed = fs::read(root.join(format!("{}.kraken", fixture.name)))
                .unwrap_or_else(|error| panic!("read packed {}: {error}", fixture.name));
            let raw =
                fs::read(root.join(&fixture.raw)).unwrap_or_else(|error| panic!("read raw {}: {error}", fixture.raw));
            assert_eq!(raw.len(), fixture.size, "{} raw size", fixture.name);
            let mut decoded = vec![0_u8; fixture.size];
            decompress_into(&packed, &mut decoded).unwrap_or_else(|error| panic!("decode {}: {error:?}", fixture.name));
            assert_eq!(decoded, raw, "{} differs from the C++ reference output", fixture.name);
        }
    }

    #[test]
    fn malformed_prefixes_never_panic() {
        for size in 0..32_usize {
            let source = vec![0_u8; size];
            let mut out = vec![0_u8; 1024];
            let _ = decompress_into(&source, &mut out);
        }
    }
}
