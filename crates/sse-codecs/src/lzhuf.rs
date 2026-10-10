//! X-Ray archive header codecs: LZHUF decoding and header descrambling.

use sse_core::{Error, Result};

const WINDOW_SIZE: usize = 4096;
const LOOKAHEAD_SIZE: usize = 60;
const THRESHOLD: usize = 2;
const MAXIMUM_FREQUENCY: i32 = 0x4000;
const CHARACTER_COUNT: usize = 256 - THRESHOLD + LOOKAHEAD_SIZE;
const TREE_SIZE: usize = CHARACTER_COUNT * 2 - 1;
const ROOT: usize = TREE_SIZE - 1;
const MAXIMUM_OUTPUT: usize = 64 * 1024 * 1024;

struct Decoder<'a> {
    source: &'a [u8],
    source_position: usize,
    bit_buffer: u32,
    bit_count: i32,
    frequency: Vec<i32>,
    son: Vec<usize>,
    parent: Vec<usize>,
}

impl<'a> Decoder<'a> {
    fn new(source: &'a [u8]) -> Result<Self> {
        let frequency_len = TREE_SIZE
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZHUF frequency size overflow"))?;
        let parent_len = TREE_SIZE
            .checked_add(CHARACTER_COUNT)
            .ok_or_else(|| Error::damaged("LZHUF parent size overflow"))?;
        let mut frequency = vec![0_i32; frequency_len];
        let mut son = vec![0_usize; TREE_SIZE];
        let mut parent = vec![0_usize; parent_len];

        let mut index = 0_usize;
        while index < CHARACTER_COUNT {
            *frequency
                .get_mut(index)
                .ok_or_else(|| Error::damaged("LZHUF frequency init"))? = 1;
            let child = index
                .checked_add(TREE_SIZE)
                .ok_or_else(|| Error::damaged("LZHUF child overflow"))?;
            *son.get_mut(index).ok_or_else(|| Error::damaged("LZHUF son init"))? = child;
            *parent
                .get_mut(child)
                .ok_or_else(|| Error::damaged("LZHUF parent init"))? = index;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF init counter overflow"))?;
        }

        let mut leaf = 0_usize;
        let mut node = CHARACTER_COUNT;
        while node <= ROOT {
            let next_leaf = leaf
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF leaf overflow"))?;
            let sum = frequency
                .get(leaf)
                .copied()
                .unwrap_or_default()
                .checked_add(frequency.get(next_leaf).copied().unwrap_or_default())
                .ok_or_else(|| Error::damaged("LZHUF frequency overflow"))?;
            *frequency
                .get_mut(node)
                .ok_or_else(|| Error::damaged("LZHUF node frequency"))? = sum;
            *son.get_mut(node).ok_or_else(|| Error::damaged("LZHUF node son"))? = leaf;
            *parent
                .get_mut(leaf)
                .ok_or_else(|| Error::damaged("LZHUF leaf parent"))? = node;
            *parent
                .get_mut(next_leaf)
                .ok_or_else(|| Error::damaged("LZHUF leaf parent"))? = node;
            leaf = leaf
                .checked_add(2)
                .ok_or_else(|| Error::damaged("LZHUF leaf overflow"))?;
            node = node
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF node overflow"))?;
        }
        *frequency
            .get_mut(TREE_SIZE)
            .ok_or_else(|| Error::damaged("LZHUF sentinel"))? = 0xFFFF;
        *parent
            .get_mut(ROOT)
            .ok_or_else(|| Error::damaged("LZHUF root parent"))? = 0;

        Ok(Self {
            source,
            source_position: 4,
            bit_buffer: 0,
            bit_count: 0,
            frequency,
            son,
            parent,
        })
    }

