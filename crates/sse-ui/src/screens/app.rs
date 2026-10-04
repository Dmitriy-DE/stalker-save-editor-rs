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
    CapabilityRow {
        game: "ТЧ",
        read: Support::Write,
        money: Support::Write,
        items: Support::Write,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::Write,
        cloud: Support::Write,
        reason: "X-Ray 1.0: чтение и проверенные мутации",
    },
    CapabilityRow {
        game: "ЧН",
        read: Support::Write,
        money: Support::Write,
        items: Support::Write,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::Write,
        cloud: Support::Write,
        reason: "X-Ray 1.5: чтение и проверенные мутации",
    },
    CapabilityRow {
        game: "ЗП",
        read: Support::Write,
        money: Support::Write,
        items: Support::Write,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::Write,
        cloud: Support::Write,
        reason: "X-Ray 1.6: чтение и проверенные мутации",
    },
    CapabilityRow {
        game: "ТЧ EE",
        read: Support::Read,
        money: Support::Read,
        items: Support::Read,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::No,
        cloud: Support::Write,
        reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации",
    },
    CapabilityRow {
        game: "ЧН EE",
        read: Support::Read,
        money: Support::Read,
        items: Support::Read,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::No,
        cloud: Support::Write,
        reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации",
    },
    CapabilityRow {
        game: "ЗП EE",
        read: Support::Read,
        money: Support::Read,
        items: Support::Read,
        s2: Support::No,
        fixes: Support::Write,
        companion: Support::No,
        cloud: Support::Write,
        reason: "Enhanced: сейвы доступны для чтения; запись ограничена до верификации",
    },
    CapabilityRow {
        game: "S2",
        read: Support::Read,
        money: Support::Read,
        items: Support::Read,
        s2: Support::Read,
        fixes: Support::No,
        companion: Support::Write,
        cloud: Support::Write,
        reason: "UE5: поддержка S2 есть, запись сейва остаётся safety-restricted",
    },
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

const SCALES: [u32; 7] = [0, 100, 110, 125, 150, 175, 200];

fn repaint_theme(tree: &mut crate::widget::Tree, old: crate::theme::Theme, new: crate::theme::Theme) {
    let mut pairs = Vec::with_capacity(24);
    pairs.extend(old.colors.background.into_iter().zip(new.colors.background));
    pairs.extend(old.colors.borders.into_iter().zip(new.colors.borders));
    pairs.extend(old.colors.text.into_iter().zip(new.colors.text));
    pairs.extend(old.colors.accent.into_iter().zip(new.colors.accent));
    pairs.extend(old.colors.state.into_iter().zip(new.colors.state));
    pairs.push((old.colors.selection, new.colors.selection));
    pairs.push((old.colors.focus_ring, new.colors.focus_ring));
    for (from, to) in pairs {
        tree.replace_color(style::rgb(from), style::rgb(to));
    }
    tree.damage_all();
}

fn load_settings() -> sse_app::AppSettings {
    sse_app::AppSettings::load(&sse_app::default_settings_path())
}

fn save_settings(settings: &sse_app::AppSettings) -> Result<()> {
    settings.save(&sse_app::default_settings_path())
}

/// Result of the background check started by the settings screen.
struct Checked(String);

/// Settings screen.
#[derive(Default)]
pub struct Settings {
    scale_value: Option<WidgetId>,
    scale_button: Option<WidgetId>,
    theme_value: Option<WidgetId>,
    theme_button: Option<WidgetId>,
    accent_value: Option<WidgetId>,
    accent_button: Option<WidgetId>,
    check_button: Option<WidgetId>,
    check_result: Option<WidgetId>,
    scale: usize,
    theme: usize,
    accent: usize,
    settings: sse_app::AppSettings,
}

impl Settings {
    fn theme_choice(&self) -> (&'static str, &'static str) {
        crate::theme::THEMES
            .get(self.theme)
            .copied()
            .or_else(|| crate::theme::THEMES.first().copied())
            .unwrap_or(("zone", "Тёмная"))
    }

