//! S4 (Gemini): installed games, fixes, game doctor, environment, encyclopedia.
//!
//! Replace each placeholder with a struct implementing [`Screen`]; keep the order.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::{Message, WindowEvent};
use crate::text::{self, Metrics};
use crate::widget::WidgetId;
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
    selected_installation: Option<PathBuf>,
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
                    let is_selected = selected_installation.as_ref() == Some(&install.directory);
                    let prefix = if is_selected { "> " } else { "  " };
                    let path_str = install.directory.to_string_lossy();
                    let shortened = text::ellipsize_middle(&path_str, 420.0, &PathMetrics);
                    let row_text = format!(
                        "{prefix}{} [{}] · сейвов: {}\n   {}",
                        install.title,
                        install.source.display(),
                        install.save_count,
                        shortened
                    );
                    cx.tree.set_text(row_id, &row_text)?;
                } else {
                    cx.tree.set_visible(row_id, false)?;
                }
            }
        }

        // Selected installation details
        let selected_install = selected_installation
            .as_ref()
            .and_then(|id| installations.iter().find(|install| &install.directory == id));

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

        // 5. Click on an installation row
        for (i, row_id) in self.rows.iter().enumerate() {
            if clicked.is_some() && clicked == Some(*row_id) {
                let mut state = self.workspace.lock();
                if let Some((target, directory)) = state
                    .installations
                    .get(i)
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
            "●  НЕ НАЙДЕНО".to_owned()
        } else {
            format!("●  НАЙДЕНО {}", found.len())
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
    let candidates = SaveDirectoryLocator::find_candidate_directories(None);
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
}

impl Environment {
    fn refresh_lists(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.snapshot_ids.clear();
        if let Some(directory) = cx.app.game_dir() {
            let snapshots = sse_fixes::toolkit::ToolkitSnapshotService::list_snapshots(directory)?;
            for (index, widget) in self.snapshot_rows.iter().copied().enumerate() {
                if let Some(snapshot) = snapshots.get(index) {
                    self.snapshot_ids.push(snapshot.id.clone());
                    cx.tree.set_text(
                        widget,
                        &format!(
                            "{} · {} · исправлений: {}",
                            snapshot.label,
                            snapshot.game.title(),
                            snapshot.installed_fixes.len()
                        ),
                    )?;
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
                cx.tree.set_text(
                    widget,
                    &format!(
                        "{} · {} · исправлений: {}",
                        profile.profile.name,
                        profile.profile.game.title(),
                        profile.profile.target_fix_ids.len()
                    ),
                )?;
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
            .ok_or_else(|| "Сначала выберите игру в «Обзоре игр»".to_owned())?;
        let directory = cx
            .app
            .game_dir()
            .map(Path::to_path_buf)
            .ok_or_else(|| "Папка игры не выбрана".to_owned())?;
        let target = match game {
            "soc" | "stalker-soc" | "soc-ee" | "stalker-soc-ee" => sse_content::CompanionGame::ShadowOfChernobyl,
            "cs" | "clear_sky" | "stalker-cs" | "cs-ee" | "stalker-cs-ee" => sse_content::CompanionGame::ClearSky,
            "cop" | "stalker-cop" | "cop-ee" | "stalker-cop-ee" => sse_content::CompanionGame::CallOfPripyat,
            _ => return Err("Среда X-Ray для выбранной игры не применяется".to_owned()),
        };
        Ok((target, directory))
    }

    fn start(&self, cx: &mut Context<'_>) {
        let Ok((game, directory)) = Self::inspect(cx) else {
            if let Some(id) = self.status {
                let _ = cx.tree.set_text(id, "Сначала выберите X-Ray игру в «Обзоре игр»");
            }
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        std::thread::spawn(move || {
            let search = sse_content::CompanionArchiveLocator::discover(&directory, &["fsgame.ltx"], game);
            let gamedata = search.game_data_directory.as_ref().is_some_and(|path| path.is_dir());
            let mods = directory.join("mods");
            let unpacked = search.game_data_directory.as_ref().map_or(0_usize, |root| {
                std::fs::read_dir(root).map_or(0, |entries| entries.flatten().count())
            });
            let mut lines = vec![
                format!(
                    "fsgame.ltx: {}",
                    search
                        .fsgame_path
                        .as_ref()
                        .map_or("не найден".to_owned(), |p| p.display().to_string())
                ),
                format!("gamedata: {}", if gamedata { "найдена" } else { "нет" }),
                format!(
                    "mods: {}",
                    if mods.is_dir() {
                        mods.display().to_string()
                    } else {
                        "нет".to_owned()
                    }
                ),
                format!("распакованные файлы/папки в gamedata: {unpacked}"),
                format!("архивов обнаружено: {}", search.archive_paths.len()),
            ];
            lines.extend(search.issues.into_iter().map(|issue| format!("⚠ {issue}")));
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
        "Что найдено в установке; экран ничего не изменяет"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "СРЕДА ИГРЫ", Text::Heading)?;
        self.status = Some(style::label(
            cx.tree,
            card,
            "Управляемая установка: не выбрана",
            Text::Note,
        )?);
        style::label(cx.tree, card, "УПРАВЛЯЕМЫЕ СНИМКИ", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "Снимки включают только файлы и манифесты Game Fix, Companion и настроек, которыми владеет инструмент.",
            Text::Note,
        )?;
        for _ in 0..6 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.snapshot_rows.push(row);
        }
        self.snapshot = Some(style::button(cx.tree, card, "СОЗДАТЬ СНИМОК", Button::Primary)?);
        self.restore = Some(style::button(cx.tree, card, "ВОССТАНОВИТЬ ВЫБРАННЫЙ", Button::Danger)?);
        self.delete_snapshot = Some(style::button(cx.tree, card, "УДАЛИТЬ СНИМОК", Button::Danger)?);
        style::label(cx.tree, card, "ПРОФИЛИ ИГРЫ", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "Профиль хранит набор Game Fix и управляемые значения user.ltx.",
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
            "СОХРАНИТЬ ТЕКУЩЕЕ СОСТОЯНИЕ",
            Button::Primary,
        )?);
        self.apply_profile = Some(style::button(cx.tree, card, "ПРИМЕНИТЬ ПРОФИЛЬ", Button::Secondary)?);
        self.delete_profile = Some(style::button(cx.tree, card, "УДАЛИТЬ ПРОФИЛЬ", Button::Danger)?);
        style::label(cx.tree, card, "НАСТРОЙКИ user.ltx", Text::Heading)?;
        style::label(
            cx.tree,
            card,
            "Изменяются только известные параметры; остальные строки user.ltx сохраняются без изменений.",
            Text::Note,
        )?;
        for setting in sse_fixes::toolkit::MANAGED_SETTINGS.iter().take(8) {
            let row = style::row(cx.tree, card)?;
            style::label(cx.tree, row, setting.key, Text::Body)?;
            let input = style::input(cx.tree, row, "")?;
            let apply = style::button(cx.tree, row, "ПРИМЕНИТЬ", Button::Secondary)?;
            let default = style::button(cx.tree, row, "ПО УМОЛЧАНИЮ", Button::Secondary)?;
            self.user_inputs.push((setting.key.to_owned(), input, apply, default));
        }
        style::label(cx.tree, card, "АУДИТ УСТАНОВКИ", Text::Heading)?;
        self.audit = Some(style::button(cx.tree, card, "ПРОВЕРИТЬ", Button::Secondary)?);
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
        let game = cx.app.selected_game().unwrap_or("—");
        let directory = cx
            .app
            .game_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "не выбрана".to_owned());
        if let Some(status) = self.status {
            cx.tree
                .set_text(status, &format!("Управляемая установка: {game} · {directory}"))?;
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
                    cx.status = Some(format!("Выбран снимок: {id}"));
                }
                return Ok(());
            }
            if let Some(index) = self.profile_rows.iter().position(|widget| *widget == clicked) {
                self.selected_profile = self.profile_ids.get(index).cloned();
                if let Some(id) = &self.selected_profile {
                    cx.status = Some(format!("Выбран профиль: {id}"));
                }
                return Ok(());
            }
            if Some(clicked) == self.delete_snapshot {
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    cx.status = Some("Управляемая установка: не выбрана".to_owned());
                    return Ok(());
                };
                let selected = self.selected_snapshot.clone();
                match selected.map_or(Ok(None), |id| {
                    sse_fixes::toolkit::ToolkitSnapshotService::delete_snapshot(&directory, &id)?;
                    Ok(Some(id))
                }) {
                    Ok(Some(id)) => cx.status = Some(format!("Снимок удалён: {id}")),
                    Ok(None) => cx.status = Some("Снимков пока нет: создайте первый кнопкой ниже.".to_owned()),
                    Err(error) => cx.status = Some(format!("Не удалось удалить снимок: {error}")),
                }
                self.selected_snapshot = None;
                self.refresh_lists(cx)?;
                return Ok(());
            }
            if Some(clicked) == self.save_profile {
                let Some(game) = cx.app.selected_game().and_then(fix_target) else {
                    cx.status = Some("Управляемая установка: не выбрана".to_owned());
                    return Ok(());
                };
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    cx.status = Some("Управляемая установка: не выбрана".to_owned());
                    return Ok(());
                };
                let name = self
                    .profile_name
                    .and_then(|id| cx.tree.input_text(id).ok())
                    .unwrap_or("")
                    .trim()
                    .to_owned();
                if name.is_empty() {
                    cx.status = Some("Введите имя профиля.".to_owned());
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
                        cx.status = Some(format!("Профиль сохранён: {name}"));
                    }
                    Err(error) => cx.status = Some(format!("Не удалось сохранить профиль: {error}")),
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
                    cx.status = Some("Сначала выберите профиль.".to_owned());
                    return Ok(());
                };
                let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
                    return Ok(());
                };
                let Some(proxy) = cx.proxy.cloned() else {
                    return Ok(());
                };
                std::thread::spawn(move || {
                    let engine = sse_fixes::GameFixEngine::new();
                    let catalog = sse_fixes::GameFixCatalog;
                    let lines = match sse_fixes::toolkit::ToolkitProfileService::apply_profile(
                        &directory,
                        &selected.profile,
                        &engine,
                        &catalog,
                    ) {
                        Ok(report) => vec![format!(
                            "Профиль применён: {} · резервная точка: {}",
                            report.profile_name, report.pre_switch_snapshot_id
                        )],
                        Err(error) => vec![format!("Не удалось применить профиль: {error}")],
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
                    match sse_fixes::toolkit::ToolkitProfileService::delete_profile(
                        &sse_app::paths::default_data_directory(),
                        &selected.id,
                    ) {
                        Ok(()) => {
                            self.selected_profile = None;
                            cx.status = Some("Профиль удалён.".to_owned());
                        }
                        Err(error) => cx.status = Some(format!("Не удалось удалить профиль: {error}")),
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
                        Ok(_) if clicked == *default => cx.status = Some(format!("Восстановлено значение игры: {key}")),
                        Ok(_) => cx.status = Some(format!("Настройка обновлена: {key}")),
                        Err(error) if clicked == *default => {
                            cx.status = Some(format!("Не удалось восстановить настройку: {error}"))
                        }
                        Err(error) => cx.status = Some(format!("Не удалось изменить настройку: {error}")),
                    }
                    return Ok(());
                }
            }
        }
        if clicked.is_some() && clicked == self.restore {
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let game = cx.app.selected_game().and_then(fix_target);
            let Some(directory) = directory else {
                cx.status = Some("Управляемая установка: не выбрана".to_owned());
                return Ok(());
            };
            let Some(snapshot_id) = self.selected_snapshot.clone() else {
                cx.status = Some("Сначала выберите снимок.".to_owned());
                return Ok(());
            };
            let snapshot = sse_fixes::toolkit::ToolkitSnapshotService::get_snapshot(&directory, &snapshot_id)
                .map_err(|e| sse_core::Error::Refused(e.to_string()))?;
            if self.pending_restore.as_deref() != Some(snapshot.id.as_str()) {
                self.pending_restore = Some(snapshot.id.clone());
                cx.status = Some(format!("ВОССТАНОВЛЕНИЕ ИЗМЕНИТ ФАЙЛЫ ИГРЫ. Нажмите «ВОССТАНОВИТЬ ПОСЛЕДНИЙ СНИМОК» ещё раз для подтверждения: {}", snapshot.label));
                return Ok(());
            }
            self.pending_restore = None;
            let Some(game) = game else {
                cx.status = Some("Игра не поддерживается Toolkit.".to_owned());
                return Ok(());
            };
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            let snapshot_id = snapshot.id.clone();
            std::thread::spawn(move || {
                let engine = sse_fixes::GameFixEngine::new();
                let catalog = sse_fixes::GameFixCatalog;
                let lines = sse_fixes::toolkit::ToolkitSnapshotService::restore_snapshot(
                    &directory,
                    &engine,
                    &catalog,
                    &snapshot_id,
                )
                .map(|r| {
                    vec![format!(
                        "Восстановлен снимок {} · установлено фиксов {} · удалено {} · user.ltx {}",
                        r.snapshot_id,
                        r.installed_fixes.len(),
                        r.uninstalled_fixes.len(),
                        r.user_ltx_updates_count
                    )]
                })
                .unwrap_or_else(|e| vec![format!("Ошибка восстановления: {e}")]);
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
            std::thread::spawn(move || {
                let result = (|| {
                    let game = game.ok_or_else(|| "Выберите поддерживаемую игру".to_owned())?;
                    let directory = directory.ok_or_else(|| "Управляемая установка: не выбрана".to_owned())?;
                    let engine = sse_fixes::GameFixEngine::new();
                    if create_snapshot {
                        if !game.is_xray() {
                            return Err("Снимки доступны только для X-Ray игр.".to_owned());
                        }
                        let snap = sse_fixes::toolkit::ToolkitSnapshotService::create_snapshot(
                            &directory, game, &engine, None,
                        )
                        .map_err(|e| e.to_string())?;
                        Ok(vec![
                            format!("Создан снимок: {}", snap.id),
                            format!(
                                "исправлений: {} · Companion: {}",
                                snap.installed_fixes.len(),
                                if snap.companion_installed {
                                    "включён"
                                } else {
                                    "выключен"
                                }
                            ),
                        ])
                    } else {
                        let report =
                            sse_fixes::toolkit::ToolkitInstallAudit::audit_installation(&directory, game, &engine)
                                .map_err(|e| e.to_string())?;
                        let mut lines = vec![format!(
                            "Проверено файлов: {}. Управляемых: {}. Неизвестных/модов: {}. Требуют проверки: {}.",
                            report.total_scanned,
                            report.managed_count,
                            report.custom_mod_count,
                            report.needs_review_count
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
                        lines: result.unwrap_or_else(|e| vec![format!("Ошибка: {e}")]),
                    }),
                ));
            });
            return Ok(());
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Environment, payload)) = message {
            if let Some(result) = payload.downcast_ref::<EnvironmentResult>() {
                if let Some(status) = self.status {
                    cx.tree.set_text(status, "Только чтение · проверка завершена")?;
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

#[derive(Clone, Debug)]
struct FixRow {
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
    List(std::result::Result<Vec<FixRow>, String>),
    Changed(std::result::Result<String, String>),
    Compatibility(std::result::Result<(String, String, PathBuf, String), String>),
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
    build: String,
}

#[derive(Default)]
struct GameFixes {
    status: Option<WidgetId>,
    detail: Option<WidgetId>,
    rows: Vec<WidgetId>,
    list_scroll: Option<WidgetId>,
    scroll_y: i32,
    items: Vec<FixRow>,
    selected: Option<String>,
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
                                "ОБНОВЛЕНИЕ ДОСТУПНО".to_owned()
                            } else {
                                match engine.get_status(definition, &directory) {
                                    Ok(sse_fixes::GameFixState::Installed) => "УСТАНОВЛЕНО".to_owned(),
                                    Ok(sse_fixes::GameFixState::Modified) => "ФАЙЛ ИЗМЕНЁН ПОСЛЕ УСТАНОВКИ".to_owned(),
                                    Ok(_) => "НЕ УСТАНОВЛЕНО".to_owned(),
                                    Err(error) => format!("ОШИБКА: {error}"),
                                }
                            }
                        } else {
                            "НЕ УСТАНОВЛЕНО".to_owned()
                        };
                        FixRow {
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
            cx.tree.set_text(
                status,
                &if self.items.is_empty() {
                    "НЕТ ПРОВЕРЕННЫХ ИСПРАВЛЕНИЙ ДЛЯ ЭТОЙ ВЕРСИИ.".to_owned()
                } else {
                    format!("ИСПРАВЛЕНИЙ В КАТАЛОГЕ: {}", self.items.len())
                },
            )?;
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
        self.compatibility = Some(style::label(
            cx.tree,
            card,
            "СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ.",
            Text::Note,
        )?);
        self.check = Some(style::button(
            cx.tree,
            card,
            "ПРОВЕРИТЬ СОВМЕСТИМОСТЬ",
            Button::Secondary,
        )?);
        let counts = style::card(cx.tree, card)?;
        style::label(
            cx.tree,
            counts,
            "КАТЕГОРИИ: ОБЯЗАТЕЛЬНЫЕ · РЕКОМЕНДУЕМЫЕ · НЕОБЯЗАТЕЛЬНЫЕ · СООБЩЕСТВО · ЭКСПЕРИМЕНТАЛЬНЫЕ",
            Text::Note,
        )?;
        self.status = Some(style::label(cx.tree, card, "Выберите игру", Text::Note)?);
        let scroll = cx.tree.add(
            Some(card),
            crate::layout::NodeKind::Scroll {
                horizontal: false,
                vertical: true,
                offset_x: 0.0,
                offset_y: 0.0,
            },
            crate::layout::Style {
                preferred: crate::layout::Size::new(0.0, 340.0),
                max: crate::layout::Size::new(f32::INFINITY, 340.0),
                grow: 1.0,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        cx.tree.set_clip_children(scroll, true)?;
        self.list_scroll = Some(scroll);
        let list = cx.tree.add(
            Some(scroll),
            crate::layout::NodeKind::Column,
            crate::layout::Style {
                gap: crate::layout::Size::new(0.0, 4.0),
                align_items: crate::layout::Align::Stretch,
                ..crate::layout::Style::default()
            },
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        for _ in 0..sse_fixes::GameFixCatalog::all().len().max(1) {
            let row = style::button(cx.tree, list, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        self.detail = Some(style::label(cx.tree, card, "ВЫБЕРИТЕ ИСПРАВЛЕНИЕ", Text::Body)?);
        let presets = style::row(cx.tree, card)?;
        self.preset_recommended = Some(style::button(
            cx.tree,
            presets,
            "ПРИМЕНИТЬ: РЕКОМЕНДУЕМЫЕ",
            Button::Primary,
        )?);
        self.preset_essential = Some(style::button(
            cx.tree,
            presets,
            "ПРИМЕНИТЬ: ОБЯЗАТЕЛЬНЫЕ",
            Button::Secondary,
        )?);
        self.preset_safe = Some(style::button(
            cx.tree,
            presets,
            "ПРИМЕНИТЬ: ВСЕ БЕЗОПАСНЫЕ",
            Button::Secondary,
        )?);
        let actions = style::row(cx.tree, card)?;
        self.install = Some(style::button(
            cx.tree,
            actions,
            "УСТАНОВИТЬ ВЫБРАННОЕ",
            Button::Primary,
        )?);
        self.remove = Some(style::button(
            cx.tree,
            actions,
            "УДАЛИТЬ И ВОССТАНОВИТЬ",
            Button::Secondary,
        )?);
        style::label(cx.tree, card, "ИСПРАВЛЕНИЕ ЗАПИСЫВАЕТСЯ ТОЛЬКО ПО НАЖАТИЮ КНОПКИ. ПРИ ИЗМЕНЕНИИ УПРАВЛЯЕМОГО ФАЙЛА УДАЛЕНИЕ ОСТАНОВИТСЯ, НЕ ПЕРЕЗАПИСЫВАЯ ЕГО.", Text::Note)?;
        let confirm = style::card(cx.tree, host)?;
        self.confirm_card = Some(confirm);
        style::label(cx.tree, confirm, "ПОДТВЕРЖДЕНИЕ ИЗМЕНЕНИЯ ИГРЫ", Text::Heading)?;
        style::label(cx.tree, confirm, "Операция изменит файлы выбранной игры. Подтверждение действует только для текущего исправления и установки.", Text::Note)?;
        let confirm_actions = style::row(cx.tree, confirm)?;
        self.confirm_write = Some(style::button(cx.tree, confirm_actions, "ПОДТВЕРДИТЬ", Button::Primary)?);
        self.confirm_cancel = Some(style::button(cx.tree, confirm_actions, "ОТМЕНА", Button::Secondary)?);
        cx.tree.set_visible(confirm, false)?;
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
        if let Message::Window(WindowEvent::Wheel { delta }) = message {
            if let Some(scroll) = self.list_scroll {
                self.scroll_y = self.scroll_y.saturating_add(delta.saturating_mul(48)).max(0);
                cx.tree.set_scroll_y(scroll, self.scroll_y)?;
                return Ok(());
            }
        }
        if self.busy && clicked.is_some() {
            return Ok(());
        }

        if clicked.is_some() {
            for (index, row) in self.rows.iter().copied().enumerate() {
                if clicked == Some(row) && self.items.get(index).is_some() {
                    self.selected = self.items.get(index).map(|item| item.id.clone());
                    self.intent = None;
                    if self.confirm_card.is_some_and(|card| cx.tree.dialog() == Some(card)) {
                        let _ = cx.tree.close_dialog()?;
                    }
                    if let (Some(detail), Some(item)) = (self.detail, self.items.get(index)) {
                        cx.tree
                            .set_text(detail, &format!("{} · {} · {} / {}\nПРОБЛЕМА: {}\nИЗМЕНЕНИЕ: {}\nПОДДЕРЖИВАЕМЫЕ STEAM-СБОРКИ: {}\nЗАТРАГИВАЕМЫЕ ФАЙЛЫ: {}\nИСТОЧНИК: {}", item.id, item.status, item.category, item.maturity, item.problem, item.description, item.builds, item.files, item.source))?;
                    }
                }
            }
        }

        if clicked.is_some() && clicked == self.check {
            self.busy = true;
            self.verified = None;
            let game = cx.app.selected_game().map(str::to_owned);
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            std::thread::spawn(move || {
                let result = (|| {
                    let game = game.ok_or_else(|| "Игра не поддерживается".to_owned())?;
                    let target = fix_target(&game).ok_or_else(|| "Игра не поддерживается".to_owned())?;
                    let directory = directory.ok_or_else(|| "Папка игры не выбрана".to_owned())?;
                    let (matches, build) = sse_fixes::identify_game(target, &directory);
                    if !matches {
                        return Err("ПАПКА НЕ ПОХОЖА НА ВЫБРАННУЮ УСТАНОВКУ ИГРЫ.".to_owned());
                    }
                    let build = build.ok_or_else(|| {
                        "ВЕРСИЯ STEAM НЕ ОПРЕДЕЛЕНА; УСТАНОВКА ИСПРАВЛЕНИЙ С ЗАЩИТОЙ ПО СБОРКЕ НЕДОСТУПНА.".to_owned()
                    })?;
                    Ok((format!("НАЙДЕНА СБОРКА STEAM: {build}."), game, directory, build))
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
                cx.status = Some("СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ.".to_owned());
                return Ok(());
            }
            let target = fix_target(&game).ok_or_else(|| sse_core::Error::damaged("Игра не поддерживается"))?;
            let build = self
                .verified
                .as_ref()
                .map(|state| state.build.as_str())
                .unwrap_or_default();
            if sse_fixes::GameFixCatalog::for_preset(target, preset)
                .iter()
                .any(|definition| {
                    !definition
                        .supported_steam_build_ids
                        .iter()
                        .any(|id| id.as_str() == build)
                })
            {
                cx.status = Some(format!("СБОРКА STEAM {build} НЕ ПОДДЕРЖИВАЕТ ВЫБРАННЫЙ ПРЕСЕТ."));
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
                cx.status = Some("Сначала выберите исправление".to_owned());
                return Ok(());
            };
            let Some(item) = self.items.iter().find(|item| item.id == selected_id) else {
                self.selected = None;
                cx.status = Some("Выбранное исправление исчезло после обновления списка".to_owned());
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
                    cx.status = Some("СНАЧАЛА ПРОВЕРЬТЕ УСТАНОВКУ И ВЕРСИЮ.".to_owned());
                    return Ok(());
                };
                if !item.builds.split(", ").any(|id| id == verified.build.as_str()) {
                    cx.status = Some(format!(
                        "СБОРКА STEAM {} НЕ ПОДДЕРЖИВАЕТ ВЫБРАННОЕ ИСПРАВЛЕНИЕ.",
                        verified.build
                    ));
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
                cx.status = Some("Выбор игры изменился; подтверждение отменено.".to_owned());
                return Ok(());
            }
            let fix_id = intent.fix_id;
            let operation = intent.operation;
            let game = intent.game;
            let directory = intent.directory;
            let _ = cx.tree.close_dialog()?;
            let Some(proxy) = cx.proxy.cloned() else { return Ok(()) };
            self.busy = true;
            std::thread::spawn(move || {
                let result = (|| {
                    let target = fix_target(&game).ok_or_else(|| "Игра не поддерживается".to_owned())?;
                    let engine = sse_fixes::GameFixEngine::new();
                    match operation {
                        FixOperation::Preset(preset) => {
                            let result = engine
                                .apply_preset(target, preset, &directory)
                                .map_err(|e| e.to_string())?;
                            Ok(format!(
                                "ПРЕСЕТ {}: УСТАНОВЛЕНО {}; УЖЕ АКТУАЛЬНЫХ {}.",
                                preset.as_str(),
                                result.installed_fix_ids.len(),
                                result.already_installed_fix_ids.len()
                            ))
                        }
                        FixOperation::Install | FixOperation::Remove => {
                            let fix_id = fix_id.ok_or_else(|| "Исправление не выбрано".to_owned())?;
                            let definition = sse_fixes::GameFixCatalog::try_get(&fix_id)
                                .ok_or_else(|| "Фикс исчез из каталога".to_owned())?;
                            if definition.game != target {
                                return Err("Фикс не относится к выбранной игре".to_owned());
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
                                        return Err(check
                                            .reason
                                            .unwrap_or_else(|| "Безопасное удаление запрещено".to_owned()));
                                    }
                                    engine.uninstall(&fix_id, &directory)
                                }
                                FixOperation::Preset(_) => unreachable!(),
                            }
                            .map_err(|e| e.to_string())?;
                            Ok(format!(
                                "{}: {:?}; файлов: {}",
                                fix_id,
                                result.state,
                                result.files.len()
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
                    FixReply::List(Ok(items)) => {
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
                        self.render(cx)?;
                    }
                    FixReply::List(Err(error)) => {
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, error)?;
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
                        cx.status = Some(format!("ОШИБКА: {error}"));
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
    severity: String,
    message: String,
}

#[derive(Debug)]
enum DoctorReply {
    Progress(String),
    Done(std::result::Result<(usize, u64, Vec<DoctorFinding>), String>),
}

#[derive(Default)]
struct GameDoctor {
    status: Option<WidgetId>,
    start: Option<WidgetId>,
    cancel: Option<WidgetId>,
    toggle_s2_mods: Option<WidgetId>,
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
            cx.status = Some("Доктор игры сейчас проверяет X-Ray установки".to_owned());
            return;
        };
        let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
            cx.status = Some("Сначала выберите установленную игру".to_owned());
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.cancellation = Some(std::sync::Arc::clone(&cancelled));
        std::thread::spawn(move || {
            use std::sync::atomic::Ordering;
            proxy.send(AppMessage::ToScreen(
                ScreenId::GameDoctor,
                Box::new(DoctorReply::Progress("25% · строю дерево файлов".to_owned())),
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
                    return Err("Проверка отменена".to_owned());
                }
                proxy.send(AppMessage::ToScreen(
                    ScreenId::GameDoctor,
                    Box::new(DoctorReply::Progress("60% · запускаю линтер".to_owned())),
                ));
                let engine = sse_lint::LintEngine::new(sse_lint::LintOptions {
                    single_checker: None,
                    config_subdir: None,
                    max_files_globals: None,
                });
                let report = engine.lint_tree(&tree);
                if cancelled.load(Ordering::Relaxed) {
                    return Err("Проверка отменена".to_owned());
                }
                let mut findings: Vec<DoctorFinding> = report
                    .findings
                    .into_iter()
                    .map(|finding| DoctorFinding {
                        file: finding.file,
                        severity: finding.severity.as_str().to_owned(),
                        message: finding.message,
                    })
                    .collect();

                let row_ids: Vec<u64> = (0..findings.len())
                    .filter_map(|index| u64::try_from(index).ok())
                    .collect();
                let headers = vec![
                    crate::widgets::table::Header {
                        label: "Файл".to_owned(),
                        sortable: true,
                        direction: None,
                    },
                    crate::widgets::table::Header {
                        label: "Тяжесть".to_owned(),
                        sortable: true,
                        direction: None,
                    },
                ];
                if let Ok(mut table) = crate::widgets::table::Table::new(row_ids, 24.0, headers) {
                    let _ = table.header_click(0, false, |left, right, _| {
                        let a = usize::try_from(left).ok().and_then(|i| findings.get(i));
                        let b = usize::try_from(right).ok().and_then(|i| findings.get(i));
                        a.map(|v| (&v.file, &v.severity))
                            .cmp(&b.map(|v| (&v.file, &v.severity)))
                    });
                    let mut ordered = Vec::with_capacity(findings.len());
                    for index in 0..findings.len() {
                        if let Some(row) = table
                            .visible_row(index)
                            .and_then(|id| usize::try_from(id).ok())
                            .and_then(|i| findings.get(i))
                            .cloned()
                        {
                            ordered.push(row);
                        }
                    }
                    findings = ordered;
                }
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
        "Фоновая проверка установки линтером; находки сгруппированы по файлу и тяжести"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ДОКТОР ИГРЫ", Text::Heading)?;
        self.status = Some(style::label(cx.tree, card, "Готов к проверке", Text::Note)?);
        let actions = style::row(cx.tree, card)?;
        self.start = Some(style::button(cx.tree, actions, "Проверить", Button::Primary)?);
        self.cancel = Some(style::button(cx.tree, actions, "Отмена", Button::Secondary)?);
        self.toggle_s2_mods = Some(style::button(
            cx.tree,
            actions,
            "ВРЕМЕННО ОТКЛЮЧИТЬ / ВОССТАНОВИТЬ КАСТОМНЫЕ МОДЫ",
            Button::Secondary,
        )?);
        style::label(cx.tree, card, "ФАЙЛ · ТЯЖЕСТЬ · НАХОДКА", Text::Value)?;
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
            let directory = cx.app.game_dir().map(Path::to_path_buf);
            if !is_s2 {
                cx.status = Some("Переключение модов доступно только для S.T.A.L.K.E.R. 2.".to_owned());
            } else if let Some(directory) = directory {
                match sse_fixes::toolkit::Stalker2ModToggle::toggle(&directory) {
                    Ok(result) => cx.status = Some(format!("S2 mods: {result:?}")),
                    Err(error) => cx.status = Some(error.to_string()),
                }
            } else {
                cx.status = Some("ВЫБЕРИТЕ ИГРУ И ПАПКУ УСТАНОВКИ ДЛЯ ПРОВЕРКИ.".to_owned());
            }
        }
        if clicked.is_some() && clicked == self.cancel {
            if let Some(cancelled) = &self.cancellation {
                cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
                if let Some(status) = self.status {
                    cx.tree.set_text(status, "Отмена запрошена…")?;
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::GameDoctor, payload)) = message {
            if let Some(reply) = payload.downcast_ref::<DoctorReply>() {
                match reply {
                    DoctorReply::Progress(text) => {
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, text)?;
                        }
                    }
                    DoctorReply::Done(Ok((files, elapsed, findings))) => {
                        self.cancellation = None;
                        self.findings.clone_from(findings);
                        if let Some(status) = self.status {
                            cx.tree.set_text(
                                status,
                                &format!("100% · файлов: {files} · находок: {} · {elapsed} мс", findings.len()),
                            )?;
                        }
                        for (index, widget) in self.rows.iter().copied().enumerate() {
                            if let Some(finding) = findings.get(index) {
                                cx.tree.set_visible(widget, true)?;
                                cx.tree.set_text(
                                    widget,
                                    &format!("{} · {} · {}", finding.file, finding.severity, finding.message),
                                )?;
                            } else {
                                cx.tree.set_visible(widget, false)?;
                            }
                        }
                    }
                    DoctorReply::Done(Err(error)) => {
                        self.cancellation = None;
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, error)?;
                        }
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
    kind: String,
    key: String,
    name: String,
    detail: String,
}

#[derive(Debug)]
struct EncyclopediaResult(std::result::Result<Vec<EncyclopediaEntry>, String>);

#[derive(Default)]
struct Encyclopedia {
    status: Option<WidgetId>,
    search_label: Option<WidgetId>,
    rows: Vec<WidgetId>,
    card: Option<WidgetId>,
    entries: Vec<EncyclopediaEntry>,
    visible: Vec<usize>,
    selected: Option<usize>,
    search: Option<crate::widgets::text_input::TextInput>,
}

impl Encyclopedia {
    fn load(&self, cx: &mut Context<'_>) {
        let Some(game) = cx.app.selected_game().and_then(encyclopedia_game) else {
            if let Some(status) = self.status {
                let _ = cx.tree.set_text(status, "Энциклопедия сейчас доступна для X-Ray игр");
            }
            return;
        };
        let Some(directory) = cx.app.game_dir().map(Path::to_path_buf) else {
            return;
        };
        let Some(proxy) = cx.proxy.cloned() else { return };
        std::thread::spawn(move || {
            let cache = std::env::temp_dir().join("stalker-save-editor").join("catalog-cache");
            let result = (|| {
                let content = sse_catalog::GameContentService::load(game, &directory, &cache, "ru")
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| "Каталог установки не построен".to_owned())?;
                let bundle = content.bundle();
                let mut entries = Vec::new();
                for item in bundle.items.items() {
                    entries.push(EncyclopediaEntry {
                        kind: "предмет".to_owned(),
                        key: item.key.clone(),
                        name: item.display_name.clone().unwrap_or_else(|| item.key.clone()),
                        detail: format!(
                            "{} · {} · цена: {}",
                            item.category.as_deref().unwrap_or("без категории"),
                            item.source,
                            item.cost.map_or("—".to_owned(), |v| v.to_string())
                        ),
                    });
                }
                if let Some(factions) = &bundle.factions {
                    for faction in factions.factions() {
                        entries.push(EncyclopediaEntry {
                            kind: "персонаж/группировка".to_owned(),
                            key: faction.key.clone(),
                            name: faction.display_name.clone().unwrap_or_else(|| faction.key.clone()),
                            detail: format!("группировка · {}", faction.source),
                        });
                    }
                }
                let search = sse_content::CompanionArchiveLocator::discover(&directory, &["fsgame.ltx"], game);
                for archive in search.archive_paths {
                    if let Some(name) = archive.file_stem().and_then(|v| v.to_str()) {
                        if name.to_ascii_lowercase().contains("level") || name.to_ascii_lowercase().contains("location")
                        {
                            entries.push(EncyclopediaEntry {
                                kind: "локация".to_owned(),
                                key: name.to_owned(),
                                name: name.to_owned(),
                                detail: archive.display().to_string(),
                            });
                        }
                    }
                }
                Ok(entries)
            })();
            proxy.send(AppMessage::ToScreen(
                ScreenId::Encyclopedia,
                Box::new(EncyclopediaResult(result)),
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
                    || crate::text::folded_contains(&entry.kind, &query, crate::text::SearchLocale::General))
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
                label: "Тип".to_owned(),
                sortable: true,
                direction: None,
            },
            crate::widgets::table::Header {
                label: "Название".to_owned(),
                sortable: true,
                direction: None,
            },
        ];
        let _table = crate::widgets::table::Table::new(ids, 24.0, headers)?;

        if let Some(label) = self.search_label {
            cx.tree.set_text(
                label,
                &format!(
                    "Поиск: {} · результатов: {}",
                    if query.is_empty() { "все" } else { &query },
                    self.visible.len()
                ),
            )?;
        }
        for (row_index, widget) in self.rows.iter().copied().enumerate() {
            if let Some(entry) = self.visible.get(row_index).and_then(|index| self.entries.get(*index)) {
                cx.tree.set_visible(widget, true)?;
                cx.tree
                    .set_text(widget, &format!("{} · {} · {}", entry.kind, entry.name, entry.key))?;
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
        "Предметы, персонажи и локации из каталога установленной игры"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ЭНЦИКЛОПЕДИЯ", Text::Heading)?;
        self.status = Some(style::label(cx.tree, card, "Загрузка каталога…", Text::Note)?);
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
            "Поиск: все · нажмите и печатайте",
            Button::Secondary,
        )?);
        style::label(cx.tree, card, "ТИП · НАЗВАНИЕ · КЛЮЧ", Text::Note)?;
        for _ in 0..10 {
            let row = style::button(cx.tree, card, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        self.card = Some(style::label(cx.tree, card, "Выберите запись", Text::Body)?);
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
                cx.status = Some("Поиск активен: вводите текст с клавиатуры".to_owned());
            }
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
                            cx.tree
                                .set_text(card, &format!("{}\n{}\n{}", entry.name, entry.kind, entry.detail))?;
                        }
                    }
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Encyclopedia, payload)) = message {
            if let Some(EncyclopediaResult(result)) = payload.downcast_ref::<EncyclopediaResult>() {
                match result {
                    Ok(entries) => {
                        self.entries.clone_from(entries);
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, &format!("Записей: {}", entries.len()))?;
                        }
                        self.apply_search(cx)?;
                    }
                    Err(error) => {
                        if let Some(status) = self.status {
                            cx.tree.set_text(status, error)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
