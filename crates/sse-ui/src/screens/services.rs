//! S5: companion, achievements, Steam Cloud, and editor updates.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::{Message, WindowEvent};
use crate::widget::WidgetId;
use sse_core::Result;
use sse_steam::api::{Achievement, CloudFile};
use sse_steam::protocol::{Request, Response};
use std::path::{Path, PathBuf};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const ROWS: usize = 8;

/// Screens implemented by the S5 services package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Companion::default()),
        Box::new(Achievements::default()),
        Box::new(Cloud::default()),
        Box::new(Updates::default()),
    ]
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

fn xray_game(game: &str) -> Option<sse_companion::bundled::Game> {
    match game {
        "soc" | "stalker-soc" => Some(sse_companion::bundled::Game::ShadowOfChernobyl),
        "cs" | "clear_sky" | "stalker-cs" => Some(sse_companion::bundled::Game::ClearSky),
        "cop" | "stalker-cop" => Some(sse_companion::bundled::Game::CallOfPripyat),
        _ => None,
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
        return Err("Не найдена папка UE4SS Mods для S.T.A.L.K.E.R. 2".to_owned());
    }
    Err("Компаньон для выбранного издания не поддерживается".to_owned())
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

fn worker(request: &Request) -> std::result::Result<Vec<u8>, String> {
    let Response { ok, payload } =
        sse_steam::worker::run_sibling_worker(request, TIMEOUT).map_err(|e| e.to_string())?;
    if ok {
        Ok(payload)
    } else {
        Err(String::from_utf8(payload).unwrap_or_else(|_| "Steam worker failed".to_owned()))
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> std::result::Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| "Steam response overflow".to_owned())?;
        let value = self
            .bytes
            .get(self.pos..end)
            .ok_or_else(|| "Truncated Steam response".to_owned())?;
        self.pos = end;
        Ok(value)
    }
    fn u8(&mut self) -> std::result::Result<u8, String> {
        self.take(1)?
            .first()
            .copied()
            .ok_or_else(|| "Truncated Steam response".to_owned())
    }
    fn u16(&mut self) -> std::result::Result<u16, String> {
        let mut b = [0; 2];
        b.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(b))
    }
    fn u32(&mut self) -> std::result::Result<u32, String> {
        let mut b = [0; 4];
        b.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(b))
    }
    fn u64(&mut self) -> std::result::Result<u64, String> {
        let mut b = [0; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(b))
    }
    fn i64(&mut self) -> std::result::Result<i64, String> {
        let mut b = [0; 8];
        b.copy_from_slice(self.take(8)?);
        Ok(i64::from_le_bytes(b))
    }
    fn string(&mut self) -> std::result::Result<String, String> {
        let n = usize::from(self.u16()?);
        String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "Steam response is not UTF-8".to_owned())
    }
}

fn cloud_files(bytes: &[u8]) -> std::result::Result<Vec<CloudFile>, String> {
    let mut c = Cursor::new(bytes);
    let count = usize::try_from(c.u32()?).map_err(|_| "Cloud count overflow".to_owned())?;
    if count > 10_000 {
        return Err("Cloud response exceeds 10,000 files".to_owned());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(CloudFile {
            name: c.string()?,
            size: c.u64()?,
            timestamp: c.i64()?,
        });
    }
    Ok(out)
}

fn achievement_list(bytes: &[u8]) -> std::result::Result<Vec<Achievement>, String> {
    let mut c = Cursor::new(bytes);
    let count = usize::try_from(c.u32()?).map_err(|_| "Achievement count overflow".to_owned())?;
    if count > 10_000 {
        return Err("Achievement response exceeds 10,000 entries".to_owned());
    }
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(Achievement {
            name: c.string()?,
            display_name: c.string()?,
            description: c.string()?,
            hidden: c.u8()? != 0,
            achieved: c.u8()? != 0,
            unlock_time: c.u32()?,
        });
    }
    Ok(out)
}

fn clip(text: &str) -> String {
    if text.chars().count() <= 80 {
        text.to_owned()
    } else {
        text.chars().take(79).chain(std::iter::once('…')).collect()
    }
}

