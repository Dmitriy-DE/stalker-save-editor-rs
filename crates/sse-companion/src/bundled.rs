//! Statically embedded companion payloads copied from the reference mod tree.
use std::path::Path;

use crate::installer::{quote, InstallError, PayloadFile};

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

/// Text of the Workshop page, shared by the three games; `{game}`, `{game_ru}` and `{game_uk}` are the game names.
const LISTING_DESCRIPTION: &str = include_str!("../assets/workshop/description.txt");
/// "What changed" text of this version.
const LISTING_CHANGELOG: &str = include_str!("../assets/workshop/changelog.txt");
/// Tags to set on the Workshop page, one per line.
const LISTING_TAGS: &str = include_str!("../assets/workshop/tags.txt");

/// What the Workshop page of one Enhanced Edition package shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkshopListing {
    /// Title of the Workshop item.
    pub title: String,
    /// One line for the `--desc` argument of GSC's uploader.
    pub short_description: String,
    /// Full description in Steam markup, English then Russian, for the Workshop page.
    pub description: String,
    /// "What changed" text of this version.
    pub changelog: &'static str,
    /// Tags, one per line.
    pub tags: &'static str,
    /// Preview picture, PNG 1024×1024 (`tools/make_workshop_previews.py`).
    pub preview_png: &'static [u8],
    /// The same picture as JPEG, the format of GSC's upload example.
    pub preview_jpg: &'static [u8],
    /// Title of the page in Russian.
    pub title_ru: String,
    /// One-line description in Russian.
    pub short_description_ru: String,
    /// The preview picture with Russian lettering, PNG.
    pub preview_ru_png: &'static [u8],
    /// The preview picture with Russian lettering, JPEG.
    pub preview_ru_jpg: &'static [u8],
    /// Title of the page in Ukrainian.
    pub title_uk: String,
    /// One-line description in Ukrainian.
    pub short_description_uk: String,
    /// The preview picture with Ukrainian lettering, PNG.
    pub preview_uk_png: &'static [u8],
    /// The preview picture with Ukrainian lettering, JPEG.
    pub preview_uk_jpg: &'static [u8],
}

/// Game names as Steam spells them for the Enhanced Editions, and the `--game` code of GSC's uploader.
const fn ee_names(game: Game) -> (&'static str, &'static str, &'static str) {
    match game {
        Game::ShadowOfChernobyl => ("Shadow of Chornobyl", "Тень Чернобыля", "SOC"),
        Game::ClearSky => ("Clear Sky", "Чистое небо", "CS"),
        Game::CallOfPripyat => ("Call of Prypiat", "Зов Припяти", "COP"),
    }
}

