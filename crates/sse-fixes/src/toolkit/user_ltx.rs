//! Managed `user.ltx` configuration engine for S.T.A.L.K.E.R. games.
//!
//! Preserves comments, ordering, line breaks, and CP1251 encoding.
//! All operations take the game installation path explicitly with no global state.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::fs_util::AtomicFileWriter;
use sse_core::{Error, Result};

/// Type of managed setting and its valid domain.
#[derive(Clone, Debug, PartialEq)]
pub enum UserLtxSettingType {
    /// Floating point value with min and max bounds.
    Float {
        /// Lower bound for valid float value.
        min: f64,
        /// Upper bound for valid float value.
        max: f64,
    },
    /// Boolean flag (`on`/`off` or `1`/`0`).
    Boolean,
    /// Integer value with bounds.
    Integer {
        /// Lower bound for valid integer value.
        min: i64,
        /// Upper bound for valid integer value.
        max: i64,
    },
}

/// Metadata definition for a managed console variable in `user.ltx`.
#[derive(Clone, Debug)]
pub struct UserLtxSettingDefinition {
    /// Console variable identifier (e.g. `g_fov`).
    pub key: &'static str,
    /// Setting value type and bounds.
    pub setting_type: UserLtxSettingType,
    /// Default vanilla value.
    pub default_val: &'static str,
    /// Short human-readable description.
    pub description: &'static str,
}

/// Allow-list of known managed console settings for X-Ray games.
pub static MANAGED_SETTINGS: &[UserLtxSettingDefinition] = &[
    UserLtxSettingDefinition {
        key: "g_fov",
        setting_type: UserLtxSettingType::Float { min: 55.0, max: 110.0 },
        default_val: "67.5",
        description: "Field of view angle in degrees",
    },
    UserLtxSettingDefinition {
        key: "hud_fov",
        setting_type: UserLtxSettingType::Float { min: 0.3, max: 0.85 },
        default_val: "0.45",
        description: "First-person weapon and hands field of view",
    },
    UserLtxSettingDefinition {
        key: "mouse_sens",
        setting_type: UserLtxSettingType::Float { min: 0.05, max: 2.0 },
        default_val: "0.12",
        description: "Mouse look sensitivity",
    },
    UserLtxSettingDefinition {
        key: "mouse_invert",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "off",
        description: "Invert mouse Y-axis",
    },
    UserLtxSettingDefinition {
        key: "hud_crosshair",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "on",
        description: "Show crosshair reticle",
    },
    UserLtxSettingDefinition {
        key: "hud_crosshair_dist",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "off",
        description: "Show distance under crosshair",
    },
    UserLtxSettingDefinition {
        key: "hud_info",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "on",
        description: "Show interactive target info and crosshair color",
    },
    UserLtxSettingDefinition {
        key: "rs_v_sync",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "off",
        description: "Vertical synchronization",
    },
    UserLtxSettingDefinition {
        key: "rs_fullscreen",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "on",
        description: "Fullscreen display mode",
    },
    UserLtxSettingDefinition {
        key: "cam_inert",
        setting_type: UserLtxSettingType::Float { min: 0.0, max: 1.0 },
        default_val: "0.0",
        description: "Camera movement inertia",
    },
    UserLtxSettingDefinition {
        key: "snd_volume_eff",
        setting_type: UserLtxSettingType::Float { min: 0.0, max: 1.0 },
        default_val: "1.0",
        description: "Sound effects volume",
    },
    UserLtxSettingDefinition {
        key: "snd_volume_music",
        setting_type: UserLtxSettingType::Float { min: 0.0, max: 1.0 },
        default_val: "0.7",
        description: "Music playback volume",
    },
    UserLtxSettingDefinition {
        key: "r2_sun",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "on",
        description: "Direct sunlight shadow rendering",
    },
    UserLtxSettingDefinition {
        key: "r2_sun_details",
        setting_type: UserLtxSettingType::Boolean,
        default_val: "off",
        description: "Grass detail shadow rendering",
    },
    UserLtxSettingDefinition {
        key: "r2_slight_fade",
        setting_type: UserLtxSettingType::Float { min: 0.2, max: 2.0 },
        default_val: "1.0",
        description: "Sunlight shadow fade distance multiplier",
    },
    UserLtxSettingDefinition {
        key: "r2_tf_mipbias",
        setting_type: UserLtxSettingType::Float { min: -0.5, max: 0.5 },
        default_val: "0.0",
        description: "Texture mipmap bias sharpness",
    },
];

