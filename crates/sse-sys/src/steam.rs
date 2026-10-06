//! Safe dynamic boundary for the Steamworks C ABI.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::Path;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use sse_core::{Error, Result};

const MAXIMUM_REMOTE_FILES: i32 = 100_000;
const MAXIMUM_FILE_BYTES: usize = 64 * 1024 * 1024;
const MAXIMUM_ACHIEVEMENTS: u32 = 100_000;

type InitClassicFn = unsafe extern "C" fn() -> u8;
type InitFlatFn = unsafe extern "C" fn(*mut c_char) -> i32;
type ShutdownFn = unsafe extern "C" fn();
type RunCallbacksFn = unsafe extern "C" fn();
type AccessorFn = unsafe extern "C" fn() -> *mut c_void;

/// A remote file reported by `ISteamRemoteStorage`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteStorageFile {
    /// Exact remote name returned by Steam.
    pub name: String,
    /// Size reported by Steam, in bytes.
    pub size: i32,
    /// Unix timestamp reported by Steam.
    pub timestamp: i64,
    /// Whether Steam has persisted the file to cloud storage.
    pub persisted: bool,
    /// Whether the file currently exists according to Steam.
    pub exists: bool,
}

/// A loaded Steamworks library. The library handle stays alive through every borrowed session.
pub struct SteamLibrary {
    handle: LibraryHandle,
    init_classic: Option<InitClassicFn>,
    init_flat: Option<InitFlatFn>,
    shutdown: ShutdownFn,
    run_callbacks: RunCallbacksFn,
}

impl SteamLibrary {
    /// Loads a Steamworks library from an absolute path.
    ///
    /// On Windows the full path is passed to `LoadLibraryExW` with
    /// `LOAD_WITH_ALTERED_SEARCH_PATH`; Unix uses `dlopen(RTLD_NOW | RTLD_LOCAL)`.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::Refused("Steam library path must be absolute".to_owned()));
        }
        let handle = LibraryHandle::load(path)?;
        let init_classic = handle.symbol(c"SteamAPI_Init").map(|symbol| {
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: this exact exported symbol has the C ABI `bool SteamAPI_Init(void)`;
            // Steam's C++ bool is one byte, represented here as u8.
            unsafe { std::mem::transmute::<*mut c_void, InitClassicFn>(symbol.as_ptr()) }
        });
        let init_flat = handle.symbol(c"SteamAPI_InitFlat").map(|symbol| {
            // SAFETY: ACCEPTANCE.md Part IV §3 keeps the exact InitFlat ABI call within sse-sys.
            // The SteamErrMsg output pointer is a writable buffer supplied by the call below.
            unsafe { std::mem::transmute::<*mut c_void, InitFlatFn>(symbol.as_ptr()) }
        });
        if choose_init_entrypoint(init_classic.is_some(), init_flat.is_some()).is_none() {
            return Err(Error::System(
                "SteamAPI_Init and SteamAPI_InitFlat exports were not found".to_owned(),
            ));
        }
        let shutdown = required_function::<ShutdownFn>(&handle, c"SteamAPI_Shutdown")?;
        let run_callbacks = required_function::<RunCallbacksFn>(&handle, c"SteamAPI_RunCallbacks")?;
        Ok(Self {
            handle,
            init_classic,
            init_flat,
            shutdown,
            run_callbacks,
        })
    }

    /// Initializes Steam and returns a session that calls `SteamAPI_Shutdown` on drop.
    pub fn init(&self) -> Result<SteamSession<'_>> {
        let initialized = match choose_init_entrypoint(self.init_classic.is_some(), self.init_flat.is_some()) {
            Some(InitEntrypoint::Classic) => {
                let initialize = self
                    .init_classic
                    .ok_or_else(|| Error::System("SteamAPI_Init export disappeared".to_owned()))?;
                // SAFETY: ACCEPTANCE.md Part IV §3 invariants: the function pointer was resolved from this live library handle and has the
                // exact `SteamAPI_Init` ABI. The worker's parent supplies SteamAppId and SteamGameId.
                unsafe { initialize() != 0 }
            }
            Some(InitEntrypoint::Flat) => {
                let initialize = self
                    .init_flat
                    .ok_or_else(|| Error::System("SteamAPI_InitFlat export disappeared".to_owned()))?;
                // SAFETY: ACCEPTANCE.md Part IV §2 specifies `SteamAPI_InitFlat(SteamErrMsg*) -> int` with zero as success. The function
                // pointer was resolved from this live library handle, and the call receives a live 1024-byte error buffer. The installed
                // SDK ABI is not runtime-verified in this environment.
                let mut error_message = [0_u8; 1024];
                unsafe { initialize(error_message.as_mut_ptr().cast()) == 0 }
            }
            None => false,
        };
        if !initialized {
            return Err(Error::System(
                "SteamAPI_Init failed. The Steam client may not be running.".to_owned(),
            ));
        }
        Ok(SteamSession { library: self })
    }
}

