//! Safe Kraken encoder for the mode-1 LZ and Huffman subset accepted by [`crate::kraken`].
//!
//! The encoder uses bounded hash chains, emits no excess-byte LZ tables, and falls back to stored
//! blocks when compression does not reduce the input. It intentionally does not claim full Oodle
//! encoder compatibility beyond streams accepted by this crate's Kraken decoder.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

const BLOCK_SIZE: usize = 0x40000;
const SUB_BLOCK_SIZE: usize = 0x20000;
const MAX_HISTORY_SIZE: usize = 0x10000;
const MAX_INPUT_SIZE: usize = 0x20000000;
const HASH_BITS: usize = 16;
const HASH_SIZE: usize = 1 << HASH_BITS;
const MAX_CHAIN_SEARCH: usize = 8;
const MAX_MATCH_LENGTH: usize = SUB_BLOCK_SIZE;
const MAX_EXTENDED_LENGTHS: usize = 512;
const NO_POSITION: u32 = u32::MAX;

/// Compresses a payload as a Kraken stream supported by [`crate::kraken::decompress_into`].
///
/// This emits only Kraken blocks, mode-1 LZ chunks, and the old sparse Huffman table form. Blocks
/// without a smaller supported representation are stored verbatim. Empty and over-limit inputs
/// return an empty stream.
#[must_use]
pub fn compress(input: &[u8]) -> Vec<u8> {
    if input.is_empty() || input.len() > MAX_INPUT_SIZE {
        return Vec::new();
    }

    let block_count = input.len().saturating_add(BLOCK_SIZE.saturating_sub(1)) / BLOCK_SIZE;
    let mut output = Vec::with_capacity(input.len().saturating_add(block_count.saturating_mul(5)));
    let mut output_offset = 0_usize;

    for block in input.chunks(BLOCK_SIZE) {
        if let Some(compressed) = encode_quantum(input, output_offset, block) {
            let encoded_size = compressed.len().saturating_sub(1);
            output.extend_from_slice(&[0x8c, 0x06]);
            if !push_u24_be(&mut output, encoded_size) {
                output.clear();
                return output;
            }
            output.extend_from_slice(&compressed);
        } else {
            output.extend_from_slice(&[0xcc, 0x06]);
            output.extend_from_slice(block);
        }
        output_offset = output_offset.saturating_add(block.len());
    }
    output
}

fn encode_quantum(source: &[u8], source_offset: usize, input: &[u8]) -> Option<Vec<u8>> {
    let mut encoded = Vec::with_capacity(input.len());
    let mut block_offset = source_offset;
    for chunk in input.chunks(SUB_BLOCK_SIZE) {
        let seed = block_offset == 0;
        let history_start = block_offset.saturating_sub(MAX_HISTORY_SIZE);
        let history = source.get(history_start..block_offset)?;
        match encode_lz_sub_block(history, chunk, seed) {
            Some(payload) if payload.len() < chunk.len() && payload.len() < 0x80000 => {
                let header = 0x800000_usize | 0x080000 | payload.len();
                if !push_u24_be(&mut encoded, header) {
                    return None;
                }
                encoded.extend_from_slice(&payload);
            }
            _ => {
                let header = 0x800000_usize | chunk.len();
                if !push_u24_be(&mut encoded, header) {
                    return None;
                }
                encoded.extend_from_slice(chunk);
            }
        }
        block_offset = block_offset.checked_add(chunk.len())?;
    }
    if encoded.len().saturating_add(3) < input.len() && encoded.len() <= 0x3ffff {
        Some(encoded)
    } else {
        None
    }
}

#[derive(Clone, Copy)]
struct MatchToken {
    start: usize,
    length: usize,
    distance: usize,
}

