use crate::embedded_json::decode_json_asset;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[test]
fn every_embedded_catalog_asset_inflates_to_its_source_json() -> io::Result<()> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = Path::new(env!("OUT_DIR"));

    for directory_name in ["data", "i18n"] {
        let source_directory = manifest_dir.join(directory_name);
        let mut sources = fs::read_dir(&source_directory)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<io::Result<Vec<PathBuf>>>()?;
        sources.sort();

        for source_path in sources {
            if source_path.extension().and_then(OsStr::to_str) != Some("json") {
                continue;
            }
            let source = fs::read(&source_path)?;
            let parent = source_path
                .parent()
                .and_then(Path::file_name)
                .and_then(OsStr::to_str)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSON asset has no parent"))?;
            let filename = source_path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "JSON asset has no filename"))?;
            let asset_path = output_dir.join(format!("{parent}_{filename}.deflate"));
            let asset = fs::read(asset_path)?;
            let decoded = decode_json_asset(&asset).map_err(|error| io::Error::other(error.to_string()))?;

            assert_eq!(
                decoded,
                source,
                "{} differs from its source JSON",
                source_path.display()
            );
            assert!(
                asset.len() < source.len().saturating_add(4),
                "{} did not compress",
                source_path.display()
            );
        }
    }

    Ok(())
}