/// An initialized Steamworks session. Dropping it shuts Steam down once.
pub struct SteamSession<'library> {
    library: &'library SteamLibrary,
}

impl SteamSession<'_> {
    /// Pumps callbacks on the current worker thread.
    pub fn run_callbacks(&self) {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: the session guarantees successful initialization and keeps the library loaded.
        unsafe { (self.library.run_callbacks)() };
    }

    /// Opens the newest available RemoteStorage interface accessor, trying v020 through v014.
    pub fn remote_storage(&self) -> Result<RemoteStorage<'_, '_>> {
        let mut available = Vec::new();
        for version in (14_u8..=20_u8).rev() {
            let name = format!("SteamAPI_SteamRemoteStorage_v{version:03}");
            let symbol_name = CString::new(name).map_err(|_| Error::Refused("invalid Steam symbol name".to_owned()))?;
            if self.library.handle.symbol(&symbol_name).is_some() {
                available.push(version);
            }
        }
        let version = first_remote_storage_version(&available)
            .ok_or_else(|| Error::System("Steam ISteamRemoteStorage accessor was not found.".to_owned()))?;
        let name = format!("SteamAPI_SteamRemoteStorage_v{version:03}");
        let symbol_name = CString::new(name).map_err(|_| Error::Refused("invalid Steam symbol name".to_owned()))?;
        let symbol = self
            .library
            .handle
            .symbol(&symbol_name)
            .ok_or_else(|| Error::System("Steam ISteamRemoteStorage accessor was not found.".to_owned()))?;
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: this versioned export is a zero-argument accessor returning its opaque interface pointer.
        let accessor: AccessorFn = unsafe { std::mem::transmute(symbol.as_ptr()) };
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: Steam has been initialized in this session and the accessor remains in its loaded library.
        let interface = unsafe { accessor() };
        let interface = NonNull::new(interface)
            .ok_or_else(|| Error::System("Steam ISteamRemoteStorage interface is unavailable.".to_owned()))?;
        let api = load_remote_storage_api(&self.library.handle)?;
        Ok(RemoteStorage {
            session: self,
            interface,
            api,
        })
    }

    /// Opens the newest available UserStats interface accessor, trying v013 through v011.
    pub fn user_stats(&self) -> Result<UserStats<'_, '_>> {
        let mut found = None;
        for version in (11_u8..=13_u8).rev() {
            let name = format!("SteamAPI_SteamUserStats_v{version:03}");
            let symbol_name = CString::new(name).map_err(|_| Error::Refused("invalid Steam symbol name".to_owned()))?;
            if let Some(symbol) = self.library.handle.symbol(&symbol_name) {
                // SAFETY: ACCEPTANCE.md Part IV §3 invariants: this versioned export is a zero-argument accessor returning its opaque interface pointer.
                let accessor: AccessorFn = unsafe { std::mem::transmute(symbol.as_ptr()) };
                // SAFETY: ACCEPTANCE.md Part IV §3 invariants: Steam has been initialized in this session and the accessor remains in its loaded library.
                let interface = unsafe { accessor() };
                let interface = NonNull::new(interface)
                    .ok_or_else(|| Error::System("Steam ISteamUserStats interface is unavailable.".to_owned()))?;
                found = Some((version, interface));
                break;
            }
        }
        let (version, interface) =
            found.ok_or_else(|| Error::System("Steam ISteamUserStats v013 accessor was not found.".to_owned()))?;
        let api = load_user_stats_api(&self.library.handle)?;
        let _ = version;
        Ok(UserStats {
            session: self,
            interface,
            api,
        })
    }
}

impl Drop for SteamSession<'_> {
    fn drop(&mut self) {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: this guard is created only after one successful init and is dropped once before the library borrow ends.
        unsafe { (self.library.shutdown)() };
    }
}

