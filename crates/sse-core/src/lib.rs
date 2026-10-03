//! Contracts every other crate builds on: errors, the owned save buffer, the checked byte cursor.
//!
//! Rules that hold for the whole workspace (see `AGENTS.md`):
//! * a save is read into one owned buffer and never copied "to be safe";
//! * every offset is computed with checked arithmetic and every read is bounds-checked;
//! * a reader that does not understand a field keeps its bytes, it never drops them;
//! * a heuristic that locates data must match in exactly one place or report nothing.

mod buffer;
mod cursor;
pub mod diff;
mod error;

pub use buffer::SaveBuffer;
pub use cursor::Cursor;
pub use error::{Error, ExitCode, Result};
