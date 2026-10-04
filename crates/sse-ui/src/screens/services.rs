//! S5 service screens. No cloud writes or unattended installation.
use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::Result;

#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Service::new(ScreenId::Companion)),
        Box::new(Service::new(ScreenId::Achievements)),
        Box::new(Service::new(ScreenId::Cloud)),
        Box::new(Service::new(ScreenId::Updates)),
    ]
}

struct Checked { id: ScreenId, text: String }

/// Service screen with retained widget identifiers.
pub struct Service {
    id: ScreenId,
    state: Option<WidgetId>,
    refresh: Option<WidgetId>,
    details: Option<WidgetId>,
    busy: bool,
}
impl Service {
    const fn new(id: ScreenId) -> Self {
        Self { id, state: None, refresh: None, details: None, busy: false }
    }
    fn initial(&self) -> &'static str {
        match self.id {
            ScreenId::Companion => "Выберите установленную игру, чтобы проверить мод-компаньон.",
            ScreenId::Achievements => "Откройте сохранение игры, чтобы посмотреть доступные достижения.",
            ScreenId::Cloud => "Steam Cloud: только чтение. Подключение к Steam пока не выполнено.",
            ScreenId::Updates => "Нажмите «Проверить», чтобы узнать текущую версию редактора.",
            _ => "Нет данных",
        }
    }
}
impl Screen for Service {
    fn id(&self) -> ScreenId { self.id }
    fn subtitle(&self) -> &str {
        match self.id {
            ScreenId::Companion => "Установка мода и горячие клавиши",
            ScreenId::Achievements => "Каталог достижений игры",
            ScreenId::Cloud => "Локальные и облачные сохранения — только чтение",
            ScreenId::Updates => "Проверка версии без автоматической установки",
            _ => "",
        }
    }
    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, self.id.title(), Text::Heading)?;
        self.state = Some(style::label(cx.tree, card, self.initial(), Text::Body)?);
        let row = style::row(cx.tree, card)?;
        self.refresh = Some(style::button(cx.tree, row, "Проверить", Button::Primary)?);
        self.details = Some(style::label(cx.tree, card, "Нет доступных данных", Text::Note)?);
        if self.id == ScreenId::Companion {
            style::label(cx.tree, card, "Установка и удаление доступны только после выбора и проверки игры. Автоматических изменений нет.", Text::Note)?;
        }
        if self.id == ScreenId::Cloud {
            style::label(cx.tree, card, "Запись в Steam Cloud отключена.", Text::Note)?;
        }
        Ok(())
    }
    fn message(&mut self, cx: &mut Context<'_>, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<()> {
        if clicked.is_some() && clicked == self.refresh && !self.busy {
            self.busy = true;
            if let Some(id) = self.state { cx.tree.set_text(id, "Проверка…")?; }
            if let Some(proxy) = cx.proxy.cloned() {
                let id = self.id;
                std::thread::spawn(move || {
                    let text = match id {
                        ScreenId::Updates => format!("Текущая версия: {}. Сетевая проверка недоступна без подключения сервиса обновлений.", env!("CARGO_PKG_VERSION")),
                        ScreenId::Companion => "Не выбрана установленная игра. Состояние Companion неизвестно; файлы не изменены.".to_owned(),
                        ScreenId::Achievements => "Нет подключённого каталога достижений для выбранной игры.".to_owned(),
                        ScreenId::Cloud => "Нет соединения с Steam Cloud; локальные и облачные файлы не изменены.".to_owned(),
                        _ => "Нет данных".to_owned(),
                    };
                    proxy.send(AppMessage::ToScreen(id, Box::new(Checked { id, text })));
                });
            } else {
                self.busy = false;
                if let Some(id) = self.state { cx.tree.set_text(id, "Фоновый сервис недоступен")?; }
            }
        }
        if let Message::User(AppMessage::ToScreen(id, payload)) = message {
            if *id == self.id {
                if let Some(done) = payload.downcast_ref::<Checked>() {
                    if done.id == self.id {
                        self.busy = false;
                        if let Some(state) = self.state { cx.tree.set_text(state, &done.text)?; }
                        if let Some(details) = self.details { cx.tree.set_text(details, "Проверка завершена без изменения файлов")?; }
                    }
                }
            }
        }
        Ok(())
    }
}
