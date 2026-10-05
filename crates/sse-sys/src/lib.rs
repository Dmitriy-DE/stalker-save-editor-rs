//! Small dependency-free operating-system adapters used by the UI.

pub mod fetch;
pub mod output;
pub mod secure_fs;

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub mod shm;
#[cfg(target_os = "linux")]
pub mod memfd;

#[cfg(target_os = "windows")]
mod win32_ffi;
#[cfg(target_os = "windows")]
pub mod window_win32;

#[cfg(target_os = "macos")]
mod macos_objc;
#[cfg(target_os = "macos")]
pub mod window_macos;
