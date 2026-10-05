//! The editor frame: grouped sidebar, header, content host, status line; builds screens lazily and routes messages.

use super::style::{self, rgb, Text};
use super::{AppMessage, Context, EditorAction, Group, Screen, ScreenId};
use crate::event_loop::{App, Flow, Message, Proxy, WindowEvent};
use crate::glyphs::{to_px, Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::path::Icon;
use crate::widget::{Content, Look, TextAlign, Tree, WidgetId};
use crate::widgets::scroll::ScrollView;
use sse_core::Result;
use std::path::Path;

const KEY_ESCAPE: u32 = 0xff1b;
const KEY_TAB: u32 = 0xff09;
const KEY_RETURN: u32 = 0xff0d;
const KEY_UP: u32 = 0xff52;
const KEY_DOWN: u32 = 0xff54;
const SAVE_LIBRARY_PAGE_SIZE: usize = 8;

struct SaveEligibility {
    reason: String,
    can_save: bool,
    change_count: usize,
}

fn save_eligibility(
    has_save: bool,
    format_id: Option<&str>,
    legacy_s2: bool,
    plan: Option<&sse_storage::drafts::DraftPlan>,
    invalid_numbers: bool,
) -> SaveEligibility {
    if !has_save {
        return SaveEligibility {
            reason: crate::strings::t("Выберите сохранение для редактирования.").to_owned(),
            can_save: false,
            change_count: 0,
        };
    }
    let has_unmapped = plan.is_some_and(|plan| plan.unmapped_legacy_plan.is_some());
    let change_count = plan.map_or(0, |plan| {
        usize::from(plan.money.is_some())
            .saturating_add(plan.stack_counts.len())
            .saturating_add(plan.durability.len())
            .saturating_add(plan.placements.len())
            .saturating_add(plan.upgrades.len())
            .saturating_add(plan.detach_handles.len())
            .saturating_add(plan.adds.len())
            .saturating_add(plan.stash_takes.len())
            .saturating_add(plan.s2_stash_takes.len())
            .saturating_add(plan.stash_puts.len())
            .saturating_add(usize::from(has_unmapped))
    });
    let has_changes = change_count > 0;
    let unsupported = has_changes && plan.is_some_and(|plan| has_unsupported_edit(format_id, legacy_s2, plan));
    let reason = if has_unmapped {
        "В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом).".to_owned()
    } else if unsupported {
        format!(
            "Эта правка для формата {} не поддерживается (см. «Возможности»).",
            display_format_name(format_id.unwrap_or("неизвестный"))
        )
    } else if !has_changes {
        "Нет несохранённых изменений.".to_owned()
    } else if invalid_numbers {
        "Введены некорректные значения (проверьте введённые числа).".to_owned()
    } else {
        "Сохранить изменения в файл сейва (с созданием резервной копии).".to_owned()
    };
    SaveEligibility {
        reason,
        can_save: has_changes && !has_unmapped && !unsupported && !invalid_numbers,
        change_count,
    }
}

fn has_unsupported_edit(format_id: Option<&str>, legacy_s2: bool, plan: &sse_storage::drafts::DraftPlan) -> bool {
    match format_id {
        Some("stalker2" | "s2") => {
            legacy_s2
                || !plan.placements.is_empty()
                || !plan.upgrades.is_empty()
                || !plan.detach_handles.is_empty()
                || !plan.adds.is_empty()
                || !plan.stash_takes.is_empty()
                || (!super::saves::S2_STASH_MOVE_ENABLED && !plan.s2_stash_takes.is_empty())
                || plan.s2_stash_takes.len() > 1
                || !plan.stash_puts.is_empty()
        }
        Some(id) => {
            let format = match id {
                "stalker-soc" | "soc" => sse_xray::Format::Soc,
                "stalker-cs" | "clear_sky" => sse_xray::Format::Cs,
                "stalker-cop" | "cop" => sse_xray::Format::Cop,
                "stalker-soc-ee" => sse_xray::Format::SocEe,
                "stalker-cs-ee" => sse_xray::Format::CsEe,
                "stalker-cop-ee" => sse_xray::Format::CopEe,
                _ => return true,
            };
            use sse_xray::writer::{Capability, ChangeKind};
            let supports = |kind| sse_xray::writer::capability(format, kind) != Capability::Unsupported;
            (plan.money.is_some() && !supports(ChangeKind::EditMoney))
                || (!plan.stack_counts.is_empty() && !supports(ChangeKind::EditStacks))
                || (!plan.durability.is_empty() && !supports(ChangeKind::EditDurability))
                || (!plan.placements.is_empty() && !supports(ChangeKind::EditPlacement))
                || (!plan.upgrades.is_empty() && !supports(ChangeKind::EditUpgrades))
                || (!plan.detach_handles.is_empty() && !supports(ChangeKind::RemoveItems))
                || (!plan.adds.is_empty() && !supports(ChangeKind::AddItems))
                || !plan.stash_takes.is_empty()
                || !plan.s2_stash_takes.is_empty()
                || !plan.stash_puts.is_empty()
        }
        None => true,
    }
}

fn display_format_name(format_id: &str) -> &'static str {
    match format_id {
        "stalker-soc" | "soc" => "Тень Чернобыля",
        "stalker-cs" | "clear_sky" => "Чистое Небо",
        "stalker-cop" | "cop" => "Зов Припяти",
        "stalker-soc-ee" => "Тень Чернобыля EE",
        "stalker-cs-ee" => "Чистое Небо EE",
        "stalker-cop-ee" => "Зов Припяти EE",
        "stalker2" | "s2" => "S.T.A.L.K.E.R. 2",
        _ => "неизвестный",
    }
}

/// The editor frame and its screens.
pub struct Shell {
    screens: Vec<Box<dyn Screen>>,
    hosts: Vec<Option<WidgetId>>,
    nav: Vec<WidgetId>,
    sidebar: WidgetId,
    nav_toggle: WidgetId,
    nav_brand: Vec<WidgetId>,
    nav_groups: Vec<WidgetId>,
    nav_version: WidgetId,
    nav_collapsed: bool,
    nav_user_choice: Option<bool>,
    content: WidgetId,
    scroll: ScrollView,
    scroll_bar: WidgetId,
    title: WidgetId,
    subtitle: WidgetId,
    breadcrumb: WidgetId,
    edition: WidgetId,
    draft_badge: WidgetId,
    save_reason: WidgetId,
    undo: WidgetId,
    redo: WidgetId,
    reset: WidgetId,
    refresh: WidgetId,
    save: WidgetId,
    library: WidgetId,
    library_refresh: WidgetId,
    library_previous: WidgetId,
    library_next: WidgetId,
    library_count: WidgetId,
    library_status: WidgetId,
    library_rows: Vec<(WidgetId, WidgetId, WidgetId)>,
    library_page: usize,
    library_workspace: super::saves::Workspace,
    reports_banner: WidgetId,
    reports_ok: WidgetId,
    reports_off: WidgetId,
    saving_dialog: WidgetId,
    tooltip: WidgetId,
    status: WidgetId,
    selected: usize,
    proxy: Option<Proxy<AppMessage>>,
    app: sse_app::state::AppState,
    wizard: super::wizard::Wizard,
    sounds: crate::sound::GameUiSounds,
    sound_game: Option<String>,
    sound_enabled: bool,
    sound_volume: f32,
}

fn padded(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges {
        left,
        top,
        right,
        bottom,
    }
}

fn top_button(tree: &mut Tree, parent: WidgetId, text: &str, primary: bool) -> Result<WidgetId> {
    let colors = crate::theme::current().colors;
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
            min: Size::new(58.0, 32.0),
            padding: padded(8.0, 0.0, 8.0, 0.0),
            shrink: 1.0,
            ..Style::default()
        },
        Content::Button {
            text: text.to_uppercase(),
            style: TextStyle::new(Face::Heading, 11.0),
        },
        Look {
            fill: primary.then(|| rgb(colors.accent[0])),
            hover_fill: Some(rgb(if primary {
                colors.accent[2]
            } else {
                colors.background[3]
            })),
            border: (!primary).then(|| (rgb(colors.borders[1]), 1.0)),
            radius: crate::theme::BUTTON_RADIUS,
            text: rgb(if primary { colors.accent[3] } else { colors.text[0] }),
            align: TextAlign::Center,
            ..Look::default()
        },
    )
}

