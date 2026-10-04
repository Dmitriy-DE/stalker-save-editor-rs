//! S1 (Claude): the shell's own screens: capabilities, settings.
//!
//! `Settings` is the reference screen for the other packages: build once, keep widget ids, react to clicks, report
//! through the status line, push slow work to a thread and come back with `AppMessage::ToScreen`.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::Result;

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![Box::new(Capabilities), Box::new(Settings::default())]
}


#[derive(Clone, Copy)]
enum Support {
    Write,
    Read,
    No,
}

impl Support {
    const fn label(self) -> &'static str {
        match self {
            Self::Write => "✓",
            Self::Read => "чт.",
            Self::No => "—",
        }
    }
}

struct CapabilityRow {
    game: &'static str,
    read: Support,
    money: Support,
    items: Support,
    s2: Support,
    fixes: Support,
    companion: Support,
    cloud: Support,
    reason: &'static str,
}

const CAPABILITY_ROWS: [CapabilityRow; 7] = [
    CapabilityRow { game: "ТЧ", read: Support::Write, money: Support::Write, items: Support::Write, s2: Support::No, fixes: Support::Write, companion: Support::Write, cloud: Support::Write, reason: "X-Ray 1.0: чтение и проверенные мутации" },
    CapabilityRow { game: "ЧН", read: Support::Write, money: Support::Write, items: Support::Write, s2: Support::No, fixes: Support::Write, companion: Support::Write, cloud: Support::Write, reason: "X-Ray 1.5: чтение и проверенные мутации" },
    CapabilityRow { game: "ЗП", read: Support::Write, money: Support::Write, items: Support::Write, s2: Support::No, fixes: Support::Write, companion: Support::Write, cloud: Support::Write, reason: "X-Ray 1.6: чтение и проверенные мутации" },
    CapabilityRow { game: "ТЧ EE", read: Support::Read, money: Support::Read, items: Support::Read, s2: Support::No, fixes: Support::Write, companion: Support::No, cloud: Support::Write, reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации" },
    CapabilityRow { game: "ЧН EE", read: Support::Read, money: Support::Read, items: Support::Read, s2: Support::No, fixes: Support::Write, companion: Support::No, cloud: Support::Write, reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации" },
    CapabilityRow { game: "ЗП EE", read: Support::Read, money: Support::Read, items: Support::Read, s2: Support::No, fixes: Support::Write, companion: Support::No, cloud: Support::Write, reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации" },
    CapabilityRow { game: "S2", read: Support::Read, money: Support::Read, items: Support::Read, s2: Support::Read, fixes: Support::No, companion: Support::Write, cloud: Support::Write, reason: "UE5: поддержка S2 есть, запись сейва остаётся safety-restricted" },
];

struct Capabilities;

impl Screen for Capabilities {
    fn id(&self) -> ScreenId {
        ScreenId::Capabilities
    }

    fn subtitle(&self) -> &str {
        "Игра × возможность: запись, чтение и причины ограничений"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "МАТРИЦА ВОЗМОЖНОСТЕЙ", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "✓ — поддержано · чт. — только чтение · — — не поддерживается",
            Text::Note,
        )?;
        style::label(
            cx.tree,
            card,
            "ИГРА     ЧТЕН.  ДЕНЬГИ  ПРЕДМ.  S2   ФИКСЫ  КОМП.  CLOUD",
            Text::Value,
        )?;
        for row in CAPABILITY_ROWS {
            let line = format!(
                "{:<8} {:<6} {:<7} {:<7} {:<4} {:<6} {:<6} {}",
                row.game,
                row.read.label(),
                row.money.label(),
                row.items.label(),
                row.s2.label(),
                row.fixes.label(),
                row.companion.label(),
                row.cloud.label()
            );
            style::label(cx.tree, card, &line, Text::Body)?;
            style::label(cx.tree, card, row.reason, Text::Note)?;
        }
        Ok(())
    }

    fn message(
        &mut self,
        _cx: &mut Context<'_>,
        _message: &Message<AppMessage>,
        _clicked: Option<WidgetId>,
    ) -> Result<()> {
        Ok(())
    }
}

const SCALES: [&str; 4] = ["100 %", "125 %", "150 %", "200 %"];

/// Result of the background check started by the settings screen.
struct Checked(String);

/// Settings screen.
#[derive(Default)]
pub struct Settings {
    scale_value: Option<WidgetId>,
    scale_button: Option<WidgetId>,
    check_button: Option<WidgetId>,
    check_result: Option<WidgetId>,
    scale: usize,
}

impl Screen for Settings {
    fn id(&self) -> ScreenId {
        ScreenId::Settings
    }

    fn subtitle(&self) -> &str {
        "Язык, тема, масштаб, обновления"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let view = style::card(cx.tree, host)?;
        style::label(cx.tree, view, "ВИД", Text::Heading)?;
        let line = style::row(cx.tree, view)?;
        style::label(cx.tree, line, "Масштаб интерфейса:", Text::Body)?;
        self.scale_value = Some(style::label(cx.tree, line, SCALES[0], Text::Value)?);
        self.scale_button = Some(style::button(cx.tree, line, "Изменить", Button::Secondary)?);

        let updates = style::card(cx.tree, host)?;
        style::label(cx.tree, updates, "ОБНОВЛЕНИЯ", Text::Heading)?;
        let line = style::row(cx.tree, updates)?;
        self.check_button = Some(style::button(cx.tree, line, "Проверить", Button::Primary)?);
        self.check_result = Some(style::label(cx.tree, line, "Ещё не проверяли", Text::Note)?);
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.scale_button {
            self.scale = self.scale.saturating_add(1).checked_rem(SCALES.len()).unwrap_or(0);
            if let (Some(value), Some(text)) = (self.scale_value, SCALES.get(self.scale)) {
                cx.tree.set_text(value, text)?;
            }
            cx.status = Some("Масштаб применится после U2".to_owned());
        }
        if clicked.is_some() && clicked == self.check_button {
            if let Some(result) = self.check_result {
                cx.tree.set_text(result, "Проверяю…")?;
            }
            // Slow work never runs on the UI thread: a worker answers through the proxy.
            if let Some(proxy) = cx.proxy.cloned() {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(300));
                    let answer = Checked(format!("Установлена {}", env!("CARGO_PKG_VERSION")));
                    proxy.send(AppMessage::ToScreen(ScreenId::Settings, Box::new(answer)));
                });
            }
        }
        if let Message::User(AppMessage::ToScreen(_, payload)) = message {
            if let (Some(Checked(text)), Some(result)) = (payload.downcast_ref::<Checked>(), self.check_result) {
                cx.tree.set_text(result, text)?;
            }
        }
        Ok(())
    }
}
