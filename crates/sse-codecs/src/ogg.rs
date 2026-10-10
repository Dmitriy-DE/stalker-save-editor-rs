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

/// MSB-first CRC-32 table for Ogg (polynomial `0x04C11DB7`, no reflection, no final XOR).
const OGG_TABLE: [u32; 256] = [
    0x00000000, 0x04C11DB7, 0x09823B6E, 0x0D4326D9, 0x130476DC, 0x17C56B6B, 0x1A864DB2, 0x1E475005, 0x2608EDB8,
    0x22C9F00F, 0x2F8AD6D6, 0x2B4BCB61, 0x350C9B64, 0x31CD86D3, 0x3C8EA00A, 0x384FBDBD, 0x4C11DB70, 0x48D0C6C7,
    0x4593E01E, 0x4152FDA9, 0x5F15ADAC, 0x5BD4B01B, 0x569796C2, 0x52568B75, 0x6A1936C8, 0x6ED82B7F, 0x639B0DA6,
    0x675A1011, 0x791D4014, 0x7DDC5DA3, 0x709F7B7A, 0x745E66CD, 0x9823B6E0, 0x9CE2AB57, 0x91A18D8E, 0x95609039,
    0x8B27C03C, 0x8FE6DD8B, 0x82A5FB52, 0x8664E6E5, 0xBE2B5B58, 0xBAEA46EF, 0xB7A96036, 0xB3687D81, 0xAD2F2D84,
    0xA9EE3033, 0xA4AD16EA, 0xA06C0B5D, 0xD4326D90, 0xD0F37027, 0xDDB056FE, 0xD9714B49, 0xC7361B4C, 0xC3F706FB,
    0xCEB42022, 0xCA753D95, 0xF23A8028, 0xF6FB9D9F, 0xFBB8BB46, 0xFF79A6F1, 0xE13EF6F4, 0xE5FFEB43, 0xE8BCCD9A,
    0xEC7DD02D, 0x34867077, 0x30476DC0, 0x3D044B19, 0x39C556AE, 0x278206AB, 0x23431B1C, 0x2E003DC5, 0x2AC12072,
    0x128E9DCF, 0x164F8078, 0x1B0CA6A1, 0x1FCDBB16, 0x018AEB13, 0x054BF6A4, 0x0808D07D, 0x0CC9CDCA, 0x7897AB07,
    0x7C56B6B0, 0x71159069, 0x75D48DDE, 0x6B93DDDB, 0x6F52C06C, 0x6211E6B5, 0x66D0FB02, 0x5E9F46BF, 0x5A5E5B08,
    0x571D7DD1, 0x53DC6066, 0x4D9B3063, 0x495A2DD4, 0x44190B0D, 0x40D816BA, 0xACA5C697, 0xA864DB20, 0xA527FDF9,
    0xA1E6E04E, 0xBFA1B04B, 0xBB60ADFC, 0xB6238B25, 0xB2E29692, 0x8AAD2B2F, 0x8E6C3698, 0x832F1041, 0x87EE0DF6,
    0x99A95DF3, 0x9D684044, 0x902B669D, 0x94EA7B2A, 0xE0B41DE7, 0xE4750050, 0xE9362689, 0xEDF73B3E, 0xF3B06B3B,
    0xF771768C, 0xFA325055, 0xFEF34DE2, 0xC6BCF05F, 0xC27DEDE8, 0xCF3ECB31, 0xCBFFD686, 0xD5B88683, 0xD1799B34,
    0xDC3ABDED, 0xD8FBA05A, 0x690CE0EE, 0x6DCDFD59, 0x608EDB80, 0x644FC637, 0x7A089632, 0x7EC98B85, 0x738AAD5C,
    0x774BB0EB, 0x4F040D56, 0x4BC510E1, 0x46863638, 0x42472B8F, 0x5C007B8A, 0x58C1663D, 0x558240E4, 0x51435D53,
    0x251D3B9E, 0x21DC2629, 0x2C9F00F0, 0x285E1D47, 0x36194D42, 0x32D850F5, 0x3F9B762C, 0x3B5A6B9B, 0x0315D626,
    0x07D4CB91, 0x0A97ED48, 0x0E56F0FF, 0x1011A0FA, 0x14D0BD4D, 0x19939B94, 0x1D528623, 0xF12F560E, 0xF5EE4BB9,
    0xF8AD6D60, 0xFC6C70D7, 0xE22B20D2, 0xE6EA3D65, 0xEBA91BBC, 0xEF68060B, 0xD727BBB6, 0xD3E6A601, 0xDEA580D8,
    0xDA649D6F, 0xC423CD6A, 0xC0E2D0DD, 0xCDA1F604, 0xC960EBB3, 0xBD3E8D7E, 0xB9FF90C9, 0xB4BCB610, 0xB07DABA7,
    0xAE3AFBA2, 0xAAFBE615, 0xA7B8C0CC, 0xA379DD7B, 0x9B3660C6, 0x9FF77D71, 0x92B45BA8, 0x9675461F, 0x8832161A,
    0x8CF30BAD, 0x81B02D74, 0x857130C3, 0x5D8A9099, 0x594B8D2E, 0x5408ABF7, 0x50C9B640, 0x4E8EE645, 0x4A4FFBF2,
    0x470CDD2B, 0x43CDC09C, 0x7B827D21, 0x7F436096, 0x7200464F, 0x76C15BF8, 0x68860BFD, 0x6C47164A, 0x61043093,
    0x65C52D24, 0x119B4BE9, 0x155A565E, 0x18197087, 0x1CD86D30, 0x029F3D35, 0x065E2082, 0x0B1D065B, 0x0FDC1BEC,
    0x3793A651, 0x3352BBE6, 0x3E119D3F, 0x3AD08088, 0x2497D08D, 0x2056CD3A, 0x2D15EBE3, 0x29D4F654, 0xC5A92679,
    0xC1683BCE, 0xCC2B1D17, 0xC8EA00A0, 0xD6AD50A5, 0xD26C4D12, 0xDF2F6BCB, 0xDBEE767C, 0xE3A1CBC1, 0xE760D676,
    0xEA23F0AF, 0xEEE2ED18, 0xF0A5BD1D, 0xF464A0AA, 0xF9278673, 0xFDE69BC4, 0x89B8FD09, 0x8D79E0BE, 0x803AC667,
    0x84FBDBD0, 0x9ABC8BD5, 0x9E7D9662, 0x933EB0BB, 0x97FFAD0C, 0xAFB010B1, 0xAB710D06, 0xA6322BDF, 0xA2F33668,
    0xBCB4666D, 0xB8757BDA, 0xB5365D03, 0xB1F740B4,
];