type GetFileCountFn = unsafe extern "C" fn(*mut c_void) -> i32;
type GetFileNameAndSizeFn = unsafe extern "C" fn(*mut c_void, i32, *mut i32) -> *const c_char;
type GetFileTimestampFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> i64;
type FilePredicateFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> u8;
type GetFileSizeFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> i32;
type FileReadFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void, i32) -> i32;
type FileWriteFn = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_void, i32) -> u8;

#[derive(Clone, Copy)]
struct RemoteStorageApi {
    get_file_count: GetFileCountFn,
    get_file_name_and_size: GetFileNameAndSizeFn,
    get_file_timestamp: GetFileTimestampFn,
    file_exists: FilePredicateFn,
    file_persisted: FilePredicateFn,
    get_file_size: GetFileSizeFn,
    file_read: FileReadFn,
    file_write: FileWriteFn,
}

/// Borrowed RemoteStorage interface valid only while its session is alive.
pub struct RemoteStorage<'session, 'library> {
    session: &'session SteamSession<'library>,
    interface: NonNull<c_void>,
    api: RemoteStorageApi,
}

impl RemoteStorage<'_, '_> {
    /// Lists files newest first, skipping entries with invalid names or negative reported sizes.
    pub fn list_files(&self) -> Result<Vec<RemoteStorageFile>> {
        self.session.run_callbacks();
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: the interface came from a non-null versioned accessor after init and is tied to the session.
        let count = unsafe { (self.api.get_file_count)(self.interface.as_ptr()) };
        if !(0..=MAXIMUM_REMOTE_FILES).contains(&count) {
            return Err(Error::System(
                "Steam RemoteStorage returned an invalid file count.".to_owned(),
            ));
        }
        let count = usize::try_from(count)
            .map_err(|_| Error::System("Steam RemoteStorage file count is not representable".to_owned()))?;
        let mut files = Vec::new();
        files
            .try_reserve_exact(count)
            .map_err(|error| Error::System(format!("Steam RemoteStorage list allocation failed: {error}")))?;
        for index in 0..count {
            let index = i32::try_from(index)
                .map_err(|_| Error::System("Steam RemoteStorage file index is not representable".to_owned()))?;
            let mut reported_size = 0_i32;
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: index is within the validated file count and reported_size is a valid output pointer.
            let name_pointer =
                unsafe { (self.api.get_file_name_and_size)(self.interface.as_ptr(), index, &mut reported_size) };
            if name_pointer.is_null() || reported_size < 0 {
                continue;
            }
            let name = copy_steam_string(name_pointer);
            if name.is_empty() {
                continue;
            }
            let remote_name = CString::new(name.as_bytes())
                .map_err(|_| Error::System("Steam returned a RemoteStorage name containing NUL".to_owned()))?;
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: remote_name is NUL-terminated and remains alive for each synchronous Steam call.
            let timestamp = unsafe { (self.api.get_file_timestamp)(self.interface.as_ptr(), remote_name.as_ptr()) };
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: same checked session/interface and live CString invariant as above.
            let persisted = unsafe { (self.api.file_persisted)(self.interface.as_ptr(), remote_name.as_ptr()) != 0 };
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: same checked session/interface and live CString invariant as above.
            let exists = unsafe { (self.api.file_exists)(self.interface.as_ptr(), remote_name.as_ptr()) != 0 };
            files.push(RemoteStorageFile {
                name,
                size: reported_size,
                timestamp,
                persisted,
                exists,
            });
        }
        files.sort_by(|left, right| {
            right
                .timestamp
                .cmp(&left.timestamp)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(files)
    }

    /// Reads a RemoteStorage file into one bounded output buffer.
    pub fn read_file(&self, name: &str) -> Result<Vec<u8>> {
        let name = steam_name(name)?;
        self.session.run_callbacks();
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface is valid for this session and name is a live NUL-terminated UTF-8 string.
        if unsafe { (self.api.file_exists)(self.interface.as_ptr(), name.as_ptr()) == 0 } {
            return Err(Error::System("Steam RemoteStorage file does not exist.".to_owned()));
        }
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: same valid interface and string lifetime as the preceding call.
        let reported_size = unsafe { (self.api.get_file_size)(self.interface.as_ptr(), name.as_ptr()) };
        let size = usize::try_from(reported_size)
            .map_err(|_| Error::System("Steam RemoteStorage returned an invalid or oversized file.".to_owned()))?;
        if size > MAXIMUM_FILE_BYTES {
            return Err(Error::System(
                "Steam RemoteStorage returned an invalid or oversized file.".to_owned(),
            ));
        }
        let size_i32 = i32::try_from(size)
            .map_err(|_| Error::System("Steam RemoteStorage file size is not representable".to_owned()))?;
        let mut data = Vec::new();
        data.try_reserve_exact(size)
            .map_err(|error| Error::System(format!("Steam RemoteStorage read allocation failed: {error}")))?;
        data.resize(size, 0);
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: data has exactly size writable bytes, size_i32 is checked, and the interface/name live through call.
        let read = unsafe {
            (self.api.file_read)(
                self.interface.as_ptr(),
                name.as_ptr(),
                data.as_mut_ptr().cast(),
                size_i32,
            )
        };
        if read != size_i32 {
            return Err(Error::System(format!(
                "Steam RemoteStorage returned {read} of {size_i32} bytes."
            )));
        }
        Ok(data)
    }

    /// Reports whether Steam has persisted this remote file.
    pub fn file_persisted(&self, name: &str) -> Result<bool> {
        let name = steam_name(name)?;
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface is valid for this session and name is a live NUL-terminated UTF-8 string.
        Ok(unsafe { (self.api.file_persisted)(self.interface.as_ptr(), name.as_ptr()) != 0 })
    }

    /// Sends one bounded file write. `Ok(false)` means Steam explicitly rejected it.
    pub fn write_file(&self, name: &str, data: &[u8]) -> Result<bool> {
        let name = steam_name(name)?;
        if data.is_empty() || data.len() > MAXIMUM_FILE_BYTES {
            return Err(Error::Refused(
                "Steam RemoteStorage write size is outside the supported range".to_owned(),
            ));
        }
        let size = i32::try_from(data.len())
            .map_err(|_| Error::Refused("Steam RemoteStorage write size is not representable".to_owned()))?;
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface is valid for this session; name and data remain valid for the synchronous call.
        Ok(unsafe { (self.api.file_write)(self.interface.as_ptr(), name.as_ptr(), data.as_ptr().cast(), size) != 0 })
    }
}

type GetNumAchievementsFn = unsafe extern "C" fn(*mut c_void) -> u32;
type GetAchievementNameFn = unsafe extern "C" fn(*mut c_void, u32) -> *const c_char;
type GetAchievementAttributeFn = unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char) -> *const c_char;
type GetAchievementStateFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut u8, *mut u32) -> u8;
type SetAchievementFn = unsafe extern "C" fn(*mut c_void, *const c_char) -> u8;
type StoreStatsFn = unsafe extern "C" fn(*mut c_void) -> u8;

