//! S5: companion, achievements, Steam Cloud, and editor updates.

use super::games::GameTarget;
use super::saves::{build_list_side, show_list_row, side_column_style, sync_side_widths, ListRow, ListSide, Workspace};
use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::{Message, WindowEvent};
use crate::layout::{NodeKind, Size, Style};
use crate::widget::{Content, Look, WidgetId};
use sse_core::Result;
use sse_steam::api::{Achievement, CloudFile, SteamApi};
use sse_steam::cloud::XRaySaveFormatVerifier;
use sse_steam::worker::WorkerSteamApi;
use sse_steam::{cloud::PreparedEdit, cloud::SteamCloudWriteTransaction, cloud::WriteStatus};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn t_in<'a>(language: &str, key: &'a str) -> &'a str {
    crate::strings::t_in(language, key)
}

fn t(key: &str) -> &str {
    t_in(crate::strings::current_language(), key)
}

fn tr_in(language: &str, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    crate::strings::tr_in(Some(language), key, args)
}

fn tr(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    tr_in(crate::strings::current_language(), key, args)
}

/// Screens implemented by the S5 services package.
#[must_use]
pub(crate) fn screens(backup_workspace: Workspace) -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Companion::default()),
        Box::new(Achievements::default()),
        Box::new(Cloud::with_backup_workspace(backup_workspace)),
        Box::new(Updates::default()),
    ]
}

/// The save-game family (`sse_storage` candidate id) of a selected game key, editions included.
fn save_family(game: &str) -> Option<&'static str> {
    match game {
        "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => Some("soc"),
        "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => Some("clear_sky"),
        "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => Some("cop"),
        "s2" | "stalker2" => Some("stalker2"),
        _ => None,
    }
}

/// True when `path` lies inside a save folder that the locator assigns to `family`.
fn in_game_directory(
    path: &Path,
    family: Option<&str>,
    directories: &[sse_storage::discovery::SaveDirectoryCandidate],
) -> bool {
    family.is_some_and(|family| {
        directories
            .iter()
            .any(|candidate| candidate.game_id == family && path.starts_with(&candidate.directory_path))
    })
}

fn app_id(game: &str) -> Option<u32> {
    match game {
        "soc" | "stalker-soc" => Some(4_500),
        "cs" | "clear_sky" | "stalker-cs" => Some(20_510),
        "cop" | "stalker-cop" => Some(41_700),
        "soc-ee" | "stalker-soc-ee" => Some(2_427_410),
        "cs-ee" | "stalker-cs-ee" => Some(2_427_420),
        "cop-ee" | "stalker-cop-ee" => Some(2_427_430),
        "s2" | "stalker2" => Some(sse_steam::discovery::STALKER_2_APP_ID),
        _ => None,
    }
}

fn hotkey_label(language: &str, action: sse_companion::hotkeys::HotkeyAction) -> &'static str {
    t_in(
        language,
        match action {
            sse_companion::hotkeys::HotkeyAction::Heal => "Лечение",
            sse_companion::hotkeys::HotkeyAction::RepairEquipped => "Ремонт экипировки",
            sse_companion::hotkeys::HotkeyAction::Mark => "Сохранить отметку",
            sse_companion::hotkeys::HotkeyAction::JumpLast => "Перейти к последней отметке",
            sse_companion::hotkeys::HotkeyAction::QuickSave => "Быстрое сохранение",
        },
    )
}

fn xray_game(game: &str) -> Option<sse_companion::bundled::Game> {
    match game {
        "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => Some(sse_companion::bundled::Game::ShadowOfChernobyl),
        "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => Some(sse_companion::bundled::Game::ClearSky),
        "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => Some(sse_companion::bundled::Game::CallOfPripyat),
        _ => None,
    }
}

fn workshop_state_text(state: sse_companion::workshop::WorkshopInstallState) -> &'static str {
    use sse_companion::workshop::WorkshopInstallState as State;
    t(match state {
        State::NotPublished => "Пакет Steam Workshop ещё не опубликован",
        State::NotSteamInstall => "Папка игры вне Steam; доступна локальная установка компаньона",
        State::NotSubscribed => "Не подписан на пакет Steam Workshop",
        State::UpToDate => "Пакет Steam Workshop установлен и актуален",
        State::Outdated => "Пакет Steam Workshop устарел или повреждён",
    })
}

fn open_workshop_url(url: &str) -> std::result::Result<(), String> {
    #[cfg(target_os = "windows")]
    let mut command = Command::new({
        let system_root = std::env::var_os("SystemRoot")
            .ok_or_else(|| "SystemRoot is unavailable; cannot locate trusted rundll32.exe".to_owned())?;
        let helper = PathBuf::from(system_root).join("System32/rundll32.exe");
        resolve_trusted_helper(&[helper])?
    });
    #[cfg(target_os = "macos")]
    let mut command = Command::new(resolve_trusted_helper(&[PathBuf::from("/usr/bin/open")])?);
    #[cfg(target_os = "linux")]
    let mut command = Command::new(resolve_trusted_helper(&[
        PathBuf::from("/usr/bin/xdg-open"),
        PathBuf::from("/bin/xdg-open"),
    ])?);
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    return Err("Opening Steam Workshop links is unsupported on this platform".to_owned());
    #[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
    {
        #[cfg(target_os = "windows")]
        command.args(["url.dll,FileProtocolHandler", url]);
        #[cfg(not(target_os = "windows"))]
        command.arg(url);
        command.spawn().map(|_| ()).map_err(|error| error.to_string())
    }
}

fn resolve_trusted_helper(candidates: &[PathBuf]) -> std::result::Result<PathBuf, String> {
    candidates
        .iter()
        .find(|candidate| candidate.is_file())
        .cloned()
        .ok_or_else(|| "No trusted system browser helper was found".to_owned())
}

#[cfg(test)]
mod workshop_opener_tests {
    use super::resolve_trusted_helper;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn missing_system_helper_is_refused_with_a_clear_error() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let missing = std::env::temp_dir().join(format!("sse-no-workshop-helper-{nonce}"));

        let result = resolve_trusted_helper(&[missing]);
        assert!(
            matches!(
                &result,
                Err(error) if error.contains("No trusted system browser helper was found")
            ),
            "missing helper should return a clear refusal: {result:?}"
        );
    }
}

fn companion_root(game: &str, root: &Path) -> std::result::Result<PathBuf, String> {
    if xray_game(game).is_some() {
        return Ok(root.to_path_buf());
    }
    if matches!(game, "s2" | "stalker2") {
        for rel in [
            "Stalker2/Binaries/Win64/ue4ss/Mods",
            "Binaries/Win64/ue4ss/Mods",
            "ue4ss/Mods",
        ] {
            let path = root.join(rel);
            if path.is_dir() {
                return Ok(path);
            }
        }
        return Err(t("Не найдена папка UE4SS Mods для S.T.A.L.K.E.R. 2").to_owned());
    }
    Err(t("Компаньон для выбранного издания не поддерживается").to_owned())
}

fn installed_version(root: &Path) -> Option<String> {
    let bytes = std::fs::read(root.join(".save-editor-companion/manifest.json")).ok()?;
    let mut reader = sse_codecs::json::Reader::new(&bytes);
    while let Ok(Some(event)) = reader.next_event() {
        if let sse_codecs::json::Event::Key(key) = event {
            if key.as_str() == "version" {
                return match reader.next_event() {
                    Ok(Some(sse_codecs::json::Event::String(value))) => Some(value.into_owned()),
                    _ => None,
                };
            }
            let _ = reader.skip_value();
        }
    }
    None
}

fn steam_api(app_id: u32) -> std::result::Result<WorkerSteamApi, String> {
    let mut api = WorkerSteamApi::new();
    api.initialize(app_id).map_err(|error| error.message)?;
    Ok(api)
}

fn cloud_files(app_id: u32) -> std::result::Result<Vec<CloudFile>, String> {
    steam_api(app_id)?.list_files().map_err(|error| error.message)
}

fn cloud_read(app_id: u32, remote_name: &str) -> std::result::Result<Vec<u8>, String> {
    steam_api(app_id)?.read_file(remote_name).map_err(|error| error.message)
}

fn achievement_list(app_id: u32) -> std::result::Result<Vec<Achievement>, String> {
    steam_api(app_id)?.achievements().map_err(|error| error.message)
}

fn change_achievement(app_id: u32, name: &str, achieved: bool) -> std::result::Result<(), String> {
    let mut api = steam_api(app_id)?;
    if achieved {
        api.set_achievement(name).map_err(|error| error.message)?;
    } else {
        api.clear_achievement(name).map_err(|error| error.message)?;
    }
    api.store_stats().map_err(|error| error.message)
}

fn clip(text: &str) -> String {
    if text.chars().count() <= 80 {
        text.to_owned()
    } else {
        text.chars().take(79).chain(std::iter::once('…')).collect()
    }
}

fn format_epoch_timestamp(timestamp: Option<i64>) -> String {
    let Some(seconds) = timestamp
        .filter(|timestamp| *timestamp > 0)
        .and_then(|timestamp| u64::try_from(timestamp).ok())
    else {
        return t("дата неизвестна").to_owned();
    };
    let Some(value) = UNIX_EPOCH.checked_add(Duration::from_secs(seconds)) else {
        return t("дата вне диапазона").to_owned();
    };
    super::history::format_system_time(value)
}

fn system_time_timestamp(value: Option<SystemTime>) -> Option<i64> {
    let seconds = value?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    i64::try_from(seconds).ok()
}

#[derive(Debug)]
enum CompanionReply {
    Status(std::result::Result<CompanionStatus, String>),
    WorkshopOpened(std::result::Result<(), String>),
    Protocol(&'static str, std::result::Result<String, String>),
    Changed(std::result::Result<String, String>),
    Hotkeys(std::result::Result<String, String>),
}

#[derive(Debug)]
struct CompanionStatus {
    local_version: Option<String>,
    workshop: Option<sse_companion::workshop::WorkshopInstallState>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CompanionIntent {
    install: bool,
    game: String,
    directory: PathBuf,
}

#[derive(Default)]
struct Companion {
    status: Option<WidgetId>,
    cards: Option<WidgetId>,
    workshop_card: Option<WidgetId>,
    workshop_status: Option<WidgetId>,
    subscribe_workshop: Option<WidgetId>,
    version: Option<WidgetId>,
    latency: Option<WidgetId>,
    path: Option<WidgetId>,
    install: Option<WidgetId>,
    remove: Option<WidgetId>,
    refresh_button: Option<WidgetId>,
    ping: Option<WidgetId>,
    inspect: Option<WidgetId>,
    info: Option<WidgetId>,
    inventory: Option<WidgetId>,
    s2_commands: Vec<(WidgetId, &'static str, &'static str)>,
    manual_path: Option<WidgetId>,
    apply_manual: Option<WidgetId>,
    manual_directory: Option<PathBuf>,
    hotkey_inputs: Vec<(sse_companion::hotkeys::HotkeyAction, WidgetId)>,
    save_hotkeys: Option<WidgetId>,
    default_hotkeys: Option<WidgetId>,
    toggle_hotkeys: Option<WidgetId>,
    hotkey_runtime: Arc<Mutex<Option<sse_companion::hotkey_runtime::HotkeyRuntime>>>,
    confirm_card: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<CompanionIntent>,
    host: Option<WidgetId>,
}

impl Companion {
    fn build_workshop_card(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let workshop = style::card(cx.tree, host)?;
        self.workshop_card = Some(workshop);
        style::label(cx.tree, workshop, t("STEAM WORKSHOP"), Text::Heading)?;
        self.workshop_status = Some(style::label(
            cx.tree,
            workshop,
            t("Пакет Steam Workshop ещё не опубликован"),
            Text::Note,
        )?);
        style::label(
            cx.tree,
            workshop,
            t("Для игр вне Steam доступна локальная установка компаньона выше."),
            Text::Note,
        )?;
        self.subscribe_workshop = Some(style::button(
            cx.tree,
            workshop,
            t("ПОДПИСАТЬСЯ В STEAM"),
            Button::Primary,
        )?);
        if let Some(button) = self.subscribe_workshop {
            cx.tree.set_enabled(button, false)?;
        }
        Ok(())
    }

