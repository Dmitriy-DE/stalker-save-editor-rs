//! Game-file bug fixes catalogue and atomic patching engine.
//!
//! Owner: Gemini (G3).

mod embedded_json;

pub mod all_spawn;
pub mod catalog;
pub mod engine;
pub mod extractor;
pub mod fs_util;
pub mod identify;
pub mod models;
pub mod store;
pub mod toolkit;

pub use all_spawn::AllSpawnEditor;
pub use catalog::GameFixCatalog;
pub use engine::{decode_patch_text, encode_patch_text, GameFixEngine};
pub use extractor::GameFileExtractor;
pub use fs_util::AtomicFileWriter;
pub use identify::identify_game;
pub use models::{
    FileOverlayOperation, GameFixCategory, GameFixDefinition, GameFixFailure, GameFixImplementationType,
    GameFixInstallResult, GameFixInstalledInfo, GameFixJournal, GameFixManifest, GameFixMaturity, GameFixPreset,
    GameFixPresetResult, GameFixSaveCompatibility, GameFixState, GameFixUninstallCheck, GameFixVerificationState,
    GameTarget, ManagedGameFile, SpawnEditKind, SpawnEditOperation, TextPatchOperation,
};
pub use store::GameFixContentStore;
#[cfg(test)]
mod embedded_json_tests;
