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

const FAST_HASH_SIZE: usize = 16_384;
const FAST_MAX_DISTANCE: usize = 0xBFFF;
const FAST_MIN_MATCH: usize = 3;

/// Compresses payload with a one-pass LZO1X-1-class matcher.
///
/// The stream uses literal runs plus M2/M3/M4 matches accepted by the decoder and standard
/// LZO1X decoders. A 2^14-entry last-position hash table is the only auxiliary allocation.
/// If the greedy stream would exceed the conventional LZO worst-case bound, this falls back
/// to the literal-only compressor.
#[must_use]
pub fn compress_fast(payload: &[u8]) -> Vec<u8> {
    if payload.len() < FAST_MIN_MATCH {
        return compress(payload);
    }

    let bound = match payload
        .len()
        .checked_div(16)
        .and_then(|extra| payload.len().checked_add(extra))
        .and_then(|value| value.checked_add(67))
    {
        Some(value) => value,
        None => return compress(payload),
    };

    let mut table = vec![usize::MAX; FAST_HASH_SIZE];
    let mut output = Vec::with_capacity(payload.len().min(bound));
    let mut position = 0_usize;
    let mut anchor = 0_usize;
    let mut previous_match_patch: Option<MatchPatch> = None;

    while has_bytes(payload, position, 4) {
        let hash = match hash4(payload, position) {
            Some(value) => value,
            None => break,
        };
        let candidate = table.get(hash).copied().unwrap_or(usize::MAX);
        if let Some(slot) = table.get_mut(hash) {
            *slot = position;
        } else {
            return compress(payload);
        }

        let literal_length = match position.checked_sub(anchor) {
            Some(value) => value,
            None => return compress(payload),
        };
        let distance = if candidate != usize::MAX && candidate < position {
            match position.checked_sub(candidate) {
                Some(value) => value,
                None => return compress(payload),
            }
        } else {
            usize::MAX
        };
        let candidate_is_usable = candidate != usize::MAX
            && candidate < position
            && distance <= FAST_MAX_DISTANCE
            && (anchor == 0 || literal_length == 0 || literal_length >= 4);

        let match_length = if candidate_is_usable {
            common_length(payload, candidate, position)
        } else {
            0
        };

        if match_length < FAST_MIN_MATCH {
            position = match position.checked_add(1) {
                Some(value) => value,
                None => break,
            };
            continue;
        }

        if literal_length != 0
            && !emit_literals(&mut output, payload, anchor, literal_length, anchor == 0)
        {
            return compress(payload);
        }

        let patch = match emit_match(&mut output, distance, match_length) {
            Some(value) => value,
            None => return compress(payload),
        };
        previous_match_patch = Some(patch);

        let match_end = match position.checked_add(match_length) {
            Some(value) => value,
            None => return compress(payload),
        };

        let mut seed = match position.checked_add(1) {
            Some(value) => value,
            None => match_end,
        };
        while seed < match_end && has_bytes(payload, seed, 4) {
            if let Some(seed_hash) = hash4(payload, seed) {
                if let Some(slot) = table.get_mut(seed_hash) {
                    *slot = seed;
                }
            }
            seed = match seed.checked_add(1) {
                Some(value) => value,
                None => break,
            };
        }

        position = match_end;
        anchor = match_end;
    }

    let tail = match payload.len().checked_sub(anchor) {
        Some(value) => value,
        None => return compress(payload),
    };
    if tail != 0 {
        if tail <= 3 {
            if let Some(patch) = previous_match_patch {
                if !patch_trailing_literals(&mut output, patch, tail) {
                    return compress(payload);
                }
                if !append_payload(&mut output, payload, anchor, tail) {
                    return compress(payload);
                }
            } else if !emit_literals(&mut output, payload, anchor, tail, anchor == 0) {
                return compress(payload);
            }
        } else if !emit_literals(&mut output, payload, anchor, tail, anchor == 0) {
            return compress(payload);
        }
    }

    output.extend_from_slice(&[0x11, 0, 0]);
    if output.len() > bound {
        return compress(payload);
    }
    output
}

#[derive(Clone, Copy)]
enum MatchPatch {
    Command(usize),
    EncodedLow(usize),
}

fn has_bytes(input: &[u8], start: usize, count: usize) -> bool {
    start.checked_add(count).is_some_and(|end| end <= input.len())
}