    fn next_source_byte(&mut self) -> Result<u8> {
        let value = self.source.get(self.source_position).copied();
        let old = self.source_position;
        self.source_position = self
            .source_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZHUF source position overflow"))?;
        if let Some(byte) = value {
            return Ok(byte);
        }
        let allowed = self
            .source
            .len()
            .checked_add(2)
            .ok_or_else(|| Error::damaged("LZHUF source limit overflow"))?;
        if old >= allowed {
            return Err(Error::damaged("LZ-Huffman payload is truncated"));
        }
        Ok(0)
    }

    fn refill(&mut self) -> Result<()> {
        while self.bit_count <= 8 {
            let value = u32::from(self.next_source_byte()?);
            let shift = u32::try_from(
                8_i32
                    .checked_sub(self.bit_count)
                    .ok_or_else(|| Error::damaged("LZHUF shift underflow"))?,
            )
            .map_err(|_| Error::damaged("LZHUF negative shift"))?;
            self.bit_buffer |= value
                .checked_shl(shift)
                .ok_or_else(|| Error::damaged("LZHUF bit-buffer shift"))?;
            self.bit_count = self
                .bit_count
                .checked_add(8)
                .ok_or_else(|| Error::damaged("LZHUF bit-count overflow"))?;
        }
        Ok(())
    }