fn compact_library_button(tree: &mut Tree, parent: WidgetId, text: &str) -> Result<WidgetId> {
    let colors = crate::theme::current().colors;
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
            min: Size::new(64.0, 30.0),
            padding: padded(5.0, 0.0, 5.0, 0.0),
            shrink: 1.0,
            ..Style::default()
        },
        Content::Button {
            text: text.to_uppercase(),
            style: TextStyle::new(Face::Heading, 10.0),
        },
        Look {
            fill: Some(rgb(colors.background[2])),
            hover_fill: Some(rgb(colors.background[3])),
            border: Some((rgb(colors.borders[1]), 1.0)),
            radius: crate::theme::BUTTON_RADIUS,
            text: rgb(colors.text[0]),
            align: TextAlign::Center,
            ..Look::default()
        },
    )
}

fn startup_language(settings: &sse_app::AppSettings) -> String {
    if let Ok(value) = std::env::var("STALKER_EDITOR_LANG") {
        if !value.trim().is_empty() {
            return value;
        }
    }
    if let Some(value) = settings.language.as_deref() {
        return value.to_owned();
    }
    let system = std::env::var("LC_ALL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|value| !value.is_empty()))
        .or_else(|| std::env::var("LANG").ok().filter(|value| !value.is_empty()));
    let Some(system) = system else { return "en".to_owned() };
    let normalized = system.split('.').next().unwrap_or(&system).replace('_', "-");
    let base = normalized.split('-').next().unwrap_or(&normalized);
    if crate::strings::LANGUAGES
        .iter()
        .any(|code| code.eq_ignore_ascii_case(&normalized) || code.eq_ignore_ascii_case(base))
    {
        normalized
    } else {
        "en".to_owned()
    }
}

impl Shell {
    /// Builds the frame into an empty tree and shows the first screen.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn build(tree: &mut Tree, proxy: Option<Proxy<AppMessage>>) -> Result<Self> {
        let settings = sse_app::AppSettings::load(&sse_app::default_settings_path());
        let language = startup_language(&settings);
        crate::strings::set_language(Some(&language));
        let root_style = Style {
            align_items: Align::Stretch,
            ..Style::default()
        };
        let root = tree.add(None, NodeKind::Stack, root_style, Content::Panel, Look::default())?;
        let frame = tree.add(
            Some(root),
            NodeKind::Row,
            Style {
                grow: 1.0,
                align_self: Some(Align::Stretch),
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;

        let sidebar_style = Style {
            preferred: Size::new(236.0, 0.0),
            min: Size::new(236.0, 0.0),
            shrink: 0.0,
            padding: padded(0.0, 18.0, 0.0, 12.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let sidebar_look = Look {
            fill: Some(rgb(style::BG_PANEL)),
            border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
            ..Look::default()
        };
        let sidebar = tree.add(
            Some(frame),
            NodeKind::Column,
            sidebar_style,
            Content::Panel,
            sidebar_look,
        )?;
        let brand = Style {
            padding: padded(20.0, 0.0, 20.0, 0.0),
            ..Style::default()
        };
        let brand_title = tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            brand,
            Content::Label {
                text: "S.T.A.L.K.E.R.".to_owned(),
                style: TextStyle::new(Face::Heading, 22.0),
            },
            Look {
                text: rgb(style::ACCENT),
                ..Look::default()
            },
        )?;
        let brand_subtitle = tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            Style {
                padding: padded(20.0, 0.0, 20.0, 6.0),
                ..Style::default()
            },
            Content::Label {
                text: crate::strings::t("РЕДАКТОР СОХРАНЕНИЙ").to_owned(),
                style: TextStyle::new(Face::Heading, 12.0),
            },
            Look {
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let nav_toggle = tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            Style {
                min: Size::new(0.0, 30.0),
                padding: padded(20.0, 0.0, 12.0, 0.0),
                ..Style::default()
            },
            Content::Button {
                text: crate::strings::t("☰  Свернуть меню").to_owned(),
                style: TextStyle::new(Face::Heading, 12.0),
            },
            style::nav(false),
        )?;

        let library_workspace =
            super::saves::Workspace::with_backup_directory(sse_app::paths::backup_directory(&settings));
        let screens = super::registry_with_save_workspace(library_workspace.clone());
        let mut nav = Vec::with_capacity(screens.len());
        let mut nav_groups = Vec::new();
        let nav_icons = [
            Icon::Saves,
            Icon::Inventory,
            Icon::Factions,
            Icon::Stash,
            Icon::MapTransitions,
            Icon::Backup,
            Icon::Compare,
            Icon::Timeline,
            Icon::Doctor,
            Icon::Games,
            Icon::Fixes,
            Icon::Doctor,
            Icon::Wrench,
            Icon::Companion,
            Icon::Trophy,
            Icon::Cloud,
            Icon::Book,
            Icon::ShieldCapabilities,
            Icon::Update,
            Icon::Settings,
        ];
        let mut group: Option<Group> = None;
        for screen in &screens {
            let id = screen.id();
            if group != Some(id.group()) {
                group = Some(id.group());
                nav_groups.push(tree.add(
                    Some(sidebar),
                    NodeKind::Leaf,
                    Style {
                        padding: padded(20.0, 12.0, 20.0, 4.0),
                        ..Style::default()
                    },
                    Content::Label {
                        text: id.group().caption().to_owned(),
                        style: TextStyle::new(Face::Heading, 11.0),
                    },
                    Look {
                        text: rgb(style::TEXT_MUTED),
                        ..Look::default()
                    },
                )?);
            }
            let item = Style {
                min: Size::new(0.0, 30.0),
                padding: padded(22.0, 0.0, 12.0, 0.0),
                ..Style::default()
            };
            let icon = nav_icons.get(nav.len()).copied().unwrap_or(Icon::Info);
            let content = Content::IconButton {
                icon,
                text: crate::strings::t(id.title()).to_owned(),
                style: TextStyle::new(Face::Heading, 14.0),
            };
            nav.push(tree.add(Some(sidebar), NodeKind::Leaf, item, content, style::nav(nav.is_empty()))?);
        }
        let nav_version = tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let version = Content::Label {
            text: format!("{} · Rust", env!("CARGO_PKG_VERSION")),
            style: Text::Note.style(),
        };
        tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            brand,
            version,
            Look {
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let main_style = Style {
            grow: 1.0,
            align_items: Align::Stretch,
            ..Style::default()
        };
        let main = tree.add(
            Some(frame),
            NodeKind::Column,
            main_style,
            Content::Panel,
            Look::default(),
        )?;
        let header_style = Style {
            min: Size::new(0.0, 118.0),
            padding: padded(24.0, 12.0, 24.0, 10.0),
            gap: Size::new(0.0, 2.0),
            align_items: Align::Stretch,
            shrink: 0.0,
            ..Style::default()
        };
        let header = tree.add(
            Some(main),
            NodeKind::Column,
            header_style,
            Content::Panel,
            Look::default(),
        )?;
        let top = tree.add(
            Some(header),
            NodeKind::Row,
            Style {
                gap: Size::new(6.0, 0.0),
                align_items: Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let brand = tree.add(
            Some(top),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(190.0, 0.0),
                ..Style::default()
            },
            Content::Label {
                text: "S.T.A.L.K.E.R. SAVE EDITOR".to_owned(),
                style: TextStyle::new(Face::Heading, 18.0),
            },
            Look {
                text: rgb(crate::theme::current().colors.text[0]),
                ..Look::default()
            },
        )?;
        let _ = brand;
        let edition = style::label(tree, top, "X-Ray / S2", Text::Value)?;
        tree.set_visible(edition, false)?;
        let draft_badge = style::label(tree, top, crate::strings::t("Черновик: 0 действ."), Text::Note)?;
        let undo = top_button(tree, top, crate::strings::t("Отменить"), false)?;
        let redo = top_button(tree, top, crate::strings::t("Вернуть"), false)?;
        let reset = top_button(tree, top, crate::strings::t("Сбросить"), false)?;
        let refresh = top_button(tree, top, crate::strings::t("Обновить"), false)?;
        let save = top_button(tree, top, crate::strings::t("СОХРАНИТЬ"), true)?;
        let save_reason = style::label(
            tree,
            header,
            crate::strings::t("Выберите сохранение для редактирования."),
            Text::Note,
        )?;
        let breadcrumb = style::label(tree, header, "", Text::Note)?;
        let title = style::label(tree, header, "", Text::Title)?;
        let subtitle = tree.add(
            Some(header),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: String::new(),
                style: TextStyle::new(Face::Body, 16.0),
            },
            Look {
                text: rgb(style::TEXT_SECONDARY),
                ..Look::default()
            },
        )?;
        let reports_banner = tree.add(
            Some(main),
            NodeKind::Row,
            Style {
                min: Size::new(0.0, 58.0),
                padding: padded(16.0, 6.0, 16.0, 6.0),
                shrink: 0.0,
                align_items: Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
                ..Look::default()
            },
        )?;
        tree.add(
            Some(reports_banner),
            NodeKind::Leaf,
            Style { grow: 1.0, shrink: 1.0, preferred: Size::new(420.0, 0.0), max: Size::new(560.0, f32::INFINITY), ..Style::default() },
            Content::Paragraph {
                text: "Редактор раз в сутки и после сбоя отправляет разработчику журнал работы, чтобы находить ошибки. Пути, имена и Steam ID из него вырезаются, сейвы не отправляются.".to_owned(),
                style: Text::Note.style(),
            },
            Look { text: rgb(style::TEXT_MUTED), ..Look::default() },
        )?;
        let reports_ok = style::button(tree, reports_banner, "Понятно", style::Button::Secondary)?;
        let reports_off = style::button(tree, reports_banner, "Не отправлять", style::Button::Secondary)?;
        tree.set_visible(reports_banner, !settings.reports_notice_shown)?;

        let viewport = tree.add(
            Some(main),
            NodeKind::Row,
            Style {
                grow: 1.0,
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let library = tree.add(
            Some(viewport),
            NodeKind::Column,
            Style {
                preferred: Size::new(256.0, 0.0),
                min: Size::new(256.0, 0.0),
                max: Size::new(256.0, f32::INFINITY),
                shrink: 0.0,
                padding: padded(12.0, 12.0, 12.0, 12.0),
                gap: Size::new(0.0, 8.0),
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
                ..Look::default()
            },
        )?;
        let library_heading = tree.add(
            Some(library),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, 4.0),
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for line in ["БИБЛИОТЕКА", "СОХРАНЕНИЙ"] {
            tree.add(
                Some(library_heading),
                NodeKind::Leaf,
                Style {
                    min: Size::new(0.0, 0.0),
                    shrink: 1.0,
                    ..Style::default()
                },
                Content::Label {
                    text: line.to_owned(),
                    style: TextStyle::new(Face::Heading, 13.0),
                },
                Look {
                    text: rgb(crate::theme::current().colors.text[0]),
                    ..Look::default()
                },
            )?;
        }
        let library_actions = tree.add(
            Some(library_heading),
            NodeKind::Row,
            Style {
                gap: Size::new(4.0, 0.0),
                align_items: Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let library_count = style::label(tree, library_actions, "0", Text::Note)?;
        let library_refresh = tree.add(
            Some(library_actions),
            NodeKind::Leaf,
            Style {
                min: Size::new(34.0, 30.0),
                padding: padded(5.0, 0.0, 5.0, 0.0),
                shrink: 0.0,
                ..Style::default()
            },
            Content::Button {
                text: "↻".to_owned(),
                style: TextStyle::new(Face::Heading, 14.0),
            },
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: rgb(crate::theme::current().colors.text[0]),
                align: TextAlign::Center,
                ..Look::default()
            },
        )?;
        let mut library_rows = Vec::with_capacity(SAVE_LIBRARY_PAGE_SIZE);
        let library_status = style::label(tree, library, "Ищу сейвы…", Text::Note)?;
        for _ in 0..SAVE_LIBRARY_PAGE_SIZE {
            let row = tree.add(
                Some(library),
                NodeKind::Column,
                Style {
                    gap: Size::new(0.0, 2.0),
                    align_items: Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let select = tree.add(
                Some(row),
                NodeKind::Leaf,
                Style {
                    min: Size::new(0.0, 30.0),
                    padding: padded(7.0, 0.0, 7.0, 0.0),
                    ..Style::default()
                },
                Content::Button {
                    text: "нет снимка".to_owned(),
                    style: TextStyle::new(Face::Body, 12.0),
                },
                Look {
                    fill: Some(rgb(crate::theme::current().colors.background[2])),
                    hover_fill: Some(rgb(crate::theme::current().colors.background[3])),
                    border: Some((rgb(crate::theme::current().colors.borders[0]), 1.0)),
                    radius: crate::theme::BUTTON_RADIUS,
                    text: rgb(crate::theme::current().colors.text[0]),
                    ..Look::default()
                },
            )?;
            let details = tree.add(
                Some(row),
                NodeKind::Leaf,
                Style {
                    padding: padded(7.0, 0.0, 4.0, 3.0),
                    ..Style::default()
                },
                Content::Label {
                    text: String::new(),
                    style: TextStyle::new(Face::Body, 11.0),
                },
                Look {
                    text: rgb(style::TEXT_MUTED),
                    ..Look::default()
                },
            )?;
            tree.set_visible(row, false)?;
            library_rows.push((row, select, details));
        }
        let library_pages = tree.add(
            Some(library),
            NodeKind::Row,
            Style {
                gap: Size::new(4.0, 0.0),
                align_items: Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let library_previous = compact_library_button(tree, library_pages, "Назад")?;
        let library_next = compact_library_button(tree, library_pages, "Дальше")?;
        tree.set_visible(library_previous, false)?;
        tree.set_visible(library_next, false)?;
        tree.set_clip_children(library, true)?;
        tree.set_visible(library, true)?;
        let content_style = Style {
            grow: 1.0,
            margin: padded(20.0, 0.0, 12.0, 20.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let content = tree.add(
            Some(viewport),
            NodeKind::Column,
            content_style,
            Content::Panel,
            Look::default(),
        )?;
        tree.set_clip_children(content, true)?;
        let scroll_bar = tree.add(
            Some(viewport),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(10.0, 0.0),
                min: Size::new(10.0, 0.0),
                margin: padded(0.0, 4.0, 10.0, 20.0),
                ..Style::default()
            },
            Content::Label {
                text: "▐".to_owned(),
                style: TextStyle::new(Face::Body, 10.0),
            },
            Look {
                text: rgb(crate::widgets::scroll::THUMB),
                ..Look::default()
            },
        )?;
        let wizard = super::wizard::Wizard::build(tree, content)?;
        let status = tree.add(
            Some(main),
            NodeKind::Leaf,
            Style {
                min: Size::new(0.0, 28.0),
                padding: padded(32.0, 0.0, 32.0, 0.0),
                shrink: 0.0,
                ..Style::default()
            },
            Content::Label {
                text: "Готово".to_owned(),
                style: Text::Note.style(),
            },
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let overlay_host = tree.add(
            Some(root),
            NodeKind::Stack,
            Style {
                align_items: Align::Center,
                align_self: Some(Align::Stretch),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        tree.set_overlay_host(overlay_host)?;
        let saving_dialog = style::card(tree, overlay_host)?;
        style::label(tree, saving_dialog, "СОХРАНЕНИЕ ФАЙЛА", Text::Heading)?;
        style::label(
            tree,
            saving_dialog,
            "Создаю резервную копию, записываю файл и проверяю его повторным чтением…",
            Text::Body,
        )?;
        tree.set_visible(saving_dialog, false)?;
        let tooltip = style::label(tree, overlay_host, "", Text::Body)?;
        tree.set_visible(tooltip, false)?;
        tree.set_tooltip(nav_toggle, crate::strings::t("Свернуть меню"))?;
        tree.set_tooltip(library_refresh, crate::strings::t("Обновить"))?;
        tree.set_tooltip(save, crate::strings::t("Выберите сохранение для редактирования."))?;

        let hosts = vec![None; screens.len()];
        let mut shell = Self {
            screens,
            hosts,
            nav,
            sidebar,
            nav_toggle,
            nav_brand: vec![brand_title, brand_subtitle],
            nav_groups,
            nav_version,
            nav_collapsed: false,
            nav_user_choice: settings.navigation_collapsed,
            content,
            scroll: ScrollView::new(),
            scroll_bar,
            title,
            subtitle,
            breadcrumb,
            edition,
            draft_badge,
            save_reason,
            undo,
            redo,
            reset,
            refresh,
            save,
            library,
            library_refresh,
            library_previous,
            library_next,
            library_count,
            library_status,
            library_rows,
            library_page: 0,
            library_workspace,
            reports_banner,
            reports_ok,
            reports_off,
            saving_dialog,
            tooltip,
            status,
            selected: 0,
            proxy,
            app: sse_app::state::AppState::new(),
            wizard,
            sounds: crate::sound::GameUiSounds::default(),
            sound_game: None,
            sound_enabled: settings.sound_enabled,
            sound_volume: (settings.sound_volume.min(100) as f32) / 100.0,
        };
        let initial_collapsed = settings.navigation_collapsed.unwrap_or(false);
        shell.apply_navigation(tree, initial_collapsed)?;
        for index in 0..shell.screens.len() {
            shell.prepare(tree, index)?;
        }
        shell.show(tree, 0)?;
        shell.render_library(tree)?;
        shell.sync_draft_controls(tree)?;
        Ok(shell)
    }

    fn sync_game_sounds(&mut self) {
        let Some(game) = self.app.selected_game().map(str::to_owned) else {
            return;
        };
        if self.sound_game.as_deref() == Some(game.as_str()) {
            return;
        }
        self.sound_game = Some(game.clone());
        let Some(directory) = self.app.game_dir().map(Path::to_path_buf) else {
            return;
        };
        let Some(proxy) = self.proxy.clone() else { return };
        std::thread::spawn(move || {
            let sounds = crate::sound::GameUiSounds::load(&game, &directory);
            let _ = proxy.send(AppMessage::SoundLoaded(game, Box::new(sounds)));
        });
    }

    fn play_sound(&self, cue: crate::sound::Cue) {
        if self.sound_enabled {
            self.sounds.play(cue, self.sound_volume);
        }
    }

    fn apply_navigation(&mut self, tree: &mut Tree, collapsed: bool) -> Result<()> {
        self.nav_collapsed = collapsed;
        let width = if collapsed { 58.0 } else { 236.0 };
        tree.set_style(
            self.sidebar,
            Style {
                preferred: Size::new(width, 0.0),
                min: Size::new(width, 0.0),
                shrink: 0.0,
                padding: padded(0.0, 18.0, 0.0, 12.0),
                align_items: Align::Stretch,
                ..Style::default()
            },
        )?;
        for id in &self.nav_brand {
            tree.set_visible(*id, !collapsed)?;
        }
        for id in &self.nav_groups {
            tree.set_visible(*id, !collapsed)?;
        }
        tree.set_visible(self.nav_version, !collapsed)?;
        tree.set_text(
            self.nav_toggle,
            if collapsed {
                "☰"
            } else {
                "☰  Свернуть меню"
            },
        )?;
        tree.set_tooltip(
            self.nav_toggle,
            crate::strings::t(if collapsed {
                "Развернуть меню"
            } else {
                "Свернуть меню"
            }),
        )?;
        for (index, id) in self.nav.iter().copied().enumerate() {
            let text = if collapsed {
                ""
            } else {
                self.screens
                    .get(index)
                    .map(|screen| crate::strings::t(screen.id().title()))
                    .unwrap_or("")
            };
            tree.set_text(id, text)?;
        }
        Ok(())
    }

    fn sync_navigation_width(&mut self, tree: &mut Tree, width: u32) -> Result<()> {
        let collapsed = width < 900 || self.nav_user_choice.unwrap_or(width < 1150);
        if collapsed != self.nav_collapsed {
            self.apply_navigation(tree, collapsed)?;
        }
        Ok(())
    }

    /// Shared application state.
    #[must_use]
    pub fn app(&self) -> &sse_app::state::AppState {
        &self.app
    }

    /// Installs the event proxy after a headless shell has been built.
    pub fn set_proxy(&mut self, proxy: Proxy<AppMessage>) {
        self.proxy = Some(proxy);
    }

    /// Currently shown screen.
    #[must_use]
    pub fn current(&self) -> Option<ScreenId> {
        self.screens.get(self.selected).map(|screen| screen.id())
    }

    /// Switches to a screen by id.
    ///
    /// # Errors
    /// Returns an error from the widget tree or the screen.
    pub fn open(&mut self, tree: &mut Tree, id: ScreenId) -> Result<()> {
        if self.library_workspace.is_saving() {
            return Ok(());
        }
        match self.screens.iter().position(|screen| screen.id() == id) {
            Some(index) => self.select(tree, index),
            None => Ok(()),
        }
    }

    /// Opens a save through the selected screen's background-aware save hook.
    ///
    /// # Errors
    /// Returns an error if the screen cannot read or parse the save.
    pub fn open_save(&mut self, tree: &mut Tree, path: &Path) -> Result<bool> {
        if self.library_workspace.is_saving() {
            return Ok(false);
        }
        self.open(tree, ScreenId::Overview)?;
        let Some(index) = self.screens.iter().position(|screen| screen.id() == ScreenId::Overview) else {
            return Ok(false);
        };
        let mut cx = Context {
            tree,
            proxy: self.proxy.as_ref(),
            status: None,
            app: &mut self.app,
        };
        let opened = self
            .screens
            .get_mut(index)
            .map(|screen| screen.open_save(&mut cx, path))
            .transpose()?
            .unwrap_or(false);
        if let Some(status) = cx.status {
            let status = crate::status::localize_writer_status(&status);
            cx.tree.set_text(self.status, &status)?;
        }
        Ok(opened)
    }

    fn select(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        if self.library_workspace.is_saving() {
            return Ok(());
        }
        if index == self.selected || index >= self.screens.len() {
            return Ok(());
        }
        if tree.dialog_open() {
            let _ = tree.close_dialog()?;
        }
        if let Some(old) = self.nav.get(self.selected) {
            tree.set_look(*old, style::nav(false))?;
        }
        if let Some(host) = self.hosts.get(self.selected).copied().flatten() {
            tree.set_visible(host, false)?;
        }
        if let Some(new) = self.nav.get(index) {
            tree.set_look(*new, style::nav(true))?;
        }
        self.selected = index;
        self.play_sound(crate::sound::Cue::Switch);
        tree.set_visible(
            self.library,
            self.screens
                .get(index)
                .is_some_and(|screen| screen.id().group() == Group::Saves),
        )?;
        self.scroll.scroll_to(0.0);
        tree.set_scroll_y(self.content, 0)?;
        self.show(tree, index)
    }

    fn prepare(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        let (Some(screen), Some(slot)) = (self.screens.get_mut(index), self.hosts.get_mut(index)) else {
            return Ok(());
        };
        if slot.is_some() {
            return Ok(());
        }
        let host_style = Style {
            grow: 1.0,
            shrink: 0.0,
            gap: Size::new(0.0, 16.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let host = tree.add(
            Some(self.content),
            NodeKind::Column,
            host_style,
            Content::Panel,
            Look::default(),
        )?;
        *slot = Some(host);
        let mut cx = Context {
            tree,
            proxy: self.proxy.as_ref(),
            status: None,
            app: &mut self.app,
        };
        screen.build(&mut cx, host)?;
        cx.tree.set_visible(host, false)?;
        Ok(())
    }

    fn show(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        self.prepare(tree, index)?;
        let (Some(screen), Some(slot)) = (self.screens.get_mut(index), self.hosts.get_mut(index)) else {
            return Ok(());
        };
        tree.set_text(self.title, crate::strings::t(screen.id().title()))?;
        tree.set_text(self.subtitle, crate::strings::t(screen.subtitle()))?;
        tree.set_text(
            self.breadcrumb,
            &format!(
                "{} / {}",
                crate::strings::t(screen.id().group().caption()),
                crate::strings::t(screen.id().title())
            ),
        )?;
        tree.set_text(self.edition, self.app.selected_game().unwrap_or("X-Ray / S2"))?;
        let mut cx = Context {
            tree,
            proxy: self.proxy.as_ref(),
            status: None,
            app: &mut self.app,
        };
        if let Some(host) = *slot {
            cx.tree.set_visible(host, true)?;
        }
        screen.shown(&mut cx)?;
        let screen_id = screen.id();
        let screen_host = *slot;
        let status = cx.status.take();
        drop(cx);
        self.wizard.sync(tree, &self.app, screen_id, screen_host)?;
        if let Some(text) = status {
            let text = crate::status::localize_writer_status(&text);
            tree.set_text(self.status, &text)?;
        }
        if !tree.dialog_open() {
            if let Some(host) = screen_host {
                if let Some(focus) = tree.first_focusable_in(host) {
                    tree.set_focus(Some(focus))?;
                }
            }
        }
        Ok(())
    }

    fn route(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<()> {
        let mut status = None;
        for (index, screen) in self.screens.iter_mut().enumerate() {
            if self.hosts.get(index).copied().flatten().is_none() {
                continue;
            }
            let wanted = match message {
                Message::User(AppMessage::Tick(_)) => true,
                Message::User(AppMessage::ToScreen(id, _)) => *id == screen.id(),
                Message::User(AppMessage::EditorAction(_)) => screen.id() == ScreenId::Inventory,
                Message::User(AppMessage::SoundLoaded(_, _)) => false,
                Message::Window(_) => index == self.selected,
            };
            if wanted {
                let mut cx = Context {
                    tree,
                    proxy: self.proxy.as_ref(),
                    status: None,
                    app: &mut self.app,
                };
                screen.message(&mut cx, message, clicked)?;
                status = cx.status.or(status);
            }
        }
        if let Some(text) = status {
            let text = crate::status::localize_writer_status(&text);
            tree.set_text(self.status, &text)?;
        }
        Ok(())
    }

    fn sync_draft_controls(&self, tree: &mut Tree) -> Result<()> {
        tree.set_text(self.edition, self.app.selected_game().unwrap_or("X-Ray / S2"))?;
        let Some(source_sha256) = self.app.current_save_sha256() else {
            tree.set_text(self.draft_badge, crate::strings::t("Черновик: 0 действ."))?;
            tree.set_text(
                self.save_reason,
                crate::strings::t("Выберите сохранение для редактирования."),
            )?;
            tree.set_enabled(self.undo, false)?;
            tree.set_enabled(self.redo, false)?;
            tree.set_enabled(self.reset, false)?;
            tree.set_enabled(self.save, false)?;
            return Ok(());
        };
        let plan = self.app.draft(source_sha256);
        let journal = self.app.draft_journal(source_sha256);
        let invalid_numbers = self.app.has_invalid_numeric_input();
        let eligibility = save_eligibility(
            true,
            self.app.current_save_format(),
            self.app.current_save_is_legacy(),
            plan,
            invalid_numbers,
        );
        let has_changes = eligibility.change_count > 0 || invalid_numbers;
        let draft_badge = format!("Черновик: {} действ.", eligibility.change_count);
        tree.set_text(self.draft_badge, &draft_badge)?;
        tree.set_text(self.save_reason, &eligibility.reason)?;
        tree.set_tooltip(self.save, crate::strings::t(&eligibility.reason))?;
        tree.set_enabled(
            self.undo,
            journal.is_some_and(sse_storage::drafts::DraftJournal::can_undo),
        )?;
        tree.set_enabled(
            self.redo,
            journal.is_some_and(sse_storage::drafts::DraftJournal::can_redo),
        )?;
        tree.set_enabled(self.reset, has_changes)?;
        tree.set_enabled(self.save, eligibility.can_save && !self.library_workspace.is_saving())?;
        Ok(())
    }

    fn dispatch_editor_action(&mut self, tree: &mut Tree, action: EditorAction) -> Result<()> {
        if action == EditorAction::Save && sse_app::tasks::named_task_active("save-restore") {
            tree.set_text(
                self.status,
                "Сохранение недоступно: дождитесь завершения восстановления сейва.",
            )?;
            return Ok(());
        }
        if let Some(index) = self
            .screens
            .iter()
            .position(|screen| screen.id() == ScreenId::Inventory)
        {
            if self.selected != index {
                self.select(tree, index)?;
            } else {
                self.show(tree, index)?;
            }
        }
        self.route(tree, &Message::User(AppMessage::EditorAction(action)), None)?;
        self.sync_draft_controls(tree)?;
        self.sync_saving_overlay(tree)
    }

    fn sync_saving_overlay(&self, tree: &mut Tree) -> Result<()> {
        if self.library_workspace.is_saving() {
            if !tree.dialog_open() {
                tree.open_dialog(self.saving_dialog)?;
            }
        } else if tree.dialog() == Some(self.saving_dialog) {
            let _ = tree.close_dialog()?;
        }
        Ok(())
    }

    fn handle(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<Flow> {
        match message {
            Message::Window(WindowEvent::CloseRequested) if self.library_workspace.is_saving() => {
                tree.set_text(self.status, "Дождитесь завершения сохранения, чтобы закрыть окно.")?;
                self.sync_saving_overlay(tree)?;
                return Ok(Flow::Continue);
            }
            Message::Window(WindowEvent::CloseRequested) if sse_app::tasks::named_task_active("save-restore") => {
                tree.set_text(self.status, "Дождитесь завершения восстановления, чтобы закрыть окно.")?;
                return Ok(Flow::Continue);
            }
            Message::Window(WindowEvent::CloseRequested)
                if sse_app::tasks::named_task_active("game-background")
                    || sse_app::tasks::named_task_active("companion-background") =>
            {
                tree.set_text(
                    self.status,
                    "Дождитесь завершения фоновой операции с игрой, чтобы закрыть окно.",
                )?;
                return Ok(Flow::Continue);
            }
            Message::Window(WindowEvent::CloseRequested) => {
                let _ = sse_app::tasks::wait_for_named_tasks(
                    &["draft-save", "draft-reset"],
                    std::time::Duration::from_secs(2),
                );
                return Ok(Flow::Exit);
            }
            Message::Window(WindowEvent::Disconnected) => {
                return Ok(Flow::Exit);
            }
            _ => {}
        }
        if let Message::User(AppMessage::SoundLoaded(game, sounds)) = message {
            if self.sound_game.as_deref() == Some(game.as_str()) {
                self.sounds = (**sounds).clone();
            }
            return Ok(Flow::Continue);
        }
        self.sync_game_sounds();
        if matches!(message, Message::User(AppMessage::Tick(_))) {
            let _ = tree.tick_tooltip();
            if let Some(text) = tree.active_tooltip().map(str::to_owned) {
                tree.set_text(self.tooltip, &text)?;
                tree.set_visible(self.tooltip, true)?;
            } else {
                tree.set_visible(self.tooltip, false)?;
            }
        }
        if let Message::Window(WindowEvent::Resized { width, .. }) = message {
            self.sync_navigation_width(tree, *width)?;
            let sidebar_width = if self.nav_collapsed { 58 } else { 236 };
            let panel_width = width.saturating_sub(sidebar_width);
            tree.set_visible(self.edition, panel_width >= 1000)?;
            let middle_width = width.saturating_sub(232);
            let library_width = if middle_width < 1100 {
                220.0
            } else if middle_width >= 1900 {
                300.0
            } else {
                280.0
            };
            tree.set_style(
                self.library,
                Style {
                    preferred: Size::new(library_width - 24.0, 0.0),
                    min: Size::new(library_width - 24.0, 0.0),
                    max: Size::new(library_width - 24.0, f32::INFINITY),
                    shrink: 0.0,
                    padding: padded(12.0, 12.0, 12.0, 12.0),
                    gap: Size::new(0.0, 8.0),
                    align_items: Align::Stretch,
                    ..Style::default()
                },
            )?;
        }
        if self.library_workspace.is_saving()
            && !matches!(
                message,
                Message::User(AppMessage::ToScreen(_, _)) | Message::User(AppMessage::Tick(_))
            )
        {
            self.sync_saving_overlay(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked == Some(self.nav_toggle) {
            let wanted = !self.nav_collapsed;
            self.apply_navigation(tree, wanted)?;
            self.nav_user_choice = Some(wanted);
            std::thread::spawn(move || {
                let path = sse_app::default_settings_path();
                let mut settings = sse_app::AppSettings::load(&path);
                settings.navigation_collapsed = Some(wanted);
                let _ = settings.save(&path);
            });
            tree.set_text(self.status, "Состояние меню сохранено.")?;
            return Ok(Flow::Continue);
        }
        if let Message::Window(WindowEvent::Wheel { delta }) = message {
            let viewport = tree.rect(self.content)?;
            let content_height = tree.content_height(self.content)?;
            self.scroll.set_extent(content_height, viewport.height as f32);
            if self.scroll.wheel(-(*delta as f32)) {
                tree.set_scroll_y(self.content, to_px(self.scroll.offset_y().round()))?;
            }
            tree.set_visible(self.scroll_bar, self.scroll.thumb().is_some())?;
            return Ok(Flow::Continue);
        }
        let mut wizard_status = None;
        if let Some(target) = self.wizard.message(tree, message, clicked, &mut wizard_status)? {
            self.open(tree, target)?;
            return Ok(Flow::Continue);
        }
        if let Some(text) = wizard_status {
            let text = crate::status::localize_writer_status(&text);
            tree.set_text(self.status, &text)?;
        }
        if clicked.is_some() && clicked == Some(self.reports_ok) {
            let _ = sse_app::settings_writer::submit(sse_app::settings_writer::SettingsPatch::ReportsNotice {
                send_reports: None,
            });
            tree.set_visible(self.reports_banner, false)?;
            tree.set_text(self.status, "Настройки отчётов сохранены.")?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.reports_off) {
            let _ = sse_app::settings_writer::submit(sse_app::settings_writer::SettingsPatch::ReportsNotice {
                send_reports: Some(false),
            });
            tree.set_visible(self.reports_banner, false)?;
            tree.set_text(self.status, "Отправка отчётов отключена.")?;
            return Ok(Flow::Continue);
        }
        let editor_action = if clicked.is_some() && clicked == Some(self.undo) {
            Some(EditorAction::Undo)
        } else if clicked.is_some() && clicked == Some(self.redo) {
            Some(EditorAction::Redo)
        } else if clicked.is_some() && clicked == Some(self.reset) {
            Some(EditorAction::Reset)
        } else if clicked.is_some() && clicked == Some(self.save) {
            Some(EditorAction::Save)
        } else {
            None
        };
        if let Some(action) = editor_action {
            self.dispatch_editor_action(tree, action)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.refresh) {
            let status = {
                let mut cx = Context {
                    tree: &mut *tree,
                    proxy: self.proxy.as_ref(),
                    status: None,
                    app: &mut self.app,
                };
                self.library_workspace.refresh_library(&mut cx);
                cx.status
            };
            if let Some(status) = status {
                tree.set_text(self.status, &status)?;
            }
            self.render_library(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.library_refresh) {
            let status = {
                let mut cx = Context {
                    tree: &mut *tree,
                    proxy: self.proxy.as_ref(),
                    status: None,
                    app: &mut self.app,
                };
                self.library_workspace.refresh_library(&mut cx);
                cx.status
            };
            if let Some(status) = status {
                tree.set_text(self.status, &status)?;
            }
            self.render_library(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.library_previous) {
            self.library_page = self.library_page.saturating_sub(1);
            self.render_library(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.library_next) {
            self.library_page = self.library_page.saturating_add(1);
            self.render_library(tree)?;
            return Ok(Flow::Continue);
        }
        if let Some(offset) = clicked.and_then(|id| self.library_rows.iter().position(|(_, select, _)| *select == id)) {
            let index = self
                .library_page
                .saturating_mul(SAVE_LIBRARY_PAGE_SIZE)
                .saturating_add(offset);
            let (_, _, slots) = self.library_workspace.library_snapshot();
            if let Some(slot) = slots.get(index) {
                let status = {
                    let mut cx = Context {
                        tree: &mut *tree,
                        proxy: self.proxy.as_ref(),
                        status: None,
                        app: &mut self.app,
                    };
                    self.library_workspace.select_library_path(&slot.path, &mut cx);
                    cx.status
                };
                if let Some(status) = status {
                    tree.set_text(self.status, &status)?;
                }
                return Ok(Flow::Continue);
            }
        }
        if let Some(index) = clicked.and_then(|id| self.nav.iter().position(|nav| *nav == id)) {
            if let Some(nav) = self.nav.get(index).copied() {
                tree.set_focus(Some(nav))?;
            }
            self.select(tree, index)?;
            return Ok(Flow::Continue);
        }
        if let Some(clicked) = clicked {
            tree.set_focus(Some(clicked))?;
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            ctrl,
            shift,
            ..
        }) = message
        {
            if *keysym == KEY_ESCAPE {
                self.route(tree, message, None)?;
                if tree.dialog_open() {
                    let _ = tree.close_dialog()?;
                } else {
                    tree.set_focus(None)?;
                }
                return Ok(Flow::Continue);
            }
            if *keysym == KEY_TAB {
                let _ = tree.focus_next(*shift);
                self.route(tree, message, None)?;
                return Ok(Flow::Continue);
            }
            if *keysym == KEY_RETURN {
                if let Some(focus) = tree.focused() {
                    let action = if focus == self.undo {
                        Some(EditorAction::Undo)
                    } else if focus == self.redo {
                        Some(EditorAction::Redo)
                    } else if focus == self.reset {
                        Some(EditorAction::Reset)
                    } else if focus == self.save {
                        Some(EditorAction::Save)
                    } else {
                        None
                    };
                    if let Some(action) = action {
                        self.dispatch_editor_action(tree, action)?;
                        return Ok(Flow::Continue);
                    }
                }
                if let Some(index) = tree
                    .focused()
                    .and_then(|focus| self.nav.iter().position(|nav| *nav == focus))
                {
                    self.select(tree, index)?;
                    return Ok(Flow::Continue);
                }
                let target = tree.focused().or_else(|| {
                    self.hosts
                        .get(self.selected)
                        .copied()
                        .flatten()
                        .and_then(|host| tree.first_focusable_in(host))
                });
                if let Some(target) = target {
                    tree.set_focus(Some(target))?;
                }
                self.route(tree, message, target)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if !self.wizard.is_showing(tree) {
                    if let Some(index) = self
                        .screens
                        .iter()
                        .position(|screen| screen.id() == ScreenId::Inventory)
                    {
                        self.select(tree, index)?;
                    }
                }
                self.route(tree, message, None)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x53 | 0x73) {
                self.dispatch_editor_action(tree, EditorAction::Save)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x5a | 0x7a) {
                let action = if *shift { EditorAction::Redo } else { EditorAction::Undo };
                self.dispatch_editor_action(tree, action)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x59 | 0x79) {
                self.dispatch_editor_action(tree, EditorAction::Redo)?;
                return Ok(Flow::Continue);
            }
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            ctrl: false,
            ..
        }) = message
        {
            if !tree.dialog_open() && !tree.focused_is_input() {
                let count = self.nav.len();
                match *keysym {
                    KEY_UP => self.select(tree, self.selected.checked_sub(1).unwrap_or(count.saturating_sub(1)))?,
                    KEY_DOWN => {
                        let next = self.selected.saturating_add(1);
                        self.select(tree, if next >= count { 0 } else { next })?;
                    }
                    _ => {}
                }
            }
        }
        self.route(tree, message, clicked)?;
        if let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message {
            if payload.is::<()>() {
                self.library_page = 0;
                if self.app.current_save().is_none() {
                    let (_, _, slots) = self.library_workspace.library_snapshot();
                    if let Some(slot) = slots.first() {
                        let status = {
                            let mut cx = Context {
                                tree: &mut *tree,
                                proxy: self.proxy.as_ref(),
                                status: None,
                                app: &mut self.app,
                            };
                            self.library_workspace.select_library_path(&slot.path, &mut cx);
                            cx.status
                        };
                        if let Some(status) = status {
                            tree.set_text(self.status, &status)?;
                        }
                    }
                }
            }
            self.show(tree, self.selected)?;
            self.render_library(tree)?;
        }
        self.sync_draft_controls(tree)?;
        self.sync_saving_overlay(tree)?;
        if let Some(screen) = self.screens.get(self.selected) {
            let host = self.hosts.get(self.selected).copied().flatten();
            self.wizard.sync(tree, &self.app, screen.id(), host)?;
        }
        Ok(Flow::Continue)
    }

    fn render_library(&mut self, tree: &mut Tree) -> Result<()> {
        let (scanning, error, slots) = self.library_workspace.library_snapshot();
        let pages = slots.len().saturating_add(SAVE_LIBRARY_PAGE_SIZE - 1) / SAVE_LIBRARY_PAGE_SIZE;
        self.library_page = self.library_page.min(pages.saturating_sub(1));
        let text = if scanning {
            format!("{} · …", slots.len())
        } else if let Some(error) = error.as_deref() {
            format!(
                "{} · ошибка: {}",
                slots.len(),
                super::saves::short_text(&crate::status::localize_writer_status(error), 24)
            )
        } else {
            slots.len().to_string()
        };
        tree.set_text(self.library_count, &text)?;
        let start = self.library_page.saturating_mul(SAVE_LIBRARY_PAGE_SIZE);
        let selected_path = self.app.current_save();
        for (offset, (row, select, details)) in self.library_rows.iter().enumerate() {
            if let Some(slot) = slots.get(start.saturating_add(offset)) {
                let filename = slot
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| "без имени".into());
                let game =
                    super::saves::format_display_name(slot.format_id.as_deref().unwrap_or(&slot.candidate_release_id));
                let displayed_filename = super::saves::short_text(&filename, 24);
                tree.set_text(*select, &format!("нет снимка · {displayed_filename}"))?;
                tree.set_enabled(*select, slot.detection_error.is_none())?;
                tree.set_text(
                    *details,
                    &format!(
                        "{} · {} · {}",
                        game,
                        super::saves::display_file_time(slot.last_write_time_utc, true, false),
                        super::saves::display_size(slot.size)
                    ),
                )?;
                let is_selected = selected_path.is_some_and(|path| path == slot.path);
                tree.set_look(
                    *select,
                    Look {
                        fill: Some(rgb(if is_selected {
                            crate::theme::current().colors.accent[0]
                        } else {
                            crate::theme::current().colors.background[2]
                        })),
                        hover_fill: Some(rgb(crate::theme::current().colors.background[3])),
                        border: Some((rgb(crate::theme::current().colors.borders[0]), 1.0)),
                        radius: crate::theme::BUTTON_RADIUS,
                        text: rgb(crate::theme::current().colors.text[0]),
                        ..Look::default()
                    },
                )?;
                tree.set_visible(*row, true)?;
            } else {
                tree.set_visible(*row, false)?;
            }
        }
        let status = if scanning {
            "Поиск сейвов…".to_owned()
        } else if let Some(error) = error.as_deref() {
            super::saves::short_text(&crate::status::localize_writer_status(error), 20)
        } else if slots.is_empty() {
            "Сейвы не найдены".to_owned()
        } else {
            String::new()
        };
        tree.set_text(self.library_status, &status)?;
        tree.set_visible(self.library_status, scanning || error.is_some() || slots.is_empty())?;
        tree.set_visible(self.library_previous, pages > 1 && self.library_page > 0)?;
        tree.set_visible(
            self.library_next,
            pages > 1 && self.library_page.saturating_add(1) < pages,
        )?;
        Ok(())
    }
}

impl App<AppMessage> for Shell {
    fn message(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Flow {
        match self.handle(tree, message, clicked) {
            Ok(flow) => flow,
            Err(error) => {
                let text = crate::status::localize_writer_status(&format!("Ошибка: {error}"));
                let _ = tree.set_text(self.status, &text);
                Flow::Continue
            }
        }
    }

    fn close_requested(&mut self, tree: &mut Tree, message: &Message<AppMessage>) -> Flow {
        self.message(tree, message, None)
    }
}

#[cfg(test)]
mod tests {
    use super::{save_eligibility, ScreenId, Shell};
    use crate::event_loop::{channel_pair, Flow, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::raster::Color;
    use crate::widget::Tree;
    use sse_storage::drafts::{DraftPlacement, DraftPlan, JsonValue};

    fn close_task_test_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| std::sync::Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn save_disabled_reason_matches_reference_priority_and_capabilities() -> sse_core::Result<()> {
        let source_sha256 = "a".repeat(64);
        assert_eq!(
            save_eligibility(false, None, false, None, false).reason,
            crate::strings::t("Выберите сохранение для редактирования.")
        );
        assert_eq!(
            save_eligibility(true, None, false, None, true).reason,
            "Нет несохранённых изменений."
        );

        let mut unsupported = DraftPlan::empty(&source_sha256)?;
        unsupported.placements.insert(5, DraftPlacement::Ruck);
        assert_eq!(
            save_eligibility(true, Some("stalker2"), false, Some(&unsupported), false).reason,
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );

        let mut supported = DraftPlan::empty(&source_sha256)?;
        supported.money = Some(700);
        let invalid = save_eligibility(true, Some("stalker2"), false, Some(&supported), true);
        assert_eq!(
            invalid.reason,
            "Введены некорректные значения (проверьте введённые числа)."
        );
        assert!(!invalid.can_save);

        supported.unmapped_legacy_plan = Some(JsonValue::Null);
        assert_eq!(
            save_eligibility(true, Some("stalker2"), false, Some(&supported), true).reason,
            "В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом)."
        );
        Ok(())
    }

    #[test]
    fn unverified_s2_stash_move_is_counted_but_not_saveable() -> sse_core::Result<()> {
        let source_sha256 = "c".repeat(64);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.s2_stash_takes.push(0x3000_0010);

        let eligibility = save_eligibility(true, Some("stalker2"), false, Some(&plan), false);

        assert!(!eligibility.can_save);
        assert_eq!(eligibility.change_count, 1);
        assert_eq!(
            eligibility.reason,
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        Ok(())
    }

    #[test]
    fn saving_uses_a_modal_overlay_and_closes_it_after_completion() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let shell = Shell::build(&mut tree, None)?;

        let request = shell
            .library_workspace
            .begin_saving()
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        shell.sync_saving_overlay(&mut tree)?;
        assert!(tree.dialog_open());
        assert_eq!(tree.dialog(), Some(shell.saving_dialog));

        assert!(shell.library_workspace.finish_saving(request));
        shell.sync_saving_overlay(&mut tree)?;
        assert!(!tree.dialog_open());
        Ok(())
    }

    #[test]
    fn close_request_waits_for_an_active_restore_and_exits_after_completion() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let close = Message::Window(WindowEvent::CloseRequested);
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let tasks = sse_app::TaskManager::new();
        let _restore = tasks.spawn("save-restore", move |_context| {
            release_rx.recv().map_err(|error| error.to_string())?;
            Ok(())
        });

        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        release_tx
            .send(())
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        for _ in 0..100 {
            if !sse_app::tasks::named_task_active("save-restore") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!sse_app::tasks::named_task_active("save-restore"));
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn close_request_waits_for_an_active_save_and_exits_after_completion() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let close = Message::Window(WindowEvent::CloseRequested);

        let request = shell
            .library_workspace
            .begin_saving()
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        assert!(shell.library_workspace.finish_saving(request));
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn screen_and_save_selection_cannot_change_during_a_write() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let current = shell.current();
        let request = shell
            .library_workspace
            .begin_saving()
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;

        shell.open(&mut tree, ScreenId::Inventory)?;
        assert_eq!(shell.current(), current);
        assert!(!shell.open_save(&mut tree, std::path::Path::new("other-save.sav"))?);
        assert_eq!(shell.current(), current);

        assert!(shell.library_workspace.finish_saving(request));
        Ok(())
    }

    #[test]
    fn save_library_is_visible_on_save_screens_and_hidden_elsewhere() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;

        assert!(tree.is_visible(shell.library));
        shell.open(&mut tree, ScreenId::Settings)?;
        assert!(!tree.is_visible(shell.library));
        shell.open(&mut tree, ScreenId::Inventory)?;
        assert!(tree.is_visible(shell.library));
        Ok(())
    }

    #[test]
    fn save_library_uses_reference_width_breakpoints() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        for (width, expected) in [(940, 220), (1500, 280), (2200, 300)] {
            tree.resize(width, 700);
            let message = Message::Window(WindowEvent::Resized { width, height: 700 });
            shell.handle(&mut tree, &message, None)?;
            let mut frame = vec![0_u32; usize::try_from(width).unwrap_or_default() * 700];
            tree.paint(&mut frame, usize::try_from(width).unwrap_or_default())?;
            assert_eq!(tree.rect(shell.library)?.width, expected, "window width {width}");
        }
        Ok(())
    }

    #[test]
    fn extended_inventory_edits_are_counted_in_the_draft_badge() -> sse_core::Result<()> {
        let source_sha256 = "b".repeat(64);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.money = Some(1);
        plan.durability.insert(1, 75);
        plan.placements.insert(2, DraftPlacement::Ruck);
        plan.upgrades.insert(3, vec!["upgrade".to_owned()]);

        let eligibility = save_eligibility(true, Some("stalker-cop"), false, Some(&plan), false);

        assert_eq!(eligibility.change_count, 4);
        assert!(eligibility.can_save);
        Ok(())
    }

    #[test]
    fn unverified_s2_stash_transfers_are_counted_but_never_saveable() -> sse_core::Result<()> {
        let source_sha256 = "c".repeat(64);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.s2_stash_takes.extend([0x1234_5678, 0x8765_4321]);

        let multiple = save_eligibility(true, Some("stalker2"), false, Some(&plan), false);

        assert_eq!(multiple.change_count, 2);
        assert!(!multiple.can_save);
        assert_eq!(
            multiple.reason,
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        plan.s2_stash_takes.pop();
        let eligibility = save_eligibility(true, Some("stalker2"), false, Some(&plan), false);
        assert_eq!(eligibility.change_count, 1);
        assert!(!eligibility.can_save);
        assert_eq!(
            eligibility.reason,
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        assert!(!save_eligibility(true, Some("stalker2"), true, Some(&plan), false).can_save);
        Ok(())
    }

    #[test]
    fn escape_does_not_close_the_application() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff1b,
            text: None,
            ctrl: false,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        Ok(())
    }

    #[test]
    fn tab_moves_visible_keyboard_focus_and_repaints_its_ring() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        tree.resize(940, 700);
        let mut frame = vec![0_u32; 940 * 700];
        tree.paint(&mut frame, 940)?;
        let before = frame.clone();
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: false,
        });
        shell.handle(&mut tree, &message, None)?;
        tree.paint(&mut frame, 940)?;
        assert!(
            frame.iter().zip(&before).any(|(after, before)| after != before),
            "Tab must show a keyboard focus ring"
        );
        Ok(())
    }

    #[test]
    fn ctrl_s_is_refused_while_restore_is_active() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let initial = shell.current();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let tasks = sse_app::TaskManager::new();
        let _restore = tasks.spawn("save-restore", move |_context| {
            release_rx.recv().map_err(|error| error.to_string())?;
            Ok(())
        });
        let save = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('s'),
            text: None,
            ctrl: true,
            shift: false,
        });

        assert!(matches!(shell.handle(&mut tree, &save, None)?, Flow::Continue));
        assert_eq!(shell.current(), initial);

        release_tx
            .send(())
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        for _ in 0..100 {
            if !sse_app::tasks::named_task_active("save-restore") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        Ok(())
    }

    #[test]
    fn ctrl_s_opens_inventory_and_keeps_the_shell_running() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('s'),
            text: None,
            ctrl: true,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        assert_eq!(shell.current(), Some(super::super::ScreenId::Inventory));
        Ok(())
    }

    #[test]
    fn ctrl_shift_z_redoes_instead_of_undoing() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let source_sha256 = "a".repeat(64);
        let mut changed = sse_storage::drafts::DraftPlan::empty(&source_sha256)?;
        changed.money = Some(5);
        shell
            .app
            .set_current_save_identity(std::path::PathBuf::from("fixture.sav"), source_sha256.clone());
        shell.app.set_draft_journal(sse_storage::drafts::DraftJournal::new(
            vec![sse_storage::drafts::DraftPlan::empty(&source_sha256)?, changed],
            1,
        )?);
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('z'),
            text: None,
            ctrl: true,
            shift: true,
        });

        shell.handle(&mut tree, &message, None)?;

        assert_eq!(shell.app.draft(&source_sha256).and_then(|plan| plan.money), Some(5));
        assert!(!shell.app.can_redo_draft(&source_sha256));
        Ok(())
    }

    #[test]
    fn ctrl_f_does_not_focus_search_hidden_by_the_first_run_wizard() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let search = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('f'),
            text: None,
            ctrl: true,
            shift: false,
        });
        shell.handle(&mut tree, &search, None)?;
        assert_eq!(tree.focused(), None);
        Ok(())
    }

    #[test]
    fn ctrl_f_opens_inventory_search_after_a_save_is_loaded() -> sse_core::Result<()> {
        let fixture = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let path = std::env::temp_dir().join(format!("sse-shell-ctrl-f-{}.sav", std::process::id()));
        std::fs::write(&path, fixture)?;
        let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, Some(proxy))?;
        assert!(shell.open_save(&mut tree, &path)?);

        while shell.app.current_save() != Some(path.as_path()) {
            let loaded = receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|error| sse_core::Error::System(error.to_string()))?;
            shell.handle(&mut tree, &loaded, None)?;
        }

        let search = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('f'),
            text: None,
            ctrl: true,
            shift: false,
        });
        shell.handle(&mut tree, &search, None)?;

        assert_eq!(shell.current(), Some(ScreenId::Inventory));
        let focused = tree
            .focused()
            .ok_or_else(|| sse_core::Error::damaged("Ctrl+F did not focus inventory search"))?;
        assert!(tree.input_text(focused).is_ok(), "Ctrl+F must focus a text input");
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn escape_closes_a_widget_dialog_without_exiting() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let overlay = tree
            .overlay_host()
            .ok_or_else(|| sse_core::Error::damaged("missing dialog overlay"))?;
        let dialog = tree.add(
            Some(overlay),
            crate::layout::NodeKind::Column,
            crate::layout::Style::default(),
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        tree.open_dialog(dialog)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff1b,
            text: None,
            ctrl: false,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        assert!(!tree.dialog_open());
        Ok(())
    }

    #[test]
    fn shift_tab_returns_to_the_previous_visible_control() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let forward = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: false,
        });
        shell.handle(&mut tree, &forward, None)?;
        let first = tree.focused();
        shell.handle(&mut tree, &forward, None)?;
        let second = tree.focused();
        let backwards = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: true,
        });
        shell.handle(&mut tree, &backwards, None)?;
        assert_ne!(first, second);
        assert_eq!(tree.focused(), first);
        Ok(())
    }
}
