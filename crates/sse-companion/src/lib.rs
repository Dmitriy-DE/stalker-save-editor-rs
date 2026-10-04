//! Companion installation, file protocol, exact-source hooks, and hotkey models.
#![forbid(unsafe_code)]

/// Embedded, byte-compatible X-Ray and S.T.A.L.K.E.R. 2 companion payloads.
pub mod bundled;
/// Exact-source Lua hook patching.
pub mod hook;
/// Hotkey layout parsing and game-window matching.
pub mod hotkeys;
/// Explicit companion install and removal transactions.
pub mod installer;
/// File-based request/reply protocol used by the in-game Lua mod.
pub mod protocol;

/// Maximum accepted mod or protocol text file size.
pub const MAX_COMPANION_FILE_BYTES: usize = 1024 * 1024;