/// Continues an Ogg CRC over `data` from state `value` (start with 0).
fn ogg_crc_update(mut value: u32, data: &[u8]) -> u32 {
    for byte in data.iter().copied() {
        let index = usize::from(u8::try_from((value >> 24) ^ u32::from(byte)).unwrap_or_default());
        let table = OGG_TABLE.get(index).copied().unwrap_or_default();
        value = value.checked_shl(8).unwrap_or_default() ^ table;
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
        // The CRC covers the page with its own 4-byte field zeroed; hash the parts around it instead of copying.
        let before_field = page.get(..22).ok_or_else(|| Error::damaged("short Ogg CRC field"))?;
        let after_field = page.get(26..).ok_or_else(|| Error::damaged("short Ogg CRC field"))?;
        let value = ogg_crc_update(0, before_field);
        let value = ogg_crc_update(value, &[0_u8; 4]);
        if ogg_crc_update(value, after_field) != expected_crc {
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

    // Two Ogg pages built with the framing rules (27-byte header, lacing table, CRC over the page with its
    // CRC field zeroed), CRC computed by a bitwise reference in Python's stdlib (generator outside the repo).
    const OGG_TWO_PAGES: &[u8] = &[
        0x4F, 0x67, 0x67, 0x53, 0x00, 0x02, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x34, 0x12, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0xFC, 0x5E, 0x73, 0xAC, 0x02, 0x05, 0x03, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x61, 0x62,
        0x63, 0x4F, 0x67, 0x67, 0x53, 0x00, 0x04, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x0C, 0x79, 0x3C, 0x08, 0x01, 0x02, 0x7A, 0x7A,
    ];
    const OGG_BAD_PAYLOAD: &[u8] = &[
        0x4F, 0x67, 0x67, 0x53, 0x00, 0x02, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x34, 0x12, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0xFC, 0x5E, 0x73, 0xAC, 0x02, 0x05, 0x03, 0x68, 0x65, 0x6C, 0x6C, 0x6F, 0x61, 0x62,
        0x62, 0x4F, 0x67, 0x67, 0x53, 0x00, 0x04, 0x63, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x34, 0x12, 0x00,
        0x00, 0x01, 0x00, 0x00, 0x00, 0x0C, 0x79, 0x3C, 0x08, 0x01, 0x02, 0x7A, 0x7A,
    ];

    #[test]
    fn two_hand_built_pages_yield_their_packets_in_order() {
        let packets = packets(OGG_TWO_PAGES).unwrap_or_else(|error| panic!("{error:?}"));
        let data: Vec<&[u8]> = packets.iter().map(|packet| packet.data.as_slice()).collect();
        assert_eq!(data, vec![b"hello".as_slice(), b"abc".as_slice(), b"zz".as_slice()]);
        let [first, second, third] = packets.as_slice() else {
            panic!("expected three packets, got {}", packets.len());
        };
        assert!(first.bos);
        assert!(third.eos);
        // Granule -1 (u64::MAX) on the first page means no packet ends on it, so no granule is reported.
        assert_eq!(second.granule, None);
        assert_eq!(third.granule, Some(99));
    }

    #[test]
    fn a_flipped_payload_bit_fails_the_page_crc() {
        assert!(matches!(packets(OGG_BAD_PAYLOAD), Err(Error::Damaged(_))));
    }

    #[test]
    fn crc_empty_is_zero() {
        assert_eq!(ogg_crc_update(0, b""), 0);
    }

    #[test]
    fn truncated_page_is_rejected() {
        assert!(packets(b"OggS").is_err());
    }
}

#[cfg(test)]
mod crc_tests {
    use super::ogg_crc_update;

    /// The original bit-by-bit algorithm, kept as the reference for the table version.
    fn bitwise(data: &[u8]) -> u32 {
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

    #[test]
    fn table_crc_matches_the_bitwise_reference_at_every_split() {
        let data: Vec<u8> = (0..1_000_u32)
            .map(|i| u8::try_from(i.wrapping_mul(97) % 256).unwrap_or(0))
            .collect();
        for len in [0_usize, 1, 3, 4, 26, 255, 1_000] {
            let prefix = data.get(..len).unwrap_or(&[]);
            assert_eq!(ogg_crc_update(0, prefix), bitwise(prefix), "length {len}");
            let (head, tail) = prefix.split_at(prefix.len() / 2);
            assert_eq!(
                ogg_crc_update(ogg_crc_update(0, head), tail),
                bitwise(prefix),
                "split of {len}"
            );
        }
    }
}
