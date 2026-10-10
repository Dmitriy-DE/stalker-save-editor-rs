//! Steam Workshop package identifiers and read-only installation status checks.
use std::path::{Path, PathBuf};

/// Workshop package metadata for one Enhanced Edition companion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnhancedEditionPackage {
    /// Steam application ID whose Workshop content directory contains the item.
    pub app_id: u32,
    /// Published Workshop item ID. Kept empty until the owner publishes the package.
    pub published_file_id: Option<&'static str>,
    /// X-Ray archive that must be present in the subscribed item directory.
    pub archive_name: &'static str,
}

/// Three Enhanced Edition Workshop slots, intentionally unpublished until the owner assigns IDs.
pub const EE_WORKSHOP_PACKAGES: [EnhancedEditionPackage; 3] = [
    EnhancedEditionPackage {
        app_id: 2_427_410,
        published_file_id: None,
        archive_name: "save_editor_companion_soc.xrp",
    },
    EnhancedEditionPackage {
        app_id: 2_427_420,
        published_file_id: None,
        archive_name: "save_editor_companion_cs.xrp",
    },
    EnhancedEditionPackage {
        app_id: 2_427_430,
        published_file_id: None,
        archive_name: "save_editor_companion_cop.xrp",
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
    /// The subscribed descriptor and expected archive match the running package version.
    UpToDate,
    /// The item exists, but its descriptor, version, or archive does not match.
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
#[must_use]
pub fn inspect_ee_install(
    install_directory: &Path,
    package: &EnhancedEditionPackage,
    expected_version: &str,
) -> WorkshopInstallState {
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

    let descriptor = item_directory.join("desc.json");
    let Ok(metadata) = std::fs::symlink_metadata(&descriptor) else {
        return WorkshopInstallState::Outdated;
    };
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return WorkshopInstallState::Outdated;
    }
    let Ok(bytes) = std::fs::read(&descriptor) else {
        return WorkshopInstallState::Outdated;
    };
    let Some((version, archive_name)) = descriptor_fields(&bytes) else {
        return WorkshopInstallState::Outdated;
    };
    let archive = item_directory.join(&archive_name);
    let Ok(archive_metadata) = std::fs::symlink_metadata(archive) else {
        return WorkshopInstallState::Outdated;
    };
    if !archive_metadata.file_type().is_file() || archive_name != package.archive_name {
        return WorkshopInstallState::Outdated;
    }
    if version == expected_version {
        WorkshopInstallState::UpToDate
    } else {
        WorkshopInstallState::Outdated
    }
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

fn descriptor_fields(bytes: &[u8]) -> Option<(String, String)> {
    let mut reader = sse_codecs::json::Reader::new(bytes);
    let mut version = None;
    let mut package_file = None;
    loop {
        let event = match reader.next_event() {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(_) => return None,
        };
        if let sse_codecs::json::Event::Key(key) = event {
            match key.as_str() {
                "version" => match reader.next_event() {
                    Ok(Some(sse_codecs::json::Event::String(value))) => version = Some(value.into_owned()),
                    _ => return None,
                },
                "package_file" => match reader.next_event() {
                    Ok(Some(sse_codecs::json::Event::String(value))) => package_file = Some(value.into_owned()),
                    _ => return None,
                },
                _ => {
                    if reader.skip_value().is_err() {
                        return None;
                    }
                }
            }
        }
    }
    Some((version?, package_file?))
}