/// Status report on configuration drift compared to a baseline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserLtxDriftReport {
    /// Number of managed settings currently present in the file.
    pub present_count: usize,
    /// Settings that differ from the expected baseline.
    pub drifted_values: BTreeMap<String, (String, String)>,
    /// Settings present in baseline but missing from the file.
    pub missing_keys: Vec<String>,
}

impl UserLtxDriftReport {
    /// Returns true if no settings have drifted from the baseline.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.drifted_values.is_empty() && self.missing_keys.is_empty()
    }
}

/// Service managing the game's `user.ltx` console settings.
pub struct ManagedUserLtxSettings;

impl ManagedUserLtxSettings {
    /// Resolves the absolute path to `user.ltx` for the given game directory.
    #[must_use]
    pub fn resolve_path(game_directory: &Path) -> Option<PathBuf> {
        // Priority 1: _appdata_/user.ltx
        let appdata_alt = game_directory.join("_appdata_").join("user.ltx");
        if appdata_alt.is_file() {
            return Some(appdata_alt);
        }

        // Priority 2: appdata/user.ltx
        let appdata_std = game_directory.join("appdata").join("user.ltx");
        if appdata_std.is_file() {
            return Some(appdata_std);
        }

        // Priority 3: Parse fsgame.ltx for $app_data_root$
        let fsgame_path = game_directory.join("fsgame.ltx");
        if fsgame_path.is_file() {
            if let Ok(content) = fs::read_to_string(&fsgame_path) {
                if let Some(resolved) = parse_fsgame_appdata(&content, game_directory) {
                    let candidate = resolved.join("user.ltx");
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        // Priority 4: user.ltx directly in game root
        let root_user = game_directory.join("user.ltx");
        if root_user.is_file() {
            return Some(root_user);
        }

        // Fallback default path if creating fresh: _appdata_/user.ltx if directory exists, else appdata/user.ltx
        if game_directory.join("_appdata_").is_dir() {
            Some(appdata_alt)
        } else if game_directory.join("appdata").is_dir() {
            Some(appdata_std)
        } else {
            Some(game_directory.join("_appdata_").join("user.ltx"))
        }
    }

    /// Reads all managed settings currently defined in the game's `user.ltx`.
    ///
    /// # Errors
    /// Returns an error if the file cannot be read or decoded.
    pub fn read_managed_settings(game_directory: &Path) -> Result<BTreeMap<String, String>> {
        let path = match Self::resolve_path(game_directory) {
            Some(p) if p.is_file() => p,
            _ => return Ok(BTreeMap::new()),
        };

        let raw_bytes = fs::read(&path).map_err(Error::from)?;
        let text = decode_ltx_bytes(&raw_bytes)?;

        let mut results = BTreeMap::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('/') {
                continue;
            }
            if let Some((k, v)) = split_key_value(trimmed) {
                if let Some(canonical_key) = canonical_managed_key(k) {
                    if results.insert(canonical_key.to_string(), v.to_string()).is_some() {
                        return Err(Error::Refused(format!(
                            "The user.ltx contains more than one {canonical_key} command; no change was made."
                        )));
                    }
                }
            }
        }

        Ok(results)
    }

    /// Checks managed settings without touching `user.ltx`, so callers can refuse before changing anything else.
    ///
    /// # Errors
    /// Returns an error for keys outside the managed allow-list or values out of range.
    pub fn validate_managed_settings(settings_to_update: &BTreeMap<String, String>) -> Result<()> {
        for (k, v) in settings_to_update {
            validate_setting_value(k, v)?;
        }
        Ok(())
    }

    /// Updates or appends managed settings in `user.ltx`, preserving existing formatting.
    ///
    /// # Errors
    /// Returns an error if validation fails or writing fails.
    pub fn update_managed_settings(
        game_directory: &Path,
        settings_to_update: &BTreeMap<String, String>,
    ) -> Result<usize> {
        for (k, v) in settings_to_update {
            validate_setting_value(k, v)?;
        }

        let path = Self::resolve_path(game_directory)
            .ok_or_else(|| Error::Refused("Could not resolve user.ltx path for game directory".to_string()))?;

        let (mut lines, line_ending) = if path.is_file() {
            let raw_bytes = fs::read(&path).map_err(Error::from)?;
            let text = decode_ltx_bytes(&raw_bytes)?;
            let le = if text.contains("\r\n") { "\r\n" } else { "\n" };
            (text.lines().map(ToString::to_string).collect::<Vec<_>>(), le)
        } else {
            (Vec::new(), "\r\n")
        };

        let mut applied_count: usize = 0;
        let mut keys_remaining = BTreeMap::new();
        for (key, value) in settings_to_update {
            let canonical_key = canonical_managed_key(key)
                .ok_or_else(|| Error::Refused(format!("Setting '{key}' is not in the managed allow-list")))?;
            if keys_remaining
                .insert(canonical_key.to_string(), value.clone())
                .is_some()
            {
                return Err(Error::Refused(format!(
                    "Setting '{canonical_key}' was provided more than once with different casing"
                )));
            }
        }
        let mut seen_keys = BTreeSet::new();

        for line in &mut lines {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('/') {
                continue;
            }
            if let Some((k, _)) = split_key_value(trimmed) {
                if let Some(canonical_key) = canonical_managed_key(k) {
                    if !seen_keys.insert(canonical_key) {
                        return Err(Error::Refused(format!(
                            "The user.ltx contains more than one {canonical_key} command; no change was made."
                        )));
                    }
                    if let Some(new_val) = keys_remaining.remove(canonical_key) {
                        // Reconstruct line keeping prefix indentation
                        let indent_len = line.len().saturating_sub(line.trim_start().len());
                        let indent = line.get(..indent_len).unwrap_or("");
                        *line = format!("{indent}{k} {new_val}");
                        applied_count = applied_count.saturating_add(1);
                    }
                }
            }
        }

        // Any keys not already present in the file are appended at the end
        if !keys_remaining.is_empty() {
            if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.push(String::new());
            }
            lines.push("; --- Managed by S.T.A.L.K.E.R. Save Editor Toolkit ---".to_string());
            for (k, v) in keys_remaining {
                lines.push(format!("{k} {v}"));
                applied_count = applied_count.saturating_add(1);
            }
        }

        let mut output_text = lines.join(line_ending);
        output_text.push_str(line_ending);

        // Refuse to write rather than fall back to UTF-8: X-Ray reads user.ltx as cp1251.
        let output_bytes = crate::engine::encode_patch_text(&output_text, 1251)
            .map_err(|e| Error::damaged(format!("{} was not written: {e}", path.display())))?;
        AtomicFileWriter::write(&path, &output_bytes, true)?;

        Ok(applied_count)
    }

    /// Checks for drift between actual `user.ltx` values and expected baseline values.
    ///
    /// # Errors
    /// Returns an error if `user.ltx` cannot be read.
    pub fn detect_drift(game_directory: &Path, baseline: &BTreeMap<String, String>) -> Result<UserLtxDriftReport> {
        let current = Self::read_managed_settings(game_directory)?;
        let mut drifted = BTreeMap::new();
        let mut missing = Vec::new();

        for (k, expected_v) in baseline {
            let current_key = canonical_managed_key(k).unwrap_or(k);
            match current.get(current_key) {
                Some(actual_v) => {
                    if !values_match(k, actual_v, expected_v) {
                        drifted.insert(k.clone(), (actual_v.clone(), expected_v.clone()));
                    }
                }
                None => {
                    missing.push(k.clone());
                }
            }
        }

        Ok(UserLtxDriftReport {
            present_count: current.len(),
            drifted_values: drifted,
            missing_keys: missing,
        })
    }
}

fn canonical_managed_key(key: &str) -> Option<&'static str> {
    MANAGED_SETTINGS
        .iter()
        .find(|setting| setting.key.eq_ignore_ascii_case(key))
        .map(|setting| setting.key)
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split_whitespace();
    let key = parts.next()?;
    let value = parts.next()?;
    Some((key, value))
}

