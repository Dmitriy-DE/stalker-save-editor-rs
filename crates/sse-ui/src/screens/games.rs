//! S4 (Gemini): installed games, fixes, game doctor, environment, encyclopedia.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::{Message, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{NodeKind, Style};
use crate::path::Icon;
use crate::text::{self, Metrics};
use crate::theme;
use crate::widget::WidgetId;
use crate::widget::{Content, Look, Tree};
use sse_core::Result;
use sse_storage::discovery::{normalize_full_path, resolve_links, SaveDirectoryLocator};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    let workspace = Workspace::default();
    vec![
        Box::new(GamesOverview::new(workspace)),
        Box::new(GameFixes::default()),
        Box::new(Environment::default()),
        Box::new(GameDoctor::default()),
        Box::new(Encyclopedia::default()),
    ]
}

/// Target game release supported by the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GameTarget {
    /// S.T.A.L.K.E.R.: Тень Чернобыля
    ShadowOfChernobyl,
    /// S.T.A.L.K.E.R.: Чистое Небо
    ClearSky,
    /// S.T.A.L.K.E.R.: Зов Припяти
    CallOfPripyat,
    /// S.T.A.L.K.E.R.: Тень Чернобыля Enhanced Edition
    ShadowOfChernobylEnhancedEdition,
    /// S.T.A.L.K.E.R.: Чистое Небо Enhanced Edition
    ClearSkyEnhancedEdition,
    /// S.T.A.L.K.E.R.: Зов Припяти Enhanced Edition
    CallOfPripyatEnhancedEdition,
    /// S.T.A.L.K.E.R. 2: Сердце Чернобыля
    Stalker2,
}

impl GameTarget {
    /// All game targets in canonical catalog order.
    pub const ALL: [Self; 7] = [
        Self::ShadowOfChernobyl,
        Self::ClearSky,
        Self::CallOfPripyat,
        Self::ShadowOfChernobylEnhancedEdition,
        Self::ClearSkyEnhancedEdition,
        Self::CallOfPripyatEnhancedEdition,
        Self::Stalker2,
    ];

    /// Short catalog identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "soc",
            Self::ClearSky => "cs",
            Self::CallOfPripyat => "cop",
            Self::ShadowOfChernobylEnhancedEdition => "soc-ee",
            Self::ClearSkyEnhancedEdition => "cs-ee",
            Self::CallOfPripyatEnhancedEdition => "cop-ee",
            Self::Stalker2 => "s2",
        }
    }

    /// Game title in the current interface language.
    #[must_use]
    pub fn title(self) -> &'static str {
        self.title_in(crate::strings::current_language())
    }

    /// Game title in `language`.
    #[must_use]
    pub fn title_in(self, language: &str) -> &'static str {
        crate::strings::t_in(
            language,
            match self {
                Self::ShadowOfChernobyl => "S.T.A.L.K.E.R.: Тень Чернобыля",
                Self::ClearSky => "S.T.A.L.K.E.R.: Чистое Небо",
                Self::CallOfPripyat => "S.T.A.L.K.E.R.: Зов Припяти",
                Self::ShadowOfChernobylEnhancedEdition => "Тень Чернобыля (Enhanced Edition)",
                Self::ClearSkyEnhancedEdition => "Чистое Небо (Enhanced Edition)",
                Self::CallOfPripyatEnhancedEdition => "Зов Припяти (Enhanced Edition)",
                Self::Stalker2 => "S.T.A.L.K.E.R. 2: Сердце Чернобыля",
            },
        )
    }

    /// Steam application identifier if published on Steam.
    #[must_use]
    pub const fn steam_app_id(self) -> Option<u32> {
        match self {
            Self::ShadowOfChernobyl => Some(4_500),
            Self::ClearSky => Some(20_510),
            Self::CallOfPripyat => Some(41_700),
            Self::ShadowOfChernobylEnhancedEdition => Some(2_427_410),
            Self::ClearSkyEnhancedEdition => Some(2_427_420),
            Self::CallOfPripyatEnhancedEdition => Some(2_427_430),
            Self::Stalker2 => Some(1_643_320),
        }
    }

    /// Game family identifier matching `SaveDirectoryCandidate::game_id`.
    #[must_use]
    pub const fn family(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl | Self::ShadowOfChernobylEnhancedEdition => "soc",
            Self::ClearSky | Self::ClearSkyEnhancedEdition => "clear_sky",
            Self::CallOfPripyat | Self::CallOfPripyatEnhancedEdition => "cop",
            Self::Stalker2 => "stalker2",
        }
    }

    /// Release identifier matching `SaveDirectoryCandidate::release_id`.
    #[must_use]
    pub const fn release_id(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "stalker-soc",
            Self::ClearSky => "stalker-cs",
            Self::CallOfPripyat => "stalker-cop",
            Self::ShadowOfChernobylEnhancedEdition => "stalker-soc-ee",
            Self::ClearSkyEnhancedEdition => "stalker-cs-ee",
            Self::CallOfPripyatEnhancedEdition => "stalker-cop-ee",
            Self::Stalker2 => "stalker2",
        }
    }

    /// Whether this game runs on the X-Ray engine (original or enhanced).
    #[must_use]
    pub const fn is_xray(self) -> bool {
        !matches!(self, Self::Stalker2)
    }

    /// Expected directory names under Steam `common/`.
    #[must_use]
    pub const fn install_directories(self) -> &'static [&'static str] {
        match self {
            Self::ShadowOfChernobyl => &["STALKER Shadow of Chernobyl", "STALKER Shadow of Chornobyl"],
            Self::ClearSky => &["STALKER Clear Sky"],
            Self::CallOfPripyat => &["Stalker Call of Pripyat", "STALKER Call of Pripyat"],
            Self::ShadowOfChernobylEnhancedEdition => &["STALKER Shadow of Chornobyl - Enhanced Edition"],
            Self::ClearSkyEnhancedEdition => &["STALKER Clear Sky - Enhanced Edition"],
            Self::CallOfPripyatEnhancedEdition => &["STALKER Call of Prypiat - Enhanced Edition"],
            Self::Stalker2 => &[
                "S.T.A.L.K.E.R. 2 Heart of Chornobyl",
                "STALKER 2 Heart of Chornobyl",
                "S.T.A.L.K.E.R. 2",
            ],
        }
    }
}

/// Where an installation was found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameInstallSource {
    /// Found in a Steam library via manifest or common folder.
    Steam,
    /// Found in GOG Galaxy or standard GOG directory.
    Gog,
    /// Retail disc / original setup installation.
    Retail,
    /// Heroic Games Launcher on Linux.
    Heroic,
    /// Chosen manually.
    Selected,
}

impl GameInstallSource {
    /// Localized display string.
    #[must_use]
    pub fn display(self) -> &'static str {
        self.display_in(crate::strings::current_language())
    }

    /// Display name in `language`.
    #[must_use]
    pub fn display_in(self, language: &str) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Gog => "GOG",
            Self::Retail => "GSC Retail",
            Self::Heroic => "Heroic",
            Self::Selected => crate::strings::t_in(language, "Выбрана вручную"),
        }
    }
}

/// Localizable summary of a completed game-installation search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryStatus {
    /// The search found no supported installations.
    NotFound,
    /// The search found this many supported installations.
    Found(usize),
}

fn tr(language: &str, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    crate::strings::tr_in(Some(language), key, args)
}

fn discovery_status_text(language: &str, status: &DiscoveryStatus) -> String {
    match status {
        DiscoveryStatus::NotFound => crate::strings::t_in(language, "●  НЕ НАЙДЕНО").to_owned(),
        DiscoveryStatus::Found(count) => tr(language, "●  НАЙДЕНО {0}", &[count]),
    }
}

/// Discovered installation of a S.T.A.L.K.E.R. game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredInstallation {
    /// Which game this installation belongs to.
    pub target: GameTarget,
    /// Title of the game.
    pub title: String,
    /// Canonical filesystem directory.
    pub directory: PathBuf,
    /// Distribution platform / source.
    pub source: GameInstallSource,
    /// Steam build ID or "—".
    pub build_id: Option<String>,
    /// Number of verified save files found for this installation.
    pub save_count: usize,
}

#[derive(Clone, Default)]
struct Workspace(Arc<Mutex<WorkspaceState>>);

impl Workspace {
    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[derive(Default)]
struct WorkspaceState {
    discovering: bool,
    idle: bool,
    installations: Vec<DiscoveredInstallation>,
    selected_target: Option<GameTarget>,
    selected_installation: Option<PathBuf>,
    status_message: Option<DiscoveryStatus>,
}

/// Message payload passed from background discovery thread to GamesOverview screen.
#[derive(Clone, Debug)]
pub struct DiscoveredResult {
    /// Found game installations.
    pub installations: Vec<DiscoveredInstallation>,
    /// Summary status message.
    pub status: DiscoveryStatus,
}

/// Installation rows the list shows at once; more installations are found but not listed.
const MAX_INSTALLATION_ROWS: usize = 6;

/// One row of the installation list: a card with its cover plate and texts, and a select button over it.
#[derive(Clone, Copy)]
struct InstallRow {
    stack: WidgetId,
    card: WidgetId,
    plate: WidgetId,
    select: WidgetId,
    name: WidgetId,
    path: WidgetId,
    source: WidgetId,
}

/// Cover plate size in the list: 128×72, or 96×54 in the compact layout.
fn cover_size(compact: bool) -> (f32, f32) {
    if compact {
        (96.0, 54.0)
    } else {
        (128.0, 72.0)
    }
}

/// Width of the selected game's cover, which keeps 16:9.
fn hero_cover_width(compact: bool) -> f32 {
    if compact {
        240.0
    } else {
        400.0
    }
}

/// A small count as a float, for layout arithmetic.
fn count_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}

/// A whole pixel count as a float, for layout arithmetic. Values beyond the range of the window are clamped.
fn px_i64(value: i64) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(i16::MAX))
}

/// Width of the installation list card.
fn list_width(compact: bool) -> f32 {
    if compact {
        340.0
    } else {
        440.0
    }
}

/// A cover plate: raised panel, subtle border, and the radiation sign in the metal colour (no cover art exists).
/// Width of the key column of the selected game's fields.
const KEY_COLUMN: f32 = 120.0;

/// Size of the radiation sign on a cover plate.
const PLATE_ICON: u32 = 32;

/// Style of a cover plate of `width`×`height`. The sign is drawn at the left padding, so the padding centres it; the
/// content box takes what is left of the width.
fn plate_style(width: f32, height: f32) -> Style {
    let left = ((width - f32::from(u16::try_from(PLATE_ICON).unwrap_or(0))) / 2.0).max(0.0);
    Style {
        min: crate::layout::Size::new(width - left, height),
        preferred: crate::layout::Size::new(width - left, height),
        padding: crate::layout::Edges {
            left,
            top: 0.0,
            right: 0.0,
            bottom: 0.0,
        },
        shrink: 0.0,
        ..Style::default()
    }
}

fn cover_plate(tree: &mut Tree, parent: WidgetId, style: Style) -> Result<WidgetId> {
    let plate = tree.add(
        Some(parent),
        NodeKind::Leaf,
        style,
        Content::IconButton {
            icon: Icon::D2Radiation,
            text: String::new(),
            style: TextStyle::new(Face::Body, 12.0),
        },
        Look {
            fill: Some(style::d2::argb(theme::d2::PANEL_RAISED)),
            border: Some((style::d2::argb(theme::d2::BORDER_SUBTLE), 1.0)),
            radius: 2.0,
            text: style::d2::argb(theme::d2::BORDER_METAL),
            icon_size: PLATE_ICON,
            ..Look::default()
        },
    )?;
    // The plate is a picture, not a control: it does nothing when pressed.
    tree.set_enabled(plate, false)?;
    Ok(plate)
}

/// Secondary action button of a card, growing with the card and never shorter than the control height.
fn fill_action_button(tree: &mut Tree, id: WidgetId) -> Result<()> {
    tree.set_style(
        id,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: crate::layout::Size::new(0.0, theme::d2::CONTROL_HEIGHT.0),
            padding: crate::layout::Edges {
                left: 8.0,
                top: 0.0,
                right: 8.0,
                bottom: 0.0,
            },
            ..Style::default()
        },
    )
}

/// Installed games overview screen (ScreenId::Games).
pub struct GamesOverview {
    workspace: Workspace,
    compact: bool,

    // Left card: НАЙДЕННЫЕ УСТАНОВКИ
    discover_button: Option<WidgetId>,
    discovery_status: Option<WidgetId>,
    rows: Vec<InstallRow>,
    empty_panel: Option<WidgetId>,
    empty_search_hint: Option<WidgetId>,
    footer_doctor_button: Option<WidgetId>,

    // Right: ВЫБРАННАЯ ИГРА
    target_prev_button: Option<WidgetId>,
    target_next_button: Option<WidgetId>,
    target_title: Option<WidgetId>,
    status_value: Option<WidgetId>,
    platform_value: Option<WidgetId>,
    build_value: Option<WidgetId>,
    folder_value: Option<WidgetId>,
    saves_value: Option<WidgetId>,
    open_folder_button: Option<WidgetId>,

    // Right: БЫСТРЫЕ ДЕЙСТВИЯ
    action_fixes: Option<WidgetId>,
    action_doctor: Option<WidgetId>,
    action_environment: Option<WidgetId>,
    action_encyclopedia: Option<WidgetId>,

    // Right: МОДЫ
    mods_button: Option<WidgetId>,
    mods_note: Option<WidgetId>,
    right_card: Option<WidgetId>,
    left_card: Option<WidgetId>,
    list_scroll: Option<WidgetId>,
    list_pager: Option<WidgetId>,
    list_previous: Option<WidgetId>,
    list_range: Option<WidgetId>,
    list_next: Option<WidgetId>,
    /// Index of the shown page of installations, and the rows a page holds at the window's size.
    page: usize,
    page_size: usize,
    hero_plate: Option<WidgetId>,
}

impl GamesOverview {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            compact: false,
            discover_button: None,
            discovery_status: None,
            rows: Vec::new(),
            empty_panel: None,
            empty_search_hint: None,
            footer_doctor_button: None,

            target_prev_button: None,
            target_next_button: None,
            target_title: None,
            status_value: None,
            platform_value: None,
            build_value: None,
            folder_value: None,
            saves_value: None,
            open_folder_button: None,

            action_fixes: None,
            action_doctor: None,
            action_environment: None,
            action_encyclopedia: None,

            mods_button: None,
            mods_note: None,
            right_card: None,
            left_card: None,
            list_scroll: None,
            list_pager: None,
            list_previous: None,
            list_range: None,
            list_next: None,
            page: 0,
            page_size: 1,
            hero_plate: None,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (idle, discovering, installations, selected_target, selected_installation, status_message) = {
            let state = self.workspace.lock();
            (
                state.idle,
                state.discovering,
                state.installations.clone(),
                state.selected_target.unwrap_or(GameTarget::ShadowOfChernobyl),
                state.selected_installation.clone(),
                state.status_message.clone(),
            )
        };
        let window_width = cx.tree.size().0;
        if window_width > 0 {
            self.compact = window_width < 1600;
        }
        // Sizes that depend on the window: the list card, the list covers and the hero cover.
        let (cover_width, cover_height) = cover_size(self.compact);
        if let Some(card) = self.left_card {
            let width = list_width(self.compact);
            cx.tree.set_style(
                card,
                crate::layout::Style {
                    preferred: crate::layout::Size::new(width, 0.0),
                    min: crate::layout::Size::new(width, 0.0),
                    max: crate::layout::Size::new(width, f32::INFINITY),
                    shrink: 0.0,
                    align_items: crate::layout::Align::Stretch,
                    ..crate::layout::Style::default()
                },
            )?;
        }
        for row in &self.rows {
            cx.tree.set_style(row.plate, plate_style(cover_width, cover_height))?;
        }
        if let Some(plate) = self.hero_plate {
            let cover_w = hero_cover_width(self.compact);
            cx.tree.set_style(plate, plate_style(cover_w, cover_w * 9.0 / 16.0))?;
        }
        // The pager is part of the card's room, so it is shown before the list's room is measured.
        let has_installations = !installations.is_empty();
        if let Some(pager) = self.list_pager {
            cx.tree.set_visible(pager, has_installations)?;
        }
        // Texts that wrap take the width they are drawn in, which is known only after a layout pass.
        cx.tree.update_layout()?;
        let capacity = self.sync_list_cap(cx)?;
        self.page_size = self.rows_that_fit(cx.tree, capacity);
        let right_width = self
            .right_card
            .map(|id| cx.tree.rect(id).map(|rect| rect.width as f32))
            .transpose()?
            .unwrap_or(0.0);
        // Until the window has its size, the note keeps its natural width.
        let left_width = list_width(self.compact) - 2.0 * 16.0;
        if let Some(hint) = self.empty_search_hint {
            cx.tree.set_style(
                hint,
                crate::layout::Style {
                    min: crate::layout::Size::new(left_width, 0.0),
                    shrink: 0.0,
                    ..crate::layout::Style::default()
                },
            )?;
        }
        if let (Some(note), true) = (self.mods_note, right_width > 0.0) {
            let note_width = right_width - 2.0 * theme::d2::PANEL_PADDING.0 - 16.0 - 220.0;
            cx.tree.set_style(
                note,
                crate::layout::Style {
                    min: crate::layout::Size::new(note_width.max(1.0), 0.0),
                    shrink: 0.0,
                    ..crate::layout::Style::default()
                },
            )?;
        }

        // Update discovery status text
        if let Some(id) = self.discovery_status {
            let language = crate::strings::current_language();
            let text = if discovering {
                crate::strings::t_in(language, "Поиск установок…").to_owned()
            } else if let Some(status) = status_message.as_ref() {
                discovery_status_text(language, status)
            } else if idle {
                crate::strings::t_in(language, "Поиск установок ещё не выполнялся.").to_owned()
            } else if installations.is_empty() {
                crate::strings::t_in(language, "●  НЕ НАЙДЕНО").to_owned()
            } else {
                discovery_status_text(language, &DiscoveryStatus::Found(installations.len()))
            };
            cx.tree.set_text(id, &text)?;
        }

        // Show/hide empty state vs installations list
        let has_installations = !installations.is_empty();
        if let Some(empty) = self.empty_panel {
            cx.tree.set_visible(empty, !has_installations)?;
        }

        // Installation rows: the card shows the cover plate, the name, the path and the source
        let (cover_width, _) = cover_size(self.compact);
        let path_width = list_width(self.compact) - 2.0 * 16.0 - 2.0 * 8.0 - cover_width - 12.0 - 2.0;
        // The page: the rows that fit, from the page's first installation.
        let total = installations.len();
        let pages = total
            .saturating_add(self.page_size.saturating_sub(1))
            .checked_div(self.page_size)
            .unwrap_or(0);
        if self.page >= pages {
            self.page = 0;
        }
        let start = self.page.saturating_mul(self.page_size);
        for (i, row) in self.rows.iter().enumerate() {
            let shown = if i < self.page_size {
                installations.get(start.saturating_add(i))
            } else {
                None
            };
            if let Some(install) = shown {
                cx.tree.set_visible(row.stack, true)?;
                let language = crate::strings::current_language();
                let selected = selected_installation.as_ref() == Some(&install.directory);
                let name_width = list_width(self.compact) - 2.0 * 8.0 - 2.0 * 8.0 - cover_width - 12.0 - 2.0;
                let name = {
                    let metrics = cx.tree.fonts().metrics(Text::Heading.style());
                    text::ellipsize_end(install.target.title_in(language), name_width, &metrics)
                };
                cx.tree.set_text(row.name, &name)?;
                let path_str = install.directory.to_string_lossy();
                let shortened = text::ellipsize_middle(&path_str, path_width, &PathMetrics);
                cx.tree.set_text(row.path, &shortened)?;
                cx.tree.set_text(row.source, install.source.display_in(language))?;
                let (fill, border) = if selected {
                    (theme::d2::ACCENT_TINT, theme::d2::ACCENT)
                } else {
                    (theme::d2::PANEL_RAISED, theme::d2::BORDER_SUBTLE)
                };
                cx.tree.set_look(
                    row.card,
                    Look {
                        fill: Some(style::d2::argb(fill)),
                        border: Some((style::d2::argb(border), 1.0)),
                        radius: 3.0,
                        ..Look::default()
                    },
                )?;
            } else {
                cx.tree.set_visible(row.stack, false)?;
            }
        }

        // Pager: arrows and the range of the rows shown, as in the library.
        if let Some(pager) = self.list_pager {
            cx.tree.set_visible(pager, total > 0)?;
        }
        if let Some(id) = self.list_previous {
            cx.tree.set_enabled(id, self.page > 0)?;
        }
        if let Some(id) = self.list_next {
            cx.tree.set_enabled(id, self.page.saturating_add(1) < pages)?;
        }
        if let Some(id) = self.list_range {
            let text = if total == 0 {
                String::new()
            } else {
                let last = start.saturating_add(self.page_size).min(total);
                tr(
                    crate::strings::current_language(),
                    "{0}–{1} из {2}",
                    &[&start.saturating_add(1), &last, &total],
                )
            };
            cx.tree.set_text(id, &text)?;
        }

        // Selected installation details
        let selected_install = selected_installation
            .as_ref()
            .and_then(|id| installations.iter().find(|install| &install.directory == id));

        if let Some(id) = self.target_title {
            let language = crate::strings::current_language();
            let title = if let Some(install) = selected_install {
                install.target.title_in(language).to_owned()
            } else {
                selected_target.title_in(language).to_owned()
            };
            // The title shares the hero's row with the cover: it is shortened with an ellipsis to the room it has.
            let room = right_width - 2.0 * theme::d2::PANEL_PADDING.0 - hero_cover_width(self.compact) - 16.0;
            let title = if right_width > 0.0 {
                let metrics = cx.tree.fonts().metrics(Text::Title.style());
                text::ellipsize_end(&title, room.max(1.0), &metrics)
            } else {
                title
            };
            cx.tree.set_text(id, &title)?;
        }

