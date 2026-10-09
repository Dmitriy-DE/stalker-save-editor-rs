//! The editor frame: grouped sidebar, header, content host, status line; builds screens lazily and routes messages.

use super::style::{self, rgb, Text};
use super::{AppMessage, Context, EditorAction, Group, Screen, ScreenId};
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key as EditKey, Modifiers};
use crate::event_loop::{App, Flow, Message, Proxy, WindowEvent};
use crate::glyphs::{to_px, Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::path::Icon;
use crate::widget::{Content, ImageData, Look, TextAlign, Tree, WidgetId};
use crate::widgets::scroll::ScrollView;
use crate::widgets::text_input::TextInput;
use sse_content::{SavePreviewReader, Stalker2SlotMeta};
use sse_core::Result;
use sse_storage::discovery::SaveSlot;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

const KEY_ESCAPE: u32 = 0xff1b;
const KEY_TAB: u32 = 0xff09;
const KEY_RETURN: u32 = 0xff0d;
const KEY_UP: u32 = 0xff52;
const KEY_DOWN: u32 = 0xff54;
const SAVE_LIBRARY_PAGE_SIZE: usize = 8;
const LIBRARY_PREVIEW_WIDTH: u32 = 96;
const LIBRARY_PREVIEW_HEIGHT: u32 = 54;
const LIBRARY_PREVIEW_CACHE_ENTRIES: usize = 32;
const DRAFT_CLOSE_WARNING: &str = "Последняя правка не сохранена в черновик.";
const FORCE_CLOSE_DEFAULT_MESSAGE: &str =
    "Фоновая операция ещё записывает файлы. Принудительное закрытие может оставить операцию незавершённой.";
const MAX_OPENED_SAVE_FILES: usize = 512;

fn open_path_edit_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 32_768,
        history_limit: 64,
        filter: InputFilter::Any,
    }
}

#[derive(Default)]
struct ShellClipboard(String);

impl Clipboard for ShellClipboard {
    fn read_text(&mut self) -> Result<String> {
        Ok(self.0.clone())
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        text.clone_into(&mut self.0);
        Ok(())
    }
}

struct OpenFilesQueue {
    remaining: VecDeque<PathBuf>,
    pending_requests: BTreeMap<u64, PathBuf>,
    active_request: Option<u64>,
    total: usize,
    completed: usize,
    opened: usize,
    last_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PreviewKey {
    path: PathBuf,
    size: u64,
    modified: SystemTime,
}

impl From<&SaveSlot> for PreviewKey {
    fn from(slot: &SaveSlot) -> Self {
        Self {
            path: slot.path.clone(),
            size: slot.size,
            modified: slot.last_write_time_utc,
        }
    }
}

#[derive(Clone)]
struct LibraryPreviewEntry {
    key: PreviewKey,
    image: Option<ImageData>,
    s2_detail: Option<String>,
    s2_jpeg_available: bool,
}

#[derive(Clone)]
struct PreviewRequest {
    id: u64,
    key: PreviewKey,
}

struct ReportUploadFinished {
    result: std::result::Result<String, String>,
    local_saved: bool,
}

struct NativeFilePickerFinished {
    request: u64,
    result: std::result::Result<Option<Vec<PathBuf>>, String>,
}

fn spawn_native_file_picker<F>(proxy: Proxy<AppMessage>, request: u64, picker: F) -> std::io::Result<()>
where
    F: FnOnce() -> sse_core::Result<Option<Vec<PathBuf>>> + Send + 'static,
{
    std::thread::Builder::new()
        .name("sse-native-file-picker".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(picker))
                .map_err(|_| "native file picker panicked".to_owned())
                .and_then(|result| result.map_err(|error| error.to_string()));
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Overview,
                Box::new(NativeFilePickerFinished { request, result }),
            ));
        })
        .map(|_| ())
}

#[derive(Default)]
struct LibraryPreviewState {
    entries: VecDeque<LibraryPreviewEntry>,
    pending: Option<PreviewRequest>,
    next_request: u64,
}

impl LibraryPreviewState {
    fn get(&mut self, key: &PreviewKey) -> Option<LibraryPreviewEntry> {
        let index = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_back(entry.clone());
        Some(entry)
    }

    fn contains(&self, key: &PreviewKey) -> bool {
        self.entries.iter().any(|entry| entry.key == *key)
    }

    fn insert(&mut self, entry: LibraryPreviewEntry) {
        self.entries.retain(|cached| cached.key != entry.key);
        self.entries.push_back(entry);
        while self.entries.len() > LIBRARY_PREVIEW_CACHE_ENTRIES {
            self.entries.pop_front();
        }
    }
}

#[derive(Clone)]
struct LibraryPreviewFinished {
    request: u64,
    key: PreviewKey,
    image: Option<ImageData>,
    s2_detail: Option<String>,
    s2_jpeg_available: bool,
}

fn format_s2_preview_detail(meta: Option<Stalker2SlotMeta>) -> Option<String> {
    let meta = meta?;
    let hours = if (meta.play_hours.fract()).abs() < 0.05 {
        format!("{:.0}", meta.play_hours)
    } else {
        format!("{:.1}", meta.play_hours)
    };
    Some(format!("{} · {hours} ч", meta.region_slug()))
}

fn format_open_error(path: &Path, error: &str, io_error: bool) -> String {
    let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
    if io_error {
        format!("Не удалось открыть «{name}»: {error}")
    } else {
        format!("«{name}» — не сохранение S.T.A.L.K.E.R. или файл повреждён.")
    }
}