#[derive(Debug)]
enum CompanionReply {
    Status(std::result::Result<Option<String>, String>),
    Protocol(&'static str, std::result::Result<String, String>),
    Changed(std::result::Result<String, String>),
    Hotkeys(std::result::Result<String, String>),
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
    manual_path: Option<WidgetId>,
    apply_manual: Option<WidgetId>,
    manual_directory: Option<PathBuf>,
    hotkey_inputs: Vec<(sse_companion::hotkeys::HotkeyAction, WidgetId)>,
    save_hotkeys: Option<WidgetId>,
    default_hotkeys: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<CompanionIntent>,
}

impl Companion {
    fn selected(&self, cx: &Context<'_>) -> std::result::Result<(String, PathBuf), String> {
        let game = cx
            .app
            .selected_game()
            .map(str::to_owned)
            .ok_or_else(|| "Игра не выбрана".to_owned())?;
        let directory = self
            .manual_directory
            .clone()
            .or_else(|| cx.app.game_dir().map(Path::to_path_buf))
            .ok_or_else(|| "Папка игры не выбрана".to_owned())?;
        Ok((game, directory))
    }
    fn exchange_directory(game: &str, directory: &Path) -> std::result::Result<PathBuf, String> {
        if xray_game(game).is_none() {
            return Err("появится после протокола компаньона".to_owned());
        }
        for relative in ["_appdata_", "appdata", "userdata"] {
            let candidate = directory.join(relative);
            if candidate.is_dir() {
                return Ok(candidate);
            }
        }
        Err("появится после протокола компаньона".to_owned())
    }
    fn refresh(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let selected = self.selected(cx);
        std::thread::spawn(move || {
            let result = selected.and_then(|(g, d)| companion_root(&g, &d).map(|r| installed_version(&r)));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Companion,
                Box::new(CompanionReply::Status(result)),
            ));
        });
    }
    fn protocol(&self, cx: &mut Context<'_>, command: &'static str) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let selected = self.selected(cx);
        std::thread::spawn(move || {
            let result = selected
                .and_then(|(game, directory)| Self::exchange_directory(&game, &directory))
                .and_then(|directory| {
                    sse_companion::protocol::CompanionClient::new(directory)
                        .send(command, &[], Duration::from_secs(3))
                        .map(|reply| reply.text)
                        .map_err(|e| e.to_string())
                });
            proxy.send(AppMessage::ToScreen(
                ScreenId::Companion,
                Box::new(CompanionReply::Protocol(command, result)),
            ));
        });
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
        "Мод-компаньон, версия и горячие клавиши"
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "МОД-КОМПАНЬОН", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "Меню в игре: Esc → F1 или КПК компаньона. Установка через приложение ниже.",
            Text::Note,
        )?;
        style::label(cx.tree, card, "Целевая игра: выбранная в «Обзоре игр»", Text::Body)?;
        style::label(cx.tree, card, "СТАТУС И СВЯЗЬ", Text::Heading)?;
        self.status = Some(style::label(cx.tree, card, "НЕ УСТАНОВЛЕН", Text::Value)?);
        self.version = Some(style::label(cx.tree, card, "Версия мода: —", Text::Note)?);
        self.latency = Some(style::label(cx.tree, card, "Связь / Задержка: Нет ответа", Text::Note)?);
        self.path = Some(style::label(cx.tree, card, "Путь установки: —", Text::Note)?);
        let row = style::row(cx.tree, card)?;
        self.install = Some(style::button(cx.tree, row, "УСТАНОВИТЬ / ОБНОВИТЬ", Button::Primary)?);
        self.remove = Some(style::button(cx.tree, row, "УДАЛИТЬ", Button::Secondary)?);
        self.ping = Some(style::button(cx.tree, row, "ПРОВЕРИТЬ СВЯЗЬ", Button::Secondary)?);
        self.refresh_button = Some(style::button(cx.tree, row, "ОБНОВИТЬ СТАТУС", Button::Secondary)?);
        let live = style::card(cx.tree, host)?;
        style::label(cx.tree, live, "ЖИВОЙ ИНСПЕКТОР", Text::Heading)?;
        style::label(
            cx.tree,
            live,
            "Показываются только ответы протокола Companion: info и list_inventory.",
            Text::Note,
        )?;
        self.inspect = Some(style::button(cx.tree, live, "ПОЛУЧИТЬ ДАННЫЕ", Button::Secondary)?);
        self.info = Some(style::label(cx.tree, live, "Информация игрока: —", Text::Body)?);
        self.inventory = Some(style::label(cx.tree, live, "Инвентарь игрока: —", Text::Body)?);
        style::label(
            cx.tree,
            live,
            "Для живой проверки нужен установленный Companion-протокол.",
            Text::Note,
        )?;
        let s2 = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            s2,
            "S.T.A.L.K.E.R. 2 — команды игры (экспериментально)",
            Text::Heading,
        )?;
        style::label(cx.tree,s2,"Нужны S2 на ПК, UE4SS и установленный мод. Команды выполняет сама игра (XSetGodMode, XSetNoClipGSC, XSetTimeSpeed).",Text::Note)?;
        for label in [
            "Бессмертие: вкл",
            "Бессмертие: выкл",
            "Полёт: вкл",
            "Полёт: выкл",
            "Время ×5",
            "Время: норма",
        ] {
            let id = style::button(cx.tree, s2, label, Button::Secondary)?;
            cx.tree.set_enabled(id, false)?;
        }
        style::label(cx.tree, s2, "появится после протокола компаньона", Text::Note)?;
        let all = style::card(cx.tree, host)?;
        style::label(cx.tree, all, "ВСЕ ИГРЫ", Text::Heading)?;
        for game in [
            "S.T.A.L.K.E.R. Зов Припяти",
            "S.T.A.L.K.E.R. Чистое Небо",
            "S.T.A.L.K.E.R. Тень Чернобыля",
            "Зов Припяти (Enhanced Edition)",
            "Чистое Небо (Enhanced Edition)",
            "Тень Чернобыля (Enhanced Edition)",
            "S.T.A.L.K.E.R. 2 (экспериментально, нужен UE4SS)",
        ] {
            style::label(cx.tree, all, &format!("[ ] {game} · игра не найдена"), Text::Body)?;
        }
        let all_install = style::button(
            cx.tree,
            all,
            "УСТАНОВИТЬ / ОБНОВИТЬ ВО ВСЕ ОТМЕЧЕННЫЕ",
            Button::Secondary,
        )?;
        cx.tree.set_enabled(all_install, false)?;
        style::label(
            cx.tree,
            all,
            "Выбор нескольких установок появится после общего API обнаружения игр.",
            Text::Note,
        )?;
        let manual = style::card(cx.tree, host)?;
        style::label(cx.tree, manual, "ПАПКА ИГРЫ (РУЧНОЙ ВЫБОР)", Text::Heading)?;
        style::label(
            cx.tree,
            manual,
            "Оставьте пустым для автоматического поиска через Steam. Укажите путь вручную, если папка нестандартная.",
            Text::Note,
        )?;
        let manual_row = style::row(cx.tree, manual)?;
        self.manual_path = Some(style::input(cx.tree, manual_row, "")?);
        self.apply_manual = Some(style::button(cx.tree, manual_row, "ПРИМЕНИТЬ", Button::Secondary)?);
        let hot = style::card(cx.tree, host)?;
        style::label(cx.tree, hot, "ГОРЯЧИЕ КЛАВИШИ", Text::Heading)?;
        style::label(cx.tree,hot,"Приложение перехватывает сочетание и отправляет команду моду через файл-протокол. Игра должна быть запущена с установленным модом.",Text::Note)?;
        for action in [
            sse_companion::hotkeys::HotkeyAction::Heal,
            sse_companion::hotkeys::HotkeyAction::RepairEquipped,
            sse_companion::hotkeys::HotkeyAction::Mark,
            sse_companion::hotkeys::HotkeyAction::JumpLast,
            sse_companion::hotkeys::HotkeyAction::QuickSave,
        ] {
            let row = style::row(cx.tree, hot)?;
            style::label(cx.tree, row, action.name(), Text::Body)?;
            let input = style::input(cx.tree, row, "")?;
            self.hotkey_inputs.push((action, input));
        }
        let hot_row = style::row(cx.tree, hot)?;
        self.save_hotkeys = Some(style::button(cx.tree, hot_row, "СОХРАНИТЬ КЛАВИШИ", Button::Primary)?);
        self.default_hotkeys = Some(style::button(cx.tree, hot_row, "ПО УМОЛЧАНИЮ", Button::Secondary)?);
        let confirm = style::card(cx.tree, host)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, "ПОДТВЕРЖДЕНИЕ ИЗМЕНЕНИЯ ИГРЫ", Text::Heading)?;
        style::label(
            cx.tree,
            confirm,
            "Будут изменены файлы выбранной игры. Проверьте игру и папку перед продолжением.",
            Text::Note,
        )?;
        let confirm_row = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, confirm_row, "ПОДТВЕРДИТЬ", Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, confirm_row, "ОТМЕНА", Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
        Ok(())
    }
    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.intent = None;
        if self.confirm_card.is_some() {
            cx.tree.close_dialog().ok();
        }
        self.load_hotkeys(cx, false)?;
        self.refresh(cx);
        Ok(())
    }
    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.refresh_button {
            self.refresh(cx);
            return Ok(());
        }
        if clicked.is_some() && clicked == self.ping {
            cx.status = Some("Проверка связи с модом…".to_owned());
            self.protocol(cx, "ping");
            return Ok(());
        }
        if clicked.is_some() && clicked == self.inspect {
            cx.status = Some("Чтение ответов Companion…".to_owned());
            self.protocol(cx, "info");
            self.protocol(cx, "list_inventory");
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
                cx.status = Some("Папка очищена, используется автообнаружение.".to_owned());
            } else {
                let path = PathBuf::from(&text);
                if path.is_dir() {
                    self.manual_directory = Some(path.clone());
                    cx.status = Some(format!("Папка задана: {}", path.display()));
                } else {
                    cx.status = Some(format!("Папка не найдена: {text}"));
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
            std::thread::spawn(move || {
                let result = sse_companion::hotkeys::HotkeyLayout::parse(&text)
                    .and_then(|layout| layout.save(&path))
                    .map(|()| format!("Клавиши сохранены: {}.", path.display()))
                    .map_err(|e| e.to_string());
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Companion,
                    Box::new(CompanionReply::Hotkeys(result)),
                ));
            });
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
                cx.status = Some("Выбор игры изменился; подтверждение отменено.".to_owned());
                return Ok(());
            }
            if self.confirm_card.is_some() {
                cx.tree.close_dialog()?;
            }
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let root = companion_root(&intent.game, &intent.directory)?;
                    if intent.install {
                        if let Some(target) = xray_game(&intent.game) {
                            sse_companion::installer::install_bundled(&root, target).map_err(|e| e.to_string())?;
                        } else {
                            sse_companion::installer::install_stalker2(&root).map_err(|e| e.to_string())?;
                        }
                        Ok("Компаньон успешно установлен!".to_owned())
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
                            "Компаньон удалён."
                        } else {
                            "Не удалось удалить компаньон."
                        }
                        .to_owned())
                    }
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Companion,
                    Box::new(CompanionReply::Changed(result)),
                ));
            });
            return Ok(());
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Companion, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<CompanionReply>() {
                match reply {
                    CompanionReply::Status(Ok(version)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(
                                id,
                                if version.is_some() {
                                    "УСТАНОВЛЕН (ОЖИДАНИЕ ИГРЫ)"
                                } else {
                                    "НЕ УСТАНОВЛЕН"
                                },
                            )?;
                        }
                        if let Some(id) = self.version {
                            cx.tree
                                .set_text(id, &format!("Версия мода: {}", version.as_deref().unwrap_or("—")))?;
                        }
                        if let (Some(id), Ok((_, dir))) = (self.path, self.selected(cx)) {
                            cx.tree.set_text(id, &format!("Путь установки: {}", dir.display()))?;
                        }
                    }
                    CompanionReply::Status(Err(e)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, "ОШИБКА")?;
                        }
                        cx.status = Some(format!("Ошибка обновления статуса: {e}"));
                    }
                    CompanionReply::Protocol(command, Ok(text)) => match *command {
                        "ping" => {
                            if let Some(id) = self.status {
                                cx.tree.set_text(id, "РАБОТАЕТ (ПОДКЛЮЧЁН)")?;
                            }
                            if let Some(id) = self.latency {
                                cx.tree.set_text(id, "Связь / Задержка: мод отвечает")?;
                            }
                            cx.status = Some(format!("Мод отвечает. {text}"));
                        }
                        "info" => {
                            if let Some(id) = self.info {
                                cx.tree.set_text(id, &format!("Информация игрока: {text}"))?;
                            }
                        }
                        "list_inventory" => {
                            if let Some(id) = self.inventory {
                                cx.tree.set_text(id, &format!("Инвентарь игрока: {text}"))?;
                            }
                        }
                        _ => {}
                    },
                    CompanionReply::Protocol(command, Err(e)) => {
                        if let Some(id) = self.latency {
                            cx.tree.set_text(id, "Связь / Задержка: Нет ответа")?;
                        }
                        cx.status = Some(if *command == "ping" {
                            format!("Ошибка пинга: {e}")
                        } else {
                            format!("Не удалось получить данные Companion: {e}")
                        });
                    }
                    CompanionReply::Changed(Ok(text)) | CompanionReply::Hotkeys(Ok(text)) => {
                        cx.status = Some(text.clone());
                        self.refresh(cx);
                    }
                    CompanionReply::Changed(Err(e)) => cx.status = Some(format!("Ошибка установки: {e}")),
                    CompanionReply::Hotkeys(Err(e)) => cx.status = Some(format!("Клавиши не сохранены: {e}")),
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct AchievementIntent {
    app_id: u32,
    name: String,
    set: bool,
}
#[derive(Default)]
struct Achievements {
    status: Option<WidgetId>,
    rows: Vec<WidgetId>,
    items: Vec<Achievement>,
    selected: Option<String>,
    set: Option<WidgetId>,
    clear: Option<WidgetId>,
    refresh: Option<WidgetId>,
    progress: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<AchievementIntent>,
    pending_set: Option<bool>,
}

