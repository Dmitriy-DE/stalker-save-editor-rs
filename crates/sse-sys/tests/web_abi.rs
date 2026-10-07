//! Contract tests for the callback table used by the browser's system ABI.

use sse_sys::web_abi::{self, WebCallbacks};

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
    });

    assert_eq!(web_abi::init(), 17);
    assert_eq!(web_abi::event(3, 5, 7, 11, 13, 17), 56);
    assert_eq!(web_abi::render(0x1234, 0x00ff), 0x12cb);
}
