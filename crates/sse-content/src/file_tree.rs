//! Game data file tree and archive locator.
//!
//! Provides the engine view of game data: archives parsed in LocatorAPI order
//! with loose `gamedata` files overlayed on top.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use sse_core::{Error, Result};

use crate::archive::{EntryDecoder, HeaderDecoder, XRayArchive, XRayArchiveEntry};
use sse_codecs::sha256::sha256_hex;

/// Supported X-Ray game trilogy titles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompanionGame {
    /// S.T.A.L.K.E.R.: Shadow of Chernobyl
    ShadowOfChernobyl,
    /// S.T.A.L.K.E.R.: Clear Sky
    ClearSky,
    /// S.T.A.L.K.E.R.: Call of Pripyat
    CallOfPripyat,
}

/// One logical file in the game's data tree.
#[derive(Clone)]
pub struct GameFile {
    /// Relative path normalized with forward slashes (e.g. `configs/system.ltx`).
    pub relative_path: String,
    /// Origin: archive path or `"gamedata"` for loose files.
    pub origin: String,
    read_fn: Arc<dyn Fn() -> Result<Vec<u8>> + Send + Sync>,
}

impl GameFile {
    /// Creates a new game file with a reader callback.
    #[must_use]
    pub fn new(
        relative_path: impl Into<String>,
        origin: impl Into<String>,
        read_fn: impl Fn() -> Result<Vec<u8>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            relative_path: relative_path.into(),
            origin: origin.into(),
            read_fn: Arc::new(read_fn),
        }
    }

    /// Creates a game file with preloaded bytes.
    #[must_use]
    pub fn from_bytes(relative_path: impl Into<String>, origin: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self::new(relative_path, origin, move || Ok(bytes.clone()))
    }

    /// Reads file bytes.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] or [`Error::System`] on failure.
    pub fn read(&self) -> Result<Vec<u8>> {
        (self.read_fn)()
    }
}

/// The composite view of an installed game's data.
#[derive(Clone)]
pub struct GameFileTree {
    /// Map of normalized relative paths to game files.
    pub files: HashMap<String, GameFile>,
    /// SHA-256 fingerprint of the install and files.
    pub fingerprint: String,
    /// True when loose files in `gamedata` override at least one wanted path.
    pub has_loose_overlay: bool,
    /// Config prefix relative to `$game_data$` with trailing slash (e.g. `"config/"` or `"configs/"`).
    pub config_prefix: String,
    /// Absolute path to `$game_data$` folder if resolved.
    pub data_directory: Option<PathBuf>,
    /// Warnings or issues encountered during discovery.
    pub issues: Vec<String>,
}