fn encode_lz_sub_block(history: &[u8], block: &[u8], has_seed: bool) -> Option<Vec<u8>> {
    let seed_length = if has_seed { 8_usize } else { 0_usize };
    if block.len() <= seed_length.saturating_add(8) {
        return None;
    }
    let mut input = Vec::with_capacity(history.len().saturating_add(block.len()));
    input.extend_from_slice(history);
    input.extend_from_slice(block);
    let start_position = history.len().checked_add(seed_length)?;
    let tokens = find_matches(&input, start_position)?;
    if tokens.is_empty() {
        return None;
    }

    let mut literals = Vec::with_capacity(block.len().saturating_sub(seed_length));
    let mut commands = Vec::with_capacity(tokens.len());
    let mut offsets = Vec::with_capacity(tokens.len());
    let mut packed_lengths = Vec::with_capacity(tokens.len().saturating_mul(2));
    let mut extended_lengths = Vec::new();
    let mut source_at = start_position;

    for token in tokens {
        let literal_length = token.start.checked_sub(source_at)?;
        let literal_end = source_at.checked_add(literal_length)?;
        literals.extend_from_slice(input.get(source_at..literal_end)?);

        let literal_code = if literal_length <= 2 {
            u8::try_from(literal_length).ok()?
        } else {
            append_length(&mut packed_lengths, &mut extended_lengths, literal_length, 3, 258)?;
            3
        };

        let match_code = if token.length <= 16 {
            u8::try_from(token.length.checked_sub(2)?).ok()?
        } else if token.length <= 271 {
            packed_lengths.push(u8::try_from(token.length.checked_sub(17)?).ok()?);
            15
        } else {
            packed_lengths.push(u8::MAX);
            extended_lengths.push(u32::try_from(token.length.checked_sub(272)?).ok()?);
            15
        };
        let command = 0xc0_u8 | match_code.checked_shl(2)? | literal_code;
        commands.push(command);
        offsets.push(token.distance);
        source_at = token.start.checked_add(token.length)?;
    }

    literals.extend_from_slice(input.get(source_at..)?);
    if literals.is_empty() || commands.is_empty() || offsets.len() != commands.len() || packed_lengths.is_empty() {
        return None;
    }
    if extended_lengths.len() > MAX_EXTENDED_LENGTHS {
        return None;
    }

    let offset_bits = encode_offset_bits(&offsets, &extended_lengths)?;
    let mut payload = Vec::with_capacity(input.len());
    if has_seed {
        payload.extend_from_slice(input.get(history.len()..start_position)?);
    }
    append_entropy(&literals, &mut payload)?;
    append_entropy(&commands, &mut payload)?;

    let mut packed_offsets = Vec::with_capacity(offsets.len());
    for distance in offsets {
        packed_offsets.push(distance_code(distance)?.packed);
    }
    append_entropy(&packed_offsets, &mut payload)?;
    append_entropy(&packed_lengths, &mut payload)?;
    payload.extend_from_slice(&offset_bits);
    Some(payload)
}

fn append_length(
    packed_lengths: &mut Vec<u8>,
    extended_lengths: &mut Vec<u32>,
    value: usize,
    packed_bias: usize,
    extended_threshold: usize,
) -> Option<()> {
    if value < extended_threshold {
        packed_lengths.push(u8::try_from(value.checked_sub(packed_bias)?).ok()?);
    } else {
        packed_lengths.push(u8::MAX);
        extended_lengths.push(u32::try_from(value.checked_sub(extended_threshold)?).ok()?);
    }
    Some(())
}

fn find_matches(input: &[u8], start_position: usize) -> Option<Vec<MatchToken>> {
    let mut heads = vec![NO_POSITION; HASH_SIZE];
    let mut previous = vec![NO_POSITION; input.len()];
    let mut tokens = Vec::new();
    let mut position = 0_usize;

    while position < start_position {
        insert_position(input, position, &mut heads, &mut previous);
        position = position.checked_add(1)?;
    }

    let first_match = start_position.checked_add(3)?;
    while position.checked_add(2)? < input.len() {
        if position < first_match {
            insert_position(input, position, &mut heads, &mut previous);
            position = position.checked_add(1)?;
            continue;
        }

        let (best_length, best_distance) = find_best_match(input, position, &heads, &previous)?;
        if best_length >= 3 {
            if best_length <= 4 {
                if let Some(future) = find_better_future_match(input, position, best_length, &mut heads, &mut previous)?
                {
                    position = future;
                    continue;
                }
            }
            let remaining = input.len().checked_sub(position)?;
            let mut selected_length = best_length;
            if selected_length == remaining {
                if selected_length <= 3 {
                    selected_length = 0;
                } else {
                    selected_length = selected_length.checked_sub(1)?;
                }
            }
            if selected_length >= 3 {
                tokens.push(MatchToken {
                    start: position,
                    length: selected_length,
                    distance: best_distance,
                });
                let end = position.checked_add(selected_length)?;
                while position < end {
                    insert_position(input, position, &mut heads, &mut previous);
                    position = position.checked_add(1)?;
                }
                continue;
            }
        }

        insert_position(input, position, &mut heads, &mut previous);
        position = position.checked_add(1)?;
    }
    Some(tokens)
}

