//! Contract tests for the callback table used by the browser's system ABI.

use sse_sys::web_abi::{self, WebCallbacks};
use std::sync::atomic::{AtomicBool, Ordering};

static CLEAR_DOWNLOAD_CALLED: AtomicBool = AtomicBool::new(false);

extern "C" fn test_init() -> u32 {
    17
}

extern "C" fn test_event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> u32 {
    [a, b, c, d, e].into_iter().fold(code, |sum, value| {
        sum.wrapping_add(u32::try_from(value).unwrap_or_default())
    })
}

extern "C" fn test_render(width: u32, height: u32) -> u32 {
    width ^ height
}

fn test_file_selected(name: String, bytes: Vec<u8>, last_modified_ms: u64) -> u32 {
    u32::from(name == "quicksave.sav" && bytes == [3, 1, 4] && last_modified_ms == 21)
}

fn test_file_cancelled() -> u32 {
    31
}

fn test_file_rejected(code: u32) -> u32 {
    code.wrapping_add(41)
}

fn test_take_open_request() -> u32 {
    1
}

fn test_download_pending() -> u32 {
    1
}

fn test_download_file_name_ptr() -> u32 {
    0x1000
}

fn test_download_file_name_len() -> u32 {
    12
}

fn test_download_bytes_ptr() -> u32 {
    0x2000
}

fn test_download_bytes_len() -> u32 {
    64
}

fn test_clear_download() {
    CLEAR_DOWNLOAD_CALLED.store(true, Ordering::SeqCst);
}

#[test]
fn unregistered_browser_runtime_returns_safe_status_values() {
    assert_eq!(web_abi::init(), 3);
    assert_eq!(web_abi::event(0, 0, 0, 0, 0, 0), 3);
    assert_eq!(web_abi::render(1, 1), 0);
}

#[test]
fn registered_browser_callbacks_receive_arguments_and_return_values() {
    web_abi::register_callbacks(WebCallbacks {
        init: test_init,
        event: test_event,
        render: test_render,
        file_selected: test_file_selected,
        file_cancelled: test_file_cancelled,
        file_rejected: test_file_rejected,
        take_open_request: test_take_open_request,
        download_pending: test_download_pending,
        download_file_name_ptr: test_download_file_name_ptr,
        download_file_name_len: test_download_file_name_len,
        download_bytes_ptr: test_download_bytes_ptr,
        download_bytes_len: test_download_bytes_len,
        clear_download: test_clear_download,
    });

    assert_eq!(web_abi::init(), 17);
    assert_eq!(web_abi::event(3, 5, 7, 11, 13, 17), 56);
    assert_eq!(web_abi::render(0x1234, 0x00ff), 0x12cb);
    assert_eq!(web_abi::file_selected("quicksave.sav".to_owned(), vec![3, 1, 4], 21), 1);
    assert_eq!(web_abi::file_cancelled(), 31);
    assert_eq!(web_abi::file_rejected(7), 48);
    assert_eq!(web_abi::take_open_request(), 1);
    assert_eq!(web_abi::download_pending(), 1);
    assert_eq!(web_abi::download_file_name_ptr(), 0x1000);
    assert_eq!(web_abi::download_file_name_len(), 12);
    assert_eq!(web_abi::download_bytes_ptr(), 0x2000);
    assert_eq!(web_abi::download_bytes_len(), 64);
    CLEAR_DOWNLOAD_CALLED.store(false, Ordering::SeqCst);
    web_abi::clear_download();
    assert!(CLEAR_DOWNLOAD_CALLED.load(Ordering::SeqCst));
}
