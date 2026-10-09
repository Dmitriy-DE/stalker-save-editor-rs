//! SHA-256 implementation (FIPS 180-4).

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
    0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
    0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
    0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
    0xc67178f2,
];

/// Incremental SHA-256 state for hashing data without retaining the full input.
pub struct Sha256 {
    state: [u32; 8],
    buffer: [u8; 64],
    buffered_len: usize,
    total_len: u64,
}

impl Sha256 {
    /// Creates a new SHA-256 state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            buffered_len: 0,
            total_len: 0,
        }
    }

    /// Adds another input chunk to the hash.
    pub fn update(&mut self, data: &[u8]) {
        self.total_len = self.total_len.saturating_add(data.len() as u64);
        let mut offset = 0_usize;
        while offset < data.len() {
            let space = 64_usize.saturating_sub(self.buffered_len);
            let available = data.len().saturating_sub(offset);
            let to_copy = space.min(available);
            let end = self.buffered_len.saturating_add(to_copy);
            let source_end = offset.saturating_add(to_copy);

            if let (Some(destination), Some(source)) = (
                self.buffer.get_mut(self.buffered_len..end),
                data.get(offset..source_end),
            ) {
                destination.copy_from_slice(source);
                self.buffered_len = end;
                offset = source_end;
            } else {
                break;
            }

            if self.buffered_len == 64 {
                process_block(&self.buffer, &mut self.state);
                self.buffered_len = 0;
            }
        }
    }

    /// Finishes the hash and returns its 32-byte digest.
    #[must_use]
    pub fn finalize(mut self) -> [u8; 32] {
        let bit_len = self.total_len.wrapping_mul(8);
        if let Some(slot) = self.buffer.get_mut(self.buffered_len) {
            *slot = 0x80;
        }
        self.buffered_len = self.buffered_len.wrapping_add(1);

        if self.buffered_len > 56 {
            while self.buffered_len < 64 {
                if let Some(slot) = self.buffer.get_mut(self.buffered_len) {
                    *slot = 0;
                }
                self.buffered_len = self.buffered_len.wrapping_add(1);
            }
            process_block(&self.buffer, &mut self.state);
            self.buffered_len = 0;
        }

        while self.buffered_len < 56 {
            if let Some(slot) = self.buffer.get_mut(self.buffered_len) {
                *slot = 0;
            }
            self.buffered_len = self.buffered_len.wrapping_add(1);
        }

        for (index, byte) in bit_len.to_be_bytes().iter().enumerate() {
            if let Some(slot) = self.buffer.get_mut(56usize.wrapping_add(index)) {
                *slot = *byte;
            }
        }
        process_block(&self.buffer, &mut self.state);

        let mut digest = [0u8; 32];
        for (index, word) in self.state.iter().enumerate() {
            let bytes = word.to_be_bytes();
            let base = index.wrapping_mul(4);
            for (offset, byte) in bytes.iter().enumerate() {
                if let Some(slot) = digest.get_mut(base.wrapping_add(offset)) {
                    *slot = *byte;
                }
            }
        }
        digest
    }

    /// Finishes the hash and returns its lowercase hexadecimal digest.
    #[must_use]
    pub fn finalize_hex(self) -> String {
        digest_hex(&self.finalize())
    }

    /// Finishes the hash and returns its 32-byte digest.
    ///
    /// This alias keeps existing streaming callers source-compatible.
    #[must_use]
    pub fn finish(self) -> [u8; 32] {
        self.finalize()
    }
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

/// Computes the SHA-256 hash of a byte slice.
#[must_use]
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(data);
    hash.finalize()
}

/// Computes the lowercase hex-encoded SHA-256 hash.
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    digest_hex(&sha256(data))
}

fn digest_hex(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(s, "{byte:02x}");
    }
    s
}

