use sse_core::{Cursor, Error, Result};

const MAXIMUM_UNPACKED_SIZE: usize = 536_870_912;
const MAX_EXTENSION_ZEROES: usize = 2_105_377;
const I32_MAX_AS_USIZE: usize = 2_147_483_647;

/// Decompresses the LZO1X subset accepted by the original C# codec.
///
/// # Errors
/// Returns [`Error::Damaged`] for truncated or malformed streams, invalid back-references,
/// a missing or invalid end marker, or output whose length differs from `expected_size`.
pub fn decompress(stream: &[u8], expected_size: usize) -> Result<Vec<u8>> {
    if expected_size > MAXIMUM_UNPACKED_SIZE {
        return Err(Error::damaged(format!("invalid LZO output size: {expected_size}")));
    }

    if stream.len() < 3 {
        return Err(Error::damaged("LZO stream is too short"));
    }

    let mut reader = Cursor::new(stream);
    let mut output = vec![0_u8; expected_size];
    let mut output_length = 0_usize;
    let mut state = 0_usize;
    let mut bitstream_version = 0_u8;

    if stream.len() >= 5 && stream.first().copied() == Some(17) {
        reader.skip(1)?;
        bitstream_version = reader.u8()?;
    }

    if reader.remaining() > 0 && peek_u8(stream, reader.position())? > 17 {
        let first_literal_length = usize::from(
            reader
                .u8()?
                .checked_sub(17)
                .ok_or_else(|| Error::damaged("invalid first LZO literal length"))?,
        );
        copy_literals(
            &mut reader,
            &mut output,
            &mut output_length,
            expected_size,
            first_literal_length,
        )?;
        state = if first_literal_length < 4 {
            first_literal_length
        } else {
            4
        };
    }

    loop {
        let command = reader.u8()?;

        if command < 16 {
            if state == 0 {
                let mut literal_length = usize::from(command);
                if literal_length == 0 {
                    literal_length = read_extended_length(&mut reader, 15)?;
                }

                let literal_length = literal_length
                    .checked_add(3)
                    .ok_or_else(|| Error::damaged("LZO literal length overflow"))?;
                copy_literals(
                    &mut reader,
                    &mut output,
                    &mut output_length,
                    expected_size,
                    literal_length,
                )?;
                state = 4;
                continue;
            }

            let next_literals = usize::from(command & 3);
            let encoded_high = usize::from(reader.u8()?);
            let shifted = encoded_high
                .checked_mul(4)
                .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
            let command_offset = usize::from(command.checked_shr(2).unwrap_or_default());
            let base_distance: usize = if state != 4 { 1 } else { 2_049 };
            let distance = base_distance
                .checked_add(command_offset)
                .and_then(|value| value.checked_add(shifted))
                .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
            let match_start = output_length
                .checked_sub(distance)
                .ok_or_else(|| Error::damaged("invalid LZO back-reference"))?;
            let match_length = if state != 4 { 2 } else { 3 };

            copy_match(
                &mut output,
                &mut output_length,
                expected_size,
                match_start,
                match_length,
            )?;
            copy_literals(
                &mut reader,
                &mut output,
                &mut output_length,
                expected_size,
                next_literals,
            )?;
            state = next_literals;
            continue;
        }

        if command >= 64 {
            let next_literals = usize::from(command & 3);
            let encoded_high = usize::from(reader.u8()?);
            let shifted = encoded_high
                .checked_mul(8)
                .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
            let command_offset = usize::from(command.checked_shr(2).unwrap_or_default() & 7);
            let distance = 1_usize
                .checked_add(command_offset)
                .and_then(|value| value.checked_add(shifted))
                .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
            let match_start = output_length
                .checked_sub(distance)
                .ok_or_else(|| Error::damaged("invalid LZO back-reference"))?;
            let match_length = usize::from(command.checked_shr(5).unwrap_or_default())
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZO match length overflow"))?;

            copy_match(
                &mut output,
                &mut output_length,
                expected_size,
                match_start,
                match_length,
            )?;
            copy_literals(
                &mut reader,
                &mut output,
                &mut output_length,
                expected_size,
                next_literals,
            )?;
            state = next_literals;
            continue;
        }

        if command >= 32 {
            let mut match_length = usize::from(command & 31)
                .checked_add(2)
                .ok_or_else(|| Error::damaged("LZO match length overflow"))?;
            if match_length == 2 {
                match_length = match_length
                    .checked_add(read_extended_length(&mut reader, 31)?)
                    .ok_or_else(|| Error::damaged("LZO match length overflow"))?;
            }

            let encoded = reader.u16()?;
            let distance = 1_usize
                .checked_add(usize::from(encoded.checked_shr(2).unwrap_or_default()))
                .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
            let match_start = output_length
                .checked_sub(distance)
                .ok_or_else(|| Error::damaged("invalid LZO back-reference"))?;
            let next_literals = usize::from(encoded & 3);

            copy_match(
                &mut output,
                &mut output_length,
                expected_size,
                match_start,
                match_length,
            )?;
            copy_literals(
                &mut reader,
                &mut output,
                &mut output_length,
                expected_size,
                next_literals,
            )?;
            state = next_literals;
            continue;
        }

        let peeked = peek_u16(stream, reader.position())?;
        let trailing_literals = usize::from(peeked & 3);
        if bitstream_version != 0 && (peeked & 0xFFFC) == 0xFFFC && (command & 0xF8) == 0x18 {
            reader.u16()?;
            if reader.remaining() == 0 {
                return Err(Error::damaged("truncated LZO zero-run extension"));
            }

            let high_length = usize::from(reader.u8()?)
                .checked_mul(8)
                .ok_or_else(|| Error::damaged("LZO zero-run length overflow"))?;
            let zero_length = usize::from(command & 7)
                .checked_add(high_length)
                .and_then(|value| value.checked_add(4))
                .ok_or_else(|| Error::damaged("LZO zero-run length overflow"))?;
            let end = ensure_output_capacity(output_length, expected_size, zero_length)?;
            let target = output
                .get_mut(output_length..end)
                .ok_or_else(|| Error::damaged("LZO output range is invalid"))?;
            target.fill(0);
            output_length = end;
            copy_literals(
                &mut reader,
                &mut output,
                &mut output_length,
                expected_size,
                trailing_literals,
            )?;
            state = trailing_literals;
            continue;
        }

        let mut short_match_length = usize::from(command & 7)
            .checked_add(2)
            .ok_or_else(|| Error::damaged("LZO match length overflow"))?;
        if short_match_length == 2 {
            short_match_length = short_match_length
                .checked_add(read_extended_length(&mut reader, 7)?)
                .ok_or_else(|| Error::damaged("LZO match length overflow"))?;
        }

        let short_encoded = reader.u16()?;
        let short_trailing_literals = usize::from(short_encoded & 3);
        let encoded_distance = usize::from(short_encoded.checked_shr(2).unwrap_or_default());

        if (command & 8) == 0 && encoded_distance == 0 {
            if short_match_length != 3 {
                return Err(Error::damaged("invalid LZO end marker"));
            }
            if reader.position() != stream.len() {
                return Err(Error::damaged("trailing bytes after LZO end marker"));
            }
            break;
        }

        let base_distance: usize = if (command & 8) == 0 { 16_384 } else { 32_768 };
        let distance = base_distance
            .checked_add(encoded_distance)
            .ok_or_else(|| Error::damaged("LZO match offset overflow"))?;
        let short_match_start = output_length
            .checked_sub(distance)
            .ok_or_else(|| Error::damaged("invalid LZO back-reference"))?;

        copy_match(
            &mut output,
            &mut output_length,
            expected_size,
            short_match_start,
            short_match_length,
        )?;
        copy_literals(
            &mut reader,
            &mut output,
            &mut output_length,
            expected_size,
            short_trailing_literals,
        )?;
        state = short_trailing_literals;
    }

    if output_length != expected_size {
        return Err(Error::damaged(format!(
            "LZO output has {output_length} bytes; expected {expected_size}"
        )));
    }

    Ok(output)
}