        if let Some(id) = self.status_value {
            let text = if selected_install.is_some() {
                crate::strings::t("Установка найдена")
            } else {
                crate::strings::t("Не выбрана")
            };
            cx.tree.set_text(id, text)?;
        }

        if let Some(id) = self.platform_value {
            let text = selected_install.map_or("—", |inst| inst.source.display());
            cx.tree.set_text(id, text)?;
        }

        if let Some(id) = self.build_value {
            let text = selected_install
                .and_then(|inst| inst.build_id.as_deref())
                .unwrap_or("—");
            cx.tree.set_text(id, text)?;
        }

        if let Some(id) = self.folder_value {
            let text = selected_install.map_or("—".to_owned(), |inst| inst.directory.to_string_lossy().to_string());
            let shortened = text::ellipsize_middle(&text, 520.0, &PathMetrics);
            cx.tree.set_text(id, &shortened)?;
        }

        if let Some(id) = self.saves_value {
            let text = selected_install.map_or("—".to_owned(), |inst| inst.save_count.to_string());
            cx.tree.set_text(id, &text)?;
        }

        Ok(())
    }
}

impl GamesOverview {
    /// Caps the installation list at the room the left card has, so the list scrolls instead of pushing the card's
    /// footer below the window. The room comes from the window and from the other parts of the card, which are
    /// measured from their rectangles and do not depend on the list.
    fn sync_list_cap(&self, cx: &mut Context<'_>) -> Result<f32> {
        let (Some(scroll), Some(card)) = (self.list_scroll, self.left_card) else {
            return Ok(0.0);
        };
        let window_height = px_i64(i64::from(cx.tree.size().1));
        let card_top = px_i64(i64::from(cx.tree.rect(card)?.y));
        let mut others = 0.0_f32;
        for child in cx.tree.children(card) {
            if child != scroll {
                others += px_i64(i64::from(cx.tree.rect(child)?.height));
            }
        }
        // The shell keeps 12 px below the content and a 30 px status bar under the window.
        let cap = (window_height - card_top - 42.0 - others).max(0.0);
        cx.tree.set_style(
            scroll,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                max: crate::layout::Size::new(f32::INFINITY, cap),
                ..Style::default()
            },
        )?;
        Ok(cap)
    }

    /// Rows that fit in a list of `capacity` pixels. A row is as tall as its card was laid out (the text column may be
    /// taller than the cover); rows are 4 px apart, and the list has 8 px of padding on each side.
    fn rows_that_fit(&self, tree: &crate::widget::Tree, capacity: f32) -> usize {
        let (_, cover_height) = cover_size(self.compact);
        let measured = self
            .rows
            .first()
            .and_then(|row| tree.rect(row.stack).ok())
            .map_or(0.0, |rect| f32::from(u16::try_from(rect.height).unwrap_or(0)));
        let row_height = if measured > 0.0 { measured } else { cover_height + 16.0 };
        let mut rows = 0_usize;
        while rows < MAX_INSTALLATION_ROWS {
            let next = count_f32(rows.saturating_add(1));
            let needed = next * row_height + (next - 1.0) * 4.0 + 16.0;
            if needed > capacity {
                break;
            }
            rows = rows.saturating_add(1);
        }
        rows.max(1)
    }
}

impl Screen for GamesOverview {
    fn id(&self) -> ScreenId {
        ScreenId::Games
    }

