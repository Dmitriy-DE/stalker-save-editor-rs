#![allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    missing_docs
)]

use sse_ui::event_loop::{channel_pair, run, App, Flow, Message, Present, WindowEvent};
use sse_ui::glyphs::{Face, Fonts, TextStyle};
use sse_ui::layout::{Align, NodeKind, Size, Style};
use sse_ui::raster::{Color, Rect};
use sse_ui::widget::{Content, Look, Tree, WidgetId};

fn grey(v: u8) -> Color {
    Color::rgba(v, v, v, 255)
}

struct Fixture {
    tree: Tree,
    buttons: Vec<WidgetId>,
    label: WidgetId,
}

fn fixture() -> Fixture {
    let mut tree = Tree::new(Fonts::bundled().unwrap(), grey(10));
    let root = tree
        .add(
            None,
            NodeKind::Column,
            Style {
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )
        .unwrap();
    let mut buttons = Vec::new();
    for name in ["ОДИН", "ДВА", "ТРИ"] {
        let look = Look {
            fill: Some(grey(30)),
            hover_fill: Some(grey(60)),
            ..Look::default()
        };
        let style = Style {
            min: Size::new(0.0, 40.0),
            ..Style::default()
        };
        let content = Content::Button {
            text: name.to_owned(),
            style: TextStyle::new(Face::Heading, 15.0),
        };
        buttons.push(tree.add(Some(root), NodeKind::Leaf, style, content, look).unwrap());
    }
    let content = Content::Label {
        text: "Готово".to_owned(),
        style: TextStyle::new(Face::Body, 14.0),
    };
    let label = tree
        .add(Some(root), NodeKind::Leaf, Style::default(), content, Look::default())
        .unwrap();
    tree.resize(400, 300);
    Fixture { tree, buttons, label }
}

fn frame() -> Vec<u32> {
    vec![0; 400 * 300]
}

#[test]
fn first_paint_covers_the_window_then_nothing_is_dirty() {
    let mut f = fixture();
    let mut pixels = frame();
    let rects = f.tree.paint(&mut pixels, 400).unwrap();
    assert_eq!(rects, vec![Rect::new(0, 0, 400, 300)]);
    assert!(!f.tree.is_dirty());
    assert!(f.tree.paint(&mut pixels, 400).unwrap().is_empty());
    // Text was drawn: the label row contains non-background pixels.
    let label = f.tree.rect(f.label).unwrap();
    let row = usize::try_from(label.y + 8).unwrap() * 400;
    assert!(pixels[row..row + 100].iter().any(|p| *p != grey(10).to_u32()));
}

#[test]
fn dpi_change_relayouts_widgets_and_ignores_non_finite_scale() {
    let mut f = fixture();
    let mut pixels = frame();
    f.tree.paint(&mut pixels, 400).unwrap();
    let button = f.tree.rect(f.buttons[0]).unwrap();
    assert_eq!(button.height, 40);

    let (proxy, receiver) = channel_pair::<u32>();
    proxy.window(WindowEvent::DpiChanged { scale: 2.0 });
    proxy.window(WindowEvent::DpiChanged { scale: f32::NAN });
    let mut screen = Screen::default();
    let mut app = Counter {
        label: f.label,
        seen: Vec::new(),
    };
    // Drop the last sender after queuing events so the loop drains both messages and exits.
    drop(proxy);
    let _ = run(&receiver, &mut f.tree, &mut app, &mut screen).unwrap();
    f.tree.paint(&mut pixels, 400).unwrap();
    assert_eq!(f.tree.rect(f.buttons[0]).unwrap().height, 40);
    assert_eq!(f.tree.scale(), 2.0);
}

#[test]
fn hover_damages_only_the_buttons_involved() {
    let mut f = fixture();
    let mut pixels = frame();
    f.tree.paint(&mut pixels, 400).unwrap();
    let first = f.tree.rect(f.buttons[0]).unwrap();
    assert!(f.tree.pointer_moved(5, first.y + 5));
    assert_eq!(f.tree.paint(&mut pixels, 400).unwrap(), vec![first]);
    assert_eq!(
        pixels[usize::try_from(first.y + 1).unwrap() * 400 + 1],
        grey(60).to_u32()
    );
    // Moving within the same button changes nothing.
    assert!(!f.tree.pointer_moved(6, first.y + 6));
    assert!(!f.tree.is_dirty());
    // Moving to the third button damages the first and the third only.
    let third = f.tree.rect(f.buttons[2]).unwrap();
    f.tree.pointer_moved(5, third.y + 5);
    let mut damaged = f.tree.paint(&mut pixels, 400).unwrap();
    damaged.sort_by_key(|r| r.y);
    assert_eq!(damaged, vec![first, third]);
}

#[test]
fn click_needs_press_and_release_on_the_same_widget() {
    let mut f = fixture();
    f.tree.paint(&mut frame(), 400).unwrap();
    let a = f.tree.rect(f.buttons[0]).unwrap();
    let b = f.tree.rect(f.buttons[1]).unwrap();
    assert_eq!(f.tree.pointer_button(true, 5, a.y + 5), None);
    assert_eq!(f.tree.pointer_button(false, 5, a.y + 5), Some(f.buttons[0]));
    assert_eq!(f.tree.pointer_button(true, 5, a.y + 5), None);
    assert_eq!(f.tree.pointer_button(false, 5, b.y + 5), None);
}

#[test]
fn text_change_relayouts_and_damages_old_and_new_area() {
    let mut f = fixture();
    let mut pixels = frame();
    f.tree.paint(&mut pixels, 400).unwrap();
    let before = f.tree.rect(f.label).unwrap();
    f.tree.set_text(f.label, "Готово · работает 12345 секунд").unwrap();
    let damaged = f.tree.paint(&mut pixels, 400).unwrap();
    let after = f.tree.rect(f.label).unwrap();
    // The label is stretched to the column width, so only its own row is repainted.
    assert_eq!(before, after);
    assert_eq!(damaged, vec![after]);
    // Unchanged text is free.
    f.tree.set_text(f.label, "Готово · работает 12345 секунд").unwrap();
    assert!(!f.tree.is_dirty());
}

#[test]
fn hidden_widget_takes_no_space_and_is_not_hit() {
    let mut f = fixture();
    f.tree.paint(&mut frame(), 400).unwrap();
    let second = f.tree.rect(f.buttons[1]).unwrap();
    f.tree.set_visible(f.buttons[0], false).unwrap();
    f.tree.paint(&mut frame(), 400).unwrap();
    assert_eq!(f.tree.rect(f.buttons[1]).unwrap().y, 0);
    assert!(f.tree.hit(5, 5) == Some(f.buttons[1]));
    assert!(second.y > 0);
}

#[test]
fn rendering_is_deterministic() {
    let mut a = frame();
    let mut b = frame();
    fixture().tree.paint(&mut a, 400).unwrap();
    fixture().tree.paint(&mut b, 400).unwrap();
    assert!(a == b);
}

#[derive(Default)]
struct Screen {
    presents: Vec<Vec<Rect>>,
}

impl Present for Screen {
    fn present(
        &mut self,
        frame: &[u32],
        stride: usize,
        width: u32,
        height: u32,
        rects: &[Rect],
    ) -> sse_core::Result<()> {
        assert_eq!(frame.len(), stride * usize::try_from(height).unwrap());
        assert_eq!(stride, usize::try_from(width).unwrap());
        self.presents.push(rects.to_vec());
        Ok(())
    }
}

struct Counter {
    label: WidgetId,
    seen: Vec<u32>,
}

impl App<u32> for Counter {
    fn message(&mut self, tree: &mut Tree, message: &Message<u32>, _: Option<WidgetId>) -> Flow {
        if matches!(message, Message::Window(WindowEvent::CloseRequested)) {
            return Flow::Exit;
        }
        if let Message::User(n) = message {
            self.seen.push(*n);
            tree.set_text(self.label, &format!("шаг {n}")).unwrap();
        }
        Flow::Continue
    }
}

#[test]
fn loop_wakes_on_worker_messages_batches_them_and_repaints_only_damage() {
    let Fixture { mut tree, label, .. } = fixture();
    let (proxy, receiver) = channel_pair::<u32>();
    proxy.window(WindowEvent::Resized {
        width: 400,
        height: 300,
    });
    let worker = proxy.clone();
    let handle = std::thread::spawn(move || {
        for n in 1..=5 {
            assert!(worker.send(n));
        }
        worker.window(WindowEvent::CloseRequested);
    });
    handle.join().unwrap();
    let mut app = Counter {
        label,
        seen: Vec::new(),
    };
    let mut screen = Screen::default();
    let stats = run(&receiver, &mut tree, &mut app, &mut screen).unwrap();
    assert_eq!(app.seen, vec![1, 2, 3, 4, 5]);
    // Everything was queued before the loop started: one wake, closed before painting.
    assert_eq!(stats.wakes, 1);
    assert!(screen.presents.is_empty());
}

#[test]
fn loop_presents_full_frame_then_small_damage() {
    let Fixture { mut tree, label, .. } = fixture();
    let (proxy, receiver) = channel_pair::<u32>();
    let mut app = Counter {
        label,
        seen: Vec::new(),
    };
    let mut screen = Screen::default();
    std::thread::spawn(move || {
        proxy.window(WindowEvent::Resized {
            width: 400,
            height: 300,
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        proxy.send(7);
        std::thread::sleep(std::time::Duration::from_millis(50));
        proxy.window(WindowEvent::CloseRequested);
    });
    let stats = run(&receiver, &mut tree, &mut app, &mut screen).unwrap();
    assert_eq!(stats.frames, 2);
    assert_eq!(screen.presents[0], vec![Rect::new(0, 0, 400, 300)]);
    let small: u32 = screen.presents[1].iter().map(|r| r.width * r.height).sum();
    assert!(small < 400 * 300 / 10, "label update repainted {small} px");
}