#[derive(Clone, Copy)]
struct UserStatsApi {
    get_num_achievements: GetNumAchievementsFn,
    get_achievement_name: GetAchievementNameFn,
    get_achievement_attribute: GetAchievementAttributeFn,
    get_achievement_state: GetAchievementStateFn,
    set_achievement: SetAchievementFn,
    clear_achievement: SetAchievementFn,
    store_stats: StoreStatsFn,
}

/// Achievement returned by the native Steam UserStats interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAchievement {
    /// Stable API identifier.
    pub name: String,
    /// Display name returned by Steam.
    pub display_name: String,
    /// Description returned by Steam.
    pub description: String,
    /// Whether Steam marks this achievement hidden.
    pub hidden: bool,
    /// Whether this achievement is unlocked.
    pub achieved: bool,
    /// Unix unlock time, or zero when locked.
    pub unlock_time: u32,
}

/// Borrowed UserStats interface valid only while its session is alive.
pub struct UserStats<'session, 'library> {
    session: &'session SteamSession<'library>,
    interface: NonNull<c_void>,
    api: UserStatsApi,
}

impl UserStats<'_, '_> {
    /// Lists achievements, waiting up to ten seconds for Steam to provide its current stats.
    pub fn achievements(&self) -> Result<Vec<SteamAchievement>> {
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(10))
            .ok_or_else(|| Error::System("Steam achievement deadline overflowed".to_owned()))?;
        let mut count = self.count();
        while count == 0 && Instant::now() < deadline {
            self.session.run_callbacks();
            std::thread::sleep(Duration::from_millis(200));
            count = self.count();
        }
        if count == 0 {
            return Err(Error::System(
                "Steam returned no achievements for this game or did not load its statistics.".to_owned(),
            ));
        }
        if count > MAXIMUM_ACHIEVEMENTS {
            return Err(Error::System("Steam returned an invalid achievement count.".to_owned()));
        }
        let capacity = usize::try_from(count)
            .map_err(|_| Error::System("Steam achievement count is not representable".to_owned()))?;
        let mut results = Vec::new();
        results
            .try_reserve_exact(capacity)
            .map_err(|error| Error::System(format!("Steam achievement list allocation failed: {error}")))?;
        for index in 0..count {
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: index is less than the validated achievement count and the interface lives with the session.
            let name_pointer = unsafe { (self.api.get_achievement_name)(self.interface.as_ptr(), index) };
            if name_pointer.is_null() {
                continue;
            }
            let name = copy_steam_string(name_pointer);
            if name.is_empty() {
                continue;
            }
            let api_name = steam_name(&name)?;
            let mut achieved = 0_u8;
            let mut unlock_time = 0_u32;
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: api_name is NUL-terminated and both output pointers are valid for this synchronous call.
            let returned = unsafe {
                (self.api.get_achievement_state)(
                    self.interface.as_ptr(),
                    api_name.as_ptr(),
                    &mut achieved,
                    &mut unlock_time,
                )
            };
            if returned == 0 {
                return Err(Error::System(format!(
                    "Steam did not return the state for achievement {name}."
                )));
            }
            let display_name = self.attribute(&api_name, c"name");
            let description = self.attribute(&api_name, c"desc");
            let hidden = self.attribute(&api_name, c"hidden") == "1";
            results.push(SteamAchievement {
                name,
                display_name,
                description,
                hidden,
                achieved: achieved != 0,
                unlock_time,
            });
        }
        Ok(results)
    }

    /// Requests a set or clear operation; the return value is Steam's explicit acceptance result.
    pub fn set_achievement(&self, name: &str, achieved: bool) -> Result<bool> {
        let name = steam_name(name)?;
        let function = if achieved {
            self.api.set_achievement
        } else {
            self.api.clear_achievement
        };
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface is valid for this session and name remains NUL-terminated through the call.
        Ok(unsafe { function(self.interface.as_ptr(), name.as_ptr()) != 0 })
    }

    /// Persists the pending achievement changes through Steam.
    pub fn store_stats(&self) -> bool {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface is valid for this session and the function takes no caller-owned pointers.
        unsafe { (self.api.store_stats)(self.interface.as_ptr()) != 0 }
    }

    fn count(&self) -> u32 {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: interface came from a non-null versioned accessor after init and is tied to the session.
        unsafe { (self.api.get_num_achievements)(self.interface.as_ptr()) }
    }

    fn attribute(&self, name: &CStr, key: &CStr) -> String {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: both strings are static or session-owned NUL-terminated values and are live for the call.
        let pointer =
            unsafe { (self.api.get_achievement_attribute)(self.interface.as_ptr(), name.as_ptr(), key.as_ptr()) };
        copy_steam_string(pointer)
    }
}