impl Achievements {
    fn load(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let id = cx.app.selected_game().and_then(app_id);
        std::thread::spawn(move || {
            let result = id
                .ok_or_else(|| "Для выбранной игры нет Steam App ID".to_owned())
                .and_then(|app_id| worker(&Request::ListAchievements { app_id }))
                .and_then(|b| achievement_list(&b));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Achievements,
                Box::new(AchReply::List(result)),
            ));
        });
    }
    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        for (i, row) in self.rows.iter().copied().skip(1).enumerate() {
            if let Some(a) = self.items.get(i) {
                cx.tree.set_visible(row, true)?;
                cx.tree.set_text(
                    row,
                    &format!("{} {}", if a.achieved { "✓" } else { "○" }, clip(&a.display_name)),
                )?;
            } else {
                cx.tree.set_visible(row, false)?;
            }
        }
        if let Some(id) = self.status {
            cx.tree
                .set_text(id, &format!("Загружено {} достижений.", self.items.len()))?;
        }
        if let Some(id) = self.progress {
            let got = self.items.iter().filter(|item| item.achieved).count();
            let percent = if self.items.is_empty() {
                0.0
            } else {
                (got as f64 * 100.0) / self.items.len() as f64
            };
            cx.tree
                .set_text(id, &format!("{got} из {} получено ({percent:.0}%)", self.items.len()))?;
        }
        Ok(())
    }
}

