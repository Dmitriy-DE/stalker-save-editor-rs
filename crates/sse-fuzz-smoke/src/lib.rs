//! Deterministic parser fuzz-smoke corpus used by CI.

#[cfg(test)]
mod tests {
    use sse_s2::S2Save;
    use sse_xray::Save;

    fn next(state: &mut u64) -> u64 {
        *state ^= state.wrapping_shl(13);
        *state ^= state.wrapping_shr(7);
        *state ^= state.wrapping_shl(17);
        *state
    }

    #[test]
    fn malformed_save_inputs_never_panic() {
        let mut state = 0x5eed_cafe_f00d_baad_u64;
        for case in 0..20_000_usize {
            let length = usize::try_from(next(&mut state) & 0x0fff).unwrap_or_default();
            let mut bytes = vec![0_u8; length];
            for byte in &mut bytes {
                *byte = u8::try_from(next(&mut state) & 0xff).unwrap_or_default();
            }
            // Mix pure random data with structured/truncated container prefixes.
            if case.checked_rem(4).unwrap_or_default() == 0 && bytes.len() >= 8 {
                let declared = u32::try_from(next(&mut state) & 0x00ff_ffff).unwrap_or_default();
                if let Some(prefix) = bytes.get_mut(..4) {
                    prefix.copy_from_slice(&declared.to_le_bytes());
                }
            }
            let _ = S2Save::from_bytes(&bytes);
            let _ = Save::read(&bytes);
        }
    }
}