fn process_block(block: &[u8; 64], state: &mut [u32; 8]) {
    let mut w = [0u32; 64];
    for t in 0..16usize {
        let base = t.wrapping_mul(4);
        let b0 = block.get(base).copied().unwrap_or(0);
        let b1 = block.get(base.wrapping_add(1)).copied().unwrap_or(0);
        let b2 = block.get(base.wrapping_add(2)).copied().unwrap_or(0);
        let b3 = block.get(base.wrapping_add(3)).copied().unwrap_or(0);
        if let Some(slot) = w.get_mut(t) {
            *slot = u32::from_be_bytes([b0, b1, b2, b3]);
        }
    }
    for t in 16..64usize {
        let w15 = w.get(t.wrapping_sub(15)).copied().unwrap_or(0);
        let s0 = w15.rotate_right(7) ^ w15.rotate_right(18) ^ (w15 >> 3);
        let w2 = w.get(t.wrapping_sub(2)).copied().unwrap_or(0);
        let s1 = w2.rotate_right(17) ^ w2.rotate_right(19) ^ (w2 >> 10);
        let w16 = w.get(t.wrapping_sub(16)).copied().unwrap_or(0);
        let w7 = w.get(t.wrapping_sub(7)).copied().unwrap_or(0);
        if let Some(slot) = w.get_mut(t) {
            *slot = w16.wrapping_add(s0).wrapping_add(w7).wrapping_add(s1);
        }
    }

    let mut a = state.first().copied().unwrap_or(0);
    let mut b = state.get(1).copied().unwrap_or(0);
    let mut c = state.get(2).copied().unwrap_or(0);
    let mut d = state.get(3).copied().unwrap_or(0);
    let mut e = state.get(4).copied().unwrap_or(0);
    let mut f = state.get(5).copied().unwrap_or(0);
    let mut g = state.get(6).copied().unwrap_or(0);
    let mut h = state.get(7).copied().unwrap_or(0);

    for (t, &kt) in K.iter().enumerate() {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let wt = w.get(t).copied().unwrap_or(0);
        let temp1 = h.wrapping_add(s1).wrapping_add(ch).wrapping_add(kt).wrapping_add(wt);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);

        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }

    if let Some(s) = state.get_mut(0) {
        *s = s.wrapping_add(a);
    }
    if let Some(s) = state.get_mut(1) {
        *s = s.wrapping_add(b);
    }
    if let Some(s) = state.get_mut(2) {
        *s = s.wrapping_add(c);
    }
    if let Some(s) = state.get_mut(3) {
        *s = s.wrapping_add(d);
    }
    if let Some(s) = state.get_mut(4) {
        *s = s.wrapping_add(e);
    }
    if let Some(s) = state.get_mut(5) {
        *s = s.wrapping_add(f);
    }
    if let Some(s) = state.get_mut(6) {
        *s = s.wrapping_add(g);
    }
    if let Some(s) = state.get_mut(7) {
        *s = s.wrapping_add(h);
    }
}

#[cfg(test)]
mod tests {
    use super::{sha256_hex, Sha256};

    #[test]
    fn test_empty() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_abc() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn finish_alias_matches_finalize_for_existing_streaming_callers() {
        let mut hasher = Sha256::new();
        hasher.update(b"abc");
        assert_eq!(
            hasher.finish(),
            [
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23, 0xb0,
                0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }

    #[test]
    fn incremental_hash_matches_one_shot_across_chunk_boundaries() {
        let data = (0..=255).cycle().take(4097).collect::<Vec<_>>();
        let expected = sha256_hex(&data);
        for chunk_size in [1, 55, 56, 63, 64, 65, 127, 1024] {
            let mut hasher = Sha256::new();
            for chunk in data.chunks(chunk_size) {
                hasher.update(chunk);
            }
            assert_eq!(hasher.finalize_hex(), expected, "chunk size {chunk_size}");
        }
    }

    #[test]
    fn incremental_hash_matches_multiblock_reference_vector() {
        let data = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        let expected = "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1";
        assert_eq!(sha256_hex(data), expected);

        let mut hasher = Sha256::new();
        for chunk in data.chunks(7) {
            hasher.update(chunk);
        }
        assert_eq!(hasher.finalize_hex(), expected);
    }
}
