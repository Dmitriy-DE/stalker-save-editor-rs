//! Game file extraction utility for research and fix development.

use std::fs;
use std::path::Path;

use sse_content::file_tree::{CompanionGame, GameFileTree};
use sse_core::{Error, Result};

use crate::fs_util::check_no_links;
use crate::models::GameTarget;

/// Game file extractor.
pub struct GameFileExtractor;

impl GameFileExtractor {
    /// Extracts files from the game archives and loose directories according to LocatorAPI rules.
    ///
    /// # Errors
    /// Returns [`Error::System`] or [`Error::Refused`] on unsupported targets or I/O failure.
    pub fn extract(
        target: GameTarget,
        game_directory: &Path,
        output_directory: &Path,
        prefixes: &[&str],
        archives_only: bool,
    ) -> Result<(usize, Vec<String>)> {
        let (game, fsgame) = match target {
            GameTarget::ShadowOfChernobyl => (CompanionGame::ShadowOfChernobyl, "fsgame.ltx"),
            GameTarget::ClearSky => (CompanionGame::ClearSky, "fsgame.ltx"),
            GameTarget::CallOfPripyat => (CompanionGame::CallOfPripyat, "fsgame.ltx"),
            GameTarget::ShadowOfChernobylEnhancedEdition => (CompanionGame::ShadowOfChernobyl, "fsgame_soc.ltx"),
            GameTarget::ClearSkyEnhancedEdition => (CompanionGame::ClearSky, "fsgame_cs.ltx"),
            GameTarget::CallOfPripyatEnhancedEdition => (CompanionGame::CallOfPripyat, "fsgame_cop.ltx"),
            GameTarget::Stalker2 => {
                return Err(Error::Refused("S2 has no X-Ray archives".to_string()));
            }
        };

        let wanted_prefixes: Vec<String> = prefixes
            .iter()
            .map(|p| p.replace('\\', "/").trim_start_matches('/').to_string())
            .collect();

        let wanted_filter = |path: &str| {
            if wanted_prefixes.is_empty() {
                true
            } else {
                wanted_prefixes
                    .iter()
                    .any(|prefix| path.to_ascii_lowercase().starts_with(&prefix.to_ascii_lowercase()))
            }
        };

        let tree = GameFileTree::load(
            game,
            game_directory,
            wanted_filter,
            Some(&[fsgame]),
            !archives_only,
            true,
            archives_only,
            None,
            None,
        )?;

        let mut issues = tree.issues;
        let mut written: usize = 0;
        let out_canonical = output_directory
            .canonicalize()
            .unwrap_or_else(|_| output_directory.to_path_buf());

        for (relative, file) in &tree.files {
            let Some(destination) = destination_inside(&out_canonical, relative) else {
                issues.push(format!("skipped a path outside the output folder: {relative}"));
                continue;
            };

            match file.read() {
                Ok(bytes) => match write_inside(&out_canonical, &destination, &bytes) {
                    Ok(()) => written = written.saturating_add(1),
                    Err(Error::Refused(reason)) => issues.push(format!("skipped {relative}: {reason}")),
                    Err(_) => issues.push(format!("failed to write: {relative}")),
                },
                Err(err) => {
                    issues.push(format!("{relative}: {err:?}"));
                }
            }
        }

        Ok((written, issues))
    }
}

/// Joins an archive-relative path onto the output folder, or returns `None` if it would leave that folder.
///
/// `..` segments are refused before they are joined, because `Path::starts_with` compares components
/// lexically and would accept `out/../elsewhere`.
fn destination_inside(output: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let mut destination = output.to_path_buf();
    for seg in relative.split('/') {
        if seg == ".." || seg == "." {
            return None;
        }
        destination.push(seg);
    }
    destination.starts_with(output).then_some(destination)
}

/// Writes one extracted file, creating its folders one by one and refusing every link on the way.
///
/// `output` must be the canonical output folder and `destination` a path below it. A link among the existing
/// folders (for example `output/scripts -> /elsewhere`) or in place of the file itself would otherwise send the
/// write outside the folder the user chose, so it is refused before anything is created.
fn write_inside(output: &Path, destination: &Path, bytes: &[u8]) -> Result<()> {
    let relative = destination
        .strip_prefix(output)
        .map_err(|_| Error::Refused("path is outside the output folder".to_owned()))?;
    let mut current = output.to_path_buf();
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        current.push(component);
        check_no_links(output, &current)?;
        if components.peek().is_some() {
            match fs::create_dir(&current) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && current.is_dir() => {}
                Err(error) => return Err(Error::from(error)),
            }
        }
    }
    fs::write(destination, bytes).map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::destination_inside;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn a_link_inside_the_output_folder_does_not_carry_a_write_outside() -> sse_core::Result<()> {
        use sse_core::Error;
        use std::fs;

        let root = std::env::temp_dir().join(format!("sse-extract-links-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let output = root.join("output");
        let outside = root.join("outside");
        fs::create_dir_all(&output)?;
        fs::create_dir_all(&outside)?;
        let output = output.canonicalize()?;
        std::os::unix::fs::symlink(&outside, output.join("scripts"))?;
        fs::write(outside.join("victim.txt"), b"keep")?;
        std::os::unix::fs::symlink(outside.join("victim.txt"), output.join("linked.txt"))?;

        // A folder link, a deeper path through it, and a link in place of the file itself.
        for relative in ["scripts/task.script", "scripts/deep/task.script", "linked.txt"] {
            let destination = super::destination_inside(&output, relative)
                .ok_or_else(|| Error::damaged("fixture path left the output folder"))?;
            let result = super::write_inside(&output, &destination, b"extracted");
            assert!(matches!(result, Err(Error::Refused(_))), "{relative}: {result:?}");
        }
        assert!(!outside.join("task.script").exists());
        assert!(!outside.join("deep").exists());
        assert_eq!(fs::read(outside.join("victim.txt"))?, b"keep");

        // Real folders are still created and written.
        let destination = super::destination_inside(&output, "gamedata/configs/a.ltx")
            .ok_or_else(|| Error::damaged("fixture path left the output folder"))?;
        super::write_inside(&output, &destination, b"extracted")?;
        assert_eq!(fs::read(output.join("gamedata/configs/a.ltx"))?, b"extracted");
        let _ = fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn a_plain_relative_path_stays_inside_the_output_folder() {
        let out = Path::new("/tmp/out");
        assert_eq!(
            destination_inside(out, "gamedata/scripts/task.script"),
            Some(out.join("gamedata").join("scripts").join("task.script"))
        );
    }

    #[test]
    fn parent_segments_are_refused_even_when_they_come_back_inside() {
        let out = Path::new("/tmp/out");
        assert_eq!(destination_inside(out, "../evil.script"), None);
        assert_eq!(destination_inside(out, "gamedata/../../evil.script"), None);
        assert_eq!(destination_inside(out, "gamedata/../scripts/task.script"), None);
        assert_eq!(destination_inside(out, "./task.script"), None);
    }
}