impl Screen for Achievements {
    fn id(&self) -> ScreenId {
        ScreenId::Achievements
    }
    fn subtitle(&self) -> &str {
        "Достижения выбранной игры и прогресс Steam"
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ДОСТИЖЕНИЯ STEAM", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "Steam доступен / недоступен определяется рабочим процессом Steam.",
            Text::Note,
        )?;
        self.status = Some(style::label(cx.tree, card, "Запрос достижений...", Text::Note)?);
        self.progress = Some(style::label(cx.tree, card, "0 из 0 получено (0%)", Text::Value)?);
        self.refresh = Some(style::button(cx.tree, card, "ОБНОВИТЬ", Button::Secondary)?);
        for _ in 0..ROWS {
            let r = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(r, false)?;
            self.rows.push(r);
        }
        let row = style::row(cx.tree, card)?;
        self.set = Some(style::button(cx.tree, row, "Разблокировать", Button::Primary)?);
        self.clear = Some(style::button(
            cx.tree,
            row,
            crate::strings::t("Сбросить"),
            Button::Secondary,
        )?);
        let confirm = style::card(cx.tree, host)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, "ПОДТВЕРЖДЕНИЕ ДОСТИЖЕНИЯ", Text::Heading)?;
        style::label(
            cx.tree,
            confirm,
            "Изменение будет отправлено в Steam для выбранной игры и достижения.",
            Text::Note,
        )?;
        let actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, actions, "ПОДТВЕРДИТЬ", Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, actions, "ОТМЕНА", Button::Secondary)?);
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
        for (i, row) in self.rows.iter().copied().enumerate() {
            if clicked == Some(row) && self.items.get(i).is_some() {
                self.selected = self.items.get(i).map(|a| a.name.clone());
                self.intent = None;
                if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
                    let _ = cx.tree.close_dialog()?;
                }
                cx.status = self.items.get(i).map(|a| a.description.clone());
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
                cx.status = Some("Сначала выберите достижение".to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
                self.selected = None;
                cx.status = Some("Выбранное достижение исчезло после обновления списка".to_owned());
                return Ok(());
            };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
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
                cx.status = Some("Выбранная игра изменилась; подтверждение отменено.".to_owned());
                return Ok(());
            }
            self.pending_set = Some(intent.set);
            let app_id = intent.app_id;
            let name = intent.name;
            let set = intent.set;
            let _ = cx.tree.close_dialog()?;
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let req = if set {
                    Request::SetAchievement {
                        app_id,
                        name,
                        confirmed: true,
                    }
                } else {
                    Request::ClearAchievement {
                        app_id,
                        name,
                        confirmed: true,
                    }
                };
                let result = worker(&req).map(|_| ());
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Achievements,
                    Box::new(AchReply::Changed(result)),
                ));
            });
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
                                    format!("Достижение «{}» получено в Steam.", item.display_name)
                                } else {
                                    format!("Достижение «{}» снято в Steam.", item.display_name)
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
    items: Vec<CloudFile>,
    selected: Option<String>,
    download: Option<WidgetId>,
    upload: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_check: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<CloudIntent>,
    overwrite_confirmed: bool,
}

