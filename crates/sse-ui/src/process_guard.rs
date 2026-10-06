//! Pure matching rules used by the K29 save/restore process guard.

use std::io;
use std::io::Read;
use std::path::Path;

/// User-facing warning when the selected game is still running.
pub const SAVE_WHILE_GAME_RUNNING_WARNING: &str = "Закройте игру перед сохранением: она может перезаписать изменения.";

/// Executable family associated with a save format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameProcessFamily {
    /// Shadow of Chernobyl and its Enhanced Edition.
    ShadowOfChernobyl,
    /// Clear Sky or Call of Pripyat.
    ClearSkyOrPripyat,
    /// S.T.A.L.K.E.R. 2.
    HeartOfChornobyl,
}

/// Reports whether `process_name` is one of the executables for `game`.
///
/// Process names may be bare names or full paths with either platform's separators.
#[must_use]
pub fn game_process_matches(game: GameProcessFamily, process_name: &str) -> bool {
    let executable = process_name.trim().rsplit(['/', '\\']).next().unwrap_or("").trim();
    match game {
        GameProcessFamily::ShadowOfChernobyl => executable.eq_ignore_ascii_case("XR_3DA.exe"),
        GameProcessFamily::ClearSkyOrPripyat => executable.eq_ignore_ascii_case("xrEngine.exe"),
        GameProcessFamily::HeartOfChornobyl => {
            executable
                .get(..8)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("Stalker2"))
                && executable.to_ascii_lowercase().ends_with(".exe")
        }
    }
}

/// Resolves a save's detected format to its executable family.
#[must_use]
pub fn game_process_family_for_format(format_id: &str) -> Option<GameProcessFamily> {
    if ["stalker-soc", "stalker-soc-ee"]
        .iter()
        .any(|id| format_id.eq_ignore_ascii_case(id))
    {
        Some(GameProcessFamily::ShadowOfChernobyl)
    } else if ["stalker-cs", "stalker-cs-ee", "stalker-cop", "stalker-cop-ee"]
        .iter()
        .any(|id| format_id.eq_ignore_ascii_case(id))
    {
        Some(GameProcessFamily::ClearSkyOrPripyat)
    } else if format_id.eq_ignore_ascii_case("stalker2") {
        Some(GameProcessFamily::HeartOfChornobyl)
    } else {
        None
    }
}

/// Returns the process name that matches a save's format, if one is present.
#[must_use]
pub fn matching_game_process<'a>(format_id: &str, process_names: &'a [String]) -> Option<&'a str> {
    let family = game_process_family_for_format(format_id)?;
    process_names
        .iter()
        .find(|name| game_process_matches(family, name))
        .map(String::as_str)
}

/// Detects a Windows sharing or lock violation after the core error was flattened to display text.
#[must_use]
pub fn is_windows_file_busy_error_text(error: &str) -> bool {
    #[cfg(windows)]
    {
        has_os_error_code(error, 32) || has_os_error_code(error, 33)
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        false
    }
}

#[cfg(any(windows, test))]
fn has_os_error_code(error: &str, code: i32) -> bool {
    error.contains(&format!("(os error {code})"))
}

/// Checks the process list for the game associated with a detected save format.
///
/// Call this from a worker thread: process enumeration can involve filesystem or OS work.
pub fn running_game_for_format(format_id: &str) -> std::result::Result<bool, String> {
    let family = game_process_family_for_format(format_id)
        .ok_or_else(|| format!("unsupported save format for process guard: {format_id}"))?;
    let processes = sse_sys::system::running_processes().map_err(|error| error.to_string())?;
    Ok(processes.iter().any(|name| game_process_matches(family, name)))
}

/// Detects a save format from a bounded sample of a backup file.
///
/// This is used before a restore when the original save may be missing.
pub fn format_id_for_save_file(path: &Path) -> std::result::Result<String, String> {
    const FORMAT_SAMPLE_LIMIT: u64 = 64 * 1024;
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut sample = Vec::new();
    file.by_ref()
        .take(FORMAT_SAMPLE_LIMIT)
        .read_to_end(&mut sample)
        .map_err(|error| error.to_string())?;
    sse_storage::discovery::detect_format(&sample)
        .0
        .ok_or_else(|| "backup save format could not be detected".to_owned())
}