    fn accent_choice(&self) -> &'static str {
        crate::theme::ACCENT_IDS
            .get(self.accent)
            .copied()
            .or_else(|| crate::theme::ACCENT_IDS.first().copied())
            .unwrap_or("amber")
    }

    fn scale_choice(&self) -> u32 {
        SCALES
            .get(self.scale)
            .copied()
            .or_else(|| SCALES.first().copied())
            .unwrap_or(100)
    }
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
        self.settings = load_settings();
        self.theme = crate::theme::THEMES
            .iter()
            .position(|(id, _)| *id == self.settings.theme_id)
            .unwrap_or(0);
        self.accent = crate::theme::ACCENT_IDS
            .iter()
            .position(|id| *id == self.settings.accent_id)
            .unwrap_or(0);
        self.scale = SCALES
            .iter()
            .position(|value| *value == self.settings.ui_scale_percent)
            .unwrap_or(0);
        let old_theme = crate::theme::current();
        crate::theme::apply_appearance(&self.settings.theme_id, &self.settings.accent_id);
        repaint_theme(cx.tree, old_theme, crate::theme::current());
        let percent = self.scale_choice();
        cx.tree.set_scale(percent as f32 / 100.0);

        let theme_line = style::row(cx.tree, view)?;
        style::label(cx.tree, theme_line, "Тема:", Text::Body)?;
        self.theme_value = Some(style::label(cx.tree, theme_line, self.theme_choice().1, Text::Value)?);
        self.theme_button = Some(style::button(cx.tree, theme_line, "Изменить", Button::Secondary)?);

        let accent_line = style::row(cx.tree, view)?;
        style::label(cx.tree, accent_line, "Акцент:", Text::Body)?;
        self.accent_value = Some(style::label(cx.tree, accent_line, self.accent_choice(), Text::Value)?);
        self.accent_button = Some(style::button(cx.tree, accent_line, "Изменить", Button::Secondary)?);

        let line = style::row(cx.tree, view)?;
        style::label(cx.tree, line, "Масштаб интерфейса:", Text::Body)?;
        self.scale_value = Some(style::label(cx.tree, line, if percent == 0 { "По размеру экрана" } else { &format!("{percent} %") }, Text::Value)?);
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
        if clicked.is_some() && clicked == self.theme_button {
            self.theme = self.theme.saturating_add(1).checked_rem(crate::theme::THEMES.len()).unwrap_or(0);
            let old = crate::theme::current();
            self.settings.theme_id = crate::theme::THEMES[self.theme].0.to_owned();
            crate::theme::apply_appearance(&self.settings.theme_id, crate::theme::ACCENT_IDS[self.accent]);
            repaint_theme(cx.tree, old, crate::theme::current());
            if let Some(value) = self.theme_value {
                cx.tree.set_text(value, crate::theme::THEMES[self.theme].1)?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
        }
        if clicked.is_some() && clicked == self.accent_button {
            self.accent = self.accent.saturating_add(1).checked_rem(crate::theme::ACCENT_IDS.len()).unwrap_or(0);
            let old = crate::theme::current();
            self.settings.accent_id = crate::theme::ACCENT_IDS[self.accent].to_owned();
            crate::theme::apply_appearance(crate::theme::THEMES[self.theme].0, &self.settings.accent_id);
            repaint_theme(cx.tree, old, crate::theme::current());
            if let Some(value) = self.accent_value {
                cx.tree.set_text(value, crate::theme::ACCENT_IDS[self.accent])?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
        }
        if clicked.is_some() && clicked == self.scale_button {
            self.scale = self.scale.saturating_add(1).checked_rem(SCALES.len()).unwrap_or(0);
            let percent = SCALES[self.scale];
            self.settings.ui_scale_percent = percent;
            cx.tree.set_scale(if percent == 0 { 1.0 } else { percent as f32 / 100.0 });
            if let Some(value) = self.scale_value {
                let label = if percent == 0 { "По размеру экрана".to_owned() } else { format!("{percent} %") };
                cx.tree.set_text(value, &label)?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
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
