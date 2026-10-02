//! Archive reader tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use sse_content::crc32;
use sse_content::encoding::decode_windows_1251;
use sse_content::XRayArchive;
use sse_core::Error;

fn fixture_path(relative: &str) -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("fixtures")
        .join(relative)
}

struct ArchiveFile {
    name: String,
    data: Vec<u8>,
    name_bytes: Option<Vec<u8>>,
}

fn build_uncompressed_archive(
    files: &[ArchiveFile],
    with_metadata: bool,
    corrupt_first_crc: bool,
    invalid_first_offset: bool,
    data_chunk_type: u32,
) -> Vec<u8> {
    let mut header_entries = Vec::new();
    let mut data_body = Vec::new();

    let metadata_len = if with_metadata {
        8 + 43 // chunk header + "[header]\nentry_point = $fs_root$\\gamedata\\\n"
    } else {
        0
    };

    let mut header_size = 0usize;
    for file in files {
        let n_len = file.name_bytes.as_ref().map_or_else(|| file.name.len(), Vec::len);
        header_size = header_size.saturating_add(14).saturating_add(n_len).saturating_add(4);
    }

    let data_start = (metadata_len + 8 + header_size + 8) as u32;

    for (index, file) in files.iter().enumerate() {
        let name_bytes = file.name_bytes.clone().unwrap_or_else(|| file.name.as_bytes().to_vec());
        let name_size = u16::try_from(16 + name_bytes.len()).unwrap();
        let uncompressed_size = u32::try_from(file.data.len()).unwrap();
        let compressed_size = uncompressed_size;
        let mut crc = crc32(&file.data);
        if corrupt_first_crc && index == 0 {
            crc ^= 1;
        }

        header_entries.extend_from_slice(&name_size.to_le_bytes());
        header_entries.extend_from_slice(&uncompressed_size.to_le_bytes());
        header_entries.extend_from_slice(&compressed_size.to_le_bytes());
        header_entries.extend_from_slice(&crc.to_le_bytes());
        header_entries.extend_from_slice(&name_bytes);

        let mut offset = data_start.wrapping_add(u32::try_from(data_body.len()).unwrap());
        if invalid_first_offset && index == 0 {
            offset = offset.wrapping_add(100_000);
        }
        header_entries.extend_from_slice(&offset.to_le_bytes());

        data_body.extend_from_slice(&file.data);
    }

    let mut result = Vec::new();
    if with_metadata {
        let meta_body = b"[header]\nentry_point = $fs_root$\\gamedata\\\n";
        result.extend_from_slice(&666u32.to_le_bytes());
        result.extend_from_slice(&(meta_body.len() as u32).to_le_bytes());
        result.extend_from_slice(meta_body);
    }

    // Chunk 1: Header
    result.extend_from_slice(&1u32.to_le_bytes());
    result.extend_from_slice(&(header_entries.len() as u32).to_le_bytes());
    result.extend_from_slice(&header_entries);

    // Chunk 0 / data_chunk_type: Data
    result.extend_from_slice(&data_chunk_type.to_le_bytes());
    result.extend_from_slice(&(data_body.len() as u32).to_le_bytes());
    result.extend_from_slice(&data_body);

    result
}

#[test]
fn lists_entries_and_reads_uncompressed_files_by_name() {
    let config = b"[items]\nname = fixture\n".to_vec();
    let texture: Vec<u8> = (0..160u8).collect();

    let archive_bytes = build_uncompressed_archive(
        &[
            ArchiveFile {
                name: "gamedata/config/items.ltx".to_string(),
                data: config.clone(),
                name_bytes: None,
            },
            ArchiveFile {
                name: "gamedata\\textures\\ui\\icon.dds".to_string(),
                data: texture.clone(),
                name_bytes: None,
            },
        ],
        true,
        false,
        false,
        0,
    );

    let archive = XRayArchive::from_slice(&archive_bytes, None, None).unwrap();

    let entry_names: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(
        entry_names,
        vec!["gamedata/config/items.ltx", "gamedata/textures/ui/icon.dds"]
    );

    assert_eq!(archive.read_file("GAMEDATA\\CONFIG\\ITEMS.LTX").unwrap(), config);
    assert_eq!(archive.read_file("gamedata/textures/ui/icon.dds").unwrap(), texture);
    assert!(matches!(archive.read_file("missing.ltx"), Err(Error::Damaged(_))));
}

#[test]
fn reads_the_synthetic_archive_fixture_verified_by_the_python_oracle() {
    let dir = fixture_path("synthetic/xray-archive");
    let archive_path = dir.join("synthetic.db");

    let archive = XRayArchive::open(&archive_path).unwrap();

    struct ExpectedEntry {
        name: &'static str,
        data_file: &'static str,
        size: usize,
        sha256: &'static str,
    }

    let expected_entries = [
        ExpectedEntry {
            name: "gamedata/config/items.ltx",
            data_file: "entry-00.bin",
            size: 49,
            sha256: "ae007498a1a3a1035531118a527d1584dd9c42103fb6906094ca9fc641bc6215",
        },
        ExpectedEntry {
            name: "gamedata/configs/text/eng/items.xml",
            data_file: "entry-01.bin",
            size: 111,
            sha256: "e5f94786d47ce22ddbcbda107a0b441107993a97ded68ac178101e5dbd9db7c5",
        },
        ExpectedEntry {
            name: "gamedata/textures/ui/icon.dds",
            data_file: "entry-02.bin",
            size: 21,
            sha256: "4f28c4c3499347208a670e56a9bb127ec3ffbd2d543313f4d99bb456ad093224",
        },
    ];

    let expected_names: Vec<&str> = expected_entries.iter().map(|e| e.name).collect();
    let actual_names: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
    assert_eq!(actual_names, expected_names);

    for entry in &expected_entries {
        let expected_bytes = fs::read(dir.join(entry.data_file)).unwrap();
        let actual_bytes = archive.read_file(entry.name).unwrap();

        assert_eq!(actual_bytes.len(), entry.size);
        assert_eq!(actual_bytes, expected_bytes);

        let actual_sha = sse_content::sha256::sha256_hex(&actual_bytes);
        assert_eq!(actual_sha, entry.sha256);
    }
}