/// Returns the K29 warning for Windows sharing/lock violations.
#[must_use]
pub fn file_busy_warning(error: &io::Error) -> Option<&'static str> {
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33)).then_some(SAVE_WHILE_GAME_RUNNING_WARNING)
    }
    #[cfg(not(windows))]
    {
        let _ = error;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        file_busy_warning, game_process_family_for_format, game_process_matches, has_os_error_code,
        matching_game_process, GameProcessFamily, SAVE_WHILE_GAME_RUNNING_WARNING,
    };
    use std::io;

    #[test]
    fn matches_original_trilogy_process_names_by_basename() {
        assert!(game_process_matches(
            GameProcessFamily::ShadowOfChernobyl,
            r"C:\Games\Shadow of Chernobyl\XR_3DA.exe"
        ));
        assert!(game_process_matches(
            GameProcessFamily::ClearSkyOrPripyat,
            "/games/cop/xrEngine.exe"
        ));
        assert!(!game_process_matches(
            GameProcessFamily::ShadowOfChernobyl,
            "xrEngine.exe"
        ));
    }

    #[test]
    fn matches_stalker2_executable_family_without_matching_unrelated_processes() {
        assert!(game_process_matches(
            GameProcessFamily::HeartOfChornobyl,
            r"C:\Games\Stalker2-Win64-Shipping.exe"
        ));
        assert!(game_process_matches(
            GameProcessFamily::HeartOfChornobyl,
            "stalker2.exe"
        ));
        assert!(!game_process_matches(
            GameProcessFamily::HeartOfChornobyl,
            "Stalker.exe"
        ));
        assert!(!game_process_matches(
            GameProcessFamily::ClearSkyOrPripyat,
            "Stalker2-Win64-Shipping.exe"
        ));
    }

    #[test]
    fn windows_sharing_and_lock_errors_use_the_game_running_warning() {
        for code in [32, 33] {
            let error = io::Error::from_raw_os_error(code);
            assert_eq!(
                file_busy_warning(&error),
                cfg!(windows).then_some(SAVE_WHILE_GAME_RUNNING_WARNING)
            );
        }
        assert_eq!(file_busy_warning(&io::Error::other("disk full")), None);
    }

    #[test]
    fn save_formats_select_only_their_own_game_processes() {
        let processes = [
            "XR_3DA.exe".to_owned(),
            "xrEngine.exe".to_owned(),
            "Stalker2-Win64-Shipping.exe".to_owned(),
        ];

        assert_eq!(
            game_process_family_for_format("stalker-soc"),
            Some(GameProcessFamily::ShadowOfChernobyl)
        );
        assert_eq!(
            game_process_family_for_format("stalker-soc-ee"),
            Some(GameProcessFamily::ShadowOfChernobyl)
        );
        assert_eq!(
            game_process_family_for_format("stalker-cs"),
            Some(GameProcessFamily::ClearSkyOrPripyat)
        );
        assert_eq!(
            game_process_family_for_format("stalker-cs-ee"),
            Some(GameProcessFamily::ClearSkyOrPripyat)
        );
        assert_eq!(
            game_process_family_for_format("stalker-cop"),
            Some(GameProcessFamily::ClearSkyOrPripyat)
        );
        assert_eq!(
            game_process_family_for_format("stalker-cop-ee"),
            Some(GameProcessFamily::ClearSkyOrPripyat)
        );
        assert_eq!(
            game_process_family_for_format("stalker2"),
            Some(GameProcessFamily::HeartOfChornobyl)
        );
        assert_eq!(game_process_family_for_format("unknown"), None);

        assert_eq!(matching_game_process("stalker-soc", &processes), Some("XR_3DA.exe"));
        assert_eq!(matching_game_process("stalker-cop", &processes), Some("xrEngine.exe"));
        assert_eq!(
            matching_game_process("stalker2", &processes),
            Some("Stalker2-Win64-Shipping.exe")
        );
        assert_eq!(matching_game_process("stalker-soc", &["xrEngine.exe".to_owned()]), None);
        assert_eq!(matching_game_process("unsupported", &processes), None);
    }

    #[test]
    fn flattened_windows_busy_errors_are_recognized_by_their_raw_code() {
        assert!(has_os_error_code("access denied (os error 32)", 32));
        assert!(has_os_error_code("sharing violation (os error 33)", 33));
        assert!(!has_os_error_code("broken pipe (os error 32)", 33));
        assert!(!has_os_error_code("access denied (os error 5)", 32));
        let error = io::Error::from_raw_os_error(32).to_string();
        assert_eq!(super::is_windows_file_busy_error_text(&error), cfg!(windows));
    }

    #[test]
    fn restore_format_detection_uses_a_bounded_save_sample() -> io::Result<()> {
        let cases: [(&[u8], &str); 2] = [
            (
                include_bytes!("../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav").as_slice(),
                "stalker-soc",
            ),
            (
                include_bytes!("../../../fixtures/synthetic/synthetic-s2.sav").as_slice(),
                "stalker2",
            ),
        ];
        for (index, (fixture, expected_format)) in cases.iter().enumerate() {
            let path = std::env::temp_dir().join(format!(
                "sse-process-guard-{}-{index}-{}.sav",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_nanos())
            ));
            std::fs::write(&path, fixture)?;

            let format = super::format_id_for_save_file(&path);

            let _ = std::fs::remove_file(path);
            assert_eq!(format.as_deref(), Ok(*expected_format));
        }
        Ok(())
    }
}