/// The Workshop page content of one game's package.
#[must_use]
pub fn workshop_listing(game: Game) -> WorkshopListing {
    let (name, name_ru, _) = ee_names(game);
    let name_uk = match game {
        Game::ShadowOfChernobyl => "Тінь Чорнобиля",
        Game::ClearSky => "Чисте небо",
        Game::CallOfPripyat => "Поклик Прип'яті",
    };
    let (preview_uk_png, preview_uk_jpg): (&[u8], &[u8]) = match game {
        Game::ShadowOfChernobyl => (
            include_bytes!("../assets/workshop/preview-soc-uk.png"),
            include_bytes!("../assets/workshop/preview-soc-uk.jpg"),
        ),
        Game::ClearSky => (
            include_bytes!("../assets/workshop/preview-cs-uk.png"),
            include_bytes!("../assets/workshop/preview-cs-uk.jpg"),
        ),
        Game::CallOfPripyat => (
            include_bytes!("../assets/workshop/preview-cop-uk.png"),
            include_bytes!("../assets/workshop/preview-cop-uk.jpg"),
        ),
    };
    let (preview_ru_png, preview_ru_jpg): (&[u8], &[u8]) = match game {
        Game::ShadowOfChernobyl => (
            include_bytes!("../assets/workshop/preview-soc-ru.png"),
            include_bytes!("../assets/workshop/preview-soc-ru.jpg"),
        ),
        Game::ClearSky => (
            include_bytes!("../assets/workshop/preview-cs-ru.png"),
            include_bytes!("../assets/workshop/preview-cs-ru.jpg"),
        ),
        Game::CallOfPripyat => (
            include_bytes!("../assets/workshop/preview-cop-ru.png"),
            include_bytes!("../assets/workshop/preview-cop-ru.jpg"),
        ),
    };
    let (preview_png, preview_jpg): (&[u8], &[u8]) = match game {
        Game::ShadowOfChernobyl => (
            include_bytes!("../assets/workshop/preview-soc.png"),
            include_bytes!("../assets/workshop/preview-soc.jpg"),
        ),
        Game::ClearSky => (
            include_bytes!("../assets/workshop/preview-cs.png"),
            include_bytes!("../assets/workshop/preview-cs.jpg"),
        ),
        Game::CallOfPripyat => (
            include_bytes!("../assets/workshop/preview-cop.png"),
            include_bytes!("../assets/workshop/preview-cop.jpg"),
        ),
    };
    WorkshopListing {
        title: format!("Save Editor Companion — {name} Enhanced Edition"),
        short_description: format!(
            "In-game menu for {name} Enhanced Edition (Esc, then F1): items, money, healing, repair, teleport, \
             NPC, weather. Part of the open-source S.T.A.L.K.E.R. Save Editor; works without it too. Unofficial fan mod."
        ),
        description: LISTING_DESCRIPTION
            .replace("{game}", name)
            .replace("{game_ru}", name_ru)
            .replace("{game_uk}", name_uk),
        changelog: LISTING_CHANGELOG,
        tags: LISTING_TAGS,
        preview_png,
        preview_jpg,
        title_ru: format!("Компаньон Save Editor — {name_ru} Enhanced Edition"),
        short_description_ru: format!(
            "Игровое меню для «{name_ru}» Enhanced Edition (Esc, затем F1): предметы, деньги, лечение, ремонт, \
             телепорт, NPC, погода. Часть открытого редактора S.T.A.L.K.E.R. Save Editor; работает и без него. \
             Неофициальный мод."
        ),
        preview_ru_png,
        preview_ru_jpg,
        title_uk: format!("Компаньйон Save Editor — {name_uk} Enhanced Edition"),
        short_description_uk: format!(
            "Ігрове меню для «{name_uk}» Enhanced Edition (Esc, потім F1): предмети, гроші, лікування, ремонт, \
             телепорт, NPC, погода. Частина відкритого редактора S.T.A.L.K.E.R. Save Editor; працює і без нього. \
             Неофіційний мод."
        ),
        preview_uk_png,
        preview_uk_jpg,
    }
}

/// The bundled mod files as an Enhanced Edition reads them. Shadow of Chornobyl keeps its configs in
/// `gamedata/config`, the other two in `gamedata/configs` (their `fsgame_*.ltx`).
pub fn ee_payloads(game: Game) -> Result<Vec<PayloadFile>, InstallError> {
    let mut files = payloads(game)?;
    if game == Game::ShadowOfChernobyl {
        for file in &mut files {
            if let Some(relative) = file.relative_path.strip_prefix("gamedata/configs/") {
                file.relative_path = format!("gamedata/config/{relative}");
            }
        }
    }
    Ok(files)
}

/// Name of the archive inside the uploaded folder. The Enhanced Edition engines register only `.pack` and `.db`
/// files of a Workshop item.
#[must_use]
pub fn ee_archive_name(game: Game) -> String {
    format!("save_editor_companion_{}.pack", game_id(game))
}