    fn read_bit(&mut self) -> Result<usize> {
        self.refill()?;
        let value = self.bit_buffer;
        self.bit_buffer = self.bit_buffer.checked_shl(1).unwrap_or_default();
        self.bit_count = self
            .bit_count
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("LZHUF bit-count underflow"))?;
        Ok(usize::try_from(value.checked_shr(15).unwrap_or_default() & 1).unwrap_or_default())
    }

    fn read_byte(&mut self) -> Result<u8> {
        self.refill()?;
        let value = self.bit_buffer;
        self.bit_buffer = self.bit_buffer.checked_shl(8).unwrap_or_default();
        self.bit_count = self
            .bit_count
            .checked_sub(8)
            .ok_or_else(|| Error::damaged("LZHUF bit-count underflow"))?;
        u8::try_from(value.checked_shr(8).unwrap_or_default() & 0xFF)
            .map_err(|_| Error::damaged("LZHUF byte conversion"))
    }

    fn update(&mut self, symbol: usize) -> Result<()> {
        if self.frequency.get(ROOT).copied().unwrap_or_default() == MAXIMUM_FREQUENCY {
            let mut first_leaf = 0_usize;
            let mut node = 0_usize;
            while node < TREE_SIZE {
                let child = self.son.get(node).copied().unwrap_or_default();
                if child >= TREE_SIZE {
                    let half = self
                        .frequency
                        .get(node)
                        .copied()
                        .unwrap_or_default()
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF rescale overflow"))?
                        .checked_div(2)
                        .ok_or_else(|| Error::damaged("LZHUF rescale division"))?;
                    *self
                        .frequency
                        .get_mut(first_leaf)
                        .ok_or_else(|| Error::damaged("LZHUF rescale frequency"))? = half;
                    *self
                        .son
                        .get_mut(first_leaf)
                        .ok_or_else(|| Error::damaged("LZHUF rescale son"))? = child;
                    first_leaf = first_leaf
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF leaf overflow"))?;
                }
                node = node
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("LZHUF node overflow"))?;
            }

            let mut left = 0_usize;
            let mut right = CHARACTER_COUNT;
            while right < TREE_SIZE {
                let left_next = left
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("LZHUF left overflow"))?;
                let next_frequency = self
                    .frequency
                    .get(left)
                    .copied()
                    .unwrap_or_default()
                    .checked_add(self.frequency.get(left_next).copied().unwrap_or_default())
                    .ok_or_else(|| Error::damaged("LZHUF frequency overflow"))?;
                let mut insert = right
                    .checked_sub(1)
                    .ok_or_else(|| Error::damaged("LZHUF insert underflow"))?;
                while next_frequency < self.frequency.get(insert).copied().unwrap_or_default() {
                    insert = insert
                        .checked_sub(1)
                        .ok_or_else(|| Error::damaged("LZHUF insert underflow"))?;
                }
                insert = insert
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("LZHUF insert overflow"))?;
                let mut index = right;
                while index > insert {
                    let previous = index
                        .checked_sub(1)
                        .ok_or_else(|| Error::damaged("LZHUF shift underflow"))?;
                    let freq = self.frequency.get(previous).copied().unwrap_or_default();
                    let child = self.son.get(previous).copied().unwrap_or_default();
                    *self
                        .frequency
                        .get_mut(index)
                        .ok_or_else(|| Error::damaged("LZHUF shift frequency"))? = freq;
                    *self
                        .son
                        .get_mut(index)
                        .ok_or_else(|| Error::damaged("LZHUF shift son"))? = child;
                    index = previous;
                }
                *self
                    .frequency
                    .get_mut(insert)
                    .ok_or_else(|| Error::damaged("LZHUF insert frequency"))? = next_frequency;
                *self
                    .son
                    .get_mut(insert)
                    .ok_or_else(|| Error::damaged("LZHUF insert son"))? = left;
                left = left
                    .checked_add(2)
                    .ok_or_else(|| Error::damaged("LZHUF left overflow"))?;
                right = right
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("LZHUF right overflow"))?;
            }

            let mut node = 0_usize;
            while node < TREE_SIZE {
                let child = self.son.get(node).copied().unwrap_or_default();
                *self
                    .parent
                    .get_mut(child)
                    .ok_or_else(|| Error::damaged("LZHUF rebuild parent"))? = node;
                if child < TREE_SIZE {
                    let sibling = child
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF sibling overflow"))?;
                    *self
                        .parent
                        .get_mut(sibling)
                        .ok_or_else(|| Error::damaged("LZHUF rebuild parent"))? = node;
                }
                node = node
                    .checked_add(1)
                    .ok_or_else(|| Error::damaged("LZHUF node overflow"))?;
            }
        }

        let leaf = symbol
            .checked_add(TREE_SIZE)
            .ok_or_else(|| Error::damaged("LZHUF symbol overflow"))?;
        let mut node_index = self
            .parent
            .get(leaf)
            .copied()
            .ok_or_else(|| Error::damaged("LZHUF symbol parent"))?;
        loop {
            let next_frequency = self
                .frequency
                .get(node_index)
                .copied()
                .unwrap_or_default()
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF frequency overflow"))?;
            *self
                .frequency
                .get_mut(node_index)
                .ok_or_else(|| Error::damaged("LZHUF frequency update"))? = next_frequency;
            let mut child = node_index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF child overflow"))?;
            if next_frequency > self.frequency.get(child).copied().unwrap_or(i32::MAX) {
                loop {
                    let next_child = child
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF child overflow"))?;
                    if next_frequency <= self.frequency.get(next_child).copied().unwrap_or(i32::MAX) {
                        break;
                    }
                    child = next_child;
                }
                let child_frequency = self
                    .frequency
                    .get(child)
                    .copied()
                    .ok_or_else(|| Error::damaged("LZHUF swap frequency"))?;
                *self
                    .frequency
                    .get_mut(node_index)
                    .ok_or_else(|| Error::damaged("LZHUF swap frequency"))? = child_frequency;
                *self
                    .frequency
                    .get_mut(child)
                    .ok_or_else(|| Error::damaged("LZHUF swap frequency"))? = next_frequency;

                let left_child = self
                    .son
                    .get(node_index)
                    .copied()
                    .ok_or_else(|| Error::damaged("LZHUF swap son"))?;
                *self
                    .parent
                    .get_mut(left_child)
                    .ok_or_else(|| Error::damaged("LZHUF swap parent"))? = child;
                if left_child < TREE_SIZE {
                    let sibling = left_child
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF sibling overflow"))?;
                    *self
                        .parent
                        .get_mut(sibling)
                        .ok_or_else(|| Error::damaged("LZHUF swap parent"))? = child;
                }
                let right_child = self
                    .son
                    .get(child)
                    .copied()
                    .ok_or_else(|| Error::damaged("LZHUF swap son"))?;
                *self
                    .son
                    .get_mut(child)
                    .ok_or_else(|| Error::damaged("LZHUF swap son"))? = left_child;
                *self
                    .parent
                    .get_mut(right_child)
                    .ok_or_else(|| Error::damaged("LZHUF swap parent"))? = node_index;
                if right_child < TREE_SIZE {
                    let sibling = right_child
                        .checked_add(1)
                        .ok_or_else(|| Error::damaged("LZHUF sibling overflow"))?;
                    *self
                        .parent
                        .get_mut(sibling)
                        .ok_or_else(|| Error::damaged("LZHUF swap parent"))? = node_index;
                }
                *self
                    .son
                    .get_mut(node_index)
                    .ok_or_else(|| Error::damaged("LZHUF swap son"))? = right_child;
                node_index = child;
            }
            node_index = self
                .parent
                .get(node_index)
                .copied()
                .ok_or_else(|| Error::damaged("LZHUF parent walk"))?;
            if node_index == 0 {
                break;
            }
        }
        Ok(())
    }

    fn decode_character(&mut self) -> Result<usize> {
        let mut node = self
            .son
            .get(ROOT)
            .copied()
            .ok_or_else(|| Error::damaged("LZHUF root"))?;
        while node < TREE_SIZE {
            let bit = self.read_bit()?;
            let child = node
                .checked_add(bit)
                .ok_or_else(|| Error::damaged("LZHUF tree index overflow"))?;
            node = self
                .son
                .get(child)
                .copied()
                .ok_or_else(|| Error::damaged("LZHUF tree walk"))?;
        }
        let symbol = node
            .checked_sub(TREE_SIZE)
            .ok_or_else(|| Error::damaged("LZHUF symbol underflow"))?;
        self.update(symbol)?;
        Ok(symbol)
    }
}

