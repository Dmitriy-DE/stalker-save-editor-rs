//! Small dependency-free operating-system adapters used by the UI.

pub mod output;

#[cfg(target_os = "windows")]
mod win32_ffi;
#[cfg(target_os = "windows")]
pub mod window_win32;
