//! Save directory discovery across Steam, GOG, Epic, and retail installs.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Platform targeted during save discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveDiscoveryPlatform {
    /// Detect platform from the running operating system.
    Current,
    /// Windows conventions (registry, AppData, Documents).
    Windows,
    /// Linux conventions (~/.local, ~/.steam, Proton).
    Linux,
    /// macOS conventions (~/Library/Application Support/Steam).
    MacOS,
}

impl SaveDiscoveryPlatform {
    /// Resolves `Current` platform to the actual target operating system.
    #[must_use]
    pub fn resolve(self) -> Self {
        match self {
            Self::Current => {
                if cfg!(target_os = "windows") {
                    Self::Windows
                } else if cfg!(target_os = "macos") {
                    Self::MacOS
                } else {
                    Self::Linux
                }
            }
            other => other,
        }
    }
}

/// A candidate directory that may contain save files.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SaveDirectoryCandidate {
    /// Game family identifier ("soc", "clear_sky", "cop", "stalker2").
    pub game_id: String,
    /// Release identifier ("stalker-soc", "stalker-soc-ee", "stalker2", etc.).
    pub release_id: String,
    /// Absolute or resolved path to the save directory.
    pub directory_path: PathBuf,
}

impl SaveDirectoryCandidate {
    /// Creates a new candidate save directory entry.
    #[must_use]
    pub fn new(game_id: impl Into<String>, release_id: impl Into<String>, directory_path: impl Into<PathBuf>) -> Self {
        Self {
            game_id: game_id.into(),
            release_id: release_id.into(),
            directory_path: directory_path.into(),
        }
    }
}

/// Options configuring candidate directory discovery.
#[derive(Debug, Clone, Default)]
pub struct SaveDirectoryDiscoveryOptions {
    /// Target operating system platform.
    pub platform: Option<SaveDiscoveryPlatform>,
    /// User home directory override.
    pub home_directory: Option<PathBuf>,
    /// User profile directory override (Windows %USERPROFILE%).
    pub user_profile_directory: Option<PathBuf>,
    /// Public directory override (Windows %PUBLIC%).
    pub public_directory: Option<PathBuf>,
    /// Local AppData directory override (Windows %LOCALAPPDATA%).
    pub local_app_data_directory: Option<PathBuf>,
    /// Windows Known Folder result for FOLDERID_Documents, if obtained by the platform layer.
    pub known_documents_directory: Option<PathBuf>,
    /// Windows Known Folder result for FOLDERID_SavedGames, if obtained by the platform layer.
    pub known_saved_games_directory: Option<PathBuf>,
    /// Environment variables table override.
    pub environment: Option<HashMap<String, String>>,
    /// Steam root installation folders override.
    pub steam_roots: Option<Vec<PathBuf>>,
    /// Additional save directories explicitly configured by the user.
    pub custom_save_directories: Option<Vec<PathBuf>>,
}

#[derive(Clone, Copy)]
struct ReleaseLocation {
    family: &'static str,
    id: &'static str,
    edition: &'static str,
    app_id: u32,
    install_directories: &'static [&'static str],
}

const RELEASES: &[ReleaseLocation] = &[
    ReleaseLocation {
        family: "stalker2",
        id: "stalker2",
        edition: "s2",
        app_id: 1_643_320,
        install_directories: &[
            "S.T.A.L.K.E.R. 2 Heart of Chornobyl",
            "STALKER 2 Heart of Chornobyl",
            "S.T.A.L.K.E.R. 2",
        ],
    },
    ReleaseLocation {
        family: "soc",
        id: "stalker-soc",
        edition: "original",
        app_id: 4_500,
        install_directories: &["STALKER Shadow of Chernobyl", "STALKER Shadow of Chornobyl"],
    },
    ReleaseLocation {
        family: "clear_sky",
        id: "stalker-cs",
        edition: "original",
        app_id: 20_510,
        install_directories: &["STALKER Clear Sky"],
    },
    ReleaseLocation {
        family: "cop",
        id: "stalker-cop",
        edition: "original",
        app_id: 41_700,
        install_directories: &["Stalker Call of Pripyat", "STALKER Call of Pripyat"],
    },
    ReleaseLocation {
        family: "soc",
        id: "stalker-soc-ee",
        edition: "enhanced",
        app_id: 2_427_410,
        install_directories: &["STALKER Shadow of Chornobyl - Enhanced Edition"],
    },
    ReleaseLocation {
        family: "clear_sky",
        id: "stalker-cs-ee",
        edition: "enhanced",
        app_id: 2_427_420,
        install_directories: &["STALKER Clear Sky - Enhanced Edition"],
    },
    ReleaseLocation {
        family: "cop",
        id: "stalker-cop-ee",
        edition: "enhanced",
        app_id: 2_427_430,
        install_directories: &["STALKER Call of Prypiat - Enhanced Edition"],
    },
];