impl Cloud {
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
        std::thread::spawn(move || {
            let result = id
                .ok_or_else(|| "Для выбранной игры нет Steam App ID".to_owned())
                .and_then(|app_id| worker(&Request::List { app_id }))
                .and_then(|b| cloud_files(&b));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::List(result)),
            ));
        });
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        for (i, row) in self.rows.iter().copied().enumerate() {
            if let Some(f) = self.items.get(i) {
                cx.tree.set_visible(row, true)?;
                cx.tree
                    .set_text(row, &format!("{} · {} KiB", clip(&f.name), f.size / 1024))?;
            } else {
                cx.tree.set_visible(row, false)?;
            }
        }
        if let Some(id) = self.status {
            cx.tree.set_text(id, &format!("Файлов: {}", self.items.len()))?;
        }
        Ok(())
    }

    fn prepare_upload(&self, cx: &mut Context<'_>) {
        let Some(selected_id) = self.selected.as_deref() else {
            cx.status = Some("Сначала выберите файл Steam Cloud".to_owned());
            return;
        };
        let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
            cx.status = Some("Выбранный облачный файл исчез после обновления списка".to_owned());
            return;
        };
        let Some(game) = cx.app.selected_game() else { return };
        let Some(app_id) = app_id(game) else { return };
        if app_id == sse_steam::discovery::STALKER_2_APP_ID {
            cx.status = Some("Запись S.T.A.L.K.E.R. 2 в Steam Cloud запрещена.".to_owned());
            return;
        }
        let Some(local) = cx.app.current_save().map(Path::to_path_buf) else {
            cx.status = Some("Сначала откройте локальный сейв".to_owned());
            return;
        };
        let remote = item.name.clone();
        let Some(proxy) = cx.proxy.cloned() else { return };
        std::thread::spawn(move || {
            let result = std::fs::read(&local)
                .map_err(|error| format!("Ошибка записи: {error}"))
                .map(|bytes| CloudIntent {
                    app_id,
                    remote,
                    local,
                    local_sha256: sse_codecs::sha256::sha256(&bytes),
                });
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::Prepared(result)),
            ));
        });
    }

    fn upload(&self, cx: &mut Context<'_>, intent: CloudIntent) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        std::thread::spawn(move || {
            let result = (|| {
                let output = std::fs::read(&intent.local).map_err(|error| format!("Ошибка записи: {error}"))?;
                if sse_codecs::sha256::sha256(&output) != intent.local_sha256 {
                    return Ok(
                        "Aborted: Локальный файл изменился после запроса записи; подтвердите запись ещё раз."
                            .to_owned(),
                    );
                }
                let source = worker(&Request::Read {
                    app_id: intent.app_id,
                    remote_name: intent.remote.clone(),
                })?;
                let artifacts = intent
                    .local
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .join("steam-cloud-backups");
                let request = Request::Write {
                    app_id: intent.app_id,
                    remote_name: intent.remote.clone(),
                    expected_source_sha256: sse_codecs::sha256::sha256(&source),
                    artifact_directory: artifacts,
                    output,
                };
                match sse_steam::worker::run_sibling_worker(&request, TIMEOUT) {
                    Ok(Response { ok: true, payload }) => match payload.first().copied() {
                        Some(0) => Ok(format!("Verified: Записано и проверено: {}", intent.remote)),
                        Some(1) => Ok(format!(
                            "Uncertain: Результат записи не подтверждён (повтор не выполняется): {}",
                            intent.remote
                        )),
                        _ => Ok(format!(
                            "Uncertain: Результат записи не подтверждён (повтор не выполняется): {}",
                            intent.remote
                        )),
                    },
                    Ok(Response { ok: false, payload }) => Ok(format!(
                        "Aborted: Запись отменена: {}",
                        String::from_utf8(payload).unwrap_or_else(|_| "Steam отклонил запись".to_owned())
                    )),
                    Err(sse_steam::worker::WorkerProcessError::Timeout {
                        write_outcome_uncertain: true,
                    }) => Ok(format!(
                        "Uncertain: Результат записи не подтверждён (повтор не выполняется): {}",
                        intent.remote
                    )),
                    Err(error) => Err(format!("Ошибка записи: {error}")),
                }
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Cloud,
                Box::new(CloudReply::Done(result)),
            ));
        });
    }
}

