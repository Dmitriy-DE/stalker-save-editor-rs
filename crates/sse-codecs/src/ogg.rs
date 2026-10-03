//! Ogg page framing and packet reconstruction with CRC verification.

use sse_core::{Error, Result};

/// One reconstructed Ogg packet.
#[derive(Clone, Debug)]
pub struct Packet {
    /// Packet payload without Ogg lacing.
    pub data: Vec<u8>,
    /// Page granule position when this packet is the last completed packet on its page.
    pub granule: Option<u64>,
    /// Whether the packet starts the logical stream.
    pub bos: bool,
    /// Whether the packet ends the logical stream.
    pub eos: bool,
}

fn crc(data: &[u8]) -> u32 {
    let mut value = 0_u32;
    for byte in data {
        value ^= u32::from(*byte).wrapping_shl(24);
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 {
                value.wrapping_shl(1) ^ 0x04c1_1db7
            } else {
                value.wrapping_shl(1)
            };
        }
    }
    value
}

fn le32(bytes: &[u8]) -> Result<u32> {
    Ok(u32::from_le_bytes(
        <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("short Ogg u32"))?,
    ))
}

fn le64(bytes: &[u8]) -> Result<u64> {
    Ok(u64::from_le_bytes(
        <[u8; 8]>::try_from(bytes).map_err(|_| Error::damaged("short Ogg u64"))?,
    ))
}

/// Validates an Ogg logical stream and reconstructs its packets.
///
/// Chained logical streams are refused because Vorbis decoder state does not
/// carry across serial numbers.
pub fn packets(input: &[u8]) -> Result<Vec<Packet>> {
    let mut position = 0_usize;
    let mut serial = None;
    let mut sequence = 0_u32;
    let mut pending = Vec::new();
    let mut output = Vec::new();

    while position < input.len() {
        let header_end = position.saturating_add(27);
        let header = input
            .get(position..header_end)
            .ok_or_else(|| Error::damaged("truncated Ogg page"))?;
        if header.get(..4) != Some(b"OggS") || header.get(4) != Some(&0) {
            return Err(Error::damaged("invalid Ogg capture/version"));
        }
        let flags = header.get(5).copied().ok_or_else(|| Error::damaged("Ogg flags"))?;
        let granule = le64(header.get(6..14).ok_or_else(|| Error::damaged("Ogg granule"))?)?;
        let page_serial = le32(header.get(14..18).ok_or_else(|| Error::damaged("Ogg serial"))?)?;
        let page_sequence = le32(header.get(18..22).ok_or_else(|| Error::damaged("Ogg sequence"))?)?;
        let expected_crc = le32(header.get(22..26).ok_or_else(|| Error::damaged("Ogg CRC"))?)?;

        if let Some(first) = serial {
            if first != page_serial {
                return Err(Error::Refused("chained Ogg streams are unsupported".to_owned()));
            }
        } else {
            serial = Some(page_serial);
        }
        if page_sequence != sequence {
            return Err(Error::damaged("Ogg page sequence gap"));
        }
        sequence = sequence.saturating_add(1);

        let segment_count = usize::from(
            header
                .get(26)
                .copied()
                .ok_or_else(|| Error::damaged("Ogg segment count"))?,
        );
        let lacing_start = header_end;
        let lacing_end = lacing_start.saturating_add(segment_count);
        let lacing = input
            .get(lacing_start..lacing_end)
            .ok_or_else(|| Error::damaged("truncated Ogg lacing"))?;
        let body_len = lacing
            .iter()
            .fold(0_usize, |sum, item| sum.saturating_add(usize::from(*item)));
        let page_end = lacing_end
            .checked_add(body_len)
            .ok_or_else(|| Error::damaged("Ogg page size overflow"))?;
        let page = input
            .get(position..page_end)
            .ok_or_else(|| Error::damaged("truncated Ogg body"))?;
        let mut checked = page.to_vec();
        checked
            .get_mut(22..26)
            .ok_or_else(|| Error::damaged("short Ogg CRC field"))?
            .fill(0);
        if crc(&checked) != expected_crc {
            return Err(Error::damaged("Ogg CRC mismatch"));
        }

        if flags & 1 == 0 && !pending.is_empty() {
            return Err(Error::damaged("Ogg continuation flag missing"));
        }
        if flags & 1 != 0 && pending.is_empty() {
            return Err(Error::damaged("unexpected Ogg continuation"));
        }

        let mut body = lacing_end;
        for (index, lace) in lacing.iter().copied().enumerate() {
            let length = usize::from(lace);
            let next = body
                .checked_add(length)
                .ok_or_else(|| Error::damaged("Ogg segment overflow"))?;
            pending.extend_from_slice(
                input
                    .get(body..next)
                    .ok_or_else(|| Error::damaged("truncated Ogg segment"))?,
            );
            body = next;
            if length < 255 {
                let last_on_page = index.saturating_add(1) == segment_count;
                output.push(Packet {
                    data: std::mem::take(&mut pending),
                    granule: (last_on_page && granule != u64::MAX).then_some(granule),
                    bos: flags & 2 != 0 && output.is_empty(),
                    eos: flags & 4 != 0 && last_on_page,
                });
            }
        }
        position = page_end;
    }

    if !pending.is_empty() {
        return Err(Error::damaged("truncated Ogg packet"));
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_empty_is_zero() {
        assert_eq!(crc(b""), 0);
    }

    #[test]
    fn truncated_page_is_rejected() {
        assert!(packets(b"OggS").is_err());
    }
}
