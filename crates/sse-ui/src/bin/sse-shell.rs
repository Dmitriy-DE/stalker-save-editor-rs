//! U1 shell: the editor frame (sidebar, header, content, status bar) on the own toolkit.
//!
//! `sse-shell` opens the native editor window. Developer screenshots and CI modes live in `sse-ui-dev`.

use sse_core::{Error, Result};
use sse_ui::event_loop::channel_pair;
use sse_ui::glyphs::Fonts;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::style::rgb;
use sse_ui::screens::AppMessage;
use sse_ui::theme::BG_BASE;
use sse_ui::widget::Tree;
use std::time::Duration;
use std::time::Instant;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(exit) = sse_steam::worker::run_if_worker(&args) {
        return exit;
    }
    let result = match args.first().map(String::as_str) {
        Some("--companion") => companion_command(&args),
        Some("--hotkey-helper") => hotkey_helper(),
        _ => {
            sse_app::diagnostics::install_crash_reporter();
            sse_app::diagnostics::info(&format!(
                "start {} on {} {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
            window()
        }
    };
    if let Err(error) = result {
        sse_app::diagnostics::error(&format!("sse-shell: {error}"));
        eprintln!("sse-shell: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

fn hotkey_helper() -> Result<()> {
    use std::io::{BufRead, Write};

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut bindings = Vec::new();
    loop {
        let mut line = String::new();
        let read = input
            .read_line(&mut line)
            .map_err(|error| Error::System(format!("hotkey helper stdin: {error}")))?;
        if read == 0 {
            return Ok(());
        }
        let line = line.trim();
        if line == "start" {
            break;
        }
        let mut fields = line.split_whitespace();
        if fields.next() != Some("bind") {
            return Err(Error::Refused(format!("unexpected hotkey helper command: {line}")));
        }
        let id = fields
            .next()
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(|| Error::Refused(format!("invalid hotkey helper ID: {line}")))?;
        let modifiers = fields
            .next()
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| *value <= 7)
            .ok_or_else(|| Error::Refused(format!("invalid hotkey helper modifiers: {line}")))?;
        let key = fields
            .next()
            .and_then(|value| value.as_bytes().first().copied())
            .filter(u8::is_ascii_uppercase)
            .ok_or_else(|| Error::Refused(format!("invalid hotkey helper key: {line}")))?;
        if fields.next().is_some() {
            return Err(Error::Refused(format!("unexpected hotkey helper fields: {line}")));
        }
        bindings.push(sse_sys::hotkeys::HotkeyBinding {
            id,
            key,
            control: modifiers & 1 != 0,
            alt: modifiers & 2 != 0,
            shift: modifiers & 4 != 0,
        });
    }
    drop(input);

    let mut session = match sse_sys::hotkeys::HotkeySession::open(&bindings) {
        Ok(session) => session,
        Err(error) => {
            println!("error {}", error.to_string().replace(['\r', '\n'], " "));
            let _ = std::io::stdout().flush();
            return Err(error);
        }
    };
    println!("ready");
    std::io::stdout()
        .flush()
        .map_err(|error| Error::System(format!("hotkey helper stdout: {error}")))?;

    let (stop_tx, stop_rx) = std::sync::mpsc::channel();
    let _reader = std::thread::Builder::new()
        .name("hotkey-helper-stdin".to_owned())
        .spawn(move || {
            let stdin = std::io::stdin();
            let mut input = stdin.lock();
            loop {
                let mut line = String::new();
                match input.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) if line.trim() == "stop" => break,
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
            let _ = stop_tx.send(());
        })
        .map_err(|error| Error::System(format!("hotkey helper reader: {error}")))?;

    loop {
        if stop_rx.try_recv().is_ok() {
            return Ok(());
        }
        if let Some(id) = session.poll(Duration::from_millis(50))? {
            println!("pressed {id}");
            std::io::stdout()
                .flush()
                .map_err(|error| Error::System(format!("hotkey helper stdout: {error}")))?;
        }
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

#[cfg(all(unix, not(target_os = "macos")))]
fn window() -> Result<()> {
    let (proxy, receiver) = channel_pair::<AppMessage>();
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, Some(proxy.clone()))?;
    let (width, height) = (1280_u16, 800_u16);
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        #[cfg(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
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
