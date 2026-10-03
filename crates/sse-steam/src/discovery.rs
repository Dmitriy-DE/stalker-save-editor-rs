//! Steam install/library and S.T.A.L.K.E.R. 2 Auto-Cloud path discovery.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use sse_codecs::vdf::{self, Value};

use crate::api::SteamError;

/// Steam app id for S.T.A.L.K.E.R. 2.
pub const STALKER_2_APP_ID: u32 = 1_643_320;
const MAXIMUM_VDF_BYTES: u64 = 16 * 1024 * 1024;

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

/// Returns platform-default Steam roots without inspecting any save or game files.
#[must_use]
pub fn default_steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(path) = std::env::var_os("STEAM_DIR").filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        for variable in ["ProgramFiles(x86)", "ProgramFiles"] {
            if let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
                roots.push(PathBuf::from(value).join("Steam"));
            }
        }
        if let Some(value) = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()) {
            roots.push(PathBuf::from(value).join("Programs").join("Steam"));
        }
    }
    #[cfg(not(windows))]
    {
        if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
            let home = PathBuf::from(home);
            roots.push(home.join(".steam").join("steam"));
            roots.push(home.join(".steam").join("root"));
            roots.push(home.join(".local").join("share").join("Steam"));
        }
        roots.push(PathBuf::from("/usr/lib/steam"));
        roots.push(PathBuf::from("/usr/lib/steam/steam"));
    }
    roots
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

/// Finds a platform Steam API library under the supplied roots.
pub fn locate_steam_api_library(
    roots: impl IntoIterator<Item = PathBuf>,
    windows: bool,
) -> Result<Option<PathBuf>, SteamError> {
    let libraries = steam_library_roots(roots)?;
    for root in libraries {
        if !windows {
            for candidate in [root.join("steamrt64/libsteam_api.so"), root.join("libsteam_api.so")] {
                if candidate.is_file() {
                    return Ok(Some(candidate));
                }
            }
        }
        let common = root.join("steamapps").join("common");
        let Ok(entries) = fs::read_dir(&common) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let game_dir = entry.path();
            let candidate = if windows {
                game_dir.join("Binaries/Win64/steam_api64.dll")
            } else {
                game_dir.join("Binaries/Linux/libsteam_api.so")
            };
            if candidate.is_file() {
                return Ok(Some(candidate));
            }
        }
    }
    Ok(None)
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
