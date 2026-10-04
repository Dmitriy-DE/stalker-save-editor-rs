//! First-run wizard: language -> discovered games -> ready.

use super::games::{discover_all_installations, DiscoveredInstallation};
use super::style::{self, Button, Text};
use super::{AppMessage, Context};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::Result;

const LANGUAGE_NAMES: [&str; 15] = [
    "Русский", "Українська", "English", "Deutsch", "Français", "Italiano", "Español", "Polski",
    "Čeština", "Português (Brasil)", "Türkçe", "日本語", "한국어", "简体中文", "繁體中文",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Language,
    Games,
    Ready,
}

#[derive(Debug)]
struct Discovery(Vec<DiscoveredInstallation>);

/// Modal first-run flow owned by the shell.
#[derive(Default)]
pub struct Wizard {
    step: Step,
    language: usize,
    settings: sse_app::AppSettings,
    title: Option<WidgetId>,
    status: Option<WidgetId>,
    language_value: Option<WidgetId>,
    language_button: Option<WidgetId>,
    game_rows: Vec<WidgetId>,
    next: Option<WidgetId>,
    installations: Vec<DiscoveredInstallation>,
    discovering: bool,
}

impl Wizard {
    /// Builds the wizard once.
    pub fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        self.settings = sse_app::AppSettings::load(&sse_app::default_settings_path());
        crate::strings::set_language(self.settings.language.as_deref());
        self.language = crate::strings::language_index(crate::strings::current_language());

        let card = style::card(cx.tree, host)?;
        self.title = Some(style::label(cx.tree, card, "МАСТЕР ПЕРВОГО ЗАПУСКА", Text::Heading)?);
        self.status = Some(style::label(
            cx.tree,
            card,
            "1/3 · Выберите язык интерфейса",
            Text::Body,
        )?);
        let language = style::row(cx.tree, card)?;
        style::label(cx.tree, language, "Язык:", Text::Body)?;
        self.language_value = Some(style::label(cx.tree, language, LANGUAGE_NAMES[self.language], Text::Value)?);
        self.language_button = Some(style::button(cx.tree, language, "Изменить", Button::Secondary)?);

        style::label(cx.tree, card, "НАЙДЕННЫЕ ИГРЫ", Text::Heading)?;
        for _ in 0..7 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.game_rows.push(row);
        }
        self.next = Some(style::button(cx.tree, card, "Далее", Button::Primary)?);
        Ok(())
    }

    fn start_discovery(&mut self, cx: &mut Context<'_>) {
        if self.discovering {
            return;
        }
        self.discovering = true;
        if let Some(status) = self.status {
            let _ = cx.tree.set_text(status, "2/3 · Ищу установленные игры…");
        }
        let Some(proxy) = cx.proxy.cloned() else {
            self.finish_discovery(cx, discover_all_installations());
            return;
        };
        std::thread::spawn(move || {
            proxy.send(AppMessage::Wizard(Box::new(Discovery(discover_all_installations()))));
        });
    }

    fn finish_discovery(&mut self, cx: &mut Context<'_>, installations: Vec<DiscoveredInstallation>) {
        self.discovering = false;
        self.installations = installations;
        if let Some(status) = self.status {
            let text = if self.installations.is_empty() {
                "2/3 · Игры не найдены — их можно добавить позже в «Обзоре игр»".to_owned()
            } else {
                format!("2/3 · Найдено установок: {} · выберите нужную или продолжайте", self.installations.len())
            };
            let _ = cx.tree.set_text(status, &text);
        }
        for (index, widget) in self.game_rows.iter().copied().enumerate() {
            if let Some(game) = self.installations.get(index) {
                let _ = cx.tree.set_visible(widget, true);
                let _ = cx.tree.set_text(widget, &format!("{}\n{}", game.title, game.directory.display()));
            } else {
                let _ = cx.tree.set_visible(widget, false);
            }
        }
    }

    fn show_ready(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.step = Step::Ready;
        if let Some(status) = self.status {
            cx.tree.set_text(status, "3/3 · Готово. Настройки можно изменить в любой момент.")?;
        }
        if let Some(button) = self.language_button {
            cx.tree.set_visible(button, false)?;
        }
        if let Some(value) = self.language_value {
            cx.tree.set_visible(value, false)?;
        }
        for row in &self.game_rows {
            cx.tree.set_visible(*row, false)?;
        }
        if let Some(next) = self.next {
            cx.tree.set_text(next, "Начать работу")?;
        }
        Ok(())
    }

    /// Handles one shell message. Returns true after the user explicitly finishes the wizard.
    pub fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<bool> {
        if let Message::User(AppMessage::Wizard(payload)) = message {
            if let Some(Discovery(found)) = payload.downcast_ref::<Discovery>() {
                self.finish_discovery(cx, found.clone());
            }
        }

        if clicked.is_some() && clicked == self.language_button && self.step == Step::Language {
            self.language = self
                .language
                .saturating_add(1)
                .checked_rem(crate::strings::LANGUAGES.len())
                .unwrap_or(0);
            if let Some(value) = self.language_value {
                cx.tree.set_text(value, LANGUAGE_NAMES[self.language])?;
            }
        }

        if clicked.is_some() && self.step == Step::Games {
            for (index, row) in self.game_rows.iter().copied().enumerate() {
                if clicked == Some(row) {
                    if let Some(game) = self.installations.get(index) {
                        cx.app.set_selected_game(Some(game.target.release_id().to_owned()));
                        cx.app.set_game_dir(Some(game.directory.clone()));
                        cx.status = Some(format!("Выбрана {}", game.title));
                    }
                }
            }
        }

        if clicked.is_some() && clicked == self.next {
            match self.step {
                Step::Language => {
                    let code = crate::strings::LANGUAGES[self.language];
                    crate::strings::set_language(Some(code));
                    self.settings.language = Some(code.to_owned());
                    self.settings.save(&sse_app::default_settings_path())?;
                    self.step = Step::Games;
                    self.start_discovery(cx);
                }
                Step::Games if !self.discovering => self.show_ready(cx)?,
                Step::Games => cx.status = Some("Поиск игр ещё выполняется".to_owned()),
                Step::Ready => {
                    self.settings.first_run_completed = true;
                    self.settings.save(&sse_app::default_settings_path())?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}
