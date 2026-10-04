//! S5: companion, achievements, Steam Cloud, and editor updates.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
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
    Changed(std::result::Result<String, String>),
}

fn confirm_twice(armed: &mut Option<WidgetId>, clicked: Option<WidgetId>, status: &mut Option<String>) -> bool {
    if *armed == clicked {
        *armed = None;
        return true;
    }
    *armed = clicked;
    *status = Some("Подтвердите действие повторным нажатием".to_owned());
    false
}

#[derive(Default)]
struct Companion {
    status: Option<WidgetId>,
    version: Option<WidgetId>,
    install: Option<WidgetId>,
    remove: Option<WidgetId>,
    /// Destructive button pressed once and waiting for the second press.
    armed: Option<WidgetId>,
}

impl Companion {
    fn refresh(&self, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else { return };
        let game = cx.app.selected_game().map(str::to_owned);
        let dir = cx.app.game_dir().map(Path::to_path_buf);
        std::thread::spawn(move || {
            let result = match (game.as_deref(), dir.as_deref()) {
                (Some(g), Some(d)) => companion_root(g, d).map(|r| installed_version(&r)),
                _ => Err("Сначала выберите установленную игру в «Обзоре игр»".to_owned()),
            };
            proxy.send(AppMessage::ToScreen(
                ScreenId::Companion,
                Box::new(CompanionReply::Status(result)),
            ));
        });
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
        self.status = Some(style::label(cx.tree, card, "Выберите игру", Text::Value)?);
        self.version = Some(style::label(cx.tree, card, "Версия: —", Text::Note)?);
        let row = style::row(cx.tree, card)?;
        self.install = Some(style::button(cx.tree, row, "Установить", Button::Primary)?);
        self.remove = Some(style::button(cx.tree, row, "Удалить", Button::Secondary)?);
        let hot = style::card(cx.tree, host)?;
        style::label(cx.tree, hot, "ГОРЯЧИЕ КЛАВИШИ МОДА", Text::Heading)?;
        let layout = sse_companion::hotkeys::HotkeyLayout::default();
        for action in [
            sse_companion::hotkeys::HotkeyAction::Heal,
            sse_companion::hotkeys::HotkeyAction::RepairEquipped,
            sse_companion::hotkeys::HotkeyAction::Mark,
            sse_companion::hotkeys::HotkeyAction::JumpLast,
            sse_companion::hotkeys::HotkeyAction::QuickSave,
        ] {
            let key = layout.binding(action).map_or_else(|| "—".to_owned(), |v| v.to_string());
            style::label(cx.tree, hot, &format!("{} — {key}", action.name()), Text::Body)?;
        }
        Ok(())
    }
    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.refresh(cx);
        Ok(())
    }
    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && (clicked == self.install || clicked == self.remove) {
            if !confirm_twice(&mut self.armed, clicked, &mut cx.status) {
                return Ok(());
            }
            let install = clicked == self.install;
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let game = cx.app.selected_game().map(str::to_owned);
            let dir = cx.app.game_dir().map(Path::to_path_buf);
            std::thread::spawn(move || {
                let result = (|| {
                    let game = game.ok_or_else(|| "Игра не выбрана".to_owned())?;
                    let dir = dir.ok_or_else(|| "Папка игры не выбрана".to_owned())?;
                    let root = companion_root(&game, &dir)?;
                    if install {
                        if let Some(target) = xray_game(&game) {
                            sse_companion::installer::install_bundled(&root, target).map_err(|e| e.to_string())?;
                        } else {
                            sse_companion::installer::install_stalker2(&root).map_err(|e| e.to_string())?;
                        }
                        Ok("Компаньон установлен".to_owned())
                    } else {
                        let id = if matches!(game.as_str(), "s2" | "stalker2") {
                            "s2"
                        } else if game.contains("soc") {
                            "soc"
                        } else if game.contains("cs") || game == "clear_sky" {
                            "cs"
                        } else {
                            "cop"
                        };
                        let removed = sse_companion::installer::uninstall(&root, id).map_err(|e| e.to_string())?;
                        Ok(if removed {
                            "Компаньон удалён; исходные файлы восстановлены"
                        } else {
                            "Компаньон не был установлен"
                        }
                        .to_owned())
                    }
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Companion,
                    Box::new(CompanionReply::Changed(result)),
                ));
            });
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Companion, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<CompanionReply>() {
                match reply {
                    CompanionReply::Status(Ok(version)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(
                                id,
                                if version.is_some() {
                                    "Установлен"
                                } else {
                                    "Не установлен"
                                },
                            )?;
                        }
                        if let Some(id) = self.version {
                            cx.tree
                                .set_text(id, &format!("Версия: {}", version.as_deref().unwrap_or("—")))?;
                        }
                    }
                    CompanionReply::Status(Err(e)) | CompanionReply::Changed(Err(e)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &clip(e))?;
                        }
                    }
                    CompanionReply::Changed(Ok(text)) => {
                        cx.status = Some(text.clone());
                        self.refresh(cx);
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
#[derive(Default)]
struct Achievements {
    status: Option<WidgetId>,
    rows: Vec<WidgetId>,
    items: Vec<Achievement>,
    selected: Option<usize>,
    set: Option<WidgetId>,
    clear: Option<WidgetId>,
    refresh: Option<WidgetId>,
    progress: Option<WidgetId>,
    confirm: Option<bool>,
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
            cx.tree.set_text(id, &format!("Загружено {} достижений.", self.items.len()))?;
        }
        if let Some(id) = self.progress {
            let got = self.items.iter().filter(|item| item.achieved).count();
            let percent = if self.items.is_empty() { 0.0 } else { (got as f64 * 100.0) / self.items.len() as f64 };
            cx.tree.set_text(id, &format!("{got} из {} получено ({percent:.0}%)", self.items.len()))?;
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
        style::label(cx.tree, card, "Steam доступен / недоступен определяется рабочим процессом Steam.", Text::Note)?;
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
        self.clear = Some(style::button(cx.tree, row, "Сбросить", Button::Secondary)?);
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
                self.selected = Some(i);
                self.confirm = None;
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
            let Some(i) = self.selected else {
                cx.status = Some("Сначала выберите достижение".to_owned());
                return Ok(());
            };
            if self.confirm != Some(set) {
                self.confirm = Some(set);
                cx.status = self.items.get(i).map(|a| if set { format!("РАЗБЛОКИРОВАТЬ ДОСТИЖЕНИЕ: подтвердите «{}» повторным нажатием.", a.display_name) } else { format!("СНЯТЬ ДОСТИЖЕНИЕ: подтвердите «{}» повторным нажатием.", a.display_name) });
                return Ok(());
            }
            self.confirm = None;
            self.pending_set = Some(set);
            let Some(item) = self.items.get(i) else { return Ok(()) };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
            let name = item.name.clone();
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
                        self.items.clone_from(items);
                        self.render(cx)?
                    }
                    AchReply::List(Err(e)) => {
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &clip(e))?;
                        }
                    }
                    AchReply::Changed(Ok(())) => {
                        if let Some(index) = self.selected {
                            if let Some(item) = self.items.get_mut(index) {
                                item.achieved = self.pending_set.take().unwrap_or(item.achieved);
                                cx.status = Some(if item.achieved { format!("Достижение «{}» получено в Steam.", item.display_name) } else { format!("Достижение «{}» снято в Steam.", item.display_name) });
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
    selected: Option<usize>,
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
            cx.tree.set_visible(card, false)?;
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
        let Some(i) = self.selected else {
            cx.status = Some("Сначала выберите файл Steam Cloud".to_owned());
            return;
        };
        let Some(item) = self.items.get(i) else { return };
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

        let confirm = style::card(cx.tree, host)?;
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

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && self.rows.first().copied() == clicked {
            self.clear_intent(cx)?;
            self.selected = None;
            self.load(cx);
        }

        let selected_row =
            clicked.and_then(|clicked_id| self.rows.iter().copied().skip(1).position(|row| row == clicked_id));
        if let Some(row_index) = selected_row.filter(|index| self.items.get(*index).is_some()) {
            self.clear_intent(cx)?;
            self.selected = Some(row_index);
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
                    cx.tree.set_visible(card, false)?;
                }
                cx.status = Some(format!("Запись {} в Steam Cloud (RemoteStorage)...", intent.remote));
                self.upload(cx, intent);
            }
        }

        if clicked.is_some() && clicked == self.download {
            let Some(i) = self.selected else {
                cx.status = Some("Сначала выберите файл Steam Cloud".to_owned());
                return Ok(());
            };
            let Some(item) = self.items.get(i) else { return Ok(()) };
            let Some(app_id) = cx.app.selected_game().and_then(app_id) else {
                return Ok(());
            };
            let Some(local) = cx.app.current_save().map(Path::to_path_buf) else {
                cx.status = Some("Сначала откройте локальный сейв".to_owned());
                return Ok(());
            };
            let remote = item.name.clone();
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let source = worker(&Request::Read {
                        app_id,
                        remote_name: remote,
                    })?;
                    let backup = local.with_extension("cloud-backup");
                    if local.is_file() {
                        std::fs::copy(&local, &backup).map_err(|error| error.to_string())?;
                    }
                    let temp = local.with_extension("cloud-download.tmp");
                    std::fs::write(&temp, &source).map_err(|error| error.to_string())?;
                    std::fs::rename(&temp, &local).map_err(|error| error.to_string())?;
                    Ok(format!("Файл успешно скачан: {}", local.display()))
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
                        if let Some(card) = self.confirm_card {
                            cx.tree.set_visible(card, true)?;
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
struct UpdateReply(std::result::Result<String, String>);
#[derive(Default)]
struct Updates {
    status: Option<WidgetId>,
    check: Option<WidgetId>,
    install: Option<WidgetId>,
    /// Destructive button pressed once and waiting for the second press.
    armed: Option<WidgetId>,
}

impl Screen for Updates {
    fn id(&self) -> ScreenId {
        ScreenId::Updates
    }
    fn subtitle(&self) -> &str {
        "Подписанные обновления редактора"
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ОБНОВЛЕНИЯ РЕДАКТОРА", Text::Heading)?;
        self.status = Some(style::label(
            cx.tree,
            card,
            &format!("Установлена {}", env!("CARGO_PKG_VERSION")),
            Text::Value,
        )?);
        let row = style::row(cx.tree, card)?;
        self.check = Some(style::button(cx.tree, row, "Проверить", Button::Secondary)?);
        self.install = Some(style::button(cx.tree, row, "Скачать и установить", Button::Primary)?);
        Ok(())
    }
    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        let install = clicked.is_some() && clicked == self.install;
        if install && !confirm_twice(&mut self.armed, clicked, &mut cx.status) {
            return Ok(());
        }
        if (clicked.is_some() && clicked == self.check) || install {
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let detected =
                        sse_update::UpdateInstallationDetector::detect(None, None, None).map_err(|e| e.to_string())?;
                    let service = sse_update::UpdateService::new(env!("CARGO_PKG_VERSION"), detected);
                    let mut fetch = sse_update::DefaultFetch;
                    let check = service.check(&mut fetch);
                    if let Some(error) = check.error {
                        return Err(error);
                    }
                    let manifest = check.manifest.ok_or_else(|| "Нет манифеста обновления".to_owned())?;
                    if !install {
                        return Ok(format!("Последняя версия: {} ({:?})", manifest.version, check.state));
                    }
                    if !matches!(check.state, sse_update::UpdateState::Available) {
                        return Ok(format!("Обновление не требуется: {}", manifest.version));
                    }
                    let artifact = check
                        .artifact
                        .ok_or_else(|| "Нет пакета для этой установки".to_owned())?;
                    let path = std::env::temp_dir().join(&artifact.file);
                    service
                        .download(&mut fetch, &artifact, &path, None)
                        .map_err(|e| e.to_string())?;
                    let mut runner = sse_update::SystemProcessRunner;
                    let done = service
                        .install(&artifact, &path, &mut runner)
                        .map_err(|e| e.to_string())?;
                    Ok(format!("{:?}: {}", done.state, done.message))
                })();
                proxy.send(AppMessage::ToScreen(ScreenId::Updates, Box::new(UpdateReply(result))));
            });
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Updates, payload)) = message {
            if let Some(UpdateReply(result)) = payload.downcast_ref::<UpdateReply>() {
                let text = match result {
                    Ok(v) => v.clone(),
                    Err(e) => format!("Ошибка обновления: {e}"),
                };
                if let Some(id) = self.status {
                    cx.tree.set_text(id, &clip(&text))?;
                }
                cx.status = Some(text);
            }
        }
        Ok(())
    }
}