#[derive(Clone, Copy)]
enum SaveReason {
    SelectSave,
    UnmappedDraft,
    UnsupportedFormat(&'static str),
    NoChanges,
    InvalidNumbers,
    CanSave,
}

impl SaveReason {
    fn localized(self, language: &str) -> String {
        let i18n = sse_catalog::I18nService::instance();
        let key = match self {
            Self::SelectSave => "Выберите сохранение для редактирования.",
            Self::UnmappedDraft => {
                "В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом)."
            }
            Self::UnsupportedFormat(_) => "Эта правка для формата {0} не поддерживается (см. «Возможности»).",
            Self::NoChanges => "Нет несохранённых изменений.",
            Self::InvalidNumbers => "Введены некорректные значения (проверьте введённые числа).",
            Self::CanSave => "Сохранить изменения в файл сейва (с созданием резервной копии).",
        };
        if let Self::UnsupportedFormat(format_name) = self {
            let format_name = i18n.tr_in(Some(language), format_name, &[]);
            i18n.tr_in(Some(language), key, &[&format_name])
        } else {
            i18n.tr_in(Some(language), key, &[])
        }
    }
}

fn draft_badge_text(language: &str, count: usize) -> String {
    sse_catalog::I18nService::instance().tr_in(Some(language), "Черновик: {0} действ.", &[&count])
}

struct SaveEligibility {
    reason: SaveReason,
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
            reason: SaveReason::SelectSave,
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
        SaveReason::UnmappedDraft
    } else if unsupported {
        SaveReason::UnsupportedFormat(display_format_name(format_id.unwrap_or("неизвестный формат")))
    } else if !has_changes {
        SaveReason::NoChanges
    } else if invalid_numbers {
        SaveReason::InvalidNumbers
    } else {
        SaveReason::CanSave
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
        _ => "Неизвестный формат",
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
    open_button: WidgetId,
    open_files_queue: Option<OpenFilesQueue>,
    native_file_picker_request: Option<u64>,
    #[cfg(not(test))]
    next_native_file_picker_request: u64,
    open_return_screen: Option<ScreenId>,
    open_file_dialog: WidgetId,
    open_path_widget: WidgetId,
    open_confirm: WidgetId,
    open_cancel: WidgetId,
    open_path_input: TextInput,
    open_path_clipboard: ShellClipboard,
    library: WidgetId,
    library_refresh: WidgetId,
    library_previous: WidgetId,
    library_next: WidgetId,
    library_count: WidgetId,
    library_status: WidgetId,
    library_rows: Vec<(WidgetId, WidgetId, WidgetId, WidgetId)>,
    library_page: usize,
    library_previews: LibraryPreviewState,
    library_workspace: super::saves::Workspace,
    reports_banner: WidgetId,
    reports_ok: WidgetId,
    reports_off: WidgetId,
    report_dialog: WidgetId,
    report_preview: WidgetId,
    report_send: WidgetId,
    report_cancel: WidgetId,
    pending_report: Option<String>,
    report_upload_pending: bool,
    reports_consented: bool,
    saving_dialog: WidgetId,
    force_close_dialog: WidgetId,
    force_close_message: WidgetId,
    force_close_yes: WidgetId,
    force_close_no: WidgetId,
    close_waiting: bool,
    draft_close_started: Option<Instant>,
    draft_close_prompted: bool,
    draft_close_idle_seen: bool,
    tooltip: WidgetId,
    status: WidgetId,
    selected: usize,
    proxy: Option<Proxy<AppMessage>>,
    app: sse_app::state::AppState,
    wizard: super::wizard::Wizard,
    wizard_task_request: Option<u64>,
    next_wizard_task_request: u64,
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

fn report_text(key: &str) -> String {
    sse_catalog::I18nService::instance().tr_in(Some(crate::strings::current_language()), key, &[])
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
        let (settings, warning) = match sse_app::AppSettings::load(&sse_app::default_settings_path()) {
            Ok(settings) => (settings, None),
            Err(error) => {
                sse_app::diagnostics::warn(&format!("settings file could not be loaded: {error}"));
                (sse_app::AppSettings::default(), Some(error.to_string()))
            }
        };
        let shell = Self::build_with_settings(tree, proxy, settings)?;
        if let Some(error) = warning {
            let detail = format!("settings.json is unchanged: {error}");
            let warning = sse_catalog::I18nService::instance().tr_in(
                Some(crate::strings::current_language()),
                "Настройки не сохранены: {0}",
                &[&detail],
            );
            tree.set_text(shell.status, &warning)?;
        }
        Ok(shell)
    }

    #[cfg(test)]
    pub(crate) fn build_for_test(tree: &mut Tree, proxy: Option<Proxy<AppMessage>>) -> Result<Self> {
        use std::sync::atomic::{AtomicU64, Ordering};

        static NEXT_TEST_BACKUP_DIRECTORY: AtomicU64 = AtomicU64::new(0);
        let mut settings = sse_app::AppSettings::new();
        settings.reports_notice_shown = true;
        settings.send_reports = false;
        let directory_id = NEXT_TEST_BACKUP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        settings.backup_directory =
            Some(std::env::temp_dir().join(format!("sse-shell-test-backups-{}-{directory_id}", std::process::id())));
        let mut shell = Self::build_with_settings(tree, None, settings)?;
        shell.proxy = proxy;
        Ok(shell)
    }

    fn build_with_settings(
        tree: &mut Tree,
        proxy: Option<Proxy<AppMessage>>,
        settings: sse_app::AppSettings,
    ) -> Result<Self> {
        let interactive = proxy.is_some();
        let language = startup_language(&settings);
        crate::strings::set_language(Some(&language));
        let open_path_input = TextInput::new("", open_path_edit_config())?;
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
        let language = crate::strings::current_language();
        let initial_draft_badge = draft_badge_text(language, 0);
        let draft_badge = style::label(tree, top, &initial_draft_badge, Text::Note)?;
        let undo = top_button(tree, top, crate::strings::t("Отменить"), false)?;
        let redo = top_button(tree, top, crate::strings::t("ПОВТОР"), false)?;
        let reset = top_button(tree, top, crate::strings::t("СБРОС"), false)?;
        let open_button = top_button(tree, top, crate::strings::t("Открыть…"), false)?;
        let refresh = top_button(tree, top, crate::strings::t("ОБНОВИТЬ"), false)?;
        let save = top_button(tree, top, crate::strings::t("СОХРАНИТЬ"), true)?;
        let initial_save_reason = SaveReason::SelectSave.localized(language);
        let save_reason = style::label(tree, header, &initial_save_reason, Text::Note)?;
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
                NodeKind::Row,
                Style {
                    gap: Size::new(5.0, 0.0),
                    align_items: Align::Center,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let image = tree.add(
                Some(row),
                NodeKind::Leaf,
                Style {
                    min: Size::new(LIBRARY_PREVIEW_WIDTH as f32, LIBRARY_PREVIEW_HEIGHT as f32),
                    preferred: Size::new(LIBRARY_PREVIEW_WIDTH as f32, LIBRARY_PREVIEW_HEIGHT as f32),
                    shrink: 0.0,
                    ..Style::default()
                },
                Content::Image(None),
                Look {
                    fill: Some(rgb(crate::theme::current().colors.background[1])),
                    border: Some((rgb(crate::theme::current().colors.borders[0]), 1.0)),
                    radius: crate::theme::BUTTON_RADIUS,
                    ..Look::default()
                },
            )?;
            let details_column = tree.add(
                Some(row),
                NodeKind::Column,
                Style {
                    grow: 1.0,
                    align_items: Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let select = tree.add(
                Some(details_column),
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
                Some(details_column),
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
            tree.set_tooltip(image, crate::strings::t("нет снимка"))?;
            library_rows.push((row, image, select, details));
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

        let reports_banner = style::card(tree, overlay_host)?;
        style::label(
            tree,
            reports_banner,
            &report_text("Отправлять анонимные отчёты об ошибках?"),
            Text::Heading,
        )?;
        tree.add(
            Some(reports_banner),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(560.0, 0.0),
                max: Size::new(620.0, f32::INFINITY),
                ..Style::default()
            },
            Content::Paragraph {
                text: report_text(
                    "В автоматический отчёт входят только версия, ОС, текст ошибки, стек и обезличенный журнал. Сохранения, пути и игровые логи не отправляются.",
                ),
                style: Text::Body.style(),
            },
            Look {
                text: rgb(style::TEXT_SECONDARY),
                ..Look::default()
            },
        )?;
        let reports_actions = style::row(tree, reports_banner)?;
        let reports_ok = style::button(tree, reports_actions, &report_text("Да"), style::Button::Primary)?;
        let reports_off = style::button(tree, reports_actions, &report_text("Нет"), style::Button::Secondary)?;
        tree.set_visible(reports_banner, false)?;

        let report_dialog = style::card(tree, overlay_host)?;
        style::label(tree, report_dialog, &report_text("Отправить отчёт?"), Text::Heading)?;
        style::label(
            tree,
            report_dialog,
            &report_text("Ниже показано всё, что войдёт в отчёт."),
            Text::Note,
        )?;
        let report_preview = tree.add(
            Some(report_dialog),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(620.0, 300.0),
                max: Size::new(680.0, 360.0),
                ..Style::default()
            },
            Content::Paragraph {
                text: String::new(),
                style: Text::Note.style(),
            },
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
                text: rgb(style::TEXT_SECONDARY),
                ..Look::default()
            },
        )?;
        let report_actions = style::row(tree, report_dialog)?;
        let report_send = style::button(tree, report_actions, &report_text("Отправить"), style::Button::Primary)?;
        let report_cancel = style::button(
            tree,
            report_actions,
            &report_text("Не отправлять"),
            style::Button::Secondary,
        )?;
        tree.set_visible(report_dialog, false)?;

        let saving_dialog = style::card(tree, overlay_host)?;
        style::label(tree, saving_dialog, "СОХРАНЕНИЕ ФАЙЛА", Text::Heading)?;
        style::label(
            tree,
            saving_dialog,
            "Создаю резервную копию, записываю файл и проверяю его повторным чтением…",
            Text::Body,
        )?;
        tree.set_visible(saving_dialog, false)?;
        let force_close_dialog = style::card(tree, overlay_host)?;
        style::label(tree, force_close_dialog, "ЗАКРЫТЬ, НЕ ДОЖИДАЯСЬ?", Text::Heading)?;
        let force_close_message = style::label(tree, force_close_dialog, FORCE_CLOSE_DEFAULT_MESSAGE, Text::Body)?;
        let force_close_actions = style::row(tree, force_close_dialog)?;
        let force_close_yes = style::button(tree, force_close_actions, "ЗАКРЫТЬ", style::Button::Danger)?;
        let force_close_no = style::button(tree, force_close_actions, "ПОДОЖДАТЬ", style::Button::Secondary)?;
        tree.set_visible(force_close_dialog, false)?;

        let open_file_dialog = style::card(tree, overlay_host)?;
        style::label(
            tree,
            open_file_dialog,
            crate::strings::t("Открыть сохранение"),
            Text::Heading,
        )?;
        style::label(
            tree,
            open_file_dialog,
            crate::strings::t("Укажите путь к файлу сохранения. Файл будет проверен и прочитан в фоне."),
            Text::Body,
        )?;
        style::label(
            tree,
            open_file_dialog,
            crate::strings::t("Путь к файлу сохранения"),
            Text::Note,
        )?;
        let open_path_widget = tree.add(
            Some(open_file_dialog),
            NodeKind::Leaf,
            Style {
                min: Size::new(420.0, 38.0),
                padding: padded(10.0, 0.0, 10.0, 0.0),
                ..Style::default()
            },
            Content::Input {
                text: String::new(),
                style: Text::Body.style(),
            },
            Look {
                fill: Some(rgb(crate::theme::current().colors.background[4])),
                border: Some((rgb(crate::theme::current().colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: rgb(crate::theme::current().colors.text[0]),
                ..Look::default()
            },
        )?;
        let open_actions = style::row(tree, open_file_dialog)?;
        let open_cancel = style::button(tree, open_actions, "Отмена", style::Button::Secondary)?;
        let open_confirm = style::button(tree, open_actions, "Открыть", style::Button::Primary)?;
        tree.set_enabled(open_confirm, false)?;
        tree.set_visible(open_file_dialog, false)?;
        let tooltip = style::label(tree, overlay_host, "", Text::Body)?;
        tree.set_visible(tooltip, false)?;
        tree.set_tooltip(nav_toggle, crate::strings::t("Свернуть меню"))?;
        tree.set_tooltip(library_refresh, crate::strings::t("Обновить"))?;
        tree.set_tooltip(undo, crate::strings::t("Отменить"))?;
        tree.set_tooltip(redo, crate::strings::t("Вернуть"))?;
        tree.set_tooltip(reset, crate::strings::t("Сбросить"))?;
        tree.set_tooltip(refresh, crate::strings::t("Обновить"))?;
        tree.set_tooltip(save, crate::strings::t("Выберите сохранение для редактирования."))?;
        tree.set_tooltip(open_button, crate::strings::t("Открыть сохранение"))?;

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
            open_button,
            open_files_queue: None,
            native_file_picker_request: None,
            #[cfg(not(test))]
            next_native_file_picker_request: 0,
            open_return_screen: None,
            open_file_dialog,
            open_path_widget,
            open_confirm,
            open_cancel,
            open_path_input,
            open_path_clipboard: ShellClipboard::default(),
            library,
            library_refresh,
            library_previous,
            library_next,
            library_count,
            library_status,
            library_rows,
            library_page: 0,
            library_previews: LibraryPreviewState::default(),
            library_workspace,
            reports_banner,
            reports_ok,
            reports_off,
            report_dialog,
            report_preview,
            report_send,
            report_cancel,
            pending_report: sse_app::diagnostics::pending_automatic_error_report(),
            report_upload_pending: false,
            reports_consented: settings.send_reports && settings.reports_notice_shown,
            saving_dialog,
            force_close_dialog,
            force_close_message,
            force_close_yes,
            force_close_no,
            close_waiting: false,
            draft_close_started: None,
            draft_close_prompted: false,
            draft_close_idle_seen: false,
            tooltip,
            status,
            selected: 0,
            proxy,
            app: sse_app::state::AppState::new(),
            wizard,
            wizard_task_request: None,
            next_wizard_task_request: 0,
            sounds: crate::sound::GameUiSounds::default(),
            sound_game: None,
            sound_enabled: settings.sound_enabled,
            sound_volume: (settings.sound_volume.min(100) as f32) / 100.0,
        };
        let initial_collapsed = settings.navigation_collapsed.unwrap_or(false);
        shell.apply_navigation(tree, initial_collapsed)?;
        shell.show(tree, 0)?;
        shell.render_library(tree)?;
        shell.sync_draft_controls(tree)?;
        if interactive && !settings.reports_notice_shown {
            tree.open_dialog(shell.reports_banner)?;
        } else if interactive && settings.send_reports {
            shell.open_pending_report_dialog(tree)?;
        }
        Ok(shell)
    }

    fn open_pending_report_dialog(&mut self, tree: &mut Tree) -> Result<()> {
        let Some(report) = self.pending_report.as_deref() else {
            return Ok(());
        };
        let preview = if report.chars().count() > 8_000 {
            report.chars().take(8_000).collect::<String>()
        } else {
            report.to_owned()
        };
        tree.set_text(self.report_preview, &preview)?;
        if tree.dialog_open() {
            let _ = tree.close_dialog()?;
        }
        tree.open_dialog(self.report_dialog)
    }

    fn capture_error_report(&mut self, tree: &mut Tree, error: &str) {
        sse_app::diagnostics::record_crash("Caught UI error", error);
        let stack = std::backtrace::Backtrace::force_capture().to_string();
        self.pending_report = Some(sse_app::diagnostics::automatic_error_report(error, &stack));
        let settings = match sse_app::AppSettings::load(&sse_app::default_settings_path()) {
            Ok(settings) => settings,
            Err(error) => {
                sse_app::diagnostics::warn(&format!(
                    "settings file could not be loaded before crash report: {error}"
                ));
                sse_app::AppSettings::default()
            }
        };
        self.reports_consented = settings.send_reports && settings.reports_notice_shown;
        if settings.send_reports && settings.reports_notice_shown {
            let _ = self.open_pending_report_dialog(tree);
        }
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
        self.open_files_queue = None;
        self.open_save_inner(tree, path)
    }

    fn open_save_inner(&mut self, tree: &mut Tree, path: &Path) -> Result<bool> {
        if self.library_workspace.is_saving() || self.library_workspace.is_restoring() {
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

    fn open_save_paths(&mut self, tree: &mut Tree, paths: Vec<PathBuf>) -> Result<bool> {
        if paths.is_empty() {
            return Ok(false);
        }
        if paths.len() > MAX_OPENED_SAVE_FILES {
            tree.set_text(self.status, "Выберите не более 512 файлов за один раз.")?;
            return Ok(false);
        }
        if self.proxy.is_none()
            || self.open_files_queue.is_some()
            || self.library_workspace.is_saving()
            || self.library_workspace.is_restoring()
            || self.library_workspace.is_loading()
        {
            return Ok(false);
        }

        self.open_files_queue = Some(OpenFilesQueue {
            total: paths.len(),
            remaining: paths.into(),
            pending_requests: BTreeMap::new(),
            active_request: None,
            completed: 0,
            opened: 0,
            last_error: None,
        });
        tree.set_enabled(self.open_button, false)?;
        self.start_next_open_file(tree)?;
        Ok(true)
    }

    fn start_next_open_file(&mut self, tree: &mut Tree) -> Result<()> {
        loop {
            let next = self
                .open_files_queue
                .as_mut()
                .and_then(|queue| queue.remaining.pop_front());
            let Some(path) = next else {
                self.finish_open_files_queue(tree)?;
                return Ok(());
            };
            if !self.open_save_inner(tree, &path)? {
                self.open_files_queue = None;
                tree.set_enabled(self.open_button, true)?;
                return Ok(());
            }
            let request = self.library_workspace.load_request();
            if self.library_workspace.is_loading() {
                if let Some(queue) = self.open_files_queue.as_mut() {
                    queue.pending_requests.insert(request, path.clone());
                    queue.active_request = Some(request);
                }
                return Ok(());
            }

            let detail = self
                .library_workspace
                .library_snapshot()
                .1
                .unwrap_or_else(|| "Не удалось запустить фоновое чтение сейва.".to_owned());
            let message = format_open_error(&path, &detail, true);
            if let Some(queue) = self.open_files_queue.as_mut() {
                queue.completed = queue.completed.saturating_add(1);
                queue.last_error = Some(message);
            }
        }
    }

    fn finish_open_files_queue(&mut self, tree: &mut Tree) -> Result<()> {
        if self.open_files_queue.as_ref().is_some_and(|queue| {
            !queue.remaining.is_empty()
                || !queue.pending_requests.is_empty()
                || queue.active_request.is_some()
                || queue.completed < queue.total
        }) {
            return Ok(());
        }
        let Some(queue) = self.open_files_queue.take() else {
            return Ok(());
        };
        let status = match queue.last_error {
            Some(error) if queue.opened > 0 => format!("Открыто {} из {} файлов. {error}", queue.opened, queue.total),
            Some(error) => error,
            None => format!("Открыто {} файлов.", queue.opened),
        };
        tree.set_text(self.status, &status)?;
        tree.set_enabled(
            self.open_button,
            !self.library_workspace.is_saving()
                && !self.library_workspace.is_restoring()
                && !self.library_workspace.is_loading(),
        )?;
        if let Some(screen) = self.open_return_screen.take() {
            self.open(tree, screen)?;
        }
        Ok(())
    }

    fn advance_open_files_queue(&mut self, tree: &mut Tree, message: &Message<AppMessage>) -> Result<()> {
        let Some(finished) = (match message {
            Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) => {
                payload.downcast_ref::<super::saves::LoadFinished>()
            }
            _ => None,
        }) else {
            return Ok(());
        };
        let Some(queue) = self.open_files_queue.as_mut() else {
            return Ok(());
        };
        if queue.pending_requests.get(&finished.request) != Some(&finished.requested_path) {
            return Ok(());
        }

        queue.pending_requests.remove(&finished.request);
        if queue.active_request == Some(finished.request) {
            queue.active_request = None;
        }
        queue.completed = queue.completed.saturating_add(1);
        if finished.selected_path.is_some() {
            queue.opened = queue.opened.saturating_add(1);
        } else if let Some(error) = finished.error.as_deref() {
            queue.last_error = Some(format_open_error(&finished.requested_path, error, finished.io_error));
        }
        if queue.remaining.is_empty() && queue.pending_requests.is_empty() {
            return self.finish_open_files_queue(tree);
        }

        if self
            .open_files_queue
            .as_ref()
            .is_some_and(|queue| queue.active_request.is_none() && !queue.remaining.is_empty())
        {
            self.start_next_open_file(tree)?;
        }
        if let Some(queue) = self.open_files_queue.as_ref() {
            let status = queue.last_error.clone().unwrap_or_else(|| {
                format!(
                    "Открываю файл {} из {}…",
                    queue.completed.saturating_add(1),
                    queue.total
                )
            });
            tree.set_text(self.status, &status)?;
        }
        Ok(())
    }

    fn cancel_superseded_open_queue(&mut self) {
        let active = self.open_files_queue.as_ref().and_then(|queue| queue.active_request);
        if active.is_some_and(|request| request != self.library_workspace.load_request()) {
            self.open_files_queue = None;
        }
    }

    fn refresh_library_after_wizard(&mut self, tree: &mut Tree) -> Result<()> {
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
        self.render_library(tree)
    }

    fn begin_wizard_task<F>(
        &mut self,
        tree: &mut Tree,
        status: &str,
        kind: super::wizard::WizardTaskKind,
        work: F,
        on_ui_thread: bool,
    ) -> Result<()>
    where
        F: FnOnce() -> std::result::Result<super::wizard::WizardWorkResult, String> + Send + 'static,
    {
        if self.wizard_task_request.is_some() {
            return Ok(());
        }
        if !on_ui_thread && self.proxy.is_none() {
            tree.set_text(self.status, crate::strings::t("Фоновая очередь недоступна."))?;
            return Ok(());
        }
        self.next_wizard_task_request = self.next_wizard_task_request.saturating_add(1);
        let request = self.next_wizard_task_request;
        self.wizard_task_request = Some(request);
        self.wizard.set_busy(tree, true)?;
        tree.set_text(self.status, status)?;

        if on_ui_thread {
            let finished = super::wizard::WizardTaskFinished {
                request,
                kind,
                result: work(),
            };
            let _ = self.handle_wizard_task_finished(tree, &finished)?;
            return Ok(());
        }

        let Some(proxy) = self.proxy.clone() else {
            self.wizard_task_request = None;
            self.wizard.set_busy(tree, false)?;
            tree.set_text(self.status, crate::strings::t("Фоновая очередь недоступна."))?;
            return Ok(());
        };
        if let Err(error) = super::wizard::spawn_wizard_task(proxy, request, kind, work) {
            self.wizard_task_request = None;
            self.wizard.set_busy(tree, false)?;
            tree.set_text(
                self.status,
                &format!("{}: {error}", crate::strings::t(kind.failure_prefix())),
            )?;
        }
        Ok(())
    }

    fn handle_wizard_action(&mut self, tree: &mut Tree, action: super::wizard::WizardAction) -> Result<()> {
        match action {
            super::wizard::WizardAction::Navigate(target) => self.open(tree, target),
            super::wizard::WizardAction::DirectoryAdded => {
                tree.set_text(self.status, crate::strings::t("Папка добавлена в список поиска."))?;
                self.refresh_library_after_wizard(tree)
            }
            super::wizard::WizardAction::AutoSearch => self.begin_wizard_task(
                tree,
                crate::strings::t("Ищу папки с сохранениями…"),
                super::wizard::WizardTaskKind::AutoSearch,
                || {
                    super::wizard::Wizard::auto_search()
                        .map(super::wizard::WizardWorkResult::AutoSearch)
                        .map_err(|error| error.to_string())
                },
                false,
            ),
            super::wizard::WizardAction::Browse => self.begin_wizard_task(
                tree,
                crate::strings::t("Выберите папку с сохранениями…"),
                super::wizard::WizardTaskKind::Browse,
                || {
                    sse_sys::directory_dialog::choose_directory()
                        .map(super::wizard::WizardWorkResult::Directory)
                        .map_err(|error| error.to_string())
                },
                cfg!(target_os = "macos"),
            ),
        }
    }

    fn handle_wizard_task_finished(
        &mut self,
        tree: &mut Tree,
        finished: &super::wizard::WizardTaskFinished,
    ) -> Result<Flow> {
        if self.wizard_task_request != Some(finished.request) {
            return Ok(Flow::Continue);
        }
        self.wizard_task_request = None;
        self.wizard.set_busy(tree, false)?;
        if self.close_waiting {
            return Ok(Flow::Continue);
        }
        match &finished.result {
            Ok(super::wizard::WizardWorkResult::AutoSearch(added)) => {
                let status = if *added == 0 {
                    crate::strings::t("Автопоиск завершён. Новых папок не найдено.").to_owned()
                } else {
                    crate::strings::t("Автопоиск завершён. Добавлено папок: {0}.").replace("{0}", &added.to_string())
                };
                tree.set_text(self.status, &status)?;
                self.refresh_library_after_wizard(tree)?;
            }
            Ok(super::wizard::WizardWorkResult::Directory(Some(path))) => {
                self.wizard.set_directory_input(tree, path.clone())?;
                tree.set_text(self.status, crate::strings::t("Папка выбрана. Нажмите «Добавить»."))?;
            }
            Ok(super::wizard::WizardWorkResult::Directory(None)) => {
                tree.set_text(self.status, crate::strings::t("Выбор папки отменён."))?;
            }
            Err(error) => {
                tree.set_text(
                    self.status,
                    &format!("{}: {error}", crate::strings::t(finished.kind.failure_prefix())),
                )?;
            }
        }
        Ok(Flow::Continue)
    }

    fn show_open_file_dialog(&mut self, tree: &mut Tree) -> Result<()> {
        if self.library_workspace.is_saving()
            || self.library_workspace.is_restoring()
            || self.library_workspace.is_loading()
            || self.open_files_queue.is_some()
            || self.native_file_picker_request.is_some()
        {
            return Ok(());
        }
        #[cfg(not(test))]
        if self.proxy.is_some() {
            if cfg!(target_os = "macos") {
                match sse_sys::file_dialog::open_files() {
                    Ok(Some(paths)) if !paths.is_empty() => {
                        self.open_save_paths(tree, paths)?;
                        return Ok(());
                    }
                    Ok(Some(_) | None) => return Ok(()),
                    Err(error) => {
                        tree.set_text(self.status, &format!("Системный диалог недоступен: {error}"))?;
                    }
                }
            } else {
                self.next_native_file_picker_request = self.next_native_file_picker_request.saturating_add(1);
                let request = self.next_native_file_picker_request;
                self.native_file_picker_request = Some(request);
                tree.set_enabled(self.open_button, false)?;
                let Some(proxy) = self.proxy.clone() else {
                    return Ok(());
                };
                if let Err(error) = spawn_native_file_picker(proxy, request, sse_sys::file_dialog::open_files) {
                    self.native_file_picker_request = None;
                    tree.set_text(self.status, &format!("Системный диалог не запущен: {error}"))?;
                    self.sync_saving_overlay(tree)?;
                }
                return Ok(());
            }
        }
        self.open_path_input = TextInput::new("", open_path_edit_config())?;
        self.open_path_clipboard.0.clear();
        tree.set_input_text(self.open_path_widget, "")?;
        tree.set_enabled(self.open_confirm, false)?;
        tree.open_dialog(self.open_file_dialog)?;
        tree.set_focus(Some(self.open_path_widget))?;
        self.open_path_input.focus(true, 0);
        Ok(())
    }

    fn close_open_file_dialog(&mut self, tree: &mut Tree) -> Result<()> {
        if tree.dialog() == Some(self.open_file_dialog) {
            let _ = tree.close_dialog()?;
        }
        self.open_path_input.focus(false, 0);
        Ok(())
    }

    fn confirm_open_file(&mut self, tree: &mut Tree) -> Result<()> {
        let path = self.open_path_input.text();
        if path.is_empty() || self.proxy.is_none() {
            return Ok(());
        }
        if self.library_workspace.is_saving() || self.library_workspace.is_restoring() {
            return Ok(());
        }
        let path = PathBuf::from(path);
        self.close_open_file_dialog(tree)?;
        let _ = self.open_save_paths(tree, vec![path])?;
        Ok(())
    }

    fn edit_open_path(
        &mut self,
        tree: &mut Tree,
        keysym: u32,
        text: Option<char>,
        ctrl: bool,
        shift: bool,
    ) -> Result<()> {
        let key = match keysym {
            0xff08 => EditKey::Backspace,
            0xffff => EditKey::Delete,
            0xff51 => EditKey::Left,
            0xff53 => EditKey::Right,
            0xff50 => EditKey::Home,
            0xff57 => EditKey::End,
            value if ctrl && matches!(value, 0x61 | 0x41) => EditKey::A,
            value if ctrl && matches!(value, 0x63 | 0x43) => EditKey::C,
            value if ctrl && matches!(value, 0x76 | 0x56) => EditKey::V,
            value if ctrl && matches!(value, 0x78 | 0x58) => EditKey::X,
            value if ctrl && matches!(value, 0x7a | 0x5a) => EditKey::Z,
            _ => EditKey::Character(text.unwrap_or('\0')),
        };
        let typed = text.map(|character| character.to_string());
        self.open_path_input.focus(true, 0);
        self.open_path_input.key(
            key,
            Modifiers { ctrl, shift },
            typed.as_deref(),
            &mut self.open_path_clipboard,
        )?;
        let path = self.open_path_input.text();
        tree.set_input_text(self.open_path_widget, &path)?;
        tree.set_enabled(
            self.open_confirm,
            !path.is_empty() && !self.library_workspace.is_saving() && !self.library_workspace.is_restoring(),
        )?;
        Ok(())
    }

    fn select(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        if self.library_workspace.is_saving() || self.library_workspace.is_restoring() {
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
                Message::User(AppMessage::OpenBackups) => false,
                Message::User(AppMessage::OpenScreen(_)) => false,
                Message::User(AppMessage::OpenGameFix { .. }) => screen.id() == ScreenId::GameFixes,
                Message::User(AppMessage::OpenSavePicker { .. }) => false,
                Message::User(AppMessage::ToScreen(id, _)) => *id == screen.id(),
                Message::User(AppMessage::EditorAction(_)) => screen.id() == ScreenId::Inventory,
                Message::User(AppMessage::SoundLoaded(_, _)) => false,
                Message::User(AppMessage::SettingsWriteFinished(_)) => false,
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
        let language = crate::strings::current_language();
        let Some(source_sha256) = self.app.current_save_sha256() else {
            tree.set_text(self.draft_badge, &draft_badge_text(language, 0))?;
            let reason = SaveReason::SelectSave.localized(language);
            tree.set_text(self.save_reason, &reason)?;
            tree.set_tooltip(self.save, &reason)?;
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
        tree.set_text(self.draft_badge, &draft_badge_text(language, eligibility.change_count))?;
        let reason = eligibility.reason.localized(language);
        tree.set_text(self.save_reason, &reason)?;
        tree.set_tooltip(self.save, &reason)?;
        tree.set_enabled(
            self.undo,
            journal.is_some_and(sse_storage::drafts::DraftJournal::can_undo),
        )?;
        tree.set_enabled(
            self.redo,
            journal.is_some_and(sse_storage::drafts::DraftJournal::can_redo),
        )?;
        tree.set_enabled(self.reset, has_changes)?;
        let save_busy = self.library_workspace.is_saving() || self.library_workspace.is_restoring();
        tree.set_enabled(self.save, eligibility.can_save && !save_busy)?;
        Ok(())
    }

    fn dispatch_editor_action(&mut self, tree: &mut Tree, action: EditorAction) -> Result<()> {
        if action == EditorAction::Save && self.library_workspace.is_restoring() {
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
        let busy = self.library_workspace.is_saving() || self.library_workspace.is_restoring();
        tree.set_enabled(
            self.open_button,
            !busy
                && !self.library_workspace.is_loading()
                && self.open_files_queue.is_none()
                && self.native_file_picker_request.is_none(),
        )?;
        if self.library_workspace.is_saving() {
            if !tree.dialog_open() {
                tree.open_dialog(self.saving_dialog)?;
            }
        } else if tree.dialog() == Some(self.saving_dialog) {
            let _ = tree.close_dialog()?;
        }
        Ok(())
    }

    fn begin_draft_close_wait(&mut self) {
        if self.draft_close_started.is_none() {
            self.draft_close_started = Some(Instant::now());
            self.draft_close_prompted = false;
            self.draft_close_idle_seen = false;
        }
    }

    fn show_draft_close_prompt(&mut self, tree: &mut Tree) -> Result<()> {
        tree.set_text(self.force_close_message, crate::strings::t(DRAFT_CLOSE_WARNING))?;
        tree.set_visible(self.force_close_yes, false)?;
        if tree.dialog() != Some(self.force_close_dialog) {
            if tree.dialog_open() {
                let _ = tree.close_dialog()?;
            }
            tree.open_dialog(self.force_close_dialog)?;
        }
        tree.set_text(self.status, crate::strings::t(DRAFT_CLOSE_WARNING))?;
        self.draft_close_prompted = true;
        Ok(())
    }

    fn restore_game_close_prompt(&mut self, tree: &mut Tree) -> Result<()> {
        tree.set_text(self.force_close_message, crate::strings::t(FORCE_CLOSE_DEFAULT_MESSAGE))?;
        tree.set_visible(self.force_close_yes, true)?;
        Ok(())
    }

    fn show_game_close_prompt(&mut self, tree: &mut Tree) -> Result<()> {
        self.restore_game_close_prompt(tree)?;
        if tree.dialog() != Some(self.force_close_dialog) {
            if tree.dialog_open() {
                let _ = tree.close_dialog()?;
            }
            tree.open_dialog(self.force_close_dialog)?;
        }
        Ok(())
    }

    fn handle(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<Flow> {
        if let Message::User(AppMessage::SettingsWriteFinished(result)) = message {
            let status = match result {
                Ok(()) => crate::strings::t("Настройки сохранены.").to_owned(),
                Err(error) => format!("{}{}", crate::strings::t("Не удалось сохранить настройки: "), error),
            };
            tree.set_text(self.status, &crate::status::localize_writer_status(&status))?;
            return Ok(Flow::Continue);
        }

        let save_session = self.library_workspace.session();
        let write_active =
            sse_app::tasks::named_task_active("game-write") || sse_app::tasks::named_task_active("companion-write");
        let draft_write_active =
            sse_app::tasks::named_task_active("draft-save") || sse_app::tasks::named_task_active("draft-reset");
        let is_tick = matches!(message, Message::User(AppMessage::Tick(_)));
        if self.close_waiting
            && self.native_file_picker_request.is_none()
            && self.wizard_task_request.is_none()
            && !write_active
            && !save_session.is_saving()
            && !save_session.is_restoring()
        {
            self.close_waiting = false;
            if draft_write_active {
                self.begin_draft_close_wait();
            } else {
                return Ok(Flow::Exit);
            }
        }
        if save_session.deferred_close_ready() {
            if write_active {
                tree.set_text(
                    self.status,
                    "Дождитесь завершения фоновой операции с игрой, чтобы закрыть окно.",
                )?;
                self.sync_saving_overlay(tree)?;
                return Ok(Flow::Continue);
            }
            if draft_write_active {
                self.begin_draft_close_wait();
            } else if self.draft_close_started.is_none() && save_session.take_deferred_close_ready() {
                return Ok(Flow::Exit);
            }
        }
        if let Some(started) = self.draft_close_started {
            if draft_write_active {
                self.draft_close_idle_seen = false;
                if !self.draft_close_prompted && started.elapsed() >= Duration::from_secs(2) {
                    self.show_draft_close_prompt(tree)?;
                }
            } else if is_tick {
                if self.draft_close_idle_seen {
                    self.draft_close_started = None;
                    self.draft_close_prompted = false;
                    self.draft_close_idle_seen = false;
                    if tree.dialog() == Some(self.force_close_dialog) {
                        let _ = tree.close_dialog()?;
                    }
                    self.restore_game_close_prompt(tree)?;
                    let _ = save_session.take_deferred_close_ready();
                    return Ok(Flow::Exit);
                }
                self.draft_close_idle_seen = true;
            }
            if matches!(message, Message::Window(WindowEvent::CloseRequested)) {
                return Ok(Flow::Continue);
            }
        }
        match message {
            Message::Window(WindowEvent::CloseRequested) => {
                if save_session.request_close() == sse_app::CloseDecision::Deferred {
                    let text = if save_session.is_restoring() {
                        "Дождитесь завершения восстановления, чтобы закрыть окно."
                    } else {
                        "Дождитесь завершения сохранения, чтобы закрыть окно."
                    };
                    tree.set_text(self.status, text)?;
                    self.sync_saving_overlay(tree)?;
                    return Ok(Flow::Continue);
                }
                if write_active {
                    if self.close_waiting {
                        self.show_game_close_prompt(tree)?;
                    } else {
                        self.close_waiting = true;
                        tree.set_text(
                            self.status,
                            "Дождитесь завершения записи в игру/компаньон, чтобы закрыть окно.",
                        )?;
                    }
                    return Ok(Flow::Continue);
                }
                if self.native_file_picker_request.is_some() {
                    self.close_waiting = true;
                    tree.set_text(
                        self.status,
                        "Закройте системный диалог выбора файла, чтобы закрыть окно.",
                    )?;
                    return Ok(Flow::Continue);
                }
                if self.wizard_task_request.is_some() {
                    self.close_waiting = true;
                    tree.set_text(
                        self.status,
                        crate::strings::t("Дождитесь завершения операции мастера, чтобы закрыть окно."),
                    )?;
                    return Ok(Flow::Continue);
                }
                if draft_write_active {
                    self.begin_draft_close_wait();
                    return Ok(Flow::Continue);
                }
                return Ok(Flow::Exit);
            }
            Message::Window(WindowEvent::Disconnected) => {
                wait_for_save_io(&save_session);
                return Ok(Flow::Exit);
            }
            _ => {}
        }
        if clicked == Some(self.force_close_yes) {
            if self.draft_close_prompted {
                return Ok(Flow::Continue);
            }
            return Ok(Flow::Exit);
        }
        if clicked == Some(self.force_close_no) {
            if tree.dialog() == Some(self.force_close_dialog) {
                let _ = tree.close_dialog()?;
            }
            if self.draft_close_prompted {
                self.restore_game_close_prompt(tree)?;
                self.draft_close_started = Some(Instant::now());
                self.draft_close_prompted = false;
                self.draft_close_idle_seen = false;
                tree.set_text(self.status, "Ожидаю сохранения последней правки в черновик…")?;
            } else {
                self.restore_game_close_prompt(tree)?;
                tree.set_text(self.status, "Ожидаю завершения записи в игру/компаньон…")?;
            }
            return Ok(Flow::Continue);
        }
        if let Message::User(AppMessage::SoundLoaded(game, sounds)) = message {
            if self.sound_game.as_deref() == Some(game.as_str()) {
                self.sounds = (**sounds).clone();
            }
            return Ok(Flow::Continue);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message {
            if let Some(finished) = payload.downcast_ref::<NativeFilePickerFinished>() {
                if self.native_file_picker_request == Some(finished.request) {
                    self.native_file_picker_request = None;
                    if self.close_waiting {
                        return Ok(Flow::Continue);
                    }
                    match &finished.result {
                        Ok(Some(paths)) if !paths.is_empty() => {
                            if !self.open_save_paths(tree, paths.clone())? {
                                self.open_return_screen = None;
                                self.sync_saving_overlay(tree)?;
                            }
                        }
                        Ok(Some(_) | None) => {
                            self.open_return_screen = None;
                            self.sync_saving_overlay(tree)?;
                        }
                        Err(error) => {
                            self.open_return_screen = None;
                            tree.set_text(self.status, &format!("Системный диалог недоступен: {error}"))?;
                            self.sync_saving_overlay(tree)?;
                        }
                    }
                    return Ok(Flow::Continue);
                }
            }
            if let Some(finished) = payload.downcast_ref::<super::wizard::WizardTaskFinished>() {
                return self.handle_wizard_task_finished(tree, finished);
            }
            if let Some(finished) = payload.downcast_ref::<ReportUploadFinished>() {
                self.report_upload_pending = false;
                tree.set_text(self.report_send, &report_text("Отправить"))?;
                let status = match &finished.result {
                    Ok(report_id) => report_text("Отчёт отправлен, номер: {0}").replace("{0}", report_id),
                    Err(error) => {
                        let mut text = report_text("Отчёт не отправлен: {0}").replace("{0}", error);
                        if finished.local_saved {
                            text.push(' ');
                            text.push_str(&report_text("Отчёт сохраняется локально; ничего не отправляется."));
                        }
                        text
                    }
                };
                tree.set_text(self.status, &status)?;
                return Ok(Flow::Continue);
            }
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
        if (self.library_workspace.is_saving() || self.library_workspace.is_restoring())
            && !matches!(
                message,
                Message::User(AppMessage::ToScreen(_, _)) | Message::User(AppMessage::Tick(_))
            )
        {
            self.sync_saving_overlay(tree)?;
            return Ok(Flow::Continue);
        }
        if let Message::User(AppMessage::OpenSavePicker { return_to }) = message {
            self.open_return_screen = Some(*return_to);
            self.show_open_file_dialog(tree)?;
            if self.open_files_queue.is_none()
                && self.native_file_picker_request.is_none()
                && tree.dialog() != Some(self.open_file_dialog)
            {
                self.open_return_screen = None;
            }
            return Ok(Flow::Continue);
        }
        if let Message::User(AppMessage::OpenScreen(screen)) = message {
            self.open(tree, *screen)?;
            return Ok(Flow::Continue);
        }
        if matches!(message, Message::User(AppMessage::OpenBackups)) {
            self.open(tree, ScreenId::Backups)?;
            return Ok(Flow::Continue);
        }
        if let Message::User(AppMessage::OpenGameFix { game_id, .. }) = message {
            if self.app.selected_game() != Some(game_id.as_str()) {
                self.app.set_game_dir(None);
            }
            self.app.set_selected_game(Some(game_id.clone()));
            self.open(tree, ScreenId::GameFixes)?;
            self.route(tree, message, None)?;
            return Ok(Flow::Continue);
        }
        if clicked == Some(self.nav_toggle) {
            let wanted = !self.nav_collapsed;
            self.apply_navigation(tree, wanted)?;
            self.nav_user_choice = Some(wanted);
            super::submit_settings_write(
                sse_app::settings_writer::SettingsPatch::NavigationCollapsed(wanted),
                self.proxy.clone(),
            )?;
            tree.set_text(self.status, crate::strings::t("Сохраняю…"))?;
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
        if let Some(action) = self.wizard.message(tree, message, clicked, &mut wizard_status)? {
            self.handle_wizard_action(tree, action)?;
            return Ok(Flow::Continue);
        }
        if let Some(text) = wizard_status {
            let text = crate::status::localize_writer_status(&text);
            tree.set_text(self.status, &text)?;
        }
        if clicked.is_some() && clicked == Some(self.reports_ok) {
            self.reports_consented = true;
            super::submit_settings_write(
                sse_app::settings_writer::SettingsPatch::ReportsNotice {
                    send_reports: Some(true),
                },
                self.proxy.clone(),
            )?;
            if tree.dialog() == Some(self.reports_banner) {
                let _ = tree.close_dialog()?;
            }
            tree.set_text(self.status, crate::strings::t("Сохраняю…"))?;
            self.open_pending_report_dialog(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.reports_off) {
            self.reports_consented = false;
            super::submit_settings_write(
                sse_app::settings_writer::SettingsPatch::ReportsNotice {
                    send_reports: Some(false),
                },
                self.proxy.clone(),
            )?;
            if tree.dialog() == Some(self.reports_banner) {
                let _ = tree.close_dialog()?;
            }
            self.pending_report = None;
            sse_app::diagnostics::dismiss_crash();
            tree.set_text(self.status, crate::strings::t("Сохраняю…"))?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.report_cancel) {
            if tree.dialog() == Some(self.report_dialog) {
                let _ = tree.close_dialog()?;
            }
            self.pending_report = None;
            sse_app::diagnostics::dismiss_crash();
            tree.set_text(self.status, &report_text("Отчёт не отправлен."))?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.report_send) {
            if self.report_upload_pending {
                tree.set_text(self.status, &report_text("Отчёт уже собирается."))?;
                return Ok(Flow::Continue);
            }
            if !self.reports_consented {
                tree.set_text(self.status, &report_text("Отправка анонимных отчётов отключена."))?;
                return Ok(Flow::Continue);
            }
            let Some(proxy) = self.proxy.clone() else {
                tree.set_text(self.status, &report_text("Фоновая очередь недоступна."))?;
                return Ok(Flow::Continue);
            };
            if let Some(report) = self.pending_report.take() {
                self.report_upload_pending = true;
                tree.set_text(self.report_send, &report_text("Отправляю отчёт…"))?;
                tree.set_text(self.status, &report_text("Отправляю отчёт…"))?;
                sse_app::tasks::spawn_named_detached("diagnostics-upload", move || {
                    let (result, local_saved) = match sse_app::diagnostics::save_automatic_error_report(&report) {
                        Ok(_) => {
                            sse_app::diagnostics::dismiss_crash();
                            (
                                sse_app::diagnostics::upload_automatic_error_report(&report).map_err(|error| {
                                    sse_app::diagnostics::error(&format!("diagnostics upload: {error}"));
                                    error.to_string()
                                }),
                                true,
                            )
                        }
                        Err(error) => {
                            sse_app::diagnostics::error(&format!("diagnostics local save: {error}"));
                            (Err(error.to_string()), false)
                        }
                    };
                    let _ = proxy.send(AppMessage::ToScreen(
                        ScreenId::Overview,
                        Box::new(ReportUploadFinished { result, local_saved }),
                    ));
                });
            }
            if tree.dialog() == Some(self.report_dialog) {
                let _ = tree.close_dialog()?;
            }
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.open_button) {
            self.show_open_file_dialog(tree)?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.open_cancel) {
            self.close_open_file_dialog(tree)?;
            self.open_return_screen = None;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.open_confirm) {
            self.confirm_open_file(tree)?;
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
        if let Some(offset) =
            clicked.and_then(|id| self.library_rows.iter().position(|(_, _, select, _)| *select == id))
        {
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
        if let Message::Window(WindowEvent::Ime(event)) = message {
            if tree.dialog() == Some(self.open_file_dialog) && tree.focused() == Some(self.open_path_widget) {
                self.open_path_input.apply_ime_event(event)?;
                tree.set_input_text(self.open_path_widget, &self.open_path_input.display_text())?;
                return Ok(Flow::Continue);
            }
        }
        if let Some(clicked) = clicked {
            tree.set_focus(Some(clicked))?;
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
        }) = message
        {
            if *keysym == KEY_ESCAPE {
                self.route(tree, message, None)?;
                if tree.dialog_open() {
                    if tree.dialog() == Some(self.open_file_dialog) {
                        self.open_path_input.focus(false, 0);
                        self.open_return_screen = None;
                    }
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
                if tree.dialog() == Some(self.open_file_dialog) {
                    self.confirm_open_file(tree)?;
                    return Ok(Flow::Continue);
                }
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
            if tree.dialog() == Some(self.open_file_dialog) {
                if tree.focused() == Some(self.open_path_widget) {
                    self.edit_open_path(tree, *keysym, *text, *ctrl, *shift)?;
                }
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
        self.cancel_superseded_open_queue();
        self.advance_open_files_queue(tree, message)?;
        let library_preview = match message {
            Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) => {
                payload.downcast_ref::<LibraryPreviewFinished>().cloned()
            }
            _ => None,
        };
        if let Some(finished) = library_preview {
            let matches_pending = self
                .library_previews
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == finished.request && pending.key == finished.key);
            if matches_pending {
                self.library_previews.pending = None;
                self.library_previews.insert(LibraryPreviewEntry {
                    key: finished.key,
                    image: finished.image,
                    s2_detail: finished.s2_detail,
                    s2_jpeg_available: finished.s2_jpeg_available,
                });
                self.render_library(tree)?;
            }
        } else if matches!(
            message,
            Message::User(AppMessage::ToScreen(ScreenId::Overview, payload))
                if payload.is::<super::saves::LoadFinished>()
        ) {
            self.render_library(tree)?;
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message {
            if payload.is::<()>() {
                self.library_page = 0;
                if self.app.current_save().is_none()
                    && !self.library_workspace.is_loading()
                    && self.open_files_queue.is_none()
                {
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
        let visible_slots: Vec<SaveSlot> = slots.iter().skip(start).take(SAVE_LIBRARY_PAGE_SIZE).cloned().collect();
        let selected_path = self.app.current_save();
        let row_widgets = self.library_rows.clone();
        for (offset, (row, image, select, details)) in row_widgets.iter().enumerate() {
            if let Some(slot) = visible_slots.get(offset) {
                let preview = self.library_previews.get(&PreviewKey::from(slot));
                let filename = slot
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| "без имени".into());
                let game =
                    super::saves::format_display_name(slot.format_id.as_deref().unwrap_or(&slot.candidate_release_id));
                let displayed_filename = super::saves::short_text(&filename, 24);
                tree.set_image(*image, preview.as_ref().and_then(|entry| entry.image.clone()))?;
                let image_tooltip = if preview.as_ref().is_some_and(|entry| entry.image.is_some()) {
                    crate::strings::t("Скриншот, сохранённый игрой вместе с этим сохранением.")
                } else {
                    crate::strings::t("нет снимка")
                };
                tree.set_tooltip(*image, image_tooltip)?;
                let selection_label = if preview.as_ref().is_some_and(|entry| entry.image.is_some()) {
                    displayed_filename.to_string()
                } else if preview.as_ref().is_some_and(|entry| entry.s2_jpeg_available) {
                    format!("JPEG · {displayed_filename}")
                } else {
                    format!("нет снимка · {displayed_filename}")
                };
                tree.set_text(*select, &selection_label)?;
                tree.set_enabled(*select, slot.detection_error.is_none())?;
                let s2_detail = preview
                    .as_ref()
                    .and_then(|entry| entry.s2_detail.as_deref())
                    .map(|detail| format!(" · {detail}"))
                    .unwrap_or_default();
                tree.set_text(
                    *details,
                    &format!(
                        "{} · {} · {}{}",
                        game,
                        super::saves::display_file_time(slot.last_write_time_utc, true, false),
                        super::saves::display_size(slot.size),
                        s2_detail
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
        self.schedule_library_preview(&visible_slots);
        Ok(())
    }

    fn schedule_library_preview(&mut self, visible_slots: &[SaveSlot]) {
        if self.library_previews.pending.is_some() {
            return;
        }
        let Some(proxy) = self.proxy.clone() else {
            return;
        };
        let Some(slot) = visible_slots
            .iter()
            .find(|slot| !self.library_previews.contains(&PreviewKey::from(*slot)))
            .cloned()
        else {
            return;
        };
        let key = PreviewKey::from(&slot);
        let request = self.library_previews.next_request;
        self.library_previews.next_request = request.saturating_add(1);
        let fallback_key = key.clone();
        self.library_previews.pending = Some(PreviewRequest {
            id: request,
            key: key.clone(),
        });
        let workspace = self.library_workspace.clone();
        if workspace
            .spawn("save-preview", move |context| {
                if context.is_cancelled() {
                    return;
                }
                let result = load_library_preview(request, key, &slot);
                let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(result)));
            })
            .is_err()
        {
            self.library_previews.pending = None;
            self.library_previews.insert(LibraryPreviewEntry {
                key: fallback_key,
                image: None,
                s2_detail: None,
                s2_jpeg_available: false,
            });
        }
    }
}

fn load_library_preview(request: u64, key: PreviewKey, slot: &SaveSlot) -> LibraryPreviewFinished {
    let image = SavePreviewReader::preview_xray(&slot.path).and_then(thumbnail_image);
    let is_s2 = slot.format_id.as_deref() == Some("stalker2")
        || slot.candidate_game_id == "stalker2"
        || slot.candidate_release_id == "stalker2";
    let (s2_detail, s2_jpeg_available) = if is_s2 {
        (
            format_s2_preview_detail(SavePreviewReader::s2_slot_meta(&slot.path)),
            SavePreviewReader::preview_s2(&slot.path).is_some(),
        )
    } else {
        (None, false)
    };
    LibraryPreviewFinished {
        request,
        key,
        image,
        s2_detail,
        s2_jpeg_available,
    }
}

fn thumbnail_image(image: sse_content::RgbaImage) -> Option<ImageData> {
    let source_width = u64::try_from(image.width).ok()?;
    let source_height = u64::try_from(image.height).ok()?;
    let source_pixels = source_width.checked_mul(source_height)?;
    let expected_bytes = usize::try_from(source_pixels).ok()?.checked_mul(4)?;
    if source_width == 0 || source_height == 0 || source_pixels > 16_777_216 || image.pixels.len() != expected_bytes {
        return None;
    }

    let target_width = u64::from(LIBRARY_PREVIEW_WIDTH);
    let target_height = u64::from(LIBRARY_PREVIEW_HEIGHT);
    let (draw_width, draw_height) =
        if source_width.saturating_mul(target_height) >= source_height.saturating_mul(target_width) {
            (
                target_width,
                source_height
                    .saturating_mul(target_width)
                    .checked_div(source_width)?
                    .max(1),
            )
        } else {
            (
                source_width
                    .saturating_mul(target_height)
                    .checked_div(source_height)?
                    .max(1),
                target_height,
            )
        };
    let draw_width_usize = usize::try_from(draw_width).ok()?;
    let draw_height_usize = usize::try_from(draw_height).ok()?;
    let target_width_usize = usize::try_from(target_width).ok()?;
    let target_height_usize = usize::try_from(target_height).ok()?;
    let pixel_count = target_width_usize.checked_mul(target_height_usize)?;
    let background = crate::raster::Color::rgba(20, 24, 30, 255).to_u32();
    let mut pixels = vec![background; pixel_count];
    let left = (target_width_usize.saturating_sub(draw_width_usize)) / 2;
    let top = (target_height_usize.saturating_sub(draw_height_usize)) / 2;
    let denominator = u128::from(source_width).checked_mul(u128::from(source_height))?;

    for dy in 0..draw_height {
        let y0 = dy.checked_mul(source_height)?;
        let y1 = dy.saturating_add(1).checked_mul(source_height)?;
        let first_source_y = y0.checked_div(draw_height)?;
        let end_source_y = y1
            .saturating_add(draw_height.saturating_sub(1))
            .checked_div(draw_height)?;
        for dx in 0..draw_width {
            let x0 = dx.checked_mul(source_width)?;
            let x1 = dx.saturating_add(1).checked_mul(source_width)?;
            let first_source_x = x0.checked_div(draw_width)?;
            let end_source_x = x1
                .saturating_add(draw_width.saturating_sub(1))
                .checked_div(draw_width)?;
            let mut red_sum = 0_u128;
            let mut green_sum = 0_u128;
            let mut blue_sum = 0_u128;
            let mut alpha_sum = 0_u128;

            for sy in first_source_y..end_source_y {
                let overlap_y = y1
                    .min(sy.saturating_add(1).saturating_mul(draw_height))
                    .saturating_sub(y0.max(sy.saturating_mul(draw_height)));
                for sx in first_source_x..end_source_x {
                    let overlap_x = x1
                        .min(sx.saturating_add(1).saturating_mul(draw_width))
                        .saturating_sub(x0.max(sx.saturating_mul(draw_width)));
                    let weight = u128::from(overlap_x.checked_mul(overlap_y)?);
                    let source_index = sy.checked_mul(source_width)?.checked_add(sx)?;
                    let byte_offset = usize::try_from(source_index).ok()?.checked_mul(4)?;
                    let source = image.pixels.get(byte_offset..byte_offset.checked_add(4)?)?;
                    let red = u16::from(*source.first()?);
                    let green = u16::from(*source.get(1)?);
                    let blue = u16::from(*source.get(2)?);
                    let alpha = u16::from(*source.get(3)?);
                    let premultiply = |channel: u16| channel.checked_mul(alpha)?.checked_add(127)?.checked_div(255);
                    red_sum = red_sum.checked_add(u128::from(premultiply(red)?).checked_mul(weight)?)?;
                    green_sum = green_sum.checked_add(u128::from(premultiply(green)?).checked_mul(weight)?)?;
                    blue_sum = blue_sum.checked_add(u128::from(premultiply(blue)?).checked_mul(weight)?)?;
                    alpha_sum = alpha_sum.checked_add(u128::from(alpha).checked_mul(weight)?)?;
                }
            }

            let average = |sum: u128| {
                let half = denominator.checked_div(2)?;
                let rounded = sum.saturating_add(half);
                u8::try_from(rounded.checked_div(denominator)?).ok()
            };
            let red = average(red_sum)?;
            let green = average(green_sum)?;
            let blue = average(blue_sum)?;
            let alpha = average(alpha_sum)?;
            let dest_x = left.checked_add(usize::try_from(dx).ok()?)?;
            let dest_y = top.checked_add(usize::try_from(dy).ok()?)?;
            let dest_index = dest_y.checked_mul(target_width_usize)?.checked_add(dest_x)?;
            *pixels.get_mut(dest_index)? = crate::raster::Color::premultiplied_bgra(blue, green, red, alpha).to_u32();
        }
    }

    ImageData::new(LIBRARY_PREVIEW_WIDTH, LIBRARY_PREVIEW_HEIGHT, pixels).ok()
}

fn wait_for_save_io(session: &sse_app::SaveSession) {
    session.wait_until_idle();
}

impl App<AppMessage> for Shell {
    fn message(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Flow {
        match self.handle(tree, message, clicked) {
            Ok(flow) => flow,
            Err(error) => {
                let text = crate::status::localize_writer_status(&format!("Ошибка: {error}"));
                let _ = tree.set_text(self.status, &text);
                self.capture_error_report(tree, &error.to_string());
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
    use super::{
        save_eligibility, spawn_native_file_picker, wait_for_save_io, NativeFilePickerFinished, OpenFilesQueue,
        ScreenId, Shell,
    };
    use crate::event_loop::{channel_pair, Flow, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::raster::Color;
    use crate::screens::AppMessage;
    use crate::widget::{Tree, WidgetId};
    use sse_storage::drafts::{DraftPlacement, DraftPlan, JsonValue};
    use std::path::Path;

    fn close_task_test_guard() -> std::sync::MutexGuard<'static, ()> {
        crate::screens::task_registry_test_guard()
    }

    #[test]
    fn native_file_picker_runs_off_ui_thread_and_reports_completion() -> sse_core::Result<()> {
        let (proxy, receiver) = channel_pair::<AppMessage>();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let ui_thread = std::thread::current().id();
        spawn_native_file_picker(proxy, 41, move || {
            started_tx
                .send(std::thread::current().id())
                .map_err(|error| sse_core::Error::System(error.to_string()))?;
            release_rx
                .recv_timeout(std::time::Duration::from_secs(2))
                .map_err(|error| sse_core::Error::System(error.to_string()))?;
            Ok(None)
        })?;

        let worker_thread = started_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        assert_ne!(worker_thread, ui_thread);
        release_tx
            .send(())
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        let message = receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message else {
            return Err(sse_core::Error::Refused(
                "file picker completion was not routed to the shell".to_owned(),
            ));
        };
        let finished = payload
            .downcast_ref::<NativeFilePickerFinished>()
            .ok_or_else(|| sse_core::Error::Refused("file picker completion payload has the wrong type".to_owned()))?;
        assert_eq!(finished.request, 41);
        assert!(matches!(finished.result, Ok(None)));
        Ok(())
    }

    #[test]
    fn native_file_picker_cancel_clears_the_pending_request() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.native_file_picker_request = Some(7);
        let message = Message::User(AppMessage::ToScreen(
            ScreenId::Overview,
            Box::new(NativeFilePickerFinished {
                request: 7,
                result: Ok(None),
            }),
        ));

        assert_eq!(shell.handle(&mut tree, &message, None)?, Flow::Continue);
        assert_eq!(shell.native_file_picker_request, None);
        Ok(())
    }

    #[test]
    fn stale_native_file_picker_result_keeps_the_current_request_pending() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.native_file_picker_request = Some(9);
        let message = Message::User(AppMessage::ToScreen(
            ScreenId::Overview,
            Box::new(NativeFilePickerFinished {
                request: 8,
                result: Ok(None),
            }),
        ));

        assert_eq!(shell.handle(&mut tree, &message, None)?, Flow::Continue);
        assert_eq!(shell.native_file_picker_request, Some(9));
        Ok(())
    }

    fn click(shell: &mut Shell, tree: &mut Tree, id: WidgetId) -> sse_core::Result<()> {
        tree.update_layout()?;
        let rect = tree.rect(id)?;
        let x = rect.x.saturating_add(i32::try_from(rect.width / 2).unwrap_or_default());
        let y = rect
            .y
            .saturating_add(i32::try_from(rect.height / 2).unwrap_or_default());
        let pressed = Message::Window(WindowEvent::Button {
            button: 1,
            pressed: true,
            x,
            y,
        });
        let released = Message::Window(WindowEvent::Button {
            button: 1,
            pressed: false,
            x,
            y,
        });
        let down = tree.pointer_button(true, x, y);
        assert!(down.is_none());
        assert_eq!(shell.handle(tree, &pressed, down)?, Flow::Continue);
        let clicked = tree.pointer_button(false, x, y);
        assert_eq!(clicked, Some(id));
        assert_eq!(shell.handle(tree, &released, clicked)?, Flow::Continue);
        Ok(())
    }

    #[test]
    #[cfg(feature = "native-ui")]
    fn in_screen_open_command_selects_cloud_and_updates() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;

        for target in [ScreenId::Cloud, ScreenId::Updates] {
            let message = Message::User(AppMessage::OpenScreen(target));
            assert_eq!(shell.handle(&mut tree, &message, None)?, Flow::Continue);
            assert_eq!(shell.current(), Some(target));
        }
        Ok(())
    }

    #[test]
    #[cfg(feature = "native-ui")]
    fn pointer_events_walk_main_screens_and_confine_to_modal_overlay() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        tree.resize(1280, 860);
        shell.handle(
            &mut tree,
            &Message::Window(WindowEvent::Resized {
                width: 1280,
                height: 860,
            }),
            None,
        )?;
        let mut frame = vec![0_u32; 1280 * 860];
        tree.paint(&mut frame, 1280)?;

        for index in [1_usize, 6, 19, 0] {
            let id = shell
                .nav
                .get(index)
                .copied()
                .ok_or_else(|| sse_core::Error::Refused("missing navigation item".to_owned()))?;
            click(&mut shell, &mut tree, id)?;
            assert_eq!(shell.current(), ScreenId::ALL.get(index).copied());
            tree.paint(&mut frame, 1280)?;
        }

        let session = shell.library_workspace.session();
        let operation = session
            .begin_save(Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        shell.sync_saving_overlay(&mut tree)?;
        assert_eq!(tree.dialog(), Some(shell.saving_dialog));
        assert!(tree.hit(20, 20).is_none(), "modal overlay must confine pointer input");
        drop(operation);
        shell.sync_saving_overlay(&mut tree)?;
        assert!(!tree.dialog_open());
        Ok(())
    }

    #[test]
    fn toolbar_pointer_events_undo_redo_and_report_unavailable_save() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        tree.resize(1280, 860);
        let sha = "d".repeat(64);
        shell
            .app
            .set_current_save_identity(std::path::PathBuf::from("interaction.sav"), sha.clone());
        shell.app.set_current_save_format(Some("stalker-cop".to_owned()), false);
        let mut changed = DraftPlan::empty(&sha)?;
        changed.money = Some(12_345);
        shell.app.record_draft(changed)?;
        shell.sync_draft_controls(&mut tree)?;
        let mut frame = vec![0_u32; 1280 * 860];
        tree.paint(&mut frame, 1280)?;

        let undo = shell.undo;
        click(&mut shell, &mut tree, undo)?;
        assert!(shell.app.draft(&sha).is_some_and(|plan| plan.money.is_none()));

        let redo = shell.redo;
        click(&mut shell, &mut tree, redo)?;
        assert_eq!(shell.app.draft(&sha).and_then(|plan| plan.money), Some(12_345));

        let previous_status = tree.text(shell.status)?.to_owned();
        let save = shell.save;
        click(&mut shell, &mut tree, save)?;
        let status = tree.text(shell.status)?;
        assert!(
            !status.is_empty() && status != previous_status,
            "save should report its unavailable state"
        );
        Ok(())
    }

    #[test]
    fn startup_recovery_offer_opens_the_backup_screen() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        assert_eq!(shell.current(), Some(ScreenId::Overview));

        let message = Message::User(AppMessage::OpenBackups);
        assert_eq!(shell.handle(&mut tree, &message, None)?, Flow::Continue);
        assert_eq!(shell.current(), Some(ScreenId::Backups));
        Ok(())
    }

    fn write_test_bytes(target: &mut [u8], offset: usize, bytes: &[u8]) -> sse_core::Result<()> {
        let end = offset
            .checked_add(bytes.len())
            .ok_or_else(|| sse_core::Error::Damaged("test image range overflow".to_owned()))?;
        let destination = target
            .get_mut(offset..end)
            .ok_or_else(|| sse_core::Error::Damaged("test image range is out of bounds".to_owned()))?;
        destination.copy_from_slice(bytes);
        Ok(())
    }

    #[test]
    fn preview_thumbnail_preserves_aspect_and_averages_source_pixels() -> sse_core::Result<()> {
        let source = sse_content::RgbaImage::new(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let image = super::thumbnail_image(source)
            .ok_or_else(|| sse_core::Error::System("test thumbnail could not be built".to_owned()))?;
        assert_eq!((image.width, image.height), (96, 54));
        let left = image
            .pixels
            .get(2_616)
            .copied()
            .ok_or_else(|| sse_core::Error::System("left test pixel is missing".to_owned()))?;
        let right = image
            .pixels
            .get(2_664)
            .copied()
            .ok_or_else(|| sse_core::Error::System("right test pixel is missing".to_owned()))?;
        assert!(left & 0x00ff_0000 > 0x00c8_0000);
        assert!(right & 0x0000_00ff > 0x0000_00c8);
        Ok(())
    }

    #[test]
    fn library_loads_visible_save_previews_in_background() -> sse_core::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("sse-shell-preview-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&directory)?;
        let path = directory.join("preview.sav");
        std::fs::write(
            &path,
            include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
        )?;
        let canonical_path = std::fs::canonicalize(&path)?;
        let mut dds = vec![0_u8; 132];
        write_test_bytes(&mut dds, 0, b"DDS ")?;
        write_test_bytes(&mut dds, 12, &1_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 16, &1_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 20, &4_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 80, &0x40_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 88, &32_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 92, &0x00ff_0000_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 96, &0x0000_ff00_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 100, &0x0000_00ff_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 104, &0xff00_0000_u32.to_le_bytes())?;
        write_test_bytes(&mut dds, 128, &[0, 0, 255, 255])?;
        std::fs::write(path.with_extension("dds"), dds)?;
        let result = (|| {
            let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
            let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
            let mut shell = Shell::build_for_test(&mut tree, None)?;
            shell.set_proxy(proxy);

            assert!(shell.open_save(&mut tree, &path)?);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            let mut load_finished = false;
            let mut preview_finished = None;
            while std::time::Instant::now() < deadline {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                let message = receiver
                    .recv_timeout(remaining.min(std::time::Duration::from_millis(100)))
                    .ok();
                let Some(message) = message else {
                    shell.render_library(&mut tree)?;
                    continue;
                };
                if let Message::User(super::super::AppMessage::ToScreen(ScreenId::Overview, payload)) = &message {
                    if let Some(finished) = payload.downcast_ref::<super::super::saves::LoadFinished>() {
                        load_finished = finished.requested_path == path && finished.selected_path.is_some();
                    }
                    if let Some(finished) = payload.downcast_ref::<super::LibraryPreviewFinished>() {
                        if finished.key.path == canonical_path {
                            preview_finished = Some(finished.clone());
                        }
                    }
                }
                shell.handle(&mut tree, &message, None)?;
                shell.render_library(&mut tree)?;
                if load_finished && preview_finished.is_some() {
                    break;
                }
            }
            assert!(load_finished, "the fixture save should finish loading");
            let finished = preview_finished.ok_or_else(|| {
                sse_core::Error::System("visible library preview did not finish in background".to_owned())
            })?;
            let image = finished
                .image
                .ok_or_else(|| sse_core::Error::System("fixture DDS did not decode".to_owned()))?;
            assert_eq!((image.width, image.height), (96, 54));
            let center = image.pixels.get(2_640).copied().unwrap_or_default();
            assert!(
                center & 0x00ff_0000 > 0x00c8_0000,
                "the preview should preserve its red pixel"
            );
            let first_image = shell
                .library_rows
                .first()
                .map(|row| row.1)
                .ok_or_else(|| sse_core::Error::System("save library has no preview row".to_owned()))?;
            assert!(tree.image(first_image)?.is_some());
            Ok(())
        })();
        let _ = std::fs::remove_dir_all(&directory);
        result
    }

    #[test]
    fn late_open_result_is_counted_while_next_request_is_active() -> sse_core::Result<()> {
        let first = Path::new("first.sav").to_path_buf();
        let second = Path::new("second.sav").to_path_buf();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.open_files_queue = Some(OpenFilesQueue {
            remaining: std::collections::VecDeque::new(),
            pending_requests: [(1, first.clone()), (2, second.clone())].into_iter().collect(),
            active_request: Some(2),
            total: 2,
            completed: 0,
            opened: 0,
            last_error: None,
        });
        let message = Message::User(super::super::AppMessage::ToScreen(
            ScreenId::Overview,
            Box::new(super::super::saves::LoadFinished {
                request: 1,
                selected_path: Some(first.clone()),
                requested_path: first,
                journal: None,
                error: None,
                io_error: false,
            }),
        ));

        shell.advance_open_files_queue(&mut tree, &message)?;

        assert!(shell.open_files_queue.as_ref().is_some_and(|queue| {
            queue.completed == 1
                && queue.opened == 1
                && queue.active_request == Some(2)
                && queue.pending_requests.len() == 1
                && queue.pending_requests.contains_key(&2)
        }));
        Ok(())
    }

    #[test]
    fn save_disabled_reason_matches_reference_priority_and_capabilities() -> sse_core::Result<()> {
        let source_sha256 = "a".repeat(64);
        assert_eq!(
            save_eligibility(false, None, false, None, false).reason.localized("ru"),
            "Выберите сохранение для редактирования."
        );
        assert_eq!(
            save_eligibility(true, None, false, None, true).reason.localized("ru"),
            "Нет несохранённых изменений."
        );

        let mut unsupported = DraftPlan::empty(&source_sha256)?;
        unsupported.placements.insert(5, DraftPlacement::Ruck);
        assert_eq!(
            save_eligibility(true, Some("stalker2"), false, Some(&unsupported), false)
                .reason
                .localized("ru"),
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );

        let mut supported = DraftPlan::empty(&source_sha256)?;
        supported.money = Some(700);
        let invalid = save_eligibility(true, Some("stalker2"), false, Some(&supported), true);
        assert_eq!(
            invalid.reason.localized("ru"),
            "Введены некорректные значения (проверьте введённые числа)."
        );
        assert!(!invalid.can_save);

        supported.unmapped_legacy_plan = Some(JsonValue::Null);
        assert_eq!(
            save_eligibility(true, Some("stalker2"), false, Some(&supported), true)
                .reason
                .localized("ru"),
            "В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом)."
        );
        Ok(())
    }

    #[test]
    fn open_toolbar_loads_a_save_from_the_path_dialog_in_the_background() -> sse_core::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sse shell open {nonce}.sav"));
        std::fs::write(
            &path,
            include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
        )?;
        let expected_path = std::fs::canonicalize(&path)?;
        let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.open(&mut tree, ScreenId::Overview)?;
        shell.set_proxy(proxy);

        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::Tick(0)),
            Some(shell.open_button),
        )?;
        assert_eq!(tree.dialog(), Some(shell.open_file_dialog));
        assert_eq!(tree.focused(), Some(shell.open_path_widget));
        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::Tick(0)),
            Some(shell.open_confirm),
        )?;
        assert!(tree.dialog_open(), "an empty path must leave the dialog open");

        shell.handle(
            &mut tree,
            &Message::Window(WindowEvent::Key {
                pressed: true,
                keysym: 0xff1b,
                text: None,
                ctrl: false,
                shift: false,
            }),
            None,
        )?;
        assert!(!tree.dialog_open());

        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::Tick(0)),
            Some(shell.open_button),
        )?;
        for character in path.to_string_lossy().chars() {
            shell.handle(
                &mut tree,
                &Message::Window(WindowEvent::Key {
                    pressed: true,
                    keysym: u32::from(character),
                    text: Some(character),
                    ctrl: false,
                    shift: false,
                }),
                None,
            )?;
        }
        assert_eq!(tree.input_text(shell.open_path_widget)?, path.to_string_lossy());
        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::Tick(0)),
            Some(shell.open_confirm),
        )?;
        assert!(!tree.dialog_open());

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while shell.app.current_save() != Some(expected_path.as_path()) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                let _ = std::fs::remove_file(&path);
                return Err(sse_core::Error::System(
                    "timed out loading a path-selected fixture".to_owned(),
                ));
            }
            let message = match receiver.recv_timeout(remaining) {
                Ok(message) => message,
                Err(error) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(sse_core::Error::System(error.to_string()));
                }
            };
            shell.handle(&mut tree, &message, None)?;
        }
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn shared_save_picker_returns_to_doctor_after_background_load() -> sse_core::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sse-doctor-picker-{nonce}.sav"));
        std::fs::write(
            &path,
            include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
        )?;
        let expected_path = std::fs::canonicalize(&path)?;
        let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.open(&mut tree, ScreenId::Overview)?;
        shell.set_proxy(proxy);

        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::OpenSavePicker {
                return_to: ScreenId::SaveDoctor,
            }),
            None,
        )?;
        assert_eq!(tree.dialog(), Some(shell.open_file_dialog));
        for character in path.to_string_lossy().chars() {
            shell.handle(
                &mut tree,
                &Message::Window(WindowEvent::Key {
                    pressed: true,
                    keysym: u32::from(character),
                    text: Some(character),
                    ctrl: false,
                    shift: false,
                }),
                None,
            )?;
        }
        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::Tick(0)),
            Some(shell.open_confirm),
        )?;

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        while shell.app.current_save() != Some(expected_path.as_path()) {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                let _ = std::fs::remove_file(&path);
                return Err(sse_core::Error::System(
                    "timed out loading the Save Doctor fixture".to_owned(),
                ));
            }
            let message = receiver
                .recv_timeout(remaining)
                .map_err(|error| sse_core::Error::System(error.to_string()))?;
            shell.handle(&mut tree, &message, None)?;
        }

        assert_eq!(shell.current(), Some(ScreenId::SaveDoctor));
        std::fs::remove_file(path)?;
        Ok(())
    }

    #[test]
    fn opening_multiple_saves_keeps_each_success_in_the_library() -> sse_core::Result<()> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|error| sse_core::Error::System(error.to_string()))?
            .as_nanos();
        let first = std::env::temp_dir().join(format!("sse-shell-open-first-{nonce}.sav"));
        let second = std::env::temp_dir().join(format!("sse-shell-open-second-{nonce}.sav"));
        let missing = std::env::temp_dir().join(format!("sse-shell-open-missing-{nonce}.sav"));
        let fixture = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        std::fs::write(&first, fixture)?;
        std::fs::write(&second, fixture)?;
        let expected_first_path = std::fs::canonicalize(&first)?;
        let expected_second_path = std::fs::canonicalize(&second)?;

        let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.open(&mut tree, ScreenId::Overview)?;
        shell.set_proxy(proxy);

        assert!(shell.open_save(&mut tree, &first)?);
        let seed_request = shell.library_workspace.load_request();
        while shell.app.current_save() != Some(expected_first_path.as_path()) {
            let message = receiver
                .recv_timeout(std::time::Duration::from_secs(10))
                .map_err(|error| sse_core::Error::System(error.to_string()))?;
            shell.handle(&mut tree, &message, None)?;
        }
        assert_eq!(shell.library_workspace.load_request(), seed_request);

        assert!(shell.open_save_paths(&mut tree, vec![missing.clone(), first.clone(), second.clone()])?);
        assert!(shell.library_workspace.is_loading());
        let first_request = shell.library_workspace.load_request();
        shell.handle(
            &mut tree,
            &Message::User(super::super::AppMessage::ToScreen(ScreenId::Overview, Box::new(()))),
            None,
        )?;
        assert_eq!(shell.library_workspace.load_request(), first_request);
        assert!(shell.open_files_queue.is_some());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut second_request = None;
        let mut completed = 0;
        let mut results = Vec::new();
        let mut states = Vec::new();
        let mut overview_refreshes = 0;
        while shell.open_files_queue.is_some() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                let _ = std::fs::remove_file(&first);
                let _ = std::fs::remove_file(&second);
                return Err(sse_core::Error::System(
                    "timed out loading selected fixtures".to_owned(),
                ));
            }
            let message = match receiver.recv_timeout(remaining) {
                Ok(message) => message,
                Err(error) => {
                    let _ = std::fs::remove_file(&first);
                    let _ = std::fs::remove_file(&second);
                    return Err(sse_core::Error::System(error.to_string()));
                }
            };
            if let Message::User(super::super::AppMessage::ToScreen(ScreenId::Overview, payload)) = &message {
                if let Some(finished) = payload.downcast_ref::<super::super::saves::LoadFinished>() {
                    completed += 1;
                    results.push((
                        finished.request,
                        finished.requested_path.clone(),
                        finished.selected_path.clone(),
                        finished.error.clone(),
                    ));
                } else if payload.is::<()>() {
                    overview_refreshes += 1;
                }
            }
            shell.handle(&mut tree, &message, None)?;
            states.push((
                shell.library_workspace.load_request(),
                shell.app.current_save().map(Path::to_path_buf),
                shell.open_files_queue.as_ref().and_then(|queue| queue.active_request),
            ));
            if second_request.is_none() && shell.library_workspace.load_request() != first_request {
                second_request = Some(shell.library_workspace.load_request());
            }
        }

        assert_eq!(
            completed,
            3,
            "a failed file must not stop later selections; load results: {results:?}; states: {states:?}; overview refreshes: {overview_refreshes}"
        );
        assert!(
            second_request.is_some(),
            "the second load starts after the first completes"
        );
        let (_, _, slots) = shell.library_workspace.library_snapshot();
        assert!(slots.iter().any(|slot| slot.path == expected_first_path));
        assert!(slots.iter().any(|slot| slot.path == expected_second_path));
        assert_eq!(
            shell.app.current_save(),
            Some(expected_second_path.as_path()),
            "load results: {results:?}; states: {states:?}; overview refreshes: {overview_refreshes}"
        );

        std::fs::remove_file(first)?;
        std::fs::remove_file(second)?;
        Ok(())
    }

