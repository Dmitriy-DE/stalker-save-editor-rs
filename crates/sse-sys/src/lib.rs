//! Small standard-library system adapters and FFI exports used by UI hosts.

pub mod fetch;
#[cfg(not(target_arch = "wasm32"))]
pub mod output;

/// WebAssembly exports for the safe browser runtime.
#[cfg(target_arch = "wasm32")]
pub mod web_abi;

#[cfg(target_os = "windows")]
mod win32_ffi;
#[cfg(target_os = "windows")]
pub mod window_win32;

#[cfg(target_os = "macos")]
mod macos_objc;
#[cfg(target_os = "macos")]
pub mod window_macos;
