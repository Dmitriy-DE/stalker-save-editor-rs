//! Small dependency-free operating-system adapters used by the UI.

pub mod fetch;
pub mod file_dialog;
pub mod hotkeys;
pub mod output;
pub mod process;
pub mod secure_fs;
pub mod steam;
pub mod system;
pub mod web_abi;

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub mod x11_ime;

#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub mod memfd;
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub mod shm;
#[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
pub mod unix_fd;

#[cfg(target_os = "windows")]
mod win32_ffi;
#[cfg(target_os = "windows")]
pub mod window_win32;

#[cfg(target_os = "macos")]
mod macos_objc;
#[cfg(target_os = "macos")]
pub mod window_macos;
