//! S4 (Gemini): installed games, fixes, game doctor, environment, encyclopedia.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Placeholder, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::Result;
use sse_storage::discovery::{normalize_full_path, resolve_links, SaveDirectoryCandidate, SaveDirectoryLocator};
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
        Box::new(Placeholder::new(ScreenId::GameDoctor, "Проверка установки игры")),
        Box::new(Placeholder::new(ScreenId::Environment, "Инструменты и среда игры")),
        Box::new(Placeholder::new(ScreenId::Encyclopedia, "Предметы, персонажи, локации")),
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

    /// Full Russian title.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::ShadowOfChernobyl => "S.T.A.L.K.E.R.: Тень Чернобыля",
            Self::ClearSky => "S.T.A.L.K.E.R.: Чистое Небо",
            Self::CallOfPripyat => "S.T.A.L.K.E.R.: Зов Припяти",
            Self::ShadowOfChernobylEnhancedEdition => "Тень Чернобыля Enhanced Edition",
            Self::ClearSkyEnhancedEdition => "Чистое Небо Enhanced Edition",
            Self::CallOfPripyatEnhancedEdition => "Зов Припяти Enhanced Edition",
            Self::Stalker2 => "S.T.A.L.K.E.R. 2: Heart of Chornobyl",
        }
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
    pub const fn display(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Gog => "GOG",
            Self::Retail => "GSC Retail",
            Self::Heroic => "Heroic",
            Self::Selected => "Выбрана вручную",
        }
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
    selected_index: Option<usize>,
    status_message: Option<String>,
}

/// Message payload passed from background discovery thread to GamesOverview screen.
#[derive(Clone, Debug)]
pub struct DiscoveredResult {
    /// Found game installations.
    pub installations: Vec<DiscoveredInstallation>,
    /// Summary status message.
    pub status: String,
}

const MAX_INSTALLATION_ROWS: usize = 6;

/// Installed games overview screen (ScreenId::Games).
pub struct GamesOverview {
    workspace: Workspace,
    // Left card: "МОИ ИГРЫ" / "НАЙДЕННЫЕ УСТАНОВКИ"
    discover_button: Option<WidgetId>,
    installations_count: Option<WidgetId>,
    discovery_status: Option<WidgetId>,
    rows: Vec<WidgetId>,
    empty_panel: Option<WidgetId>,
    empty_search_hint: Option<WidgetId>,
    empty_doctor_button: Option<WidgetId>,

    // Right card: "ВЫБРАННАЯ ИГРА"
    target_prev_button: Option<WidgetId>,
    target_next_button: Option<WidgetId>,
    target_title: Option<WidgetId>,
    status_value: Option<WidgetId>,
    platform_value: Option<WidgetId>,
    build_value: Option<WidgetId>,
    folder_value: Option<WidgetId>,
    saves_value: Option<WidgetId>,
    open_folder_button: Option<WidgetId>,

    // Bottom card: "БЫСТРЫЕ ДЕЙСТВИЯ"
    action_fixes: Option<WidgetId>,
    action_doctor: Option<WidgetId>,
    action_environment: Option<WidgetId>,
    action_encyclopedia: Option<WidgetId>,

    // Mod notice button
    mods_button: Option<WidgetId>,
}

impl GamesOverview {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            discover_button: None,
            installations_count: None,
            discovery_status: None,
            rows: Vec::new(),
            empty_panel: None,
            empty_search_hint: None,
            empty_doctor_button: None,

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
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (idle, discovering, installations, selected_target, selected_index, status_message) = {
            let state = self.workspace.lock();
            (
                state.idle,
                state.discovering,
                state.installations.clone(),
                state.selected_target.unwrap_or(GameTarget::ShadowOfChernobyl),
                state.selected_index,
                state.status_message.clone(),
            )
        };

        // Update count label
        if let Some(id) = self.installations_count {
            let count_text = format!("Найдено: {}", installations.len());
            cx.tree.set_text(id, &count_text)?;
        }

