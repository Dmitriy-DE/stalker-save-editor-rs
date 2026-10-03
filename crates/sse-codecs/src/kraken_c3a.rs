//! Independent, bounded Kraken decoding and RLE encoding for differential checks against the main decoder.
//!
//! This implementation handles raw and fill quanta, mode-1 LZ commands, raw/Huffman entropy
//! tables, RLE-only quanta, and the small LZNA raw/fill/whole-match subset used by the synthetic S2
//! fixture. Legacy Huffman tables, tANS, recursive entropy, checksums, and other Oodle codecs are
//! refused.

use sse_core::{Cursor, Error, Result};

use crate::kraken_c3a_entropy::decode as decode_entropy;
use crate::kraken_c3a_lz::decode_lz_chunk;

const MAXIMUM_OUTPUT_SIZE: usize = 536_870_912;
const KRAKEN_QUANTUM_SIZE: usize = 0x40000;
const LZNA_QUANTUM_SIZE: usize = 0x4000;

/// Encodes payloads as valid Kraken blocks, using a safe RLE-only quantum when it saves space.
///
/// The RLE mode encodes zero runs and leaves other bytes literal. Blocks without a compact RLE form are stored raw.
/// Empty or oversized payloads return an empty stream.
#[must_use]
pub fn compress(payload: &[u8]) -> Vec<u8> {
    if payload.is_empty() || payload.len() > MAXIMUM_OUTPUT_SIZE {
        return Vec::new();
    }
    let Some(block_count) = payload
        .len()
        .checked_add(KRAKEN_QUANTUM_SIZE.saturating_sub(1))
        .map(|length| length / KRAKEN_QUANTUM_SIZE)
    else {
        return Vec::new();
    };
    let Some(capacity) = block_count
        .checked_mul(2)
        .and_then(|headers| payload.len().checked_add(headers))
    else {
        return Vec::new();
    };
    let mut encoded = Vec::with_capacity(capacity);
    let mut compressed_quantum = Vec::new();
    let mut rle_tokens = Vec::new();
    let mut offset = 0_usize;
    while offset < payload.len() {
        let end = offset.saturating_add(KRAKEN_QUANTUM_SIZE).min(payload.len());
        let Some(block) = payload.get(offset..end) else {
            return Vec::new();
        };
        compressed_quantum.clear();
        for chunk in block.chunks(0x20000) {
            if append_rle_chunk(chunk, &mut compressed_quantum, &mut rle_tokens).is_err() {
                compressed_quantum.clear();
                break;
            }
        }
        let compressed = compressed_quantum
            .len()
            .checked_add(3)
            .is_some_and(|size| size < block.len() && size <= 0x40000);
        if compressed {
            let compressed_size = compressed_quantum.len();
            let encoded_size = compressed_size.saturating_sub(1);
            encoded.extend_from_slice(&[0x8c, 0x06]);
            let Ok(high) = u8::try_from((encoded_size >> 16) & 0x03) else {
                return Vec::new();
            };
            let Ok(middle) = u8::try_from((encoded_size >> 8) & 0xff) else {
                return Vec::new();
            };
            let Ok(low) = u8::try_from(encoded_size & 0xff) else {
                return Vec::new();
            };
            encoded.extend_from_slice(&[high, middle, low]);
            encoded.extend_from_slice(&compressed_quantum);
        } else {
            encoded.extend_from_slice(&[0xcc, 0x06]);
            encoded.extend_from_slice(block);
        }
        offset = end;
    }
    encoded
}

