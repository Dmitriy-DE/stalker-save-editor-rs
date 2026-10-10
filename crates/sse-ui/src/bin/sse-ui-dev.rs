//! Developer-only screenshot, benchmark, and CI layout commands for the native UI.

use sse_core::{Error, Result};
#[cfg(all(unix, not(target_os = "macos")))]
use sse_ui::event_loop::Present;
use sse_ui::event_loop::{channel_pair, App, Message};
use sse_ui::glyphs::Fonts;
use sse_ui::screens::shell::Shell;
use sse_ui::screens::style::rgb;
use sse_ui::screens::{AppMessage, ScreenId};
use sse_ui::theme::BG_BASE;
use sse_ui::widget::Tree;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct TemporaryDataDirectory(Option<PathBuf>);

impl TemporaryDataDirectory {
    fn initialize() -> Result<Self> {
        if std::env::var_os("STALKER_SAVE_EDITOR_DATA")
            .map(PathBuf::from)
            .is_some_and(|path| is_temporary_data_directory(&path))
        {
            return Ok(Self(None));
        }

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| Error::System(format!("system clock is before Unix epoch: {error}")))?
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("stalker-save-editor-dev-{}-{timestamp}", std::process::id()));
        std::fs::create_dir(&directory)
            .map_err(|error| Error::System(format!("could not create temporary developer data directory: {error}")))?;
        std::env::set_var("STALKER_SAVE_EDITOR_DATA", &directory);
        Ok(Self(Some(directory)))
    }
}

fn is_temporary_data_directory(path: &Path) -> bool {
    let Ok(temporary_root) = std::fs::canonicalize(std::env::temp_dir()) else {
        return false;
    };
    let Ok(directory) = std::fs::canonicalize(path) else {
        return false;
    };
    directory != temporary_root && directory.starts_with(temporary_root)
}

