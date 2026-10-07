//! Compress catalog JSON into bounded raw-DEFLATE assets for embedding.

use sse_codecs::deflate::{compress_raw, Level};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let manifest_dir = env_path("CARGO_MANIFEST_DIR")?;
    let output_dir = env_path("OUT_DIR")?;

    for directory_name in ["data", "i18n"] {
        let directory = manifest_dir.join(directory_name);
        println!("cargo:rerun-if-changed={}", directory.display());
        let mut sources = fs::read_dir(&directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<PathBuf>>>()?;
        sources.sort();

        for source in sources {
            if source.extension().and_then(OsStr::to_str) != Some("json") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", source.display());
            let contents = fs::read(&source)?;
            let length = u32::try_from(contents.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "JSON asset exceeds u32 length"))?;
            let compressed = compress_raw(&contents, Level::Fast)
                .map_err(|error| io::Error::other(format!("compressing {}: {error}", source.display())))?;
            let parent = source
                .parent()
                .and_then(Path::file_name)
                .and_then(OsStr::to_str)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSON asset has no parent"))?;
            let filename = source
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSON asset has no filename"))?;
            let destination = output_dir.join(format!("{parent}_{filename}.deflate"));
            let mut asset = Vec::with_capacity(4_usize.saturating_add(compressed.len()));
            asset.extend_from_slice(&length.to_le_bytes());
            asset.extend_from_slice(&compressed);
            fs::write(destination, asset)?;
        }
    }

    Ok(())
}

fn env_path(key: &str) -> io::Result<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("missing {key}")))
}