fn load_remote_storage_api(handle: &LibraryHandle) -> Result<RemoteStorageApi> {
    Ok(RemoteStorageApi {
        get_file_count: required_function(handle, c"SteamAPI_ISteamRemoteStorage_GetFileCount")?,
        get_file_name_and_size: required_function(handle, c"SteamAPI_ISteamRemoteStorage_GetFileNameAndSize")?,
        get_file_timestamp: required_function(handle, c"SteamAPI_ISteamRemoteStorage_GetFileTimestamp")?,
        file_exists: required_function(handle, c"SteamAPI_ISteamRemoteStorage_FileExists")?,
        file_persisted: required_function(handle, c"SteamAPI_ISteamRemoteStorage_FilePersisted")?,
        get_file_size: required_function(handle, c"SteamAPI_ISteamRemoteStorage_GetFileSize")?,
        file_read: required_function(handle, c"SteamAPI_ISteamRemoteStorage_FileRead")?,
        file_write: required_function(handle, c"SteamAPI_ISteamRemoteStorage_FileWrite")?,
    })
}

fn load_user_stats_api(handle: &LibraryHandle) -> Result<UserStatsApi> {
    Ok(UserStatsApi {
        get_num_achievements: required_function(handle, c"SteamAPI_ISteamUserStats_GetNumAchievements")?,
        get_achievement_name: required_function(handle, c"SteamAPI_ISteamUserStats_GetAchievementName")?,
        get_achievement_attribute: required_function(
            handle,
            c"SteamAPI_ISteamUserStats_GetAchievementDisplayAttribute",
        )?,
        get_achievement_state: required_function(handle, c"SteamAPI_ISteamUserStats_GetAchievementAndUnlockTime")?,
        set_achievement: required_function(handle, c"SteamAPI_ISteamUserStats_SetAchievement")?,
        clear_achievement: required_function(handle, c"SteamAPI_ISteamUserStats_ClearAchievement")?,
        store_stats: required_function(handle, c"SteamAPI_ISteamUserStats_StoreStats")?,
    })
}

