//! Same-executable worker dispatch and bounded child-process supervision.

use std::io::{BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use crate::api::{Achievement, CloudFile, SteamApi, SteamError, WriteFailure, WriteStage};
use crate::native;
use crate::native_protocol::{self, NativeRequest, NativeResponse};

/// Result of checking the first command-line argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerArgs {
    /// Normal application command.
    NormalCli,
    /// Exact Steam worker invocation.
    Worker,
    /// Steam operation worker invocation.
    NativeOp,
    /// Unknown worker-mode spelling or malformed worker arguments.
    UsageError,
}

/// Classifies worker arguments before ordinary CLI command dispatch.
#[must_use]
pub fn classify_worker_args(arguments: &[String]) -> WorkerArgs {
    match arguments.first().map(String::as_str) {
        Some("--steam-native-worker") if arguments.len() == 1 => WorkerArgs::Worker,
        Some("--steam-native-op") if parse_native_op_args(arguments).is_ok() => WorkerArgs::NativeOp,
        Some(argument) if argument.starts_with("--steam-") => WorkerArgs::UsageError,
        _ => WorkerArgs::NormalCli,
    }
}

/// Runs the worker or returns `None` to continue normal CLI command dispatch.
pub fn run_if_worker(arguments: &[String]) -> Option<ExitCode> {
    match classify_worker_args(arguments) {
        WorkerArgs::NormalCli => None,
        WorkerArgs::UsageError => {
            eprintln!("Usage: --steam-native-worker | --steam-native-op <session|achievements|achievement> --app-id <positive-id> [--name <api-name> --achieved <0|1>]");
            Some(ExitCode::from(sse_core::ExitCode::Usage as u8))
        }
        WorkerArgs::Worker => {
            let stdin = std::io::stdin();
            let stdout = std::io::stdout();
            let mut input = stdin.lock();
            let mut output = stdout.lock();
            match native::serve_native_worker(&mut input, &mut output) {
                Ok(()) => Some(ExitCode::SUCCESS),
                Err(error) => {
                    eprintln!("Error: {error}");
                    Some(ExitCode::from(1))
                }
            }
        }
        WorkerArgs::NativeOp => {
            let operation = match parse_native_op_args(arguments) {
                Ok(operation) => operation,
                Err(_) => {
                    eprintln!("Usage: --steam-native-worker | --steam-native-op <session|achievements|achievement> --app-id <positive-id> [--name <api-name> --achieved <0|1>]");
                    return Some(ExitCode::from(sse_core::ExitCode::Usage as u8));
                }
            };
            let stdout = std::io::stdout();
            let mut output = stdout.lock();
            let result = match operation {
                NativeOp::Session { app_id } => native::run_native_session(app_id, std::io::stdin(), &mut output),
                NativeOp::Achievements { app_id } => native::run_native_achievements(app_id, None, &mut output),
                NativeOp::Achievement { app_id, name, achieved } => {
                    native::run_native_achievements(app_id, Some((&name, achieved)), &mut output)
                }
            };
            match result {
                Ok(()) => Some(ExitCode::SUCCESS),
                Err(error) => {
                    eprintln!("Error: {error}");
                    Some(ExitCode::from(1))
                }
            }
        }
    }
}

/// Parsed long-running or achievement worker mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeOp {
    /// Keeps Steam callbacks active for one S.T.A.L.K.E.R. 2 Auto-Cloud session.
    Session {
        /// Steam application identifier.
        app_id: u32,
    },
    /// Lists achievements for a supported app.
    Achievements {
        /// Steam application identifier.
        app_id: u32,
    },
    /// Sets or clears one achievement.
    Achievement {
        /// Steam application identifier.
        app_id: u32,
        /// Stable Steam API achievement identifier.
        name: String,
        /// True to set the achievement; false to clear it.
        achieved: bool,
    },
}

/// Invalid command-line shape for `--steam-native-op`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeOpArgsError;

impl std::fmt::Display for NativeOpArgsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid Steam native operation arguments")
    }
}

impl std::error::Error for NativeOpArgsError {}

