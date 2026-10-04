//! Integration checks for the games screens (S4 / ScreenId::Games).

use sse_ui::event_loop::Message;
use sse_ui::glyphs::Fonts;
use sse_ui::layout::{NodeKind, Style};
use sse_ui::raster::Color;
use sse_ui::screens::games::{screens, DiscoveredInstallation, DiscoveredResult, GameInstallSource, GameTarget};
use sse_ui::screens::{AppMessage, Context, ScreenId};
use sse_ui::widget::{Content, Look, Tree};
use std::fs;

#[test]
fn games_overview_headless_build_and_render_without_installations() -> sse_core::Result<()> {
    let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
    let host = tree.add(
        None,
        NodeKind::Column,
        Style::default(),
        Content::Panel,
        Look::default(),
    )?;
    let Some(mut screen) = screens().into_iter().next() else {
        return Err(sse_core::Error::damaged("games screen registry is empty"));
    };
    {
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
        };
        screen.build(&mut context, host)?;
    }
    tree.resize(1280, 860);
    tree.update_layout()?;

    // Screen should have clickable buttons (discover button, target switchers, actions)
    let has_clickable_action = (0..860)
        .step_by(10)
        .any(|y| (0..1280).step_by(10).any(|x| tree.hit(x, y).is_some()));
    assert!(
        has_clickable_action,
        "games overview must have interactive buttons even when no games are installed"
    );

    // Initial shown() call should succeed and keep layout intact
    {
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
        };
        screen.shown(&mut context)?;
    }
    tree.update_layout()?;

    Ok(())
}

#[test]
fn games_overview_handles_synthetic_installation_state() -> sse_core::Result<()> {
    let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
    let host = tree.add(
        None,
        NodeKind::Column,
        Style::default(),
        Content::Panel,
        Look::default(),
    )?;
    let Some(mut screen) = screens().into_iter().next() else {
        return Err(sse_core::Error::damaged("games screen registry is empty"));
    };
    {
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
        };
        screen.build(&mut context, host)?;
    }

    // Create a temporary synthetic installation directory
    let temp_dir = std::env::temp_dir().join(format!("sse_test_synth_game_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).map_err(|e| sse_core::Error::damaged(e.to_string()))?;
    fs::write(temp_dir.join("fsgame.ltx"), b"; synthetic stalker test file\n")
        .map_err(|e| sse_core::Error::damaged(e.to_string()))?;

    let synth_install = DiscoveredInstallation {
        target: GameTarget::ShadowOfChernobyl,
        title: "S.T.A.L.K.E.R.: Shadow of Chernobyl (Synthetic)".to_string(),
        directory: temp_dir.clone(),
        source: GameInstallSource::Steam,
        build_id: Some("123456".to_string()),
        save_count: 5,
    };

    let result = DiscoveredResult {
        installations: vec![synth_install],
        status: "Найдено 1".to_string(),
    };

    // Simulate worker message delivered to the screen
    let payload = Box::new(result);
    let app_msg = Message::User(AppMessage::ToScreen(ScreenId::Games, payload));
    {
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
        };
        screen.message(&mut context, &app_msg, None)?;
    }

    tree.resize(1280, 860);
    tree.update_layout()?;

    // Ensure cleanup of temp dir
    let _ = fs::remove_dir_all(&temp_dir);
    Ok(())
}

#[test]
fn game_target_properties_and_cycles() {
    let all = GameTarget::ALL;
    assert_eq!(all.len(), 7);

    for target in all.iter().copied() {
        assert!(!target.title().is_empty());
        assert!(!target.id().is_empty());
        assert!(!target.family().is_empty());
        assert!(!target.release_id().is_empty());
        assert!(!target.install_directories().is_empty());
        if target != GameTarget::Stalker2 {
            assert!(target.is_xray());
        } else {
            assert!(!target.is_xray());
        }
    }
}