fn validate_setting_value(key: &str, val: &str) -> Result<()> {
    let canonical_key = canonical_managed_key(key)
        .ok_or_else(|| Error::Refused(format!("Setting '{key}' is not in the managed allow-list")))?;
    let def = MANAGED_SETTINGS
        .iter()
        .find(|setting| setting.key == canonical_key)
        .ok_or_else(|| Error::Refused(format!("Setting '{key}' is not in the managed allow-list")))?;

    match &def.setting_type {
        UserLtxSettingType::Boolean => {
            let lower = val.to_ascii_lowercase();
            if !matches!(lower.as_str(), "on" | "off" | "1" | "0" | "true" | "false") {
                return Err(Error::Refused(format!(
                    "Invalid boolean value '{val}' for setting '{key}'"
                )));
            }
        }
        UserLtxSettingType::Float { min, max } => {
            let trimmed = val.trim_end_matches('.');
            let parsed: f64 = trimmed
                .parse()
                .map_err(|_| Error::Refused(format!("Invalid float value '{val}' for setting '{key}'")))?;
            // NaN compares false with everything, so it must be refused explicitly.
            if !parsed.is_finite() || parsed < *min || parsed > *max {
                return Err(Error::Refused(format!(
                    "Value {parsed} for '{key}' is out of bounds [{min}, {max}]"
                )));
            }
        }
        UserLtxSettingType::Integer { min, max } => {
            let parsed: i64 = val
                .parse()
                .map_err(|_| Error::Refused(format!("Invalid integer value '{val}' for setting '{key}'")))?;
            if parsed < *min || parsed > *max {
                return Err(Error::Refused(format!(
                    "Value {parsed} for '{key}' is out of bounds [{min}, {max}]"
                )));
            }
        }
    }

    Ok(())
}