/// Parses native-op arguments before any library lookup or UI initialization.
pub fn parse_native_op_args(arguments: &[String]) -> Result<NativeOp, NativeOpArgsError> {
    if arguments.first().map(String::as_str) != Some("--steam-native-op") {
        return Err(NativeOpArgsError);
    }
    let operation = arguments.get(1).map(String::as_str).ok_or(NativeOpArgsError)?;
    let mut app_id = None;
    let mut name = None;
    let mut achieved = None;
    let mut index = 2;
    while index < arguments.len() {
        let key = arguments.get(index).map(String::as_str).ok_or(NativeOpArgsError)?;
        let value = arguments.get(index.saturating_add(1)).ok_or(NativeOpArgsError)?;
        match key {
            "--app-id" if app_id.is_none() => {
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(NativeOpArgsError);
                }
                app_id = value.parse::<u32>().ok().filter(|id| *id > 0);
                if app_id.is_none() {
                    return Err(NativeOpArgsError);
                }
            }
            "--name" if name.is_none() => name = Some(value.clone()),
            "--achieved" if achieved.is_none() => {
                achieved = match value.as_str() {
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => return Err(NativeOpArgsError),
                };
            }
            _ => return Err(NativeOpArgsError),
        }
        index = index.saturating_add(2);
    }
    let app_id = app_id.ok_or(NativeOpArgsError)?;
    match operation {
        "session" if name.is_none() && achieved.is_none() => Ok(NativeOp::Session { app_id }),
        "achievements" if name.is_none() && achieved.is_none() => Ok(NativeOp::Achievements { app_id }),
        "achievement" => Ok(NativeOp::Achievement {
            app_id,
            name: name.filter(|value| !value.is_empty()).ok_or(NativeOpArgsError)?,
            achieved: achieved.ok_or(NativeOpArgsError)?,
        }),
        _ => Err(NativeOpArgsError),
    }
}

/// Failure returned while starting or supervising one native worker process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeProcessError {
    /// Human-readable worker or transport failure.
    pub message: String,
    /// True when a cloud write may have reached Steam and must not be retried.
    pub write_outcome_uncertain: bool,
}

impl std::fmt::Display for NativeProcessError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for NativeProcessError {}

/// Runs one RemoteStorage request in a sibling process using the JSON-lines/raw-bytes protocol.
pub fn run_native_sibling_worker(
    request: NativeRequest,
    timeout: Duration,
) -> Result<NativeResponse, NativeProcessError> {
    let app_id = match &request {
        NativeRequest::List { app_id } | NativeRequest::Read { app_id, .. } | NativeRequest::Write { app_id, .. } => {
            *app_id
        }
    };
    let header = native_protocol::encode_request_header(&request).map_err(|error| NativeProcessError {
        message: error.to_string(),
        write_outcome_uncertain: false,
    })?;
    let (payload, is_write) = match request {
        NativeRequest::Write { bytes, .. } => (bytes, true),
        _ => (Vec::new(), false),
    };
    let executable = std::env::current_exe().map_err(|error| NativeProcessError {
        message: error.to_string(),
        write_outcome_uncertain: false,
    })?;
    run_native_process(
        &executable,
        &[String::from("--steam-native-worker")],
        app_id,
        header,
        payload,
        timeout,
        is_write,
    )
}

fn run_native_operation(
    app_id: u32,
    operation: &[String],
    timeout: Duration,
) -> Result<NativeResponse, NativeProcessError> {
    let executable = std::env::current_exe().map_err(|error| NativeProcessError {
        message: error.to_string(),
        write_outcome_uncertain: false,
    })?;
    let mut arguments = Vec::with_capacity(operation.len().saturating_add(1));
    arguments.push(String::from("--steam-native-op"));
    arguments.extend(operation.iter().cloned());
    run_native_process(&executable, &arguments, app_id, Vec::new(), Vec::new(), timeout, false)
}

