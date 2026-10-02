//! Hostile input and truncation tests across all parsers.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use sse_content::dds::DdsImage;
use sse_content::ltx::LtxDocument;
use sse_content::string_tables::XRayStringTables;
use sse_content::XRayArchive;
use sse_core::Error;

#[test]
fn archive_hostile_chunk_and_entry_sizes_are_rejected() {
    // 1. Chunk claiming 2 GiB header size
    let mut huge_chunk_hdr = Vec::new();
    huge_chunk_hdr.extend_from_slice(&1u32.to_le_bytes()); // header chunk
    huge_chunk_hdr.extend_from_slice(&0x7FFF_FFFFu32.to_le_bytes()); // 2 GiB size
    huge_chunk_hdr.extend_from_slice(&[0u8; 100]); // truncated payload
    assert!(matches!(
        XRayArchive::from_slice(&huge_chunk_hdr, None, None),
        Err(Error::Damaged(_))
    ));

    // 2. Archive with entry claiming 4 GiB uncompressed size
    let name = b"huge.bin";
    let mut header = Vec::new();
    header.extend_from_slice(&(16u16 + name.len() as u16).to_le_bytes());
    header.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes()); // 4 GiB uncompressed!
    header.extend_from_slice(&4u32.to_le_bytes()); // 4 bytes comp
    header.extend_from_slice(&0u32.to_le_bytes()); // crc
    header.extend_from_slice(name);
    header.extend_from_slice(&8u32.to_le_bytes()); // offset = 8

    let mut arc = Vec::new();
    // Chunk 0: Data
    arc.extend_from_slice(&0u32.to_le_bytes());
    arc.extend_from_slice(&4u32.to_le_bytes());
    arc.extend_from_slice(b"test");
    // Chunk 1: Header
    arc.extend_from_slice(&1u32.to_le_bytes());
    arc.extend_from_slice(&(header.len() as u32).to_le_bytes());
    arc.extend_from_slice(&header);

    let parsed = XRayArchive::from_slice(&arc, None, None).unwrap();
    // Attempting to read entry must fail size limit check without allocating 4 GiB
    assert!(matches!(parsed.read_file("huge.bin"), Err(Error::Damaged(_))));
}

#[test]
fn dds_hostile_dimensions_and_payloads_are_rejected() {
    let mut data = vec![0u8; 128];
    data[0..4].copy_from_slice(b"DDS ");
    // 100,000 x 100,000 = 10 billion pixels
    data[12..16].copy_from_slice(&100_000u32.to_le_bytes());
    data[16..20].copy_from_slice(&100_000u32.to_le_bytes());
    data[80..84].copy_from_slice(&0x40u32.to_le_bytes()); // uncompressed

    assert!(matches!(DdsImage::decode(&data), Err(Error::Damaged(_))));

    // Zero dimensions
    data[12..16].copy_from_slice(&0u32.to_le_bytes());
    assert!(matches!(DdsImage::decode(&data), Err(Error::Damaged(_))));
}

#[test]
fn ltx_handles_deep_inheritance_cycles_and_huge_lines() {
    let cycle_text = "[a]:b\nx=1\n[b]:c\nx=2\n[c]:a\nx=3\n";
    let sections = LtxDocument::parse(cycle_text, "cycle.ltx");
    let resolved = LtxDocument::resolve(&sections);
    assert_eq!(resolved.len(), 3);

    // Huge line without closing bracket
    let mut huge = "[incomplete".to_string();
    huge.push_str(&"a".repeat(100_000));
    let parsed = LtxDocument::parse(&huge, "huge.ltx");
    assert!(parsed.is_empty());
}

#[test]
fn string_tables_handles_unterminated_xml_gracefully() {
    let text = "<string_table><string id=\"unterminated\"><text>no close tag";
    let files = [("test.xml", text.as_bytes())];
    let res = XRayStringTables::read(&files, |f| f.0, |f| Some(f.1.to_vec()), "ru");
    assert!(res.is_empty());
}
