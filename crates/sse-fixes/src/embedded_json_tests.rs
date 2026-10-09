use sse_codecs::embedded_json::decode_json_asset;
use std::fs;
use std::io;
use std::path::Path;

#[test]
fn embedded_game_fixes_asset_inflates_to_its_source_json() -> io::Result<()> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let output_dir = Path::new(env!("OUT_DIR"));
    let source_path = manifest_dir.join("data/game-fixes.json");
    let source = fs::read(&source_path)?;
    let asset_path = output_dir.join("data_game-fixes.json.deflate");
    let asset = fs::read(asset_path)?;
    let decoded = decode_json_asset(&asset).map_err(|error| io::Error::other(error.to_string()))?;

    assert_eq!(decoded, source);
    assert!(asset.len() < source.len().saturating_add(4));
    Ok(())
}
