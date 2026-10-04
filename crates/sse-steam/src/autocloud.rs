//! Read-only API adapter for local S.T.A.L.K.E.R. 2 Auto-Cloud files.

use std::path::{Path, PathBuf};

use crate::api::{Achievement, CloudFile, SteamApi, SteamError, WriteFailure};
use crate::discovery::{
    default_steam_roots, find_auto_cloud_root, list_auto_cloud_files, read_auto_cloud_file, steam_library_roots,
    STALKER_2_APP_ID,
};

/// Steam API surface backed by local S.T.A.L.K.E.R. 2 Auto-Cloud files.
///
/// Listing and reading use the discovered Steam library and Proton/Windows Auto-Cloud roots. Steam Remote Storage,
/// achievements and writes still require native `sse-sys` calls and are rejected by this adapter.
#[derive(Debug, Clone)]
pub struct AutoCloudSteamApi {
    steam_roots: Vec<PathBuf>,
    windows_local_app_data: Option<PathBuf>,
    selected_root: Option<PathBuf>,
}

impl AutoCloudSteamApi {
    /// Creates an adapter using explicitly supplied Steam library roots and optional Windows LocalAppData.
    #[must_use]
    pub fn with_roots(steam_roots: impl IntoIterator<Item = PathBuf>, windows_local_app_data: Option<PathBuf>) -> Self {
        Self {
            steam_roots: steam_roots.into_iter().collect(),
            windows_local_app_data,
            selected_root: None,
        }
    }

    fn root(&self) -> Result<&Path, SteamError> {
        self.selected_root
            .as_deref()
            .ok_or_else(|| SteamError::new("S.T.A.L.K.E.R. 2 Auto-Cloud has not been initialized"))
    }

    fn native_steam_unavailable() -> SteamError {
        SteamError::new("native Steam Remote Storage and achievement calls await sse-sys integration")
    }
}

impl Default for AutoCloudSteamApi {
    fn default() -> Self {
        let windows_local_app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
        Self::with_roots(default_steam_roots(), windows_local_app_data)
    }
}

impl SteamApi for AutoCloudSteamApi {
    fn initialize(&mut self, app_id: u32) -> Result<(), SteamError> {
        if app_id != STALKER_2_APP_ID {
            return Err(Self::native_steam_unavailable());
        }
        let libraries = steam_library_roots(self.steam_roots.clone())?;
        self.selected_root = Some(
            find_auto_cloud_root(app_id, self.windows_local_app_data.as_deref(), libraries)
                .ok_or_else(|| SteamError::new("S.T.A.L.K.E.R. 2 Auto-Cloud directory was not found"))?,
        );
        Ok(())
    }

    fn run_callbacks(&mut self) -> Result<(), SteamError> {
        self.root().map(|_| ())
    }

    fn list_files(&mut self) -> Result<Vec<CloudFile>, SteamError> {
        list_auto_cloud_files(self.root()?)
    }

    fn read_file(&mut self, name: &str) -> Result<Vec<u8>, SteamError> {
        read_auto_cloud_file(self.root()?, name)
    }

    fn write_file(&mut self, _name: &str, _data: &[u8]) -> Result<(), WriteFailure> {
        Err(WriteFailure::NotAttempted(Self::native_steam_unavailable()))
    }

    fn file_persisted(&mut self, _name: &str) -> Result<bool, SteamError> {
        Err(Self::native_steam_unavailable())
    }

    fn achievements(&mut self) -> Result<Vec<Achievement>, SteamError> {
        Err(Self::native_steam_unavailable())
    }

    fn set_achievement(&mut self, _name: &str) -> Result<(), SteamError> {
        Err(Self::native_steam_unavailable())
    }

    fn clear_achievement(&mut self, _name: &str) -> Result<(), SteamError> {
        Err(Self::native_steam_unavailable())
    }

    fn store_stats(&mut self) -> Result<(), SteamError> {
        Err(Self::native_steam_unavailable())
    }
}
