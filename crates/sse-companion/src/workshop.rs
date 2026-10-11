//! Steam Workshop package identifiers and read-only installation status checks.
use std::path::{Path, PathBuf};

use crate::bundled::Game;

/// Largest subscribed archive that is read for the status check.
const MAX_ARCHIVE_BYTES: u64 = 64 * 1024 * 1024;

/// Workshop package metadata for one Enhanced Edition companion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnhancedEditionPackage {
    /// Steam application ID whose Workshop content directory contains the item.
    pub app_id: u32,
    /// Published Workshop item ID. Kept empty until the owner publishes the package.
    pub published_file_id: Option<&'static str>,
    /// X-Ray archive that must be present in the subscribed item directory.
    pub archive_name: &'static str,
    /// The game whose bundled mod files the archive carries.
    pub game: Game,
}

/// Three Enhanced Edition Workshop slots, intentionally unpublished until the owner assigns IDs.
pub const EE_WORKSHOP_PACKAGES: [EnhancedEditionPackage; 3] = [
    EnhancedEditionPackage {
        app_id: 2_427_410,
        published_file_id: None,
        archive_name: "save_editor_companion_soc.pack",
        game: Game::ShadowOfChernobyl,
    },
    EnhancedEditionPackage {
        app_id: 2_427_420,
        published_file_id: None,
        archive_name: "save_editor_companion_cs.pack",
        game: Game::ClearSky,
    },
    EnhancedEditionPackage {
        app_id: 2_427_430,
        published_file_id: None,
        archive_name: "save_editor_companion_cop.pack",
        game: Game::CallOfPripyat,
    },
];

/// Result of checking a Workshop item under the selected Steam library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkshopInstallState {
    /// The owner has not published an item ID yet.
    NotPublished,
    /// The selected installation is outside a Steam `steamapps/common` directory.
    NotSteamInstall,
    /// The item directory does not exist for the selected Steam app.
    NotSubscribed,
    /// The subscribed archive carries every mod file of this editor, byte for byte.
    UpToDate,
    /// The item exists, but its archive is missing, unreadable, or carries other mod files.
    Outdated,
}

/// Workshop package for a selected release identifier (`stalker-soc-ee`, and so on).
#[must_use]
pub fn package_for_release(release: &str) -> Option<&'static EnhancedEditionPackage> {
    match release {
        "soc-ee" | "stalker-soc-ee" => EE_WORKSHOP_PACKAGES.first(),
        "cs-ee" | "stalker-cs-ee" => EE_WORKSHOP_PACKAGES.get(1),
        "cop-ee" | "stalker-cop-ee" => EE_WORKSHOP_PACKAGES.get(2),
        _ => None,
    }
}

/// Read-only status check for the Workshop package belonging to the selected game install.
///
/// A subscribed item holds only the archive (GSC's uploader takes nothing but `.pack` and `.mp4` files), so the
/// check opens the archive and compares the mod files in it with the ones this editor ships.
#[must_use]
pub fn inspect_ee_install(install_directory: &Path, package: &EnhancedEditionPackage) -> WorkshopInstallState {
    let Some(steamapps) = steamapps_root(install_directory) else {
        return WorkshopInstallState::NotSteamInstall;
    };
    let Some(item_id) = package.published_file_id else {
        return WorkshopInstallState::NotPublished;
    };
    if item_id.parse::<u64>().ok().filter(|id| *id > 0).is_none() {
        return WorkshopInstallState::NotPublished;
    }
    let item_directory = steamapps
        .join("workshop/content")
        .join(package.app_id.to_string())
        .join(item_id);
    let Ok(item_metadata) = std::fs::symlink_metadata(&item_directory) else {
        return WorkshopInstallState::NotSubscribed;
    };
    if !item_metadata.file_type().is_dir() {
        return WorkshopInstallState::Outdated;
    }
    if archive_carries_shipped_files(&item_directory.join(package.archive_name), package.game) {
        WorkshopInstallState::UpToDate
    } else {
        WorkshopInstallState::Outdated
    }
}

fn archive_carries_shipped_files(archive_path: &Path, game: Game) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(archive_path) else {
        return false;
    };
    if !metadata.file_type().is_file() || metadata.len() > MAX_ARCHIVE_BYTES {
        return false;
    }
    let Ok(bytes) = std::fs::read(archive_path) else {
        return false;
    };
    let Ok(archive) = sse_content::XRayArchive::from_slice(&bytes, None, None) else {
        return false;
    };
    let Ok(shipped) = crate::bundled::ee_payloads(game) else {
        return false;
    };
    shipped.iter().all(|file| {
        archive
            .read_file(&file.relative_path)
            .is_ok_and(|found| found == file.bytes)
    })
}

/// Build a Steam Community URL only for a positive numeric published item ID.
#[must_use]
pub fn workshop_page_url(item_id: &str) -> Option<String> {
    item_id
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)
        .map(|_| format!("https://steamcommunity.com/sharedfiles/filedetails/?id={item_id}"))
}

fn steamapps_root(install_directory: &Path) -> Option<PathBuf> {
    install_directory.ancestors().find_map(|ancestor| {
        let common = ancestor.file_name()?;
        if !common.to_string_lossy().eq_ignore_ascii_case("common") {
            return None;
        }
        let steamapps = ancestor.parent()?;
        if !steamapps
            .file_name()?
            .to_string_lossy()
            .eq_ignore_ascii_case("steamapps")
        {
            return None;
        }
        Some(steamapps.to_path_buf())
    })
}