fn find_better_future_match(
    input: &[u8],
    position: usize,
    current_length: usize,
    heads: &mut [u32],
    previous: &mut [u32],
) -> Option<Option<usize>> {
    let mut inserted = Vec::new();
    let mut probe = position;
    while probe.checked_add(2)? < input.len() && probe.saturating_sub(position) < 12 {
        if probe > position {
            let (future_length, _) = find_best_match(input, probe, heads, previous)?;
            if future_length > current_length.saturating_add(probe.saturating_sub(position)) {
                return Some(Some(probe));
            }
        }

        if let Some(hash) = hash_at(input, probe) {
            let old_head = *heads.get(hash)?;
            insert_position(input, probe, heads, previous);
            inserted.push((probe, hash, old_head));
        }
        probe = probe.checked_add(1)?;
    }

    for (inserted_position, hash, old_head) in inserted.into_iter().rev() {
        *heads.get_mut(hash)? = old_head;
        *previous.get_mut(inserted_position)? = NO_POSITION;
    }
    Some(None)
}

fn find_best_match(input: &[u8], position: usize, heads: &[u32], previous: &[u32]) -> Option<(usize, usize)> {
    let hash = hash_at(input, position)?;
    let mut candidate = *heads.get(hash)?;
    let mut best_length = 0_usize;
    let mut best_distance = 0_usize;
    let mut attempts = 0_usize;
    let max_length = input.len().saturating_sub(position).min(MAX_MATCH_LENGTH);

    while candidate != NO_POSITION && attempts < MAX_CHAIN_SEARCH {
        let candidate_position = usize::try_from(candidate).ok()?;
        let distance = position.checked_sub(candidate_position)?;
        if distance == 0 {
            break;
        }
        if distance_code(distance).is_some() {
            let mut length = 0_usize;
            while length < max_length
                && input.get(position.checked_add(length)?)? == input.get(candidate_position.checked_add(length)?)?
            {
                length = length.checked_add(1)?;
            }
            if length > best_length {
                best_length = length;
                best_distance = distance;
                if length >= max_length.saturating_div(2) {
                    break;
                }
            }
        }
        candidate = *previous.get(candidate_position)?;
        attempts = attempts.checked_add(1)?;
    }
    Some((best_length, best_distance))
}

fn hash_at(input: &[u8], position: usize) -> Option<usize> {
    let first = u32::from(*input.get(position)?);
    let second = u32::from(*input.get(position.checked_add(1)?)?);
    let third = u32::from(*input.get(position.checked_add(2)?)?);
    let mixed = first
        .wrapping_mul(0x1e35_a7bd)
        .wrapping_add(second.wrapping_mul(0x9e37_79b9))
        .wrapping_add(third.wrapping_mul(0x85eb_ca6b));
    usize::try_from(mixed & u32::try_from(HASH_SIZE.saturating_sub(1)).ok()?).ok()
}

fn insert_position(input: &[u8], position: usize, heads: &mut [u32], previous: &mut [u32]) {
    if position.saturating_add(2) >= input.len() {
        return;
    }
    let Some(hash) = hash_at(input, position) else {
        return;
    };
    let Ok(encoded_position) = u32::try_from(position) else {
        return;
    };
    let Some(head) = heads.get_mut(hash) else {
        return;
    };
    if let Some(previous) = previous.get_mut(position) {
        *previous = *head;
    }
    *head = encoded_position;
}

struct DistanceCode {
    packed: u8,
    prefix: u32,
    width: u8,
}

fn distance_code(distance: usize) -> Option<DistanceCode> {
    if distance == 0 {
        return None;
    }
    let encoded = distance.checked_add(248)?;
    let high = encoded.checked_shr(4)?;
    let width = usize::BITS.checked_sub(high.leading_zeros())?.checked_sub(1)?.max(4);
    if width > 18 {
        return None;
    }
    let base = 1_usize.checked_shl(width)?;
    if high < base || high >= base.checked_mul(2)? {
        return None;
    }
    let prefix = u8::try_from(width.checked_sub(4)?).ok()?.checked_shl(4)?;
    Some(DistanceCode {
        packed: prefix.checked_add(u8::try_from(encoded & 0x0f).ok()?)?,
        prefix: u32::try_from(high.checked_sub(base)?).ok()?,
        width: u8::try_from(width).ok()?,
    })
}

