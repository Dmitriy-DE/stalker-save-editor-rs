//! Read-only detection and URL contract tests for Enhanced Edition Workshop packages.
use sse_companion::workshop::{inspect_ee_install, EnhancedEditionPackage, WorkshopInstallState, EE_WORKSHOP_PACKAGES};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir() -> Result<PathBuf, Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!("sse-workshop-test-{}-{nonce}", std::process::id()));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn steam_install(root: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let install = root.join("steamapps/common/Clear Sky Enhanced Edition");
    fs::create_dir_all(&install)?;
    Ok(install)
}

fn subscribed_folder(
    install: &Path,
    package: &EnhancedEditionPackage,
    mod_id: &str,
) -> Result<PathBuf, Box<dyn Error>> {
    let folder = install
        .ancestors()
        .find(|path| path.file_name().is_some_and(|name| name == "common"))
        .and_then(Path::parent)
        .ok_or("Steam install has no steamapps/common parent")?
        .join("workshop/content")
        .join(package.app_id.to_string())
        .join(mod_id);
    fs::create_dir_all(&folder)?;
    Ok(folder)
}

#[test]
fn workshop_ids_stay_unpublished_until_the_owner_sets_them() {
    assert!(EE_WORKSHOP_PACKAGES
        .iter()
        .all(|package| package.published_file_id.is_none()));
}

#[test]
fn workshop_detection_distinguishes_non_steam_missing_and_unsubscribed() -> Result<(), Box<dyn Error>> {
    let root = temp_dir()?;
    let package = EnhancedEditionPackage {
        published_file_id: Some("123456789"),
        ..EE_WORKSHOP_PACKAGES[1]
    };
    let non_steam = root.join("gog/Clear Sky");
    fs::create_dir_all(&non_steam)?;
    assert_eq!(
        inspect_ee_install(&non_steam, &package, "2.0.0-dev"),
        WorkshopInstallState::NotSteamInstall
    );

    let install = steam_install(&root)?;
    assert_eq!(
        inspect_ee_install(&install, &package, "2.0.0-dev"),
        WorkshopInstallState::NotSubscribed
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn steamapps_component_is_case_insensitive() -> Result<(), Box<dyn Error>> {
    let root = temp_dir()?;
    let install = root.join("SteamApps/common/Clear Sky Enhanced Edition");
    assert_eq!(
        inspect_ee_install(&install, &EE_WORKSHOP_PACKAGES[1], "2.0.0-dev"),
        WorkshopInstallState::NotPublished
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn workshop_detection_checks_descriptor_version_and_archive() -> Result<(), Box<dyn Error>> {
    let root = temp_dir()?;
    let install = steam_install(&root)?;
    let package = EnhancedEditionPackage {
        published_file_id: Some("123456789"),
        ..EE_WORKSHOP_PACKAGES[1]
    };
    let folder = subscribed_folder(&install, &package, "123456789")?;
    fs::write(
        folder.join("desc.json"),
        format!(
            "{{\"version\":\"2.0.0-dev\",\"package_file\":\"{}\"}}",
            package.archive_name
        ),
    )?;
    fs::write(folder.join(package.archive_name), b"archive")?;
    assert_eq!(
        inspect_ee_install(&install, &package, "2.0.0-dev"),
        WorkshopInstallState::UpToDate
    );

    assert_eq!(
        inspect_ee_install(&install, &package, "2.0.1"),
        WorkshopInstallState::Outdated
    );
    fs::remove_file(folder.join(package.archive_name))?;
    assert_eq!(
        inspect_ee_install(&install, &package, "2.0.0-dev"),
        WorkshopInstallState::Outdated
    );
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn workshop_page_url_accepts_only_a_numeric_published_item_id() {
    assert_eq!(
        sse_companion::workshop::workshop_page_url("123456789"),
        Some("https://steamcommunity.com/sharedfiles/filedetails/?id=123456789".to_owned())
    );
    assert_eq!(sse_companion::workshop::workshop_page_url(""), None);
    assert_eq!(sse_companion::workshop::workshop_page_url("12345&cmd=install"), None);
}
