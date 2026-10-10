//! S2 save screens: discovery, overview, inventory, factions, stashes and transitions.

use super::shell::library_icon_button;
use super::style::{self, Button, Text};
use super::{AppMessage, BrowserDownload, BrowserFileBridge, Context, EditorAction, Screen, ScreenId};
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::{Message, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{NodeKind, Size, Style};
use crate::path::Icon;
use crate::process_guard::{is_windows_file_busy_error_text, running_game_for_format, SAVE_WHILE_GAME_RUNNING_WARNING};
use crate::widget::{Content, Look, Tree, WidgetId};
use crate::widgets::text_input::TextInput;
use sse_core::{Error, Result, SaveBuffer};
use sse_s2::{S2Change, S2InventoryItem, S2Save, S2StashItem, S2StashLayout};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::drafts::{AddRequest, DraftJournal, DraftPlacement, DraftPlan, DraftStore};
use sse_storage::transaction::{self, EditSummary};
use sse_xray::{save::InventoryItem, writer, Save};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

fn t(key: &str) -> &str {
    crate::strings::t(key)
}

fn tr_in(language: &str, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    crate::strings::tr_in(Some(language), key, args)
}

fn tr(key: &str, args: &[&dyn std::fmt::Display]) -> String {
    tr_in(crate::strings::current_language(), key, args)
}

fn format_xray_game_time(game_time: u64) -> String {
    const MILLIS_PER_DAY: u64 = 86_400_000;
    const MILLIS_PER_HOUR: u64 = 3_600_000;
    const MILLIS_PER_MINUTE: u64 = 60_000;

    let mut days_since_year_one = game_time / MILLIS_PER_DAY;
    if days_since_year_one >= 3_650_000 {
        return "—".to_owned();
    }

    let mut year = 1_u32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days_since_year_one < days_in_year {
            break;
        }
        days_since_year_one = days_since_year_one.saturating_sub(days_in_year);
        year = year.saturating_add(1);
    }
    if !(1990..=2100).contains(&year) {
        return "—".to_owned();
    }

    let month_lengths = [
        31_u64,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1_u32;
    for days_in_month in month_lengths {
        if days_since_year_one < days_in_month {
            break;
        }
        days_since_year_one = days_since_year_one.saturating_sub(days_in_month);
        month = month.saturating_add(1);
    }
    if month > 12 {
        return "—".to_owned();
    }

    let day = days_since_year_one.saturating_add(1);
    let milliseconds_today = game_time % MILLIS_PER_DAY;
    let hour = milliseconds_today / MILLIS_PER_HOUR;
    let minute = (milliseconds_today % MILLIS_PER_HOUR) / MILLIS_PER_MINUTE;
    format!("{day:02}.{month:02}.{year:04} {hour:02}:{minute:02}")
}

fn is_leap_year(year: u32) -> bool {
    year % 400 == 0 || year % 4 == 0 && year % 100 != 0
}

fn tr_named_in(language: &str, key: &str, args: &[(&str, &dyn std::fmt::Display)]) -> String {
    let mut translated = tr_in(language, key, &[]);
    for (name, value) in args {
        replace_named_placeholder(&mut translated, name, &value.to_string());
    }
    translated
}

fn replace_named_placeholder(text: &mut String, name: &str, value: &str) {
    let simple = format!("{{{name}}}");
    *text = text.replace(&simple, value);

    let formatted = format!("{{{name}:");
    while let Some(start) = text.find(&formatted) {
        let Some(end_offset) = text.get(start..).and_then(|tail| tail.find('}')) else {
            break;
        };
        let end = start.saturating_add(end_offset);
        text.replace_range(start..=end, value);
    }
}

macro_rules! tr {
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {{
        tr_named_in(
            crate::strings::current_language(),
            $key,
            &[$((stringify!($name), &$value as &dyn std::fmt::Display)),+],
        )
    }};
}

const INVENTORY_MAX_PAGE_SIZE: usize = 12;
const INVENTORY_ROW_HEIGHT: u32 = 44;
const INVENTORY_ROW_GAP: u32 = 4;

/// Style of one item row. The gap between rows is a bottom margin, not the list's gap: a hidden child still
/// reserves the list's gap, and the rows beyond the page are hidden.
fn inventory_row_style(shown: bool) -> Style {
    Style {
        min: crate::layout::Size::new(0.0, INVENTORY_ROW_HEIGHT as f32),
        preferred: crate::layout::Size::new(0.0, INVENTORY_ROW_HEIGHT as f32),
        shrink: 0.0,
        margin: crate::layout::Edges {
            bottom: if shown { INVENTORY_ROW_GAP as f32 } else { 0.0 },
            ..crate::layout::Edges::default()
        },
        align_items: crate::layout::Align::Stretch,
        ..Style::default()
    }
}
const INVENTORY_PLACE_WIDTH: f32 = 76.0;
const INVENTORY_CONDITION_WIDTH: f32 = 84.0;
const INVENTORY_COUNT_WIDTH: f32 = 76.0;
const INVENTORY_STEPPER_WIDTH: f32 = 64.0;
const ADD_ITEM_PAGE_SIZE: usize = 8;
const MAXIMUM_UPGRADE_ROWS: usize = 16;
const MAX_BROWSER_SAVE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_BROWSER_FILENAME_BYTES: usize = 240;
const S2_LEGACY_EDIT_REFUSAL: &str = "This save was written by game version 1.0.x. It can be read, but its layout is not supported for editing; load it in the current game and save again.";
const INVENTORY_CATEGORIES: [&str; 8] = [
    "ВСЕ",
    "ОРУЖИЕ",
    "БОЕПРИПАСЫ",
    "СНАРЯЖЕНИЕ",
    "РАСХОДНИКИ",
    "АРТЕФАКТЫ",
    "КЛЮЧИ",
    "ПРОЧЕЕ",
];

fn xray_inventory_category(category: &str, section: &str) -> &'static str {
    let category = category.to_lowercase();
    let section = section.to_ascii_lowercase();
    if section.contains("key_") || section.starts_with("quest_") || category.contains("ключ") {
        "КЛЮЧИ"
    } else if category.contains("оруж") {
        "ОРУЖИЕ"
    } else if category.contains("патрон") {
        "БОЕПРИПАСЫ"
    } else if category.contains("брон") || category.contains("экип") || category.contains("устрой") {
        "СНАРЯЖЕНИЕ"
    } else if category.contains("расход") || category.contains("гранат") {
        "РАСХОДНИКИ"
    } else if category.contains("артефакт") {
        "АРТЕФАКТЫ"
    } else {
        "ПРОЧЕЕ"
    }
}

fn s2_inventory_category(kind: u8, display_name: Option<&str>) -> &'static str {
    let name = display_name.unwrap_or_default().to_ascii_lowercase();
    if name.contains("key_") || name.starts_with("quest_") {
        "КЛЮЧИ"
    } else {
        match kind {
            0 => "ОРУЖИЕ",
            1 | 6 | 10 | 11 => "СНАРЯЖЕНИЕ",
            2 => "АРТЕФАКТЫ",
            4 | 7 => "РАСХОДНИКИ",
            5 => "БОЕПРИПАСЫ",
            _ => "ПРОЧЕЕ",
        }
    }
}

fn search_matches(text: &str, query: &str) -> bool {
    query.is_empty() || text.to_lowercase().contains(query)
}

fn xray_change_supported(save: &Save, kind: writer::ChangeKind) -> bool {
    writer::capability(save.format(), kind) != writer::Capability::Unsupported
}

fn paragraph(tree: &mut crate::widget::Tree, parent: WidgetId, text: &str, role: Text) -> Result<WidgetId> {
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style::default(),
        Content::Paragraph {
            text: crate::strings::t(text).to_owned(),
            style: role.style(),
        },
        Look {
            text: role.color(),
            ..Look::default()
        },
    )
}

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    screens_with_workspace(Workspace::default())
}

pub(crate) fn screens_with_workspace(workspace: Workspace) -> Vec<Box<dyn Screen>> {
    vec![
        Box::new(Overview::new(workspace.clone())),
        Box::new(Inventory::new(workspace.clone())),
        Box::new(Factions::new(workspace.clone())),
        Box::new(Stashes::new(workspace.clone())),
        Box::new(Transitions::new(workspace)),
    ]
}

/// S2 stash transfer stays disabled until a saved result is validated in-game.
pub(super) const S2_STASH_MOVE_ENABLED: bool = false;

/// Refreshes the save library and, after an external write, reloads the active save.
#[derive(Debug)]
pub(super) struct RefreshOverview {
    pub path: Option<PathBuf>,
}

struct StartupBackupCheck(std::result::Result<usize, String>);

#[derive(Clone)]
pub(crate) struct Workspace {
    state: Arc<Mutex<WorkspaceState>>,
    draft_directory: Arc<PathBuf>,
    backup_directory: Arc<Mutex<PathBuf>>,
    session: sse_app::SaveSession,
    draft_write_lock: Arc<Mutex<()>>,
    draft_writes_pending: Arc<AtomicUsize>,
    browser_file_bridge: Option<BrowserFileBridge>,
}

/// Counts one queued or running draft write; the count drops when the write task ends or never starts.
struct DraftWriteGuard(Arc<AtomicUsize>);

impl Drop for DraftWriteGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Default for Workspace {
    fn default() -> Self {
        Self::with_paths(
            default_draft_directory(),
            sse_app::paths::backup_directory(&sse_app::AppSettings::default()),
        )
    }
}

impl Workspace {
    #[cfg(test)]
    fn with_draft_directory(directory: PathBuf) -> Self {
        Self::with_paths(
            directory,
            sse_app::paths::backup_directory(&sse_app::AppSettings::default()),
        )
    }

    pub(crate) fn with_backup_directory(directory: PathBuf) -> Self {
        Self::with_paths(default_draft_directory(), directory)
    }

    fn begin_draft_write(&self) -> DraftWriteGuard {
        self.draft_writes_pending.fetch_add(1, Ordering::SeqCst);
        DraftWriteGuard(Arc::clone(&self.draft_writes_pending))
    }

    fn draft_writes_pending(&self) -> bool {
        self.draft_writes_pending.load(Ordering::SeqCst) > 0
    }

    fn with_paths(draft_directory: PathBuf, backup_directory: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(WorkspaceState::default())),
            draft_directory: Arc::new(draft_directory),
            backup_directory: Arc::new(Mutex::new(backup_directory)),
            session: sse_app::SaveSession::new(),
            draft_write_lock: Arc::new(Mutex::new(())),
            draft_writes_pending: Arc::new(AtomicUsize::new(0)),
            browser_file_bridge: None,
        }
    }

    pub(crate) fn with_browser_file_bridge(backup_directory: PathBuf, bridge: BrowserFileBridge) -> Self {
        let mut workspace = Self::with_paths(default_draft_directory(), backup_directory);
        workspace.browser_file_bridge = Some(bridge);
        workspace
    }

    pub(crate) fn is_browser_file_mode(&self) -> bool {
        self.browser_file_bridge.is_some()
    }

    pub(crate) fn request_browser_file_open(&self) -> bool {
        let Some(bridge) = self.browser_file_bridge.as_ref() else {
            return false;
        };
        bridge.request_open_file();
        true
    }

    fn queue_browser_download(&self, download: BrowserDownload) -> Result<()> {
        let Some(bridge) = self.browser_file_bridge.as_ref() else {
            return Err(Error::Refused("browser file transfer is unavailable".to_owned()));
        };
        bridge.queue_download(download)
    }

    pub(crate) fn backup_directory(&self) -> PathBuf {
        self.backup_directory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn set_backup_directory(&self, directory: PathBuf) {
        *self
            .backup_directory
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = directory;
    }

    pub(crate) fn session(&self) -> sse_app::SaveSession {
        self.session.clone()
    }

    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn is_saving(&self) -> bool {
        self.session.is_saving()
    }

    pub(crate) fn is_restoring(&self) -> bool {
        self.session.is_restoring()
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.lock().loading
    }

    fn finish_load_request(&self, request: u64) {
        let mut state = self.lock();
        if state.load_request == request {
            state.loading = false;
        }
    }

    pub(crate) fn load_request(&self) -> u64 {
        self.lock().load_request
    }

    pub(crate) fn library_snapshot(&self) -> (bool, Option<String>, Vec<SaveSlot>) {
        let state = self.lock();
        let query = state.search_query.to_lowercase();
        (
            state.scanning,
            state.load_error.clone(),
            library_slots(&state)
                .into_iter()
                .filter(|slot| slot.detection_error.is_none())
                .filter(|slot| {
                    slot.path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_lowercase().contains(&query))
                        .unwrap_or(query.is_empty())
                })
                .collect(),
        )
    }

    /// The file-name filter shared by the library panel and the save list.
    pub(crate) fn search_query(&self) -> String {
        self.lock().search_query.clone()
    }

    /// How many directories the last discovery searched; shown in the library status.
    pub(crate) fn searched_path_count(&self) -> usize {
        self.lock()
            .discovery
            .as_ref()
            .map_or(0, |result| result.searched_paths.len())
    }

    pub(crate) fn set_search_query(&self, query: &str) {
        query.clone_into(&mut self.lock().search_query);
    }

    pub(crate) fn refresh_library(&self, cx: &mut Context<'_>) {
        start_discovery(self, cx);
    }

    pub(crate) fn select_library_path(&self, path: &Path, cx: &mut Context<'_>) {
        let slot = library_slots(&self.lock())
            .into_iter()
            .find(|slot| same_file_path(&slot.path, path));
        if let Some(slot) = slot {
            start_load(self, slot, cx);
        }
    }

    pub(crate) fn spawn<F>(&self, name: &'static str, work: F) -> Result<()>
    where
        F: FnOnce(sse_app::tasks::TaskContext) + Send + 'static,
    {
        self.lock()
            .tasks
            .spawn(name, move |context| {
                work(context);
                Ok(())
            })
            .map(|_| ())
            .map_err(|error| Error::System(format!("failed to start {name} task: {error}")))
    }

    pub(crate) fn poll_tasks(&self) {
        let _ = self.lock().tasks.poll_events();
    }

    fn persist_draft(&self, journal: DraftJournal, cx: &mut Context<'_>) {
        self.persist_drafts(vec![journal], cx);
    }

    fn reset_draft(&self, journal: DraftJournal, preserve_unmapped: bool, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some(t("Черновик сброшен в памяти; фоновый канал недоступен.").to_owned());
            return;
        };
        let Some(source_sha256) = journal.current().map(|plan| plan.source_sha256.clone()) else {
            return;
        };
        let Some(selected) = self
            .lock()
            .selected
            .as_ref()
            .filter(|selected| selected.source_sha256 == source_sha256)
            .cloned()
        else {
            cx.status = Some(t("Не удалось определить путь сейва для черновика.").to_owned());
            return;
        };
        let store = DraftStore::for_source(self.draft_directory.as_path(), &selected.slot.path);
        let identity = match store.identity_key(&source_sha256) {
            Ok(identity) => identity,
            Err(error) => {
                cx.status = Some(tr("Не удалось определить черновик: {0}", &[&error]));
                return;
            }
        };
        let Some(generation) = self.session.next_draft_generation(&identity) else {
            cx.status = Some(t("Не удалось назначить поколение черновика.").to_owned());
            return;
        };
        let session = self.session.clone();
        let write_lock = Arc::clone(&self.draft_write_lock);
        let draft_directory = Arc::clone(&self.draft_directory);
        let write = self.begin_draft_write();
        if let Err(error) = self.spawn("draft-reset", move |context| {
            let _write = write;
            if context.is_cancelled() {
                return;
            }
            let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if !session.is_current_draft_generation(&identity, generation) {
                return;
            }
            let store = DraftStore::for_source(draft_directory.as_path(), &selected.slot.path);
            let result = (|| {
                if preserve_unmapped {
                    store.set_aside(&source_sha256)?;
                }
                store.save(journal).map(|_| ())
            })()
            .map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(DraftPersisted(result)),
            ));
        }) {
            cx.status = Some(tr("Не удалось запустить сохранение черновика: {0}", &[&error]));
        }
    }

    fn persist_drafts(&self, journals: Vec<DraftJournal>, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some(t("Черновик изменён только в памяти: фоновый канал недоступен.").to_owned());
            return;
        };
        let Some(selected) = self.lock().selected.clone() else {
            cx.status = Some(t("Не удалось определить путь сейва для черновика.").to_owned());
            return;
        };
        let store = DraftStore::for_source(self.draft_directory.as_path(), &selected.slot.path);
        let hashes: Result<Vec<String>> = journals
            .iter()
            .filter_map(|journal| journal.current().map(|plan| plan.source_sha256.as_str()))
            .map(|source_sha256| store.identity_key(source_sha256))
            .collect();
        let hashes = match hashes {
            Ok(hashes) => hashes,
            Err(error) => {
                cx.status = Some(tr("Не удалось определить черновик: {0}", &[&error]));
                return;
            }
        };
        let Some(generation) = self
            .session
            .next_draft_generation_for(hashes.iter().map(String::as_str))
        else {
            cx.status = Some(t("Не удалось назначить поколение черновика.").to_owned());
            return;
        };
        let session = self.session.clone();
        let write_lock = Arc::clone(&self.draft_write_lock);
        let draft_directory = Arc::clone(&self.draft_directory);
        let write = self.begin_draft_write();
        if let Err(error) = self.spawn("draft-save", move |context| {
            let _write = write;
            if context.is_cancelled() {
                return;
            }
            let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if hashes
                .iter()
                .any(|hash| !session.is_current_draft_generation(hash, generation))
            {
                return;
            }
            let store = DraftStore::for_source(draft_directory.as_path(), &selected.slot.path);
            let result = journals
                .into_iter()
                .try_for_each(|journal| store.save(journal).map(|_| ()))
                .map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(DraftPersisted(result)),
            ));
        }) {
            cx.status = Some(tr("Не удалось запустить сохранение черновика: {0}", &[&error]));
        }
    }
}

#[derive(Default)]
struct WorkspaceState {
    tasks: sse_app::TaskManager,
    search_query: String,
    scanning: bool,
    discovery: Option<sse_storage::discovery::SaveDiscoveryResult>,
    manually_opened: Vec<SaveSlot>,
    loading: bool,
    load_request: u64,
    load_error: Option<String>,
    selected: Option<Arc<LoadedSave>>,
    pending_money: Option<u32>,
    pending_stacks: BTreeMap<ItemHandle, u32>,
    pending_durability: BTreeMap<ItemHandle, u8>,
    pending_placements: BTreeMap<ItemHandle, DraftPlacement>,
    pending_upgrades: BTreeMap<ItemHandle, Vec<String>>,
    pending_removed: BTreeSet<ItemHandle>,
    pending_adds: Vec<AddRequest>,
    pending_stash_moves: BTreeSet<u32>,
    pending_xray_stash_takes: BTreeSet<u16>,
    pending_xray_stash_puts: BTreeMap<u16, u16>,
    pending_faction_relations: BTreeMap<String, i32>,
    pending_relocation: Option<u16>,
    external_change: bool,
    last_file_check: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ItemHandle {
    Xray(u16),
    Stalker2(u32),
}

impl ItemHandle {
    const fn as_u32(self) -> u32 {
        match self {
            Self::Xray(handle) => handle as u32,
            Self::Stalker2(handle) => handle,
        }
    }
}

#[derive(Clone, Default)]
struct PendingInventoryEdits {
    money: Option<u32>,
    stacks: BTreeMap<ItemHandle, u32>,
    durability: BTreeMap<ItemHandle, u8>,
    placements: BTreeMap<ItemHandle, DraftPlacement>,
    upgrades: BTreeMap<ItemHandle, Vec<String>>,
    removals: BTreeSet<ItemHandle>,
    adds: Vec<AddRequest>,
    stash_takes: BTreeSet<u16>,
    stash_puts: BTreeMap<u16, u16>,
    faction_relations: BTreeMap<String, i32>,
    relocate_to: Option<u16>,
}

impl PendingInventoryEdits {
    fn has_changes(&self) -> bool {
        self.money.is_some()
            || !self.stacks.is_empty()
            || !self.durability.is_empty()
            || !self.placements.is_empty()
            || !self.upgrades.is_empty()
            || !self.removals.is_empty()
            || !self.adds.is_empty()
            || !self.stash_takes.is_empty()
            || !self.stash_puts.is_empty()
            || !self.faction_relations.is_empty()
            || self.relocate_to.is_some()
    }
}

struct LoadedSave {
    slot: SaveSlot,
    source_sha256: String,
    info: String,
    parameters: String,
    integrity: String,
    data: SaveData,
}

struct PendingSaveRequest {
    selected: Arc<LoadedSave>,
    edits: PendingInventoryEdits,
    stash_moves: BTreeSet<u32>,
}

struct SaveProcessCheckFinished {
    request_id: u64,
    result: std::result::Result<bool, String>,
}

fn process_check_prompt(result: &std::result::Result<bool, String>) -> Option<(String, &'static str)> {
    match result {
        Ok(false) => None,
        Ok(true) => Some((t(SAVE_WHILE_GAME_RUNNING_WARNING).to_owned(), t("Всё равно сохранить"))),
        Err(error) => Some((
            tr(
                "Не удалось проверить запущенную игру: {0}. Сохранение не проверено.",
                &[&error],
            ),
            t("Сохранить всё равно"),
        )),
    }
}

enum SaveData {
    Xray {
        save: Save,
        inventory: Vec<InventoryItem>,
    },
    Stalker2 {
        save: Box<S2Save>,
        inventory: Vec<S2InventoryItem>,
        stash_items: Option<std::result::Result<Vec<S2StashItem>, String>>,
    },
}

impl LoadedSave {
    fn read(slot: SaveSlot) -> Result<Self> {
        let packed = SaveBuffer::read(&slot.path)?;
        Self::from_buffer(slot, packed)
    }

    #[cfg(test)]
    fn from_bytes(slot: SaveSlot, bytes: &[u8]) -> Result<Self> {
        Self::from_buffer(slot, SaveBuffer::from_vec(bytes.to_vec()))
    }

    fn from_buffer(slot: SaveSlot, packed: SaveBuffer) -> Result<Self> {
        let source_sha256 = sse_codecs::sha256::sha256_hex(packed.as_slice());
        let parsed = match S2Save::from_bytes(packed.as_slice()) {
            Ok(save) => {
                let inventory = save.items();
                let stash = save.stash().ok();
                Ok(Self::from_s2(slot, packed, source_sha256, save, inventory, stash))
            }
            Err(s2_error) => match Save::read(packed.as_slice()) {
                Ok(save) => {
                    let inventory = save.inventory()?;
                    Self::from_xray(slot, packed, source_sha256, save, inventory)
                }
                Err(xray_error) => Err(Error::damaged(format!(
                    "unsupported or damaged save (S2: {s2_error}; X-Ray: {xray_error})"
                ))),
            },
        }?;
        Ok(parsed)
    }

    fn from_xray(
        mut slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: Save,
        inventory: Vec<InventoryItem>,
    ) -> Result<Self> {
        let money = save.money()?;
        let format = save.format().id();
        let game = match save.format() {
            sse_xray::Format::Soc | sse_xray::Format::SocEe => "soc",
            sse_xray::Format::Cs | sse_xray::Format::CsEe => "clear_sky",
            sse_xray::Format::Cop | sse_xray::Format::CopEe => "cop",
        };
        slot.game_id = Some(game.to_owned());
        slot.candidate_game_id = game.to_owned();
        slot.candidate_release_id = format.to_owned();
        slot.format_id = Some(format.to_owned());
        let info = save_info(&slot);
        let item_count = inventory.len();
        let game_time = format_xray_game_time(save.game_time());
        let parameters = tr(
            "Деньги: {0} RU · Предметов: {1} · Тайников: —\nИгровое время: {2} · Персонаж: — · Здоровье: —\nРанг: — · Репутация: — · Задания: — · Убито: — · Погода: —",
            &[&money, &item_count, &game_time],
        );
        let integrity = save_integrity(
            &slot,
            &source_sha256,
            packed.len(),
            t("не подтверждается отдельным полем"),
            format,
        );
        Ok(Self {
            slot,
            source_sha256,
            info,
            parameters,
            integrity,
            data: SaveData::Xray { save, inventory },
        })
    }

    fn from_s2(
        mut slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: S2Save,
        inventory: Vec<S2InventoryItem>,
        stash: Option<S2StashLayout>,
    ) -> Self {
        slot.game_id = Some("stalker2".to_owned());
        slot.candidate_game_id = "stalker2".to_owned();
        slot.candidate_release_id = "stalker2".to_owned();
        slot.format_id = Some("stalker2".to_owned());
        let info = save_info(&slot);
        let money = save.money();
        let item_count = inventory.len();
        let stash_count = stash
            .as_ref()
            .map_or_else(|| "—".to_owned(), |items| items.live_handles().len().to_string());
        let unresolved_count = save.unresolved_handles().len();
        let parameters = tr(
            "Деньги: {0} RU · Предметов: {1} · Тайников: {2}\nИгровое время · Персонаж · Здоровье · Ранг · Репутация · Задания · Убито · Погода: —\nНеопознанных ссылок: {3}",
            &[&money, &item_count, &stash_count, &unresolved_count],
        );
        let integrity = save_integrity(
            &slot,
            &source_sha256,
            packed.len(),
            if save.container().stored_crc32() == save.container().computed_crc32() {
                "OK (CRC32)"
            } else {
                t("ошибка")
            },
            "S2",
        );
        let stash_items = stash
            .as_ref()
            .map(|_| save.stash_items().map_err(|error| error.to_string()));
        Self {
            slot,
            source_sha256,
            info,
            parameters,
            integrity,
            data: SaveData::Stalker2 {
                save: Box::new(save),
                inventory,
                stash_items,
            },
        }
    }
}

fn save_info(slot: &SaveSlot) -> String {
    let game = slot.format_id.as_deref().unwrap_or(&slot.candidate_release_id);
    let filename = slot
        .path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| t("без имени").into());
    let path = slot.path.display().to_string();
    tr(
        "Игра: {0}\nИмя файла: {1}\nПуть: {2}",
        &[&format_display_name(game), &filename, &path],
    )
}

fn save_integrity(slot: &SaveSlot, source_sha256: &str, bytes_read: usize, crc_status: &str, format: &str) -> String {
    let modified = display_file_time(slot.last_write_time_utc, false, true);
    tr(
        "Размер файла: {0} байт · Изменён: {1} UTC\nSHA-256: {2}\nCRC: {3} · Формат: {4} · Сборка игры: —",
        &[&bytes_read, &modified, &source_sha256, &crc_status, &format],
    )
}

pub(super) fn format_display_name(format: &str) -> &'static str {
    match format {
        "stalker-soc-ee" => t("Тень Чернобыля (Enhanced Edition)"),
        "stalker-soc" | "soc" => t("Тень Чернобыля"),
        "stalker-cs-ee" => t("Чистое Небо (Enhanced Edition)"),
        "stalker-cs" | "clear_sky" => t("Чистое Небо"),
        "stalker-cop-ee" => t("Зов Припяти (Enhanced Edition)"),
        "stalker-cop" | "cop" => t("Зов Припяти"),
        "stalker2" | "s2" => t("S.T.A.L.K.E.R. 2: Сердце Чернобыля"),
        _ => "S.T.A.L.K.E.R.",
    }
}

pub(super) fn display_file_time(time: std::time::SystemTime, short_year: bool, seconds: bool) -> String {
    let unix_seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let days = i64::try_from(unix_seconds / 86_400).unwrap_or(1_000_000_000);
    let shifted = days.saturating_add(719_468);
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era = day_of_era
        .saturating_sub(day_of_era / 1_460)
        .saturating_add(day_of_era / 36_524)
        .saturating_sub(day_of_era / 146_096)
        / 365;
    let year = year_of_era.saturating_add(era.saturating_mul(400));
    let day_of_year = day_of_era.saturating_sub(
        365_i64
            .saturating_mul(year_of_era)
            .saturating_add(year_of_era / 4)
            .saturating_sub(year_of_era / 100),
    );
    let month_prime = 5_i64.saturating_mul(day_of_year).saturating_add(2) / 153;
    let day = day_of_year
        .saturating_sub(153_i64.saturating_mul(month_prime).saturating_add(2) / 5)
        .saturating_add(1);
    let month = if month_prime < 10 {
        month_prime.saturating_add(3)
    } else {
        month_prime.saturating_sub(9)
    };
    let year = year.saturating_add(i64::from(month <= 2));
    let day_seconds = unix_seconds % 86_400;
    let hour = day_seconds / 3_600;
    let minute = day_seconds % 3_600 / 60;
    let second = day_seconds % 60;
    let shown_year = if short_year { year.rem_euclid(100) } else { year };
    if seconds {
        format!("{day:02}.{month:02}.{shown_year:04} {hour:02}:{minute:02}:{second:02}")
    } else {
        format!("{day:02}.{month:02}.{shown_year:02} {hour:02}:{minute:02}")
    }
}

pub(super) fn display_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.0} KB", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} B")
    }
}

pub(super) fn start_discovery(workspace: &Workspace, cx: &mut Context<'_>) {
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some(t("Поиск сейвов начнётся в работающем окне редактора.").to_owned());
        return;
    };
    {
        let mut state = workspace.lock();
        if state.scanning {
            return;
        }
        state.scanning = true;
    }
    let shared = workspace.clone();
    if let Err(error) = workspace.spawn("save-discovery", move |context| {
        if context.is_cancelled() {
            return;
        }
        let discovery_options = super::save_directory_discovery_options();
        let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&discovery_options));
        let mut result = SaveSlotDiscovery::discover(&candidates);
        result.slots.sort_by(|left, right| {
            save_game_key(left)
                .cmp(save_game_key(right))
                .then_with(|| right.last_write_time_utc.cmp(&left.last_write_time_utc))
        });
        let mut state = shared.lock();
        state.discovery = Some(result);
        state.scanning = false;
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
    }) {
        workspace.lock().scanning = false;
        cx.status = Some(tr("Не удалось запустить поиск сейвов: {0}", &[&error]));
    } else {
        cx.status = Some(t("Ищу сейвы в обнаруженных каталогах…").to_owned());
    }
}

fn start_load(workspace: &Workspace, slot: SaveSlot, cx: &mut Context<'_>) {
    let requested_path = slot.path.clone();
    start_load_from(workspace, move || Ok(slot), false, requested_path, cx);
}

fn start_load_path(workspace: &Workspace, path: &Path, cx: &mut Context<'_>) {
    let requested_path = path.to_path_buf();
    let path = requested_path.clone();
    start_load_from(workspace, move || slot_for_path(&path), true, requested_path, cx);
}

fn start_load_from<F>(
    workspace: &Workspace,
    slot: F,
    include_discovery: bool,
    requested_path: PathBuf,
    cx: &mut Context<'_>,
) where
    F: FnOnce() -> Result<SaveSlot> + Send + 'static,
{
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some(t("Загрузка сейва доступна в работающем окне редактора.").to_owned());
        return;
    };
    if workspace.session.is_busy() {
        let text = if workspace.session.is_restoring() {
            t("Нельзя сменить сейв, пока выполняется восстановление.")
        } else {
            t("Нельзя сменить сейв, пока выполняется запись.")
        };
        cx.status = Some(text.to_owned());
        return;
    }
    if workspace.draft_writes_pending() {
        cx.status = Some(t("Дождитесь записи черновика, затем смените сейв.").to_owned());
        return;
    }
    let request = {
        let mut state = workspace.lock();
        state.load_request = state.load_request.saturating_add(1);
        state.loading = true;
        state.load_error = None;
        state.selected = None;
        state.pending_money = None;
        state.pending_stacks.clear();
        state.pending_durability.clear();
        state.pending_placements.clear();
        state.pending_upgrades.clear();
        state.pending_removed.clear();
        state.pending_adds.clear();
        state.pending_stash_moves.clear();
        state.pending_xray_stash_takes.clear();
        state.pending_xray_stash_puts.clear();
        state.pending_faction_relations.clear();
        state.pending_relocation = None;
        state.external_change = false;
        state.last_file_check = 0;
        state.load_request
    };
    cx.app.set_current_save(None);
    let shared = workspace.clone();
    let draft_directory = Arc::clone(&workspace.draft_directory);
    if let Err(error) = workspace.spawn("save-load", move |context| {
        if context.is_cancelled() {
            return;
        }
        let result = slot().and_then(LoadedSave::read).and_then(|save| {
            let journal = load_draft_journal(draft_directory.as_path(), &save.slot.path, &save.source_sha256)?;
            Ok((save, journal))
        });
        let io_error = matches!(&result, Err(Error::System(_)));
        let mut state = shared.lock();
        let is_current_request = state.load_request == request;
        let completion = match result {
            Ok((save, journal)) => {
                if include_discovery {
                    upsert_slot(&mut state.manually_opened, save.slot.clone());
                }
                let path = save.slot.path.clone();
                if is_current_request {
                    state.load_error = None;
                    state.selected = Some(Arc::new(save));
                }
                LoadFinished {
                    request,
                    selected_path: Some(path),
                    requested_path: requested_path.clone(),
                    journal: Some(journal),
                    error: None,
                    io_error: false,
                }
            }
            Err(error) => {
                let error = error.to_string();
                if is_current_request {
                    state.selected = None;
                    state.load_error = Some(error.clone());
                }
                LoadFinished {
                    request,
                    selected_path: None,
                    requested_path: requested_path.clone(),
                    journal: None,
                    error: Some(error),
                    io_error,
                }
            }
        };
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(completion)));
    }) {
        let mut state = workspace.lock();
        if state.load_request == request {
            state.loading = false;
            state.load_error = Some(error.to_string());
        }
        cx.status = Some(tr("Не удалось запустить чтение сейва: {0}", &[&error]));
    } else {
        cx.status = Some(t("Загружаю и проверяю выбранный сейв…").to_owned());
    }
}

fn schedule_file_check(workspace: &Workspace, cx: &mut Context<'_>, seconds: u64) {
    let Some(proxy) = cx.proxy.cloned() else {
        return;
    };
    let (path, expected_size, expected_modified, source_sha256) = {
        let mut state = workspace.lock();
        if seconds.saturating_sub(state.last_file_check) < 3 {
            return;
        }
        let Some(selected) = state.selected.as_ref() else {
            return;
        };
        let snapshot = (
            selected.slot.path.clone(),
            selected.slot.size,
            selected.slot.last_write_time_utc,
            selected.source_sha256.clone(),
        );
        state.last_file_check = seconds;
        (snapshot.0, snapshot.1, snapshot.2, snapshot.3)
    };
    let Some(mut file_check) = workspace.session.begin_file_check(&path) else {
        return;
    };
    let shared = workspace.clone();
    if let Err(error) = workspace.spawn("save-file-monitor", move |context| {
        if context.is_cancelled() {
            return;
        }
        let changed = std::fs::metadata(&path).map_or(true, |metadata| {
            metadata.len() != expected_size || metadata.modified().is_ok_and(|modified| modified != expected_modified)
        });
        let still_selected = {
            let state = shared.lock();
            state
                .selected
                .as_ref()
                .is_some_and(|selected| selected.slot.path == path && selected.source_sha256 == source_sha256)
        };
        let current_check = file_check.finish();
        if still_selected && current_check {
            shared.lock().external_change = changed;
            let finished = FileCheckFinished {
                path,
                source_sha256,
                changed,
            };
            let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(finished.clone())));
            let _ = proxy.send(AppMessage::ToScreen(ScreenId::Inventory, Box::new(finished)));
        }
    }) {
        cx.status = Some(tr("Не удалось запустить проверку файла: {0}", &[&error]));
    }
}

fn start_reload_selected(workspace: &Workspace, cx: &mut Context<'_>) -> Result<()> {
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some(t("Повторное чтение доступно в работающем окне редактора.").to_owned());
        return Ok(());
    };
    if workspace.session.is_busy() {
        let text = if workspace.session.is_restoring() {
            t("Нельзя перечитать сейв, пока выполняется восстановление.")
        } else {
            t("Нельзя перечитать сейв, пока выполняется запись.")
        };
        cx.status = Some(text.to_owned());
        return Ok(());
    }
    let Some(selected) = workspace.lock().selected.clone() else {
        return Ok(());
    };
    let path = selected.slot.path.clone();
    let old_sha256 = selected.source_sha256.clone();
    let draft_store = DraftStore::for_source(workspace.draft_directory.as_path(), &path);
    let draft_identity = draft_store.identity_key(&old_sha256)?;
    let Some(generation) = workspace.session.next_draft_generation(&draft_identity) else {
        cx.status = Some(t("Не удалось назначить поколение черновика.").to_owned());
        return Ok(());
    };
    let empty_journal = DraftJournal::new(vec![DraftPlan::empty(&old_sha256)?], 0)?;
    let request = {
        let mut state = workspace.lock();
        if state
            .selected
            .as_ref()
            .is_none_or(|current| current.slot.path != path || current.source_sha256 != old_sha256)
        {
            return Ok(());
        }
        state.load_request = state.load_request.saturating_add(1);
        state.loading = true;
        state.load_error = None;
        state.selected = None;
        state.pending_money = None;
        state.pending_stacks.clear();
        state.pending_durability.clear();
        state.pending_placements.clear();
        state.pending_upgrades.clear();
        state.pending_removed.clear();
        state.pending_adds.clear();
        state.pending_stash_moves.clear();
        state.pending_xray_stash_takes.clear();
        state.pending_xray_stash_puts.clear();
        state.pending_faction_relations.clear();
        state.pending_relocation = None;
        state.external_change = false;
        state.last_file_check = 0;
        state.load_request
    };
    cx.app.set_current_save(None);
    cx.app.discard_draft(&old_sha256);
    let write_lock = Arc::clone(&workspace.draft_write_lock);
    let session = workspace.session.clone();
    let directory = Arc::clone(&workspace.draft_directory);
    let shared = workspace.clone();
    let requested_path = path.clone();
    if let Err(error) = workspace.spawn("save-reload", move |context| {
        if context.is_cancelled() {
            return;
        }
        let result: Result<(LoadedSave, DraftJournal)> = (|| {
            {
                let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if session.is_current_draft_generation(&draft_identity, generation) {
                    DraftStore::for_source(directory.as_path(), &path).save(empty_journal)?;
                }
            }
            let loaded = LoadedSave::read(slot_for_path(&path)?)?;
            let journal = load_draft_journal(directory.as_path(), &loaded.slot.path, &loaded.source_sha256)?;
            Ok((loaded, journal))
        })();
        let io_error = matches!(&result, Err(Error::System(_)));
        let mut state = shared.lock();
        if state.load_request != request {
            return;
        }
        let completion = match result {
            Ok((loaded, journal)) => {
                let path = loaded.slot.path.clone();
                state.selected = Some(Arc::new(loaded));
                state.load_error = None;
                LoadFinished {
                    request,
                    selected_path: Some(path),
                    requested_path: requested_path.clone(),
                    journal: Some(journal),
                    error: None,
                    io_error: false,
                }
            }
            Err(error) => {
                let error = error.to_string();
                state.selected = None;
                state.load_error = Some(error.clone());
                LoadFinished {
                    request,
                    selected_path: None,
                    requested_path: requested_path.clone(),
                    journal: None,
                    error: Some(error),
                    io_error,
                }
            }
        };
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(completion)));
    }) {
        let mut state = workspace.lock();
        if state.load_request == request {
            state.loading = false;
            state.load_error = Some(error.to_string());
        }
        cx.status = Some(tr("Не удалось запустить повторное чтение: {0}", &[&error]));
    } else {
        cx.status = Some(t("Сбрасываю черновик и перечитываю сейв с диска…").to_owned());
    }
    Ok(())
}

pub(super) struct LoadFinished {
    pub(super) request: u64,
    pub(super) selected_path: Option<PathBuf>,
    pub(super) requested_path: PathBuf,
    pub(super) journal: Option<DraftJournal>,
    pub(super) error: Option<String>,
    pub(super) io_error: bool,
}

#[derive(Clone)]
struct FileCheckFinished {
    path: PathBuf,
    source_sha256: String,
    changed: bool,
}

struct DraftPersisted(std::result::Result<(), String>);

fn default_draft_directory() -> PathBuf {
    if let Some(path) = std::env::var_os("SSE_DRAFT_DIR") {
        return PathBuf::from(path);
    }
    sse_app::default_settings_path()
        .parent()
        .map(|directory| directory.join("drafts"))
        .unwrap_or_else(|| std::env::temp_dir().join("StalkerSaveEditorData/drafts"))
}

fn load_draft_journal(directory: &Path, source_path: &Path, source_sha256: &str) -> Result<DraftJournal> {
    if let Some(journal) = DraftStore::for_source(directory, source_path).load(source_sha256)? {
        return Ok(journal);
    }
    DraftJournal::new(vec![DraftPlan::empty(source_sha256)?], 0)
}

fn set_workspace_draft(workspace: &Workspace, journal: &DraftJournal) {
    let mut state = workspace.lock();
    state.pending_money = journal.current().and_then(|plan| plan.money);
    let pending_stacks: BTreeMap<ItemHandle, u32> = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Xray { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.stack_counts
                            .get(&u32::from(item.handle))
                            .map(|count| (ItemHandle::Xray(item.handle), *count))
                    })
                    .collect(),
                SaveData::Stalker2 { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.stack_counts
                            .get(&item.handle)
                            .map(|count| (ItemHandle::Stalker2(item.handle), *count))
                    })
                    .collect(),
            })
        })
        .unwrap_or_default();
    let pending_durability: BTreeMap<ItemHandle, u8> = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Xray { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.durability
                            .get(&u32::from(item.handle))
                            .map(|condition| (ItemHandle::Xray(item.handle), *condition))
                    })
                    .collect(),
                SaveData::Stalker2 { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.durability
                            .get(&item.handle)
                            .map(|condition| (ItemHandle::Stalker2(item.handle), *condition))
                    })
                    .collect(),
            })
        })
        .unwrap_or_default();
    let pending_placements: BTreeMap<ItemHandle, DraftPlacement> = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Xray { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.placements
                            .get(&u32::from(item.handle))
                            .map(|placement| (ItemHandle::Xray(item.handle), *placement))
                    })
                    .collect(),
                SaveData::Stalker2 { .. } => BTreeMap::new(),
            })
        })
        .unwrap_or_default();
    let pending_upgrades: BTreeMap<ItemHandle, Vec<String>> = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Xray { inventory, .. } => inventory
                    .iter()
                    .filter_map(|item| {
                        plan.upgrades
                            .get(&u32::from(item.handle))
                            .map(|upgrades| (ItemHandle::Xray(item.handle), upgrades.clone()))
                    })
                    .collect(),
                SaveData::Stalker2 { .. } => BTreeMap::new(),
            })
        })
        .unwrap_or_default();
    let pending_removed: BTreeSet<ItemHandle> = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Xray { inventory, .. } => plan
                    .detach_handles
                    .iter()
                    .filter(|handle| inventory.iter().any(|item| item.handle == **handle))
                    .map(|handle| ItemHandle::Xray(*handle))
                    .collect(),
                SaveData::Stalker2 { .. } => BTreeSet::new(),
            })
        })
        .unwrap_or_default();
    let pending_adds = journal.current().map_or_else(Vec::new, |plan| plan.adds.clone());
    let pending_stash_moves = journal
        .current()
        .and_then(|plan| {
            state.selected.as_ref().map(|selected| match &selected.data {
                SaveData::Stalker2 { stash_items, .. } => {
                    let Some(items) = stash_items.as_ref().and_then(|items| items.as_ref().ok()) else {
                        return BTreeSet::new();
                    };
                    plan.s2_stash_takes
                        .iter()
                        .filter(|handle| items.iter().any(|item| item.handle == **handle))
                        .copied()
                        .collect()
                }
                SaveData::Xray { .. } => BTreeSet::new(),
            })
        })
        .unwrap_or_default();
    state.pending_stacks = pending_stacks;
    state.pending_durability = pending_durability;
    state.pending_placements = pending_placements;
    state.pending_upgrades = pending_upgrades;
    state.pending_removed = pending_removed;
    state.pending_adds = pending_adds;
    state.pending_stash_moves = pending_stash_moves;
    let (pending_xray_stash_takes, pending_xray_stash_puts) = journal.current().map_or_else(
        || (BTreeSet::new(), BTreeMap::new()),
        |plan| match state.selected.as_ref().map(|selected| &selected.data) {
            Some(SaveData::Xray { .. }) => (
                plan.stash_takes.iter().copied().collect(),
                plan.stash_puts
                    .iter()
                    .map(|transfer| (transfer.object_id, transfer.box_id))
                    .collect(),
            ),
            _ => (BTreeSet::new(), BTreeMap::new()),
        },
    );
    state.pending_xray_stash_takes = pending_xray_stash_takes;
    state.pending_xray_stash_puts = pending_xray_stash_puts;
    state.pending_faction_relations = journal
        .current()
        .map_or_else(BTreeMap::new, |plan| plan.faction_relations.clone());
    state.pending_relocation = journal.current().and_then(|plan| plan.relocate_to);
}

fn save_game_key(slot: &SaveSlot) -> &str {
    slot.game_id.as_deref().unwrap_or(slot.candidate_game_id.as_str())
}

fn library_slots(state: &WorkspaceState) -> Vec<SaveSlot> {
    let mut slots = state
        .discovery
        .as_ref()
        .map_or_else(Vec::new, |discovery| discovery.slots.clone());
    for slot in &state.manually_opened {
        upsert_slot(&mut slots, slot.clone());
    }
    slots.sort_by(|left, right| {
        save_game_key(left)
            .cmp(save_game_key(right))
            .then_with(|| right.last_write_time_utc.cmp(&left.last_write_time_utc))
    });
    slots
}

fn upsert_slot(slots: &mut Vec<SaveSlot>, slot: SaveSlot) {
    if let Some(existing) = slots
        .iter_mut()
        .find(|existing| same_file_path(&existing.path, &slot.path))
    {
        *existing = slot;
    } else {
        slots.push(slot);
    }
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        windows_path_units_equal(left.as_os_str().encode_wide(), right.as_os_str().encode_wide())
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

#[cfg(any(windows, test))]
fn windows_path_units_equal(left: impl Iterator<Item = u16>, right: impl Iterator<Item = u16>) -> bool {
    fn fold_ascii(unit: u16) -> u16 {
        if (b'A' as u16..=b'Z' as u16).contains(&unit) {
            unit.saturating_add(32)
        } else {
            unit
        }
    }

    let (mut left, mut right) = (left, right);
    loop {
        match (left.next(), right.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) if fold_ascii(left) == fold_ascii(right) => {}
            _ => return false,
        }
    }
}

fn slot_for_path(path: &Path) -> Result<SaveSlot> {
    const MAX_SAVE_BYTES: u64 = 512 * 1024 * 1024;
    let path = std::fs::canonicalize(path)?;
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() == 0 || metadata.len() > MAX_SAVE_BYTES {
        return Err(Error::Refused(
            "save file size is outside the supported range".to_owned(),
        ));
    }
    Ok(SaveSlot {
        path,
        candidate_game_id: "unknown".to_owned(),
        candidate_release_id: "unknown".to_owned(),
        size: metadata.len(),
        last_write_time_utc: metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
        format_id: None,
        game_id: None,
        detection_error: None,
    })
}

fn render_external_file_banner(
    cx: &mut Context<'_>,
    container: Option<WidgetId>,
    label: Option<WidgetId>,
    reload: Option<WidgetId>,
    changed: bool,
) -> Result<()> {
    if let Some(container) = container {
        cx.tree.set_visible(container, changed)?;
    }
    if let Some(label) = label {
        if changed {
            cx.tree.set_text(
                label,
                t("Файл сейва изменился после открытия (игра или другая программа). Несохранённые правки относятся к старой версии."),
            )?;
        }
    }
    if let Some(reload) = reload {
        cx.tree.set_enabled(reload, changed)?;
    }
    Ok(())
}

fn add_external_file_banner(
    tree: &mut crate::widget::Tree,
    parent: WidgetId,
) -> Result<(WidgetId, WidgetId, WidgetId)> {
    let row = style::row(tree, parent)?;
    let label = style::label(tree, row, "Файл сейва изменился после открытия.", Text::Note)?;
    let reload = style::button(tree, row, "Открыть заново", Button::Secondary)?;
    tree.set_visible(row, false)?;
    Ok((row, label, reload))
}

fn add_backup_recovery_banner(
    tree: &mut crate::widget::Tree,
    parent: WidgetId,
) -> Result<(WidgetId, WidgetId, WidgetId)> {
    let row = style::row(tree, parent)?;
    let label = style::label(
        tree,
        row,
        "Обнаружена прерванная запись сейва. Проверьте резервную копию перед продолжением.",
        Text::Note,
    )?;
    let open = style::button(tree, row, "Открыть восстановление", Button::Secondary)?;
    tree.set_visible(row, false)?;
    tree.set_visible(label, false)?;
    tree.set_visible(open, false)?;
    Ok((row, label, open))
}

/// Save list and selected-save overview.
struct Overview {
    workspace: Workspace,
    header_panel: Option<WidgetId>,
    header_name: Option<WidgetId>,
    header_path: Option<WidgetId>,
    parameters_panel: Option<WidgetId>,
    tiles: Vec<WidgetId>,
    details_wide: Option<WidgetId>,
    details_narrow: Option<WidgetId>,
    details: Vec<DetailSet>,
    external_banner_row: Option<WidgetId>,
    external_banner: Option<WidgetId>,
    external_reload: Option<WidgetId>,
    backup_recovery_row: Option<WidgetId>,
    backup_recovery_label: Option<WidgetId>,
    backup_recovery_button: Option<WidgetId>,
    startup_backup_check_started: bool,
}

impl Overview {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            header_panel: None,
            header_name: None,
            header_path: None,
            parameters_panel: None,
            tiles: Vec::new(),
            details_wide: None,
            details_narrow: None,
            details: Vec::new(),
            external_banner_row: None,
            external_banner: None,
            external_reload: None,
            backup_recovery_row: None,
            backup_recovery_label: None,
            backup_recovery_button: None,
            startup_backup_check_started: false,
        }
    }

    fn startup_backup_check(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.startup_backup_check_started {
            return Ok(());
        }
        let Some(proxy) = cx.proxy.cloned() else {
            return Ok(());
        };
        self.startup_backup_check_started = true;
        let backup_directory = self.workspace.backup_directory();
        if let Err(error) = self.workspace.spawn("startup-backup-check", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = sse_storage::transaction::list_backups(&backup_directory)
                .map(|entries| {
                    entries
                        .iter()
                        .filter(|entry| entry.status == sse_storage::transaction::BackupStatus::Interrupted)
                        .count()
                })
                .map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Overview,
                Box::new(StartupBackupCheck(result)),
            ));
        }) {
            cx.status = Some(tr("Не удалось проверить резервные копии после запуска: {0}", &[&error]));
        }
        Ok(())
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        render_external_file_banner(
            cx,
            self.external_banner_row,
            self.external_banner,
            self.external_reload,
            state.external_change,
        )?;
        let selected = state
            .selected
            .as_ref()
            .map(|save| (save.info.clone(), save.parameters.clone(), save.integrity.clone()));
        let compact = cx.tree.size().0 < 1600;
        self.apply_compact(cx.tree, compact)?;
        self.render_details(cx, selected)?;
        Ok(())
    }
}

impl Overview {
    /// Width-dependent layout: the library width and the wide or stacked detail layout.
    fn apply_compact(&self, tree: &mut crate::widget::Tree, compact: bool) -> Result<()> {
        if let Some(wide) = self.details_wide {
            tree.set_visible(wide, !compact)?;
        }
        if let Some(narrow) = self.details_narrow {
            tree.set_visible(narrow, compact)?;
        }
        Ok(())
    }

    fn render_details(&self, cx: &mut Context<'_>, selected: Option<(String, String, String)>) -> Result<()> {
        let has_save = selected.is_some();
        let (info, parameters, integrity) = selected.unwrap_or_default();
        let info_pairs = detail_pairs(&info);
        let parameter_pairs = detail_pairs(&parameters);
        let integrity_pairs = detail_pairs(&integrity);
        if let Some(header) = self.header_panel {
            cx.tree.set_visible(header, has_save)?;
        }
        let (game, name, path) = info_header_values(&info_pairs);
        if let Some(id) = self.header_name {
            cx.tree.set_text(id, name)?;
        }
        if let Some(id) = self.header_path {
            cx.tree.set_text(id, &format!("{game} · {path}"))?;
        }
        if let Some(panel) = self.parameters_panel {
            cx.tree.set_visible(panel, has_save && !parameter_pairs.is_empty())?;
        }
        for (index, tile) in self.tiles.iter().enumerate() {
            let pair = parameter_pairs.get(index);
            cx.tree.set_visible(*tile, pair.is_some())?;
            if let Some((key, value)) = pair {
                set_pair(cx.tree, *tile, key, value)?;
            }
        }
        for set in &self.details {
            cx.tree.set_visible(set.info_empty, !has_save)?;
            for (index, row) in set.info_rows.iter().enumerate() {
                let pair = info_pairs.get(index).filter(|_| has_save);
                cx.tree.set_visible(*row, pair.is_some())?;
                if let Some((key, value)) = pair {
                    set_pair(cx.tree, *row, key, value)?;
                }
            }
            cx.tree.set_visible(set.integrity_panel, has_save)?;
            for (index, row) in set.integrity_rows.iter().enumerate() {
                let pair = integrity_pairs.get(index).filter(|_| has_save);
                cx.tree.set_visible(*row, pair.is_some())?;
                if let Some((key, value)) = pair {
                    set_pair(cx.tree, *row, key, value)?;
                }
            }
        }
        Ok(())
    }
}

/// One layout of the information and integrity panels; the screen keeps one for the wide row and one for the stack.
struct DetailSet {
    info_empty: WidgetId,
    info_rows: Vec<WidgetId>,
    integrity_panel: WidgetId,
    integrity_rows: Vec<WidgetId>,
}

fn build_detail_set(tree: &mut crate::widget::Tree, parent: WidgetId) -> Result<DetailSet> {
    let info_panel = style::d2::panel(tree, parent)?;
    tree.set_style(info_panel, detail_panel_style())?;
    style::d2::panel_title(tree, info_panel, crate::strings::t("ИНФОРМАЦИЯ О СОХРАНЕНИИ"))?;
    let info_empty = paragraph(tree, info_panel, "Выберите сохранение для просмотра.", Text::Body)?;
    let mut info_rows = Vec::with_capacity(DETAIL_INFO_SLOTS);
    for _ in 0..DETAIL_INFO_SLOTS {
        let row = style::d2::key_value_row(tree, info_panel, "", "")?;
        tree.set_visible(row, false)?;
        info_rows.push(row);
    }
    let integrity_panel = style::d2::panel(tree, parent)?;
    tree.set_style(integrity_panel, detail_panel_style())?;
    style::d2::panel_title(tree, integrity_panel, crate::strings::t("ЦЕЛОСТНОСТЬ И МЕТАДАННЫЕ"))?;
    let mut integrity_rows = Vec::with_capacity(DETAIL_INTEGRITY_SLOTS);
    for _ in 0..DETAIL_INTEGRITY_SLOTS {
        let row = style::d2::key_value_row(tree, integrity_panel, "", "")?;
        tree.set_visible(row, false)?;
        integrity_rows.push(row);
    }
    Ok(DetailSet {
        info_empty,
        info_rows,
        integrity_panel,
        integrity_rows,
    })
}

const DETAIL_TILE_SLOTS: usize = 8;
const DETAIL_INFO_SLOTS: usize = 4;
const DETAIL_INTEGRITY_SLOTS: usize = 6;

fn detail_panel_style() -> Style {
    Style {
        grow: 1.0,
        shrink: 0.0,
        min: Size::new(340.0, 0.0),
        padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
        gap: Size::new(0.0, 0.0),
        align_items: crate::layout::Align::Stretch,
        ..Style::default()
    }
}

fn grow_panel(tree: &mut crate::widget::Tree, panel: WidgetId) -> Result<()> {
    tree.set_style(
        panel,
        Style {
            grow: 1.0,
            shrink: 0.0,
            padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
            gap: Size::new(0.0, 0.0),
            align_items: crate::layout::Align::Stretch,
            ..Style::default()
        },
    )
}

/// Key and value of a tile or a key-value row, both labels of the widget in order.
fn set_pair(tree: &mut crate::widget::Tree, widget: WidgetId, key: &str, value: &str) -> Result<()> {
    let parts = tree.children(widget);
    if let Some(label) = parts.first() {
        tree.set_text(*label, &key.to_uppercase())?;
    }
    if let Some(label) = parts.get(1) {
        tree.set_text(*label, value)?;
    }
    Ok(())
}

/// Splits a details text into key and value pairs: one per line segment separated by " · ", the key before the first
/// ": ". Segments without a value or with the "—" placeholder carry no data and are left out.
fn detail_pairs(text: &str) -> Vec<(String, String)> {
    text.lines()
        .flat_map(|line| line.split(" · "))
        .filter_map(|segment| {
            let (key, value) = segment.split_once(": ")?;
            let value = value.trim();
            if value.is_empty() || value == "—" {
                None
            } else {
                Some((key.trim().to_owned(), value.to_owned()))
            }
        })
        .collect()
}

fn info_header_values(pairs: &[(String, String)]) -> (&str, &str, &str) {
    (
        pairs.first().map_or("", |(_, value)| value.as_str()),
        pairs.get(1).map_or("", |(_, value)| value.as_str()),
        pairs.get(2).map_or("", |(_, value)| value.as_str()),
    )
}

impl Screen for Overview {
    fn id(&self) -> ScreenId {
        ScreenId::Overview
    }

    fn subtitle(&self) -> &str {
        t("Список сохранений и сведения о выбранном файле")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (row, banner, reload) = add_external_file_banner(cx.tree, host)?;
        self.external_banner_row = Some(row);
        self.external_banner = Some(banner);
        self.external_reload = Some(reload);
        let (recovery_row, recovery_label, recovery_button) = add_backup_recovery_banner(cx.tree, host)?;
        self.backup_recovery_row = Some(recovery_row);
        self.backup_recovery_label = Some(recovery_label);
        self.backup_recovery_button = Some(recovery_button);
        let body = cx.tree.add(
            Some(host),
            NodeKind::Row,
            Style {
                grow: 1.0,
                gap: Size::new(crate::theme::CONTROL_GAP + 6.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let column = cx.tree.add(
            Some(body),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                gap: Size::new(0.0, crate::theme::CONTROL_GAP + 2.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let header = style::d2::panel(cx.tree, column)?;
        cx.tree.set_style(
            header,
            Style {
                shrink: 0.0,
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                ..Style::default()
            },
        )?;
        self.header_panel = Some(header);
        let header_row = cx.tree.add(
            Some(header),
            NodeKind::Row,
            Style {
                gap: Size::new(16.0, 0.0),
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.add(
            Some(header_row),
            NodeKind::Leaf,
            Style {
                min: Size::new(112.0, 63.0),
                shrink: 0.0,
                ..Style::default()
            },
            Content::Label {
                text: crate::strings::t("нет снимка").to_owned(),
                style: Text::Note.style(),
            },
            Look {
                fill: Some(style::d2::argb(crate::theme::d2::PANEL_RAISED)),
                border: Some((style::d2::argb(crate::theme::d2::BORDER_SUBTLE), 1.0)),
                text: style::d2::argb(crate::theme::d2::TEXT_MUTED),
                align: crate::widget::TextAlign::Center,
                ..Look::default()
            },
        )?;
        let header_text = cx.tree.add(
            Some(header_row),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                gap: Size::new(0.0, 4.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.header_name = Some(cx.tree.add(
            Some(header_text),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: String::new(),
                style: TextStyle::new(Face::HeadingMedium, 22.0),
            },
            Look {
                text: style::rgb(crate::theme::TEXT_PRIMARY),
                ..Look::default()
            },
        )?);
        self.header_path = Some(cx.tree.add(
            Some(header_text),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: String::new(),
                style: TextStyle::new(Face::Body, 13.0),
            },
            Look {
                text: style::rgb(crate::theme::TEXT_SECONDARY),
                ..Look::default()
            },
        )?);
        let parameters = style::d2::panel(cx.tree, column)?;
        self.parameters_panel = Some(parameters);
        grow_panel(cx.tree, parameters)?;
        style::d2::panel_title(cx.tree, parameters, crate::strings::t("ПАРАМЕТРЫ СТАЛКЕРА"))?;
        let grid = cx.tree.add(
            Some(parameters),
            NodeKind::Wrap,
            Style {
                gap: Size::new(8.0, 8.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for _ in 0..DETAIL_TILE_SLOTS {
            let tile = style::d2::tile(cx.tree, grid, "", "")?;
            cx.tree.set_visible(tile, false)?;
            self.tiles.push(tile);
        }
        let wide = cx.tree.add(
            Some(column),
            NodeKind::Row,
            Style {
                gap: Size::new(crate::theme::CONTROL_GAP + 2.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.details_wide = Some(wide);
        let wide_set = build_detail_set(cx.tree, wide)?;
        let narrow = cx.tree.add(
            Some(column),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, crate::theme::CONTROL_GAP + 2.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.details_narrow = Some(narrow);
        let narrow_set = build_detail_set(cx.tree, narrow)?;
        self.details = vec![wide_set, narrow_set];
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.workspace.poll_tasks();
        self.startup_backup_check(cx)?;
        if self.workspace.lock().discovery.is_none() {
            start_discovery(&self.workspace, cx);
        }
        self.render(cx)
    }

    fn open_save(&mut self, cx: &mut Context<'_>, path: &Path) -> Result<bool> {
        if cx.proxy.is_none() {
            cx.status = Some(t("Открытие сейва требует фонового канала приложения.").to_owned());
            return Ok(false);
        }
        start_load_path(&self.workspace, path, cx);
        Ok(true)
    }

    fn open_browser_file(
        &mut self,
        cx: &mut Context<'_>,
        file_name: &str,
        bytes: Vec<u8>,
        last_modified_ms: u64,
    ) -> Result<bool> {
        if bytes.is_empty() || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BROWSER_SAVE_BYTES {
            return Err(Error::Refused(
                "browser save file size is outside the supported range".to_owned(),
            ));
        }
        if self.workspace.session.is_busy() {
            return Err(Error::Refused("a save operation is already active".to_owned()));
        }
        let file_name = validate_browser_file_name(file_name)?;
        let slot = SaveSlot {
            path: PathBuf::from(format!("browser://{file_name}")),
            candidate_game_id: "unknown".to_owned(),
            candidate_release_id: "unknown".to_owned(),
            size: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
            last_write_time_utc: std::time::UNIX_EPOCH
                .checked_add(std::time::Duration::from_millis(last_modified_ms))
                .unwrap_or(std::time::UNIX_EPOCH),
            format_id: None,
            game_id: None,
            detection_error: None,
        };
        let loaded = Arc::new(LoadedSave::from_buffer(slot, SaveBuffer::from_vec(bytes))?);
        let journal = DraftJournal::new(vec![DraftPlan::empty(&loaded.source_sha256)?], 0)?;
        {
            let mut state = self.workspace.lock();
            state.load_request = state.load_request.saturating_add(1);
            state.loading = false;
            state.load_error = None;
            state.selected = Some(Arc::clone(&loaded));
            state.pending_money = None;
            state.pending_stacks.clear();
            state.pending_durability.clear();
            state.pending_placements.clear();
            state.pending_upgrades.clear();
            state.pending_removed.clear();
            state.pending_adds.clear();
            state.pending_stash_moves.clear();
            state.pending_xray_stash_takes.clear();
            state.pending_xray_stash_puts.clear();
            state.pending_faction_relations.clear();
            state.pending_relocation = None;
            state.external_change = false;
        }
        cx.app
            .set_current_save_identity(loaded.slot.path.clone(), loaded.source_sha256.clone());
        cx.app.set_selected_game(loaded.slot.game_id.clone());
        let legacy_s2 = matches!(&loaded.data, SaveData::Stalker2 { save, .. } if save.index().is_legacy());
        cx.app.set_current_save_format(loaded.slot.format_id.clone(), legacy_s2);
        cx.app.set_draft_journal(journal.clone());
        set_workspace_draft(&self.workspace, &journal);
        self.render(cx)?;
        cx.status = Some(crate::strings::t("Файл обрабатывается только в этом браузере.").to_owned());
        Ok(true)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        self.workspace.poll_tasks();
        if let Message::User(AppMessage::Tick(seconds)) = message {
            schedule_file_check(&self.workspace, cx, *seconds);
        }
        if let Message::Window(WindowEvent::Resized { width, .. }) = message {
            self.apply_compact(cx.tree, *width < 1600)?;
        }
        if clicked.is_some() && clicked == self.backup_recovery_button {
            if let Some(proxy) = cx.proxy {
                let _ = proxy.send(AppMessage::OpenBackups);
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.external_reload {
            return start_reload_selected(&self.workspace, cx);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message {
            if let Some(StartupBackupCheck(Ok(interrupted_count))) = payload.downcast_ref::<StartupBackupCheck>() {
                let found = *interrupted_count > 0;
                if let Some(row) = self.backup_recovery_row {
                    cx.tree.set_visible(row, found)?;
                }
                if let Some(label) = self.backup_recovery_label {
                    cx.tree.set_visible(label, found)?;
                }
                if let Some(button) = self.backup_recovery_button {
                    cx.tree.set_visible(button, found)?;
                }
                if found {
                    cx.status = Some(tr(
                        "Обнаружена прерванная запись сейва ({0}). Откройте восстановление, чтобы проверить копию и продолжить.",
                        &[interrupted_count],
                    ));
                }
            } else if let Some(StartupBackupCheck(Err(error))) = payload.downcast_ref::<StartupBackupCheck>() {
                cx.status = Some(tr("Не удалось проверить резервные копии после запуска: {0}", &[&error]));
            }
            if let Some(refresh) = payload.downcast_ref::<RefreshOverview>() {
                start_discovery(&self.workspace, cx);
                if let Some(path) = refresh.path.clone() {
                    let requested_path = path.clone();
                    start_load_from(&self.workspace, move || slot_for_path(&path), false, requested_path, cx);
                }
            }
            if let Some(FileCheckFinished {
                path,
                source_sha256,
                changed,
            }) = payload.downcast_ref::<FileCheckFinished>()
            {
                let mut state = self.workspace.lock();
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.slot.path == *path && selected.source_sha256 == *source_sha256)
                {
                    state.external_change = *changed;
                }
            }
            if let Some(LoadFinished {
                request,
                selected_path,
                requested_path,
                journal,
                error,
                io_error,
            }) = payload.downcast_ref::<LoadFinished>()
            {
                self.workspace.finish_load_request(*request);
                if self.workspace.lock().load_request == *request {
                    if let (Some(path), Some(journal)) = (selected_path.as_ref(), journal.as_ref()) {
                        let source_sha256 = journal
                            .current()
                            .map(|plan| plan.source_sha256.clone())
                            .unwrap_or_default();
                        cx.app.set_current_save_identity(path.clone(), source_sha256);
                        cx.app.set_draft_journal(journal.clone());
                        let (game, format_id, legacy_s2) = self
                            .workspace
                            .lock()
                            .selected
                            .as_ref()
                            .map(|save| {
                                (
                                    save.slot.game_id.clone(),
                                    save.slot.format_id.clone(),
                                    matches!(&save.data, SaveData::Stalker2 { save, .. } if save.index().is_legacy()),
                                )
                            })
                            .unwrap_or((None, None, false));
                        cx.app.set_selected_game(game);
                        cx.app.set_current_save_format(format_id, legacy_s2);
                        set_workspace_draft(&self.workspace, journal);
                    } else {
                        cx.app.set_current_save(None);
                        cx.app.set_selected_game(None);
                    }
                    if let Some(error) = error {
                        let name = requested_path
                            .file_name()
                            .unwrap_or(requested_path.as_os_str())
                            .to_string_lossy();
                        cx.status = Some(if *io_error {
                            tr!("Не удалось открыть «{name}»: {error}", name = &name, error = error)
                        } else {
                            tr!(
                                "«{name}» — не сохранение S.T.A.L.K.E.R. или файл повреждён.",
                                name = &name
                            )
                        });
                    } else {
                        cx.status = Some(t("Сейв прочитан и проверен.").to_owned());
                    }
                }
            }
            self.render(cx)?;
        }
        Ok(())
    }
}

/// A flexible table column: `grow` shares the spare width, `min` keeps a floor for the text.
fn set_column_style(tree: &mut Tree, id: WidgetId, grow: f32, min: f32) -> Result<()> {
    tree.set_style(
        id,
        Style {
            grow,
            shrink: 1.0,
            min: Size::new(min, 0.0),
            ..Style::default()
        },
    )
}

/// A fixed-width table column shared by the header and the rows.
fn set_fixed_column_style(tree: &mut Tree, id: WidgetId, width: f32) -> Result<()> {
    tree.set_style(
        id,
        Style {
            min: Size::new(width, 0.0),
            preferred: Size::new(width, 0.0),
            max: Size::new(width, f32::INFINITY),
            shrink: 0.0,
            ..Style::default()
        },
    )
}

/// Text and state of one item row in the inventory table.
struct ItemCells<'a> {
    name: &'a str,
    key: &'a str,
    place: &'a str,
    condition: &'a str,
    condition_ratio: Option<f32>,
    count: &'a str,
    selected: bool,
    editable: bool,
}

/// Fills one table row: the name, key, placement, condition (coloured by threshold), count and the selection look.
fn fill_item_row(tree: &mut Tree, row: &ItemControls, cells: &ItemCells<'_>, compact: bool) -> Result<()> {
    tree.set_visible(row.row, true)?;
    tree.set_text(row.label, cells.name)?;
    tree.set_text(row.key, cells.key)?;
    tree.set_visible(row.key, !compact)?;
    tree.set_text(row.place, cells.place)?;
    let condition_color = match cells.condition_ratio {
        Some(value) if value >= 0.75 => crate::theme::d2::SUCCESS,
        Some(value) if value >= 0.4 => crate::theme::d2::ACCENT,
        Some(_) => crate::theme::d2::DANGER,
        None => crate::theme::d2::TEXT_MUTED,
    };
    tree.set_text(row.condition, cells.condition)?;
    tree.set_look(
        row.condition,
        Look {
            text: style::d2::argb(condition_color),
            ..Look::default()
        },
    )?;
    tree.set_text(row.count, cells.count)?;
    tree.set_visible(row.select, true)?;
    tree.set_enabled(row.select, true)?;
    tree.set_look(
        row.select,
        Look {
            fill: cells.selected.then(|| style::d2::argb(crate::theme::d2::ACCENT_TINT)),
            border: cells.selected.then(|| (style::d2::argb(crate::theme::d2::ACCENT), 1.0)),
            hover_fill: Some(style::d2::argb(crate::theme::d2::ROW_HOVER)),
            radius: crate::theme::BUTTON_RADIUS,
            ..Look::default()
        },
    )?;
    tree.set_visible(row.decrease, cells.editable)?;
    tree.set_visible(row.increase, cells.editable)?;
    Ok(())
}

/// Placement shown in the table: the draft change when there is one, else the item's own placement.
fn placement_cell(state: &WorkspaceState, handle: ItemHandle, base: Option<&str>) -> String {
    match state.pending_placements.get(&handle) {
        Some(DraftPlacement::Ruck) => t("Рюкзак").to_owned(),
        Some(DraftPlacement::Belt) => t("Пояс").to_owned(),
        Some(DraftPlacement::Slot(slot)) => tr("Слот {0}", &[slot]),
        None => match base {
            Some("ruck") => t("Рюкзак").to_owned(),
            Some("belt") => t("Пояс").to_owned(),
            Some("slot") => t("Слот").to_owned(),
            _ => String::new(),
        },
    }
}

struct ItemControls {
    row: WidgetId,
    label: WidgetId,
    key: WidgetId,
    place: WidgetId,
    condition: WidgetId,
    count: WidgetId,
    select: WidgetId,
    decrease: WidgetId,
    increase: WidgetId,
    handle: Option<ItemHandle>,
}

#[derive(Hash, PartialEq, Eq)]
struct XrayItemGroupKey<'a> {
    section: &'a str,
    category: &'a str,
    placement: Option<&'a str>,
    condition_bits: Option<u32>,
    upgrades: Vec<String>,
    unique_handle: Option<u16>,
}

fn group_xray_items<'a>(
    save: &Save,
    items: Vec<&'a InventoryItem>,
    state: &WorkspaceState,
) -> Vec<Vec<&'a InventoryItem>> {
    let mut groups: Vec<Vec<&InventoryItem>> = Vec::new();
    let mut positions: HashMap<XrayItemGroupKey<'a>, usize> = HashMap::new();
    for item in items {
        let handle = ItemHandle::Xray(item.handle);
        let placement = state
            .pending_placements
            .get(&handle)
            .map(|value| match value {
                DraftPlacement::Ruck => "ruck",
                DraftPlacement::Belt => "belt",
                DraftPlacement::Slot(_) => "slot",
            })
            .or(item.placement.as_deref());
        let condition_bits = state
            .pending_durability
            .get(&handle)
            .map(|value| (f32::from(*value) / 100.0).to_bits())
            .or_else(|| item.condition.map(f32::to_bits));
        let upgrades = writer::current_upgrades(save, item.handle);
        let has_pending_edit = state.pending_durability.contains_key(&handle)
            || state.pending_placements.contains_key(&handle)
            || state.pending_upgrades.contains_key(&handle)
            || state.pending_stacks.contains_key(&handle);
        let key = XrayItemGroupKey {
            section: &item.section,
            category: &item.category,
            placement,
            condition_bits,
            upgrades: upgrades.clone().unwrap_or_default(),
            unique_handle: (item.count.is_some() || has_pending_edit || upgrades.is_err()).then_some(item.handle),
        };
        if let Some(index) = positions.get(&key).copied() {
            if let Some(group) = groups.get_mut(index) {
                group.push(item);
            }
        } else {
            let index = groups.len();
            positions.insert(key, index);
            groups.push(vec![item]);
        }
    }
    groups
}

#[derive(Hash, PartialEq, Eq)]
struct S2ItemGroupKey {
    type_key: [u8; 3],
    kind_code: u8,
    count: u32,
    width: Option<u16>,
    height: Option<u16>,
    condition_bits: Option<u32>,
    weight_bits: u32,
    modules: Vec<String>,
    upgrades: Vec<String>,
    unique_handle: Option<u32>,
}

fn group_s2_items<'a>(items: &[&'a S2InventoryItem], state: &WorkspaceState) -> Vec<Vec<&'a S2InventoryItem>> {
    let mut groups: Vec<Vec<&S2InventoryItem>> = Vec::new();
    let mut positions: HashMap<S2ItemGroupKey, usize> = HashMap::new();
    for item in items {
        let handle = ItemHandle::Stalker2(item.handle);
        let condition_bits = state
            .pending_durability
            .get(&handle)
            .map(|value| (f32::from(*value) / 100.0).to_bits())
            .or_else(|| item.condition.map(f32::to_bits));
        let has_pending_edit = state.pending_stacks.contains_key(&handle)
            || state.pending_durability.contains_key(&handle)
            || state.pending_placements.contains_key(&handle)
            || state.pending_upgrades.contains_key(&handle);
        let key = S2ItemGroupKey {
            type_key: item.type_key,
            kind_code: item.kind_code,
            count: item.count,
            width: item.width,
            height: item.height,
            condition_bits,
            weight_bits: item.total_weight.to_bits(),
            modules: item.modules.clone(),
            upgrades: item.upgrades.clone(),
            unique_handle: (item.editable_count || has_pending_edit).then_some(item.handle),
        };
        if let Some(index) = positions.get(&key).copied() {
            if let Some(group) = groups.get_mut(index) {
                group.push(item);
            }
        } else {
            let index = groups.len();
            positions.insert(key, index);
            groups.push(vec![item]);
        }
    }
    groups
}

struct UpgradeControl {
    widget: WidgetId,
    key: Option<String>,
}

#[derive(Clone)]
struct AddCandidate {
    key: String,
    display_name: String,
    template_available: bool,
}

fn add_template_preference(
    story_id: Option<u32>,
    spawn_story_id: Option<u32>,
    has_custom_data: bool,
    spawn_id: Option<u16>,
    object_id: u16,
) -> (bool, bool, bool, bool, u16) {
    (
        story_id.is_some_and(|value| value != u32::MAX),
        spawn_story_id.is_some_and(|value| value != u32::MAX),
        has_custom_data,
        spawn_id != Some(u16::MAX),
        object_id,
    )
}

fn add_candidates(selected: &LoadedSave, removed: &BTreeSet<u32>) -> Vec<AddCandidate> {
    let SaveData::Xray { save, inventory } = &selected.data else {
        return Vec::new();
    };
    if writer::capability(save.format(), writer::ChangeKind::AddItems) == writer::Capability::Unsupported {
        return Vec::new();
    }
    let Some(catalog) = sse_catalog::CatalogBundleReader::load_embedded().get(save.format().id()) else {
        return Vec::new();
    };
    let mut candidates = catalog
        .items
        .items()
        .iter()
        .map(|item| AddCandidate {
            key: item.key.clone(),
            display_name: sse_catalog::SaveNaming::item_name(
                save.format().id(),
                &item.key,
                item.display_name.as_deref(),
            ),
            template_available: inventory
                .iter()
                .any(|candidate| candidate.section == item.key && !removed.contains(&u32::from(candidate.handle))),
        })
        .collect::<Vec<_>>();
    // Candidates that can be added (with a template in the save) come first, then by name.
    candidates.sort_by(|left, right| {
        right
            .template_available
            .cmp(&left.template_available)
            .then_with(|| left.display_name.to_lowercase().cmp(&right.display_name.to_lowercase()))
            .then_with(|| left.key.cmp(&right.key))
    });
    candidates
}

/// Style of the right-hand column. While the add panel is open the column has no gaps: the hidden inspector would
/// otherwise keep a gap above the panel, and the panel gives its bottom margin instead.
fn side_column_style(compact: bool, add_open: bool) -> Style {
    let width = side_column_width(compact);
    Style {
        preferred: Size::new(width, 0.0),
        min: Size::new(width, 0.0),
        max: Size::new(width, f32::INFINITY),
        shrink: 0.0,
        gap: Size::new(0.0, if add_open { 0.0 } else { crate::theme::CONTROL_GAP }),
        align_items: crate::layout::Align::Stretch,
        ..Style::default()
    }
}

/// Style of the add panel: it grows into the side column above the pinned action panel.
fn add_panel_style(bottom_margin: f32) -> Style {
    Style {
        grow: 1.0,
        shrink: 1.0,
        min: crate::layout::Size::new(0.0, 0.0),
        padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
        margin: crate::layout::Edges {
            bottom: bottom_margin,
            ..crate::layout::Edges::default()
        },
        gap: Size::new(0.0, ADD_PANEL_GAP),
        align_items: crate::layout::Align::Stretch,
        ..Style::default()
    }
}

/// Width a candidate's text may take: the side column less the panel padding and the button's 16 px insets.
fn add_candidate_text_width(compact: bool) -> f32 {
    side_column_width(compact) - 2.0 * crate::theme::d2::PANEL_PADDING.0 - 32.0
}

/// `text`, shortened with an ellipsis at its end until it fits in `max_width` pixels.
fn fit_text(tree: &crate::widget::Tree, text: &str, style: TextStyle, max_width: f32) -> String {
    if tree.measure_text(text, style) <= max_width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if tree.measure_text(&candidate, style) <= max_width {
            return candidate;
        }
    }
    "…".to_owned()
}

/// Gap between the children of the add panel.
const ADD_PANEL_GAP: f32 = 10.0;

/// A whole pixel count as a float, for layout arithmetic. Values beyond the range of the window are clamped.
fn px_i64(value: i64) -> f32 {
    f32::from(i16::try_from(value).unwrap_or(i16::MAX))
}

/// Width of the inspector's column: the right-hand column of the inventory.
fn side_column_width(compact: bool) -> f32 {
    if compact {
        296.0
    } else {
        360.0
    }
}

/// A child that keeps its measured height when its parent is short of room.
fn keep_height(tree: &mut Tree, id: WidgetId) -> Result<()> {
    tree.set_style(
        id,
        Style {
            shrink: 0.0,
            ..Style::default()
        },
    )
}

/// A row that keeps its height and its gap between the buttons it holds.
fn keep_height_row(tree: &mut Tree, id: WidgetId) -> Result<()> {
    tree.set_style(
        id,
        Style {
            gap: Size::new(crate::theme::CONTROL_GAP, 0.0),
            align_items: crate::layout::Align::Center,
            shrink: 0.0,
            ..Style::default()
        },
    )
}

/// Inventory screen with guarded X-Ray and S2 edits.
struct Inventory {
    workspace: Workspace,
    page: usize,
    inventory_card: Option<WidgetId>,
    add_panel: Option<WidgetId>,
    add_search_widget: Option<WidgetId>,
    add_search: Option<TextInput>,
    add_search_query: String,
    add_quantity_widget: Option<WidgetId>,
    add_quantity: Option<TextInput>,
    add_candidate_rows: Vec<(WidgetId, Option<String>)>,
    /// Per candidate row: its column and the wrapped note under the name (key and reason).
    add_candidate_slots: Vec<(WidgetId, WidgetId)>,
    add_page_range: Option<WidgetId>,
    add_empty: Option<WidgetId>,
    add_previous: Option<WidgetId>,
    add_next: Option<WidgetId>,
    add_confirm: Option<WidgetId>,
    add_cancel: Option<WidgetId>,
    add_candidates: Vec<AddCandidate>,
    add_selected_key: Option<String>,
    add_selected_name: Option<WidgetId>,
    add_selected_key_label: Option<WidgetId>,
    add_note: Option<WidgetId>,
    add_scroll: Option<WidgetId>,
    add_filler: Option<WidgetId>,
    add_draft_note: Option<WidgetId>,
    add_page: usize,
    add_panel_open: bool,
    add_previous_focus: Option<WidgetId>,
    money_label: Option<WidgetId>,
    money_input_widget: Option<WidgetId>,
    money_input: Option<TextInput>,
    money_buttons: Vec<(WidgetId, u32)>,
    search_widget: Option<WidgetId>,
    search: Option<TextInput>,
    search_query: String,
    clear_search: Option<WidgetId>,
    search_count: Option<WidgetId>,
    empty_results: Option<WidgetId>,
    reset_filters: Option<WidgetId>,
    categories: Vec<(WidgetId, &'static str)>,
    selected_category: &'static str,
    previous: Option<WidgetId>,
    next: Option<WidgetId>,
    export: Option<WidgetId>,
    status: Option<WidgetId>,
    rows: Vec<ItemControls>,
    key_header: Option<WidgetId>,
    page_range: Option<WidgetId>,
    side_column: Option<WidgetId>,
    inspector_panel: Option<WidgetId>,
    actions_panel: Option<WidgetId>,
    compact: bool,
    page_size: usize,
    item_list: Option<WidgetId>,
    item_spacer: Option<WidgetId>,
    selected_item: Option<ItemHandle>,
    inspector_summary: Option<WidgetId>,
    inspector_condition_heading: Option<WidgetId>,
    inspector_condition: Option<WidgetId>,
    inspector_placement: Option<WidgetId>,
    inspector_upgrades: Option<WidgetId>,
    condition_buttons: Vec<(WidgetId, u8)>,
    placement_buttons: Vec<(WidgetId, DraftPlacement)>,
    upgrade_controls: Vec<UpgradeControl>,
    remove_button: Option<WidgetId>,
    add_button: Option<WidgetId>,
    last_path: Option<PathBuf>,
    external_banner_row: Option<WidgetId>,
    external_banner: Option<WidgetId>,
    external_reload: Option<WidgetId>,
    process_confirmation: Option<WidgetId>,
    process_description: Option<WidgetId>,
    process_continue: Option<WidgetId>,
    process_cancel: Option<WidgetId>,
    next_process_check_id: u64,
    pending_save_request: Option<(u64, PendingSaveRequest)>,
    process_check_complete: bool,
}

fn money_input_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 10,
        history_limit: 32,
        filter: InputFilter::Any,
    }
}

/// The file-name field of the library panel. It writes the shared filter in [`Workspace`].
pub(super) struct LibrarySearch {
    pub(super) widget: Option<WidgetId>,
    input: TextInput,
}

impl LibrarySearch {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            widget: None,
            input: TextInput::new("", inventory_search_config())?,
        })
    }

    /// Adds the field to `parent` and keeps its widget for focus and clicks.
    pub(super) fn build(&mut self, tree: &mut Tree, parent: WidgetId) -> Result<WidgetId> {
        let colors = crate::theme::current().colors;
        let widget = tree.add(
            Some(parent),
            NodeKind::Leaf,
            Style {
                grow: 0.0,
                shrink: 1.0,
                preferred: Size::new(0.0, crate::theme::BUTTON_HEIGHT),
                min: Size::new(0.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: crate::strings::t("Поиск по имени файла…").to_owned(),
                style: Text::Body.style(),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[0]),
                ..Look::default()
            },
        )?;
        self.widget = Some(widget);
        Ok(widget)
    }

    /// Shows the filter text, or the placeholder while the filter is empty.
    pub(super) fn show(&self, tree: &mut Tree, query: &str) -> Result<()> {
        if let Some(widget) = self.widget {
            tree.set_text(
                widget,
                if query.is_empty() {
                    crate::strings::t("Поиск по имени файла…")
                } else {
                    query
                },
            )?;
        }
        Ok(())
    }

    /// Handles focus, text input and clicks for the field. Returns `true` when the shared filter changed.
    pub(super) fn handle(
        &mut self,
        tree: &mut Tree,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
        workspace: &Workspace,
    ) -> Result<bool> {
        if let Some(widget) = self.widget {
            let focused = tree.focused() == Some(widget);
            self.input.focus(focused, 0);
        }
        if let Message::Window(crate::event_loop::WindowEvent::Ime(event)) = message {
            if self.input.focused() {
                self.input.apply_ime_event(event)?;
                let display = self.input.display_text();
                if let Some(widget) = self.widget {
                    tree.set_input_text(widget, &display)?;
                }
                if matches!(event, crate::event_loop::ImeEvent::Commit(_)) {
                    workspace.set_search_query(&self.input.text());
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
            ..
        }) = message
        {
            if *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if let Some(widget) = self.widget.filter(|widget| tree.is_visible(*widget)) {
                    tree.set_focus(Some(widget))?;
                    self.input.focus(true, 0);
                }
                return Ok(false);
            }
            if self.input.focused() {
                if matches!(*keysym, 0xff0d | 0xff1b) {
                    self.input.focus(false, 0);
                    tree.set_focus(None)?;
                    return Ok(false);
                }
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = SaveClipboard::default();
                let _ = self.input.key(
                    key,
                    Modifiers {
                        ctrl: *ctrl,
                        shift: *shift,
                    },
                    typed.as_deref(),
                    &mut clipboard,
                )?;
                let query = self.input.text();
                if let Some(widget) = self.widget {
                    tree.set_text(widget, &query)?;
                }
                workspace.set_search_query(&query);
                return Ok(true);
            }
        }
        if clicked.is_some() && clicked == self.widget {
            if let Some(widget) = self.widget {
                tree.set_focus(Some(widget))?;
            }
            self.input.focus(true, 0);
        }
        Ok(false)
    }
}

fn inventory_search_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 128,
        history_limit: 32,
        filter: InputFilter::Any,
    }
}

fn add_quantity_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 10,
        history_limit: 32,
        filter: InputFilter::Digits {
            min: Some(0),
            max: None,
            allow_empty: true,
        },
    }
}

#[derive(Default)]
struct SaveClipboard(String);

impl Clipboard for SaveClipboard {
    fn read_text(&mut self) -> Result<String> {
        Ok(self.0.clone())
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        self.0.clear();
        self.0.push_str(text);
        Ok(())
    }
}

impl Inventory {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            page: 0,
            inventory_card: None,
            add_panel: None,
            add_search_widget: None,
            add_search: None,
            add_search_query: String::new(),
            add_quantity_widget: None,
            add_quantity: None,
            add_candidate_rows: Vec::new(),
            add_candidate_slots: Vec::new(),
            add_page_range: None,
            add_empty: None,
            add_previous: None,
            add_next: None,
            add_confirm: None,
            add_cancel: None,
            add_candidates: Vec::new(),
            add_selected_key: None,
            add_selected_name: None,
            add_selected_key_label: None,
            add_note: None,
            add_scroll: None,
            add_filler: None,
            add_draft_note: None,
            add_page: 0,
            add_panel_open: false,
            add_previous_focus: None,
            money_label: None,
            money_input_widget: None,
            money_input: None,
            money_buttons: Vec::new(),
            search_widget: None,
            search: None,
            search_query: String::new(),
            clear_search: None,
            search_count: None,
            empty_results: None,
            reset_filters: None,
            categories: Vec::new(),
            selected_category: "ВСЕ",
            previous: None,
            next: None,
            export: None,
            status: None,
            rows: Vec::new(),
            key_header: None,
            page_range: None,
            side_column: None,
            inspector_panel: None,
            actions_panel: None,
            compact: false,
            page_size: 8,
            item_list: None,
            item_spacer: None,
            selected_item: None,
            inspector_summary: None,
            inspector_condition_heading: None,
            inspector_condition: None,
            inspector_placement: None,
            inspector_upgrades: None,
            condition_buttons: Vec::new(),
            placement_buttons: Vec::new(),
            upgrade_controls: Vec::new(),
            remove_button: None,
            add_button: None,
            last_path: None,
            external_banner_row: None,
            external_banner: None,
            external_reload: None,
            process_confirmation: None,
            process_description: None,
            process_continue: None,
            process_cancel: None,
            next_process_check_id: 0,
            pending_save_request: None,
            process_check_complete: false,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let window_width = cx.tree.size().0;
        if window_width > 0 {
            self.compact = window_width < 1600;
        }
        self.sync_add_list_cap(cx)?;
        // A wrapped note measures its lines at its minimum width, so the width it is drawn in is given here.
        let note_width = side_column_width(self.compact) - 2.0 * crate::theme::d2::PANEL_PADDING.0;
        for note in [self.add_note, self.add_draft_note].into_iter().flatten() {
            cx.tree.set_style(
                note,
                Style {
                    min: crate::layout::Size::new(note_width, 0.0),
                    shrink: 0.0,
                    ..Style::default()
                },
            )?;
        }
        cx.tree.update_layout()?;
        if let Some(inspector) = self.inspector_panel {
            // The inspector takes the room the pinned action panel leaves in the side column. It grows into that room
            // and shrinks with it; a measured height would feed back into the next layout and push the panels down.
            cx.tree.set_style(
                inspector,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, 0.0),
                    padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                    gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
            )?;
        }
        if let (Some(list), Some(spacer)) = (self.item_list, self.item_spacer) {
            // The list shows the rows that fit in the space it shares with the spacer below the paging row.
            // Before the window has a size there is no room yet: keep the last page size.
            let room = usize::try_from(cx.tree.rect(list)?.height)
                .unwrap_or(0)
                .saturating_add(usize::try_from(cx.tree.rect(spacer)?.height).unwrap_or(0));
            let row_pitch = (INVENTORY_ROW_HEIGHT + INVENTORY_ROW_GAP) as usize;
            if room > 0 {
                self.page_size = room
                    .saturating_add(INVENTORY_ROW_GAP as usize)
                    .checked_div(row_pitch)
                    .unwrap_or(0)
                    .clamp(1, INVENTORY_MAX_PAGE_SIZE);
            }
            let rows = u32::try_from(self.page_size).unwrap_or(1);
            let list_pixels = rows
                .saturating_mul(INVENTORY_ROW_HEIGHT)
                .saturating_add(rows.saturating_sub(1).saturating_mul(INVENTORY_ROW_GAP));
            let list_height = f32::from(u16::try_from(list_pixels).unwrap_or(0));
            cx.tree.set_style(
                list,
                Style {
                    preferred: crate::layout::Size::new(0.0, list_height),
                    min: crate::layout::Size::new(0.0, 0.0),
                    shrink: 1.0,
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
            )?;
        }
        let page_size = self.page_size;
        if let Some(id) = self.key_header {
            cx.tree.set_visible(id, !self.compact)?;
        }
        render_external_file_banner(
            cx,
            self.external_banner_row,
            self.external_banner,
            self.external_reload,
            state.external_change,
        )?;
        let money_input_state;
        let Some(selected) = state.selected.as_ref() else {
            self.add_panel_open = false;
            if let Some(panel) = self.add_panel {
                cx.tree.set_visible(panel, false)?;
            }
            if let Some(inspector) = self.inspector_panel {
                cx.tree.set_visible(inspector, true)?;
            }
            if let Some(side) = self.side_column {
                cx.tree.set_style(side, side_column_style(self.compact, false))?;
            }
            if let Some(actions) = self.actions_panel {
                cx.tree.set_visible(actions, true)?;
            }
            if let Some(id) = self.money_label {
                cx.tree.set_text(id, t("Сначала выберите сейв на экране «Обзор»."))?;
            }
            self.set_edit_controls(cx, false)?;
            if let Some(status) = self.status {
                cx.tree.set_text(status, t("Выберите сейв для просмотра и правки."))?;
            }
            return Ok(());
        };
        self.set_edit_controls(cx, true)?;
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.page = 0;
            self.selected_item = None;
            self.add_panel_open = false;
            if let Some(panel) = self.add_panel {
                cx.tree.set_visible(panel, false)?;
            }
            if let Some(inspector) = self.inspector_panel {
                cx.tree.set_visible(inspector, true)?;
            }
            if let Some(side) = self.side_column {
                cx.tree.set_style(side, side_column_style(self.compact, false))?;
            }
            if let Some(actions) = self.actions_panel {
                cx.tree.set_visible(actions, true)?;
            }
        }
        let selected_for_inspector = Arc::clone(selected);
        match &selected.data {
            SaveData::Xray { save, inventory } => {
                let money = save.money()?;
                let money_editable =
                    writer::capability(save.format(), writer::ChangeKind::EditMoney) == writer::Capability::Verified;
                let pending_money = state.pending_money.unwrap_or(money);
                if let Some(id) = self.money_label {
                    let disabled_reason = if money_editable {
                        ""
                    } else {
                        t(" · запись отключена для этого формата")
                    };
                    cx.tree.set_text(
                        id,
                        &tr!(
                            "Деньги: {pending_money}{disabled_reason}",
                            pending_money = pending_money,
                            disabled_reason = disabled_reason
                        ),
                    )?;
                }
                for (id, _) in &self.money_buttons {
                    cx.tree.set_visible(*id, true)?;
                    cx.tree.set_enabled(*id, money_editable)?;
                }
                money_input_state = Some((pending_money, money_editable));
                let query = self.search_query.to_lowercase();
                let visible_items: Vec<&InventoryItem> = inventory
                    .iter()
                    .filter(|item| {
                        !state.pending_removed.contains(&ItemHandle::Xray(item.handle))
                            && (self.selected_category == "ВСЕ"
                                || xray_inventory_category(&item.category, &item.section) == self.selected_category)
                            && search_matches(
                                &format!(
                                    "{} {}",
                                    sse_catalog::SaveNaming::item_name(save.format().id(), &item.section, None),
                                    item.section
                                ),
                                &query,
                            )
                    })
                    .collect();
                let visible_groups = group_xray_items(save, visible_items.clone(), &state);
                self.update_inventory_filters(cx, visible_groups.len())?;
                if !self.selected_item.is_some_and(|selected| {
                    visible_items
                        .iter()
                        .any(|item| selected == ItemHandle::Xray(item.handle))
                }) {
                    self.selected_item = visible_items
                        .iter()
                        .find(|item| item.condition.is_some())
                        .or_else(|| visible_items.first())
                        .map(|item| ItemHandle::Xray(item.handle));
                }
                let stack_editable =
                    writer::capability(save.format(), writer::ChangeKind::EditStacks) == writer::Capability::Verified;
                let start = self.page.saturating_mul(page_size);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(group) = visible_groups
                        .get(start.saturating_add(offset))
                        .filter(|_| offset < page_size)
                    {
                        let Some(item) = group.first().copied() else {
                            continue;
                        };
                        cx.tree.set_visible(row.row, true)?;
                        cx.tree.set_style(row.row, inventory_row_style(true))?;
                        let count = item.count.map_or_else(
                            || group.len().to_string(),
                            |original| {
                                state
                                    .pending_stacks
                                    .get(&ItemHandle::Xray(item.handle))
                                    .copied()
                                    .unwrap_or(u32::from(original))
                                    .to_string()
                            },
                        );
                        let condition_ratio = state
                            .pending_durability
                            .get(&ItemHandle::Xray(item.handle))
                            .map(|value| f32::from(*value) / 100.0)
                            .or(item.condition);
                        let condition = state
                            .pending_durability
                            .get(&ItemHandle::Xray(item.handle))
                            .map(|value| format!("{value}%"))
                            .or_else(|| item.condition.map(|value| format!("{:.0}%", value * 100.0)))
                            .unwrap_or_default();
                        let name = sse_catalog::SaveNaming::item_name(save.format().id(), &item.section, None);
                        let place = placement_cell(&state, ItemHandle::Xray(item.handle), item.placement.as_deref());
                        let selected = self.selected_item == Some(ItemHandle::Xray(item.handle));
                        let editable = stack_editable && item.count.is_some();
                        fill_item_row(
                            cx.tree,
                            row,
                            &ItemCells {
                                name: &short_text(&name, 22),
                                key: &item.section,
                                place: &place,
                                condition: &condition,
                                condition_ratio,
                                count: &count,
                                selected,
                                editable,
                            },
                            self.compact,
                        )?;
                        row.handle = Some(ItemHandle::Xray(item.handle));
                    } else {
                        row.handle = None;
                        cx.tree.set_visible(row.row, false)?;
                        cx.tree.set_visible(row.label, false)?;
                        cx.tree.set_visible(row.select, false)?;
                        cx.tree.set_visible(row.decrease, false)?;
                        cx.tree.set_visible(row.increase, false)?;
                    }
                }
                let pages = visible_groups
                    .len()
                    .saturating_add(page_size.saturating_sub(1))
                    .checked_div(page_size)
                    .unwrap_or(0);
                self.page = self.page.min(pages.saturating_sub(1));
                if let Some(id) = self.previous {
                    cx.tree.set_enabled(id, self.page > 0)?;
                }
                if let Some(id) = self.next {
                    cx.tree.set_enabled(id, self.page.saturating_add(1) < pages)?;
                }
                if let Some(id) = self.page_range {
                    let last = start.saturating_add(page_size).min(visible_groups.len());
                    let range = crate::strings::tr_in(
                        Some(crate::strings::current_language()),
                        "{0}–{1} из {2}",
                        &[&start.saturating_add(1), &last, &visible_groups.len()],
                    );
                    cx.tree
                        .set_text(id, if visible_groups.is_empty() { "" } else { &range })?;
                }
                let has_changes = pending_money != money
                    || !state.pending_durability.is_empty()
                    || !state.pending_placements.is_empty()
                    || !state.pending_upgrades.is_empty()
                    || !state.pending_removed.is_empty()
                    || !state.pending_adds.is_empty()
                    || !state.pending_xray_stash_takes.is_empty()
                    || !state.pending_xray_stash_puts.is_empty()
                    || !state.pending_faction_relations.is_empty()
                    || state.pending_relocation.is_some()
                    || state.pending_stacks.iter().any(|(handle, count)| {
                        if let ItemHandle::Xray(handle) = handle {
                            inventory
                                .iter()
                                .find(|item| item.handle == *handle)
                                .and_then(|item| item.count)
                                .is_some_and(|old| u32::from(old) != *count)
                        } else {
                            false
                        }
                    });
                if let Some(id) = self.export {
                    cx.tree.set_visible(id, has_changes)?;
                }
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        t("Изменения подготовлены. Сохранение создаст бэкап, запишет файл и повторно его прочитает."),
                    )?;
                }
            }
            SaveData::Stalker2 { save, inventory, .. } => {
                let writable = !save.index().is_legacy();
                let money = save.money();
                let pending_money = state.pending_money.unwrap_or(money);
                if let Some(id) = self.money_label {
                    cx.tree
                        .set_text(id, &tr!("Деньги: {pending_money}", pending_money = pending_money))?;
                }
                for (id, _) in &self.money_buttons {
                    cx.tree.set_visible(*id, true)?;
                    cx.tree.set_enabled(*id, writable)?;
                }
                money_input_state = Some((pending_money, writable));
                let query = self.search_query.to_lowercase();
                let visible_items: Vec<&S2InventoryItem> = inventory
                    .iter()
                    .filter(|item| {
                        let key = format!(
                            "{:02x}{:02x}{:02x}",
                            item.type_key[0], item.type_key[1], item.type_key[2]
                        );
                        (self.selected_category == "ВСЕ"
                            || s2_inventory_category(item.kind_code, item.display_name.as_deref())
                                == self.selected_category)
                            && search_matches(&format!("{} {key}", item.display_name.as_deref().unwrap_or("")), &query)
                    })
                    .collect();
                let visible_groups = group_s2_items(&visible_items, &state);
                self.update_inventory_filters(cx, visible_groups.len())?;
                if !self.selected_item.is_some_and(|selected| {
                    visible_items
                        .iter()
                        .any(|item| selected == ItemHandle::Stalker2(item.handle))
                }) {
                    self.selected_item = visible_items
                        .iter()
                        .find(|item| item.condition.is_some())
                        .or_else(|| visible_items.first())
                        .map(|item| ItemHandle::Stalker2(item.handle));
                }
                let start = self.page.saturating_mul(page_size);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(group) = visible_groups
                        .get(start.saturating_add(offset))
                        .filter(|_| offset < page_size)
                    {
                        let Some(item) = group.first().copied() else {
                            continue;
                        };
                        cx.tree.set_visible(row.row, true)?;
                        let name = item.display_name.as_deref().map(t).unwrap_or(t("Неизвестный предмет"));
                        let count = if item.editable_count {
                            state
                                .pending_stacks
                                .get(&ItemHandle::Stalker2(item.handle))
                                .copied()
                                .unwrap_or(item.count)
                                .to_string()
                        } else {
                            group
                                .iter()
                                .fold(0_u64, |total, item| total.saturating_add(u64::from(item.count)))
                                .to_string()
                        };
                        let key = format!(
                            "{:02x}{:02x}{:02x}",
                            item.type_key[0], item.type_key[1], item.type_key[2]
                        );
                        let condition_ratio = item.condition.map(|value| value / 100.0);
                        let condition = item.condition.map_or_else(String::new, |value| format!("{value:.0}%"));
                        let place = placement_cell(
                            &state,
                            ItemHandle::Stalker2(item.handle),
                            (item.x.is_some() && item.y.is_some()).then_some("ruck"),
                        );
                        let selected = self.selected_item == Some(ItemHandle::Stalker2(item.handle));
                        let editable = writable && item.editable_count;
                        fill_item_row(
                            cx.tree,
                            row,
                            &ItemCells {
                                name: &short_text(name, 18),
                                key: &key,
                                place: &place,
                                condition: &condition,
                                condition_ratio,
                                count: &count,
                                selected,
                                editable,
                            },
                            self.compact,
                        )?;
                        row.handle = Some(ItemHandle::Stalker2(item.handle));
                    } else {
                        cx.tree.set_visible(row.row, false)?;
                        cx.tree.set_visible(row.label, false)?;
                        cx.tree.set_visible(row.select, false)?;
                        cx.tree.set_visible(row.decrease, false)?;
                        cx.tree.set_visible(row.increase, false)?;
                        row.handle = None;
                    }
                }
                let pages = visible_groups
                    .len()
                    .saturating_add(page_size.saturating_sub(1))
                    .checked_div(page_size)
                    .unwrap_or(0);
                self.page = self.page.min(pages.saturating_sub(1));
                if let Some(id) = self.previous {
                    cx.tree.set_enabled(id, self.page > 0)?;
                }
                if let Some(id) = self.next {
                    cx.tree.set_enabled(id, self.page.saturating_add(1) < pages)?;
                }
                if let Some(id) = self.page_range {
                    let last = start.saturating_add(page_size).min(visible_groups.len());
                    let range = crate::strings::tr_in(
                        Some(crate::strings::current_language()),
                        "{0}–{1} из {2}",
                        &[&start.saturating_add(1), &last, &visible_groups.len()],
                    );
                    cx.tree
                        .set_text(id, if visible_groups.is_empty() { "" } else { &range })?;
                }
                let has_changes = pending_money != money
                    || !state.pending_durability.is_empty()
                    || state.pending_stacks.iter().any(|(handle, count)| {
                        matches!(handle, ItemHandle::Stalker2(_))
                            && inventory
                                .iter()
                                .find(|item| *handle == ItemHandle::Stalker2(item.handle))
                                .is_some_and(|item| item.count != *count)
                    })
                    || !state.pending_stash_moves.is_empty()
                    || !state.pending_xray_stash_takes.is_empty()
                    || !state.pending_xray_stash_puts.is_empty()
                    || !state.pending_faction_relations.is_empty()
                    || state.pending_relocation.is_some();
                let blocked_stash_draft = !S2_STASH_MOVE_ENABLED && !state.pending_stash_moves.is_empty();
                if let Some(id) = self.export {
                    cx.tree
                        .set_visible(id, writable && has_changes && !blocked_stash_draft)?;
                    cx.tree.set_text(id, t("Сохранить"))?;
                }
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        if blocked_stash_draft {
                            t("Черновик содержит перенос S2 из тайника, отключённый до проверки в игре. Сбросьте этот черновик, чтобы продолжить.")
                        } else if writable {
                            t("Изменения сохраняются с резервной копией и проверкой повторным чтением.")
                        } else {
                            S2_LEGACY_EDIT_REFUSAL
                        },
                    )?;
                }
            }
        }
        drop(state);
        self.update_search_input(cx)?;
        if let Some((value, enabled)) = money_input_state {
            self.update_money_input(cx, value, enabled)?;
        }
        self.render_inspector(cx, &selected_for_inspector)?;
        self.render_add_panel(cx)?;
        Ok(())
    }

    fn render_inspector(&mut self, cx: &mut Context<'_>, selected: &LoadedSave) -> Result<()> {
        let state = self.workspace.lock();
        let Some(handle) = self.selected_item else {
            if let Some(id) = self.inspector_summary {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(
                    id,
                    t("Предмет не выбран\nВыберите предмет для редактирования характеристик."),
                )?;
            }
            if let Some(id) = self.inspector_condition {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, t("Прочность: —"))?;
            }
            if let Some(id) = self.inspector_placement {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, t("Размещение: —"))?;
            }
            if let Some(id) = self.inspector_upgrades {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, t("Модификации: —"))?;
            }
            for (id, _) in &self.condition_buttons {
                cx.tree.set_visible(*id, false)?;
            }
            if let Some(id) = self.add_button {
                cx.tree.set_enabled(id, false)?;
            }
            return Ok(());
        };

        let (
            summary,
            condition,
            mut placement,
            upgrades,
            condition_editable,
            placement_editable,
            belt_allowed,
            remove_editable,
        ) = match (&selected.data, handle) {
            (SaveData::Xray { save, inventory }, ItemHandle::Xray(item_handle)) => {
                let Some(item) = inventory.iter().find(|item| item.handle == item_handle) else {
                    return Ok(());
                };
                let count = item.count.map_or_else(
                    || "—".to_owned(),
                    |count| {
                        state
                            .pending_stacks
                            .get(&handle)
                            .copied()
                            .unwrap_or(u32::from(count))
                            .to_string()
                    },
                );
                let upgrades = writer::current_upgrades(save, item_handle)
                    .map(|values| values.join(", "))
                    .unwrap_or_else(|_| "—".to_owned());
                (
                    tr(
                        "{0}\nКлюч: {1}\nКоличество в пачке: {2}",
                        &[
                            &t(&sse_catalog::SaveNaming::item_name(
                                save.format().id(),
                                &item.section,
                                None,
                            )),
                            &item.section,
                            &count,
                        ],
                    ),
                    item.condition,
                    item.placement
                        .clone()
                        .map(|placement| t(&placement).to_owned())
                        .unwrap_or_else(|| t("Размещение не прочитано").to_owned()),
                    upgrades,
                    item.durability_editable
                        && writer::capability(save.format(), writer::ChangeKind::EditDurability)
                            != writer::Capability::Unsupported,
                    item.placement.is_some()
                        && writer::capability(save.format(), writer::ChangeKind::EditPlacement)
                            != writer::Capability::Unsupported,
                    false,
                    writer::capability(save.format(), writer::ChangeKind::RemoveItems)
                        != writer::Capability::Unsupported
                        && item.placement.as_deref() != Some("slot"),
                )
            }
            (SaveData::Stalker2 { save, inventory, .. }, ItemHandle::Stalker2(item_handle)) => {
                let Some(item) = inventory.iter().find(|item| item.handle == item_handle) else {
                    return Ok(());
                };
                let placement = match (item.x, item.y) {
                    (Some(x), Some(y)) => tr("Рюкзак: столбец {0}, строка {1}", &[&x, &y]),
                    _ => t("Размещение не прочитано").to_owned(),
                };
                (
                    tr(
                        "{0}\nКлюч: {1}\nКоличество в пачке: {2}",
                        &[
                            &t(item.display_name.as_deref().unwrap_or("Неизвестный предмет")),
                            &format!(
                                "{:02x}{:02x}{:02x}",
                                item.type_key[0], item.type_key[1], item.type_key[2]
                            ),
                            &state.pending_stacks.get(&handle).copied().unwrap_or(item.count),
                        ],
                    ),
                    item.condition,
                    placement,
                    item.upgrades.join(", "),
                    !save.index().is_legacy() && item.condition.is_some(),
                    false,
                    false,
                    false,
                )
            }
            _ => {
                if let Some(id) = self.inspector_summary {
                    cx.tree
                        .set_text(id, t("Выбранный предмет отсутствует в текущем сейве."))?;
                }
                for (id, _) in &self.condition_buttons {
                    cx.tree.set_visible(*id, false)?;
                }
                return Ok(());
            }
        };

        if let Some(id) = self.inspector_summary {
            cx.tree.set_visible(id, true)?;
            cx.tree.set_text(id, &summary)?;
        }
        if let Some(id) = self.inspector_condition {
            cx.tree.set_visible(id, true)?;
            let shown = condition
                .map(|value| {
                    state
                        .pending_durability
                        .get(&handle)
                        .map_or_else(|| format!("{:.0}%", value * 100.0), |pending| format!("{pending}%"))
                })
                .unwrap_or_else(|| t("нет шкалы состояния / износа").to_owned());
            cx.tree
                .set_text(id, &tr!("Состояние / прочность: {shown}", shown = shown))?;
        }
        if let Some(id) = self.inspector_placement {
            cx.tree.set_visible(id, true)?;
            if let Some(pending) = state.pending_placements.get(&handle) {
                placement = match pending {
                    DraftPlacement::Ruck => t("Рюкзак").to_owned(),
                    DraftPlacement::Belt => t("Пояс").to_owned(),
                    DraftPlacement::Slot(slot) => tr("Слот {0}", &[slot]),
                };
            }
            cx.tree
                .set_text(id, &tr!("Размещение: {placement}", placement = placement))?;
        }
        let upgrades = state
            .pending_upgrades
            .get(&handle)
            .map(|values| values.join(", "))
            .unwrap_or(upgrades);
        if let Some(id) = self.inspector_upgrades {
            cx.tree.set_visible(id, true)?;
            cx.tree.set_text(
                id,
                &tr!(
                    "Модификации: {upgrades}",
                    upgrades = if upgrades.is_empty() { "—" } else { &upgrades }
                ),
            )?;
        }
        for (id, _) in &self.condition_buttons {
            cx.tree.set_visible(*id, condition.is_some())?;
            cx.tree.set_enabled(*id, condition_editable)?;
        }
        for (id, destination) in &self.placement_buttons {
            cx.tree.set_visible(*id, true)?;
            let allowed = placement_editable && (*destination != DraftPlacement::Belt || belt_allowed);
            cx.tree.set_enabled(*id, allowed)?;
        }
        if let Some(id) = self.remove_button {
            cx.tree.set_visible(id, true)?;
            cx.tree.set_enabled(id, remove_editable)?;
        }
        if let Some(id) = self.add_button {
            cx.tree.set_visible(id, true)?;
            let add_enabled = match &selected.data {
                SaveData::Xray { save, .. } => {
                    writer::capability(save.format(), writer::ChangeKind::AddItems) != writer::Capability::Unsupported
                }
                _ => false,
            };
            cx.tree.set_enabled(id, add_enabled)?;
        }
        let (upgrade_options, selected_upgrades, upgrades_editable) = match (&selected.data, handle) {
            (SaveData::Xray { save, inventory }, ItemHandle::Xray(item_handle)) => {
                let item = inventory.iter().find(|item| item.handle == item_handle);
                let current = writer::current_upgrades(save, item_handle).ok();
                let selected = state
                    .pending_upgrades
                    .get(&handle)
                    .cloned()
                    .or_else(|| current.clone())
                    .unwrap_or_default();
                let options = item
                    .and_then(|item| {
                        sse_catalog::CatalogBundleReader::load_embedded()
                            .get(save.format().id())
                            .and_then(|bundle| bundle.upgrades.as_ref())
                            .map(|catalog| catalog.for_item(&item.section))
                    })
                    .unwrap_or_default()
                    .into_iter()
                    .take(MAXIMUM_UPGRADE_ROWS)
                    .map(|upgrade| {
                        let label = sse_catalog::SaveNaming::upgrade_name(
                            Some(save.format().id()),
                            &upgrade.key,
                            upgrade.display_name.as_deref(),
                        );
                        let effect = upgrade.property_name.as_deref().unwrap_or(&label);
                        (upgrade.key.clone(), format!("{effect} · {label}"))
                    })
                    .collect::<Vec<_>>();
                let editable = current.is_some()
                    && writer::capability(save.format(), writer::ChangeKind::EditUpgrades)
                        != writer::Capability::Unsupported;
                (options, selected, editable)
            }
            _ => (Vec::new(), Vec::new(), false),
        };
        for (index, control) in self.upgrade_controls.iter_mut().enumerate() {
            let Some((key, label)) = upgrade_options.get(index) else {
                control.key = None;
                cx.tree.set_visible(control.widget, false)?;
                continue;
            };
            control.key = Some(key.clone());
            let checked = selected_upgrades.contains(key);
            cx.tree
                .set_text(control.widget, &format!("{} {label}", if checked { "✓" } else { "□" }))?;
            cx.tree.set_look(control.widget, style::nav(checked))?;
            cx.tree.set_visible(control.widget, true)?;
            cx.tree.set_enabled(control.widget, upgrades_editable)?;
        }
        Ok(())
    }

    fn set_edit_controls(&self, cx: &mut Context<'_>, visible: bool) -> Result<()> {
        for id in self
            .money_buttons
            .iter()
            .map(|(id, _)| *id)
            .chain(self.money_input_widget)
            .chain(self.search_widget)
            .chain(self.clear_search)
            .chain(self.search_count)
            .chain(self.empty_results)
            .chain(self.reset_filters)
            .chain(self.export)
            .chain(self.inspector_summary)
            .chain(self.inspector_condition)
            .chain(self.inspector_placement)
            .chain(self.inspector_upgrades)
            .chain(self.remove_button)
            .chain(self.add_button)
        {
            cx.tree.set_visible(id, visible)?;
        }
        for (id, _) in &self.categories {
            cx.tree.set_visible(*id, visible)?;
        }
        for row in &self.rows {
            cx.tree.set_visible(row.row, visible)?;
            cx.tree.set_visible(row.decrease, visible)?;
            cx.tree.set_visible(row.increase, visible)?;
            cx.tree.set_visible(row.label, visible)?;
            cx.tree.set_visible(row.select, visible)?;
        }
        for (id, _) in &self.condition_buttons {
            cx.tree.set_visible(*id, visible)?;
        }
        for (id, _) in &self.placement_buttons {
            cx.tree.set_visible(*id, visible)?;
        }
        for control in &self.upgrade_controls {
            cx.tree.set_visible(control.widget, visible)?;
        }
        if let Some(id) = self.previous {
            cx.tree.set_visible(id, visible)?;
        }
        if let Some(id) = self.next {
            cx.tree.set_visible(id, visible)?;
        }
        Ok(())
    }

    fn update_money_input(&mut self, cx: &mut Context<'_>, value: u32, enabled: bool) -> Result<()> {
        let Some(widget) = self.money_input_widget else {
            return Ok(());
        };
        let focused = self.money_input.as_mut().is_some_and(|input| {
            input.focus(cx.tree.focused() == Some(widget), 0);
            input.focused()
        });
        if !focused && !cx.app.has_invalid_numeric_input() {
            let text = value.to_string();
            self.money_input = Some(TextInput::new(&text, money_input_config())?);
            cx.tree.set_text(widget, &text)?;
        }
        cx.tree.set_enabled(widget, enabled)?;
        cx.tree.set_visible(widget, true)?;
        Ok(())
    }

    fn update_inventory_filters(&self, cx: &mut Context<'_>, count: usize) -> Result<()> {
        if let Some(id) = self.search_count {
            cx.tree.set_text(id, &tr!("Найдено: {count}", count = count))?;
        }
        if let Some(id) = self.clear_search {
            cx.tree.set_enabled(id, !self.search_query.is_empty())?;
        }
        if let Some(id) = self.empty_results {
            cx.tree.set_visible(id, count == 0)?;
        }
        if let Some(id) = self.reset_filters {
            cx.tree.set_visible(id, count == 0)?;
        }
        for (id, category) in &self.categories {
            cx.tree.set_look(*id, style::nav(self.selected_category == *category))?;
        }
        Ok(())
    }

    fn update_search_input(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let Some(widget) = self.search_widget else {
            return Ok(());
        };
        let focused = self.search.as_mut().is_some_and(|input| {
            input.focus(cx.tree.focused() == Some(widget), 0);
            input.focused()
        });
        if !focused {
            self.search = Some(TextInput::new(&self.search_query, inventory_search_config())?);
            cx.tree.set_text(widget, &self.search_query)?;
        }
        Ok(())
    }

    fn stage_money(&self, cx: &mut Context<'_>, delta: i64) -> Result<()> {
        let (selected, pending_money) = {
            let state = self.workspace.lock();
            (state.selected.clone(), state.pending_money)
        };
        let Some(selected) = selected else {
            return Ok(());
        };
        let current_money = match &selected.data {
            SaveData::Xray { save, .. } => save.money().ok(),
            SaveData::Stalker2 { save, .. } if !save.index().is_legacy() => Some(save.money()),
            SaveData::Stalker2 { .. } => None,
        };
        let Some(current_money) = current_money else {
            return Ok(());
        };
        let source_sha256 = selected.source_sha256.as_str();
        let plan = cx.app.draft(source_sha256);
        let current = if let Some(input) = self.money_input.as_ref() {
            let typed = input.text();
            match typed.parse::<u32>() {
                Ok(value) if value <= 2_000_000_000 => value,
                _ => {
                    cx.app.set_invalid_numeric_input(true);
                    cx.status = Some(t("Введены некорректные значения (проверьте введённые числа).").to_owned());
                    return Ok(());
                }
            }
        } else {
            pending_money
                .or_else(|| plan.and_then(|plan| plan.money))
                .unwrap_or(current_money)
        };
        let next = if delta >= 0 {
            current
                .saturating_add(u32::try_from(delta).unwrap_or(u32::MAX))
                .min(2_000_000_000)
        } else {
            current.saturating_sub(u32::try_from(delta.unsigned_abs()).unwrap_or(u32::MAX))
        };
        if next == current {
            return Ok(());
        }
        self.stage_money_value(cx, next)
    }

    fn stage_money_value(&self, cx: &mut Context<'_>, value: u32) -> Result<()> {
        cx.app.set_invalid_numeric_input(false);
        if value > 2_000_000_000 {
            cx.status = Some(t("Введены некорректные значения (проверьте введённые числа).").to_owned());
            return Ok(());
        }
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let current_money = match &selected.data {
            SaveData::Xray { save, .. } => save.money().ok(),
            SaveData::Stalker2 { save, .. } if !save.index().is_legacy() => Some(save.money()),
            SaveData::Stalker2 { .. } => None,
        };
        let Some(current_money) = current_money else {
            return Ok(());
        };
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        if plan.money.unwrap_or(current_money) == value {
            return Ok(());
        }
        plan.money = (value != current_money).then_some(value);
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn stage_stack(&self, cx: &mut Context<'_>, handle: ItemHandle, increase: bool) -> Result<()> {
        let (selected, pending_count) = {
            let state = self.workspace.lock();
            (state.selected.clone(), state.pending_stacks.get(&handle).copied())
        };
        let Some(selected) = selected else {
            return Ok(());
        };
        let original = match (&selected.data, handle) {
            (SaveData::Xray { inventory, .. }, ItemHandle::Xray(handle)) => inventory
                .iter()
                .find(|item| item.handle == handle)
                .and_then(|item| item.count)
                .map(u32::from),
            (SaveData::Stalker2 { inventory, .. }, ItemHandle::Stalker2(handle)) => inventory
                .iter()
                .find(|item| item.handle == handle && item.editable_count)
                .map(|item| item.count),
            _ => None,
        };
        let Some(original) = original else {
            return Ok(());
        };
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        let current = pending_count
            .or_else(|| plan.stack_counts.get(&handle.as_u32()).copied())
            .unwrap_or(original);
        let next = if increase {
            let maximum = match handle {
                ItemHandle::Xray(_) => u32::from(u16::MAX),
                ItemHandle::Stalker2(_) => 10_000_000,
            };
            current.saturating_add(1).min(maximum)
        } else {
            current.saturating_sub(1).max(1)
        };
        if next == current {
            return Ok(());
        }
        if next == original {
            plan.stack_counts.remove(&handle.as_u32());
        } else {
            plan.stack_counts.insert(handle.as_u32(), next);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn stage_durability(&self, cx: &mut Context<'_>, handle: ItemHandle, percent: u8) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let original = match (&selected.data, handle) {
            (SaveData::Xray { save, inventory }, ItemHandle::Xray(item_handle))
                if writer::capability(save.format(), writer::ChangeKind::EditDurability)
                    != writer::Capability::Unsupported =>
            {
                inventory
                    .iter()
                    .find(|item| item.handle == item_handle)
                    .and_then(|item| item.condition)
            }
            (SaveData::Stalker2 { save, inventory, .. }, ItemHandle::Stalker2(item_handle))
                if !save.index().is_legacy() =>
            {
                inventory
                    .iter()
                    .find(|item| item.handle == item_handle)
                    .and_then(|item| item.condition)
            }
            _ => None,
        };
        let Some(original) = original else {
            return Ok(());
        };
        let requested = f32::from(percent) / 100.0;
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        if (original - requested).abs() <= 0.005 {
            plan.durability.remove(&handle.as_u32());
        } else {
            plan.durability.insert(handle.as_u32(), percent);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn stage_placement(&self, cx: &mut Context<'_>, handle: ItemHandle, destination: DraftPlacement) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let current = match (&selected.data, handle) {
            (SaveData::Xray { save, inventory }, ItemHandle::Xray(item_handle))
                if writer::capability(save.format(), writer::ChangeKind::EditPlacement)
                    != writer::Capability::Unsupported =>
            {
                let Some(item) = inventory.iter().find(|item| item.handle == item_handle) else {
                    return Ok(());
                };
                let Some(current) = item.placement.as_deref() else {
                    return Ok(());
                };
                if destination == DraftPlacement::Belt && !item.section.to_ascii_lowercase().starts_with("af_") {
                    return Ok(());
                }
                current.to_owned()
            }
            _ => return Ok(()),
        };
        let requested_name = match destination {
            DraftPlacement::Ruck => "ruck",
            DraftPlacement::Belt => "belt",
            DraftPlacement::Slot(_) => return Ok(()),
        };
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        if current == requested_name {
            plan.placements.remove(&handle.as_u32());
        } else {
            plan.placements.insert(handle.as_u32(), destination);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn stage_remove(&self, cx: &mut Context<'_>, handle: ItemHandle) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let ItemHandle::Xray(item_handle) = handle else {
            return Ok(());
        };
        let SaveData::Xray { save, inventory } = &selected.data else {
            return Ok(());
        };
        if writer::capability(save.format(), writer::ChangeKind::RemoveItems) == writer::Capability::Unsupported
            || !inventory.iter().any(|item| item.handle == item_handle)
        {
            return Ok(());
        }
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        if !plan.detach_handles.contains(&item_handle) {
            plan.detach_handles.push(item_handle);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn open_add_panel(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (selected, removed) = {
            let state = self.workspace.lock();
            let removed = state
                .pending_removed
                .iter()
                .filter_map(|handle| match handle {
                    ItemHandle::Xray(handle) => Some(u32::from(*handle)),
                    ItemHandle::Stalker2(_) => None,
                })
                .collect::<BTreeSet<_>>();
            (state.selected.clone(), removed)
        };
        let Some(selected) = selected else {
            return Ok(());
        };
        self.add_candidates = add_candidates(&selected, &removed);
        self.add_search_query.clear();
        self.add_page = 0;
        self.add_previous_focus = cx.tree.focused();
        self.add_selected_key = self
            .add_candidates
            .iter()
            .find(|candidate| candidate.template_available)
            .map(|candidate| candidate.key.clone());
        self.add_search = Some(TextInput::new("", inventory_search_config())?);
        self.add_quantity = Some(TextInput::new("1", add_quantity_config())?);
        if let Some(inspector) = self.inspector_panel {
            cx.tree.set_visible(inspector, false)?;
        }
        if let Some(side) = self.side_column {
            cx.tree.set_style(side, side_column_style(self.compact, true))?;
        }
        // The frame's add branch has no remove/add-item bar: the panel takes its place too.
        if let Some(actions) = self.actions_panel {
            cx.tree.set_visible(actions, false)?;
        }
        if let Some(panel) = self.add_panel {
            cx.tree.set_style(panel, add_panel_style(0.0))?;
            cx.tree.set_visible(panel, true)?;
        }
        self.add_panel_open = true;
        if let Some(widget) = self.add_search_widget {
            cx.tree.set_text(widget, t(""))?;
            cx.tree.set_focus(Some(widget))?;
            if let Some(search) = self.add_search.as_mut() {
                search.focus(true, 0);
            }
        }
        if let Some(widget) = self.add_quantity_widget {
            cx.tree.set_text(widget, "1")?;
        }
        self.render_add_panel(cx)
    }

    fn close_add_panel(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.add_panel_open = false;
        if let Some(panel) = self.add_panel {
            cx.tree.set_visible(panel, false)?;
        }
        if let Some(inspector) = self.inspector_panel {
            cx.tree.set_visible(inspector, true)?;
        }
        if let Some(side) = self.side_column {
            cx.tree.set_style(side, side_column_style(self.compact, false))?;
        }
        if let Some(actions) = self.actions_panel {
            cx.tree.set_visible(actions, true)?;
        }
        if let Some(search) = self.add_search.as_mut() {
            search.focus(false, 0);
        }
        if let Some(quantity) = self.add_quantity.as_mut() {
            quantity.focus(false, 0);
        }
        let restore = self
            .add_previous_focus
            .take()
            .filter(|widget| cx.tree.is_visible(*widget));
        cx.tree.set_focus(restore)?;
        Ok(())
    }

    fn render_add_panel(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if !self.add_panel_open {
            return Ok(());
        }
        let query = self.add_search_query.to_lowercase();
        let matching = self
            .add_candidates
            .iter()
            .filter(|candidate| search_matches(&format!("{} {}", candidate.display_name, candidate.key), &query))
            .collect::<Vec<_>>();
        let pages = matching.len().saturating_add(ADD_ITEM_PAGE_SIZE.saturating_sub(1)) / ADD_ITEM_PAGE_SIZE;
        if pages == 0 || self.add_page >= pages {
            self.add_page = 0;
        }
        let start = self.add_page.saturating_mul(ADD_ITEM_PAGE_SIZE);
        if !self.add_selected_key.as_ref().is_some_and(|key| {
            matching
                .iter()
                .skip(start)
                .take(ADD_ITEM_PAGE_SIZE)
                .any(|candidate| candidate.key == *key && candidate.template_available)
        }) {
            self.add_selected_key = matching
                .iter()
                .skip(start)
                .take(ADD_ITEM_PAGE_SIZE)
                .find(|candidate| candidate.template_available)
                .map(|candidate| candidate.key.clone());
        }
        let slots = self.add_candidate_slots.clone();
        for (offset, (widget, key)) in self.add_candidate_rows.iter_mut().enumerate() {
            let Some((column, note)) = slots.get(offset).copied() else {
                continue;
            };
            if let Some(candidate) = matching.get(start.saturating_add(offset)) {
                *key = Some(candidate.key.clone());
                let suffix = if candidate.template_available {
                    String::new()
                } else {
                    t(" · нет подтверждённого шаблона в сейве").to_owned()
                };
                // The name is shortened to the button's width; the key and reason wrap under it.
                let width = add_candidate_text_width(self.compact);
                let name = fit_text(cx.tree, &candidate.display_name, Text::Body.style(), width);
                cx.tree.set_text(*widget, &name)?;
                let key_line = fit_text(
                    cx.tree,
                    &format!("{}{suffix}", candidate.key),
                    Text::Note.style(),
                    width,
                );
                cx.tree.set_text(note, &key_line)?;
                cx.tree.set_visible(column, true)?;
                cx.tree.set_enabled(*widget, candidate.template_available)?;
                cx.tree.set_look(
                    *widget,
                    style::nav(self.add_selected_key.as_deref() == Some(&candidate.key)),
                )?;
            } else {
                *key = None;
                cx.tree.set_visible(column, false)?;
            }
        }
        if let Some(empty) = self.add_empty {
            cx.tree.set_visible(empty, matching.is_empty())?;
            cx.tree.set_text(empty, t("Предметы не найдены."))?;
        }
        let selected = self
            .add_selected_key
            .as_ref()
            .and_then(|key| matching.iter().find(|candidate| candidate.key == *key).copied());
        if let (Some(name), Some(key_label)) = (self.add_selected_name, self.add_selected_key_label) {
            cx.tree.set_visible(name, selected.is_some())?;
            cx.tree.set_visible(key_label, selected.is_some())?;
            if let Some(candidate) = selected {
                cx.tree.set_text(name, &candidate.display_name)?;
                cx.tree.set_text(key_label, &candidate.key)?;
            }
        }
        if let Some(previous) = self.add_previous {
            cx.tree.set_enabled(previous, self.add_page > 0)?;
        }
        if let Some(next) = self.add_next {
            cx.tree.set_enabled(next, self.add_page.saturating_add(1) < pages)?;
        }
        if let Some(range) = self.add_page_range {
            let text = if matching.is_empty() {
                String::new()
            } else {
                let last = start.saturating_add(ADD_ITEM_PAGE_SIZE).min(matching.len());
                crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "{0}–{1} из {2}",
                    &[&start.saturating_add(1), &last, &matching.len()],
                )
            };
            cx.tree.set_text(range, &text)?;
        }
        let has_template = self.add_selected_key.as_ref().is_some_and(|key| {
            matching
                .iter()
                .any(|candidate| candidate.key == *key && candidate.template_available)
        });
        if let Some(confirm) = self.add_confirm {
            cx.tree.set_enabled(confirm, has_template)?;
        }
        self.sync_add_list_cap(cx)?;
        if let (Some(widget), Some(quantity)) = (self.add_quantity_widget, self.add_quantity.as_ref()) {
            if !quantity.focused() {
                cx.tree.set_text(widget, &quantity.text())?;
            }
        }
        Ok(())
    }

    /// Caps the candidate list at the room the add panel has in the side column, so the list scrolls instead of
    /// pushing the panels below the window. The room comes from the window and from the other side-column children;
    /// the fixed rows of the panel are measured from their rectangles, which do not depend on the list.
    fn sync_add_list_cap(&self, cx: &mut Context<'_>) -> Result<()> {
        let (Some(scroll), Some(panel), Some(side)) = (self.add_scroll, self.add_panel, self.side_column) else {
            return Ok(());
        };
        if !cx.tree.is_visible(panel) {
            return Ok(());
        }
        // The fixed rows are measured from the current layout; the list's cap does not change them.
        cx.tree.update_layout()?;
        let window_height = px_i64(i64::from(cx.tree.size().1));
        let side_top = px_i64(i64::from(cx.tree.rect(side)?.y));
        // Everything in the side column but the panel (the hidden inspector included) takes room from the window.
        let mut others = 0.0_f32;
        for child in &cx.tree.children(side) {
            if *child != panel {
                others += px_i64(i64::from(cx.tree.rect(*child)?.height));
            }
        }
        // The shell keeps 12 px below the content and a 30 px status bar under the window.
        let side_room = window_height - side_top - 42.0 - others;
        let inner = side_room - 2.0 * crate::theme::d2::PANEL_PADDING.0;
        let children = cx.tree.children(panel);
        let mut fixed = 0.0_f32;
        for child in &children {
            if *child == scroll || Some(*child) == self.add_filler {
                continue;
            }
            fixed += px_i64(i64::from(cx.tree.rect(*child)?.height));
        }
        let gaps = px_i64(i64::try_from(children.len().saturating_sub(1)).unwrap_or(0)) * ADD_PANEL_GAP;
        let cap = (inner - fixed - gaps).max(0.0);
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

    fn stage_add_key(&self, cx: &mut Context<'_>, item_key: &str, quantity: u32) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let SaveData::Xray { save, inventory } = &selected.data else {
            cx.status = Some(t("Добавление доступно только для подтверждённых X-Ray форматов.").to_owned());
            return Ok(());
        };
        if writer::capability(save.format(), writer::ChangeKind::AddItems) == writer::Capability::Unsupported
            || sse_catalog::CatalogBundleReader::load_embedded()
                .get(save.format().id())
                .and_then(|bundle| bundle.items.resolve(item_key))
                .is_none()
            || !inventory.iter().any(|item| item.section == item_key)
        {
            cx.status = Some(t("Для этого предмета или формата нет подтверждённого шаблона добавления.").to_owned());
            return Ok(());
        }
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        plan.adds.push(AddRequest::new(item_key, quantity, "inventory")?);
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        cx.status = Some(tr!(
            "Предмет {item_key} ({quantity} шт.) добавлен в очередь на запись.",
            item_key = item_key,
            quantity = quantity
        ));
        Ok(())
    }

    fn stage_upgrade(&self, cx: &mut Context<'_>, handle: ItemHandle, upgrade_key: &str) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let (save, item) = match (&selected.data, handle) {
            (SaveData::Xray { save, inventory }, ItemHandle::Xray(item_handle)) => {
                let Some(item) = inventory.iter().find(|item| item.handle == item_handle) else {
                    return Ok(());
                };
                (save, item)
            }
            _ => return Ok(()),
        };
        if writer::capability(save.format(), writer::ChangeKind::EditUpgrades) == writer::Capability::Unsupported
            || !sse_catalog::CatalogBundleReader::load_embedded()
                .get(save.format().id())
                .and_then(|bundle| bundle.upgrades.as_ref())
                .is_some_and(|catalog| {
                    catalog
                        .for_item(&item.section)
                        .iter()
                        .any(|upgrade| upgrade.key == upgrade_key)
                })
        {
            return Ok(());
        }
        let mut values = self
            .workspace
            .lock()
            .pending_upgrades
            .get(&handle)
            .cloned()
            .unwrap_or(writer::current_upgrades(save, item.handle)?);
        if let Some(index) = values.iter().position(|value| value == upgrade_key) {
            values.remove(index);
        } else {
            values.push(upgrade_key.to_owned());
        }
        let current = writer::current_upgrades(save, item.handle)?;
        let source_sha256 = selected.source_sha256.as_str();
        let mut plan = cx
            .app
            .draft(source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(source_sha256)?);
        if values == current {
            plan.upgrades.remove(&u32::from(item.handle));
        } else {
            plan.upgrades.insert(u32::from(item.handle), values);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after editing".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        Ok(())
    }

    fn save(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.pending_save_request.is_some() {
            cx.status = Some(t("Проверка запущенной игры уже выполняется.").to_owned());
            return Ok(());
        }
        let (selected, edits, stash_moves) = {
            let state = self.workspace.lock();
            (
                state.selected.clone(),
                PendingInventoryEdits {
                    money: state.pending_money,
                    stacks: state.pending_stacks.clone(),
                    durability: state.pending_durability.clone(),
                    placements: state.pending_placements.clone(),
                    upgrades: state.pending_upgrades.clone(),
                    removals: state.pending_removed.clone(),
                    adds: state.pending_adds.clone(),
                    stash_takes: state.pending_xray_stash_takes.clone(),
                    stash_puts: state.pending_xray_stash_puts.clone(),
                    faction_relations: state.pending_faction_relations.clone(),
                    relocate_to: state.pending_relocation,
                },
                state.pending_stash_moves.clone(),
            )
        };
        let Some(selected) = selected else {
            cx.status = Some(t("Сначала выберите сейв.").to_owned());
            return Ok(());
        };
        if !S2_STASH_MOVE_ENABLED && !stash_moves.is_empty() {
            let text = t("Черновик содержит перенос S2 из тайника, отключённый до проверки в игре. Сбросьте этот черновик, чтобы продолжить.");
            if let Some(status) = self.status {
                cx.tree.set_text(status, text)?;
            }
            cx.status = Some(text.to_owned());
            return Ok(());
        }
        if cx.app.has_invalid_numeric_input() {
            cx.status = Some(t("Введены некорректные значения (проверьте введённые числа).").to_owned());
            return Ok(());
        }
        if !cx.app.has_draft(&selected.source_sha256) && !edits.has_changes() && stash_moves.is_empty() {
            cx.status = Some(t("Нет несохранённых изменений.").to_owned());
            return Ok(());
        }
        if let Some(plan) = cx.app.draft(&selected.source_sha256) {
            if plan.unmapped_legacy_plan.is_some() {
                cx.status = Some(t("В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом).").to_owned());
                return Ok(());
            }
        }
        if self.workspace.is_saving() || self.workspace.is_restoring() {
            let text = if self.workspace.is_restoring() {
                t("Дождитесь завершения восстановления сейва.")
            } else {
                t("Сохранение уже выполняется.")
            };
            if let Some(status) = self.status {
                cx.tree.set_text(status, text)?;
            }
            cx.status = Some(text.to_owned());
            return Ok(());
        }
        let request = PendingSaveRequest {
            selected,
            edits,
            stash_moves,
        };
        if self.workspace.is_browser_file_mode() {
            return self.save_browser_copy(cx, request);
        }
        self.start_save_process_check(cx, request)
    }

    fn save_browser_copy(&mut self, cx: &mut Context<'_>, request: PendingSaveRequest) -> Result<()> {
        let source_sha256 = request.selected.source_sha256.clone();
        let (reloaded, download) = match prepare_browser_save(&request.selected, &request.edits, &request.stash_moves) {
            Ok(prepared) => prepared,
            Err(error) => {
                let message = tr("Не удалось сохранить копию в браузере: {0}", &[&error]);
                if let Some(status) = self.status {
                    cx.tree.set_text(status, &message)?;
                }
                cx.status = Some(message);
                return Ok(());
            }
        };
        if let Err(error) = self.workspace.queue_browser_download(download) {
            let message = tr("Не удалось начать скачивание копии: {0}", &[&error]);
            if let Some(status) = self.status {
                cx.tree.set_text(status, &message)?;
            }
            cx.status = Some(message);
            return Ok(());
        }
        let new_source_sha256 = reloaded.source_sha256.clone();
        let journal = DraftJournal::new(vec![DraftPlan::empty(&new_source_sha256)?], 0)?;
        {
            let mut state = self.workspace.lock();
            state.selected = Some(Arc::clone(&reloaded));
            state.pending_money = None;
            state.pending_stacks.clear();
            state.pending_durability.clear();
            state.pending_placements.clear();
            state.pending_upgrades.clear();
            state.pending_removed.clear();
            state.pending_adds.clear();
            state.pending_stash_moves.clear();
            state.pending_xray_stash_takes.clear();
            state.pending_xray_stash_puts.clear();
            state.pending_faction_relations.clear();
            state.pending_relocation = None;
            state.external_change = false;
        }
        cx.app.discard_draft(&source_sha256);
        cx.app
            .set_current_save_identity(reloaded.slot.path.clone(), new_source_sha256.clone());
        let legacy_s2 = matches!(&reloaded.data, SaveData::Stalker2 { save, .. } if save.index().is_legacy());
        cx.app.set_selected_game(reloaded.slot.game_id.clone());
        cx.app
            .set_current_save_format(reloaded.slot.format_id.clone(), legacy_s2);
        cx.app.set_draft_journal(journal.clone());
        set_workspace_draft(&self.workspace, &journal);
        let message = crate::strings::t("Копия подготовлена для скачивания; исходный файл не изменён.").to_owned();
        if let Some(status) = self.status {
            cx.tree.set_text(status, &message)?;
        }
        cx.status = Some(message);
        self.render(cx)
    }

    fn start_save_process_check(&mut self, cx: &mut Context<'_>, request: PendingSaveRequest) -> Result<()> {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some(t("Сохранение доступно в работающем окне редактора.").to_owned());
            return Ok(());
        };
        let Some(format_id) = request.selected.slot.format_id.clone() else {
            cx.status = Some(t("Не удалось определить формат сейва для проверки запущенной игры.").to_owned());
            return Ok(());
        };
        let Some(request_id) = self.next_process_check_id.checked_add(1) else {
            cx.status = Some(t("Исчерпан номер проверки запущенной игры.").to_owned());
            return Ok(());
        };
        self.next_process_check_id = request_id;
        self.pending_save_request = Some((request_id, request));
        self.process_check_complete = false;
        if let (Some(dialog), Some(description), Some(continue_button)) = (
            self.process_confirmation,
            self.process_description,
            self.process_continue,
        ) {
            cx.tree
                .set_text(description, t("Проверяю, запущена ли игра для выбранного сейва…"))?;
            cx.tree.set_text(continue_button, t("Проверка…"))?;
            cx.tree.set_enabled(continue_button, false)?;
            cx.tree.open_dialog(dialog)?;
        }
        if let Some(status) = self.status {
            cx.tree.set_text(status, t("Проверяю запущенную игру…"))?;
        }
        cx.status = Some(t("Проверяю запущенную игру…").to_owned());
        if let Err(error) = self.workspace.spawn("save-process-check", move |context| {
            let result = if context.is_cancelled() {
                Err("process check was cancelled".to_owned())
            } else {
                running_game_for_format(&format_id)
            };
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(SaveProcessCheckFinished { request_id, result }),
            ));
        }) {
            self.pending_save_request = None;
            self.process_check_complete = false;
            let _ = cx.tree.close_dialog()?;
            let text = tr!("Не удалось начать проверку запущенной игры: {error}", error = error);
            if let Some(status) = self.status {
                cx.tree.set_text(status, &text)?;
            }
            cx.status = Some(text);
        }
        Ok(())
    }

    fn start_save_write(&self, cx: &mut Context<'_>, request: PendingSaveRequest) -> Result<()> {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some(t("Сохранение доступно в работающем окне редактора.").to_owned());
            return Ok(());
        };
        let PendingSaveRequest {
            selected,
            edits,
            stash_moves,
        } = request;
        let source_sha256 = selected.source_sha256.clone();
        let session = self.workspace.session();
        let Some(operation_guard) = session.begin_save(&selected.slot.path) else {
            let text = if session.is_restoring() {
                t("Дождитесь завершения восстановления сейва.")
            } else {
                t("Сохранение уже выполняется.")
            };
            if let Some(status) = self.status {
                cx.tree.set_text(status, text)?;
            }
            cx.status = Some(text.to_owned());
            return Ok(());
        };
        let request_id = operation_guard.id();
        let draft_identity = DraftStore::for_source(self.workspace.draft_directory.as_path(), &selected.slot.path)
            .identity_key(&source_sha256)?;
        let draft_generation = session
            .draft_generation(&draft_identity)
            .or_else(|| session.next_draft_generation(&draft_identity));
        let Some(draft_generation) = draft_generation else {
            cx.status = Some(t("Не удалось назначить поколение черновика.").to_owned());
            return Ok(());
        };
        let source_path = selected.slot.path.clone();
        let log_path = source_path.clone();
        let backup_directory = self.workspace.backup_directory();
        if let Some(status) = self.status {
            cx.tree.set_text(status, t("Сохранение…"))?;
        }
        if let Err(error) = self.workspace.spawn("save-write", move |context| {
            let save_guard = operation_guard;
            let result = if context.is_cancelled() {
                sse_app::diagnostics::save_write_cancelled(&source_path);
                Err(t("Сохранение отменено.").to_owned())
            } else {
                let result = commit_save_edits(&selected, &edits, &stash_moves, &backup_directory)
                    .map_err(|error| error.to_string());
                match &result {
                    Ok(_) => sse_app::diagnostics::save_write_succeeded(&source_path),
                    Err(error) => sse_app::diagnostics::save_write_failed(&source_path, error),
                }
                result
            };
            drop(save_guard);
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(SaveFinished {
                    request_id,
                    draft_generation,
                    source_path,
                    draft_identity,
                    source_sha256,
                    result,
                }),
            ));
        }) {
            sse_app::diagnostics::save_write_failed(&log_path, &error.to_string());
            let text = tr!("Не удалось начать сохранение: {error}", error = error);
            if let Some(status) = self.status {
                cx.tree.set_text(status, &text)?;
            }
            cx.status = Some(text);
        }
        Ok(())
    }

    fn editor_action(&mut self, action: EditorAction, cx: &mut Context<'_>) -> Result<()> {
        if action == EditorAction::Save {
            return self.save(cx);
        }
        let Some(source_sha256) = cx.app.current_save_sha256().map(str::to_owned) else {
            cx.status = Some(t("Выберите сохранение для редактирования.").to_owned());
            return Ok(());
        };
        match action {
            EditorAction::Undo => cx.app.undo_draft(&source_sha256)?,
            EditorAction::Redo => cx.app.redo_draft(&source_sha256)?,
            EditorAction::Reset => {
                let preserve_unmapped = cx
                    .app
                    .draft(&source_sha256)
                    .is_some_and(|plan| plan.unmapped_legacy_plan.is_some());
                let empty = DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?;
                cx.app.set_invalid_numeric_input(false);
                cx.app.set_draft_journal(empty.clone());
                set_workspace_draft(&self.workspace, &empty);
                self.workspace.reset_draft(empty, preserve_unmapped, cx);
                cx.status = Some(t("Черновик сброшен.").to_owned());
                return self.render(cx);
            }
            EditorAction::Save => return self.save(cx),
        }
        if let Some(journal) = cx.app.draft_journal(&source_sha256).cloned() {
            set_workspace_draft(&self.workspace, &journal);
            self.workspace.persist_draft(journal, cx);
        }
        self.render(cx)
    }
}

struct SaveFinished {
    request_id: sse_app::SaveOperationId,
    draft_generation: sse_app::DraftGeneration,
    source_path: PathBuf,
    draft_identity: String,
    source_sha256: String,
    result: std::result::Result<(Arc<LoadedSave>, String), String>,
}

fn commit_save_edits(
    selected: &LoadedSave,
    edits: &PendingInventoryEdits,
    stash_moves: &BTreeSet<u32>,
    backup_directory: &Path,
) -> Result<(Arc<LoadedSave>, String)> {
    commit_save_edits_to(selected, edits, stash_moves, backup_directory)
}

fn commit_save_edits_to(
    selected: &LoadedSave,
    edits: &PendingInventoryEdits,
    stash_moves: &BTreeSet<u32>,
    backup_directory: &Path,
) -> Result<(Arc<LoadedSave>, String)> {
    let (packed, summary) = prepare_save_edits(selected, edits, stash_moves)?;
    // SaveBuffer clones share their Arc<[u8]>; move the one preflight handle into the reloaded save.
    let preflight_image = packed.clone();
    let request = transaction::ReplacementRequest::new(
        &selected.slot.path,
        &selected.source_sha256,
        packed.as_slice(),
        backup_directory,
    )
    .with_summary(summary);
    let (receipt, mut reloaded, (size, modified)) = transaction::replace_transaction(
        &transaction::StdFileSystem,
        request,
        move |_, replacement| {
            if replacement != preflight_image.as_slice() {
                return Err(Error::damaged("prepared save bytes changed before semantic preflight"));
            }
            let reloaded = LoadedSave::from_buffer(selected.slot.clone(), preflight_image)?;
            verify_requested_values(selected, &reloaded, edits, stash_moves)?;
            Ok(reloaded)
        },
        |read_back| {
            if read_back != packed.as_slice() {
                return Err(Error::damaged("save bytes differ after durable read-back"));
            }
            let metadata = std::fs::metadata(&selected.slot.path)?;
            Ok((
                metadata.len(),
                metadata.modified().unwrap_or(selected.slot.last_write_time_utc),
            ))
        },
    )?;
    reloaded.slot.size = size;
    reloaded.slot.last_write_time_utc = modified;
    reloaded.info = save_info(&reloaded.slot);
    let (crc_status, format) = match &reloaded.data {
        SaveData::Xray { save, .. } => (t("не подтверждается отдельным полем"), save.format().id()),
        SaveData::Stalker2 { save, .. } => (
            if save.container().stored_crc32() == save.container().computed_crc32() {
                "OK (CRC32)"
            } else {
                t("ошибка")
            },
            "S2",
        ),
    };
    reloaded.integrity = save_integrity(
        &reloaded.slot,
        &reloaded.source_sha256,
        packed.len(),
        crc_status,
        format,
    );
    let backup_name = receipt.backup_path.file_name().map_or_else(
        || receipt.backup_path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let mut save_message = tr("Сохранено успешно. Резервная копия: {0}", &[&backup_name]);
    if let Some(warning) = receipt.maintenance_warning.as_deref() {
        save_message.push_str(&tr(" Ротация старых копий не завершена: {0}", &[&warning]));
    }
    Ok((Arc::new(reloaded), save_message))
}

fn prepare_browser_save(
    selected: &LoadedSave,
    edits: &PendingInventoryEdits,
    stash_moves: &BTreeSet<u32>,
) -> Result<(Arc<LoadedSave>, BrowserDownload)> {
    let (packed, _) = prepare_save_edits(selected, edits, stash_moves)?;
    if u64::try_from(packed.len()).unwrap_or(u64::MAX) > MAX_BROWSER_SAVE_BYTES {
        return Err(Error::Refused(
            "edited save exceeds the browser download limit".to_owned(),
        ));
    }
    let mut reloaded = LoadedSave::from_buffer(selected.slot.clone(), packed.clone())?;
    verify_requested_values(selected, &reloaded, edits, stash_moves)?;
    reloaded.slot.size = u64::try_from(packed.len()).unwrap_or(u64::MAX);
    reloaded.info = save_info(&reloaded.slot);
    let (crc_status, format) = match &reloaded.data {
        SaveData::Xray { save, .. } => (t("не подтверждается отдельным полем"), save.format().id()),
        SaveData::Stalker2 { save, .. } => (
            if save.container().stored_crc32() == save.container().computed_crc32() {
                "OK (CRC32)"
            } else {
                t("ошибка")
            },
            "S2",
        ),
    };
    reloaded.integrity = save_integrity(
        &reloaded.slot,
        &reloaded.source_sha256,
        packed.len(),
        crc_status,
        format,
    );
    let file_name = browser_download_filename(&selected.slot.path);
    Ok((
        Arc::new(reloaded),
        BrowserDownload {
            file_name,
            bytes: packed,
        },
    ))
}

fn validate_browser_file_name(name: &str) -> Result<String> {
    if name.trim().is_empty()
        || name.len() > MAX_BROWSER_FILENAME_BYTES
        || name == "."
        || name == ".."
        || name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | ':'))
    {
        return Err(Error::Refused("browser save filename is invalid".to_owned()));
    }
    Ok(name.to_owned())
}

fn browser_download_filename(source: &Path) -> String {
    let stem = source
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("save");
    match source.extension().and_then(std::ffi::OsStr::to_str) {
        Some(extension) if !extension.is_empty() => format!("{stem}_edited.{extension}"),
        _ => format!("{stem}_edited.sav"),
    }
}

#[cfg(test)]
fn prepare_xray_edits(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<u16, u16>,
) -> Result<(SaveBuffer, EditSummary)> {
    let stacks = stacks
        .iter()
        .map(|(handle, count)| (ItemHandle::Xray(*handle), u32::from(*count)))
        .collect::<BTreeMap<_, _>>();
    prepare_save_edits(
        selected,
        &PendingInventoryEdits {
            money,
            stacks,
            ..PendingInventoryEdits::default()
        },
        &BTreeSet::new(),
    )
}

fn prepare_save_edits(
    selected: &LoadedSave,
    edits: &PendingInventoryEdits,
    stash_moves: &BTreeSet<u32>,
) -> Result<(SaveBuffer, EditSummary)> {
    match &selected.data {
        SaveData::Xray { save, inventory } => {
            if !stash_moves.is_empty() {
                return Err(Error::Refused(
                    "S2 stash changes cannot be applied to an X-Ray save".to_owned(),
                ));
            }
            if (!edits.stash_takes.is_empty() || !edits.stash_puts.is_empty())
                && writer::capability(save.format(), writer::ChangeKind::MoveItems) == writer::Capability::Unsupported
            {
                return Err(Error::Refused(
                    "stash transfers are unsupported for this X-Ray format".to_owned(),
                ));
            }
            let current_money = save.money()?;
            let money_change = edits.money.filter(|value| *value != current_money);
            let mut changes = Vec::new();
            let mut relation_count = 0_usize;
            let mut move_count = 0_usize;
            if let Some(new_value) = money_change {
                changes.push(writer::Change::SetMoney {
                    target_object: save.actor_id(),
                    old_value: current_money,
                    new_value,
                });
            }
            let mut stack_count = 0_usize;
            for (handle, new_value) in &edits.stacks {
                let ItemHandle::Xray(handle) = handle else {
                    return Err(Error::Refused(
                        "S2 stack edit cannot be applied to an X-Ray save".to_owned(),
                    ));
                };
                let Some(item) = inventory.iter().find(|item| item.handle == *handle) else {
                    return Err(Error::Refused("selected X-Ray stack no longer exists".to_owned()));
                };
                let Some(old_value) = item.count else {
                    return Err(Error::Refused("selected X-Ray stack has no confirmed count".to_owned()));
                };
                let new_value = u16::try_from(*new_value)
                    .map_err(|_| Error::Refused("X-Ray stack count exceeds its supported range".to_owned()))?;
                if old_value != new_value {
                    changes.push(writer::Change::SetStack {
                        target_object: *handle,
                        old_value,
                        new_value,
                    });
                    stack_count = stack_count.saturating_add(1);
                }
            }
            for (handle, percent) in &edits.durability {
                let ItemHandle::Xray(handle) = handle else {
                    return Err(Error::Refused(
                        "S2 durability edit cannot be applied to an X-Ray save".to_owned(),
                    ));
                };
                let Some(item) = inventory.iter().find(|item| item.handle == *handle) else {
                    return Err(Error::Refused(
                        "selected X-Ray durability item no longer exists".to_owned(),
                    ));
                };
                let Some(old_value) = item.condition else {
                    return Err(Error::Refused(
                        "selected X-Ray item has no confirmed condition field".to_owned(),
                    ));
                };
                if !item.durability_editable {
                    return Err(Error::Refused(
                        "selected X-Ray item has no proven UPDATE condition mirror".to_owned(),
                    ));
                }
                let new_value = f32::from(*percent) / 100.0;
                if (old_value - new_value).abs() > 0.005 {
                    changes.push(writer::Change::SetDurability {
                        target_object: *handle,
                        old_value,
                        new_value,
                    });
                }
            }
            for (handle, destination) in &edits.placements {
                let ItemHandle::Xray(handle) = handle else {
                    return Err(Error::Refused(
                        "S2 placement edit cannot be applied to an X-Ray save".to_owned(),
                    ));
                };
                if !inventory.iter().any(|item| item.handle == *handle) {
                    return Err(Error::Refused(
                        "selected X-Ray placement item no longer exists".to_owned(),
                    ));
                }
                let destination = match destination {
                    DraftPlacement::Ruck => writer::Placement::Ruck,
                    DraftPlacement::Belt => writer::Placement::Belt,
                    DraftPlacement::Slot(slot) => writer::Placement::Slot(*slot),
                };
                changes.push(writer::Change::SetPlacement {
                    target_object: *handle,
                    destination,
                });
            }
            for (handle, upgrades) in &edits.upgrades {
                let ItemHandle::Xray(handle) = handle else {
                    return Err(Error::Refused(
                        "S2 upgrade edit cannot be applied to an X-Ray save".to_owned(),
                    ));
                };
                let old_value = writer::current_upgrades(save, *handle)?;
                if old_value != *upgrades {
                    changes.push(writer::Change::SetUpgrades {
                        target_object: *handle,
                        old_value,
                        new_value: upgrades.clone(),
                    });
                }
            }
            for handle in &edits.removals {
                let ItemHandle::Xray(handle) = handle else {
                    return Err(Error::Refused(
                        "S2 removal cannot be applied to an X-Ray save".to_owned(),
                    ));
                };
                changes.push(writer::Change::RemoveItem { target_object: *handle });
            }
            if !edits.adds.is_empty()
                && writer::capability(save.format(), writer::ChangeKind::AddItems) == writer::Capability::Unsupported
            {
                return Err(Error::Refused(
                    "adding items is not supported for this X-Ray format".to_owned(),
                ));
            }
            let catalog = sse_catalog::CatalogBundleReader::load_embedded().get(save.format().id());
            let mut used_object_ids: BTreeSet<u16> =
                save.registry_objects().iter().map(|object| object.object_id).collect();
            for request in &edits.adds {
                if request.destination != "inventory" {
                    return Err(Error::Refused(
                        "only inventory item additions are supported here".to_owned(),
                    ));
                }
                if catalog
                    .and_then(|bundle| bundle.items.resolve(&request.item_key))
                    .is_none()
                {
                    return Err(Error::Refused(format!(
                        "item '{}' is absent from the release catalog",
                        request.item_key
                    )));
                }
                let template_object = inventory
                    .iter()
                    .filter(|item| {
                        item.section == request.item_key && !edits.removals.contains(&ItemHandle::Xray(item.handle))
                    })
                    .filter_map(|item| {
                        save.registry_objects()
                            .iter()
                            .find(|object| object.object_id == item.handle && object.parent_id == save.actor_id())
                    })
                    .min_by_key(|object| {
                        add_template_preference(
                            object.story_id,
                            object.spawn_story_id,
                            save.custom_data(object).is_some_and(|data| !data.is_empty()),
                            object.spawn_id,
                            object.object_id,
                        )
                    })
                    .ok_or_else(|| {
                        Error::Refused(format!(
                            "item '{}' has no matching actor-owned serialized template in this save",
                            request.item_key
                        ))
                    })?;
                let object_id = used_object_ids
                    .iter()
                    .next_back()
                    .copied()
                    .and_then(|maximum| maximum.checked_add(1))
                    .filter(|candidate| *candidate < u16::MAX && !used_object_ids.contains(candidate))
                    .or_else(|| (1..u16::MAX).find(|candidate| !used_object_ids.contains(candidate)))
                    .ok_or_else(|| Error::Refused("no free X-Ray registry object id remains".to_owned()))?;
                used_object_ids.insert(object_id);
                let quantity = u16::try_from(request.quantity)
                    .map_err(|_| Error::Refused("item quantity exceeds its supported range".to_owned()))?;
                changes.push(writer::Change::AddItem {
                    template_object: template_object.object_id,
                    item_key: request.item_key.clone(),
                    object_id,
                    quantity,
                });
            }
            if !edits.faction_relations.is_empty() {
                if writer::capability(save.format(), writer::ChangeKind::EditRelations)
                    == writer::Capability::Unsupported
                {
                    return Err(Error::Refused(
                        "faction relations are unsupported for this X-Ray format".to_owned(),
                    ));
                }
                let faction_catalog = catalog
                    .and_then(|bundle| bundle.factions.as_ref())
                    .ok_or_else(|| Error::Refused("faction catalog is unavailable for this save".to_owned()))?;
                let actor_relations = save
                    .actor_relations()
                    .ok_or_else(|| Error::Refused("actor relation row is unavailable in this save".to_owned()))?;
                for (faction_key, new_value) in &edits.faction_relations {
                    let faction = faction_catalog
                        .resolve(faction_key)
                        .map_err(|_| Error::Refused(format!("unknown faction key '{faction_key}'")))?;
                    let community_id = faction.numeric_id.ok_or_else(|| {
                        Error::Refused(format!("faction '{faction_key}' has no numeric community id"))
                    })?;
                    if actor_relations
                        .iter()
                        .find(|(id, _)| *id == community_id)
                        .is_some_and(|(_, old_value)| *old_value == *new_value)
                    {
                        continue;
                    }
                    changes.push(writer::Change::SetFactionRelation {
                        target_object: save.actor_id(),
                        faction_key: faction_key.clone(),
                        old_value: actor_relations
                            .iter()
                            .find(|(id, _)| *id == community_id)
                            .map(|(_, value)| *value),
                        new_value: *new_value,
                    });
                    relation_count = relation_count.saturating_add(1);
                }
            }
            for handle in &edits.stash_takes {
                let object = save
                    .registry_objects()
                    .iter()
                    .find(|object| object.object_id == *handle)
                    .ok_or_else(|| Error::Refused(format!("stash item 0x{handle:04X} is missing")))?;
                changes.push(writer::Change::MoveItem {
                    target_object: *handle,
                    old_parent: object.parent_id,
                    new_parent: save.actor_id(),
                });
                move_count = move_count.saturating_add(1);
            }
            for (handle, box_id) in &edits.stash_puts {
                changes.push(writer::Change::MoveItem {
                    target_object: *handle,
                    old_parent: save.actor_id(),
                    new_parent: *box_id,
                });
                move_count = move_count.saturating_add(1);
            }
            if let Some(destination_changer) = edits.relocate_to {
                if writer::capability(save.format(), writer::ChangeKind::RelocateActor)
                    == writer::Capability::Unsupported
                {
                    return Err(Error::Refused(
                        "actor relocation is unsupported for this X-Ray format".to_owned(),
                    ));
                }
                changes.push(writer::Change::RelocateActor { destination_changer });
            }
            if changes.is_empty() {
                return Err(Error::Refused("there are no inventory changes to save".to_owned()));
            }
            let packed = writer::apply(save, &writer::ChangeSet::new(changes))?;
            Ok((
                packed,
                EditSummary {
                    money: money_change,
                    stack_count,
                    move_count,
                    relation_count,
                    ..EditSummary::default()
                },
            ))
        }
        SaveData::Stalker2 {
            save,
            inventory,
            stash_items,
        } => {
            if !edits.stash_takes.is_empty()
                || !edits.stash_puts.is_empty()
                || !edits.faction_relations.is_empty()
                || edits.relocate_to.is_some()
            {
                return Err(Error::Refused(
                    "factions, X-Ray stashes, and transitions are only writable for X-Ray saves".to_owned(),
                ));
            }
            if save.index().is_legacy() {
                return Err(Error::Refused(S2_LEGACY_EDIT_REFUSAL.to_owned()));
            }
            if !S2_STASH_MOVE_ENABLED && !stash_moves.is_empty() {
                return Err(Error::Refused(
                    "S2 stash transfer is disabled until the saved result is validated in-game".to_owned(),
                ));
            }
            if !edits.placements.is_empty()
                || !edits.upgrades.is_empty()
                || !edits.removals.is_empty()
                || !edits.adds.is_empty()
            {
                return Err(Error::Refused(
                    "S2 placement, upgrades, removal, and item addition are not supported by this writer".to_owned(),
                ));
            }
            if !stash_moves.is_empty() {
                let items = stash_items
                    .as_ref()
                    .ok_or_else(|| Error::Refused("S2 save has no confirmed stash block".to_owned()))?
                    .as_ref()
                    .map_err(|error| Error::Refused(format!("S2 stash cannot be indexed: {error}")))?;
                if !save.unresolved_handles().is_empty() {
                    return Err(Error::Refused(
                        "S2 stash move requires a fully resolved inventory".to_owned(),
                    ));
                }
                for handle in stash_moves {
                    if !items.iter().any(|item| item.handle == *handle) {
                        return Err(Error::Refused(format!(
                            "S2 stash item 0x{handle:08X} is missing or ambiguous"
                        )));
                    }
                }
            }
            let current_money = save.money();
            let money_change = edits.money.filter(|value| *value != current_money);
            let mut changes = Vec::new();
            if let Some(new_value) = money_change {
                changes.push(S2Change::SetMoney(new_value));
            }
            let mut stack_count = 0_usize;
            for (handle, new_value) in &edits.stacks {
                let ItemHandle::Stalker2(handle) = handle else {
                    return Err(Error::Refused(
                        "X-Ray stack edit cannot be applied to an S2 save".to_owned(),
                    ));
                };
                let Some(item) = inventory
                    .iter()
                    .find(|item| item.handle == *handle && item.editable_count)
                else {
                    return Err(Error::Refused("selected S2 stack is not confirmed editable".to_owned()));
                };
                if item.count != *new_value {
                    changes.push(S2Change::SetStackCount {
                        handle: *handle,
                        count: *new_value,
                    });
                    stack_count = stack_count.saturating_add(1);
                }
            }
            for (handle, percent) in &edits.durability {
                let ItemHandle::Stalker2(handle) = handle else {
                    return Err(Error::Refused(
                        "X-Ray durability edit cannot be applied to an S2 save".to_owned(),
                    ));
                };
                let Some(item) = inventory.iter().find(|item| item.handle == *handle) else {
                    return Err(Error::Refused(
                        "selected S2 durability item no longer exists".to_owned(),
                    ));
                };
                let Some(old_value) = item.condition else {
                    return Err(Error::Refused(
                        "selected S2 item has no confirmed durability field".to_owned(),
                    ));
                };
                let condition = f32::from(*percent) / 100.0;
                if (old_value - condition).abs() > 0.005 {
                    changes.push(S2Change::SetDurability {
                        handle: *handle,
                        condition,
                    });
                }
            }
            for handle in stash_moves {
                changes.push(S2Change::MoveStashToBackpack { handle: *handle });
            }
            if changes.is_empty() {
                return Err(Error::Refused("there are no inventory changes to save".to_owned()));
            }
            let packed = SaveBuffer::from_vec(save.write_changes(&changes)?);
            Ok((
                packed,
                EditSummary {
                    money: money_change,
                    stack_count,
                    move_count: stash_moves.len(),
                    ..EditSummary::default()
                },
            ))
        }
    }
}

fn verify_requested_values(
    original: &LoadedSave,
    selected: &LoadedSave,
    edits: &PendingInventoryEdits,
    stash_moves: &BTreeSet<u32>,
) -> Result<()> {
    let actual_money = match &selected.data {
        SaveData::Xray { save, .. } => save.money()?,
        SaveData::Stalker2 { save, .. } => save.money(),
    };
    if edits.money.is_some_and(|expected| expected != actual_money) {
        return Err(Error::damaged("saved wallet value differs after read-back"));
    }
    for (handle, expected) in &edits.stacks {
        let actual = match (&selected.data, handle) {
            (SaveData::Xray { inventory, .. }, ItemHandle::Xray(handle)) => inventory
                .iter()
                .find(|item| item.handle == *handle)
                .and_then(|item| item.count)
                .map(u32::from),
            (SaveData::Stalker2 { inventory, .. }, ItemHandle::Stalker2(handle)) => inventory
                .iter()
                .find(|item| item.handle == *handle)
                .map(|item| item.count),
            _ => None,
        };
        if actual != Some(*expected) {
            return Err(Error::damaged("saved stack count differs after read-back"));
        }
    }
    for (handle, percent) in &edits.durability {
        let actual = match (&selected.data, handle) {
            (SaveData::Xray { inventory, .. }, ItemHandle::Xray(handle)) => inventory
                .iter()
                .find(|item| item.handle == *handle)
                .and_then(|item| item.condition),
            (SaveData::Stalker2 { inventory, .. }, ItemHandle::Stalker2(handle)) => inventory
                .iter()
                .find(|item| item.handle == *handle)
                .and_then(|item| item.condition),
            _ => None,
        };
        let expected = f32::from(*percent) / 100.0;
        if actual.is_none_or(|value| (value - expected).abs() > 0.005) {
            return Err(Error::damaged("saved item durability differs after read-back"));
        }
    }
    for (handle, destination) in &edits.placements {
        let expected = match destination {
            DraftPlacement::Ruck => "ruck",
            DraftPlacement::Belt => "belt",
            DraftPlacement::Slot(_) => {
                return Err(Error::Refused(
                    "slot placement has no verified read-back field".to_owned(),
                ))
            }
        };
        let actual = match (&selected.data, handle) {
            (SaveData::Xray { inventory, .. }, ItemHandle::Xray(handle)) => inventory
                .iter()
                .find(|item| item.handle == *handle)
                .and_then(|item| item.placement.as_deref()),
            _ => None,
        };
        if actual != Some(expected) {
            return Err(Error::damaged("saved item placement differs after read-back"));
        }
    }
    for (handle, expected) in &edits.upgrades {
        let actual = match (&selected.data, handle) {
            (SaveData::Xray { save, .. }, ItemHandle::Xray(handle)) => writer::current_upgrades(save, *handle)?,
            _ => {
                return Err(Error::Refused(
                    "S2 upgrades are not supported by this writer".to_owned(),
                ))
            }
        };
        if &actual != expected {
            return Err(Error::damaged("saved item upgrades differ after read-back"));
        }
    }
    for handle in &edits.removals {
        let exists = match (&selected.data, handle) {
            (SaveData::Xray { inventory, .. }, ItemHandle::Xray(handle)) => {
                inventory.iter().any(|item| item.handle == *handle)
            }
            _ => {
                return Err(Error::Refused(
                    "S2 item removal is not supported by this writer".to_owned(),
                ))
            }
        };
        if exists {
            return Err(Error::damaged("removed item remains after read-back"));
        }
    }
    if !edits.adds.is_empty() {
        let (before, after) = match (&original.data, &selected.data) {
            (SaveData::Xray { inventory: before, .. }, SaveData::Xray { inventory: after, .. }) => (before, after),
            _ => {
                return Err(Error::Refused(
                    "S2 item additions are not supported by this writer".to_owned(),
                ))
            }
        };
        let before_handles: BTreeSet<u16> = before.iter().map(|item| item.handle).collect();
        let mut verified_handles = BTreeSet::new();
        for request in &edits.adds {
            let ammo = request.item_key.to_ascii_lowercase().starts_with("ammo_");
            let expected_count = ammo.then(|| u16::try_from(request.quantity).ok()).flatten();
            let added_item = after.iter().find(|item| {
                !before_handles.contains(&item.handle)
                    && !verified_handles.contains(&item.handle)
                    && item.section == request.item_key
                    && if ammo {
                        item.count == expected_count
                    } else {
                        request.quantity == 1 && item.count.is_none_or(|count| count == 1)
                    }
            });
            let Some(added_item) = added_item else {
                return Err(Error::damaged("added item differs after read-back"));
            };
            verified_handles.insert(added_item.handle);
        }
    }
    for handle in stash_moves {
        let before = match &original.data {
            SaveData::Stalker2 { stash_items, .. } => stash_items
                .as_ref()
                .ok_or_else(|| Error::damaged("original S2 stash is unavailable for read-back verification"))?
                .as_ref()
                .map_err(|error| Error::damaged(error.clone()))?
                .iter()
                .find(|item| item.handle == *handle)
                .ok_or_else(|| Error::damaged("requested S2 stash item is missing from the original save"))?,
            SaveData::Xray { .. } => {
                return Err(Error::Refused(
                    "S2 stash transfer cannot be verified against an X-Ray save".to_owned(),
                ))
            }
        };
        let SaveData::Stalker2 { save, inventory, .. } = &selected.data else {
            return Err(Error::Refused(
                "S2 stash transfer cannot be verified against an X-Ray read-back".to_owned(),
            ));
        };
        let after_stash = save.stash_items()?;
        let mut matching_items = inventory.iter().filter(|item| item.handle == *handle);
        let Some(after) = matching_items.next() else {
            return Err(Error::damaged("saved stash transfer differs after read-back"));
        };
        let after_x = after
            .x
            .ok_or_else(|| Error::damaged("moved S2 item has no read-back grid position"))?;
        let after_y = after
            .y
            .ok_or_else(|| Error::damaged("moved S2 item has no read-back grid position"))?;
        if after_stash.iter().any(|item| item.handle == *handle)
            || matching_items.next().is_some()
            || after.count != before.count
            || after.total_weight.to_bits() != before.total_weight.to_bits()
            || after.kind_code != before.kind_code
            || after.type_key != before.type_key
            || after.display_name != before.display_name
            || after.width != Some(before.width)
            || after.height != Some(before.height)
            || normalized_s2_grid_shape(&before.cells, before.x, before.y)
                != normalized_s2_grid_shape(&after.cells, after_x, after_y)
        {
            return Err(Error::damaged(
                "saved stash item identity or contents differ after read-back",
            ));
        }
    }
    if !edits.faction_relations.is_empty() {
        let (original_save, verified_save) = match (&original.data, &selected.data) {
            (SaveData::Xray { save: before, .. }, SaveData::Xray { save: after, .. }) => (before, after),
            _ => return Err(Error::Refused("faction edits require an X-Ray read-back".to_owned())),
        };
        let bundle = sse_catalog::CatalogBundleReader::load_embedded()
            .get(original_save.format().id())
            .and_then(|bundle| bundle.factions.as_ref())
            .ok_or_else(|| Error::Refused("faction catalog is unavailable for read-back".to_owned()))?;
        let actual_relations = verified_save
            .actor_relations()
            .ok_or_else(|| Error::damaged("actor relation row is missing after read-back"))?;
        for (key, expected) in &edits.faction_relations {
            let community_id = bundle
                .resolve(key)
                .map_err(|_| Error::Refused(format!("unknown faction key '{key}'")))?
                .numeric_id
                .ok_or_else(|| Error::Refused(format!("faction '{key}' has no numeric community id")))?;
            if !actual_relations
                .iter()
                .any(|(id, value)| *id == community_id && value == expected)
            {
                return Err(Error::damaged(format!(
                    "faction relation '{key}' differs after read-back"
                )));
            }
        }
    }
    if !edits.stash_takes.is_empty() || !edits.stash_puts.is_empty() {
        let (before, after) = match (&original.data, &selected.data) {
            (SaveData::Xray { save: before, .. }, SaveData::Xray { save: after, .. }) => (before, after),
            _ => {
                return Err(Error::Refused(
                    "X-Ray stash edits require an X-Ray read-back".to_owned(),
                ))
            }
        };
        for handle in &edits.stash_takes {
            if !before
                .registry_objects()
                .iter()
                .any(|object| object.object_id == *handle)
                || !after
                    .registry_objects()
                    .iter()
                    .any(|object| object.object_id == *handle && object.parent_id == after.actor_id())
            {
                return Err(Error::damaged(format!(
                    "stash take 0x{handle:04X} differs after read-back"
                )));
            }
        }
        for (handle, box_id) in &edits.stash_puts {
            if !before
                .registry_objects()
                .iter()
                .any(|object| object.object_id == *handle && object.parent_id == before.actor_id())
                || !after
                    .registry_objects()
                    .iter()
                    .any(|object| object.object_id == *handle && object.parent_id == *box_id)
            {
                return Err(Error::damaged(format!(
                    "stash put 0x{handle:04X} differs after read-back"
                )));
            }
        }
    }
    if let Some(destination_changer) = edits.relocate_to {
        let (before, after) = match (&original.data, &selected.data) {
            (SaveData::Xray { save: before, .. }, SaveData::Xray { save: after, .. }) => (before, after),
            _ => {
                return Err(Error::Refused(
                    "actor relocation requires an X-Ray read-back".to_owned(),
                ))
            }
        };
        if !before
            .level_changer_destinations()?
            .iter()
            .any(|(handle, _)| *handle == destination_changer)
            || !after
                .registry_objects()
                .iter()
                .any(|object| object.object_id == after.actor_id())
        {
            return Err(Error::damaged("actor relocation target is not present after read-back"));
        }
    }
    Ok(())
}

fn normalized_s2_grid_shape(cells: &[sse_s2::S2GridCell], x: u16, y: u16) -> Vec<(u16, u16)> {
    let mut shape = cells
        .iter()
        .map(|cell| (cell.x.saturating_sub(x), cell.y.saturating_sub(y)))
        .collect::<Vec<_>>();
    shape.sort_unstable();
    shape
}

impl Screen for Inventory {
    fn id(&self) -> ScreenId {
        ScreenId::Inventory
    }

    fn subtitle(&self) -> &str {
        t("Состав рюкзака и подтверждённые изменения X-Ray / S2")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (row, banner, reload) = add_external_file_banner(cx.tree, host)?;
        self.external_banner_row = Some(row);
        self.external_banner = Some(banner);
        self.external_reload = Some(reload);
        let body = cx.tree.add(
            Some(host),
            NodeKind::Row,
            Style {
                grow: 1.0,
                margin: crate::layout::Edges {
                    left: 16.0,
                    ..crate::layout::Edges::default()
                },
                gap: Size::new(crate::theme::CONTROL_GAP + 6.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let inventory = style::d2::panel(cx.tree, body)?;
        cx.tree.set_style(
            inventory,
            Style {
                grow: 1.0,
                shrink: 1.0,
                preferred: crate::layout::Size::new(0.0, 0.0),
                min: crate::layout::Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        self.inventory_card = Some(inventory);
        style::d2::panel_title(cx.tree, inventory, t("ИНВЕНТАРЬ"))?;
        let money = cx.tree.add(
            Some(inventory),
            NodeKind::Wrap,
            Style {
                gap: Size::new(8.0, 8.0),
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.money_label = Some(style::label(
            cx.tree,
            money,
            "Выберите сейв на экране «Обзор».",
            Text::Value,
        )?);
        let colors = crate::theme::current().colors;
        let input = cx.tree.add(
            Some(money),
            NodeKind::Leaf,
            Style {
                min: crate::layout::Size::new(120.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: String::new(),
                style: Text::Value.style(),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[0]),
                ..Look::default()
            },
        )?;
        self.money_input_widget = Some(input);
        self.money_input = Some(TextInput::new("", money_input_config())?);
        for (amount, label) in [(10_000_u32, "+10 000"), (50_000, "+50 000"), (100_000, "+100 000")] {
            let button = style::d2::button(
                cx.tree,
                money,
                label,
                style::d2::ButtonKind::Secondary,
                style::d2::ButtonSize::Small,
            )?;
            cx.tree.set_style(
                button,
                Style {
                    min: crate::layout::Size::new(0.0, crate::theme::d2::CONTROL_HEIGHT_SMALL.1),
                    padding: crate::layout::Edges {
                        left: 10.0,
                        top: 0.0,
                        right: 10.0,
                        bottom: 0.0,
                    },
                    shrink: 0.0,
                    ..Style::default()
                },
            )?;
            self.money_buttons.push((button, amount));
        }
        let filters = style::row(cx.tree, inventory)?;
        let search_widget = cx.tree.add(
            Some(filters),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                min: crate::layout::Size::new(180.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: t("Поиск предметов…").to_owned(),
                style: Text::Body.style(),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[2]),
                ..Look::default()
            },
        )?;
        self.search_widget = Some(search_widget);
        self.search = Some(TextInput::new("", inventory_search_config())?);
        self.clear_search = Some(style::button(cx.tree, filters, "Очистить", Button::Secondary)?);
        self.search_count = Some(style::label(cx.tree, filters, "Найдено: 0", Text::Note)?);
        let chips = cx.tree.add(
            Some(inventory),
            NodeKind::Row,
            Style {
                gap: Size::new(4.0, 0.0),
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for category in INVENTORY_CATEGORIES {
            let id = cx.tree.add(
                Some(chips),
                NodeKind::Leaf,
                Style {
                    min: crate::layout::Size::new(0.0, 28.0),
                    shrink: 1.0,
                    padding: crate::layout::Edges {
                        left: 6.0,
                        top: 0.0,
                        right: 6.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
                Content::Button {
                    text: t(category).to_owned(),
                    style: Text::Note.style(),
                },
                style::nav(category == self.selected_category),
            )?;
            self.categories.push((id, category));
        }
        let header = cx.tree.add(
            Some(inventory),
            NodeKind::Row,
            Style {
                min: crate::layout::Size::new(0.0, 36.0),
                preferred: crate::layout::Size::new(0.0, 36.0),
                shrink: 0.0,
                gap: Size::new(8.0, 0.0),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look {
                border: Some((style::d2::argb(crate::theme::d2::BORDER_SUBTLE), 1.0)),
                ..Look::default()
            },
        )?;
        let header_name = style::label(cx.tree, header, t("Предмет"), Text::Note)?;
        set_column_style(cx.tree, header_name, 2.0, 0.0)?;
        let key_header = style::label(cx.tree, header, t("Ключ"), Text::Note)?;
        set_column_style(cx.tree, key_header, 1.0, 0.0)?;
        self.key_header = Some(key_header);
        let place_header = style::label(cx.tree, header, t("Место"), Text::Note)?;
        set_fixed_column_style(cx.tree, place_header, INVENTORY_PLACE_WIDTH)?;
        let condition_header = style::label(cx.tree, header, t("Состояние"), Text::Note)?;
        set_fixed_column_style(cx.tree, condition_header, INVENTORY_CONDITION_WIDTH)?;
        let count_header = style::label(cx.tree, header, t("Количество"), Text::Note)?;
        set_fixed_column_style(cx.tree, count_header, INVENTORY_COUNT_WIDTH)?;
        cx.tree.add(
            Some(header),
            NodeKind::Leaf,
            Style {
                min: crate::layout::Size::new(INVENTORY_STEPPER_WIDTH, 0.0),
                preferred: crate::layout::Size::new(INVENTORY_STEPPER_WIDTH, 0.0),
                shrink: 0.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let item_rows = cx.tree.add(
            Some(inventory),
            NodeKind::Column,
            Style {
                grow: 0.0,
                shrink: 0.0,
                min: crate::layout::Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_clip_children(item_rows, true)?;
        self.item_list = Some(item_rows);
        self.empty_results = Some(paragraph(
            cx.tree,
            item_rows,
            "⌕\nПредметы не найдены\nИзмените поиск или категорию.",
            Text::Note,
        )?);
        if let Some(id) = self.empty_results {
            cx.tree.set_visible(id, false)?;
        }
        self.reset_filters = Some(style::button(
            cx.tree,
            item_rows,
            "Сбросить фильтры",
            Button::Secondary,
        )?);
        if let Some(id) = self.reset_filters {
            cx.tree.set_visible(id, false)?;
        }
        for _ in 0..INVENTORY_MAX_PAGE_SIZE {
            let stack = cx.tree.add(
                Some(item_rows),
                NodeKind::Stack,
                inventory_row_style(false),
                Content::Panel,
                Look::default(),
            )?;
            cx.tree.set_visible(stack, false)?;
            let select = cx.tree.add(
                Some(stack),
                NodeKind::Leaf,
                Style::default(),
                Content::Button {
                    text: String::new(),
                    style: Text::Body.style(),
                },
                Look {
                    hover_fill: Some(style::d2::argb(crate::theme::d2::ROW_HOVER)),
                    radius: crate::theme::BUTTON_RADIUS,
                    ..Look::default()
                },
            )?;
            let content = style::row(cx.tree, stack)?;
            cx.tree.set_style(
                content,
                Style {
                    gap: Size::new(8.0, 0.0),
                    align_items: crate::layout::Align::Center,
                    padding: crate::layout::Edges {
                        left: 10.0,
                        top: 0.0,
                        right: 10.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
            )?;
            let label = style::label(cx.tree, content, "", Text::Body)?;
            set_column_style(cx.tree, label, 2.0, 0.0)?;
            let key = style::label(cx.tree, content, "", Text::Note)?;
            set_column_style(cx.tree, key, 1.0, 0.0)?;
            let place = style::label(cx.tree, content, "", Text::Note)?;
            set_fixed_column_style(cx.tree, place, INVENTORY_PLACE_WIDTH)?;
            let condition = style::label(cx.tree, content, "", Text::Body)?;
            set_fixed_column_style(cx.tree, condition, INVENTORY_CONDITION_WIDTH)?;
            let count = style::label(cx.tree, content, "", Text::Body)?;
            set_fixed_column_style(cx.tree, count, INVENTORY_COUNT_WIDTH)?;
            let stepper = style::row(cx.tree, content)?;
            set_fixed_column_style(cx.tree, stepper, INVENTORY_STEPPER_WIDTH)?;
            let decrease = style::button(cx.tree, stepper, "−", Button::Secondary)?;
            let increase = style::button(cx.tree, stepper, "+", Button::Secondary)?;
            self.rows.push(ItemControls {
                row: stack,
                label,
                key,
                place,
                condition,
                count,
                select,
                decrease,
                increase,
                handle: None,
            });
        }
        let pages = style::row(cx.tree, inventory)?;
        self.previous = Some(library_icon_button(cx.tree, pages, Icon::D2ArrowLeft)?);
        self.page_range = Some(style::label(cx.tree, pages, "", Text::Note)?);
        self.next = Some(library_icon_button(cx.tree, pages, Icon::D2ArrowRight)?);
        self.export = Some(style::button(cx.tree, inventory, "Сохранить", Button::Primary)?);
        let status = style::label(cx.tree, pages, "Изменения пока не подготовлены.", Text::Note)?;
        cx.tree.set_style(
            status,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                ..Style::default()
            },
        )?;
        self.status = Some(status);
        self.item_spacer = Some(cx.tree.add(
            Some(inventory),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: crate::layout::Size::new(0.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?);
        let side = cx.tree.add(
            Some(body),
            NodeKind::Column,
            Style {
                preferred: Size::new(360.0, 0.0),
                min: Size::new(360.0, 0.0),
                max: Size::new(360.0, f32::INFINITY),
                shrink: 0.0,
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.side_column = Some(side);
        let inspector = style::d2::panel(cx.tree, side)?;
        cx.tree.set_style(
            inspector,
            Style {
                grow: 1.0,
                shrink: 1.0,
                // Takes the room left by the action panel and clips its content instead of pushing that panel down.
                preferred: crate::layout::Size::new(0.0, 0.0),
                min: crate::layout::Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        self.inspector_panel = Some(inspector);
        cx.tree.set_clip_children(inspector, true)?;
        style::d2::panel_title(cx.tree, inspector, t("ВЫБРАННЫЙ ПРЕДМЕТ"))?;
        let inspector_summary = paragraph(
            cx.tree,
            inspector,
            "Предмет не выбран\nКлюч: —\nКоличество в пачке: —",
            Text::Body,
        )?;
        // Reserve four lines because a long item name or key may wrap at the narrow shell widths.
        cx.tree.set_style(
            inspector_summary,
            Style {
                min: Size::new(0.0, 80.0),
                preferred: Size::new(0.0, 80.0),
                shrink: 0.0,
                ..Style::default()
            },
        )?;
        self.inspector_summary = Some(inspector_summary);
        self.inspector_condition_heading = Some(style::label(cx.tree, inspector, "ПРОЧНОСТЬ", Text::Heading)?);
        self.inspector_condition = Some(style::label(
            cx.tree,
            inspector,
            "Состояние / прочность: —",
            Text::Value,
        )?);
        let condition_row = style::row(cx.tree, inspector)?;
        for (percent, label) in [(100_u8, "100%"), (75, "75%"), (50, "50%")] {
            let button = style::button(cx.tree, condition_row, label, Button::Secondary)?;
            cx.tree.set_style(
                button,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: Size::new(0.0, crate::theme::BUTTON_HEIGHT),
                    ..Style::default()
                },
            )?;
            self.condition_buttons.push((button, percent));
        }
        style::label(cx.tree, inspector, "РАЗМЕЩЕНИЕ", Text::Heading)?;
        self.inspector_placement = Some(style::label(cx.tree, inspector, "Размещение: —", Text::Body)?);
        let placement_row = cx.tree.add(
            Some(inspector),
            NodeKind::Wrap,
            Style {
                gap: Size::new(6.0, 6.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for (label, destination) in [("Рюкзак", DraftPlacement::Ruck), ("Пояс", DraftPlacement::Belt)] {
            let button = style::button(cx.tree, placement_row, label, Button::Secondary)?;
            self.placement_buttons.push((button, destination));
        }
        style::label(cx.tree, inspector, "МОДИФИКАЦИИ", Text::Heading)?;
        self.inspector_upgrades = Some(style::label(cx.tree, inspector, "Модификации: —", Text::Body)?);
        let upgrade_row = cx.tree.add(
            Some(inspector),
            NodeKind::Wrap,
            Style {
                gap: Size::new(6.0, 6.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for _ in 0..MAXIMUM_UPGRADE_ROWS {
            let widget = style::button(cx.tree, upgrade_row, "", Button::Secondary)?;
            cx.tree.set_visible(widget, false)?;
            self.upgrade_controls.push(UpgradeControl { widget, key: None });
        }
        let panel = style::d2::panel(cx.tree, side)?;
        // The add panel takes the inspector's room: it grows into the side column above the pinned action panel.
        cx.tree.set_style(panel, add_panel_style(0.0))?;
        cx.tree.set_clip_children(panel, true)?;
        self.add_panel = Some(panel);
        style::d2::panel_title(cx.tree, panel, t("ДОБАВИТЬ ПРЕДМЕТ"))?;
        let note = paragraph(
            cx.tree,
            panel,
            "Доступны записи каталога с сериализованным шаблоном в этом сейве.",
            Text::Note,
        )?;
        keep_height(cx.tree, note)?;
        self.add_note = Some(note);
        let add_search = cx.tree.add(
            Some(panel),
            NodeKind::Leaf,
            Style {
                shrink: 0.0,
                min: crate::layout::Size::new(220.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: t("Поиск по названию или ключу секции…").to_owned(),
                style: Text::Body.style(),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[2]),
                ..Look::default()
            },
        )?;
        self.add_search_widget = Some(add_search);
        self.add_search = Some(TextInput::new("", inventory_search_config())?);
        // The candidate list is the only part that shrinks: it scrolls out of sight in a short panel, while the fixed
        // rows below keep their height.
        let add_scroll = cx.tree.add(
            Some(panel),
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
        cx.tree.set_clip_children(add_scroll, true)?;
        self.add_scroll = Some(add_scroll);
        let add_list = cx.tree.add(
            Some(add_scroll),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, 2.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let add_empty = style::label(cx.tree, add_list, "Предметы не найдены.", Text::Note)?;
        self.add_empty = Some(add_empty);
        cx.tree.set_visible(add_empty, false)?;
        for _ in 0..ADD_ITEM_PAGE_SIZE {
            // A candidate is two lines: its name on the button, and below it the key and the reason, wrapped.
            let slot = cx.tree.add(
                Some(add_list),
                NodeKind::Column,
                Style {
                    gap: Size::new(0.0, 2.0),
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let widget = style::button(cx.tree, slot, "", Button::Secondary)?;
            // One line, shortened with an ellipsis; its inset matches the button's text so both start together.
            let note = style::label(cx.tree, slot, "", Text::Note)?;
            cx.tree.set_style(
                note,
                Style {
                    shrink: 0.0,
                    padding: crate::layout::Edges {
                        left: 16.0,
                        top: 0.0,
                        right: 16.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
            )?;
            cx.tree.set_visible(slot, false)?;
            self.add_candidate_rows.push((widget, None));
            self.add_candidate_slots.push((slot, note));
        }
        // The paging row stays under the candidate list, outside its scroll, so it is always in reach.
        let add_pages = style::row(cx.tree, panel)?;
        keep_height_row(cx.tree, add_pages)?;
        // The same pager as the inventory table: arrows and the range of the rows shown.
        self.add_previous = Some(library_icon_button(cx.tree, add_pages, Icon::D2ArrowLeft)?);
        self.add_page_range = Some(style::label(cx.tree, add_pages, "", Text::Note)?);
        self.add_next = Some(library_icon_button(cx.tree, add_pages, Icon::D2ArrowRight)?);
        // The chosen candidate (its name and key) and the quantity form one group without a gap of its own: a hidden
        // label keeps no room, and the visible ones take their spacing from their margins.
        let selected_group = cx.tree.add(
            Some(panel),
            NodeKind::Column,
            Style {
                shrink: 0.0,
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let selected_name = style::label(cx.tree, selected_group, "", Text::Heading)?;
        let selected_key = style::label(cx.tree, selected_group, "", Text::Note)?;
        // The chosen candidate starts 12 px below the list, as the blocks of the frame do.
        cx.tree.set_style(
            selected_name,
            Style {
                shrink: 0.0,
                margin: crate::layout::Edges {
                    top: 12.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;
        cx.tree.set_style(
            selected_key,
            Style {
                shrink: 0.0,
                margin: crate::layout::Edges {
                    bottom: 10.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;
        cx.tree.set_visible(selected_name, false)?;
        cx.tree.set_visible(selected_key, false)?;
        self.add_selected_name = Some(selected_name);
        self.add_selected_key_label = Some(selected_key);
        let quantity_row = style::row(cx.tree, selected_group)?;
        keep_height_row(cx.tree, quantity_row)?;
        style::label(cx.tree, quantity_row, "Количество:", Text::Body)?;
        let quantity = cx.tree.add(
            Some(quantity_row),
            NodeKind::Leaf,
            Style {
                min: crate::layout::Size::new(120.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: "1".to_owned(),
                style: Text::Value.style(),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[0]),
                ..Look::default()
            },
        )?;
        self.add_quantity_widget = Some(quantity);
        self.add_quantity = Some(TextInput::new("1", add_quantity_config())?);
        let draft_note = paragraph(
            cx.tree,
            panel,
            "Предмет появится в рюкзаке после «Сохранить». До этого он лежит в черновике.",
            Text::Note,
        )?;
        keep_height(cx.tree, draft_note)?;
        self.add_draft_note = Some(draft_note);
        let filler = cx.tree.add(
            Some(panel),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.add_filler = Some(filler);
        let add_actions = style::row(cx.tree, panel)?;
        keep_height_row(cx.tree, add_actions)?;
        self.add_cancel = Some(style::d2::button(
            cx.tree,
            add_actions,
            t("Отмена"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?);
        self.add_confirm = Some(style::d2::button(
            cx.tree,
            add_actions,
            t("Добавить"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        // Two equal buttons share the panel's content width.
        for id in [self.add_cancel, self.add_confirm].into_iter().flatten() {
            cx.tree.set_style(
                id,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, crate::theme::d2::CONTROL_HEIGHT.0),
                    padding: crate::layout::Edges {
                        left: 8.0,
                        top: 0.0,
                        right: 8.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
            )?;
        }
        cx.tree.set_visible(panel, false)?;
        let actions = style::d2::panel(cx.tree, side)?;
        cx.tree.set_style(
            actions,
            Style {
                shrink: 0.0,
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        self.actions_panel = Some(actions);
        let edit_actions = cx.tree.add(
            Some(actions),
            NodeKind::Row,
            Style {
                gap: Size::new(8.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        // Two equal buttons share the panel width on one row at every window size.
        let remove = style::d2::button(
            cx.tree,
            edit_actions,
            t("Удалить предмет"),
            style::d2::ButtonKind::Danger,
            style::d2::ButtonSize::Normal,
        )?;
        let add = style::d2::button(
            cx.tree,
            edit_actions,
            t("+ Добавить предмет"),
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?;
        for id in [remove, add] {
            cx.tree.set_style(
                id,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, crate::theme::d2::CONTROL_HEIGHT.0),
                    padding: crate::layout::Edges {
                        left: 8.0,
                        top: 0.0,
                        right: 8.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
            )?;
        }
        self.remove_button = Some(remove);
        self.add_button = Some(add);
        let overlay_host = cx.tree.overlay_host().unwrap_or(host);
        let confirmation = style::card(cx.tree, overlay_host)?;
        self.process_confirmation = Some(confirmation);
        style::label(cx.tree, confirmation, "ПРОВЕРКА ЗАПУЩЕННОЙ ИГРЫ", Text::Heading)?;
        self.process_description = Some(style::label(
            cx.tree,
            confirmation,
            "Проверяю, запущена ли игра для выбранного сейва…",
            Text::Body,
        )?);
        let process_actions = style::row(cx.tree, confirmation)?;
        self.process_continue = Some(style::button(cx.tree, process_actions, "Проверка…", Button::Primary)?);
        self.process_cancel = Some(style::button(cx.tree, process_actions, "Отмена", Button::Secondary)?);
        if let Some(continue_button) = self.process_continue {
            cx.tree.set_enabled(continue_button, false)?;
        }
        cx.tree.set_visible(confirmation, false)?;
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if let Message::Window(crate::event_loop::WindowEvent::Resized { width, .. }) = message {
            self.compact = *width < 1600;
            if let Some(side) = self.side_column {
                cx.tree
                    .set_style(side, side_column_style(self.compact, self.add_panel_open))?;
            }
            self.render(cx)?;
        }
        self.workspace.poll_tasks();
        if let Message::User(AppMessage::ToScreen(ScreenId::Inventory, payload)) = message {
            if let Some(SaveProcessCheckFinished { request_id, result }) =
                payload.downcast_ref::<SaveProcessCheckFinished>()
            {
                let is_current = self
                    .pending_save_request
                    .as_ref()
                    .is_some_and(|(pending_id, _)| pending_id == request_id);
                if is_current {
                    match result {
                        Ok(false) => {
                            let Some((_, request)) = self.pending_save_request.take() else {
                                return Ok(());
                            };
                            self.process_check_complete = false;
                            let _ = cx.tree.close_dialog()?;
                            self.start_save_write(cx, request)?;
                        }
                        Ok(true) | Err(_) => {
                            let Some((text, continue_label)) = process_check_prompt(result) else {
                                return Ok(());
                            };
                            self.process_check_complete = true;
                            if let Some(description) = self.process_description {
                                cx.tree.set_text(description, &text)?;
                            }
                            if let Some(continue_button) = self.process_continue {
                                cx.tree.set_text(continue_button, continue_label)?;
                                cx.tree.set_enabled(continue_button, true)?;
                            }
                            if let Some(status) = self.status {
                                cx.tree.set_text(status, &text)?;
                            }
                            cx.status = Some(text);
                        }
                    }
                }
                return self.render(cx);
            }
        }
        if self.pending_save_request.is_some() {
            let escape = matches!(
                message,
                Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym: 0xff1b,
                    ..
                })
            );
            if escape || clicked.is_some_and(|id| Some(id) == self.process_cancel) {
                self.pending_save_request = None;
                self.process_check_complete = false;
                let _ = cx.tree.close_dialog()?;
                if let Some(status) = self.status {
                    cx.tree.set_text(status, t("Сохранение отменено."))?;
                }
                cx.status = Some(t("Сохранение отменено.").to_owned());
                return self.render(cx);
            }
            if clicked.is_some_and(|id| Some(id) == self.process_continue) && self.process_check_complete {
                let Some((_, request)) = self.pending_save_request.take() else {
                    return Ok(());
                };
                self.process_check_complete = false;
                let _ = cx.tree.close_dialog()?;
                self.start_save_write(cx, request)?;
                return self.render(cx);
            }
            return Ok(());
        }
        if let Message::User(AppMessage::Tick(seconds)) = message {
            schedule_file_check(&self.workspace, cx, *seconds);
        }
        if let Message::User(AppMessage::EditorAction(action)) = message {
            return self.editor_action(*action, cx);
        }
        if let (Some(widget), Some(input)) = (self.search_widget, self.search.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if let (Some(widget), Some(input)) = (self.money_input_widget, self.money_input.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if let (Some(widget), Some(input)) = (self.add_search_widget, self.add_search.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if let (Some(widget), Some(input)) = (self.add_quantity_widget, self.add_quantity.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if let Message::Window(crate::event_loop::WindowEvent::Ime(event)) = message {
            let committed = matches!(event, crate::event_loop::ImeEvent::Commit(_));
            if self.add_panel_open && self.add_search.as_ref().is_some_and(TextInput::focused) {
                let (query, display) = if let Some(input) = self.add_search.as_mut() {
                    input.apply_ime_event(event)?;
                    (committed.then(|| input.text()), Some(input.display_text()))
                } else {
                    (None, None)
                };
                if let (Some(widget), Some(display)) = (self.add_search_widget, display) {
                    cx.tree.set_input_text(widget, &display)?;
                }
                if let Some(query) = query {
                    self.add_search_query = query;
                    self.add_page = 0;
                    self.render_add_panel(cx)?;
                }
                return Ok(());
            }
            if self.search.as_ref().is_some_and(TextInput::focused) {
                let (query, display) = if let Some(input) = self.search.as_mut() {
                    input.apply_ime_event(event)?;
                    (committed.then(|| input.text()), Some(input.display_text()))
                } else {
                    (None, None)
                };
                if let (Some(widget), Some(display)) = (self.search_widget, display) {
                    cx.tree.set_input_text(widget, &display)?;
                }
                if let Some(query) = query {
                    self.search_query = query;
                    self.page = 0;
                    return self.render(cx);
                }
                return Ok(());
            }
            if self.money_input.as_ref().is_some_and(TextInput::focused) {
                let (value, display) = if let Some(input) = self.money_input.as_mut() {
                    input.apply_ime_event(event)?;
                    (committed.then(|| input.text()), Some(input.display_text()))
                } else {
                    (None, None)
                };
                if let (Some(widget), Some(display)) = (self.money_input_widget, display) {
                    cx.tree.set_input_text(widget, &display)?;
                }
                if let Some(value) = value {
                    match value.parse::<u32>() {
                        Ok(value) if value <= 2_000_000_000 => {
                            cx.app.set_invalid_numeric_input(false);
                            self.stage_money_value(cx, value)?;
                        }
                        _ => cx.app.set_invalid_numeric_input(true),
                    }
                    return self.render(cx);
                }
                return Ok(());
            }
            if self.add_panel_open && self.add_quantity.as_ref().is_some_and(TextInput::focused) {
                let display = if let Some(input) = self.add_quantity.as_mut() {
                    input.apply_ime_event(event)?;
                    Some(input.display_text())
                } else {
                    None
                };
                if let (Some(widget), Some(display)) = (self.add_quantity_widget, display) {
                    cx.tree.set_input_text(widget, &display)?;
                }
                return Ok(());
            }
        }
        if clicked.is_some() && clicked == self.search_widget {
            if let Some(input) = self.search.as_mut() {
                input.focus(true, 0);
            }
        }
        if clicked.is_some() && clicked == self.add_search_widget {
            if let Some(input) = self.add_search.as_mut() {
                input.focus(true, 0);
            }
        }
        if clicked.is_some() && clicked == self.add_quantity_widget {
            if let Some(input) = self.add_quantity.as_mut() {
                input.focus(true, 0);
            }
        }
        if let Some(key) = self
            .add_candidate_rows
            .iter()
            .find(|(widget, _)| clicked.is_some() && clicked == Some(*widget))
            .and_then(|(_, key)| key.clone())
        {
            self.add_selected_key = Some(key);
            return self.render_add_panel(cx);
        }
        if clicked.is_some() && clicked == self.add_previous {
            self.add_page = self.add_page.saturating_sub(1);
            return self.render_add_panel(cx);
        }
        if clicked.is_some() && clicked == self.add_next {
            self.add_page = self.add_page.saturating_add(1);
            return self.render_add_panel(cx);
        }
        if clicked.is_some() && clicked == self.add_cancel {
            self.close_add_panel(cx)?;
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.add_confirm {
            let quantity = self
                .add_quantity
                .as_ref()
                .map(TextInput::text)
                .and_then(|value| value.parse::<u32>().ok())
                .unwrap_or(1)
                .max(1);
            if quantity > u32::from(u16::MAX) {
                cx.status = Some(t("Количество должно быть от 1 до 65535 для этого формата.").to_owned());
                return Ok(());
            }
            let Some(key) = self.add_selected_key.clone() else {
                cx.status = Some(t("Выберите предмет с подтверждённым шаблоном добавления.").to_owned());
                return Ok(());
            };
            self.stage_add_key(cx, &key, quantity)?;
            if let (Some(input), Some(widget)) = (self.add_quantity.as_mut(), self.add_quantity_widget) {
                input.focus(false, 0);
                cx.tree.set_text(widget, &quantity.to_string())?;
            }
            self.close_add_panel(cx)?;
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.clear_search {
            self.search_query.clear();
            self.search = Some(TextInput::new("", inventory_search_config())?);
            self.page = 0;
            return self.render(cx);
        }
        if let Some(category) = self
            .categories
            .iter()
            .find(|(id, _)| clicked.is_some() && clicked == Some(*id))
            .map(|(_, category)| *category)
        {
            self.selected_category = category;
            self.page = 0;
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.reset_filters {
            self.search_query.clear();
            self.selected_category = "ВСЕ";
            self.search = Some(TextInput::new("", inventory_search_config())?);
            self.page = 0;
            return self.render(cx);
        }
        if let Some(handle) = self
            .rows
            .iter()
            .find(|row| clicked.is_some() && clicked == Some(row.select))
            .and_then(|row| row.handle)
        {
            self.selected_item = Some(handle);
            return self.render(cx);
        }
        if let Some(percent) = self
            .condition_buttons
            .iter()
            .find(|(id, _)| clicked.is_some() && clicked == Some(*id))
            .map(|(_, percent)| *percent)
        {
            if let Some(handle) = self.selected_item {
                self.stage_durability(cx, handle, percent)?;
            }
            return self.render(cx);
        }
        if let Some(destination) = self
            .placement_buttons
            .iter()
            .find(|(id, _)| clicked.is_some() && clicked == Some(*id))
            .map(|(_, destination)| *destination)
        {
            if let Some(handle) = self.selected_item {
                self.stage_placement(cx, handle, destination)?;
            }
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.remove_button {
            if let Some(handle) = self.selected_item {
                self.stage_remove(cx, handle)?;
            }
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.add_button {
            return self.open_add_panel(cx);
        }
        if clicked.is_some() && clicked == self.external_reload {
            return start_reload_selected(&self.workspace, cx);
        }
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
        }) = message
        {
            if self.add_panel_open && *keysym == 0xff1b {
                self.close_add_panel(cx)?;
                return self.render(cx);
            }
            if self.add_panel_open && *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if let Some(widget) = self.add_search_widget {
                    if let Some(input) = self.add_search.as_mut() {
                        input.focus(true, 0);
                    }
                    cx.tree.set_focus(Some(widget))?;
                }
                return Ok(());
            }
            if self.add_panel_open && self.add_search.as_ref().is_some_and(TextInput::focused) {
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = SaveClipboard::default();
                if let Some(input) = self.add_search.as_mut() {
                    let _ = input.key(
                        key,
                        Modifiers {
                            ctrl: *ctrl,
                            shift: false,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    self.add_search_query = input.text();
                    if let Some(widget) = self.add_search_widget {
                        cx.tree.set_text(widget, &self.add_search_query)?;
                    }
                    self.add_page = 0;
                    self.add_selected_key = None;
                    return self.render_add_panel(cx);
                }
            }
            if self.add_panel_open && self.add_quantity.as_ref().is_some_and(TextInput::focused) {
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = SaveClipboard::default();
                if let Some(input) = self.add_quantity.as_mut() {
                    let _ = input.key(
                        key,
                        Modifiers {
                            ctrl: *ctrl,
                            shift: *shift,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    if let Some(widget) = self.add_quantity_widget {
                        cx.tree.set_text(widget, &input.text())?;
                    }
                    return self.render_add_panel(cx);
                }
            }
            if *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if let Some(widget) = self.search_widget {
                    self.search.as_mut().map(|input| input.focus(true, 0));
                    cx.tree.set_focus(Some(widget))?;
                }
                return Ok(());
            }
            if self.search.as_ref().is_some_and(TextInput::focused) {
                if matches!(*keysym, 0xff0d | 0xff1b) {
                    if let Some(input) = self.search.as_mut() {
                        input.focus(false, 0);
                    }
                    cx.tree.set_focus(None)?;
                    return self.render(cx);
                }
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = SaveClipboard::default();
                if let Some(input) = self.search.as_mut() {
                    let _ = input.key(
                        key,
                        Modifiers {
                            ctrl: *ctrl,
                            shift: false,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    self.search_query = input.text();
                    self.page = 0;
                    return self.render(cx);
                }
            }
            if self.money_input.as_ref().is_some_and(TextInput::focused) {
                if matches!(*keysym, 0xff0d | 0xff1b) {
                    let editing = *keysym == 0xff0d;
                    if editing {
                        let value = self.money_input.as_ref().map(TextInput::text).unwrap_or_default();
                        match value.parse::<u32>() {
                            Ok(value) if value <= 2_000_000_000 => {
                                cx.app.set_invalid_numeric_input(false);
                                self.stage_money_value(cx, value)?;
                            }
                            _ => {
                                cx.app.set_invalid_numeric_input(true);
                                cx.status =
                                    Some(t("Введены некорректные значения (проверьте введённые числа).").to_owned())
                            }
                        }
                    } else {
                        cx.app.set_invalid_numeric_input(false);
                    }
                    if !cx.app.has_invalid_numeric_input() {
                        if let Some(input) = self.money_input.as_mut() {
                            input.focus(false, 0);
                        }
                        cx.tree.set_focus(None)?;
                        return self.render(cx);
                    }
                    return Ok(());
                }
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = SaveClipboard::default();
                let edited = if let Some(input) = self.money_input.as_mut() {
                    let _ = input.key(
                        key,
                        Modifiers {
                            ctrl: *ctrl,
                            shift: *shift,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    if let Some(widget) = self.money_input_widget {
                        let text = input.text();
                        cx.tree.set_text(widget, &text)?;
                        Some(text)
                    } else {
                        Some(input.text())
                    }
                } else {
                    None
                };
                if let Some(text) = edited {
                    match text.parse::<u32>() {
                        Ok(value) if value <= 2_000_000_000 => {
                            cx.app.set_invalid_numeric_input(false);
                            self.stage_money_value(cx, value)?;
                        }
                        _ => cx.app.set_invalid_numeric_input(true),
                    }
                }
                return self.render(cx);
            }
        }
        if let Some((_, amount)) = self
            .money_buttons
            .iter()
            .find(|(id, _)| clicked.is_some() && clicked == Some(*id))
        {
            self.stage_money(cx, i64::from(*amount))?;
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        for row in &self.rows {
            if clicked == Some(row.decrease) || clicked == Some(row.increase) {
                if let Some(handle) = row.handle {
                    self.stage_stack(cx, handle, clicked == Some(row.increase))?;
                }
                return self.render(cx);
            }
        }
        for control in &self.upgrade_controls {
            if clicked.is_some() && clicked == Some(control.widget) {
                if let Some(key) = control.key.as_deref() {
                    if let Some(handle) = self.selected_item {
                        self.stage_upgrade(cx, handle, key)?;
                    }
                }
                return self.render(cx);
            }
        }
        if clicked.is_some() && clicked == self.export {
            return self.save(cx);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Inventory, payload)) = message {
            if let Some(FileCheckFinished {
                path,
                source_sha256,
                changed,
            }) = payload.downcast_ref::<FileCheckFinished>()
            {
                let mut state = self.workspace.lock();
                if state
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.slot.path == *path && selected.source_sha256 == *source_sha256)
                {
                    state.external_change = *changed;
                }
            }
            if let Some(SaveFinished {
                request_id,
                draft_generation,
                source_path,
                draft_identity,
                source_sha256,
                result,
            }) = payload.downcast_ref::<SaveFinished>()
            {
                if !self.workspace.session.is_latest_operation(*request_id) {
                    return self.render(cx);
                }
                let still_selected = self.workspace.lock().selected.as_ref().is_some_and(|selected| {
                    selected.slot.path == *source_path && selected.source_sha256 == *source_sha256
                });
                if !still_selected {
                    return self.render(cx);
                }
                match result {
                    Ok((loaded, text)) => {
                        let mut state = self.workspace.lock();
                        state.selected = Some(Arc::clone(loaded));
                        state.pending_money = None;
                        state.pending_stacks.clear();
                        state.pending_durability.clear();
                        state.pending_placements.clear();
                        state.pending_upgrades.clear();
                        state.pending_removed.clear();
                        state.pending_adds.clear();
                        state.pending_stash_moves.clear();
                        state.pending_xray_stash_takes.clear();
                        state.pending_xray_stash_puts.clear();
                        state.pending_faction_relations.clear();
                        state.pending_relocation = None;
                        state.external_change = false;
                        drop(state);
                        let old_draft_is_current = self
                            .workspace
                            .session
                            .clear_draft_if_current(draft_identity, *draft_generation);
                        if old_draft_is_current {
                            cx.app.discard_draft(source_sha256);
                        }
                        let new_journal = DraftJournal::new(vec![DraftPlan::empty(&loaded.source_sha256)?], 0)?;
                        cx.app
                            .set_current_save_identity(loaded.slot.path.clone(), loaded.source_sha256.clone());
                        let legacy_s2 =
                            matches!(&loaded.data, SaveData::Stalker2 { save, .. } if save.index().is_legacy());
                        cx.app.set_current_save_format(loaded.slot.format_id.clone(), legacy_s2);
                        cx.app.set_draft_journal(new_journal.clone());
                        set_workspace_draft(&self.workspace, &new_journal);
                        let mut journals = vec![new_journal];
                        if old_draft_is_current {
                            journals.push(DraftJournal::new(vec![DraftPlan::empty(source_sha256)?], 0)?);
                        }
                        self.workspace.persist_drafts(journals, cx);
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, text)?;
                        }
                        cx.status = Some(text.clone());
                        if let Some(proxy) = cx.proxy.as_ref() {
                            for screen in [ScreenId::Overview, ScreenId::Backups, ScreenId::Timeline] {
                                let _ = proxy.send(AppMessage::ToScreen(screen, Box::new(())));
                            }
                        }
                    }
                    Err(error) => {
                        let text = if is_windows_file_busy_error_text(error) {
                            t(SAVE_WHILE_GAME_RUNNING_WARNING).to_owned()
                        } else {
                            tr!("Не удалось сохранить: {error}", error = error)
                        };
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &text)?;
                        }
                        cx.status = Some(text);
                    }
                }
            }
            if let Some(DraftPersisted(result)) = payload.downcast_ref::<DraftPersisted>() {
                match result {
                    Ok(()) => cx.status = Some(t("Черновик сохранён.").to_owned()),
                    Err(error) => cx.status = Some(tr!("Не удалось сохранить черновик: {error}", error = error)),
                }
            }
            self.render(cx)?;
        }
        Ok(())
    }
}

/// Faction information available from the selected save.
/// One row of a list panel: a card with two lines of text and a transparent button over it that takes the clicks.
#[derive(Clone, Copy)]
struct ListRow {
    stack: WidgetId,
    card: WidgetId,
    title: WidgetId,
    meta: WidgetId,
    select: WidgetId,
}

/// The widgets of a list panel (left) and its side panel (right), shared by the faction and stash screens.
struct ListSide {
    list_panel: WidgetId,
    count: WidgetId,
    note: WidgetId,
    rows: Vec<ListRow>,
    pages: WidgetId,
    previous: WidgetId,
    page_range: WidgetId,
    next: WidgetId,
    empty: WidgetId,
    kv_rows: Vec<WidgetId>,
    kv_values: Vec<WidgetId>,
    detail: WidgetId,
    status: WidgetId,
    actions: WidgetId,
    side: WidgetId,
}

/// Builds the list panel and the side panel. `title` heads the list, `side_title` the side panel; `keys` are the
/// key–value rows of the side panel, translated by the caller; `empty` is the side panel's text while nothing is chosen.
fn build_list_side(
    cx: &mut Context<'_>,
    host: WidgetId,
    title: &str,
    side_title: &str,
    keys: &[String],
    empty: &str,
    note: &str,
) -> Result<ListSide> {
    let body = cx.tree.add(
        Some(host),
        NodeKind::Row,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: Size::new(0.0, 0.0),
            gap: Size::new(crate::theme::CONTROL_GAP + 6.0, 0.0),
            align_items: crate::layout::Align::Stretch,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;
    let list_panel = style::d2::panel(cx.tree, body)?;
    cx.tree.set_style(
        list_panel,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: Size::new(0.0, 0.0),
            preferred: Size::new(0.0, 0.0),
            padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
            gap: Size::new(0.0, 8.0),
            align_items: crate::layout::Align::Stretch,
            ..Style::default()
        },
    )?;
    let header = style::row(cx.tree, list_panel)?;
    style::d2::panel_title(cx.tree, header, title)?;
    spacer(cx, header)?;
    let count = style::label(cx.tree, header, "0", Text::Note)?;
    // A paragraph: a long note wraps inside the list panel (its width is set when the layout is known).
    let note = paragraph(cx.tree, list_panel, note, Text::Note)?;
    let list = cx.tree.add(
        Some(list_panel),
        NodeKind::Column,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: Size::new(0.0, 0.0),
            align_items: crate::layout::Align::Stretch,
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )?;
    let mut rows = Vec::new();
    for _ in 0..TRANSITION_ROWS {
        // Gaps are bottom margins: a hidden row would otherwise keep a gap of its own.
        let stack = cx.tree.add(
            Some(list),
            NodeKind::Stack,
            Style {
                shrink: 0.0,
                align_items: crate::layout::Align::Stretch,
                margin: crate::layout::Edges {
                    bottom: 4.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let card = cx.tree.add(
            Some(stack),
            NodeKind::Row,
            Style {
                min: Size::new(0.0, 46.0),
                padding: crate::layout::Edges {
                    left: 12.0,
                    top: 4.0,
                    right: 12.0,
                    bottom: 4.0,
                },
                gap: Size::new(12.0, 0.0),
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
                min: Size::new(0.0, 0.0),
                gap: Size::new(0.0, 2.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let title = style::label(cx.tree, text_column, "", Text::Body)?;
        let meta = style::label(cx.tree, text_column, "", Text::Note)?;
        // The select button is last, so it covers the card and takes the clicks.
        let select = style::button(cx.tree, stack, "", Button::Secondary)?;
        cx.tree.set_look(select, Look::default())?;
        cx.tree.set_visible(stack, false)?;
        rows.push(ListRow {
            stack,
            card,
            title,
            meta,
            select,
        });
    }
    let pages = style::row(cx.tree, list_panel)?;
    let previous = library_icon_button(cx.tree, pages, Icon::D2ArrowLeft)?;
    let page_range = style::label(cx.tree, pages, "", Text::Note)?;
    let next = library_icon_button(cx.tree, pages, Icon::D2ArrowRight)?;
    cx.tree.set_visible(pages, false)?;

    let side = cx.tree.add(
        Some(body),
        NodeKind::Column,
        side_column_style(false, false),
        Content::Panel,
        Look::default(),
    )?;
    let inspector = style::d2::panel(cx.tree, side)?;
    cx.tree.set_style(
        inspector,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: Size::new(0.0, 0.0),
            preferred: Size::new(0.0, 0.0),
            padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
            gap: Size::new(0.0, 8.0),
            align_items: crate::layout::Align::Stretch,
            ..Style::default()
        },
    )?;
    cx.tree.set_clip_children(inspector, true)?;
    style::d2::panel_title(cx.tree, inspector, side_title)?;
    let empty = paragraph(cx.tree, inspector, empty, Text::Body)?;
    let mut kv_rows = Vec::new();
    let mut kv_values = Vec::new();
    for key in keys {
        let row = style::d2::key_value_row(cx.tree, inspector, key, "—")?;
        if let Some(value) = cx.tree.children(row).last().copied() {
            kv_values.push(value);
        }
        cx.tree.set_visible(row, false)?;
        kv_rows.push(row);
    }
    let detail = paragraph(cx.tree, inspector, "", Text::Note)?;
    let status = paragraph(cx.tree, inspector, "", Text::Note)?;
    cx.tree.set_visible(detail, false)?;
    cx.tree.set_visible(status, false)?;
    spacer(cx, inspector)?;
    let actions = style::row(cx.tree, inspector)?;
    cx.tree.set_visible(actions, false)?;
    Ok(ListSide {
        list_panel,
        count,
        note,
        rows,
        pages,
        previous,
        page_range,
        next,
        empty,
        kv_rows,
        kv_values,
        detail,
        status,
        actions,
        side,
    })
}

/// Sets the side column's width for the window and the wrap width of its paragraphs (a wrapped paragraph measures its
/// lines at its minimum width).
fn sync_side_widths(tree: &mut Tree, list: &ListSide) -> Result<()> {
    let (window_width, _) = window_pixels(tree);
    let compact = window_width < 1600.0;
    tree.set_style(list.side, side_column_style(compact, false))?;
    let note_width = side_column_width(compact) - 2.0 * crate::theme::d2::PANEL_PADDING.0;
    for id in [list.detail, list.status] {
        tree.set_style(
            id,
            Style {
                min: Size::new(note_width, 0.0),
                ..Style::default()
            },
        )?;
    }
    // The list's note takes the list panel's width, as laid out at the last frame.
    let panel_width = f32::from(u16::try_from(tree.rect(list.list_panel)?.width).unwrap_or(0));
    tree.set_style(
        list.note,
        Style {
            min: Size::new((panel_width - 2.0 * crate::theme::d2::PANEL_PADDING.0).max(0.0), 0.0),
            ..Style::default()
        },
    )?;
    Ok(())
}

/// A column that takes the free room of its parent: pushes the widgets after it to the far edge.
fn spacer(cx: &mut Context<'_>, parent: WidgetId) -> Result<WidgetId> {
    cx.tree.add(
        Some(parent),
        NodeKind::Column,
        Style {
            grow: 1.0,
            shrink: 1.0,
            min: Size::new(0.0, 0.0),
            ..Style::default()
        },
        Content::Panel,
        Look::default(),
    )
}

/// Shows a list row with its two lines; a chosen row has the accent border.
fn show_list_row(tree: &mut Tree, row: ListRow, title: &str, meta: &str, chosen: bool) -> Result<()> {
    tree.set_text(row.title, title)?;
    tree.set_text(row.meta, meta)?;
    tree.set_look(
        row.card,
        if chosen {
            Look {
                fill: Some(style::d2::argb(crate::theme::d2::ACCENT_TINT)),
                border: Some((style::d2::argb(crate::theme::d2::ACCENT), 1.0)),
                radius: crate::theme::d2::RADIUS_BADGE,
                ..Look::default()
            }
        } else {
            Look::default()
        },
    )?;
    tree.set_visible(row.stack, true)
}

/// Splits a label written as "title · details" at its first separator; a label without one has no details.
fn split_label(label: &str) -> (&str, &str) {
    label.split_once(" · ").unwrap_or((label, ""))
}

/// Rows per page of a list: the rows that fit the window's height. Before the window has a size, every row slot is used.
fn list_window(tree: &Tree) -> usize {
    let (_, height) = window_pixels(tree);
    if height <= 0.0 {
        return TRANSITION_ROWS;
    }
    transition_page_size(height)
}

struct Factions {
    workspace: Workspace,
    list: Option<ListSide>,
    decrease: Option<WidgetId>,
    increase: Option<WidgetId>,
    faction_keys: Vec<String>,
    faction_rows: Vec<Option<usize>>,
    index: usize,
    last_path: Option<PathBuf>,
}

impl Factions {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            list: None,
            decrease: None,
            increase: None,
            faction_keys: Vec::new(),
            faction_rows: Vec::new(),
            index: 0,
            last_path: None,
        }
    }

    /// Hides the list and the side panel's values: nothing is chosen, so there is nothing to act on.
    fn hide_list(&self, cx: &mut Context<'_>) -> Result<()> {
        let Some(list) = self.list.as_ref() else {
            return Ok(());
        };
        for row in &list.rows {
            cx.tree.set_visible(row.stack, false)?;
        }
        cx.tree.set_visible(list.pages, false)?;
        for id in &list.kv_rows {
            cx.tree.set_visible(*id, false)?;
        }
        cx.tree.set_visible(list.actions, false)?;
        cx.tree.set_visible(list.empty, true)?;
        cx.tree.set_visible(list.status, false)?;
        cx.tree.set_text(list.count, "0")?;
        Ok(())
    }

    /// The note above the list: a message about the save.
    fn set_text(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(list) = self.list.as_ref() {
            cx.tree.set_text(list.note, t(text))?;
        }
        Ok(())
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (selected, pending) = {
            let state = self.workspace.lock();
            (state.selected.clone(), state.pending_faction_relations.clone())
        };
        let page_size = list_window(cx.tree);
        if let Some(list) = self.list.as_ref() {
            sync_side_widths(cx.tree, list)?;
        }
        let Some(selected) = selected else {
            self.faction_keys.clear();
            self.set_text(cx, "Сначала выберите сейв на экране «Обзор».")?;
            return self.hide_list(cx);
        };
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.index = 0;
        }
        let SaveData::Xray { save, .. } = &selected.data else {
            self.faction_keys.clear();
            self.set_text(cx, "Редактирование отношений доступно только для X-Ray сейвов.")?;
            return self.hide_list(cx);
        };
        if !xray_change_supported(save, writer::ChangeKind::EditRelations) {
            self.faction_keys.clear();
            self.set_text(
                cx,
                "Редактирование отношений фракций не поддерживается данным форматом.",
            )?;
            return self.hide_list(cx);
        }
        if save.actor_relations().is_none() {
            self.faction_keys.clear();
            self.set_text(
                cx,
                "Отношения актёра не подтверждены индексом сохранения; редактирование недоступно.",
            )?;
            return self.hide_list(cx);
        }
        let bundle = sse_catalog::CatalogBundleReader::load_embedded().get(save.format().id());
        let Some(catalog) = bundle.and_then(|bundle| bundle.factions.as_ref()) else {
            self.faction_keys.clear();
            self.set_text(cx, "Каталог фракций для этого формата недоступен.")?;
            return self.hide_list(cx);
        };
        self.faction_keys = catalog
            .factions()
            .iter()
            .filter(|faction| faction.key != "actor" && faction.numeric_id.is_some())
            .map(|faction| faction.key.clone())
            .collect();
        self.faction_keys.sort();
        self.index = self.index.min(self.faction_keys.len().saturating_sub(1));
        if self.faction_keys.is_empty() {
            self.set_text(cx, "Каталог не содержит изменяемых числовых фракций.")?;
            return self.hide_list(cx);
        }
        let faction_count = self.faction_keys.len();
        let pages = faction_count.div_ceil(page_size.max(1));
        let start = self.index.checked_div(page_size).unwrap_or(0).saturating_mul(page_size);
        let can_edit = !self.workspace.is_saving() && !self.workspace.is_restoring();
        let mut chosen_name = None;
        let mut chosen_lines = None;
        let mut shown = Vec::new();
        for (offset, key) in self.faction_keys.iter().enumerate().skip(start).take(page_size) {
            let faction = catalog
                .resolve(key)
                .map_err(|error| Error::Refused(error.to_string()))?;
            let current = save.actor_relations().and_then(|relations| {
                faction
                    .numeric_id
                    .and_then(|community| relations.into_iter().find(|(id, _)| *id == community))
                    .map(|(_, value)| value)
            });
            let value = pending.get(key).copied().or(current);
            let label = faction.display_name.as_deref().unwrap_or(&faction.key).to_owned();
            let relation = value.map_or_else(|| t("нет записи").to_owned(), |value| value.to_string());
            let source = t(if pending.contains_key(key) {
                "черновик"
            } else {
                "сейв"
            });
            let meta = tr("{0} · {1}", &[&relation, &source]);
            let chosen = offset == self.index;
            if chosen {
                chosen_name = Some(t(&label).to_owned());
                chosen_lines = Some((relation.clone(), source.to_owned(), key.clone()));
            }
            shown.push((t(&label).to_owned(), meta, chosen, offset));
        }
        let list = self
            .list
            .as_mut()
            .ok_or_else(|| Error::Refused("factions list is not built".to_owned()))?;
        for row in &list.rows {
            cx.tree.set_visible(row.stack, false)?;
        }
        self.faction_rows.clear();
        for (row, (title, meta, chosen, offset)) in list.rows.iter().zip(shown.iter()) {
            show_list_row(cx.tree, *row, title, meta, *chosen)?;
            self.faction_rows.push(Some(*offset));
        }
        cx.tree.set_text(list.count, &faction_count.to_string())?;
        cx.tree.set_visible(list.pages, pages > 1)?;
        cx.tree.set_enabled(list.previous, start > 0)?;
        cx.tree
            .set_enabled(list.next, start.saturating_add(page_size) < faction_count)?;
        let last = start.saturating_add(page_size).min(faction_count);
        let range = crate::strings::tr_in(
            Some(crate::strings::current_language()),
            "{0}–{1} из {2}",
            &[&start.saturating_add(1), &last, &faction_count],
        );
        cx.tree.set_text(list.page_range, &range)?;
        let note = t("Выберите фракцию слева; значение попадает в черновик.");
        cx.tree.set_text(list.note, note)?;
        let Some((relation, source, _key)) = chosen_lines else {
            return self.hide_list(cx);
        };
        let name = chosen_name.unwrap_or_default();
        for id in &list.kv_rows {
            cx.tree.set_visible(*id, true)?;
        }
        for (value, text) in list.kv_values.iter().zip([name, relation, source]) {
            cx.tree.set_text(*value, &text)?;
        }
        cx.tree.set_visible(list.empty, false)?;
        cx.tree.set_visible(list.actions, true)?;
        cx.tree.set_visible(list.status, true)?;
        cx.tree.set_text(
            list.status,
            t("Изменение отношений экспериментальное. После выбора примените черновик кнопкой «Сохранить» в «Инвентаре»; проверка в игре не выполнена."),
        )?;
        for id in [self.decrease, self.increase].into_iter().flatten() {
            cx.tree.set_enabled(id, can_edit)?;
        }
        Ok(())
    }

    fn stage_relation(&mut self, cx: &mut Context<'_>, delta: i32) -> Result<()> {
        if self.workspace.is_saving() || self.workspace.is_restoring() {
            cx.status = Some(t("Отношения недоступны во время записи или восстановления.").to_owned());
            return Ok(());
        }
        let Some(key) = self.faction_keys.get(self.index).cloned() else {
            return Ok(());
        };
        let (source_sha256, current, format_id) = {
            let state = self.workspace.lock();
            let Some(selected) = state.selected.as_ref() else {
                return Ok(());
            };
            let SaveData::Xray { save, .. } = &selected.data else {
                return Ok(());
            };
            if !xray_change_supported(save, writer::ChangeKind::EditRelations) {
                cx.status = Some(t("Редактирование отношений фракций не поддерживается данным форматом.").to_owned());
                return Ok(());
            }
            if save.actor_relations().is_none() {
                cx.status = Some(t("Отношения актёра не подтверждены индексом сохранения.").to_owned());
                return Ok(());
            }
            let bundle = sse_catalog::CatalogBundleReader::load_embedded().get(save.format().id());
            let faction = bundle
                .and_then(|bundle| bundle.factions.as_ref())
                .and_then(|catalog| catalog.resolve(&key).ok());
            let current = state.pending_faction_relations.get(&key).copied().or_else(|| {
                let community_id = faction?.numeric_id?;
                save.actor_relations()?
                    .into_iter()
                    .find(|(id, _)| *id == community_id)
                    .map(|(_, value)| value)
            });
            (selected.source_sha256.clone(), current, save.format().id().to_owned())
        };
        let bundle = sse_catalog::CatalogBundleReader::load_embedded().get(&format_id);
        let Some(catalog) = bundle.and_then(|bundle| bundle.factions.as_ref()) else {
            cx.status = Some(t("Каталог фракций недоступен для выбранного сейва.").to_owned());
            return Ok(());
        };
        let next = current.unwrap_or(0).saturating_add(delta);
        let next = catalog.goodwill_min().map_or(next, |minimum| next.max(minimum));
        let next = catalog.goodwill_max().map_or(next, |maximum| next.min(maximum));
        let mut plan = cx
            .app
            .draft(&source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(&source_sha256)?);
        let original = {
            let state = self.workspace.lock();
            let selected = state.selected.as_ref();
            let Some(SaveData::Xray { save, .. }) = selected.map(|selected| &selected.data) else {
                return Ok(());
            };
            let Some(community_id) = catalog.resolve(&key).ok().and_then(|faction| faction.numeric_id) else {
                cx.status = Some(t("У этой фракции не задан числовой id.").to_owned());
                return Ok(());
            };
            save.actor_relations().and_then(|relations| {
                relations
                    .into_iter()
                    .find(|(id, _)| *id == community_id)
                    .map(|(_, value)| value)
            })
        };
        if original == Some(next) {
            plan.faction_relations.remove(&key);
        } else {
            plan.faction_relations.insert(key.clone(), next);
        }
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(&source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after faction edit".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        cx.status = Some(tr!(
            "Отношение {key} изменено в черновике: {next}.",
            key = key,
            next = next
        ));
        self.render(cx)
    }
}

impl Screen for Factions {
    fn id(&self) -> ScreenId {
        ScreenId::Factions
    }

    fn subtitle(&self) -> &str {
        t("Только сведения, подтверждённые индексатором сейва")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let keys = [t("Фракция"), t("Отношение"), t("Источник")].map(str::to_owned);
        let list = build_list_side(
            cx,
            host,
            t("ФРАКЦИИ"),
            t("ВЫБРАННАЯ ФРАКЦИЯ"),
            &keys,
            t("Фракция не выбрана."),
            t("Выберите сейв на экране «Обзор»."),
        )?;
        // The two adjustments share the row's width equally.
        for (slot, label) in [(&mut self.decrease, "−100"), (&mut self.increase, "+100")] {
            let button = style::d2::button(
                cx.tree,
                list.actions,
                label,
                style::d2::ButtonKind::Secondary,
                style::d2::ButtonSize::Normal,
            )?;
            cx.tree.set_style(
                button,
                Style {
                    grow: 1.0,
                    shrink: 1.0,
                    min: crate::layout::Size::new(0.0, crate::theme::BUTTON_HEIGHT),
                    ..Style::default()
                },
            )?;
            *slot = Some(button);
        }
        self.list = Some(list);
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if let Message::Window(crate::event_loop::WindowEvent::Resized { .. }) = message {
            return self.render(cx);
        }
        let page_size = list_window(cx.tree).max(1);
        let Some(list) = self.list.as_ref() else {
            return Ok(());
        };
        let previous = list.previous;
        let next = list.next;
        let rows = list.rows.clone();
        if clicked.is_some() && clicked == Some(previous) {
            self.index = self.index.saturating_sub(page_size);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == Some(next) {
            self.index = self
                .index
                .saturating_add(page_size)
                .min(self.faction_keys.len().saturating_sub(1));
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.decrease {
            return self.stage_relation(cx, -100);
        }
        if clicked.is_some() && clicked == self.increase {
            return self.stage_relation(cx, 100);
        }
        if let Some(offset) = rows
            .iter()
            .position(|row| clicked.is_some() && clicked == Some(row.select))
            .and_then(|position| self.faction_rows.get(position).copied().flatten())
        {
            self.index = offset;
            return self.render(cx);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StashAction {
    Stalker2Take(u32),
    XrayTake(u16),
    XrayPut { object_id: u16, box_id: u16 },
}

/// One stash entry of the selected save: its line, the action on it and whether that action is available.
#[derive(Clone)]
struct StashEntry {
    label: String,
    action: StashAction,
    action_text: String,
    staged: bool,
    enabled: bool,
}

/// Confirmed stash contents for the selected save: a list of entries with the chosen one in the side panel.
struct Stashes {
    workspace: Workspace,
    list: Option<ListSide>,
    action: Option<WidgetId>,
    entries: Vec<StashEntry>,
    row_entries: Vec<usize>,
    selected: Option<StashAction>,
    page: usize,
    last_path: Option<PathBuf>,
}

impl Stashes {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            list: None,
            action: None,
            entries: Vec::new(),
            row_entries: Vec::new(),
            selected: None,
            page: 0,
            last_path: None,
        }
    }

    /// The note above the list: a message about the save, or the header of the entries.
    fn set_text(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(list) = self.list.as_ref() {
            cx.tree.set_text(list.note, t(text))?;
        }
        Ok(())
    }

    /// The status line in the side panel; the same text goes to the window's status bar.
    fn set_status(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        let translated = t(text);
        if let Some(list) = self.list.as_ref() {
            cx.tree.set_text(list.status, translated)?;
            cx.tree.set_visible(list.status, !translated.is_empty())?;
        }
        cx.status = Some(translated.to_owned());
        Ok(())
    }

    /// Hides the rows and the chosen entry: nothing to show or nothing chosen.
    fn hide_list(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let Some(list) = self.list.as_ref() else {
            return Ok(());
        };
        for row in &list.rows {
            cx.tree.set_visible(row.stack, false)?;
        }
        cx.tree.set_visible(list.pages, false)?;
        for id in &list.kv_rows {
            cx.tree.set_visible(*id, false)?;
        }
        if let Some(action) = self.action {
            cx.tree.set_visible(action, false)?;
        }
        cx.tree.set_visible(list.actions, false)?;
        cx.tree.set_visible(list.empty, true)?;
        cx.tree.set_text(list.count, "0")?;
        Ok(())
    }

    /// Shows the entries of the current page in the rows, and the chosen entry in the side panel.
    /// Returns the number of pages.
    fn show_entries(&mut self, cx: &mut Context<'_>, page_size: usize) -> Result<usize> {
        let page_size = page_size.max(1);
        let pages = self.entries.len().div_ceil(page_size);
        self.page = self.page.min(pages.saturating_sub(1));
        let start = self.page.saturating_mul(page_size);
        let Some(list) = self.list.as_ref() else {
            return Ok(pages);
        };
        let rows = list.rows.clone();
        self.row_entries.clear();
        for row in &rows {
            cx.tree.set_visible(row.stack, false)?;
        }
        for (row, (index, entry)) in rows
            .iter()
            .zip(self.entries.iter().enumerate().skip(start).take(page_size))
        {
            let (title, meta) = split_label(&entry.label);
            show_list_row(cx.tree, *row, title, meta, self.selected == Some(entry.action))?;
            self.row_entries.push(index);
        }
        let list = self
            .list
            .as_ref()
            .ok_or_else(|| Error::Refused("stash list is not built".to_owned()))?;
        cx.tree.set_text(list.count, &self.entries.len().to_string())?;
        cx.tree.set_visible(list.pages, pages > 1)?;
        cx.tree.set_enabled(list.previous, self.page > 0)?;
        cx.tree.set_enabled(list.next, self.page.saturating_add(1) < pages)?;
        let last = start.saturating_add(page_size).min(self.entries.len());
        let range = crate::strings::tr_in(
            Some(crate::strings::current_language()),
            "{0}–{1} из {2}",
            &[&start.saturating_add(1), &last, &self.entries.len()],
        );
        cx.tree
            .set_text(list.page_range, if self.entries.is_empty() { "" } else { &range })?;
        let chosen = self
            .selected
            .and_then(|action| self.entries.iter().find(|entry| entry.action == action).cloned());
        let Some(entry) = chosen else {
            self.selected = None;
            for id in &list.kv_rows {
                cx.tree.set_visible(*id, false)?;
            }
            if let Some(action) = self.action {
                cx.tree.set_visible(action, false)?;
            }
            cx.tree.set_visible(list.actions, false)?;
            cx.tree.set_visible(list.empty, true)?;
            cx.tree.set_visible(list.status, false)?;
            cx.tree.set_visible(list.detail, false)?;
            return Ok(pages);
        };
        let (title, meta) = split_label(&entry.label);
        let values = [title.to_owned(), t(if entry.staged { "Да" } else { "Нет" }).to_owned()];
        cx.tree.set_text(list.detail, meta)?;
        cx.tree.set_visible(list.detail, true)?;
        for id in &list.kv_rows {
            cx.tree.set_visible(*id, true)?;
        }
        for (value, text) in list.kv_values.iter().zip(values.iter()) {
            cx.tree.set_text(*value, text)?;
        }
        cx.tree.set_visible(list.empty, false)?;
        cx.tree.set_visible(list.actions, true)?;
        if let Some(action) = self.action {
            cx.tree.set_text(action, &entry.action_text)?;
            cx.tree.set_visible(action, true)?;
            cx.tree.set_enabled(action, entry.enabled)?;
        }
        cx.tree.set_visible(list.status, true)?;
        Ok(pages)
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (selected, pending_moves, pending_xray_takes, pending_xray_puts) = {
            let state = self.workspace.lock();
            (
                state.selected.clone(),
                state.pending_stash_moves.clone(),
                state.pending_xray_stash_takes.clone(),
                state.pending_xray_stash_puts.clone(),
            )
        };
        let page_size = list_window(cx.tree).max(1);
        if let Some(list) = self.list.as_ref() {
            sync_side_widths(cx.tree, list)?;
        }
        self.entries.clear();
        self.set_status(cx, "")?;
        let Some(selected) = selected else {
            self.set_text(cx, t("Сначала выберите сейв на экране «Обзор»."))?;
            return self.hide_list(cx);
        };
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.page = 0;
            self.selected = None;
        }
        if let SaveData::Xray { save, inventory } = &selected.data {
            return self.render_xray_stashes(cx, save, inventory, &pending_xray_takes, &pending_xray_puts, page_size);
        }
        let SaveData::Stalker2 { save, stash_items, .. } = &selected.data else {
            return self.hide_list(cx);
        };
        let items = match stash_items {
            Some(Ok(items)) => items,
            Some(Err(error)) => {
                self.set_text(cx, &tr("Подтверждённые данные тайника недоступны: {0}", &[&error]))?;
                return self.hide_list(cx);
            }
            None => {
                self.set_text(cx, "Подтверждённый блок тайника в этом сохранении не найден.")?;
                return self.hide_list(cx);
            }
        };
        if items.is_empty() {
            self.set_text(cx, "Подтверждённый тайник найден, но живых предметов в нём нет.")?;
            return self.hide_list(cx);
        }
        let can_move = S2_STASH_MOVE_ENABLED && !save.index().is_legacy() && save.unresolved_handles().is_empty();
        for item in items {
            let name = item
                .display_name
                .as_deref()
                .map(|name| t(name).to_owned())
                .unwrap_or_else(|| {
                    tr(
                        "Предмет · ключ {0}",
                        &[&format!(
                            "{:02X}{:02X}{:02X}",
                            item.type_key[0], item.type_key[1], item.type_key[2]
                        )],
                    )
                });
            let weight = if item.total_weight.is_finite() {
                format!("{:.1}", item.total_weight)
            } else {
                t("неизвестен").to_owned()
            };
            let staged = pending_moves.contains(&item.handle);
            self.entries.push(StashEntry {
                label: tr(
                    "{0} · кол-во {1} · вес {2} · ячейки {3} · {4}×{5} от {6},{7} · 0x{8}",
                    &[
                        &name,
                        &item.count,
                        &weight,
                        &item.cells.len(),
                        &item.width,
                        &item.height,
                        &item.x,
                        &item.y,
                        &format!("{:08X}", item.handle),
                    ],
                ),
                action: StashAction::Stalker2Take(item.handle),
                action_text: t(if staged {
                    "Отменить перенос"
                } else {
                    "В рюкзак"
                })
                .to_owned(),
                staged,
                enabled: can_move,
            });
        }
        let pages = self.show_entries(cx, page_size)?;
        self.set_text(
            cx,
            &tr(
                "S2: {0} предметов в подтверждённом тайнике · страница {1} из {2}. {3}",
                &[
                    &self.entries.len(),
                    &self.page.saturating_add(1),
                    &pages,
                    &t(if can_move {
                        "Отметьте перенос и сохраните его в «Инвентаре»."
                    } else {
                        "Перенос в рюкзак отключён до проверки сохранения в игре."
                    }),
                ],
            ),
        )?;
        if !S2_STASH_MOVE_ENABLED {
            self.set_status(cx, "Перенос из тайника S2 отключён до проверки сохранения в игре.")?;
        } else if !save.unresolved_handles().is_empty() {
            self.set_status(cx, "Перенос отключён: индекс сейва содержит неразрешённые ссылки.")?;
        } else if save.index().is_legacy() {
            self.set_status(cx, S2_LEGACY_EDIT_REFUSAL)?;
        } else if pending_moves.is_empty() {
            self.set_status(
                cx,
                "Отметьте предметы и примените перенос кнопкой «Сохранить» в «Инвентаре».",
            )?;
        } else {
            self.set_status(
                cx,
                &tr(
                    "{0} предмет(ов) будет перенесено при сохранении из «Инвентаря».",
                    &[&pending_moves.len()],
                ),
            )?;
        }
        Ok(())
    }

    fn render_xray_stashes(
        &mut self,
        cx: &mut Context<'_>,
        save: &Save,
        inventory: &[InventoryItem],
        pending_takes: &BTreeSet<u16>,
        pending_puts: &BTreeMap<u16, u16>,
        page_size: usize,
    ) -> Result<()> {
        let can_move = xray_change_supported(save, writer::ChangeKind::MoveItems)
            && !self.workspace.is_saving()
            && !self.workspace.is_restoring();
        let catalog = sse_catalog::CatalogBundleReader::load_embedded().get(save.format().id());
        let Some(catalog) = catalog else {
            self.set_text(cx, "Каталог предметов для этой игры недоступен.")?;
            return self.hide_list(cx);
        };
        let mut boxes = save
            .registry_objects()
            .iter()
            .filter(|object| object.name == "inventory_box")
            .collect::<Vec<_>>();
        boxes.sort_by_key(|object| object.object_id);
        if boxes.is_empty() {
            self.set_text(cx, "Подтверждённые тайники X-Ray в этом сейве не найдены.")?;
            return self.hide_list(cx);
        }
        for box_object in &boxes {
            for object in save
                .registry_objects()
                .iter()
                .filter(|object| object.parent_id == box_object.object_id)
            {
                let Some(item) = catalog.items.resolve(&object.name_replace) else {
                    continue;
                };
                let name = item.display_name.as_deref().unwrap_or(&item.key);
                let staged = pending_takes.contains(&object.object_id);
                self.entries.push(StashEntry {
                    label: tr(
                        "{0} · тайник {1} · 0x{2}",
                        &[&t(name), &box_object.name_replace, &format!("{:04X}", object.object_id)],
                    ),
                    action: StashAction::XrayTake(object.object_id),
                    action_text: t(if staged {
                        "Отменить"
                    } else {
                        "Перенести"
                    })
                    .to_owned(),
                    staged,
                    enabled: can_move,
                });
            }
        }
        if let Some(destination_box) = boxes.first() {
            let box_id = destination_box.object_id;
            for item in inventory
                .iter()
                .filter(|item| item.placement.as_deref() == Some("ruck"))
            {
                let Some(definition) = catalog.items.resolve(&item.section) else {
                    continue;
                };
                let name = t(definition.display_name.as_deref().unwrap_or(&definition.key));
                let staged = pending_puts.get(&item.handle) == Some(&box_id);
                self.entries.push(StashEntry {
                    label: tr(
                        "{0} · в тайник {1} · 0x{2}",
                        &[&name, &destination_box.name_replace, &format!("{:04X}", item.handle)],
                    ),
                    action: StashAction::XrayPut {
                        object_id: item.handle,
                        box_id,
                    },
                    action_text: t(if staged {
                        "Отменить"
                    } else {
                        "Перенести"
                    })
                    .to_owned(),
                    staged,
                    enabled: can_move,
                });
            }
        }
        if self.entries.is_empty() {
            self.set_text(
                cx,
                "В найденных тайниках нет предметов каталога, а в рюкзаке нет предметов с подтверждённым размещением.",
            )?;
            return self.hide_list(cx);
        }
        let pages = self.show_entries(cx, page_size)?;
        let move_count = pending_takes.len().saturating_add(pending_puts.len());
        self.set_text(
            cx,
            &tr(
                "X-Ray: {0} тайник(ов), {1} предмет(ов) для переноса · страница {2} из {3}. Перенос рюкзак↔первый тайник подтверждён writer-ом и сохранится из «Инвентаря».",
                &[&boxes.len(), &self.entries.len(), &self.page.saturating_add(1), &pages],
            ),
        )?;
        let status = if !xray_change_supported(save, writer::ChangeKind::MoveItems) {
            t("Перемещение из тайников не поддерживается данным форматом.").to_owned()
        } else if self.workspace.is_saving() || self.workspace.is_restoring() {
            t("Перемещение временно недоступно во время записи или восстановления.").to_owned()
        } else {
            tr(
                "{0} перенос(ов) в черновике; проверьте результат после записи.",
                &[&move_count],
            )
        };
        self.set_status(cx, &status)?;
        Ok(())
    }

    fn stage_xray_move(&mut self, cx: &mut Context<'_>, action: StashAction) -> Result<()> {
        if self.workspace.is_saving() || self.workspace.is_restoring() {
            cx.status = Some(t("Перенос недоступен во время записи или восстановления.").to_owned());
            return Ok(());
        }
        let source_sha256 = {
            let state = self.workspace.lock();
            let Some(selected) = state.selected.as_ref() else {
                cx.status = Some(t("Сначала выберите сейв.").to_owned());
                return Ok(());
            };
            if !matches!(selected.data, SaveData::Xray { .. }) {
                cx.status = Some(t("X-Ray тайники доступны только для X-Ray сейвов.").to_owned());
                return Ok(());
            }
            let SaveData::Xray { save, .. } = &selected.data else {
                return Ok(());
            };
            if !xray_change_supported(save, writer::ChangeKind::MoveItems) {
                cx.status = Some(t("Перемещение из тайников не поддерживается данным форматом.").to_owned());
                return Ok(());
            }
            selected.source_sha256.clone()
        };
        let mut plan = cx
            .app
            .draft(&source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(&source_sha256)?);
        let message = match action {
            StashAction::XrayTake(handle) => {
                plan.stash_puts.retain(|put| put.object_id != handle);
                if let Some(index) = plan.stash_takes.iter().position(|candidate| *candidate == handle) {
                    plan.stash_takes.remove(index);
                    tr("Перенос 0x{0} из тайника отменён.", &[&format!("{handle:04X}")])
                } else {
                    plan.stash_takes.push(handle);
                    tr(
                        "Предмет 0x{0} будет перенесён в рюкзак при сохранении.",
                        &[&format!("{handle:04X}")],
                    )
                }
            }
            StashAction::XrayPut { object_id, box_id } => {
                plan.stash_takes.retain(|candidate| *candidate != object_id);
                if let Some(index) = plan.stash_puts.iter().position(|put| put.object_id == object_id) {
                    let same_destination = plan.stash_puts.get(index).is_some_and(|put| put.box_id == box_id);
                    plan.stash_puts.remove(index);
                    if same_destination {
                        tr("Перенос 0x{0} в тайник отменён.", &[&format!("{object_id:04X}")])
                    } else {
                        plan.stash_puts
                            .push(sse_storage::drafts::StashPut::new(object_id, box_id)?);
                        tr(
                            "Предмет 0x{0} назначен другому тайнику.",
                            &[&format!("{object_id:04X}")],
                        )
                    }
                } else {
                    plan.stash_puts
                        .push(sse_storage::drafts::StashPut::new(object_id, box_id)?);
                    tr(
                        "Предмет 0x{0} будет перенесён в тайник при сохранении.",
                        &[&format!("{object_id:04X}")],
                    )
                }
            }
            StashAction::Stalker2Take(_) => {
                return Err(Error::Refused(
                    "S2 stash movement must use its guarded draft path".to_owned(),
                ))
            }
        };
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(&source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after stash edit".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        cx.status = Some(message.clone());
        self.set_status(cx, &message)?;
        self.render(cx)
    }

    fn move_item(&mut self, cx: &mut Context<'_>, handle: u32) -> Result<()> {
        let status = {
            let mut state = self.workspace.lock();
            let Some(selected) = state.selected.as_ref() else {
                cx.status = Some(t("Сначала выберите сейв.").to_owned());
                return Ok(());
            };
            let SaveData::Stalker2 { save, stash_items, .. } = &selected.data else {
                cx.status = Some(t("Перенос тайника поддерживается только для S2.").to_owned());
                return Ok(());
            };
            if !S2_STASH_MOVE_ENABLED {
                cx.status = Some(t("Перенос из тайника S2 отключён до проверки сохранения в игре.").to_owned());
                return Ok(());
            }
            if save.index().is_legacy() || !save.unresolved_handles().is_empty() {
                cx.status = Some(t("Перенос недоступен для этого S2-сейва.").to_owned());
                return Ok(());
            }
            if !state.pending_stash_moves.is_empty() && !state.pending_stash_moves.contains(&handle) {
                cx.status =
                    Some(t("За один раз можно перенести только один предмет. Отмените предыдущий перенос.").to_owned());
                return Ok(());
            }
            let Some(items) = stash_items.as_ref().and_then(|items| items.as_ref().ok()) else {
                cx.status = Some(t("Содержимое тайника не подтверждено индексом.").to_owned());
                return Ok(());
            };
            let Some(item) = items.iter().find(|item| item.handle == handle) else {
                cx.status = Some(t("Предмет больше не найден в выбранном сейве.").to_owned());
                return Ok(());
            };
            let name = item
                .display_name
                .as_deref()
                .map(|name| t(name).to_owned())
                .unwrap_or_else(|| tr("предмет 0x{0}", &[&format!("{:08X}", item.handle)]));
            if state.pending_stash_moves.remove(&handle) {
                tr("Перенос {0} отменён.", &[&name])
            } else {
                state.pending_stash_moves.insert(handle);
                tr("{0} будет перенесён в рюкзак при сохранении.", &[&name])
            }
        };
        let (source_sha256, pending_moves) = {
            let state = self.workspace.lock();
            (
                state.selected.as_ref().map(|selected| selected.source_sha256.clone()),
                state.pending_stash_moves.iter().copied().collect::<Vec<_>>(),
            )
        };
        let Some(source_sha256) = source_sha256 else {
            return Ok(());
        };
        let mut plan = cx
            .app
            .draft(&source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(&source_sha256)?);
        plan.s2_stash_takes = pending_moves;
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(&source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after stash edit".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        cx.status = Some(status.clone());
        self.set_status(cx, &status)?;
        self.render(cx)?;
        Ok(())
    }
}

impl Screen for Stashes {
    fn id(&self) -> ScreenId {
        ScreenId::Stashes
    }

    fn subtitle(&self) -> &str {
        t("Подтверждённые тайники и их предметы")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let keys = [t("Предмет"), t("Черновик")].map(str::to_owned);
        let list = build_list_side(
            cx,
            host,
            t("ТАЙНИКИ"),
            t("ВЫБРАННЫЙ ПРЕДМЕТ"),
            &keys,
            t("Предмет не выбран."),
            t("Выберите сейв на экране «Обзор»."),
        )?;
        self.action = Some(style::d2::button(
            cx.tree,
            list.actions,
            "",
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        self.list = Some(list);
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.workspace.poll_tasks();
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if let Message::Window(crate::event_loop::WindowEvent::Resized { .. }) = message {
            return self.render(cx);
        }
        let Some(list) = self.list.as_ref() else {
            return Ok(());
        };
        let (previous, next, rows) = (list.previous, list.next, list.rows.clone());
        if clicked.is_some() && clicked == Some(previous) {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == Some(next) {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if let Some(index) = rows
            .iter()
            .position(|row| clicked.is_some() && clicked == Some(row.select))
            .and_then(|position| self.row_entries.get(position).copied())
        {
            if let Some(entry) = self.entries.get(index) {
                self.selected = Some(entry.action);
            }
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.action {
            if let Some(action) = self.selected {
                match action {
                    StashAction::Stalker2Take(handle) => self.move_item(cx, handle)?,
                    StashAction::XrayTake(handle) => self.stage_xray_move(cx, StashAction::XrayTake(handle))?,
                    StashAction::XrayPut { object_id, box_id } => {
                        self.stage_xray_move(cx, StashAction::XrayPut { object_id, box_id })?
                    }
                }
            }
            self.workspace.poll_tasks();
            return self.render(cx);
        }
        self.workspace.poll_tasks();
        Ok(())
    }
}

/// Parsed level-changer destinations in the selected save.
/// Transition rows the list can hold at most.
const TRANSITION_ROWS: usize = 12;
/// Height of one list row with its gap.
const TRANSITION_ROW_PITCH: f32 = 50.0;
/// Window height the list does not get: the shell's top and bottom, the panel's padding, header, note and pager.
const TRANSITION_CHROME: f32 = 393.0;

/// Rows that fit the window height, from one to [`TRANSITION_ROWS`].
fn transition_page_size(window_height: f32) -> usize {
    let available = window_height - TRANSITION_CHROME;
    let mut rows = 0_usize;
    while rows < TRANSITION_ROWS {
        let next = f32::from(u16::try_from(rows.saturating_add(1)).unwrap_or(u16::MAX));
        if next * TRANSITION_ROW_PITCH > available {
            break;
        }
        rows = rows.saturating_add(1);
    }
    rows.max(1)
}

/// Window size in pixels as floats: (width, height).
fn window_pixels(tree: &Tree) -> (f32, f32) {
    let (width, height) = tree.size();
    (
        f32::from(u16::try_from(width).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(height).unwrap_or(u16::MAX)),
    )
}

struct Transitions {
    workspace: Workspace,
    text: Option<WidgetId>,
    status: Option<WidgetId>,
    count: Option<WidgetId>,
    side: Option<WidgetId>,
    detail: Option<WidgetId>,
    detail_rows: Vec<WidgetId>,
    detail_values: Vec<WidgetId>,
    confirmation: Option<WidgetId>,
    confirmation_actions: Option<WidgetId>,
    confirm: Option<WidgetId>,
    cancel: Option<WidgetId>,
    move_here: Option<WidgetId>,
    pages: Option<WidgetId>,
    previous: Option<WidgetId>,
    page_range: Option<WidgetId>,
    next: Option<WidgetId>,
    rows: Vec<TransitionRow>,
    page: usize,
    selected: Option<u16>,
    pending_confirmation: Option<u16>,
    last_path: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct TransitionRow {
    stack: WidgetId,
    card: WidgetId,
    title: WidgetId,
    meta: WidgetId,
    select: WidgetId,
    handle: Option<u16>,
}

impl Transitions {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            text: None,
            status: None,
            count: None,
            side: None,
            detail: None,
            detail_rows: Vec::new(),
            detail_values: Vec::new(),
            confirmation: None,
            confirmation_actions: None,
            confirm: None,
            cancel: None,
            move_here: None,
            pages: None,
            previous: None,
            page_range: None,
            next: None,
            rows: Vec::new(),
            page: 0,
            selected: None,
            pending_confirmation: None,
            last_path: None,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (selected_save, pending) = {
            let state = self.workspace.lock();
            (state.selected.clone(), state.pending_relocation)
        };
        let (window_width, window_height) = window_pixels(cx.tree);
        let page_size = transition_page_size(window_height);
        let compact = window_width < 1600.0;
        if let Some(side) = self.side {
            cx.tree.set_style(side, side_column_style(compact, false))?;
        }
        // A wrapped status line measures its lines at its minimum width, so the width it is drawn in is set here.
        if let Some(status) = self.status {
            let note_width = side_column_width(compact) - 2.0 * crate::theme::d2::PANEL_PADDING.0;
            cx.tree.set_style(
                status,
                Style {
                    min: Size::new(note_width, 0.0),
                    ..Style::default()
                },
            )?;
        }
        for row in &mut self.rows {
            row.handle = None;
            cx.tree.set_visible(row.stack, false)?;
        }
        for id in [
            self.pages,
            self.confirmation,
            self.confirmation_actions,
            self.confirm,
            self.cancel,
        ]
        .into_iter()
        .flatten()
        {
            cx.tree.set_visible(id, false)?;
        }
        if let Some(id) = self.move_here {
            cx.tree.set_enabled(id, false)?;
        }
        if let Some(id) = self.count {
            cx.tree.set_text(id, "0")?;
        }
        let Some(selected) = selected_save else {
            self.set_text(cx, t("Сначала выберите сейв на экране «Обзор»."))?;
            self.set_detail(cx, t("Сначала выберите сейв на экране «Обзор»."))?;
            self.set_status(cx, "")?;
            return Ok(());
        };
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.pending_confirmation = None;
            self.selected = None;
        }
        let SaveData::Xray { save, .. } = &selected.data else {
            self.set_text(cx, t("Перенос персонажа поддерживается только для X-Ray сейвов."))?;
            self.set_detail(cx, t("Перенос персонажа поддерживается только для X-Ray сейвов."))?;
            self.set_status(cx, "")?;
            return Ok(());
        };
        let destinations = match save.level_changer_destinations() {
            Ok(destinations) => destinations,
            Err(error) => {
                self.set_text(cx, &tr("Не удалось проверить переходы: {0}", &[&error]))?;
                return Ok(());
            }
        };
        let can_relocate = xray_change_supported(save, writer::ChangeKind::RelocateActor)
            && !self.workspace.is_saving()
            && !self.workspace.is_restoring();
        let destination_count = destinations.len();
        if let Some(id) = self.count {
            cx.tree.set_text(id, &destination_count.to_string())?;
        }
        let pages = destination_count
            .saturating_add(page_size.saturating_sub(1))
            .checked_div(page_size)
            .unwrap_or(0);
        self.page = self.page.min(pages.saturating_sub(1));
        let start = self.page.saturating_mul(page_size);
        let availability = t(if can_relocate {
            "Выберите точку назначения; изменение попадёт в черновик и запишется с бэкапом после нажатия «Сохранить» в «Инвентаре»."
        } else {
            "Перенос персонажа не поддерживается этим форматом или временно занят."
        });
        let empty_text = t("В этом сейве нет подтверждённых переходов.");
        self.set_text(
            cx,
            if destination_count == 0 {
                empty_text
            } else {
                availability
            },
        )?;
        if let Some(id) = self.pages {
            cx.tree.set_visible(id, destination_count > page_size)?;
        }
        for (row, (handle, destination)) in self
            .rows
            .iter_mut()
            .zip(destinations.iter().skip(start).take(page_size))
        {
            let position = destination.dest_position.map_or_else(
                || t("позиция неизвестна").to_owned(),
                |point| format!("x {:.1}, y {:.1}, z {:.1}", point.x, point.y, point.z),
            );
            let staged_suffix = t(if pending == Some(*handle) {
                " · в черновике"
            } else {
                ""
            });
            cx.tree.set_text(
                row.title,
                &format!(
                    "{} → {}",
                    t(&destination.dest_level_name),
                    t(&destination.dest_level_point_name)
                ),
            )?;
            cx.tree.set_text(
                row.meta,
                &tr(
                    "{0} · id 0x{1}{2}",
                    &[&position, &format!("{handle:04X}"), &staged_suffix],
                ),
            )?;
            let chosen = self.selected == Some(*handle);
            cx.tree.set_look(
                row.card,
                if chosen {
                    Look {
                        fill: Some(style::d2::argb(crate::theme::d2::ACCENT_TINT)),
                        border: Some((style::d2::argb(crate::theme::d2::ACCENT), 1.0)),
                        radius: crate::theme::d2::RADIUS_BADGE,
                        ..Look::default()
                    }
                } else {
                    Look::default()
                },
            )?;
            cx.tree.set_visible(row.stack, true)?;
            row.handle = Some(*handle);
        }
        if let Some(id) = self.previous {
            cx.tree.set_enabled(id, self.page > 0)?;
        }
        if let Some(id) = self.next {
            cx.tree.set_enabled(id, self.page.saturating_add(1) < pages)?;
        }
        if let Some(id) = self.page_range {
            let last = start.saturating_add(page_size).min(destination_count);
            let range = crate::strings::tr_in(
                Some(crate::strings::current_language()),
                "{0}–{1} из {2}",
                &[&start.saturating_add(1), &last, &destination_count],
            );
            cx.tree.set_text(id, if destination_count == 0 { "" } else { &range })?;
        }
        if !can_relocate {
            self.pending_confirmation = None;
        }
        let chosen = self
            .selected
            .and_then(|handle| destinations.iter().find(|(candidate, _)| *candidate == handle));
        let has_choice = chosen.is_some();
        if let Some(id) = self.detail {
            cx.tree.set_visible(id, !has_choice)?;
        }
        for id in &self.detail_rows {
            cx.tree.set_visible(*id, has_choice)?;
        }
        match chosen {
            Some((handle, destination)) => {
                let position = destination.dest_position.map_or_else(
                    || t("позиция неизвестна").to_owned(),
                    |point| format!("x {:.1}, y {:.1}, z {:.1}", point.x, point.y, point.z),
                );
                let route = format!(
                    "{} → {}",
                    t(&destination.dest_level_name),
                    t(&destination.dest_level_point_name)
                );
                let draft = t(if pending == Some(*handle) { "Да" } else { "Нет" });
                let values = [route, position, format!("0x{handle:04X}"), draft.to_owned()];
                for (value, text) in self.detail_values.iter().zip(values.iter()) {
                    cx.tree.set_text(*value, text)?;
                }
                if let Some(id) = self.move_here {
                    cx.tree.set_enabled(id, can_relocate && pending.is_none())?;
                }
            }
            None => {
                self.selected = None;
                self.set_detail(cx, t("Переход не выбран. Выберите строку слева."))?;
            }
        }
        if let Some(handle) = self.pending_confirmation {
            if let Some((_, destination)) = destinations.iter().find(|(candidate, _)| *candidate == handle) {
                if let Some(id) = self.confirmation {
                    cx.tree.set_text(
                        id,
                        &tr(
                            "Подтвердить перенос в {0} → {1}? Затем отдельно нажмите «Сохранить» в «Инвентаре».",
                            &[&t(&destination.dest_level_name), &t(&destination.dest_level_point_name)],
                        ),
                    )?;
                    cx.tree.set_visible(id, true)?;
                }
                for id in [self.confirm, self.cancel, self.confirmation_actions]
                    .into_iter()
                    .flatten()
                {
                    cx.tree.set_visible(id, true)?;
                }
            } else {
                self.pending_confirmation = None;
            }
        }
        self.set_status(
            cx,
            &if !xray_change_supported(save, writer::ChangeKind::RelocateActor) {
                t("Перенос персонажа не поддерживается данным форматом.").to_owned()
            } else if self.workspace.is_saving() || self.workspace.is_restoring() {
                t("Перенос временно недоступен во время записи или восстановления.").to_owned()
            } else {
                pending.map_or_else(
                    || t("Персонаж не перемещён. Выберите подтверждённый переход.").to_owned(),
                    |handle| tr("Перенос 0x{0} находится в черновике.", &[&format!("{handle:04X}")]),
                )
            },
        )?;
        Ok(())
    }

    fn set_text(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(id) = self.text {
            cx.tree.set_text(id, t(text))?;
        }
        Ok(())
    }

    fn set_detail(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(id) = self.detail {
            cx.tree.set_text(id, t(text))?;
        }
        Ok(())
    }

    fn set_status(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        let translated = t(text);
        if let Some(id) = self.status {
            cx.tree.set_text(id, translated)?;
        }
        cx.status = Some(translated.to_owned());
        Ok(())
    }

    fn confirm_relocation(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.workspace.is_saving() || self.workspace.is_restoring() {
            cx.status = Some(t("Перенос недоступен во время записи или восстановления.").to_owned());
            return Ok(());
        }
        let Some(handle) = self.pending_confirmation else {
            return Ok(());
        };
        let selected_source = {
            let state = self.workspace.lock();
            let Some(selected) = state.selected.as_ref() else {
                return Ok(());
            };
            let SaveData::Xray { save, .. } = &selected.data else {
                return Ok(());
            };
            if !xray_change_supported(save, writer::ChangeKind::RelocateActor) {
                cx.status = Some(t("Перенос персонажа не поддерживается данным форматом.").to_owned());
                self.pending_confirmation = None;
                self.set_status(cx, "Перенос персонажа не поддерживается данным форматом.")?;
                for id in [self.confirmation, self.confirmation_actions, self.confirm, self.cancel]
                    .into_iter()
                    .flatten()
                {
                    cx.tree.set_visible(id, false)?;
                }
                return Ok(());
            }
            Some((
                selected.source_sha256.clone(),
                save.level_changer_destinations()?
                    .iter()
                    .any(|(candidate, _)| *candidate == handle),
            ))
        };
        let Some((source_sha256, destination_is_valid)) = selected_source else {
            return Ok(());
        };
        if !destination_is_valid {
            cx.status = Some(t("Выбранный переход больше не подтверждается сейвом.").to_owned());
            self.pending_confirmation = None;
            return self.render(cx);
        }
        let mut plan = cx
            .app
            .draft(&source_sha256)
            .cloned()
            .unwrap_or(DraftPlan::empty(&source_sha256)?);
        plan.relocate_to = Some(handle);
        cx.app.record_draft(plan)?;
        let journal = cx
            .app
            .draft_journal(&source_sha256)
            .cloned()
            .ok_or_else(|| Error::Refused("draft journal disappeared after relocation edit".to_owned()))?;
        set_workspace_draft(&self.workspace, &journal);
        self.workspace.persist_draft(journal, cx);
        self.pending_confirmation = None;
        cx.status = Some(tr(
            "Перенос 0x{0} подтверждён и добавлен в черновик.",
            &[&format!("{handle:04X}")],
        ));
        self.render(cx)
    }
}

impl Screen for Transitions {
    fn id(&self) -> ScreenId {
        ScreenId::Transitions
    }

    fn subtitle(&self) -> &str {
        t("Переходы из индексированных объектов level_changer")
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (window_width, _) = window_pixels(cx.tree);
        let body = cx.tree.add(
            Some(host),
            NodeKind::Row,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                gap: Size::new(crate::theme::CONTROL_GAP + 6.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        // The list panel takes the room left by the side column; its bottom is on the library's bottom edge.
        let list_panel = style::d2::panel(cx.tree, body)?;
        cx.tree.set_style(
            list_panel,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                preferred: Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        let header = style::row(cx.tree, list_panel)?;
        style::d2::panel_title(cx.tree, header, t("ПЕРЕХОДЫ"))?;
        // The counter is neutral text at the right of the header, like the library's; green is for statuses only.
        cx.tree.add(
            Some(header),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.count = Some(style::label(cx.tree, header, "0", Text::Note)?);
        self.text = Some(style::label(cx.tree, list_panel, "", Text::Note)?);
        let list = cx.tree.add(
            Some(list_panel),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        for _ in 0..TRANSITION_ROWS {
            // Gaps are bottom margins: a hidden row would otherwise keep a gap of its own.
            let stack = cx.tree.add(
                Some(list),
                NodeKind::Stack,
                Style {
                    shrink: 0.0,
                    align_items: crate::layout::Align::Stretch,
                    margin: crate::layout::Edges {
                        bottom: 4.0,
                        ..crate::layout::Edges::default()
                    },
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let card = cx.tree.add(
                Some(stack),
                NodeKind::Row,
                Style {
                    min: Size::new(0.0, 46.0),
                    padding: crate::layout::Edges {
                        left: 12.0,
                        top: 4.0,
                        right: 12.0,
                        bottom: 4.0,
                    },
                    gap: Size::new(12.0, 0.0),
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
                    min: Size::new(0.0, 0.0),
                    gap: Size::new(0.0, 2.0),
                    align_items: crate::layout::Align::Stretch,
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let title = style::label(cx.tree, text_column, "", Text::Body)?;
            let meta = style::label(cx.tree, text_column, "", Text::Note)?;
            // The select button is last, so it covers the card and takes the clicks.
            let select = style::button(cx.tree, stack, "", Button::Secondary)?;
            cx.tree.set_look(select, Look::default())?;
            cx.tree.set_visible(stack, false)?;
            self.rows.push(TransitionRow {
                stack,
                card,
                title,
                meta,
                select,
                handle: None,
            });
        }
        // The pager sits under the list, as in the library: arrows and the range of the rows shown.
        let pages = style::row(cx.tree, list_panel)?;
        self.pages = Some(pages);
        self.previous = Some(library_icon_button(cx.tree, pages, Icon::D2ArrowLeft)?);
        self.page_range = Some(style::label(cx.tree, pages, "", Text::Note)?);
        self.next = Some(library_icon_button(cx.tree, pages, Icon::D2ArrowRight)?);
        cx.tree.set_visible(pages, false)?;
        // The side column holds the chosen transition and the one action on it.
        let side = cx.tree.add(
            Some(body),
            NodeKind::Column,
            side_column_style(window_width < 1600.0, false),
            Content::Panel,
            Look::default(),
        )?;
        self.side = Some(side);
        let inspector = style::d2::panel(cx.tree, side)?;
        cx.tree.set_style(
            inspector,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                preferred: Size::new(0.0, 0.0),
                padding: crate::layout::Edges::all(crate::theme::d2::PANEL_PADDING.0),
                gap: Size::new(0.0, 8.0),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
        )?;
        cx.tree.set_clip_children(inspector, true)?;
        style::d2::panel_title(cx.tree, inspector, t("ВЫБРАННЫЙ ПЕРЕХОД"))?;
        let detail = paragraph(cx.tree, inspector, "Переход не выбран.", Text::Body)?;
        // Four lines are reserved: a long level or point name wraps at the narrow side column.
        cx.tree.set_style(
            detail,
            Style {
                min: Size::new(0.0, 96.0),
                preferred: Size::new(0.0, 96.0),
                shrink: 0.0,
                ..Style::default()
            },
        )?;
        self.detail = Some(detail);
        for key in [t("Куда"), t("Координаты"), t("Идентификатор"), t("Черновик")] {
            let row = style::d2::key_value_row(cx.tree, inspector, key, "—")?;
            self.detail_rows.push(row);
            if let Some(value) = cx.tree.children(row).last().copied() {
                self.detail_values.push(value);
            }
            cx.tree.set_visible(row, false)?;
        }
        self.status = Some(paragraph(cx.tree, inspector, "", Text::Note)?);
        self.confirmation = Some(style::label(cx.tree, inspector, "", Text::Body)?);
        if let Some(confirmation) = self.confirmation {
            cx.tree.set_visible(confirmation, false)?;
        }
        let actions = style::row(cx.tree, inspector)?;
        self.confirmation_actions = Some(actions);
        self.confirm = Some(style::button(cx.tree, actions, "Подтвердить перенос", Button::Primary)?);
        self.cancel = Some(style::button(cx.tree, actions, "Отмена", Button::Secondary)?);
        cx.tree.set_visible(actions, false)?;
        // Spacer: pushes the one action to the bottom of the panel.
        cx.tree.add(
            Some(inspector),
            NodeKind::Column,
            Style {
                grow: 1.0,
                shrink: 1.0,
                min: Size::new(0.0, 0.0),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        self.move_here = Some(style::d2::button(
            cx.tree,
            inspector,
            t("Перенести сюда…"),
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?);
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        // The number of rows follows the window height, so a resize renders the list again.
        if let Message::Window(crate::event_loop::WindowEvent::Resized { .. }) = message {
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.confirm {
            return self.confirm_relocation(cx);
        }
        if clicked.is_some() && clicked == self.cancel {
            self.pending_confirmation = None;
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.move_here {
            if let Some(handle) = self.selected {
                self.pending_confirmation = Some(handle);
                return self.render(cx);
            }
            return Ok(());
        }
        if let Some(handle) = self
            .rows
            .iter()
            .find(|row| clicked.is_some() && clicked == Some(row.select))
            .and_then(|row| row.handle)
        {
            self.selected = Some(handle);
            self.pending_confirmation = None;
            return self.render(cx);
        }
        Ok(())
    }
}

pub(super) fn short_text(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let short = chars.by_ref().take(limit).collect::<String>();
    if chars.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::{
        add_external_file_banner, commit_save_edits_to, prepare_browser_save, prepare_save_edits, prepare_xray_edits,
        AddRequest, DraftJournal, DraftPlan, DraftStore, Inventory, ItemHandle, LoadFinished, LoadedSave, Overview,
        PendingInventoryEdits, S2Save, SaveBuffer, SaveSlot, StartupBackupCheck, Workspace,
    };
    use crate::event_loop::{channel_pair, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::{AppMessage, Context, Screen, ScreenId};
    use crate::widget::{Content, Look, Tree};
    use sse_core::Error;
    use sse_s2::S2Change;
    use sse_xray::Save;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    #[test]
    fn named_translation_arguments_preserve_translated_placeholder_positions() {
        assert_eq!(
            super::tr_named_in(
                "en",
                "Не удалось открыть «{name}»: {error}",
                &[("name", &"slot.sav"), ("error", &"permission denied")],
            ),
            "Could not open “slot.sav”: permission denied"
        );
    }
    use std::time::UNIX_EPOCH;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn windows_path_comparison_does_not_merge_distinct_unpaired_surrogates() {
        assert!(!super::windows_path_units_equal(
            [0xd800].into_iter(),
            [0xd801].into_iter()
        ));
    }

    #[test]
    fn windows_path_comparison_keeps_ascii_case_insensitive_matching() {
        assert!(super::windows_path_units_equal(
            [
                b'C' as u16,
                b'\\' as u16,
                b'S' as u16,
                b'a' as u16,
                b'v' as u16,
                b'e' as u16
            ]
            .into_iter(),
            [
                b'c' as u16,
                b'\\' as u16,
                b's' as u16,
                b'a' as u16,
                b'v' as u16,
                b'e' as u16
            ]
            .into_iter(),
        ));
    }

    #[test]
    fn save_library_and_overview_dates_match_reference_patterns() {
        assert_eq!(super::display_file_time(UNIX_EPOCH, true, false), "01.01.70 00:00");
        assert_eq!(super::display_file_time(UNIX_EPOCH, false, true), "01.01.1970 00:00:00");
    }

    #[test]
    fn xray_game_time_matches_csharp_overview_format() {
        assert_eq!(super::format_xray_game_time(63_480_696_003_240), "16.08.2012 06:40");
        assert_eq!(super::format_xray_game_time(0), "—");
        assert_eq!(super::format_xray_game_time(u64::MAX), "—");
    }

    #[test]
    fn save_switch_waits_while_a_draft_write_is_pending() -> sse_core::Result<()> {
        let directory = std::env::temp_dir().join(format!(
            "sse-switch-wait-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        let workspace = Workspace::with_draft_directory(directory.clone());
        let pending = workspace.begin_draft_write();
        let (proxy, _receiver) = channel_pair::<AppMessage>();
        let mut app = sse_app::state::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };

        super::start_load(&workspace, fixture_slot("switch-b.sav", "stalker-cs", "cs"), &mut cx);
        assert_eq!(
            cx.status.as_deref(),
            Some(crate::strings::t("Дождитесь записи черновика, затем смените сейв."))
        );
        assert!(
            !workspace.is_loading(),
            "the switch must not start while a draft write is pending"
        );

        drop(pending);
        assert!(!workspace.draft_writes_pending());
        super::start_load(&workspace, fixture_slot("switch-b.sav", "stalker-cs", "cs"), &mut cx);
        assert!(
            workspace.is_loading(),
            "the switch starts once the draft write has finished"
        );
        let _ = fs::remove_dir_all(directory);
        Ok(())
    }

    #[test]
    fn earlier_save_identifier_cannot_claim_a_newer_operation() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let session = workspace.session();
        let path = PathBuf::from("fixture.sav");
        let first = session
            .begin_save(&path)
            .ok_or_else(|| Error::Refused("first test save did not start".to_owned()))?;
        let first_id = first.id();
        drop(first);
        let second = session
            .begin_save(&path)
            .ok_or_else(|| Error::Refused("second test save did not start".to_owned()))?;
        assert_ne!(first_id, second.id());

        assert!(!session.is_latest_operation(first_id));
        assert!(workspace.is_saving());
        drop(second);
        assert!(!workspace.is_saving());
        Ok(())
    }

    #[test]
    fn save_and_in_place_restore_requests_are_mutually_exclusive() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let session = workspace.session();
        let path = PathBuf::from("fixture.sav");
        let save_guard = session
            .begin_save(&path)
            .ok_or_else(|| Error::Refused("test save did not start".to_owned()))?;
        assert!(session.begin_restore(&path).is_none());
        drop(save_guard);

        let restore_guard = session
            .begin_restore(&path)
            .ok_or_else(|| Error::Refused("test restore did not start".to_owned()))?;
        assert!(workspace.is_restoring());
        assert!(session.begin_save(&path).is_none());
        drop(restore_guard);
        assert!(!workspace.is_restoring());
        assert!(session.begin_save(&path).is_some());
        Ok(())
    }

    #[test]
    fn backup_directory_is_shared_and_updates_before_settings_are_saved() {
        let workspace = Workspace::with_backup_directory(PathBuf::from("first-backups"));
        assert_eq!(workspace.backup_directory(), PathBuf::from("first-backups"));

        workspace.set_backup_directory(PathBuf::from("next-backups"));

        assert_eq!(workspace.backup_directory(), PathBuf::from("next-backups"));
    }

    #[test]
    fn external_change_banner_takes_no_space_until_the_file_changes() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let (container, label, reload) = add_external_file_banner(&mut tree, host)?;

        assert!(!tree.is_visible(container));
        assert!(!tree.is_visible(label));
        assert!(!tree.is_visible(reload));
        Ok(())
    }

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let id = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sse-save-ui-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create test directory: {error}"));
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_slot(path: &str, format_id: &str, game_id: &str) -> SaveSlot {
        SaveSlot {
            path: PathBuf::from(path),
            candidate_game_id: game_id.to_owned(),
            candidate_release_id: format_id.to_owned(),
            size: 1,
            last_write_time_utc: UNIX_EPOCH,
            format_id: Some(format_id.to_owned()),
            game_id: Some(game_id.to_owned()),
            detection_error: None,
        }
    }

    fn load_xray(bytes: &[u8], path: &str, format_id: &str, game_id: &str) -> sse_core::Result<LoadedSave> {
        let packed = SaveBuffer::from_vec(bytes.to_vec());
        let sha256 = sse_codecs::sha256::sha256_hex(packed.as_slice());
        let save = Save::read(packed.as_slice())?;
        let inventory = save.inventory()?;
        LoadedSave::from_xray(fixture_slot(path, format_id, game_id), packed, sha256, save, inventory)
    }

    #[test]
    fn staged_stack_edit_matches_the_reference_packed_bytes() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-stacks/xray-stack-cop-source.sav");
        let expected = include_bytes!("../../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.sav");
        let loaded = load_xray(source, "xray-stack-cop-source.sav", "stalker-cop", "cop")?;
        let (output, summary) = prepare_xray_edits(&loaded, None, &BTreeMap::from([(0x1234, 44)]))?;

        assert_eq!(output.as_slice(), expected);
        assert_eq!(summary.stack_count, 1);
        Ok(())
    }

    #[test]
    fn staged_money_edit_matches_the_reference_packed_bytes() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let expected = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let loaded = load_xray(source, "xray-money-cop-source.sav", "stalker-cop", "cop")?;
        let new_money = Save::read(expected)?.money()?;
        let (output, summary) = prepare_xray_edits(&loaded, Some(new_money), &BTreeMap::new())?;

        assert_eq!(output.as_slice(), expected);
        assert_eq!(summary.money, Some(new_money));
        Ok(())
    }

    #[test]
    fn browser_save_queues_a_verified_edited_copy_without_changing_the_source() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let expected = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let loaded = load_xray(source, "quicksave.sav", "stalker-cop", "cop")?;
        let source_money = Save::read(source)?.money()?;
        let new_money = Save::read(expected)?.money()?;
        let edits = PendingInventoryEdits {
            money: Some(new_money),
            ..PendingInventoryEdits::default()
        };

        let (reloaded, download) = prepare_browser_save(&loaded, &edits, &BTreeSet::new())?;

        assert_eq!(download.file_name, "quicksave_edited.sav");
        assert_eq!(download.bytes.as_slice(), expected);
        assert_eq!(Save::read(source)?.money()?, source_money);
        assert_eq!(reloaded.source_sha256, sse_codecs::sha256::sha256_hex(expected));
        Ok(())
    }

    #[test]
    fn faction_screen_stages_a_catalogued_relation_for_the_shared_writer() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-factions/cop-source.sav");
        let loaded = Arc::new(load_xray(source, "cop-source.sav", "stalker-cop", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Factions::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.index = screen
            .faction_keys
            .iter()
            .position(|key| key == "bandit")
            .ok_or_else(|| Error::damaged("catalogued bandit faction is missing"))?;
        screen.stage_relation(&mut cx, 1)?;

        let relations = cx
            .app
            .draft(&source_sha256)
            .map(|plan| plan.faction_relations.clone())
            .ok_or_else(|| Error::damaged("faction edit was not added to the draft"))?;
        assert!(relations.contains_key("bandit"));
        let edits = PendingInventoryEdits {
            faction_relations: relations,
            ..PendingInventoryEdits::default()
        };
        let (output, summary) = prepare_save_edits(&loaded, &edits, &BTreeSet::new())?;
        let reloaded = LoadedSave::from_bytes(loaded.slot.clone(), output.as_slice())?;
        super::verify_requested_values(&loaded, &reloaded, &edits, &BTreeSet::new())?;
        assert_eq!(summary.relation_count, 1);
        Ok(())
    }

    #[test]
    fn unsupported_ee_faction_edit_is_refused_before_catalog_lookup() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-factions/cop-ee-source.sav");
        let loaded = Arc::new(load_xray(source, "cop-ee-source.sav", "stalker-cop-ee", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop-ee".to_owned()), false);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Factions::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.faction_keys = vec!["bandit".to_owned()];
        screen.stage_relation(&mut cx, 100)?;

        assert!(cx.app.draft(&source_sha256).is_none());
        assert!(cx
            .status
            .as_deref()
            .is_some_and(|status| status.contains("не поддерживается")));
        Ok(())
    }

    #[test]
    fn xray_stash_screen_stages_a_transfer_for_the_shared_writer() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/xray-stashes/xray-stash-cop-source.sav");
        let expected = include_bytes!("../../../../fixtures/synthetic/xray-stashes/xray-stash-cop-take.sav");
        let loaded = Arc::new(load_xray(source, "stash-cop-source.sav", "stalker-cop", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Stashes::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.stage_xray_move(&mut cx, super::StashAction::XrayTake(9029))?;

        let plan = cx
            .app
            .draft(&source_sha256)
            .cloned()
            .ok_or_else(|| Error::damaged("X-Ray stash transfer was not added to the draft"))?;
        assert_eq!(plan.stash_takes, [9029]);
        let edits = PendingInventoryEdits {
            stash_takes: plan.stash_takes.into_iter().collect(),
            ..PendingInventoryEdits::default()
        };
        let (output, summary) = prepare_save_edits(&loaded, &edits, &BTreeSet::new())?;
        assert_eq!(output.as_slice(), expected);
        let reloaded = LoadedSave::from_bytes(loaded.slot.clone(), output.as_slice())?;
        super::verify_requested_values(&loaded, &reloaded, &edits, &BTreeSet::new())?;
        assert_eq!(summary.move_count, 1);
        Ok(())
    }

    #[test]
    fn s2_stash_row_then_action_stays_disabled_and_writes_nothing() -> sse_core::Result<()> {
        // The row only chooses the item; the one action is off while S2 transfers are unverified, so it changes nothing.
        let temp = TempDirectory::new();
        let path = temp.0.join("stash.sav");
        let original = include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");
        fs::write(&path, original)?;
        let selected = LoadedSave::read(fixture_slot(&path.to_string_lossy(), "stalker2", "stalker2"))?;
        let source_sha256 = selected.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(std::sync::Arc::new(selected));
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker2".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Stashes::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let action = screen
            .action
            .ok_or_else(|| Error::damaged("stash action was not built"))?;
        let select = screen
            .list
            .as_ref()
            .and_then(|list| list.rows.first())
            .map(|row| row.select)
            .ok_or_else(|| Error::damaged("no row for the S2 item"))?;
        let pointer = Message::Window(WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        assert!(
            cx.tree.is_visible(action),
            "the action is hidden after choosing the item"
        );
        assert!(
            !cx.tree.is_enabled(action)?,
            "the S2 action is on while transfers are unverified"
        );
        let _ = screen.message(&mut cx, &pointer, Some(action));
        assert!(cx
            .app
            .draft(&source_sha256)
            .is_none_or(|plan| plan.stash_takes.is_empty()));
        assert_eq!(fs::read(&path)?, original);
        Ok(())
    }

    #[test]
    fn faction_row_then_adjustment_stages_the_chosen_faction() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-factions/cop-source.sav");
        let loaded = Arc::new(load_xray(source, "cop-source.sav", "stalker-cop", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Factions::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let list = screen
            .list
            .as_ref()
            .ok_or_else(|| Error::damaged("factions list was not built"))?;
        let (select, increase) = (
            list.rows
                .get(2)
                .map(|row| row.select)
                .ok_or_else(|| Error::damaged("no third row"))?,
            screen.increase.ok_or_else(|| Error::damaged("no adjustment button"))?,
        );
        let key = screen
            .faction_keys
            .get(2)
            .cloned()
            .ok_or_else(|| Error::damaged("fewer than three factions"))?;
        let pointer = Message::Window(WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        screen.message(&mut cx, &pointer, Some(increase))?;

        let relations = cx
            .app
            .draft(&source_sha256)
            .map(|plan| plan.faction_relations.clone())
            .ok_or_else(|| Error::damaged("the adjustment did not reach the draft"))?;
        assert!(
            relations.contains_key(&key),
            "the draft changed another faction than the chosen row"
        );
        assert_eq!(relations.len(), 1);
        Ok(())
    }

    #[test]
    fn unsupported_ee_stash_move_never_enters_the_draft() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/xray-stashes/xray-stash-cop-ee-source.sav");
        let loaded = Arc::new(load_xray(source, "cop-ee-stash.sav", "stalker-cop-ee", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop-ee".to_owned()), false);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Stashes::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.stage_xray_move(&mut cx, super::StashAction::XrayTake(9029))?;

        assert!(cx.app.draft(&source_sha256).is_none());
        assert!(cx
            .status
            .as_deref()
            .is_some_and(|status| status.contains("не поддерживается")));
        Ok(())
    }

    #[test]
    fn unsupported_ee_transition_confirmation_is_refused() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-factions/cop-ee-source.sav");
        let loaded = Arc::new(load_xray(source, "cop-ee-source.sav", "stalker-cop-ee", "cop")?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::clone(&loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(loaded.slot.path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker-cop-ee".to_owned()), false);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Transitions::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.pending_confirmation = Some(0x1234);
        screen.confirm_relocation(&mut cx)?;

        assert!(cx.app.draft(&source_sha256).is_none());
        assert!(cx
            .status
            .as_deref()
            .is_some_and(|status| status.contains("не поддерживается")));
        Ok(())
    }

    #[test]
    fn inventory_shows_condition_presets_for_a_proven_condition_field() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let loaded = Arc::new(load_xray(
            source,
            "xray-durability-cop-source.sav",
            "stalker-cop",
            "cop",
        )?);
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::default();
        workspace.lock().selected = Some(loaded);
        let mut screen = Inventory::new(workspace);
        let mut app = sse_app::AppState::new();
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

        assert_eq!(
            screen
                .condition_buttons
                .iter()
                .map(|(_, value)| *value)
                .collect::<Vec<_>>(),
            [100, 75, 50]
        );
        let seventy_five = screen
            .condition_buttons
            .iter()
            .find(|(_, value)| *value == 75)
            .map(|(id, _)| *id)
            .ok_or_else(|| Error::damaged("75% durability preset is missing"))?;
        screen.message(&mut cx, &Message::Window(WindowEvent::PointerLeft), Some(seventy_five))?;
        assert_eq!(
            cx.app
                .draft(&source_sha256)
                .and_then(|plan| plan.durability.get(&0x3456)),
            Some(&75)
        );
        Ok(())
    }

    #[test]
    fn inventory_inspector_reserves_space_for_three_line_item_summary() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let loaded = Arc::new(load_xray(
            source,
            "xray-durability-cop-source.sav",
            "stalker-cop",
            "cop",
        )?);
        let workspace = Workspace::default();
        workspace.lock().selected = Some(loaded);
        let mut screen = Inventory::new(workspace);
        let mut app = sse_app::AppState::new();
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
        // The shell leaves roughly this width for the content pane after navigation and the save library.
        cx.tree.resize(720, 800);
        cx.tree.update_layout()?;
        let summary = cx.tree.rect(
            screen
                .inspector_summary
                .ok_or_else(|| Error::damaged("item summary missing"))?,
        )?;
        let condition_heading = cx.tree.rect(
            screen
                .inspector_condition_heading
                .ok_or_else(|| Error::damaged("condition heading missing"))?,
        )?;

        assert!(
            summary.height >= 80,
            "item summary has insufficient height: {summary:?}"
        );
        assert!(
            i64::from(summary.y) + i64::from(summary.height) <= i64::from(condition_heading.y),
            "item summary {summary:?} overlaps condition heading {condition_heading:?}"
        );
        Ok(())
    }

    /// The Inventory with the durability fixture loaded and rendered at the given window size.
    fn rendered_inventory(width: u32, height: u32) -> sse_core::Result<(Inventory, Tree, crate::widget::WidgetId)> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let loaded = Arc::new(load_xray(
            source,
            "xray-durability-cop-source.sav",
            "stalker-cop",
            "cop",
        )?);
        let workspace = Workspace::default();
        workspace.lock().selected = Some(loaded);
        let mut screen = Inventory::new(workspace);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        tree.resize(width, height);
        let mut app = sse_app::AppState::new();
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.render(&mut cx)?;
        cx.tree.update_layout()?;
        Ok((screen, tree, host))
    }

    #[test]
    fn inventory_row_click_selects_the_item_and_keeps_the_stepper_on_the_row() -> sse_core::Result<()> {
        let (mut screen, mut tree, _) = rendered_inventory(1920, 1080)?;
        let row = screen.rows.first().ok_or_else(|| Error::damaged("no item row"))?;
        let clicked = row.select;
        let handle = row.handle.ok_or_else(|| Error::damaged("first row has no item"))?;
        let mut app = sse_app::AppState::new();
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.message(
            &mut cx,
            &Message::Window(crate::event_loop::WindowEvent::Resized {
                width: 1920,
                height: 1080,
            }),
            Some(clicked),
        )?;
        assert_eq!(
            screen.selected_item,
            Some(handle),
            "a click on the row selects its item"
        );
        Ok(())
    }

    #[test]
    fn inventory_layout_fits_the_window_and_compact_hides_the_key_column() -> sse_core::Result<()> {
        for (width, height, compact) in [(1920_u32, 1080_u32, false), (1366, 768, true)] {
            let (screen, tree, _) = rendered_inventory(width, height)?;
            let card = tree.rect(screen.inventory_card.ok_or_else(|| Error::damaged("no centre panel"))?)?;
            let side = tree.rect(screen.side_column.ok_or_else(|| Error::damaged("no side column"))?)?;
            assert!(
                card.x >= 0 && i64::from(card.x) + i64::from(card.width) <= i64::from(width),
                "centre {card:?} inside {width}"
            );
            assert!(
                side.x >= 0 && i64::from(side.x) + i64::from(side.width) <= i64::from(width),
                "side {side:?} inside {width}"
            );
            assert!(
                card.x + i32::try_from(card.width).unwrap_or_default() <= side.x,
                "centre and side do not overlap"
            );
            let key = screen.key_header.ok_or_else(|| Error::damaged("no key header"))?;
            assert_eq!(tree.is_visible(key), !compact, "key column visibility at {width}");
        }
        Ok(())
    }

    #[test]
    fn inventory_actions_and_paging_stay_inside_the_work_area() -> sse_core::Result<()> {
        // The work area is the window minus the shell's header, tabs and status bar (about 240 px at the top and 30 px
        // at the bottom). Every control and column must end above its bottom edge.
        for (width, work_height) in [(1920_u32, 1080_u32 - 270), (1366, 768 - 270)] {
            let (screen, tree, _) = rendered_inventory(width, work_height)?;
            let work_bottom = i64::from(work_height);
            let card = tree.rect(screen.inventory_card.ok_or_else(|| Error::damaged("no centre panel"))?)?;
            let side = tree.rect(screen.side_column.ok_or_else(|| Error::damaged("no side column"))?)?;
            let inspector = tree.rect(screen.inspector_panel.ok_or_else(|| Error::damaged("no inspector"))?)?;
            let actions = tree.rect(screen.actions_panel.ok_or_else(|| Error::damaged("no action panel"))?)?;
            let controls = [
                ("remove", screen.remove_button),
                ("add", screen.add_button),
                ("previous page", screen.previous),
                ("next page", screen.next),
            ];
            let mut rects = vec![
                ("centre", card),
                ("side", side),
                ("inspector", inspector),
                ("actions", actions),
            ];
            for (name, id) in controls {
                let id = id.ok_or_else(|| Error::damaged("missing inventory control"))?;
                rects.push((name, tree.rect(id)?));
            }
            for (name, rect) in rects {
                assert!(
                    rect.x >= 0 && rect.y >= 0,
                    "{name} starts outside at {width}x{work_height}: {rect:?}"
                );
                assert!(
                    i64::from(rect.x) + i64::from(rect.width) <= i64::from(width),
                    "{name} runs past the right edge at {width}: {rect:?}"
                );
                assert!(
                    i64::from(rect.y) + i64::from(rect.height) <= work_bottom,
                    "{name} runs below the work area at {width}x{work_height}: {rect:?}"
                );
            }
            assert_eq!(
                card.y + i32::try_from(card.height).unwrap_or_default(),
                side.y + i32::try_from(side.height).unwrap_or_default(),
                "centre and side columns end on the same line"
            );
        }
        Ok(())
    }

    #[test]
    fn inventory_shows_at_least_five_rows_at_1366x768() -> sse_core::Result<()> {
        // The shell header and tabs take about 240 px of the 768 px window; the inventory gets the rest.
        let (screen, _, _) = rendered_inventory(1366, 768 - 240)?;
        assert!(
            screen.page_size >= 5,
            "only {} rows fit in the 1366x768 window",
            screen.page_size
        );
        Ok(())
    }

    #[test]
    fn inventory_add_panel_takes_the_inspector_place() -> sse_core::Result<()> {
        let (mut screen, mut tree, _) = rendered_inventory(1920, 1080)?;
        let inspector = screen.inspector_panel.ok_or_else(|| Error::damaged("no inspector"))?;
        let panel = screen.add_panel.ok_or_else(|| Error::damaged("no add panel"))?;
        let mut app = sse_app::AppState::new();
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        assert!(cx.tree.is_visible(inspector));
        assert!(!cx.tree.is_visible(panel));
        screen.open_add_panel(&mut cx)?;
        assert!(!cx.tree.is_visible(inspector), "the inspector steps aside while adding");
        assert!(cx.tree.is_visible(panel));
        screen.close_add_panel(&mut cx)?;
        assert!(cx.tree.is_visible(inspector));
        assert!(!cx.tree.is_visible(panel));
        Ok(())
    }

    #[test]
    fn inventory_add_panel_fits_the_work_area_with_the_chosen_item() -> sse_core::Result<()> {
        // The add panel fills the inspector's place; its buttons and the draft note stay inside the work area and
        // the chosen candidate's name and key are shown above the quantity.
        for (width, work_height) in [(1920_u32, 1080_u32 - 270), (1366, 768 - 270)] {
            let (mut screen, mut tree, _) = rendered_inventory(width, work_height)?;
            let mut app = sse_app::AppState::new();
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.open_add_panel(&mut cx)?;
            // This save has no item that a catalog entry can be templated from, so offer the first candidate as if it
            // had one, and render the choice: the name and key of the chosen candidate must show.
            if let Some(candidate) = screen.add_candidates.first_mut() {
                candidate.template_available = true;
                screen.add_selected_key = Some(candidate.key.clone());
            }
            screen.render_add_panel(&mut cx)?;
            cx.tree.update_layout()?;
            let work_bottom = i64::from(work_height);
            let panel = screen.add_panel.ok_or_else(|| Error::damaged("no add panel"))?;
            let panel_rect = cx.tree.rect(panel)?;
            let panel_bottom = i64::from(panel_rect.y) + i64::from(panel_rect.height);
            assert!(
                panel_bottom <= work_bottom,
                "add panel runs below the work area at {width}x{work_height}"
            );
            let inside = [
                ("confirm", screen.add_confirm),
                ("cancel", screen.add_cancel),
                ("draft note", screen.add_draft_note),
            ];
            for (name, id) in inside {
                let id = id.ok_or_else(|| Error::damaged("missing add-panel control"))?;
                let rect = cx.tree.rect(id)?;
                assert!(cx.tree.is_visible(id), "{name} is hidden at {width}x{work_height}");
                assert!(
                    i64::from(rect.y) + i64::from(rect.height) <= panel_bottom,
                    "{name} runs below the add panel at {width}x{work_height}: {rect:?}"
                );
            }
            let name = screen
                .add_selected_name
                .ok_or_else(|| Error::damaged("no selected name"))?;
            assert!(cx.tree.is_visible(name), "the chosen candidate's name is not shown");
            assert!(!cx.tree.text(name)?.is_empty(), "the chosen candidate has no name");
            let key = screen
                .add_selected_key_label
                .ok_or_else(|| Error::damaged("no selected key"))?;
            assert!(cx.tree.is_visible(key), "the chosen candidate's key is not shown");
        }
        Ok(())
    }

    #[test]
    fn durability_draft_matches_reference_bytes_and_read_back() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let expected =
            include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-expected.sav");
        let loaded = load_xray(source, "xray-durability-cop-source.sav", "stalker-cop", "cop")?;
        let edits = super::PendingInventoryEdits {
            durability: BTreeMap::from([(super::ItemHandle::Xray(0x3456), 75)]),
            ..super::PendingInventoryEdits::default()
        };
        let (output, _) = prepare_save_edits(&loaded, &edits, &BTreeSet::new())?;

        assert_eq!(output.as_slice(), expected);
        let reloaded = load_xray(
            output.as_slice(),
            "xray-durability-cop-expected.sav",
            "stalker-cop",
            "cop",
        )?;
        super::verify_requested_values(&loaded, &reloaded, &edits, &BTreeSet::new())?;
        Ok(())
    }

    #[test]
    fn add_item_draft_uses_a_catalogued_template_and_clears_clone_metadata() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav");
        let expected = Save::read(include_bytes!(
            "../../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-expected.sav"
        ))?;
        let loaded = load_xray(source, "xray-add-cop-ammo-source.sav", "stalker-cop", "cop")?;
        let edits = super::PendingInventoryEdits {
            adds: vec![AddRequest::new("ammo_9x39_pab9", 17, "inventory")?],
            ..super::PendingInventoryEdits::default()
        };
        let (output, _) = prepare_save_edits(&loaded, &edits, &BTreeSet::new())?;
        let reloaded = LoadedSave::from_bytes(loaded.slot.clone(), output.as_slice())?;
        super::verify_requested_values(&loaded, &reloaded, &edits, &BTreeSet::new())?;
        assert_eq!(reloaded.slot.format_id.as_deref(), Some("stalker-cop"));
        let actual = Save::read(output.as_slice())?;
        let added = actual
            .registry_objects()
            .iter()
            .find(|record| record.object_id == 4661)
            .ok_or_else(|| Error::damaged("added inventory object is missing after read-back"))?;
        assert_eq!(added.name_replace, "");
        assert_eq!(added.spawn_id, Some(u16::MAX));
        assert_eq!(added.story_id, Some(u32::MAX));
        assert_eq!(added.spawn_story_id, Some(u32::MAX));
        assert_eq!(actual.custom_data(added), Some(&[][..]));
        let actual_inventory = actual.inventory()?;
        let expected_inventory = expected.inventory()?;
        assert_eq!(
            actual_inventory
                .iter()
                .map(|item| (item.handle, item.section.as_str(), item.count))
                .collect::<Vec<_>>(),
            expected_inventory
                .iter()
                .map(|item| (item.handle, item.section.as_str(), item.count))
                .collect::<Vec<_>>()
        );
        Ok(())
    }

    #[test]
    fn add_template_prefers_unbound_metadata_and_spawn_ffff() {
        let clean = super::add_template_preference(Some(u32::MAX), Some(u32::MAX), false, Some(u16::MAX), 20);
        let story_bound = super::add_template_preference(Some(7), Some(u32::MAX), false, Some(u16::MAX), 1);
        let custom_bound = super::add_template_preference(Some(u32::MAX), Some(u32::MAX), true, Some(u16::MAX), 2);
        let spawn_bound = super::add_template_preference(Some(u32::MAX), Some(u32::MAX), false, Some(9), 3);

        assert!(clean < story_bound);
        assert!(clean < custom_bound);
        assert!(clean < spawn_bound);
    }

    #[test]
    fn add_catalog_shows_template_backed_items_and_disables_unsafe_templates() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav");
        let loaded = load_xray(source, "xray-add-cop-ammo-source.sav", "stalker-cop", "cop")?;
        let candidates = super::add_candidates(&loaded, &BTreeSet::new());

        let candidate = candidates
            .iter()
            .find(|candidate| candidate.key == "ammo_9x39_pab9")
            .ok_or_else(|| Error::damaged("catalog candidate is missing"))?;
        assert!(candidate.template_available);
        assert!(!candidate.display_name.is_empty());
        Ok(())
    }

    #[test]
    fn cancelling_add_panel_restores_the_previous_focus() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-add/xray-add-cop-ammo-source.sav");
        let loaded = load_xray(source, "xray-add-cop-ammo-source.sav", "stalker-cop", "cop")?;
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(PathBuf::from("fixture.sav"), source_sha256.clone());
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut inventory = Inventory::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        inventory.build(&mut cx, host)?;
        let previous_focus = inventory
            .search_widget
            .ok_or_else(|| Error::damaged("inventory search is missing"))?;
        let add_button = inventory
            .add_button
            .ok_or_else(|| Error::damaged("add button is missing"))?;
        let cancel_button = inventory
            .add_cancel
            .ok_or_else(|| Error::damaged("cancel button is missing"))?;
        cx.tree.set_focus(Some(previous_focus))?;
        inventory.message(&mut cx, &Message::User(AppMessage::Tick(3)), Some(add_button))?;
        inventory.message(&mut cx, &Message::User(AppMessage::Tick(3)), Some(cancel_button))?;

        assert_eq!(cx.tree.focused(), Some(previous_focus));
        Ok(())
    }

    #[test]
    fn identical_unit_items_share_one_inventory_row() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-durability/xray-durability-cop-source.sav");
        let loaded = load_xray(source, "xray-durability-cop-source.sav", "stalker-cop", "cop")?;
        let super::SaveData::Xray { save, inventory } = &loaded.data else {
            return Err(Error::damaged("X-Ray fixture parsed as S2"));
        };
        let item = inventory
            .iter()
            .find(|item| item.count.is_none())
            .ok_or_else(|| Error::damaged("fixture has no individually represented item"))?;
        let groups = super::group_xray_items(save, vec![item, item], &super::WorkspaceState::default());
        assert_eq!(groups.len(), 1);
        assert_eq!(groups.first().map(Vec::len), Some(2));
        Ok(())
    }

    #[test]
    fn identical_s2_items_share_a_row_until_one_is_edited() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav");
        let packed = SaveBuffer::from_vec(source.to_vec());
        let source_sha256 = sse_codecs::sha256::sha256_hex(packed.as_slice());
        let save = S2Save::from_bytes(packed.as_slice())?;
        let stash = save.stash().ok();
        let mut item = save
            .items()
            .into_iter()
            .next()
            .ok_or_else(|| Error::damaged("S2 fixture has no inventory item"))?;
        item.editable_count = false;
        item.count = 1;
        item.condition = None;
        item.condition_offset = None;
        item.modules.clear();
        item.upgrades.clear();
        let mut duplicate = item.clone();
        duplicate.handle = duplicate.handle.saturating_add(1);
        duplicate.x = duplicate.x.map(|value| value.saturating_add(1));
        duplicate.record_offset = duplicate.record_offset.saturating_add(1);
        duplicate.count_offset = duplicate.count_offset.saturating_add(1);
        let loaded = LoadedSave::from_s2(
            fixture_slot("s2-stacks-source.sav", "stalker2", "stalker2"),
            packed,
            source_sha256.clone(),
            save,
            vec![item, duplicate.clone()],
            stash,
        );
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(PathBuf::from("s2-stacks-source.sav"), source_sha256.clone());
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = Inventory::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };

        screen.build(&mut cx, host)?;
        assert_eq!(screen.rows.iter().filter(|row| cx.tree.is_visible(row.row)).count(), 1);

        workspace
            .lock()
            .pending_stacks
            .insert(super::ItemHandle::Stalker2(duplicate.handle), 2);
        screen.render(&mut cx)?;
        assert_eq!(screen.rows.iter().filter(|row| cx.tree.is_visible(row.row)).count(), 2);
        Ok(())
    }

    #[test]
    fn s2_money_edit_uses_the_verified_writer() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav");
        let packed = SaveBuffer::from_vec(source.to_vec());
        let sha256 = sse_codecs::sha256::sha256_hex(packed.as_slice());
        let save = S2Save::from_bytes(packed.as_slice())?;
        let inventory = save.items();
        let stash = save.stash().ok();
        let loaded = LoadedSave::from_s2(
            fixture_slot("s2-stacks-source.sav", "stalker2", "stalker2"),
            packed,
            sha256,
            save,
            inventory,
            stash,
        );

        let (output, summary) = prepare_save_edits(
            &loaded,
            &PendingInventoryEdits {
                money: Some(1_000),
                ..PendingInventoryEdits::default()
            },
            &BTreeSet::new(),
        )?;
        assert_eq!(S2Save::from_bytes(output.as_slice())?.money(), 1_000);
        assert_eq!(summary.money, Some(1_000));
        Ok(())
    }

    #[test]
    fn s2_stash_transfer_transaction_refuses_unverified_ui_writes() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let saves = temp.0.join("saves");
        fs::create_dir_all(&saves)?;
        let backup = temp.0.join("backups");
        let path = saves.join("stash.sav");
        fs::write(
            &path,
            include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav"),
        )?;
        let path_string = path.to_string_lossy().into_owned();
        let selected = LoadedSave::read(fixture_slot(&path_string, "stalker2", "stalker2"))?;
        let handle = match &selected.data {
            super::SaveData::Stalker2 { save, stash_items, .. } => {
                assert!(!save.index().is_legacy());
                assert!(save.unresolved_handles().is_empty());
                let item = stash_items
                    .as_ref()
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no stash index"))?
                    .as_ref()
                    .map_err(|error| Error::damaged(error.clone()))?
                    .first()
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no items"))?;
                item.handle
            }
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };

        fs::create_dir_all(&backup)?;
        let result = commit_save_edits_to(
            &selected,
            &PendingInventoryEdits::default(),
            &BTreeSet::from([handle]),
            &backup,
        );
        assert!(result.is_err_and(|error| error
            .to_string()
            .contains("S2 stash transfer is disabled until the saved result is validated in-game")));
        assert_eq!(
            fs::read(&path)?,
            include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav")
        );
        let backups = sse_storage::transaction::list_backups(&backup)?;
        assert!(backups.is_empty());
        Ok(())
    }

    #[test]
    fn s2_stash_screen_keeps_transfer_disabled_until_game_validation() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("stash.sav");
        let original = include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");
        fs::write(&path, original)?;
        let selected = LoadedSave::read(fixture_slot(&path.to_string_lossy(), "stalker2", "stalker2"))?;
        let source_sha256 = selected.source_sha256.clone();
        let handle = match &selected.data {
            super::SaveData::Stalker2 { save, stash_items, .. } => {
                assert!(!save.index().is_legacy());
                assert!(save.unresolved_handles().is_empty());
                stash_items
                    .as_ref()
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no stash index"))?
                    .as_ref()
                    .map_err(|error| Error::damaged(error.clone()))?
                    .first()
                    .map(|item| item.handle)
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no items"))?
            }
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(std::sync::Arc::new(selected));
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker2".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Stashes::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;

        // Nothing is chosen yet, so the one action stays hidden; the entries are rows of the list.
        let move_button = screen
            .action
            .ok_or_else(|| Error::damaged("S2 stash action was not built"))?;
        assert!(!cx.tree.is_visible(move_button));
        screen.move_item(&mut cx, handle)?;
        assert!(cx.status.as_deref().is_some_and(|text| text.contains("отключён")));
        assert!(!workspace.lock().pending_stash_moves.contains(&handle));
        assert_eq!(
            cx.app.draft(&source_sha256).map(|plan| plan.s2_stash_takes.as_slice()),
            Some(&[][..])
        );
        assert_eq!(fs::read(&path)?, original);
        Ok(())
    }

    #[test]
    fn s2_stash_screen_preserves_an_existing_unverified_transfer_draft() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("stash.sav");
        let original = include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");
        fs::write(&path, original)?;
        let selected = LoadedSave::read(fixture_slot(&path.to_string_lossy(), "stalker2", "stalker2"))?;
        let source_sha256 = selected.source_sha256.clone();
        let handle = match &selected.data {
            super::SaveData::Stalker2 { stash_items, .. } => stash_items
                .as_ref()
                .ok_or_else(|| Error::damaged("S2 stash fixture has no stash index"))?
                .as_ref()
                .map_err(|error| Error::damaged(error.clone()))?
                .first()
                .map(|item| item.handle)
                .ok_or_else(|| Error::damaged("S2 stash fixture has no items"))?,
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };
        let already_pending = handle.wrapping_add(1);
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(selected));
        workspace.lock().pending_stash_moves.insert(already_pending);
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker2".to_owned()), false);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.s2_stash_takes.push(already_pending);
        app.set_draft_journal(DraftJournal::new(vec![plan], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = super::Stashes::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.move_item(&mut cx, handle)?;

        assert!(cx.status.as_deref().is_some_and(|text| text.contains("отключён")));
        assert_eq!(workspace.lock().pending_stash_moves, BTreeSet::from([already_pending]));
        assert_eq!(
            cx.app.draft(&source_sha256).map(|plan| plan.s2_stash_takes.as_slice()),
            Some(&[already_pending][..])
        );
        assert_eq!(fs::read(&path)?, original);
        Ok(())
    }

    #[test]
    fn inventory_blocks_save_for_an_unverified_stash_draft() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("stash.sav");
        let original = include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");
        fs::write(&path, original)?;
        let selected = LoadedSave::read(fixture_slot(&path.to_string_lossy(), "stalker2", "stalker2"))?;
        let handle = match &selected.data {
            super::SaveData::Stalker2 { stash_items, .. } => stash_items
                .as_ref()
                .ok_or_else(|| Error::damaged("S2 stash fixture has no stash index"))?
                .as_ref()
                .map_err(|error| Error::damaged(error.clone()))?
                .first()
                .map(|item| item.handle)
                .ok_or_else(|| Error::damaged("S2 stash fixture has no items"))?,
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };
        let source_sha256 = selected.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(selected));
        workspace.lock().pending_stash_moves.insert(handle);
        let mut plan = DraftPlan::empty(&source_sha256)?;
        plan.s2_stash_takes.push(handle);
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(path.clone(), source_sha256.clone());
        app.set_current_save_format(Some("stalker2".to_owned()), false);
        app.set_draft_journal(DraftJournal::new(vec![plan], 0)?);
        let (proxy, _receiver) = channel_pair::<AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut screen = Inventory::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;

        let save_button = screen.export.ok_or_else(|| Error::damaged("save button is missing"))?;
        assert!(!cx.tree.is_visible(save_button));
        screen.save(&mut cx)?;
        assert!(cx.status.as_deref().is_some_and(|text| text.contains("отключён")));
        assert_eq!(workspace.lock().pending_stash_moves, BTreeSet::from([handle]));
        assert_eq!(
            cx.app
                .draft(&source_sha256)
                .map(|draft| draft.s2_stash_takes.as_slice()),
            Some(&[handle][..])
        );
        assert_eq!(fs::read(&path)?, original);
        Ok(())
    }

    #[test]
    fn stash_move_readback_rejects_changed_item_count() -> sse_core::Result<()> {
        let original_bytes = include_bytes!("../../../../fixtures/synthetic/writer-s2-stash/s2-stash-source.sav");
        let slot = fixture_slot("stash.sav", "stalker2", "stalker2");
        let original = LoadedSave::from_bytes(slot.clone(), original_bytes)?;
        let (handle, count) = match &original.data {
            super::SaveData::Stalker2 { save, stash_items, .. } => {
                let item = stash_items
                    .as_ref()
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no stash index"))?
                    .as_ref()
                    .map_err(|error| Error::damaged(error.clone()))?
                    .first()
                    .ok_or_else(|| Error::damaged("S2 stash fixture has no items"))?;
                let moved = save.write_changes(&[S2Change::MoveStashToBackpack { handle: item.handle }])?;
                (item.handle, LoadedSave::from_bytes(slot.clone(), &moved)?)
            }
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };
        let mut corrupted = count;
        let item_count = match &mut corrupted.data {
            super::SaveData::Stalker2 { inventory, .. } => {
                let item = inventory
                    .iter_mut()
                    .find(|item| item.handle == handle)
                    .ok_or_else(|| Error::damaged("moved item is missing from the read-back inventory"))?;
                item.count = item.count.saturating_add(1);
                item.count
            }
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 read-back parsed as X-Ray")),
        };
        assert!(item_count > 0);
        assert!(super::verify_requested_values(
            &original,
            &corrupted,
            &PendingInventoryEdits::default(),
            &BTreeSet::from([handle]),
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn headless_fixture_money_edit_creates_backup_writes_and_reads_back_xray_and_s2() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let saves = temp.0.join("saves");
        fs::create_dir_all(&saves)?;
        let backup = temp.0.join("backups");
        let xray_source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let xray_path = saves.join("xray.sav");
        fs::write(&xray_path, xray_source)?;
        let xray_slot = fixture_slot(&xray_path.to_string_lossy(), "stalker-cop", "cop");
        let xray = LoadedSave::read(xray_slot)?;
        let xray_money = match &xray.data {
            super::SaveData::Xray { save, .. } => save.money()?.saturating_add(321),
            super::SaveData::Stalker2 { .. } => return Err(Error::damaged("X-Ray fixture parsed as S2")),
        };
        let (xray_after, _) = commit_save_edits_to(
            &xray,
            &PendingInventoryEdits {
                money: Some(xray_money),
                ..PendingInventoryEdits::default()
            },
            &BTreeSet::new(),
            &backup,
        )?;
        assert!(matches!(
            &xray_after.data,
            super::SaveData::Xray { save, .. } if save.money().ok() == Some(xray_money)
        ));
        let xray_backup = sse_storage::transaction::list_backups(&backup)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::damaged("X-Ray write did not create a backup journal"))?;
        let xray_journal = fs::read_to_string(xray_backup.journal_path)?;
        assert!(xray_journal.contains(&format!(
            "\"operation\":{{\"mode\":\"replace\",\"money\":{xray_money},\"stack_count\":0,\"move_count\":0,\"detach_count\":0,\"attach_count\":0,\"raw_count\":0,\"add_count\":0,\"durability_count\":0,\"upgrade_count\":0,\"relation_count\":0,\"player_faction\":false}}"
        )));

        let xray_stack_source =
            include_bytes!("../../../../fixtures/synthetic/writer-stacks/xray-stack-cop-source.sav");
        let xray_stack_path = saves.join("xray-stack.sav");
        fs::write(&xray_stack_path, xray_stack_source)?;
        let xray_stack = LoadedSave::read(fixture_slot(&xray_stack_path.to_string_lossy(), "stalker-cop", "cop"))?;
        let xray_stack_money = match &xray_stack.data {
            super::SaveData::Xray { save, .. } => save.money()?.saturating_add(777),
            super::SaveData::Stalker2 { .. } => return Err(Error::damaged("X-Ray stack fixture parsed as S2")),
        };
        let stack_backup = temp.0.join("xray-stack-backups");
        let (xray_stack_after, _) = commit_save_edits_to(
            &xray_stack,
            &PendingInventoryEdits {
                money: Some(xray_stack_money),
                stacks: BTreeMap::from([(ItemHandle::Xray(0x1234), 44)]),
                ..PendingInventoryEdits::default()
            },
            &BTreeSet::new(),
            &stack_backup,
        )?;
        assert!(matches!(
            &xray_stack_after.data,
            super::SaveData::Xray { save, inventory }
                if save.money().ok() == Some(xray_stack_money)
                    && inventory.iter().any(|item| item.handle == 0x1234 && item.count == Some(44))
        ));
        let stack_journal = sse_storage::transaction::list_backups(&stack_backup)?
            .into_iter()
            .next()
            .ok_or_else(|| Error::damaged("X-Ray stack write did not create a backup journal"))?;
        let stack_journal = fs::read_to_string(stack_journal.journal_path)?;
        assert!(stack_journal.contains(&format!(
            "\"operation\":{{\"mode\":\"replace\",\"money\":{xray_stack_money},\"stack_count\":1,\"move_count\":0,\"detach_count\":0,\"attach_count\":0,\"raw_count\":0,\"add_count\":0,\"durability_count\":0,\"upgrade_count\":0,\"relation_count\":0,\"player_faction\":false}}"
        )));

        let s2_source = include_bytes!("../../../../fixtures/synthetic/writer-s2-stacks/s2-stacks-source.sav");
        let s2_path = saves.join("s2.sav");
        fs::write(&s2_path, s2_source)?;
        let s2_slot = fixture_slot(&s2_path.to_string_lossy(), "stalker2", "stalker2");
        let s2 = LoadedSave::read(s2_slot)?;
        let s2_money = match &s2.data {
            super::SaveData::Stalker2 { save, .. } => save.money().saturating_add(654),
            super::SaveData::Xray { .. } => return Err(Error::damaged("S2 fixture parsed as X-Ray")),
        };
        let (s2_after, _) = commit_save_edits_to(
            &s2,
            &PendingInventoryEdits {
                money: Some(s2_money),
                ..PendingInventoryEdits::default()
            },
            &BTreeSet::new(),
            &temp.0.join("s2-backups"),
        )?;
        assert!(matches!(
            &s2_after.data,
            super::SaveData::Stalker2 { save, .. } if save.money() == s2_money
        ));
        assert_eq!(S2Save::from_bytes(&fs::read(s2_path)?)?.money(), s2_money);
        assert!(sse_storage::transaction::list_backups(&backup)?
            .iter()
            .any(|entry| { entry.status == sse_storage::transaction::BackupStatus::Verified }));
        assert!(sse_storage::transaction::list_backups(&temp.0.join("s2-backups"))?
            .iter()
            .any(|entry| entry.status == sse_storage::transaction::BackupStatus::Verified));
        Ok(())
    }

    #[test]
    fn background_fixture_load_updates_the_shared_app_state_via_to_screen() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("fixture.sav");
        fs::write(
            &path,
            include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"),
        )?;
        let expected_path = fs::canonicalize(&path)?;
        let (proxy, receiver) = channel_pair::<AppMessage>();
        let mut overview = Overview::new(Workspace::with_draft_directory(temp.0.join("drafts")));
        let mut app = sse_app::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: Some(&proxy),
                status: None,
                app: &mut app,
            };
            overview.build(&mut cx, host)?;
            assert!(overview.open_save(&mut cx, &path)?);
        }
        let message = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|error| Error::System(error.to_string()))?;
        let completion = match &message {
            Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) => payload.downcast_ref::<LoadFinished>(),
            _ => None,
        };
        assert!(completion.is_some(), "unexpected background message: {message:?}");
        let completion = completion.ok_or_else(|| Error::damaged("missing save-load completion"))?;
        assert_eq!(completion.request, overview.workspace.lock().load_request);
        assert_eq!(
            completion.selected_path.as_deref(),
            Some(expected_path.as_path()),
            "background loader did not return the fixture path"
        );
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        overview.message(&mut cx, &message, None)?;
        assert_eq!(cx.app.current_save(), Some(expected_path.as_path()));
        assert_eq!(cx.app.current_save_format(), Some("stalker-cop"));
        assert!(!cx.app.current_save_is_legacy());
        let source_sha256 = sse_codecs::sha256::sha256_hex(include_bytes!(
            "../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"
        ));
        assert_eq!(cx.app.current_save_sha256(), Some(source_sha256.as_str()));
        assert!(cx.app.draft_journal(&source_sha256).is_some());
        assert!(!cx.app.has_draft(&source_sha256));
        Ok(())
    }

    #[test]
    fn startup_interrupted_backup_check_offers_recovery() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let saves = temp.0.join("saves");
        let backups = temp.0.join("backups");
        fs::create_dir_all(&saves)?;
        let source = saves.join("slot.sav");
        let original = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-soc-source.sav");
        let replacement = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-soc-expected.sav");
        fs::write(&source, original)?;
        let (receipt, (), ()) = sse_storage::transaction::replace_transaction(
            &sse_storage::transaction::StdFileSystem,
            sse_storage::transaction::ReplacementRequest::new(
                &source,
                &sse_codecs::sha256::sha256_hex(original),
                replacement,
                &backups,
            ),
            |_, _| Ok(()),
            |_| Ok(()),
        )?;
        let verified = fs::read_to_string(&receipt.journal_path)?;
        fs::write(
            &receipt.journal_path,
            verified.replace("\"status\":\"verified\"", "\"status\":\"prepared\""),
        )?;

        let workspace = Workspace::with_paths(temp.0.join("drafts"), backups);
        let mut overview = Overview::new(workspace);
        let (proxy, receiver) = channel_pair::<AppMessage>();
        let mut app = sse_app::state::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: Some(&proxy),
                status: None,
                app: &mut app,
            };
            overview.build(&mut cx, host)?;
            overview.startup_backup_check(&mut cx)?;
        }

        let message = receiver
            .recv_timeout(std::time::Duration::from_secs(10))
            .map_err(|error| Error::System(error.to_string()))?;
        let result = match &message {
            Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) => {
                payload.downcast_ref::<StartupBackupCheck>()
            }
            _ => None,
        };
        assert!(result.is_some(), "unexpected background message: {message:?}");
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: Some(&proxy),
                status: None,
                app: &mut app,
            };
            overview.message(&mut cx, &message, None)?;
            assert!(cx
                .status
                .as_deref()
                .is_some_and(|status| status.starts_with("Обнаружена прерванная запись сейва (1).")));
        }
        let button = overview
            .backup_recovery_button
            .ok_or_else(|| Error::damaged("recovery button was not built"))?;
        assert!(tree.is_visible(button));
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: Some(&proxy),
                status: None,
                app: &mut app,
            };
            overview.message(&mut cx, &Message::User(AppMessage::Tick(0)), Some(button))?;
        }
        assert!(matches!(
            receiver.recv_timeout(std::time::Duration::from_secs(1)),
            Ok(Message::User(AppMessage::OpenBackups))
        ));
        Ok(())
    }

    #[test]
    fn inventory_draft_persists_and_global_undo_redo_updates_the_selected_save() -> sse_core::Result<()> {
        let _task_guard = crate::screens::task_registry_test_guard();
        let temp = TempDirectory::new();
        let draft_directory = temp.0.join("drafts");
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let loaded = load_xray(source, "fixture.sav", "stalker-cop", "cop")?;
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(draft_directory.clone());
        workspace.lock().selected = Some(Arc::new(loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(PathBuf::from("fixture.sav"), source_sha256.clone());
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let (proxy, receiver) = channel_pair::<AppMessage>();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut inventory = Inventory::new(workspace.clone());
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        inventory.build(&mut cx, host)?;
        let original_money = match &workspace
            .lock()
            .selected
            .as_ref()
            .map(Arc::as_ref)
            .ok_or_else(|| Error::damaged("loaded fixture was not selected"))?
            .data
        {
            super::SaveData::Xray { save, .. } => save.money()?,
            super::SaveData::Stalker2 { .. } => return Err(Error::damaged("X-Ray fixture parsed as S2")),
        };
        inventory.stage_money_value(&mut cx, original_money.saturating_add(77))?;
        let _ = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| Error::System(format!("draft persistence timed out: {error}")))?;
        assert_eq!(
            cx.app.draft(&source_sha256).and_then(|draft| draft.money),
            Some(original_money.saturating_add(77))
        );
        assert!(DraftStore::for_source(&draft_directory, "fixture.sav")
            .load(&source_sha256)?
            .is_some_and(|journal| journal.current().and_then(|draft| draft.money) == Some(original_money + 77)));

        inventory.editor_action(super::EditorAction::Undo, &mut cx)?;
        let _ = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| Error::System(format!("draft undo persistence timed out: {error}")))?;
        assert_eq!(cx.app.draft(&source_sha256).and_then(|draft| draft.money), None);
        assert!(DraftStore::for_source(&draft_directory, "fixture.sav")
            .load(&source_sha256)?
            .is_none());

        inventory.editor_action(super::EditorAction::Redo, &mut cx)?;
        let _ = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| Error::System(format!("draft redo persistence timed out: {error}")))?;
        assert_eq!(
            cx.app.draft(&source_sha256).and_then(|draft| draft.money),
            Some(original_money.saturating_add(77))
        );
        assert!(sse_app::tasks::wait_for_named_tasks(
            &["draft-save"],
            std::time::Duration::from_secs(1)
        ));
        Ok(())
    }

    #[test]
    fn inventory_rejects_invalid_money_text_without_discarding_it() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let loaded = load_xray(source, "fixture.sav", "stalker-cop", "cop")?;
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(PathBuf::from("fixture.sav"), source_sha256.clone());
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut inventory = Inventory::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        inventory.build(&mut cx, host)?;
        let field = inventory
            .money_input_widget
            .ok_or_else(|| Error::damaged("money input is missing"))?;
        cx.tree.set_focus(Some(field))?;
        inventory.message(
            &mut cx,
            &Message::Window(WindowEvent::Key {
                pressed: true,
                keysym: u32::from(b'a'),
                text: Some('a'),
                ctrl: false,
                shift: false,
            }),
            None,
        )?;
        assert!(cx.app.has_invalid_numeric_input());
        let text = inventory
            .money_input
            .as_ref()
            .map(|input| input.text())
            .ok_or_else(|| Error::damaged("money input model is missing"))?;
        assert!(text.ends_with('a'));
        inventory.message(
            &mut cx,
            &Message::Window(WindowEvent::Key {
                pressed: true,
                keysym: 0xff0d,
                text: None,
                ctrl: false,
                shift: false,
            }),
            None,
        )?;
        assert!(cx.app.has_invalid_numeric_input());
        assert_eq!(cx.tree.focused(), Some(field));
        Ok(())
    }

    #[test]
    fn money_increment_reports_invalid_input_instead_of_silent_noop() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let loaded = load_xray(source, "fixture.sav", "stalker-cop", "cop")?;
        let source_sha256 = loaded.source_sha256.clone();
        let workspace = Workspace::with_draft_directory(temp.0.join("drafts"));
        workspace.lock().selected = Some(Arc::new(loaded));
        let mut app = sse_app::state::AppState::new();
        app.set_current_save_identity(PathBuf::from("fixture.sav"), source_sha256.clone());
        app.set_draft_journal(DraftJournal::new(vec![DraftPlan::empty(&source_sha256)?], 0)?);
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut inventory = Inventory::new(workspace);
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        inventory.build(&mut cx, host)?;
        let button = inventory
            .money_buttons
            .first()
            .map(|(id, _)| *id)
            .ok_or_else(|| Error::damaged("money increment button is missing"))?;

        for invalid in ["x", "2000000001"] {
            inventory.money_input = Some(super::TextInput::new(invalid, super::money_input_config())?);
            cx.app.set_invalid_numeric_input(false);
            cx.status = None;

            inventory.message(
                &mut cx,
                &Message::Window(WindowEvent::Button {
                    button: 1,
                    pressed: false,
                    x: 0,
                    y: 0,
                }),
                Some(button),
            )?;

            assert_eq!(
                cx.status.as_deref(),
                Some(super::t("Введены некорректные значения (проверьте введённые числа)."))
            );
            assert!(cx.app.has_invalid_numeric_input());
            assert_eq!(
                inventory.money_input.as_ref().map(super::TextInput::text).as_deref(),
                Some(invalid)
            );
        }
        Ok(())
    }

    #[test]
    fn inventory_category_filters_cover_reference_groups() {
        assert_eq!(super::xray_inventory_category("Оружие", "wpn_ak74"), "ОРУЖИЕ");
        assert_eq!(
            super::xray_inventory_category("Патроны", "ammo_5.45x39_fmj"),
            "БОЕПРИПАСЫ"
        );
        assert_eq!(super::xray_inventory_category("Артефакт", "af_medusa"), "АРТЕФАКТЫ");
        assert_eq!(super::s2_inventory_category(5, Some("ammo")), "БОЕПРИПАСЫ");
        assert_eq!(super::s2_inventory_category(3, None), "ПРОЧЕЕ");
    }

    #[test]
    fn process_enumeration_error_requires_explicit_save_confirmation() {
        assert_eq!(
            super::process_check_prompt(&Err("permission denied".to_owned())),
            Some((
                "Не удалось проверить запущенную игру: permission denied. Сохранение не проверено.".to_owned(),
                "Сохранить всё равно"
            ))
        );
        assert_eq!(super::process_check_prompt(&Ok(false)), None);
    }

    #[test]
    fn overview_details_switch_between_row_and_stack_by_width() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let (proxy, _receiver) = channel_pair::<AppMessage>();
        let mut overview = Overview::new(Workspace::with_draft_directory(temp.0.join("drafts")));
        let mut app = sse_app::AppState::new();
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
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        overview.build(&mut cx, host)?;
        let wide = overview
            .details_wide
            .ok_or_else(|| Error::damaged("missing wide details"))?;
        let narrow = overview
            .details_narrow
            .ok_or_else(|| Error::damaged("missing stacked details"))?;
        overview.message(
            &mut cx,
            &Message::Window(crate::event_loop::WindowEvent::Resized {
                width: 1920,
                height: 1080,
            }),
            None,
        )?;
        assert!(cx.tree.is_visible(wide));
        assert!(!cx.tree.is_visible(narrow));
        overview.message(
            &mut cx,
            &Message::Window(crate::event_loop::WindowEvent::Resized {
                width: 1366,
                height: 768,
            }),
            None,
        )?;
        assert!(!cx.tree.is_visible(wide));
        assert!(cx.tree.is_visible(narrow));
        Ok(())
    }

    #[test]
    fn detail_pairs_skip_placeholders_and_split_segments() {
        let pairs = super::detail_pairs("Деньги: 100 RU · Предметов: —\nРанг: —\nПуть: /a: b");
        assert_eq!(
            pairs,
            vec![
                ("Деньги".to_owned(), "100 RU".to_owned()),
                ("Путь".to_owned(), "/a: b".to_owned()),
            ]
        );
        assert!(super::detail_pairs("").is_empty());
    }

    #[test]
    fn overview_header_uses_values_without_assuming_localized_field_names() {
        let pairs =
            super::detail_pairs("Game: Shadow of Chernobyl\nFile name: quicksave.sav\nPath: browser:/quicksave.sav");

        assert_eq!(
            super::info_header_values(&pairs),
            ("Shadow of Chernobyl", "quicksave.sav", "browser:/quicksave.sav")
        );
    }
}