fn read_declared_size(code: &[u8]) -> Result<usize> {
    let bytes = code
        .get(..4)
        .ok_or_else(|| Error::damaged("LZ-Huffman header is truncated"))?;
    let array = <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("LZ-Huffman header is truncated"))?;
    usize::try_from(u32::from_le_bytes(array)).map_err(|_| Error::damaged("LZ-Huffman output size does not fit usize"))
}

/// Decodes the adaptive LZHUF stream used for X-Ray archive headers.
///
/// # Errors
/// Returns [`Error::Damaged`] for truncated or malformed input or when the declared output exceeds 64 MiB.
pub fn decode(code: &[u8]) -> Result<Vec<u8>> {
    let text_size = read_declared_size(code)?;
    if text_size > MAXIMUM_OUTPUT {
        return Err(Error::damaged("LZ-Huffman output exceeds the size limit"));
    }
    crate::validate_declared_output_size(code.len(), text_size, "LZ-Huffman")?;
    let mut decoder = Decoder::new(code)?;
    let mut output = vec![0_u8; text_size];
    let buffer_len = WINDOW_SIZE
        .checked_add(LOOKAHEAD_SIZE)
        .and_then(|v| v.checked_sub(1))
        .ok_or_else(|| Error::damaged("LZHUF window size overflow"))?;
    let mut text_buffer = vec![b' '; buffer_len];
    let mut write_position = WINDOW_SIZE
        .checked_sub(LOOKAHEAD_SIZE)
        .ok_or_else(|| Error::damaged("LZHUF write position underflow"))?;
    let mut output_length = 0_usize;
    let window_mask = WINDOW_SIZE
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("LZHUF window mask underflow"))?;

    while output_length < output.len() {
        let symbol = decoder.decode_character()?;
        if symbol < 256 {
            let value = u8::try_from(symbol).map_err(|_| Error::damaged("LZHUF literal conversion"))?;
            *output
                .get_mut(output_length)
                .ok_or_else(|| Error::damaged("LZHUF output index"))? = value;
            *text_buffer
                .get_mut(write_position)
                .ok_or_else(|| Error::damaged("LZHUF window index"))? = value;
            output_length = output_length
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF output overflow"))?;
            write_position = write_position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF window overflow"))?
                & window_mask;
            continue;
        }
        let mut encoded_position = usize::from(decoder.read_byte()?);
        let mut distance = usize::from(distance_code(encoded_position))
            .checked_shl(6)
            .ok_or_else(|| Error::damaged("LZHUF distance shift"))?;
        let bit_length = usize::from(distance_length(encoded_position))
            .checked_sub(2)
            .ok_or_else(|| Error::damaged("LZHUF distance length"))?;
        let mut index = 0_usize;
        while index < bit_length {
            let bit = decoder.read_bit()?;
            encoded_position = encoded_position
                .checked_shl(1)
                .and_then(|value| value.checked_add(bit))
                .ok_or_else(|| Error::damaged("LZHUF encoded position overflow"))?;
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF bit counter overflow"))?;
        }
        distance |= encoded_position & 0x3F;
        let backward = distance
            .checked_add(1)
            .ok_or_else(|| Error::damaged("LZHUF distance overflow"))?;
        let mut copy_position = write_position
            .checked_add(WINDOW_SIZE)
            .and_then(|value| value.checked_sub(backward))
            .ok_or_else(|| Error::damaged("LZHUF copy position underflow"))?
            & window_mask;
        let copy_length = symbol
            .checked_sub(255)
            .and_then(|v| v.checked_add(THRESHOLD))
            .ok_or_else(|| Error::damaged("LZHUF copy length overflow"))?;
        let mut copied = 0_usize;
        while copied < copy_length && output_length < output.len() {
            let value = text_buffer
                .get(copy_position)
                .copied()
                .ok_or_else(|| Error::damaged("LZHUF copy source"))?;
            *output
                .get_mut(output_length)
                .ok_or_else(|| Error::damaged("LZHUF output index"))? = value;
            *text_buffer
                .get_mut(write_position)
                .ok_or_else(|| Error::damaged("LZHUF window index"))? = value;
            output_length = output_length
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF output overflow"))?;
            copy_position = copy_position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF copy position overflow"))?
                & window_mask;
            write_position = write_position
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF write position overflow"))?
                & window_mask;
            copied = copied
                .checked_add(1)
                .ok_or_else(|| Error::damaged("LZHUF copy counter overflow"))?;
        }
    }
    Ok(output)
}

