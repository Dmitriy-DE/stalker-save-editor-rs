//! Game content service: loads installed game catalogs, manages on-disk cache, crops icons.

use crate::builder::InstalledGameCatalogBuilder;
use crate::bundle::{CatalogBundleReader, CatalogBundleWriter};
use crate::models::{CatalogBundle, ItemCatalog};
use sse_codecs::crc32::crc32;
use sse_codecs::sha256::sha256;
use sse_content::{CompanionGame, DdsImage, GameFile, GameFileTree, LtxDocument, RgbaImage, XRayStringTables};
use sse_core::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const ICON_CELL_SIZE: u32 = 50;
const BUILDER_VERSION: u32 = 5;
const MOD_MARKERS: &[&str] = &["ogsm", "srp", "anomaly", "misery", "complete", "gunslinger", "amk"];

/// Status information of loaded game content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameContentStatus {
    /// Release ID (e.g. "stalker-cop").
    pub release_id: String,
    /// Root game directory path.
    pub game_directory: String,
    /// Fingerprint of game files.
    pub fingerprint: String,
    /// Whether loaded from on-disk cache.
    pub from_cache: bool,
    /// Whether loose gamedata overlay is present.
    pub has_loose_overlay: bool,
    /// Name of detected mod if any.
    pub mod_name: Option<String>,
    /// Number of items loaded.
    pub item_count: usize,
    /// Number of upgrades loaded.
    pub upgrade_count: usize,
    /// Number of factions loaded.
    pub faction_count: usize,
    /// Informational warnings or issues.
    pub issues: Vec<String>,
}

/// Loaded game content containing catalog and icon provider.
pub struct GameContent {
    bundle: CatalogBundle,
    status: GameContentStatus,
    icon_source: IconSource,
}

impl GameContent {
    /// The loaded catalog bundle.
    #[must_use]
    pub fn bundle(&self) -> &CatalogBundle {
        &self.bundle
    }

    /// Status of the loaded content.
    #[must_use]
    pub fn status(&self) -> &GameContentStatus {
        &self.status
    }

    /// Returns PNG bytes for an item icon, cropped on demand and cached.
    pub fn icon_png(&self, item_key: &str) -> Option<Vec<u8>> {
        self.icon_source.png(item_key)
    }
}

/// Service managing installed game content and caching.
pub struct GameContentService;