fn encode_offset_bits(offsets: &[usize], extended: &[u32]) -> Option<Vec<u8>> {
    if offsets.is_empty() || extended.len() > MAX_EXTENDED_LENGTHS {
        return None;
    }
    let mut forward = MsbBitWriter::default();
    let mut backward = MsbBitWriter::default();
    write_extension_count(&mut backward, extended.len())?;

    for (index, distance) in offsets.iter().copied().enumerate() {
        let writer = if index % 2 == 0 { &mut forward } else { &mut backward };
        let code = distance_code(distance)?;
        writer.write_bits(code.prefix, code.width)?;
    }
    for (index, value) in extended.iter().copied().enumerate() {
        let writer = if index % 2 == 0 { &mut forward } else { &mut backward };
        write_extended_length(writer, value)?;
    }

    let mut bytes = forward.finish();
    let reverse_bytes = backward.finish();
    bytes.extend(reverse_bytes.into_iter().rev());
    Some(bytes)
}

fn write_extension_count(writer: &mut MsbBitWriter, count: usize) -> Option<()> {
    let value = count.checked_add(1)?;
    let width = usize::BITS.checked_sub(value.leading_zeros())?;
    let zeros = width.checked_sub(1)?;
    for _ in 0..zeros {
        writer.write_bit(false);
    }
    writer.write_bits(u32::try_from(value).ok()?, u8::try_from(width).ok()?)
}

fn write_extended_length(writer: &mut MsbBitWriter, value: u32) -> Option<()> {
    let biased = value.checked_add(64)?;
    let width = u32::BITS.checked_sub(biased.leading_zeros())?;
    let prefix_zeros = width.checked_sub(7)?;
    for _ in 0..prefix_zeros {
        writer.write_bit(false);
    }
    writer.write_bits(biased, u8::try_from(width).ok()?)
}

fn append_entropy(input: &[u8], output: &mut Vec<u8>) -> Option<()> {
    if input.is_empty() || input.len() > 0x3ffff {
        return None;
    }
    match encode_huffman(input) {
        Some(payload) if payload.len() < input.len() => {
            append_huffman_entropy_header(input.len(), payload.len(), output)?;
            output.extend_from_slice(&payload);
        }
        _ => {
            append_raw_entropy_header(input.len(), output)?;
            output.extend_from_slice(input);
        }
    }
    Some(())
}

fn append_raw_entropy_header(output_size: usize, output: &mut Vec<u8>) -> Option<()> {
    if output_size == 0 || output_size > 0x3ffff {
        return None;
    }
    if !push_u24_be(output, output_size) {
        return None;
    }
    Some(())
}

fn append_huffman_entropy_header(output_size: usize, payload_size: usize, output: &mut Vec<u8>) -> Option<()> {
    if output_size == 0 || output_size > 0x3ffff || payload_size >= output_size || payload_size > 0x3ffff {
        return None;
    }
    let destination = output_size.checked_sub(1)?;
    let first = 0x20_u8 | u8::try_from(destination.checked_shr(14)?).ok()?;
    let word = u32::try_from(payload_size).ok()? | u32::try_from(destination & 0x3fff).ok()?.checked_shl(18)?;
    output.push(first);
    output.extend_from_slice(&word.to_be_bytes());
    Some(())
}

