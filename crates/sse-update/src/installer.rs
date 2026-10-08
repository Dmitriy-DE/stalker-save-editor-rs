//! Platform-specific installation handoff matching release 1.3.1.

use crate::detector::UpdateInstallation;
use crate::fetch::{hex_encode, Sha256Hasher};
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

    /// Checks if a command or binary is available to execute.
    fn has_command(&self, program: &str) -> bool {
        find_in_path(program).is_some()
    }
}

/// Default system command runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemProcessRunner;

impl ProcessRunner for SystemProcessRunner {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<i32> {
        let mut cmd = Command::new(program);
        cmd.args(args);
        let status = cmd.status().map_err(Error::from)?;
        Ok(status.code().unwrap_or(-1))
    }
}

/// Mock process runner for unit tests.
#[derive(Clone, Debug, Default)]
pub struct MockProcessRunner {
    /// Exit code to return.
    pub exit_code: i32,
    /// Recorded program call.
    pub last_program: Option<String>,
    /// Recorded arguments.
    pub last_args: Vec<String>,
    /// Number of executions.
    pub call_count: usize,
    /// Explicit list of supported commands if mocked.
    pub available_commands: Option<Vec<String>>,
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

impl MockProcessRunner {
    /// Creates a mock runner with the given exit code.
    #[must_use]
    pub fn new(exit_code: i32) -> Self {
        Self {
            exit_code,
            last_program: None,
            last_args: Vec::new(),
            call_count: 0,
            available_commands: None,
        }
    }
}

impl ProcessRunner for MockProcessRunner {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<i32> {
        self.call_count = self.call_count.saturating_add(1);
        self.last_program = Some(program.to_string());
        self.last_args = args.iter().map(|s| (*s).to_string()).collect();
        Ok(self.exit_code)
    }

    fn has_command(&self, program: &str) -> bool {
        match &self.available_commands {
            Some(cmds) => cmds.iter().any(|c| c == program),
            None => true,
        }
    }
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
    let actual = hex_encode(&hasher.finish());
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
        return Err(Error::Refused(format!(
            "Portable update verified at {}. Close the editor, extract the archive into its install folder, then launch sse-shell from that folder.",
            verified_archive.display()
        )));
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
        let exit_code = runner.run(&archive_str, &[])?;
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

fn find_in_path(executable_name: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(executable_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