const XRAY_SAVE_FOLDERS_SOC: &[&str] = &["stalker-shoc", "Stalker-SHOC"];
const XRAY_SAVE_FOLDERS_CS: &[&str] = &["Stalker-STCS"];
const XRAY_SAVE_FOLDERS_COP: &[&str] = &["S.T.A.L.K.E.R. - Call of Pripyat", "Stalker-COP"];

const ENHANCED_SAVE_FOLDERS_SOC: &[&str] = &["STALKER Shadow of Chornobyl - EE"];
const ENHANCED_SAVE_FOLDERS_CS: &[&str] = &["STALKER Clear Sky - EE"];
const ENHANCED_SAVE_FOLDERS_COP: &[&str] = &["STALKER Call of Prypiat - EE", "STALKER Call of Pripyat - EE"];

const DOCUMENT_FOLDER_NAMES: &[&str] = &[
    "Documents",
    "My Documents",
    "Mes documents",
    "Documentos",
    "Dokumente",
    "Documenti",
    "Dokumenty",
    "Документы",
    "Документи",
    "文档",
];

const SAVED_GAMES_FOLDER_NAMES: &[&str] = &[
    "Saved Games",
    "My Saved Games",
    "Parties enregistrées",
    "Gespeicherte Spiele",
    "Partite salvate",
    "Сохраненные игры",
    "Сохранённые игры",
    "Збережені ігри",
];

const S2_PROFILES: &[&str] = &["SaveGames", "STEAM/SaveGames", "EOS/SaveGames", "GOG/SaveGames"];

/// Locates save directories according to game versions, stores, and operating systems.
pub struct SaveDirectoryLocator;

impl SaveDirectoryLocator {
    /// Returns default candidate Steam root paths for the platform.
    #[must_use]
    pub fn default_steam_roots() -> Vec<PathBuf> {
        sse_sys::steam::default_steam_roots()
    }

