//! Game environment toolkit module: managed `user.ltx`, S2 mod toggle, snapshots, profiles, and audit.
//!
//! Owner: Gemini (G6).
//! Every function takes the game installation directory explicitly with no global state.

pub mod audit;
pub mod profile;
pub mod s2_mods;
pub mod snapshot;
pub mod user_ltx;

pub use audit::{AuditItem, FileClassification, ToolkitAuditReport, ToolkitInstallAudit};
pub use profile::{ProfileApplyResult, StoredToolkitProfile, ToolkitProfile, ToolkitProfileService};
pub use s2_mods::{ModToggleResult, ModToggleStatus, Stalker2ModToggle};
pub use snapshot::{InstalledFixSnapshot, SnapshotRestoreReport, ToolkitSnapshot, ToolkitSnapshotService};
pub use user_ltx::{
    ManagedUserLtxSettings, UserLtxDriftReport, UserLtxSettingDefinition, UserLtxSettingType, MANAGED_SETTINGS,
};