        // Update discovery status text
        if let Some(id) = self.discovery_status {
            let text = if discovering {
                "Поиск установок…".to_owned()
            } else if let Some(msg) = status_message {
                msg
            } else if idle {
                "Поиск установок ещё не выполнялся.".to_owned()
            } else if installations.is_empty() {
                "●  НЕ НАЙДЕНО".to_owned()
            } else {
                format!("●  НАЙДЕНО {}", installations.len())
            };
            cx.tree.set_text(id, &text)?;
        }

        // Show/hide empty state vs installations list
        let has_installations = !installations.is_empty();
        if let Some(empty) = self.empty_panel {
            cx.tree.set_visible(empty, !has_installations)?;
        }

        // Update installation rows
        for i in 0..MAX_INSTALLATION_ROWS {
            if let Some(row_id) = self.rows.get(i).copied() {
                if let Some(install) = installations.get(i) {
                    cx.tree.set_visible(row_id, true)?;
                    let is_selected = selected_index == Some(i);
                    let prefix = if is_selected { "> " } else { "  " };
                    let path_str = install.directory.to_string_lossy();
                    let shortened = short_text(&path_str, 24);
                    let row_text = format!(
                        "{prefix}{} [{}] · {} · сейвов: {}",
                        install.title,
                        install.source.display(),
                        shortened,
                        install.save_count
                    );
                    cx.tree.set_text(row_id, &row_text)?;
                } else {
                    cx.tree.set_visible(row_id, false)?;
                }
            }
        }

        // Selected installation details
        let selected_install = selected_index.and_then(|idx| installations.get(idx));

        if let Some(id) = self.target_title {
            let title = if let Some(install) = selected_install {
                install.title.clone()
            } else {
                selected_target.title().to_owned()
            };
            cx.tree.set_text(id, &title)?;
        }

        if let Some(id) = self.status_value {
            let text = if selected_install.is_some() {
                "Установка найдена"
            } else {
                "Не выбрана"
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
            let shortened = short_text(&text, 48);
            cx.tree.set_text(id, &shortened)?;
        }

        if let Some(id) = self.saves_value {
            let text = selected_install.map_or("—".to_owned(), |inst| inst.save_count.to_string());
            cx.tree.set_text(id, &text)?;
        }

        Ok(())
    }
}

impl Screen for GamesOverview {
    fn id(&self) -> ScreenId {
        ScreenId::Games
    }