    /// Finds candidate save directories according to discovery options.
    #[must_use]
    pub fn find_candidate_directories(options: Option<&SaveDirectoryDiscoveryOptions>) -> Vec<SaveDirectoryCandidate> {
        let env_map = options
            .and_then(|opts| opts.environment.clone())
            .unwrap_or_else(read_environment);

        let platform = options
            .and_then(|opts| opts.platform)
            .unwrap_or(SaveDiscoveryPlatform::Current)
            .resolve();

        let (known_documents_directory, known_saved_games_directory) =
            resolve_known_folder_paths(options, platform, sse_sys::system::known_folder);

        let home = options
            .and_then(|opts| opts.home_directory.clone())
            .or_else(|| get_env(&env_map, "HOME").map(PathBuf::from))
            .unwrap_or_else(|| get_fallback_home(&env_map));

        let user_profile = options
            .and_then(|opts| opts.user_profile_directory.clone())
            .or_else(|| get_env(&env_map, "USERPROFILE").map(PathBuf::from))
            .unwrap_or_else(|| home.clone());

        let public_directory = options
            .and_then(|opts| opts.public_directory.clone())
            .or_else(|| get_env(&env_map, "PUBLIC").map(PathBuf::from));

        let local_app_data = options
            .and_then(|opts| opts.local_app_data_directory.clone())
            .or_else(|| get_env(&env_map, "LOCALAPPDATA").map(PathBuf::from))
            .unwrap_or_else(|| {
                if platform == SaveDiscoveryPlatform::Windows {
                    user_profile.join("AppData").join("Local")
                } else {
                    home.join("AppData").join("Local")
                }
            });

        let steam_roots = match options.and_then(|opts| opts.steam_roots.clone()) {
            Some(roots) => roots,
            None if options.is_none() => Self::default_steam_roots(),
            None => get_default_steam_roots(platform, &home, &local_app_data, &env_map),
        };

        let libraries = get_steam_libraries(&steam_roots);
        let mut candidates = Vec::new();
        let mut seen = HashSet::new();

        let mut doc_parents = vec![home.clone(), user_profile.clone()];
        if let Some(pub_dir) = &public_directory {
            doc_parents.push(pub_dir.clone());
        }
        if platform == SaveDiscoveryPlatform::Windows {
            for variable in ["OneDrive", "OneDriveConsumer"] {
                if let Some(directory) = get_env(&env_map, variable).filter(|value| !value.trim().is_empty()) {
                    doc_parents.push(PathBuf::from(directory));
                }
            }
        }
        let windows_paths_are_case_insensitive = platform == SaveDiscoveryPlatform::Windows;
        let document_parent_refs: Vec<&Path> = doc_parents.iter().map(PathBuf::as_path).collect();
        let mut document_roots = get_known_roots(
            &document_parent_refs,
            DOCUMENT_FOLDER_NAMES,
            windows_paths_are_case_insensitive,
        );
        let mut document_root_keys: HashSet<String> = document_roots
            .iter()
            .map(|path| known_root_key(path, windows_paths_are_case_insensitive))
            .collect();
        if let Some(path) = known_documents_directory.as_deref() {
            add_known_root(
                &mut document_roots,
                &mut document_root_keys,
                path,
                windows_paths_are_case_insensitive,
                true,
            );
        }

        let mut saved_game_roots = get_known_roots(
            &[home.as_path(), user_profile.as_path()],
            SAVED_GAMES_FOLDER_NAMES,
            windows_paths_are_case_insensitive,
        );
        let mut saved_game_root_keys: HashSet<String> = saved_game_roots
            .iter()
            .map(|path| known_root_key(path, windows_paths_are_case_insensitive))
            .collect();
        if let Some(path) = known_saved_games_directory.as_deref() {
            add_known_root(
                &mut saved_game_roots,
                &mut saved_game_root_keys,
                path,
                windows_paths_are_case_insensitive,
                true,
            );
        }

        for release in RELEASES {
            if release.edition == "original" {
                let folders = match release.family {
                    "soc" => XRAY_SAVE_FOLDERS_SOC,
                    "clear_sky" => XRAY_SAVE_FOLDERS_CS,
                    "cop" => XRAY_SAVE_FOLDERS_COP,
                    _ => &[],
                };
                for root in &document_roots {
                    for folder in folders {
                        let path = root.join(folder).join("savedgames");
                        add_candidate(&mut candidates, &mut seen, release, path);
                    }
                }
            } else if release.edition == "enhanced" {
                let folders = match release.id {
                    "stalker-soc-ee" => ENHANCED_SAVE_FOLDERS_SOC,
                    "stalker-cs-ee" => ENHANCED_SAVE_FOLDERS_CS,
                    "stalker-cop-ee" => ENHANCED_SAVE_FOLDERS_COP,
                    _ => &[],
                };
                for root in &saved_game_roots {
                    for folder in folders {
                        let steam_path = root.join(folder).join("STEAM").join("savedgames");
                        add_candidate(&mut candidates, &mut seen, release, steam_path);
                        let gog_path = root.join(folder).join("gog").join("savedgames");
                        add_candidate(&mut candidates, &mut seen, release, gog_path);
                    }
                }
            }
        }

        if let Some(s2_release) = RELEASES.first() {
            add_stalker2_local_candidates(&mut candidates, &mut seen, s2_release, &local_app_data);
            add_stalker2_package_candidates(&mut candidates, &mut seen, s2_release, &local_app_data);
        }

        for library in &libraries {
            for release in RELEASES {
                if let Some(install_dir) = find_install_directory(library, release) {
                    if let Some(fsgame_dir) = try_read_save_directory(&install_dir, release.family, &env_map) {
                        add_candidate(&mut candidates, &mut seen, release, fsgame_dir);
                    }

                    if release.edition == "original" {
                        let appdata_path = install_dir.join("_appdata_").join("savedgames");
                        add_candidate(&mut candidates, &mut seen, release, appdata_path);
                    }
                }

                if platform == SaveDiscoveryPlatform::Linux {
                    add_proton_candidates(&mut candidates, &mut seen, release, library);
                }
            }

            for release in RELEASES {
                if release.edition == "original" {
                    for install_name in release.install_directories {
                        let path = library
                            .join("steamapps")
                            .join("common")
                            .join(install_name)
                            .join("_appdata_")
                            .join("savedgames");
                        add_candidate(&mut candidates, &mut seen, release, path);
                    }
                }
            }

            if platform == SaveDiscoveryPlatform::Linux {
                if let Some(s2_release) = RELEASES.first() {
                    add_stalker2_proton_candidates(&mut candidates, &mut seen, s2_release, library);
                }
            }
        }

        for directory in options
            .and_then(|opts| opts.custom_save_directories.as_ref())
            .into_iter()
            .flatten()
        {
            if directory.as_os_str().is_empty() {
                continue;
            }

            let normalized = normalize_full_path(directory);
            let resolved = resolve_links(&normalized);
            let key = format!(
                "custom\0{}",
                known_root_key(&resolved, windows_paths_are_case_insensitive)
            );
            if seen.insert(key) {
                candidates.push(SaveDirectoryCandidate::new("unknown", "custom", resolved));
            }
        }

        candidates
    }
}