fn run_native_process(
    executable: &Path,
    arguments: &[String],
    app_id: u32,
    header: Vec<u8>,
    payload: Vec<u8>,
    timeout: Duration,
    is_write: bool,
) -> Result<NativeResponse, NativeProcessError> {
    if timeout.is_zero() {
        return Err(native_process_error("Steam worker timeout must be positive", false));
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| native_process_error("Steam worker timeout is too large", false))?;
    let app_id_text = app_id.to_string();
    let mut command = Command::new(executable);
    sse_sys::process::ProcessTree::configure(&mut command);
    command
        .args(arguments)
        .env("SteamAppId", &app_id_text)
        .env("SteamGameId", &app_id_text)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| native_process_error(error.to_string(), false))?;
    let process_tree = match sse_sys::process::ProcessTree::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(native_process_error(error.to_string(), false));
        }
    };
    let mut child_input = child
        .stdin
        .take()
        .ok_or_else(|| native_process_error("worker stdin was not piped", false))?;
    let child_output = child
        .stdout
        .take()
        .ok_or_else(|| native_process_error("worker stdout was not piped", false))?;
    let child_error = child
        .stderr
        .take()
        .ok_or_else(|| native_process_error("worker stderr was not piped", false))?;

    let payload_sent = Arc::new(AtomicBool::new(false));
    let payload_sent_writer = Arc::clone(&payload_sent);
    let (write_sender, write_receiver) = mpsc::channel();
    let writer = std::thread::spawn(move || {
        let result = child_input
            .write_all(&header)
            .and_then(|()| child_input.write_all(&payload))
            .map_err(|error| error.to_string());
        if result.is_ok() && is_write {
            payload_sent_writer.store(true, Ordering::Release);
        }
        drop(child_input);
        let _ = write_sender.send(result);
    });

    let (response_sender, response_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = BufReader::new(child_output);
        let response = native_protocol::read_response(&mut output).map_err(|error| error.to_string());
        let _ = response_sender.send(response);
    });
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    let stderr_reader = std::thread::spawn(move || {
        let tail = read_stderr_tail(child_error);
        let _ = stderr_sender.send(tail);
    });

    let mut writer_finished = false;
    let mut response = None;
    let mut protocol_failure = None;
    loop {
        if let Ok(result) = write_receiver.try_recv() {
            writer_finished = true;
            if let Err(message) = result {
                process_tree.terminate(&mut child);
                return Err(native_process_error(message, false));
            }
        }
        if response.is_none() {
            match response_receiver.try_recv() {
                Ok(Ok(result)) => response = Some(result),
                Ok(Err(message)) => {
                    protocol_failure = Some(message);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    protocol_failure
                        .get_or_insert_with(|| "Steam worker closed stdout before returning a response.".to_owned());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(status) = poll_child(&mut child, &process_tree)? {
            if response.is_none() {
                if let Ok(Ok(result)) = response_receiver.recv_timeout(Duration::from_millis(25)) {
                    response = Some(result);
                }
            }
            if !writer_finished {
                if let Ok(Err(message)) = write_receiver.recv_timeout(Duration::from_millis(20)) {
                    return Err(native_process_error(message, false));
                }
            }
            if let Some(response) = response {
                if status.success() || matches!(response, NativeResponse::Error { .. }) {
                    process_tree.terminate(&mut child);
                    let _ = writer.join();
                    let _ = reader.join();
                    let _ = stderr_reader.join();
                    return Ok(response);
                }
            }
            process_tree.terminate(&mut child);
            drop(writer);
            drop(reader);
            let stderr = stderr_receiver
                .recv_timeout(Duration::from_millis(20))
                .unwrap_or_default();
            let suffix = String::from_utf8_lossy(&stderr);
            let base = protocol_failure.unwrap_or_else(|| format!("Steam worker exited with {status}"));
            let message = if suffix.is_empty() {
                base
            } else {
                format!("{base}: {suffix}")
            };
            return Err(native_process_error(
                message,
                is_write && payload_sent.load(Ordering::Acquire),
            ));
        }
        if Instant::now() >= deadline {
            process_tree.terminate(&mut child);
            let uncertain = is_write && payload_sent.load(Ordering::Acquire);
            drop(writer);
            drop(reader);
            drop(stderr_reader);
            let seconds = timeout.as_secs().saturating_add(u64::from(timeout.subsec_nanos() != 0));
            return Err(native_process_error(
                if uncertain {
                    format!("Steam worker exceeded the {seconds} second timeout; write outcome is uncertain.")
                } else {
                    format!("Steam worker exceeded the {seconds} second timeout.")
                },
                uncertain,
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn read_stderr_tail(mut input: impl Read) -> Vec<u8> {
    const TAIL_LIMIT: usize = 64 * 1024;
    let mut tail = std::collections::VecDeque::with_capacity(TAIL_LIMIT);
    let mut chunk = [0_u8; 4096];
    loop {
        let read = match input.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for byte in chunk.iter().take(read).copied() {
            if tail.len() == TAIL_LIMIT {
                let _ = tail.pop_front();
            }
            tail.push_back(byte);
        }
    }
    tail.into_iter().collect()
}

fn native_process_error(message: impl Into<String>, write_outcome_uncertain: bool) -> NativeProcessError {
    NativeProcessError {
        message: message.into(),
        write_outcome_uncertain,
    }
}

/// Steam API proxy that runs every native operation in the sibling executable.
#[derive(Debug)]
pub struct WorkerSteamApi {
    app_id: Option<u32>,
    remote_timeout: Duration,
    achievement_timeout: Duration,
    write_target: Option<(String, u64)>,
    pending_achievement: Option<(String, bool)>,
}

impl Default for WorkerSteamApi {
    fn default() -> Self {
        Self {
            app_id: None,
            remote_timeout: Duration::from_secs(15),
            achievement_timeout: Duration::from_secs(30),
            write_target: None,
            pending_achievement: None,
        }
    }
}

impl WorkerSteamApi {
    /// Creates a proxy with the contract's default 15 s and 30 s process deadlines.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    fn app_id(&self) -> Result<u32, SteamError> {
        self.app_id
            .ok_or_else(|| SteamError::new("Steam RemoteStorage is not connected."))
    }

    fn remote(&self, request: NativeRequest) -> Result<NativeResponse, SteamError> {
        run_native_sibling_worker(request, self.remote_timeout).map_err(|error| SteamError::new(error.to_string()))
    }

    fn achievement_operation(&self, arguments: &[String]) -> Result<NativeResponse, SteamError> {
        run_native_operation(self.app_id()?, arguments, self.achievement_timeout)
            .map_err(|error| SteamError::new(error.to_string()))
    }
}

impl WorkerSteamApi {
    /// Holds one change until `store_stats`; a second change before storing is refused, not overwritten.
    fn queue_achievement(&mut self, name: &str, achieved: bool) -> Result<(), SteamError> {
        if self.pending_achievement.is_some() {
            return Err(SteamError::new(
                "another achievement change is waiting to be stored; store it before requesting this one",
            ));
        }
        self.pending_achievement = Some((name.to_owned(), achieved));
        Ok(())
    }
}

impl SteamApi for WorkerSteamApi {
    fn initialize(&mut self, app_id: u32) -> Result<(), SteamError> {
        if !crate::api::SUPPORTED_APP_IDS.contains(&app_id) {
            return Err(SteamError::new(
                "Steam operations are limited to supported S.T.A.L.K.E.R. releases.",
            ));
        }
        self.app_id = Some(app_id);
        Ok(())
    }

    fn run_callbacks(&mut self) -> Result<(), SteamError> {
        // Each request is isolated in a native process, which pumps callbacks before returning.
        Ok(())
    }

    fn list_files(&mut self) -> Result<Vec<CloudFile>, SteamError> {
        let response = self.remote(NativeRequest::List { app_id: self.app_id()? })?;
        match response {
            NativeResponse::Files(files) => Ok(files),
            NativeResponse::Error { message, .. } => Err(SteamError::new(message)),
            _ => Err(SteamError::new("Steam worker returned an unexpected list response.")),
        }
    }

    fn read_file(&mut self, name: &str) -> Result<Vec<u8>, SteamError> {
        let response = self.remote(NativeRequest::Read {
            app_id: self.app_id()?,
            file_name: name.to_owned(),
        })?;
        match response {
            NativeResponse::Data(bytes) => Ok(bytes),
            NativeResponse::Error { message, .. } => Err(SteamError::new(message)),
            _ => Err(SteamError::new("Steam worker returned an unexpected read response.")),
        }
    }

    fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), WriteFailure> {
        let app_id = self.app_id().map_err(WriteFailure::NotAttempted)?;
        let size = data.len();
        let response = run_native_sibling_worker(
            NativeRequest::Write {
                app_id,
                file_name: name.to_owned(),
                size,
                bytes: data.to_vec(),
            },
            self.remote_timeout,
        );
        match response {
            Ok(NativeResponse::Ok) => {
                self.write_target = Some((name.to_owned(), u64::try_from(size).unwrap_or(u64::MAX)));
                Ok(())
            }
            Ok(NativeResponse::Error { message, stage }) => Err(match stage {
                Some(WriteStage::WriteRejected) => WriteFailure::Rejected(SteamError::new(message)),
                Some(WriteStage::AfterWrite) => WriteFailure::Uncertain(SteamError::new(message)),
                Some(WriteStage::BeforeWrite) | None => WriteFailure::NotAttempted(SteamError::new(message)),
            }),
            Ok(_) => Err(WriteFailure::Uncertain(SteamError::new(
                "Steam worker returned an unexpected write response",
            ))),
            Err(error) if error.write_outcome_uncertain => {
                Err(WriteFailure::Uncertain(SteamError::new(error.to_string())))
            }
            Err(error) => Err(WriteFailure::NotAttempted(SteamError::new(error.to_string()))),
        }
    }

    fn file_persisted(&mut self, name: &str) -> Result<bool, SteamError> {
        let files = self.list_files()?;
        let expected_size = self
            .write_target
            .as_ref()
            .filter(|(expected_name, _)| expected_name == name)
            .map(|(_, size)| *size);
        Ok(files.iter().any(|file| {
            file.name.replace('\\', "/") == name.replace('\\', "/")
                && file.exists
                && file.persisted
                && expected_size.is_none_or(|size| file.size == size)
        }))
    }

    fn achievements(&mut self) -> Result<Vec<Achievement>, SteamError> {
        let response = self.achievement_operation(&[
            String::from("achievements"),
            String::from("--app-id"),
            self.app_id()?.to_string(),
        ])?;
        match response {
            NativeResponse::Achievements(entries) => Ok(entries),
            NativeResponse::Error { message, .. } => Err(SteamError::new(message)),
            _ => Err(SteamError::new(
                "Steam worker returned an unexpected achievements response.",
            )),
        }
    }

    fn set_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        self.queue_achievement(name, true)
    }

    fn clear_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        self.queue_achievement(name, false)
    }

    fn store_stats(&mut self) -> Result<(), SteamError> {
        let Some((name, achieved)) = self.pending_achievement.take() else {
            return Ok(());
        };
        let response = self.achievement_operation(&[
            String::from("achievement"),
            String::from("--app-id"),
            self.app_id()?.to_string(),
            String::from("--name"),
            name.clone(),
            String::from("--achieved"),
            if achieved { String::from("1") } else { String::from("0") },
        ])?;
        match response {
            NativeResponse::Achievement(entry) if entry.name == name && entry.achieved == achieved => Ok(()),
            NativeResponse::Error { message, .. } => Err(SteamError::new(message)),
            _ => Err(SteamError::new(
                "Steam worker returned an unexpected achievement response.",
            )),
        }
    }
}

/// Parent-side handle for a three-hour bounded S.T.A.L.K.E.R. 2 Steam session.
pub struct SteamGameSession {
    close_sender: Option<mpsc::Sender<SessionCommand>>,
    done_receiver: mpsc::Receiver<Result<(), NativeProcessError>>,
    manager: Option<std::thread::JoinHandle<()>>,
}

impl SteamGameSession {
    /// Starts a session and returns after the worker sends its ready line (15 s maximum).
    pub fn start(app_id: u32) -> Result<Self, NativeProcessError> {
        if app_id != 1_643_320 {
            return Err(native_process_error(
                "Steam game sessions are supported only for S.T.A.L.K.E.R. 2.",
                false,
            ));
        }
        let (close_sender, close_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::channel();
        let (done_sender, done_receiver) = mpsc::channel();
        let manager = std::thread::spawn(move || {
            let result = run_session_process(app_id, close_receiver, ready_sender);
            let _ = done_sender.send(result);
        });
        match ready_receiver.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(Self {
                close_sender: Some(close_sender),
                done_receiver,
                manager: Some(manager),
            }),
            Ok(Err(error)) => {
                let _ = manager.join();
                Err(error)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let _ = close_sender.send(SessionCommand::Abort);
                Err(native_process_error(
                    "Steam worker exceeded the 15 second timeout.",
                    false,
                ))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let _ = manager.join();
                Err(native_process_error(
                    "Steam worker closed stdout before returning a response.",
                    false,
                ))
            }
        }
    }

    /// Closes stdin, waits for the four final callback passes, and requires the worker's ok response.
    pub fn shutdown(mut self) -> Result<(), NativeProcessError> {
        let close_sender = self.close_sender.take();
        if let Some(sender) = close_sender.as_ref() {
            let _ = sender.send(SessionCommand::Close);
        }
        let result = match self.done_receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if let Some(sender) = close_sender {
                    let _ = sender.send(SessionCommand::Abort);
                }
                if self.done_receiver.recv_timeout(Duration::from_secs(2)).is_ok() {
                    if let Some(manager) = self.manager.take() {
                        let _ = manager.join();
                    }
                }
                return Err(native_process_error(
                    "Steam worker exceeded the 30 second shutdown timeout.",
                    false,
                ));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(manager) = self.manager.take() {
                    let _ = manager.join();
                }
                return Err(native_process_error(
                    "Steam worker exited without a shutdown response.",
                    false,
                ));
            }
        };
        if let Some(manager) = self.manager.take() {
            let _ = manager.join();
        }
        result
    }
}

impl Drop for SteamGameSession {
    fn drop(&mut self) {
        if let Some(sender) = self.close_sender.take() {
            let _ = sender.send(SessionCommand::Close);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionCommand {
    Close,
    Abort,
}

fn run_session_process(
    app_id: u32,
    close_receiver: mpsc::Receiver<SessionCommand>,
    ready_sender: mpsc::Sender<Result<(), NativeProcessError>>,
) -> Result<(), NativeProcessError> {
    let executable = std::env::current_exe().map_err(|error| native_process_error(error.to_string(), false))?;
    run_session_process_with_executable(&executable, app_id, close_receiver, ready_sender)
}

fn run_session_process_with_executable(
    executable: &Path,
    app_id: u32,
    close_receiver: mpsc::Receiver<SessionCommand>,
    ready_sender: mpsc::Sender<Result<(), NativeProcessError>>,
) -> Result<(), NativeProcessError> {
    let app_id_text = app_id.to_string();
    let mut command = Command::new(executable);
    sse_sys::process::ProcessTree::configure(&mut command);
    let mut child = command
        .arg("--steam-native-op")
        .arg("session")
        .arg("--app-id")
        .arg(&app_id_text)
        .env("SteamAppId", &app_id_text)
        .env("SteamGameId", &app_id_text)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| native_process_error(error.to_string(), false))?;
    let process_tree = match sse_sys::process::ProcessTree::attach(&child) {
        Ok(process_tree) => process_tree,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(native_process_error(error.to_string(), false));
        }
    };
    let child_input = child
        .stdin
        .take()
        .ok_or_else(|| native_process_error("worker stdin was not piped", false))?;
    let child_output = child
        .stdout
        .take()
        .ok_or_else(|| native_process_error("worker stdout was not piped", false))?;
    let child_error = child
        .stderr
        .take()
        .ok_or_else(|| native_process_error("worker stderr was not piped", false))?;
    let (response_sender, response_receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut output = BufReader::new(child_output);
        loop {
            let line = match native_protocol::read_header(&mut output) {
                Ok(line) => line,
                Err(error) => {
                    if error.message.starts_with("Steam worker closed the pipe") {
                        break;
                    }
                    let _ = response_sender.send(Err(error.to_string()));
                    break;
                }
            };
            let mut framed_line = line;
            framed_line.push(b'\n');
            let response = native_protocol::read_response(&mut BufReader::new(std::io::Cursor::new(framed_line)))
                .map_err(|error| error.to_string());
            if response_sender.send(response).is_err() {
                break;
            }
        }
    });
    let (stderr_sender, stderr_receiver) = mpsc::channel();
    let stderr_reader = std::thread::spawn(move || {
        let _ = stderr_sender.send(read_stderr_tail(child_error));
    });

    let startup_deadline = Instant::now()
        .checked_add(Duration::from_secs(15))
        .ok_or_else(|| native_process_error("Steam worker startup deadline overflowed.", false))?;
    let mut child_input = Some(child_input);
    loop {
        match response_receiver.try_recv() {
            Ok(Ok(NativeResponse::Ready)) => {
                let _ = ready_sender.send(Ok(()));
                break;
            }
            Ok(Ok(NativeResponse::Error { message, .. })) => {
                let error = native_process_error(message, false);
                let _ = ready_sender.send(Err(error.clone()));
                process_tree.terminate(&mut child);
                drop(reader);
                drop(stderr_reader);
                return Err(error);
            }
            Ok(Ok(_)) => {
                let error = native_process_error("Steam worker returned an unexpected session response.", false);
                let _ = ready_sender.send(Err(error.clone()));
                process_tree.terminate(&mut child);
                drop(reader);
                drop(stderr_reader);
                return Err(error);
            }
            Ok(Err(message)) => {
                let error = native_process_error(message, false);
                let _ = ready_sender.send(Err(error.clone()));
                process_tree.terminate(&mut child);
                drop(reader);
                drop(stderr_reader);
                return Err(error);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                let error = native_process_error("Steam worker closed stdout before returning a response.", false);
                let _ = ready_sender.send(Err(error.clone()));
                process_tree.terminate(&mut child);
                drop(reader);
                drop(stderr_reader);
                return Err(error);
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if let Some(status) = poll_child(&mut child, &process_tree)? {
            process_tree.terminate(&mut child);
            let stderr = stderr_receiver
                .recv_timeout(Duration::from_millis(20))
                .unwrap_or_default();
            let detail = String::from_utf8_lossy(&stderr);
            let message = if detail.is_empty() {
                format!("Steam worker exited with {status} before the session was ready.")
            } else {
                format!("Steam worker exited with {status}: {detail}")
            };
            let error = native_process_error(message, false);
            let _ = ready_sender.send(Err(error.clone()));
            drop(reader);
            drop(stderr_reader);
            return Err(error);
        }
        if Instant::now() >= startup_deadline {
            process_tree.terminate(&mut child);
            let error = native_process_error("Steam worker exceeded the 15 second timeout.", false);
            let _ = ready_sender.send(Err(error.clone()));
            drop(child_input.take());
            drop(reader);
            drop(stderr_reader);
            return Err(error);
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    let session_deadline = Instant::now()
        .checked_add(Duration::from_secs(3 * 60 * 60 + 30))
        .ok_or_else(|| native_process_error("Steam worker session deadline overflowed.", false))?;
    let mut requested_close = false;
    loop {
        match close_receiver.try_recv() {
            Ok(SessionCommand::Abort) => {
                process_tree.terminate(&mut child);
                drop(reader);
                drop(stderr_reader);
                return Err(native_process_error(
                    "Steam worker shutdown was aborted after its timeout.",
                    false,
                ));
            }
            Ok(SessionCommand::Close) | Err(mpsc::TryRecvError::Disconnected) if !requested_close => {
                requested_close = true;
                drop(child_input.take());
            }
            _ => {}
        }
        if let Some(status) = poll_child(&mut child, &process_tree)? {
            process_tree.terminate(&mut child);
            let response = response_receiver.recv_timeout(Duration::from_millis(50)).ok();
            let stderr = stderr_receiver
                .recv_timeout(Duration::from_millis(20))
                .unwrap_or_default();
            let _ = reader.join();
            let _ = stderr_reader.join();
            return session_exit_result(status, response, &stderr, requested_close);
        }
        if Instant::now() >= session_deadline {
            process_tree.terminate(&mut child);
            drop(reader);
            drop(stderr_reader);
            return Err(native_process_error(
                "Steam worker exceeded the session lifetime.",
                false,
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn session_exit_result(
    status: std::process::ExitStatus,
    response: Option<Result<NativeResponse, String>>,
    stderr: &[u8],
    requested_close: bool,
) -> Result<(), NativeProcessError> {
    match response {
        Some(Ok(NativeResponse::Ok)) if status.success() && requested_close => Ok(()),
        Some(Ok(NativeResponse::Error { message, .. })) => Err(native_process_error(message, false)),
        Some(Err(message)) => Err(native_process_error(message, false)),
        _ => {
            let detail = String::from_utf8_lossy(stderr);
            let message = if detail.is_empty() {
                format!("Steam worker exited with {status} without a valid session response.")
            } else {
                format!("Steam worker exited with {status}: {detail}")
            };
            Err(native_process_error(message, false))
        }
    }
}

fn poll_child(
    child: &mut std::process::Child,
    process_tree: &sse_sys::process::ProcessTree,
) -> Result<Option<std::process::ExitStatus>, NativeProcessError> {
    match process_tree.try_wait(child) {
        Ok(status) => Ok(status),
        Err(error) => {
            process_tree.terminate(child);
            Err(native_process_error(error.to_string(), false))
        }
    }
}

#[cfg(test)]
mod pending_achievement_tests {
    use super::{SteamApi, WorkerSteamApi};

    #[test]
    fn second_achievement_change_before_store_is_refused_not_overwritten() {
        let mut api = WorkerSteamApi::new();
        assert!(api.set_achievement("ACH_ONE").is_ok());
        assert!(api.clear_achievement("ACH_TWO").is_err());
        assert!(api.set_achievement("ACH_THREE").is_err());
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use super::*;
    #[cfg(unix)]
    use std::time::Instant;

    #[cfg(unix)]
    fn temp_dir(label: &str) -> std::path::PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let path = std::env::temp_dir().join(format!("sse-worker-{label}-{}-{nanos}", std::process::id()));
        assert!(std::fs::create_dir_all(&path).is_ok());
        path
    }

    #[cfg(unix)]
    fn worker_script(directory: &std::path::Path, script: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = directory.join("worker");
        assert!(std::fs::write(&path, script).is_ok());
        let permissions = std::fs::metadata(&path).map(|metadata| metadata.permissions());
        assert!(permissions.is_ok_and(|mut value| {
            value.set_mode(0o755);
            std::fs::set_permissions(&path, value).is_ok()
        }));
        path
    }

    #[cfg(unix)]
    #[test]
    fn timed_out_worker_returns_uncertain_for_a_write() {
        // Run a stable system shell directly so this timeout test does not execute a
        // freshly written script file that can transiently fail with ETXTBSY.
        let worker = Path::new("/bin/sh");
        let arguments = [
            String::from("-c"),
            String::from("cat >/dev/null\nexec sleep 3"),
            String::from("--steam-native-worker"),
        ];
        let request = NativeRequest::Write {
            app_id: 4500,
            file_name: "_appdata_/savedgames/slot.sav".into(),
            size: 4,
            bytes: b"save".to_vec(),
        };
        let header = native_protocol::encode_request_header(&request);
        assert!(header.is_ok());
        let header = header.unwrap_or_default();
        let bytes = match request {
            NativeRequest::Write { bytes, .. } => bytes,
            _ => Vec::new(),
        };
        let started = Instant::now();
        let result = run_native_process(
            worker,
            &arguments,
            4500,
            header,
            bytes,
            Duration::from_millis(250),
            true,
        );
        assert!(
            matches!(
                &result,
                Err(NativeProcessError {
                    write_outcome_uncertain: true,
                    ..
                })
            ),
            "unexpected timeout result: {result:?}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_does_not_join_reader_while_descendant_keeps_stdout_open() {
        let directory = temp_dir("timeout-descendant");
        let marker = directory.join("descendant-ran");
        let worker = worker_script(&directory, "#!/bin/sh\n(sleep 1; touch \"$2\") &\nwait\n");
        let request = NativeRequest::List { app_id: 4500 };
        let header = native_protocol::encode_request_header(&request);
        assert!(header.is_ok());
        let started = Instant::now();
        let arguments = [
            String::from("--steam-native-worker"),
            marker.to_string_lossy().into_owned(),
        ];
        let result = run_native_process(
            &worker,
            &arguments,
            4500,
            header.unwrap_or_default(),
            Vec::new(),
            Duration::from_millis(40),
            false,
        );
        assert!(matches!(
            result,
            Err(NativeProcessError {
                write_outcome_uncertain: false,
                ..
            })
        ));
        assert!(started.elapsed() < Duration::from_millis(500));
        std::thread::sleep(Duration::from_millis(1_100));
        assert!(!marker.exists());
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn native_worker_process_preserves_raw_read_bytes() {
        // Run a stable system shell directly so this test does not execute a
        // freshly written script file that can transiently fail with ETXTBSY.
        let worker = Path::new("/bin/sh");
        let arguments = [
            String::from("-c"),
            String::from("IFS= read -r request\nprintf '{\"type\":\"data\",\"size\":4}\\n'\nprintf test\n"),
            String::from("--steam-native-worker"),
        ];
        let response = run_native_process(
            worker,
            &arguments,
            4500,
            br#"{"operation":"read","appId":4500,"fileName":"slot.sav"}"#.to_vec(),
            Vec::new(),
            Duration::from_secs(1),
            false,
        );
        assert_eq!(response, Ok(NativeResponse::Data(b"test".to_vec())));
    }

    #[cfg(unix)]
    #[test]
    fn session_process_announces_ready_and_shuts_down_after_close() {
        let directory = temp_dir("session");
        let worker = worker_script(
            &directory,
            "#!/bin/sh\nprintf '{\"type\":\"ready\"}\\n'\nIFS= read -r close\nprintf '{\"type\":\"ok\"}\\n'\n",
        );
        let (close_sender, close_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::channel();
        let manager = std::thread::spawn(move || {
            run_session_process_with_executable(&worker, 1_643_320, close_receiver, ready_sender)
        });

        assert_eq!(ready_receiver.recv_timeout(Duration::from_secs(10)), Ok(Ok(())));
        assert!(close_sender.send(SessionCommand::Close).is_ok());
        assert_eq!(manager.join().ok(), Some(Ok(())));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[cfg(unix)]
    #[test]
    fn abort_command_terminates_a_session_worker() {
        let directory = temp_dir("session-abort");
        let worker = worker_script(
            &directory,
            "#!/bin/sh\nprintf '{\"type\":\"ready\"}\\n'\nexec sleep 5\n",
        );
        let (close_sender, close_receiver) = mpsc::channel();
        let (ready_sender, ready_receiver) = mpsc::channel();
        let started = Instant::now();
        let manager = std::thread::spawn(move || {
            run_session_process_with_executable(&worker, 1_643_320, close_receiver, ready_sender)
        });
        assert_eq!(ready_receiver.recv_timeout(Duration::from_secs(10)), Ok(Ok(())));
        assert!(close_sender.send(SessionCommand::Abort).is_ok());
        assert!(manager.join().is_ok_and(|result| result.is_err()));
        assert!(started.elapsed() < Duration::from_secs(1));
        let _ = std::fs::remove_dir_all(directory);
    }
}
