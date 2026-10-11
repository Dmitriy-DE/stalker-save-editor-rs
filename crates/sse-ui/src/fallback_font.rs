//! System fonts for the CJK characters the bundled faces do not have.
//!
//! The editor bundles only Latin and Cyrillic faces, since a CJK font is tens of megabytes. When a Japanese, Chinese or
//! Korean label needs a glyph the bundled faces lack, the first matching system font is read from the platform's font
//! folders, once, and only then. Nothing is read at start-up, and a Russian interface never triggers it.

use sse_codecs::font::{Font, GlyphId};
use std::path::{Path, PathBuf};

/// Whether `character` is one of the CJK scripts (kana, hangul, ideographs, and their full-width punctuation).
#[must_use]
pub fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x3000..=0x303F
            | 0x3040..=0x30FF
            | 0x3100..=0x318F
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0xFF00..=0xFFEF
    )
}

/// Font file names (or fragments of them) to look for on Linux, in the order they are tried.
#[cfg(target_os = "linux")]
const NAMES: &[&str] = &[
    "NotoSansCJK",
    "NotoSerifCJK",
    "NotoSansJP",
    "NotoSansSC",
    "NotoSansTC",
    "NotoSansKR",
    "SourceHanSans",
    "wqy",
    "DroidSansFallback",
    "takao",
    "IPAGothic",
    "VLGothic",
    "NanumGothic",
    "UnDotum",
];
#[cfg(target_os = "windows")]
const NAMES: &[&str] = &["YuGothR", "meiryo", "msgothic", "msyh", "simsun", "msjh", "malgun"];
#[cfg(target_os = "macos")]
const NAMES: &[&str] = &["PingFang", "Hiragino Sans GB", "ヒラギノ角ゴシック", "AppleSDGothicNeo"];
#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
const NAMES: &[&str] = &[];

/// Name fragments that suit one interface language best; they are tried first.
fn preferred_fragments(language: &str) -> &'static [&'static str] {
    match language {
        "ja" => &[
            "JP", "Takao", "IPA", "VLGothic", "YuGoth", "meiryo", "msgothic", "Hiragino",
        ],
        "zh-CN" => &["SC", "GB", "msyh", "simsun", "PingFang"],
        "zh-TW" => &["TC", "JH", "msjh"],
        "ko" => &["KR", "Nanum", "UnDotum", "malgun", "AppleSDGothicNeo"],
        _ => &[],
    }
}

/// Folders that may hold system fonts on this platform.
fn font_folders() -> Vec<PathBuf> {
    let mut folders = Vec::new();
    #[cfg(target_os = "linux")]
    {
        folders.push(PathBuf::from("/usr/share/fonts"));
        folders.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(home) = std::env::var_os("HOME") {
            folders.push(PathBuf::from(&home).join(".local/share/fonts"));
            folders.push(PathBuf::from(&home).join(".fonts"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR").unwrap_or_else(|| "C:\\Windows".into());
        folders.push(PathBuf::from(windir).join("Fonts"));
    }
    #[cfg(target_os = "macos")]
    {
        folders.push(PathBuf::from("/System/Library/Fonts"));
        folders.push(PathBuf::from("/Library/Fonts"));
    }
    folders
}

/// Looks for font files under `folders`, a few folders deep, whose names contain one of `fragments`.
/// The first fragment that matches wins, so the order of `fragments` is the order of preference.
fn find_font(folders: &[PathBuf], fragments: &[&str]) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for folder in folders {
        collect_fonts(folder, 0, &mut found);
    }
    for fragment in fragments {
        let needle = fragment.to_lowercase();
        if let Some(path) = found.iter().find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().contains(&needle))
        }) {
            return Some(path.clone());
        }
    }
    None
}

fn collect_fonts(folder: &Path, depth: u32, found: &mut Vec<PathBuf>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, depth.saturating_add(1), found);
            continue;
        }
        let is_font = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension.to_lowercase().as_str(), "ttc" | "ttf" | "otf"));
        if is_font
            && path.file_name().and_then(|name| name.to_str()).is_some_and(|name| {
                NAMES
                    .iter()
                    .any(|fragment| name.to_lowercase().contains(&fragment.to_lowercase()))
            })
        {
            found.push(path);
        }
    }
}

/// Every face of one system font file, parsed from memory that lives as long as the program does.
pub struct SystemFaces {
    faces: Vec<Font<'static>>,
}

impl SystemFaces {
    /// Parses every face of a font file already in memory. Returns `None` when no face parses.
    #[must_use]
    pub fn from_bytes(data: &'static [u8]) -> Option<Self> {
        let mut faces = Vec::new();
        for index in 0..32_u32 {
            match Font::parse(data, index) {
                Ok(face) => faces.push(face),
                Err(_) => break,
            }
        }
        (!faces.is_empty()).then_some(Self { faces })
    }

    /// The first face that has `character`, with its glyph.
    #[must_use]
    pub fn glyph(&self, character: char) -> Option<(&Font<'static>, GlyphId)> {
        self.faces.iter().find_map(|face| match face.glyph(character) {
            Some(glyph) if glyph.0 != 0 => Some((face, glyph)),
            _ => None,
        })
    }

    /// Loads the system font best suited to `language`, or `None` when this machine has none.
    #[must_use]
    pub fn load(language: &str) -> Option<Self> {
        let mut fragments: Vec<&str> = preferred_fragments(language).to_vec();
        fragments.extend(NAMES.iter().copied());
        let path = find_font(&font_folders(), &fragments)?;
        let bytes = std::fs::read(&path).ok()?;
        // The face data is kept for the rest of the run, so that the glyph masks drawn from it stay valid.
        let data: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        Self::from_bytes(data)
    }
}
