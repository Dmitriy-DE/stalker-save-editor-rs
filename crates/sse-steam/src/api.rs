//! Safe boundary for Steam operations. The native implementation belongs in `sse-sys`.

use std::collections::HashMap;
use std::fmt::{Display, Formatter};

/// Failure returned by the Steam API boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SteamError {
    /// Human-readable operation failure.
    pub message: String,
    /// Transaction phase when the error came from a cloud write.
    pub stage: Option<WriteStage>,
}

impl SteamError {
    /// Creates an operation failure.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            stage: None,
        }
    }

    /// Marks this error with the cloud-write phase where it occurred.
    #[must_use]
    pub fn at_stage(mut self, stage: WriteStage) -> Self {
        self.stage = Some(stage);
        self
    }
}

impl Display for SteamError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SteamError {}

/// Transaction phase used to distinguish a rejected request from an uncertain write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteStage {
    /// Validation or setup failed before Steam was asked to write.
    BeforeWrite,
    /// Steam's `FileWrite` call explicitly returned false.
    WriteRejected,
    /// Steam accepted the request, but persistence or read-back could not be confirmed.
    AfterWrite,
}

impl WriteStage {
    /// Returns the stable worker-protocol spelling required by the Steam contract.
    #[must_use]
    pub const fn as_protocol_value(self) -> &'static str {
        match self {
            Self::BeforeWrite => "before_write",
            Self::WriteRejected => "write_rejected",
            Self::AfterWrite => "after_write",
        }
    }
}

/// A Steam Remote Storage entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudFile {
    /// Remote filename.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Steam's file timestamp.
    pub timestamp: i64,
    /// Whether Steam has persisted the file to cloud storage.
    pub persisted: bool,
    /// Whether the file currently exists according to Steam.
    pub exists: bool,
}

/// One Steam achievement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Achievement {
    /// Stable Steam API identifier.
    pub name: String,
    /// Display name returned by Steam.
    pub display_name: String,
    /// Description returned by Steam.
    pub description: String,
    /// Whether the achievement is hidden until earned.
    pub hidden: bool,
    /// Whether it is currently unlocked.
    pub achieved: bool,
    /// Unlock time as a Unix timestamp, or zero when locked.
    pub unlock_time: u32,
}

/// Distinguishes a rejected write from a request whose outcome became uncertain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteFailure {
    /// The operation was rejected before a write request was issued.
    NotAttempted(SteamError),
    /// Steam explicitly rejected the write request.
    Rejected(SteamError),
    /// The write request was issued but its result could not be established.
    Uncertain(SteamError),
}

impl WriteFailure {
    /// Returns the phase corresponding to this low-level write result.
    #[must_use]
    pub const fn stage(&self) -> WriteStage {
        match self {
            Self::NotAttempted(_) => WriteStage::BeforeWrite,
            Self::Rejected(_) => WriteStage::WriteRejected,
            Self::Uncertain(_) => WriteStage::AfterWrite,
        }
    }
}

/// Operations required by cloud and achievement services.
pub trait SteamApi {
    /// Initializes Steam for the selected application.
    fn initialize(&mut self, app_id: u32) -> Result<(), SteamError>;
    /// Pumps pending Steam callbacks.
    fn run_callbacks(&mut self) -> Result<(), SteamError>;
    /// Lists cloud files.
    fn list_files(&mut self) -> Result<Vec<CloudFile>, SteamError>;
    /// Reads one cloud file.
    fn read_file(&mut self, name: &str) -> Result<Vec<u8>, SteamError>;
    /// Writes one cloud file. An error after request dispatch is `Uncertain`.
    fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), WriteFailure>;
    /// Reports whether Steam has persisted a cloud file.
    fn file_persisted(&mut self, name: &str) -> Result<bool, SteamError>;
    /// Lists known achievements and their current state.
    fn achievements(&mut self) -> Result<Vec<Achievement>, SteamError>;
    /// Requests an achievement unlock.
    fn set_achievement(&mut self, name: &str) -> Result<(), SteamError>;
    /// Requests an achievement clear.
    fn clear_achievement(&mut self, name: &str) -> Result<(), SteamError>;
    /// Persists achievement changes.
    fn store_stats(&mut self) -> Result<(), SteamError>;
}

