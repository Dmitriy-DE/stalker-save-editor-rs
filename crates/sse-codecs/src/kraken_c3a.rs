//! Independent, bounded Kraken decoding for differential checks against the main decoder.
//!
//! This implementation handles raw and fill quanta, mode-1 LZ commands, raw/Huffman entropy
//! tables, and the small LZNA raw/fill/whole-match subset used by the synthetic S2 fixture. It
//! refuses legacy Huffman tables, tANS, RLE, recursive entropy, checksums, and other Oodle codecs.

use sse_core::{Cursor, Error, Result};

use crate::kraken_c3a_entropy::decode as decode_entropy;
use crate::kraken_c3a_lz::decode_lz_chunk;

const MAXIMUM_OUTPUT_SIZE: usize = 536_870_912;
const KRAKEN_QUANTUM_SIZE: usize = 0x40000;
const LZNA_QUANTUM_SIZE: usize = 0x4000;

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
    use super::{decompress_into, LZNA_QUANTUM_SIZE};
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
}
