//! Files of an installed game: archives, LTX configs, XML string tables, DDS textures, and file tree.
//! Icon atlas, item icon service, and save previews.
//!
//! Owner: Gemini (G1, G4).

pub mod archive;
pub mod atlas;
pub mod dds;
pub mod encoding;
pub mod file_tree;
pub mod icon;
pub mod ltx;
pub mod preview;
pub mod string_tables;
/// Read-only Unreal IoStore and legacy PAK containers.
pub mod unreal;

pub use archive::{EntryDecoder, HeaderDecoder, ReadAt, XRayArchive, XRayArchiveEntry};
pub use atlas::{AtlasBuilder, AtlasEntry, IconAtlas, PageFormat};
pub use dds::{DdsImage, RgbaImage};
pub use encoding::{decode_archive_name, decode_text, decode_windows_1250, decode_windows_1251};
pub use file_tree::{CompanionArchiveLocator, CompanionGame, GameFile, GameFileTree};
pub use icon::ItemIconService;
pub use ltx::{LtxDocument, LtxSection};
pub use preview::{parse_campaigns, slot_guid, PreviewCache, SavePreviewReader, Stalker2SlotMeta};
pub use string_tables::XRayStringTables;
