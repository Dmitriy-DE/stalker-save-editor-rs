//! Integration checks for the save screens.

use sse_ui::glyphs::Fonts;
use sse_ui::layout::{NodeKind, Style};
use sse_ui::raster::Color;
use sse_ui::screens::saves;
use sse_ui::screens::Context;
use sse_ui::widget::{Content, Look, Tree};

#[test]
fn save_overview_exposes_an_interactive_action_without_scanning_on_screenshot() -> sse_core::Result<()> {
    let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
    let host = tree.add(
        None,
        NodeKind::Column,
        Style::default(),
        Content::Panel,
        Look::default(),
    )?;
    let Some(mut screen) = saves::screens().into_iter().next() else {
        return Err(sse_core::Error::damaged("save screen registry is empty"));
    };
    let mut context = Context {
        tree: &mut tree,
        proxy: None,
        status: None,
    };
    screen.build(&mut context, host)?;
    tree.resize(1280, 800);
    tree.update_layout()?;

    let has_body_action = (0..800)
        .step_by(4)
        .any(|y| (240..1280).step_by(4).any(|x| tree.hit(x, y).is_some()));
    assert!(
        has_body_action,
        "save overview should have a clickable refresh or save row"
    );
    Ok(())
}
