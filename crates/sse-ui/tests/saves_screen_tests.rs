//! Integration checks for the save screens.

use sse_ui::glyphs::Fonts;
use sse_ui::raster::Color;
use sse_ui::screens::shell::Shell;
use sse_ui::widget::Tree;

/// Points the settings and save data at an empty directory, so the check never reads the machine's files.
fn isolate_data_directory() -> sse_core::Result<std::path::PathBuf> {
    let directory = std::env::temp_dir().join(format!("sse-saves-screen-{}", std::process::id()));
    std::fs::create_dir_all(&directory)?;
    for key in [
        "HOME",
        "XDG_DATA_HOME",
        "STALKER_SAVE_EDITOR_DATA",
        "USERPROFILE",
        "LOCALAPPDATA",
    ] {
        std::env::set_var(key, &directory);
    }
    Ok(directory)
}

#[test]
fn save_overview_exposes_an_interactive_action_without_scanning_on_screenshot() -> sse_core::Result<()> {
    let directory = isolate_data_directory()?;
    let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
    let mut shell = Shell::build(&mut tree, None)?;
    shell.resize_window(&mut tree, 1280, 800)?;

    let find = shell.library_find_button();
    let rect = tree.rect(find)?;
    let x = rect.x + i32::try_from(rect.width / 2).unwrap_or_default();
    let y = rect.y + i32::try_from(rect.height / 2).unwrap_or_default();
    assert_eq!(
        tree.hit(x, y),
        Some(find),
        "the save library should offer a clickable Find action"
    );
    assert!(!shell.library_scanning(), "a screenshot should not start a save scan");
    let _ = std::fs::remove_dir_all(&directory);
    Ok(())
}