    fn subtitle(&self) -> &str {
        "Найденные установки, версии и быстрые действия для каждой игры"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        // Main two-column or stacked layout:
        // Left: НАЙДЕННЫЕ УСТАНОВКИ (card)
        // Right: ВЫБРАННАЯ ИГРА (card)
        let main_row_style = crate::layout::Style {
            gap: crate::layout::Size::new(12.0, 12.0),
            align_items: crate::layout::Align::Stretch,
            ..crate::layout::Style::default()
        };
        let main_row = cx.tree.add(
            Some(host),
            crate::layout::NodeKind::Row,
            main_row_style,
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;

        // --- LEFT CARD: НАЙДЕННЫЕ УСТАНОВКИ ---
        let left_card = style::card(cx.tree, main_row)?;
        let heading_row = style::row(cx.tree, left_card)?;
        style::label(cx.tree, heading_row, "НАЙДЕННЫЕ УСТАНОВКИ", Text::Heading)?;
        self.installations_count = Some(style::label(cx.tree, heading_row, "Найдено: 0", Text::Note)?);
        self.discover_button = Some(style::button(cx.tree, heading_row, "Найти установки", Button::Primary)?);

        self.discovery_status = Some(style::label(
            cx.tree,
            left_card,
            "Поиск установок ещё не выполнялся.",
            Text::Note,
        )?);

        // Installation rows container
        let list_container = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                gap: crate::layout::Size::new(0.0, 4.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;

        for _ in 0..MAX_INSTALLATION_ROWS {
            let row_button = style::button(cx.tree, list_container, "", Button::Secondary)?;
            cx.tree.set_visible(row_button, false)?;
            self.rows.push(row_button);
        }

        // Empty state pane (shown when no installations)
        let empty_card = cx.tree.add(
            Some(left_card),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                padding: crate::layout::Edges::all(14.0),
                gap: crate::layout::Size::new(0.0, 6.0),
                align_items: crate::layout::Align::Center,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        self.empty_panel = Some(empty_card);

        style::label(cx.tree, empty_card, "Установки не выбраны", Text::Heading)?;
        self.empty_search_hint = Some(style::label(
            cx.tree,
            empty_card,
            "Нажмите «Найти установки», чтобы проверить поддерживаемые игры на этом компьютере.",
            Text::Note,
        )?);
        self.empty_doctor_button = Some(style::button(
            cx.tree,
            empty_card,
            "Открыть Доктор игры",
            Button::Secondary,
        )?);

        // --- RIGHT CARD: ВЫБРАННАЯ ИГРА ---
        let right_card = style::card(cx.tree, main_row)?;
        let right_header = style::row(cx.tree, right_card)?;
        style::label(cx.tree, right_header, "ВЫБРАННАЯ ИГРА", Text::Heading)?;

        self.target_prev_button = Some(style::button(cx.tree, right_header, "<", Button::Secondary)?);
        self.target_next_button = Some(style::button(cx.tree, right_header, ">", Button::Secondary)?);

        self.target_title = Some(style::label(
            cx.tree,
            right_card,
            GameTarget::ShadowOfChernobyl.title(),
            Text::Title,
        )?);

        let details_table = style::card(cx.tree, right_card)?;

        let row1 = style::row(cx.tree, details_table)?;
        style::label(cx.tree, row1, "Статус:        ", Text::Note)?;
        self.status_value = Some(style::label(cx.tree, row1, "Не выбрана", Text::Value)?);

        let row2 = style::row(cx.tree, details_table)?;
        style::label(cx.tree, row2, "Платформа:     ", Text::Note)?;
        self.platform_value = Some(style::label(cx.tree, row2, "—", Text::Value)?);

        let row3 = style::row(cx.tree, details_table)?;
        style::label(cx.tree, row3, "Номер сборки:  ", Text::Note)?;
        self.build_value = Some(style::label(cx.tree, row3, "—", Text::Value)?);

        let row4 = style::row(cx.tree, details_table)?;
        style::label(cx.tree, row4, "Папка игры:    ", Text::Note)?;
        self.folder_value = Some(style::label(cx.tree, row4, "—", Text::Value)?);

        let row5 = style::row(cx.tree, details_table)?;
        style::label(cx.tree, row5, "Число сейвов:  ", Text::Note)?;
        self.saves_value = Some(style::label(cx.tree, row5, "—", Text::Value)?);

        self.open_folder_button = Some(style::button(cx.tree, right_card, "Открыть папку", Button::Secondary)?);

        // --- BOTTOM ACTIONS: БЫСТРЫЕ ДЕЙСТВИЯ ---
        let bottom_card = style::card(cx.tree, host)?;
        style::label(cx.tree, bottom_card, "БЫСТРЫЕ ДЕЙСТВИЯ", Text::Heading)?;
        let actions_row = style::row(cx.tree, bottom_card)?;

        self.action_fixes = Some(style::button(cx.tree, actions_row, "Исправления", Button::Secondary)?);
        self.action_doctor = Some(style::button(cx.tree, actions_row, "Доктор игры", Button::Secondary)?);
        self.action_environment = Some(style::button(cx.tree, actions_row, "Среда игры", Button::Secondary)?);
        self.action_encyclopedia = Some(style::button(cx.tree, actions_row, "Энциклопедия", Button::Secondary)?);

        // --- MODS NOTICE: МОДЫ ---
        let mods_card = style::card(cx.tree, host)?;
        let mods_row = style::row(cx.tree, mods_card)?;
        self.mods_button = Some(style::button(cx.tree, mods_row, "Моды", Button::Secondary)?);
        style::label(
            cx.tree,
            mods_row,
            "Отдельный менеджер модов отсутствует; используйте профили в разделе «Среда игры».",
            Text::Note,
        )?;

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
        // 1. Check if user clicked "Найти установки"
        if clicked.is_some() && clicked == self.discover_button {
            start_background_discovery(&self.workspace, cx);
            return self.render(cx);
        }

        // 2. Check if user clicked "Открыть Доктор игры"
        if clicked.is_some() && (clicked == self.empty_doctor_button || clicked == self.action_doctor) {
            cx.status = Some("Переход в раздел «Доктор игры»".to_owned());
            return Ok(());
        }

        // 3. Quick action buttons
        if clicked.is_some() && clicked == self.action_fixes {
            cx.status = Some("Переход в раздел «Исправления игры»".to_owned());
            return Ok(());
        }
        if clicked.is_some() && clicked == self.action_environment {
            cx.status = Some("Переход в раздел «Среда игры»".to_owned());
            return Ok(());
        }
        if clicked.is_some() && clicked == self.action_encyclopedia {
            cx.status = Some("Переход в раздел «Энциклопедия»".to_owned());
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
            state.selected_index = state.installations.iter().position(|inst| inst.target == new_target);
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
            state.selected_index = state.installations.iter().position(|inst| inst.target == new_target);
            drop(state);
            return self.render(cx);
        }

        // 5. Click on an installation row
        for (i, row_id) in self.rows.iter().enumerate() {
            if clicked.is_some() && clicked == Some(*row_id) {
                let mut state = self.workspace.lock();
                if let Some(inst) = state.installations.get(i) {
                    state.selected_target = Some(inst.target);
                    state.selected_index = Some(i);
                }
                drop(state);
                return self.render(cx);
            }
        }

        // 6. Click on "Открыть папку"
        if clicked.is_some() && clicked == self.open_folder_button {
            let state = self.workspace.lock();
            let selected_install = state.selected_index.and_then(|idx| state.installations.get(idx));
            if let Some(install) = selected_install {
                cx.status = Some(format!("Папка игры: {}", install.directory.display()));
            } else {
                cx.status = Some("Установка игры не выбрана".to_owned());
            }
            return Ok(());
        }

        // 7. Click on "Моды" note button
        if clicked.is_some() && clicked == self.mods_button {
            cx.status = Some("Отдельный менеджер модов отсутствует; используйте «Среда игры».".to_owned());
            return Ok(());
        }

        // 8. Result of background discovery
        if let Message::User(AppMessage::ToScreen(ScreenId::Games, payload)) = message {
            if let Some(result) = payload.downcast_ref::<DiscoveredResult>() {
                let mut state = self.workspace.lock();
                state.discovering = false;
                state.installations.clone_from(&result.installations);
                state.status_message = Some(result.status.clone());
                if state.selected_index.is_none() && !state.installations.is_empty() {
                    state.selected_index = Some(0);
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
            "●  НЕ НАЙДЕНО".to_owned()
        } else {
            format!("●  НАЙДЕНО {}", found.len())
        };
        let mut state = workspace.lock();
        state.discovering = false;
        state.installations = found;
        state.status_message = Some(status);
        if state.selected_index.is_none() && !state.installations.is_empty() {
            state.selected_index = Some(0);
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
        state.status_message = Some("Поиск установок…".to_owned());
    }

    let workspace_clone = workspace.clone();
    std::thread::spawn(move || {
        let found = discover_all_installations();
        let status = if found.is_empty() {
            "●  НЕ НАЙДЕНО".to_owned()
        } else {
            format!("●  НАЙДЕНО {}", found.len())
        };
        let result = DiscoveredResult {
            installations: found,
            status,
        };
        proxy.send(AppMessage::ToScreen(ScreenId::Games, Box::new(result)));
        let mut state = workspace_clone.lock();
        state.discovering = false;
    });

    cx.status = Some("Поиск установок игр на диске…".to_owned());
}

/// Discovers installations across Steam libraries, GOG, Heroic, and known standard paths.
#[must_use]
pub fn discover_all_installations() -> Vec<DiscoveredInstallation> {
    let mut installations = Vec::new();
    let mut seen_dirs = HashSet::new();

    // 1. Steam discovery
    let steam_roots = SaveDirectoryLocator::default_steam_roots();
    let libraries = get_steam_libraries_list(&steam_roots);

    for target in GameTarget::ALL {
        if let Some(app_id) = target.steam_app_id() {
            for library in &libraries {
                if let Some(install_dir) = find_manifest_install_directory(library, app_id) {
                    if has_expected_marker(target, &install_dir) {
                        let build_id = try_read_steam_build_id(library, app_id);
                        add_installation(
                            &mut installations,
                            &mut seen_dirs,
                            target,
                            install_dir,
                            GameInstallSource::Steam,
                            build_id,
                        );
                    }
                }

                // Check common folder fallback under steamapps/common
                for install_name in target.install_directories() {
                    let common_path = library.join("steamapps").join("common").join(install_name);
                    if has_expected_marker(target, &common_path) {
                        let build_id = try_read_steam_build_id(library, app_id);
                        add_installation(
                            &mut installations,
                            &mut seen_dirs,
                            target,
                            common_path,
                            GameInstallSource::Steam,
                            build_id,
                        );
                    }
                }
            }
        }
    }

    // 2. Non-Steam discovery (GOG, Heroic, standard user directories)
    discover_non_steam_installations(&mut installations, &mut seen_dirs);

    // 3. Count saves for each discovered installation
    count_saves_for_installations(&mut installations);

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
        if let Ok(vdf_text) = fs::read_to_string(&vdf_file) {
            for path_str in parse_vdf_library_paths(&vdf_text) {
                let full_path = normalize_full_path(Path::new(&path_str));
                let resolved_path = resolve_links(&full_path);
                if resolved_path.is_dir() && seen.insert(resolved_path.clone()) {
                    libraries.push(resolved_path);
                }
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

fn find_manifest_install_directory(library_root: &Path, app_id: u32) -> Option<PathBuf> {
    let manifest = library_root.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
    let content = fs::read_to_string(&manifest).ok()?;
    let parsed_app_id = parse_acf_string_value(&content, "appid")?;
    if parsed_app_id.trim() != app_id.to_string() {
        return None;
    }
    let install_dir = parse_acf_string_value(&content, "installdir")?;
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

fn try_read_steam_build_id(library_root: &Path, app_id: u32) -> Option<String> {
    let manifest = library_root.join("steamapps").join(format!("appmanifest_{app_id}.acf"));
    let content = fs::read_to_string(&manifest).ok()?;
    parse_acf_string_value(&content, "buildid").map(|s| s.trim().to_owned())
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
    // Collect all candidate save directories from sse_storage
    let candidates = SaveDirectoryLocator::find_candidate_directories(None);

    for install in installations.iter_mut() {
        let release_id = install.target.release_id();
        let family_id = install.target.family();

        // Candidates matching this release
        let matching_candidates: Vec<SaveDirectoryCandidate> = candidates
            .iter()
            .filter(|c| c.release_id == release_id || c.game_id == family_id)
            .cloned()
            .collect();

        // Check also relative save folder if fsgame.ltx resolves inside install dir
        let mut save_paths = Vec::new();
        for candidate in &matching_candidates {
            save_paths.push(candidate.directory_path.clone());
        }

        // Add local game data saves if found
        let local_appdata = install.directory.join("_appdata_").join("savedgames");
        if local_appdata.is_dir() {
            save_paths.push(local_appdata);
        }

        let mut count = 0_usize;
        let mut seen_slots = HashSet::new();

        for save_path in save_paths {
            if let Ok(entries) = fs::read_dir(&save_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        let lower = file_name.to_ascii_lowercase();
                        if (lower.ends_with(".sav") || lower.ends_with(".scop") || lower.ends_with(".scs"))
                            && lower != "campaignssave.sav"
                            && lower != "analyticsdata.sav"
                        {
                            let key = path.to_string_lossy().to_string();
                            if seen_slots.insert(key) {
                                count = count.saturating_add(1);
                            }
                        }
                    }
                }
            }
        }

        install.save_count = count;
    }
}

fn short_text(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let keep = max_chars.saturating_sub(1);
    let mut result = text.chars().take(keep).collect::<String>();
    result.push('…');
    result
}

#[derive(Clone, Debug)]
struct FixRow {
    id: String,
    title: String,
    description: String,
    version: String,
    status: String,
}

#[derive(Debug)]
enum FixReply {
    List(std::result::Result<Vec<FixRow>, String>),
    Changed(std::result::Result<String, String>),
}

#[derive(Default)]
struct GameFixes {
    status: Option<WidgetId>,
    detail: Option<WidgetId>,
    rows: Vec<WidgetId>,
    items: Vec<FixRow>,
    selected: Option<usize>,
    install: Option<WidgetId>,
    remove: Option<WidgetId>,
    confirm: Option<bool>,
}

fn fix_target(game: &str) -> Option<sse_fixes::GameTarget> {
    sse_fixes::GameTarget::parse(game).or_else(|| match game {
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

impl GameFixes {
    fn refresh(&self, cx: &mut Context<'_>) {
        let game = cx.app.selected_game().map(str::to_owned);
        let directory = cx.app.game_dir().map(Path::to_path_buf);
        let Some(proxy) = cx.proxy.cloned() else { return };
        std::thread::spawn(move || {
            let result = (|| {
                let target = game
                    .as_deref()
                    .and_then(fix_target)
                    .ok_or_else(|| "Выбранная игра не поддерживается каталогом фиксов".to_owned())?;
                let directory = directory.ok_or_else(|| "Сначала выберите установленную игру".to_owned())?;
                let engine = sse_fixes::GameFixEngine::new();
                let installed = engine.list_installed(&directory, None).map_err(|e| e.to_string())?;
                let rows = sse_fixes::GameFixCatalog::for_game(target)
                    .into_iter()
                    .map(|definition| {
                        let current = installed.iter().find(|item| item.id == definition.id);
                        let status = if let Some(item) = current {
                            if item.version != definition.version {
                                format!("устарел {} → {}", item.version, definition.version)
                            } else {
                                match engine.get_status(definition, &directory) {
                                    Ok(sse_fixes::GameFixState::Installed) => "установлен".to_owned(),
                                    Ok(sse_fixes::GameFixState::Modified) => "изменён".to_owned(),
                                    Ok(_) => "не установлен".to_owned(),
                                    Err(error) => format!("ошибка: {error}"),
                                }
                            }
                        } else {
                            "не установлен".to_owned()
                        };
                        FixRow {
                            id: definition.id.clone(),
                            title: definition.title.clone(),
                            description: definition.description.clone(),
                            version: definition.version.clone(),
                            status,
                        }
                    })
                    .collect();
                Ok(rows)
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::GameFixes,
                Box::new(FixReply::List(result)),
            ));
        });
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if let Some(status) = self.status {
            cx.tree.set_text(status, &format!("Фиксов: {}", self.items.len()))?;
        }
        for (index, widget) in self.rows.iter().copied().enumerate() {
            if let Some(item) = self.items.get(index) {
                cx.tree.set_visible(widget, true)?;
                cx.tree
                    .set_text(widget, &format!("{} · v{} · {}", item.title, item.version, item.status))?;
            } else {
                cx.tree.set_visible(widget, false)?;
            }
        }
        Ok(())
    }
}

impl Screen for GameFixes {
    fn id(&self) -> ScreenId {
        ScreenId::GameFixes
    }
    fn subtitle(&self) -> &str {
        "Каталог исправлений выбранной игры с транзакционной установкой"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ИСПРАВЛЕНИЯ ИГРЫ", Text::Heading)?;
        self.status = Some(style::label(cx.tree, card, "Выберите игру", Text::Note)?);
        for _ in 0..8 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        self.detail = Some(style::label(cx.tree, card, "Выберите исправление", Text::Body)?);
        let actions = style::row(cx.tree, card)?;
        self.install = Some(style::button(
            cx.tree,
            actions,
            "Установить / обновить",
            Button::Primary,
        )?);
        self.remove = Some(style::button(cx.tree, actions, "Удалить", Button::Secondary)?);
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
        if clicked.is_some() {
            for (index, row) in self.rows.iter().copied().enumerate() {
                if clicked == Some(row) && self.items.get(index).is_some() {
                    self.selected = Some(index);
                    self.confirm = None;
                    if let (Some(detail), Some(item)) = (self.detail, self.items.get(index)) {
                        cx.tree
                            .set_text(detail, &format!("{}\n{}", item.status, item.description))?;
                    }
                }
            }
        }

        let action = if clicked.is_some() && clicked == self.install {
            Some(true)
        } else if clicked.is_some() && clicked == self.remove {
            Some(false)
        } else {
            None
        };
        if let Some(install) = action {
            let Some(index) = self.selected else {
                cx.status = Some("Сначала выберите исправление".to_owned());
                return Ok(());
            };
            if self.confirm != Some(install) {
                self.confirm = Some(install);
                cx.status =
                    Some("Операция изменит файлы игры. Нажмите ту же кнопку ещё раз для подтверждения.".to_owned());
                return Ok(());
            }
            self.confirm = None;
            let Some(item) = self.items.get(index) else {
                return Ok(());
            };
            let fix_id = item.id.clone();
            let game = cx.app.selected_game().map(str::to_owned);
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let directory = directory.ok_or_else(|| "Папка игры не выбрана".to_owned())?;
                    let target = game
                        .as_deref()
                        .and_then(fix_target)
                        .ok_or_else(|| "Игра не поддерживается".to_owned())?;
                    let definition = sse_fixes::GameFixCatalog::try_get(&fix_id)
                        .ok_or_else(|| "Фикс исчез из каталога".to_owned())?;
                    if definition.game != target {
                        return Err("Фикс не относится к выбранной игре".to_owned());
                    }
                    let engine = sse_fixes::GameFixEngine::new();
                    let result = if install {
                        match engine.get_status(definition, &directory).map_err(|e| e.to_string())? {
                            sse_fixes::GameFixState::Installed => engine.update(definition, &directory),
                            _ => engine.install(definition, &directory),
                        }
                    } else {
                        let check = engine.check_uninstall(&fix_id, &directory);
                        if !check.can_uninstall {
                            return Err(check
                                .reason
                                .unwrap_or_else(|| "Безопасное удаление запрещено".to_owned()));
                        }
                        engine.uninstall(&fix_id, &directory)
                    }
                    .map_err(|e| e.to_string())?;
                    Ok(format!(
                        "{}: {:?}; файлов: {}",
                        fix_id,
                        result.state,
                        result.files.len()
                    ))
                })();
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameFixes,
                    Box::new(FixReply::Changed(result)),
                ));
            });
        }

        if let Message::User(AppMessage::ToScreen(ScreenId::GameFixes, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<FixReply>() {
                match reply {
                    FixReply::List(Ok(items)) => {
                        self.items.clone_from(items);
                        self.render(cx)?;
                    }
                    FixReply::List(Err(error)) => {
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, error)?;
                        }
                    }
                    FixReply::Changed(Ok(text)) => {
                        cx.status = Some(text.clone());
                        self.refresh(cx);
                    }
                    FixReply::Changed(Err(error)) => cx.status = Some(format!("Исправления: {error}")),
                }
            }
        }
        Ok(())
    }
}