/// Emits the literal-only LZO1X stream produced by the original C# codec.
#[must_use]
pub fn compress(payload: &[u8]) -> Vec<u8> {
    if payload.is_empty() {
        return vec![0x11, 0, 0];
    }

    let mut output = Vec::new();
    let length = payload.len();

    if length <= 238 {
        let Some(prefix) = length.checked_add(17) else {
            return Vec::new();
        };
        let Ok(prefix) = u8::try_from(prefix) else {
            return Vec::new();
        };
        output.push(prefix);
    } else {
        let Some(mut value) = length.checked_sub(18) else {
            return Vec::new();
        };
        output.push(0);
        while value > usize::from(u8::MAX) {
            output.push(0);
            let Some(next) = value.checked_sub(usize::from(u8::MAX)) else {
                return Vec::new();
            };
            value = next;
        }
        let Ok(last) = u8::try_from(value) else {
            return Vec::new();
        };
        output.push(last);
    }

    output.extend_from_slice(payload);
    output.extend_from_slice(&[0x11, 0, 0]);
    output
}

fn peek_u8(stream: &[u8], position: usize) -> Result<u8> {
    stream
        .get(position)
        .copied()
        .ok_or_else(|| Error::damaged(format!("1 byte wanted at offset {position}")))
}

fn peek_u16(stream: &[u8], position: usize) -> Result<u16> {
    let end = position
        .checked_add(2)
        .ok_or_else(|| Error::damaged("LZO stream offset overflow"))?;
    let bytes = stream
        .get(position..end)
        .ok_or_else(|| Error::damaged(format!("2 bytes wanted at offset {position}")))?;
    let array =
        <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged(format!("2 bytes wanted at offset {position}")))?;
    Ok(u16::from_le_bytes(array))
}

