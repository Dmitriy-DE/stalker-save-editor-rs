//! U1 shell: the editor frame (sidebar, header, content, status bar) on the own toolkit.
//!
//! `sse-shell` opens a window (X11). `sse-shell --screenshot out.png [WIDTHxHEIGHT] [NAV] [--open SAVE]` renders
//! headless and writes a PNG without opening a window or playing sounds; `--bench` reports paint timings.

use sse_core::{Error, Result};
#[cfg(all(unix, not(target_os = "macos")))]
use sse_ui::event_loop::Present;
use sse_ui::event_loop::{channel_pair, App, Message};
use sse_ui::glyphs::Fonts;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::style::rgb;
use sse_ui::screens::AppMessage;
use sse_ui::screens::ScreenId;
use sse_ui::theme::BG_BASE;
use sse_ui::widget::Tree;
use std::time::Duration;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--screenshot") => screenshot(&args),
        Some("--bench") => bench(&args),
        Some("--ci-budget") => ci_budget(),
        Some("--companion") => companion_command(&args),
        _ => window(),
    };
    if let Err(error) = result {
        eprintln!("sse-shell: {error}");
        std::process::exit(1);
    }
}

fn companion_command(args: &[String]) -> Result<()> {
    let directory = args
        .get(1)
        .ok_or_else(|| Error::Refused("usage: --companion DIR COMMAND [ARG ...]".to_owned()))?;
    let command = args
        .get(2)
        .ok_or_else(|| Error::Refused("usage: --companion DIR COMMAND [ARG ...]".to_owned()))?;
    let arguments = args.iter().skip(3).map(String::as_str).collect::<Vec<_>>();
    let client = sse_companion::protocol::CompanionClient::new(std::path::PathBuf::from(directory));
    let reply = client
        .send(command, &arguments, Duration::from_secs(3))
        .map_err(|error| Error::System(format!("companion: {error}")))?;
    println!("{:?} {}", reply.status, reply.text);
    Ok(())
}

fn screenshot(args: &[String]) -> Result<()> {
    let path = args
        .get(1)
        .ok_or_else(|| Error::Refused("usage: --screenshot OUT.png [WxH] [NAV] [--open SAVE]".to_owned()))?;
    let mut size: Option<(u32, u32)> = None;
    let mut nav: Option<usize> = None;
    let mut open_save = None;
    let mut index = 2;
    while index < args.len() {
        match args.get(index).map(String::as_str) {
            Some("--open") => {
                index = index.saturating_add(1);
                open_save = Some(
                    args.get(index)
                        .ok_or_else(|| Error::Refused("--open requires a save path".to_owned()))?,
                );
            }
            Some(value) if value.contains('x') && size.is_none() => {
                let (width, height) = value
                    .split_once('x')
                    .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
                    .ok_or_else(|| Error::Refused("screenshot size must be WxH".to_owned()))?;
                size = Some((width, height));
            }
            Some(value) if nav.is_none() => {
                nav = Some(
                    value
                        .parse::<usize>()
                        .map_err(|_| Error::Refused("screenshot NAV must be a screen index".to_owned()))?,
                );
            }
            Some(_) => return Err(Error::Refused("unexpected screenshot argument".to_owned())),
            None => return Err(Error::Refused("invalid screenshot arguments".to_owned())),
        }
        index = index.saturating_add(1);
    }
    let (width, height) = size.unwrap_or((1280, 800));
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, None)?;
    if let Some(save_path) = open_save {
        let (proxy, receiver) = channel_pair::<AppMessage>();
        shell.set_proxy(proxy);
        if !shell.open_save(&mut tree, std::path::Path::new(save_path))? {
            return Err(Error::Refused("could not start background save loading".to_owned()));
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(15))
            .ok_or_else(|| Error::System("invalid screenshot save-load deadline".to_owned()))?;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::System("timed out loading screenshot save".to_owned()));
            }
            let message = receiver
                .recv_timeout(remaining)
                .map_err(|error| Error::System(format!("screenshot save load failed: {error}")))?;
            let finished = matches!(&message, Message::User(AppMessage::ToScreen(ScreenId::Overview, _)));
            shell.message(&mut tree, &message, None);
            if finished {
                if shell.app().current_save().is_none() {
                    return Err(Error::Damaged("screenshot save could not be read".to_owned()));
                }
                break;
            }
        }
    }
    if let Some(id) = nav.and_then(|i| ScreenId::ALL.get(i)) {
        shell.open(&mut tree, *id)?;
    }
    tree.resize(width, height);
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    tree.paint(&mut frame, stride)?;
    std::fs::write(path, encode_png(&frame, width, height)).map_err(|e| Error::System(e.to_string()))
}

