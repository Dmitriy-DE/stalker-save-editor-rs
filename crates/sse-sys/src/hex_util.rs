//! Hex digit parsing shared by the Linux picker dialogs.

/// Value of one ASCII hex digit, or `None` for any other byte.
#[cfg(target_os = "linux")]
pub(crate) fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => byte.checked_sub(b'0'),
        b'a'..=b'f' => byte.checked_sub(b'a').and_then(|value| value.checked_add(10)),
        b'A'..=b'F' => byte.checked_sub(b'A').and_then(|value| value.checked_add(10)),
        _ => None,
    }
}
