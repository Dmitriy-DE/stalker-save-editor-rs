//! Test opening a 1 GiB synthetic archive under 20 MiB RSS limit.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use sse_codecs::crc32::crc32;
use sse_content::XRayArchive;

struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("se-1gib-{name}-{}", std::process::id()));
        let _ = fs::remove_file(&path);
        Self { path }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn get_process_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let statm = fs::read_to_string("/proc/self/statm").ok()?;
        let parts: Vec<&str> = statm.split_whitespace().collect();
        let resident_pages: u64 = parts.get(1)?.parse().ok()?;
        let page_size = 4096u64;
        Some(resident_pages * page_size)
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

#[test]
fn opening_1_gib_archive_keeps_memory_under_20_mib() {
    let temp = TempFile::new("sparse.db");
    let file_size: u64 = 1024 * 1024 * 1024; // 1 GiB

    // Create a 1 GiB sparse file
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temp.path)
        .unwrap();

    file.set_len(file_size).unwrap();

    // Prepare header entry
    let name = b"entry.txt";
    let name_size = (16 + name.len()) as u16;
    let data_len = 4u32;
    let payload = b"ok!!";
    let crc = crc32(payload);
    let offset = 500 * 1024 * 1024u32; // Offset far in the middle of 1 GiB

    let mut header = Vec::new();
    header.extend_from_slice(&name_size.to_le_bytes());
    header.extend_from_slice(&data_len.to_le_bytes());
    header.extend_from_slice(&data_len.to_le_bytes());
    header.extend_from_slice(&crc.to_le_bytes());
    header.extend_from_slice(name);
    header.extend_from_slice(&offset.to_le_bytes());

    // Write chunk 1 (Header) and chunk 0 (Data) at the start
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&1u32.to_le_bytes()); // Chunk 1: Header
    prefix.extend_from_slice(&(header.len() as u32).to_le_bytes());
    prefix.extend_from_slice(&header);

    let data_chunk_size = u32::try_from(file_size - prefix.len() as u64 - 8).unwrap();
    prefix.extend_from_slice(&0u32.to_le_bytes()); // Chunk 0: Data
    prefix.extend_from_slice(&data_chunk_size.to_le_bytes());

    file.write_all(&prefix).unwrap();

    // Write the entry payload at `offset`
    use std::io::Seek;
    file.seek(std::io::SeekFrom::Start(offset as u64)).unwrap();
    file.write_all(payload).unwrap();
    file.flush().unwrap();
    drop(file);

    // Open the 1 GiB archive via positional reads
    let t0 = std::time::Instant::now();
    let archive = XRayArchive::open(&temp.path).unwrap();
    let open_duration = t0.elapsed();
    assert_eq!(archive.entries().len(), 1);
    assert_eq!(archive.entries()[0].name, "entry.txt");

    // Read the single 4-byte entry
    let t1 = std::time::Instant::now();
    let data = archive.read_file("entry.txt").unwrap();
    let read_duration = t1.elapsed();
    assert_eq!(data, b"ok!!");

    // Measure memory usage
    let rss_bytes = get_process_rss_bytes().unwrap_or(0);
    println!(
        "1 GiB archive metrics: index open time = {:?}, read entry time = {:?}, process RSS = {:.2} MiB ({} bytes)",
        open_duration,
        read_duration,
        rss_bytes as f64 / (1024.0 * 1024.0),
        rss_bytes
    );

    if let Some(rss) = get_process_rss_bytes() {
        const TWENTY_MEBIBYTES: u64 = 20 * 1024 * 1024;
        assert!(rss < TWENTY_MEBIBYTES, "Process RSS exceeds 20 MiB: {} bytes", rss);
    }
}
