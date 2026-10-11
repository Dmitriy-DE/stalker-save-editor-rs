//! Platform-specific installation handoff matching release 1.3.1.

use crate::detector::UpdateInstallation;
use crate::fetch::Sha256Hasher;
use crate::manifest::UpdateArtifact;
use crate::platform;
use sse_core::{Error, Result};
use std::fs::OpenOptions;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

const STALE_INSTALL_STAGE_AGE: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The outcome state of an update installation handoff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateInstallState {
    /// Installation process completed successfully.
    Succeeded,
    /// A verified portable archive needs to be extracted by the user.
    ManualInstructions,
    /// An external GUI installer or disk image was opened.
    OpenedExternally,
    /// Authorization or execution was cancelled by the user.
    Cancelled,
    /// Installation process failed or returned a non-zero exit code.
    Failed,
}

/// Detailed result of an update installation operation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateInstallResult {
    /// Result state.
    pub state: UpdateInstallState,
    /// Process exit code, if available.
    pub exit_code: Option<i32>,
    /// Informational message for the user.
    pub message: String,
}

/// Trait abstracting process execution for unit testability without running live system installers.
pub trait ProcessRunner {
    /// Runs a configured command and returns the exit code.
    ///
    /// # Errors
    /// Returns an error if the process could not be launched.
    fn run(&mut self, program: &str, args: &[&str]) -> Result<i32>;

    /// Runs an installer that was already verified (signature and SHA-256) and copied into the private staging
    /// directory. `artifact` must be an absolute path to a regular file inside `staging_root`; implementations that
    /// start a real process must check that with [`staged_artifact_path`] and never search `PATH`.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the path is not inside the staging directory, or an error if the process
    /// could not be launched.
    fn run_staged_artifact(&mut self, staging_root: &Path, artifact: &Path) -> Result<i32>;

    /// Checks if a command or binary is available to execute.
    fn has_command(&self, program: &str) -> bool {
        trusted_program(program).is_some()
    }
}

/// Default system command runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<i32> {
        // Only system locations are trusted: a PATH entry must not decide which installer runs.
        let resolved = trusted_program(program)
            .ok_or_else(|| Error::Refused(format!("{program} is not installed in a system directory")))?;
        let mut cmd = Command::new(resolved);
        cmd.args(args);
        let status = cmd.status().map_err(Error::from)?;
        Ok(status.code().unwrap_or(-1))
    }

    fn run_staged_artifact(&mut self, staging_root: &Path, artifact: &Path) -> Result<i32> {
        let resolved = staged_artifact_path(staging_root, artifact)?;
        let status = Command::new(resolved).status().map_err(Error::from)?;
        Ok(status.code().unwrap_or(-1))
    }
}

/// Resolves a staged installer for execution, refusing every path that is not a regular file inside `staging_root`.
///
/// Both paths must be absolute and free of `.`/`..` components, and no component below the staging directory may be
/// a symbolic link: the resolved file has to be exactly the staging directory plus the relative part of `artifact`.
///
/// # Errors
/// Returns [`Error::Refused`] for a relative path, a path outside the staging directory, a link or a non-file.
pub fn staged_artifact_path(staging_root: &Path, artifact: &Path) -> Result<PathBuf> {
    let refuse = |reason: &str| Error::Refused(format!("Staged installer {reason}: {}", artifact.display()));
    if !staging_root.is_absolute() || !artifact.is_absolute() {
        return Err(refuse("path is not absolute"));
    }
    if artifact
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir | std::path::Component::CurDir))
    {
        return Err(refuse("path has relative components"));
    }
    let relative = artifact
        .strip_prefix(staging_root)
        .map_err(|_| refuse("is outside the staging directory"))?;
    if relative.as_os_str().is_empty() {
        return Err(refuse("is the staging directory itself"));
    }
    let root = std::fs::canonicalize(staging_root).map_err(|_| refuse("staging directory is unavailable"))?;
    let resolved = std::fs::canonicalize(artifact).map_err(|_| refuse("is unavailable"))?;
    if resolved != root.join(relative) {
        return Err(refuse("is reached through a link"));
    }
    let metadata = std::fs::symlink_metadata(&resolved).map_err(|_| refuse("is unavailable"))?;
    if !metadata.file_type().is_file() {
        return Err(refuse("is not a regular file"));
    }
    Ok(resolved)
}