fn add_stalker2_local_candidates(
    candidates: &mut Vec<SaveDirectoryCandidate>,
    seen: &mut HashSet<String>,
    release: &ReleaseLocation,
    local_app_data: &Path,
) {
    let saved = local_app_data.join("Stalker2").join("Saved");
    for profile in S2_PROFILES {
        let root = saved.join(profile);
        add_candidate(candidates, seen, release, root.clone());
        add_candidate(candidates, seen, release, root.join("Data"));
    }
}

fn add_stalker2_package_candidates(
    candidates: &mut Vec<SaveDirectoryCandidate>,
    seen: &mut HashSet<String>,
    release: &ReleaseLocation,
    local_app_data: &Path,
) {
    let xgs = local_app_data
        .join("Packages")
        .join("GSCGameWorld.S.T.A.L.K.E.R.2HeartofChornobyl_6fr1t1rwfarwt")
        .join("SystemAppData")
        .join("xgs");

    let Ok(entries) = fs::read_dir(&xgs) else {
        return;
    };

    for entry in entries.flatten() {
        if let Ok(file_type) = entry.file_type() {
            if file_type.is_dir() {
                let path = entry.path().join("SaveGames");
                add_candidate(candidates, seen, release, path);
            }
        }
    }
}

fn add_proton_candidates(
    candidates: &mut Vec<SaveDirectoryCandidate>,
    seen: &mut HashSet<String>,
    release: &ReleaseLocation,
    library: &Path,
) {
    if release.family == "stalker2" {
        add_stalker2_proton_candidates(candidates, seen, release, library);
        return;
    }

    let drive_c = library
        .join("steamapps")
        .join("compatdata")
        .join(release.app_id.to_string())
        .join("pfx")
        .join("drive_c");

    let users = get_proton_user_directories(&drive_c);
    let folders = match release.family {
        "soc" => XRAY_SAVE_FOLDERS_SOC,
        "clear_sky" => XRAY_SAVE_FOLDERS_CS,
        "cop" => XRAY_SAVE_FOLDERS_COP,
        _ => &[],
    };

    for user in &users {
        for folder in folders {
            let doc_path = user.join("Documents").join(folder).join("savedgames");
            add_candidate(candidates, seen, release, doc_path);
            let prog_path = drive_c
                .join("ProgramData")
                .join("Documents")
                .join(folder)
                .join("savedgames");
            add_candidate(candidates, seen, release, prog_path);
        }

        if release.edition == "enhanced" {
            let ee_folders = match release.id {
                "stalker-soc-ee" => ENHANCED_SAVE_FOLDERS_SOC,
                "stalker-cs-ee" => ENHANCED_SAVE_FOLDERS_CS,
                "stalker-cop-ee" => ENHANCED_SAVE_FOLDERS_COP,
                _ => &[],
            };
            for folder in ee_folders {
                let steam_path = user.join("Saved Games").join(folder).join("STEAM").join("savedgames");
                add_candidate(candidates, seen, release, steam_path);
                let gog_path = user.join("Saved Games").join(folder).join("gog").join("savedgames");
                add_candidate(candidates, seen, release, gog_path);
            }
        }
    }
}

