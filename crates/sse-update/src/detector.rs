//! Installation detector identifying platform, architecture, and packaging kind.

use crate::platform;
use sse_core::{Error, Result};
use std::path::{Path, PathBuf};

/// Default location of the build manifest installed by the system deb package on Linux.
pub const LINUX_PACKAGE_INSTALL_ROOT: &str = "/usr/share/stalker-save-editor";

/// Detected running or target installation metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInstallation {
    /// Target platform name (`windows`, `linux`, `macos`).
    pub target: String,
    /// CPU architecture (`x86_64`, `arm64`).
    pub architecture: String,
    /// Installation kind (`portable`, `package`, `installer`, `app-bundle`, `appimage`, `development`).
    pub kind: String,
    /// Installation root directory.
    pub root: PathBuf,
    /// Executable path.
    pub executable: PathBuf,
}

/// Detects the installation environment around the application executable.
pub struct UpdateInstallationDetector;

impl UpdateInstallationDetector {
    /// Detects installation parameters for the current running process or specified path.
    ///
    /// # Errors
    /// Returns an error if the executable path is unavailable or platform is unsupported.
    pub fn detect(
        executable_path: Option<&Path>,
        platform_override: Option<&str>,
        package_install_root: Option<&str>,
    ) -> Result<UpdateInstallation> {
        let default_pkg_root = package_install_root.unwrap_or(LINUX_PACKAGE_INSTALL_ROOT);

        let target = if let Some(p) = platform_override {
            let lower = p.to_ascii_lowercase();
            match lower.as_str() {
                "windows" | "win32nt" => platform::target::WINDOWS.to_string(),
                "macos" | "darwin" => platform::target::MACOS.to_string(),
                "linux" | "unix" => platform::target::LINUX.to_string(),
                _ => return Err(Error::Refused(format!("Unsupported update platform: {p}"))),
            }
        } else if cfg!(target_os = "windows") {
            platform::target::WINDOWS.to_string()
        } else if cfg!(target_os = "macos") {
            platform::target::MACOS.to_string()
        } else if cfg!(target_os = "linux") {
            platform::target::LINUX.to_string()
        } else {
            return Err(Error::Refused("Unsupported operating system".to_string()));
        };

        let exe = match executable_path {
            Some(p) => p.to_path_buf(),
            None => std::env::current_exe().map_err(Error::from)?,
        };

        // Canonicalize or resolve symlinks if possible
        let exe = match std::fs::canonicalize(&exe) {
            Ok(canonical) => canonical,
            Err(_) => exe,
        };

        let exe_dir = exe.parent().unwrap_or_else(|| Path::new("/")).to_path_buf();

        // Check for macOS .app bundle ancestor
        let app_bundle = if target == platform::target::MACOS {
            find_app_bundle_ancestor(&exe_dir)
        } else {
            None
        };

        let is_deb_entrypoint = target == platform::target::LINUX
            && is_linux_deb_entrypoint(&exe, &exe_dir, Path::new(default_pkg_root))
            && Path::new(default_pkg_root).join("BUILD_MANIFEST.json").is_file();
        let root = if let Some(ref bundle) = app_bundle {
            bundle.clone()
        } else if is_deb_entrypoint {
            std::fs::canonicalize(default_pkg_root).unwrap_or_else(|_| PathBuf::from(default_pkg_root))
        } else {
            exe_dir.clone()
        };

        let manifest_path = if app_bundle.is_some() {
            root.join("Contents").join("Resources").join("BUILD_MANIFEST.json")
        } else {
            root.join("BUILD_MANIFEST.json")
        };

        let default_arch = if target == platform::target::MACOS && cfg!(target_arch = "aarch64") {
            platform::architecture::ARM64
        } else {
            platform::architecture::X86_64
        };
        let mut architecture = default_arch.to_string();

        // 1. Linux package install root match
        if target == platform::target::LINUX && paths_equal(&root, Path::new(default_pkg_root)) {
            return Ok(UpdateInstallation {
                target,
                architecture,
                kind: platform::kind::PACKAGE.to_string(),
                root,
                executable: exe,
            });
        }

        // 2. Linux AppImage environment check
        if target == platform::target::LINUX
            && (std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some())
        {
            return Ok(UpdateInstallation {
                target,
                architecture,
                kind: platform::kind::APP_IMAGE.to_string(),
                root,
                executable: exe,
            });
        }

        // 3. Check BUILD_MANIFEST.json
        if !manifest_path.is_file() {
            return Ok(UpdateInstallation {
                target,
                architecture,
                kind: platform::kind::DEVELOPMENT.to_string(),
                root,
                executable: exe,
            });
        }

        let manifest_bytes = std::fs::read(&manifest_path).map_err(Error::from)?;
        let (manifest_target, manifest_arch) = parse_build_manifest(&manifest_bytes)?;

        if manifest_target != target {
            return Err(Error::Refused(
                "Packaged build target does not match the current platform".to_string(),
            ));
        }

        if let Some(arch) = manifest_arch {
            if !arch.trim().is_empty() {
                architecture = arch;
            }
        }

        let kind = if target == platform::target::WINDOWS && root.join("INSTALLER_MARKER").is_file() {
            platform::kind::INSTALLER.to_string()
        } else if target == platform::target::MACOS && app_bundle.is_some() {
            platform::kind::APP_BUNDLE.to_string()
        } else {
            platform::kind::PORTABLE.to_string()
        };

        Ok(UpdateInstallation {
            target,
            architecture,
            kind,
            root,
            executable: exe,
        })
    }
}