impl Screen for Cloud {
    fn id(&self) -> ScreenId {
        ScreenId::Cloud
    }

    fn subtitle(&self) -> &str {
        "Steam Cloud: список, локальная копия и защищённая запись"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "STEAM CLOUD", Text::Heading)?;
        self.status = Some(style::label(
            cx.tree,
            card,
            "Список не загружен. Нажмите «ОБНОВИТЬ СПИСОК».",
            Text::Note,
        )?);
        let refresh = style::button(cx.tree, card, "ОБНОВИТЬ СПИСОК", Button::Secondary)?;
        self.download = Some(style::button(cx.tree, card, "СКАЧАТЬ В ЛОКАЛЬНЫЕ", Button::Secondary)?);
        self.upload = Some(style::button(cx.tree, card, "ЗАПИСАТЬ В ОБЛАКО...", Button::Primary)?);
        self.rows.push(refresh);
        for _ in 0..ROWS {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }

        let overlay = cx.tree.overlay_host().unwrap_or(host);
        let confirm = style::card(cx.tree, overlay)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, "ПОДТВЕРЖДЕНИЕ ЗАПИСИ", Text::Heading)?;
        style::label(
            cx.tree,
            confirm,
            "Внимание: локальный файл будет отправлен в Steam Cloud и перезапишет облачное сохранение. Резервная копия будет сохранена в бэкапы.",
            Text::Body,
        )?;
        self.confirm_check = Some(style::button(
            cx.tree,
            confirm,
            "[ ] Я подтверждаю перезапись",
            Button::Secondary,
        )?);
        let actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, actions, "ЗАПИСАТЬ", Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, actions, "ОТМЕНА", Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.intent.is_some() {
            if let Some(card) = self.confirm_card {
                cx.tree.open_dialog(card)?;
            }
        }
        Ok(())
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
            cx.status = Some("Aborted: Запись отменена пользователем.".to_owned());
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
            cx.status = self.items.get(row_index).map(|file| format!("Выбран {}", file.name));
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
                        "[✓] Я подтверждаю перезапись"
                    } else {
                        "[ ] Я подтверждаю перезапись"
                    },
                )?;
            }
        }

        if clicked.is_some() && clicked == self.confirm_cancel {
            self.clear_intent(cx)?;
            cx.status = Some("Aborted: Запись отменена пользователем.".to_owned());
        }

        if clicked.is_some() && clicked == self.confirm_write {
            if !self.overwrite_confirmed {
                cx.status = Some("Установите флажок «Я подтверждаю перезапись».".to_owned());
            } else if let Some(intent) = self.intent.take() {
                self.overwrite_confirmed = false;
                if let Some(card) = self.confirm_card {
                    if cx.tree.dialog() == Some(card) {
                        let _ = cx.tree.close_dialog()?;
                    } else {
                        cx.tree.set_visible(card, false)?;
                    }
                }
                cx.status = Some(format!("Запись {} в Steam Cloud (RemoteStorage)...", intent.remote));
                self.upload(cx, intent);
            }
        }

        if clicked.is_some() && clicked == self.download {
            let Some(selected_id) = self.selected.as_deref() else {
                cx.status = Some("Сначала выберите файл Steam Cloud".to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.name == selected_id) else {
                self.selected = None;
                cx.status = Some("Выбранный облачный файл исчез после обновления списка".to_owned());
                return Ok(());
            };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
            // Never touch the open save: a cloud file goes into <data>/backups/cloud_downloads as a new file.
            // Replacing a local save from the cloud needs the checks of ACCEPTANCE §18.3 (same name, same game,
            // valid save, journaled backup) and is done by the save writer, not here.
            let downloads = sse_app::paths::default_data_directory()
                .join("backups")
                .join("cloud_downloads");
            let remote = item.name.clone();
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let source = worker(&Request::Read {
                        app_id,
                        remote_name: remote.clone(),
                    })?;
                    let name = std::path::Path::new(&remote)
                        .file_name()
                        .ok_or_else(|| "Облачный файл без имени".to_owned())?;
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
                                format!("{} уже есть", target.display()),
                            ));
                        }
                        std::fs::rename(&temp, &target)
                    })();
                    if let Err(error) = written {
                        let _ = std::fs::remove_file(&temp);
                        return Err(error.to_string());
                    }
                    Ok(format!(
                        "Файл скачан отдельно, открытый сейв не тронут: {}",
                        target.display()
                    ))
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Cloud,
                    Box::new(CloudReply::Done(result)),
                ));
            });
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
                            cx.tree.set_text(status, &format!("Ошибка загрузки списка: {error}"))?;
                        }
                    }
                    CloudReply::Prepared(Ok(intent)) => {
                        self.intent = Some(intent.clone());
                        self.overwrite_confirmed = false;
                        if let Some(check) = self.confirm_check {
                            cx.tree.set_text(check, "[ ] Я подтверждаю перезапись")?;
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
    Installed(std::result::Result<String, String>),
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
            let _ = cx.tree.set_text(id, "Проверка наличия обновлений...");
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        std::thread::spawn(move || {
            let result = (|| {
                let detected = sse_update::UpdateInstallationDetector::detect(None, None, None)
                    .map_err(|e| format!("Updates are not available: {e}"))?;
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
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Checked(result)),
            ));
        });
    }

    fn download(&mut self, cx: &mut Context<'_>) {
        if self.busy {
            return;
        }
        let Some(artifact) = self.artifact.clone() else {
            cx.status = Some("Нет пакета для этой установки".to_owned());
            return;
        };
        self.busy = true;
        if let Some(id) = self.status {
            let _ = cx.tree.set_text(id, "Скачивание пакета обновления...");
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        std::thread::spawn(move || {
            let result = (|| {
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
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Downloaded(result)),
            ));
        });
    }

    fn install(&mut self, cx: &mut Context<'_>) {
        if self.busy {
            return;
        }
        let (Some(artifact), Some(path)) = (self.artifact.clone(), self.downloaded.clone()) else {
            cx.status = Some("Сначала скачайте обновление.".to_owned());
            return;
        };
        self.busy = true;
        if let Some(id) = self.status {
            let _ = cx.tree.set_text(id, "Установка обновления...");
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.busy = false;
            return;
        };
        std::thread::spawn(move || {
            let result = (|| {
                let detected =
                    sse_update::UpdateInstallationDetector::detect(None, None, None).map_err(|e| e.to_string())?;
                let service = sse_update::UpdateService::new(env!("CARGO_PKG_VERSION"), detected);
                let mut runner = sse_update::SystemProcessRunner;
                let done = service
                    .install(&artifact, &path, &mut runner)
                    .map_err(|e| e.to_string())?;
                Ok(format!("Обновление запущено: {}", done.message))
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Updates,
                Box::new(UpdateReply::Installed(result)),
            ));
        });
    }
}

