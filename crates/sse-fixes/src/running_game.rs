//! Refuses to change game files while the game executable is running.
//!
//! The process list is read through `sse_sys::system::running_processes`. The name rules mirror the
//! save-editor process guard in `sse-ui`; keep them in sync when executable names change.

use sse_core::{Error, Result};

use crate::models::GameTarget;

/// Reports whether the game executable for a target is currently running.
pub trait GameRunningProbe: Send + Sync {
    /// Returns `true` when a process matching the game is running.
    ///
    /// # Errors
    /// Returns an error when the process list cannot be read; callers must then refuse the change.
    fn is_game_running(&self, game: GameTarget) -> Result<bool>;
}

/// Probe backed by the operating system process list.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemGameRunningProbe;

impl GameRunningProbe for SystemGameRunningProbe {
    fn is_game_running(&self, game: GameTarget) -> Result<bool> {
        let names = sse_sys::system::running_processes()
            .map_err(|error| Error::System(format!("Could not list running processes: {error}")))?;
        Ok(names.iter().any(|name| game_process_matches(game, name)))
    }
}

/// Probe for tests and synthetic definitions: it never reports a running game.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoGameRunningProbe;

impl GameRunningProbe for NoGameRunningProbe {
    fn is_game_running(&self, _game: GameTarget) -> Result<bool> {
        Ok(false)
    }
}

/// Reports whether `process_name` (bare name or path) is an executable of `game`.
#[must_use]
pub fn game_process_matches(game: GameTarget, process_name: &str) -> bool {
    let executable = process_name.trim().rsplit(['/', '\\']).next().unwrap_or("").trim();
    match game {
        GameTarget::ShadowOfChernobyl | GameTarget::ShadowOfChernobylEnhancedEdition => {
            executable.eq_ignore_ascii_case("XR_3DA.exe")
        }
        GameTarget::ClearSky
        | GameTarget::ClearSkyEnhancedEdition
        | GameTarget::CallOfPripyat
        | GameTarget::CallOfPripyatEnhancedEdition => executable.eq_ignore_ascii_case("xrEngine.exe"),
        GameTarget::Stalker2 => {
            executable.eq_ignore_ascii_case("Stalker2-Win64-")
                || (executable
                    .get(..8)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Stalker2"))
                    && executable.to_ascii_lowercase().ends_with(".exe"))
        }
    }
}

/// Fails with [`Error::Refused`] when the game for `game` is running, or when that cannot be checked.
pub(crate) fn ensure_game_not_running(probe: &dyn GameRunningProbe, game: GameTarget) -> Result<()> {
    if probe.is_game_running(game)? {
        return Err(Error::Refused(
            "Close the game before changing its files; it may overwrite them.".to_string(),
        ));
    }
    Ok(())
}