fn add_stalker2_proton_candidates(
    candidates: &mut Vec<SaveDirectoryCandidate>,
    seen: &mut HashSet<String>,
    release: &ReleaseLocation,
    library: &Path,
) {
    let drive_c = library
        .join("steamapps")
        .join("compatdata")
        .join(release.app_id.to_string())
        .join("pfx")
        .join("drive_c");

    let users = get_proton_user_directories(&drive_c);
    for user in &users {
        for local in &[
            user.join("AppData").join("Local"),
            user.join("Local Settings").join("Application Data"),
        ] {
            let saved = local.join("Stalker2").join("Saved");
            for profile in S2_PROFILES {
                let root = saved.join(profile);
                add_candidate(candidates, seen, release, root.clone());
                add_candidate(candidates, seen, release, root.join("Data"));
            }
        }
    }
}

fn get_proton_user_directories(drive_c: &Path) -> Vec<PathBuf> {
    let users_root = drive_c.join("users");
    let mut users = Vec::new();
    if let Ok(entries) = fs::read_dir(&users_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                users.push(path);
            }
        }
    }
    let public_profile = users_root.join("Public");
    if !users.contains(&public_profile) {
        users.push(public_profile);
    }
    users
}

fn find_install_directory(library: &Path, release: &ReleaseLocation) -> Option<PathBuf> {
    if let Some(manifest_path) = get_manifest_install_directory(library, release.app_id) {
        if manifest_path.is_dir() {
            return Some(manifest_path);
        }
    }

    let common = library.join("steamapps").join("common");
    for directory_name in release.install_directories {
        let candidate = common.join(directory_name);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }

    let entries = fs::read_dir(&common).ok()?;
    for entry in entries.flatten() {
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                for dir in release.install_directories {
                    if file_name_str.eq_ignore_ascii_case(dir) {
                        return Some(entry.path());
                    }
                }
            }
        }
    }
    None
}

fn get_manifest_install_directory(library_root: &Path, app_id: u32) -> Option<PathBuf> {
    let manifest = library_root.join("steamapps").join(format!("appmanifest_{app_id}.acf"));

    let content = fs::read_to_string(&manifest).ok()?;
    let parsed_app_id = parse_acf_string_value(&content, "appid")?;
    if parsed_app_id.trim() != app_id.to_string() {
        return None;
    }
    let install_dir = parse_acf_string_value(&content, "installdir")?;
    if install_dir.trim().is_empty() {
        return None;
    }
    Some(library_root.join("steamapps").join("common").join(install_dir.trim()))
}

fn extract_quoted_strings(line: &str) -> Vec<&str> {
    line.split('"')
        .enumerate()
        .filter(|(idx, _)| idx % 2 == 1)
        .map(|(_, s)| s)
        .collect()
}

fn parse_acf_string_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        let quotes = extract_quoted_strings(line);
        for (idx, &token) in quotes.iter().enumerate() {
            if token.eq_ignore_ascii_case(key) {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn get_steam_libraries(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    let mut seen = HashSet::new();

    for root in steam_roots {
        if root.as_os_str().is_empty() {
            continue;
        }
        let full_root = normalize_full_path(root);
        let resolved_root = resolve_links(&full_root);
        if !resolved_root.is_dir() {
            continue;
        }
        let key = resolved_root.to_string_lossy().to_string();
        if seen.insert(key) {
            libraries.push(resolved_root.clone());
        }

        let vdf_file = resolved_root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(vdf_text) = fs::read_to_string(&vdf_file) {
            for path_str in parse_vdf_library_paths(&vdf_text) {
                let full_path = normalize_full_path(Path::new(&path_str));
                let resolved_path = resolve_links(&full_path);
                if resolved_path.is_dir() {
                    let path_key = resolved_path.to_string_lossy().to_string();
                    if seen.insert(path_key) {
                        libraries.push(resolved_path);
                    }
                }
            }
        }
    }

    libraries
}

fn parse_vdf_library_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let quotes = extract_quoted_strings(line);
        for (idx, &token) in quotes.iter().enumerate() {
            if token.eq_ignore_ascii_case("path") {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    paths.push(val.replace("\\\\", "\\"));
                }
            } else if token.chars().all(|c| c.is_ascii_digit()) {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    if val.contains('/') || val.contains('\\') {
                        paths.push(val.replace("\\\\", "\\"));
                    }
                }
            }
        }
    }
    paths
}