fn hash4(input: &[u8], position: usize) -> Option<usize> {
    let a = u32::from(*input.get(position)?);
    let p1 = position.checked_add(1)?;
    let p2 = position.checked_add(2)?;
    let p3 = position.checked_add(3)?;
    let b = u32::from(*input.get(p1)?);
    let c = u32::from(*input.get(p2)?);
    let d = u32::from(*input.get(p3)?);
    let word = a
        | b.checked_shl(8).unwrap_or_default()
        | c.checked_shl(16).unwrap_or_default()
        | d.checked_shl(24).unwrap_or_default();
    let mixed = word.wrapping_mul(0x9E37_79B1);
    usize::try_from(mixed.checked_shr(18).unwrap_or_default()).ok()
}

fn common_length(input: &[u8], left: usize, right: usize) -> usize {
    let maximum = match input.len().checked_sub(right) {
        Some(value) => value,
        None => return 0,
    };
    let mut length = 0_usize;
    while length < maximum {
        let Some(left_position) = left.checked_add(length) else {
            break;
        };
        let Some(right_position) = right.checked_add(length) else {
            break;
        };
        if input.get(left_position) != input.get(right_position) {
            break;
        }
        length = match length.checked_add(1) {
            Some(value) => value,
            None => break,
        };
    }
    length
}

fn emit_literals(output: &mut Vec<u8>, input: &[u8], start: usize, length: usize, first: bool) -> bool {
    if length == 0 {
        return true;
    }

    if first && length <= 238 {
        let Some(prefix) = length.checked_add(17) else {
            return false;
        };
        let Ok(byte) = u8::try_from(prefix) else {
            return false;
        };
        output.push(byte);
        return append_payload(output, input, start, length);
    }

    if length < 4 {
        return false;
    }
    if length <= 18 {
        let Some(value) = length.checked_sub(3) else {
            return false;
        };
        let Ok(byte) = u8::try_from(value) else {
            return false;
        };
        output.push(byte);
    } else {
        output.push(0);
        let Some(extension) = length.checked_sub(18) else {
            return false;
        };
        if !emit_extension(output, extension) {
            return false;
        }
    }
    append_payload(output, input, start, length)
}

fn append_payload(output: &mut Vec<u8>, input: &[u8], start: usize, length: usize) -> bool {
    let Some(end) = start.checked_add(length) else {
        return false;
    };
    let Some(bytes) = input.get(start..end) else {
        return false;
    };
    output.extend_from_slice(bytes);
    true
}

fn emit_extension(output: &mut Vec<u8>, mut value: usize) -> bool {
    if value == 0 {
        return false;
    }
    while value > usize::from(u8::MAX) {
        output.push(0);
        value = match value.checked_sub(usize::from(u8::MAX)) {
            Some(next) => next,
            None => return false,
        };
    }
    let Ok(last) = u8::try_from(value) else {
        return false;
    };
    output.push(last);
    true
}

fn emit_match(output: &mut Vec<u8>, distance: usize, length: usize) -> Option<MatchPatch> {
    if distance == 0 || distance > FAST_MAX_DISTANCE || length < FAST_MIN_MATCH {
        return None;
    }

    if distance <= 0x800 && length <= 8 {
        let encoded_distance = distance.checked_sub(1)?;
        let distance_low = u8::try_from(encoded_distance & 7).ok()?;
        let distance_high = u8::try_from(encoded_distance.checked_shr(3)?).ok()?;
        let length_bits = u8::try_from(length.checked_sub(1)?).ok()?.checked_shl(5)?;
        let distance_bits = distance_low.checked_shl(2)?;
        let command = length_bits.checked_add(distance_bits)?;
        let patch = output.len();
        output.push(command);
        output.push(distance_high);
        Some(MatchPatch::Command(patch))
    } else if distance <= 0x4000 {
        if length <= 33 {
            let encoded_length = length.checked_sub(2)?;
            let encoded_length_u8 = u8::try_from(encoded_length).ok()?;
            output.push(32_u8.checked_add(encoded_length_u8)?);
        } else {
            output.push(32);
            emit_extension(output, length.checked_sub(33)?).then_some(())?;
        }
        let encoded_distance = distance.checked_sub(1)?.checked_shl(2)?;
        let encoded = u16::try_from(encoded_distance).ok()?;
        let patch = output.len();
        output.extend_from_slice(&encoded.to_le_bytes());
        Some(MatchPatch::EncodedLow(patch))
    } else {
        let (base, high_bit) = if distance < 0x8000 {
            (0x4000_usize, 0_u8)
        } else {
            (0x8000_usize, 8_u8)
        };
        let length_bits = if length <= 9 {
            u8::try_from(length.checked_sub(2)?).ok()?
        } else {
            0
        };
        let command = 16_u8.checked_add(high_bit)?.checked_add(length_bits)?;
        output.push(command);
        if length > 9 {
            emit_extension(output, length.checked_sub(9)?).then_some(())?;
        }
        let encoded_distance = distance.checked_sub(base)?.checked_shl(2)?;
        let encoded = u16::try_from(encoded_distance).ok()?;
        let patch = output.len();
        output.extend_from_slice(&encoded.to_le_bytes());
        Some(MatchPatch::EncodedLow(patch))
    }
}