    fn selected(&self, cx: &Context<'_>) -> std::result::Result<(String, PathBuf), String> {
        let game = cx
            .app
            .selected_game()
            .map(str::to_owned)
            .ok_or_else(|| t("Игра не выбрана").to_owned())?;
        let directory = self
            .manual_directory
            .clone()
            .or_else(|| cx.app.game_dir().map(Path::to_path_buf))
            .ok_or_else(|| t("Папка игры не выбрана").to_owned())?;
        Ok((game, directory))
    }
    fn exchange_directory(game: &str, directory: &Path) -> std::result::Result<PathBuf, String> {
        if xray_game(game).is_none() {
            if matches!(game, "s2" | "stalker2") || game.contains("stalker2") {
                return std::env::var_os("LOCALAPPDATA")
                    .map(PathBuf::from)
                    .map(|root| root.join("Stalker2").join("Saved"))
                    .ok_or_else(|| t("LOCALAPPDATA не задан; папка протокола S.T.A.L.K.E.R. 2 не найдена").to_owned());
            }
            return Err(t("Для выбранной игры протокол Companion не поддерживается.").to_owned());
        }
        for relative in ["_appdata_", "appdata", "userdata"] {
            let candidate = directory.join(relative);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
        Err(t("появится после протокола компаньона").to_owned())
    }
    fn refresh(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let selected = self.selected(cx);
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-read", move || {
            let result = selected.and_then(|(game, directory)| {
                let root = companion_root(&game, &directory)?;
                let local_version = installed_version(&root);
                let workshop = sse_companion::workshop::package_for_release(&game).map(|package| {
                    sse_companion::workshop::inspect_ee_install(&directory, package, env!("CARGO_PKG_VERSION"))
                });
                Ok(CompanionStatus {
                    local_version,
                    workshop,
                })
            });
            proxy.send(AppMessage::ToScreen(
                ScreenId::Companion,
                Box::new(CompanionReply::Status(result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }
    fn protocol_args(&self, cx: &mut Context<'_>, command: &'static str, argument: Option<&'static str>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let selected = self.selected(cx);
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
            let result = selected
                .and_then(|(game, directory)| Self::exchange_directory(&game, &directory))
                .and_then(|directory| {
                    let client = sse_companion::protocol::CompanionClient::new(directory);
                    let timeout = Duration::from_secs(3);
                    let result = match (command, argument) {
                        ("ping", _) => client.ping(timeout).map(|(latency, _)| {
                            let milliseconds = format!("{:.0}", latency.as_secs_f64() * 1000.0);
                            tr("{0} мс", &[&milliseconds])
                        }),
                        ("info", _) => client.info(timeout),
                        ("list_inventory", _) => client.list_inventory(timeout),
                        ("god", Some("on")) => client.s2_god(true, timeout),
                        ("god", Some("off")) => client.s2_god(false, timeout),
                        ("noclip", Some("on")) => client.s2_noclip(true, timeout),
                        ("noclip", Some("off")) => client.s2_noclip(false, timeout),
                        ("timespeed", Some(value)) => value
                            .parse::<f32>()
                            .map_err(|_| {
                                sse_companion::protocol::ProtocolError::Invalid("time speed is invalid".to_owned())
                            })
                            .and_then(|speed| client.s2_time_speed(speed, timeout)),
                        _ => client.send(command, argument.as_slice(), timeout).and_then(|reply| {
                            if reply.status == sse_companion::protocol::ReplyStatus::Ok {
                                Ok(reply.text)
                            } else {
                                Err(sse_companion::protocol::ProtocolError::Invalid(format!(
                                    "{command} failed: {} {}",
                                    reply.status.as_str(),
                                    reply.text
                                )))
                            }
                        }),
                    };
                    result.map_err(|error| error.to_string())
                });
            proxy.send(AppMessage::ToScreen(
                ScreenId::Companion,
                Box::new(CompanionReply::Protocol(command, result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }
    fn protocol(&self, cx: &mut Context<'_>, command: &'static str) {
        self.protocol_args(cx, command, None);
    }
    fn load_hotkeys(&mut self, cx: &mut Context<'_>, defaults: bool) -> Result<()> {
        let path = sse_app::paths::default_data_directory().join("hotkeys.txt");
        let layout = if defaults {
            sse_companion::hotkeys::HotkeyLayout::default()
        } else {
            sse_companion::hotkeys::HotkeyLayout::load(&path)
        };
        for (action, id) in &self.hotkey_inputs {
            let value = layout
                .binding(*action)
                .map_or_else(String::new, |gesture| gesture.to_string());
            cx.tree.set_input_text(*id, &value)?;
        }
        let _ = cx.tree.take_changed_inputs();
        Ok(())
    }
}

impl Screen for Companion {
    fn id(&self) -> ScreenId {
        ScreenId::Companion
    }
    fn subtitle(&self) -> &str {
        t("Мод-компаньон, версия и горячие клавиши")
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        self.host = Some(host);
        let language = crate::strings::current_language();
        // The action cards to the left; the state and the primary action to the right.
        let body = cx.tree.add(
            Some(host),
            NodeKind::Row,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                gap: Size::new(crate::theme::CONTROL_GAP + 6.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let left = style::d2::panel(cx.tree, body)?;
        cx.tree.set_style(
            left,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                preferred: Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        style::d2::panel_title(cx.tree, left, t("ДЕЙСТВИЯ КОМПАНЬОНА"))?;
        let cards = cx.tree.add(
            Some(left),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.cards = Some(cards);
        let side = cx.tree.add(
            Some(body),
            NodeKind::Column,
            side_column_style(false, false),
            Content::Panel,
            Look::default(),
        )?;
        let state = style::d2::panel(cx.tree, side)?;
        cx.tree.set_style(
            state,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                preferred: Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        style::d2::panel_title(cx.tree, state, t("СОСТОЯНИЕ"))?;
        self.status = Some(style::label(cx.tree, state, t("НЕ УСТАНОВЛЕН"), Text::Value)?);
        self.version = Some(style::label(cx.tree, state, t("Версия мода: —"), Text::Note)?);
        self.latency = Some(style::label(
            cx.tree,
            state,
            t("Связь / Задержка: Нет ответа"),
            Text::Note,
        )?);
        self.path = Some(style::label(cx.tree, state, t("Путь установки: —"), Text::Note)?);
        self.install = Some(style::d2::button(
            cx.tree,
            state,
            t("УСТАНОВИТЬ"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        let card = style::card(cx.tree, cards)?;
        style::label(cx.tree, card, t("МОД-КОМПАНЬОН"), Text::Heading)?;
        style::label(
            cx.tree,
            card,
            t("Меню в игре: Esc → F1 или КПК компаньона. Хук создаёт распакованный скрипт в gamedata/scripts; установка через приложение справа."),
            Text::Note,
        )?;
        style::label(cx.tree, card, t("Целевая игра: выбранная в «Обзоре игр»"), Text::Body)?;
        let row = style::row(cx.tree, card)?;
        self.remove = Some(style::button(cx.tree, row, t("УДАЛИТЬ"), Button::Secondary)?);
        self.ping = Some(style::button(cx.tree, row, t("ПРОВЕРИТЬ СВЯЗЬ"), Button::Secondary)?);
        self.refresh_button = Some(style::button(cx.tree, row, t("ОБНОВИТЬ СТАТУС"), Button::Secondary)?);
        let live = style::card(cx.tree, cards)?;
        style::label(cx.tree, live, t("ЖИВОЙ ИНСПЕКТОР"), Text::Heading)?;
        style::label(
            cx.tree,
            live,
            t("Показываются только ответы протокола Companion: info и list_inventory."),
            Text::Note,
        )?;
        self.inspect = Some(style::button(cx.tree, live, t("ПОЛУЧИТЬ ДАННЫЕ"), Button::Secondary)?);
        self.info = Some(style::label(cx.tree, live, t("Информация игрока: —"), Text::Body)?);
        self.inventory = Some(style::label(cx.tree, live, t("Инвентарь игрока: —"), Text::Body)?);
        style::label(
            cx.tree,
            live,
            t("Для живой проверки нужен установленный Companion-протокол."),
            Text::Note,
        )?;
        let s2 = style::card(cx.tree, cards)?;
        style::label(
            cx.tree,
            s2,
            t("S.T.A.L.K.E.R. 2 — команды игры (экспериментально)"),
            Text::Heading,
        )?;
        style::label(cx.tree,s2,t("Нужны S2 на ПК, UE4SS и установленный мод. Команды выполняет сама игра (XSetGodMode, XSetNoClipGSC, XSetTimeSpeed)."),Text::Note)?;
        for (command, argument) in [
            ("god", "on"),
            ("god", "off"),
            ("noclip", "on"),
            ("noclip", "off"),
            ("timespeed", "5"),
            ("timespeed", "0"),
        ] {
            let label = match (command, argument) {
                ("god", "on") => t("Бессмертие: вкл"),
                ("god", "off") => t("Бессмертие: выкл"),
                ("noclip", "on") => t("Полёт: вкл"),
                ("noclip", "off") => t("Полёт: выкл"),
                ("timespeed", "5") => t("Время ×5"),
                _ => t("Время: норма"),
            };
            let id = style::button(cx.tree, s2, label, Button::Secondary)?;
            self.s2_commands.push((id, command, argument));
        }
        style::label(
            cx.tree,
            s2,
            t("Команды отправляются через протокол Companion в Stalker2\\Saved."),
            Text::Note,
        )?;
        let all = style::card(cx.tree, cards)?;
        style::label(cx.tree, all, t("ВСЕ ИГРЫ"), Text::Heading)?;
        for target in GameTarget::ALL {
            let game = target.title_in(language);
            style::label(
                cx.tree,
                all,
                &tr("[ ] {0} · наличие не проверено", &[&game]),
                Text::Body,
            )?;
        }
        let all_install = style::button(
            cx.tree,
            all,
            t("УСТАНОВИТЬ / ОБНОВИТЬ ВО ВСЕ ОТМЕЧЕННЫЕ"),
            Button::Secondary,
        )?;
        cx.tree.set_enabled(all_install, false)?;
        style::label(
            cx.tree,
            all,
            t("Выбор нескольких установок появится после общего API обнаружения игр."),
            Text::Note,
        )?;
        let manual = style::card(cx.tree, cards)?;
        style::label(cx.tree, manual, t("ПАПКА ИГРЫ (РУЧНОЙ ВЫБОР)"), Text::Heading)?;
        style::label(
            cx.tree,
            manual,
            t("Оставьте пустым для автоматического поиска через Steam. Укажите путь вручную, если папка нестандартная."),
            Text::Note,
        )?;
        let manual_row = style::row(cx.tree, manual)?;
        self.manual_path = Some(style::input(cx.tree, manual_row, "")?);
        self.apply_manual = Some(style::button(cx.tree, manual_row, t("ПРИМЕНИТЬ"), Button::Secondary)?);
        let hot = style::card(cx.tree, cards)?;
        style::label(cx.tree, hot, t("ГОРЯЧИЕ КЛАВИШИ"), Text::Heading)?;
        style::label(cx.tree,hot,t("Приложение перехватывает сочетание и отправляет команду моду через файл-протокол. Игра должна быть запущена с установленным модом."),Text::Note)?;
        for action in [
            sse_companion::hotkeys::HotkeyAction::Heal,
            sse_companion::hotkeys::HotkeyAction::RepairEquipped,
            sse_companion::hotkeys::HotkeyAction::Mark,
            sse_companion::hotkeys::HotkeyAction::JumpLast,
            sse_companion::hotkeys::HotkeyAction::QuickSave,
        ] {
            let row = style::row(cx.tree, hot)?;
            style::label(cx.tree, row, hotkey_label(language, action), Text::Body)?;
            let input = style::input(cx.tree, row, "")?;
            self.hotkey_inputs.push((action, input));
        }
        let hot_row = style::row(cx.tree, hot)?;
        self.save_hotkeys = Some(style::button(
            cx.tree,
            hot_row,
            t("СОХРАНИТЬ КЛАВИШИ"),
            Button::Primary,
        )?);
        self.default_hotkeys = Some(style::button(cx.tree, hot_row, t("ПО УМОЛЧАНИЮ"), Button::Secondary)?);
        self.toggle_hotkeys = Some(style::button(
            cx.tree,
            hot_row,
            t("ВКЛЮЧИТЬ ГОРЯЧИЕ КЛАВИШИ"),
            Button::Secondary,
        )?);
        if let Some(reason) = sse_companion::hotkeys::unavailable_reason() {
            if let Some(button) = self.toggle_hotkeys {
                cx.tree.set_enabled(button, false)?;
            }
            style::label(cx.tree, hot, reason, Text::Note)?;
        }
        let confirm = style::card(cx.tree, host)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, t("ПОДТВЕРЖДЕНИЕ ИЗМЕНЕНИЯ ИГРЫ"), Text::Heading)?;
        style::label(
            cx.tree,
            confirm,
            t("Будут изменены файлы выбранной игры. Проверьте игру и папку перед продолжением."),
            Text::Note,
        )?;
        let confirm_row = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, confirm_row, t("ПОДТВЕРДИТЬ"), Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, confirm_row, t("ОТМЕНА"), Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
        Ok(())
    }
    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.intent = None;
        // Close only this screen's own confirmation: another dialog may be open, and its close must not be lost.
        if let Some(card) = self.confirm_card {
            if cx.tree.dialog() == Some(card) {
                cx.tree.close_dialog()?;
            }
        }
        self.load_hotkeys(cx, false)?;
        if let Some(button) = self.toggle_hotkeys {
            let active = self.hotkey_runtime.lock().ok().is_some_and(|runtime| runtime.is_some());
            cx.tree.set_text(
                button,
                if active {
                    t("ВЫКЛЮЧИТЬ ГОРЯЧИЕ КЛАВИШИ")
                } else {
                    t("ВКЛЮЧИТЬ ГОРЯЧИЕ КЛАВИШИ")
                },
            )?;
        }
        let s2 = cx
            .app
            .selected_game()
            .is_some_and(|game| matches!(game, "s2" | "stalker2") || game.contains("stalker2"));
        for (id, _, _) in &self.s2_commands {
            cx.tree.set_enabled(*id, s2)?;
        }
        let workshop_package = cx
            .app
            .selected_game()
            .and_then(sse_companion::workshop::package_for_release);
        if workshop_package.is_some() && self.workshop_card.is_none() {
            if let Some(cards) = self.cards {
                self.build_workshop_card(cx, cards)?;
            }
        }
        if let Some(card) = self.workshop_card {
            cx.tree.set_visible(card, workshop_package.is_some())?;
        }
        if let Some(button) = self.subscribe_workshop {
            let has_published_id = workshop_package
                .and_then(|package| package.published_file_id)
                .and_then(sse_companion::workshop::workshop_page_url)
                .is_some();
            cx.tree.set_enabled(button, has_published_id)?;
        }
        self.refresh(cx);
        Ok(())
    }
    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.subscribe_workshop {
            let url = self
                .selected(cx)
                .ok()
                .and_then(|(game, _)| sse_companion::workshop::package_for_release(&game))
                .and_then(|package| package.published_file_id)
                .and_then(sse_companion::workshop::workshop_page_url);
            if let Some(url) = url {
                let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
                sse_app::tasks::spawn_named_detached("open-companion-workshop", move || {
                    let result = open_workshop_url(&url);
                    proxy.send(AppMessage::ToScreen(
                        ScreenId::Companion,
                        Box::new(CompanionReply::WorkshopOpened(result)),
                    ));
                });
                cx.status = Some(t("Открываю страницу Steam Workshop…").to_owned());
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.refresh_button {
            self.refresh(cx);
            return Ok(());
        }
        if clicked.is_some() && clicked == self.ping {
            cx.status = Some(t("Проверка связи с модом…").to_owned());
            self.protocol(cx, "ping");
            return Ok(());
        }
        if clicked.is_some() && clicked == self.inspect {
            cx.status = Some(t("Чтение ответов Companion…").to_owned());
            self.protocol(cx, "info");
            self.protocol(cx, "list_inventory");
            return Ok(());
        }
        if let Some((_, command, argument)) = self.s2_commands.iter().find(|(id, _, _)| clicked == Some(*id)) {
            cx.status = Some(t("Команда отправляется в S.T.A.L.K.E.R. 2…").to_owned());
            self.protocol_args(cx, command, Some(argument));
            return Ok(());
        }
        if clicked.is_some() && clicked == self.apply_manual {
            let text = self
                .manual_path
                .and_then(|id| cx.tree.input_text(id).ok())
                .unwrap_or("")
                .trim()
                .to_owned();
            if text.is_empty() {
                self.manual_directory = None;
                cx.status = Some(t("Папка очищена, используется автообнаружение.").to_owned());
            } else {
                let path = PathBuf::from(&text);
                if path.is_dir() {
                    self.manual_directory = Some(path.clone());
                    cx.status = Some(tr("Папка задана: {0}", &[&path.display()]));
                } else {
                    cx.status = Some(tr("Папка не найдена: {0}", &[&text]));
                }
            }
            self.intent = None;
            self.refresh(cx);
            return Ok(());
        }
        if clicked.is_some() && clicked == self.default_hotkeys {
            self.load_hotkeys(cx, true)?;
            return Ok(());
        }
        if clicked.is_some() && clicked == self.toggle_hotkeys {
            let active = self.hotkey_runtime.lock().ok().is_some_and(|runtime| runtime.is_some());
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            if active {
                let runtime = self.hotkey_runtime.lock().ok().and_then(|mut runtime| runtime.take());
                if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-hotkeys-stop", move || {
                    let result = runtime
                        .map_or(Ok(()), |mut runtime| runtime.stop())
                        .map(|()| t("Горячие клавиши выключены.").to_owned())
                        .map_err(|error| error.to_string());
                    proxy.send(AppMessage::ToScreen(
                        ScreenId::Companion,
                        Box::new(CompanionReply::Hotkeys(result)),
                    ));
                }) {
                    cx.status = Some(tr("Ошибка: {0}", &[&error]));
                }
            } else {
                let selected = self.selected(cx).and_then(|(game, directory)| {
                    if xray_game(&game).is_none() {
                        return Err(t("Горячие клавиши поддерживаются только для игр X-Ray.").to_owned());
                    }
                    Self::exchange_directory(&game, &directory)
                });
                let path = sse_app::paths::default_data_directory().join("hotkeys.txt");
                let layout = sse_companion::hotkeys::HotkeyLayout::load(&path);
                let runtime_slot = Arc::clone(&self.hotkey_runtime);
                if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-hotkeys-start", move || {
                    let result = selected.and_then(|directory| {
                        sse_companion::hotkey_runtime::HotkeyRuntime::start(&layout, directory)
                            .map_err(|error| error.to_string())
                            .and_then(|runtime| {
                                runtime_slot
                                    .lock()
                                    .map_err(|_| "hotkey runtime lock was poisoned".to_owned())
                                    .map(|mut slot| {
                                        *slot = Some(runtime);
                                    })
                            })
                    });
                    proxy.send(AppMessage::ToScreen(
                        ScreenId::Companion,
                        Box::new(CompanionReply::Hotkeys(
                            result.map(|()| t("Горячие клавиши включены.").to_owned()),
                        )),
                    ));
                }) {
                    cx.status = Some(tr("Ошибка: {0}", &[&error]));
                }
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.save_hotkeys {
            let mut text = String::new();
            for (action, id) in &self.hotkey_inputs {
                let value = cx.tree.input_text(*id).unwrap_or("");
                text.push_str(action.name());
                text.push('=');
                text.push_str(value);
                text.push('\n');
            }
            let path = sse_app::paths::default_data_directory().join("hotkeys.txt");
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let runtime_slot = Arc::clone(&self.hotkey_runtime);
            if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
                let result = (|| {
                    let layout =
                        sse_companion::hotkeys::HotkeyLayout::parse(&text).map_err(|error| error.to_string())?;
                    layout.save(&path).map_err(|error| error.to_string())?;
                    let running = runtime_slot
                        .lock()
                        .map_err(|_| "hotkey runtime lock was poisoned".to_owned())?
                        .take();
                    if let Some(mut runtime) = running {
                        let directory = runtime.exchange_directory().to_path_buf();
                        runtime.stop().map_err(|error| error.to_string())?;
                        let restarted = sse_companion::hotkey_runtime::HotkeyRuntime::start(&layout, directory)
                            .map_err(|error| error.to_string())?;
                        let mut slot = runtime_slot
                            .lock()
                            .map_err(|_| "hotkey runtime lock was poisoned".to_owned())?;
                        *slot = Some(restarted);
                    }
                    Ok(tr("Клавиши сохранены: {0}.", &[&path.display()]))
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Companion,
                    Box::new(CompanionReply::Hotkeys(result)),
                ));
            }) {
                cx.status = Some(tr("Ошибка: {0}", &[&error]));
            }
            return Ok(());
        }
        if clicked.is_some() && (clicked == self.install || clicked == self.remove) {
            let (game, directory) = match self.selected(cx) {
                Ok(v) => v,
                Err(e) => {
                    cx.status = Some(e);
                    return Ok(());
                }
            };
            self.intent = Some(CompanionIntent {
                install: clicked == self.install,
                game,
                directory,
            });
            if let Some(card) = self.confirm_card {
                cx.tree.open_dialog(card)?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_cancel {
            self.intent = None;
            if self.confirm_card.is_some() {
                cx.tree.close_dialog()?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_write {
            let Some(intent) = self.intent.take() else {
                return Ok(());
            };
            let current = self.selected(cx).ok();
            if current.as_ref() != Some(&(intent.game.clone(), intent.directory.clone())) {
                if self.confirm_card.is_some() {
                    cx.tree.close_dialog()?;
                }
                cx.status = Some(t("Выбор игры изменился; подтверждение отменено.").to_owned());
                return Ok(());
            }
            if self.confirm_card.is_some() {
                cx.tree.close_dialog()?;
            }
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
                let result = (|| {
                    let root = companion_root(&intent.game, &intent.directory)?;
                    if intent.install {
                        if let Some(target) = xray_game(&intent.game) {
                            sse_companion::installer::install_bundled(&root, target).map_err(|e| e.to_string())?;
                        } else {
                            sse_companion::installer::install_stalker2(&root).map_err(|e| e.to_string())?;
                        }
                        Ok(t("Компаньон успешно установлен!").to_owned())
                    } else {
                        let id = if matches!(intent.game.as_str(), "s2" | "stalker2") {
                            "s2"
                        } else if intent.game.contains("soc") {
                            "soc"
                        } else if intent.game.contains("cs") || intent.game == "clear_sky" {
                            "cs"
                        } else {
                            "cop"
                        };
                        let removed = sse_companion::installer::uninstall(&root, id).map_err(|e| e.to_string())?;
                        Ok(if removed {
                            t("Компаньон удалён.")
                        } else {
                            t("Не удалось удалить компаньон.")
                        }
                        .to_owned())
                    }
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Companion,
                    Box::new(CompanionReply::Changed(result)),
                ));
            }) {
                cx.status = Some(tr("Ошибка: {0}", &[&error]));
            }
            return Ok(());
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Companion, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<CompanionReply>() {
                match reply {
                    CompanionReply::Status(Ok(status)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(
                                id,
                                if status.local_version.is_some() {
                                    t("УСТАНОВЛЕН (ОЖИДАНИЕ ИГРЫ)")
                                } else {
                                    t("НЕ УСТАНОВЛЕН")
                                },
                            )?;
                        }
                        if let Some(id) = self.version {
                            cx.tree.set_text(
                                id,
                                &tr("Версия мода: {0}", &[&status.local_version.as_deref().unwrap_or("—")]),
                            )?;
                        }
                        if let Some(id) = self.install {
                            cx.tree.set_text(
                                id,
                                if status.local_version.is_some() {
                                    t("ОБНОВИТЬ")
                                } else {
                                    t("УСТАНОВИТЬ")
                                },
                            )?;
                        }
                        if let (Some(id), Ok((_, dir))) = (self.path, self.selected(cx)) {
                            cx.tree.set_text(id, &tr("Путь установки: {0}", &[&dir.display()]))?;
                        }
                        if let (Some(id), Some(workshop)) = (self.workshop_status, status.workshop) {
                            cx.tree.set_text(id, workshop_state_text(workshop))?;
                        }
                    }
                    CompanionReply::WorkshopOpened(Ok(())) => {
                        cx.status = Some(t("Страница Steam Workshop открыта").to_owned());
                    }
                    CompanionReply::WorkshopOpened(Err(error)) => {
                        cx.status = Some(tr("Не удалось открыть Steam Workshop: {0}", &[error]));
                    }
                    CompanionReply::Status(Err(e)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, t("ОШИБКА"))?;
                        }
                        cx.status = Some(tr("Ошибка обновления статуса: {0}", &[e]));
                    }
                    CompanionReply::Protocol(command, Ok(text)) => match *command {
                        "ping" => {
                            if let Some(id) = self.status {
                                cx.tree.set_text(id, t("РАБОТАЕТ (ПОДКЛЮЧЁН)"))?;
                            }
                            if let Some(id) = self.latency {
                                cx.tree.set_text(id, &tr("Связь / Задержка: {0}", &[text]))?;
                            }
                            cx.status = Some(tr("Мод отвечает. Задержка: {0}", &[text]));
                        }
                        "info" => {
                            if let Some(id) = self.info {
                                cx.tree.set_text(id, &tr("Информация игрока: {0}", &[text]))?;
                            }
                        }
                        "list_inventory" => {
                            if let Some(id) = self.inventory {
                                cx.tree.set_text(id, &tr("Инвентарь игрока: {0}", &[text]))?;
                            }
                        }
                        "god" | "noclip" | "timespeed" => {
                            cx.status = Some(tr("Игра выполнила: {0}", &[text]));
                        }
                        _ => cx.status = Some(format!("Companion: {text}")),
                    },
                    CompanionReply::Protocol(command, Err(e)) => {
                        if let Some(id) = self.latency {
                            cx.tree.set_text(id, t("Связь / Задержка: Нет ответа"))?;
                        }
                        cx.status = Some(if *command == "ping" {
                            tr("Ошибка пинга: {0}", &[e])
                        } else if matches!(*command, "god" | "noclip" | "timespeed") {
                            tr("Не выполнено: {0}", &[e])
                        } else {
                            tr("Не удалось получить данные Companion: {0}", &[e])
                        });
                    }
                    CompanionReply::Changed(Ok(text)) | CompanionReply::Hotkeys(Ok(text)) => {
                        cx.status = Some(text.clone());
                        if let Some(button) = self.toggle_hotkeys {
                            let active = self.hotkey_runtime.lock().ok().is_some_and(|runtime| runtime.is_some());
                            cx.tree.set_text(
                                button,
                                if active {
                                    t("ВЫКЛЮЧИТЬ ГОРЯЧИЕ КЛАВИШИ")
                                } else {
                                    t("ВКЛЮЧИТЬ ГОРЯЧИЕ КЛАВИШИ")
                                },
                            )?;
                        }
                        self.refresh(cx);
                    }
                    CompanionReply::Changed(Err(e)) => {
                        cx.status = Some(tr("Ошибка установки: {0}", &[e]));
                    }
                    CompanionReply::Hotkeys(Err(e)) => {
                        cx.status = Some(tr("Клавиши не сохранены: {0}", &[e]));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum AchReply {
    List(std::result::Result<Vec<Achievement>, String>),
    Changed(std::result::Result<(), String>),
}
/// Eight invented achievements for the developer screenshot tool; nothing here comes from Steam.
/// Only for sse-ui-dev (screenshots); not part of the screen API.
#[must_use]
pub fn achievement_fixture_message() -> AppMessage {
    let items = (1..=8_u32)
        .map(|index| Achievement {
            name: format!("FIXTURE_ACHIEVEMENT_{index}"),
            display_name: format!("Тестовое достижение {index}"),
            description: format!("Выдуманное описание достижения {index} для снимка экрана."),
            hidden: false,
            achieved: index % 3 == 0,
            unlock_time: 0,
        })
        .collect();
    AppMessage::ToScreen(ScreenId::Achievements, Box::new(AchReply::List(Ok(items))))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AchievementIntent {
    app_id: u32,
    name: String,
    set: bool,
}
/// The package the update screen may install: only when the check found a newer version.
fn offered_artifact(
    state: sse_update::UpdateState,
    artifact: &Option<sse_update::UpdateArtifact>,
) -> Option<sse_update::UpdateArtifact> {
    match state {
        sse_update::UpdateState::Available => artifact.clone(),
        _ => None,
    }
}

#[derive(Default)]
struct Achievements {
    status: Option<WidgetId>,
    list: Option<ListSide>,
    items: Vec<Achievement>,
    selected: Option<String>,
    set: Option<WidgetId>,
    clear: Option<WidgetId>,
    refresh: Option<WidgetId>,
    progress: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_name: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<AchievementIntent>,
    pending_set: Option<bool>,
}

impl Achievements {
    fn load(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let id = cx.app.selected_game().and_then(app_id);
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-read", move || {
            let result = id
                .ok_or_else(|| t("Для выбранной игры нет Steam App ID").to_owned())
                .and_then(achievement_list);
            proxy.send(AppMessage::ToScreen(
                ScreenId::Achievements,
                Box::new(AchReply::List(result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }
    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let Some(list) = self.list.as_ref() else { return Ok(()) };
        let chosen = self
            .selected
            .as_deref()
            .and_then(|name| self.items.iter().position(|item| item.name == name));
        // rows[i] shows items[i]; a click on it selects items[i] (see message).
        for (i, row) in list.rows.iter().copied().enumerate() {
            match self.items.get(i) {
                Some(a) => show_list_row(
                    cx.tree,
                    row,
                    &format!("{} {}", if a.achieved { "●" } else { "○" }, clip(&a.display_name)),
                    &clip(&a.description),
                    chosen == Some(i),
                )?,
                None => cx.tree.set_visible(row.stack, false)?,
            }
        }
        cx.tree.set_text(list.count, &self.items.len().to_string())?;
        let shown = chosen.is_some();
        for id in &list.kv_rows {
            cx.tree.set_visible(*id, shown)?;
        }
        cx.tree.set_visible(list.empty, !shown)?;
        if let Some(a) = chosen.and_then(|index| self.items.get(index)) {
            let status = if a.achieved {
                t("Получено")
            } else {
                t("Не получено")
            };
            let values = [clip(&a.display_name), status.to_owned()];
            // The description wraps in the side panel's paragraph; a key–value row would run past the panel's edge.
            cx.tree.set_text(list.detail, &a.description)?;
            cx.tree.set_visible(list.detail, true)?;
            for (value, text) in list.kv_values.iter().zip(values.iter()) {
                cx.tree.set_text(*value, text)?;
            }
        }
        for id in [self.set, self.clear].into_iter().flatten() {
            cx.tree.set_enabled(id, shown)?;
        }
        if chosen.is_none() {
            cx.tree.set_visible(list.detail, false)?;
        }
        if let Some(id) = self.status {
            let count = self.items.len();
            let mut text = tr("Загружено {0} достижений.", &[&count]);
            if count > list.rows.len() {
                text.push(' ');
                text.push_str(&tr("Показаны первые {0}.", &[&list.rows.len()]));
            }
            cx.tree.set_text(id, &text)?;
        }
        if let Some(id) = self.progress {
            let got = self.items.iter().filter(|item| item.achieved).count();
            let percent = if self.items.is_empty() {
                0.0
            } else {
                (got as f64 * 100.0) / self.items.len() as f64
            };
            let total = self.items.len();
            let percent = format!("{percent:.0}");
            cx.tree
                .set_text(id, &tr("{0} из {1} получено ({2}%)", &[&got, &total, &percent]))?;
        }
        sync_side_widths(cx.tree, list)
    }
}

impl Screen for Achievements {
    fn id(&self) -> ScreenId {
        ScreenId::Achievements
    }
    fn subtitle(&self) -> &str {
        t("Достижения выбранной игры и прогресс Steam")
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            card,
            t("Steam доступен / недоступен определяется рабочим процессом Steam."),
            Text::Note,
        )?;
        self.progress = Some(style::label(cx.tree, card, t("0 из 0 получено (0%)"), Text::Value)?);
        // The list of achievements to the left, the chosen one and its actions to the right.
        let keys = [t("Название"), t("Статус")].map(str::to_owned);
        let list = build_list_side(
            cx,
            host,
            t("ДОСТИЖЕНИЯ STEAM"),
            t("ВЫБРАННОЕ ДОСТИЖЕНИЕ"),
            &keys,
            (t("Достижение не выбрано."), t("Запрос достижений...")),
            Some(t("ОБНОВИТЬ")),
        )?;
        let inspector = cx.tree.children(list.side).first().copied().unwrap_or(host);
        cx.tree.set_visible(list.actions, false)?;
        let stacked = cx.tree.add(
            Some(inspector),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.set = Some(style::d2::button(
            cx.tree,
            stacked,
            t("ПОЛУЧИТЬ"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        self.clear = Some(style::d2::button(
            cx.tree,
            stacked,
            t("СНЯТЬ"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        for id in [self.set, self.clear].into_iter().flatten() {
            cx.tree.set_enabled(id, false)?;
        }
        self.refresh = list.action_button;
        self.status = Some(list.note);
        self.list = Some(list);
        let confirm = style::card(cx.tree, host)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, t("ПОДТВЕРЖДЕНИЕ ДОСТИЖЕНИЯ"), Text::Heading)?;
        self.confirm_name = Some(style::label(cx.tree, confirm, "", Text::Value)?);
        style::label(
            cx.tree,
            confirm,
            t("Изменение будет отправлено в Steam для выбранной игры и достижения."),
            Text::Note,
        )?;
        let actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, actions, t("ПОДТВЕРДИТЬ"), Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, actions, t("ОТМЕНА"), Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
        Ok(())
    }
    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.load(cx);
        Ok(())
    }
    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.refresh {
            self.load(cx);
            return Ok(());
        }
        let row_index = clicked.and_then(|id| {
            self.list
                .as_ref()
                .and_then(|list| list.rows.iter().position(|row| row.select == id))
        });
        if let Some(i) = row_index {
            if self.items.get(i).is_some() {
                self.selected = self.items.get(i).map(|a| a.name.clone());
                self.intent = None;
                if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
                    let _ = cx.tree.close_dialog()?;
                }
                cx.status = self.items.get(i).map(|a| a.description.clone());
                self.render(cx)?;
            }
        }
        let change = if clicked.is_none() {
            None
        } else if clicked == self.set {
            Some(true)
        } else if clicked == self.clear {
            Some(false)
        } else {
            None
        };
        if let Some(set) = change {
            let Some(selected_id) = self.selected.as_deref() else {
                cx.status = Some(t("Сначала выберите достижение").to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
                self.selected = None;
                cx.status = Some(t("Выбранное достижение исчезло после обновления списка").to_owned());
                return Ok(());
            };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
            if let Some(name) = self.confirm_name {
                cx.tree.set_text(name, &item.display_name)?;
            }
            self.intent = Some(AchievementIntent {
                app_id,
                name: item.name.clone(),
                set,
            });
            if let Some(card) = self.confirm_card {
                cx.tree.open_dialog(card)?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_cancel {
            self.intent = None;
            let _ = cx.tree.close_dialog()?;
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_write {
            let Some(intent) = self.intent.take() else {
                return Ok(());
            };
            if cx.app.selected_game().and_then(app_id) != Some(intent.app_id) {
                let _ = cx.tree.close_dialog()?;
                cx.status = Some(t("Выбранная игра изменилась; подтверждение отменено.").to_owned());
                return Ok(());
            }
            self.pending_set = Some(intent.set);
            let app_id = intent.app_id;
            let name = intent.name;
            let set = intent.set;
            let _ = cx.tree.close_dialog()?;
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
                let result = change_achievement(app_id, &name, set);
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Achievements,
                    Box::new(AchReply::Changed(result)),
                ));
            }) {
                cx.status = Some(tr("Ошибка: {0}", &[&error]));
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Achievements, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<AchReply>() {
                match reply {
                    AchReply::List(Ok(items)) => {
                        self.intent = None;
                        if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
                            let _ = cx.tree.close_dialog()?;
                        }
                        self.items.clone_from(items);
                        self.render(cx)?
                    }
                    AchReply::List(Err(e)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &clip(e))?;
                        }
                    }
                    AchReply::Changed(Ok(())) => {
                        if let Some(selected_id) = self.selected.as_deref() {
                            if let Some(item) = self.items.iter_mut().find(|item| item.name == selected_id) {
                                item.achieved = self.pending_set.take().unwrap_or(item.achieved);
                                cx.status = Some(if item.achieved {
                                    tr("Достижение «{0}» получено в Steam.", &[&item.display_name])
                                } else {
                                    tr("Достижение «{0}» снято в Steam.", &[&item.display_name])
                                });
                            }
                        }
                        self.render(cx)?;
                        self.load(cx)
                    }
                    AchReply::Changed(Err(e)) => cx.status = Some(format!("Steam: {e}")),
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct CloudIntent {
    app_id: u32,
    remote: String,
    local: PathBuf,
    local_sha256: [u8; 32],
    cloud_sha256: [u8; 32],
    cloud_size: u64,
    cloud_timestamp: Option<i64>,
    local_size: u64,
    local_timestamp: Option<i64>,
    backup_directory: PathBuf,
}

/// A cloud write may only go to the game whose save was checked.
fn intent_is_for_selected_game(selected_game: Option<&str>, intent_app_id: u32) -> bool {
    selected_game.and_then(app_id) == Some(intent_app_id)
}

#[derive(Debug)]
enum CloudReply {
    List(std::result::Result<Vec<CloudFile>, String>),
    Prepared(std::result::Result<CloudIntent, String>),
    Done(std::result::Result<String, String>),
}

#[derive(Default)]
struct Cloud {
    status: Option<WidgetId>,
    rows: Vec<WidgetId>,
    list_rows: Vec<ListRow>,
    side_column: Option<WidgetId>,
    side_empty: Option<WidgetId>,
    side_kv: Vec<WidgetId>,
    side_values: Vec<WidgetId>,
    items: Vec<CloudFile>,
    selected: Option<String>,
    download: Option<WidgetId>,
    upload: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_cloud_version: Option<WidgetId>,
    confirm_local_version: Option<WidgetId>,
    confirm_backup_directory: Option<WidgetId>,
    confirm_check: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<CloudIntent>,
    overwrite_confirmed: bool,
    backup_workspace: Workspace,
}

impl Cloud {
    fn with_backup_workspace(backup_workspace: Workspace) -> Self {
        Self {
            backup_workspace,
            ..Self::default()
        }
    }

    fn clear_intent(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.intent = None;
        self.overwrite_confirmed = false;
        if let Some(card) = self.confirm_card {
            if cx.tree.dialog() == Some(card) {
                let _ = cx.tree.close_dialog()?;
            } else {
                cx.tree.set_visible(card, false)?;
            }
        }
        Ok(())
    }

    fn load(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let id = cx.app.selected_game().and_then(app_id);
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-read", move || {
            let result = id
                .ok_or_else(|| t("Для выбранной игры нет Steam App ID").to_owned())
                .and_then(cloud_files);
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::List(result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        for (i, row) in self.list_rows.clone().into_iter().enumerate() {
            if let Some(f) = self.items.get(i) {
                let chosen = self.selected.as_deref() == Some(f.name.as_str());
                show_list_row(cx.tree, row, &clip(&f.name), &format!("{} KiB", f.size / 1024), chosen)?;
            } else {
                cx.tree.set_visible(row.stack, false)?;
            }
        }
        if let Some(id) = self.status {
            cx.tree.set_text(id, &tr("Файлов: {0}", &[&self.items.len()]))?;
        }
        let chosen = self
            .selected
            .as_deref()
            .and_then(|name| self.items.iter().find(|file| file.name == name));
        let shown = chosen.is_some();
        for id in &self.side_kv {
            cx.tree.set_visible(*id, shown)?;
        }
        if let Some(empty) = self.side_empty {
            cx.tree.set_visible(empty, !shown)?;
        }
        if let Some(file) = chosen {
            let values = [clip(&file.name), format!("{} KiB", file.size / 1024)];
            for (value, text) in self.side_values.iter().zip(values.iter()) {
                cx.tree.set_text(*value, text)?;
            }
        }
        if let Some(download) = self.download {
            cx.tree.set_enabled(download, shown)?;
        }
        // The upload needs a chosen file, a game with a Steam App ID, and not Stalker 2 (see prepare_upload).
        let uploadable = shown
            && cx
                .app
                .selected_game()
                .and_then(app_id)
                .is_some_and(|id| id != sse_steam::discovery::STALKER_2_APP_ID);
        if let Some(upload) = self.upload {
            cx.tree.set_enabled(upload, uploadable)?;
        }
        // The window's status line shows this screen's state, not the save search's.
        let line = match self.items.is_empty() {
            true => t("Список не загружен. Нажмите «ОБНОВИТЬ СПИСОК»."),
            false => "",
        };
        if line.is_empty() {
            cx.status = Some(tr("Файлов: {0}", &[&self.items.len()]));
        } else {
            cx.status = Some(line.to_owned());
        }
        Ok(())
    }

    fn prepare_upload(&self, cx: &mut Context<'_>) {
        let Some(selected_id) = self.selected.as_deref() else {
            cx.status = Some(t("Сначала выберите файл Steam Cloud").to_owned());
            return;
        };
        let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
            cx.status = Some(t("Выбранный облачный файл исчез после обновления списка").to_owned());
            return;
        };
        let Some(game) = cx.app.selected_game() else { return };
        let Some(app_id) = app_id(game) else { return };
        if app_id == sse_steam::discovery::STALKER_2_APP_ID {
            cx.status = Some(t("Запись S.T.A.L.K.E.R. 2 в Steam Cloud запрещена.").to_owned());
            return;
        }
        let remote = item.name.clone();
        let selected_file = item.clone();
        let remote_name = Path::new(&remote)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&remote);
        let snapshot = cx.app.snapshot();
        let mut candidates = snapshot.recent_saves;
        if let Some(current) = snapshot.current_save {
            if !candidates.iter().any(|path| path == &current) {
                candidates.push(current);
            }
        }
        // A recent save of another game can share the file name, so the local file must sit in this game's folder.
        let game_directories = sse_storage::discovery::SaveDirectoryLocator::find_candidate_directories(None);
        let family = save_family(game);
        let mut matching = candidates.into_iter().filter(|path| {
            in_game_directory(path, family, &game_directories)
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(remote_name))
        });
        let Some(local) = matching.next() else {
            cx.status = Some(t("Для выбранного облачного файла не найден локальный сейв с тем же именем.").to_owned());
            return;
        };
        if matching.next().is_some() {
            cx.status =
                Some(t("Найдено несколько локальных сейвов с тем же именем; запись в облако отменена.").to_owned());
            return;
        };

        let Some(proxy) = cx.proxy.cloned() else { return };
        let backup_directory = self.backup_workspace.backup_directory();
        cx.status = Some(t("Сверяю облачную и локальную версии перед подтверждением…").to_owned());
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-read", move || {
            let result = (|| {
                let metadata_before =
                    std::fs::metadata(&local).map_err(|error| tr("Ошибка локального сейва: {0}", &[&error]))?;
                if metadata_before.len() == 0
                    || metadata_before.len() > u64::try_from(sse_steam::cloud::MAX_CLOUD_FILE_BYTES).unwrap_or(u64::MAX)
                {
                    return Err(t("Размер локального сейва вне поддерживаемого диапазона для Steam Cloud").to_owned());
                }
                let local_bytes =
                    std::fs::read(&local).map_err(|error| tr("Ошибка локального сейва: {0}", &[&error]))?;
                let metadata_after =
                    std::fs::metadata(&local).map_err(|error| tr("Ошибка локального сейва: {0}", &[&error]))?;
                let local_size = u64::try_from(local_bytes.len())
                    .map_err(|_| t("Размер локального сейва превышает диапазон метаданных").to_owned())?;
                if metadata_before.len() != local_size
                    || metadata_after.len() != local_size
                    || metadata_before.modified().ok() != metadata_after.modified().ok()
                {
                    return Err(t("Локальный сейв изменился во время подготовки; повторите попытку").to_owned());
                }

                let mut api = steam_api(app_id)?;
                let listed_before = api.list_files().map_err(|error| error.message)?;
                let current = listed_before
                    .iter()
                    .find(|file| file.name == remote)
                    .ok_or_else(|| t("Облачный файл исчез; обновите список").to_owned())?;
                if current != &selected_file {
                    return Err(t("Облачная версия изменилась после загрузки списка; обновите список").to_owned());
                }
                if !current.exists || !current.persisted {
                    return Err(t("Steam не подтвердил сохранённую облачную версию; запись отменена").to_owned());
                }
                if current.size == 0
                    || current.size > u64::try_from(sse_steam::cloud::MAX_CLOUD_FILE_BYTES).unwrap_or(u64::MAX)
                {
                    return Err(t("Размер облачного сейва вне поддерживаемого диапазона").to_owned());
                }
                let cloud_bytes = api.read_file(&remote).map_err(|error| error.message)?;
                let cloud_size = u64::try_from(cloud_bytes.len())
                    .map_err(|_| t("Размер облачного сейва превышает диапазон метаданных").to_owned())?;
                if cloud_size != current.size {
                    return Err(t("Размер облачного файла изменился во время чтения; обновите список").to_owned());
                }
                let listed_after = api.list_files().map_err(|error| error.message)?;
                if listed_after.iter().find(|file| file.name == remote) != Some(current) {
                    return Err(t("Облачная версия изменилась во время чтения; обновите список").to_owned());
                }

                Ok(CloudIntent {
                    app_id,
                    remote,
                    local,
                    local_sha256: sse_codecs::sha256::sha256(&local_bytes),
                    cloud_sha256: sse_codecs::sha256::sha256(&cloud_bytes),
                    cloud_size,
                    cloud_timestamp: Some(current.timestamp),
                    local_size,
                    local_timestamp: system_time_timestamp(metadata_after.modified().ok()),
                    backup_directory,
                })
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::Prepared(result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }

    fn upload(&self, cx: &mut Context<'_>, intent: CloudIntent) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
            let result = (|| {
                let output = std::fs::read(&intent.local).map_err(|error| tr("Ошибка записи: {0}", &[&error]))?;
                if sse_codecs::sha256::sha256(&output) != intent.local_sha256 {
                    return Ok(
                        t("Локальный файл изменился после запроса записи; подтвердите запись ещё раз").to_owned(),
                    );
                }
                let mut api = steam_api(intent.app_id)?;
                let prepared = PreparedEdit::from_source_sha256(intent.cloud_sha256, output);
                let mut verifier = XRaySaveFormatVerifier::default();
                let receipt = SteamCloudWriteTransaction::upload(
                    &mut api,
                    &mut verifier,
                    intent.app_id,
                    &intent.remote,
                    &prepared,
                    &intent.backup_directory,
                    true,
                )
                .map_err(|error| error.message)?;
                let artifact_paths = format!(
                    "{}; {}; {}",
                    tr("Копия облачной версии: {0}", &[&receipt.backup_path.display()]),
                    tr("Копия отправки: {0}", &[&receipt.recovery_path.display()]),
                    tr("Журнал: {0}", &[&receipt.journal_path.display()]),
                );
                match receipt.status {
                    WriteStatus::Verified => {
                        Ok(tr("Записано и проверено: {0}. {1}", &[&intent.remote, &artifact_paths]))
                    }
                    WriteStatus::Uncertain => {
                        let reason = receipt
                            .reason
                            .map(|reason| tr(" — {0}", &[&reason]))
                            .unwrap_or_default();
                        Ok(tr(
                            "Результат записи не подтверждён (повтор не выполняется): {0}{1}. {2}",
                            &[&intent.remote, &reason, &artifact_paths],
                        ))
                    }
                }
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::Done(result)),
            ));
        }) {
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }
}

impl Screen for Cloud {
    fn id(&self) -> ScreenId {
        ScreenId::Cloud
    }

    fn subtitle(&self) -> &str {
        t("Steam Cloud: список, локальная копия и защищённая запись")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        // The list of cloud files to the left, the chosen file and its actions to the right; the list is read only when
        // the user asks for it ("ОБНОВИТЬ СПИСОК"), so nothing here calls Steam.
        let keys = [t("Файл"), t("Размер")].map(str::to_owned);
        let list: ListSide = build_list_side(
            cx,
            host,
            t("STEAM CLOUD"),
            t("ВЫБРАННЫЙ ФАЙЛ"),
            &keys,
            (
                t("Файл не выбран."),
                t("Список не загружен. Нажмите «ОБНОВИТЬ СПИСОК»."),
            ),
            Some(t("ОБНОВИТЬ СПИСОК")),
        )?;
        let inspector = cx.tree.children(list.side).first().copied().unwrap_or(host);
        let stacked = cx.tree.add(
            Some(inspector),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_visible(list.actions, false)?;
        self.download = Some(style::d2::button(
            cx.tree,
            stacked,
            t("СКАЧАТЬ В ЛОКАЛЬНЫЕ"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        // Writing to the cloud is the riskiest action of the program: it is never the primary one, and it stays off
        // until the handler would accept it (see render).
        self.upload = Some(style::d2::button(
            cx.tree,
            stacked,
            t("ЗАПИСАТЬ В ОБЛАКО..."),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        self.side_column = Some(stacked);
        self.side_empty = Some(list.empty);
        self.side_kv = list.kv_rows.clone();
        self.side_values = list.kv_values.clone();
        self.status = Some(list.note);
        // The refresh is the header's action; the rows are the list's select buttons, after it.
        self.rows = list.action_button.into_iter().collect();
        self.rows.extend(list.rows.iter().map(|row| row.select));
        self.list_rows = list.rows.clone();
        for id in [self.download, self.upload].into_iter().flatten() {
            cx.tree.set_enabled(id, false)?;
        }
        let overlay = cx.tree.overlay_host().unwrap_or(host);
        let confirm = style::card(cx.tree, overlay)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, t("ПОДТВЕРЖДЕНИЕ ЗАПИСИ"), Text::Heading)?;
        style::label(
            cx.tree,
            confirm,
            t("Внимание: локальный файл будет отправлен в Steam Cloud и перезапишет облачное сохранение. Резервная копия будет сохранена в бэкапы."),
            Text::Body,
        )?;
        self.confirm_cloud_version = Some(style::label(cx.tree, confirm, t("Облако Steam: —"), Text::Note)?);
        self.confirm_local_version = Some(style::label(cx.tree, confirm, t("Локальный сейв: —"), Text::Note)?);
        self.confirm_backup_directory = Some(style::label(
            cx.tree,
            confirm,
            t("Папка резервных копий: —"),
            Text::Note,
        )?);
        self.confirm_check = Some(style::button(
            cx.tree,
            confirm,
            t("[ ] Я подтверждаю перезапись"),
            Button::Secondary,
        )?);
        let actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, actions, t("ЗАПИСАТЬ"), Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, actions, t("ОТМЕНА"), Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.intent.is_some() {
            if let Some(card) = self.confirm_card {
                cx.tree.open_dialog(card)?;
            }
        }
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if matches!(
            message,
            Message::Window(WindowEvent::Key {
                pressed: true,
                keysym: 0xff1b,
                ..
            })
        ) && self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card))
        {
            self.clear_intent(cx)?;
            cx.status = Some(t("Запись отменена пользователем.").to_owned());
            return Ok(());
        }
        if clicked.is_some() && self.rows.first().copied() == clicked {
            self.clear_intent(cx)?;
            self.selected = None;
            self.load(cx);
        }

        let selected_row =
            clicked.and_then(|clicked_id| self.rows.iter().copied().skip(1).position(|row| row == clicked_id));
        if let Some(row_index) = selected_row.filter(|index| self.items.get(*index).is_some()) {
            self.clear_intent(cx)?;
            self.selected = self.items.get(row_index).map(|file| file.name.clone());
            cx.status = self.items.get(row_index).map(|file| tr("Выбран {0}", &[&file.name]));
            self.render(cx)?;
        }

        if clicked.is_some() && clicked == self.upload {
            self.clear_intent(cx)?;
            self.prepare_upload(cx);
        }

        if clicked.is_some() && clicked == self.confirm_check && self.intent.is_some() {
            self.overwrite_confirmed = !self.overwrite_confirmed;
            if let Some(check) = self.confirm_check {
                cx.tree.set_text(
                    check,
                    if self.overwrite_confirmed {
                        t("[✓] Я подтверждаю перезапись")
                    } else {
                        t("[ ] Я подтверждаю перезапись")
                    },
                )?;
            }
        }

        if clicked.is_some() && clicked == self.confirm_cancel {
            self.clear_intent(cx)?;
            cx.status = Some(t("Запись отменена пользователем.").to_owned());
        }

        if clicked.is_some() && clicked == self.confirm_write {
            if !self.overwrite_confirmed {
                cx.status = Some(t("Установите флажок «Я подтверждаю перезапись».").to_owned());
            } else if let Some(intent) = self.intent.take() {
                self.overwrite_confirmed = false;
                if let Some(card) = self.confirm_card {
                    if cx.tree.dialog() == Some(card) {
                        let _ = cx.tree.close_dialog()?;
                    } else {
                        cx.tree.set_visible(card, false)?;
                    }
                }
                // The prepared intent belongs to the game that was selected when it was checked.
                if !intent_is_for_selected_game(cx.app.selected_game(), intent.app_id) {
                    cx.status = Some(t("Выбранная игра изменилась; подтверждение отменено.").to_owned());
                    return Ok(());
                }
                cx.status = Some(tr("Запись {0} в Steam Cloud (RemoteStorage)...", &[&intent.remote]));
                self.upload(cx, intent);
            }
        }

        if clicked.is_some() && clicked == self.download {
            let Some(selected_id) = self.selected.as_deref() else {
                cx.status = Some(t("Сначала выберите файл Steam Cloud").to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
                self.selected = None;
                cx.status = Some(t("Выбранный облачный файл исчез после обновления списка").to_owned());
                return Ok(());
            };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
            // Never touch the open save: a cloud file goes into the configured backup folder as a new file.
            // Replacing a local save from the cloud needs the checks of ACCEPTANCE §18.3 (same name, same game,
            // valid save, journaled backup) and is done by the save writer, not here.
            let downloads = self.backup_workspace.backup_directory().join("cloud_downloads");
            let remote = item.name.clone();
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
                let result = (|| {
                    let source = cloud_read(app_id, &remote)?;
                    let name = std::path::Path::new(&remote)
                        .file_name()
                        .ok_or_else(|| t("Облачный файл без имени").to_owned())?;
                    std::fs::create_dir_all(&downloads).map_err(|error| error.to_string())?;
                    let target = downloads.join(name);
                    let temp = downloads.join(format!(".{}.download.tmp", name.to_string_lossy()));
                    let written = (|| {
                        use std::io::Write;
                        let mut file = std::fs::File::create(&temp)?;
                        file.write_all(&source)?;
                        file.sync_all()?;
                        if target.exists() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::AlreadyExists,
                                tr("{0} уже есть", &[&target.display()]),
                            ));
                        }
                        std::fs::rename(&temp, &target)
                    })();
                    if let Err(error) = written {
                        let _ = std::fs::remove_file(&temp);
                        return Err(error.to_string());
                    }
                    Ok(tr(
                        "Файл скачан отдельно, открытый сейв не тронут: {0}",
                        &[&target.display()],
                    ))
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Cloud,
                    Box::new(CloudReply::Done(result)),
                ));
            }) {
                cx.status = Some(tr("Ошибка: {0}", &[&error]));
            }
        }

        if let Message::User(AppMessage::ToScreen(ScreenId::Cloud, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<CloudReply>() {
                match reply {
                    CloudReply::List(Ok(items)) => {
                        if self
                            .selected
                            .as_ref()
                            .is_some_and(|id| !items.iter().any(|item| &item.name == id))
                        {
                            self.selected = None;
                            self.clear_intent(cx)?;
                        }
                        self.items.clone_from(items);
                        self.render(cx)?;
                    }
                    CloudReply::List(Err(error)) => {
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, &tr("Ошибка загрузки списка: {0}", &[error]))?;
                        }
                    }
                    CloudReply::Prepared(Ok(intent)) => {
                        // The selection may have moved while the comparison ran: never confirm a write for another game.
                        if cx.app.selected_game().and_then(app_id) != Some(intent.app_id) {
                            cx.status = Some(t("Выбор игры изменился; запись в облако отменена.").to_owned());
                            return Ok(());
                        }
                        // The dialog does not repeat the game title or file name, so the status line names both.
                        if let Some(game) = cx.app.selected_game() {
                            cx.status = Some(format!("{game} · {}", intent.remote));
                        }
                        if let Some(label) = self.confirm_cloud_version {
                            cx.tree.set_text(
                                label,
                                &tr(
                                    "Облако Steam: {0} Б · {1}",
                                    &[&intent.cloud_size, &format_epoch_timestamp(intent.cloud_timestamp)],
                                ),
                            )?;
                        }
                        if let Some(label) = self.confirm_local_version {
                            cx.tree.set_text(
                                label,
                                &tr(
                                    "Локальный сейв: {0} Б · {1}",
                                    &[&intent.local_size, &format_epoch_timestamp(intent.local_timestamp)],
                                ),
                            )?;
                        }
                        if let Some(label) = self.confirm_backup_directory {
                            cx.tree.set_text(
                                label,
                                &tr("Папка резервных копий: {0}", &[&intent.backup_directory.display()]),
                            )?;
                        }
                        self.intent = Some(intent.clone());
                        self.overwrite_confirmed = false;
                        if let Some(check) = self.confirm_check {
                            cx.tree.set_text(check, t("[ ] Я подтверждаю перезапись"))?;
                        }
                        if self.upload.is_some_and(|upload| cx.tree.is_visible(upload)) {
                            if let Some(card) = self.confirm_card {
                                cx.tree.open_dialog(card)?;
                            }
                        }
                    }
                    CloudReply::Prepared(Err(error)) => cx.status = Some(error.clone()),
                    CloudReply::Done(Ok(text)) => {
                        cx.status = Some(text.clone());
                        self.clear_intent(cx)?;
                        self.load(cx);
                    }
                    CloudReply::Done(Err(error)) => {
                        cx.status = Some(error.clone());
                        self.clear_intent(cx)?;
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
enum UpdateReply {
    Checked(std::result::Result<(sse_update::UpdateState, String, Option<sse_update::UpdateArtifact>), String>),
    Progress(u64, u64),
    Downloaded(std::result::Result<(sse_update::UpdateArtifact, PathBuf), String>),
    Installed(std::result::Result<sse_update::UpdateInstallResult, String>),
}

fn update_install_status(result: &sse_update::UpdateInstallResult) -> String {
    match result.state {
        sse_update::UpdateInstallState::ManualInstructions => tr(
            "Портативное обновление проверено: {0}. Закройте редактор, распакуйте архив в папку установки и запустите sse-shell из этой папки.",
            &[&result.message],
        ),
        sse_update::UpdateInstallState::Succeeded => t("Обновление установлено.").to_owned(),
        sse_update::UpdateInstallState::OpenedExternally => {
            t("Открыт проверенный установщик. Завершите установку в его окне.").to_owned()
        }
        sse_update::UpdateInstallState::Cancelled => t("Установка обновления отменена.").to_owned(),
        sse_update::UpdateInstallState::Failed => tr("Не удалось установить обновление: {0}", &[&result.message]),
    }
}

#[derive(Default)]
struct Updates {
    status: Option<WidgetId>,
    badge: Option<WidgetId>,
    latest: Option<WidgetId>,
    check: Option<WidgetId>,
    download: Option<WidgetId>,
    install: Option<WidgetId>,
    artifact: Option<sse_update::UpdateArtifact>,
    downloaded: Option<PathBuf>,
    busy: bool,
}

impl Updates {
    fn check(&mut self, cx: &mut Context<'_>) {
        if self.busy {
            return;
        }
        self.busy = true;
        if let Some(id) = self.status {
            let _ = cx.tree.set_text(id, t("Проверка наличия обновлений..."));
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        let language = crate::strings::current_language();
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-read", move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let detected = sse_update::UpdateInstallationDetector::detect(None, None, None)
                    .map_err(|e| tr_in(language, "Обновления недоступны: {0}", &[&e]))?;
                let service = sse_update::UpdateService::new(env!("CARGO_PKG_VERSION"), detected);
                let mut fetch = sse_update::DefaultFetch;
                let check = service.check(&mut fetch);
                if matches!(check.state, sse_update::UpdateState::Unavailable) {
                    return Ok((check.state, String::new(), None));
                }
                if let Some(error) = check.error {
                    return Err(error);
                }
                let version = check.manifest.as_ref().map_or_else(String::new, |m| m.version.clone());
                Ok((check.state, version, check.artifact))
            }))
            .unwrap_or_else(|_| Err(t("Ошибка").to_owned()));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Checked(result)),
            ));
        }) {
            self.busy = false;
            if let Some(id) = self.status {
                let _ = cx.tree.set_text(id, &tr("Ошибка сервера обновлений: {0}", &[&error]));
            }
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }

    fn download(&mut self, cx: &mut Context<'_>) {
        if self.busy {
            return;
        }
        let Some(artifact) = self.artifact.clone() else {
            cx.status = Some(t("Нет пакета для этой установки").to_owned());
            return;
        };
        self.busy = true;
        if let Some(id) = self.status {
            let _ = cx.tree.set_text(id, t("Скачивание пакета обновления..."));
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let detected =
                    sse_update::UpdateInstallationDetector::detect(None, None, None).map_err(|e| e.to_string())?;
                let service = sse_update::UpdateService::new(env!("CARGO_PKG_VERSION"), detected);
                let mut fetch = sse_update::DefaultFetch;
                let directory = sse_app::paths::update_download_directory();
                sse_update::prepare_private_directory(&directory).map_err(|e| e.to_string())?;
                let path = directory.join(&artifact.file);
                let progress_proxy = proxy.clone();
                let mut progress = move |downloaded: u64, total: u64| {
                    progress_proxy.send(AppMessage::ToScreen(
                        ScreenId::Updates,
                        Box::new(UpdateReply::Progress(downloaded, total)),
                    ));
                };
                service
                    .download(&mut fetch, &artifact, &path, Some(&mut progress))
                    .map_err(|e| e.to_string())?;
                Ok((artifact, path))
            }))
            .unwrap_or_else(|_| Err(t("Ошибка").to_owned()));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Downloaded(result)),
            ));
        }) {
            self.busy = false;
            if let Some(id) = self.status {
                let _ = cx.tree.set_text(id, &tr("Ошибка скачивания: {0}", &[&error]));
            }
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }

    fn install(&mut self, cx: &mut Context<'_>) {
        if self.busy {
            return;
        }
        let (Some(artifact), Some(path)) = (self.artifact.clone(), self.downloaded.clone()) else {
            cx.status = Some(t("Сначала скачайте обновление.").to_owned());
            return;
        };
        self.busy = true;
        if let Some(id) = self.status {
            let _ = cx.tree.set_text(id, t("Установка обновления..."));
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        if let Err(error) = sse_app::tasks::try_spawn_named_detached("companion-write", move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let detected =
                    sse_update::UpdateInstallationDetector::detect(None, None, None).map_err(|e| e.to_string())?;
                let service = sse_update::UpdateService::new(env!("CARGO_PKG_VERSION"), detected);
                let mut runner = sse_update::SystemProcessRunner;
                let done = service
                    .install(&artifact, &path, &mut runner)
                    .map_err(|e| e.to_string())?;
                Ok(done)
            }))
            .unwrap_or_else(|_| Err(t("Ошибка").to_owned()));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Installed(result)),
            ));
        }) {
            self.busy = false;
            if let Some(id) = self.status {
                let _ = cx.tree.set_text(id, &tr("Ошибка запуска установки: {0}", &[&error]));
            }
            cx.status = Some(tr("Ошибка: {0}", &[&error]));
        }
    }
}

impl Screen for Updates {
    fn id(&self) -> ScreenId {
        ScreenId::Updates
    }
    fn subtitle(&self) -> &str {
        t("Проверка, загрузка и установка новой версии приложения")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, t("ОБНОВЛЕНИЕ ПРИЛОЖЕНИЯ"), Text::Heading)?;
        style::label(
            cx.tree,
            card,
            &tr("ТЕКУЩАЯ ВЕРСИЯ: {0}", &[&env!("CARGO_PKG_VERSION")]),
            Text::Value,
        )?;
        self.latest = Some(style::label(cx.tree, card, t("ПОСЛЕДНЯЯ ВЕРСИЯ: —"), Text::Value)?);
        self.badge = Some(style::label(cx.tree, card, t("Статус неизвестен"), Text::Body)?);
        self.status = Some(style::label(cx.tree, card, "", Text::Note)?);
        let row = style::row(cx.tree, card)?;
        self.check = Some(style::button(
            cx.tree,
            row,
            t("ПРОВЕРИТЬ ОБНОВЛЕНИЯ"),
            Button::Secondary,
        )?);
        self.download = Some(style::button(cx.tree, row, t("СКАЧАТЬ ОБНОВЛЕНИЕ"), Button::Secondary)?);
        self.install = Some(style::button(
            cx.tree,
            row,
            t("УСТАНОВИТЬ ОБНОВЛЕНИЕ"),
            Button::Primary,
        )?);
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.check {
            self.check(cx);
        }
        if clicked.is_some() && clicked == self.download {
            self.download(cx);
        }
        if clicked.is_some() && clicked == self.install {
            self.install(cx);
        }

        if let Message::User(AppMessage::ToScreen(ScreenId::Updates, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<UpdateReply>() {
                if matches!(reply, UpdateReply::Progress(_, _)) {
                    if let UpdateReply::Progress(downloaded, total) = reply {
                        let percent = if *total == 0 {
                            0
                        } else {
                            downloaded.saturating_mul(100).checked_div(*total).unwrap_or(0).min(100)
                        };
                        if let Some(id) = self.status {
                            cx.tree
                                .set_text(id, &tr("Скачивание пакета обновления... {0}%", &[&percent]))?;
                        }
                    }
                    return Ok(());
                }
                self.busy = false;
                match reply {
                    UpdateReply::Progress(_, _) => {}
                    UpdateReply::Checked(Ok((state, version, artifact))) => {
                        // A package is offered for download and install only when the check said it is newer.
                        // Invalid and DowngradeRefused keep the error text, but no package may be used.
                        self.artifact = offered_artifact(*state, artifact);
                        self.downloaded = None;
                        if let Some(id) = self.latest {
                            let version = if version.is_empty() { "—" } else { version };
                            cx.tree.set_text(id, &tr("ПОСЛЕДНЯЯ ВЕРСИЯ: {0}", &[&version]))?;
                        }
                        let (badge, status) = match state {
                            sse_update::UpdateState::Current => (
                                t("У вас актуальная версия"),
                                t("Установлена последняя версия приложения.").to_owned(),
                            ),
                            sse_update::UpdateState::Available => {
                                (t("Доступно обновление"), tr("Доступна новая версия {0}!", &[version]))
                            }
                            sse_update::UpdateState::Unavailable => (
                                t("Обновление недоступно"),
                                t("Не удалось проверить обновления.").to_owned(),
                            ),
                            sse_update::UpdateState::Invalid | sse_update::UpdateState::DowngradeRefused => (
                                t("Ошибка проверки манифеста"),
                                t("Проверка завершилась с ошибкой.").to_owned(),
                            ),
                        };
                        if let Some(id) = self.badge {
                            cx.tree.set_text(id, badge)?;
                        }
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &status)?;
                        }
                        cx.status = Some(status);
                    }
                    UpdateReply::Checked(Err(error)) => {
                        self.artifact = None;
                        if let Some(id) = self.badge {
                            cx.tree.set_text(id, t("Обновление недоступно"))?;
                        }
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &tr("Ошибка сервера обновлений: {0}", &[error]))?;
                        }
                        cx.status = Some(t("Ошибка подключения к серверу обновлений.").to_owned());
                    }
                    UpdateReply::Downloaded(Ok((artifact, path))) => {
                        self.artifact = Some(artifact.clone());
                        self.downloaded = Some(path.clone());
                        let text = tr("Пакет обновления скачан: {0}", &[&path.display()]);
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &text)?;
                        }
                        cx.status = Some(text);
                    }
                    UpdateReply::Downloaded(Err(error)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &tr("Ошибка скачивания: {0}", &[error]))?;
                        }
                        cx.status = Some(t("Не удалось завершить скачивание.").to_owned());
                    }
                    UpdateReply::Installed(Ok(result)) => {
                        let text = update_install_status(result);
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &text)?;
                        }
                        cx.status = Some(text);
                    }
                    UpdateReply::Installed(Err(error)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &tr("Ошибка запуска установки: {0}", &[error]))?;
                        }
                        cx.status = Some(t("Не удалось запустить установку.").to_owned());
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod service_localization_tests {
    use super::super::saves::Workspace;
    use super::super::{Context, Screen};
    use super::intent_is_for_selected_game;

    #[test]
    fn cloud_write_is_refused_when_the_selected_game_changed_after_the_check() {
        let checked = 4_500;
        assert!(intent_is_for_selected_game(Some("soc"), checked));
        assert!(!intent_is_for_selected_game(Some("cop"), checked));
        assert!(!intent_is_for_selected_game(None, checked));
    }

    use super::{hotkey_label, t_in, tr_in};

    #[test]
    fn a_package_is_offered_only_when_the_check_found_a_newer_version() {
        let artifact = sse_update::UpdateArtifact {
            target: "linux-deb-amd64".to_owned(),
            architecture: "x86_64".to_owned(),
            kind: "package".to_owned(),
            file: "SaveEditor.deb".to_owned(),
            size: 1,
            sha256: "00".repeat(32),
            url: "https://example.invalid/SaveEditor.deb".to_owned(),
            release_version: "2.0.0".to_owned(),
        };
        assert!(super::offered_artifact(sse_update::UpdateState::Available, &Some(artifact.clone())).is_some());
        for refused in [
            sse_update::UpdateState::Invalid,
            sse_update::UpdateState::DowngradeRefused,
            sse_update::UpdateState::Current,
            sse_update::UpdateState::Unavailable,
        ] {
            assert!(super::offered_artifact(refused, &Some(artifact.clone())).is_none());
        }
    }
    use crate::event_loop::{Message, WindowEvent};
    use sse_steam::api::CloudFile;

    #[test]
    fn cloud_upload_is_off_and_sends_nothing_without_a_chosen_file() -> sse_core::Result<()> {
        let fonts = crate::glyphs::Fonts::bundled()?;
        let mut tree = crate::widget::Tree::new(fonts, crate::screens::style::rgb(crate::theme::BG_BASE));
        let host = tree.add(
            None,
            crate::layout::NodeKind::Column,
            crate::layout::Style::default(),
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let mut app = sse_app::AppState::new();
        let mut screen = super::Cloud::with_backup_workspace(Workspace::default());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let upload = screen
            .upload
            .ok_or_else(|| sse_core::Error::damaged("no upload action"))?;
        assert!(
            !cx.tree.is_enabled(upload)?,
            "upload is on without a chosen file and a save"
        );
        // Even a click that reaches the handler opens no confirmation and sends nothing.
        let pointer = Message::Window(WindowEvent::PointerLeft);
        let _ = screen.message(&mut cx, &pointer, Some(upload));
        assert!(screen.intent.is_none(), "upload prepared a write without a chosen file");
        Ok(())
    }

    #[test]
    fn cloud_file_row_then_download_acts_on_the_chosen_file() -> sse_core::Result<()> {
        // Nothing is read from Steam here: the list is filled directly. Choosing a file enables the download of that file only.
        let fonts = crate::glyphs::Fonts::bundled()?;
        let mut tree = crate::widget::Tree::new(fonts, crate::screens::style::rgb(crate::theme::BG_BASE));
        let host = tree.add(
            None,
            crate::layout::NodeKind::Column,
            crate::layout::Style::default(),
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let mut app = sse_app::AppState::new();
        let mut screen = super::Cloud::with_backup_workspace(Workspace::default());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let download = screen
            .download
            .ok_or_else(|| sse_core::Error::damaged("no download action"))?;
        assert!(!cx.tree.is_enabled(download)?, "download is on before a file is chosen");
        screen.items = vec![
            CloudFile {
                name: "alpha.sav".to_owned(),
                size: 2048,
                timestamp: 0,
                persisted: true,
                exists: true,
            },
            CloudFile {
                name: "beta.sav".to_owned(),
                size: 4096,
                timestamp: 0,
                persisted: true,
                exists: true,
            },
        ];
        screen.render(&mut cx)?;
        let select = screen
            .rows
            .get(2)
            .copied()
            .ok_or_else(|| sse_core::Error::damaged("no row for the second file"))?;
        let pointer = Message::Window(WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        assert_eq!(screen.selected.as_deref(), Some("beta.sav"));
        assert!(
            cx.tree.is_enabled(download)?,
            "download stays off after choosing a file"
        );
        Ok(())
    }

    #[test]
    fn steam_and_companion_labels_keep_dynamic_values_when_translated() {
        assert_eq!(hotkey_label("en", sse_companion::hotkeys::HotkeyAction::Heal), "Heal");
        assert_eq!(
            t_in("en", "Для выбранной игры нет Steam App ID"),
            "The selected game has no Steam App ID"
        );

        let size = 2_048_u64;
        let date = t_in("en", "дата неизвестна");
        assert_eq!(
            tr_in("en", "Облако Steam: {0} Б · {1}", &[&size, &date]),
            "Steam Cloud: 2048 B · date unknown"
        );
    }
}

#[cfg(test)]
mod update_tests {
    use super::{UpdateReply, Updates};
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::{AppMessage, Context, Screen, ScreenId};
    use crate::widget::{Content, Look, Tree};
    use crate::{event_loop::Message, screens::style};

    #[test]
    fn portable_install_status_shows_path_and_manual_steps() -> sse_core::Result<()> {
        let path = r"C:\Users\Player\Downloads\SaveEditor.zip";
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let status = style::label(&mut tree, host, "", style::Text::Note)?;
        let mut screen = Updates {
            status: Some(status),
            ..Updates::default()
        };
        let mut app = sse_app::AppState::new();
        let message = Message::User(AppMessage::ToScreen(
            ScreenId::Updates,
            Box::new(UpdateReply::Installed(Ok(sse_update::UpdateInstallResult {
                state: sse_update::UpdateInstallState::ManualInstructions,
                exit_code: None,
                message: path.to_owned(),
            }))),
        ));
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };

        screen.message(&mut context, &message, None)?;

        let displayed = context.tree.text(status)?;
        let expected = super::tr_in(
            crate::strings::current_language(),
            "Портативное обновление проверено: {0}. Закройте редактор, распакуйте архив в папку установки и запустите sse-shell из этой папки.",
            &[&path],
        );
        assert!(displayed.contains(path));
        assert!(displayed.contains("sse-shell"));
        assert_eq!(displayed, expected);
        assert_eq!(context.status.as_deref(), Some(displayed));
        Ok(())
    }
}

#[cfg(test)]
mod cloud_intent_guard_tests {
    use super::app_id;

    #[test]
    fn a_prepared_write_is_tied_to_one_game_app_id() {
        assert_eq!(app_id("stalker-cs"), Some(20_510));
        assert_ne!(app_id("stalker-cop"), app_id("stalker-cs"));
    }
}

#[cfg(test)]
mod game_directory_tests {
    use super::{in_game_directory, save_family};
    use sse_storage::discovery::SaveDirectoryCandidate;
    use std::path::Path;

    #[test]
    fn a_local_save_counts_only_inside_the_selected_games_folder() {
        let directories = [
            SaveDirectoryCandidate::new("clear_sky", "stalker-cs", "/games/cs/SaveGames"),
            SaveDirectoryCandidate::new("cop", "stalker-cop", "/games/cop/SaveGames"),
        ];
        let clear_sky = save_family("stalker-cs-ee");
        assert_eq!(clear_sky, Some("clear_sky"));
        assert!(in_game_directory(
            Path::new("/games/cs/SaveGames/slot.sav"),
            clear_sky,
            &directories
        ));
        assert!(!in_game_directory(
            Path::new("/games/cop/SaveGames/slot.sav"),
            clear_sky,
            &directories
        ));
        assert!(!in_game_directory(
            Path::new("/elsewhere/slot.sav"),
            clear_sky,
            &directories
        ));
        assert!(!in_game_directory(
            Path::new("/games/cs/SaveGames/slot.sav"),
            None,
            &directories
        ));
    }
}

#[cfg(test)]
mod achievement_list_tests {
    use super::{Achievement, Achievements, Context, Message, Screen};
    use crate::event_loop::WindowEvent;
    use sse_core::Result;

    fn achievement(name: &str, achieved: bool) -> Achievement {
        Achievement {
            name: name.to_owned(),
            display_name: format!("Title {name}"),
            description: format!("Description {name}"),
            hidden: false,
            achieved,
            unlock_time: 0,
        }
    }

    #[test]
    fn a_row_selects_its_achievement_and_the_side_actions_follow_that_choice() -> Result<()> {
        // Nothing is read from Steam here: the list is filled directly. Choosing a row enables both side actions.
        let fonts = crate::glyphs::Fonts::bundled()?;
        let mut tree = crate::widget::Tree::new(fonts, crate::screens::style::rgb(crate::theme::BG_BASE));
        let host = tree.add(
            None,
            crate::layout::NodeKind::Column,
            crate::layout::Style::default(),
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let mut app = sse_app::AppState::new();
        let mut screen = Achievements::default();
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let set = screen.set.ok_or_else(|| sse_core::Error::damaged("no set action"))?;
        let clear = screen
            .clear
            .ok_or_else(|| sse_core::Error::damaged("no clear action"))?;
        assert!(
            !cx.tree.is_enabled(set)? && !cx.tree.is_enabled(clear)?,
            "actions are on before a choice"
        );
        screen.items = vec![achievement("first", false), achievement("second", true)];
        screen.render(&mut cx)?;
        let select = screen
            .list
            .as_ref()
            .and_then(|list| list.rows.get(1))
            .map(|row| row.select)
            .ok_or_else(|| sse_core::Error::damaged("no row for the second achievement"))?;
        let pointer = Message::Window(WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        assert_eq!(screen.selected.as_deref(), Some("second"));
        assert!(
            cx.tree.is_enabled(set)? && cx.tree.is_enabled(clear)?,
            "actions stay off after a choice"
        );
        Ok(())
    }
}