fn paths_equal(left: &Path, right: &Path) -> bool {
    if let (Ok(l), Ok(r)) = (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        return l == r;
    }
    #[cfg(windows)]
    {
        left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

fn is_linux_deb_entrypoint(executable: &Path, executable_directory: &Path, package_root: &Path) -> bool {
    let Some(prefix) = package_root.parent().and_then(Path::parent) else {
        return false;
    };
    let executable_name = executable.file_name();
    if !matches!(
        executable_name.and_then(|name| name.to_str()),
        Some("sse-shell" | "stalker-save")
    ) {
        return false;
    }
    paths_equal(executable_directory, &prefix.join("bin"))
}

fn find_app_bundle_ancestor(start: &Path) -> Option<PathBuf> {
    let mut current = Some(start);
    while let Some(dir) = current {
        if let Some(ext) = dir.extension() {
            if ext.eq_ignore_ascii_case("app") {
                return Some(dir.to_path_buf());
            }
        }
        current = dir.parent();
    }
    None
}

fn parse_build_manifest(bytes: &[u8]) -> Result<(String, Option<String>)> {
    let mut reader = sse_codecs::json::Reader::new(bytes);
    match reader.next_event()? {
        Some(sse_codecs::json::Event::ObjectStart) => {}
        _ => return Err(Error::damaged("BUILD_MANIFEST.json must be a JSON object")),
    }

    let mut target: Option<String> = None;
    let mut architecture: Option<String> = None;

    while let Some(event) = reader.next_event()? {
        match event {
            sse_codecs::json::Event::Key(k) => match k.as_str() {
                "target" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        target = Some(s.into_owned());
                    }
                }
                "architecture" => {
                    if let Some(sse_codecs::json::Event::String(s)) = reader.next_event()? {
                        architecture = Some(s.into_owned());
                    }
                }
                _ => {
                    reader.skip_value()?;
                }
            },
            sse_codecs::json::Event::ObjectEnd => break,
            _ => return Err(Error::damaged("Malformed BUILD_MANIFEST.json token")),
        }
    }

    let target = target.ok_or_else(|| Error::damaged("BUILD_MANIFEST.json missing target"))?;
    Ok((target, architecture))
}
