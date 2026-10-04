//! C ABI used by the dependency-free browser host.

use sse_web::{decode_event, WebRuntime};
use std::cell::RefCell;

const CONTINUE: u32 = 0;
const EXIT: u32 = 1;
const INVALID_EVENT: u32 = 2;
const RUNTIME_UNAVAILABLE: u32 = 3;

thread_local! {
    static RUNTIME: RefCell<Option<WebRuntime>> = const { RefCell::new(None) };
}

/// Initializes one browser shell runtime; returns zero on success.
#[no_mangle]
pub extern "C" fn sse_web_init() -> u32 {
    RUNTIME.with(|runtime| {
        let Ok(mut runtime) = runtime.try_borrow_mut() else {
            return RUNTIME_UNAVAILABLE;
        };
        match WebRuntime::new() {
            Ok(shell) => {
                *runtime = Some(shell);
                CONTINUE
            }
            Err(_) => RUNTIME_UNAVAILABLE,
        }
    })
}

/// Dispatches one browser event. Event codes: 0 resize, 1 pointer move, 2 pointer left, 3 button, 4 wheel,
/// 5 key, 6 focus, and 7 close requested. Returns zero to continue, one to exit, or an error code.
#[no_mangle]
pub extern "C" fn sse_web_event(code: u32, a: i32, b: i32, c: i32, d: i32, e: i32) -> u32 {
    let Some(event) = decode_event(code, a, b, c, d, e) else {
        return INVALID_EVENT;
    };
    RUNTIME.with(|runtime| {
        let Ok(mut runtime) = runtime.try_borrow_mut() else {
            return RUNTIME_UNAVAILABLE;
        };
        let Some(runtime) = runtime.as_mut() else {
            return RUNTIME_UNAVAILABLE;
        };
        if runtime.dispatch_browser(event) {
            EXIT
        } else {
            CONTINUE
        }
    })
}

/// Resizes, paints, and returns a pointer to the AARRGGBB frame in exported WebAssembly memory; zero means failure.
#[no_mangle]
pub extern "C" fn sse_web_render(width: u32, height: u32) -> u32 {
    RUNTIME.with(|runtime| {
        let Ok(mut runtime) = runtime.try_borrow_mut() else {
            return 0;
        };
        let Some(runtime) = runtime.as_mut() else {
            return 0;
        };
        let Ok(frame) = runtime.frame(width, height) else {
            return 0;
        };
        u32::try_from(frame.as_ptr() as usize).unwrap_or(0)
    })
}