fn try_read_save_directory(
    install_directory: &Path,
    game_id: &str,
    environment: &HashMap<String, String>,
) -> Option<PathBuf> {
    let filenames: &[&str] = if game_id == "soc" {
        &["fsgame_soc.ltx", "fsgame.ltx"]
    } else {
        &["fsgame.ltx"]
    };

    for &filename in filenames {
        let path = install_directory.join(filename);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };

        let mut definitions = HashMap::new();
        for line in text.lines() {
            let code = line.split(';').next().unwrap_or("").trim();
            if let Some((lhs, rhs)) = code.split_once('=') {
                let key = lhs.trim().to_ascii_lowercase();
                if key.starts_with('$') && key.ends_with('$') {
                    let parts: Vec<String> = rhs.split('|').map(|s| s.trim().trim_matches('"').to_string()).collect();
                    if parts.len() >= 4 {
                        definitions.insert(key, parts);
                    }
                }
            }
        }

        let mut stack = HashSet::new();
        if let Some(resolved) =
            resolve_fsgame_alias("$game_saves$", &definitions, install_directory, environment, &mut stack)
        {
            return Some(resolved);
        }
    }
    None
}

fn resolve_fsgame_alias(
    alias: &str,
    definitions: &HashMap<String, Vec<String>>,
    install_directory: &Path,
    environment: &HashMap<String, String>,
    stack: &mut HashSet<String>,
) -> Option<PathBuf> {
    let alias_lower = alias.to_ascii_lowercase();
    if alias_lower == "$fs_root$" {
        return Some(install_directory.to_path_buf());
    }

    if !stack.insert(alias_lower.clone()) {
        return None;
    }

    let values = definitions.get(&alias_lower)?;
    let parent_val_raw = values.get(2)?;
    let child_val_raw = values.get(3)?;

    let parent_val = expand_env_vars(parent_val_raw, environment);
    let parent_path = if parent_val.starts_with('$') && parent_val.ends_with('$') {
        resolve_fsgame_alias(&parent_val, definitions, install_directory, environment, stack)?
    } else {
        let norm = normalize_relative_path(&parent_val);
        if Path::new(&norm).is_absolute() {
            norm
        } else {
            install_directory.join(norm)
        }
    };

    stack.remove(&alias_lower);

    let child_val = expand_env_vars(child_val_raw, environment);
    let child_norm = normalize_relative_path(&child_val);
    Some(normalize_full_path(&parent_path.join(child_norm)))
}

fn expand_env_vars(value: &str, environment: &HashMap<String, String>) -> String {
    let mut result = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '%' {
            let mut var_name = String::new();
            let mut closed = false;
            for next_ch in chars.by_ref() {
                if next_ch == '%' {
                    closed = true;
                    break;
                }
                var_name.push(next_ch);
            }
            if closed {
                if let Some(val) = get_env(environment, &var_name) {
                    result.push_str(val);
                } else {
                    result.push('%');
                    result.push_str(&var_name);
                    result.push('%');
                }
            } else {
                result.push('%');
                result.push_str(&var_name);
            }
        } else if ch == '$' {
            let mut var_name = String::new();
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    var_name.push(next_ch);
                    chars.next();
                } else {
                    break;
                }
            }
            if var_name.is_empty() {
                result.push('$');
            } else if let Some(val) = get_env(environment, &var_name) {
                result.push_str(val);
            } else {
                result.push('$');
                result.push_str(&var_name);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn normalize_relative_path(path: &str) -> PathBuf {
    let trimmed = path.trim_matches(['\\', '/']);
    let mut buf = PathBuf::new();
    for part in trimmed.split(['\\', '/']) {
        if !part.is_empty() && part != "." {
            buf.push(part);
        }
    }
    buf
}

fn get_known_roots(parents: &[&Path], known_names: &[&str], case_insensitive: bool) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut seen = HashSet::new();

    for parent in parents {
        if parent.as_os_str().is_empty() {
            continue;
        }

        for name in known_names {
            add_known_root(&mut roots, &mut seen, &parent.join(name), case_insensitive, false);
        }

        if let Ok(entries) = fs::read_dir(parent) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                for known in known_names {
                    if file_name_str.eq_ignore_ascii_case(known) {
                        add_known_root(&mut roots, &mut seen, &entry.path(), case_insensitive, false);
                    }
                }
            }
        }
    }

    roots
}

