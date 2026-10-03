//! U1 shell: the editor frame (sidebar, header, content, status bar) on the own toolkit.
//!
//! `sse-shell` opens a window (X11). `sse-shell --screenshot out.png [WIDTHxHEIGHT]` renders headless and writes a
//! PNG without opening a window or playing sounds; `--bench` reports paint timings.

use sse_core::{Error, Result};
use sse_ui::event_loop::{channel_pair, App, Flow, Message, WindowEvent};
use sse_ui::glyphs::{Face, Fonts, TextStyle};
use sse_ui::layout::{Align, Edges, NodeKind, Size, Style};
use sse_ui::raster::Color;
use sse_ui::widget::{Content, Look, TextAlign, Tree, WidgetId};
use std::time::{Duration, Instant};

// StalkerTheme.cs dark palette; replaced by the X36 theme tokens.
const BG_BASE: u32 = 0x0C0D0A;
const BG_PANEL: u32 = 0x101311;
const BG_ELEVATED: u32 = 0x151814;
const BG_HOVER: u32 = 0x23261F;
const BORDER_SUBTLE: u32 = 0x242922;
const ACCENT: u32 = 0xD6A62D;
const TEXT_PRIMARY: u32 = 0xD8D2BE;
const TEXT_SECONDARY: u32 = 0xA29D90;
const TEXT_MUTED: u32 = 0x716F67;
const TEXT_KHAKI: u32 = 0xD8BA8C;

const SCREENS: [(&str, &str); 7] = [
    ("СЕЙВЫ", "Сохранения на этом компьютере"),
    ("ИНВЕНТАРЬ", "Предметы в рюкзаке и на поясе"),
    ("ДЕНЬГИ", "Деньги и торговля"),
    ("ИСПРАВЛЕНИЯ", "Исправления вылетов и ошибок игры"),
    ("КОМПАНЬОН", "Мод-компаньон в игре"),
    ("ПРОВЕРКА", "Проверка скриптов и модов"),
    ("НАСТРОЙКИ", "Язык, тема, обновления"),
];

fn rgb(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    Color::rgba(r, g, b, 255)
}

struct Shell {
    nav: Vec<WidgetId>,
    title: WidgetId,
    subtitle: WidgetId,
    status: WidgetId,
    selected: usize,
}

fn nav_look(selected: bool) -> Look {
    Look {
        fill: selected.then(|| rgb(BG_ELEVATED)),
        hover_fill: Some(rgb(BG_HOVER)),
        pressed_fill: Some(rgb(BG_ELEVATED)),
        accent_bar: selected.then(|| (rgb(ACCENT), 3.0)),
        text: rgb(if selected { TEXT_PRIMARY } else { TEXT_SECONDARY }),
        hover_text: Some(rgb(TEXT_PRIMARY)),
        align: TextAlign::Start,
        ..Look::default()
    }
}