impl GameContentService {
    /// Maps a companion game to its release identifier.
    #[must_use]
    pub fn release_id_for(game: CompanionGame) -> &'static str {
        match game {
            CompanionGame::ShadowOfChernobyl => "stalker-soc",
            CompanionGame::ClearSky => "stalker-cs",
            CompanionGame::CallOfPripyat => "stalker-cop",
        }
    }

    /// Loads content for an installed game from cache or builds it.
    pub fn load(
        game: CompanionGame,
        game_directory: &Path,
        cache_directory: &Path,
        ui_language: &str,
    ) -> Result<Option<GameContent>> {
        let release_id = Self::release_id_for(game);
        let mut index = GameFileTree::load_simple(game, game_directory, is_wanted, true)?;
        let mut tree = index.clone();
        let mod_name = detect_mod(&tree);

        let fingerprint_prefix = if tree.fingerprint.len() >= 32 {
            tree.fingerprint.get(..32).unwrap_or(&tree.fingerprint)
        } else {
            &tree.fingerprint
        };

        let mut cache_root = cache_directory.join(format!("{release_id}-v{BUILDER_VERSION}-{fingerprint_prefix}"));
        let mut catalog_path = cache_root.join(format!("catalog-{}.json", safe_language(ui_language)));

        let mut bundle: Option<CatalogBundle> = None;
        let mut from_cache = false;

        if catalog_path.exists() {
            if let Ok(bytes) = fs::read(&catalog_path) {
                if let Ok(mut bundles) = CatalogBundleReader::load(&bytes) {
                    bundle = bundles.remove(release_id);
                    from_cache = bundle.is_some();
                }
            }
        }

        if bundle.is_none() {
            tree = GameFileTree::load_simple(game, game_directory, is_wanted, false)?;
            if tree.fingerprint != index.fingerprint {
                index = GameFileTree::load_simple(game, game_directory, is_wanted, true)?;
            }
            let fp_prefix = if tree.fingerprint.len() >= 32 {
                tree.fingerprint.get(..32).unwrap_or(&tree.fingerprint)
            } else {
                &tree.fingerprint
            };
            cache_root = cache_directory.join(format!("{release_id}-v{BUILDER_VERSION}-{fp_prefix}"));
            catalog_path = cache_root.join(format!("catalog-{}.json", safe_language(ui_language)));
        }

        let mut issues = tree.issues.clone();

        if bundle.is_none() {
            let config_prefix = &tree.config_prefix;
            let system_ltx_path = format!("{config_prefix}system.ltx");
            let mut sections = LtxDocument::parse_include_graph(&system_ltx_path, &tree.files, |f| f.read().ok());

            if sections.is_none() {
                issues.push("system.ltx was not found; the catalog was read from every LTX file.".to_string());
                let mut fallback_sections = HashMap::new();
                let mut ltx_files: Vec<&GameFile> = tree
                    .files
                    .values()
                    .filter(|f| {
                        f.relative_path.to_ascii_lowercase().starts_with(config_prefix)
                            && f.relative_path.to_ascii_lowercase().ends_with(".ltx")
                    })
                    .collect();
                ltx_files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));

                for file in ltx_files {
                    let decoded = LtxDocument::decode(&file.read()?);
                    let parsed = LtxDocument::parse(&decoded, &file.relative_path);
                    for (name, section) in parsed {
                        fallback_sections.insert(name, section);
                    }
                }
                sections = Some(fallback_sections);
            }

            let sections = match sections {
                Some(s) => s,
                None => return Ok(None),
            };

            let config_files: Vec<&GameFile> = tree
                .files
                .values()
                .filter(|f| f.relative_path.to_ascii_lowercase().starts_with(config_prefix))
                .collect();
            let strings = XRayStringTables::read(&config_files, |f| &f.relative_path, |f| f.read().ok(), ui_language);

            let built = InstalledGameCatalogBuilder::build(release_id, &sections, &strings)?;
            if let Some(b) = built {
                let _ = fs::create_dir_all(&cache_root);
                if let Ok(written_bytes) = CatalogBundleWriter::write(&[&b]) {
                    atomic_write_file(&catalog_path, &written_bytes);
                }
                bundle = Some(b);
            } else {
                return Ok(None);
            }
        }

        let bundle = match bundle {
            Some(b) => b,
            None => return Ok(None),
        };

        let status = GameContentStatus {
            release_id: release_id.to_string(),
            game_directory: game_directory.to_string_lossy().to_string(),
            fingerprint: tree.fingerprint.clone(),
            from_cache,
            has_loose_overlay: tree.has_loose_overlay,
            mod_name,
            item_count: bundle.items.items().len(),
            upgrade_count: bundle.upgrades.as_ref().map(|u| u.upgrades().len()).unwrap_or(0),
            faction_count: bundle.factions.as_ref().map(|f| f.factions().len()).unwrap_or(0),
            issues,
        };

        let icon_source = IconSource::new(index, bundle.clone(), cache_root.join("icons"));
        let adjusted_bundle = with_fallback_names(bundle, ui_language);

        Ok(Some(GameContent {
            bundle: adjusted_bundle,
            status,
            icon_source,
        }))
    }

    /// Computes safe file name for cached item icon.
    #[must_use]
    pub fn icon_cache_file_name(key: &str) -> String {
        let readable: String = key
            .chars()
            .take(48)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let hash = sha256(key.as_bytes());
        let hash_hex = format!(
            "{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            hash.first().copied().unwrap_or(0),
            hash.get(1).copied().unwrap_or(0),
            hash.get(2).copied().unwrap_or(0),
            hash.get(3).copied().unwrap_or(0),
            hash.get(4).copied().unwrap_or(0),
            hash.get(5).copied().unwrap_or(0),
            hash.get(6).copied().unwrap_or(0),
            hash.get(7).copied().unwrap_or(0),
        );
        format!("{readable}-{hash_hex}")
    }

    /// Encodes an RGBA8 image as PNG bytes.
    #[must_use]
    pub fn to_png(image: &RgbaImage) -> Vec<u8> {
        let capacity = 32usize.saturating_add(image.width.saturating_mul(image.height).saturating_mul(4));
        let mut out = Vec::with_capacity(capacity);
        out.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);

        // IHDR chunk
        let mut ihdr_data = [0u8; 13];
        let w_u32 = u32::try_from(image.width).unwrap_or(0);
        let h_u32 = u32::try_from(image.height).unwrap_or(0);
        if let Some(slice) = ihdr_data.get_mut(0..4) {
            slice.copy_from_slice(&w_u32.to_be_bytes());
        }
        if let Some(slice) = ihdr_data.get_mut(4..8) {
            slice.copy_from_slice(&h_u32.to_be_bytes());
        }
        ihdr_data[8] = 8; // bit depth
        ihdr_data[9] = 6; // color type RGBA
        ihdr_data[10] = 0; // compression
        ihdr_data[11] = 0; // filter
        ihdr_data[12] = 0; // interlace
        write_chunk(&mut out, b"IHDR", &ihdr_data);

        // IDAT chunk (scanlines with filter byte 0)
        let row_stride = image.width.saturating_mul(4);
        let mut raw_scanlines = Vec::with_capacity(image.height.saturating_mul(row_stride.saturating_add(1)));
        for row in 0..image.height {
            raw_scanlines.push(0); // filter type None
            let start = row.saturating_mul(row_stride);
            let end = start.saturating_add(row_stride);
            if let Some(slice) = image.pixels.get(start..end) {
                raw_scanlines.extend_from_slice(slice);
            }
        }

        // RFC 1950 zlib wrapper around RFC 1951 stored DEFLATE blocks
        let mut zlib_stream = Vec::with_capacity(raw_scanlines.len().saturating_add(64));
        zlib_stream.push(0x78); // CMF: deflate, 32K window
        zlib_stream.push(0x01); // FLG: check bits

        let mut pos = 0usize;
        while pos < raw_scanlines.len() {
            let remaining = raw_scanlines.len().saturating_sub(pos);
            let chunk_len = remaining.min(65535);
            let is_last = pos.saturating_add(chunk_len) == raw_scanlines.len();
            zlib_stream.push(if is_last { 0x01 } else { 0x00 });
            let len_u16 = u16::try_from(chunk_len).unwrap_or(0);
            zlib_stream.extend_from_slice(&len_u16.to_le_bytes());
            zlib_stream.extend_from_slice(&(!len_u16).to_le_bytes());
            if let Some(chunk) = raw_scanlines.get(pos..pos.saturating_add(chunk_len)) {
                zlib_stream.extend_from_slice(chunk);
            }
            pos = pos.saturating_add(chunk_len);
        }

        let adler = adler32(&raw_scanlines);
        zlib_stream.extend_from_slice(&adler.to_be_bytes());

        write_chunk(&mut out, b"IDAT", &zlib_stream);

        // IEND chunk
        write_chunk(&mut out, b"IEND", &[]);
        out
    }
}