fn values_match(key: &str, val_a: &str, val_b: &str) -> bool {
    let canonical_key = canonical_managed_key(key);
    let def = MANAGED_SETTINGS
        .iter()
        .find(|setting| Some(setting.key) == canonical_key);

    match def.map(|d| &d.setting_type) {
        Some(UserLtxSettingType::Boolean) => {
            let norm_a = matches!(val_a.to_ascii_lowercase().as_str(), "on" | "1" | "true");
            let norm_b = matches!(val_b.to_ascii_lowercase().as_str(), "on" | "1" | "true");
            norm_a == norm_b
        }
        Some(UserLtxSettingType::Float { .. }) => {
            let num_a = val_a.trim_end_matches('.').parse::<f64>().ok();
            let num_b = val_b.trim_end_matches('.').parse::<f64>().ok();
            match (num_a, num_b) {
                (Some(a), Some(b)) => (a - b).abs() < 1e-4,
                _ => val_a == val_b,
            }
        }
        _ => val_a.eq_ignore_ascii_case(val_b),
    }
}

fn parse_fsgame_appdata(content: &str, game_directory: &Path) -> Option<PathBuf> {
    let game_root = game_directory.canonicalize().ok()?;
    for line in content.lines() {
        let trimmed = line.trim();
        let Some((key, definition)) = trimmed.split_once('=') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("$app_data_root$") {
            continue;
        }

        let parts: Vec<&str> = definition.split('|').collect();
        let rel = parts.get(3)?.trim().trim_end_matches(['\\', '/']);
        let normalized = rel.replace('\\', "/");
        let relative = Path::new(&normalized);
        let bytes = normalized.as_bytes();
        let has_drive_prefix = bytes.first().is_some_and(u8::is_ascii_alphabetic) && bytes.get(1) == Some(&b':');
        if relative.is_absolute()
            || has_drive_prefix
            || relative.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return None;
        }

        let candidate = game_root.join(relative);
        let canonical_candidate = candidate.canonicalize().ok()?;
        if !canonical_candidate.starts_with(&game_root) {
            return None;
        }
        return Some(canonical_candidate);
    }
    None
}

fn decode_ltx_bytes(bytes: &[u8]) -> Result<String> {
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return Ok(utf8.to_string());
    }
    crate::engine::decode_patch_text(bytes, 1251)
}
