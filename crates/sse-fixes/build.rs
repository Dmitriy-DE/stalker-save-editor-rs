//! Compress the game-fixes JSON into a bounded raw-DEFLATE asset for embedding.

use sse_codecs::deflate::{compress_raw, Level};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let manifest_dir = env_path("CARGO_MANIFEST_DIR")?;
    let output_dir = env_path("OUT_DIR")?;
    let source = manifest_dir.join("data/game-fixes.json");
    println!("cargo:rerun-if-changed={}", source.display());

    let contents = fs::read(&source)?;
    let length = u32::try_from(contents.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "game fixes JSON exceeds u32 length"))?;
    let compressed = compress_raw(&contents, Level::Fast)
        .map_err(|error| io::Error::other(format!("compressing {}: {error}", source.display())))?;
    let filename = source
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "game fixes JSON has no filename"))?;
    let destination = output_dir.join(format!("data_{filename}.deflate"));
    let mut asset = Vec::with_capacity(4_usize.saturating_add(compressed.len()));
    asset.extend_from_slice(&length.to_le_bytes());
    asset.extend_from_slice(&compressed);
    fs::write(destination, asset)
}

fn env_path(key: &str) -> io::Result<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("missing {key}")))
}
