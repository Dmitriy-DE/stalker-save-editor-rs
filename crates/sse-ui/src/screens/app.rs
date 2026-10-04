//! S1 (Claude): the shell's own screens: capabilities, settings.
//!
//! `Settings` is the reference screen for the other packages: build once, keep widget ids, react to clicks, report
//! through the status line, push slow work to a thread and come back with `AppMessage::ToScreen`.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::glyphs::{Face, TextStyle};
use crate::layout::{GridPlacement, NodeKind, Size, Style, Track};
use crate::widget::{Content, Look, TextAlign, WidgetId};
use sse_core::Result;

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![Box::new(Capabilities::default()), Box::new(Settings::default())]
}

#[derive(Clone, Copy)]
enum Support {
    Verified,
    Experimental,
    Research,
    Unsupported,
}

impl Support {
    const fn label(self) -> &'static str {
        match self {
            Self::Verified => "Запись",
            Self::Experimental => "Эксперим.",
            Self::Research => "Чтение",
            Self::Unsupported => "Нет",
        }
    }

    const fn reason(self) -> &'static str {
        match self {
            Self::Verified => "Подтверждено и верифицировано в игре.",
            Self::Experimental => "Экспериментальная поддержка (требуется проверка в игре).",
            Self::Research => "Исследование / режим только для чтения.",
            Self::Unsupported => "Не поддерживается движком или форматом сохранения.",
        }
    }
}

const GAMES: [&str; 7] = ["ТЧ", "ЧН", "ЗП", "ТЧ EE", "ЧН EE", "ЗП EE", "S2"];

struct CapabilityRow {
    name: &'static str,
    description: &'static str,
    support: [Support; 7],
}

const CAPABILITY_ROWS: [CapabilityRow; 12] = [
    CapabilityRow {
        name: "Деньги",
        description: "Изменение количества рублей у сталкера",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
        ],
    },
    CapabilityRow {
        name: "Стаки предметов",
        description: "Изменение количества в пачках патронов и расходников",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
        ],
    },
    CapabilityRow {
        name: "Прочность снаряжения",
        description: "Состояние и износ оружия, бронекостюмов и шлемов",
        support: [
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
            Support::Experimental,
        ],
    },
    CapabilityRow {
        name: "Размещение в слотах",
        description: "Слоты оружия, пояс для артефактов и рюкзак",
        support: [
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Апгрейды и модификации",
        description: "Установка и снятие веток улучшений оружия и брони",
        support: [
            Support::Unsupported,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Отношения группировок",
        description: "Редактирование очков репутации и враждебности фракций",
        support: [
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Фракция игрока",
        description: "Смена принадлежности сталкера к группировке",
        support: [
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Тайники (перемещение)",
        description: "Перемещение хабара из тайников в рюкзак и обратно",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Добавление предметов",
        description: "Спавн новых предметов из каталога в инвентарь",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Удаление предметов",
        description: "Безопасное удаление объектов из инвентаря",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Experimental,
            Support::Experimental,
            Support::Experimental,
            Support::Unsupported,
        ],
    },
    CapabilityRow {
        name: "Чтение инвентаря",
        description: "Парсинг предметов, патронов и экипировки",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
        ],
    },
    CapabilityRow {
        name: "Каталог предметов",
        description: "Сопоставление идентификаторов с официальными именами",
        support: [
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Verified,
            Support::Research,
        ],
    },
];

#[derive(Default)]
struct Capabilities {
    cells: Vec<(WidgetId, usize, usize)>,
    detail: Option<WidgetId>,
}

fn grid_label(
    tree: &mut crate::widget::Tree,
    parent: WidgetId,
    text: &str,
    column: usize,
    row: usize,
    heading: bool,
) -> Result<WidgetId> {
    let colors = crate::theme::current().colors;
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
            min: Size::new(0.0, 36.0),
            padding: crate::layout::Edges {
                left: 8.0,
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
            },
            grid: Some(GridPlacement::cell(column, row)),
            ..Style::default()
        },
        Content::Label {
            text: text.to_owned(),
            style: TextStyle::new(
                if heading { Face::Heading } else { Face::Body },
                if heading { 13.0 } else { 14.0 },
            ),
        },
        Look {
            border: Some((style::rgb(colors.borders[0]), 1.0)),
            fill: heading.then(|| style::rgb(colors.background[2])),
            text: style::rgb(if heading { colors.text[0] } else { colors.text[1] }),
            align: if column == 0 {
                TextAlign::Start
            } else {
                TextAlign::Center
            },
            ..Look::default()
        },
    )
}