fn adler32(data: &[u8]) -> u32 {
    let mut s1 = 1u32;
    let mut s2 = 0u32;
    for &b in data {
        s1 = (s1.saturating_add(u32::from(b))) % 65521;
        s2 = (s2.saturating_add(s1)) % 65521;
    }
    (s2 << 16) | s1
}

fn write_chunk(output: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
    let len_u32 = u32::try_from(data.len()).unwrap_or(0);
    output.extend_from_slice(&len_u32.to_be_bytes());
    let crc_start = output.len();
    output.extend_from_slice(chunk_type);
    output.extend_from_slice(data);
    let crc = crc32(output.get(crc_start..).unwrap_or(&[]));
    output.extend_from_slice(&crc.to_be_bytes());
}

fn atomic_write_file(path: &Path, bytes: &[u8]) {
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, bytes).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn safe_language(lang: &str) -> String {
    let s: String = lang.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    if s.is_empty() {
        "ru".to_string()
    } else {
        s
    }
}

fn detect_mod(tree: &GameFileTree) -> Option<String> {
    let data_dir = tree.data_directory.as_ref()?;
    let entries = fs::read_dir(data_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        for &marker in MOD_MARKERS {
            if name.contains(marker) {
                return Some(marker.to_ascii_uppercase());
            }
        }
    }
    None
}