fn read_extended_length(reader: &mut Cursor<'_>, base_length: usize) -> Result<usize> {
    let mut zeroes = 0_usize;
    let value = loop {
        let value = reader.u8()?;
        if value != 0 {
            break value;
        }

        zeroes = zeroes
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZO length extension overflow"))?;
        if zeroes > MAX_EXTENSION_ZEROES {
            return Err(Error::damaged("LZO length extension is too large"));
        }
    };

    let result = zeroes
        .checked_mul(usize::from(u8::MAX))
        .and_then(|length| length.checked_add(base_length))
        .and_then(|length| length.checked_add(usize::from(value)))
        .ok_or_else(|| Error::damaged("LZO length extension overflow"))?;
    if result > I32_MAX_AS_USIZE {
        return Err(Error::damaged("LZO length extension overflows the supported range"));
    }

    Ok(result)
}

fn copy_literals(
    reader: &mut Cursor<'_>,
    output: &mut [u8],
    output_length: &mut usize,
    expected_size: usize,
    length: usize,
) -> Result<()> {
    if length > reader.remaining() {
        return Err(Error::damaged("truncated LZO literal run"));
    }

    let end = ensure_output_capacity(*output_length, expected_size, length)?;
    let source = reader.take(length)?;
    let target = output
        .get_mut(*output_length..end)
        .ok_or_else(|| Error::damaged("LZO output range is invalid"))?;
    target.copy_from_slice(source);
    *output_length = end;
    Ok(())
}