/// Decodes `source` into the caller-owned `output` buffer.
///
/// # Errors
/// Returns [`Error::Damaged`] for malformed or truncated streams and output-size mismatches.
/// Entropy variants, checksums, or Oodle codecs outside this decoder's supported subset return
/// [`Error::Refused`].
pub fn decompress_into(source: &[u8], output: &mut [u8]) -> Result<()> {
    if output.len() > MAXIMUM_OUTPUT_SIZE {
        return Err(Error::damaged("Kraken output exceeds the 512 MiB limit"));
    }
    if output.is_empty() {
        return Err(Error::damaged("Kraken output size must be positive"));
    }

    let mut input = Cursor::new(source);
    let mut output_offset = 0_usize;
    let mut block = None;

    while output_offset < output.len() {
        if output_offset % KRAKEN_QUANTUM_SIZE == 0 {
            block = Some(parse_block_header(&mut input)?);
        }
        let header = block.ok_or_else(|| Error::damaged("missing Kraken block header"))?;
        let quantum_limit = if is_kraken_type(header.decoder_type) {
            KRAKEN_QUANTUM_SIZE
        } else {
            LZNA_QUANTUM_SIZE
        };
        let remaining_output = output
            .len()
            .checked_sub(output_offset)
            .ok_or_else(|| Error::damaged("Kraken output offset is out of bounds"))?;
        let quantum_len = quantum_limit.min(remaining_output);

        if header.uncompressed {
            copy_input(&mut input, output, output_offset, quantum_len)?;
        } else {
            let quantum = if is_kraken_type(header.decoder_type) {
                parse_kraken_quantum(&mut input, header.checksums)?
            } else {
                parse_lzna_quantum(&mut input, header.checksums)?
            };
            match quantum {
                Quantum::Compressed(length) => {
                    let bytes = input.take(length)?;
                    decode_kraken_quantum(bytes, output, output_offset, quantum_len)?;
                }
                Quantum::Stored(length) if length == quantum_len => {
                    copy_input(&mut input, output, output_offset, quantum_len)?;
                }
                Quantum::Stored(length) if length > quantum_len => {
                    return Err(Error::damaged("LZNA quantum exceeds its output boundary"));
                }
                Quantum::Stored(length) => {
                    input.skip(length)?;
                    return Err(Error::Refused(
                        "compressed Kraken entropy modes are not implemented yet".to_owned(),
                    ));
                }
                Quantum::Fill(byte) => fill_output(output, output_offset, quantum_len, byte)?,
                Quantum::RawLzna => copy_input(&mut input, output, output_offset, quantum_len)?,
                Quantum::BackReference(distance) => {
                    copy_back_reference(output, output_offset, quantum_len, distance)?;
                }
            }
        }
        output_offset = output_offset
            .checked_add(quantum_len)
            .ok_or_else(|| Error::damaged("Kraken output offset overflow"))?;
    }

    if input.remaining() != 0 {
        return Err(Error::damaged("Kraken stream has trailing bytes"));
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct BlockHeader {
    decoder_type: u8,
    uncompressed: bool,
    checksums: bool,
}

enum Quantum {
    Compressed(usize),
    Stored(usize),
    Fill(u8),
    RawLzna,
    BackReference(usize),
}

fn parse_block_header(input: &mut Cursor<'_>) -> Result<BlockHeader> {
    let flags = input.u8()?;
    if flags & 0x0f != 0x0c || flags & 0x30 != 0 {
        return Err(Error::damaged("invalid Kraken block header"));
    }
    let decoder = input.u8()?;
    let decoder_type = decoder & 0x7f;
    if !matches!(decoder_type, 5 | 6) {
        return Err(Error::damaged("unsupported Kraken decoder type"));
    }
    Ok(BlockHeader {
        decoder_type,
        uncompressed: flags & 0x40 != 0,
        checksums: decoder & 0x80 != 0,
    })
}

fn is_kraken_type(decoder_type: u8) -> bool {
    decoder_type == 6
}

fn parse_kraken_quantum(input: &mut Cursor<'_>, checksums: bool) -> Result<Quantum> {
    let header = read_u24_be(input)?;
    let encoded_size = header & 0x3ffff;
    if encoded_size == 0x3ffff {
        if header >> 18 != 1 {
            return Err(Error::damaged("invalid Kraken quantum sentinel"));
        }
        return Ok(Quantum::Fill(input.u8()?));
    }
    let size = usize::try_from(encoded_size)
        .ok()
        .and_then(|value| value.checked_add(1))
        .ok_or_else(|| Error::damaged("Kraken quantum size overflow"))?;
    if checksums {
        input.skip(3)?;
        input.skip(size)?;
        return Err(Error::Refused(
            "Kraken quantum checksums are not implemented yet".to_owned(),
        ));
    }
    Ok(Quantum::Compressed(size))
}

fn decode_kraken_quantum(source: &[u8], output: &mut [u8], offset: usize, length: usize) -> Result<()> {
    let first = source
        .first()
        .copied()
        .ok_or_else(|| Error::damaged("empty Kraken compressed quantum"))?;
    if (first >> 4) & 0x07 == 3 {
        let end = offset
            .checked_add(length)
            .ok_or_else(|| Error::damaged("Kraken RLE output range overflow"))?;
        let decoded = output
            .get_mut(offset..end)
            .ok_or_else(|| Error::damaged("Kraken RLE output range is out of bounds"))?;
        return decode_rle_quantum(source, decoded);
    }

    const SUB_BLOCK_SIZE: usize = 0x20000;
    let mut input = Cursor::new(source);
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::damaged("Kraken quantum output range overflow"))?;
    let mut destination = offset;

    while destination < end {
        let raw_size = end.saturating_sub(destination).min(SUB_BLOCK_SIZE);
        let raw_end = destination
            .checked_add(raw_size)
            .ok_or_else(|| Error::damaged("Kraken sub-block range overflow"))?;
        let header = read_u24_be(&mut input)?;
        if header & 0x80_0000 == 0 {
            let written = output
                .get_mut(destination..raw_end)
                .ok_or_else(|| Error::damaged("Kraken entropy output outside buffer"))?;
            let input_at = input.position();
            let remainder = source
                .get(input_at..)
                .ok_or_else(|| Error::damaged("Kraken entropy source position outside input"))?;
            let used = decode_entropy(remainder, written, 0)?;
            input.skip(used)?;
        } else {
            let compressed_size = usize::try_from(header & 0x7_ffff)
                .map_err(|_| Error::damaged("Kraken LZ size does not fit this platform"))?;
            let mode = u8::try_from((header >> 19) & 0x0f)
                .map_err(|_| Error::damaged("Kraken LZ mode does not fit a byte"))?;
            let compressed = input.take(compressed_size)?;
            if compressed_size == raw_size && mode == 0 {
                output
                    .get_mut(destination..raw_end)
                    .ok_or_else(|| Error::damaged("raw Kraken LZ output outside buffer"))?
                    .copy_from_slice(compressed);
            } else if compressed_size < raw_size {
                decode_lz_chunk(mode, compressed, output, destination, raw_end)?;
            } else {
                return Err(Error::damaged("invalid Kraken LZ payload size or mode"));
            }
        }
        destination = raw_end;
    }
    if input.remaining() != 0 {
        return Err(Error::damaged("Kraken quantum contains trailing compressed bytes"));
    }
    Ok(())
}

fn parse_lzna_quantum(input: &mut Cursor<'_>, checksums: bool) -> Result<Quantum> {
    let header = u16::from_be_bytes(
        input
            .take(2)?
            .try_into()
            .map_err(|_| Error::damaged("truncated LZNA quantum header"))?,
    );
    let encoded_size = usize::from(header & 0x3fff);
    if encoded_size != 0x3fff {
        let size = encoded_size
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZNA quantum size overflow"))?;
        if checksums {
            input.skip(3)?;
            input.skip(size)?;
            return Err(Error::Refused(
                "LZNA quantum checksums are not implemented yet".to_owned(),
            ));
        }
        return Ok(Quantum::Stored(size));
    }
    match header >> 14 {
        1 => Ok(Quantum::Fill(input.u8()?)),
        2 => Ok(Quantum::RawLzna),
        0 => parse_lzna_whole_match(input),
        _ => Err(Error::damaged("invalid LZNA quantum mode")),
    }
}

fn parse_lzna_whole_match(input: &mut Cursor<'_>) -> Result<Quantum> {
    let encoded = u16::from_be_bytes(
        input
            .take(2)?
            .try_into()
            .map_err(|_| Error::damaged("truncated LZNA whole-match header"))?,
    );
    let distance = if encoded >= 0x8000 {
        u64::from(
            encoded
                .checked_sub(0x8000)
                .ok_or_else(|| Error::damaged("LZNA match distance underflow"))?,
        )
        .checked_add(1)
        .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?
    } else {
        let mut accumulated = 0_u64;
        let mut shift = 0_u32;
        let mut terminal = None;
        for _ in 0..5 {
            let byte = input.u8()?;
            if byte & 0x80 != 0 {
                terminal = Some(byte);
                break;
            }
            let part = u64::from(
                byte.checked_add(0x80)
                    .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?,
            )
            .checked_shl(shift)
            .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?;
            accumulated = accumulated
                .checked_add(part)
                .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?;
            shift = shift
                .checked_add(7)
                .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?;
        }
        let terminal = terminal.ok_or_else(|| Error::damaged("unterminated LZNA match distance"))?;
        accumulated = accumulated
            .checked_add(
                u64::from(terminal & 0x7f)
                    .checked_shl(shift)
                    .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?,
            )
            .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?;
        u64::from(encoded)
            .checked_add(0x8000)
            .and_then(|value| accumulated.checked_shl(15).and_then(|part| value.checked_add(part)))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| Error::damaged("LZNA match distance overflow"))?
    };
    let distance =
        usize::try_from(distance).map_err(|_| Error::damaged("LZNA match distance exceeds this platform"))?;
    Ok(Quantum::BackReference(distance))
}