    #[test]
    fn open_file_errors_use_the_acceptance_text() {
        assert_eq!(
            super::format_open_error(Path::new("/tmp/broken save.sav"), "ignored parse detail", false),
            "«broken save.sav» — не сохранение S.T.A.L.K.E.R. или файл повреждён."
        );
        assert_eq!(
            super::format_open_error(Path::new("/tmp/missing save.sav"), "permission denied", true),
            "Не удалось открыть «missing save.sav»: permission denied"
        );
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
            eligibility.reason.localized("ru"),
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        Ok(())
    }

    #[test]
    fn saving_uses_a_modal_overlay_and_closes_it_after_completion() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let shell = Shell::build_for_test(&mut tree, None)?;

        let session = shell.library_workspace.session();
        let operation = session
            .begin_save(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        shell.sync_saving_overlay(&mut tree)?;
        assert!(tree.dialog_open());
        assert_eq!(tree.dialog(), Some(shell.saving_dialog));

        drop(operation);
        shell.sync_saving_overlay(&mut tree)?;
        assert!(!tree.dialog_open());
        Ok(())
    }

    #[test]
    fn close_waits_for_the_matching_wizard_task_and_ignores_stale_results() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.wizard_task_request = Some(9);
        shell.native_file_picker_request = Some(11);