fn encode_huffman(input: &[u8]) -> Option<Vec<u8>> {
    if input.len() < 8 {
        return None;
    }
    let mut frequencies = [0_u64; 256];
    for symbol in input.iter().copied() {
        let frequency = frequencies.get_mut(usize::from(symbol))?;
        *frequency = frequency.checked_add(1)?;
    }

    let mut nodes = Vec::new();
    let mut heap = BinaryHeap::new();
    for (symbol, frequency) in frequencies.iter().copied().enumerate() {
        if frequency == 0 {
            continue;
        }
        let symbol = u8::try_from(symbol).ok()?;
        let index = nodes.len();
        nodes.push(HuffmanNode {
            frequency,
            symbol: Some(symbol),
            left: None,
            right: None,
        });
        heap.push(Reverse((frequency, symbol, index)));
    }
    if nodes.len() < 2 || nodes.len() > 255 {
        return None;
    }

    while heap.len() > 1 {
        let Reverse((left_frequency, left_symbol, left_index)) = heap.pop()?;
        let Reverse((right_frequency, right_symbol, right_index)) = heap.pop()?;
        let frequency = left_frequency.checked_add(right_frequency)?;
        let minimum_symbol = left_symbol.min(right_symbol);
        let index = nodes.len();
        nodes.push(HuffmanNode {
            frequency,
            symbol: None,
            left: Some(left_index),
            right: Some(right_index),
        });
        heap.push(Reverse((frequency, minimum_symbol, index)));
    }
    let Reverse((_, _, root)) = heap.pop()?;

    let mut depths = Vec::with_capacity(nodes.len());
    let mut pending = vec![(root, 0_usize)];
    while let Some((index, depth)) = pending.pop() {
        let node = nodes.get(index)?;
        if let Some(symbol) = node.symbol {
            depths.push((symbol, depth.max(1), node.frequency));
        } else {
            pending.push((node.left?, depth.checked_add(1)?));
            pending.push((node.right?, depth.checked_add(1)?));
        }
    }

    let mut counts = [0_usize; 12];
    let mut overflow = 0_i32;
    for (_, depth, _) in depths.iter_mut() {
        if *depth > 11 {
            *depth = 11;
            overflow = overflow.checked_add(1)?;
        }
        let count = counts.get_mut(*depth)?;
        *count = count.checked_add(1)?;
    }
    while overflow > 0 {
        let bits = (1_usize..11)
            .rev()
            .find(|bits| counts.get(*bits).copied().unwrap_or_default() != 0)?;
        let count = counts.get_mut(bits)?;
        *count = count.checked_sub(1)?;
        let next_count = counts.get_mut(bits.checked_add(1)?)?;
        *next_count = next_count.checked_add(2)?;
        let last_count = counts.get_mut(11)?;
        *last_count = last_count.checked_sub(1)?;
        overflow = overflow.checked_sub(2)?;
    }

    let mut by_frequency = depths.clone();
    by_frequency.sort_by_key(|(symbol, _, frequency)| (*frequency, *symbol));
    let mut lengths = [0_u8; 256];
    let mut assigned = 0_usize;
    for bits in (1_usize..=11).rev() {
        for _ in 0..counts.get(bits).copied().unwrap_or_default() {
            let (symbol, _, _) = *by_frequency.get(assigned)?;
            *lengths.get_mut(usize::from(symbol))? = u8::try_from(bits).ok()?;
            assigned = assigned.checked_add(1)?;
        }
    }
    if assigned != depths.len() {
        return None;
    }
    let kraft_sum = lengths
        .iter()
        .copied()
        .filter(|length| *length != 0)
        .fold(0_usize, |sum, length| {
            sum.saturating_add(
                1_usize
                    .checked_shl(u32::from(11_u8.saturating_sub(length)))
                    .unwrap_or_default(),
            )
        });
    if kraft_sum != 1_usize.checked_shl(11)? {
        return None;
    }

    let mut ordered = Vec::with_capacity(depths.len());
    for (symbol, length) in lengths.iter().copied().enumerate() {
        if length != 0 {
            ordered.push((length, u8::try_from(symbol).ok()?));
        }
    }
    ordered.sort_unstable();
    let mut codes = [(0_u16, 0_u8); 256];
    let mut code = 0_u32;
    let mut prior_length = 0_u8;
    for (length, symbol) in ordered.iter().copied() {
        code = code.checked_shl(u32::from(length.saturating_sub(prior_length)))?;
        *codes.get_mut(usize::from(symbol))? = (u16::try_from(code).ok()?, length);
        code = code.checked_add(1)?;
        prior_length = length;
    }

    let mut header = MsbBitWriter::default();
    header.write_bit(false);
    header.write_bit(false);
    header.write_bits(u32::try_from(ordered.len()).ok()?, 8)?;
    if ordered.len() > 1 {
        header.write_bits(4, 3)?;
        for (_, symbol) in ordered.iter().copied() {
            header.write_bits(u32::from(symbol), 8)?;
            header.write_bits(u32::from(lengths.get(usize::from(symbol))?.checked_sub(1)?), 4)?;
        }
    }
    let mut output = header.finish();

    let mut stream_a = LsbBitWriter::default();
    let mut stream_middle = LsbBitWriter::default();
    let mut stream_backwards = LsbBitWriter::default();
    for (index, symbol) in input.iter().copied().enumerate() {
        let (canonical, length) = *codes.get(usize::from(symbol))?;
        let mut reversed = 0_u32;
        for bit in 0..u32::from(length) {
            reversed = reversed.checked_shl(1)? | (u32::from(canonical).checked_shr(bit)? & 1);
        }
        match index % 3 {
            0 => stream_a.write_bits(reversed, length)?,
            1 => stream_backwards.write_bits(reversed, length)?,
            _ => stream_middle.write_bits(reversed, length)?,
        }
    }
    let stream_a = stream_a.finish();
    let stream_middle = stream_middle.finish();
    let stream_backwards = stream_backwards.finish();
    output.extend_from_slice(&u16::try_from(stream_a.len()).ok()?.to_le_bytes());
    output.extend_from_slice(&stream_a);
    output.extend_from_slice(&stream_middle);
    output.extend(stream_backwards.into_iter().rev());
    Some(output)
}

