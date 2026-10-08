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
use std::path::PathBuf;

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    vec![Box::new(Capabilities::default()), Box::new(Settings::default())]
}

pub(crate) fn screens_with_workspace(workspace: super::saves::Workspace) -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Capabilities::default()),
        Box::new(Settings {
            backup_workspace: Some(workspace),
            ..Settings::default()
        }),
    ]
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
        if heading {
            Content::Paragraph {
                text: text.to_owned(),
                style: TextStyle::new(Face::Heading, 11.0),
            }
        } else {
            Content::Label {
                text: text.to_owned(),
                style: TextStyle::new(Face::Body, 13.0),
            }
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
                    Track::Fraction(2.2),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                    Track::Fraction(1.0),
                ],
                rows: std::iter::once(Track::Fixed(52.0))
                    .chain(std::iter::repeat_n(Track::Fixed(38.0), CAPABILITY_ROWS.len()))
                    .collect(),
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
                let id = grid_cell(cx.tree, grid, support.label(), column.saturating_add(1), grid_row)?;
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
                    let Some(support) = capability.support.get(*column).copied() else {
                        return Ok(());
                    };
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
    let _ = sse_app::settings_writer::submit(sse_app::settings_writer::SettingsPatch::Replace(settings.clone()));
    Ok(())
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

struct DiagnosticReportFinished {
    result: std::result::Result<PathBuf, String>,
}

/// Settings screen.
#[derive(Default)]
pub struct Settings {
    section_buttons: Vec<WidgetId>,
    section_panels: Vec<WidgetId>,
    selected_section: usize,
    sound_button: Option<WidgetId>,
    music_button: Option<WidgetId>,
    volume_button: Option<WidgetId>,
    reports_button: Option<WidgetId>,
    send_report_button: Option<WidgetId>,
    support_check_button: Option<WidgetId>,
    support_save_button: Option<WidgetId>,
    game_logs_button: Option<WidgetId>,
    game_logs_confirmation: Option<WidgetId>,
    confirm_game_logs: Option<WidgetId>,
    cancel_game_logs: Option<WidgetId>,
    support_dismiss_button: Option<WidgetId>,
    support_result: Option<WidgetId>,
    include_game_logs: bool,
    report_pending: bool,
    backup_input: Option<WidgetId>,
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
    #[cfg(feature = "native-ui")]
    open_cloud_button: Option<WidgetId>,
    #[cfg(feature = "native-ui")]
    open_updates_button: Option<WidgetId>,
    scale: usize,
    theme: usize,
    accent: usize,
    settings: sse_app::AppSettings,
    backup_workspace: Option<super::saves::Workspace>,
}

impl Settings {
    fn sync_backup_directory(&mut self, tree: &crate::widget::Tree) {
        if let (Some(input), Some(workspace)) = (self.backup_input, self.backup_workspace.as_ref()) {
            let value = tree.input_text(input).unwrap_or("").trim();
            self.settings.backup_directory = (!value.is_empty()).then(|| std::path::PathBuf::from(value));
            workspace.set_backup_directory(sse_app::paths::backup_directory(&self.settings));
        }
    }

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
        if let Some(workspace) = &self.backup_workspace {
            workspace.set_backup_directory(sse_app::paths::backup_directory(&self.settings));
        }
        self.language = crate::strings::language_index(self.settings.language.as_deref().unwrap_or("ru"));
        let settings_root = style::row(cx.tree, host)?;
        let sections = style::card(cx.tree, settings_root)?;
        style::label(cx.tree, sections, "Разделы", Text::Heading)?;
        for name in [
            "ОБЩИЕ",
            "ИНТЕРФЕЙС",
            "Звук",
            "ПУТИ И АВТОПОИСК",
            "ОБНОВЛЕНИЯ",
            "РЕЗЕРВНЫЕ КОПИИ",
            "ИНСТРУМЕНТЫ ДЛЯ ПОДДЕРЖКИ",
            "ОТЧЁТЫ И ПРИВАТНОСТЬ",
            "ВЕРСИЯ",
        ] {
            self.section_buttons
                .push(style::button(cx.tree, sections, name, Button::Secondary)?);
        }
        let content = style::card(cx.tree, settings_root)?;