fn build(tree: &mut Tree) -> Result<Shell> {
    let column = |grow: f32| Style {
        grow,
        align_items: Align::Stretch,
        ..Style::default()
    };
    let root = tree.add(
        None,
        NodeKind::Row,
        Style {
            align_items: Align::Stretch,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;

    let sidebar_style = Style {
        preferred: Size::new(232.0, 0.0),
        min: Size::new(232.0, 0.0),
        shrink: 0.0,
        padding: Edges {
            left: 0.0,
            top: 20.0,
            right: 0.0,
            bottom: 16.0,
        },
        gap: Size::new(0.0, 2.0),
        align_items: Align::Stretch,
        ..Style::default()
    };
    let sidebar_look = Look {
        fill: Some(rgb(BG_PANEL)),
        border: Some((rgb(BORDER_SUBTLE), 1.0)),
        ..Look::default()
    };
    let sidebar = tree.add(
        Some(root),
        NodeKind::Column,
        sidebar_style,
        Content::Panel,
        sidebar_look,
    )?;
    let brand = Style {
        padding: Edges {
            left: 20.0,
            top: 0.0,
            right: 20.0,
            bottom: 0.0,
        },
        ..Style::default()
    };
    tree.add(
        Some(sidebar),
        NodeKind::Leaf,
        brand,
        Content::Label {
            text: "S.T.A.L.K.E.R.".to_owned(),
            style: TextStyle::new(Face::Heading, 22.0),
        },
        Look {
            text: rgb(ACCENT),
            ..Look::default()
        },
    )?;
    tree.add(
        Some(sidebar),
        NodeKind::Leaf,
        Style {
            padding: Edges {
                left: 20.0,
                top: 0.0,
                right: 20.0,
                bottom: 18.0,
            },
            ..Style::default()
        },
        Content::Label {
            text: "Редактор сохранений".to_owned(),
            style: TextStyle::new(Face::Body, 14.0),
        },
        Look {
            text: rgb(TEXT_MUTED),
            ..Look::default()
        },
    )?;
    let mut nav = Vec::new();
    for (index, (name, _)) in SCREENS.iter().enumerate() {
        let style = Style {
            min: Size::new(0.0, 40.0),
            padding: Edges {
                left: 22.0,
                top: 0.0,
                right: 12.0,
                bottom: 0.0,
            },
            ..Style::default()
        };
        let id = tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            style,
            Content::Button {
                text: (*name).to_owned(),
                style: TextStyle::new(Face::Heading, 15.0),
            },
            nav_look(index == 0),
        )?;
        nav.push(id);
    }
    tree.add(
        Some(sidebar),
        NodeKind::Leaf,
        Style {
            grow: 1.0,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;
    tree.add(
        Some(sidebar),
        NodeKind::Leaf,
        brand,
        Content::Label {
            text: "2.0.0-dev · Rust".to_owned(),
            style: TextStyle::new(Face::Body, 13.0),
        },
        Look {
            text: rgb(TEXT_MUTED),
            ..Look::default()
        },
    )?;

    let main = tree.add(
        Some(root),
        NodeKind::Column,
        column(1.0),
        Content::Panel,
        Look::default(),
    )?;
    let header = tree.add(
        Some(main),
        NodeKind::Column,
        Style {
            padding: Edges {
                left: 32.0,
                top: 24.0,
                right: 32.0,
                bottom: 16.0,
            },
            align_items: Align::Stretch,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;
    let title = tree.add(
        Some(header),
        NodeKind::Leaf,
        Style::default(),
        Content::Label {
            text: SCREENS[0].0.to_owned(),
            style: TextStyle::new(Face::Heading, 30.0),
        },
        Look {
            text: rgb(TEXT_PRIMARY),
            ..Look::default()
        },
    )?;
    let subtitle = tree.add(
        Some(header),
        NodeKind::Leaf,
        Style::default(),
        Content::Label {
            text: SCREENS[0].1.to_owned(),
            style: TextStyle::new(Face::Body, 16.0),
        },
        Look {
            text: rgb(TEXT_SECONDARY),
            ..Look::default()
        },
    )?;
    let content = tree.add(
        Some(main),
        NodeKind::Column,
        Style {
            grow: 1.0,
            margin: Edges {
                left: 32.0,
                top: 0.0,
                right: 32.0,
                bottom: 20.0,
            },
            padding: Edges::all(20.0),
            gap: Size::new(0.0, 10.0),
            align_items: Align::Stretch,
            ..Style::default()
        },
        Content::Panel,
        Look {
            fill: Some(rgb(BG_ELEVATED)),
            border: Some((rgb(BORDER_SUBTLE), 1.0)),
            radius: 4.0,
            ..Look::default()
        },
    )?;
    let row_look = Look {
        fill: Some(rgb(BG_PANEL)),
        hover_fill: Some(rgb(BG_HOVER)),
        radius: 3.0,
        text: rgb(TEXT_KHAKI),
        ..Look::default()
    };
    for line in ["Зона · Свалка · 14:32", "Бар «100 рентген» · 09:05", "Янтарь · 22:47"]
    {
        tree.add(
            Some(content),
            NodeKind::Leaf,
            Style {
                min: Size::new(0.0, 44.0),
                padding: Edges {
                    left: 16.0,
                    top: 0.0,
                    right: 16.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Button {
                text: line.to_owned(),
                style: TextStyle::new(Face::Body, 16.0),
            },
            row_look,
        )?;
    }
    let status = tree.add(
        Some(main),
        NodeKind::Leaf,
        Style {
            min: Size::new(0.0, 28.0),
            padding: Edges {
                left: 32.0,
                top: 0.0,
                right: 32.0,
                bottom: 0.0,
            },
            ..Style::default()
        },
        Content::Label {
            text: "Готово".to_owned(),
            style: TextStyle::new(Face::Body, 13.0),
        },
        Look {
            fill: Some(rgb(BG_PANEL)),
            text: rgb(TEXT_MUTED),
            ..Look::default()
        },
    )?;
    Ok(Shell {
        nav,
        title,
        subtitle,
        status,
        selected: 0,
    })
}

/// Worker message: seconds since start, from a timer thread.
struct Tick(u64);

impl Shell {
    fn select(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        if index == self.selected {
            return Ok(());
        }
        if let Some(old) = self.nav.get(self.selected) {
            tree.set_look(*old, nav_look(false))?;
        }
        if let Some(new) = self.nav.get(index) {
            tree.set_look(*new, nav_look(true))?;
        }
        if let Some((name, line)) = SCREENS.get(index) {
            tree.set_text(self.title, name)?;
            tree.set_text(self.subtitle, line)?;
        }
        self.selected = index;
        Ok(())
    }
}

impl App<Tick> for Shell {
    fn message(&mut self, tree: &mut Tree, message: &Message<Tick>, clicked: Option<WidgetId>) -> Flow {
        if let Some(index) = clicked.and_then(|id| self.nav.iter().position(|nav| *nav == id)) {
            let _ = self.select(tree, index);
        }
        match message {
            Message::User(Tick(seconds)) => {
                let _ = tree.set_text(self.status, &format!("Готово · работает {seconds} с"));
            }
            Message::Window(WindowEvent::Key {
                pressed: true, keysym, ..
            }) => {
                // Up/Down walk the navigation, Escape quits.
                let count = self.nav.len();
                match keysym {
                    0xff1b => return Flow::Exit,
                    0xff52 => {
                        let _ = self.select(tree, self.selected.checked_sub(1).unwrap_or(count.saturating_sub(1)));
                    }
                    0xff54 => {
                        let next = self.selected.saturating_add(1);
                        let _ = self.select(tree, if next >= count { 0 } else { next });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Flow::Continue
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--screenshot") => screenshot(&args),
        Some("--bench") => bench(),
        _ => window(),
    };
    if let Err(error) = result {
        eprintln!("sse-shell: {error}");
        std::process::exit(1);
    }
}

fn parse_size(text: Option<&String>) -> (u32, u32) {
    text.and_then(|value| {
        let (w, h) = value.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    })
    .unwrap_or((1280, 800))
}

fn screenshot(args: &[String]) -> Result<()> {
    let path = args
        .get(1)
        .ok_or_else(|| Error::Refused("usage: --screenshot OUT.png [WxH] [NAV]".to_owned()))?;
    let (width, height) = parse_size(args.get(2));
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = build(&mut tree)?;
    if let Some(index) = args.get(3).and_then(|value| value.parse().ok()) {
        shell.select(&mut tree, index)?;
    }
    tree.resize(width, height);
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    tree.paint(&mut frame, stride)?;
    std::fs::write(path, encode_png(&frame, width, height)).map_err(|e| Error::System(e.to_string()))
}

fn bench() -> Result<()> {
    let (width, height) = (1280, 800);
    let started = Instant::now();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = build(&mut tree)?;
    tree.resize(width, height);
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    tree.paint(&mut frame, stride)?;
    let first = started.elapsed();
    let rounds = 200_usize;
    let mut pixels = 0_u64;
    let switching = Instant::now();
    for round in 0..rounds {
        shell.select(&mut tree, round.checked_rem(SCREENS.len()).unwrap_or(0))?;
        for rect in tree.paint(&mut frame, stride)? {
            pixels = pixels.saturating_add(u64::from(rect.width).saturating_mul(u64::from(rect.height)));
        }
    }
    let per_switch = switching
        .elapsed()
        .checked_div(u32::try_from(rounds).unwrap_or(1))
        .unwrap_or_default();
    let hover = Instant::now();
    for step in 0..rounds {
        let y = i32::try_from(step.checked_rem(400).unwrap_or(0))
            .unwrap_or(0)
            .saturating_add(80);
        tree.pointer_moved(100, y);
        tree.paint(&mut frame, stride)?;
    }
    let per_hover = hover
        .elapsed()
        .checked_div(u32::try_from(rounds).unwrap_or(1))
        .unwrap_or_default();
    println!("first frame (fonts + layout + full paint) {first:?}");
    println!(
        "screen switch repaint {per_switch:?}, {} px avg",
        pixels.checked_div(200).unwrap_or(0)
    );
    println!("hover move repaint {per_hover:?}");
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn window() -> Result<()> {
    let (proxy, receiver) = channel_pair::<Tick>();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = build(&mut tree)?;
    let (width, height) = (1280_u16, 800_u16);
    let mut backend =
        sse_ui::x11_window::X11Window::open("S.T.A.L.K.E.R. Save Editor", width, height, BG_BASE, proxy.clone())?;
    tree.resize(u32::from(width), u32::from(height));
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));
        if !proxy.send(Tick(started.elapsed().as_secs())) {
            return;
        }
    });
    let stats = sse_ui::event_loop::run(&receiver, &mut tree, &mut shell, &mut backend)?;
    eprintln!("wakes {} frames {} pixels {}", stats.wakes, stats.frames, stats.pixels);
    Ok(())
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn window() -> Result<()> {
    let _ = (channel_pair::<Tick>, Duration::from_secs, BG_BASE);
    Err(Error::Refused(
        "window backend for this platform comes in U3/U4; use --screenshot".to_owned(),
    ))
}

/// Minimal PNG (RGBA, stored deflate) until X37 lands its encoder.
fn encode_png(frame: &[u32], width: u32, height: u32) -> Vec<u8> {
    let mut raw = Vec::new();
    let row = usize::try_from(width).unwrap_or(0);
    for line in frame.chunks(row.max(1)) {
        raw.push(0);
        for pixel in line {
            let [a, r, g, b] = pixel.to_be_bytes();
            raw.extend_from_slice(&[r, g, b, a]);
        }
    }
    let mut zlib = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65_535).peekable();
    while let Some(block) = chunks.next() {
        zlib.push(u8::from(chunks.peek().is_none()));
        let len = u16::try_from(block.len()).unwrap_or(u16::MAX);
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(block);
    }
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in &raw {
        a = a.wrapping_add(u32::from(*byte)).checked_rem(65_521).unwrap_or(0);
        b = b.wrapping_add(a).checked_rem(65_521).unwrap_or(0);
    }
    zlib.extend_from_slice(&(b.wrapping_shl(16) | a).to_be_bytes());
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut header = Vec::new();
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    for (kind, body) in [(b"IHDR", header), (b"IDAT", zlib), (b"IEND", Vec::new())] {
        out.extend_from_slice(&u32::try_from(body.len()).unwrap_or(0).to_be_bytes());
        let mut tagged = kind.to_vec();
        tagged.extend_from_slice(&body);
        out.extend_from_slice(&tagged);
        out.extend_from_slice(&sse_codecs::crc32::crc32(&tagged).to_be_bytes());
    }
    out
}
