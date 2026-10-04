//! Statically embedded companion payloads copied from the reference mod tree.
use std::path::Path;

use crate::installer::{InstallError, PayloadFile};

/// Bundled game target for X-Ray installation and Enhanced Edition archive staging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Game {
    /// Shadow of Chernobyl.
    ShadowOfChernobyl,
    /// Clear Sky.
    ClearSky,
    /// Call of Pripyat.
    CallOfPripyat,
}

static COMMON_ASSETS: &[(&str, &[u8])] = &[
    (
        "gamedata/scripts/save_editor_companion.script",
        include_bytes!("../assets/companion/gamedata/scripts/save_editor_companion.script"),
    ),
    (
        "gamedata/scripts/save_editor_level_changer.script",
        include_bytes!("../assets/companion/gamedata/scripts/save_editor_level_changer.script"),
    ),
];

static SOC_ASSETS: &[(&str, &[u8])] = &[
    (
        "gamedata/configs/misc/save_editor_companion.ltx",
        include_bytes!("../assets/companion/soc/gamedata/configs/misc/save_editor_companion.ltx"),
    ),
    (
        "gamedata/configs/ui/ui_save_editor_companion.xml",
        include_bytes!("../assets/companion/soc/gamedata/configs/ui/ui_save_editor_companion.xml"),
    ),
    (
        "gamedata/scripts/save_editor_catalog.script",
        include_bytes!("../assets/companion/soc/gamedata/scripts/save_editor_catalog.script"),
    ),
    (
        "gamedata/scripts/save_editor_companion_ui.script",
        include_bytes!("../assets/companion/soc/gamedata/scripts/save_editor_companion_ui.script"),
    ),
    (
        "gamedata/scripts/save_editor_places.script",
        include_bytes!("../assets/companion/soc/gamedata/scripts/save_editor_places.script"),
    ),
    (
        "gamedata/scripts/save_editor_squads.script",
        include_bytes!("../assets/companion/soc/gamedata/scripts/save_editor_squads.script"),
    ),
    (
        "gamedata/scripts/save_editor_weather.script",
        include_bytes!("../assets/companion/soc/gamedata/scripts/save_editor_weather.script"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/amber.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber_dim.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/amber_dim.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/bar_bg.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/bar_bg.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_d.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/btn_d.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_e.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/btn_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_h.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/btn_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_t.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/btn_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/danger.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/danger.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/line.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line_soft.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/line_soft.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/ok.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/ok.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/panel.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/panel.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/row.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/row.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/shade.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/shade.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_e.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/tab_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_h.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/tab_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_t.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/tab_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/window.dds",
        include_bytes!("../assets/companion/soc/gamedata/textures/ui/se_companion/window.dds"),
    ),
];

static CS_ASSETS: &[(&str, &[u8])] = &[
    (
        "gamedata/configs/misc/save_editor_companion.ltx",
        include_bytes!("../assets/companion/cs/gamedata/configs/misc/save_editor_companion.ltx"),
    ),
    (
        "gamedata/configs/ui/ui_save_editor_companion.xml",
        include_bytes!("../assets/companion/cs/gamedata/configs/ui/ui_save_editor_companion.xml"),
    ),
    (
        "gamedata/scripts/save_editor_catalog.script",
        include_bytes!("../assets/companion/cs/gamedata/scripts/save_editor_catalog.script"),
    ),
    (
        "gamedata/scripts/save_editor_companion_ui.script",
        include_bytes!("../assets/companion/cs/gamedata/scripts/save_editor_companion_ui.script"),
    ),
    (
        "gamedata/scripts/save_editor_places.script",
        include_bytes!("../assets/companion/cs/gamedata/scripts/save_editor_places.script"),
    ),
    (
        "gamedata/scripts/save_editor_squads.script",
        include_bytes!("../assets/companion/cs/gamedata/scripts/save_editor_squads.script"),
    ),
    (
        "gamedata/scripts/save_editor_weather.script",
        include_bytes!("../assets/companion/cs/gamedata/scripts/save_editor_weather.script"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/amber.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber_dim.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/amber_dim.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/bar_bg.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/bar_bg.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_d.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/btn_d.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_e.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/btn_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_h.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/btn_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_t.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/btn_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/danger.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/danger.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/line.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line_soft.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/line_soft.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/ok.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/ok.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/panel.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/panel.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/row.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/row.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/shade.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/shade.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_e.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/tab_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_h.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/tab_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_t.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/tab_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/window.dds",
        include_bytes!("../assets/companion/cs/gamedata/textures/ui/se_companion/window.dds"),
    ),
];

static COP_ASSETS: &[(&str, &[u8])] = &[
    (
        "gamedata/configs/misc/save_editor_companion.ltx",
        include_bytes!("../assets/companion/cop/gamedata/configs/misc/save_editor_companion.ltx"),
    ),
    (
        "gamedata/configs/ui/ui_save_editor_companion.xml",
        include_bytes!("../assets/companion/cop/gamedata/configs/ui/ui_save_editor_companion.xml"),
    ),
    (
        "gamedata/scripts/save_editor_catalog.script",
        include_bytes!("../assets/companion/cop/gamedata/scripts/save_editor_catalog.script"),
    ),
    (
        "gamedata/scripts/save_editor_companion_ui.script",
        include_bytes!("../assets/companion/cop/gamedata/scripts/save_editor_companion_ui.script"),
    ),
    (
        "gamedata/scripts/save_editor_places.script",
        include_bytes!("../assets/companion/cop/gamedata/scripts/save_editor_places.script"),
    ),
    (
        "gamedata/scripts/save_editor_squads.script",
        include_bytes!("../assets/companion/cop/gamedata/scripts/save_editor_squads.script"),
    ),
    (
        "gamedata/scripts/save_editor_weather.script",
        include_bytes!("../assets/companion/cop/gamedata/scripts/save_editor_weather.script"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/amber.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/amber_dim.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/amber_dim.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/bar_bg.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/bar_bg.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_d.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/btn_d.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_e.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/btn_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_h.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/btn_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/btn_t.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/btn_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/danger.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/danger.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/line.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/line_soft.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/line_soft.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/ok.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/ok.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/panel.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/panel.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/row.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/row.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/shade.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/shade.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_e.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/tab_e.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_h.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/tab_h.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/tab_t.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/tab_t.dds"),
    ),
    (
        "gamedata/textures/ui/se_companion/window.dds",
        include_bytes!("../assets/companion/cop/gamedata/textures/ui/se_companion/window.dds"),
    ),
];

static S2_ASSETS: &[(&str, &[u8])] = &[
    (
        "SaveEditorCompanion/Scripts/main.lua",
        include_bytes!("../assets/companion/s2/SaveEditorCompanion/Scripts/main.lua"),
    ),
    (
        "SaveEditorCompanion/enabled.txt",
        include_bytes!("../assets/companion/s2/SaveEditorCompanion/enabled.txt"),
    ),
];

/// Returns the exact shipped payload after converting text assets to Windows-1251, as the C# editor does.
pub fn payloads(game: Game) -> Result<Vec<PayloadFile>, InstallError> {
    let specific = match game {
        Game::ShadowOfChernobyl => SOC_ASSETS,
        Game::ClearSky => CS_ASSETS,
        Game::CallOfPripyat => COP_ASSETS,
    };
    COMMON_ASSETS
        .iter()
        .chain(specific.iter())
        .map(|(path, bytes)| {
            let extension = Path::new(path).extension().and_then(|value| value.to_str());
            let encoded = if matches!(extension, Some("script" | "xml" | "ltx")) {
                encode_windows_1251(bytes)?
            } else {
                bytes.to_vec()
            };
            Ok(PayloadFile::new(*path, encoded))
        })
        .collect()
}

/// Returns the S2 UE4SS payload, including the C#-compatible ownership marker.
pub fn stalker2_payloads() -> Result<Vec<PayloadFile>, InstallError> {
    let mut payloads = S2_ASSETS
        .iter()
        .map(|(path, bytes)| PayloadFile::new(*path, bytes.to_vec()))
        .collect::<Vec<_>>();
    let marker = format!("{{\"build\":{}}}\n", quote(&stalker2_build()));
    payloads.push(PayloadFile::new(
        "SaveEditorCompanion/save_editor_install.json",
        marker.into_bytes(),
    ));
    Ok(payloads)
}

/// Build identifier embedded in the bundled S2 Lua mod.
#[must_use]
pub fn stalker2_build() -> String {
    let source = S2_ASSETS
        .first()
        .and_then(|(_, bytes)| std::str::from_utf8(bytes).ok())
        .unwrap_or_default();
    source
        .split_once("local MOD_BUILD = \"")
        .and_then(|(_, rest)| rest.split_once('\"'))
        .map_or_else(|| "unknown".to_owned(), |(build, _)| build.to_owned())
}

/// Writes a stored X-Ray `.xrp` archive and its C#-compatible descriptor. The output directory must not already exist.
pub fn stage_enhanced_edition(
    output: &Path,
    game: Game,
    version: &str,
    author: &str,
    title: &str,
    description: &str,
) -> Result<(), InstallError> {
    if version.is_empty() {
        return Err(InstallError::new("Enhanced Edition package version is empty"));
    }
    let game_id = game_id(game);
    let code = match game {
        Game::ShadowOfChernobyl => "SOC",
        Game::ClearSky => "CS",
        Game::CallOfPripyat => "COP",
    };
    let parent = output
        .parent()
        .ok_or_else(|| InstallError::new("package output must have a parent directory"))?;
    std::fs::create_dir_all(parent).map_err(InstallError::from)?;
    std::fs::create_dir(output).map_err(InstallError::from)?;
    let result = (|| {
        let mut archive_files = Vec::new();
        for file in payloads(game)? {
            if !file.relative_path.starts_with("gamedata/") {
                return Err(InstallError::new("invalid bundled game payload path"));
            }
            archive_files.push(file);
        }
        let archive_name = format!("save_editor_companion_{game_id}.xrp");
        let archive = build_ee_archive(&archive_files)?;
        std::fs::write(output.join(&archive_name), archive).map_err(InstallError::from)?;
        let metadata = format!(
            "{{\n  \"title\": {},\n  \"game\": {},\n  \"version\": {},\n  \"author\": {},\n  \"description\": {},\n  \"preview\": null,\n  \"entry_point\": \"gamedata/scripts/save_editor_companion.script\",\n  \"package_file\": {},\n  \"steam_workshop\": {{\n    \"game_code\": {},\n    \"published_file_id\": 0,\n    \"visibility\": \"public\"\n  }}\n}}\n",
            quote(title),
            quote(game_id),
            quote(version),
            quote(author),
            quote(description),
            quote(&archive_name),
            quote(code),
        );
        std::fs::write(output.join("desc.json"), metadata).map_err(InstallError::from)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

/// Builds a stored (uncompressed) X-Ray archive with the same chunk table and metadata framing
/// as the reference `xrCompress -store` package path.
pub fn build_ee_archive(files: &[PayloadFile]) -> Result<Vec<u8>, InstallError> {
    const METADATA: &[u8] = b"[header]\nentry_point = $fs_root$\\gamedata\\\n";
    let mut sorted = files.iter().collect::<Vec<_>>();
    sorted.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    if sorted.is_empty() {
        return Err(InstallError::new("Enhanced Edition archive has no files"));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut header_size = 0_usize;
    let mut data_size = 0_usize;
    for file in &sorted {
        let name = file.relative_path.as_bytes();
        if !file.relative_path.starts_with("gamedata/")
            || name.len().saturating_add(16) > usize::from(u16::MAX)
            || !seen.insert(&file.relative_path)
            || u32::try_from(file.bytes.len()).is_err()
        {
            return Err(InstallError::new(
                "Enhanced Edition archive contains an invalid path or length",
            ));
        }
        header_size = header_size
            .checked_add(18)
            .and_then(|size| size.checked_add(name.len()))
            .ok_or_else(|| InstallError::new("Enhanced Edition archive header is too large"))?;
        data_size = data_size
            .checked_add(file.bytes.len())
            .ok_or_else(|| InstallError::new("Enhanced Edition archive data is too large"))?;
    }
    let metadata_chunk_size = 8_usize
        .checked_add(METADATA.len())
        .ok_or_else(|| InstallError::new("Enhanced Edition archive metadata is too large"))?;
    let data_offset = metadata_chunk_size
        .checked_add(8)
        .and_then(|offset| offset.checked_add(header_size))
        .and_then(|offset| offset.checked_add(8))
        .ok_or_else(|| InstallError::new("Enhanced Edition archive offset overflow"))?;
    let mut table = Vec::with_capacity(header_size);
    let mut offset = u32::try_from(data_offset)
        .map_err(|_| InstallError::new("Enhanced Edition archive exceeds the X-Ray offset range"))?;
    for file in &sorted {
        let name = file.relative_path.as_bytes();
        let name_length = name
            .len()
            .checked_add(16)
            .ok_or_else(|| InstallError::new("Enhanced Edition archive path is too long"))?;
        let name_size =
            u16::try_from(name_length).map_err(|_| InstallError::new("Enhanced Edition archive path is too long"))?;
        let size = u32::try_from(file.bytes.len())
            .map_err(|_| InstallError::new("Enhanced Edition archive file is too large"))?;
        table.extend_from_slice(&name_size.to_le_bytes());
        table.extend_from_slice(&size.to_le_bytes());
        table.extend_from_slice(&size.to_le_bytes());
        table.extend_from_slice(&sse_codecs::crc32::crc32(&file.bytes).to_le_bytes());
        table.extend_from_slice(name);
        table.extend_from_slice(&offset.to_le_bytes());
        offset = offset
            .checked_add(size)
            .ok_or_else(|| InstallError::new("Enhanced Edition archive file offset overflow"))?;
    }
    let mut archive = Vec::with_capacity(data_offset.saturating_add(data_size));
    archive.extend_from_slice(&666_u32.to_le_bytes());
    archive.extend_from_slice(
        &u32::try_from(METADATA.len())
            .map_err(|_| InstallError::new("metadata length overflow"))?
            .to_le_bytes(),
    );
    archive.extend_from_slice(METADATA);
    archive.extend_from_slice(&1_u32.to_le_bytes());
    archive.extend_from_slice(
        &u32::try_from(table.len())
            .map_err(|_| InstallError::new("archive header length overflow"))?
            .to_le_bytes(),
    );
    archive.extend_from_slice(&table);
    archive.extend_from_slice(&0_u32.to_le_bytes());
    archive.extend_from_slice(
        &u32::try_from(data_size)
            .map_err(|_| InstallError::new("archive data length overflow"))?
            .to_le_bytes(),
    );
    for file in sorted {
        archive.extend_from_slice(&file.bytes);
    }
    Ok(archive)
}

fn game_id(game: Game) -> &'static str {
    match game {
        Game::ShadowOfChernobyl => "soc",
        Game::ClearSky => "cs",
        Game::CallOfPripyat => "cop",
    }
}

fn quote(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control <= '\u{1f}' => {
                output.push_str(&format!("\\u{:04x}", u32::from(control)));
            }
            other => output.push(other),
        }
    }
    output.push('"');
    output
}

fn encode_windows_1251(bytes: &[u8]) -> Result<Vec<u8>, InstallError> {
    let text = std::str::from_utf8(bytes).map_err(|_| InstallError::new("bundled text asset is not UTF-8"))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut output = Vec::with_capacity(text.len());
    for character in text.chars() {
        let encoded = u8::try_from(u32::from(character))
            .ok()
            .filter(|byte| *byte < 0x80)
            .or_else(|| {
                sse_content::encoding::CP1251_TABLE
                    .iter()
                    .position(|codepoint| *codepoint != 65_533 && u32::from(*codepoint) == u32::from(character))
                    .and_then(|offset| offset.checked_add(0x80))
                    .and_then(|offset| u8::try_from(offset).ok())
            });
        match encoded {
            Some(byte) => output.push(byte),
            None => {
                return Err(InstallError::new(
                    "bundled text asset cannot be represented as Windows-1251",
                ))
            }
        }
    }
    Ok(output)
}