fn read_u24_be(input: &mut Cursor<'_>) -> Result<u32> {
    input
        .take(3)?
        .iter()
        .try_fold(0_u32, |value, byte| {
            value.checked_shl(8).map(|shifted| shifted | u32::from(*byte))
        })
        .ok_or_else(|| Error::damaged("Kraken quantum header overflow"))
}

fn append_rle_chunk(source: &[u8], encoded: &mut Vec<u8>, tokens: &mut Vec<u16>) -> Result<()> {
    if source.is_empty() || source.len() > 0x20000 {
        return Err(Error::damaged("Kraken RLE chunk size is out of bounds"));
    }

    let header_offset = encoded.len();
    encoded.extend_from_slice(&[0; 5]);
    let body_offset = encoded.len();
    encoded.push(0);
    tokens.clear();

    let mut cursor = 0_usize;
    while cursor < source.len() {
        let next_run = next_zero_run(source, cursor);
        let Some((run_start, run_end)) = next_run else {
            append_rle_literals(
                source
                    .get(cursor..)
                    .ok_or_else(|| Error::damaged("Kraken RLE literal range is out of bounds"))?,
                encoded,
                tokens,
            )?;
            cursor = source.len();
            continue;
        };

        append_rle_literals(
            source
                .get(cursor..run_start)
                .ok_or_else(|| Error::damaged("Kraken RLE literal range is out of bounds"))?,
            encoded,
            tokens,
        )?;
        let mut remaining_run = run_end
            .checked_sub(run_start)
            .ok_or_else(|| Error::damaged("Kraken RLE run underflow"))?;
        while remaining_run >= 128 {
            let repeat_count = (remaining_run / 128).min(0x700);
            let repeat_count =
                u16::try_from(repeat_count).map_err(|_| Error::damaged("Kraken RLE run exceeds its long token"))?;
            let long_token = 0x08ff_u16
                .checked_add(repeat_count)
                .ok_or_else(|| Error::damaged("Kraken RLE long token overflow"))?;
            tokens.push(long_token);
            remaining_run = remaining_run
                .checked_sub(usize::from(repeat_count).saturating_mul(128))
                .ok_or_else(|| Error::damaged("Kraken RLE run underflow"))?;
        }
        while remaining_run != 0 {
            let run_length = remaining_run.min(63);
            let run_length =
                u16::try_from(run_length).map_err(|_| Error::damaged("Kraken RLE run exceeds its token"))?;
            let run_bits = run_length
                .checked_shl(6)
                .ok_or_else(|| Error::damaged("Kraken RLE run overflow"))?;
            tokens.push(0x1000 | run_bits);
            remaining_run = remaining_run
                .checked_sub(usize::from(run_length))
                .ok_or_else(|| Error::damaged("Kraken RLE run underflow"))?;
        }
        cursor = run_end;
    }

    for token in tokens.iter().rev() {
        encoded.extend_from_slice(&token.to_le_bytes());
    }

    let encoded_body_size = encoded
        .len()
        .checked_sub(body_offset)
        .ok_or_else(|| Error::damaged("Kraken RLE body size underflow"))?;
    if encoded_body_size > 0x3ffff {
        return Err(Error::damaged("Kraken RLE body exceeds its size field"));
    }
    let decoded_size = source
        .len()
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("Kraken RLE output size underflow"))?;
    let high_size =
        u8::try_from(decoded_size >> 14).map_err(|_| Error::damaged("Kraken RLE output size exceeds its header"))?;
    *encoded
        .get_mut(header_offset)
        .ok_or_else(|| Error::damaged("Kraken RLE header is out of bounds"))? = 0x30 | high_size;
    let decoded_field =
        u32::try_from(decoded_size).map_err(|_| Error::damaged("Kraken RLE output size exceeds its header"))?;
    let body_field =
        u32::try_from(encoded_body_size).map_err(|_| Error::damaged("Kraken RLE body exceeds its header"))?;
    let packed_header = decoded_field.wrapping_shl(18) | body_field;
    let header_bytes = packed_header.to_be_bytes();
    let header_data_start = header_offset
        .checked_add(1)
        .ok_or_else(|| Error::damaged("Kraken RLE header range overflow"))?;
    let header_end = header_offset
        .checked_add(5)
        .ok_or_else(|| Error::damaged("Kraken RLE header range overflow"))?;
    encoded
        .get_mut(header_data_start..header_end)
        .ok_or_else(|| Error::damaged("Kraken RLE header range is out of bounds"))?
        .copy_from_slice(&header_bytes);
    Ok(())
}

