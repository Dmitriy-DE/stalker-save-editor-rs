//! Achievement reads and explicitly confirmed mutations.

use crate::api::{Achievement, SteamApi, SteamError};

const SUPPORTED_APP_IDS: [u32; 7] = [1_643_320, 4_500, 20_510, 41_700, 2_427_410, 2_427_420, 2_427_430];

/// Confirmation supplied by an explicit user action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AchievementConfirmation {
    /// The user declined the requested change.
    Declined,
    /// The user explicitly confirmed the requested change.
    Confirmed,
}

/// Shared achievement operations.
#[derive(Debug, Default, Clone, Copy)]
pub struct AchievementService;

impl AchievementService {
    /// Reads achievements for a supported S.T.A.L.K.E.R. Steam app.
    pub fn list(&self, api: &mut dyn SteamApi, app_id: u32) -> Result<Vec<Achievement>, SteamError> {
        validate_app_id(app_id)?;
        api.initialize(app_id)?;
        api.run_callbacks()?;
        api.achievements()
    }

    /// Sets an achievement only after an explicit confirmation, then verifies its state.
    pub fn set(
        &self,
        api: &mut dyn SteamApi,
        app_id: u32,
        name: &str,
        confirmation: AchievementConfirmation,
    ) -> Result<(), SteamError> {
        self.change(api, app_id, name, confirmation, true)
    }

    /// Clears an achievement only after an explicit confirmation, then verifies its state.
    pub fn clear(
        &self,
        api: &mut dyn SteamApi,
        app_id: u32,
        name: &str,
        confirmation: AchievementConfirmation,
    ) -> Result<(), SteamError> {
        self.change(api, app_id, name, confirmation, false)
    }

    fn change(
        &self,
        api: &mut dyn SteamApi,
        app_id: u32,
        name: &str,
        confirmation: AchievementConfirmation,
        achieved: bool,
    ) -> Result<(), SteamError> {
        validate_app_id(app_id)?;
        if confirmation != AchievementConfirmation::Confirmed {
            return Err(SteamError::new("achievement change was not confirmed"));
        }
        if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
            return Err(SteamError::new("achievement name is invalid"));
        }
        api.initialize(app_id)?;
        api.run_callbacks()?;
        let known = api.achievements()?.iter().any(|entry| entry.name == name);
        if !known {
            return Err(SteamError::new("achievement is not present in the selected app"));
        }
        if achieved {
            api.set_achievement(name)?;
        } else {
            api.clear_achievement(name)?;
        }
        api.store_stats()?;
        api.run_callbacks()?;
        let verified = api
            .achievements()?
            .iter()
            .any(|entry| entry.name == name && entry.achieved == achieved);
        if !verified {
            return Err(SteamError::new("Steam did not report the requested achievement state"));
        }
        Ok(())
    }
}

fn validate_app_id(app_id: u32) -> Result<(), SteamError> {
    if SUPPORTED_APP_IDS.contains(&app_id) {
        Ok(())
    } else {
        Err(SteamError::new(
            "achievement operations are limited to supported STALKER app ids",
        ))
    }
}