        let close = Message::Window(WindowEvent::CloseRequested);
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        assert!(shell.close_waiting);

        let stale = super::super::wizard::WizardTaskFinished {
            request: 8,
            kind: super::super::wizard::WizardTaskKind::Browse,
            result: Ok(super::super::wizard::WizardWorkResult::Directory(None)),
        };
        assert_eq!(shell.handle_wizard_task_finished(&mut tree, &stale)?, Flow::Continue);
        assert_eq!(shell.wizard_task_request, Some(9));

        let finished = super::super::wizard::WizardTaskFinished {
            request: 9,
            kind: super::super::wizard::WizardTaskKind::Browse,
            result: Ok(super::super::wizard::WizardWorkResult::Directory(None)),
        };
        assert_eq!(shell.handle_wizard_task_finished(&mut tree, &finished)?, Flow::Continue);
        assert_eq!(shell.wizard_task_request, None);

        let native_finished = Message::User(AppMessage::ToScreen(
            ScreenId::Overview,
            Box::new(super::NativeFilePickerFinished {
                request: 11,
                result: Ok(None),
            }),
        ));
        assert_eq!(shell.handle(&mut tree, &native_finished, None)?, Flow::Continue);
        assert_eq!(shell.native_file_picker_request, None);
        assert!(shell.close_waiting);
        assert_eq!(
            shell.handle(&mut tree, &Message::User(AppMessage::Tick(1)), None)?,
            Flow::Exit
        );
        assert!(!shell.close_waiting);
        Ok(())
    }

    #[test]
    fn close_request_waits_for_an_active_restore_and_exits_after_completion() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let close = Message::Window(WindowEvent::CloseRequested);
        let session = shell.library_workspace.session();
        let operation = session
            .begin_restore(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test restore did not start".to_owned()))?;

        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        drop(operation);
        let tick = Message::User(super::AppMessage::Tick(1));
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn deferred_close_keeps_game_operation_blocker_until_it_finishes() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let session = shell.library_workspace.session();
        let operation = session
            .begin_restore(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test restore did not start".to_owned()))?;
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        sse_app::tasks::spawn_named_detached("game-write", move || {
            let _ = started_sender.send(());
            let _ = release_receiver.recv();
        });
        started_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;

        let close = Message::Window(WindowEvent::CloseRequested);
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        drop(operation);
        let tick = Message::User(super::AppMessage::Tick(1));
        let while_game_operation_active = shell.handle(&mut tree, &tick, None)?;
        release_sender
            .send(())
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        assert!(sse_app::tasks::wait_for_named_tasks(
            &["game-write"],
            std::time::Duration::from_secs(1)
        ));

        assert_eq!(while_game_operation_active, Flow::Continue);
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn close_request_waits_for_an_active_save_and_exits_after_completion() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let close = Message::Window(WindowEvent::CloseRequested);

        let operation = shell
            .library_workspace
            .session()
            .begin_save(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        drop(operation);
        let tick = Message::User(super::AppMessage::Tick(1));
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn close_request_does_not_block_on_a_pending_draft_write() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        sse_app::tasks::spawn_named_detached("draft-save", move || {
            let _ = started_sender.send(());
            let _ = release_receiver.recv();
        });
        started_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;

        let started = std::time::Instant::now();
        let close = Message::Window(WindowEvent::CloseRequested);
        let result = shell.handle(&mut tree, &close, None);
        let elapsed = started.elapsed();
        let _ = release_sender.send(());
        assert!(sse_app::tasks::wait_for_named_tasks(
            &["draft-save"],
            std::time::Duration::from_secs(1)
        ));
        assert_eq!(result?, Flow::Continue);
        assert!(elapsed < std::time::Duration::from_millis(100));
        let tick = Message::User(super::AppMessage::Tick(1));
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Continue);
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn close_prompt_offers_to_wait_when_draft_write_takes_two_seconds() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let (started_sender, started_receiver) = std::sync::mpsc::channel();
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        sse_app::tasks::spawn_named_detached("draft-save", move || {
            let _ = started_sender.send(());
            let _ = release_receiver.recv();
        });
        started_receiver
            .recv_timeout(std::time::Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;

        let close = Message::Window(WindowEvent::CloseRequested);
        let close_flow = shell.handle(&mut tree, &close, None)?;
        if close_flow == Flow::Continue {
            std::thread::sleep(std::time::Duration::from_millis(2_100));
            let tick = Message::User(super::AppMessage::Tick(3));
            assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Continue);
            assert_eq!(tree.dialog(), Some(shell.force_close_dialog));
            assert_eq!(
                shell.handle(&mut tree, &tick, Some(shell.force_close_no))?,
                Flow::Continue
            );
            assert!(!tree.dialog_open());
        }
        let _ = release_sender.send(());
        assert!(sse_app::tasks::wait_for_named_tasks(
            &["draft-save"],
            std::time::Duration::from_secs(1)
        ));
        assert_eq!(close_flow, Flow::Continue);
        let tick = Message::User(super::AppMessage::Tick(4));
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Continue);
        assert_eq!(shell.handle(&mut tree, &tick, None)?, Flow::Exit);
        Ok(())
    }

    #[test]
    fn screen_and_save_selection_cannot_change_during_a_write() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let current = shell.current();
        let operation = shell
            .library_workspace
            .session()
            .begin_save(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;

        shell.open(&mut tree, ScreenId::Inventory)?;
        assert_eq!(shell.current(), current);
        assert!(!shell.open_save(&mut tree, std::path::Path::new("other-save.sav"))?);
        assert_eq!(shell.current(), current);

        drop(operation);
        Ok(())
    }

    #[test]
    fn save_library_is_visible_on_save_screens_and_hidden_elsewhere() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;

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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
    fn draft_badge_translation_preserves_the_count_template() {
        assert_eq!(super::draft_badge_text("en", 3), "Draft: 3 actions");
    }

    #[test]
    fn save_reason_translation_preserves_the_format_name_argument() -> sse_core::Result<()> {
        let source_sha256 = "d".repeat(64);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.placements.insert(1, DraftPlacement::Ruck);

        let eligibility = save_eligibility(true, Some("stalker2"), false, Some(&plan), false);

        assert_eq!(
            eligibility.reason.localized("en"),
            "This change is not supported for S.T.A.L.K.E.R. 2 (see \"Capabilities\")."
        );
        Ok(())
    }

    #[test]
    fn draft_badge_and_save_reasons_are_localized_in_every_language() {
        let reasons = [
            super::SaveReason::SelectSave,
            super::SaveReason::UnmappedDraft,
            super::SaveReason::UnsupportedFormat(super::display_format_name("stalker-soc")),
            super::SaveReason::NoChanges,
            super::SaveReason::InvalidNumbers,
            super::SaveReason::CanSave,
        ];

        for language in crate::strings::LANGUAGES {
            let badge = super::draft_badge_text(language, 3);
            assert!(badge.contains('3'), "missing count for {language}: {badge}");
            assert!(!badge.contains("{0}"), "unformatted count for {language}: {badge}");
            if language != "ru" {
                assert_ne!(badge, "Черновик: 3 действ.", "untranslated badge for {language}");
            }

            for reason in reasons {
                let text = reason.localized(language);
                assert!(!text.is_empty(), "empty save reason for {language}");
                assert!(!text.contains("{0}"), "unformatted save reason for {language}: {text}");
                if language == "en" {
                    assert!(
                        !text.chars().any(|ch| ('\u{0400}'..='\u{04ff}').contains(&ch)),
                        "Russian text in English save reason: {text}"
                    );
                }
            }
        }
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
            multiple.reason.localized("ru"),
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        plan.s2_stash_takes.pop();
        let eligibility = save_eligibility(true, Some("stalker2"), false, Some(&plan), false);
        assert_eq!(eligibility.change_count, 1);
        assert!(!eligibility.can_save);
        assert_eq!(
            eligibility.reason.localized("ru"),
            "Эта правка для формата S.T.A.L.K.E.R. 2 не поддерживается (см. «Возможности»)."
        );
        assert!(!save_eligibility(true, Some("stalker2"), true, Some(&plan), false).can_save);
        Ok(())
    }

    #[test]
    fn disconnect_waits_until_save_write_finishes() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let session = sse_app::SaveSession::new();
        let operation = session
            .begin_save(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("save lease should start".to_owned()))?;
        let waiter_session = session.clone();
        let waiter = std::thread::spawn(move || wait_for_save_io(&waiter_session));
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(session.is_busy());
        drop(operation);
        assert!(waiter.join().is_ok());
        assert!(!session.is_busy());
        Ok(())
    }

    #[test]
    fn escape_does_not_close_the_application() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        let initial = shell.current();
        let operation = shell
            .library_workspace
            .session()
            .begin_restore(std::path::Path::new("fixture.sav"))
            .ok_or_else(|| sse_core::Error::Refused("test restore did not start".to_owned()))?;
        let save = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('s'),
            text: None,
            ctrl: true,
            shift: false,
        });

        assert!(matches!(shell.handle(&mut tree, &save, None)?, Flow::Continue));
        assert_eq!(shell.current(), initial);
        drop(operation);
        assert!(!shell.library_workspace.is_restoring());
        Ok(())
    }

    #[test]
    fn ctrl_s_opens_inventory_and_keeps_the_shell_running() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let expected_path = std::fs::canonicalize(&path)?;
        let (proxy, receiver) = channel_pair::<super::super::AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build_for_test(&mut tree, None)?;
        shell.open(&mut tree, ScreenId::Overview)?;
        shell.set_proxy(proxy);
        assert!(shell.open_save(&mut tree, &path)?);

        while shell.app.current_save() != Some(expected_path.as_path()) {
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
        let mut shell = Shell::build_for_test(&mut tree, None)?;
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
    #[test]
    fn close_ignores_reads_and_confirms_second_request_during_write() -> sse_core::Result<()> {
        let _guard = close_task_test_guard();
        let close = Message::Window(WindowEvent::CloseRequested);

        let (read_tx, read_rx) = std::sync::mpsc::channel();
        sse_app::tasks::spawn_named_detached("game-read", move || {
            let _ = read_rx.recv();
        });
        let mut read_tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut read_shell = Shell::build(&mut read_tree, None)?;
        assert_eq!(read_shell.handle(&mut read_tree, &close, None)?, Flow::Exit);
        let _ = read_tx.send(());

        let (write_tx, write_rx) = std::sync::mpsc::channel();
        sse_app::tasks::spawn_named_detached("game-write", move || {
            let _ = write_rx.recv();
        });
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        assert!(!tree.dialog_open());
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        assert_eq!(tree.dialog(), Some(shell.force_close_dialog));

        let tick = Message::User(super::AppMessage::Tick(0));
        assert_eq!(
            shell.handle(&mut tree, &tick, Some(shell.force_close_no))?,
            Flow::Continue
        );
        assert!(!tree.dialog_open());
        assert_eq!(shell.handle(&mut tree, &close, None)?, Flow::Continue);
        assert_eq!(tree.dialog(), Some(shell.force_close_dialog));
        assert_eq!(shell.handle(&mut tree, &tick, Some(shell.force_close_yes))?, Flow::Exit);
        let _ = write_tx.send(());
        let _ = sse_app::tasks::wait_for_named_tasks(&["game-read", "game-write"], std::time::Duration::from_secs(1));
        Ok(())
    }
}