#[test]
fn reads_a_cp1251_file_name() {
    // Encode "gamedata/config/предметы.ltx" in CP1251
    let mut name_bytes = b"gamedata/config/".to_vec();
    // 'п' (0xEF), 'р' (0xF0), 'е' (0xE5), 'д' (0xE4), 'м' (0xEC), 'е' (0xE5), 'т' (0xF2), 'ы' (0xFB)
    name_bytes.extend_from_slice(&[0xEF, 0xF0, 0xE5, 0xE4, 0xEC, 0xE5, 0xF2, 0xFB]);
    name_bytes.extend_from_slice(b".ltx");

    let data = b"[items]\nname = fixture\n".to_vec();
    let archive_bytes = build_uncompressed_archive(
        &[ArchiveFile {
            name: decode_windows_1251(&name_bytes),
            data: data.clone(),
            name_bytes: Some(name_bytes),
        }],
        false,
        false,
        false,
        0,
    );

    let archive = XRayArchive::from_slice(&archive_bytes, None, None).unwrap();
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries().first().unwrap().name, "gamedata/config/предметы.ltx");
    assert_eq!(archive.read_file("gamedata/config/предметы.ltx").unwrap(), data);
}

#[test]
fn reads_a_data_chunk_with_the_compressed_flag_set() {
    let data = b"archive body".to_vec();
    let archive_bytes = build_uncompressed_archive(
        &[ArchiveFile {
            name: "gamedata/config/flagged.ltx".to_string(),
            data: data.clone(),
            name_bytes: None,
        }],
        false,
        false,
        false,
        0x8000_0000,
    );

    let archive = XRayArchive::from_slice(&archive_bytes, None, None).unwrap();
    assert_eq!(archive.read_file("gamedata/config/flagged.ltx").unwrap(), data);
}

#[test]
fn reads_archive_with_custom_header_decoder() {
    // When chunk 1 has compressed flag (0x80000001), passes header to decoder
    let header_content = {
        let name = b"gamedata/config/compressed.ltx";
        let mut h = Vec::new();
        let name_size = u16::try_from(16 + name.len()).unwrap();
        h.extend_from_slice(&name_size.to_le_bytes());
        h.extend_from_slice(&4u32.to_le_bytes()); // uncomp
        h.extend_from_slice(&4u32.to_le_bytes()); // comp
        h.extend_from_slice(&crc32(b"test").to_le_bytes());
        h.extend_from_slice(name);
        h.extend_from_slice(&8u32.to_le_bytes()); // offset = 8 (data body start)
        h
    };

    let fake_compressed_header = b"COMPRESSED_HEADER_PAYLOAD";
    let mut archive_bytes = Vec::new();
    // Chunk 0: Data
    archive_bytes.extend_from_slice(&0u32.to_le_bytes());
    archive_bytes.extend_from_slice(&4u32.to_le_bytes());
    archive_bytes.extend_from_slice(b"test");

    // Chunk 1: Compressed Header
    archive_bytes.extend_from_slice(&0x8000_0001u32.to_le_bytes());
    archive_bytes.extend_from_slice(&(fake_compressed_header.len() as u32).to_le_bytes());
    archive_bytes.extend_from_slice(fake_compressed_header);

    let header_decoder = Arc::new(move |data: &[u8]| -> sse_core::Result<Vec<Vec<u8>>> {
        assert_eq!(data, b"COMPRESSED_HEADER_PAYLOAD");
        Ok(vec![header_content.clone()])
    });

    let archive = XRayArchive::from_slice(&archive_bytes, Some(header_decoder), None).unwrap();
    assert_eq!(archive.read_file("gamedata/config/compressed.ltx").unwrap(), b"test");
}

#[test]
fn rejects_a_truncated_archive_chunk() {
    let truncated = [1u8, 0, 0, 0, 16, 0, 0, 0, 1, 2];
    assert!(matches!(
        XRayArchive::from_slice(&truncated, None, None),
        Err(Error::Damaged(_))
    ));
}

#[test]
fn rejects_a_file_with_a_bad_crc() {
    let archive_bytes = build_uncompressed_archive(
        &[ArchiveFile {
            name: "gamedata/config/bad.ltx".to_string(),
            data: b"bad crc".to_vec(),
            name_bytes: None,
        }],
        false,
        true, // corrupt crc
        false,
        0,
    );

    let archive = XRayArchive::from_slice(&archive_bytes, None, None).unwrap();
    assert!(matches!(
        archive.read_file("gamedata/config/bad.ltx"),
        Err(Error::Damaged(_))
    ));
}

#[test]
fn rejects_an_entry_that_points_outside_the_data_chunk() {
    let archive_bytes = build_uncompressed_archive(
        &[ArchiveFile {
            name: "gamedata/config/outside.ltx".to_string(),
            data: b"outside".to_vec(),
            name_bytes: None,
        }],
        false,
        false,
        true, // invalid offset
        0,
    );

    assert!(matches!(
        XRayArchive::from_slice(&archive_bytes, None, None),
        Err(Error::Damaged(_))
    ));
}
