//! S1 (Claude): the shell's own screens: capabilities, settings.
//!
//! `Settings` is the reference screen for the other packages: build once, keep widget ids, react to clicks, report
//! through the status line, push slow work to a thread and come back with `AppMessage::ToScreen`.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::{Message, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{Align, Edges, GridPlacement, NodeKind, Size, Style, Track};
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

fn tr(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    crate::strings::tr_in(Some(crate::strings::current_language()), key, args)
}

#[derive(Clone, Copy)]
enum Support {
    Verified,
    Experimental,
    Research,
    Unsupported,
}

impl Support {
    fn label(self) -> &'static str {
        match self {
            Self::Verified => crate::strings::t("Запись"),
            Self::Experimental => crate::strings::t("Эксперим."),
            Self::Research => crate::strings::t("Чтение"),
            Self::Unsupported => crate::strings::t("Нет"),
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::Verified => crate::strings::t("Подтверждено и верифицировано в игре."),
            Self::Experimental => crate::strings::t("Экспериментальная поддержка (требуется проверка в игре)."),
            Self::Research => crate::strings::t("Исследование / режим только для чтения."),
            Self::Unsupported => crate::strings::t("Не поддерживается движком или форматом сохранения."),
        }
    }
}

fn game_labels() -> [&'static str; 7] {
    [
        crate::strings::t("ТЧ"),
        crate::strings::t("ЧН"),
        crate::strings::t("ЗП"),
        crate::strings::t("ТЧ EE"),
        crate::strings::t("ЧН EE"),
        crate::strings::t("ЗП EE"),
        "S2",
    ]
}

struct CapabilityRow {
    name: &'static str,
    description: &'static str,
    support: [Support; 7],
}

fn capability_rows() -> [CapabilityRow; 12] {
    [
        CapabilityRow {
            name: crate::strings::t("Деньги"),
            description: crate::strings::t("Изменение количества рублей у сталкера"),
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
            name: crate::strings::t("Стаки предметов"),
            description: crate::strings::t("Изменение количества в пачках патронов и расходников"),
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
            name: crate::strings::t("Прочность снаряжения"),
            description: crate::strings::t("Состояние и износ оружия, бронекостюмов и шлемов"),
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
            name: crate::strings::t("Размещение в слотах"),
            description: crate::strings::t("Слоты оружия, пояс для артефактов и рюкзак"),
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
            name: crate::strings::t("Апгрейды и модификации"),
            description: crate::strings::t("Установка и снятие веток улучшений оружия и брони"),
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
            name: crate::strings::t("Отношения группировок"),
            description: crate::strings::t("Редактирование очков репутации и враждебности фракций"),
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
            name: crate::strings::t("Фракция игрока"),
            description: crate::strings::t("Смена принадлежности сталкера к группировке"),
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
            name: crate::strings::t("Тайники (перемещение)"),
            description: crate::strings::t("Перемещение хабара из тайников в рюкзак и обратно"),
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
            name: crate::strings::t("Добавление предметов"),
            description: crate::strings::t("Спавн новых предметов из каталога в инвентарь"),
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
            name: crate::strings::t("Удаление предметов"),
            description: crate::strings::t("Безопасное удаление объектов из инвентаря"),
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
            name: crate::strings::t("Чтение инвентаря"),
            description: crate::strings::t("Парсинг предметов, патронов и экипировки"),
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
            name: crate::strings::t("Каталог предметов"),
            description: crate::strings::t("Сопоставление идентификаторов с официальными именами"),
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
    ]
}

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
        crate::strings::t("Игра × возможность: запись, чтение и причины ограничений")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let rows = capability_rows();
        let help = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            help,
            crate::strings::t("СПРАВКА ПО ВОЗМОЖНОСТЯМ"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            help,
            crate::strings::t("Что редактор умеет делать с сейвами каждой игры."),
            Text::Body,
        )?;

        let matrix = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            matrix,
            crate::strings::t("МАТРИЦА ПОДДЕРЖИВАЕМЫХ ВОЗМОЖНОСТЕЙ"),
            Text::Heading,
        )?;
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
                    .chain(std::iter::repeat_n(Track::Fixed(38.0), rows.len()))
                    .collect(),
            },
            Style {
                gap: Size::new(2.0, 2.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        grid_label(cx.tree, grid, crate::strings::t("ОПЕРАЦИЯ"), 0, 0, true)?;
        for (column, game) in game_labels().into_iter().enumerate() {
            grid_label(cx.tree, grid, game, column.saturating_add(1), 0, true)?;
        }
        for (row_index, row) in rows.iter().enumerate() {
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
            crate::strings::t("Выберите ячейку, чтобы увидеть причину уровня поддержки."),
            Text::Note,
        )?);

        let legend = style::card(cx.tree, host)?;
        style::label(cx.tree, legend, crate::strings::t("ОБОЗНАЧЕНИЯ"), Text::Heading)?;
        style::label(
            cx.tree,
            legend,
            crate::strings::t("Запись — полная поддержка чтения и записи, верифицировано тестами."),
            Text::Body,
        )?;
        style::label(
            cx.tree,
            legend,
            crate::strings::t("Эксперим. — поддержка реализована, ожидается подтверждение в игре."),
            Text::Body,
        )?;
        style::label(
            cx.tree,
            legend,
            crate::strings::t("Чтение — режим только для чтения."),
            Text::Body,
        )?;
        style::label(
            cx.tree,
            legend,
            crate::strings::t("Нет — механика отсутствует или не поддерживается."),
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
                let rows = capability_rows();
                let games = game_labels();
                if let (Some(capability), Some(game), Some(detail)) = (rows.get(*row), games.get(*column), self.detail)
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

fn load_settings() -> (sse_app::AppSettings, Option<String>) {
    match sse_app::AppSettings::load(&sse_app::default_settings_path()) {
        Ok(settings) => (settings, None),
        Err(error) => {
            sse_app::diagnostics::warn(&format!("settings file could not be loaded: {error}"));
            (
                sse_app::AppSettings::default(),
                Some(crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "Файл настроек не прочитан: {0}. При следующей записи он будет сохранён копией рядом.",
                    &[&error],
                )),
            )
        }
    }
}

/// Sends one field of the settings to the shared writer; the screen never writes a whole snapshot,
/// so changes made elsewhere (save folders, sidebar, report consent) are kept.
fn save_setting(
    patch: sse_app::settings_writer::SettingsPatch,
    proxy: Option<crate::event_loop::Proxy<AppMessage>>,
) -> Result<()> {
    super::submit_settings_write(patch, proxy)
}

/// Sends every field this screen edits; used by the explicit «Сохранить настройки» action.
fn save_all_settings(
    settings: &sse_app::AppSettings,
    proxy: Option<crate::event_loop::Proxy<AppMessage>>,
) -> Result<()> {
    use sse_app::settings_writer::SettingsPatch;
    let patches = [
        SettingsPatch::Language(settings.language.clone()),
        SettingsPatch::BackupDirectory(settings.backup_directory.clone()),
        SettingsPatch::SoundEnabled(settings.sound_enabled),
        SettingsPatch::MusicEnabled(settings.music_enabled),
        SettingsPatch::SoundVolume(settings.sound_volume),
        SettingsPatch::SendReports(settings.send_reports),
        SettingsPatch::MetricsConsent(settings.send_metrics),
    ];
    for patch in patches {
        save_setting(patch, proxy.clone())?;
    }
    Ok(())
}

const LANGUAGE_NAMES: [&str; 15] = [
    "",
    "",
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

fn language_name(index: usize) -> &'static str {
    match index {
        0 => crate::strings::t("Русский"),
        1 => crate::strings::t("Українська"),
        _ => LANGUAGE_NAMES
            .get(index)
            .copied()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| crate::strings::t("Русский")),
    }
}