impl Drop for TemporaryDataDirectory {
    fn drop(&mut self) {
        if let Some(directory) = self.0.take() {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}

fn main() -> std::process::ExitCode {
    let _isolated_data = match TemporaryDataDirectory::initialize() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("sse-ui-dev: {error}");
            return std::process::ExitCode::FAILURE;
        }
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("--screenshot") => screenshot(&args),
        Some("--bench") => bench(&args),
        Some("--bench-save") => bench_save(&args),
        Some("--ci-budget") => ci_budget(),
        Some("--ci-i18n-buttons") => ci_i18n_buttons(),
        Some("--package-ee") => package_ee(&args),
        _ => Err(Error::Refused(
            "usage: sse-ui-dev --screenshot|--bench|--ci-budget|--ci-i18n-buttons|--package-ee <soc|cs|cop> <dir>"
                .to_owned(),
        )),
    };
    if let Err(error) = result {
        eprintln!("sse-ui-dev: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

fn package_ee(args: &[String]) -> Result<()> {
    if args.len() != 3 {
        return Err(Error::Refused("usage: --package-ee <soc|cs|cop> <dir>".to_owned()));
    }
    let (game, game_name, code) = match args.get(1).map(String::as_str) {
        Some("soc") => (
            sse_companion::bundled::Game::ShadowOfChernobyl,
            "Shadow of Chernobyl",
            "SOC",
        ),
        Some("cs") => (sse_companion::bundled::Game::ClearSky, "Clear Sky", "CS"),
        Some("cop") => (sse_companion::bundled::Game::CallOfPripyat, "Call of Pripyat", "COP"),
        _ => return Err(Error::Refused("--package-ee expects soc, cs, or cop".to_owned())),
    };
    let output = args
        .get(2)
        .map(PathBuf::from)
        .ok_or_else(|| Error::Refused("--package-ee requires an output directory".to_owned()))?;
    sse_companion::bundled::stage_enhanced_edition(
        &output,
        game,
        env!("CARGO_PKG_VERSION"),
        "Dmitriy-DE",
        &format!("Save Editor Companion — {game_name} Enhanced Edition"),
        &format!("Enhanced Edition Workshop package for S.T.A.L.K.E.R.: {game_name}."),
    )
    .map_err(|error| Error::System(error.to_string()))?;
    println!("Staged {code} Enhanced Edition package at {}", output.display());
    Ok(())
}

fn screenshot(args: &[String]) -> Result<()> {
    let path = args.get(1).ok_or_else(|| {
        Error::Refused(
            "usage: --screenshot OUT.png [WxH] [NAV] [--open SAVE] [--add-item] [--discover] [--press LABEL] [--settle] [--wait] [--select-first] [--achievements-fixture]"
                .to_owned(),
        )
    })?;
    let mut size: Option<(u32, u32)> = None;
    let mut nav: Option<usize> = None;
    let mut open_save = None;
    let mut add_item = false;
    let mut discover = false;
    let mut wait = false;
    let mut select_first = false;
    let mut achievements_fixture = false;
    let mut press: Option<&String> = None;
    let mut settle = false;
    let mut index = 2;
    while index < args.len() {
        match args.get(index).map(String::as_str) {
            Some("--add-item") => add_item = true,
            Some("--discover") => discover = true,
            Some("--wait") => wait = true,
            Some("--select-first") => select_first = true,
            Some("--achievements-fixture") => achievements_fixture = true,
            Some("--settle") => settle = true,
            Some("--press") => {
                index = index.saturating_add(1);
                press = Some(
                    args.get(index)
                        .ok_or_else(|| Error::Refused("--press requires a button label".to_owned()))?,
                );
            }
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
    let mut loader: Option<std::sync::mpsc::Receiver<Message<AppMessage>>> = None;
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
        loader = Some(receiver);
    }
    if let Some(id) = nav.and_then(|i| ScreenId::ALL.get(i)) {
        shell.open(&mut tree, *id)?;
    }
    if wait {
        // The screen's own background results (its list, loaded for the open save) come back as messages to it.
        if let (Some(receiver), Some(id)) = (loader.as_ref(), nav.and_then(|i| ScreenId::ALL.get(i))) {
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(60))
                .ok_or_else(|| Error::System("invalid screenshot wait deadline".to_owned()))?;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::System("timed out waiting for the screen's data".to_owned()));
                }
                let message = receiver
                    .recv_timeout(remaining)
                    .map_err(|error| Error::System(format!("screen data failed: {error}")))?;
                let finished = matches!(&message, Message::User(AppMessage::ToScreen(screen, _)) if screen == id);
                shell.message(&mut tree, &message, None);
                if finished {
                    // Results can come in more than one part (the first one before the game's folder is known):
                    // the frame waits until the screen has been quiet for a moment.
                    while let Ok(later) = receiver.recv_timeout(Duration::from_secs(2)) {
                        shell.message(&mut tree, &later, None);
                    }
                    break;
                }
            }
        }
    }
    if press.is_some() || settle {
        // A press or an opened screen starts background reads; their answers come back as messages, drained here.
        if let Some(label) = press {
            if !shell.press_button(&mut tree, label)? {
                return Err(Error::Refused(format!("the screen has no button {label}")));
            }
        }
        if let Some(receiver) = loader.as_ref() {
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(20))
                .ok_or_else(|| Error::System("invalid screenshot press deadline".to_owned()))?;
            // The read may answer in several messages (the library and the screen), so the window is timed, not counted.
            while Instant::now() < deadline {
                let Ok(message) = receiver.recv_timeout(Duration::from_millis(200)) else {
                    continue;
                };
                shell.message(&mut tree, &message, None);
            }
        }
    }
    if achievements_fixture {
        let fixture = Message::User(sse_ui::screens::services::achievement_fixture_message());
        shell.message(&mut tree, &fixture, None);
    }
    if select_first && !shell.select_first_fix(&mut tree)? {
        return Err(Error::Refused("the screen has no fix row to select".to_owned()));
    }
    if discover {
        // Background search (a save is open) is awaited: the result comes back to the screen as a message.
        if !shell.start_game_discovery(&mut tree)? {
            return Err(Error::Refused("the screen has no game search button".to_owned()));
        }
        if let Some(receiver) = loader.as_ref() {
            let deadline = Instant::now()
                .checked_add(Duration::from_secs(60))
                .ok_or_else(|| Error::System("invalid screenshot search deadline".to_owned()))?;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(Error::System("timed out waiting for the game search".to_owned()));
                }
                let message = receiver
                    .recv_timeout(remaining)
                    .map_err(|error| Error::System(format!("game search failed: {error}")))?;
                let finished = matches!(&message, Message::User(AppMessage::ToScreen(ScreenId::Games, _)));
                shell.message(&mut tree, &message, None);
                if finished {
                    break;
                }
            }
        }
    }
    shell.resize_window(&mut tree, width, height)?;
    if add_item && !shell.open_add_item(&mut tree)? {
        return Err(Error::Refused("the screen has no add-item button".to_owned()));
    }
    shell.load_art_now(&mut tree, &sse_app::paths::default_data_directory().join("art"))?;
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    tree.paint(&mut frame, stride)?;
    std::fs::write(path, encode_png(&frame, width, height)?).map_err(|error| Error::System(error.to_string()))
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct ClippedButton {
    language: String,
    screen: ScreenId,
    label: String,
}