fn resolve_known_folder_paths(
    options: Option<&SaveDirectoryDiscoveryOptions>,
    platform: SaveDiscoveryPlatform,
    mut query: impl FnMut(sse_sys::system::KnownFolder) -> Option<PathBuf>,
) -> (Option<PathBuf>, Option<PathBuf>) {
    if platform != SaveDiscoveryPlatform::Windows {
        return (None, None);
    }
    if let Some(options) = options {
        return (
            options.known_documents_directory.clone(),
            options.known_saved_games_directory.clone(),
        );
    }
    (
        query(sse_sys::system::KnownFolder::Documents),
        query(sse_sys::system::KnownFolder::SavedGames),
    )
}

fn add_known_root(
    roots: &mut Vec<PathBuf>,
    seen: &mut HashSet<String>,
    path: &Path,
    case_insensitive: bool,
    require_absolute: bool,
) {
    if path.as_os_str().is_empty() || (require_absolute && !path.is_absolute()) {
        return;
    }
    let normalized = normalize_full_path(path);
    let key = known_root_key(&normalized, case_insensitive);
    if seen.insert(key) {
        roots.push(normalized);
    }
}

fn known_root_key(path: &Path, case_insensitive: bool) -> String {
    if case_insensitive {
        path.to_string_lossy().replace('/', "\\").to_ascii_lowercase()
    } else {
        path.to_string_lossy().into_owned()
    }
}

fn add_candidate(
    candidates: &mut Vec<SaveDirectoryCandidate>,
    seen: &mut HashSet<String>,
    release: &ReleaseLocation,
    path: PathBuf,
) {
    let full_path = normalize_full_path(&path);
    let resolved_path = resolve_links(&full_path);
    let key = format!("{}\0{}", release.id, resolved_path.to_string_lossy());
    if seen.insert(key) {
        candidates.push(SaveDirectoryCandidate::new(release.family, release.id, resolved_path));
    }
}

fn get_default_steam_roots(
    platform: SaveDiscoveryPlatform,
    home: &Path,
    local_app_data: &Path,
    env_map: &HashMap<String, String>,
) -> Vec<PathBuf> {
    let platform = match platform {
        SaveDiscoveryPlatform::Windows => sse_sys::steam::SteamRootPlatform::Windows,
        SaveDiscoveryPlatform::MacOS => sse_sys::steam::SteamRootPlatform::MacOS,
        SaveDiscoveryPlatform::Linux | SaveDiscoveryPlatform::Current => sse_sys::steam::SteamRootPlatform::Linux,
    };
    let environment = env_map
        .iter()
        .map(|(key, value)| (std::ffi::OsString::from(key), std::ffi::OsString::from(value)))
        .collect();
    sse_sys::steam::steam_root_candidates(platform, home, local_app_data, &environment, None)
}

fn read_environment() -> HashMap<String, String> {
    env::vars().collect()
}

fn get_env<'a>(map: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    if let Some(val) = map.get(key) {
        return Some(val.as_str());
    }
    for (k, v) in map {
        if k.eq_ignore_ascii_case(key) {
            return Some(v.as_str());
        }
    }
    None
}

fn get_fallback_home(env_map: &HashMap<String, String>) -> PathBuf {
    if let Some(user_profile) = get_env(env_map, "USERPROFILE") {
        return PathBuf::from(user_profile);
    }
    PathBuf::from("/")
}