struct StagedArtifact {
    path: PathBuf,
    directory: PathBuf,
    preserve: bool,
}

impl Drop for StagedArtifact {
    fn drop(&mut self) {
        if self.preserve {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.directory);
        if let Some(root) = self.directory.parent() {
            let _ = std::fs::remove_dir(root);
        }
    }
}

fn clean_stale_install_stages(root: &Path, current_stage: &Path) -> Result<()> {
    let now = SystemTime::now();
    for entry in std::fs::read_dir(root).map_err(Error::from)? {
        let entry = entry.map_err(Error::from)?;
        let path = entry.path();
        if path == current_stage
            || !entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("install-"))
        {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path).map_err(Error::from)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            continue;
        }
        if now
            .duration_since(metadata.modified().map_err(Error::from)?)
            .unwrap_or_default()
            > STALE_INSTALL_STAGE_AGE
        {
            std::fs::remove_dir_all(&path).map_err(Error::from)?;
        }
    }
    Ok(())
}

/// Creates or validates a private update directory.
///
/// On Unix the directory is forced to mode 0700. On Linux its owner is also
/// checked against the effective user before it is trusted.
pub fn prepare_private_directory(path: &Path) -> Result<()> {
    std::fs::create_dir_all(path).map_err(Error::from)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).map_err(Error::from)?;
    }
    sse_sys::secure_fs::verify_directory_owner(path)
}

fn stage_verified_copy(artifact: &UpdateArtifact, source: &Path, root: &Path) -> Result<StagedArtifact> {
    artifact.validate()?;
    prepare_private_directory(root)?;

    let mut source_file = sse_sys::secure_fs::open_owned_regular(source)?;
    let source_metadata = source_file.metadata().map_err(Error::from)?;
    if source_metadata.len() != artifact.size {
        return Err(Error::damaged("File size mismatch"));
    }

    let stage_dir = root.join(format!("install-{}-{}", std::process::id(), artifact.sha256));
    clean_stale_install_stages(root, &stage_dir)?;
    let target = stage_dir.join(&artifact.file);
    let stage_already_exists = match std::fs::create_dir(&stage_dir) {
        Ok(()) => false,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => true,
        Err(error) => return Err(Error::from(error)),
    };
    if stage_already_exists {
        let metadata = std::fs::symlink_metadata(&stage_dir).map_err(Error::from)?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            return Err(Error::Refused("Update staging path is not a real directory".to_owned()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o7777 != 0o700 {
                return Err(Error::Refused(
                    "Update staging directory permissions are not private".to_owned(),
                ));
            }
        }
        sse_sys::secure_fs::verify_directory_owner(&stage_dir)?;
        let staged_artifact = StagedArtifact {
            path: target,
            directory: stage_dir,
            preserve: true,
        };
        crate::fetch::verify_existing_file(&staged_artifact.path, artifact)?;
        return Ok(staged_artifact);
    }
    let staged_artifact = StagedArtifact {
        path: target.clone(),
        directory: stage_dir.clone(),
        preserve: false,
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stage_dir, std::fs::Permissions::from_mode(0o700)).map_err(Error::from)?;
    }
    sse_sys::secure_fs::verify_directory_owner(&stage_dir)?;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut target_file = options.open(&target).map_err(Error::from)?;

    let mut hasher = Sha256Hasher::new();
    let mut copied = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = source_file.read(&mut buffer).map_err(Error::from)?;
        if read == 0 {
            break;
        }
        copied = copied
            .checked_add(u64::try_from(read).map_err(|_| Error::damaged("Update size overflow"))?)
            .ok_or_else(|| Error::damaged("Update size overflow"))?;
        if copied > artifact.size {
            return Err(Error::damaged("File size mismatch"));
        }
        let chunk = buffer
            .get(..read)
            .ok_or_else(|| Error::damaged("Update read overflow"))?;
        hasher.update(chunk);
        target_file.write_all(chunk).map_err(Error::from)?;
    }
    if copied != artifact.size {
        return Err(Error::damaged("File size mismatch"));
    }
    let actual = hasher.finalize_hex();
    if !actual.eq_ignore_ascii_case(&artifact.sha256) {
        return Err(Error::damaged("File SHA-256 mismatch"));
    }
    target_file.flush().map_err(Error::from)?;
    target_file.sync_all().map_err(Error::from)?;
    Ok(staged_artifact)
}