fn ensure_buttons_fit(buttons: &[ClippedButton]) -> Result<()> {
    if buttons.is_empty() {
        return Ok(());
    }
    let details = buttons
        .iter()
        .map(|button| format!("{} / {:?}: {}", button.language, button.screen, button.label))
        .collect::<Vec<_>>()
        .join("\n");
    Err(Error::Refused(format!(
        "{} button labels exceed their available width:\n{details}",
        buttons.len()
    )))
}

fn ci_i18n_buttons() -> Result<()> {
    let previous = std::env::var_os("STALKER_EDITOR_LANG");
    let scan = (|| -> Result<Vec<ClippedButton>> {
        let mut clipped = Vec::new();
        for language in sse_ui::strings::LANGUAGES {
            std::env::set_var("STALKER_EDITOR_LANG", language);
            let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
            let mut shell = Shell::build(&mut tree, None)?;
            let (width, height) = (940_u32, 600_u32);
            tree.resize(width, height);
            let _ = shell.message(
                &mut tree,
                &Message::Window(sse_ui::event_loop::WindowEvent::Resized { width, height }),
                None,
            );
            let mut frame = vec![0_u32; 940 * 600];
            for id in ScreenId::ALL {
                shell.open(&mut tree, id)?;
                tree.paint(&mut frame, 940)?;
                clipped.extend(tree.ellipsized_button_labels()?.into_iter().map(|label| ClippedButton {
                    language: language.to_owned(),
                    screen: id,
                    label,
                }));
            }
        }
        Ok(clipped)
    })();
    if let Some(value) = previous.as_ref() {
        std::env::set_var("STALKER_EDITOR_LANG", value);
    } else {
        std::env::remove_var("STALKER_EDITOR_LANG");
    }
    let clipped = scan?;
    for button in &clipped {
        println!(
            "clipped_button language={} screen={:?} label={:?}",
            button.language, button.screen, button.label
        );
    }
    println!("ellipsized_buttons_total={}", clipped.len());
    ensure_buttons_fit(&clipped)
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

    // The window decodes its pictures on a worker thread, so they are not part of the startup budget. Load them
    // here, after the startup measurement, and measure the screen switches with them on screen.
    let art_started = Instant::now();
    shell.load_art_now(&mut tree, &sse_app::paths::default_data_directory().join("art"))?;
    let art_load = art_started.elapsed();

    // Screen hosts are lazy by design so startup stays below its own budget. Warm each host once,
    // then measure the steady-state switch that the cache is intended to make cheap. Shell::open
    // still calls shown() on every activation, so this does not bypass fresh screen state.
    for id in ScreenId::ALL {
        shell.open(&mut tree, id)?;
        tree.paint(&mut frame, 1280)?;
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
        println!("budget startup={startup:?} art_load={art_load:?} switch_worst={worst_switch:?} rss={rss_kib}KiB");
    }
    #[cfg(not(target_os = "linux"))]
    println!("budget startup={startup:?} art_load={art_load:?} switch_worst={worst_switch:?}");
    Ok(())
}