struct HuffmanNode {
    frequency: u64,
    symbol: Option<u8>,
    left: Option<usize>,
    right: Option<usize>,
}

#[derive(Default)]
struct MsbBitWriter {
    bytes: Vec<u8>,
    bit_position: u8,
}

impl MsbBitWriter {
    fn write_bit(&mut self, bit: bool) {
        if self.bit_position == 0 {
            self.bytes.push(0);
        }
        if bit {
            let shift = 7_u8.saturating_sub(self.bit_position);
            if let Some(byte) = self.bytes.last_mut() {
                *byte |= 1_u8.checked_shl(u32::from(shift)).unwrap_or_default();
            }
        }
        self.bit_position = self.bit_position.saturating_add(1) & 7;
    }

    fn write_bits(&mut self, value: u32, count: u8) -> Option<()> {
        if count > 32 {
            return None;
        }
        for shift in (0..count).rev() {
            self.write_bit(value.checked_shr(u32::from(shift))? & 1 != 0);
        }
        Some(())
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

#[derive(Default)]
struct LsbBitWriter {
    bytes: Vec<u8>,
    bit_position: u8,
}

impl LsbBitWriter {
    fn write_bits(&mut self, value: u32, count: u8) -> Option<()> {
        if count > 32 {
            return None;
        }
        for shift in 0..count {
            if self.bit_position == 0 {
                self.bytes.push(0);
            }
            if value.checked_shr(u32::from(shift))? & 1 != 0 {
                if let Some(byte) = self.bytes.last_mut() {
                    *byte |= 1_u8.checked_shl(u32::from(self.bit_position)).unwrap_or_default();
                }
            }
            self.bit_position = self.bit_position.saturating_add(1) & 7;
        }
        Some(())
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

fn push_u24_be(output: &mut Vec<u8>, value: usize) -> bool {
    if value > 0x00ff_ffff {
        return false;
    }
    let Ok(high) = u8::try_from(value.checked_shr(16).unwrap_or_default()) else {
        return false;
    };
    let Ok(middle) = u8::try_from(value.checked_shr(8).unwrap_or_default() & 0xff) else {
        return false;
    };
    let Ok(low) = u8::try_from(value & 0xff) else {
        return false;
    };
    output.extend_from_slice(&[high, middle, low]);
    true
}

#[cfg(test)]
mod tests {
    use super::{append_entropy, push_u24_be};
    use crate::kraken;

    #[test]
    fn sparse_huffman_three_streams_round_trip() {
        let mut original = vec![0_u8; 8_000];
        original.extend(std::iter::repeat_n(1_u8, 2_000));
        let mut entropy = Vec::new();
        append_entropy(&original, &mut entropy).unwrap_or_else(|| panic!("encode entropy"));
        assert_eq!((entropy.first().copied().unwrap_or_default() >> 4) & 7, 2);

        let mut stream = vec![0x8c, 0x06];
        assert!(push_u24_be(&mut stream, entropy.len().saturating_sub(1)));
        stream.extend_from_slice(&entropy);
        let mut decoded = vec![0; original.len()];
        kraken::decompress_into(&stream, &mut decoded)
            .unwrap_or_else(|error| panic!("decode Huffman stream: {error:?}"));
        assert_eq!(decoded, original);
    }
}
