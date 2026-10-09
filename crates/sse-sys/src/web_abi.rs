//! Stable browser exports forwarding to callbacks registered by the browser host.

use std::cell::Cell;
#[cfg(target_arch = "wasm32")]
use std::cell::RefCell;
#[cfg(target_arch = "wasm32")]
use std::collections::HashMap;

/// Browser runtime callbacks supplied by `sse-web` without a crate dependency from this module.
#[derive(Clone, Copy)]
pub struct WebCallbacks {
    /// Initializes the browser runtime.
    pub init: extern "C" fn() -> u32,
    /// Sends a scalar input event to the browser runtime.
    pub event: extern "C" fn(u32, i32, i32, i32, i32, i32) -> u32,
    /// Renders a frame and returns its linear-memory address.
    pub render: extern "C" fn(u32, u32) -> u32,
    /// Imports a browser-selected save without writing it to a local path.
    pub file_selected: fn(String, Vec<u8>, u64) -> u32,
    /// Reports cancellation of the browser file picker.
    pub file_cancelled: fn() -> u32,
    /// Reports a file that the browser host rejected before reading its contents.
    pub file_rejected: fn(u32) -> u32,
    /// Consumes a pending file-picker request.
    pub take_open_request: fn() -> u32,
    /// Returns whether a browser download is pending.
    pub download_pending: fn() -> u32,
    /// Returns the pending download filename pointer.
    pub download_file_name_ptr: fn() -> u32,
    /// Returns the pending download filename length.
    pub download_file_name_len: fn() -> u32,
    /// Returns the pending download bytes pointer.
    pub download_bytes_ptr: fn() -> u32,
    /// Returns the pending download bytes length.
    pub download_bytes_len: fn() -> u32,
    /// Clears a pending browser download after JavaScript copies it.
    pub clear_download: fn(),
}

const RUNTIME_UNAVAILABLE: u32 = 3;
#[cfg(target_arch = "wasm32")]
const INVALID_FILE_BUFFER: u32 = 4;
#[cfg(target_arch = "wasm32")]
const MAX_WEB_FILE_BYTES: usize = 256 * 1024 * 1024;
#[cfg(target_arch = "wasm32")]
const MAX_WEB_NAME_BYTES: usize = 1024;

thread_local! {
    static CALLBACKS: Cell<Option<WebCallbacks>> = const { Cell::new(None) };
    #[cfg(target_arch = "wasm32")]
    static ALLOCATIONS: RefCell<HashMap<u32, Vec<u8>>> = RefCell::new(HashMap::new());
}

/// Registers callbacks that the exported WebAssembly functions forward to.
pub fn register_callbacks(callbacks: WebCallbacks) {
    CALLBACKS.with(|registered| registered.set(Some(callbacks)));
}

/// Forwards browser initialization to the registered host runtime.
pub fn init() -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(RUNTIME_UNAVAILABLE, |callbacks| (callbacks.init)())
    })
}

/// Forwards one browser input event to the registered host runtime.
pub fn event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(RUNTIME_UNAVAILABLE, |callbacks| (callbacks.event)(code, a, b, c, d, e))
    })
}

/// Forwards one frame request to the registered host runtime.
pub fn render(width: u32, height: u32) -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(0, |callbacks| (callbacks.render)(width, height))
    })
}

/// Forwards one browser-selected filename and byte buffer to the registered runtime.
pub fn file_selected(file_name: String, bytes: Vec<u8>, last_modified_ms: u64) -> u32 {
    CALLBACKS.with(|registered| {
        registered.get().map_or(RUNTIME_UNAVAILABLE, |callbacks| {
            (callbacks.file_selected)(file_name, bytes, last_modified_ms)
        })
    })
}

/// Forwards browser file-picker cancellation to the registered runtime.
pub fn file_cancelled() -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(RUNTIME_UNAVAILABLE, |callbacks| (callbacks.file_cancelled)())
    })
}

/// Forwards a browser file selection rejection to the registered runtime.
pub fn file_rejected(code: u32) -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(RUNTIME_UNAVAILABLE, |callbacks| (callbacks.file_rejected)(code))
    })
}

/// Consumes a pending browser file-picker request.
pub fn take_open_request() -> u32 {
    CALLBACKS.with(|registered| registered.get().map_or(0, |callbacks| (callbacks.take_open_request)()))
}

/// Returns whether the host has a browser download ready.
pub fn download_pending() -> u32 {
    CALLBACKS.with(|registered| registered.get().map_or(0, |callbacks| (callbacks.download_pending)()))
}

/// Returns the pending browser download filename pointer.
pub fn download_file_name_ptr() -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(0, |callbacks| (callbacks.download_file_name_ptr)())
    })
}

/// Returns the pending browser download filename length.
pub fn download_file_name_len() -> u32 {
    CALLBACKS.with(|registered| {
        registered
            .get()
            .map_or(0, |callbacks| (callbacks.download_file_name_len)())
    })
}