impl Screen for Updates {
    fn id(&self) -> ScreenId {
        ScreenId::Updates
    }
    fn subtitle(&self) -> &str {
        "Проверка, загрузка и установка новой версии приложения"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ОБНОВЛЕНИЕ ПРИЛОЖЕНИЯ", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            &format!("ТЕКУЩАЯ ВЕРСИЯ: {}", env!("CARGO_PKG_VERSION")),
            Text::Value,
        )?;
        self.latest = Some(style::label(cx.tree, card, "ПОСЛЕДНЯЯ ВЕРСИЯ: —", Text::Value)?);
        self.badge = Some(style::label(cx.tree, card, "Статус неизвестен", Text::Body)?);
        self.status = Some(style::label(cx.tree, card, "", Text::Note)?);
        let row = style::row(cx.tree, card)?;
        self.check = Some(style::button(cx.tree, row, "ПРОВЕРИТЬ ОБНОВЛЕНИЯ", Button::Secondary)?);
        self.download = Some(style::button(cx.tree, row, "СКАЧАТЬ ОБНОВЛЕНИЕ", Button::Secondary)?);
        self.install = Some(style::button(cx.tree, row, "УСТАНОВИТЬ ОБНОВЛЕНИЕ", Button::Primary)?);
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
                                .set_text(id, &format!("Скачивание пакета обновления... {percent}%"))?;
                        }
                    }
                    return Ok(());
                }
                self.busy = false;
                match reply {
                    UpdateReply::Progress(_, _) => {}
                    UpdateReply::Checked(Ok((state, version, artifact))) => {
                        self.artifact.clone_from(artifact);
                        self.downloaded = None;
                        if let Some(id) = self.latest {
                            cx.tree.set_text(
                                id,
                                &format!("ПОСЛЕДНЯЯ ВЕРСИЯ: {}", if version.is_empty() { "—" } else { version }),
                            )?;
                        }
                        let (badge, status) = match state {
                            sse_update::UpdateState::Current => (
                                "У вас актуальная версия",
                                "Установлена последняя версия приложения.".to_owned(),
                            ),
                            sse_update::UpdateState::Available => {
                                ("Доступно обновление", format!("Доступна новая версия {version}!"))
                            }
                            sse_update::UpdateState::Unavailable => {
                                ("Обновление недоступно", "Не удалось проверить обновления.".to_owned())
                            }
                            sse_update::UpdateState::Invalid | sse_update::UpdateState::DowngradeRefused => (
                                "Ошибка проверки манифеста",
                                "Проверка завершилась с ошибкой.".to_owned(),
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
                            cx.tree.set_text(id, "Обновление недоступно")?;
                        }
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, error)?;
                        }
                        cx.status = Some("Ошибка подключения к серверу обновлений.".to_owned());
                    }
                    UpdateReply::Downloaded(Ok((artifact, path))) => {
                        self.artifact = Some(artifact.clone());
                        self.downloaded = Some(path.clone());
                        let text = format!("Пакет обновления скачан: {}", path.display());
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &text)?;
                        }
                        cx.status = Some(text);
                    }
                    UpdateReply::Downloaded(Err(error)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &format!("Ошибка скачивания: {error}"))?;
                        }
                        cx.status = Some("Не удалось завершить скачивание.".to_owned());
                    }
                    UpdateReply::Installed(Ok(text)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, text)?;
                        }
                        cx.status = Some(text.clone());
                    }
                    UpdateReply::Installed(Err(error)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &format!("Ошибка запуска установки: {error}"))?;
                        }
                        cx.status = Some("Не удалось запустить установку.".to_owned());
                    }
                }
            }
        }
        Ok(())
    }
}