impl GameFileTree {
    /// Simplified loader with default options.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Damaged`] on failure.
    pub fn load_simple(
        game: CompanionGame,
        game_directory: impl AsRef<Path>,
        wanted: impl Fn(&str) -> bool,
        defer_archive_content: bool,
    ) -> Result<Self> {
        Self::load(
            game,
            game_directory,
            wanted,
            None,
            true,
            defer_archive_content,
            false,
            None,
            None,
        )
    }
    /// Loads the game file tree according to `fsgame.ltx` and wanted paths.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Damaged`] on failure.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        game: CompanionGame,
        game_directory: impl AsRef<Path>,
        wanted: impl Fn(&str) -> bool,
        fsgame_file_names: Option<&[&str]>,
        include_loose_files: bool,
        defer_archive_content: bool,
        archives_only: bool,
        header_decoder: Option<HeaderDecoder>,
        entry_decoder: Option<EntryDecoder>,
    ) -> Result<Self> {
        let game_dir = game_directory.as_ref();
        let default_fsgame = ["fsgame.ltx"];
        let fsgame_names = fsgame_file_names.unwrap_or(&default_fsgame);

        let search = CompanionArchiveLocator::discover(game_dir, fsgame_names, game);
        let mut issues = search.issues;
        let mut files: HashMap<String, GameFile> = HashMap::new();
        let mut stamp = String::new();

        let full_game_dir = game_dir.canonicalize().unwrap_or_else(|_| game_dir.to_path_buf());
        let game_dir_str = full_game_dir
            .to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_string();
        stamp.push_str(&format!("R|{game_dir_str}\n"));

        for archive_path in &search.archive_paths {
            let meta = match fs::metadata(archive_path) {
                Ok(m) => m,
                Err(e) => {
                    issues.push(format!(
                        "Could not read archive {}: {e}",
                        archive_path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    continue;
                }
            };

            let file_name = archive_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let length = meta.len();
            let ticks = file_time_ticks(&meta);
            stamp.push_str(&format!("A|{file_name}|{length}|{ticks}\n"));

            let table_res = XRayArchive::open_with_decoder(archive_path, header_decoder.clone(), entry_decoder.clone());
            let archive = match table_res {
                Ok(a) => a,
                Err(e) => {
                    issues.push(format!("Could not read archive {file_name}: {e}"));
                    continue;
                }
            };

            let wanted_entries: Vec<XRayArchiveEntry> = archive
                .entries()
                .iter()
                .filter(|entry| {
                    let rel = normalize_game_path(&entry.name);
                    !rel.is_empty() && !rel.ends_with('/') && wanted(&rel)
                })
                .cloned()
                .collect();

            if wanted_entries.is_empty() {
                continue;
            }

            let archive = Arc::new(archive);
            for entry in wanted_entries {
                let relative = normalize_game_path(&entry.name);
                stamp.push_str(&format!(
                    "E|{}|{}|{}|{}|{}\n",
                    relative, entry.uncompressed_size, entry.compressed_size, entry.crc32, entry.offset
                ));

                let origin = archive_path.to_string_lossy().to_string();
                let file = if defer_archive_content {
                    let arc = Arc::clone(&archive);
                    let cap_entry_name = entry.name.clone();
                    GameFile::new(relative.clone(), origin, move || arc.read_file(&cap_entry_name))
                } else {
                    match archive.read_file(&entry.name) {
                        Ok(bytes) => GameFile::from_bytes(relative.clone(), origin, bytes),
                        Err(e) => {
                            issues.push(format!("Could not read {} from {file_name}: {e}", entry.name));
                            continue;
                        }
                    }
                };

                files.insert(relative, file);
            }
        }

        let mut overlay = false;
        let data_root = search.game_data_directory.as_ref();
        if include_loose_files && !archives_only {
            if let Some(root) = data_root {
                if root.is_dir() {
                    let mut loose_paths = Vec::new();
                    enumerate_files_recursive(root, &mut loose_paths);
                    loose_paths.sort_by_key(|a| a.to_string_lossy().to_ascii_lowercase());

                    for path in loose_paths {
                        let Ok(rel_path) = path.strip_prefix(root) else {
                            continue;
                        };
                        let relative = normalize_game_path(&rel_path.to_string_lossy());
                        if !wanted(&relative) {
                            continue;
                        }

                        let meta = match fs::metadata(&path) {
                            Ok(m) => m,
                            Err(_) => continue,
                        };
                        let length = meta.len();
                        let ticks = file_time_ticks(&meta);
                        stamp.push_str(&format!("L|{relative}|{length}|{ticks}\n"));

                        if !relative.to_ascii_lowercase().contains("save_editor") {
                            overlay = true;
                        }

                        let captured_path = path.clone();
                        let file = GameFile::new(relative.clone(), "gamedata", move || {
                            fs::read(&captured_path).map_err(|e| Error::System(e.to_string()))
                        });
                        files.insert(relative, file);
                    }
                }
            }
        }

        let fingerprint = sha256_hex(stamp.as_bytes());
        let mut config_prefix = if game == CompanionGame::ShadowOfChernobyl {
            "config/".to_string()
        } else {
            "configs/".to_string()
        };

        if let (Some(cfg_dir), Some(root)) = (&search.game_config_directory, data_root) {
            if let Ok(rel) = cfg_dir.strip_prefix(root) {
                let norm = normalize_game_path(&rel.to_string_lossy())
                    .trim_end_matches('/')
                    .to_string();
                if !norm.is_empty() && !norm.starts_with("..") {
                    config_prefix = format!("{norm}/");
                }
            }
        }

        Ok(Self {
            files,
            fingerprint,
            has_loose_overlay: overlay,
            config_prefix,
            data_directory: search.game_data_directory,
            issues,
        })
    }
}

/// Archive and path discovery result.
pub struct CompanionArchiveSearchResult {
    /// Ordered list of archive file paths.
    pub archive_paths: Vec<PathBuf>,
    /// Warnings or issues encountered.
    pub issues: Vec<String>,
    /// Path to `fsgame.ltx` if found.
    pub fsgame_path: Option<PathBuf>,
    /// Resolved `$game_data$` directory.
    pub game_data_directory: Option<PathBuf>,
    /// Resolved `$game_config$` directory.
    pub game_config_directory: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct AliasDefinition {
    name: String,
    parent: String,
    relative_path: String,
    recursive: bool,
    order: usize,
}

/// Discovers archive paths using `fsgame.ltx` definitions.
pub struct CompanionArchiveLocator;

impl CompanionArchiveLocator {
    /// Discovers archives according to `fsgame.ltx`.
    #[must_use]
    pub fn discover(
        game_directory: &Path,
        fsgame_file_names: &[&str],
        game: CompanionGame,
    ) -> CompanionArchiveSearchResult {
        let mut fsgame_path = None;
        for name in fsgame_file_names {
            let p = game_directory.join(name);
            if p.is_file() {
                fsgame_path = Some(p);
                break;
            }
        }

        let Some(fsgame) = fsgame_path else {
            return CompanionArchiveSearchResult {
                archive_paths: Vec::new(),
                issues: vec![format!(
                    "Could not find fsgame.ltx for archive discovery in {}",
                    game_directory.display()
                )],
                fsgame_path: None,
                game_data_directory: None,
                game_config_directory: None,
            };
        };

        let contents = match fs::read_to_string(&fsgame) {
            Ok(s) => s,
            Err(e) => {
                let fsgame_name = fsgame.file_name().unwrap_or_default().to_string_lossy();
                return CompanionArchiveSearchResult {
                    archive_paths: Vec::new(),
                    issues: vec![format!("Could not read {fsgame_name}: {e}")],
                    fsgame_path: Some(fsgame),
                    game_data_directory: None,
                    game_config_directory: None,
                };
            }
        };

        let fsgame_name = fsgame.file_name().unwrap_or_default().to_string_lossy().to_string();
        let (definitions, parse_issues) = parse_aliases(&contents, &fsgame_name);
        let mut issues = parse_issues;

        let mut resolving = HashSet::new();
        let game_data_directory = resolve_alias("$game_data$", &definitions, game_directory, &mut resolving);
        let game_config_directory = resolve_alias("$game_config$", &definitions, game_directory, &mut resolving);

        if game_data_directory.is_none() {
            issues.push(format!(
                "Could not resolve fsgame alias $game_data$ from {fsgame_name}."
            ));
        }
        if game_config_directory.is_none() {
            issues.push(format!(
                "Could not resolve fsgame alias $game_config$ from {fsgame_name}."
            ));
        }

        let mut archive_aliases: Vec<&AliasDefinition> = definitions
            .iter()
            .filter(|a| a.name.to_ascii_lowercase().contains("arch"))
            .collect();
        archive_aliases.sort_by_key(|a| a.order);

        if archive_aliases.is_empty() && game != CompanionGame::ShadowOfChernobyl {
            issues.push(format!("No X-Ray archive aliases were found in {fsgame_name}."));
        }

        let mut archive_paths = Vec::new();
        for alias in &archive_aliases {
            let Some(dir) = resolve_alias(&alias.name, &definitions, game_directory, &mut resolving) else {
                issues.push(format!(
                    "Could not resolve archive alias {} from {fsgame_name}.",
                    alias.name
                ));
                continue;
            };

            if !dir.is_dir() {
                continue;
            }

            let mut files = Vec::new();
            if alias.recursive {
                enumerate_files_recursive(&dir, &mut files);
            } else {
                enumerate_files_top(&dir, &mut files);
            }

            let mut valid_archives: Vec<PathBuf> = files.into_iter().filter(|p| is_xray_archive(p)).collect();

            valid_archives.sort_by(|a, b| {
                let rel_a = a.strip_prefix(&dir).unwrap_or(a).to_string_lossy();
                let rel_b = b.strip_prefix(&dir).unwrap_or(b).to_string_lossy();
                let lower_cmp = rel_a.to_ascii_lowercase().cmp(&rel_b.to_ascii_lowercase());
                if lower_cmp.is_eq() {
                    rel_a.cmp(&rel_b)
                } else {
                    lower_cmp
                }
            });

            archive_paths.extend(valid_archives);
        }

        if game == CompanionGame::ShadowOfChernobyl {
            let mut root_files = Vec::new();
            enumerate_files_top(game_directory, &mut root_files);
            let mut soc_archives: Vec<PathBuf> = root_files.into_iter().filter(|p| is_soc_root_archive(p)).collect();

            soc_archives.sort_by(|a, b| {
                let name_a = a.file_name().unwrap_or_default().to_string_lossy();
                let name_b = b.file_name().unwrap_or_default().to_string_lossy();
                let lower_cmp = name_a.to_ascii_lowercase().cmp(&name_b.to_ascii_lowercase());
                if lower_cmp.is_eq() {
                    name_a.cmp(&name_b)
                } else {
                    lower_cmp
                }
            });

            archive_paths.extend(soc_archives);
        }

        // Deduplicate preserving order
        let mut seen = HashSet::new();
        let mut ordered_archives = Vec::new();
        for path in archive_paths {
            let key = path.canonicalize().unwrap_or_else(|_| path.clone());
            if seen.insert(key) {
                ordered_archives.push(path);
            }
        }

        CompanionArchiveSearchResult {
            archive_paths: ordered_archives,
            issues,
            fsgame_path: Some(fsgame),
            game_data_directory,
            game_config_directory,
        }
    }
}

fn parse_aliases(contents: &str, fsgame_name: &str) -> (Vec<AliasDefinition>, Vec<String>) {
    let mut definitions = Vec::new();
    let mut known = HashSet::new();
    let mut errors = Vec::new();

    for (line_idx, raw_line) in contents.lines().enumerate() {
        let line_num = line_idx.saturating_add(1);
        let line = raw_line.split(';').next().unwrap_or("").trim();
        let Some(equals) = line.find('=') else {
            continue;
        };

        let name = line.get(..equals).unwrap_or("").trim().to_string();
        if !name.starts_with('$') || !name.ends_with('$') {
            continue;
        }

        let remainder = line.get(equals.saturating_add(1)..).unwrap_or("").trim();
        let fields: Vec<&str> = remainder.split('|').map(|f| f.trim().trim_matches('"')).collect();

        if fields.len() < 3 {
            if name.to_ascii_lowercase().contains("arch") {
                errors.push(format!(
                    "Invalid archive alias {name} on line {line_num} of {fsgame_name}."
                ));
            }
            continue;
        }

        let recursive = fields
            .first()
            .copied()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case("true");
        let parent = fields.get(2).copied().unwrap_or("").trim().to_string();
        let rel_path = fields.get(3).copied().unwrap_or("").trim().to_string();

        if !known.insert(name.to_ascii_lowercase()) {
            if name.to_ascii_lowercase().contains("arch") {
                errors.push(format!("Duplicate archive alias {name} in {fsgame_name}."));
            }
            continue;
        }

        definitions.push(AliasDefinition {
            name,
            parent,
            relative_path: rel_path,
            recursive,
            order: line_num,
        });
    }

    (definitions, errors)
}

fn resolve_alias(
    alias_name: &str,
    definitions: &[AliasDefinition],
    game_directory: &Path,
    resolving: &mut HashSet<String>,
) -> Option<PathBuf> {
    if alias_name.eq_ignore_ascii_case("$fs_root$") {
        return Some(
            game_directory
                .canonicalize()
                .unwrap_or_else(|_| game_directory.to_path_buf()),
        );
    }

    let alias = definitions.iter().find(|d| d.name.eq_ignore_ascii_case(alias_name))?;

    let lower_name = alias.name.to_ascii_lowercase();
    if !resolving.insert(lower_name.clone()) {
        return None;
    }

    let parent_path: Option<PathBuf> = if alias.parent.starts_with('$') && alias.parent.ends_with('$') {
        resolve_alias(&alias.parent, definitions, game_directory, resolving)
    } else {
        let norm_parent = normalize_path_separators(&alias.parent);
        let p = Path::new(&norm_parent);
        if p.is_absolute() {
            Some(p.to_path_buf())
        } else {
            Some(game_directory.join(p))
        }
    };

    resolving.remove(&lower_name);

    let parent = parent_path?;
    let rel = normalize_path_separators(&alias.relative_path);
    let resolved = if rel.is_empty() { parent } else { parent.join(rel) };

    Some(resolved.canonicalize().unwrap_or(resolved))
}

fn is_xray_archive(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".db") || lower.ends_with(".xdb") || lower.ends_with(".xrp") {
        return true;
    }
    if let Some(marker) = lower.rfind(".db") {
        let suffix = lower.get(marker.saturating_add(3)..).unwrap_or("");
        return is_archive_suffix(suffix);
    }
    false
}

fn is_soc_root_archive(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|f| f.to_str()) else {
        return false;
    };
    let lower = file_name.to_ascii_lowercase();
    if let Some(suffix) = lower.strip_prefix("gamedata.db") {
        return is_archive_suffix(suffix);
    }
    false
}