        let general = style::card(cx.tree, content)?;
        style::label(cx.tree, general, "ОБЩИЕ", Text::Heading)?;
        style::label(cx.tree, general, "УПРАВЛЕНИЕ ОСНОВНЫМ ПОВЕДЕНИЕМ РЕДАКТОРА", Text::Note)?;
        #[cfg(feature = "native-ui")]
        {
            style::label(cx.tree, general, "[ STEAM CLOUD ]", Text::Value)?;
            style::label(cx.tree, general, "Откройте экран Steam Cloud, чтобы просматривать состояние синхронизации и выполнять действия с явным подтверждением.", Text::Body)?;
            self.open_cloud_button = Some(style::button(
                cx.tree,
                general,
                "Открыть Steam Cloud",
                Button::Secondary,
            )?);
        }
        self.section_panels.push(general);

        let view = style::card(cx.tree, content)?;
        style::label(cx.tree, view, "ИНТЕРФЕЙС", Text::Heading)?;
        style::label(cx.tree, view, "ЯЗЫК, ТЕМА, АКЦЕНТ И МАСШТАБ", Text::Note)?;
        self.section_panels.push(view);
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

        let sound = style::card(cx.tree, content)?;
        style::label(cx.tree, sound, "Звуки интерфейса", Text::Heading)?;
        self.sound_button = Some(style::button(
            cx.tree,
            sound,
            if self.settings.sound_enabled {
                "Звуковые эффекты: ВКЛ"
            } else {
                "Звуковые эффекты: ВЫКЛ"
            },
            Button::Secondary,
        )?);
        self.music_button = Some(style::button(
            cx.tree,
            sound,
            if self.settings.music_enabled {
                "Музыка меню: ВКЛ"
            } else {
                "Музыка меню: ВЫКЛ"
            },
            Button::Secondary,
        )?);
        self.volume_button = Some(style::button(
            cx.tree,
            sound,
            &format!("Громкость звуков: {}%", self.settings.sound_volume),
            Button::Secondary,
        )?);
        self.section_panels.push(sound);

        let paths = style::card(cx.tree, content)?;
        style::label(cx.tree, paths, "КАТАЛОГИ СОХРАНЕНИЙ", Text::Heading)?;
        style::label(
            cx.tree,
            paths,
            "Папки автоматического поиска сохранений (ТЧ, ЧН, ЗП, S2):",
            Text::Body,
        )?;
        if let Some(dirs) = &self.settings.save_directories {
            for dir in dirs {
                style::label(cx.tree, paths, &dir.to_string_lossy(), Text::Note)?;
            }
        } else {
            style::label(cx.tree, paths, "Используется автопоиск папок.", Text::Note)?;
        }
        style::label(
            cx.tree,
            paths,
            "Добавление/удаление/обзор требуют контроллера выбора папки; до его подключения изменения путей отключены.",
            Text::Note,
        )?;
        self.section_panels.push(paths);

        let updates = style::card(cx.tree, content)?;
        style::label(cx.tree, updates, "ОБНОВЛЕНИЯ", Text::Heading)?;
        let line = style::row(cx.tree, updates)?;
        #[cfg(feature = "native-ui")]
        {
            self.open_updates_button = Some(style::button(cx.tree, line, "Открыть обновления", Button::Primary)?);
            style::label(
                cx.tree,
                line,
                "Проверка и установка доступны на экране обновлений.",
                Text::Note,
            )?;
        }
        style::label(
            cx.tree,
            updates,
            "Исправления для игры устанавливаются или обновляются только после явного действия пользователя.",
            Text::Note,
        )?;
        self.section_panels.push(updates);