fn copy_match(
    output: &mut [u8],
    output_length: &mut usize,
    expected_size: usize,
    start: usize,
    length: usize,
) -> Result<()> {
    if length == 0 || start >= *output_length {
        return Err(Error::damaged("invalid LZO back-reference"));
    }

    let end = ensure_output_capacity(*output_length, expected_size, length)?;
    let mut source_position = start;
    let mut destination_position = *output_length;

    for _ in 0..length {
        let value = output
            .get(source_position)
            .copied()
            .ok_or_else(|| Error::damaged("invalid LZO back-reference"))?;
        let destination = output
            .get_mut(destination_position)
            .ok_or_else(|| Error::damaged("LZO output range is invalid"))?;
        *destination = value;

        destination_position = destination_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZO output position overflow"))?;
        let next_source = source_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZO match position overflow"))?;
        source_position = if next_source == *output_length {
            start
        } else {
            next_source
        };
    }

    if destination_position != end {
        return Err(Error::damaged("LZO match length mismatch"));
    }
    *output_length = end;
    Ok(())
}

fn ensure_output_capacity(current_length: usize, expected_size: usize, additional_length: usize) -> Result<usize> {
    let end = current_length
        .checked_add(additional_length)
        .ok_or_else(|| Error::damaged("LZO output length overflow"))?;
    if end > expected_size {
        return Err(Error::damaged("LZO output exceeds its advertised size"));
    }
    Ok(end)
}

#[cfg(test)]
mod tests {
    use super::{compress, decompress};
    use sse_core::Error;

    #[test]
    fn attached_literal_fixtures_work_both_ways() {
        for (raw, encoded) in literal_fixtures() {
            assert_eq!(decompress(&encoded, raw.len()), Ok(raw.clone()));
            assert_eq!(compress(&raw), encoded);
        }
    }

    #[test]
    fn attached_extended_m4_fixture_decodes_and_the_csharp_writer_stays_literal_only() {
        let raw = extended_m4_raw();
        let encoded = extended_m4_encoded();
        assert_eq!(decompress(&encoded, raw.len()), Ok(raw.clone()));

        let written = compress(&raw);
        assert_ne!(written, encoded);
        assert_eq!(decompress(&written, raw.len()), Ok(raw));
    }

    #[test]
    fn one_thousand_fixed_seed_round_trips() {
        let mut rng = TestRng::new(0xD1CE_BA5E_0123_4567);
        for _ in 0..1_000 {
            let size = rng.below(70_001);
            let mode = rng.below(4);
            let payload = make_payload(&mut rng, size, mode);
            let encoded = compress(&payload);
            assert_eq!(decompress(&encoded, payload.len()), Ok(payload));
        }
    }

    #[test]
    fn every_truncation_of_extended_m4_is_damaged() {
        let encoded = extended_m4_encoded();
        let expected_size = extended_m4_raw().len();
        for cut in 0..encoded.len() {
            let prefix = encoded.get(..cut).unwrap_or_default();
            assert!(matches!(decompress(prefix, expected_size), Err(Error::Damaged(_))));
        }
    }

    #[test]
    fn versioned_zero_run_extension_matches_csharp() {
        let stream = [
            0x11, 0x01, 0x01, b'A', b'B', b'C', b'D', 0x18, 0xFC, 0xFF, 0x00, 0x11, 0, 0,
        ];
        let expected = [b'A', b'B', b'C', b'D', 0, 0, 0, 0];
        assert_eq!(decompress(&stream, expected.len()), Ok(expected.to_vec()));
    }

    #[test]
    fn hostile_four_gib_literal_is_rejected_without_a_four_gib_output_allocation() {
        let mut stream = vec![0];
        stream.extend(std::iter::repeat_n(0_u8, 16_843_010));
        stream.push(1);
        stream.extend_from_slice(&[0x11, 0, 0]);

        assert!(matches!(decompress(&stream, 0), Err(Error::Damaged(_))));
    }

    #[test]
    fn rejects_output_size_over_csharp_limit_before_allocating() {
        assert!(matches!(decompress(&[0x11, 0, 0], 536_870_913), Err(Error::Damaged(_))));
    }