struct DiagnosticReportFinished {
    result: std::result::Result<PathBuf, String>,
}

/// Result of the background folder dialog for the backup directory; `None` means the user cancelled.
struct PickedBackupDirectory(std::result::Result<Option<PathBuf>, String>);
struct MetricsUploadFinished {
    result: std::result::Result<(), String>,
}

fn performance_summary_text() -> String {
    let sessions = sse_app::metrics::recent_sessions();
    if sessions.is_empty() {
        return crate::strings::t("Метрики появятся после первого сеанса редактора.").to_owned();
    }
    sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let switch = session.screen_switch.map_or_else(
                || "—".to_owned(),
                |value| tr("{0}/{1}/{2} мс", &[&value.p50_ms, &value.p95_ms, &value.max_ms]),
            );
            let scroll = session.scrolling_frame.map_or_else(
                || "—".to_owned(),
                |value| tr("{0}/{1}/{2} мс", &[&value.p50_ms, &value.p95_ms, &value.max_ms]),
            );
            let memory = session
                .peak_memory_bytes
                .map_or_else(|| "—".to_owned(), |bytes| (bytes / (1024 * 1024)).to_string());
            let (width, height) = session.resolution.unwrap_or_default();
            let scale = session.scale_percent.map_or_else(|| "—".to_owned(), |value| value.to_string());
            let os = session.operating_system.unwrap_or("—");
            let saves = if session.save_operations.is_empty() {
                "—".to_owned()
            } else {
                session
                    .save_operations
                    .iter()
                    .map(|operation| {
                        let label = if operation.operation == "read" {
                            crate::strings::t("Чтение")
                        } else {
                            crate::strings::t("Запись")
                        };
                        tr(
                            "{0} {1}/{2}: {3} мс ×{4}",
                            &[
                                &label,
                                &operation.format,
                                &operation.size_bucket,
                                &operation.average_ms,
                                &operation.count,
                            ],
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let first_frame = session
                .first_frame_ms
                .map_or_else(|| "—".to_owned(), |value| value.to_string());
            let discovery = session
                .save_discovery_ms
                .map_or_else(|| "—".to_owned(), |value| value.to_string());
            tr(
                "Сеанс {0}: первый кадр {1} мс; переключения экранов p50/p95/макс {2}; прокрутка p50/p95/макс {3}; поиск сейвов {4} мс; пик памяти {5} МиБ; среда {6}, {7}×{8}, масштаб {9}%; чтение/запись: {10}.",
                &[&index.saturating_add(1), &first_frame, &switch, &scroll, &discovery, &memory, &os, &width, &height, &scale, &saves],
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn scrollable_paragraph(
    tree: &mut crate::widget::Tree,
    parent: WidgetId,
    text: &str,
    role: Text,
    width: f32,
    height: f32,
) -> Result<(WidgetId, WidgetId)> {
    let scroll = tree.add(
        Some(parent),
        NodeKind::Scroll {
            horizontal: false,
            vertical: true,
            offset_x: 0.0,
            offset_y: 0.0,
        },
        Style {
            min: Size::new(180.0, 100.0),
            preferred: Size::new(width, height),
            max: Size::new(width, height),
            grow: 1.0,
            shrink: 1.0,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;
    tree.set_clip_children(scroll, true)?;
    let paragraph = tree.add(
        Some(scroll),
        NodeKind::Leaf,
        Style {
            preferred: Size::new(width, 0.0),
            shrink: 0.0,
            ..Style::default()
        },
        Content::Paragraph {
            text: text.to_owned(),
            style: role.style(),
        },
        Look {
            text: role.color(),
            ..Look::default()
        },
    )?;
    Ok((scroll, paragraph))
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
    metrics_summary: Option<WidgetId>,
    metrics_summary_width: f32,
    metrics_refresh_button: Option<WidgetId>,
    metrics_consent_button: Option<WidgetId>,
    metrics_preview_button: Option<WidgetId>,
    metrics_preview_dialog: Option<WidgetId>,
    metrics_preview_scroll: Option<WidgetId>,
    metrics_preview_scroll_y: i32,
    metrics_preview_text: Option<WidgetId>,
    metrics_preview_send_button: Option<WidgetId>,
    metrics_preview_close: Option<WidgetId>,
    metrics_preview_body: Option<String>,
    metrics_upload_pending: bool,
    include_game_logs: bool,
    report_pending: bool,
    backup_input: Option<WidgetId>,
    backup_browse: Option<WidgetId>,
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
    /// Starts the system folder dialog. On macOS it must run on the interface thread; elsewhere it runs in the
    /// background so the window keeps painting. Cancelling leaves the field and the settings unchanged.
    fn browse_backup_directory(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if cfg!(target_os = "macos") {
            match sse_sys::directory_dialog::choose_directory() {
                Ok(Some(path)) => self.apply_backup_directory(cx.tree, &path)?,
                Ok(None) => {}
                Err(error) => cx.status = Some(tr("Системный диалог недоступен: {0}", &[&error])),
            }
            return Ok(());
        }
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some(crate::strings::t("Выбор папки доступен в работающем окне редактора.").to_owned());
            return Ok(());
        };
        let spawned = sse_app::tasks::try_spawn_named_detached("pick-backup-directory", move || {
            let picked = sse_sys::directory_dialog::choose_directory().map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Settings,
                Box::new(PickedBackupDirectory(picked)),
            ));
        });
        if let Err(error) = spawned {
            cx.status = Some(tr("Не удалось открыть выбор папки: {0}", &[&error]));
        }
        Ok(())
    }

    /// Puts a chosen folder into the field and runs the same sync as typing it.
    fn apply_backup_directory(&mut self, tree: &mut crate::widget::Tree, path: &std::path::Path) -> Result<()> {
        if let Some(input) = self.backup_input {
            tree.set_input_text(input, &path.display().to_string())?;
        }
        self.sync_backup_directory(tree);
        Ok(())
    }

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
            .unwrap_or(("zone", crate::strings::t("Тёмная")))
    }

    fn accent_name(&self) -> &'static str {
        crate::theme::ACCENT_NAMES
            .get(self.accent)
            .copied()
            .or_else(|| crate::theme::ACCENT_NAMES.first().copied())
            .unwrap_or_else(|| crate::strings::t("Янтарный"))
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
        crate::strings::t("Язык, тема, масштаб, обновления")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (settings, warning) = load_settings();
        self.settings = settings;
        if warning.is_some() {
            cx.status = warning;
        }
        if let Some(workspace) = &self.backup_workspace {
            workspace.set_backup_directory(sse_app::paths::backup_directory(&self.settings));
        }
        self.language = crate::strings::language_index(self.settings.language.as_deref().unwrap_or("ru"));
        let settings_root = style::row(cx.tree, host)?;
        let sections = style::card(cx.tree, settings_root)?;
        cx.tree.set_style(
            sections,
            Style {
                preferred: Size::new(200.0, 0.0),
                min: Size::new(200.0, 0.0),
                max: Size::new(200.0, f32::INFINITY),
                shrink: 0.0,
                padding: Edges::all(crate::theme::CARD_PADDING),
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: Align::Stretch,
                ..Style::default()
            },
        )?;
        style::label(cx.tree, sections, crate::strings::t("Разделы"), Text::Heading)?;
        for label in [
            crate::strings::t("ОБЩИЕ"),
            crate::strings::t("ИНТЕРФЕЙС"),
            crate::strings::t("ЗВУК"),
            crate::strings::t("ПУТИ"),
            crate::strings::t("ОБНОВЛЕНИЯ"),
            crate::strings::t("БЭКАПЫ"),
            crate::strings::t("ПОДДЕРЖКА"),
            crate::strings::t("ОТЧЁТЫ"),
            crate::strings::t("ВЕРСИЯ"),
        ] {
            self.section_buttons
                .push(style::button(cx.tree, sections, label, Button::Secondary)?);
        }
        let content = style::card(cx.tree, settings_root)?;

        let general = style::card(cx.tree, content)?;
        style::label(cx.tree, general, crate::strings::t("ОБЩИЕ"), Text::Heading)?;
        style::label(
            cx.tree,
            general,
            crate::strings::t("УПРАВЛЕНИЕ ОСНОВНЫМ ПОВЕДЕНИЕМ РЕДАКТОРА"),
            Text::Note,
        )?;
        #[cfg(feature = "native-ui")]
        {
            self.open_cloud_button = Some(style::button(
                cx.tree,
                general,
                crate::strings::t("Открыть Steam Cloud"),
                Button::Secondary,
            )?);
        }
        self.section_panels.push(general);

        let view = style::card(cx.tree, content)?;
        style::label(cx.tree, view, crate::strings::t("ИНТЕРФЕЙС"), Text::Heading)?;
        style::label(
            cx.tree,
            view,
            crate::strings::t("ЯЗЫК, ТЕМА, АКЦЕНТ И МАСШТАБ"),
            Text::Note,
        )?;
        self.section_panels.push(view);
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
        style::label(cx.tree, theme_line, crate::strings::t("Тема:"), Text::Body)?;
        self.theme_value = Some(style::label(
            cx.tree,
            theme_line,
            crate::strings::t(self.theme_choice().1),
            Text::Value,
        )?);
        self.theme_button = Some(style::button(
            cx.tree,
            theme_line,
            crate::strings::t("Изменить"),
            Button::Secondary,
        )?);

        let accent_line = style::row(cx.tree, view)?;
        style::label(cx.tree, accent_line, crate::strings::t("Акцент:"), Text::Body)?;
        self.accent_value = Some(style::label(
            cx.tree,
            accent_line,
            crate::strings::t(self.accent_name()),
            Text::Value,
        )?);
        self.accent_button = Some(style::button(
            cx.tree,
            accent_line,
            crate::strings::t("Изменить"),
            Button::Secondary,
        )?);

        let line = style::row(cx.tree, view)?;
        style::label(cx.tree, line, crate::strings::t("Масштаб интерфейса:"), Text::Body)?;
        let scale_label = if percent == 0 {
            crate::strings::t("По размеру экрана").to_owned()
        } else {
            format!("{percent} %")
        };
        self.scale_value = Some(style::label(cx.tree, line, &scale_label, Text::Value)?);
        self.scale_button = Some(style::button(
            cx.tree,
            line,
            crate::strings::t("Изменить"),
            Button::Secondary,
        )?);

        let language_line = style::row(cx.tree, view)?;
        style::label(
            cx.tree,
            language_line,
            crate::strings::t("Язык интерфейса:"),
            Text::Body,
        )?;
        let language_name = language_name(self.language);
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
        style::label(cx.tree, sound, crate::strings::t("Звуки интерфейса"), Text::Heading)?;
        self.sound_button = Some(style::button(
            cx.tree,
            sound,
            if self.settings.sound_enabled {
                crate::strings::t("Звуковые эффекты: ВКЛ")
            } else {
                crate::strings::t("Звуковые эффекты: ВЫКЛ")
            },
            Button::Secondary,
        )?);
        self.music_button = Some(style::button(
            cx.tree,
            sound,
            if self.settings.music_enabled {
                crate::strings::t("Музыка меню: ВКЛ")
            } else {
                crate::strings::t("Музыка меню: ВЫКЛ")
            },
            Button::Secondary,
        )?);
        self.volume_button = Some(style::button(
            cx.tree,
            sound,
            &tr("Громкость звуков: {0}%", &[&self.settings.sound_volume]),
            Button::Secondary,
        )?);
        self.section_panels.push(sound);

        let paths = style::card(cx.tree, content)?;
        style::label(cx.tree, paths, crate::strings::t("КАТАЛОГИ СОХРАНЕНИЙ"), Text::Heading)?;
        style::label(
            cx.tree,
            paths,
            crate::strings::t("Папки автоматического поиска сохранений (ТЧ, ЧН, ЗП, S2):"),
            Text::Body,
        )?;
        if let Some(dirs) = &self.settings.save_directories {
            for dir in dirs {
                style::label(cx.tree, paths, &dir.to_string_lossy(), Text::Note)?;
            }
        } else {
            style::label(
                cx.tree,
                paths,
                crate::strings::t("Используется автопоиск папок."),
                Text::Note,
            )?;
        }
        style::label(
            cx.tree,
            paths,
            crate::strings::t("Добавление/удаление/обзор требуют контроллера выбора папки; до его подключения изменения путей отключены."),
            Text::Note,
        )?;
        self.section_panels.push(paths);

        let updates = style::card(cx.tree, content)?;
        style::label(cx.tree, updates, crate::strings::t("ОБНОВЛЕНИЯ"), Text::Heading)?;
        let _line = style::row(cx.tree, updates)?;
        #[cfg(feature = "native-ui")]
        {
            self.open_updates_button = Some(style::button(
                cx.tree,
                _line,
                crate::strings::t("Открыть обновления"),
                Button::Primary,
            )?);
            style::label(
                cx.tree,
                _line,
                crate::strings::t("Проверка и установка доступны на экране обновлений."),
                Text::Note,
            )?;
        }
        style::label(
            cx.tree,
            updates,
            crate::strings::t(
                "Исправления для игры устанавливаются или обновляются только после явного действия пользователя.",
            ),
            Text::Note,
        )?;
        self.section_panels.push(updates);

        let backups = style::card(cx.tree, content)?;
        style::label(
            cx.tree,
            backups,
            crate::strings::t("РЕЗЕРВНОЕ КОПИРОВАНИЕ"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            backups,
            crate::strings::t("Папка для создания резервных копий и журналов восстановления:"),
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
        self.backup_browse = Some(style::button(
            cx.tree,
            backups,
            crate::strings::t("Обзор…"),
            Button::Secondary,
        )?);
        style::label(
            cx.tree,
            backups,
            crate::strings::t("Новое значение применяется сразу; settings.json обновится после сохранения настроек."),
            Text::Note,
        )?;
        self.section_panels.push(backups);

        let support = style::card(cx.tree, content)?;
        style::label(cx.tree, support, crate::strings::t("ПРОВЕРКА ОКРУЖЕНИЯ"), Text::Heading)?;
        style::label(
            cx.tree,
            support,
            crate::strings::t("Диагностика не нужна для обычного использования, но полезна для отчётов об ошибках."),
            Text::Body,
        )?;
        if let Some(crash) = sse_app::diagnostics::pending_crash() {
            style::label(
                cx.tree,
                support,
                crate::strings::t("Прошлый запуск завершился ошибкой. Сохраните отчёт и приложите его к issue."),
                Text::Value,
            )?;
            let preview = crash
                .lines()
                .next()
                .unwrap_or(crate::strings::t("Сведения о сбое записаны."));
            style::label(cx.tree, support, preview, Text::Note)?;
        }
        let support_actions = style::row(cx.tree, support)?;
        self.support_check_button = Some(style::button(
            cx.tree,
            support_actions,
            crate::strings::t("Проверить окружение"),
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
                crate::strings::t("Скрыть ошибку"),
                Button::Secondary,
            )?);
        }
        self.support_result = Some(style::label(
            cx.tree,
            support,
            crate::strings::t("Проверка окружения ещё не запускалась."),
            Text::Note,
        )?);
        style::label(
            cx.tree,
            support,
            crate::strings::t("МЕТРИКИ ПРОИЗВОДИТЕЛЬНОСТИ"),
            Text::Heading,
        )?;
        let window_width = f32::from(u16::try_from(cx.tree.size().0).unwrap_or(u16::MAX));
        self.metrics_summary_width = (window_width - 600.0).clamp(260.0, 720.0);
        self.metrics_summary = Some(cx.tree.add(
            Some(support),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(self.metrics_summary_width, 0.0),
                shrink: 0.0,
                ..Style::default()
            },
            Content::Paragraph {
                text: performance_summary_text(),
                style: Text::Note.style(),
            },
            Look {
                text: Text::Note.color(),
                ..Look::default()
            },
        )?);
        self.metrics_refresh_button = Some(style::button(
            cx.tree,
            support,
            crate::strings::t("Обновить сводку"),
            Button::Secondary,
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
        style::label(cx.tree, reports, crate::strings::t("ОТЧЁТЫ ОБ ОШИБКАХ"), Text::Heading)?;
        self.reports_button = Some(style::button(
            cx.tree,
            reports,
            if self.settings.send_reports {
                crate::strings::t("Отправлять отчёты: ВКЛ")
            } else {
                crate::strings::t("Отправлять отчёты: ВЫКЛ")
            },
            Button::Secondary,
        )?);
        self.send_report_button = None;
        style::label(
            cx.tree,
            reports,
            crate::strings::t("Ручная отправка отчёта отключена в Rust-версии."),
            Text::Note,
        )?;
        style::label(
            cx.tree,
            reports,
            crate::strings::t("Отправить обезличенные журналы и отчёт окружения. Сохранения не отправляются."),
            Text::Note,
        )?;
        self.metrics_consent_button = Some(style::button(
            cx.tree,
            reports,
            if self.settings.send_metrics {
                crate::strings::t("Отдельная передача метрик: ВКЛ")
            } else {
                crate::strings::t("Отдельная передача метрик: ВЫКЛ")
            },
            Button::Secondary,
        )?);
        self.metrics_preview_button = Some(style::button(
            cx.tree,
            reports,
            crate::strings::t("Предпросмотр агрегата"),
            Button::Secondary,
        )?);
        style::label(
            cx.tree,
            reports,
            crate::strings::t("Сейвы, пути и имена файлов в агрегат не входят."),
            Text::Note,
        )?;
        self.section_panels.push(reports);

        let (window_width, window_height) = cx.tree.size();
        let dialog_width = f32::from(u16::try_from(window_width.saturating_sub(48).clamp(320, 920)).unwrap_or(920));
        let dialog_height = f32::from(u16::try_from(window_height.saturating_sub(48).clamp(300, 680)).unwrap_or(680));
        let dialog_padding = crate::theme::CARD_PADDING;
        let preview_width = (dialog_width - dialog_padding * 2.0 - 8.0).max(240.0);
        let preview_height = (dialog_height - 170.0).max(100.0);
        let metrics_preview_dialog = style::card(cx.tree, dialog_host)?;
        self.metrics_preview_dialog = Some(metrics_preview_dialog);
        cx.tree.set_style(
            metrics_preview_dialog,
            Style {
                min: Size::new(280.0, 300.0),
                preferred: Size::new(dialog_width, dialog_height),
                max: Size::new(920.0, 680.0),
                padding: Edges::all(dialog_padding),
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: Align::Stretch,
                align_self: Some(Align::Center),
                shrink: 1.0,
                ..Style::default()
            },
        )?;
        style::label(
            cx.tree,
            metrics_preview_dialog,
            crate::strings::t("ПРЕДПРОСМОТР АГРЕГАТА"),
            Text::Heading,
        )?;
        let (preview_scroll, preview_text) = scrollable_paragraph(
            cx.tree,
            metrics_preview_dialog,
            "",
            Text::Note,
            preview_width,
            preview_height,
        )?;
        self.metrics_preview_scroll = Some(preview_scroll);
        self.metrics_preview_text = Some(preview_text);
        self.metrics_preview_send_button = Some(style::button(
            cx.tree,
            metrics_preview_dialog,
            crate::strings::t("Отправить агрегат"),
            Button::Secondary,
        )?);
        self.metrics_preview_close = Some(style::button(
            cx.tree,
            metrics_preview_dialog,
            crate::strings::t("Закрыть"),
            Button::Secondary,
        )?);
        cx.tree.set_visible(metrics_preview_dialog, false)?;

        let about = style::card(cx.tree, content)?;
        style::label(cx.tree, about, crate::strings::t("О ПРОГРАММЕ"), Text::Heading)?;
        style::label(
            cx.tree,
            about,
            &format!("S.T.A.L.K.E.R. Save Editor {}", env!("CARGO_PKG_VERSION")),
            Text::Value,
        )?;
        style::label(
            cx.tree,
            about,
            crate::strings::t("Редактор сохранений для всей серии S.T.A.L.K.E.R."),
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
        if let Message::Window(WindowEvent::Wheel { delta }) = message {
            if self
                .metrics_preview_dialog
                .is_some_and(|dialog| cx.tree.dialog() == Some(dialog))
            {
                if let Some(scroll) = self.metrics_preview_scroll {
                    let content_height = cx.tree.content_height(scroll)?;
                    let viewport = cx.tree.rect(scroll)?;
                    let viewport_height = f32::from(u16::try_from(viewport.height).unwrap_or(u16::MAX));
                    let limit = format!("{:.0}", (content_height - viewport_height).max(0.0))
                        .parse::<i32>()
                        .unwrap_or(0);
                    self.metrics_preview_scroll_y = self
                        .metrics_preview_scroll_y
                        .saturating_add(delta.saturating_mul(48))
                        .clamp(0, limit);
                    cx.tree.set_scroll_y(scroll, self.metrics_preview_scroll_y)?;
                    return Ok(());
                }
            }
        }
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
            cx.status =
                Some(crate::strings::t("Звук изменён; нажмите «Сохранить настройки», чтобы применить.").to_owned());
        }
        if clicked.is_some() && clicked == self.music_button {
            self.settings.music_enabled = !self.settings.music_enabled;
            cx.status =
                Some(crate::strings::t("Музыка изменена; нажмите «Сохранить настройки», чтобы применить.").to_owned());
        }
        if clicked.is_some() && clicked == self.volume_button {
            self.settings.sound_volume = self
                .settings
                .sound_volume
                .saturating_add(10)
                .checked_rem(110)
                .unwrap_or(0);
            cx.status = Some(tr(
                "Громкость: {0}%. Нажмите «Сохранить настройки».",
                &[&self.settings.sound_volume],
            ));
        }
        if clicked.is_some() && clicked == self.support_check_button {
            let report = sse_app::diagnostics::environment_report();
            if let Some(result) = self.support_result {
                cx.tree.set_text(result, &report.replace('\n', " · "))?;
            }
            cx.status = Some(crate::strings::t("Проверка окружения завершена.").to_owned());
        }
        if clicked.is_some() && clicked == self.metrics_refresh_button {
            if let Some(summary) = self.metrics_summary {
                cx.tree.set_text(summary, &performance_summary_text())?;
            }
        }
        if clicked.is_some() && clicked == self.metrics_consent_button {
            self.settings.send_metrics = !self.settings.send_metrics;
            if let Some(button) = self.metrics_consent_button {
                cx.tree.set_text(
                    button,
                    crate::strings::t(if self.settings.send_metrics {
                        "Отдельная передача метрик: ВКЛ"
                    } else {
                        "Отдельная передача метрик: ВЫКЛ"
                    }),
                )?;
            }
            match save_setting(
                sse_app::settings_writer::SettingsPatch::MetricsConsent(self.settings.send_metrics),
                cx.proxy.cloned(),
            ) {
                Ok(()) => cx.status = Some(crate::strings::t("Сохраняю…").to_owned()),
                Err(error) => cx.status = Some(tr("Не удалось сохранить настройки: {0}", &[&error])),
            }
        }
        if clicked.is_some() && clicked == self.metrics_preview_button {
            if let (Some(dialog), Some(preview_text)) = (self.metrics_preview_dialog, self.metrics_preview_text) {
                let preview = sse_app::metrics::upload_preview();
                self.metrics_preview_body = Some(preview.clone());
                cx.tree.set_text(preview_text, &preview)?;
                self.metrics_preview_scroll_y = 0;
                if let Some(scroll) = self.metrics_preview_scroll {
                    cx.tree.set_scroll_y(scroll, 0)?;
                }
                cx.tree.open_dialog(dialog)?;
            }
        }
        if clicked.is_some()
            && clicked == self.metrics_preview_send_button
            && self
                .metrics_preview_dialog
                .is_some_and(|dialog| cx.tree.dialog() == Some(dialog))
        {
            if self.metrics_upload_pending {
                cx.status = Some(crate::strings::t("Отправка метрик уже выполняется.").to_owned());
            } else if !self.settings.send_metrics {
                cx.status =
                    Some(crate::strings::t("Сначала включите отдельное согласие на передачу метрик.").to_owned());
            } else if let (Some(proxy), Some(preview)) = (cx.proxy.cloned(), self.metrics_preview_body.clone()) {
                self.metrics_upload_pending = true;
                if let Some(button) = self.metrics_preview_send_button {
                    cx.tree.set_text(button, crate::strings::t("Отправляю агрегат…"))?;
                }
                let task_proxy = proxy.clone();
                if let Err(error) = sse_app::tasks::try_spawn_named_detached("metrics-upload", move || {
                    let result = sse_app::metrics::upload_aggregate_preview(true, &preview).map_err(|error| {
                        sse_app::diagnostics::error(&format!("metrics upload: {error}"));
                        error.to_string()
                    });
                    let _ = task_proxy.send(AppMessage::ToScreen(
                        ScreenId::Settings,
                        Box::new(MetricsUploadFinished { result }),
                    ));
                }) {
                    self.metrics_upload_pending = false;
                    if let Some(button) = self.metrics_preview_send_button {
                        cx.tree.set_text(button, crate::strings::t("Отправить агрегат"))?;
                    }
                    cx.status = Some(tr("Не удалось запустить отправку: {0}", &[&error]));
                } else {
                    cx.status = Some(crate::strings::t("Отправляю агрегат…").to_owned());
                }
            } else {
                cx.status = Some(crate::strings::t("Предпросмотр агрегата недоступен.").to_owned());
            }
        }
        if clicked.is_some()
            && clicked == self.metrics_preview_close
            && self
                .metrics_preview_dialog
                .is_some_and(|dialog| cx.tree.dialog() == Some(dialog))
        {
            cx.tree.close_dialog()?;
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
            cx.status = Some(crate::strings::t("Ошибка скрыта.").to_owned());
        }
        if clicked.is_some() && clicked == self.reports_button {
            self.settings.send_reports = !self.settings.send_reports;
            cx.status =
                Some(crate::strings::t("Настройка отчётов изменена; нажмите «Сохранить настройки».").to_owned());
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
                .unwrap_or(("zone", crate::strings::t("Зона (тёмная)")));
            let accent_id = crate::theme::ACCENT_IDS.get(self.accent).copied().unwrap_or("amber");
            self.settings.theme_id = theme_id.to_owned();
            crate::theme::apply_appearance(&self.settings.theme_id, accent_id);
            repaint_theme(cx.tree, old, crate::theme::current());
            if let Some(value) = self.theme_value {
                cx.tree.set_text(value, crate::strings::t(theme_name))?;
            }
            let patch = sse_app::settings_writer::SettingsPatch::Theme(self.settings.theme_id.clone());
            match save_setting(patch, cx.proxy.cloned()) {
                Ok(()) => cx.status = Some(crate::strings::t("Сохраняю…").to_owned()),
                Err(error) => cx.status = Some(tr("Не удалось сохранить настройки: {0}", &[&error])),
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
                    crate::strings::t(
                        crate::theme::ACCENT_NAMES
                            .get(self.accent)
                            .copied()
                            .unwrap_or_else(|| crate::strings::t("Янтарный")),
                    ),
                )?;
            }
            let patch = sse_app::settings_writer::SettingsPatch::Accent(self.settings.accent_id.clone());
            match save_setting(patch, cx.proxy.cloned()) {
                Ok(()) => cx.status = Some(crate::strings::t("Сохраняю…").to_owned()),
                Err(error) => cx.status = Some(tr("Не удалось сохранить настройки: {0}", &[&error])),
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
                    crate::strings::t("По размеру экрана").to_owned()
                } else {
                    format!("{percent} %")
                };
                cx.tree.set_text(value, &label)?;
            }
            let patch = sse_app::settings_writer::SettingsPatch::Scale(percent);
            match save_setting(patch, cx.proxy.cloned()) {
                Ok(()) => cx.status = Some(crate::strings::t("Сохраняю…").to_owned()),
                Err(error) => cx.status = Some(tr("Не удалось сохранить настройки: {0}", &[&error])),
            }
        }

        if clicked.is_some() && clicked == self.language_button {
            self.language = self
                .language
                .saturating_add(1)
                .checked_rem(crate::strings::LANGUAGES.len())
                .unwrap_or(0);
            if let Some(value) = self.language_value {
                cx.tree.set_text(value, language_name(self.language))?;
            }
            cx.status = Some(crate::strings::t("Язык применится после перезапуска приложения.").to_owned());
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Settings, payload)) = message {
            if let Some(PickedBackupDirectory(picked)) = payload.downcast_ref::<PickedBackupDirectory>() {
                match picked {
                    Ok(Some(path)) => self.apply_backup_directory(cx.tree, path.as_path())?,
                    Ok(None) => {}
                    Err(error) => cx.status = Some(tr("Системный диалог недоступен: {0}", &[error])),
                }
                return Ok(());
            }
        }
        if clicked.is_some() && clicked == self.backup_browse {
            return self.browse_backup_directory(cx);
        }
        if clicked.is_some() && clicked == self.save_button {
            if let Some(input) = self.backup_input {
                let value = cx.tree.input_text(input).unwrap_or("").trim();
                self.settings.backup_directory = (!value.is_empty()).then(|| std::path::PathBuf::from(value));
            }
            self.settings.language = crate::strings::LANGUAGES
                .get(self.language)
                .map(|code| (*code).to_owned());
            match save_all_settings(&self.settings, cx.proxy.cloned()) {
                Ok(()) => cx.status = Some(crate::strings::t("Сохраняю…").to_owned()),
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
            if let Some(MetricsUploadFinished { result }) = payload.downcast_ref::<MetricsUploadFinished>() {
                self.metrics_upload_pending = false;
                if let Some(button) = self.metrics_preview_send_button {
                    cx.tree.set_text(button, crate::strings::t("Отправить агрегат"))?;
                }
                cx.status = Some(match result {
                    Ok(()) => crate::strings::t("Метрики отправлены.").to_owned(),
                    Err(error) => tr("Не удалось отправить метрики: {0}", &[error]),
                });
            }
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
    use super::{scrollable_paragraph, style, Settings, Text};
    use crate::glyphs::{Face, Fonts, TextStyle};
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::saves::Workspace;
    use crate::widget::{Content, Look, Tree};
    use std::path::PathBuf;

    #[test]
    fn capability_details_have_english_and_ukrainian_translations() {
        assert_eq!(
            crate::strings::t_in("en", "Выберите ячейку, чтобы увидеть причину уровня поддержки."),
            "Select a cell to see why this support level is available."
        );
        assert_ne!(
            crate::strings::t_in("uk", "Выберите ячейку, чтобы увидеть причину уровня поддержки."),
            crate::strings::t_in("ru", "Выберите ячейку, чтобы увидеть причину уровня поддержки.")
        );
    }

    #[test]
    fn metrics_preview_is_multiline_and_scrolls_through_the_cached_payload() -> sse_core::Result<()> {
        use crate::event_loop::{Message, WindowEvent};
        use crate::screens::{Context, Screen};

        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let root = tree.add(None, NodeKind::Stack, Style::default(), Content::Panel, Look::default())?;
        let dialog = tree.add(
            Some(root),
            NodeKind::Column,
            Style {
                preferred: crate::layout::Size::new(600.0, 360.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let body = format!(
            "{{\"schema\":1,\"sessions\":1,\"samples\":[{}]}}",
            "1234567890,".repeat(450)
        );
        let (scroll, preview) = scrollable_paragraph(&mut tree, dialog, &body, Text::Note, 520.0, 180.0)?;
        tree.resize(720, 500);
        tree.update_layout()?;
        let before = tree.rect(preview)?;
        let viewport = tree.rect(scroll)?;
        assert!(tree.content_height(scroll)? > f32::from(u16::try_from(viewport.height).unwrap_or(u16::MAX)));
        tree.open_dialog(dialog)?;

        let mut screen = Settings {
            metrics_preview_dialog: Some(dialog),
            metrics_preview_scroll: Some(scroll),
            metrics_preview_text: Some(preview),
            metrics_preview_body: Some(body),
            ..Settings::default()
        };
        let message = Message::Window(WindowEvent::Wheel { delta: 3 });
        let mut app = sse_app::AppState::new();
        let mut context = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.message(&mut context, &message, None)?;
        context.tree.update_layout()?;

        assert_eq!(screen.metrics_preview_scroll_y, 144);
        assert!(context.tree.rect(preview)?.y < before.y);
        Ok(())
    }

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
        let cloud_button = style::button(
            &mut tree,
            host,
            crate::strings::t("Открыть Steam Cloud"),
            style::Button::Secondary,
        )?;
        let updates_button = style::button(
            &mut tree,
            host,
            crate::strings::t("Открыть обновления"),
            style::Button::Primary,
        )?;
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
    fn picked_backup_folder_takes_the_same_path_as_typing_it() -> sse_core::Result<()> {
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
        let typed = Workspace::with_backup_directory(PathBuf::from("old-backups"));
        let mut typed_settings = Settings {
            backup_input: Some(input),
            backup_workspace: Some(typed.clone()),
            ..Settings::default()
        };
        tree.set_input_text(input, "picked-backups")?;
        typed_settings.sync_backup_directory(&tree);

        let picked = Workspace::with_backup_directory(PathBuf::from("old-backups"));
        let mut picked_settings = Settings {
            backup_input: Some(input),
            backup_workspace: Some(picked.clone()),
            ..Settings::default()
        };
        picked_settings.apply_backup_directory(&mut tree, std::path::Path::new("picked-backups"))?;

        assert_eq!(picked.backup_directory(), typed.backup_directory());
        assert_eq!(tree.input_text(input)?, "picked-backups");
        assert_eq!(
            picked_settings.settings.backup_directory,
            typed_settings.settings.backup_directory
        );
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
            crate::strings::t("Игровые логи: ВЫКЛ"),
            crate::screens::style::Button::Secondary,
        )?;
        let dialog = crate::screens::style::card(&mut tree, host)?;
        let confirm = crate::screens::style::button(
            &mut tree,
            dialog,
            crate::strings::t("Добавить"),
            crate::screens::style::Button::Primary,
        )?;
        let cancel = crate::screens::style::button(
            &mut tree,
            dialog,
            crate::strings::t("Отмена"),
            crate::screens::style::Button::Secondary,
        )?;
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
