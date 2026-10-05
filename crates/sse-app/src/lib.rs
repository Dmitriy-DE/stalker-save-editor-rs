//! Application-level logic, configuration, background tasks, and state management.
//!
//! Owner: Gemini (G8). No UI dependencies.

pub mod diagnostics;
pub mod paths;
pub mod save_session;
pub mod settings;
pub mod settings_writer;
pub mod state;
pub mod tasks;

pub use paths::default_settings_path;
pub use save_session::{
    CloseDecision, DraftGeneration, FileCheckGuard, SaveOperationGuard, SaveOperationId, SaveOperationKind, SaveSession,
};
pub use settings::AppSettings;
pub use state::{AppEvent, AppState, AppStateSnapshot};
pub use tasks::{CancellationToken, TaskEvent, TaskHandle, TaskId, TaskManager, TaskProgress};
