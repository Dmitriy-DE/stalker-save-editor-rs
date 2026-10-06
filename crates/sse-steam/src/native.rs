//! Native Steamworks adapter. All raw FFI is confined to `sse-sys`.

use sse_sys::steam::{RemoteStorageFile, SteamAchievement, SteamLibrary, SteamSession};
use std::io::{BufRead, Read, Write};
use std::time::{Duration, Instant};

use crate::api::{Achievement, CloudFile, SteamApi, SteamError, WriteFailure, WriteStage};
use crate::cloud::{validate_remote_save_path, SaveFormatVerifier, XRaySaveFormatVerifier, MAX_CLOUD_FILE_BYTES};
use crate::discovery::{default_steam_roots, locate_steam_api_library};
use crate::native_protocol::{self, NativeRequest};

const APP_SOC: u32 = 4_500;
const APP_CS: u32 = 20_510;
const APP_COP: u32 = 41_700;
const APP_SOC_EE: u32 = 2_427_410;
const APP_CS_EE: u32 = 2_427_420;
const APP_COP_EE: u32 = 2_427_430;
const APP_STALKER_2: u32 = 1_643_320;

/// Loads Steam's API library from the configured Steam roots.
pub fn load_default_library() -> Result<SteamLibrary, SteamError> {
    let path = locate_steam_api_library(default_steam_roots(), cfg!(windows))?
        .ok_or_else(|| SteamError::new("Steam libsteam_api library was not found."))?;
    SteamLibrary::load(&path).map_err(|error| SteamError::new(format!("Could not locate Steam libsteam_api: {error}")))
}

/// Steam API adapter borrowed from the process-owned library handle.
pub struct NativeSteamApi<'library> {
    library: &'library SteamLibrary,
    session: Option<SteamSession<'library>>,
    app_id: Option<u32>,
}

impl<'library> NativeSteamApi<'library> {
    /// Creates an adapter; Steam initialization happens only when a request calls `initialize`.
    #[must_use]
    pub const fn new(library: &'library SteamLibrary) -> Self {
        Self {
            library,
            session: None,
            app_id: None,
        }
    }

    fn session(&self) -> Result<&SteamSession<'library>, SteamError> {
        self.session
            .as_ref()
            .ok_or_else(|| SteamError::new("Steam RemoteStorage is not connected."))
    }

    fn stats_session(&self) -> Result<&SteamSession<'library>, SteamError> {
        self.session
            .as_ref()
            .ok_or_else(|| SteamError::new("Steam ISteamUserStats is not connected."))
    }

    fn ensure_remote_storage_app(&self) -> Result<(), SteamError> {
        if self.app_id.is_some_and(is_remote_storage_app) {
            Ok(())
        } else {
            Err(SteamError::new(
                "RemoteStorage save writing is supported only for official X-Ray trilogy releases.",
            ))
        }
    }
}