/// Scripted Steam API used by deterministic tests and downstream adapters.
#[derive(Debug, Default)]
pub struct ScriptedSteamApi {
    /// Remote files keyed by exact name.
    pub files: HashMap<String, Vec<u8>>,
    /// File listing returned by Steam.
    pub listed_files: Vec<CloudFile>,
    /// Achievements keyed by stable API name.
    pub achievements: HashMap<String, bool>,
    /// Write behavior for the next write request.
    pub write_behavior: WriteBehavior,
    /// Number of cloud write requests issued.
    pub write_count: usize,
    /// App id passed to `initialize`.
    pub initialized_app_id: Option<u32>,
}

/// Scripted result for cloud writes.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum WriteBehavior {
    /// Write succeeds normally.
    #[default]
    Succeed,
    /// Fail before any write request is issued.
    FailBeforeWrite(String),
    /// Simulate Steam returning false from `FileWrite`.
    RejectWrite(String),
    /// Store the bytes, then simulate a lost response.
    FailAfterWrite(String),
}

impl SteamApi for ScriptedSteamApi {
    fn initialize(&mut self, app_id: u32) -> Result<(), SteamError> {
        self.initialized_app_id = Some(app_id);
        Ok(())
    }

    fn run_callbacks(&mut self) -> Result<(), SteamError> {
        Ok(())
    }

    fn list_files(&mut self) -> Result<Vec<CloudFile>, SteamError> {
        if self.listed_files.is_empty() {
            Ok(self
                .files
                .iter()
                .map(|(name, bytes)| CloudFile {
                    name: name.clone(),
                    size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
                    timestamp: 0,
                    persisted: true,
                    exists: true,
                })
                .collect())
        } else {
            Ok(self.listed_files.clone())
        }
    }

    fn read_file(&mut self, name: &str) -> Result<Vec<u8>, SteamError> {
        self.files
            .get(name)
            .cloned()
            .ok_or_else(|| SteamError::new("cloud file not found"))
    }

    fn write_file(&mut self, name: &str, data: &[u8]) -> Result<(), WriteFailure> {
        match self.write_behavior.clone() {
            WriteBehavior::FailBeforeWrite(message) => Err(WriteFailure::NotAttempted(SteamError::new(message))),
            WriteBehavior::RejectWrite(message) => Err(WriteFailure::Rejected(SteamError::new(message))),
            WriteBehavior::FailAfterWrite(message) => {
                self.files.insert(name.to_owned(), data.to_vec());
                self.write_count = self.write_count.saturating_add(1);
                Err(WriteFailure::Uncertain(SteamError::new(message)))
            }
            WriteBehavior::Succeed => {
                self.files.insert(name.to_owned(), data.to_vec());
                self.write_count = self.write_count.saturating_add(1);
                Ok(())
            }
        }
    }

    fn file_persisted(&mut self, name: &str) -> Result<bool, SteamError> {
        Ok(self.files.contains_key(name))
    }

    fn achievements(&mut self) -> Result<Vec<Achievement>, SteamError> {
        Ok(self
            .achievements
            .iter()
            .map(|(name, achieved)| Achievement {
                name: name.clone(),
                display_name: name.clone(),
                description: String::new(),
                hidden: false,
                achieved: *achieved,
                unlock_time: 0,
            })
            .collect())
    }

    fn set_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        match self.achievements.get_mut(name) {
            Some(achieved) => {
                *achieved = true;
                Ok(())
            }
            None => Err(SteamError::new("unknown achievement")),
        }
    }

    fn clear_achievement(&mut self, name: &str) -> Result<(), SteamError> {
        match self.achievements.get_mut(name) {
            Some(achieved) => {
                *achieved = false;
                Ok(())
            }
            None => Err(SteamError::new("unknown achievement")),
        }
    }

    fn store_stats(&mut self) -> Result<(), SteamError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{SteamError, WriteFailure, WriteStage};

    #[test]
    fn write_failure_stage_names_match_worker_contract() {
        assert_eq!(WriteStage::BeforeWrite.as_protocol_value(), "before_write");
        assert_eq!(WriteStage::WriteRejected.as_protocol_value(), "write_rejected");
        assert_eq!(WriteStage::AfterWrite.as_protocol_value(), "after_write");
    }

    #[test]
    fn rejected_filewrite_is_distinct_from_an_unattempted_or_uncertain_write() {
        assert_eq!(
            WriteFailure::NotAttempted(SteamError::new("validation failed")).stage(),
            WriteStage::BeforeWrite
        );
        assert_eq!(
            WriteFailure::Rejected(SteamError::new("FileWrite returned false")).stage(),
            WriteStage::WriteRejected
        );
        assert_eq!(
            WriteFailure::Uncertain(SteamError::new("response lost")).stage(),
            WriteStage::AfterWrite
        );
    }
}