fn bench(args: &[String]) -> Result<()> {
    let (width, height) = bench_size(args);
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    #[cfg(all(unix, not(target_os = "macos")))]
    let (proxy, _receiver, mut backend) = if std::env::var_os("DISPLAY").is_some() {
        let (proxy, receiver) = channel_pair::<AppMessage>();
        let x_width =
            u16::try_from(width).map_err(|_| Error::Refused("benchmark width exceeds X11 limit".to_owned()))?;
        let x_height =
            u16::try_from(height).map_err(|_| Error::Refused("benchmark height exceeds X11 limit".to_owned()))?;
        let backend = sse_ui::x11_window::X11Window::open(
            "S.T.A.L.K.E.R. Save Editor benchmark",
            x_width,
            x_height,
            BG_BASE,
            proxy.clone(),
        )?;
        (Some(proxy), Some(receiver), Some(backend))
    } else {
        (None, None, None)
    };
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let (proxy, _receiver, _backend): (Option<sse_ui::event_loop::Proxy<AppMessage>>, Option<()>, Option<()>) =
        (None, None, None);
    let mut shell = Shell::build(&mut tree, proxy)?;
    tree.resize(width, height);
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    let started = Instant::now();
    tree.paint(&mut frame, stride)?;
    let first = started.elapsed();
    let rounds = 200_usize;
    let mut pixels = 0_u64;
    let switching = Instant::now();
    for round in 0..rounds {
        let id = ScreenId::ALL
            .get(round.checked_rem(ScreenId::ALL.len()).unwrap_or(0))
            .copied();
        shell.open(&mut tree, id.unwrap_or(ScreenId::Overview))?;
        let damage = tree.paint(&mut frame, stride)?;
        for rect in &damage {
            pixels = pixels.saturating_add(u64::from(rect.width).saturating_mul(u64::from(rect.height)));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        if let Some(window) = backend.as_mut() {
            window.present(&frame, stride, width, height, &damage)?;
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
    #[cfg(all(unix, not(target_os = "macos")))]
    let mode = match backend.as_ref() {
        Some(window) if window.uses_mit_shm() => "X11 MIT-SHM",
        Some(_) => "X11 PutImage fallback",
        None => "headless",
    };
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    let mode = "headless";
    println!("first frame (fonts + layout + full paint) {first:?} at {width}x{height} ({mode})");
    println!(
        "screen switch repaint {per_switch:?}, {} px avg",
        pixels.checked_div(200).unwrap_or(0)
    );
    println!("hover move repaint {per_hover:?}");
    Ok(())
}

fn ci_budget() -> Result<()> {
    const START_BUDGET: Duration = Duration::from_millis(200);
    const SWITCH_BUDGET: Duration = Duration::from_millis(16);
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    const RSS_BUDGET_KIB: u64 = 30 * 1024;

    let started = Instant::now();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, None)?;
    tree.resize(1280, 860);
    // Budget the application state/layout startup separately from the presentation framebuffer.
    // Native presenters own their buffers; counting this 4.2 MiB benchmark Vec as idle application
    // RSS made the measurement dependent on benchmark resolution rather than editor state.
    let mut frame = vec![0_u32; 1280 * 860];
    tree.paint(&mut frame, 1280)?;
    let startup = started.elapsed();
    if startup > START_BUDGET {
        return Err(Error::Refused(format!(
            "startup budget exceeded: {startup:?} > {START_BUDGET:?}"
        )));
    }

    let mut worst_switch = Duration::ZERO;
    for id in ScreenId::ALL {
        let switch_started = Instant::now();
        shell.open(&mut tree, id)?;
        tree.paint(&mut frame, 1280)?;
        worst_switch = worst_switch.max(switch_started.elapsed());
    }
    if worst_switch > SWITCH_BUDGET {
        return Err(Error::Refused(format!(
            "screen-switch budget exceeded: {worst_switch:?} > {SWITCH_BUDGET:?}"
        )));
    }

    // Drop the synthetic CI framebuffer before measuring idle retained-state RSS.
    drop(frame);
    #[cfg(target_os = "linux")]
    if let Some(rss_kib) = linux_rss_kib() {
        if rss_kib > RSS_BUDGET_KIB {
            return Err(Error::Refused(format!(
                "idle RSS budget exceeded: {rss_kib} KiB > {RSS_BUDGET_KIB} KiB"
            )));
        }
        println!("budget startup={startup:?} switch_worst={worst_switch:?} rss={rss_kib}KiB");
    }
    #[cfg(not(target_os = "linux"))]
    println!("budget startup={startup:?} switch_worst={worst_switch:?}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn bench_size(args: &[String]) -> (u32, u32) {
    args.get(1)
        .and_then(|value| value.split_once('x'))
        .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
        .unwrap_or((1280, 800))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn window() -> Result<()> {
    let (proxy, receiver) = channel_pair::<AppMessage>();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, Some(proxy.clone()))?;
    let (width, height) = (1280_u16, 800_u16);
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        match sse_ui::wayland_window::WaylandWindow::open(
            "S.T.A.L.K.E.R. Save Editor",
            u32::from(width),
            u32::from(height),
            proxy.clone(),
        ) {
            Ok(mut backend) => {
                tree.resize(u32::from(width), u32::from(height));
                let started = Instant::now();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(500));
                    if !proxy.send(AppMessage::Tick(started.elapsed().as_secs())) {
                        return;
                    }
                });
                let stats = sse_ui::event_loop::run(&receiver, &mut tree, &mut shell, &mut backend)?;
                eprintln!(
                    "Wayland wakes {} frames {} pixels {}",
                    stats.wakes, stats.frames, stats.pixels
                );
                return Ok(());
            }
            Err(error) => eprintln!("Wayland unavailable ({error}); falling back to X11/XWayland"),
        }
    }
    let mut backend =
        sse_ui::x11_window::X11Window::open("S.T.A.L.K.E.R. Save Editor", width, height, BG_BASE, proxy.clone())?;
    tree.resize(u32::from(width), u32::from(height));
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if !proxy.send(AppMessage::Tick(started.elapsed().as_secs())) {
            return;
        }
    });
    let stats = sse_ui::event_loop::run(&receiver, &mut tree, &mut shell, &mut backend)?;
    eprintln!("wakes {} frames {} pixels {}", stats.wakes, stats.frames, stats.pixels);
    Ok(())
}