impl SteamApi for NativeSteamApi<'_> {
    fn initialize(&mut self, app_id: u32) -> Result<(), SteamError> {
        if !is_supported_app(app_id) {
            return Err(SteamError::new(
                "Steam operations are limited to supported S.T.A.L.K.E.R. releases.",
            ));
        }
        if let Some(current) = self.app_id {
            if current != app_id {
                return Err(SteamError::new("one Steam worker can serve only one app id"));
            }
            return Ok(());
        }
        self.session = Some(
            self.library
                .init()
                .map_err(|error| SteamError::new(error.to_string()))?,
        );
        self.app_id = Some(app_id);
        Ok(())
    }

    fn run_callbacks(&mut self) -> Result<(), SteamError> {
        self.session()?.run_callbacks();
        Ok(())
    }

    fn list_files(&mut self) -> Result<Vec<CloudFile>, SteamError> {
        self.ensure_remote_storage_app()?;
        let files = self
            .session()?
            .remote_storage()
            .map_err(|error| SteamError::new(error.to_string()))?
            .list_files()
            .map_err(|error| SteamError::new(error.to_string()))?;
        files.into_iter().map(cloud_file).collect()
    }

    fn read_file(&mut self, name: &str) -> Result<Vec<u8>, SteamError> {
        self.ensure_remote_storage_app()?;
        self.session()?
            .remote_storage()
            .map_err(|error| SteamError::new(error.to_string()))?
            .read_file(name)
            .map_err(|error| SteamError::new(error.to_string()))
    }

    fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), WriteFailure> {
        self.ensure_remote_storage_app().map_err(WriteFailure::NotAttempted)?;
        let result = self
            .session()
            .and_then(|session| {
                session
                    .remote_storage()
                    .map_err(|error| SteamError::new(error.to_string()))
            })
            .and_then(|remote| {
                remote
                    .write_file(name, data)
                    .map_err(|error| SteamError::new(error.to_string()))
            });
        match result {
            Ok(true) => Ok(()),
            Ok(false) => Err(WriteFailure::Rejected(SteamError::new(format!(
                "Steam RemoteStorage rejected the write for {name}."
            )))),
            Err(error) => Err(WriteFailure::NotAttempted(error)),
        }
    }

    fn file_persisted(&mut self, name: &str) -> Result<bool, SteamError> {
        self.ensure_remote_storage_app()?;
        self.session()?
            .remote_storage()
            .map_err(|error| SteamError::new(error.to_string()))?
            .file_persisted(name)
            .map_err(|error| SteamError::new(error.to_string()))
    }

    fn achievements(&mut self) -> Result<Vec<Achievement>, SteamError> {
        let native = self
            .stats_session()?
            .user_stats()
            .map_err(|error| SteamError::new(error.to_string()))?
            .achievements()
            .map_err(|error| SteamError::new(error.to_string()))?;
        native.into_iter().map(achievement).collect()
    }

    fn set_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        let accepted = self
            .stats_session()?
            .user_stats()
            .map_err(|error| SteamError::new(error.to_string()))?
            .set_achievement(name, true)
            .map_err(|error| SteamError::new(error.to_string()))?;
        if accepted {
            Ok(())
        } else {
            Err(SteamError::new(format!("Steam refused to change achievement {name}.")))
        }
    }

    fn clear_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        let accepted = self
            .stats_session()?
            .user_stats()
            .map_err(|error| SteamError::new(error.to_string()))?
            .set_achievement(name, false)
            .map_err(|error| SteamError::new(error.to_string()))?;
        if accepted {
            Ok(())
        } else {
            Err(SteamError::new(format!("Steam refused to change achievement {name}.")))
        }
    }

    fn store_stats(&mut self) -> Result<(), SteamError> {
        let stored = self
            .stats_session()?
            .user_stats()
            .map_err(|error| SteamError::new(error.to_string()))?
            .store_stats();
        if stored {
            Ok(())
        } else {
            Err(SteamError::new("Steam refused to change the achievement state."))
        }
    }
}

/// Runs one validated native-worker request. Save writes are checked before loading or initializing Steam.
pub fn serve_native_worker(input: &mut impl BufRead, output: &mut impl Write) -> Result<(), NativeWorkerFailure> {
    let header = match native_protocol::read_header(input) {
        Ok(header) => header,
        Err(error) => {
            let message = if error.message.contains("closed the pipe") {
                "Steam worker received no request.".to_owned()
            } else if error.message.contains("size limit") {
                "Steam worker request exceeded the size limit.".to_owned()
            } else {
                error.to_string()
            };
            return reject_native_worker(output, NativeWorkerFailure::new(message, None));
        }
    };
    let mut request = match native_protocol::parse_request(&header) {
        Ok(request) => request,
        Err(error) => {
            let stage = native_protocol::is_write_request_header(&header).then_some(WriteStage::BeforeWrite);
            return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), stage));
        }
    };
    let write_stage = match &request {
        NativeRequest::Write {
            app_id,
            file_name,
            size,
            ..
        } => {
            if !is_remote_storage_app(*app_id) {
                return reject_native_worker(
                    output,
                    NativeWorkerFailure::new(
                        "RemoteStorage writes are limited to official X-Ray trilogy releases.",
                        Some(WriteStage::BeforeWrite),
                    ),
                );
            }
            if validate_remote_save_path(*app_id, file_name).is_err() {
                return reject_native_worker(
                    output,
                    NativeWorkerFailure::new(
                        "RemoteStorage write path is outside the selected release's save allow-list.",
                        Some(WriteStage::BeforeWrite),
                    ),
                );
            }
            if *size == 0 || *size > MAX_CLOUD_FILE_BYTES {
                return reject_native_worker(
                    output,
                    NativeWorkerFailure::new(
                        "RemoteStorage write size is outside the supported range.",
                        Some(WriteStage::BeforeWrite),
                    ),
                );
            }
            Some(WriteStage::BeforeWrite)
        }
        _ => None,
    };
    if let Err(error) =
        native_protocol::read_request_body(input, &mut request).and_then(|()| native_protocol::finish_request(input))
    {
        return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), write_stage));
    }

    let mut verifier = XRaySaveFormatVerifier::default();
    if let NativeRequest::Write {
        app_id,
        file_name,
        bytes,
        ..
    } = &request
    {
        if verifier.verify(*app_id, file_name, bytes).is_err() {
            return reject_native_worker(
                output,
                NativeWorkerFailure::new(
                    "RemoteStorage write payload is not a save for the selected release.",
                    Some(WriteStage::BeforeWrite),
                ),
            );
        }
    }

    let app_id = match &request {
        NativeRequest::List { app_id } | NativeRequest::Read { app_id, .. } | NativeRequest::Write { app_id, .. } => {
            *app_id
        }
    };
    let library = match load_default_library() {
        Ok(library) => library,
        Err(error) => {
            let stage = write_stage;
            return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), stage));
        }
    };
    let mut api = NativeSteamApi::new(&library);
    if let Err(error) = api.initialize(app_id) {
        return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), write_stage));
    }

    match request {
        NativeRequest::List { .. } => {
            let files = match api.list_files() {
                Ok(files) => files,
                Err(error) => {
                    return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), None));
                }
            };
            native_protocol::write_files_response(output, &files)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            Ok(())
        }
        NativeRequest::Read { file_name, .. } => {
            let bytes = match api.read_file(&file_name) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return reject_native_worker(output, NativeWorkerFailure::new(error.to_string(), None));
                }
            };
            native_protocol::write_data_response(output, &bytes)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            Ok(())
        }
        NativeRequest::Write { file_name, bytes, .. } => {
            api.write_file(&file_name, &bytes).map_err(|failure| {
                let stage = failure.stage();
                let message = match failure {
                    WriteFailure::NotAttempted(error)
                    | WriteFailure::Rejected(error)
                    | WriteFailure::Uncertain(error) => error.to_string(),
                };
                NativeWorkerFailure::new(message, Some(stage))
            })?;
            if let Err(error) = api.run_callbacks() {
                return reject_native_worker(
                    output,
                    NativeWorkerFailure::new(error.to_string(), Some(WriteStage::AfterWrite)),
                );
            }
            native_protocol::write_ok_response(output)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), Some(WriteStage::AfterWrite)))?;
            Ok(())
        }
    }
}

