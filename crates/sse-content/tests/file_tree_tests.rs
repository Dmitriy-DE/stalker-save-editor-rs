//! Game file tree and archive locator tests.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use std::fs;
use std::path::PathBuf;

use sse_codecs::crc32::crc32;
use sse_content::file_tree::{CompanionArchiveLocator, CompanionGame, GameFileTree};

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("se-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn build_simple_archive(name: &str, content: &[u8]) -> Vec<u8> {
    let name_bytes = name.as_bytes();
    let name_size = (16 + name_bytes.len()) as u16;
    let data_len = content.len() as u32;
    let crc = crc32(content);
    let offset = 8 + 14 + name_bytes.len() as u32 + 4 + 8; // chunk0 header(8) + chunk1 header(8) + entry(14+name+4) + data chunk header(8)

    let mut header = Vec::new();
    header.extend_from_slice(&name_size.to_le_bytes());
    header.extend_from_slice(&data_len.to_le_bytes());
    header.extend_from_slice(&data_len.to_le_bytes());
    header.extend_from_slice(&crc.to_le_bytes());
    header.extend_from_slice(name_bytes);
    header.extend_from_slice(&offset.to_le_bytes());

    let mut result = Vec::new();
    // Chunk 1: Header
    result.extend_from_slice(&1u32.to_le_bytes());
    result.extend_from_slice(&(header.len() as u32).to_le_bytes());
    result.extend_from_slice(&header);

    // Chunk 0: Data
    result.extend_from_slice(&0u32.to_le_bytes());
    result.extend_from_slice(&(content.len() as u32).to_le_bytes());
    result.extend_from_slice(content);

    result
}

#[test]
fn loads_archives_and_loose_overlay() {
    let temp = TempDir::new("file-tree");
    let root = &temp.path;

    // fsgame.ltx
    fs::write(
        root.join("fsgame.ltx"),
        "$game_data$ = false | true | $fs_root$ | gamedata\\\n\
         $game_config$ = true | false | $game_data$ | configs\\\n\
         $arch_dir$ = false | false | $fs_root$ | archives\\\n",
    )
    .unwrap();

    let archives_dir = root.join("archives");
    fs::create_dir_all(&archives_dir).unwrap();

    let arc_bytes = build_simple_archive("gamedata/configs/system.ltx", b"[system]\nversion = 1.0\n");
    fs::write(archives_dir.join("resources.db"), arc_bytes).unwrap();

    // First load from archive
    let tree = GameFileTree::load(
        CompanionGame::CallOfPripyat,
        root,
        |_| true,
        None,
        true,
        false,
        false,
        None,
        None,
    )
    .unwrap();

    assert_eq!(tree.config_prefix, "configs/");
    assert!(!tree.has_loose_overlay);
    assert!(tree.files.contains_key("configs/system.ltx"));
    let content = tree.files["configs/system.ltx"].read().unwrap();
    assert_eq!(content, b"[system]\nversion = 1.0\n");

    // Add loose overlay in gamedata
    let loose_dir = root.join("gamedata/configs");
    fs::create_dir_all(&loose_dir).unwrap();
    fs::write(loose_dir.join("system.ltx"), b"[system]\nversion = 2.0_mod\n").unwrap();

    let tree_with_overlay = GameFileTree::load(
        CompanionGame::CallOfPripyat,
        root,
        |_| true,
        None,
        true,
        false,
        false,
        None,
        None,
    )
    .unwrap();

    assert!(tree_with_overlay.has_loose_overlay);
    assert_ne!(tree.fingerprint, tree_with_overlay.fingerprint);
    let overlay_content = tree_with_overlay.files["configs/system.ltx"].read().unwrap();
    assert_eq!(overlay_content, b"[system]\nversion = 2.0_mod\n");
}

#[cfg(unix)]
#[test]
fn loose_scan_does_not_follow_directory_symlinks() {
    let temp = TempDir::new("file-tree-symlink");
    let root = &temp.path;
    fs::write(
        root.join("fsgame.ltx"),
        "$game_data$ = false | true | $fs_root$ | gamedata\\\n\
         $game_config$ = true | false | $game_data$ | configs\\\n\
         $arch_dir$ = false | false | $fs_root$ | archives\\\n",
    )
    .unwrap();

    let config_dir = root.join("gamedata/configs");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("system.ltx"), b"[system]\n").unwrap();
    std::os::unix::fs::symlink(&config_dir, root.join("gamedata/config_alias")).unwrap();

    let tree = GameFileTree::load_simple(CompanionGame::CallOfPripyat, root, |_| true, true).unwrap();

    assert!(tree.files.contains_key("configs/system.ltx"));
    assert!(!tree.files.contains_key("config_alias/system.ltx"));
}

#[test]
fn oversized_loose_file_is_refused_before_reading_its_contents() {
    let temp = TempDir::new("file-tree-oversized");
    let root = &temp.path;
    fs::write(
        root.join("fsgame.ltx"),
        "$game_data$ = false | true | $fs_root$ | gamedata\\\n\
         $game_config$ = true | false | $game_data$ | configs\\\n\
         $arch_dir$ = false | false | $fs_root$ | archives\\\n",
    )
    .unwrap();
    let config_dir = root.join("gamedata/configs");
    fs::create_dir_all(&config_dir).unwrap();
    let oversized = config_dir.join("large.ltx");
    fs::File::create(&oversized)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();

    let tree = GameFileTree::load_simple(
        CompanionGame::CallOfPripyat,
        root,
        |path| path == "configs/large.ltx",
        true,
    )
    .unwrap();
    let file = tree.files.get("configs/large.ltx").unwrap();

    assert!(file.read().is_err(), "oversized loose content must be refused");
}

#[test]
fn oversized_fsgame_file_is_refused_before_text_parsing() {
    const MAX_FSGAME_BYTES: u64 = 1024 * 1024;
    let temp = TempDir::new("fsgame-size-limit");
    fs::File::create(temp.path.join("fsgame.ltx"))
        .unwrap()
        .set_len(MAX_FSGAME_BYTES + 1)
        .unwrap();

    let search = CompanionArchiveLocator::discover(&temp.path, &["fsgame.ltx"], CompanionGame::CallOfPripyat);

    assert!(search.issues.iter().any(|issue| issue.contains("read limit")));
}
