//! X-Ray saves (SoC, CS, CoP and the Enhanced Editions): container, index, readers, writers. Owner: Codex (C1, C2).

pub mod container;
pub mod level_changer;
mod registry;
pub mod save;
pub mod statistics;
pub mod tasks;
pub mod weather;
pub mod writer;

pub use level_changer::{LevelChangerDestination, Vector3};
pub use save::{format_id_for_versions, Format, InventoryItem, RegistryObject, Save};
pub use statistics::Kills;
pub use tasks::{Task, TaskState};
pub use weather::Weather;
