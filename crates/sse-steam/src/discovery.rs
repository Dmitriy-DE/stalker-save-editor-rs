//! Steam install/library and S.T.A.L.K.E.R. 2 Auto-Cloud path discovery.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use sse_codecs::vdf::{self, Value};
pub use sse_sys::steam::default_steam_roots;

use crate::api::{CloudFile, SteamError};
use crate::cloud::MAX_CLOUD_FILE_BYTES;

/// Steam app id for S.T.A.L.K.E.R. 2.
pub const STALKER_2_APP_ID: u32 = 1_643_320;
const MAXIMUM_VDF_BYTES: u64 = 16 * 1024 * 1024;
const MAXIMUM_AUTO_CLOUD_FILES: usize = 10_000;
const MAXIMUM_AUTO_CLOUD_DEPTH: usize = 32;
const WINDOWS_STEAM_API_PATHS: &[&[&str]] = &[
    &["Binaries", "Win64", "steam_api64.dll"],
    &["steam_api64.dll"],
    &["bin", "steam_api64.dll"],
    &["bin_x64", "steam_api64.dll"],
];
const UNIX_STEAM_API_PATHS: &[&[&str]] = &[
    &["Binaries", "Linux", "libsteam_api.so"],
    &["libsteam_api.so"],
    &["bin", "libsteam_api.so"],
];

/// Reads library paths from both old and current `libraryfolders.vdf` layouts.
pub fn parse_library_paths(text: &str) -> Result<Vec<PathBuf>, SteamError> {
    let document = vdf::parse(text).map_err(|error| SteamError::new(error.to_string()))?;
    let folders = document
        .get_object("libraryfolders")
        .ok_or_else(|| SteamError::new("VDF has no libraryfolders object"))?;
    let mut paths = Vec::new();
    for (key, value) in folders.children() {
        if key.parse::<u32>().is_err() {
            continue;
        }
        let path = match value {
            Value::String(path) => Some(path.as_str()),
            Value::Object(entry) => entry.get_string("path"),
        };
        if let Some(path) = path.filter(|path| !path.trim().is_empty()) {
            paths.push(PathBuf::from(path));
        }
    }
    Ok(paths)
}

