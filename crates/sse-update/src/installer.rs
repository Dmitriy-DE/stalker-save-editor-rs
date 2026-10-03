//! Platform-specific installation handoff matching release 1.3.1.

use crate::detector::UpdateInstallation;
use crate::fetch::verify_existing_file;
use crate::manifest::UpdateArtifact;
use crate::platform;
use sse_core::{Error, Result};
use std::path::Path;
use std::process::Command;

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

    // Fail closed if local file has been tampered with
    verify_existing_file(verified_archive, artifact)?;

    let archive_str = verified_archive
        .to_str()
        .ok_or_else(|| Error::damaged("Invalid archive path encoding"))?;

    // 1. Linux package (.deb) handoff
    if installation.target == platform::target::LINUX && installation.kind == platform::kind::PACKAGE {
        if !artifact.file.ends_with(".deb") {
            return Err(Error::Refused(
                "Linux package handoff requires a verified .deb artifact".to_string(),
            ));
        }

        let has_pkexec = find_in_path("pkexec").is_some();
        let has_apt = find_in_path("apt-get").is_some();

        if has_pkexec && has_apt {
            let exit_code = runner.run("pkexec", &["apt-get", "install", "-y", "--", archive_str])?;
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

        if let Some(xdg) = find_in_path("xdg-open") {
            let xdg_str = xdg.to_str().unwrap_or("xdg-open");
            let exit_code = runner.run(xdg_str, &[archive_str])?;
            return Ok(UpdateInstallResult {
                state: UpdateInstallState::OpenedExternally,
                exit_code: Some(exit_code),
                message: "The verified installer was opened.".to_string(),
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
        let exit_code = runner.run(archive_str, &[])?;
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
        let exit_code = runner.run("open", &[archive_str])?;
        return Ok(UpdateInstallResult {
            state: UpdateInstallState::OpenedExternally,
            exit_code: Some(exit_code),
            message: "The verified installer was opened.".to_string(),
        });
    }

    Err(Error::Refused(
        "Automatic replacement of portable .zip or .tar.gz installations is not supported by this installer handoff."
            .to_string(),
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