fn distance_code(value: usize) -> u8 {
    match value {
        0..=31 => 0,
        32..=47 => 1,
        48..=63 => 2,
        64..=79 => 3,
        80..=143 => value
            .checked_sub(80)
            .and_then(|v| v.checked_div(8))
            .and_then(|v| v.checked_add(4))
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or_default(),
        144..=191 => value
            .checked_sub(144)
            .and_then(|v| v.checked_div(4))
            .and_then(|v| v.checked_add(12))
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or_default(),
        192..=239 => value
            .checked_sub(192)
            .and_then(|v| v.checked_div(2))
            .and_then(|v| v.checked_add(24))
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or_default(),
        240..=255 => value
            .checked_sub(240)
            .and_then(|v| v.checked_add(48))
            .and_then(|v| u8::try_from(v).ok())
            .unwrap_or_default(),
        _ => 0,
    }
}

fn distance_length(value: usize) -> u8 {
    match value {
        0..=31 => 3,
        32..=79 => 4,
        80..=143 => 5,
        144..=191 => 6,
        192..=239 => 7,
        240..=255 => 8,
        _ => 0,
    }
}

fn next_seed(seed: u32) -> u32 {
    let product = u64::from(seed)
        .checked_mul(0x0808_8405_u64)
        .and_then(|value| value.checked_add(1))
        .unwrap_or_default();
    u32::try_from(product & u64::from(u32::MAX)).unwrap_or_default()
}

