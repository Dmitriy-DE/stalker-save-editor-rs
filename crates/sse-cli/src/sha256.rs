//! Temporary local SHA-256; replace with `sse_codecs::sha256` when the shared codec is available.

const ROUND: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5, 0xD807AA98,
    0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174, 0xE49B69C1, 0xEFBE4786,
    0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA, 0x983E5152, 0xA831C66D, 0xB00327C8,
    0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967, 0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13,
    0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85, 0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819,
    0xD6990624, 0xF40E3585, 0x106AA070, 0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A,
    0x5B9CCA4F, 0x682E6FF3, 0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7,
    0xC67178F2,
];

/// Computes a SHA-256 digest without dependencies.
#[must_use]
pub fn digest(input: &[u8]) -> [u8; 32] {
    let mut state = [
        0x6A09E667_u32,
        0xBB67AE85,
        0x3C6EF372,
        0xA54FF53A,
        0x510E527F,
        0x9B05688C,
        0x1F83D9AB,
        0x5BE0CD19,
    ];

    let mut full_blocks = input.chunks_exact(64);
    for block in full_blocks.by_ref() {
        compress(&mut state, block);
    }

    let remainder = full_blocks.remainder();
    let mut final_block = [0_u8; 64];
    if let Some(prefix) = final_block.get_mut(..remainder.len()) {
        prefix.copy_from_slice(remainder);
    }
    if let Some(marker) = final_block.get_mut(remainder.len()) {
        *marker = 0x80;
    }

    if remainder.len() >= 56 {
        compress(&mut state, &final_block);
        final_block = [0_u8; 64];
    }

    let bit_length = u64::try_from(input.len()).unwrap_or_default().wrapping_mul(8);
    if let Some(length_bytes) = final_block.get_mut(56..64) {
        length_bytes.copy_from_slice(&bit_length.to_be_bytes());
    }
    compress(&mut state, &final_block);

    let mut output = [0_u8; 32];
    for (bytes, word) in output.chunks_exact_mut(4).zip(state) {
        bytes.copy_from_slice(&word.to_be_bytes());
    }
    output
}

fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut words = [0_u32; 64];
    for (word, bytes) in words.iter_mut().take(16).zip(block.chunks_exact(4)) {
        if let Ok(array) = <[u8; 4]>::try_from(bytes) {
            *word = u32::from_be_bytes(array);
        }
    }
    for index in 16_usize..64 {
        let first = words.get(index.saturating_sub(15)).copied().unwrap_or_default();
        let second = words.get(index.saturating_sub(2)).copied().unwrap_or_default();
        let third = words.get(index.saturating_sub(16)).copied().unwrap_or_default();
        let fourth = words.get(index.saturating_sub(7)).copied().unwrap_or_default();
        let sigma0 = first.rotate_right(7) ^ first.rotate_right(18) ^ (first >> 3);
        let sigma1 = second.rotate_right(17) ^ second.rotate_right(19) ^ (second >> 10);
        if let Some(word) = words.get_mut(index) {
            *word = third.wrapping_add(sigma0).wrapping_add(fourth).wrapping_add(sigma1);
        }
    }

    let mut working = *state;
    for (constant, word) in ROUND.iter().zip(words) {
        let [a, b, c, d, e, f, g, h] = working;
        let sigma1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choice = (e & f) ^ (!e & g);
        let temp1 = h
            .wrapping_add(sigma1)
            .wrapping_add(choice)
            .wrapping_add(*constant)
            .wrapping_add(word);
        let sigma0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = sigma0.wrapping_add(majority);
        working = [temp1.wrapping_add(temp2), a, b, c, d.wrapping_add(temp1), e, f, g];
    }
    for (value, addition) in state.iter_mut().zip(working) {
        *value = value.wrapping_add(addition);
    }
}

/// Formats digest bytes using lowercase hexadecimal digits.
#[must_use]
pub fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        let high = usize::from(byte >> 4);
        let low = usize::from(byte & 0x0F);
        output.push(char::from(HEX.get(high).copied().unwrap_or(b'0')));
        output.push(char::from(HEX.get(low).copied().unwrap_or(b'0')));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{digest, lower_hex};

    #[test]
    fn matches_the_sha256_abc_vector() {
        assert_eq!(
            lower_hex(&digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn matches_the_empty_and_padding_boundary_vectors() {
        assert_eq!(
            lower_hex(&digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            lower_hex(&digest(&[b'a'; 56])),
            "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"
        );
        assert_eq!(
            lower_hex(&digest(&[b'a'; 64])),
            "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"
        );
    }
}
