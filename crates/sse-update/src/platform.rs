//! Platform, architecture, and artifact definitions for updates.

/// Target platform names.
pub mod target {
    /// Microsoft Windows.
    pub const WINDOWS: &str = "windows";
    /// Linux distributions.
    pub const LINUX: &str = "linux";
    /// Apple macOS.
    pub const MACOS: &str = "macos";
}

/// Target CPU architectures.
pub mod architecture {
    /// x86_64 / amd64.
    pub const X86_64: &str = "x86_64";
    /// ARM64 / aarch64.
    pub const ARM64: &str = "arm64";
}

/// Artifact and installation kinds.
pub mod kind {
    /// Portable zip or tar.gz archive.
    pub const PORTABLE: &str = "portable";
    /// Native package (e.g. Debian .deb).
    pub const PACKAGE: &str = "package";
    /// Windows setup installer (.exe).
    pub const INSTALLER: &str = "installer";
    /// macOS disk image (.dmg).
    pub const DISK_IMAGE: &str = "disk-image";
    /// macOS installed application bundle (.app).
    pub const APP_BUNDLE: &str = "app-bundle";
    /// Linux AppImage bundle.
    pub const APP_IMAGE: &str = "appimage";
    /// Development unbundled installation.
    pub const DEVELOPMENT: &str = "development";
}

/// A registered artifact descriptor in the release matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArtifactDescriptor {
    /// Target platform.
    pub target: &'static str,
    /// CPU architecture.
    pub architecture: &'static str,
    /// Artifact kind.
    pub kind: &'static str,
}

const ARTIFACT_TABLE: &[(&str, ArtifactDescriptor)] = &[
    (
        "windows-x86_64",
        ArtifactDescriptor {
            target: target::WINDOWS,
            architecture: architecture::X86_64,
            kind: kind::PORTABLE,
        },
    ),
    (
        "windows-installer-x86_64",
        ArtifactDescriptor {
            target: target::WINDOWS,
            architecture: architecture::X86_64,
            kind: kind::INSTALLER,
        },
    ),
    (
        "linux-x86_64",
        ArtifactDescriptor {
            target: target::LINUX,
            architecture: architecture::X86_64,
            kind: kind::PORTABLE,
        },
    ),
    (
        "linux-appimage-x86_64",
        ArtifactDescriptor {
            target: target::LINUX,
            architecture: architecture::X86_64,
            kind: kind::APP_IMAGE,
        },
    ),
    (
        "linux-deb-amd64",
        ArtifactDescriptor {
            target: target::LINUX,
            architecture: architecture::X86_64,
            kind: kind::PACKAGE,
        },
    ),
    (
        "macos-arm64",
        ArtifactDescriptor {
            target: target::MACOS,
            architecture: architecture::ARM64,
            kind: kind::DISK_IMAGE,
        },
    ),
    (
        "macos-x86_64",
        ArtifactDescriptor {
            target: target::MACOS,
            architecture: architecture::X86_64,
            kind: kind::DISK_IMAGE,
        },
    ),
];

/// Returns the manifest key for a target platform, architecture, and artifact kind.
#[must_use]
pub fn artifact_key(target_name: &str, arch: &str, kind_name: &str) -> Option<&'static str> {
    for (key, desc) in ARTIFACT_TABLE {
        if desc.target == target_name && desc.kind == kind_name && desc.architecture == arch {
            return Some(key);
        }
    }
    None
}

/// Checks whether an artifact key is valid and describes the given architecture and kind.
#[must_use]
pub fn describes(key: &str, arch: &str, kind_name: &str) -> bool {
    for (k, desc) in ARTIFACT_TABLE {
        if *k == key {
            return desc.architecture == arch && desc.kind == kind_name;
        }
    }
    false
}

/// Returns the artifact kind that updates an installation of the given kind.
#[must_use]
pub fn artifact_kind_for(target_name: &str, install_kind: &str) -> &'static str {
    match target_name {
        target::MACOS => kind::DISK_IMAGE,
        target::LINUX if install_kind == kind::PACKAGE => kind::PACKAGE,
        target::WINDOWS if install_kind == kind::INSTALLER => kind::INSTALLER,
        _ => kind::PORTABLE,
    }
}