/// Expands supplied Steam roots through their VDF library lists, preserving first-seen order.
pub fn steam_library_roots(roots: impl IntoIterator<Item = PathBuf>) -> Result<Vec<PathBuf>, SteamError> {
    let mut expanded = Vec::new();
    let mut seen = Vec::<PathBuf>::new();
    for root in roots {
        let normalized = absolute_lexical(&root)?;
        push_unique(&mut expanded, &mut seen, normalized.clone());
        let vdf_path = normalized.join("steamapps").join("libraryfolders.vdf");
        let Ok(file) = File::open(vdf_path) else {
            continue;
        };
        let Ok(metadata) = file.metadata() else {
            continue;
        };
        if metadata.len() > MAXIMUM_VDF_BYTES {
            continue;
        }
        let capacity = usize::try_from(metadata.len()).unwrap_or(0);
        let mut bytes = Vec::with_capacity(capacity);
        if file
            .take(MAXIMUM_VDF_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .is_err()
        {
            continue;
        }
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAXIMUM_VDF_BYTES {
            continue;
        }
        let Ok(contents) = String::from_utf8(bytes) else {
            continue;
        };
        let Ok(libraries) = parse_library_paths(&contents) else {
            continue;
        };
        for library in libraries {
            if library.is_absolute() {
                push_unique(&mut expanded, &mut seen, absolute_lexical(&library)?);
            }
        }
    }
    Ok(expanded)
}

/// Returns compatible candidate Steam API libraries for one selected app.
///
/// Windows candidates are confined to the install directory in that app's Steam manifest. Linux
/// prefers Steam's shared runtime library, then checks only the selected app's native install.
pub fn steam_api_library_candidates(
    roots: impl IntoIterator<Item = PathBuf>,
    app_id: u32,
    windows: bool,
) -> Result<Vec<PathBuf>, SteamError> {
    let libraries = steam_library_roots(roots)?;
    let mut candidates = Vec::new();
    if !windows {
        for root in &libraries {
            for candidate in [
                root.join("steamrt64").join("libsteam_api.so"),
                root.join("libsteam_api.so"),
            ] {
                if let Some(candidate) = canonical_file_under(&candidate, root) {
                    if !candidates.contains(&candidate) {
                        candidates.push(candidate);
                    }
                }
            }
        }
    }
    for library in libraries {
        let Some(install_directory) = steam_app_install_directory(&library, app_id) else {
            continue;
        };
        let app_paths = if windows {
            WINDOWS_STEAM_API_PATHS
        } else {
            UNIX_STEAM_API_PATHS
        };
        for app_path in app_paths {
            let candidate = join_path_components(&install_directory, app_path);
            if let Some(candidate) = canonical_file_under(&candidate, &install_directory) {
                if !candidates.contains(&candidate) {
                    candidates.push(candidate);
                }
            }
        }
    }
    Ok(candidates)
}

/// Finds the first compatible Steam API library for one selected app.
pub fn locate_steam_api_library(
    roots: impl IntoIterator<Item = PathBuf>,
    app_id: u32,
    windows: bool,
) -> Result<Option<PathBuf>, SteamError> {
    Ok(steam_api_library_candidates(roots, app_id, windows)?.into_iter().next())
}

fn canonical_file_under(path: &Path, root: &Path) -> Option<PathBuf> {
    let canonical_root = root.canonicalize().ok()?;
    let canonical_file = path.canonicalize().ok()?;
    if canonical_file.starts_with(&canonical_root) && canonical_file.is_file() {
        Some(canonical_file)
    } else {
        None
    }
}

fn join_path_components(root: &Path, components: &[&str]) -> PathBuf {
    components.iter().fold(root.to_path_buf(), |mut path, component| {
        path.push(component);
        path
    })
}

fn steam_app_install_directory(library: &Path, app_id: u32) -> Option<PathBuf> {
    let manifest = library.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
    let text = read_bounded_vdf(&manifest)?;
    let document = vdf::parse(&text).ok()?;
    let app_state = document.get_object("AppState")?;
    let manifest_app_id = app_state.get_string("appid")?.parse::<u32>().ok()?;
    if manifest_app_id != app_id {
        return None;
    }
    let install_directory = app_state.get_string("installdir")?;
    let mut components = Path::new(install_directory).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_))) || components.next().is_some() {
        return None;
    }
    let common_directory = library.join("steamapps").join("common").canonicalize().ok()?;
    let install_directory = common_directory.join(install_directory).canonicalize().ok()?;
    install_directory
        .starts_with(&common_directory)
        .then_some(install_directory)
}

