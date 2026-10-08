//! Strict RFC 4648 standard Base64 decoding.

use sse_core::{Error, Result};

/// Decodes standard Base64 with canonical terminal padding and no whitespace.
///
/// # Errors
/// Returns an error for empty input, invalid characters, malformed padding, or non-canonical
/// unused bits in the final quantum.
pub fn decode_standard(text: &str) -> Result<Vec<u8>> {
    decode_bytes(text.as_bytes())
}

/// Decodes standard Base64 after ignoring ASCII whitespace, as permitted in PEM-like inputs.
///
/// # Errors
/// Returns an error for empty input, invalid characters, malformed padding, or non-canonical
/// unused bits in the final quantum.
pub fn decode_standard_ignoring_ascii_whitespace(text: &str) -> Result<Vec<u8>> {
    let mut clean = Vec::with_capacity(text.len());
    for byte in text.bytes() {
        if !byte.is_ascii_whitespace() {
            clean.push(byte);
        }
    }
    decode_bytes(&clean)
}

fn decode_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.is_empty() || bytes.len().checked_rem(4) != Some(0) {
        return Err(Error::damaged("invalid base64 length"));
    }

    let capacity = bytes
        .len()
        .checked_div(4)
        .and_then(|quanta| quanta.checked_mul(3))
        .ok_or_else(|| Error::damaged("base64 output size overflow"))?;
    let mut output = Vec::with_capacity(capacity);
    let mut position = 0_usize;

    while position < bytes.len() {
        let a = base64_value(*bytes.get(position).ok_or_else(|| Error::damaged("base64 a"))?)?;
        let b_position = position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("base64 offset overflow"))?;
        let c_position = position
            .checked_add(2)
            .ok_or_else(|| Error::damaged("base64 offset overflow"))?;
        let d_position = position
            .checked_add(3)
            .ok_or_else(|| Error::damaged("base64 offset overflow"))?;
        let b = base64_value(*bytes.get(b_position).ok_or_else(|| Error::damaged("base64 b"))?)?;
        let c_byte = *bytes.get(c_position).ok_or_else(|| Error::damaged("base64 c"))?;
        let d_byte = *bytes.get(d_position).ok_or_else(|| Error::damaged("base64 d"))?;
        let final_quantum = d_position
            .checked_add(1)
            .ok_or_else(|| Error::damaged("base64 offset overflow"))?
            == bytes.len();

        let c = if c_byte == b'=' { 0 } else { base64_value(c_byte)? };
        let d = if d_byte == b'=' { 0 } else { base64_value(d_byte)? };
        output.push((a << 2) | (b >> 4));

        if c_byte == b'=' {
            if d_byte != b'=' || !final_quantum || b & 0x0F != 0 {
                return Err(Error::damaged("non-canonical base64 padding"));
            }
        } else {
            output.push((b << 4) | (c >> 2));
            if d_byte == b'=' {
                if !final_quantum || c & 0x03 != 0 {
                    return Err(Error::damaged("non-canonical base64 padding"));
                }
            } else {
                output.push((c << 6) | d);
            }
        }

        position = position
            .checked_add(4)
            .ok_or_else(|| Error::damaged("base64 offset overflow"))?;
    }

    Ok(output)
}

fn base64_value(byte: u8) -> Result<u8> {
    match byte {
        b'A'..=b'Z' => byte
            .checked_sub(b'A')
            .ok_or_else(|| Error::damaged("base64 uppercase underflow")),
        b'a'..=b'z' => byte
            .checked_sub(b'a')
            .and_then(|value| value.checked_add(26))
            .ok_or_else(|| Error::damaged("base64 lowercase overflow")),
        b'0'..=b'9' => byte
            .checked_sub(b'0')
            .and_then(|value| value.checked_add(52))
            .ok_or_else(|| Error::damaged("base64 digit overflow")),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(Error::damaged("invalid base64 character")),
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_standard, decode_standard_ignoring_ascii_whitespace};

    #[test]
    fn decodes_canonical_quanta_and_padding() {
        assert_eq!(decode_standard("AAAA"), Ok(vec![0, 0, 0]));
        assert_eq!(decode_standard("AAA="), Ok(vec![0, 0]));
        assert_eq!(decode_standard("AA=="), Ok(vec![0]));
    }

    #[test]
    fn rejects_noncanonical_or_misplaced_padding() {
        for malformed in ["", "A===", "AA=A", "AA==AAAA", "AAB=", "AA== "] {
            assert!(decode_standard(malformed).is_err(), "accepted {malformed:?}");
        }
    }

    #[test]
    fn whitespace_is_ignored_only_by_the_explicit_pem_decoder() {
        assert_eq!(decode_standard_ignoring_ascii_whitespace("A A\n==\t"), Ok(vec![0]));
        assert!(decode_standard("AA\n==").is_err());
    }
}
