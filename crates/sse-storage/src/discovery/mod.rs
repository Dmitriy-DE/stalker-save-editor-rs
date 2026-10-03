//! Save directory locator, slot discovery, format identification, and library index.

pub mod index;
pub mod locator;
pub mod slot;

pub use index::{LibraryIndex, LibraryIndexEntry};
pub use locator::{
    normalize_full_path, resolve_links, SaveDirectoryCandidate, SaveDirectoryDiscoveryOptions, SaveDirectoryLocator,
    SaveDiscoveryPlatform,
};
pub use slot::{detect_format, SaveDiscoveryResult, SaveSlot, SaveSlotDiscovery};