/// Returns the pending browser download bytes pointer.
pub fn download_bytes_ptr() -> u32 {
    CALLBACKS.with(|registered| registered.get().map_or(0, |callbacks| (callbacks.download_bytes_ptr)()))
}

/// Returns the pending browser download bytes length.
pub fn download_bytes_len() -> u32 {
    CALLBACKS.with(|registered| registered.get().map_or(0, |callbacks| (callbacks.download_bytes_len)()))
}

/// Clears a pending browser download after JavaScript copies it.
pub fn clear_download() {
    CALLBACKS.with(|registered| {
        if let Some(callbacks) = registered.get() {
            (callbacks.clear_download)();
        }
    });
}

#[cfg(target_arch = "wasm32")]
fn allocate_browser_buffer(length: u32) -> u32 {
    let Ok(length) = usize::try_from(length) else {
        return 0;
    };
    if length == 0 || length > MAX_WEB_FILE_BYTES.max(MAX_WEB_NAME_BYTES) {
        return 0;
    }
    let mut bytes = Vec::new();
    if bytes.try_reserve_exact(length).is_err() {
        return 0;
    }
    bytes.resize(length, 0);
    let Ok(pointer) = u32::try_from(bytes.as_mut_ptr() as usize) else {
        return 0;
    };
    if pointer == 0 {
        return 0;
    }
    ALLOCATIONS.with(|allocations| {
        allocations.borrow_mut().insert(pointer, bytes);
    });
    pointer
}

#[cfg(target_arch = "wasm32")]
fn take_browser_buffer(pointer: u32, length: u32) -> Option<Vec<u8>> {
    let expected_length = usize::try_from(length).ok()?;
    ALLOCATIONS.with(|allocations| {
        let mut allocations = allocations.borrow_mut();
        if allocations
            .get(&pointer)
            .is_some_and(|bytes| bytes.len() == expected_length)
        {
            allocations.remove(&pointer)
        } else {
            None
        }
    })
}

/// Allocates a bounded byte buffer in WebAssembly memory for the JavaScript file bridge.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_alloc(length: u32) -> u32 {
    allocate_browser_buffer(length)
}

/// Frees a WebAssembly file-bridge buffer if JavaScript could not transfer it.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_free(pointer: u32, length: u32) {
    drop(take_browser_buffer(pointer, length));
}

/// Accepts the file name and bytes allocated by [`sse_web_alloc`].
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_file_selected(
    name_pointer: u32,
    name_length: u32,
    bytes_pointer: u32,
    bytes_length: u32,
    last_modified_ms: u64,
) -> u32 {
    let Some(name) = take_browser_buffer(name_pointer, name_length) else {
        drop(take_browser_buffer(bytes_pointer, bytes_length));
        return INVALID_FILE_BUFFER;
    };
    let Some(bytes) = take_browser_buffer(bytes_pointer, bytes_length) else {
        return INVALID_FILE_BUFFER;
    };
    let Ok(name) = String::from_utf8(name) else {
        return file_rejected(4);
    };
    if name_length == 0 || usize::try_from(name_length).unwrap_or(usize::MAX) > MAX_WEB_NAME_BYTES {
        return file_rejected(4);
    }
    let byte_count = usize::try_from(bytes_length).unwrap_or(usize::MAX);
    if byte_count == 0 {
        return file_rejected(1);
    }
    if byte_count > MAX_WEB_FILE_BYTES {
        return file_rejected(2);
    }
    file_selected(name, bytes, last_modified_ms)
}

/// Reports that the browser file picker was cancelled.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_file_cancelled() -> u32 {
    file_cancelled()
}

/// Reports a browser-side file rejection code.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_file_rejected(code: u32) -> u32 {
    file_rejected(code)
}

/// Stable browser file-picker request export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_take_open_request() -> u32 {
    take_open_request()
}

/// Stable browser pending-download query export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_download_pending() -> u32 {
    download_pending()
}

/// Stable browser pending-download filename pointer export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_download_file_name_ptr() -> u32 {
    download_file_name_ptr()
}

/// Stable browser pending-download filename length export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_download_file_name_len() -> u32 {
    download_file_name_len()
}

/// Stable browser pending-download bytes pointer export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_download_bytes_ptr() -> u32 {
    download_bytes_ptr()
}

/// Stable browser pending-download bytes length export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_download_bytes_len() -> u32 {
    download_bytes_len()
}

/// Stable browser pending-download clear export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_clear_download() {
    clear_download();
}

/// Stable browser initialization export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_init() -> u32 {
    init()
}

/// Stable browser input export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> u32 {
    event(code, a, b, c, d, e)
}

/// Stable browser rendering export.
#[cfg(target_arch = "wasm32")]
#[unsafe(no_mangle)]
pub extern "C" fn sse_web_render(width: u32, height: u32) -> u32 {
    render(width, height)
}