fn required_function<T: Copy>(handle: &LibraryHandle, name: &CStr) -> Result<T> {
    let symbol = handle
        .symbol(name)
        .ok_or_else(|| Error::System(format!("Steam export {} was not found.", name.to_string_lossy())))?;
    if std::mem::size_of::<T>() != std::mem::size_of::<*mut c_void>() {
        return Err(Error::System("Steam function pointer size is not supported".to_owned()));
    }
    // SAFETY: ACCEPTANCE.md Part IV §3 invariants: all callers supply the exact C ABI function-pointer type documented for this symbol;
    // the pointer is non-null and belongs to the retained library handle.
    Ok(unsafe { std::mem::transmute_copy(&symbol.as_ptr()) })
}

fn steam_name(name: &str) -> Result<CString> {
    CString::new(name).map_err(|_| Error::Refused("Steam name contains NUL".to_owned()))
}

fn copy_steam_string(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    // SAFETY: ACCEPTANCE.md Part IV §3 invariants: Steam returns null or a NUL-terminated string owned by the library; this value is copied
    // immediately before another Steam call can invalidate its temporary storage.
    unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InitEntrypoint {
    Classic,
    Flat,
}

fn choose_init_entrypoint(classic: bool, flat: bool) -> Option<InitEntrypoint> {
    if classic {
        Some(InitEntrypoint::Classic)
    } else if flat {
        Some(InitEntrypoint::Flat)
    } else {
        None
    }
}

fn first_remote_storage_version(available: &[u8]) -> Option<u8> {
    (14_u8..=20_u8).rev().find(|version| available.contains(version))
}

struct LibraryHandle {
    handle: NonNull<c_void>,
}

impl LibraryHandle {
    fn load(path: &Path) -> Result<Self> {
        platform::load(path).map(|handle| Self { handle })
    }

    fn symbol(&self, name: &CStr) -> Option<NonNull<c_void>> {
        platform::symbol(self.handle, name)
    }
}

impl Drop for LibraryHandle {
    fn drop(&mut self) {
        platform::close(self.handle);
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use super::{c_char, c_void, CStr, Error, NonNull, Path, Result};
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    #[cfg(target_os = "linux")]
    const RTLD_LOCAL: i32 = 0;
    #[cfg(target_os = "macos")]
    const RTLD_LOCAL: i32 = 4;
    const RTLD_NOW: i32 = 2;

    #[cfg(target_os = "linux")]
    #[link(name = "dl")]
    // SAFETY: ACCEPTANCE.md Part IV §3 keeps the platform ABI declarations in sse-sys.
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
        fn dlerror() -> *const c_char;
    }

    #[cfg(target_os = "macos")]
    // SAFETY: ACCEPTANCE.md Part IV §3 keeps the platform ABI declarations in sse-sys.
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flags: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlclose(handle: *mut c_void) -> i32;
        fn dlerror() -> *const c_char;
    }

    pub(super) fn load(path: &Path) -> Result<NonNull<c_void>> {
        let bytes = path.as_os_str().as_bytes();
        let path = CString::new(bytes).map_err(|_| Error::Refused("Steam library path contains NUL".to_owned()))?;
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: path is absolute and NUL-terminated; RTLD_NOW resolves dependencies before returning
        // and RTLD_LOCAL keeps Steam symbols out of the process-wide namespace.
        let handle = unsafe { dlopen(path.as_ptr(), RTLD_NOW | RTLD_LOCAL) };
        NonNull::new(handle).ok_or_else(|| Error::System(format!("Could not load Steam library: {}", last_error())))
    }

    pub(super) fn symbol(handle: NonNull<c_void>, name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: handle is a live dlopen handle and name is a NUL-terminated symbol name.
        let pointer = unsafe { dlsym(handle.as_ptr(), name.as_ptr()) };
        NonNull::new(pointer)
    }

    pub(super) fn close(handle: NonNull<c_void>) {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: handle was returned by dlopen and is closed exactly once by LibraryHandle::drop.
        let _ = unsafe { dlclose(handle.as_ptr()) };
    }

    fn last_error() -> String {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: dlerror returns a thread-local NUL-terminated diagnostic or null.
        let pointer = unsafe { dlerror() };
        if pointer.is_null() {
            "dynamic loader returned no diagnostic".to_owned()
        } else {
            // SAFETY: ACCEPTANCE.md Part IV §3 invariants: non-null dlerror output is NUL-terminated and is copied before another loader call.
            unsafe { CStr::from_ptr(pointer) }.to_string_lossy().into_owned()
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{c_void, CStr, Error, NonNull, Path, Result};
    use std::os::windows::ffi::OsStrExt;

    const LOAD_WITH_ALTERED_SEARCH_PATH: u32 = 0x0000_0008;

    #[link(name = "kernel32")]
    // SAFETY: ACCEPTANCE.md Part IV §3 keeps the platform ABI declarations in sse-sys.
    unsafe extern "system" {
        fn LoadLibraryExW(filename: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> Option<unsafe extern "system" fn()>;
        fn FreeLibrary(module: *mut c_void) -> i32;
    }

    pub(super) fn load(path: &Path) -> Result<NonNull<c_void>> {
        let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
        if wide.contains(&0) {
            return Err(Error::Refused("Steam library path contains NUL".to_owned()));
        }
        wide.push(0);
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: path was checked absolute, wide is terminated UTF-16, null file handle is required,
        // and the altered-search-path flag is used with the full DLL path.
        let handle = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
        NonNull::new(handle).ok_or_else(|| {
            Error::System(format!(
                "Could not load Steam library: {}",
                std::io::Error::last_os_error()
            ))
        })
    }

    pub(super) fn symbol(handle: NonNull<c_void>, name: &CStr) -> Option<NonNull<c_void>> {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: handle is a live LoadLibraryExW module and name is a NUL-terminated ASCII export name.
        let function = unsafe { GetProcAddress(handle.as_ptr(), name.to_bytes_with_nul().as_ptr()) }?;
        NonNull::new(function as *const () as *mut c_void)
    }

    pub(super) fn close(handle: NonNull<c_void>) {
        // SAFETY: ACCEPTANCE.md Part IV §3 invariants: handle was returned by LoadLibraryExW and is freed once by LibraryHandle::drop.
        let _ = unsafe { FreeLibrary(handle.as_ptr()) };
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod platform {
    use super::{c_void, Error, NonNull, Path, Result};

    pub(super) fn load(_path: &Path) -> Result<NonNull<c_void>> {
        Err(Error::Refused(
            "Steam library loading is not supported on this platform".to_owned(),
        ))
    }

    pub(super) fn symbol(_handle: NonNull<c_void>, _name: &std::ffi::CStr) -> Option<NonNull<c_void>> {
        None
    }

    pub(super) fn close(_handle: NonNull<c_void>) {}
}

#[cfg(test)]
mod tests {
    use super::{choose_init_entrypoint, first_remote_storage_version, InitEntrypoint, SteamLibrary};
    use std::path::Path;

    #[test]
    fn init_uses_classic_symbol_before_flat_fallback() {
        assert_eq!(choose_init_entrypoint(true, true), Some(InitEntrypoint::Classic));
        assert_eq!(choose_init_entrypoint(false, true), Some(InitEntrypoint::Flat));
        assert_eq!(choose_init_entrypoint(false, false), None);
    }

    #[test]
    fn remote_storage_accessor_selection_prefers_newest_available_version() {
        assert_eq!(first_remote_storage_version(&[14, 16, 20]), Some(20));
        assert_eq!(first_remote_storage_version(&[14, 16]), Some(16));
        assert_eq!(first_remote_storage_version(&[]), None);
    }

    #[test]
    fn dynamic_library_rejects_relative_paths_before_loading() {
        let result = SteamLibrary::load(Path::new("steam_api"));
        assert!(matches!(result, Err(sse_core::Error::Refused(_))));
    }
}
