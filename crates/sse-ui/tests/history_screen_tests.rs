//! Integration checks for the backup and diagnostics screens.

use sse_ui::event_loop::{App, Flow, Message};
use sse_ui::glyphs::Fonts;
use sse_ui::raster::Color;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::{AppMessage, ScreenId};
use sse_ui::widget::Tree;

#[test]
fn backup_screen_has_a_clickable_action_without_reading_user_backups_in_screenshot_mode() -> sse_core::Result<()> {
    let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
    let mut shell = Shell::build(&mut tree, None)?;
    tree.resize(1280, 800);
    shell.open(&mut tree, ScreenId::Backups)?;
    tree.update_layout()?;

    let action = (0..800)
        .step_by(4)
        .find_map(|y| (240..1280).step_by(4).find_map(|x| tree.hit(x, y)));
    let Some(action) = action else {
        return Err(sse_core::Error::damaged(
            "backup screen should have an interactive refresh action",
        ));
    };
    assert_eq!(
        shell.message(&mut tree, &Message::User(AppMessage::Tick(0)), Some(action)),
        Flow::Continue
    );
    Ok(())
}
