//! Portable hotkey layout representation and game-window filter.

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_HOTKEY_LAYOUT_BYTES: usize = 64 * 1024;
static NEXT_HOTKEY_TEMP: AtomicU64 = AtomicU64::new(0);

/// Companion action assigned to one global key gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HotkeyAction {
    /// Restore player health.
    Heal,
    /// Repair equipped items.
    RepairEquipped,
    /// Save a location mark.
    Mark,
    /// Jump to the last saved mark.
    JumpLast,
    /// Run a quick save.
    QuickSave,
}

impl HotkeyAction {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "heal" => Some(Self::Heal),
            "repair_equipped" => Some(Self::RepairEquipped),
            "mark" => Some(Self::Mark),
            "jump_last" => Some(Self::JumpLast),
            "quicksave" => Some(Self::QuickSave),
            _ => None,
        }
    }

    /// Stable C#-compatible action name used in the layout file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Heal => "heal",
            Self::RepairEquipped => "repair_equipped",
            Self::Mark => "mark",
            Self::JumpLast => "jump_last",
            Self::QuickSave => "quicksave",
        }
    }
}

/// Modifier bits for a single letter key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// Control key.
    pub control: bool,
    /// Alt key.
    pub alt: bool,
    /// Shift key.
    pub shift: bool,
}

/// One modifier-plus-letter gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HotkeyGesture {
    /// Required modifier keys.
    pub modifiers: Modifiers,
    /// Uppercase ASCII letter.
    pub key: char,
}

impl HotkeyGesture {
    /// Parses the persisted `Ctrl+Alt+H` syntax.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let parts = text.split('+').map(str::trim);
        let mut modifiers = Modifiers {
            control: false,
            alt: false,
            shift: false,
        };
        let mut key = None;
        for part in parts {
            if key.is_some() {
                return Err(HotkeyError::new("hotkey modifiers must precede the key"));
            }
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" if !modifiers.control => modifiers.control = true,
                "alt" if !modifiers.alt => modifiers.alt = true,
                "shift" if !modifiers.shift => modifiers.shift = true,
                "ctrl" | "control" | "alt" | "shift" => return Err(HotkeyError::new("hotkey repeats a modifier")),
                _ if part.len() == 1
                    && part.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
                    && key.is_none() =>
                {
                    key = part.chars().next().map(|letter| letter.to_ascii_uppercase());
                }
                _ => return Err(HotkeyError::new("hotkey needs modifiers and one A-Z letter")),
            }
        }
        if !modifiers.control && !modifiers.alt && !modifiers.shift {
            return Err(HotkeyError::new("hotkey must include at least one modifier"));
        }
        Ok(Self {
            modifiers,
            key: key.ok_or_else(|| HotkeyError::new("hotkey is missing its letter"))?,
        })
    }
}

impl std::fmt::Display for HotkeyGesture {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.modifiers.control {
            formatter.write_str("Ctrl+")?;
        }
        if self.modifiers.alt {
            formatter.write_str("Alt+")?;
        }
        if self.modifiers.shift {
            formatter.write_str("Shift+")?;
        }
        write!(formatter, "{}", self.key)
    }
}

/// A duplicate-free portable hotkey layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyLayout {
    bindings: BTreeMap<HotkeyAction, HotkeyGesture>,
}

impl HotkeyLayout {
    /// Current default: Ctrl+H / R / M / J / S.
    fn defaults() -> Self {
        let mut bindings = BTreeMap::new();
        for (action, key) in [
            (HotkeyAction::Heal, 'H'),
            (HotkeyAction::RepairEquipped, 'R'),
            (HotkeyAction::Mark, 'M'),
            (HotkeyAction::JumpLast, 'J'),
            (HotkeyAction::QuickSave, 'S'),
        ] {
            let _ = bindings.insert(
                action,
                HotkeyGesture {
                    modifiers: Modifiers {
                        control: true,
                        alt: false,
                        shift: false,
                    },
                    key,
                },
            );
        }
        Self { bindings }
    }