fn append_rle_literals(source: &[u8], encoded: &mut Vec<u8>, tokens: &mut Vec<u16>) -> Result<()> {
    for literal_chunk in source.chunks(63) {
        let literal_length =
            u16::try_from(literal_chunk.len()).map_err(|_| Error::damaged("Kraken RLE literal exceeds its token"))?;
        encoded.extend_from_slice(literal_chunk);
        tokens.push(
            0x1000_u16
                .checked_add(literal_length)
                .ok_or_else(|| Error::damaged("Kraken RLE literal token overflow"))?,
        );
    }
    Ok(())
}

fn next_zero_run(source: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start;
    while cursor < source.len() {
        let byte = source.get(cursor).copied()?;
        if byte != 0 {
            cursor = cursor.checked_add(1)?;
            continue;
        }
        let run_start = cursor;
        while source.get(cursor).copied() == Some(0) {
            cursor = cursor.checked_add(1)?;
        }
        if cursor.checked_sub(run_start)? >= 3 {
            return Some((run_start, cursor));
        }
    }
    None
}

fn decode_rle_quantum(mut source: &[u8], output: &mut [u8]) -> Result<()> {
    let mut output_offset = 0_usize;
    while output_offset < output.len() {
        let first = source
            .first()
            .copied()
            .ok_or_else(|| Error::damaged("truncated Kraken RLE chunk header"))?;
        let chunk_type = (first >> 4) & 0x07;
        if chunk_type != 3 {
            return Err(Error::Refused(
                "compressed Kraken entropy modes are not implemented yet".to_owned(),
            ));
        }
        if first & 0x80 != 0 {
            return Err(Error::Refused(
                "short Kraken RLE chunk headers are not implemented".to_owned(),
            ));
        }
        let header = source
            .get(..5)
            .ok_or_else(|| Error::damaged("truncated Kraken RLE chunk header"))?;
        let packed = u32::from_be_bytes(
            header
                .get(1..5)
                .ok_or_else(|| Error::damaged("truncated Kraken RLE size fields"))?
                .try_into()
                .map_err(|_| Error::damaged("invalid Kraken RLE size fields"))?,
        );
        let source_size = usize::try_from(packed & 0x3ffff)
            .map_err(|_| Error::damaged("Kraken RLE input size exceeds this platform"))?;
        let decoded_size = usize::try_from(((packed >> 18) | (u32::from(first) << 14)) & 0x3ffff)
            .map_err(|_| Error::damaged("Kraken RLE output size exceeds this platform"))?
            .checked_add(1)
            .ok_or_else(|| Error::damaged("Kraken RLE output size overflow"))?;
        let expected_size = output
            .len()
            .checked_sub(output_offset)
            .ok_or_else(|| Error::damaged("Kraken RLE output offset is out of bounds"))?
            .min(0x20000);
        if decoded_size != expected_size {
            return Err(Error::damaged("Kraken RLE chunk output size mismatch"));
        }
        let chunk_size = 5_usize
            .checked_add(source_size)
            .ok_or_else(|| Error::damaged("Kraken RLE chunk size overflow"))?;
        let chunk = source
            .get(..chunk_size)
            .ok_or_else(|| Error::damaged("truncated Kraken RLE chunk data"))?;
        let chunk_output_end = output_offset
            .checked_add(decoded_size)
            .ok_or_else(|| Error::damaged("Kraken RLE output range overflow"))?;
        let chunk_output = output
            .get_mut(output_offset..chunk_output_end)
            .ok_or_else(|| Error::damaged("Kraken RLE output range is out of bounds"))?;
        decode_rle_commands(
            chunk
                .get(5..)
                .ok_or_else(|| Error::damaged("truncated Kraken RLE command stream"))?,
            chunk_output,
        )?;
        source = source
            .get(chunk_size..)
            .ok_or_else(|| Error::damaged("Kraken RLE chunk range is out of bounds"))?;
        output_offset = chunk_output_end;
    }
    if !source.is_empty() {
        return Err(Error::damaged("Kraken RLE quantum has trailing bytes"));
    }
    Ok(())
}