/// Normalizes path, trimming trailing slashes, collapsing separators.
#[must_use]
pub fn normalize_full_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

/// Resolves symbolic links component-by-component up to a maximum depth of 16.
#[must_use]
pub fn resolve_links(path: &Path) -> PathBuf {
    resolve_links_internal(path, 0)
}

pub(super) fn resolve_entry_path(directory_identity: &Path, entry: &fs::DirEntry) -> PathBuf {
    let path = normalize_full_path(&directory_identity.join(entry.file_name()));
    match entry.file_type() {
        Ok(file_type) if !file_type.is_symlink() => path,
        Ok(_) | Err(_) => resolve_links(&path),
    }
}

fn resolve_links_internal(path: &Path, depth: usize) -> PathBuf {
    if depth >= 16 {
        return path.to_path_buf();
    }

    let mut current = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(segment) => {
                current.push(segment);
                if let Ok(target) = fs::read_link(&current) {
                    let resolved_target = if target.is_absolute() {
                        target
                    } else if let Some(parent) = current.parent() {
                        parent.join(target)
                    } else {
                        target
                    };
                    current = resolve_links_internal(&resolved_target, depth.saturating_add(1));
                }
            }
        }
    }

    normalize_full_path(&current)
}

#[cfg(test)]
mod known_folder_tests {
    use super::{resolve_known_folder_paths, SaveDirectoryDiscoveryOptions, SaveDiscoveryPlatform};
    use sse_sys::system::KnownFolder;
    use std::path::PathBuf;

    #[test]
    fn default_windows_discovery_queries_both_known_folders() {
        let documents = PathBuf::from(r"C:\redirected\Documents");
        let saved_games = PathBuf::from(r"D:\redirected\Saved Games");
        let mut queried = Vec::new();

        let (actual_documents, actual_saved_games) =
            resolve_known_folder_paths(None, SaveDiscoveryPlatform::Windows, |folder| {
                queried.push(folder);
                match folder {
                    KnownFolder::Documents => Some(documents.clone()),
                    KnownFolder::SavedGames => Some(saved_games.clone()),
                }
            });

        assert_eq!(queried, [KnownFolder::Documents, KnownFolder::SavedGames]);
        assert_eq!(actual_documents, Some(documents));
        assert_eq!(actual_saved_games, Some(saved_games));
    }

    #[test]
    fn explicit_discovery_options_do_not_query_host_known_folders() {
        let documents = PathBuf::from(r"C:\test-profile\Documents");
        let saved_games = PathBuf::from(r"C:\test-profile\Saved Games");
        let options = SaveDirectoryDiscoveryOptions {
            platform: Some(SaveDiscoveryPlatform::Windows),
            known_documents_directory: Some(documents.clone()),
            known_saved_games_directory: Some(saved_games.clone()),
            ..SaveDirectoryDiscoveryOptions::default()
        };

        let (actual_documents, actual_saved_games) =
            resolve_known_folder_paths(Some(&options), SaveDiscoveryPlatform::Windows, |_| {
                panic!("explicit test options must not query the host")
            });

        assert_eq!(actual_documents, Some(documents));
        assert_eq!(actual_saved_games, Some(saved_games));
    }
}

#[cfg(test)]
mod steam_roots_tests {
    use super::{get_default_steam_roots, SaveDiscoveryPlatform};
    use std::collections::HashMap;
    use std::path::PathBuf;

    #[test]
    fn linux_steam_roots_include_environment_root_and_standard_install_locations() {
        let home = PathBuf::from("/home/tester");
        let local_app_data = home.join("AppData/Local");
        let environment = HashMap::from([(String::from("STEAM_DIR"), String::from("/opt/Steam"))]);
        let roots = get_default_steam_roots(SaveDiscoveryPlatform::Linux, &home, &local_app_data, &environment);

        for expected in [
            PathBuf::from("/opt/Steam"),
            home.join(".steam/root"),
            PathBuf::from("/usr/lib/steam"),
            PathBuf::from("/usr/lib/steam/steam"),
        ] {
            assert!(roots.contains(&expected), "missing Steam root {expected:?}: {roots:?}");
        }
    }
}