fn patch_trailing_literals(output: &mut [u8], patch: MatchPatch, count: usize) -> bool {
    if count == 0 || count > 3 {
        return false;
    }
    let Ok(bits) = u8::try_from(count) else {
        return false;
    };
    let position = match patch {
        MatchPatch::Command(position) | MatchPatch::EncodedLow(position) => position,
    };
    let Some(byte) = output.get_mut(position) else {
        return false;
    };
    *byte |= bits;
    true
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
    use super::{compress, compress_fast, decompress};
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

    #[test]
    fn fast_compressor_round_trips_two_thousand_fixed_seed_buffers() {
        let mut rng = TestRng::new(0xA11C_E5E1_5EED_900D);
        for case in 0_usize..2_000_usize {
            let size = rng.below(300_001);
            let mode = case.checked_rem(5).unwrap_or_default();
            let payload = make_fast_payload(&mut rng, size, mode);
            let encoded = compress_fast(&payload);
            let bound = payload
                .len()
                .checked_add(payload.len().checked_div(16).unwrap_or_default())
                .and_then(|value| value.checked_add(67))
                .unwrap_or(usize::MAX);
            assert!(encoded.len() <= bound);
            assert_eq!(decompress(&encoded, payload.len()), Ok(payload));
        }
    }

    #[test]
    fn fast_compressor_produces_all_distance_classes() {
        let near = distance_fixture(0x0600, 0x11);
        let medium = distance_fixture(0x3000, 0x22);
        let far = distance_fixture(0xA000, 0x33);

        let near_distances = parsed_match_distances(&compress_fast(&near));
        let medium_distances = parsed_match_distances(&compress_fast(&medium));
        let far_distances = parsed_match_distances(&compress_fast(&far));

        assert!(near_distances.iter().any(|distance| *distance <= 0x800));
        assert!(medium_distances
            .iter()
            .any(|distance| *distance > 0x800 && *distance <= 0x4000));
        assert!(far_distances
            .iter()
            .any(|distance| *distance > 0x4000 && *distance <= 0xBFFF));
    }

    #[test]
    fn attached_repetitive_fixture_ratio_is_not_worse_than_reference() {
        let raw = extended_m4_raw();
        let reference = extended_m4_encoded();
        let encoded = compress_fast(&raw);
        assert!(encoded.len() <= reference.len());
        assert_eq!(decompress(&encoded, raw.len()), Ok(raw));
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

    fn make_fast_payload(rng: &mut TestRng, size: usize, mode: usize) -> Vec<u8> {
        match mode {
            0 => vec![0_u8; size],
            1 => make_payload(rng, size, 1),
            2 => make_payload(rng, size, 3),
            3 => {
                let mut value = make_payload(rng, size, 3);
                let mut position = 0_usize;
                while position < size {
                    let remaining = size.saturating_sub(position);
                    let run = 256_usize.min(remaining);
                    let end = position.checked_add(run).unwrap_or(size);
                    if let Some(part) = value.get_mut(position..end) {
                        part.fill(0);
                    }
                    position = match position.checked_add(4_096) {
                        Some(next) => next,
                        None => break,
                    };
                }
                value
            }
            _ => make_payload(rng, size, 2),
        }
    }

    fn distance_fixture(distance: usize, salt: u8) -> Vec<u8> {
        let marker = [0xFA, salt, 0xCE, 0xD0, 0x0D, 0xBA, 0xBE, salt];
        let capacity = distance.checked_add(marker.len()).unwrap_or(distance);
        let mut data = Vec::with_capacity(capacity);
        data.extend_from_slice(&marker);
        let filler = distance.saturating_sub(marker.len());
        let mut state = u64::from(salt).wrapping_add(0x1234_5678_9ABC_DEF0);
        for _ in 0..filler {
            state ^= state.checked_shl(13).unwrap_or_default();
            state ^= state.checked_shr(7).unwrap_or_default();
            state ^= state.checked_shl(17).unwrap_or_default();
            let byte = state.to_le_bytes().first().copied().unwrap_or_default();
            data.push(byte);
        }
        data.extend_from_slice(&marker);
        data
    }

    fn parsed_match_distances(stream: &[u8]) -> Vec<usize> {
        let mut distances = Vec::new();
        let mut position = 0_usize;
        let mut state = 0_usize;

        if stream.first().copied().unwrap_or_default() > 17 {
            let length = usize::from(stream.first().copied().unwrap_or_default()).saturating_sub(17);
            position = 1_usize.checked_add(length).unwrap_or(stream.len());
            state = length.min(4);
        }

        while position < stream.len() {
            let command = stream.get(position).copied().unwrap_or_default();
            position = position.checked_add(1).unwrap_or(stream.len());

            if command < 16 {
                if state == 0 {
                    let mut length = usize::from(command);
                    if length == 0 {
                        let (value, next) = parse_extension(stream, position, 15);
                        length = value;
                        position = next;
                    }
                    length = length.checked_add(3).unwrap_or(stream.len());
                    position = position.checked_add(length).unwrap_or(stream.len());
                    state = 4;
                    continue;
                }

                let high = stream.get(position).copied().unwrap_or_default();
                position = position.checked_add(1).unwrap_or(stream.len());
                let base: usize = if state == 4 { 2_049 } else { 1 };
                let distance = base
                    .checked_add(usize::from(command.checked_shr(2).unwrap_or_default()))
                    .and_then(|value| value.checked_add(usize::from(high).checked_mul(4)?))
                    .unwrap_or_default();
                distances.push(distance);
                let trailing = usize::from(command & 3);
                position = position.checked_add(trailing).unwrap_or(stream.len());
                state = trailing;
                continue;
            }

            if command >= 64 {
                let high = stream.get(position).copied().unwrap_or_default();
                position = position.checked_add(1).unwrap_or(stream.len());
                let distance = 1_usize
                    .checked_add(usize::from(command.checked_shr(2).unwrap_or_default() & 7))
                    .and_then(|value| value.checked_add(usize::from(high).checked_mul(8)?))
                    .unwrap_or_default();
                distances.push(distance);
                let trailing = usize::from(command & 3);
                position = position.checked_add(trailing).unwrap_or(stream.len());
                state = trailing;
                continue;
            }

            if command >= 32 {
                if command & 31 == 0 {
                    let (_, next) = parse_extension(stream, position, 31);
                    position = next;
                }
                let low = stream.get(position).copied().unwrap_or_default();
                let high_pos = position.checked_add(1).unwrap_or(stream.len());
                let high = stream.get(high_pos).copied().unwrap_or_default();
                position = position.checked_add(2).unwrap_or(stream.len());
                let encoded = u16::from_le_bytes([low, high]);
                distances.push(
                    1_usize
                        .checked_add(usize::from(encoded.checked_shr(2).unwrap_or_default()))
                        .unwrap_or_default(),
                );
                let trailing = usize::from(encoded & 3);
                position = position.checked_add(trailing).unwrap_or(stream.len());
                state = trailing;
                continue;
            }

            if command & 7 == 0 {
                let (_, next) = parse_extension(stream, position, 7);
                position = next;
            }
            let low = stream.get(position).copied().unwrap_or_default();
            let high_pos = position.checked_add(1).unwrap_or(stream.len());
            let high = stream.get(high_pos).copied().unwrap_or_default();
            position = position.checked_add(2).unwrap_or(stream.len());
            let encoded = u16::from_le_bytes([low, high]);
            let encoded_distance = encoded.checked_shr(2).unwrap_or_default();
            if command & 8 == 0 && encoded_distance == 0 {
                break;
            }
            let base: usize = if command & 8 == 0 { 16_384 } else { 32_768 };
            distances.push(base.checked_add(usize::from(encoded_distance)).unwrap_or_default());
            let trailing = usize::from(encoded & 3);
            position = position.checked_add(trailing).unwrap_or(stream.len());
            state = trailing;
        }

        distances
    }

    fn parse_extension(stream: &[u8], mut position: usize, base: usize) -> (usize, usize) {
        let mut value = base;
        loop {
            let byte = stream.get(position).copied().unwrap_or(1);
            position = position.checked_add(1).unwrap_or(stream.len());
            if byte != 0 {
                value = value.saturating_add(usize::from(byte));
                return (value, position);
            }
            value = value.saturating_add(255);
        }
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