/// Hands off an update to the platform installer after validating the file on disk.
///
/// # Errors
/// Returns an error if the artifact has been tampered with or is incompatible.
pub fn install_artifact(
    artifact: &UpdateArtifact,
    verified_archive: &Path,
    installation: &UpdateInstallation,
    runner: &mut dyn ProcessRunner,
) -> Result<UpdateInstallResult> {
    if installation.kind == platform::kind::DEVELOPMENT {
        return Err(Error::Refused("Update installation type is unsupported".to_string()));
    }

    if artifact.kind == platform::kind::PORTABLE {
        artifact.validate()?;
        crate::fetch::verify_existing_file(verified_archive, artifact)?;
        return Ok(UpdateInstallResult {
            state: UpdateInstallState::ManualInstructions,
            exit_code: None,
            message: verified_archive.display().to_string(),
        });
    }

    let parent = verified_archive
        .parent()
        .ok_or_else(|| Error::Refused("Update package has no parent directory".to_owned()))?;
    let staging_root = parent.join("verified-install");
    let mut staged_archive = stage_verified_copy(artifact, verified_archive, &staging_root)?;

    let archive_str = staged_archive
        .path
        .to_str()
        .ok_or_else(|| Error::damaged("Invalid archive path encoding"))?;
    let archive_str = archive_str.to_owned();

    // 1. Linux package (.deb) handoff
    if installation.target == platform::target::LINUX && installation.kind == platform::kind::PACKAGE {
        if !artifact.file.ends_with(".deb") {
            return Err(Error::Refused(
                "Linux package handoff requires a verified .deb artifact".to_string(),
            ));
        }

        let has_pkexec = runner.has_command("pkexec");
        let has_apt = runner.has_command("apt-get");

        if has_pkexec && has_apt {
            let exit_code = runner.run("pkexec", &["apt-get", "install", "-y", "--", &archive_str])?;
            return match exit_code {
                0 => Ok(UpdateInstallResult {
                    state: UpdateInstallState::Succeeded,
                    exit_code: Some(0),
                    message: "Update installation completed.".to_string(),
                }),
                126 | 127 => Ok(UpdateInstallResult {
                    state: UpdateInstallState::Cancelled,
                    exit_code: Some(exit_code),
                    message: "The installer authorization was cancelled.".to_string(),
                }),
                code => Ok(UpdateInstallResult {
                    state: UpdateInstallState::Failed,
                    exit_code: Some(code),
                    message: format!("Installer exited with code {code}."),
                }),
            };
        }

        if runner.has_command("xdg-open") {
            let exit_code = runner.run("xdg-open", &[&archive_str])?;
            // The external package handler may still be reading this copy after xdg-open exits.
            staged_archive.preserve = exit_code == 0;
            let state = if exit_code == 0 {
                UpdateInstallState::OpenedExternally
            } else {
                UpdateInstallState::Failed
            };
            let message = if exit_code == 0 {
                "The verified installer was opened.".to_string()
            } else {
                format!("Installer opener exited with code {exit_code}.")
            };
            return Ok(UpdateInstallResult {
                state,
                exit_code: Some(exit_code),
                message,
            });
        }

        return Err(Error::Refused(
            "Linux package manager handoff is unavailable (pkexec/xdg-open missing)".to_string(),
        ));
    }

    // 2. Windows installer (.exe) handoff
    if installation.target == platform::target::WINDOWS
        && installation.kind == platform::kind::INSTALLER
        && artifact.file.ends_with(".exe")
    {
        let exit_code = runner.run_staged_artifact(&staging_root, &staged_archive.path)?;
        return match exit_code {
            0 => Ok(UpdateInstallResult {
                state: UpdateInstallState::Succeeded,
                exit_code: Some(0),
                message: "Update installation completed.".to_string(),
            }),
            code => Ok(UpdateInstallResult {
                state: UpdateInstallState::Failed,
                exit_code: Some(code),
                message: format!("Installer exited with code {code}."),
            }),
        };
    }

    // 3. macOS disk image (.dmg) handoff
    if installation.target == platform::target::MACOS
        && artifact.kind == platform::kind::DISK_IMAGE
        && artifact.file.ends_with(".dmg")
    {
        let exit_code = runner.run("open", &[&archive_str])?;
        // Finder may still be reading the disk image after `open` returns.
        staged_archive.preserve = exit_code == 0;
        let state = if exit_code == 0 {
            UpdateInstallState::OpenedExternally
        } else {
            UpdateInstallState::Failed
        };
        let message = if exit_code == 0 {
            "The verified installer was opened.".to_string()
        } else {
            format!("Installer opener exited with code {exit_code}.")
        };
        return Ok(UpdateInstallResult {
            state,
            exit_code: Some(exit_code),
            message,
        });
    }

    Err(Error::Refused(
        "Automatic replacement is not supported for this installation type.".to_string(),
    ))
}