fn decode_rle_commands(source: &[u8], output: &mut [u8]) -> Result<()> {
    if source.first().copied() != Some(0) {
        return Err(Error::Refused(
            "compressed Kraken RLE command tables are not implemented".to_owned(),
        ));
    }
    let mut literal_offset = 1_usize;
    let mut command_end = source.len();
    let mut output_offset = 0_usize;
    while literal_offset < command_end {
        if command_end.saturating_sub(literal_offset) < 2 {
            return Err(Error::damaged("truncated Kraken RLE command"));
        }
        let token_start = command_end
            .checked_sub(2)
            .ok_or_else(|| Error::damaged("Kraken RLE command offset underflow"))?;
        let token = u16::from_le_bytes(
            source
                .get(token_start..command_end)
                .ok_or_else(|| Error::damaged("truncated Kraken RLE command"))?
                .try_into()
                .map_err(|_| Error::damaged("invalid Kraken RLE command"))?,
        );
        let command_type = token >> 8;
        let (literal_length, run_length) = if (0x10..=0x1f).contains(&command_type) {
            let Some(data) = token.checked_sub(0x1000) else {
                return Err(Error::damaged("invalid Kraken RLE command tag"));
            };
            (usize::from(data & 0x3f), usize::from(data >> 6))
        } else if (0x09..=0x0f).contains(&command_type) {
            let Some(repetitions) = token.checked_sub(0x08ff) else {
                return Err(Error::damaged("invalid Kraken RLE long command tag"));
            };
            let run_length = usize::from(repetitions)
                .checked_mul(128)
                .ok_or_else(|| Error::damaged("Kraken RLE long run overflow"))?;
            (0, run_length)
        } else {
            return Err(Error::damaged("unsupported Kraken RLE command"));
        };
        let literal_end = literal_offset
            .checked_add(literal_length)
            .ok_or_else(|| Error::damaged("Kraken RLE literal range overflow"))?;
        let output_end = output_offset
            .checked_add(literal_length)
            .and_then(|end| end.checked_add(run_length))
            .ok_or_else(|| Error::damaged("Kraken RLE output range overflow"))?;
        let literal_output_end = output_offset
            .checked_add(literal_length)
            .ok_or_else(|| Error::damaged("Kraken RLE output range overflow"))?;
        if literal_end > command_end || output_end > output.len() {
            return Err(Error::damaged("Kraken RLE token exceeds its buffer"));
        }
        let literal = source
            .get(literal_offset..literal_end)
            .ok_or_else(|| Error::damaged("Kraken RLE literal range is out of bounds"))?;
        output
            .get_mut(output_offset..literal_output_end)
            .ok_or_else(|| Error::damaged("Kraken RLE output range is out of bounds"))?
            .copy_from_slice(literal);
        output
            .get_mut(literal_output_end..output_end)
            .ok_or_else(|| Error::damaged("Kraken RLE run range is out of bounds"))?
            .fill(0);
        literal_offset = literal_end;
        command_end = token_start;
        output_offset = output_end;
    }
    if literal_offset != command_end || output_offset != output.len() {
        return Err(Error::damaged("Kraken RLE output size mismatch"));
    }
    Ok(())
}

fn copy_input(input: &mut Cursor<'_>, output: &mut [u8], offset: usize, length: usize) -> Result<()> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::damaged("Kraken output range overflow"))?;
    let source = input.take(length)?;
    output
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("Kraken output range is out of bounds"))?
        .copy_from_slice(source);
    Ok(())
}

fn fill_output(output: &mut [u8], offset: usize, length: usize, byte: u8) -> Result<()> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::damaged("Kraken output range overflow"))?;
    output
        .get_mut(offset..end)
        .ok_or_else(|| Error::damaged("Kraken output range is out of bounds"))?
        .fill(byte);
    Ok(())
}