fn read_bounded_vdf(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if metadata.len() > MAXIMUM_VDF_BYTES {
        return None;
    }
    let capacity = usize::try_from(metadata.len()).ok()?;
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(capacity).is_err()
        || file
            .take(MAXIMUM_VDF_BYTES.saturating_add(1))
            .read_to_end(&mut bytes)
            .is_err()
    {
        return None;
    }
    if u64::try_from(bytes.len()).ok()? > MAXIMUM_VDF_BYTES {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Finds the S.T.A.L.K.E.R. 2 Auto-Cloud root from injected platform roots.
pub fn find_auto_cloud_root(
    app_id: u32,
    windows_local_app_data: Option<&Path>,
    steam_libraries: impl IntoIterator<Item = PathBuf>,
) -> Option<PathBuf> {
    if app_id != STALKER_2_APP_ID {
        return None;
    }
    if let Some(root) = windows_local_app_data {
        if root.join("Stalker2").is_dir() {
            return root.canonicalize().ok().or_else(|| Some(root.to_path_buf()));
        }
    }
    for library in steam_libraries {
        let users = library
            .join("steamapps/compatdata")
            .join(STALKER_2_APP_ID.to_string())
            .join("pfx/drive_c/users");
        let Ok(entries) = fs::read_dir(users) else { continue };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let user = entry.path();
            for candidate in [user.join("Local Settings/Application Data"), user.join("AppData/Local")] {
                if candidate.join("Stalker2").is_dir() {
                    return candidate.canonicalize().ok().or(Some(candidate));
                }
            }
        }
    }
    None
}

/// Resolves a safe `Stalker2/` remote name beneath an Auto-Cloud root.
pub fn auto_cloud_path(root: &Path, remote_name: &str) -> Result<PathBuf, SteamError> {
    if remote_name.trim().is_empty() {
        return Err(SteamError::new("Auto-Cloud path is empty"));
    }
    let normalized = remote_name.replace('\\', "/");
    let segments: Vec<&str> = normalized.split('/').collect();
    if segments.len() < 2
        || segments.first().copied() != Some("Stalker2")
        || segments.iter().any(|segment| {
            segment.is_empty()
                || *segment == "."
                || *segment == ".."
                || segment.contains(':')
                || segment.chars().any(char::is_control)
        })
    {
        return Err(SteamError::new("Auto-Cloud path is not a safe Stalker2-relative name"));
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    let candidate = segments
        .iter()
        .fold(canonical_root.clone(), |path, segment| path.join(segment));
    let mut existing = candidate.as_path();
    while !existing.exists() {
        existing = existing
            .parent()
            .ok_or_else(|| SteamError::new("Auto-Cloud path has no existing parent"))?;
        if existing == canonical_root {
            break;
        }
    }
    let resolved_existing = existing
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    if !resolved_existing.starts_with(&canonical_root) || !candidate.starts_with(&canonical_root) {
        return Err(SteamError::new("Auto-Cloud path escapes its root through a symlink"));
    }
    Ok(candidate)
}

/// Lists files below a real local `Stalker2/` Auto-Cloud directory without following symlinks.
///
/// The result is bounded to 10,000 regular files and sorted by remote name.
pub fn list_auto_cloud_files(root: &Path) -> Result<Vec<CloudFile>, SteamError> {
    let (canonical_root, game_root) = canonical_auto_cloud_root(root)?;
    let mut files = Vec::new();
    collect_auto_cloud_files(&canonical_root, &game_root, &game_root, 0, &mut files)?;
    files.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(files)
}

/// Reads one local Auto-Cloud file with the same 64 MiB limit as Steam cloud frames.
///
/// The target must stay below `Stalker2/`; symlinks and non-regular files are refused. This
/// standard-library reader cannot provide descriptor-relative race protection, so callers must
/// treat these bytes as read-only input and must not use this function to authorize a write.
pub fn read_auto_cloud_file(root: &Path, remote_name: &str) -> Result<Vec<u8>, SteamError> {
    let (canonical_root, game_root) = canonical_auto_cloud_root(root)?;
    let path = auto_cloud_path(&canonical_root, remote_name)?;
    if !path.starts_with(&game_root) {
        return Err(SteamError::new("Auto-Cloud path is outside Stalker2/"));
    }
    reject_symlink_components(&canonical_root, &path)?;
    let resolved = path
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    if !resolved.starts_with(&game_root) {
        return Err(SteamError::new("Auto-Cloud file resolves outside Stalker2/"));
    }

    let file = File::open(resolved).map_err(|error| SteamError::new(error.to_string()))?;
    let metadata = file.metadata().map_err(|error| SteamError::new(error.to_string()))?;
    if !metadata.is_file() {
        return Err(SteamError::new("Auto-Cloud path is not a regular file"));
    }
    if metadata.len() > u64::try_from(MAX_CLOUD_FILE_BYTES).unwrap_or(u64::MAX) {
        return Err(SteamError::new("Auto-Cloud file exceeds the 64 MiB read limit"));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| SteamError::new("Auto-Cloud file length does not fit this platform"))?;
    let mut bytes = Vec::with_capacity(capacity);
    let read_limit = u64::try_from(MAX_CLOUD_FILE_BYTES)
        .unwrap_or(u64::MAX)
        .saturating_add(1);
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| SteamError::new(error.to_string()))?;
    let final_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if final_length != metadata.len() {
        return Err(SteamError::new("Auto-Cloud file changed while it was read"));
    }
    Ok(bytes)
}

fn canonical_auto_cloud_root(root: &Path) -> Result<(PathBuf, PathBuf), SteamError> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    let game_path = canonical_root.join("Stalker2");
    let metadata = fs::symlink_metadata(&game_path).map_err(|error| SteamError::new(error.to_string()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(SteamError::new("Stalker2/ is not a regular directory"));
    }
    let game_root = game_path
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    if !game_root.starts_with(&canonical_root) {
        return Err(SteamError::new("Stalker2/ resolves outside the Auto-Cloud root"));
    }
    Ok((canonical_root, game_root))
}

fn collect_auto_cloud_files(
    canonical_root: &Path,
    game_root: &Path,
    directory: &Path,
    depth: usize,
    files: &mut Vec<CloudFile>,
) -> Result<(), SteamError> {
    if depth > MAXIMUM_AUTO_CLOUD_DEPTH {
        return Err(SteamError::new("Auto-Cloud directory nesting exceeds the limit"));
    }
    let resolved_directory = directory
        .canonicalize()
        .map_err(|error| SteamError::new(error.to_string()))?;
    if !resolved_directory.starts_with(game_root) {
        return Err(SteamError::new("Auto-Cloud directory resolves outside Stalker2/"));
    }
    let entries = fs::read_dir(&resolved_directory).map_err(|error| SteamError::new(error.to_string()))?;
    for entry in entries {
        let entry = entry.map_err(|error| SteamError::new(error.to_string()))?;
        let file_type = entry.file_type().map_err(|error| SteamError::new(error.to_string()))?;
        if file_type.is_symlink() {
            continue;
        }
        let path = entry.path();
        if file_type.is_dir() {
            collect_auto_cloud_files(canonical_root, game_root, &path, depth.saturating_add(1), files)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        if files.len() >= MAXIMUM_AUTO_CLOUD_FILES {
            return Err(SteamError::new("Auto-Cloud listing exceeds 10,000 files"));
        }
        let metadata = entry.metadata().map_err(|error| SteamError::new(error.to_string()))?;
        let relative = path
            .strip_prefix(canonical_root)
            .map_err(|_| SteamError::new("Auto-Cloud file is outside the selected root"))?;
        let name = relative
            .to_str()
            .ok_or_else(|| SteamError::new("Auto-Cloud file name is not valid Unicode"))?
            .replace('\\', "/");
        let _ = auto_cloud_path(canonical_root, &name)?;
        let timestamp = metadata
            .modified()
            .unwrap_or(UNIX_EPOCH)
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        files.push(CloudFile {
            name,
            size: metadata.len(),
            timestamp: i64::try_from(timestamp).unwrap_or(i64::MAX),
            persisted: false,
            exists: true,
        });
    }
    Ok(())
}

fn reject_symlink_components(root: &Path, path: &Path) -> Result<(), SteamError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| SteamError::new("Auto-Cloud file is outside the selected root"))?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current).map_err(|error| SteamError::new(error.to_string()))?;
        if metadata.file_type().is_symlink() {
            return Err(SteamError::new("Auto-Cloud file path contains a symlink"));
        }
    }
    Ok(())
}

fn absolute_lexical(path: &Path) -> Result<PathBuf, SteamError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir()
            .map(|directory| directory.join(path))
            .map_err(|error| SteamError::new(error.to_string()))
    }
}

fn push_unique(paths: &mut Vec<PathBuf>, seen: &mut Vec<PathBuf>, path: PathBuf) {
    if !seen.contains(&path) {
        seen.push(path.clone());
        paths.push(path);
    }
}