fn is_archive_suffix(suffix: &str) -> bool {
    if suffix.is_empty() || suffix.chars().all(|c| c.is_ascii_digit()) {
        return true;
    }
    if suffix.len() == 1 {
        let ch = suffix.chars().next().unwrap_or('\0').to_ascii_lowercase();
        return ('a'..='d').contains(&ch);
    }
    false
}

fn normalize_game_path(name: &str) -> String {
    let val = name.replace('\\', "/").trim_start_matches('/').to_string();
    if let Some(stripped) = val.strip_prefix("gamedata/") {
        stripped.to_string()
    } else if let Some(stripped) = val.strip_prefix("GAMEDATA/") {
        stripped.to_string()
    } else {
        val
    }
}

fn normalize_path_separators(path: &str) -> String {
    path.replace('\\', "/").trim().to_string()
}

fn enumerate_files_top(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() {
                files.push(p);
            }
        }
    }
}

fn enumerate_files_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                enumerate_files_recursive(&p, files);
            } else if p.is_file() {
                files.push(p);
            }
        }
    }
}

fn file_time_ticks(meta: &fs::Metadata) -> u64 {
    let mtime = meta.modified().unwrap_or(UNIX_EPOCH);
    let duration = mtime.duration_since(UNIX_EPOCH).unwrap_or_default();
    const TICKS_PER_SEC: u64 = 10_000_000;
    const TICKS_TO_UNIX_EPOCH: u64 = 621_355_968_000_000_000;
    let sec_ticks = duration.as_secs().wrapping_mul(TICKS_PER_SEC);
    let subsec_ticks = u64::from(duration.subsec_nanos() / 100);
    TICKS_TO_UNIX_EPOCH.wrapping_add(sec_ticks).wrapping_add(subsec_ticks)
}