        let backups = style::card(cx.tree, content)?;
        style::label(cx.tree, backups, "РЕЗЕРВНОЕ КОПИРОВАНИЕ", Text::Heading)?;
        style::label(
            cx.tree,
            backups,
            "Папка для создания резервных копий и журналов восстановления:",
            Text::Body,
        )?;
        let backup_value = self
            .settings
            .backup_directory
            .as_ref()
            .map_or("", |p| p.to_str().unwrap_or(""));
        self.backup_input = Some(cx.tree.add(
            Some(backups),
            NodeKind::Leaf,
            Style {
                min: Size::new(220.0, 36.0),
                ..Style::default()
            },
            Content::Input {
                text: backup_value.to_owned(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?);
        style::label(
            cx.tree,
            backups,
            "Новое значение применяется сразу; settings.json обновится после сохранения настроек.",
            Text::Note,
        )?;
        self.section_panels.push(backups);

        let support = style::card(cx.tree, content)?;
        style::label(cx.tree, support, "ПРОВЕРКА ОКРУЖЕНИЯ", Text::Heading)?;
        style::label(
            cx.tree,
            support,
            "Диагностика не нужна для обычного использования, но полезна для отчётов об ошибках.",
            Text::Body,
        )?;
        if let Some(crash) = sse_app::diagnostics::pending_crash() {
            style::label(
                cx.tree,
                support,
                "Прошлый запуск завершился ошибкой. Сохраните отчёт и приложите его к issue.",
                Text::Value,
            )?;
            let preview = crash.lines().next().unwrap_or("Сведения о сбое записаны.");
            style::label(cx.tree, support, preview, Text::Note)?;
        }
        let support_actions = style::row(cx.tree, support)?;
        self.support_check_button = Some(style::button(
            cx.tree,
            support_actions,
            "Проверить окружение",
            Button::Secondary,
        )?);
        self.support_save_button = Some(style::button(
            cx.tree,
            support_actions,
            crate::strings::t("Сохранить отчёт…"),
            Button::Secondary,
        )?);
        let game_logs_row = style::row(cx.tree, support)?;
        self.game_logs_button = Some(style::button(
            cx.tree,
            game_logs_row,
            crate::strings::t("Игровые логи: ВЫКЛ"),
            Button::Secondary,
        )?);
        style::label(
            cx.tree,
            game_logs_row,
            crate::strings::t("Отчёт сохраняется локально; ничего не отправляется."),
            Text::Note,
        )?;
        if sse_app::diagnostics::pending_crash().is_some() {
            self.support_dismiss_button = Some(style::button(
                cx.tree,
                support_actions,
                "Скрыть ошибку",
                Button::Secondary,
            )?);
        }
        self.support_result = Some(style::label(
            cx.tree,
            support,
            "Проверка окружения ещё не запускалась.",
            Text::Note,
        )?);
        self.section_panels.push(support);

        let dialog_host = cx.tree.overlay_host().unwrap_or(host);
        let game_logs_confirmation = style::card(cx.tree, dialog_host)?;
        self.game_logs_confirmation = Some(game_logs_confirmation);
        style::label(
            cx.tree,
            game_logs_confirmation,
            crate::strings::t("ЛОГИ ИГРЫ В ОТЧЁТЕ"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            game_logs_confirmation,
            crate::strings::t(
                "Логи игры могут содержать имя пользователя и список модов. Добавить их только в локальный ZIP-отчёт?",
            ),
            Text::Body,
        )?;
        let game_logs_actions = style::row(cx.tree, game_logs_confirmation)?;
        self.confirm_game_logs = Some(style::button(
            cx.tree,
            game_logs_actions,
            crate::strings::t("Добавить"),
            Button::Primary,
        )?);
        self.cancel_game_logs = Some(style::button(
            cx.tree,
            game_logs_actions,
            crate::strings::t("Отмена"),
            Button::Secondary,
        )?);
        cx.tree.set_visible(game_logs_confirmation, false)?;

        let reports = style::card(cx.tree, content)?;
        style::label(cx.tree, reports, "ОТЧЁТЫ ОБ ОШИБКАХ", Text::Heading)?;
        self.reports_button = Some(style::button(
            cx.tree,
            reports,
            if self.settings.send_reports {
                "Отправлять отчёты: ВКЛ"
            } else {
                "Отправлять отчёты: ВЫКЛ"
            },
            Button::Secondary,
        )?);
        self.send_report_button = None;
        style::label(
            cx.tree,
            reports,
            "Ручная отправка отчёта отключена в Rust-версии.",
            Text::Note,
        )?;
        style::label(
            cx.tree,
            reports,
            "Отправить обезличенные журналы и отчёт окружения. Сохранения не отправляются.",
            Text::Note,
        )?;
        self.section_panels.push(reports);

        let about = style::card(cx.tree, content)?;
        style::label(cx.tree, about, "О ПРОГРАММЕ", Text::Heading)?;
        style::label(
            cx.tree,
            about,
            &format!("S.T.A.L.K.E.R. Save Editor {}", env!("CARGO_PKG_VERSION")),
            Text::Value,
        )?;
        style::label(
            cx.tree,
            about,
            "Редактор сохранений для всей серии S.T.A.L.K.E.R.",
            Text::Body,
        )?;
        self.section_panels.push(about);

        for (index, panel) in self.section_panels.iter().copied().enumerate() {
            cx.tree.set_visible(panel, index == 0)?;
        }
        self.save_button = Some(style::button(
            cx.tree,
            host,
            crate::strings::t("Сохранить настройки"),
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
        self.sync_backup_directory(cx.tree);
        if clicked.is_some() && clicked == self.game_logs_button {
            if self.include_game_logs {
                self.include_game_logs = false;
                if let Some(button) = self.game_logs_button {
                    cx.tree.set_text(button, crate::strings::t("Игровые логи: ВЫКЛ"))?;
                }
            } else if let Some(dialog) = self.game_logs_confirmation {
                cx.tree.open_dialog(dialog)?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.cancel_game_logs {
            if self
                .game_logs_confirmation
                .is_some_and(|dialog| cx.tree.dialog() == Some(dialog))
            {
                cx.tree.close_dialog()?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_game_logs {
            if self
                .game_logs_confirmation
                .is_some_and(|dialog| cx.tree.dialog() == Some(dialog))
            {
                cx.tree.close_dialog()?;
            }
            self.include_game_logs = true;
            if let Some(button) = self.game_logs_button {
                cx.tree.set_text(button, crate::strings::t("Игровые логи: ВКЛ"))?;
            }
            return Ok(());
        }
        if clicked.is_some() {
            if let Some(index) = self.section_buttons.iter().position(|id| Some(*id) == clicked) {
                self.selected_section = index;
                for (panel_index, panel) in self.section_panels.iter().copied().enumerate() {
                    cx.tree.set_visible(panel, panel_index == index)?;
                }
                return Ok(());
            }
        }
        if clicked.is_some() && clicked == self.sound_button {
            self.settings.sound_enabled = !self.settings.sound_enabled;
            cx.status = Some("Звук изменён; нажмите «Сохранить настройки», чтобы применить.".to_owned());
        }
        if clicked.is_some() && clicked == self.music_button {
            self.settings.music_enabled = !self.settings.music_enabled;
            cx.status = Some("Музыка изменена; нажмите «Сохранить настройки», чтобы применить.".to_owned());
        }
        if clicked.is_some() && clicked == self.volume_button {
            self.settings.sound_volume = self
                .settings
                .sound_volume
                .saturating_add(10)
                .checked_rem(110)
                .unwrap_or(0);
            cx.status = Some(format!(
                "Громкость: {}%. Нажмите «Сохранить настройки».",
                self.settings.sound_volume
            ));
        }
        if clicked.is_some() && clicked == self.support_check_button {
            let report = sse_app::diagnostics::environment_report();
            if let Some(result) = self.support_result {
                cx.tree.set_text(result, &report.replace('\n', " · "))?;
            }
            cx.status = Some("Проверка окружения завершена.".to_owned());
        }
        if clicked.is_some() && clicked == self.support_save_button {
            if self.report_pending {
                cx.status = Some(crate::strings::t("Отчёт уже собирается.").to_owned());
            } else if let Some(proxy) = cx.proxy.cloned() {
                self.report_pending = true;
                if let Some(button) = self.support_save_button {
                    cx.tree.set_text(button, crate::strings::t("Собираю отчёт в фоне…"))?;
                }
                let path = sse_app::paths::default_data_directory().join("diagnostics-report.zip");
                let worker_path = path.clone();
                let include_game_logs = self.include_game_logs;
                sse_app::tasks::spawn_named_detached("diagnostics-background", move || {
                    let games = {
                        #[cfg(feature = "native-ui")]
                        {
                            crate::screens::games::discover_game_installations()
                                .into_iter()
                                .map(|installation| sse_app::diagnostics::DiagnosticGame {
                                    title: installation.title,
                                    install_directory: installation.directory,
                                    is_stalker2: installation.target == crate::screens::games::GameTarget::Stalker2,
                                })
                                .collect::<Vec<_>>()
                        }
                        #[cfg(not(feature = "native-ui"))]
                        {
                            Vec::<sse_app::diagnostics::DiagnosticGame>::new()
                        }
                    };
                    let result = sse_app::diagnostics::save_diagnostics_zip(&worker_path, &games, include_game_logs)
                        .map(|()| worker_path.clone())
                        .map_err(|error| {
                            sse_app::diagnostics::error(&format!("diagnostic report: {error}"));
                            error.to_string()
                        });
                    proxy.send(AppMessage::ToScreen(
                        ScreenId::Settings,
                        Box::new(DiagnosticReportFinished { result }),
                    ));
                });
                cx.status = Some(crate::strings::t("Собираю отчёт в фоне…").to_owned());
            } else {
                cx.status = Some(crate::strings::t("Фоновая очередь недоступна.").to_owned());
            }
        }
        if clicked.is_some() && clicked == self.support_dismiss_button {
            sse_app::diagnostics::dismiss_crash();
            cx.status = Some("Ошибка скрыта.".to_owned());
        }
        if clicked.is_some() && clicked == self.reports_button {
            self.settings.send_reports = !self.settings.send_reports;
            cx.status = Some("Настройка отчётов изменена; нажмите «Сохранить настройки».".to_owned());
        }
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
            if let Some(input) = self.backup_input {
                let value = cx.tree.input_text(input).unwrap_or("").trim();
                self.settings.backup_directory = (!value.is_empty()).then(|| std::path::PathBuf::from(value));
            }
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

        #[cfg(feature = "native-ui")]
        if clicked.is_some() && clicked == self.open_cloud_button {
            if let Some(proxy) = cx.proxy.cloned() {
                proxy.send(AppMessage::OpenScreen(ScreenId::Cloud));
            }
        }
        #[cfg(feature = "native-ui")]
        if clicked.is_some() && clicked == self.open_updates_button {
            if let Some(proxy) = cx.proxy.cloned() {
                proxy.send(AppMessage::OpenScreen(ScreenId::Updates));
            }
        }
        if let Message::User(AppMessage::ToScreen(_, payload)) = message {
            if let Some(DiagnosticReportFinished { result }) = payload.downcast_ref::<DiagnosticReportFinished>() {
                self.report_pending = false;
                if let Some(button) = self.support_save_button {
                    cx.tree.set_text(button, crate::strings::t("Сохранить отчёт…"))?;
                }
                cx.status = Some(match result {
                    Ok(path) => format!("{}{}", crate::strings::t("Отчёт сохранён: "), path.display()),
                    Err(error) => format!("{}{error}", crate::strings::t("Не удалось сохранить отчёт: ")),
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{style, Settings};
    use crate::glyphs::{Face, Fonts, TextStyle};
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::saves::Workspace;
    use crate::widget::{Content, Look, Tree};
    use std::path::PathBuf;

    #[test]
    #[cfg(feature = "native-ui")]
    fn settings_actions_dispatch_to_cloud_and_update_screens() -> sse_core::Result<()> {
        use crate::event_loop::{channel_pair, Message};
        use crate::screens::{AppMessage, Context, Screen, ScreenId};
        use crate::widget::{Content, Look, Tree};

        let mut tree = Tree::new(Fonts::bundled()?, crate::raster::Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let cloud_button = style::button(&mut tree, host, "Открыть Steam Cloud", style::Button::Secondary)?;
        let updates_button = style::button(&mut tree, host, "Открыть обновления", style::Button::Primary)?;
        let (proxy, receiver) = channel_pair();
        let mut app = sse_app::AppState::new();
        let mut screen = Settings {
            open_cloud_button: Some(cloud_button),
            open_updates_button: Some(updates_button),
            ..Settings::default()
        };
        let tick = Message::User(AppMessage::Tick(0));
        let mut context = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };

        screen.message(&mut context, &tick, Some(cloud_button))?;
        screen.message(&mut context, &tick, Some(updates_button))?;

        assert!(matches!(
            receiver.try_recv().ok(),
            Some(Message::User(AppMessage::OpenScreen(ScreenId::Cloud)))
        ));
        assert!(matches!(
            receiver.try_recv().ok(),
            Some(Message::User(AppMessage::OpenScreen(ScreenId::Updates)))
        ));
        Ok(())
    }

    #[test]
    fn backup_input_updates_shared_path_before_settings_are_saved() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let input = tree.add(
            None,
            NodeKind::Leaf,
            Style::default(),
            Content::Input {
                text: String::new(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?;
        tree.set_input_text(input, "custom-backups")?;
        let workspace = Workspace::with_backup_directory(PathBuf::from("old-backups"));
        let mut settings = Settings {
            backup_input: Some(input),
            backup_workspace: Some(workspace.clone()),
            ..Settings::default()
        };

        settings.sync_backup_directory(&tree);

        assert_eq!(workspace.backup_directory(), PathBuf::from("custom-backups"));
        Ok(())
    }

    #[test]
    fn game_log_collection_requires_a_warning_confirmation() -> sse_core::Result<()> {
        use crate::screens::{AppMessage, Context, Screen};
        use crate::widget::Content;
        use crate::{event_loop::Message, layout::NodeKind};

        assert_eq!(crate::strings::t_in("en", "Игровые логи: ВЫКЛ"), "Game logs: OFF");
        assert_eq!(
            crate::strings::t_in("de", "Отчёт сохраняется локально; ничего не отправляется."),
            "Der Bericht wird lokal gespeichert; es wird nichts gesendet."
        );

        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let toggle = crate::screens::style::button(
            &mut tree,
            host,
            "Игровые логи: ВЫКЛ",
            crate::screens::style::Button::Secondary,
        )?;
        let dialog = crate::screens::style::card(&mut tree, host)?;
        let confirm =
            crate::screens::style::button(&mut tree, dialog, "Добавить", crate::screens::style::Button::Primary)?;
        let cancel =
            crate::screens::style::button(&mut tree, dialog, "Отмена", crate::screens::style::Button::Secondary)?;
        tree.set_visible(dialog, false)?;
        let mut screen = Settings {
            game_logs_button: Some(toggle),
            game_logs_confirmation: Some(dialog),
            confirm_game_logs: Some(confirm),
            cancel_game_logs: Some(cancel),
            ..Settings::default()
        };
        let mut app = sse_app::AppState::new();
        let message = Message::User(AppMessage::Tick(0));
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };

        screen.message(&mut context, &message, Some(toggle))?;
        assert!(context.tree.dialog_open());
        assert!(!screen.include_game_logs);
        screen.message(&mut context, &message, Some(cancel))?;
        assert!(!context.tree.dialog_open());
        assert!(!screen.include_game_logs);
        screen.message(&mut context, &message, Some(toggle))?;
        screen.message(&mut context, &message, Some(confirm))?;
        assert!(!context.tree.dialog_open());
        assert!(screen.include_game_logs);
        Ok(())
    }
}