#[cfg(windows)]
fn window() -> Result<()> {
    let (proxy, receiver) = channel_pair::<AppMessage>();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, Some(proxy.clone()))?;
    let (width, height) = (1280_u32, 800_u32);
    let mut backend =
        sse_ui::win32_window::Win32Presenter::open("S.T.A.L.K.E.R. Save Editor", width, height, proxy.clone())?;
    tree.resize(width, height);
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if !proxy.send(AppMessage::Tick(started.elapsed().as_secs())) {
            return;
        }
    });
    let stats = sse_ui::event_loop::run(&receiver, &mut tree, &mut shell, &mut backend)?;
    eprintln!("wakes {} frames {} pixels {}", stats.wakes, stats.frames, stats.pixels);
    Ok(())
}

#[cfg(target_os = "macos")]
fn window() -> Result<()> {
    let (proxy, receiver) = channel_pair::<AppMessage>();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, Some(proxy.clone()))?;
    let (width, height) = (1280_u32, 800_u32);
    let mut backend =
        sse_ui::macos_window::MacPresenter::open("S.T.A.L.K.E.R. Save Editor", width, height, proxy.clone())?;
    tree.resize(width, height);
    let started = Instant::now();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(500));
        if !proxy.send(AppMessage::Tick(started.elapsed().as_secs())) {
            return;
        }
    });
    let stats = sse_ui::event_loop::run(&receiver, &mut tree, &mut shell, &mut backend)?;
    eprintln!("wakes {} frames {} pixels {}", stats.wakes, stats.frames, stats.pixels);
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::bench_size;

    #[test]
    fn benchmark_reads_requested_pixel_dimensions() {
        let args = vec!["--bench".to_owned(), "1920x1080".to_owned()];
        assert_eq!(bench_size(&args), (1920, 1080));
    }
}