/// Directories searched for system helpers (`pkexec`, `apt-get`, `xdg-open`, `open`). `PATH` is not used.
const TRUSTED_SYSTEM_DIRECTORIES: &[&str] = &["/usr/bin", "/bin"];

/// Resolves a bare helper name inside [`TRUSTED_SYSTEM_DIRECTORIES`]. Names with separators are refused.
fn trusted_program(program: &str) -> Option<std::path::PathBuf> {
    if program.is_empty() || program.contains(['/', '\\']) {
        return None;
    }
    TRUSTED_SYSTEM_DIRECTORIES
        .iter()
        .map(|dir| std::path::Path::new(dir).join(program))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod staged_artifact_tests {
    use super::{staged_artifact_path, ProcessRunner, SystemProcessRunner};
    use sse_core::Error;
    use std::path::{Path, PathBuf};

    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("sse-update-staged-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(std::fs::canonicalize(&path).unwrap())
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn refused(root: &Path, artifact: &Path) {
        let error = SystemProcessRunner.run_staged_artifact(root, artifact).unwrap_err();
        assert!(matches!(error, Error::Refused(_)), "{artifact:?}: {error:?}");
        assert!(matches!(staged_artifact_path(root, artifact), Err(Error::Refused(_))));
    }

    #[test]
    fn staged_runner_refuses_paths_outside_the_staging_directory() {
        let temp = Temp::new("outside");
        let staging = temp.0.join("verified-install");
        std::fs::create_dir(&staging).unwrap();
        let outside = temp.0.join("Setup.exe");
        std::fs::write(&outside, b"outside").unwrap();
        std::fs::write(staging.join("Setup.exe"), b"inside").unwrap();

        refused(&staging, &outside);
        refused(&staging, &staging.join("..").join("Setup.exe"));
        refused(&staging, &staging);
        refused(&staging, &staging.join("missing.exe"));
        assert!(staged_artifact_path(&staging, &staging.join("Setup.exe")).is_ok());
    }

    #[test]
    fn staged_runner_refuses_relative_paths() {
        let temp = Temp::new("relative");
        let staging = temp.0.join("verified-install");
        std::fs::create_dir(&staging).unwrap();
        std::fs::write(staging.join("Setup.exe"), b"inside").unwrap();

        refused(&staging, Path::new("Setup.exe"));
        refused(&staging, Path::new("verified-install/Setup.exe"));
        refused(Path::new("verified-install"), &staging.join("Setup.exe"));
    }

    #[cfg(unix)]
    #[test]
    fn staged_runner_refuses_links_and_runs_a_real_staged_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp = Temp::new("links");
        let staging = temp.0.join("verified-install");
        std::fs::create_dir(&staging).unwrap();
        let outside = temp.0.join("outside.sh");
        std::fs::write(&outside, b"#!/bin/sh\nexit 0\n").unwrap();
        std::os::unix::fs::symlink(&outside, staging.join("link.sh")).unwrap();
        std::os::unix::fs::symlink(&temp.0, staging.join("escape")).unwrap();
        refused(&staging, &staging.join("link.sh"));
        refused(&staging, &staging.join("escape").join("outside.sh"));

        // The file is written and closed before it is started, so no writer keeps it busy.
        let staged = staging.join("Setup.sh");
        std::fs::write(&staged, b"#!/bin/sh\nexit 7\n").unwrap();
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(SystemProcessRunner.run_staged_artifact(&staging, &staged).unwrap(), 7);
    }
}

#[cfg(test)]
mod trusted_program_tests {
    use super::trusted_program;

    #[test]
    fn helper_lookup_ignores_unknown_names_and_explicit_paths() {
        assert!(trusted_program("sse-no-such-helper-7f3a").is_none());
        assert!(trusted_program("").is_none());
        assert!(trusted_program("../usr/bin/sh").is_none());
        assert!(trusted_program("/usr/bin/sh").is_none());
    }
}
