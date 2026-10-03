#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    missing_docs
)]

use sse_ui::event_loop::{channel_pair, App, Message};
use sse_ui::glyphs::Fonts;
use sse_ui::raster::Color;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::{registry, AppMessage, ScreenId};
use sse_ui::widget::Tree;

#[test]
fn registry_has_every_screen_once_in_sidebar_order() {
    let ids: Vec<ScreenId> = registry().iter().map(|screen| screen.id()).collect();
    assert_eq!(ids, ScreenId::ALL.to_vec());
}

#[test]
fn every_screen_opens_and_paints() {
    let mut tree = Tree::new(Fonts::bundled().unwrap(), Color::rgba(0, 0, 0, 255));
    let mut shell = Shell::build(&mut tree, None).unwrap();
    tree.resize(1280, 860);
    let mut frame = vec![0_u32; 1280 * 860];
    for id in ScreenId::ALL {
        shell.open(&mut tree, id).unwrap();
        assert_eq!(shell.current(), Some(id));
        tree.paint(&mut frame, 1280).unwrap();
    }
}

#[test]
fn worker_result_reaches_its_screen() {
    let (proxy, receiver) = channel_pair::<AppMessage>();
    let mut tree = Tree::new(Fonts::bundled().unwrap(), Color::rgba(0, 0, 0, 255));
    let mut shell = Shell::build(&mut tree, Some(proxy.clone())).unwrap();
    tree.resize(1280, 860);
    shell.open(&mut tree, ScreenId::Settings).unwrap();
    let mut frame = vec![0_u32; 1280 * 860];
    tree.paint(&mut frame, 1280).unwrap();
    // Unknown payloads for a screen are ignored, not a crash.
    proxy.send(AppMessage::ToScreen(ScreenId::Settings, Box::new(5_u8)));
    let message = receiver.recv().unwrap();
    let _ = shell.message(&mut tree, &message, None);
    let _ = shell.message(&mut tree, &Message::User(AppMessage::Tick(3)), None);
    tree.paint(&mut frame, 1280).unwrap();
}