/// Reverses X-Ray archive header scrambling exactly as the reference editor does.
#[must_use]
pub fn descramble(data: &[u8], world_wide: bool) -> Vec<u8> {
    let mut seed = if world_wide { 0x016E_B2EB_u32 } else { 0x0131_A9D3_u32 };
    let mut seed0 = if world_wide { 0x005B_BC4B_u32 } else { 0x0132_9436_u32 };
    let multiplier = if world_wide { 4_usize } else { 8_usize };
    let mut sbox: Vec<u8> = (0_u16..=255).map(|v| u8::try_from(v).unwrap_or_default()).collect();
    let rounds = multiplier.checked_mul(256).unwrap_or_default();
    let mut index = 0_usize;
    while index < rounds {
        seed0 = next_seed(seed0);
        let first = u8::try_from(seed0.checked_shr(24).unwrap_or_default()).unwrap_or_default();
        let second = loop {
            seed0 = next_seed(seed0);
            let candidate = u8::try_from(seed0.checked_shr(24).unwrap_or_default()).unwrap_or_default();
            if candidate != first {
                break candidate;
            }
        };
        let a = usize::from(first);
        let b = usize::from(second);
        let av = sbox.get(a).copied().unwrap_or_default();
        let bv = sbox.get(b).copied().unwrap_or_default();
        if let Some(slot) = sbox.get_mut(a) {
            *slot = bv;
        }
        if let Some(slot) = sbox.get_mut(b) {
            *slot = av;
        }
        index = index.checked_add(1).unwrap_or(rounds);
    }
    let mut inverse = vec![0_u8; 256];
    for (index, value) in sbox.iter().copied().enumerate() {
        if let Some(slot) = inverse.get_mut(usize::from(value)) {
            *slot = u8::try_from(index).unwrap_or_default();
        }
    }
    let mut output = Vec::with_capacity(data.len());
    for byte in data.iter().copied() {
        seed = next_seed(seed);
        let mask = u8::try_from(seed.checked_shr(24).unwrap_or_default()).unwrap_or_default();
        output.push(inverse.get(usize::from(byte ^ mask)).copied().unwrap_or_default());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{decode, descramble};
    use sse_core::Error;

    #[test]
    fn rejects_short_header() {
        assert!(matches!(decode(&[1, 2, 3]), Err(Error::Damaged(_))));
    }

    #[test]
    fn refuses_oversize_before_allocation() {
        let bytes = (64_u32 * 1024 * 1024 + 1).to_le_bytes();
        assert!(matches!(decode(&bytes), Err(Error::Damaged(_))));
    }

    #[test]
    fn tiny_stream_cannot_claim_a_large_output() {
        // A 4-byte stream declaring 16 MiB must be refused before the output buffer is allocated.
        let bytes = (16_u32 * 1024 * 1024).to_le_bytes();
        match decode(&bytes) {
            Err(Error::Damaged(message)) => assert!(message.contains("disproportionate"), "{message}"),
            other => panic!("expected a disproportionate-size error, got {other:?}"),
        }
    }

    #[test]
    fn truncated_body_is_an_error_not_a_short_output() {
        // Declares 4 KiB of output but carries no bit stream after the header.
        let mut bytes = 4096_u32.to_le_bytes().to_vec();
        bytes.extend_from_slice(&[0, 0]);
        assert!(matches!(decode(&bytes), Err(Error::Damaged(_))));
    }

    #[test]
    fn arbitrary_bodies_never_panic() {
        // Not a reference vector: only checks that malformed bodies end in Ok or Err, never a panic.
        for seed in 0_u8..64 {
            let mut bytes = 256_u32.to_le_bytes().to_vec();
            bytes.extend((0_u8..200).map(|i| i.wrapping_mul(seed).wrapping_add(seed)));
            let _ = decode(&bytes);
        }
    }

    #[test]
    fn descramble_is_deterministic() {
        let input = b"archive header";
        assert_eq!(descramble(input, false), descramble(input, false));
        assert_eq!(descramble(input, true), descramble(input, true));
        assert_ne!(descramble(input, false), descramble(input, true));
    }
}