    fn literal_fixtures() -> Vec<(Vec<u8>, Vec<u8>)> {
        let empty_raw = Vec::new();
        let empty_encoded = vec![0x11, 0, 0];

        let one_raw = b"a".to_vec();
        let one_encoded = vec![0x12, b'a', 0x11, 0, 0];

        let short_raw: Vec<u8> = (0_u8..=237_u8).collect();
        let mut short_encoded = vec![0xFF];
        short_encoded.extend_from_slice(&short_raw);
        short_encoded.extend_from_slice(&[0x11, 0, 0]);

        let mut long_raw = Vec::new();
        for _ in 0..3 {
            long_raw.extend(0_u8..=238_u8);
        }
        let mut long_encoded = vec![0, 0, 0, 0xBD];
        long_encoded.extend_from_slice(&long_raw);
        long_encoded.extend_from_slice(&[0x11, 0, 0]);

        let mut repeated_raw = Vec::new();
        for _ in 0..1_000 {
            repeated_raw.extend_from_slice(b"abc");
        }
        let mut repeated_encoded = vec![0; 12];
        repeated_encoded.push(0xB1);
        repeated_encoded.extend_from_slice(&repeated_raw);
        repeated_encoded.extend_from_slice(&[0x11, 0, 0]);

        vec![
            (empty_raw, empty_encoded),
            (one_raw, one_encoded),
            (short_raw, short_encoded),
            (long_raw, long_encoded),
            (repeated_raw, repeated_encoded),
        ]
    }

    fn extended_m4_raw() -> Vec<u8> {
        vec![b'A'; 16_397]
    }

    fn extended_m4_encoded() -> Vec<u8> {
        let mut encoded = vec![0xFF];
        encoded.extend(std::iter::repeat_n(b'A', 238));
        for _ in 0..5_383 {
            encoded.push(0x40);
            encoded.push(0);
        }
        encoded.extend_from_slice(&[0x10, 0x01, 0x04, 0x00, 0x11, 0, 0]);
        encoded
    }

    fn make_payload(rng: &mut TestRng, size: usize, mode: usize) -> Vec<u8> {
        let mut payload = Vec::with_capacity(size);
        match mode {
            0 => {
                let byte = rng.next_u8();
                payload.extend(std::iter::repeat_n(byte, size));
            }
            1 => {
                let pattern_length = rng.below(31).checked_add(1).unwrap_or(1);
                let mut pattern = Vec::with_capacity(pattern_length);
                for _ in 0..pattern_length {
                    pattern.push(rng.next_u8());
                }
                while payload.len() < size {
                    let remaining = size.saturating_sub(payload.len());
                    let take = remaining.min(pattern.len());
                    let Some(part) = pattern.get(..take) else {
                        break;
                    };
                    payload.extend_from_slice(part);
                }
            }
            2 => {
                for position in 0..size {
                    let reduced = position.checked_rem(251).unwrap_or_default();
                    let byte = u8::try_from(reduced).unwrap_or_default();
                    payload.push(byte);
                }
            }
            _ => {
                for _ in 0..size {
                    payload.push(rng.next_u8());
                }
            }
        }
        payload
    }

    struct TestRng {
        state: u64,
    }

    impl TestRng {
        const fn new(seed: u64) -> Self {
            Self { state: seed }
        }

        fn next_u64(&mut self) -> u64 {
            let mut value = self.state;
            value ^= value.checked_shl(13).unwrap_or_default();
            value ^= value.checked_shr(7).unwrap_or_default();
            value ^= value.checked_shl(17).unwrap_or_default();
            self.state = value;
            value
        }

        fn next_u8(&mut self) -> u8 {
            self.next_u64().to_le_bytes().first().copied().unwrap_or_default()
        }

        fn below(&mut self, upper_exclusive: usize) -> usize {
            let Ok(upper) = u64::try_from(upper_exclusive) else {
                return 0;
            };
            let Some(value) = self.next_u64().checked_rem(upper) else {
                return 0;
            };
            usize::try_from(value).unwrap_or_default()
        }
    }
}
