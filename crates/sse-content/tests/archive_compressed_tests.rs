//! X-Ray archives with a compressed file table and LZO entries (synthetic, no game files).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::arithmetic_side_effects)]

use sse_codecs::crc32::crc32;
use sse_content::{xray_entry_decoder, xray_header_decoder, XRayArchive};

/// Builds `[data chunk][table chunk]` where the single entry is stored as `stored` at offset 8.
fn archive_with_entry(name: &[u8], original: &[u8], stored: &[u8], table_flag: u32) -> Vec<u8> {
    let mut table = Vec::new();
    table.extend_from_slice(&u16::try_from(16 + name.len()).unwrap().to_le_bytes());
    table.extend_from_slice(&u32::try_from(original.len()).unwrap().to_le_bytes());
    table.extend_from_slice(&u32::try_from(stored.len()).unwrap().to_le_bytes());
    table.extend_from_slice(&crc32(original).to_le_bytes());
    table.extend_from_slice(name);
    table.extend_from_slice(&8u32.to_le_bytes());

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(stored.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(stored);
    bytes.extend_from_slice(&table_flag.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(table.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&table);
    bytes
}

#[test]
fn reads_lzo_entry_when_the_entry_decoder_is_given() {
    let original = b"configs/text/rus/st_items.xml ".repeat(40);
    let stored = sse_codecs::lzo1x::compress_fast(&original);
    assert!(stored.len() < original.len());
    let bytes = archive_with_entry(b"gamedata/config/text.ltx", &original, &stored, 1);

    let archive = XRayArchive::from_slice(&bytes, None, Some(xray_entry_decoder())).unwrap();
    assert_eq!(archive.read_file("gamedata/config/text.ltx").unwrap(), original);
}

#[test]
fn refuses_lzo_entry_without_an_entry_decoder() {
    let original = b"abcabcabc".repeat(30);
    let stored = sse_codecs::lzo1x::compress_fast(&original);
    let bytes = archive_with_entry(b"gamedata/config/text.ltx", &original, &stored, 1);

    let archive = XRayArchive::from_slice(&bytes, None, None).unwrap();
    let error = archive.read_file("gamedata/config/text.ltx").unwrap_err();
    assert!(format!("{error:?}").contains("no LZO decoder"));
}

#[test]
fn compressed_file_table_without_a_decoder_is_refused() {
    let original = b"payload";
    let bytes = archive_with_entry(b"gamedata/a.ltx", original, original, 0x8000_0001);

    let Err(error) = XRayArchive::from_slice(&bytes, None, None) else {
        panic!("compressed table must be refused without a decoder");
    };
    assert!(format!("{error:?}").contains("no decoder provided"));
}

#[test]
fn compressed_file_table_that_is_not_lzhuf_is_refused_without_panic() {
    let original = b"payload";
    let bytes = archive_with_entry(b"gamedata/a.ltx", original, original, 0x8000_0001);

    assert!(XRayArchive::from_slice(&bytes, Some(xray_header_decoder()), None).is_err());
}