    /// Parses the C# editor's `action=Modifier+Letter` text format.
    pub fn parse(text: &str) -> Result<Self, HotkeyError> {
        let mut bindings = BTreeMap::new();
        let mut gestures = HashSet::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (name, value) = line
                .split_once('=')
                .ok_or_else(|| HotkeyError::new("hotkey line must use action=Modifier+Letter"))?;
            let action =
                HotkeyAction::from_name(name.trim()).ok_or_else(|| HotkeyError::new("unknown hotkey action"))?;
            let gesture = HotkeyGesture::parse(value.trim())?;
            if bindings.insert(action, gesture).is_some() || !gestures.insert(gesture) {
                return Err(HotkeyError::new("hotkey action or gesture is duplicated"));
            }
        }
        if bindings.is_empty() {
            return Err(HotkeyError::new("hotkey layout is empty"));
        }
        Ok(Self { bindings })
    }

    /// Returns the gesture assigned to an action.
    #[must_use]
    pub fn binding(&self, action: HotkeyAction) -> Option<HotkeyGesture> {
        self.bindings.get(&action).copied()
    }

    /// Emits deterministic C#-compatible text.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut output = String::new();
        for (action, gesture) in &self.bindings {
            output.push_str(action.name());
            output.push('=');
            output.push_str(&gesture.to_string());
            output.push('\n');
        }
        output
    }

    /// Loads a user layout or falls back to the default if the file is missing or malformed.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        let contents = (|| {
            let file = File::open(path).ok()?;
            if file.metadata().ok()?.len() > u64::try_from(MAX_HOTKEY_LAYOUT_BYTES).ok()? {
                return None;
            }
            let mut bytes = Vec::new();
            file.take(u64::try_from(MAX_HOTKEY_LAYOUT_BYTES).ok()?.saturating_add(1))
                .read_to_end(&mut bytes)
                .ok()?;
            if bytes.len() > MAX_HOTKEY_LAYOUT_BYTES {
                return None;
            }
            String::from_utf8(bytes).ok()
        })();
        contents
            .and_then(|text| Self::parse(&text).ok())
            .unwrap_or_else(Self::default)
    }

    /// Writes the layout through a same-directory temporary file and rename.
    pub fn save(&self, path: &Path) -> Result<(), HotkeyError> {
        let contents = self.to_text();
        if contents.len() > MAX_HOTKEY_LAYOUT_BYTES {
            return Err(HotkeyError::new("hotkey layout exceeds the size limit"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| HotkeyError::new("layout path has no parent"))?;
        std::fs::create_dir_all(parent).map_err(HotkeyError::from_io)?;
        let sequence = NEXT_HOTKEY_TEMP.fetch_add(1, Ordering::Relaxed);
        let file_name = path
            .file_name()
            .ok_or_else(|| HotkeyError::new("layout path has no file name"))?
            .to_string_lossy();
        let temporary = parent.join(format!(".{file_name}.pending-{}-{sequence}", std::process::id()));
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(HotkeyError::from_io)?;
        use std::io::Write;
        if let Err(error) = file.write_all(contents.as_bytes()).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&temporary);
            return Err(HotkeyError::from_io(error));
        }
        drop(file);
        let previous = parent.join(format!(".{file_name}.previous-{}-{sequence}", std::process::id()));
        let had_target = path.exists();
        if had_target {
            if let Err(error) = fs::rename(path, &previous) {
                let _ = fs::remove_file(&temporary);
                return Err(HotkeyError::from_io(error));
            }
        }
        if let Err(error) = fs::rename(&temporary, path) {
            if had_target {
                let _ = fs::rename(&previous, path);
            }
            let _ = fs::remove_file(&temporary);
            return Err(HotkeyError::from_io(error));
        }
        if had_target {
            fs::remove_file(previous).map_err(HotkeyError::from_io)?;
        }
        Ok(())
    }
}

impl Default for HotkeyLayout {
    fn default() -> Self {
        Self::defaults()
    }
}

/// Windows and X11 share the same exact process/class allow-list.
#[derive(Debug, Default, Clone, Copy)]
pub struct HotkeyMatcher;

impl HotkeyMatcher {
    /// True only for X-Ray release process/window identifiers recognized by C#.
    #[must_use]
    pub fn matches(&self, process_name: &str, window_class: &str) -> bool {
        Self::is_game_name(process_name) || Self::is_game_name(window_class)
    }

    /// Matches an executable name, Steam app window name, or X-Ray class name.
    #[must_use]
    pub fn is_game_name(name: &str) -> bool {
        let value = name.trim().to_ascii_lowercase();
        let value = value.strip_suffix(".exe").unwrap_or(&value);
        [
            "xr_3da",
            "xrengine",
            "steam_app_4500",
            "steam_app_20510",
            "steam_app_41700",
            "steam_app_2427410",
            "steam_app_2427420",
            "steam_app_2427430",
        ]
        .iter()
        .any(|expected| {
            value == *expected
                || value
                    .strip_suffix(expected)
                    .is_some_and(|prefix| prefix.ends_with('/') || prefix.ends_with('\\'))
        })
    }
}

/// Invalid or unpersistable hotkey layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyError {
    message: String,
}

impl HotkeyError {
    fn new(message: &str) -> Self {
        Self {
            message: message.to_owned(),
        }
    }
    fn from_io(error: std::io::Error) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl std::fmt::Display for HotkeyError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
impl std::error::Error for HotkeyError {}
