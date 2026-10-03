//! Offline packer for icon atlas assets.

use sse_content::atlas::AtlasBuilder;
use sse_content::dds::RgbaImage;
use std::fs;
use std::path::{Path, PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let icons_dir = Path::new("/home/dmytro/Projects/save-editor-next/src/StalkerSaveEditor.Desktop/Assets/Icons");
    if !icons_dir.is_dir() {
        eprintln!("Icons directory not found: {}", icons_dir.display());
        return Ok(());
    }

    let mut icon_pairs: Vec<(String, RgbaImage)> = Vec::new();
    for sub in &["xray", "s2"] {
        let dir = icons_dir.join(sub);
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|ext| ext == "png") {
                    if let Some(file_name) = p.file_name().and_then(|n| n.to_str()) {
                        let name = format!("{sub}/{file_name}");
                        let bytes = fs::read(&p)?;
                        let decoded = sse_codecs::png::decode(&bytes)?;
                        let w = usize::try_from(decoded.width)?;
                        let h = usize::try_from(decoded.height)?;
                        let rgba = RgbaImage::new(w, h, decoded.pixels);
                        icon_pairs.push((name, rgba));
                    }
                }
            }
        }
    }

    println!("Read and decoded {} icon PNGs", icon_pairs.len());

    let builder = AtlasBuilder::default_2048();
    let atlas_bytes = builder.build(icon_pairs);

    let out_path = PathBuf::from("crates/sse-content/data/icons.atlas");
    fs::write(&out_path, &atlas_bytes)?;

    let len_mib = (atlas_bytes.len() as f64) / (1024.0 * 1024.0);
    println!(
        "Successfully wrote {} ({:.2} MiB, target <= 3.5 MiB)",
        out_path.display(),
        len_mib
    );
    Ok(())
}
