//! Files of an installed game: archives, LTX configs, XML string tables, DDS textures, and file tree.
//!
//! Owner: Gemini (G1).

pub mod archive;
pub mod dds;
pub mod encoding;
pub mod file_tree;
pub mod ltx;
pub mod string_tables;

pub use archive::{EntryDecoder, HeaderDecoder, ReadAt, XRayArchive, XRayArchiveEntry};
pub use dds::{DdsImage, RgbaImage};
pub use encoding::{decode_archive_name, decode_text, decode_windows_1250, decode_windows_1251};
pub use file_tree::{CompanionArchiveLocator, CompanionGame, GameFile, GameFileTree};
pub use ltx::{LtxDocument, LtxSection};
pub use string_tables::XRayStringTables;