/// Runs one native achievements operation and writes its PascalCase JSON response.
pub fn run_native_achievements(
    app_id: u32,
    mutation: Option<(&str, bool)>,
    output: &mut impl Write,
) -> Result<(), NativeWorkerFailure> {
    let result = (|| {
        if !is_supported_app(app_id) {
            return Err("Achievements are limited to official S.T.A.L.K.E.R. releases.".to_owned());
        }
        let library = load_default_library().map_err(|error| error.to_string())?;
        let mut api = NativeSteamApi::new(&library);
        api.initialize(app_id).map_err(|error| error.to_string())?;
        api.run_callbacks().map_err(|error| error.to_string())?;
        let Some((name, achieved)) = mutation else {
            let entries = api.achievements().map_err(|error| error.to_string())?;
            let header = native_protocol::encode_achievements(&entries).map_err(|error| error.to_string())?;
            output.write_all(&header).map_err(|error| error.to_string())?;
            return Ok(());
        };
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return Err("achievement name is invalid".to_owned());
        }
        let entries = api.achievements().map_err(|error| error.to_string())?;
        if !entries.iter().any(|entry| entry.name == name) {
            return Err(format!("The game does not define achievement {name}."));
        }
        if achieved {
            api.set_achievement(name).map_err(|error| error.to_string())?;
        } else {
            api.clear_achievement(name).map_err(|error| error.to_string())?;
        }
        api.store_stats().map_err(|error| error.to_string())?;

        for attempt in 0..10 {
            api.run_callbacks().map_err(|error| error.to_string())?;
            if let Some(entry) = api
                .achievements()
                .map_err(|error| error.to_string())?
                .into_iter()
                .find(|entry| entry.name == name && entry.achieved == achieved)
            {
                let header = native_protocol::encode_achievement(&entry).map_err(|error| error.to_string())?;
                output.write_all(&header).map_err(|error| error.to_string())?;
                return Ok(());
            }
            if attempt < 9 {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
        Err(format!("Steam did not confirm achievement state {name}."))
    })();

    match result {
        Ok(()) => Ok(()),
        Err(message) => {
            let header = native_protocol::encode_operation_error(&message)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            output
                .write_all(&header)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            Err(NativeWorkerFailure::new(message, None))
        }
    }
}

/// Runs the bounded Auto-Cloud session worker for S.T.A.L.K.E.R. 2.
pub fn run_native_session(
    app_id: u32,
    mut input: std::io::Stdin,
    output: &mut impl Write,
) -> Result<(), NativeWorkerFailure> {
    if app_id != APP_STALKER_2 {
        return send_session_error(output, "Steam game sessions are supported only for S.T.A.L.K.E.R. 2.");
    }
    let library = match load_default_library() {
        Ok(library) => library,
        Err(error) => return send_session_error(output, &error.to_string()),
    };
    let mut api = NativeSteamApi::new(&library);
    if let Err(error) = api.initialize(app_id) {
        return send_session_error(output, &error.to_string());
    }
    if let Err(error) = api.run_callbacks() {
        return send_session_error(output, &error.to_string());
    }
    output
        .write_all(b"{\"type\":\"ready\"}\n")
        .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
    output
        .flush()
        .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;

    let (closed_sender, closed_receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut byte = [0_u8; 1];
        loop {
            match input.read(&mut byte) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let _ = closed_sender.send(());
    });
    let lifetime = Duration::from_secs(3 * 60 * 60);
    let started = Instant::now();
    loop {
        if closed_receiver.try_recv().is_ok() {
            for _ in 0..4 {
                api.run_callbacks()
                    .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
                std::thread::sleep(Duration::from_millis(250));
            }
            output
                .write_all(b"{\"type\":\"ok\"}\n")
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            output
                .flush()
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            return Ok(());
        }
        if started.elapsed() >= lifetime {
            let message = "Steam game session exceeded its lifetime.";
            let header = native_protocol::encode_session_error(message)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            output
                .write_all(&header)
                .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
            return Err(NativeWorkerFailure::new(message, None));
        }
        api.run_callbacks()
            .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Failure returned by the native worker, including a save-write stage when known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeWorkerFailure {
    /// Human-readable error.
    pub message: String,
    /// Save-write phase, if the request was a write.
    pub stage: Option<WriteStage>,
}

impl NativeWorkerFailure {
    fn new(message: impl Into<String>, stage: Option<WriteStage>) -> Self {
        Self {
            message: message.into(),
            stage,
        }
    }
}

impl std::fmt::Display for NativeWorkerFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

fn reject_native_worker(output: &mut impl Write, failure: NativeWorkerFailure) -> Result<(), NativeWorkerFailure> {
    native_protocol::write_error_response(output, &failure.message, failure.stage)
        .map_err(|error| NativeWorkerFailure::new(error.to_string(), failure.stage))?;
    Err(failure)
}

fn send_session_error(output: &mut impl Write, message: &str) -> Result<(), NativeWorkerFailure> {
    let header = native_protocol::encode_session_error(message)
        .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
    output
        .write_all(&header)
        .map_err(|error| NativeWorkerFailure::new(error.to_string(), None))?;
    Err(NativeWorkerFailure::new(message, None))
}

fn cloud_file(file: RemoteStorageFile) -> Result<CloudFile, SteamError> {
    let size =
        u64::try_from(file.size).map_err(|_| SteamError::new("Steam RemoteStorage returned a negative file size"))?;
    Ok(CloudFile {
        name: file.name,
        size,
        timestamp: file.timestamp,
        persisted: file.persisted,
        exists: file.exists,
    })
}

fn achievement(entry: SteamAchievement) -> Result<Achievement, SteamError> {
    Ok(Achievement {
        name: entry.name,
        display_name: entry.display_name,
        description: entry.description,
        hidden: entry.hidden,
        achieved: entry.achieved,
        unlock_time: entry.unlock_time,
    })
}

const fn is_remote_storage_app(app_id: u32) -> bool {
    matches!(app_id, APP_SOC | APP_CS | APP_COP | APP_SOC_EE | APP_CS_EE | APP_COP_EE)
}

const fn is_supported_app(app_id: u32) -> bool {
    is_remote_storage_app(app_id) || app_id == APP_STALKER_2
}

#[cfg(test)]
mod tests {
    use super::{cloud_file, is_remote_storage_app, APP_COP, APP_SOC};
    use crate::api::SteamError;
    use sse_sys::steam::RemoteStorageFile;

    #[test]
    fn only_official_xray_releases_allow_remote_storage() {
        assert!(is_remote_storage_app(APP_SOC));
        assert!(is_remote_storage_app(APP_COP));
        assert!(!is_remote_storage_app(1_643_320));
        assert!(!is_remote_storage_app(1));
    }

    #[test]
    fn cloud_file_conversion_refuses_negative_size() {
        let file = RemoteStorageFile {
            name: "save.sav".to_owned(),
            size: -1,
            timestamp: 1,
            persisted: true,
            exists: true,
        };
        let result = cloud_file(file);
        assert!(matches!(result, Err(SteamError { .. })));
    }

    #[test]
    fn cloud_file_conversion_preserves_metadata() -> sse_core::Result<()> {
        let file = RemoteStorageFile {
            name: "save.sav".to_owned(),
            size: 123,
            timestamp: 456,
            persisted: true,
            exists: true,
        };
        let converted = cloud_file(file).map_err(|error| sse_core::Error::Refused(error.message))?;
        assert_eq!(converted.name, "save.sav");
        assert_eq!(converted.size, 123);
        assert_eq!(converted.timestamp, 456);
        Ok(())
    }
}