fn copy_back_reference(output: &mut [u8], offset: usize, length: usize, distance: usize) -> Result<()> {
    if distance == 0 || distance > offset {
        return Err(Error::damaged("LZNA whole match points before decoded output"));
    }
    let end = offset
        .checked_add(length)
        .ok_or_else(|| Error::damaged("Kraken output range overflow"))?;
    for target_offset in offset..end {
        let source_offset = target_offset
            .checked_sub(distance)
            .ok_or_else(|| Error::damaged("LZNA match source offset underflow"))?;
        let value = output
            .get(source_offset)
            .copied()
            .ok_or_else(|| Error::damaged("LZNA match source is out of bounds"))?;
        *output
            .get_mut(target_offset)
            .ok_or_else(|| Error::damaged("Kraken output range is out of bounds"))? = value;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{compress, decompress_into, KRAKEN_QUANTUM_SIZE, LZNA_QUANTUM_SIZE};
    use sse_core::Error;

    const CONTAINER: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.sav");
    const EXPECTED: &[u8] = include_bytes!("../../../fixtures/synthetic/synthetic-s2.raw");
    const STREAM_OFFSET: usize = 4;
    const STREAM_LENGTH: usize = 352;

    fn fixture_stream() -> &'static [u8] {
        let Some(end) = STREAM_OFFSET.checked_add(STREAM_LENGTH) else {
            return &[];
        };
        CONTAINER.get(STREAM_OFFSET..end).unwrap_or_default()
    }

    fn one_quantum_stream(payload: &[u8]) -> Vec<u8> {
        let encoded_size = u32::try_from(payload.len()).unwrap_or_default().saturating_sub(1);
        let mut stream = vec![
            0x0c,
            0x06,
            u8::try_from((encoded_size >> 16) & 0xff).unwrap_or_default(),
            u8::try_from((encoded_size >> 8) & 0xff).unwrap_or_default(),
            u8::try_from(encoded_size & 0xff).unwrap_or_default(),
        ];
        stream.extend_from_slice(payload);
        stream
    }

    #[test]
    fn synthetic_s2_fixture_decodes_byte_for_byte_into_the_callers_buffer() {
        let mut output = vec![0; EXPECTED.len()];
        assert_eq!(decompress_into(fixture_stream(), &mut output), Ok(()));
        assert_eq!(output, EXPECTED);
    }

    #[test]
    fn every_truncated_fixture_prefix_is_rejected() {
        let stream = fixture_stream();
        for length in 0..stream.len() {
            let prefix = stream.get(..length).unwrap_or_default();
            let mut output = vec![0; EXPECTED.len()];
            assert!(matches!(decompress_into(prefix, &mut output), Err(Error::Damaged(_))));
        }
    }

    #[test]
    fn invalid_header_bits_and_decoder_types_are_rejected() {
        let stream = fixture_stream();
        for (offset, mask) in [(0, 0x10), (1, 0x01)] {
            let mut changed = stream.to_vec();
            let changed_header = changed.get_mut(offset).map(|byte| *byte ^= mask);
            assert!(changed_header.is_some());
            let mut output = vec![0; EXPECTED.len()];
            assert!(matches!(decompress_into(&changed, &mut output), Err(Error::Damaged(_))));
        }
    }

    #[test]
    fn hostile_quantum_length_is_rejected_without_allocating_from_the_stream() {
        let hostile = [0x0c, 0x06, 0x03, 0xff, 0xfe];
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(decompress_into(&hostile, &mut output), Err(Error::Damaged(_))));
    }

    #[test]
    fn raw_lz_subblock_decodes_without_an_intermediate_output_buffer() {
        let length = u32::try_from(EXPECTED.len()).unwrap_or_default();
        let header = 0x80_0000_u32 | length;
        let mut payload = vec![
            u8::try_from(header >> 16).unwrap_or_default(),
            u8::try_from((header >> 8) & 0xff).unwrap_or_default(),
            u8::try_from(header & 0xff).unwrap_or_default(),
        ];
        payload.extend_from_slice(EXPECTED);
        let stream = one_quantum_stream(&payload);
        let mut output = vec![0; EXPECTED.len()];
        assert_eq!(decompress_into(&stream, &mut output), Ok(()));
        assert_eq!(output, EXPECTED);
    }

    #[test]
    fn fill_quantum_expands_into_the_callers_buffer() {
        let stream = [0x0c, 0x06, 0x07, 0xff, 0xff, 0xa5];
        let mut output = vec![0; EXPECTED.len()];
        assert_eq!(decompress_into(&stream, &mut output), Ok(()));
        assert!(output.iter().all(|byte| *byte == 0xa5));
    }

    #[test]
    fn unsupported_tans_entropy_mode_is_refused_instead_of_misdecoded() {
        let mut payload = vec![0, 0, 0, 0x10];
        let packed = (u32::try_from(EXPECTED.len().saturating_sub(1)).unwrap_or_default() << 18) | 1;
        payload.extend_from_slice(&packed.to_be_bytes());
        payload.push(0xa5);
        let stream = one_quantum_stream(&payload);
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(decompress_into(&stream, &mut output), Err(Error::Refused(_))));
    }

    #[test]
    fn checked_quantum_reports_unsupported_checksum_only_after_bounds_checks() {
        let complete = [0x0c, 0x86, 0x00, 0x00, 0x02, 0, 0, 0, 1, 2, 3];
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(
            decompress_into(&complete, &mut output),
            Err(Error::Refused(_))
        ));

        let truncated = [0x0c, 0x86, 0x00, 0x00, 0x02];
        assert!(matches!(
            decompress_into(&truncated, &mut output),
            Err(Error::Damaged(_))
        ));
    }

    #[test]
    fn decoder_restarts_blocks_at_the_256_kib_boundary() {
        let first_block_size = 0x40000;
        let tail = b"tail-bytes";
        let mut stream = vec![0xcc, 0x06];
        stream.resize(2 + first_block_size, 0x31);
        stream.extend_from_slice(&[0xcc, 0x06]);
        stream.extend_from_slice(tail);
        let mut output = vec![0; first_block_size + tail.len()];

        assert_eq!(decompress_into(&stream, &mut output), Ok(()));
        assert!(output
            .get(..first_block_size)
            .unwrap_or_default()
            .iter()
            .all(|byte| *byte == 0x31));
        assert_eq!(output.get(first_block_size..).unwrap_or_default(), tail);
    }

    #[test]
    fn raw_block_encoder_round_trips_across_quantum_boundaries() {
        for length in [
            1_usize,
            17,
            EXPECTED.len(),
            KRAKEN_QUANTUM_SIZE,
            KRAKEN_QUANTUM_SIZE + 1,
        ] {
            let mut payload = vec![0_u8; length];
            for (position, byte) in payload.iter_mut().enumerate() {
                let value = position.wrapping_mul(37).wrapping_add(position / 13);
                *byte = u8::try_from(value % 251).unwrap_or_default();
            }
            let encoded = compress(&payload);
            let block_count = length.saturating_add(KRAKEN_QUANTUM_SIZE.saturating_sub(1)) / KRAKEN_QUANTUM_SIZE;
            assert_eq!(encoded.len(), length.saturating_add(block_count.saturating_mul(2)));
            let mut output = vec![0_u8; length];
            assert_eq!(decompress_into(&encoded, &mut output), Ok(()));
            assert_eq!(output, payload);
        }
    }

    #[test]
    fn rle_encoder_round_trips_zero_heavy_fixture_without_an_image_scratch_copy() {
        let encoded = compress(EXPECTED);
        assert_eq!(encoded.first(), Some(&0x8c));
        let mut output = vec![0_u8; EXPECTED.len()];
        assert_eq!(decompress_into(&encoded, &mut output), Ok(()));
        assert_eq!(output, EXPECTED);
    }

    #[test]
    fn rle_stream_handles_128_kib_and_256_kib_boundaries() {
        let mut payload = vec![0_u8; KRAKEN_QUANTUM_SIZE + 37];
        for offset in (13..payload.len()).step_by(97) {
            if let Some(byte) = payload.get_mut(offset) {
                *byte = u8::try_from(offset % 251).unwrap_or_default();
            }
        }
        let encoded = compress(&payload);
        let mut output = vec![0_u8; payload.len()];

        assert_eq!(decompress_into(&encoded, &mut output), Ok(()));
        assert_eq!(output, payload);
    }

    #[test]
    fn raw_block_encoder_returns_empty_for_unsupported_empty_payload() {
        assert!(compress(&[]).is_empty());
    }

    #[test]
    fn rle_encoder_is_smaller_than_the_stored_fallback_on_s2_fixtures() {
        let fixtures: &[&[u8]] = &[
            EXPECTED,
            include_bytes!("../../../fixtures/synthetic/synthetic-s2-stash.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-armor-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-weapon-expected.raw"),
        ];

        for payload in fixtures {
            let actual_size = compress(payload).len();
            let stored_size = payload.len().saturating_add(2);
            assert!(
                actual_size < stored_size,
                "{actual_size} bytes is not smaller than {stored_size}"
            );
        }
    }

    #[test]
    fn encoder_round_trips_s2_writer_fixtures_byte_for_byte() {
        let fixtures: &[&[u8]] = &[
            EXPECTED,
            include_bytes!("../../../fixtures/synthetic/synthetic-s2-stash.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-money/s2-money-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-stash/s2-stash-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-armor-expected.raw"),
            include_bytes!("../../../fixtures/synthetic/writer-s2-equipment/s2-equipment-weapon-expected.raw"),
        ];

        for payload in fixtures {
            let encoded = compress(payload);
            let mut decoded = vec![0_u8; payload.len()];
            assert_eq!(decompress_into(&encoded, &mut decoded), Ok(()));
            assert_eq!(&decoded, payload);
        }
    }

    #[test]
    fn rle_stream_rejects_every_truncated_prefix() {
        let stream = compress(EXPECTED);
        for length in 0..stream.len() {
            let mut output = vec![0_u8; EXPECTED.len()];
            assert!(matches!(
                decompress_into(stream.get(..length).unwrap_or_default(), &mut output),
                Err(Error::Damaged(_))
            ));
        }
    }

    #[test]
    fn rle_stream_rejects_hostile_compressed_lengths() {
        let mut stream = compress(EXPECTED);
        let header = stream.get_mut(2..5).unwrap_or_default();
        header.copy_from_slice(&[0x03, 0xff, 0xfe]);
        let mut output = vec![0_u8; EXPECTED.len()];
        assert!(matches!(decompress_into(&stream, &mut output), Err(Error::Damaged(_))));
    }

    #[test]
    fn rle_stream_bit_flips_and_deterministic_mutations_never_panic() {
        let stream = compress(EXPECTED);
        for offset in 0..stream.len() {
            let mut changed = stream.clone();
            if let Some(byte) = changed.get_mut(offset) {
                *byte ^= 0x01;
            }
            let mut output = vec![0_u8; EXPECTED.len()];
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decompress_into(&changed, &mut output)));
            assert!(result.is_ok(), "bit flip at byte {offset} panicked");
        }

        let mut state = 0xa341_316c_u32;
        for _ in 0..512 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let offset = usize::try_from(state).unwrap_or_default() % stream.len();
            state = state.rotate_left(11).wrapping_add(0x9e37_79b9);
            let bit = u8::try_from(state & 7).unwrap_or_default();
            let mut changed = stream.clone();
            if let Some(byte) = changed.get_mut(offset) {
                *byte ^= 1_u8.checked_shl(u32::from(bit)).unwrap_or_default();
            }
            let mut output = vec![0_u8; EXPECTED.len()];
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decompress_into(&changed, &mut output)));
            assert!(result.is_ok(), "mutation at byte {offset} panicked");
        }
    }

    #[test]
    fn lzna_raw_quantum_copies_into_the_requested_output_slice() {
        let payload = b"LZNA raw quantum";
        let mut stream = vec![0x0c, 0x05, 0xbf, 0xff];
        stream.extend_from_slice(payload);
        let mut output = vec![0; payload.len()];

        assert_eq!(decompress_into(&stream, &mut output), Ok(()));
        assert_eq!(output, payload);
    }

    #[test]
    fn lzna_whole_match_copies_from_previous_output_with_overlap() {
        let mut stream = vec![0x0c, 0x05, 0xbf, 0xff];
        stream.resize(4 + LZNA_QUANTUM_SIZE, b'A');
        stream.extend_from_slice(&[0x3f, 0xff, 0x80, 0x00]);
        let mut output = vec![0; LZNA_QUANTUM_SIZE + 32];

        assert_eq!(decompress_into(&stream, &mut output), Ok(()));
        assert!(output.iter().all(|byte| *byte == b'A'));
    }

    #[test]
    fn lzna_whole_match_cannot_reference_before_the_output_start() {
        let stream = [0x0c, 0x05, 0x3f, 0xff, 0x80, 0x00];
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(decompress_into(&stream, &mut output), Err(Error::Damaged(_))));
    }

    #[test]
    fn lzna_extended_whole_match_distance_is_bounded_and_truncation_is_damaged() {
        let extended = [0x0c, 0x05, 0x3f, 0xff, 0x00, 0x00, 0x80];
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(
            decompress_into(&extended, &mut output),
            Err(Error::Damaged(_))
        ));

        let truncated = [0x0c, 0x05, 0x3f, 0xff, 0x00, 0x00];
        assert!(matches!(
            decompress_into(&truncated, &mut output),
            Err(Error::Damaged(_))
        ));
    }

    #[test]
    fn output_length_mismatch_is_damaged() {
        let mut output = vec![0; EXPECTED.len() - 1];
        assert!(matches!(
            decompress_into(fixture_stream(), &mut output),
            Err(Error::Damaged(_))
        ));
    }

    #[test]
    fn empty_output_is_rejected_like_the_reference_codec() {
        assert!(matches!(decompress_into(&[], &mut []), Err(Error::Damaged(_))));
    }

    #[test]
    fn deterministic_mutations_never_panic() {
        let stream = fixture_stream();
        let mut state = 0x9e37_79b9_u32;
        for _ in 0..512 {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let offset = usize::try_from(state).unwrap_or_default() % stream.len();
            state = state.rotate_left(13).wrapping_add(0xa5a5_5a5a);
            let bit = u8::try_from(state & 7).unwrap_or_default();
            let mut changed = stream.to_vec();
            let changed_byte = changed
                .get_mut(offset)
                .map(|byte| *byte ^= 1_u8.checked_shl(u32::from(bit)).unwrap_or_default());
            assert!(changed_byte.is_some());
            let mut output = vec![0; EXPECTED.len()];
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| decompress_into(&changed, &mut output)));
            assert!(result.is_ok(), "mutation at byte {offset} panicked");
        }
    }

    #[test]
    fn trailing_source_bytes_are_rejected() {
        let mut stream = fixture_stream().to_vec();
        stream.push(0);
        let mut output = vec![0; EXPECTED.len()];
        assert!(matches!(decompress_into(&stream, &mut output), Err(Error::Damaged(_))));
    }

    #[test]
    #[ignore = "manual release throughput measurement"]
    fn release_fixture_throughput_measurement() {
        let mut output = vec![0; EXPECTED.len()];
        let start = std::time::Instant::now();
        for _ in 0..100_000 {
            let result = decompress_into(
                std::hint::black_box(fixture_stream()),
                std::hint::black_box(&mut output),
            );
            assert_eq!(result, Ok(()));
        }
        eprintln!(
            "100000 Kraken fixture decodes: {:.3} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    #[ignore = "manual release encoder throughput measurement"]
    fn release_encoder_fixture_throughput_measurement() {
        let start = std::time::Instant::now();
        let mut encoded_size = 0_usize;
        for _ in 0..10_000 {
            let encoded = compress(std::hint::black_box(EXPECTED));
            encoded_size = encoded.len();
            std::hint::black_box(encoded);
        }
        eprintln!(
            "10000 Kraken fixture encodes ({} image bytes): {:.3} ms; output={encoded_size} bytes",
            EXPECTED.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
    }
}
