//! Reads every X-Ray archive under `SSE_GAME_DIRS` (colon-separated game folders, read-only) and checks
//! that each file table opens and each non-directory entry reads with its CRC32 verified.
//!
//! Known exception: vanilla Clear Sky `resources/resources.db2`, entry `textures/ui/ui_pda2_noice.dds`, fails
//! its CRC32 check in the installed copy. It is listed here and reported, not skipped silently.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use sse_content::{xray_entry_decoder, xray_header_decoder, EntryDecoder, XRayArchive};

const KNOWN_CRC_EXCEPTIONS: &[(&str, &str)] = &[("resources/resources.db2", "textures/ui/ui_pda2_noice.dds")];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else {
            let name = path.file_name().unwrap().to_string_lossy().to_ascii_lowercase();
            if name.contains(".db") || name.contains(".xdb") || name.ends_with(".xrp") {
                out.push(path);
            }
        }
    }
}

#[test]
#[ignore = "reads installed game archives from SSE_GAME_DIRS (read-only)"]
fn installed_archives_open_and_read() {
    let dirs = std::env::var("SSE_GAME_DIRS").expect("SSE_GAME_DIRS");
    let entry_decoder: EntryDecoder = xray_entry_decoder();
    let mut files = Vec::new();
    for dir in dirs.split(':') {
        walk(Path::new(dir), &mut files);
    }
    files.sort();
    assert!(!files.is_empty(), "no archives were found under SSE_GAME_DIRS");

    let mut opened_failures = Vec::new();
    let mut unexpected = Vec::new();
    let mut seen_exception = false;
    for path in &files {
        let archive =
            match XRayArchive::open_with_decoder(path, Some(xray_header_decoder()), Some(entry_decoder.clone())) {
                Ok(archive) => archive,
                Err(error) => {
                    opened_failures.push(format!("{}: {error:?}", path.display()));
                    continue;
                }
            };
        let archive_path = path.to_string_lossy().to_string();
        for entry in archive.entries().iter().filter(|e| !e.name.ends_with('/')) {
            if let Err(error) = archive.read_file(&entry.name) {
                let known = KNOWN_CRC_EXCEPTIONS
                    .iter()
                    .any(|(suffix, name)| archive_path.ends_with(suffix) && entry.name == *name);
                if known {
                    seen_exception = true;
                } else {
                    unexpected.push(format!("{}: {}: {error:?}", path.display(), entry.name));
                }
            }
        }
    }
    println!("ARCHIVES {} opened_failures={}", files.len(), opened_failures.len());
    assert!(
        opened_failures.is_empty(),
        "archives whose file table did not open: {opened_failures:?}"
    );
    assert!(unexpected.is_empty(), "entries that failed to read: {unexpected:?}");
    println!("KNOWN_CRC_EXCEPTION_SEEN={seen_exception}");
}