    fn subtitle(&self) -> &str {
        crate::strings::t("ИГРЫ И ИНСТРУМЕНТЫ")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        // Two columns as in the frame: the installation list on the left, the selected game, its actions and the
        // mods note on the right.
        let window_width = cx.tree.size().0;
        if window_width > 0 {
            self.compact = window_width < 1600;
        }
        let gap = if self.compact { 12.0 } else { 16.0 };
        let main_row = cx.tree.add(
            Some(host),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(gap, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;

        // --- LEFT CARD: НАЙДЕННЫЕ УСТАНОВКИ ---
        let left_width = list_width(self.compact);
        let left_card = style::d2::panel(cx.tree, main_row)?;
        cx.tree.set_style(
            left_card,
            crate::layout::Style {
                preferred: crate::layout::Size::new(left_width, 0.0),
                min: crate::layout::Size::new(left_width, 0.0),
                max: crate::layout::Size::new(left_width, f32::INFINITY),
                shrink: 0.0,
                padding: crate::layout::Edges::default(),
                gap: crate::layout::Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
        )?;
        cx.tree.set_clip_children(left_card, true)?;
        self.left_card = Some(left_card);
        let heading_row = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                min: crate::layout::Size::new(0.0, 52.0),
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 0.0,
                    right: 12.0,
                    bottom: 0.0,
                },
                gap: crate::layout::Size::new(10.0, 0.0),
                align_items: crate::layout::Align::Center,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let heading = style::d2::panel_title(cx.tree, heading_row, crate::strings::t("НАЙДЕННЫЕ УСТАНОВКИ"))?;
        cx.tree.set_style(
            heading,
            crate::layout::Style {
                grow: 1.0,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
        )?;
        let find_row = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 8.0,
                    right: 16.0,
                    bottom: 0.0,
                },
                shrink: 0.0,
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.discover_button = Some(style::d2::button(
            cx.tree,
            find_row,
            crate::strings::t("Найти установки"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Small,
        )?);
        if let Some(button) = self.discover_button {
            cx.tree.set_style(
                button,
                crate::layout::Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, theme::d2::CONTROL_HEIGHT.1),
                    ..crate::layout::Style::default()
                },
            )?;
        }

        let status_row = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 10.0,
                    right: 16.0,
                    bottom: 2.0,
                },
                gap: crate::layout::Size::new(10.0, 0.0),
                align_items: crate::layout::Align::Center,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.discovery_status = Some(style::label(
            cx.tree,
            status_row,
            crate::strings::t("Поиск установок ещё не выполнялся."),
            Text::Value,
        )?);

        // Empty state (shown when no installations)
        let empty_card = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                padding: crate::layout::Edges::all(16.0),
                gap: crate::layout::Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.empty_panel = Some(empty_card);
        style::label(
            cx.tree,
            empty_card,
            crate::strings::t("Установки не выбраны"),
            Text::Heading,
        )?;
        self.empty_search_hint = Some(
            cx.tree.add(
                Some(empty_card),
                NodeKind::Leaf,
                Style {
                    shrink: 0.0,
                    ..Style::default()
                },
                Content::Paragraph {
                    text: crate::strings::t(
                        "Нажмите «Найти установки», чтобы проверить поддерживаемые игры на этом компьютере.",
                    )
                    .to_owned(),
                    style: Text::Note.style(),
                },
                Look {
                    text: Text::Note.color(),
                    ..Look::default()
                },
            )?,
        );

        // Installation rows container
        // The list scrolls inside a height capped to the room the card has: its rows must not size the window.
        let list_scroll = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Scroll {
                horizontal: false,
                vertical: true,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            crate::layout::Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        cx.tree.set_clip_children(list_scroll, true)?;
        self.list_scroll = Some(list_scroll);
        let list_container = cx.tree.add(
            Some(list_scroll),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                padding: crate::layout::Edges::all(8.0),
                gap: crate::layout::Size::new(0.0, 4.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let (cover_width, cover_height) = cover_size(self.compact);
        for _ in 0..MAX_INSTALLATION_ROWS {
            let stack = cx.tree.add(
                Some(list_container),
                crate::layout::NodeKind::Stack,
                crate::layout::Style {
                    shrink: 0.0,
                    align_items: crate::layout::Align::Stretch,
                    ..crate::layout::Style::default()
                },
                crate::widget::Content::Panel,
                crate::widget::Look::default(),
            )?;
            let card = cx.tree.add(
                Some(stack),
                crate::layout::NodeKind::Row,
                crate::layout::Style {
                    padding: crate::layout::Edges::all(8.0),
                    gap: crate::layout::Size::new(12.0, 0.0),
                    align_items: crate::layout::Align::Center,
                    min: crate::layout::Size::new(0.0, cover_height + 16.0),
                    shrink: 0.0,
                    ..crate::layout::Style::default()
                },
                crate::widget::Content::Panel,
                crate::widget::Look::default(),
            )?;
            let plate = cover_plate(cx.tree, card, plate_style(cover_width, cover_height))?;
            let info = cx.tree.add(
                Some(card),
                crate::layout::NodeKind::Column,
                crate::layout::Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, 0.0),
                    gap: crate::layout::Size::new(0.0, 4.0),
                    align_items: crate::layout::Align::Stretch,
                    ..crate::layout::Style::default()
                },
                crate::widget::Content::Panel,
                crate::widget::Look::default(),
            )?;
            let name = style::label(cx.tree, info, "", Text::Heading)?;
            let path = style::label(cx.tree, info, "", Text::Note)?;
            let source = style::label(cx.tree, info, "", Text::Value)?;
            cx.tree.set_style(
                source,
                crate::layout::Style {
                    shrink: 0.0,
                    align_self: Some(crate::layout::Align::Start),
                    padding: crate::layout::Edges {
                        left: 7.0,
                        top: 0.0,
                        right: 7.0,
                        bottom: 0.0,
                    },
                    ..crate::layout::Style::default()
                },
            )?;
            cx.tree.set_look(
                source,
                Look {
                    text: style::d2::argb(theme::d2::TEXT_MUTED),
                    border: Some((style::d2::argb(theme::d2::BORDER_METAL), 1.0)),
                    ..Look::default()
                },
            )?;
            // The select button is the last child, so it covers the card and takes the clicks.
            let select = style::button(cx.tree, stack, "", Button::Secondary)?;
            cx.tree.set_look(select, Look::default())?;
            cx.tree.set_style(
                select,
                crate::layout::Style {
                    min: crate::layout::Size::new(0.0, cover_height + 16.0),
                    shrink: 0.0,
                    ..crate::layout::Style::default()
                },
            )?;
            cx.tree.set_visible(stack, false)?;
            self.rows.push(InstallRow {
                stack,
                card,
                plate,
                select,
                name,
                path,
                source,
            });
        }

        // Pager of the list, as in the library: arrows and the range of the rows shown.
        let pager = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                min: crate::layout::Size::new(0.0, 28.0),
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 0.0,
                    right: 16.0,
                    bottom: 0.0,
                },
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Center,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.list_pager = Some(pager);
        self.list_previous = Some(super::shell::library_icon_button(
            cx.tree,
            pager,
            crate::path::Icon::D2ArrowLeft,
        )?);
        self.list_range = Some(style::label(cx.tree, pager, "", Text::Note)?);
        self.list_next = Some(super::shell::library_icon_button(
            cx.tree,
            pager,
            crate::path::Icon::D2ArrowRight,
        )?);
        cx.tree.set_visible(pager, false)?;

        // Footer of the list: the doctor for the selected installation.
        let footer = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 12.0,
                    right: 16.0,
                    bottom: 12.0,
                },
                shrink: 0.0,
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.footer_doctor_button = Some(style::d2::button(
            cx.tree,
            footer,
            crate::strings::t("Открыть Доктор игры"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);

        // --- RIGHT COLUMN ---
        let right = cx.tree.add(
            Some(main_row),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(0.0, gap),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;

        // Hero: ВЫБРАННАЯ ИГРА
        let hero = style::d2::panel(cx.tree, right)?;
        cx.tree.set_style(
            hero,
            crate::layout::Style {
                shrink: 0.0,
                padding: crate::layout::Edges::all(16.0),
                gap: crate::layout::Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
        )?;
        self.right_card = Some(hero);
        let hero_row = cx.tree.add(
            Some(hero),
            crate::layout::NodeKind::Row,
            crate::layout::Style {
                gap: crate::layout::Size::new(16.0, 0.0),
                align_items: crate::layout::Align::Start,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let cover_w = hero_cover_width(self.compact);
        self.hero_plate = Some(cover_plate(
            cx.tree,
            hero_row,
            plate_style(cover_w, cover_w * 9.0 / 16.0),
        )?);
        let details = cx.tree.add(
            Some(hero_row),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let hero_header = style::row(cx.tree, details)?;
        let hero_title = style::d2::panel_title(cx.tree, hero_header, crate::strings::t("ВЫБРАННАЯ ИГРА"))?;
        cx.tree.set_style(
            hero_title,
            crate::layout::Style {
                grow: 1.0,
                shrink: 0.0,
                ..crate::layout::Style::default()
            },
        )?;
        self.target_prev_button = Some(super::shell::library_icon_button(
            cx.tree,
            hero_header,
            crate::path::Icon::D2ArrowLeft,
        )?);
        self.target_next_button = Some(super::shell::library_icon_button(
            cx.tree,
            hero_header,
            crate::path::Icon::D2ArrowRight,
        )?);
        self.target_title = Some(style::label(
            cx.tree,
            details,
            GameTarget::ShadowOfChernobyl.title(),
            Text::Title,
        )?);

        let value_of = |tree: &Tree, row: WidgetId| -> Result<WidgetId> {
            tree.children(row)
                .get(1)
                .copied()
                .ok_or_else(|| sse_core::Error::damaged("key-value row without a value"))
        };
        let status_row = style::d2::key_value_row(cx.tree, details, crate::strings::t("Статус:        "), "")?;
        self.status_value = Some(value_of(cx.tree, status_row)?);
        let platform_row = style::d2::key_value_row(cx.tree, details, crate::strings::t("Платформа:     "), "")?;
        self.platform_value = Some(value_of(cx.tree, platform_row)?);
        let build_row = style::d2::key_value_row(cx.tree, details, crate::strings::t("Номер сборки:  "), "")?;
        self.build_value = Some(value_of(cx.tree, build_row)?);
        let folder_row = style::d2::key_value_row(cx.tree, details, crate::strings::t("Папка игры:    "), "")?;
        self.folder_value = Some(value_of(cx.tree, folder_row)?);
        let saves_row = style::d2::key_value_row(cx.tree, details, crate::strings::t("Число сейвов:  "), "")?;
        self.saves_value = Some(value_of(cx.tree, saves_row)?);
        // Every key takes the same column, so the values line up; the last row keeps 12 px above the button.
        for row in [status_row, platform_row, build_row, folder_row, saves_row] {
            if let Some(key) = cx.tree.children(row).first().copied() {
                cx.tree.set_style(
                    key,
                    Style {
                        min: crate::layout::Size::new(KEY_COLUMN, 0.0),
                        shrink: 0.0,
                        ..Style::default()
                    },
                )?;
            }
        }
        cx.tree.set_style(
            saves_row,
            Style {
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Center,
                margin: crate::layout::Edges {
                    bottom: 12.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;

        self.open_folder_button = Some(style::d2::button(
            cx.tree,
            hero,
            crate::strings::t("Открыть папку"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);

        // Quick actions: БЫСТРЫЕ ДЕЙСТВИЯ
        let actions = style::d2::panel(cx.tree, right)?;
        cx.tree.set_style(
            actions,
            crate::layout::Style {
                shrink: 0.0,
                padding: crate::layout::Edges::all(16.0),
                gap: crate::layout::Size::new(0.0, 12.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
        )?;
        style::d2::panel_title(cx.tree, actions, crate::strings::t("БЫСТРЫЕ ДЕЙСТВИЯ"))?;
        // Four columns of equal share of the card's width.
        let actions_row = cx.tree.add(
            Some(actions),
            crate::layout::NodeKind::Grid {
                columns: vec![crate::layout::Track::Fraction(1.0); 4],
                rows: vec![crate::layout::Track::Auto],
            },
            crate::layout::Style {
                shrink: 0.0,
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        let action = |tree: &mut Tree, row: WidgetId, text: &str| -> Result<WidgetId> {
            let id = style::d2::button(
                tree,
                row,
                text,
                style::d2::ButtonKind::Secondary,
                style::d2::ButtonSize::Normal,
            )?;
            fill_action_button(tree, id)?;
            Ok(id)
        };
        self.action_fixes = Some(action(cx.tree, actions_row, crate::strings::t("Исправления"))?);
        self.action_doctor = Some(action(cx.tree, actions_row, crate::strings::t("Доктор игры"))?);
        self.action_environment = Some(action(cx.tree, actions_row, crate::strings::t("Среда игры"))?);
        self.action_encyclopedia = Some(action(cx.tree, actions_row, crate::strings::t("Энциклопедия"))?);

        // Mods note: МОДЫ, with the mods button on the right
        let mods = style::d2::panel(cx.tree, right)?;
        cx.tree.set_style(
            mods,
            crate::layout::Style {
                grow: 1.0,
                shrink: 0.0,
                padding: crate::layout::Edges::all(16.0),
                gap: crate::layout::Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
        )?;
        let mods_row = style::row(cx.tree, mods)?;
        cx.tree.set_style(
            mods_row,
            crate::layout::Style {
                shrink: 0.0,
                gap: crate::layout::Size::new(16.0, 0.0),
                align_items: crate::layout::Align::Center,
                ..crate::layout::Style::default()
            },
        )?;
        let mods_text = cx.tree.add(
            Some(mods_row),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(0.0, 6.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        style::d2::panel_title(cx.tree, mods_text, crate::strings::t("Моды"))?;
        self.mods_note = Some(
            cx.tree.add(
                Some(mods_text),
                crate::layout::NodeKind::Leaf,
                crate::layout::Style {
                    shrink: 0.0,
                    ..crate::layout::Style::default()
                },
                crate::widget::Content::Paragraph {
                    text: crate::strings::t(
                        "Отдельный менеджер модов отсутствует; используйте профили в разделе «Среда игры».",
                    )
                    .to_owned(),
                    style: Text::Note.style(),
                },
                crate::widget::Look {
                    text: Text::Note.color(),
                    ..crate::widget::Look::default()
                },
            )?,
        );
        self.mods_button = Some(style::d2::button(
            cx.tree,
            mods_row,
            crate::strings::t("Моды"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);

        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let needs_initial_scan = {
            let mut state = self.workspace.lock();
            if state.installations.is_empty() && !state.discovering && !state.idle {
                state.idle = true;
                true
            } else {
                false
            }
        };

        if needs_initial_scan {
            start_background_discovery(&self.workspace, cx);
        }

        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        // The widths of the wrapped texts and of the title follow the window.
        if let Message::Window(WindowEvent::Resized { .. }) = message {
            return self.render(cx);
        }
        // 1. Check if user clicked "Найти установки"
        if clicked.is_some() && clicked == self.discover_button {
            start_background_discovery(&self.workspace, cx);
            return self.render(cx);
        }

        // 2. Check if user clicked "Открыть Доктор игры"
        if clicked.is_some() && (clicked == self.footer_doctor_button || clicked == self.action_doctor) {
            cx.status = Some(crate::strings::t("Переход в раздел «Доктор игры»").to_owned());
            return Ok(());
        }

        // 3. Quick action buttons
        if clicked.is_some() && clicked == self.action_fixes {
            cx.status = Some(crate::strings::t("Переход в раздел «Исправления игры»").to_owned());
            return Ok(());
        }
        if clicked.is_some() && clicked == self.action_environment {
            cx.status = Some(crate::strings::t("Переход в раздел «Среда игры»").to_owned());
            return Ok(());
        }
        if clicked.is_some() && clicked == self.action_encyclopedia {
            cx.status = Some(crate::strings::t("Переход в раздел «Энциклопедия»").to_owned());
            return Ok(());
        }

        // 4. Target switcher (◀ / ▶)
        if clicked.is_some() && clicked == self.target_prev_button {
            let mut state = self.workspace.lock();
            let current = state.selected_target.unwrap_or(GameTarget::ShadowOfChernobyl);
            let idx = GameTarget::ALL.iter().position(|&t| t == current).unwrap_or(0);
            let new_idx = idx.checked_sub(1).unwrap_or(GameTarget::ALL.len().saturating_sub(1));
            let new_target = GameTarget::ALL
                .get(new_idx)
                .copied()
                .unwrap_or(GameTarget::ShadowOfChernobyl);
            state.selected_target = Some(new_target);
            // Also select matching installation if any
            state.selected_installation = state
                .installations
                .iter()
                .find(|inst| inst.target == new_target)
                .map(|inst| inst.directory.clone());
            drop(state);
            return self.render(cx);
        }

        if clicked.is_some() && clicked == self.target_next_button {
            let mut state = self.workspace.lock();
            let current = state.selected_target.unwrap_or(GameTarget::ShadowOfChernobyl);
            let idx = GameTarget::ALL.iter().position(|&t| t == current).unwrap_or(0);
            let next_idx = idx.saturating_add(1);
            let new_idx = if next_idx >= GameTarget::ALL.len() { 0 } else { next_idx };
            let new_target = GameTarget::ALL
                .get(new_idx)
                .copied()
                .unwrap_or(GameTarget::ShadowOfChernobyl);
            state.selected_target = Some(new_target);
            state.selected_installation = state
                .installations
                .iter()
                .find(|inst| inst.target == new_target)
                .map(|inst| inst.directory.clone());
            drop(state);
            return self.render(cx);
        }

        // Pager arrows of the installation list
        if clicked.is_some() && clicked == self.list_previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.list_next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }

        // 5. Click on an installation row
        for (i, row) in self.rows.iter().enumerate() {
            if clicked.is_some() && clicked == Some(row.select) {
                let index = self.page.saturating_mul(self.page_size).saturating_add(i);
                let mut state = self.workspace.lock();
                if let Some((target, directory)) = state
                    .installations
                    .get(index)
                    .map(|inst| (inst.target, inst.directory.clone()))
                {
                    state.selected_target = Some(target);
                    state.selected_installation = Some(directory);
                }
                drop(state);
                return self.render(cx);
            }
        }

        // 6. Click on "Открыть папку"
        if clicked.is_some() && clicked == self.open_folder_button {
            let state = self.workspace.lock();
            let selected_install = state
                .selected_installation
                .as_ref()
                .and_then(|id| state.installations.iter().find(|install| &install.directory == id));
            if let Some(install) = selected_install {
                cx.status = Some(tr(
                    crate::strings::current_language(),
                    "Папка игры: {0}",
                    &[&install.directory.display()],
                ));
            } else {
                cx.status = Some(crate::strings::t("Установка игры не выбрана").to_owned());
            }
            return Ok(());
        }

        // 7. Click on "Моды" note button
        if clicked.is_some() && clicked == self.mods_button {
            cx.status =
                Some(crate::strings::t("Отдельный менеджер модов отсутствует; используйте «Среда игры».").to_owned());
            return Ok(());
        }

        // 8. Result of background discovery
        if let Message::User(AppMessage::ToScreen(ScreenId::Games, payload)) = message {
            if let Some(result) = payload.downcast_ref::<DiscoveredResult>() {
                let mut state = self.workspace.lock();
                state.discovering = false;
                state.installations.clone_from(&result.installations);
                state.status_message = Some(result.status.clone());
                // The search is over: the status bar says what it found, instead of "searching".
                cx.status = Some(discovery_status_text(
                    crate::strings::current_language(),
                    &result.status,
                ));
                self.page = 0;
                if state
                    .selected_installation
                    .as_ref()
                    .is_none_or(|id| !state.installations.iter().any(|install| &install.directory == id))
                    && !state.installations.is_empty()
                {
                    state.selected_installation = state.installations.first().map(|install| install.directory.clone());
                    if let Some(first) = state.installations.first() {
                        state.selected_target = Some(first.target);
                    }
                }
                drop(state);
                self.render(cx)?;
            }
        }

        Ok(())
    }
}

fn start_background_discovery(workspace: &Workspace, cx: &mut Context<'_>) {
    let Some(proxy) = cx.proxy.cloned() else {
        // Running headless / screenshot test mode: execute discovery synchronously or immediately
        let found = discover_all_installations();
        let status = if found.is_empty() {
            DiscoveryStatus::NotFound
        } else {
            DiscoveryStatus::Found(found.len())
        };
        let mut state = workspace.lock();
        state.discovering = false;
        state.installations = found;
        state.status_message = Some(status);
        if state
            .selected_installation
            .as_ref()
            .is_none_or(|id| !state.installations.iter().any(|install| &install.directory == id))
            && !state.installations.is_empty()
        {
            state.selected_installation = state.installations.first().map(|install| install.directory.clone());
            if let Some(first) = state.installations.first() {
                state.selected_target = Some(first.target);
            }
        }
        return;
    };

    {
        let mut state = workspace.lock();
        if state.discovering {
            return;
        }
        state.discovering = true;
        state.status_message = None;
    }

    let workspace_clone = workspace.clone();
    sse_app::tasks::spawn_named_detached("game-read", move || {
        let found = discover_all_installations();
        let status = if found.is_empty() {
            DiscoveryStatus::NotFound
        } else {
            DiscoveryStatus::Found(found.len())
        };
        let result = DiscoveredResult {
            installations: found,
            status,
        };
        proxy.send(AppMessage::ToScreen(ScreenId::Games, Box::new(result)));
        let mut state = workspace_clone.lock();
        state.discovering = false;
    });

    cx.status = Some(crate::strings::t("Поиск установок игр на диске…").to_owned());
}

/// Discovers installations across Steam libraries, GOG, Heroic, and known standard paths.
#[must_use]
pub fn discover_all_installations() -> Vec<DiscoveredInstallation> {
    let mut installations = discover_game_installations();

    // Save headers are needed only for the Games overview's save-count column.
    count_saves_for_installations(&mut installations);

    installations
}

/// Discovers game installation paths without opening or counting save files.
///
/// This is intended for local diagnostics, where the report needs installation paths only.
#[must_use]
pub fn discover_game_installations() -> Vec<DiscoveredInstallation> {
    let mut installations = Vec::new();
    let mut seen_dirs = HashSet::new();

    // 1. Steam discovery
    let steam_roots = SaveDirectoryLocator::default_steam_roots();
    let libraries = get_steam_libraries_list(&steam_roots);

    for target in GameTarget::ALL {
        if let Some(app_id) = target.steam_app_id() {
            for library in &libraries {
                let manifest = read_steam_app_manifest(library, app_id);
                let build_id = manifest
                    .as_deref()
                    .and_then(|content| parse_acf_string_value(content, "buildid"))
                    .map(|build_id| build_id.trim().to_owned());
                if let Some(install_dir) = manifest
                    .as_deref()
                    .and_then(|content| find_manifest_install_directory(library, app_id, content))
                {
                    if has_expected_marker(target, &install_dir) {
                        add_installation(
                            &mut installations,
                            &mut seen_dirs,
                            target,
                            install_dir,
                            GameInstallSource::Steam,
                            build_id.clone(),
                        );
                    }
                }

                // Check common folder fallback under steamapps/common
                for install_name in target.install_directories() {
                    let common_path = library.join("steamapps").join("common").join(install_name);
                    if has_expected_marker(target, &common_path) {
                        add_installation(
                            &mut installations,
                            &mut seen_dirs,
                            target,
                            common_path,
                            GameInstallSource::Steam,
                            build_id.clone(),
                        );
                    }
                }
            }
        }
    }

    // 2. Non-Steam discovery (GOG, Heroic, standard user directories)
    discover_non_steam_installations(&mut installations, &mut seen_dirs);

    installations.sort_by(|a, b| a.target.cmp(&b.target).then_with(|| a.directory.cmp(&b.directory)));

    installations
}

fn add_installation(
    installations: &mut Vec<DiscoveredInstallation>,
    seen_dirs: &mut HashSet<PathBuf>,
    target: GameTarget,
    directory: PathBuf,
    source: GameInstallSource,
    build_id: Option<String>,
) {
    let canonical = normalize_full_path(&directory);
    let resolved = resolve_links(&canonical);
    if !seen_dirs.insert(resolved.clone()) {
        return;
    }

    installations.push(DiscoveredInstallation {
        target,
        title: target.title().to_owned(),
        directory: resolved,
        source,
        build_id,
        save_count: 0,
    });
}

fn discover_non_steam_installations(installations: &mut Vec<DiscoveredInstallation>, seen_dirs: &mut HashSet<PathBuf>) {
    let home = std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/"));

    // Heroic config path on Linux: ~/.config/heroic/gog_store/installed.json
    let heroic_json = home
        .join(".config")
        .join("heroic")
        .join("gog_store")
        .join("installed.json");
    if let Ok(content) = fs::read_to_string(&heroic_json) {
        parse_heroic_gog_installs(&content, installations, seen_dirs);
    }

    // Common GOG directories: ~/Games, ~/GOG Games, C:\GOG Games
    let common_game_roots = [
        home.join("Games"),
        home.join("GOG Games"),
        home.join("Games").join("Heroic"),
        PathBuf::from("C:\\GOG Games"),
    ];

    for root in &common_game_roots {
        if !root.is_dir() {
            continue;
        }
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let dir_path = entry.path();
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    for target in GameTarget::ALL {
                        if folder_matches_target(&folder_name, target) && has_expected_marker(target, &dir_path) {
                            add_installation(
                                installations,
                                seen_dirs,
                                target,
                                dir_path.clone(),
                                GameInstallSource::Gog,
                                None,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn parse_heroic_gog_installs(
    json_text: &str,
    installations: &mut Vec<DiscoveredInstallation>,
    seen_dirs: &mut HashSet<PathBuf>,
) {
    // Simple robust scanner for "install_path": "..."
    for line in json_text.lines() {
        if let Some((_, rhs)) = line.split_once("\"install_path\":") {
            let path_part = rhs.trim().trim_matches([',', '"']);
            let path = PathBuf::from(path_part);
            if path.is_dir() {
                let folder_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                for target in GameTarget::ALL {
                    if folder_matches_target(&folder_name, target) && has_expected_marker(target, &path) {
                        add_installation(
                            installations,
                            seen_dirs,
                            target,
                            path.clone(),
                            GameInstallSource::Heroic,
                            None,
                        );
                    }
                }
            }
        }
    }
}

fn folder_matches_target(name: &str, target: GameTarget) -> bool {
    let lower = name.to_ascii_lowercase();
    match target {
        GameTarget::ShadowOfChernobyl => {
            !lower.contains("enhanced")
                && !lower.contains(" - ee")
                && (lower.contains("shadow of ch") || lower.contains("shoc"))
        }
        GameTarget::ClearSky => {
            !lower.contains("enhanced")
                && !lower.contains(" - ee")
                && (lower.contains("clear sky") || lower.contains("cs"))
        }
        GameTarget::CallOfPripyat => {
            !lower.contains("enhanced")
                && !lower.contains(" - ee")
                && (lower.contains("call of pr") || lower.contains("cop"))
        }
        GameTarget::ShadowOfChernobylEnhancedEdition => {
            (lower.contains("shadow of ch") || lower.contains("shoc"))
                && (lower.contains("enhanced") || lower.contains(" - ee"))
        }
        GameTarget::ClearSkyEnhancedEdition => {
            (lower.contains("clear sky") || lower.contains("cs"))
                && (lower.contains("enhanced") || lower.contains(" - ee"))
        }
        GameTarget::CallOfPripyatEnhancedEdition => {
            (lower.contains("call of pr") || lower.contains("cop"))
                && (lower.contains("enhanced") || lower.contains(" - ee"))
        }
        GameTarget::Stalker2 => {
            lower.contains("s.t.a.l.k.e.r. 2") || lower.contains("stalker 2") || lower.contains("heart of ch")
        }
    }
}

/// Checks whether the game directory contains expected markers.
#[must_use]
pub fn has_expected_marker(target: GameTarget, directory: &Path) -> bool {
    if !directory.is_dir() {
        return false;
    }
    if target.is_xray() {
        directory.join("fsgame.ltx").is_file()
            || directory.join("fsgame_soc.ltx").is_file()
            || directory.join("fsgame_cs.ltx").is_file()
            || directory.join("fsgame_cop.ltx").is_file()
    } else {
        directory.join("Stalker2").join("Content").join("Paks").is_dir()
    }
}

fn get_steam_libraries_list(steam_roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut libraries = Vec::new();
    let mut seen = HashSet::new();

    for root in steam_roots {
        if root.as_os_str().is_empty() {
            continue;
        }
        let full_root = normalize_full_path(root);
        let resolved = resolve_links(&full_root);
        if !resolved.is_dir() {
            continue;
        }
        if seen.insert(resolved.clone()) {
            libraries.push(resolved.clone());
        }

        let vdf_file = resolved.join("steamapps").join("libraryfolders.vdf");
        match fs::read_to_string(&vdf_file) {
            Ok(vdf_text) => {
                for path_str in parse_vdf_library_paths(&vdf_text) {
                    let full_path = normalize_full_path(Path::new(&path_str));
                    let resolved_path = resolve_links(&full_path);
                    if resolved_path.is_dir() && seen.insert(resolved_path.clone()) {
                        libraries.push(resolved_path);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                sse_app::diagnostics::warn(&format!(
                    "failed to read Steam library folders index ({:?})",
                    error.kind()
                ));
            }
        }
    }

    libraries
}

fn parse_vdf_library_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let quotes = extract_quoted_strings(line);
        for (idx, &token) in quotes.iter().enumerate() {
            if token.eq_ignore_ascii_case("path") {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    paths.push(val.replace("\\\\", "\\"));
                }
            } else if token.chars().all(|c| c.is_ascii_digit()) {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    if val.contains('/') || val.contains('\\') {
                        paths.push(val.replace("\\\\", "\\"));
                    }
                }
            }
        }
    }
    paths
}

fn read_steam_app_manifest(library_root: &Path, app_id: u32) -> Option<String> {
    let manifest = library_root.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
    match fs::read_to_string(manifest) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            report_steam_manifest_read_error(app_id, error);
            None
        }
    }
}

fn report_steam_manifest_read_error(app_id: u32, error: std::io::Error) {
    sse_app::diagnostics::warn(&format!(
        "failed to read Steam app manifest for app id {app_id}: {error}"
    ));
}

fn find_manifest_install_directory(library_root: &Path, app_id: u32, content: &str) -> Option<PathBuf> {
    let parsed_app_id = parse_acf_string_value(content, "appid")?;
    if parsed_app_id.trim() != app_id.to_string() {
        return None;
    }
    let install_dir = parse_acf_string_value(content, "installdir")?;
    if install_dir.trim().is_empty() {
        return None;
    }
    let common_dir = library_root.join("steamapps").join("common").join(install_dir.trim());
    if common_dir.is_dir() {
        Some(common_dir)
    } else {
        None
    }
}

fn parse_acf_string_value<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    for line in text.lines() {
        let quotes = extract_quoted_strings(line);
        for (idx, &token) in quotes.iter().enumerate() {
            if token.eq_ignore_ascii_case(key) {
                if let Some(&val) = quotes.get(idx.saturating_add(1)) {
                    return Some(val);
                }
            }
        }
    }
    None
}

fn extract_quoted_strings(line: &str) -> Vec<&str> {
    line.split('"')
        .enumerate()
        .filter(|(idx, _)| idx % 2 == 1)
        .map(|(_, s)| s)
        .collect()
}

fn count_saves_for_installations(installations: &mut [DiscoveredInstallation]) {
    let discovery_options = super::save_directory_discovery_options();
    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&discovery_options));
    let mut family_counts = std::collections::HashMap::new();

    for family in ["soc", "clear_sky", "cop", "stalker2"] {
        let mut save_paths: Vec<PathBuf> = candidates
            .iter()
            .filter(|candidate| candidate.game_id == family)
            .map(|candidate| candidate.directory_path.clone())
            .collect();

        for install in installations.iter().filter(|install| install.target.family() == family) {
            let local = install.directory.join("_appdata_").join("savedgames");
            if local.is_dir() {
                save_paths.push(local);
            }
        }

        let mut seen_directories = HashSet::new();
        let mut seen_slots = HashSet::new();
        let mut count = 0_usize;
        for save_path in save_paths {
            let canonical = resolve_links(&normalize_full_path(&save_path));
            if !seen_directories.insert(canonical) {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };
                    let lower = file_name.to_ascii_lowercase();
                    if (lower.ends_with(".sav") || lower.ends_with(".scop") || lower.ends_with(".scs"))
                        && lower != "campaignssave.sav"
                        && lower != "analyticsdata.sav"
                    {
                        let key = resolve_links(&normalize_full_path(&path));
                        if seen_slots.insert(key) {
                            count = count.saturating_add(1);
                        }
                    }
                }
            }
        }
        family_counts.insert(family, count);
    }

    for install in installations {
        install.save_count = family_counts.get(install.target.family()).copied().unwrap_or(0);
    }
}

struct PathMetrics;

impl Metrics for PathMetrics {
    fn advance(&self, character: char) -> f32 {
        if character.is_ascii() {
            7.0
        } else {
            8.0
        }
    }

    fn kerning(&self, _left: char, _right: char) -> f32 {
        0.0
    }
}

#[derive(Debug)]
struct EnvironmentResult {
    lines: Vec<String>,
}

#[derive(Default)]
struct Environment {
    status: Option<WidgetId>,
    lines: Vec<WidgetId>,
    snapshot: Option<WidgetId>,
    restore: Option<WidgetId>,
    audit: Option<WidgetId>,
    delete_snapshot: Option<WidgetId>,
    snapshot_rows: Vec<WidgetId>,
    snapshot_ids: Vec<String>,
    selected_snapshot: Option<String>,
    profile_name: Option<WidgetId>,
    save_profile: Option<WidgetId>,
    apply_profile: Option<WidgetId>,
    delete_profile: Option<WidgetId>,
    user_inputs: Vec<(String, WidgetId, WidgetId, WidgetId)>,
    profile_rows: Vec<WidgetId>,
    profile_ids: Vec<String>,
    selected_profile: Option<String>,
    pending_restore: Option<String>,
    pending_delete_snapshot: Option<String>,
    pending_delete_profile: Option<String>,
}

impl Environment {
    fn refresh_lists(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let language = crate::strings::current_language();
        self.snapshot_ids.clear();
        if let Some(directory) = cx.app.game_dir() {
            let snapshots = sse_fixes::toolkit::ToolkitSnapshotService::list_snapshots(directory)?;
            for (index, widget) in self.snapshot_rows.iter().copied().enumerate() {
                if let Some(snapshot) = snapshots.get(index) {
                    self.snapshot_ids.push(snapshot.id.clone());
                    let game_title = crate::strings::t_in(language, snapshot.game.title());
                    let fix_count = snapshot.installed_fixes.len();
                    let text = tr(
                        language,
                        "{0} · {1} · исправлений: {2}",
                        &[&snapshot.label, &game_title, &fix_count],
                    );
                    cx.tree.set_text(widget, &text)?;
                    cx.tree.set_visible(widget, true)?;
                } else {
                    cx.tree.set_visible(widget, false)?;
                }
            }
        }
        let profiles =
            sse_fixes::toolkit::ToolkitProfileService::list_profiles(&sse_app::paths::default_data_directory())?;
        self.profile_ids.clear();
        for (index, widget) in self.profile_rows.iter().copied().enumerate() {
            if let Some(profile) = profiles.get(index) {
                self.profile_ids.push(profile.id.clone());
                let game_title = crate::strings::t_in(language, profile.profile.game.title());
                let fix_count = profile.profile.target_fix_ids.len();
                let text = tr(
                    language,
                    "{0} · {1} · исправлений: {2}",
                    &[&profile.profile.name, &game_title, &fix_count],
                );
                cx.tree.set_text(widget, &text)?;
                cx.tree.set_visible(widget, true)?;
            } else {
                cx.tree.set_visible(widget, false)?;
            }
        }
        Ok(())
    }

    fn inspect(cx: &Context<'_>) -> std::result::Result<(sse_content::CompanionGame, PathBuf), String> {
        let game = cx
            .app
            .selected_game()
            .ok_or_else(|| crate::strings::t("Сначала выберите игру в «Обзоре игр»").to_owned())?;
        let directory = cx
            .app
            .game_dir()
            .map(Path::to_path_buf)
            .ok_or_else(|| crate::strings::t("Папка игры не выбрана").to_owned())?;
        let target = match game {
            "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => sse_content::CompanionGame::ShadowOfChernobyl,
            "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => sse_content::CompanionGame::ClearSky,
            "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => sse_content::CompanionGame::CallOfPripyat,
            _ => return Err(crate::strings::t("Среда X-Ray для выбранной игры не применяется").to_owned()),
        };
        Ok((target, directory))
    }

    fn start(&self, cx: &mut Context<'_>) {
        let Ok((game, directory)) = Self::inspect(cx) else {
            if let Some(id) = self.status {
                let _ = cx
                    .tree
                    .set_text(id, crate::strings::t("Сначала выберите X-Ray игру в «Обзоре игр»"));
            }
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        let language = crate::strings::current_language().to_owned();
        sse_app::tasks::spawn_named_detached("game-read", move || {
            let search = sse_content::CompanionArchiveLocator::discover(&directory, &["fsgame.ltx"], game);
            let gamedata = search.game_data_directory.as_ref().is_some_and(|path| path.is_dir());
            let mods = directory.join("mods");
            let unpacked = search.game_data_directory.as_ref().map_or(0_usize, |root| {
                std::fs::read_dir(root).map_or(0, |entries| entries.flatten().count())
            });
            let mods_path = if mods.is_dir() {
                mods.display().to_string()
            } else {
                crate::strings::t_in(&language, "нет").to_owned()
            };
            let mut lines = vec![
                tr(
                    &language,
                    "fsgame.ltx: {0}",
                    &[&search.fsgame_path.as_ref().map_or_else(
                        || crate::strings::t_in(&language, "не найден").to_owned(),
                        |path| path.display().to_string(),
                    )],
                ),
                tr(
                    &language,
                    "gamedata: {0}",
                    &[&crate::strings::t_in(
                        &language,
                        if gamedata { "найдена" } else { "нет" },
                    )],
                ),
                tr(&language, "mods: {0}", &[&mods_path]),
                tr(&language, "распакованные файлы/папки в gamedata: {0}", &[&unpacked]),
                tr(&language, "архивов обнаружено: {0}", &[&search.archive_paths.len()]),
            ];
            lines.extend(search.issues.into_iter().map(|issue| tr(&language, "⚠ {0}", &[&issue])));
            proxy.send(AppMessage::ToScreen(
                ScreenId::Environment,
                Box::new(EnvironmentResult { lines }),
            ));
        });
    }
}

impl Screen for Environment {
    fn id(&self) -> ScreenId {
        ScreenId::Environment
    }
    fn subtitle(&self) -> &str {
        crate::strings::t("Что найдено в установке; экран ничего не изменяет")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, crate::strings::t("СРЕДА ИГРЫ"), Text::Heading)?;
        self.status = Some(style::label(
            cx.tree,
            card,
            crate::strings::t("Управляемая установка: не выбрана"),
            Text::Note,
        )?);
        style::label(cx.tree, card, crate::strings::t("УПРАВЛЯЕМЫЕ СНИМКИ"), Text::Heading)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t(
                "Снимки включают только файлы и манифесты Game Fix, Companion и настроек, которыми владеет инструмент.",
            ),
            Text::Note,
        )?;
        for _ in 0..6 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.snapshot_rows.push(row);
        }
        self.snapshot = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("СОЗДАТЬ СНИМОК"),
            Button::Primary,
        )?);
        self.restore = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("ВОССТАНОВИТЬ ВЫБРАННЫЙ"),
            Button::Danger,
        )?);
        self.delete_snapshot = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("УДАЛИТЬ СНИМОК"),
            Button::Danger,
        )?);
        style::label(cx.tree, card, crate::strings::t("ПРОФИЛИ ИГРЫ"), Text::Heading)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t("Профиль хранит набор Game Fix и управляемые значения user.ltx."),
            Text::Note,
        )?;
        for _ in 0..6 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.profile_rows.push(row);
        }
        self.profile_name = Some(style::input(cx.tree, card, "")?);
        self.save_profile = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("СОХРАНИТЬ ТЕКУЩЕЕ СОСТОЯНИЕ"),
            Button::Primary,
        )?);
        self.apply_profile = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("ПРИМЕНИТЬ ПРОФИЛЬ"),
            Button::Secondary,
        )?);
        self.delete_profile = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("УДАЛИТЬ ПРОФИЛЬ"),
            Button::Danger,
        )?);
        style::label(cx.tree, card, crate::strings::t("НАСТРОЙКИ user.ltx"), Text::Heading)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t(
                "Изменяются только известные параметры; остальные строки user.ltx сохраняются без изменений.",
            ),
            Text::Note,
        )?;
        for setting in sse_fixes::toolkit::MANAGED_SETTINGS.iter().take(8) {
            let row = style::row(cx.tree, card)?;
            style::label(cx.tree, row, setting.key, Text::Body)?;
            let input = style::input(cx.tree, row, "")?;
            let apply = style::button(cx.tree, row, crate::strings::t("ПРИМЕНИТЬ"), Button::Secondary)?;
            let default = style::button(cx.tree, row, crate::strings::t("ПО УМОЛЧАНИЮ"), Button::Secondary)?;
            self.user_inputs.push((setting.key.to_owned(), input, apply, default));
        }
        style::label(cx.tree, card, crate::strings::t("АУДИТ УСТАНОВКИ"), Text::Heading)?;
        self.audit = Some(style::button(
            cx.tree,
            card,
            crate::strings::t("ПРОВЕРИТЬ"),
            Button::Secondary,
        )?);
        for _ in 0..10 {
            let line = style::label(cx.tree, card, "", Text::Body)?;
            cx.tree.set_visible(line, false)?;
            self.lines.push(line);
        }
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.start(cx);
        self.refresh_lists(cx)?;
        let language = crate::strings::current_language();
        let game = cx.app.selected_game().unwrap_or("—");
        let directory = cx
            .app
            .game_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| crate::strings::t_in(language, "не выбрана").to_owned());
        if let Some(status) = self.status {
            let text = tr(language, "Управляемая установка: {0} · {1}", &[&game, &directory]);
            cx.tree.set_text(status, &text)?;
        }
        if let Some(game_directory) = cx.app.game_dir() {
            if let Ok(settings) = sse_fixes::toolkit::ManagedUserLtxSettings::read_managed_settings(game_directory) {
                for (key, input, _, _) in &self.user_inputs {
                    cx.tree
                        .set_input_text(*input, settings.get(key).map_or("", String::as_str))?;
                }
                let _ = cx.tree.take_changed_inputs();
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
        let changed = cx.tree.take_changed_inputs();
        if changed.iter().any(|id| self.profile_name == Some(*id)) {
            self.selected_profile = None;
        }
        if let Some(clicked) = clicked {
            if let Some(index) = self.snapshot_rows.iter().position(|widget| *widget == clicked) {
                self.selected_snapshot = self.snapshot_ids.get(index).cloned();
                if let Some(id) = &self.selected_snapshot {
                    cx.status = Some(tr(crate::strings::current_language(), "Выбран снимок: {0}", &[id]));
                }
                return Ok(());
            }
            if let Some(index) = self.profile_rows.iter().position(|widget| *widget == clicked) {
                self.selected_profile = self.profile_ids.get(index).cloned();
                if let Some(id) = &self.selected_profile {
                    cx.status = Some(tr(crate::strings::current_language(), "Выбран профиль: {0}", &[id]));
                }
                return Ok(());
            }
            if Some(clicked) == self.delete_snapshot {
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    cx.status = Some(crate::strings::t("Управляемая установка: не выбрана").to_owned());
                    return Ok(());
                };
                let Some(id) = self.selected_snapshot.clone() else {
                    cx.status = Some(crate::strings::t("Снимков пока нет: создайте первый кнопкой ниже.").to_owned());
                    return Ok(());
                };
                if self.pending_delete_snapshot.as_deref() != Some(id.as_str()) {
                    self.pending_delete_snapshot = Some(id.clone());
                    cx.status = Some(tr(
                        crate::strings::current_language(),
                        "Удаление снимка необратимо. Нажмите «УДАЛИТЬ СНИМОК» ещё раз для подтверждения: {0}",
                        &[&id],
                    ));
                    return Ok(());
                }
                self.pending_delete_snapshot = None;
                sse_fixes::toolkit::ToolkitSnapshotService::delete_snapshot(&directory, &id)?;
                cx.status = Some(tr(crate::strings::current_language(), "Снимок удалён: {0}", &[&id]));
                self.selected_snapshot = None;
                self.refresh_lists(cx)?;
                return Ok(());
            }
            if Some(clicked) == self.save_profile {
                let Some(game) = cx.app.selected_game().and_then(fix_target) else {
                    cx.status = Some(crate::strings::t("Управляемая установка: не выбрана").to_owned());
                    return Ok(());
                };
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    cx.status = Some(crate::strings::t("Управляемая установка: не выбрана").to_owned());
                    return Ok(());
                };
                let name = self
                    .profile_name
                    .and_then(|id| cx.tree.input_text(id).ok())
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                if name.is_empty() {
                    cx.status = Some(crate::strings::t("Введите имя профиля.").to_owned());
                    return Ok(());
                }
                let engine = sse_fixes::GameFixEngine::new();
                let fixes = engine.list_installed(&directory, None)?;
                let settings = sse_fixes::toolkit::ManagedUserLtxSettings::read_managed_settings(&directory)?;
                let profile = sse_fixes::toolkit::ToolkitProfile {
                    name: name.clone(),
                    description: String::new(),
                    game,
                    target_fix_ids: fixes
                        .into_iter()
                        .filter(|fix| fix.state == sse_fixes::GameFixState::Installed)
                        .map(|fix| fix.id)
                        .collect(),
                    user_ltx_overrides: settings,
                    s2_mods_enabled: None,
                };
                match sse_fixes::toolkit::ToolkitProfileService::save_profile(
                    &sse_app::paths::default_data_directory(),
                    &directory,
                    &profile,
                    &engine,
                ) {
                    Ok(id) => {
                        self.selected_profile = Some(id);
                        cx.status = Some(tr(
                            crate::strings::current_language(),
                            "Профиль сохранён: {0}",
                            &[&name],
                        ));
                    }
                    Err(error) => {
                        cx.status = Some(tr(
                            crate::strings::current_language(),
                            "Не удалось сохранить профиль: {0}",
                            &[&error],
                        ))
                    }
                }
                return Ok(());
            }
            if Some(clicked) == self.apply_profile {
                let profiles = sse_fixes::toolkit::ToolkitProfileService::list_profiles(
                    &sse_app::paths::default_data_directory(),
                )?;
                let selected = self
                    .selected_profile
                    .as_deref()
                    .and_then(|id| profiles.iter().find(|item| item.id == id))
                    .or_else(|| profiles.first())
                    .cloned();
                let Some(selected) = selected else {
                    cx.status = Some(crate::strings::t("Сначала выберите профиль.").to_owned());
                    return Ok(());
                };
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    return Ok(());
                };
                let Some(proxy) = cx.proxy.cloned() else {
                    return Ok(());
                };
                let language = crate::strings::current_language().to_owned();
                sse_app::tasks::spawn_named_detached("game-write", move || {
                    let engine = sse_fixes::GameFixEngine::new();
                    let catalog = sse_fixes::GameFixCatalog;
                    let lines = match sse_fixes::toolkit::ToolkitProfileService::apply_profile(
                        &directory,
                        &selected.profile,
                        &engine,
                        &catalog,
                    ) {
                        Ok(report) => vec![tr(
                            &language,
                            "Профиль применён: {0} · резервная точка: {1}",
                            &[&report.profile_name, &report.pre_switch_snapshot_id],
                        )],
                        Err(error) => vec![tr(&language, "Не удалось применить профиль: {0}", &[&error])],
                    };
                    proxy.send(AppMessage::ToScreen(
                        ScreenId::Environment,
                        Box::new(EnvironmentResult { lines }),
                    ));
                });
                return Ok(());
            }
            if Some(clicked) == self.delete_profile {
                let profiles = sse_fixes::toolkit::ToolkitProfileService::list_profiles(
                    &sse_app::paths::default_data_directory(),
                )?;
                let selected = self
                    .selected_profile
                    .as_deref()
                    .and_then(|id| profiles.iter().find(|item| item.id == id))
                    .or_else(|| profiles.first());
                if let Some(selected) = selected {
                    if self.pending_delete_profile.as_deref() != Some(selected.id.as_str()) {
                        self.pending_delete_profile = Some(selected.id.clone());
                        cx.status = Some(tr(
                            crate::strings::current_language(),
                            "Удаление профиля необратимо. Нажмите «УДАЛИТЬ ПРОФИЛЬ» ещё раз для подтверждения: {0}",
                            &[&selected.profile.name],
                        ));
                        return Ok(());
                    }
                    self.pending_delete_profile = None;
                    match sse_fixes::toolkit::ToolkitProfileService::delete_profile(
                        &sse_app::paths::default_data_directory(),
                        &selected.id,
                    ) {
                        Ok(()) => {
                            self.selected_profile = None;
                            cx.status = Some(crate::strings::t("Профиль удалён.").to_owned());
                        }
                        Err(error) => {
                            cx.status = Some(tr(
                                crate::strings::current_language(),
                                "Не удалось удалить профиль: {0}",
                                &[&error],
                            ))
                        }
                    }
                }
                return Ok(());
            }
            for (key, input, apply, default) in &self.user_inputs {
                if clicked == *apply || clicked == *default {
                    let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                        return Ok(());
                    };
                    let value = if clicked == *default {
                        sse_fixes::toolkit::MANAGED_SETTINGS
                            .iter()
                            .find(|item| item.key == key)
                            .map_or("", |item| item.default_val)
                            .to_owned()
                    } else {
                        cx.tree.input_text(*input)?.trim().to_owned()
                    };
                    let mut update = std::collections::BTreeMap::new();
                    update.insert(key.clone(), value);
                    match sse_fixes::toolkit::ManagedUserLtxSettings::update_managed_settings(&directory, &update) {
                        Ok(_) if clicked == *default => {
                            cx.status = Some(tr(
                                crate::strings::current_language(),
                                "Восстановлено значение игры: {0}",
                                &[key],
                            ));
                        }
                        Ok(_) => {
                            cx.status = Some(tr(
                                crate::strings::current_language(),
                                "Настройка обновлена: {0}",
                                &[key],
                            ))
                        }
                        Err(error) if clicked == *default => {
                            cx.status = Some(tr(
                                crate::strings::current_language(),
                                "Не удалось восстановить настройку: {0}",
                                &[&error],
                            ))
                        }
                        Err(error) => {
                            cx.status = Some(tr(
                                crate::strings::current_language(),
                                "Не удалось изменить настройку: {0}",
                                &[&error],
                            ))
                        }
                    }
                    return Ok(());
                }
            }
        }
        if clicked.is_some() && clicked == self.restore {
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let game = cx.app.selected_game().and_then(fix_target);
            let Some(directory) = directory else {
                cx.status = Some(crate::strings::t("Управляемая установка: не выбрана").to_owned());
                return Ok(());
            };
            let Some(snapshot_id) = self.selected_snapshot.clone() else {
                cx.status = Some(crate::strings::t("Сначала выберите снимок.").to_owned());
                return Ok(());
            };
            let snapshot = sse_fixes::toolkit::ToolkitSnapshotService::get_snapshot(&directory, &snapshot_id)
                .map_err(|e| sse_core::Error::Refused(e.to_string()))?;
            if self.pending_restore.as_deref() != Some(snapshot.id.as_str()) {
                self.pending_restore = Some(snapshot.id.clone());
                cx.status = Some(tr(
                    crate::strings::current_language(),
                    "ВОССТАНОВЛЕНИЕ ИЗМЕНИТ ФАЙЛЫ ИГРЫ. Нажмите «ВОССТАНОВИТЬ ПОСЛЕДНИЙ СНИМОК» ещё раз для подтверждения: {0}",
                    &[&snapshot.label],
                ));
                return Ok(());
            }
            self.pending_restore = None;
            let Some(game) = game else {
                cx.status = Some(crate::strings::t("Игра не поддерживается Toolkit.").to_owned());
                return Ok(());
            };
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let snapshot_id = snapshot.id.clone();
            let language = crate::strings::current_language().to_owned();
            sse_app::tasks::spawn_named_detached("game-write", move || {
                let engine = sse_fixes::GameFixEngine::new();
                let catalog = sse_fixes::GameFixCatalog;
                let lines = sse_fixes::toolkit::ToolkitSnapshotService::restore_snapshot(
                    &directory,
                    &engine,
                    &catalog,
                    &snapshot_id,
                )
                .map(|r| {
                    vec![tr(
                        &language,
                        "Восстановлен снимок {0} · установлено фиксов {1} · удалено {2} · user.ltx {3}",
                        &[
                            &r.snapshot_id,
                            &r.installed_fixes.len(),
                            &r.uninstalled_fixes.len(),
                            &r.user_ltx_updates_count,
                        ],
                    )]
                })
                .unwrap_or_else(|error| vec![tr(&language, "Ошибка восстановления: {0}", &[&error])]);
                let _ = game;
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Environment,
                    Box::new(EnvironmentResult { lines }),
                ));
            });
            return Ok(());
        }
        if clicked.is_some() && (clicked == self.snapshot || clicked == self.audit) {
            let create_snapshot = clicked.is_some() && clicked == self.snapshot;
            let game = cx.app.selected_game().and_then(fix_target);
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let language = crate::strings::current_language().to_owned();
            sse_app::tasks::spawn_named_detached(if create_snapshot { "game-write" } else { "game-read" }, move || {
                let result = (|| {
                    let game =
                        game.ok_or_else(|| crate::strings::t_in(&language, "Выберите поддерживаемую игру").to_owned())?;
                    let directory = directory.ok_or_else(|| {
                        crate::strings::t_in(&language, "Управляемая установка: не выбрана").to_owned()
                    })?;
                    let engine = sse_fixes::GameFixEngine::new();
                    if create_snapshot {
                        if !game.is_xray() {
                            return Err(
                                crate::strings::t_in(&language, "Снимки доступны только для X-Ray игр.").to_owned()
                            );
                        }
                        let snap = sse_fixes::toolkit::ToolkitSnapshotService::create_snapshot(
                            &directory, game, &engine, None,
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(vec![
                            tr(&language, "Создан снимок: {0}", &[&snap.id]),
                            tr(
                                &language,
                                "исправлений: {0} · Companion: {1}",
                                &[
                                    &snap.installed_fixes.len(),
                                    &crate::strings::t_in(
                                        &language,
                                        if snap.companion_installed {
                                            "включён"
                                        } else {
                                            "выключен"
                                        },
                                    ),
                                ],
                            ),
                        ])
                    } else {
                        let report =
                            sse_fixes::toolkit::ToolkitInstallAudit::audit_installation(&directory, game, &engine)
                                .map_err(|e| e.to_string())?;
                        let mut lines = vec![tr(
                            &language,
                            "Проверено файлов: {0}. Управляемых: {1}. Неизвестных/модов: {2}. Требуют проверки: {3}.",
                            &[
                                &report.total_scanned,
                                &report.managed_count,
                                &report.custom_mod_count,
                                &report.needs_review_count,
                            ],
                        )];
                        lines.extend(report.items.into_iter().take(8).map(|item| {
                            format!("{:?} · {} · {}", item.classification, item.relative_path, item.details)
                        }));
                        Ok(lines)
                    }
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::Environment,
                    Box::new(EnvironmentResult {
                        lines: result.unwrap_or_else(|error| vec![tr(&language, "Ошибка: {0}", &[&error])]),
                    }),
                ));
            });
            return Ok(());
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Environment, payload)) = message {
            if let Some(result) = payload.downcast_ref::<EnvironmentResult>() {
                if let Some(status) = self.status {
                    cx.tree
                        .set_text(status, crate::strings::t("Только чтение · проверка завершена"))?;
                }
                for (index, widget) in self.lines.iter().copied().enumerate() {
                    if let Some(line) = result.lines.get(index) {
                        cx.tree.set_visible(widget, true)?;
                        cx.tree.set_text(widget, line)?;
                    } else {
                        cx.tree.set_visible(widget, false)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// State of a fix in the list, as the badge shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixBadge {
    Installed,
    Update,
    Changed,
    NotInstalled,
}

#[derive(Clone, Debug)]
struct FixRow {
    badge: FixBadge,
    id: String,
    title: String,
    description: String,
    version: String,
    status: String,
    category: String,
    maturity: String,
    problem: String,
    source: String,
    builds: String,
    files: usize,
}

#[derive(Debug)]
enum FixReply {
    List {
        request_id: u64,
        game_id: Option<String>,
        directory: Option<PathBuf>,
        selected_fix: Option<String>,
        result: std::result::Result<Vec<FixRow>, String>,
    },
    Changed(std::result::Result<String, String>),
    Compatibility(std::result::Result<(String, String, PathBuf, Option<String>), String>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FixOperation {
    Install,
    Remove,
    Preset(sse_fixes::GameFixPreset),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixIntent {
    fix_id: Option<String>,
    operation: FixOperation,
    game: String,
    directory: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FixCompatibility {
    game: String,
    directory: PathBuf,
    build: Option<String>,
}

/// One row of the fix list: a card with the title, the id and version, and the state badge.
#[derive(Clone, Copy)]
struct FixListRow {
    stack: WidgetId,
    card: WidgetId,
    title: WidgetId,
    meta: WidgetId,
    badge: WidgetId,
}

/// Height of a fix row in the list (the frame's 56 px, with the card's 4 px of padding above and below).
const FIX_ROW_HEIGHT: f32 = 56.0;

/// Style of a column of fixed width: a panel's children with a gap and the panel padding.
fn fixed_column_style(width: f32, gap: f32, padding: f32) -> Style {
    Style {
        preferred: crate::layout::Size::new(width, 0.0),
        min: crate::layout::Size::new(width, 0.0),
        max: crate::layout::Size::new(width, f32::INFINITY),
        shrink: 0.0,
        ..panel_style(gap, padding)
    }
}

/// Style of a panel's column: a gap between its children and the panel padding.
fn panel_style(gap: f32, padding: f32) -> Style {
    Style {
        shrink: 0.0,
        padding: crate::layout::Edges::all(padding),
        gap: crate::layout::Size::new(0.0, gap),
        align_items: crate::layout::Align::Stretch,
        ..Style::default()
    }
}

/// A child that takes the room it can (heading beside a button).
fn grow_style() -> Style {
    Style {
        grow: 1.0,
        shrink: 0.0,
        ..Style::default()
    }
}

/// Sentence case for a note written in capitals: its words in lower case (a path keeps its case), the first letter up.
pub(super) fn sentence_case(text: &str) -> String {
    let words: Vec<String> = text
        .split(' ')
        .map(|word| {
            let capitals = word.chars().any(char::is_alphabetic)
                && word.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
                && !word.contains('/')
                && !word.contains('\\');
            if capitals {
                word.to_lowercase()
            } else {
                word.to_owned()
            }
        })
        .collect();
    // Each sentence starts with a capital letter.
    words
        .join(" ")
        .split(". ")
        .map(|sentence| {
            let mut chars = sentence.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(". ")
}

/// Look of a state badge in the fix list: the state's colours from the theme.
fn badge_look(kind: FixBadge) -> Look {
    let colours = match kind {
        FixBadge::Installed => theme::d2::BADGE_INSTALLED,
        FixBadge::Update => theme::d2::BADGE_UPDATE,
        FixBadge::Changed => theme::d2::BADGE_CHANGED,
        FixBadge::NotInstalled => theme::d2::BADGE_NOT_INSTALLED,
    };
    Look {
        fill: Some(style::d2::argb(colours.fill)),
        border: Some((style::d2::argb(colours.border), 1.0)),
        text: style::d2::argb(colours.text),
        radius: 2.0,
        ..Look::default()
    }
}

/// Sets the width a wrapped text takes, so that its lines are measured at the width it is drawn in.
fn set_wrap_width(tree: &mut Tree, id: WidgetId, width: f32) -> Result<()> {
    tree.set_style(
        id,
        Style {
            min: crate::layout::Size::new(width.max(1.0), 0.0),
            shrink: 0.0,
            ..Style::default()
        },
    )
}

/// A multi-line text, wrapped to the width it is drawn in; the caller passes the translated text.
fn paragraph_leaf(tree: &mut Tree, parent: WidgetId, text: &str, role: Text) -> Result<WidgetId> {
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
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
    )
}

#[derive(Default)]
struct GameFixes {
    compact: bool,
    details_scroll: Option<WidgetId>,
    details_title: Option<WidgetId>,
    details_key: Option<WidgetId>,
    details_badge: Option<WidgetId>,
    details_thumb: Option<WidgetId>,
    details_offset: i32,
    right_column: Option<WidgetId>,
    top_card: Option<WidgetId>,
    categories: Option<WidgetId>,
    list_card: Option<WidgetId>,
    list_rows: Vec<FixListRow>,
    list_pager: Option<WidgetId>,
    list_previous: Option<WidgetId>,
    list_range: Option<WidgetId>,
    list_next: Option<WidgetId>,
    page: usize,
    page_size: usize,
    details_card: Option<WidgetId>,
    bottom_card: Option<WidgetId>,
    write_note: Option<WidgetId>,
    status: Option<WidgetId>,
    detail: Option<WidgetId>,
    rows: Vec<WidgetId>,
    list_scroll: Option<WidgetId>,
    scroll_y: i32,
    items: Vec<FixRow>,
    selected: Option<String>,
    list_request_id: u64,
    requested_fix: Option<String>,
    install: Option<WidgetId>,
    remove: Option<WidgetId>,
    check: Option<WidgetId>,
    preset_recommended: Option<WidgetId>,
    preset_essential: Option<WidgetId>,
    preset_safe: Option<WidgetId>,
    compatibility: Option<WidgetId>,
    confirm_card: Option<WidgetId>,
    confirm_write: Option<WidgetId>,
    confirm_cancel: Option<WidgetId>,
    intent: Option<FixIntent>,
    verified: Option<FixCompatibility>,
    busy: bool,
}

fn fix_target(game: &str) -> Option<sse_fixes::GameTarget> {
    sse_fixes::GameTarget::parse(game).or(match game {
        "stalker-soc" => Some(sse_fixes::GameTarget::ShadowOfChernobyl),
        "stalker-cs" | "clear_sky" => Some(sse_fixes::GameTarget::ClearSky),
        "stalker-cop" => Some(sse_fixes::GameTarget::CallOfPripyat),
        "stalker-soc-ee" => Some(sse_fixes::GameTarget::ShadowOfChernobylEnhancedEdition),
        "stalker-cs-ee" => Some(sse_fixes::GameTarget::ClearSkyEnhancedEdition),
        "stalker-cop-ee" => Some(sse_fixes::GameTarget::CallOfPripyatEnhancedEdition),
        "stalker2" => Some(sse_fixes::GameTarget::Stalker2),
        _ => None,
    })
}

fn matching_game_installation(
    target: sse_fixes::GameTarget,
    current_directory: Option<&Path>,
    installations: &[DiscoveredInstallation],
) -> Option<PathBuf> {
    if let Some(directory) = current_directory {
        if sse_fixes::identify_game(target, directory).0 {
            return Some(directory.to_path_buf());
        }
    }

    installations
        .iter()
        .filter(|installation| fix_target(installation.target.id()) == Some(target))
        .find(|installation| sse_fixes::identify_game(target, &installation.directory).0)
        .map(|installation| installation.directory.clone())
}

fn load_fix_rows(
    target: sse_fixes::GameTarget,
    directory: Option<&Path>,
    language: &str,
) -> std::result::Result<Vec<FixRow>, String> {
    let engine = sse_fixes::GameFixEngine::new();
    let installed = match directory {
        Some(directory) => {
            engine
                .recover_interrupted(directory)
                .map_err(|error| error.to_string())?;
            engine
                .list_installed(directory, None)
                .map_err(|error| error.to_string())?
        }
        None => Vec::new(),
    };

    Ok(sse_fixes::GameFixCatalog::for_game(target)
        .into_iter()
        .map(|definition| {
            let current = installed.iter().find(|item| item.id == definition.id);
            let (status, badge) = if let Some(item) = current {
                if item.version != definition.version {
                    (
                        crate::strings::t_in(language, "ОБНОВЛЕНИЕ ДОСТУПНО").to_owned(),
                        FixBadge::Update,
                    )
                } else if let Some(directory) = directory {
                    match engine.get_status(definition, directory) {
                        Ok(sse_fixes::GameFixState::Installed) => (
                            crate::strings::t_in(language, "УСТАНОВЛЕНО").to_owned(),
                            FixBadge::Installed,
                        ),
                        Ok(sse_fixes::GameFixState::Modified) => (
                            crate::strings::t_in(language, "ФАЙЛ ИЗМЕНЁН ПОСЛЕ УСТАНОВКИ").to_owned(),
                            FixBadge::Changed,
                        ),
                        Ok(_) => (
                            crate::strings::t_in(language, "НЕ УСТАНОВЛЕНО").to_owned(),
                            FixBadge::NotInstalled,
                        ),
                        Err(error) => (tr(language, "ОШИБКА: {0}", &[&error]), FixBadge::Changed),
                    }
                } else {
                    (
                        crate::strings::t_in(language, "НЕ УСТАНОВЛЕНО").to_owned(),
                        FixBadge::NotInstalled,
                    )
                }
            } else {
                (
                    crate::strings::t_in(language, "НЕ УСТАНОВЛЕНО").to_owned(),
                    FixBadge::NotInstalled,
                )
            };
            FixRow {
                badge,
                id: definition.id.clone(),
                title: definition.title.clone(),
                description: definition.description.clone(),
                version: definition.version.clone(),
                status,
                category: definition.category.as_str().to_owned(),
                maturity: definition.maturity.as_str().to_owned(),
                problem: definition.problem.clone(),
                source: definition.source.clone(),
                builds: definition.supported_steam_build_ids.join(", "),
                files: definition
                    .text_patches
                    .len()
                    .saturating_add(definition.overlays.len())
                    .saturating_add(definition.spawn_edits.len()),
            }
        })
        .collect())
}

impl GameFixes {
    fn refresh(&mut self, cx: &mut Context<'_>) {
        self.load_fix_list(cx, None);
    }

    fn load_fix_list(&mut self, cx: &mut Context<'_>, selected_fix: Option<String>) {
        self.list_request_id = self.list_request_id.saturating_add(1);
        let request_id = self.list_request_id;
        self.requested_fix = selected_fix.clone();
        let game = cx.app.selected_game().map(str::to_owned);
        let directory = cx.app.game_dir().map(Path::to_path_buf);
        let Some(proxy) = cx.proxy.cloned() else { return };
        let language = crate::strings::current_language().to_owned();
        sse_app::tasks::spawn_named_detached("game-read", move || {
            let result = (|| {
                let target = game.as_deref().and_then(fix_target).ok_or_else(|| {
                    crate::strings::t_in(&language, "Выбранная игра не поддерживается каталогом фиксов").to_owned()
                })?;
                if selected_fix
                    .as_deref()
                    .and_then(sse_fixes::GameFixCatalog::try_get)
                    .is_some_and(|definition| definition.game != target)
                {
                    return Err(
                        crate::strings::t_in(&language, "Связанное исправление не относится к выбранной игре")
                            .to_owned(),
                    );
                }
                let directory = directory
                    .filter(|path| sse_fixes::identify_game(target, path).0)
                    .or_else(|| matching_game_installation(target, None, &discover_game_installations()));
                let rows = load_fix_rows(target, directory.as_deref(), &language)?;
                Ok((directory, rows))
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::GameFixes,
                Box::new(FixReply::List {
                    request_id,
                    game_id: game,
                    directory: result.as_ref().ok().and_then(|(directory, _)| directory.clone()),
                    selected_fix,
                    result: result.map(|(_, rows)| rows),
                }),
            ));
        });
    }

    fn show_selection(&self, cx: &mut Context<'_>) -> Result<()> {
        let Some(detail) = self.detail else { return Ok(()) };
        let language = crate::strings::current_language();
        let text = self
            .selected
            .as_deref()
            .and_then(|id| self.items.iter().find(|item| item.id == id))
            .map(|item| {
                let status = crate::strings::t_in(language, &item.status);
                let category = crate::strings::t_in(language, &item.category);
                let maturity = crate::strings::t_in(language, &item.maturity);
                let problem = crate::strings::t_in(language, &item.problem);
                let description = crate::strings::t_in(language, &item.description);
                let source = crate::strings::t_in(language, &item.source);
                tr(
                    language,
                    "{0} · {1} · {2} / {3}\nПРОБЛЕМА: {4}\nИЗМЕНЕНИЕ: {5}\nПОДДЕРЖИВАЕМЫЕ STEAM-СБОРКИ: {6}\nЗАТРАГИВАЕМЫЕ ФАЙЛЫ: {7}\nИСТОЧНИК: {8}",
                    &[
                        &item.id,
                        &status,
                        &category,
                        &maturity,
                        &problem,
                        &description,
                        &item.builds,
                        &item.files,
                        &source,
                    ],
                )
            })
            .or_else(|| {
                self.requested_fix
                    .as_deref()
                    .and_then(sse_fixes::GameFixCatalog::try_get)
                    .map(|definition| {
                        format!(
                            "{} · {}\n{}",
                            definition.id,
                            crate::strings::t_in(language, &definition.title),
                            crate::strings::t_in(language, &definition.description)
                        )
                    })
            })
            .unwrap_or_else(|| crate::strings::t_in(language, "ВЫБЕРИТЕ ИСПРАВЛЕНИЕ").to_owned());
        // The first line of the text is the fix's title and key; the rest is its body.
        let (head, body) = text.split_once('\n').unwrap_or((text.as_str(), ""));
        let item = self
            .selected
            .as_deref()
            .and_then(|id| self.items.iter().find(|item| item.id == id));
        if let Some(item) = item {
            if let Some(title) = self.details_title {
                // The title is one line: it is shortened with an ellipsis to the card's inner width.
                let details_width = if cx.tree.size().0 < 1600 { 340.0 } else { 440.0 };
                let width = details_width - 2.0 * theme::d2::PANEL_PADDING.0;
                let heading = crate::strings::t_in(language, &item.title).to_uppercase();
                let heading = {
                    let metrics = cx.tree.fonts().metrics(Text::Heading.style());
                    text::ellipsize_end(&heading, width, &metrics)
                };
                cx.tree.set_text(title, &heading)?;
            }
            if let Some(key) = self.details_key {
                cx.tree.set_text(key, &item.id)?;
            }
            if let Some(badge) = self.details_badge {
                cx.tree.set_text(badge, crate::strings::t_in(language, &item.status))?;
                cx.tree.set_look(badge, badge_look(item.badge))?;
            }
            cx.tree.set_text(detail, body)
        } else {
            for id in [self.details_title, self.details_key].into_iter().flatten() {
                cx.tree.set_text(id, "")?;
            }
            cx.tree.set_text(detail, head)
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let language = crate::strings::current_language();
        let window_width = cx.tree.size().0;
        if window_width > 0 {
            self.compact = window_width < 1600;
        }
        let left_width = if self.compact { 240.0 } else { 280.0 };
        let details_width = if self.compact { 340.0 } else { 440.0 };
        // The columns' widths follow the window (they are set when the window's size is known).
        let gap = if self.compact { 12.0 } else { 16.0 };
        if let Some(left) = self.top_card {
            cx.tree.set_style(left, fixed_column_style(left_width, 10.0, 16.0))?;
        }
        if let Some(right) = self.right_column {
            cx.tree.set_style(
                right,
                Style {
                    shrink: 0.0,
                    gap: crate::layout::Size::new(0.0, gap),
                    align_items: crate::layout::Align::Stretch,
                    preferred: crate::layout::Size::new(details_width, 0.0),
                    min: crate::layout::Size::new(details_width, 0.0),
                    max: crate::layout::Size::new(details_width, f32::INFINITY),
                    ..Style::default()
                },
            )?;
        }
        // The catalog's count is the pager's range; without fixes the status line says why there are none.
        let total = self.items.len();
        if let Some(status) = self.status {
            cx.tree.set_visible(status, total == 0)?;
            cx.tree.set_text(
                status,
                crate::strings::t_in(language, "НЕТ ПРОВЕРЕННЫХ ИСПРАВЛЕНИЙ ДЛЯ ЭТОЙ ВЕРСИИ."),
            )?;
        }
        if let Some(pager) = self.list_pager {
            cx.tree.set_visible(pager, total > 0)?;
        }
        // Wrapped texts take the width they are drawn in (set before the list's room is measured).
        for id in [self.compatibility, self.status, self.categories].into_iter().flatten() {
            set_wrap_width(cx.tree, id, left_width - 2.0 * theme::d2::PANEL_PADDING.0)?;
        }
        for id in [self.detail, self.write_note].into_iter().flatten() {
            set_wrap_width(cx.tree, id, details_width - 2.0 * theme::d2::PANEL_PADDING.0)?;
        }
        // The list's room comes from the window; the page is what fits in it.
        cx.tree.update_layout()?;
        let capacity = self.sync_list_cap(cx)?;
        self.page_size = self.rows_that_fit(cx.tree, capacity);
        // The presets' card's height is settled by one pass with the new cap; the second pass uses it.
        self.sync_details_cap(cx)?;
        cx.tree.update_layout()?;
        self.sync_details_cap(cx)?;
        let pages = total
            .saturating_add(self.page_size.saturating_sub(1))
            .checked_div(self.page_size)
            .unwrap_or(0);
        if self.page >= pages {
            self.page = 0;
        }
        let start = self.page.saturating_mul(self.page_size);

        // Rows of the page: title, id and version, and the state badge; the selected fix has the accent border.
        let row_width = self.row_text_width(cx.tree);
        for (index, row) in self.list_rows.clone().into_iter().enumerate() {
            let shown = if index < self.page_size {
                self.items.get(start.saturating_add(index))
            } else {
                None
            };
            let Some(item) = shown else {
                cx.tree.set_visible(row.stack, false)?;
                continue;
            };
            cx.tree.set_visible(row.stack, true)?;
            let title = crate::strings::t_in(language, &item.title).to_owned();
            let meta = format!("{} · v{}", item.id, item.version);
            let (title, meta) = {
                let metrics = cx.tree.fonts().metrics(Text::Body.style());
                let title = text::ellipsize_end(&title, row_width, &metrics);
                let metrics = cx.tree.fonts().metrics(Text::Note.style());
                (title, text::ellipsize_end(&meta, row_width, &metrics))
            };
            cx.tree.set_text(row.title, &title)?;
            cx.tree.set_text(row.meta, &meta)?;
            let status = crate::strings::t_in(language, &item.status).to_owned();
            cx.tree.set_text(row.badge, &status)?;
            cx.tree.set_look(row.badge, badge_look(item.badge))?;
            let selected = self.selected.as_deref() == Some(item.id.as_str());
            let border = if selected {
                theme::d2::ACCENT
            } else {
                theme::d2::BORDER_SUBTLE
            };
            let fill = if selected {
                theme::d2::ACCENT_TINT
            } else {
                theme::d2::PANEL_RAISED
            };
            cx.tree.set_look(
                row.card,
                Look {
                    fill: Some(style::d2::argb(fill)),
                    border: Some((style::d2::argb(border), 1.0)),
                    radius: 3.0,
                    ..Look::default()
                },
            )?;
        }
        // Pager: arrows and the range shown, as in the library.
        if let Some(id) = self.list_previous {
            cx.tree.set_enabled(id, self.page > 0)?;
        }
        if let Some(id) = self.list_next {
            cx.tree.set_enabled(id, self.page.saturating_add(1) < pages)?;
        }
        if let Some(id) = self.list_range {
            let text = if total == 0 {
                String::new()
            } else {
                let last = start.saturating_add(self.page_size).min(total);
                tr(language, "{0}–{1} из {2}", &[&start.saturating_add(1), &last, &total])
            };
            cx.tree.set_text(id, &text)?;
        }
        self.sync_details_thumb(cx)?;
        // The left block's notes are sentence case, as the other explanations; their paths keep their case.
        for id in [self.compatibility, self.categories].into_iter().flatten() {
            let text = cx.tree.text(id)?.to_owned();
            let next = sentence_case(&text);
            if next != text {
                cx.tree.set_text(id, &next)?;
            }
        }
        // The actions need a selected fix: without one they are off.
        let has_selection = self.selected.is_some();
        for id in [self.install, self.remove].into_iter().flatten() {
            cx.tree.set_enabled(id, has_selection)?;
        }
        Ok(())
    }

    /// Width a row's text may take: the list card less the list padding, the row's padding and the badge.
    fn row_text_width(&self, tree: &crate::widget::Tree) -> f32 {
        let list = self
            .list_card
            .and_then(|id| tree.rect(id).ok())
            .map_or(0.0, |rect| f32::from(u16::try_from(rect.width).unwrap_or(0)));
        (list - 2.0 * 6.0 - 2.0 * 12.0 - 12.0 - 96.0).max(1.0)
    }

    /// Caps the list at the room the list card has: the card's other parts are measured, the window is the limit.
    fn sync_list_cap(&self, cx: &mut Context<'_>) -> Result<f32> {
        let (Some(scroll), Some(card)) = (self.list_scroll, self.list_card) else {
            return Ok(0.0);
        };
        let window_height = f32::from(u16::try_from(cx.tree.size().1).unwrap_or(u16::MAX));
        let card_top = f32::from(u16::try_from(cx.tree.rect(card)?.y.max(0)).unwrap_or(0));
        let mut others = 0.0_f32;
        for child in cx.tree.children(card) {
            if child != scroll {
                others += f32::from(u16::try_from(cx.tree.rect(child)?.height).unwrap_or(0));
            }
        }
        // The shell keeps 12 px below the content and a 30 px status bar under the window; the card's borders take 4.
        let cap = (window_height - card_top - 42.0 - others - 4.0).max(0.0);
        cx.tree.set_style(
            scroll,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                max: crate::layout::Size::new(f32::INFINITY, cap),
                ..Style::default()
            },
        )?;
        // The new cap changes the layout of the card and of the parts under it: settle it before they are read.
        cx.tree.update_layout()?;
        Ok(cap)
    }

    /// How far the details' text can scroll: its height less the height of the area it shows (0 when it fits).
    fn details_limit(&self, tree: &mut crate::widget::Tree) -> Result<f32> {
        let Some(scroll) = self.details_scroll else {
            return Ok(0.0);
        };
        let viewport = f32::from(u16::try_from(tree.rect(scroll)?.height).unwrap_or(0));
        let content = tree.content_height(scroll)?;
        Ok((content - viewport).max(0.0))
    }

    /// Sizes and places the thumb of the details scroll: it shows where the text is, and only when the text overflows.
    fn sync_details_thumb(&self, cx: &mut Context<'_>) -> Result<()> {
        let (Some(scroll), Some(thumb)) = (self.details_scroll, self.details_thumb) else {
            return Ok(());
        };
        let limit = self.details_limit(cx.tree)?;
        if limit <= 0.0 {
            return cx.tree.set_visible(thumb, false);
        }
        let viewport = f32::from(u16::try_from(cx.tree.rect(scroll)?.height).unwrap_or(0));
        let content = viewport + limit;
        let thumb_height = (viewport * viewport / content).clamp(24.0, viewport.max(24.0));
        let offset = (self.details_offset as f32).clamp(0.0, limit);
        let top = offset / limit * (viewport - thumb_height).max(0.0);
        cx.tree.set_style(
            thumb,
            Style {
                shrink: 0.0,
                min: crate::layout::Size::new(8.0, thumb_height),
                preferred: crate::layout::Size::new(8.0, thumb_height),
                margin: crate::layout::Edges {
                    top,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;
        cx.tree.set_visible(thumb, true)
    }

    /// Caps the details' scroll at the room the right column has below the details card's top: the presets' card and
    /// the card's other parts are measured; the window is the limit.
    fn sync_details_cap(&self, cx: &mut Context<'_>) -> Result<()> {
        let (Some(scroll), Some(details)) = (self.details_scroll, self.details_card) else {
            return Ok(());
        };
        let window_height = f32::from(u16::try_from(cx.tree.size().1).unwrap_or(u16::MAX));
        let details_top = f32::from(u16::try_from(cx.tree.rect(details)?.y.max(0)).unwrap_or(0));
        let mut below = 0.0_f32;
        if let Some(apply) = self.bottom_card {
            below += f32::from(u16::try_from(cx.tree.rect(apply)?.height).unwrap_or(0));
            below += if self.compact { 12.0 } else { 16.0 };
        }
        // The card's children besides the scroll, its gaps, its padding and its borders take room too.
        let children = cx.tree.children(details);
        let mut others = 0.0_f32;
        for child in &children {
            if *child != scroll {
                others += f32::from(u16::try_from(cx.tree.rect(*child)?.height).unwrap_or(0));
            }
        }
        let gaps = f32::from(u16::try_from(children.len().saturating_sub(1)).unwrap_or(0)) * 12.0;
        let padding = 2.0 * theme::d2::PANEL_PADDING.0 + 2.0;
        let cap = (window_height - details_top - 42.0 - below - others - gaps - padding).max(0.0);
        cx.tree.set_style(
            scroll,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                max: crate::layout::Size::new(f32::INFINITY, cap),
                ..Style::default()
            },
        )
    }

    /// Rows that fit in a list of `capacity` pixels, from the measured row height.
    fn rows_that_fit(&self, tree: &crate::widget::Tree, capacity: f32) -> usize {
        let measured = self
            .list_rows
            .first()
            .and_then(|row| tree.rect(row.stack).ok())
            .map_or(0.0, |rect| f32::from(u16::try_from(rect.height).unwrap_or(0)));
        let row_height = if measured > 0.0 { measured } else { 56.0 };
        let mut rows = 0_usize;
        while rows < self.list_rows.len() {
            let next = f32::from(u16::try_from(rows.saturating_add(1)).unwrap_or(u16::MAX));
            if next * row_height + (next - 1.0) * 4.0 + 12.0 > capacity {
                break;
            }
            rows = rows.saturating_add(1);
        }
        rows.max(1)
    }

    fn select_item(&mut self, cx: &mut Context<'_>, fix_id: &str) -> Result<()> {
        if !self.items.iter().any(|item| item.id == fix_id) {
            self.selected = None;
            return Ok(());
        }
        self.selected = Some(fix_id.to_owned());
        self.details_offset = 0;
        if let Some(scroll) = self.details_scroll {
            cx.tree.set_scroll_y(scroll, 0)?;
        }
        self.intent = None;
        if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
            let _ = cx.tree.close_dialog()?;
        }
        self.render(cx)?;
        self.show_selection(cx)
    }

    fn open_linked_fix(&mut self, cx: &mut Context<'_>, game_id: &str, fix_id: &str) -> Result<()> {
        let Some(definition) = sse_fixes::GameFixCatalog::try_get(fix_id) else {
            cx.status = Some(crate::strings::t("Связанное исправление отсутствует в каталоге.").to_owned());
            return Ok(());
        };
        if definition.game.id() != game_id {
            cx.status = Some(crate::strings::t("Связанное исправление не относится к выбранной игре.").to_owned());
            return Ok(());
        }
        if cx.app.selected_game() != Some(game_id) {
            cx.app.set_game_dir(None);
        }
        cx.app.set_selected_game(Some(game_id.to_owned()));
        self.verified = None;
        self.intent = None;
        self.items.clear();
        self.selected = Some(fix_id.to_owned());
        self.requested_fix = Some(fix_id.to_owned());
        self.render(cx)?;
        self.show_selection(cx)?;
        if let Some(compatibility) = self.compatibility {
            cx.tree
                .set_text(compatibility, crate::strings::t("ПОИСК УСТАНОВКИ ИГРЫ…"))?;
        }
        self.load_fix_list(cx, Some(fix_id.to_owned()));
        Ok(())
    }
}

impl Screen for GameFixes {
    fn id(&self) -> ScreenId {
        ScreenId::GameFixes
    }
    fn subtitle(&self) -> &str {
        crate::strings::t("Каталог исправлений выбранной игры с транзакционной установкой")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        // Three columns as in the frame: the game and its check on the left, the fix list in the middle, the selected
        // fix with its actions and the presets on the right. The window's height is the list's room.
        let window_width = cx.tree.size().0;
        if window_width > 0 {
            self.compact = window_width < 1600;
        }
        let gap = if self.compact { 12.0 } else { 16.0 };
        let left_width = if self.compact { 240.0 } else { 280.0 };
        let details_width = if self.compact { 340.0 } else { 440.0 };
        let main = cx.tree.add(
            Some(host),
            NodeKind::Row,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(gap, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;

        // --- LEFT: the game's installation and its check ---
        let left = style::d2::panel(cx.tree, main)?;
        cx.tree.set_style(left, fixed_column_style(left_width, 10.0, 16.0))?;
        self.top_card = Some(left);
        style::d2::panel_title(cx.tree, left, crate::strings::t("ИСПРАВЛЕНИЯ ИГРЫ"))?;
        self.check = Some(style::d2::button(
            cx.tree,
            left,
            crate::strings::t("ПРОВЕРИТЬ СОВМЕСТИМОСТЬ"),
            style::d2::ButtonKind::Outline,
            style::d2::ButtonSize::Normal,
        )?);
        self.compatibility = Some(paragraph_leaf(
            cx.tree,
            left,
            crate::strings::t("СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ."),
            Text::Note,
        )?);
        // The catalog's count is the pager's range; without fixes this line says why there are none.
        self.status = Some(paragraph_leaf(
            cx.tree,
            left,
            crate::strings::t("Выберите игру"),
            Text::Note,
        )?);
        self.categories = Some(paragraph_leaf(
            cx.tree,
            left,
            crate::strings::t(
                "КАТЕГОРИИ: ОБЯЗАТЕЛЬНЫЕ · РЕКОМЕНДУЕМЫЕ · НЕОБЯЗАТЕЛЬНЫЕ · СООБЩЕСТВО · ЭКСПЕРИМЕНТАЛЬНЫЕ",
            ),
            Text::Note,
        )?);

        // --- MIDDLE: the fix list with its pager ---
        let list_card = style::d2::panel(cx.tree, main)?;
        cx.tree.set_style(
            list_card,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        cx.tree.set_clip_children(list_card, true)?;
        self.list_card = Some(list_card);
        let list_head = cx.tree.add(
            Some(list_card),
            NodeKind::Row,
            Style {
                min: crate::layout::Size::new(0.0, 44.0),
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 0.0,
                    right: 16.0,
                    bottom: 0.0,
                },
                align_items: crate::layout::Align::Center,
                shrink: 0.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let list_title = style::d2::panel_title(cx.tree, list_head, crate::strings::t("ДОСТУПНЫЕ ИСПРАВЛЕНИЯ"))?;
        cx.tree.set_style(list_title, grow_style())?;
        let scroll = cx.tree.add(
            Some(list_card),
            NodeKind::Scroll {
                horizontal: false,
                vertical: true,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_clip_children(scroll, true)?;
        self.list_scroll = Some(scroll);
        let list = cx.tree.add(
            Some(scroll),
            NodeKind::Column,
            Style {
                padding: crate::layout::Edges::all(6.0),
                gap: crate::layout::Size::new(0.0, 4.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for _ in 0..sse_fixes::GameFixCatalog::all().len().max(1) {
            let stack = cx.tree.add(
                Some(list),
                NodeKind::Stack,
                Style {
                    shrink: 0.0,
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let card = cx.tree.add(
                Some(stack),
                NodeKind::Row,
                Style {
                    min: crate::layout::Size::new(0.0, FIX_ROW_HEIGHT - 8.0),
                    padding: crate::layout::Edges {
                        left: 12.0,
                        top: 4.0,
                        right: 12.0,
                        bottom: 4.0,
                    },
                    gap: crate::layout::Size::new(12.0, 0.0),
                    align_items: crate::layout::Align::Center,
                    shrink: 0.0,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let text_column = cx.tree.add(
                Some(card),
                NodeKind::Column,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, 0.0),
                    gap: crate::layout::Size::new(0.0, 2.0),
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let title = style::label(cx.tree, text_column, "", Text::Body)?;
            let meta = style::label(cx.tree, text_column, "", Text::Note)?;
            let badge = style::d2::badge(
                cx.tree,
                card,
                crate::strings::t("НЕ УСТАНОВЛЕНО"),
                style::d2::BadgeKind::NotInstalled,
            )?;
            // The select button is last, so it covers the card and takes the clicks.
            let select = style::button(cx.tree, stack, "", Button::Secondary)?;
            cx.tree.set_look(select, Look::default())?;
            cx.tree.set_visible(stack, false)?;
            self.rows.push(select);
            self.list_rows.push(FixListRow {
                stack,
                card,
                title,
                meta,
                badge,
            });
        }
        // Pager: arrows and the range, as in the library; 12 px above the card's bottom edge.
        let pager = cx.tree.add(
            Some(list_card),
            NodeKind::Row,
            Style {
                min: crate::layout::Size::new(0.0, 28.0),
                padding: crate::layout::Edges {
                    left: 16.0,
                    top: 0.0,
                    right: 16.0,
                    bottom: 12.0,
                },
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Center,
                shrink: 0.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.list_pager = Some(pager);
        self.list_previous = Some(super::shell::library_icon_button(
            cx.tree,
            pager,
            crate::path::Icon::D2ArrowLeft,
        )?);
        self.list_range = Some(style::label(cx.tree, pager, "", Text::Note)?);
        self.list_next = Some(super::shell::library_icon_button(
            cx.tree,
            pager,
            crate::path::Icon::D2ArrowRight,
        )?);
        cx.tree.set_visible(pager, false)?;

        // --- RIGHT: the selected fix, its actions, the write note and the presets ---
        let right = cx.tree.add(
            Some(main),
            NodeKind::Column,
            Style {
                shrink: 0.0,
                gap: crate::layout::Size::new(0.0, gap),
                align_items: crate::layout::Align::Stretch,
                preferred: crate::layout::Size::new(details_width, 0.0),
                min: crate::layout::Size::new(details_width, 0.0),
                max: crate::layout::Size::new(details_width, f32::INFINITY),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.right_column = Some(right);
        // The details take the column's rest; the presets' card keeps its own height at the bottom.
        let details = style::d2::panel(cx.tree, right)?;
        cx.tree.set_style(
            details,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                ..panel_style(12.0, 16.0)
            },
        )?;
        self.details_card = Some(details);
        // The fix's title, its id and status, then its text; the text scrolls with a thumb at its right.
        self.details_title = Some(style::label(cx.tree, details, "", Text::Heading)?);
        let key_row = style::row(cx.tree, details)?;
        cx.tree.set_style(
            key_row,
            Style {
                shrink: 0.0,
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
        )?;
        self.details_key = Some(style::label(cx.tree, key_row, "", Text::Note)?);
        self.details_badge = Some(style::d2::badge(
            cx.tree,
            key_row,
            crate::strings::t("НЕ УСТАНОВЛЕНО"),
            style::d2::BadgeKind::NotInstalled,
        )?);
        let body_row = cx.tree.add(
            Some(details),
            NodeKind::Row,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                gap: crate::layout::Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        // The details scroll inside a height capped to the room the right column has.
        let details_scroll = cx.tree.add(
            Some(body_row),
            NodeKind::Scroll {
                horizontal: false,
                vertical: true,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_clip_children(details_scroll, true)?;
        self.details_scroll = Some(details_scroll);
        self.detail = Some(paragraph_leaf(
            cx.tree,
            details_scroll,
            crate::strings::t("ВЫБЕРИТЕ ИСПРАВЛЕНИЕ"),
            Text::Body,
        )?);
        // The thumb of the details scroll: a bar at the right, sized and placed by render.
        let track = cx.tree.add(
            Some(body_row),
            NodeKind::Column,
            Style {
                shrink: 0.0,
                min: crate::layout::Size::new(8.0, 0.0),
                preferred: crate::layout::Size::new(8.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.details_thumb = Some(cx.tree.add(
            Some(track),
            NodeKind::Leaf,
            Style {
                shrink: 0.0,
                min: crate::layout::Size::new(8.0, 24.0),
                preferred: crate::layout::Size::new(8.0, 24.0),
                ..Style::default()
            },
            Content::Panel,
            Look {
                fill: Some(style::d2::argb(theme::d2::BORDER_METAL)),
                radius: 3.0,
                ..Look::default()
            },
        )?);
        cx.tree.set_visible(self.details_thumb.unwrap_or(track), false)?;
        // The two actions stack with 8 px between them, at the card's full width.
        let actions = cx.tree.add(
            Some(details),
            NodeKind::Column,
            Style {
                shrink: 0.0,
                gap: crate::layout::Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.install = Some(style::d2::button(
            cx.tree,
            actions,
            crate::strings::t("УСТАНОВИТЬ ВЫБРАННОЕ"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        self.remove = Some(style::d2::button(
            cx.tree,
            actions,
            crate::strings::t("УДАЛИТЬ И ВОССТАНОВИТЬ"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);

        // The presets' card sits at the bottom of the column, at its own height.
        let apply = style::d2::panel(cx.tree, right)?;
        cx.tree.set_style(apply, panel_style(10.0, 16.0))?;
        self.bottom_card = Some(apply);
        self.write_note = Some(paragraph_leaf(
            cx.tree,
            apply,
            crate::strings::t(
                "ИСПРАВЛЕНИЕ ЗАПИСЫВАЕТСЯ ТОЛЬКО ПО НАЖАТИЮ КНОПКИ. ПРИ ИЗМЕНЕНИИ УПРАВЛЯЕМОГО ФАЙЛА УДАЛЕНИЕ ОСТАНОВИТСЯ, НЕ ПЕРЕЗАПИСЫВАЯ ЕГО.",
            ),
            Text::Note,
        )?);
        self.preset_safe = Some(style::d2::button(
            cx.tree,
            apply,
            crate::strings::t("ПРИМЕНИТЬ: ВСЕ БЕЗОПАСНЫЕ"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        self.preset_essential = Some(style::d2::button(
            cx.tree,
            apply,
            crate::strings::t("ПРИМЕНИТЬ: ОБЯЗАТЕЛЬНЫЕ"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        self.preset_recommended = Some(style::d2::button(
            cx.tree,
            apply,
            crate::strings::t("ПРИМЕНИТЬ: РЕКОМЕНДУЕМЫЕ"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);

        // --- CONFIRMATION: shown only when a write is asked for ---
        // The confirmation is a dialog: it sits in the row, where a hidden child takes no height.
        let confirm = style::card(cx.tree, main)?;
        self.confirm_card = Some(confirm);
        style::label(
            cx.tree,
            confirm,
            crate::strings::t("ПОДТВЕРЖДЕНИЕ ИЗМЕНЕНИЯ ИГРЫ"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            confirm,
            crate::strings::t("Операция изменит файлы выбранной игры. Подтверждение действует только для текущего исправления и установки."),
            Text::Note,
        )?;
        let confirm_actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::d2::button(
            cx.tree,
            confirm_actions,
            crate::strings::t("ПОДТВЕРДИТЬ"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        self.confirm_cancel = Some(style::d2::button(
            cx.tree,
            confirm_actions,
            crate::strings::t("ОТМЕНА"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        cx.tree.set_visible(confirm, false)?;
        self.page_size = 1;
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
        if let Message::User(AppMessage::OpenGameFix { game_id, fix_id }) = message {
            self.open_linked_fix(cx, game_id, fix_id)?;
            return Ok(());
        }
        if let Message::Window(WindowEvent::Wheel { delta }) = message {
            if let Some(scroll) = self
                .details_scroll
                .filter(|_| self.details_limit(cx.tree).is_ok_and(|limit| limit > 0.0))
            {
                // The limit in whole pixels, from its rounded text.
                let limit: i32 = format!("{:.0}", self.details_limit(cx.tree)?).parse().unwrap_or(0);
                self.details_offset = self
                    .details_offset
                    .saturating_add(delta.saturating_mul(24))
                    .clamp(0, limit);
                cx.tree.set_scroll_y(scroll, self.details_offset)?;
                return Ok(());
            }
            if let Some(scroll) = self.list_scroll {
                self.scroll_y = self.scroll_y.saturating_add(delta.saturating_mul(48)).max(0);
                cx.tree.set_scroll_y(scroll, self.scroll_y)?;
                return Ok(());
            }
        }
        if self.busy && clicked.is_some() {
            return Ok(());
        }

        // The list's pager and its rows: a row is the item at its place on the shown page.
        if clicked.is_some() && clicked == self.list_previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.list_next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if let Message::Window(WindowEvent::Resized { .. }) = message {
            return self.render(cx);
        }
        let page_start = self.page.saturating_mul(self.page_size);
        let clicked_fix = self.rows.iter().copied().enumerate().find_map(|(index, row)| {
            (clicked == Some(row))
                .then(|| {
                    self.items
                        .get(page_start.saturating_add(index))
                        .map(|item| item.id.clone())
                })
                .flatten()
        });
        if let Some(fix_id) = clicked_fix {
            self.select_item(cx, &fix_id)?;
        }

        if clicked.is_some() && clicked == self.check {
            self.busy = true;
            self.verified = None;
            let game = cx.app.selected_game().map(str::to_owned);
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let language = crate::strings::current_language().to_owned();
            sse_app::tasks::spawn_named_detached("game-read", move || {
                let result = (|| {
                    let game =
                        game.ok_or_else(|| crate::strings::t_in(&language, "Игра не поддерживается").to_owned())?;
                    let target = fix_target(&game)
                        .ok_or_else(|| crate::strings::t_in(&language, "Игра не поддерживается").to_owned())?;
                    let directory =
                        directory.ok_or_else(|| crate::strings::t_in(&language, "Папка игры не выбрана").to_owned())?;
                    let (matches, build) = sse_fixes::identify_game(target, &directory);
                    if !matches {
                        return Err(
                            crate::strings::t_in(&language, "ПАПКА НЕ ПОХОЖА НА ВЫБРАННУЮ УСТАНОВКУ ИГРЫ.").to_owned(),
                        );
                    }
                    let text = match build.as_deref() {
                        Some(build) => {
                            crate::strings::t_in(&language, "НАЙДЕНА СБОРКА STEAM: {0}.").replace("{0}", build)
                        }
                        None => crate::strings::t_in(
                            &language,
                            "STEAM BUILD ID НЕ НАЙДЕН. ПРИ УСТАНОВКЕ ИСХОДНЫЕ ФАЙЛЫ ПРОВЕРЯТСЯ ПО SHA-256.",
                        )
                        .to_owned(),
                    };
                    Ok((text, game, directory, build))
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameFixes,
                    Box::new(FixReply::Compatibility(result)),
                ));
            });
            return Ok(());
        }
        let preset = if clicked.is_some() && clicked == self.preset_recommended {
            Some(sse_fixes::GameFixPreset::Recommended)
        } else if clicked.is_some() && clicked == self.preset_essential {
            Some(sse_fixes::GameFixPreset::EssentialOnly)
        } else if clicked.is_some() && clicked == self.preset_safe {
            Some(sse_fixes::GameFixPreset::AllSafeFixes)
        } else {
            None
        };
        if let Some(preset) = preset {
            let Some(game) = cx.app.selected_game().map(str::to_owned) else {
                return Ok(());
            };
            let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                return Ok(());
            };
            let verified = self
                .verified
                .as_ref()
                .is_some_and(|state| state.game == game && state.directory == directory);
            if !verified {
                cx.status = Some(crate::strings::t("СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ.").to_owned());
                return Ok(());
            }
            let target = fix_target(&game)
                .ok_or_else(|| sse_core::Error::damaged(crate::strings::t("Игра не поддерживается")))?;
            let build = self.verified.as_ref().and_then(|state| state.build.as_deref());
            if sse_fixes::GameFixCatalog::for_preset(target, preset)
                .iter()
                .any(|definition| !definition.supports_detected_build_or_hashes(build))
            {
                cx.status = Some(match build {
                    Some(build) => {
                        crate::strings::t("СБОРКА STEAM {0} НЕ ПОДДЕРЖИВАЕТ ВЫБРАННЫЙ ПРЕСЕТ.").replace("{0}", build)
                    }
                    None => crate::strings::t(
                        "НЕТ STEAM BUILD ID; У ОДНОГО ИЗ ВЫБРАННЫХ ИСПРАВЛЕНИЙ НЕТ ПОЛНЫХ SHA-256 ЯКОРЕЙ.",
                    )
                    .to_owned(),
                });
                return Ok(());
            }
            self.intent = Some(FixIntent {
                fix_id: None,
                operation: FixOperation::Preset(preset),
                game,
                directory,
            });
            if let Some(card) = self.confirm_card {
                cx.tree.open_dialog(card)?;
            }
            return Ok(());
        }

        let action = if clicked.is_some() && clicked == self.install {
            Some(true)
        } else if clicked.is_some() && clicked == self.remove {
            Some(false)
        } else {
            None
        };
        if let Some(install) = action {
            let Some(selected_id) = self.selected.as_deref() else {
                cx.status = Some(crate::strings::t("Сначала выберите исправление").to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.id == selected_id) else {
                self.selected = None;
                cx.status = Some(crate::strings::t("Выбранное исправление исчезло после обновления списка").to_owned());
                return Ok(());
            };
            let Some(game) = cx.app.selected_game().map(str::to_owned) else {
                return Ok(());
            };
            let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                return Ok(());
            };
            if install {
                let Some(verified) = self
                    .verified
                    .as_ref()
                    .filter(|state| state.game == game && state.directory == directory)
                else {
                    cx.status = Some(crate::strings::t("СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ.").to_owned());
                    return Ok(());
                };
                let compatible = sse_fixes::GameFixCatalog::try_get(&item.id)
                    .is_some_and(|definition| definition.supports_detected_build_or_hashes(verified.build.as_deref()));
                if !compatible {
                    cx.status = Some(match verified.build.as_deref() {
                        Some(build) => crate::strings::t("СБОРКА STEAM {0} НЕ ПОДДЕРЖИВАЕТ ВЫБРАННОЕ ИСПРАВЛЕНИЕ.")
                            .replace("{0}", build),
                        None => {
                            crate::strings::t("НЕТ STEAM BUILD ID; ЭТО ИСПРАВЛЕНИЕ НЕ ИМЕЕТ ПОЛНЫХ SHA-256 ЯКОРЕЙ.")
                                .to_owned()
                        }
                    });
                    return Ok(());
                }
            }
            self.intent = Some(FixIntent {
                fix_id: Some(item.id.clone()),
                operation: if install {
                    FixOperation::Install
                } else {
                    FixOperation::Remove
                },
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
            let _ = cx.tree.close_dialog()?;
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_write {
            let Some(intent) = self.intent.take() else {
                return Ok(());
            };
            if cx.app.selected_game() != Some(intent.game.as_str())
                || cx.app.game_dir() != Some(intent.directory.as_path())
            {
                let _ = cx.tree.close_dialog()?;
                cx.status = Some(crate::strings::t("Выбор игры изменился; подтверждение отменено.").to_owned());
                return Ok(());
            }
            let fix_id = intent.fix_id;
            let operation = intent.operation;
            let game = intent.game;
            let directory = intent.directory;
            let language = crate::strings::current_language().to_owned();
            let _ = cx.tree.close_dialog()?;
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            self.busy = true;
            sse_app::tasks::spawn_named_detached("game-write", move || {
                let result = (|| {
                    let target = fix_target(&game)
                        .ok_or_else(|| crate::strings::t_in(&language, "Игра не поддерживается").to_owned())?;
                    let engine = sse_fixes::GameFixEngine::new();
                    match operation {
                        FixOperation::Preset(preset) => {
                            let result = engine
                                .apply_preset(target, preset, &directory)
                                .map_err(|e| e.to_string())?;
                            Ok(tr(
                                &language,
                                "ПРЕСЕТ {0}: УСТАНОВЛЕНО {1}; УЖЕ АКТУАЛЬНЫХ {2}.",
                                &[
                                    &crate::strings::t_in(&language, preset.as_str()),
                                    &result.installed_fix_ids.len(),
                                    &result.already_installed_fix_ids.len(),
                                ],
                            ))
                        }
                        FixOperation::Install | FixOperation::Remove => {
                            let fix_id = fix_id
                                .ok_or_else(|| crate::strings::t_in(&language, "Исправление не выбрано").to_owned())?;
                            let definition = sse_fixes::GameFixCatalog::try_get(&fix_id)
                                .ok_or_else(|| crate::strings::t_in(&language, "Фикс исчез из каталога").to_owned())?;
                            if definition.game != target {
                                return Err(
                                    crate::strings::t_in(&language, "Фикс не относится к выбранной игре").to_owned()
                                );
                            }
                            let result = match operation {
                                FixOperation::Install => {
                                    match engine.get_status(definition, &directory).map_err(|e| e.to_string())? {
                                        sse_fixes::GameFixState::Installed => engine.update(definition, &directory),
                                        _ => engine.install(definition, &directory),
                                    }
                                }
                                FixOperation::Remove => {
                                    let check = engine.check_uninstall(&fix_id, &directory);
                                    if !check.can_uninstall {
                                        return Err(check.reason.unwrap_or_else(|| {
                                            crate::strings::t_in(&language, "Безопасное удаление запрещено").to_owned()
                                        }));
                                    }
                                    engine.uninstall(&fix_id, &directory)
                                }
                                FixOperation::Preset(_) => unreachable!(),
                            }
                            .map_err(|e| e.to_string())?;
                            Ok(tr(
                                &language,
                                "{0}: {1}; файлов: {2}",
                                &[&fix_id, &format!("{:?}", result.state), &result.files.len()],
                            ))
                        }
                    }
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameFixes,
                    Box::new(FixReply::Changed(result)),
                ));
            });
            return Ok(());
        }

        if let Message::User(AppMessage::ToScreen(ScreenId::GameFixes, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<FixReply>() {
                match reply {
                    FixReply::List {
                        request_id,
                        game_id,
                        directory,
                        selected_fix,
                        result: Ok(items),
                    } => {
                        if *request_id != self.list_request_id || game_id.as_deref() != cx.app.selected_game() {
                            return Ok(());
                        }
                        if self
                            .selected
                            .as_ref()
                            .is_some_and(|id| !items.iter().any(|item| &item.id == id))
                        {
                            self.selected = None;
                        }
                        self.intent = None;
                        if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
                            let _ = cx.tree.close_dialog()?;
                        }
                        self.items.clone_from(items);
                        // The search is over: the status line says whether the game's installation was found.
                        cx.status = Some(
                            crate::strings::t(if directory.is_some() {
                                "Установка найдена"
                            } else {
                                "Не выбрана"
                            })
                            .to_owned(),
                        );
                        let selection = selected_fix
                            .as_deref()
                            .or(self.selected.as_deref())
                            .filter(|id| self.items.iter().any(|item| item.id == *id))
                            .map(str::to_owned);
                        self.selected = selection.clone();
                        self.requested_fix = selection;
                        cx.app.set_game_dir(directory.clone());
                        if let Some(compatibility) = self.compatibility {
                            let message = directory.as_ref().map_or_else(
                                || {
                                    crate::strings::t("УСТАНОВКА НЕ НАЙДЕНА. ВЫБЕРИТЕ ПАПКУ ИГРЫ НА ЭКРАНЕ «ИГРЫ».")
                                        .to_owned()
                                },
                                |path| {
                                    tr(
                                        crate::strings::current_language(),
                                        "УСТАНОВКА ОБНАРУЖЕНА: {0}. ПРОВЕРЬТЕ СОВМЕСТИМОСТЬ ПЕРЕД УСТАНОВКОЙ.",
                                        &[&path.display()],
                                    )
                                },
                            );
                            cx.tree.set_text(compatibility, &message)?;
                        }
                        self.render(cx)?;
                        self.show_selection(cx)?;
                    }
                    FixReply::List {
                        request_id,
                        game_id,
                        result: Err(error),
                        ..
                    } => {
                        if *request_id != self.list_request_id || game_id.as_deref() != cx.app.selected_game() {
                            return Ok(());
                        }
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, error)?;
                        }
                        if let Some(compatibility) = self.compatibility {
                            cx.tree.set_text(compatibility, error)?;
                        }
                    }
                    FixReply::Compatibility(Ok((text, game, directory, build))) => {
                        self.busy = false;
                        self.verified = Some(FixCompatibility {
                            game: game.clone(),
                            directory: directory.clone(),
                            build: build.clone(),
                        });
                        if let Some(id) = self.compatibility {
                            cx.tree.set_text(id, text)?;
                        }
                    }
                    FixReply::Compatibility(Err(error)) => {
                        self.busy = false;
                        self.verified = None;
                        if let Some(id) = self.compatibility {
                            cx.tree.set_text(id, error)?;
                        }
                    }
                    FixReply::Changed(Ok(text)) => {
                        self.busy = false;
                        cx.status = Some(text.clone());
                        self.refresh(cx);
                    }
                    FixReply::Changed(Err(error)) => {
                        self.busy = false;
                        cx.status = Some(tr(crate::strings::current_language(), "ОШИБКА: {0}", &[error]));
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct DoctorFinding {
    file: String,
    line: usize,
    checker: String,
    severity: String,
    message: String,
}

#[derive(Debug)]
enum DoctorReply {
    Progress(DoctorProgress),
    Done(std::result::Result<(usize, u64, Vec<DoctorFinding>), String>),
    Mods(std::result::Result<String, String>),
}

#[derive(Clone, Copy, Debug)]
enum DoctorProgress {
    BuildingTree,
    StartingLinter,
}

impl DoctorProgress {
    fn text(self, language: &str) -> &'static str {
        crate::strings::t_in(
            language,
            match self {
                Self::BuildingTree => "25% · строю дерево файлов",
                Self::StartingLinter => "60% · запускаю линтер",
            },
        )
    }
}

#[derive(Default)]
struct GameDoctor {
    status: Option<WidgetId>,
    start: Option<WidgetId>,
    cancel: Option<WidgetId>,
    toggle_s2_mods: Option<WidgetId>,
    confirm_s2: Option<WidgetId>,
    confirm_s2_write: Option<WidgetId>,
    confirm_s2_cancel: Option<WidgetId>,
    pending_s2_toggle: Option<PathBuf>,
    rows: Vec<WidgetId>,
    findings: Vec<DoctorFinding>,
    cancellation: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

fn content_game(game: &str) -> Option<sse_content::CompanionGame> {
    match game {
        "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => Some(sse_content::CompanionGame::ShadowOfChernobyl),
        "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => Some(sse_content::CompanionGame::ClearSky),
        "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => Some(sse_content::CompanionGame::CallOfPripyat),
        _ => None,
    }
}

impl GameDoctor {
    fn run(&mut self, cx: &mut Context<'_>) {
        let Some(game) = cx.app.selected_game().and_then(content_game) else {
            cx.status = Some(
                crate::strings::t_in(
                    crate::strings::current_language(),
                    "Доктор игры сейчас проверяет X-Ray установки",
                )
                .to_owned(),
            );
            return;
        };
        let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
            cx.status = Some(
                crate::strings::t_in(
                    crate::strings::current_language(),
                    "Сначала выберите установленную игру",
                )
                .to_owned(),
            );
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.cancellation = Some(std::sync::Arc::clone(&cancelled));
        sse_app::tasks::spawn_named_detached("game-read", move || {
            use std::sync::atomic::Ordering;
            proxy.send(AppMessage::ToScreen(
                ScreenId::GameDoctor,
                Box::new(DoctorReply::Progress(DoctorProgress::BuildingTree)),
            ));
            let result = (|| {
                let tree = sse_content::GameFileTree::load_simple(
                    game,
                    &directory,
                    |path| {
                        let lower = path.to_ascii_lowercase();
                        lower.ends_with(".ltx") || lower.ends_with(".xml") || lower.ends_with(".script")
                    },
                    false,
                )
                .map_err(|e| e.to_string())?;
                if cancelled.load(Ordering::Relaxed) {
                    return Err("cancelled".to_owned());
                }
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameDoctor,
                    Box::new(DoctorReply::Progress(DoctorProgress::StartingLinter)),
                ));
                let engine = sse_lint::LintEngine::new(sse_lint::LintOptions {
                    single_checker: None,
                    config_subdir: None,
                    max_files_globals: None,
                });
                let report = engine.lint_tree(&tree);
                if cancelled.load(Ordering::Relaxed) {
                    return Err("cancelled".to_owned());
                }
                let mut findings: Vec<DoctorFinding> = report
                    .findings
                    .into_iter()
                    .map(|finding| DoctorFinding {
                        file: finding.file,
                        line: finding.line,
                        checker: finding.checker,
                        severity: finding.severity.as_str().to_owned(),
                        message: finding.message,
                    })
                    .collect();
                findings.sort_by(|left, right| {
                    left.file
                        .cmp(&right.file)
                        .then_with(|| left.line.cmp(&right.line))
                        .then_with(|| left.checker.cmp(&right.checker))
                        .then_with(|| left.message.cmp(&right.message))
                });
                Ok((report.files_checked, report.elapsed_ms, findings))
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::GameDoctor,
                Box::new(DoctorReply::Done(result)),
            ));
        });
    }
}

impl Screen for GameDoctor {
    fn id(&self) -> ScreenId {
        ScreenId::GameDoctor
    }
    fn subtitle(&self) -> &str {
        crate::strings::t_in(
            crate::strings::current_language(),
            "Фоновая проверка установки линтером; находки сгруппированы по файлу и тяжести",
        )
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let language = crate::strings::current_language();
        let card = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "ДОКТОР ИГРЫ"),
            Text::Heading,
        )?;
        self.status = Some(style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "Готов к проверке"),
            Text::Note,
        )?);
        let actions = style::row(cx.tree, card)?;
        self.start = Some(style::button(
            cx.tree,
            actions,
            crate::strings::t_in(language, "Проверить"),
            Button::Primary,
        )?);
        self.cancel = Some(style::button(
            cx.tree,
            actions,
            crate::strings::t_in(language, "Отмена"),
            Button::Secondary,
        )?);
        self.toggle_s2_mods = Some(style::button(
            cx.tree,
            actions,
            crate::strings::t_in(language, "ВРЕМЕННО ОТКЛЮЧИТЬ / ВОССТАНОВИТЬ КАСТОМНЫЕ МОДЫ"),
            Button::Secondary,
        )?);
        let confirm = style::card(cx.tree, host)?;
        self.confirm_s2 = Some(confirm);
        style::label(
            cx.tree,
            confirm,
            crate::strings::t_in(language, "ПОДТВЕРЖДЕНИЕ ИЗМЕНЕНИЯ МОДОВ S2"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            confirm,
            crate::strings::t_in(
                language,
                "Папка ~mods будет атомарно переименована. Проверьте выбранную установку.",
            ),
            Text::Note,
        )?;
        let confirm_row = style::row(cx.tree, confirm)?;
        self.confirm_s2_write = Some(style::button(
            cx.tree,
            confirm_row,
            crate::strings::t_in(language, "ПОДТВЕРДИТЬ"),
            Button::Primary,
        )?);
        self.confirm_s2_cancel = Some(style::button(
            cx.tree,
            confirm_row,
            crate::strings::t_in(language, "ОТМЕНА"),
            Button::Secondary,
        )?);
        cx.tree.set_visible(confirm, false)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "МОДИФИКАЦИИ · АУДИТ ФАЙЛОВ"),
            Text::Value,
        )?;
        for _ in 0..10 {
            let row = style::label(cx.tree, card, "", Text::Body)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.start {
            self.run(cx);
        }
        if clicked.is_some() && clicked == self.toggle_s2_mods {
            let is_s2 = cx.app.selected_game().is_some_and(|g| matches!(g, "s2" | "stalker2"));
            if !is_s2 {
                cx.status = Some(
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "Переключение модов доступно только для S.T.A.L.K.E.R. 2.",
                    )
                    .to_owned(),
                );
            } else if let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) {
                self.pending_s2_toggle = Some(directory);
                if let Some(card) = self.confirm_s2 {
                    cx.tree.open_dialog(card)?;
                }
            } else {
                cx.status = Some(
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "ВЫБЕРИТЕ ИГРУ И ПАПКУ УСТАНОВКИ ДЛЯ ПРОВЕРКИ.",
                    )
                    .to_owned(),
                );
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_s2_cancel {
            self.pending_s2_toggle = None;
            if self.confirm_s2.is_some() {
                cx.tree.close_dialog()?;
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.confirm_s2_write {
            let Some(directory) = self.pending_s2_toggle.take() else {
                return Ok(());
            };
            if cx.app.game_dir() != Some(directory.as_path()) {
                if self.confirm_s2.is_some() {
                    cx.tree.close_dialog()?;
                }
                cx.status = Some(
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "Выбор установки изменился; подтверждение отменено.",
                    )
                    .to_owned(),
                );
                return Ok(());
            }
            if self.confirm_s2.is_some() {
                cx.tree.close_dialog()?;
            }
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            sse_app::tasks::spawn_named_detached("game-write", move || {
                let result = sse_fixes::toolkit::Stalker2ModToggle::toggle(&directory)
                    .map(|value| format!("S2 mods: {value:?}"))
                    .map_err(|e| e.to_string());
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameDoctor,
                    Box::new(DoctorReply::Mods(result)),
                ));
            });
            return Ok(());
        }
        if clicked.is_some() && clicked == self.cancel {
            if let Some(cancelled) = &self.cancellation {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                if let Some(status) = self.status {
                    cx.tree.set_text(
                        status,
                        crate::strings::t_in(crate::strings::current_language(), "Отмена запрошена…"),
                    )?;
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::GameDoctor, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<DoctorReply>() {
                match reply {
                    DoctorReply::Progress(text) => {
                        if let Some(status) = self.status {
                            cx.tree
                                .set_text(status, text.text(crate::strings::current_language()))?;
                        }
                    }
                    DoctorReply::Done(Ok((files, elapsed, findings))) => {
                        self.cancellation = None;
                        self.findings.clone_from(findings);
                        if let Some(status) = self.status {
                            let language = crate::strings::current_language();
                            let findings_count = findings.len();
                            let elapsed = format!("{elapsed}");
                            let args: [&dyn std::fmt::Display; 3] = [&files, &findings_count, &elapsed];
                            cx.tree.set_text(
                                status,
                                &tr(language, "100% · файлов: {0} · находок: {1} · {2} мс", &args),
                            )?;
                        }
                        for (index, widget) in self.rows.iter().copied().enumerate() {
                            if let Some(finding) = findings.get(index) {
                                cx.tree.set_visible(widget, true)?;
                                cx.tree.set_text(
                                    widget,
                                    &format!(
                                        "{}:{} · {} · {} · {}",
                                        finding.file, finding.line, finding.severity, finding.checker, finding.message
                                    ),
                                )?;
                            } else {
                                cx.tree.set_visible(widget, false)?;
                            }
                        }
                    }
                    DoctorReply::Done(Err(error)) => {
                        self.cancellation = None;
                        if let Some(status) = self.status {
                            let language = crate::strings::current_language();
                            let text = if error == "cancelled" {
                                crate::strings::t_in(language, "Проверка отменена").to_owned()
                            } else {
                                tr(language, "Проверка не выполнена: {0}", &[error])
                            };
                            cx.tree.set_text(status, &text)?;
                        }
                    }
                    DoctorReply::Mods(Ok(status)) => {
                        cx.status = Some(tr(
                            crate::strings::current_language(),
                            "Переключение модов S2: {0}",
                            &[status],
                        ));
                        self.run(cx);
                    }
                    DoctorReply::Mods(Err(error)) => {
                        cx.status = Some(tr(
                            crate::strings::current_language(),
                            "Не удалось переключить моды S2: {0}",
                            &[error],
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

fn encyclopedia_game(game: &str) -> Option<sse_content::CompanionGame> {
    match game {
        "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => Some(sse_content::CompanionGame::ShadowOfChernobyl),
        "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => Some(sse_content::CompanionGame::ClearSky),
        "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => Some(sse_content::CompanionGame::CallOfPripyat),
        _ => None,
    }
}

struct EncyclopediaClipboard;
impl crate::edit::Clipboard for EncyclopediaClipboard {
    fn read_text(&mut self) -> Result<String> {
        Ok(String::new())
    }
    fn write_text(&mut self, _text: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct EncyclopediaEntry {
    kind: EncyclopediaKind,
    key: String,
    name: String,
    detail: EncyclopediaDetail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EncyclopediaKind {
    Item,
    Faction,
    Location,
}

impl EncyclopediaKind {
    fn label(self, language: &str) -> &'static str {
        crate::strings::t_in(
            language,
            match self {
                Self::Item => "предмет",
                Self::Faction => "персонаж/группировка",
                Self::Location => "локация",
            },
        )
    }
}

#[derive(Clone, Debug)]
enum EncyclopediaDetail {
    Item {
        category: Option<String>,
        source: String,
        cost: Option<String>,
    },
    Faction {
        source: String,
    },
    Location {
        path: String,
    },
}

impl EncyclopediaDetail {
    fn text(&self, language: &str) -> String {
        match self {
            Self::Item { category, source, cost } => {
                let missing_category = crate::strings::t_in(language, "без категории");
                let category = category.as_deref().unwrap_or(missing_category);
                let cost = cost.as_deref().unwrap_or("—");
                tr(language, "{0} · {1} · цена: {2}", &[&category, source, &cost])
            }
            Self::Faction { source } => tr(language, "группировка · {0}", &[source]),
            Self::Location { path } => path.clone(),
        }
    }
}

fn encyclopedia_entry_text(language: &str, entry: &EncyclopediaEntry) -> String {
    tr(
        language,
        "{0} · {1} · {2}",
        &[&entry.kind.label(language), &entry.name, &entry.key],
    )
}

#[derive(Debug)]
struct EncyclopediaResult {
    game: String,
    generation: u64,
    result: std::sync::Mutex<Option<std::result::Result<Vec<EncyclopediaEntry>, String>>>,
}

impl EncyclopediaResult {
    fn take_if_current(
        &self,
        game: Option<&str>,
        generation: u64,
    ) -> Option<std::result::Result<Vec<EncyclopediaEntry>, String>> {
        if self.generation != generation || game != Some(self.game.as_str()) {
            return None;
        }
        self.result.lock().ok()?.take()
    }
}

#[derive(Default)]
struct Encyclopedia {
    status: Option<WidgetId>,
    search_label: Option<WidgetId>,
    rows: Vec<WidgetId>,
    card: Option<WidgetId>,
    to_save: Option<WidgetId>,
    to_game: Option<WidgetId>,
    entries: Vec<EncyclopediaEntry>,
    visible: Vec<usize>,
    selected: Option<usize>,
    search: Option<crate::widgets::text_input::TextInput>,
    generation: u64,
}

impl Encyclopedia {
    fn load(&mut self, cx: &mut Context<'_>) {
        self.generation = self.generation.saturating_add(1);
        let generation = self.generation;
        let game_id = cx.app.selected_game().map(str::to_owned);
        let Some(game) = game_id.as_deref().and_then(encyclopedia_game) else {
            if let Some(status) = self.status {
                let _ = cx.tree.set_text(
                    status,
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "Энциклопедия сейчас доступна для X-Ray игр",
                    ),
                );
            }
            return;
        };
        let Some(game_id) = game_id else {
            return;
        };
        let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
            if let Some(status) = self.status {
                let _ = cx.tree.set_text(
                    status,
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "Сначала выберите установленную игру",
                    ),
                );
            }
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        sse_app::tasks::spawn_named_detached("game-read", move || {
            let cache = std::env::temp_dir().join("stalker-save-editor").join("catalog-cache");
            let result = (|| {
                let content = sse_catalog::GameContentService::load(game, &directory, &cache, "ru")
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "catalog_not_built".to_owned())?;
                let bundle = content.bundle();
                let mut entries = Vec::new();
                for item in bundle.items.items() {
                    entries.push(EncyclopediaEntry {
                        kind: EncyclopediaKind::Item,
                        key: item.key.clone(),
                        name: item.display_name.clone().unwrap_or_else(|| item.key.clone()),
                        detail: EncyclopediaDetail::Item {
                            category: item.category.clone(),
                            source: item.source.clone(),
                            cost: item.cost.map(|value| value.to_string()),
                        },
                    });
                }
                if let Some(factions) = &bundle.factions {
                    for faction in factions.factions() {
                        entries.push(EncyclopediaEntry {
                            kind: EncyclopediaKind::Faction,
                            key: faction.key.clone(),
                            name: faction.display_name.clone().unwrap_or_else(|| faction.key.clone()),
                            detail: EncyclopediaDetail::Faction {
                                source: faction.source.clone(),
                            },
                        });
                    }
                }
                let search = sse_content::CompanionArchiveLocator::discover(&directory, &["fsgame.ltx"], game);
                for archive in search.archive_paths {
                    if let Some(name) = archive.file_stem().and_then(|v| v.to_str()) {
                        if name.to_ascii_lowercase().contains("level") || name.to_ascii_lowercase().contains("location")
                        {
                            entries.push(EncyclopediaEntry {
                                kind: EncyclopediaKind::Location,
                                key: name.to_owned(),
                                name: name.to_owned(),
                                detail: EncyclopediaDetail::Location {
                                    path: archive.display().to_string(),
                                },
                            });
                        }
                    }
                }
                Ok(entries)
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Encyclopedia,
                Box::new(EncyclopediaResult {
                    game: game_id,
                    generation,
                    result: std::sync::Mutex::new(Some(result)),
                }),
            ));
        });
    }

    fn apply_search(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let query = self
            .search
            .as_ref()
            .map_or_else(String::new, crate::widgets::text_input::TextInput::text);
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(index, entry)| {
                (query.is_empty()
                    || crate::text::folded_contains(&entry.name, &query, crate::text::SearchLocale::General)
                    || crate::text::folded_contains(&entry.key, &query, crate::text::SearchLocale::General)
                    || crate::text::folded_contains(
                        entry.kind.label(crate::strings::current_language()),
                        &query,
                        crate::text::SearchLocale::General,
                    ))
                .then_some(index)
            })
            .collect();

        let ids: Vec<u64> = self
            .visible
            .iter()
            .filter_map(|index| u64::try_from(*index).ok())
            .collect();
        let headers = vec![
            crate::widgets::table::Header {
                label: crate::strings::t_in(crate::strings::current_language(), "Тип").to_owned(),
                sortable: true,
                direction: None,
            },
            crate::widgets::table::Header {
                label: crate::strings::t_in(crate::strings::current_language(), "Название").to_owned(),
                sortable: true,
                direction: None,
            },
        ];
        let _table = crate::widgets::table::Table::new(ids, 24.0, headers)?;

        if let Some(label) = self.search_label {
            let language = crate::strings::current_language();
            let query_display = if query.is_empty() {
                crate::strings::t_in(language, "все")
            } else {
                &query
            };
            let count = self.visible.len();
            cx.tree.set_text(
                label,
                &tr(language, "Поиск: {0} · результатов: {1}", &[&query_display, &count]),
            )?;
        }
        for (row_index, widget) in self.rows.iter().copied().enumerate() {
            if let Some(entry) = self.visible.get(row_index).and_then(|index| self.entries.get(*index)) {
                cx.tree.set_visible(widget, true)?;
                cx.tree.set_text(
                    widget,
                    &encyclopedia_entry_text(crate::strings::current_language(), entry),
                )?;
            } else {
                cx.tree.set_visible(widget, false)?;
            }
        }
        Ok(())
    }
}

impl Screen for Encyclopedia {
    fn id(&self) -> ScreenId {
        ScreenId::Encyclopedia
    }
    fn subtitle(&self) -> &str {
        crate::strings::t_in(
            crate::strings::current_language(),
            "Предметы, персонажи и локации из каталога установленной игры",
        )
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let language = crate::strings::current_language();
        let card = style::card(cx.tree, host)?;
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "ЭНЦИКЛОПЕДИЯ ПРЕДМЕТОВ"),
            Text::Heading,
        )?;
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(
                language,
                "У каждой записи показаны имя, значок, вес, цена и секция из файлов установленной игры.",
            ),
            Text::Note,
        )?;
        self.status = Some(style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "Загрузка каталога…"),
            Text::Note,
        )?);
        self.search = Some(crate::widgets::text_input::TextInput::new(
            "",
            crate::edit::EditConfig {
                mode: crate::edit::FieldMode::SingleLine,
                max_graphemes: 128,
                history_limit: 32,
                filter: crate::edit::InputFilter::Any,
            },
        )?);
        self.search_label = Some(style::button(
            cx.tree,
            card,
            crate::strings::t_in(language, "Поиск: все · нажмите и печатайте"),
            Button::Secondary,
        )?);
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "ТИП · НАЗВАНИЕ · КЛЮЧ"),
            Text::Note,
        )?;
        for _ in 0..10 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        self.card = Some(style::label(
            cx.tree,
            card,
            crate::strings::t_in(language, "Выберите запись"),
            Text::Body,
        )?);
        let actions = style::row(cx.tree, card)?;
        self.to_save = Some(style::button(
            cx.tree,
            actions,
            crate::strings::t_in(language, "В сохранение"),
            Button::Primary,
        )?);
        self.to_game = Some(style::button(
            cx.tree,
            actions,
            crate::strings::t_in(language, "В игру"),
            Button::Secondary,
        )?);
        if let Some(id) = self.to_save {
            cx.tree.set_enabled(id, false)?;
        }
        if let Some(id) = self.to_game {
            cx.tree.set_enabled(id, false)?;
        }
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(
                language,
                "Выберите совместимое сохранение с поддержкой добавления предметов.",
            ),
            Text::Note,
        )?;
        style::label(
            cx.tree,
            card,
            crate::strings::t_in(
                language,
                "Выберите эту игру в Компаньоне и подключитесь к запущенной игре.",
            ),
            Text::Note,
        )?;
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
        if let (Some(search), Some(search_label)) = (self.search.as_mut(), self.search_label) {
            search.focus(cx.tree.focused() == Some(search_label), 0);
        }
        if clicked.is_some() && clicked == self.search_label {
            if let Some(search) = self.search.as_mut() {
                search.focus(true, 0);
                cx.status = Some(
                    crate::strings::t_in(
                        crate::strings::current_language(),
                        "Поиск активен: вводите текст с клавиатуры",
                    )
                    .to_owned(),
                );
            }
        }
        if let Message::Window(crate::event_loop::WindowEvent::Ime(event)) = message {
            if self
                .search
                .as_ref()
                .is_some_and(crate::widgets::text_input::TextInput::focused)
            {
                if let Some(search) = self.search.as_mut() {
                    search.apply_ime_event(event)?;
                }
                if matches!(event, crate::event_loop::ImeEvent::Commit(_))
                    || matches!(event, crate::event_loop::ImeEvent::Cancel)
                {
                    return self.apply_search(cx);
                }
                if let (Some(search), Some(label)) = (self.search.as_ref(), self.search_label) {
                    cx.tree.set_text(
                        label,
                        &tr(
                            crate::strings::current_language(),
                            "Поиск: {0} · результатов: {1}",
                            &[&search.display_text(), &self.visible.len()],
                        ),
                    )?;
                }
            }
            return Ok(());
        }
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
        }) = message
        {
            if *keysym == 0xff09 {
                if let (Some(search), Some(search_label)) = (self.search.as_mut(), self.search_label) {
                    search.focus(cx.tree.focused() == Some(search_label), 0);
                }
                return Ok(());
            }
            if matches!(*keysym, 0xff0d | 0xff1b)
                && self
                    .search
                    .as_ref()
                    .is_some_and(crate::widgets::text_input::TextInput::focused)
            {
                if let Some(search) = self.search.as_mut() {
                    search.focus(false, 0);
                }
                cx.tree.set_focus(None)?;
                return Ok(());
            }
            if self
                .search
                .as_ref()
                .is_some_and(crate::widgets::text_input::TextInput::focused)
            {
                let key = match *keysym {
                    0xff08 => crate::edit::Key::Backspace,
                    0xffff => crate::edit::Key::Delete,
                    0xff51 => crate::edit::Key::Left,
                    0xff53 => crate::edit::Key::Right,
                    0xff50 => crate::edit::Key::Home,
                    0xff57 => crate::edit::Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => crate::edit::Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => crate::edit::Key::Z,
                    _ => crate::edit::Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = EncyclopediaClipboard;
                if let Some(search) = self.search.as_mut() {
                    let changed = search.key(
                        key,
                        crate::edit::Modifiers {
                            ctrl: *ctrl,
                            shift: *shift,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    if changed {
                        self.apply_search(cx)?;
                    }
                }
            }
        }
        if let Message::User(AppMessage::Tick(seconds)) = message {
            if let Some(search) = self.search.as_mut() {
                let _ = search.tick(seconds.saturating_mul(1_000));
            }
        }

        if clicked.is_some() {
            for (row_index, widget) in self.rows.iter().copied().enumerate() {
                if clicked == Some(widget) {
                    if let Some(index) = self.visible.get(row_index).copied() {
                        self.selected = Some(index);
                        if let (Some(card), Some(entry)) = (self.card, self.entries.get(index)) {
                            cx.tree.set_text(
                                card,
                                &format!(
                                    "{}\n{}\n{}",
                                    entry.name,
                                    entry.kind.label(crate::strings::current_language()),
                                    entry.detail.text(crate::strings::current_language())
                                ),
                            )?;
                            if let Some(id) = self.to_save {
                                cx.tree.set_enabled(id, false)?;
                            }
                            if let Some(id) = self.to_game {
                                cx.tree.set_enabled(id, false)?;
                            }
                        }
                    }
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Encyclopedia, payload)) = message {
            if let Some(result) = payload.downcast_ref::<EncyclopediaResult>() {
                let Some(result) = result.take_if_current(cx.app.selected_game(), self.generation) else {
                    return Ok(());
                };
                match result {
                    Ok(entries) => {
                        let count = entries.len();
                        self.entries = entries;
                        if let Some(status) = self.status {
                            let language = crate::strings::current_language();
                            cx.tree.set_text(
                                status,
                                &tr(
                                    language,
                                    "Источник: файлы выбранной установленной игры. Записей: {0}",
                                    &[&count],
                                ),
                            )?;
                        }
                        self.apply_search(cx)?;
                    }
                    Err(error) => {
                        if let Some(status) = self.status {
                            let language = crate::strings::current_language();
                            let text = if error == "catalog_not_built" {
                                crate::strings::t_in(language, "Каталог установки не построен").to_owned()
                            } else {
                                tr(language, "Каталог не загружен: {0}", &[&error])
                            };
                            cx.tree.set_text(status, &text)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod game_fixes_tests {
    use super::{
        load_fix_rows, AppMessage, Context, DiscoveredInstallation, FixBadge, FixReply, FixRow, GameFixes,
        GameInstallSource, GameTarget, Screen, ScreenId,
    };
    use crate::event_loop::Message;
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::widget::{Content, Look, Tree};
    use sse_core::Result;

    fn fix_row(id: &str) -> Result<FixRow> {
        let definition = sse_fixes::GameFixCatalog::try_get(id)
            .ok_or_else(|| sse_core::Error::damaged("test fix is missing from the catalog"))?;
        Ok(FixRow {
            badge: FixBadge::NotInstalled,
            id: definition.id.clone(),
            title: definition.title.clone(),
            description: definition.description.clone(),
            version: definition.version.clone(),
            status: "NOT INSTALLED".to_owned(),
            category: definition.category.as_str().to_owned(),
            maturity: definition.maturity.as_str().to_owned(),
            problem: definition.problem.clone(),
            source: definition.source.clone(),
            builds: definition.supported_steam_build_ids.join(", "),
            files: definition
                .text_patches
                .len()
                .saturating_add(definition.overlays.len())
                .saturating_add(definition.spawn_edits.len()),
        })
    }

    #[test]
    fn loading_game_fixes_recovers_an_interrupted_install_before_listing() -> sse_core::Result<()> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sse-game-fixes-recovery-{unique}"));
        let target = root.join("gamedata/configs/a.ltx");
        let state = root.join(".save-editor-game-fixes/test.ui-recovery");
        let backup = state.join("backups/file-0000.before");
        let original = b"value=before\n";
        let after = b"value=partial\n";
        let before_sha = sse_codecs::sha256::sha256_hex(original);
        let after_sha = sse_codecs::sha256::sha256_hex(after);

        let target_parent = target
            .parent()
            .ok_or_else(|| sse_core::Error::damaged("Missing fixture parent"))?;
        let backup_parent = backup
            .parent()
            .ok_or_else(|| sse_core::Error::damaged("Missing backup parent"))?;
        std::fs::create_dir_all(target_parent)?;
        std::fs::create_dir_all(backup_parent)?;
        std::fs::write(root.join("fsgame.ltx"), b"$game_data$=true|true|$fs_root$|gamedata\\")?;
        std::fs::write(&target, after)?;
        std::fs::write(&backup, original)?;
        let journal = format!(
            "{{\n  \"schemaVersion\": 1,\n  \"kind\": \"install\",\n  \"freshState\": true,\n  \"files\": [{{\n    \"relativePath\": \"gamedata/configs/a.ltx\",\n    \"beforeSha256\": \"{before_sha}\",\n    \"afterSha256\": \"{after_sha}\",\n    \"backupPath\": \"backups/file-0000.before\",\n    \"targetExistedBefore\": true\n  }}]\n}}"
        );
        std::fs::write(state.join("transaction.json"), journal)?;

        load_fix_rows(sse_fixes::GameTarget::ShadowOfChernobyl, Some(&root), "ru").map_err(sse_core::Error::System)?;

        assert_eq!(std::fs::read(&target)?, original);
        assert!(!state.exists(), "recovered transaction state should be cleaned up");
        let _ = std::fs::remove_dir_all(&root);
        Ok(())
    }

    #[test]
    fn linked_fix_opens_selected_without_creating_an_install_intent() -> Result<()> {
        let fix_id = "cs.quest.wolf-offline-cancellation";
        let mut screen = GameFixes::default();
        let mut app = sse_app::state::AppState::new();
        app.set_selected_game(Some("cs".to_owned()));
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;

        screen.message(
            &mut cx,
            &Message::User(AppMessage::OpenGameFix {
                game_id: "cs".to_owned(),
                fix_id: fix_id.to_owned(),
            }),
            None,
        )?;

        assert_eq!(screen.selected.as_deref(), Some(fix_id));
        assert!(screen.intent.is_none(), "opening a linked fix must never install it");

        screen.message(
            &mut cx,
            &Message::User(AppMessage::ToScreen(
                ScreenId::GameFixes,
                Box::new(FixReply::List {
                    request_id: screen.list_request_id.saturating_sub(1),
                    game_id: Some("cs".to_owned()),
                    directory: Some(std::path::PathBuf::from("stale")),
                    selected_fix: None,
                    result: Ok(Vec::new()),
                }),
            )),
            None,
        )?;
        assert!(
            screen.items.is_empty(),
            "a stale discovery result must not replace the linked fix"
        );
        assert_eq!(cx.app.game_dir(), None);

        screen.message(
            &mut cx,
            &Message::User(AppMessage::ToScreen(
                ScreenId::GameFixes,
                Box::new(FixReply::List {
                    request_id: screen.list_request_id,
                    game_id: Some("cs".to_owned()),
                    directory: None,
                    selected_fix: Some(fix_id.to_owned()),
                    result: Ok(vec![fix_row(fix_id)?]),
                }),
            )),
            None,
        )?;

        assert_eq!(screen.selected.as_deref(), Some(fix_id));
        assert_eq!(screen.items.first().map(|item| item.id.as_str()), Some(fix_id));
        assert!(screen.intent.is_none());
        Ok(())
    }

    #[test]
    fn missing_game_folder_uses_a_discovered_installation_of_the_exact_release() -> Result<()> {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let root = std::env::temp_dir().join(format!("sse-game-fixes-{unique}"));
        let clear_sky = root.join("clear-sky");
        let call_of_pripyat = root.join("call-of-pripyat");
        std::fs::create_dir_all(&clear_sky)?;
        std::fs::create_dir_all(&call_of_pripyat)?;
        std::fs::write(clear_sky.join("fsgame_cs.ltx"), "")?;
        std::fs::write(call_of_pripyat.join("fsgame_cop.ltx"), "")?;
        let installations = vec![
            DiscoveredInstallation {
                target: GameTarget::CallOfPripyat,
                title: GameTarget::CallOfPripyat.title().to_owned(),
                directory: call_of_pripyat,
                source: GameInstallSource::Steam,
                build_id: None,
                save_count: 0,
            },
            DiscoveredInstallation {
                target: GameTarget::ClearSky,
                title: GameTarget::ClearSky.title().to_owned(),
                directory: clear_sky.clone(),
                source: GameInstallSource::Steam,
                build_id: None,
                save_count: 0,
            },
        ];

        let found = super::matching_game_installation(sse_fixes::GameTarget::ClearSky, None, &installations);

        assert_eq!(found.as_deref(), Some(clear_sky.as_path()));
        std::fs::remove_dir_all(root)?;
        Ok(())
    }
}

#[cfg(test)]
mod games_localization_tests {
    use super::{encyclopedia_entry_text, DoctorProgress, EncyclopediaDetail, EncyclopediaEntry, EncyclopediaKind};

    #[test]
    fn game_doctor_progress_and_encyclopedia_details_are_localized() {
        assert_eq!(DoctorProgress::BuildingTree.text("en"), "25% · building file tree");
        assert_eq!(DoctorProgress::StartingLinter.text("en"), "60% · starting linter");

        let entry = EncyclopediaEntry {
            kind: EncyclopediaKind::Item,
            key: "medkit".to_owned(),
            name: "Medkit".to_owned(),
            detail: EncyclopediaDetail::Item {
                category: None,
                source: "gamedata".to_owned(),
                cost: None,
            },
        };
        assert_eq!(encyclopedia_entry_text("en", &entry), "item · Medkit · medkit");
        assert_eq!(entry.detail.text("en"), "No category · gamedata · price: —");
    }
}

#[cfg(test)]
mod encyclopedia_result_tests {
    use super::{EncyclopediaDetail, EncyclopediaEntry, EncyclopediaKind, EncyclopediaResult};

    #[test]
    fn stale_game_or_generation_is_rejected_and_current_result_is_moved_once() {
        let stale_game = EncyclopediaResult {
            game: "stalker-cop".to_owned(),
            generation: 4,
            result: std::sync::Mutex::new(Some(Ok(vec![EncyclopediaEntry {
                kind: EncyclopediaKind::Item,
                key: "medkit".to_owned(),
                name: "Medkit".to_owned(),
                detail: EncyclopediaDetail::Item {
                    category: None,
                    source: "test".to_owned(),
                    cost: None,
                },
            }]))),
        };
        assert!(stale_game.take_if_current(Some("stalker-soc"), 4).is_none());

        let stale_generation = EncyclopediaResult {
            game: "stalker-cop".to_owned(),
            generation: 4,
            result: std::sync::Mutex::new(Some(Ok(Vec::new()))),
        };
        assert!(stale_generation.take_if_current(Some("stalker-cop"), 5).is_none());

        let current = EncyclopediaResult {
            game: "stalker-cop".to_owned(),
            generation: 5,
            result: std::sync::Mutex::new(Some(Ok(Vec::new()))),
        };
        assert!(matches!(
            current.take_if_current(Some("stalker-cop"), 5),
            Some(Ok(entries)) if entries.is_empty()
        ));
        assert!(current.take_if_current(Some("stalker-cop"), 5).is_none());
    }
}

#[cfg(test)]
mod steam_manifest_warning_tests {
    use super::report_steam_manifest_read_error;
    use sse_core::{Error, Result};
    use std::sync::Mutex;

    static TEST_GATE: Mutex<()> = Mutex::new(());

    #[test]
    fn unreadable_manifest_is_logged_without_its_path() -> Result<()> {
        let _guard = TEST_GATE
            .lock()
            .map_err(|_| Error::System("Steam manifest test gate poisoned".to_owned()))?;
        let root = std::env::temp_dir().join(format!("sse-manifest-warning-{}", std::process::id()));
        let logs = root.join("logs");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&logs)?;
        sse_app::diagnostics::configure_log_directory(Some(logs.clone()));

        let result = (|| -> Result<String> {
            report_steam_manifest_read_error(
                123,
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "simulated read failure"),
            );
            Ok(std::fs::read_to_string(logs.join("save-editor.log"))?)
        })();
        sse_app::diagnostics::configure_log_directory(None);
        let _ = std::fs::remove_dir_all(&root);
        let log = result?;

        assert!(log.contains(" WARN "));
        assert!(log.contains("app id 123"));
        assert!(!log.contains(&root.to_string_lossy().to_string()));
        Ok(())
    }
}

#[cfg(test)]
mod game_target_localization_tests {
    use super::{discovery_status_text, tr, DiscoveryStatus, GameInstallSource, GameTarget};

    #[test]
    fn game_titles_and_selected_install_source_follow_the_interface_language() {
        assert_eq!(
            GameTarget::ShadowOfChernobyl.title_in("en"),
            "S.T.A.L.K.E.R.: Shadow of Chornobyl"
        );
        assert_eq!(GameTarget::ClearSky.title_in("en"), "S.T.A.L.K.E.R.: Clear Sky");
        assert_eq!(
            GameTarget::CallOfPripyat.title_in("en"),
            "S.T.A.L.K.E.R.: Call of Prypiat"
        );
        assert_eq!(
            GameTarget::ShadowOfChernobylEnhancedEdition.title_in("en"),
            "Shadow of Chornobyl (Enhanced Edition)"
        );
        assert_eq!(
            GameTarget::ClearSkyEnhancedEdition.title_in("en"),
            "Clear Sky (Enhanced Edition)"
        );
        assert_eq!(
            GameTarget::CallOfPripyatEnhancedEdition.title_in("en"),
            "Call of Prypiat (Enhanced Edition)"
        );
        assert_eq!(
            GameTarget::Stalker2.title_in("en"),
            "S.T.A.L.K.E.R. 2: Heart of Chornobyl"
        );
        assert_eq!(GameInstallSource::Selected.display_in("en"), "Selected manually");
    }

    #[test]
    fn games_overview_header_and_installation_count_are_localized() {
        assert_eq!(crate::strings::t_in("en", "НАЙДЕННЫЕ УСТАНОВКИ"), "FOUND INSTALLATIONS");
        assert_eq!(discovery_status_text("en", &DiscoveryStatus::Found(2)), "●  FOUND 2");
    }

    #[test]
    fn environment_statuses_are_localized_with_dynamic_values() {
        assert_eq!(
            tr("en", "Управляемая установка: не выбрана", &[]),
            "Managed installation: none selected"
        );
        assert_eq!(
            tr(
                "en",
                "Профиль применён: {0} · резервная точка: {1}",
                &[&"default", &"snapshot-1"],
            ),
            "Profile applied: default · restore point: snapshot-1"
        );
    }

    #[test]
    fn game_fix_details_and_status_values_are_localized() {
        assert_eq!(crate::strings::t_in("en", "УСТАНОВЛЕНО"), "Installed");
        assert_eq!(
            tr(
                "en",
                "{0} · {1} · {2} / {3}\nПРОБЛЕМА: {4}\nИЗМЕНЕНИЕ: {5}\nПОДДЕРЖИВАЕМЫЕ STEAM-СБОРКИ: {6}\nЗАТРАГИВАЕМЫЕ ФАЙЛЫ: {7}\nИСТОЧНИК: {8}",
                &[
                    &"fix-1",
                    &"Installed",
                    &"essential",
                    &"verified",
                    &"startup crash",
                    &"patch applied",
                    &"4500",
                    &2,
                    &"audit",
                ],
            ),
            "fix-1 · Installed · essential / verified\nPROBLEM: startup crash\nCHANGE: patch applied\nSUPPORTED STEAM BUILDS: 4500\nAFFECTED FILES: 2\nSOURCE: audit"
        );
    }
}