fn is_wanted(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let is_config = (lower.starts_with("config/") || lower.starts_with("configs/"))
        && (lower.ends_with(".ltx") || (lower.ends_with(".xml") && lower.contains("/text/")));
    let is_icon = lower.starts_with("textures/ui/ui_icon_") && lower.ends_with(".dds");
    is_config || is_icon
}

fn with_fallback_names(bundle: CatalogBundle, ui_language: &str) -> CatalogBundle {
    let embedded = match CatalogBundleReader::load_embedded().get(&bundle.release_id) {
        Some(e) => e,
        None => return bundle,
    };

    let want_cyrillic = ui_language == "ru" || ui_language == "uk";
    let mut changed = false;
    let mut items = Vec::with_capacity(bundle.items.items().len());

    for item in bundle.items.items() {
        let fallback = embedded
            .items
            .resolve(&item.key)
            .and_then(|e| e.display_name.as_deref());

        if let Some(fb) = fallback {
            let missing = item
                .display_name
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            let wrong_script = want_cyrillic && !has_cyrillic(item.display_name.as_deref()) && has_cyrillic(Some(fb));

            if missing || wrong_script {
                changed = true;
                items.push(item.with_display_name(Some(fb.to_string())));
                continue;
            }
        }
        items.push(item.clone());
    }

    if changed {
        if let Ok(updated_catalog) = ItemCatalog::new(bundle.release_id.clone(), items) {
            if let Ok(updated_bundle) = bundle.with(Some(updated_catalog), None) {
                return updated_bundle;
            }
        }
    }
    bundle
}

fn has_cyrillic(value: Option<&str>) -> bool {
    value.is_some_and(|text| text.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c)))
}

struct IconSource {
    tree: GameFileTree,
    bundle: CatalogBundle,
    cache_directory: PathBuf,
    atlases: Mutex<HashMap<String, Option<Arc<RgbaImage>>>>,
}

impl IconSource {
    fn new(tree: GameFileTree, bundle: CatalogBundle, cache_directory: PathBuf) -> Self {
        Self {
            tree,
            bundle,
            cache_directory,
            atlases: Mutex::new(HashMap::new()),
        }
    }

    fn png(&self, item_key: &str) -> Option<Vec<u8>> {
        let item = self.bundle.items.resolve(item_key)?;
        let icon_x = item.icon_x?;
        let icon_y = item.icon_y?;

        let cache_file_name = format!("{}.png", GameContentService::icon_cache_file_name(item_key));
        let cache_path = self.cache_directory.join(cache_file_name);

        if cache_path.exists() {
            if let Ok(bytes) = fs::read(&cache_path) {
                return Some(bytes);
            }
        }

        let texture_name = item.icon_texture.as_deref().unwrap_or("ui_icon_equipment");
        let atlas = self.load_atlas(texture_name)?;

        let cell = ICON_CELL_SIZE as usize;
        let crop_x = (icon_x as usize).saturating_mul(cell);
        let crop_y = (icon_y as usize).saturating_mul(cell);
        let crop_w = (item.width.unwrap_or(1).max(1) as usize).saturating_mul(cell);
        let crop_h = (item.height.unwrap_or(1).max(1) as usize).saturating_mul(cell);

        let cropped = atlas.crop(crop_x, crop_y, crop_w, crop_h)?;
        let png_bytes = GameContentService::to_png(&cropped);

        let _ = fs::create_dir_all(&self.cache_directory);
        atomic_write_file(&cache_path, &png_bytes);

        Some(png_bytes)
    }

    fn load_atlas(&self, texture: &str) -> Option<Arc<RgbaImage>> {
        let mut atlases = self.atlases.lock().ok()?;
        if let Some(cached) = atlases.get(texture) {
            return cached.clone();
        }

        let normalized = texture.replace('\\', "/").trim_matches('/').to_string();
        let relative = if normalized.contains('/') {
            format!("textures/{normalized}.dds")
        } else {
            format!("textures/ui/{normalized}.dds")
        };

        let file = self.tree.files.get(&relative)?;
        let data = file.read().ok()?;
        let decoded = DdsImage::decode(&data).ok().map(Arc::new);

        atlases.insert(texture.to_string(), decoded.clone());
        decoded
    }
}
