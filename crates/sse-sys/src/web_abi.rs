//! Stable browser exports forwarding to callbacks registered by the browser host.

use std::cell::Cell;

/// Browser runtime callbacks supplied by `sse-web` without a crate dependency from this module.
#[derive(Clone, Copy)]
pub struct WebCallbacks {
    /// Initializes the browser runtime.
    pub init: extern "C" fn() -> u32,
    /// Sends a scalar input event to the browser runtime.
    pub event: extern "C" fn(u32, i32, i32, i32, i32, i32) -> u32,
    /// Renders a frame and returns its linear-memory address.
    pub render: extern "C" fn(u32, u32) -> u32,
}

const RUNTIME_UNAVAILABLE: u32 = 3;

thread_local! {
    static CALLBACKS: Cell<Option<WebCallbacks>> = const { Cell::new(None) };
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
