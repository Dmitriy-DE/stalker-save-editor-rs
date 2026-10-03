//! Same-executable worker dispatch and bounded child-process supervision.

use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::api::NativeSteamApiUnavailable;
use crate::protocol::{self, ProtocolError, Request, Response};

/// Result of checking the first command-line argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerArgs {
    /// Normal application command.
    NormalCli,
    /// Exact Steam worker invocation.
    Worker,
    /// Unknown worker-mode spelling or malformed worker arguments.
    UsageError,
}

/// Classifies worker arguments before ordinary CLI command dispatch.
#[must_use]
pub fn classify_worker_args(arguments: &[String]) -> WorkerArgs {
    match arguments.first().map(String::as_str) {
        Some("--steam-worker") if arguments.len() == 1 => WorkerArgs::Worker,
        Some(argument) if argument.starts_with("--steam-") => WorkerArgs::UsageError,
        _ => WorkerArgs::NormalCli,
    }
}

/// Runs the worker or returns `None` to continue normal CLI command dispatch.
pub fn run_if_worker(arguments: &[String]) -> Option<ExitCode> {
    match classify_worker_args(arguments) {
        WorkerArgs::NormalCli => None,
        WorkerArgs::UsageError => {
            eprintln!("Usage: stalker-save --steam-worker (binary frame on stdin/stdout)");
            Some(ExitCode::from(sse_core::ExitCode::Usage as u8))
        }
        WorkerArgs::Worker => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut input = stdin.lock();
            let mut output = stdout.lock();
            let mut api = NativeSteamApiUnavailable;
            match protocol::serve_one(&mut api, &mut input, &mut output) {
                Ok(()) => Some(ExitCode::SUCCESS),
                Err(error) => {
                    eprintln!("Steam worker protocol error: {error}");
                    Some(ExitCode::from(sse_core::ExitCode::Damaged as u8))
                }
            }
        }
    }
}

/// Timeout and process/protocol failure returned to the editor process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerProcessError {
    /// Child creation or pipe I/O failed.
    Io(String),
    /// The child did not complete before timeout and was killed. A write may have reached Steam.
    Timeout {
        /// True if a cloud write or achievement mutation may have reached Steam.
        write_outcome_uncertain: bool,
    },
    /// Worker emitted a malformed response.
    Protocol(String),
    /// Worker exited without a complete response frame.
    NoResponse,
}

impl std::fmt::Display for WorkerProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(message) | Self::Protocol(message) => formatter.write_str(message),
            Self::Timeout {
                write_outcome_uncertain: true,
            } => formatter.write_str("Steam worker timed out; write outcome is uncertain"),
            Self::Timeout {
                write_outcome_uncertain: false,
            } => formatter.write_str("Steam worker timed out and was killed"),
            Self::NoResponse => formatter.write_str("Steam worker exited without a response"),
        }
    }
}

impl std::error::Error for WorkerProcessError {}

/// Runs the same executable as a worker and kills it when its deadline expires.
pub fn run_worker_process(
    executable: &Path,
    request: &Request,
    timeout: Duration,
) -> Result<Response, WorkerProcessError> {
    if timeout.is_zero() {
        return Err(WorkerProcessError::Io(
            "Steam worker timeout must be positive".to_owned(),
        ));
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| WorkerProcessError::Io("Steam worker timeout is too large".to_owned()))?;
    let frame = protocol::encode_frame(request).map_err(protocol_error)?;
    let app_id = match request {
        Request::List { app_id }
        | Request::Read { app_id, .. }
        | Request::Write { app_id, .. }
        | Request::ListAchievements { app_id }
        | Request::SetAchievement { app_id, .. }
        | Request::ClearAchievement { app_id, .. } => *app_id,
    };
    let app_id_text = app_id.to_string();
    let mut child = Command::new(executable)
        .arg("--steam-worker")
        .env("SteamAppId", &app_id_text)
        .env("SteamGameId", &app_id_text)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| WorkerProcessError::Io(error.to_string()))?;
    let mut child_input = child
        .stdin
        .take()
        .ok_or_else(|| WorkerProcessError::Io("worker stdin was not piped".to_owned()))?;
    let mut child_output = child
        .stdout
        .take()
        .ok_or_else(|| WorkerProcessError::Io("worker stdout was not piped".to_owned()))?;

    let (write_sender, write_receiver) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = child_input.write_all(&frame).map_err(|error| error.to_string());
        drop(child_input);
        let _ = write_sender.send(result);
    });
    let (response_sender, response_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let result = protocol::read_frame(&mut child_output).map_err(|error| error.to_string());
        let _ = response_sender.send(result);
    });

    let mut response_body = None;
    loop {
        if let Ok(Err(message)) = write_receiver.try_recv() {
            terminate_child(&mut child);
            let _ = writer.join();
            let _ = reader.join();
            return Err(WorkerProcessError::Io(message));
        }
        if response_body.is_none() {
            match response_receiver.try_recv() {
                Ok(Ok(body)) => response_body = Some(body),
                Ok(Err(_)) => {
                    terminate_child(&mut child);
                    let _ = writer.join();
                    let _ = reader.join();
                    return Err(WorkerProcessError::NoResponse);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    terminate_child(&mut child);
                    let _ = writer.join();
                    let _ = reader.join();
                    return Err(WorkerProcessError::NoResponse);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(body) = response_body.take() {
            if let Some(status) = poll_child(&mut child)? {
                let _ = writer.join();
                let _ = reader.join();
                if !status.success() {
                    return Err(WorkerProcessError::Io(format!("Steam worker exited with {status}")));
                }
                return protocol::decode_response_body(&body).map_err(protocol_error);
            }
            response_body = Some(body);
        }
        if Instant::now() >= deadline {
            terminate_child(&mut child);
            // A descendant may have inherited stdout and keep the reader blocked after the
            // direct worker exits. Dropping these handles detaches cleanup from the deadline.
            drop(writer);
            drop(reader);
            return Err(WorkerProcessError::Timeout {
                write_outcome_uncertain: matches!(
                    request,
                    Request::Write { .. }
                        | Request::SetAchievement { confirmed: true, .. }
                        | Request::ClearAchievement { confirmed: true, .. }
                ),
            });
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Starts a worker sibling of the running editor executable.
pub fn run_sibling_worker(request: &Request, timeout: Duration) -> Result<Response, WorkerProcessError> {
    let executable = std::env::current_exe().map_err(|error| WorkerProcessError::Io(error.to_string()))?;
    run_worker_process(&executable, request, timeout)
}

fn terminate_child(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn poll_child(child: &mut std::process::Child) -> Result<Option<std::process::ExitStatus>, WorkerProcessError> {
    match child.try_wait() {
        Ok(status) => Ok(status),
        Err(error) => {
            terminate_child(child);
            Err(WorkerProcessError::Io(error.to_string()))
        }
    }
}

fn protocol_error(error: ProtocolError) -> WorkerProcessError {
    WorkerProcessError::Protocol(error.message)
}