fn grid_cell(
    tree: &mut crate::widget::Tree,
    parent: WidgetId,
    text: &str,
    column: usize,
    row: usize,
) -> Result<WidgetId> {
    let colors = crate::theme::current().colors;
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
            min: Size::new(0.0, 36.0),
            padding: crate::layout::Edges {
                left: 6.0,
                top: 0.0,
                right: 6.0,
                bottom: 0.0,
            },
            grid: Some(GridPlacement::cell(column, row)),
            ..Style::default()
        },
        Content::Button {
            text: text.to_owned(),
            style: TextStyle::new(Face::Heading, 12.0),
        },
        Look {
            fill: Some(style::rgb(colors.background[1])),
            hover_fill: Some(style::rgb(colors.background[3])),
            border: Some((style::rgb(colors.borders[0]), 1.0)),
            text: style::rgb(colors.text[0]),
            align: TextAlign::Center,
            ..Look::default()
        },
    )
}

impl Screen for Capabilities {
    fn id(&self) -> ScreenId {
        ScreenId::Capabilities
    }

    fn subtitle(&self) -> &str {
        "Игра × возможность: запись, чтение и причины ограничений"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let help = style::card(cx.tree, host)?;
        style::label(cx.tree, help, "СПРАВКА ПО ВОЗМОЖНОСТЯМ", Text::Heading)?;
        style::label(
            cx.tree,
            help,
            "Что редактор умеет делать с сейвами каждой игры.",
            Text::Body,
        )?;

        let matrix = style::card(cx.tree, host)?;
        style::label(cx.tree, matrix, "МАТРИЦА ПОДДЕРЖИВАЕМЫХ ВОЗМОЖНОСТЕЙ", Text::Heading)?;
        let grid = cx.tree.add(
            Some(matrix),
            NodeKind::Grid {
                columns: vec![
                    Track::Fixed(220.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                    Track::Fixed(92.0),
                ],
                rows: vec![Track::Fixed(38.0); CAPABILITY_ROWS.len().saturating_add(1)],
            },
            Style {
                gap: Size::new(2.0, 2.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        grid_label(cx.tree, grid, "ОПЕРАЦИЯ", 0, 0, true)?;
        for (column, game) in GAMES.into_iter().enumerate() {
            grid_label(cx.tree, grid, game, column.saturating_add(1), 0, true)?;
        }
        for (row_index, row) in CAPABILITY_ROWS.iter().enumerate() {
            let grid_row = row_index.saturating_add(1);
            grid_label(cx.tree, grid, row.name, 0, grid_row, false)?;
            for (column, support) in row.support.into_iter().enumerate() {
                let id = grid_cell(cx.tree, grid, support.label(), column + 1, grid_row)?;
                self.cells.push((id, row_index, column));
            }
        }
        self.detail = Some(style::label(
            cx.tree,
            matrix,
            "Выберите ячейку, чтобы увидеть причину уровня поддержки.",
            Text::Note,
        )?);

        let legend = style::card(cx.tree, host)?;
        style::label(cx.tree, legend, "ОБОЗНАЧЕНИЯ", Text::Heading)?;
        style::label(
            cx.tree,
            legend,
            "Запись — полная поддержка чтения и записи, верифицировано тестами.",
            Text::Body,
        )?;
        style::label(
            cx.tree,
            legend,
            "Эксперим. — поддержка реализована, ожидается подтверждение в игре.",
            Text::Body,
        )?;
        style::label(cx.tree, legend, "Чтение — режим только для чтения.", Text::Body)?;
        style::label(
            cx.tree,
            legend,
            "Нет — механика отсутствует или не поддерживается.",
            Text::Body,
        )?;
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        _message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() {
            if let Some((_, row, column)) = self.cells.iter().find(|(id, _, _)| Some(*id) == clicked) {
                if let (Some(capability), Some(game), Some(detail)) =
                    (CAPABILITY_ROWS.get(*row), GAMES.get(*column), self.detail)
                {
                    let Some(support) = capability.support.get(*column).copied() else { return Ok(()) };
                    cx.tree.set_text(
                        detail,
                        &format!(
                            "{} · {} · {} — {}",
                            capability.name,
                            game,
                            capability.description,
                            support.reason()
                        ),
                    )?;
                }
            }
        }
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

const LANGUAGE_NAMES: [&str; 15] = [
    "Русский",
    "Українська",
    "English",
    "Deutsch",
    "Français",
    "Italiano",
    "Español",
    "Polski",
    "Čeština",
    "Português (Brasil)",
    "Türkçe",
    "日本語",
    "한국어",
    "简体中文",
    "繁體中文",
];

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
    language_value: Option<WidgetId>,
    language_button: Option<WidgetId>,
    save_button: Option<WidgetId>,
    language: usize,
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

    fn accent_name(&self) -> &'static str {
        crate::theme::ACCENT_NAMES
            .get(self.accent)
            .copied()
            .or_else(|| crate::theme::ACCENT_NAMES.first().copied())
            .unwrap_or("Янтарный")
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
        self.settings = load_settings();
        self.language = crate::strings::language_index(self.settings.language.as_deref().unwrap_or("ru"));
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
        self.accent_value = Some(style::label(cx.tree, accent_line, self.accent_name(), Text::Value)?);
        self.accent_button = Some(style::button(cx.tree, accent_line, "Изменить", Button::Secondary)?);

        let line = style::row(cx.tree, view)?;
        style::label(cx.tree, line, "Масштаб интерфейса:", Text::Body)?;
        let scale_label = if percent == 0 {
            "По размеру экрана".to_owned()
        } else {
            format!("{percent} %")
        };
        self.scale_value = Some(style::label(cx.tree, line, &scale_label, Text::Value)?);
        self.scale_button = Some(style::button(cx.tree, line, "Изменить", Button::Secondary)?);

        let language_line = style::row(cx.tree, view)?;
        style::label(
            cx.tree,
            language_line,
            crate::strings::t("Язык интерфейса:"),
            Text::Body,
        )?;
        let language_name = LANGUAGE_NAMES.get(self.language).copied().unwrap_or("Русский");
        self.language_value = Some(style::label(cx.tree, language_line, language_name, Text::Value)?);
        self.language_button = Some(style::button(
            cx.tree,
            language_line,
            crate::strings::t("Изменить"),
            Button::Secondary,
        )?);
        style::label(
            cx.tree,
            view,
            crate::strings::t("Язык применится после перезапуска приложения."),
            Text::Note,
        )?;
        self.save_button = Some(style::button(
            cx.tree,
            view,
            crate::strings::t("Сохранить настройки"),
            Button::Primary,
        )?);

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
            self.theme = self
                .theme
                .saturating_add(1)
                .checked_rem(crate::theme::THEMES.len())
                .unwrap_or(0);
            let old = crate::theme::current();
            let (theme_id, theme_name) = crate::theme::THEMES
                .get(self.theme)
                .copied()
                .unwrap_or(("zone", "Зона (тёмная)"));
            let accent_id = crate::theme::ACCENT_IDS.get(self.accent).copied().unwrap_or("amber");
            self.settings.theme_id = theme_id.to_owned();
            crate::theme::apply_appearance(&self.settings.theme_id, accent_id);
            repaint_theme(cx.tree, old, crate::theme::current());
            if let Some(value) = self.theme_value {
                cx.tree.set_text(value, theme_name)?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
        }
        if clicked.is_some() && clicked == self.accent_button {
            self.accent = self
                .accent
                .saturating_add(1)
                .checked_rem(crate::theme::ACCENT_IDS.len())
                .unwrap_or(0);
            let old = crate::theme::current();
            let accent_id = crate::theme::ACCENT_IDS.get(self.accent).copied().unwrap_or("amber");
            let theme_id = crate::theme::THEMES.get(self.theme).map_or("zone", |entry| entry.0);
            self.settings.accent_id = accent_id.to_owned();
            crate::theme::apply_appearance(theme_id, &self.settings.accent_id);
            repaint_theme(cx.tree, old, crate::theme::current());
            if let Some(value) = self.accent_value {
                cx.tree.set_text(
                    value,
                    crate::theme::ACCENT_NAMES
                        .get(self.accent)
                        .copied()
                        .unwrap_or("Янтарный"),
                )?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
        }
        if clicked.is_some() && clicked == self.scale_button {
            self.scale = self.scale.saturating_add(1).checked_rem(SCALES.len()).unwrap_or(0);
            let percent = SCALES.get(self.scale).copied().unwrap_or(100);
            self.settings.ui_scale_percent = percent;
            cx.tree
                .set_scale(if percent == 0 { 1.0 } else { percent as f32 / 100.0 });
            if let Some(value) = self.scale_value {
                let label = if percent == 0 {
                    "По размеру экрана".to_owned()
                } else {
                    format!("{percent} %")
                };
                cx.tree.set_text(value, &label)?;
            }
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some("Настройки сохранены.".to_owned()),
                Err(error) => cx.status = Some(format!("Не удалось сохранить настройки: {error}")),
            }
        }

        if clicked.is_some() && clicked == self.language_button {
            self.language = self
                .language
                .saturating_add(1)
                .checked_rem(crate::strings::LANGUAGES.len())
                .unwrap_or(0);
            if let Some(value) = self.language_value {
                cx.tree
                    .set_text(value, LANGUAGE_NAMES.get(self.language).copied().unwrap_or("Русский"))?;
            }
            cx.status = Some(crate::strings::t("Язык применится после перезапуска приложения.").to_owned());
        }
        if clicked.is_some() && clicked == self.save_button {
            self.settings.language = crate::strings::LANGUAGES
                .get(self.language)
                .map(|code| (*code).to_owned());
            match save_settings(&self.settings) {
                Ok(()) => cx.status = Some(crate::strings::t("Настройки сохранены.").to_owned()),
                Err(error) => {
                    cx.status = Some(format!(
                        "{}{}",
                        crate::strings::t("Не удалось сохранить настройки: "),
                        error
                    ))
                }
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