/// Stages one Enhanced Edition Workshop package. The output directory must not already exist.
///
/// Layout: `upload/<archive>.pack` is the folder GSC's uploader takes (it must hold nothing but `.pack` and
/// `.mp4` files); next to it lie the preview pictures, the page texts under `listing/`, the upload command and
/// `desc.json`, this editor's own record of the package.
///
/// `hooked` are the mod files together with the three patched game files that start the mod
/// ([`crate::installer::hooked_payloads`]). Without them (`None`) the archive holds the mod files only: the game
/// mounts it but nothing runs, and the package says so in `NOT-READY.txt`.
pub fn stage_enhanced_edition(
    output: &Path,
    game: Game,
    version: &str,
    author: &str,
    hooked: Option<Vec<PayloadFile>>,
) -> Result<(), InstallError> {
    if version.is_empty() {
        return Err(InstallError::new("Enhanced Edition package version is empty"));
    }
    let game_id = game_id(game);
    let (_, _, code) = ee_names(game);
    let listing = workshop_listing(game);
    let parent = output
        .parent()
        .ok_or_else(|| InstallError::new("package output must have a parent directory"))?;
    std::fs::create_dir_all(parent).map_err(InstallError::from)?;
    std::fs::create_dir(output).map_err(InstallError::from)?;
    let result = (|| {
        let starts_in_game = hooked.is_some();
        let archive_files = match hooked {
            Some(files) => files,
            None => ee_payloads(game)?,
        };
        if archive_files
            .iter()
            .any(|file| !file.relative_path.starts_with("gamedata/"))
        {
            return Err(InstallError::new("invalid bundled game payload path"));
        }
        let archive_name = ee_archive_name(game);
        let archive = build_ee_archive(&archive_files)?;
        let write =
            |relative: &str, bytes: &[u8]| std::fs::write(output.join(relative), bytes).map_err(InstallError::from);
        std::fs::create_dir(output.join("upload")).map_err(InstallError::from)?;
        std::fs::create_dir(output.join("listing")).map_err(InstallError::from)?;
        write(&format!("upload/{archive_name}"), &archive)?;
        write("preview.png", listing.preview_png)?;
        write("preview.jpg", listing.preview_jpg)?;
        write("preview-ru.png", listing.preview_ru_png)?;
        write("preview-ru.jpg", listing.preview_ru_jpg)?;
        write("preview-uk.png", listing.preview_uk_png)?;
        write("preview-uk.jpg", listing.preview_uk_jpg)?;
        write("listing/title-uk.txt", format!("{}\n", listing.title_uk).as_bytes())?;
        write(
            "listing/short-description-uk.txt",
            format!("{}\n", listing.short_description_uk).as_bytes(),
        )?;
        write("listing/title-ru.txt", format!("{}\n", listing.title_ru).as_bytes())?;
        write(
            "listing/short-description-ru.txt",
            format!("{}\n", listing.short_description_ru).as_bytes(),
        )?;
        write("listing/title.txt", format!("{}\n", listing.title).as_bytes())?;
        write(
            "listing/short-description.txt",
            format!("{}\n", listing.short_description).as_bytes(),
        )?;
        write("listing/description.txt", listing.description.as_bytes())?;
        // Steam limits a description to 8000 characters and keeps one text per language, so each language is
        // also written on its own: English, Ukrainian, Russian, in the order of the shared text.
        for (language, part) in ["en", "uk", "ru"].iter().zip(listing.description.split("[hr][/hr]")) {
            write(
                &format!("listing/description-{language}.txt"),
                format!("{}\n", part.trim()).as_bytes(),
            )?;
        }
        write("listing/changelog.txt", listing.changelog.as_bytes())?;
        write("listing/tags.txt", listing.tags.as_bytes())?;
        let command = format!(
            "xrSWS_Upload.exe --mode=create --game={code} --path=<this folder>\\upload --preview=<this folder>\\preview.jpg \
             --title=\"{}\" --desc=\"{}\" --changelog=\"{version}\"\n",
            listing.title, listing.short_description
        );
        write("upload-command.txt", command.as_bytes())?;
        if !starts_in_game {
            write(
                "NOT-READY.txt",
                b"This archive holds the companion files only. The game mounts it, but nothing calls the mod:\n\
                  the three patched game files (bind_stalker.script, ui_main_menu.script, quest_items.ltx) are missing.\n\
                  Stage it again with --hooks-from <Enhanced Edition folder> before uploading. See docs/WORKSHOP.md.\n",
            )?;
        }
        let metadata = format!(
            "{{\n  \"title\": {},\n  \"game\": {},\n  \"version\": {},\n  \"author\": {},\n  \"description\": {},\n  \"preview\": \"preview.png\",\n  \"package_file\": {},\n  \"starts_in_game\": {starts_in_game},\n  \"steam_workshop\": {{\n    \"game_code\": {},\n    \"published_file_id\": 0,\n    \"visibility\": \"public\"\n  }}\n}}\n",
            quote(&listing.title),
            quote(game_id),
            quote(version),
            quote(author),
            quote(&listing.short_description),
            quote(&format!("upload/{archive_name}")),
            quote(code),
        );
        write("desc.json", metadata.as_bytes())
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(output);
    }
    result
}

/// Builds a stored (uncompressed) X-Ray archive: the classic chunk table with an `[header] entry_point` chunk
/// that mounts the files under the game's `gamedata`. It follows the open X-Ray archive format; it was never
/// compared with the output of GSC's own `xrCompress -store`, which is not available here.
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