#[cfg(target_os = "linux")]
fn linux_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Times screen switches, first paints and pointer moves on a loaded save (the cost a user sees on a real save).
fn bench_save(args: &[String]) -> Result<()> {
    let save_path = args
        .get(1)
        .ok_or_else(|| Error::Refused("usage: sse-ui-dev --bench-save SAVE_PATH [WxH]".to_owned()))?;
    let (width, height) = args.get(2).map_or((1280, 800), |value| {
        let args = vec![String::new(), value.clone()];
        bench_size(&args)
    });
    let mut tree = Tree::new(Fonts::bundled()?, rgb(BG_BASE));
    let mut shell = Shell::build(&mut tree, None)?;
    let (proxy, receiver) = channel_pair::<AppMessage>();
    shell.set_proxy(proxy);
    tree.resize(width, height);
    let load_started = Instant::now();
    if !shell.open_save(&mut tree, Path::new(save_path))? {
        return Err(Error::Refused("could not start background save loading".to_owned()));
    }
    let deadline = Instant::now()
        .checked_add(Duration::from_secs(60))
        .unwrap_or_else(Instant::now);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let message = receiver
            .recv_timeout(remaining)
            .map_err(|error| Error::System(format!("save load failed: {error}")))?;
        let finished = matches!(&message, Message::User(AppMessage::ToScreen(ScreenId::Overview, _)));
        shell.message(&mut tree, &message, None);
        if finished {
            break;
        }
    }
    println!("load_ms {:.1}", load_started.elapsed().as_secs_f64() * 1000.0);
    let stride = usize::try_from(width).unwrap_or(0);
    let mut frame = vec![0_u32; stride.saturating_mul(usize::try_from(height).unwrap_or(0))];
    for id in ScreenId::ALL.iter().copied() {
        let started = Instant::now();
        shell.open(&mut tree, id)?;
        let open = started.elapsed();
        let started = Instant::now();
        tree.paint(&mut frame, stride)?;
        let first = started.elapsed();
        let mut repaint = Duration::ZERO;
        let mut small = Duration::ZERO;
        for _ in 0..5 {
            tree.damage_all();
            let started = Instant::now();
            tree.paint(&mut frame, stride)?;
            repaint = repaint.max(started.elapsed());
            // A one-button-sized damage rectangle: the cost here is mostly the walk over every node.
            tree.add_damage(sse_ui::raster::Rect {
                x: 20,
                y: 20,
                width: 16,
                height: 16,
            });
            let started = Instant::now();
            tree.paint(&mut frame, stride)?;
            small = small.max(started.elapsed());
        }
        println!(
            "screen {:?} open_ms {:.2} first_paint_ms {:.2} repaint_worst_ms {:.2} small_damage_worst_ms {:.3}",
            id,
            open.as_secs_f64() * 1000.0,
            first.as_secs_f64() * 1000.0,
            repaint.as_secs_f64() * 1000.0,
            small.as_secs_f64() * 1000.0
        );
    }
    shell.open(&mut tree, ScreenId::Overview)?;
    tree.paint(&mut frame, stride)?;
    let mut worst_move = Duration::ZERO;
    for step in 0..200_i32 {
        let span_x = i32::try_from(width).unwrap_or(1).max(1);
        let span_y = i32::try_from(height).unwrap_or(1).max(1);
        let x = 40_i32.saturating_add(step.saturating_mul(5).checked_rem(span_x).unwrap_or(0));
        let y = 40_i32.saturating_add(step.saturating_mul(3).checked_rem(span_y).unwrap_or(0));
        let started = Instant::now();
        tree.pointer_moved(x, y);
        worst_move = worst_move.max(started.elapsed());
    }
    println!("pointer_move_worst_ms {:.3}", worst_move.as_secs_f64() * 1000.0);
    Ok(())
}

fn bench_size(args: &[String]) -> (u32, u32) {
    args.get(1)
        .and_then(|value| value.split_once('x'))
        .and_then(|(width, height)| Some((width.parse().ok()?, height.parse().ok()?)))
        .unwrap_or((1280, 800))
}

fn encode_png(frame: &[u32], width: u32, height: u32) -> Result<Vec<u8>> {
    let mut rgba = Vec::with_capacity(frame.len().saturating_mul(4));
    for pixel in frame {
        let [alpha, red, green, blue] = pixel.to_be_bytes();
        rgba.extend_from_slice(&[red, green, blue, alpha]);
    }
    sse_codecs::png_encode::encode_rgba8(width, height, &rgba)
}

#[cfg(test)]
mod tests {
    use super::{bench_size, ensure_buttons_fit, is_temporary_data_directory, ClippedButton};
    use sse_ui::screens::ScreenId;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn benchmark_reads_requested_pixel_dimensions() {
        let args = vec!["--bench".to_owned(), "1920x1080".to_owned()];
        assert_eq!(bench_size(&args), (1920, 1080));
    }

    #[test]
    fn button_clipping_gate_rejects_any_clipped_label() {
        let button = ClippedButton {
            language: "en".to_owned(),
            screen: ScreenId::Settings,
            label: "A label that does not fit".to_owned(),
        };
        let message = ensure_buttons_fit(&[button])
            .err()
            .map_or_else(String::new, |error| error.to_string());

        assert!(message.contains("Settings"));
        assert!(message.contains("A label that does not fit"));
        assert!(ensure_buttons_fit(&[]).is_ok());
    }

    #[test]
    fn developer_data_override_is_only_trusted_inside_a_temporary_subdirectory() -> std::io::Result<()> {
        let temporary_root = std::env::temp_dir();
        assert!(!is_temporary_data_directory(&temporary_root));

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let directory = temporary_root.join(format!("sse-ui-dev-data-{}-{unique}", std::process::id()));
        std::fs::create_dir(&directory)?;
        assert!(is_temporary_data_directory(&directory));
        std::fs::remove_dir(directory)?;
        Ok(())
    }
}
