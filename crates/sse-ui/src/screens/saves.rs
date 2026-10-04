//! S2 save screens: discovery, overview, inventory, factions, stashes and transitions.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, EditorAction, Screen, ScreenId};
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::Message;
use crate::layout::{NodeKind, Style};
use crate::widget::{Content, Look, WidgetId};
use crate::widgets::table::{Header, Table};
use crate::widgets::text_input::TextInput;
use sse_core::{Error, Result, SaveBuffer};
use sse_s2::{S2Change, S2InventoryItem, S2Save, S2StashItem, S2StashLayout};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::drafts::{DraftJournal, DraftPlan, DraftStore};
use sse_storage::transaction::{self, EditSummary};
use sse_xray::{save::InventoryItem, writer, Save};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const SAVE_PAGE_SIZE: usize = 10;
const INVENTORY_PAGE_SIZE: usize = 8;
const MAXIMUM_STASH_ROWS: usize = 10;
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

fn paragraph(tree: &mut crate::widget::Tree, parent: WidgetId, text: &str, role: Text) -> Result<WidgetId> {
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style::default(),
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

/// S2 stash transfer stays off until a written save is proven to load in the game.
const S2_STASH_MOVE_ENABLED: bool = false;

#[derive(Clone)]
pub(crate) struct Workspace {
    state: Arc<Mutex<WorkspaceState>>,
    draft_directory: Arc<PathBuf>,
    draft_generation: Arc<AtomicU64>,
    draft_latest: Arc<Mutex<BTreeMap<String, u64>>>,
    draft_write_lock: Arc<Mutex<()>>,
}

impl Default for Workspace {
    fn default() -> Self {
        Self::with_draft_directory(default_draft_directory())
    }
}

impl Workspace {
    fn with_draft_directory(directory: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(WorkspaceState::default())),
            draft_directory: Arc::new(directory),
            draft_generation: Arc::new(AtomicU64::new(0)),
            draft_latest: Arc::new(Mutex::new(BTreeMap::new())),
            draft_write_lock: Arc::new(Mutex::new(())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn spawn<F>(&self, name: &'static str, work: F)
    where
        F: FnOnce(sse_app::tasks::TaskContext) + Send + 'static,
    {
        let _handle = self.lock().tasks.spawn(name, move |context| {
            work(context);
            Ok(())
        });
    }

    pub(crate) fn poll_tasks(&self) {
        let _ = self.lock().tasks.poll_events();
    }

    fn persist_draft(&self, journal: DraftJournal, cx: &mut Context<'_>) {
        self.persist_drafts(vec![journal], cx);
    }

    fn reset_draft(&self, journal: DraftJournal, preserve_unmapped: bool, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some("Черновик сброшен в памяти; фоновый канал недоступен.".to_owned());
            return;
        };
        let Some(source_sha256) = journal.current().map(|plan| plan.source_sha256.clone()) else {
            return;
        };
        let generation = self.draft_generation.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        let latest = Arc::clone(&self.draft_latest);
        latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(source_sha256.clone(), generation);
        let latest = Arc::clone(&self.draft_latest);
        let write_lock = Arc::clone(&self.draft_write_lock);
        let draft_directory = Arc::clone(&self.draft_directory);
        self.spawn("draft-reset", move |context| {
            if context.is_cancelled() {
                return;
            }
            let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if latest
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&source_sha256)
                .copied()
                != Some(generation)
            {
                return;
            }
            let store = DraftStore::new(draft_directory.as_path());
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
        });
    }

    fn persist_drafts(&self, journals: Vec<DraftJournal>, cx: &mut Context<'_>) {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some("Черновик изменён только в памяти: фоновой канал недоступен.".to_owned());
            return;
        };
        let generation = self.draft_generation.fetch_add(1, Ordering::AcqRel).saturating_add(1);
        let hashes: Vec<String> = journals
            .iter()
            .filter_map(|journal| journal.current().map(|plan| plan.source_sha256.clone()))
            .collect();
        let latest = Arc::clone(&self.draft_latest);
        {
            let mut latest = latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            for hash in &hashes {
                latest.insert(hash.clone(), generation);
            }
        }
        let write_lock = Arc::clone(&self.draft_write_lock);
        let draft_directory = Arc::clone(&self.draft_directory);
        self.spawn("draft-save", move |context| {
            if context.is_cancelled() {
                return;
            }
            let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let is_latest = latest.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if hashes
                .iter()
                .any(|hash| is_latest.get(hash).copied() != Some(generation))
            {
                return;
            }
            drop(is_latest);
            let store = DraftStore::new(draft_directory.as_path());
            let result = journals
                .into_iter()
                .try_for_each(|journal| store.save(journal).map(|_| ()))
                .map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(DraftPersisted(result)),
            ));
        });
    }
}

#[derive(Default)]
struct WorkspaceState {
    tasks: sse_app::TaskManager,
    scanning: bool,
    discovery: Option<sse_storage::discovery::SaveDiscoveryResult>,
    loading: bool,
    load_request: u64,
    load_error: Option<String>,
    selected: Option<Arc<LoadedSave>>,
    pending_money: Option<u32>,
    pending_stacks: BTreeMap<ItemHandle, u32>,
    pending_stash_moves: BTreeSet<u32>,
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

struct LoadedSave {
    slot: SaveSlot,
    source_sha256: String,
    summary: String,
    factions: String,
    stashes: String,
    transitions: String,
    data: SaveData,
}

enum SaveData {
    Xray {
        save: Save,
        inventory: Vec<InventoryItem>,
    },
    Stalker2 {
        save: S2Save,
        inventory: Vec<S2InventoryItem>,
        stash_items: Option<std::result::Result<Vec<S2StashItem>, String>>,
    },
}

impl LoadedSave {
    fn read(slot: SaveSlot) -> Result<Self> {
        let packed = SaveBuffer::read(&slot.path)?;
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
        let summary = format!(
            "Игра: {format}\nЛокация: не подтверждена текущим индексатором\nИзменён (Unix UTC): {}\nРазмер: {} байт\nФормат: {format}\nCRC: контейнер X-Ray не хранит CRC\nВремя игры: {}\nДеньги: {money}\nПредметов в инвентаре: {}",
            unix_time(slot.last_write_time_utc),
            packed.len(),
            save.game_time(),
            inventory.len()
        );
        let factions = match save.player_faction() {
            Some(id) => format!("Фракция игрока: ID {id}\nРедактирование отношений недоступно в текущем индексаторе."),
            None => "Идентификатор фракции игрока не подтверждён этим сохранением.".to_owned(),
        };
        let stashes = describe_xray_stashes(&save);
        let transitions = describe_xray_transitions(&save);
        Ok(Self {
            slot,
            source_sha256,
            summary,
            factions,
            stashes,
            transitions,
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
        let summary = format!(
            "Игра: S.T.A.L.K.E.R. 2\nЛокация: не подтверждена текущим индексатором\nИзменён (Unix UTC): {}\nРазмер: {} байт\nФормат: S2\nCRC32: {:08X} — проверен\nДеньги: {}\nПредметов в рюкзаке: {}\nНеопознанных ссылок: {}",
            unix_time(slot.last_write_time_utc),
            packed.len(),
            save.container().stored_crc32(),
            save.money(),
            inventory.len(),
            save.unresolved_handles().len()
        );
        let factions =
            "Фракции S2 доступны только для чтения; отношения и принадлежность пока не индексируются.".to_owned();
        let stashes = describe_s2_stash(stash.as_ref());
        let transitions = "Переходы S2 доступны только для чтения, но их формат пока не индексируется.".to_owned();
        let stash_items = stash
            .as_ref()
            .map(|_| save.stash_items().map_err(|error| error.to_string()));
        Self {
            slot,
            source_sha256,
            summary,
            factions,
            stashes,
            transitions,
            data: SaveData::Stalker2 {
                save,
                inventory,
                stash_items,
            },
        }
    }
}

fn unix_time(time: std::time::SystemTime) -> u64 {
    time.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn display_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{} MiB", bytes / 1_048_576)
    } else if bytes >= 1_024 {
        format!("{} KiB", bytes / 1_024)
    } else {
        format!("{bytes} B")
    }
}

fn describe_xray_stashes(save: &Save) -> String {
    let boxes = save
        .registry_objects()
        .iter()
        .filter(|object| object.name.eq_ignore_ascii_case("inventory_box"))
        .collect::<Vec<_>>();
    if boxes.is_empty() {
        return "В этом сохранении не найдено подтверждённых тайников.".to_owned();
    }
    let mut lines = Vec::new();
    for box_object in boxes.iter().take(32) {
        let children = save
            .registry_objects()
            .iter()
            .filter(|item| item.parent_id == box_object.object_id)
            .map(|item| format!("{} (0x{:04X})", item.name_replace, item.object_id))
            .take(32)
            .collect::<Vec<_>>();
        lines.push(format!(
            "Тайник 0x{:04X}: {}",
            box_object.object_id,
            if children.is_empty() {
                "пуст"
            } else {
                "содержимое ниже"
            }
        ));
        lines.extend(children.into_iter().map(|child| format!("  · {child}")));
    }
    if boxes.len() > 32 {
        lines.push("Показаны первые 32 тайника.".to_owned());
    }
    lines.join("\n")
}

fn describe_s2_stash(stash: Option<&S2StashLayout>) -> String {
    let Some(stash) = stash else {
        return "Подтверждённый блок тайника в этом сохранении не найден.".to_owned();
    };
    let mut lines = vec![format!(
        "Предметов: {} · ячеек сетки: {}",
        stash.live_handles().len(),
        stash.grid_cells().len()
    )];
    for cell in stash.grid_cells().iter().take(32) {
        lines.push(format!(
            "0x{:08X} · столбец {} · строка {}",
            cell.handle, cell.x, cell.y
        ));
    }
    if stash.grid_cells().len() > 32 {
        lines.push("Показаны первые 32 ячейки.".to_owned());
    }
    lines.join("\n")
}

fn describe_xray_transitions(save: &Save) -> String {
    let destinations = match save.level_changer_destinations() {
        Ok(destinations) => destinations,
        Err(error) => return format!("Не удалось проверить переходы: {error}"),
    };
    if destinations.is_empty() {
        return "Подтверждённые переходы не найдены.".to_owned();
    }
    destinations
        .iter()
        .take(64)
        .map(|(handle, destination)| {
            let position = destination.dest_position.map_or_else(
                || "позиция неизвестна".to_owned(),
                |point| format!("x {:.1}, y {:.1}, z {:.1}", point.x, point.y, point.z),
            );
            format!(
                "0x{handle:04X} · {} → {} · {position}",
                destination.dest_level_name, destination.dest_level_point_name
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn start_discovery(workspace: &Workspace, cx: &mut Context<'_>) {
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some("Поиск сейвов начнётся в работающем окне редактора.".to_owned());
        return;
    };
    {
        let mut state = workspace.lock();
        if state.scanning {
            return;
        }
        state.scanning = true;
    }
    let workspace = workspace.clone();
    workspace.clone().spawn("save-discovery", move |context| {
        if context.is_cancelled() {
            return;
        }
        let candidates = SaveDirectoryLocator::find_candidate_directories(None);
        let mut result = SaveSlotDiscovery::discover(&candidates);
        result.slots.sort_by(|left, right| {
            save_game_key(left)
                .cmp(save_game_key(right))
                .then_with(|| right.last_write_time_utc.cmp(&left.last_write_time_utc))
        });
        let mut state = workspace.lock();
        state.discovery = Some(result);
        state.scanning = false;
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
    });
    cx.status = Some("Ищу сейвы в обнаруженных каталогах…".to_owned());
}

fn start_load(workspace: &Workspace, slot: SaveSlot, cx: &mut Context<'_>) {
    start_load_from(workspace, move || Ok(slot), false, cx);
}

fn start_load_path(workspace: &Workspace, path: &Path, cx: &mut Context<'_>) {
    let path = path.to_path_buf();
    start_load_from(workspace, move || slot_for_path(&path), true, cx);
}

fn start_load_from<F>(workspace: &Workspace, slot: F, include_discovery: bool, cx: &mut Context<'_>)
where
    F: FnOnce() -> Result<SaveSlot> + Send + 'static,
{
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some("Загрузка сейва доступна в работающем окне редактора.".to_owned());
        return;
    };
    let request = {
        let mut state = workspace.lock();
        state.load_request = state.load_request.saturating_add(1);
        state.loading = true;
        state.load_error = None;
        state.selected = None;
        state.pending_money = None;
        state.pending_stacks.clear();
        state.pending_stash_moves.clear();
        state.load_request
    };
    cx.app.set_current_save(None);
    let workspace = workspace.clone();
    let draft_directory = Arc::clone(&workspace.draft_directory);
    workspace.clone().spawn("save-load", move |context| {
        if context.is_cancelled() {
            return;
        }
        let result = slot().and_then(LoadedSave::read).and_then(|save| {
            let journal = load_draft_journal(draft_directory.as_path(), &save.source_sha256)?;
            Ok((save, journal))
        });
        let mut state = workspace.lock();
        if state.load_request != request {
            return;
        }
        state.loading = false;
        let completion = match result {
            Ok((save, journal)) => {
                state.load_error = None;
                if include_discovery {
                    let searched_paths = save.slot.path.parent().map(Path::to_path_buf).into_iter().collect();
                    state.discovery = Some(sse_storage::discovery::SaveDiscoveryResult {
                        slots: vec![save.slot.clone()],
                        searched_paths,
                    });
                }
                let path = save.slot.path.clone();
                state.selected = Some(Arc::new(save));
                LoadFinished {
                    request,
                    selected_path: Some(path),
                    journal: Some(journal),
                    error: None,
                }
            }
            Err(error) => {
                state.selected = None;
                let error = error.to_string();
                state.load_error = Some(error.clone());
                LoadFinished {
                    request,
                    selected_path: None,
                    journal: None,
                    error: Some(error),
                }
            }
        };
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(completion)));
    });
    cx.status = Some("Загружаю и проверяю выбранный сейв…".to_owned());
}

struct LoadFinished {
    request: u64,
    selected_path: Option<PathBuf>,
    journal: Option<DraftJournal>,
    error: Option<String>,
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

fn load_draft_journal(directory: &Path, source_sha256: &str) -> Result<DraftJournal> {
    if let Some(journal) = DraftStore::new(directory).load(source_sha256)? {
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
    state.pending_stacks = pending_stacks;
}

fn save_game_key(slot: &SaveSlot) -> &str {
    slot.game_id.as_deref().unwrap_or(slot.candidate_game_id.as_str())
}

fn slot_for_path(path: &Path) -> Result<SaveSlot> {
    let metadata = std::fs::metadata(path)?;
    Ok(SaveSlot {
        path: path.to_path_buf(),
        candidate_game_id: "unknown".to_owned(),
        candidate_release_id: "unknown".to_owned(),
        size: metadata.len(),
        last_write_time_utc: metadata.modified().unwrap_or(std::time::UNIX_EPOCH),
        format_id: None,
        game_id: None,
        detection_error: None,
    })
}

/// Save list and selected-save overview.
struct Overview {
    workspace: Workspace,
    page: usize,
    refresh: Option<WidgetId>,
    previous: Option<WidgetId>,
    next: Option<WidgetId>,
    rows: Vec<WidgetId>,
    list_status: Option<WidgetId>,
    selected_summary: Option<WidgetId>,
    search_button: Option<WidgetId>,
    search_text: Option<WidgetId>,
    search_query: String,
    search_focused: bool,
}

impl Overview {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            page: 0,
            refresh: None,
            previous: None,
            next: None,
            rows: Vec::new(),
            list_status: None,
            selected_summary: None,
            search_button: None,
            search_text: None,
            search_query: String::new(),
            search_focused: false,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let query = self.search_query.to_lowercase();
        let slots = state
            .discovery
            .as_ref()
            .map_or(&[][..], |result| result.slots.as_slice());
        let row_ids = slots
            .iter()
            .enumerate()
            .filter(|(_, slot)| {
                slot.path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_lowercase().contains(&query))
                    .unwrap_or(query.is_empty())
            })
            .map(|(index, _)| u64::try_from(index).unwrap_or(u64::MAX))
            .collect::<Vec<_>>();
        let mut table = Table::new(
            row_ids,
            40.0,
            vec![
                Header {
                    label: "Игра".to_owned(),
                    sortable: true,
                    direction: None,
                },
                Header {
                    label: "Дата".to_owned(),
                    sortable: true,
                    direction: None,
                },
            ],
        )?;
        table.header_click(0, false, |left, right, _| {
            let left = usize::try_from(left).ok().and_then(|index| slots.get(index));
            let right = usize::try_from(right).ok().and_then(|index| slots.get(index));
            left.map(save_game_key).cmp(&right.map(save_game_key))
        })?;
        table.header_click(1, true, |left, right, _| {
            let left = usize::try_from(left).ok().and_then(|index| slots.get(index));
            let right = usize::try_from(right).ok().and_then(|index| slots.get(index));
            left.map(|slot| slot.last_write_time_utc)
                .cmp(&right.map(|slot| slot.last_write_time_utc))
        })?;
        table.header_click(1, true, |left, right, _| {
            let left = usize::try_from(left).ok().and_then(|index| slots.get(index));
            let right = usize::try_from(right).ok().and_then(|index| slots.get(index));
            left.map(|slot| slot.last_write_time_utc)
                .cmp(&right.map(|slot| slot.last_write_time_utc))
        })?;
        let slot_count = table.view_len();
        let pages = slot_count.saturating_add(SAVE_PAGE_SIZE.saturating_sub(1)) / SAVE_PAGE_SIZE;
        self.page = self.page.min(pages.saturating_sub(1));
        let start = self.page.saturating_mul(SAVE_PAGE_SIZE);
        if let Some(status) = self.list_status {
            let text = if state.scanning {
                "Ищу сейвы…".to_owned()
            } else if let Some(error) = state.load_error.as_deref() {
                format!("Ошибка чтения: {error}")
            } else if state.loading {
                "Проверяю выбранный сейв…".to_owned()
            } else if slot_count == 0 {
                "Сейвы ещё не искали или не найдены. Нажмите «Найти сейвы».".to_owned()
            } else {
                format!(
                    "{} сейвов · каталогов проверено: {} · страница {} из {}",
                    slot_count,
                    state.discovery.as_ref().map_or(0, |result| result.searched_paths.len()),
                    self.page.saturating_add(1),
                    pages
                )
            };
            cx.tree.set_text(status, &text)?;
        }
        if let Some(id) = self.search_text {
            cx.tree.set_text(
                id,
                &format!(
                    "Поиск: {}{}",
                    if self.search_query.is_empty() {
                        "имя файла"
                    } else {
                        &self.search_query
                    },
                    if self.search_focused { " · ввод" } else { "" }
                ),
            )?;
        }
        for (offset, id) in self.rows.iter().enumerate() {
            let slot = table
                .visible_row(start.saturating_add(offset))
                .and_then(|row| usize::try_from(row).ok())
                .and_then(|index| slots.get(index));
            if let Some(slot) = slot {
                let file_name = slot
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_else(|| "без имени".into());
                let shortened = short_text(&file_name, 34);
                let game = slot
                    .format_id
                    .as_deref()
                    .or(Some(slot.candidate_release_id.as_str()))
                    .unwrap_or("неизвестный формат");
                let modified = slot
                    .last_write_time_utc
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |duration| duration.as_secs());
                cx.tree.set_text(
                    *id,
                    &format!("{game} · {shortened} · {} · {modified}", display_size(slot.size)),
                )?;
                cx.tree.set_visible(*id, true)?;
            } else {
                cx.tree.set_visible(*id, false)?;
            }
        }
        let selected_summary = state
            .selected
            .as_ref()
            .map(|save| save.summary.as_str())
            .unwrap_or("Выберите сейв из списка.");
        if let Some(id) = self.selected_summary {
            cx.tree.set_text(id, selected_summary)?;
        }
        if let Some(id) = self.previous {
            cx.tree.set_visible(id, pages > 1 && self.page > 0)?;
        }
        if let Some(id) = self.next {
            cx.tree
                .set_visible(id, pages > 1 && self.page.saturating_add(1) < pages)?;
        }
        Ok(())
    }
}

impl Screen for Overview {
    fn id(&self) -> ScreenId {
        ScreenId::Overview
    }

    fn subtitle(&self) -> &str {
        "Список сохранений и сведения о выбранном файле"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let list = style::card(cx.tree, host)?;
        style::label(cx.tree, list, "СОХРАНЕНИЯ", Text::Heading)?;
        let actions = style::row(cx.tree, list)?;
        self.refresh = Some(style::button(cx.tree, actions, "Найти сейвы", Button::Primary)?);
        self.search_button = Some(style::button(cx.tree, actions, "Поиск по имени", Button::Secondary)?);
        self.search_text = Some(style::label(cx.tree, actions, "Поиск: имя файла", Text::Note)?);
        self.previous = Some(style::button(cx.tree, actions, "Назад", Button::Secondary)?);
        self.next = Some(style::button(cx.tree, actions, "Дальше", Button::Secondary)?);
        style::label(cx.tree, list, "ИГРА · СОХРАНЕНИЕ · РАЗМЕР · ДАТА ИЗМЕНЕНИЯ", Text::Note)?;
        self.list_status = Some(style::label(cx.tree, list, "Сейвы ещё не искали.", Text::Note)?);
        for _ in 0..SAVE_PAGE_SIZE {
            let row = style::button(cx.tree, list, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        let overview = style::card(cx.tree, host)?;
        style::label(cx.tree, overview, "ОБЗОР ВЫБРАННОГО СЕЙВА", Text::Heading)?;
        self.selected_summary = Some(paragraph(cx.tree, overview, "Выберите сейв из списка.", Text::Body)?);
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.workspace.poll_tasks();
        if self.workspace.lock().discovery.is_none() {
            start_discovery(&self.workspace, cx);
        }
        self.render(cx)
    }

    fn open_save(&mut self, cx: &mut Context<'_>, path: &Path) -> Result<bool> {
        if cx.proxy.is_none() {
            cx.status = Some("Открытие сейва требует фонового канала приложения.".to_owned());
            return Ok(false);
        }
        self.page = 0;
        start_load_path(&self.workspace, path, cx);
        Ok(true)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        self.workspace.poll_tasks();
        if let Some(search_button) = self.search_button {
            self.search_focused = cx.tree.focused() == Some(search_button);
        }
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            ..
        }) = message
        {
            if *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if let Some(search_button) = self.search_button {
                    self.search_focused = cx.tree.is_visible(search_button);
                    if self.search_focused {
                        cx.tree.set_focus(Some(search_button))?;
                    }
                }
                return self.render(cx);
            }
            if *keysym == 0xff09 {
                return self.render(cx);
            }
            if self.search_focused {
                match *keysym {
                    0xff08 => {
                        self.search_query.pop();
                    }
                    0xff0d | 0xff1b => {
                        self.search_focused = false;
                        cx.tree.set_focus(None)?;
                        return self.render(cx);
                    }
                    _ => {
                        if let Some(character) = text.filter(|character| !character.is_control()) {
                            self.search_query.push(character);
                        }
                    }
                }
                self.page = 0;
                return self.render(cx);
            }
        }
        if clicked.is_some() && clicked == self.refresh {
            start_discovery(&self.workspace, cx);
            return Ok(());
        }
        if clicked.is_some() && clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.search_button {
            self.search_focused = true;
            return self.render(cx);
        }
        if let Some(offset) = clicked.and_then(|id| self.rows.iter().position(|row| *row == id)) {
            let state = self.workspace.lock();
            let index = self.page.saturating_mul(SAVE_PAGE_SIZE).saturating_add(offset);
            let query = self.search_query.to_lowercase();
            let slot = state.discovery.as_ref().and_then(|result| {
                result
                    .slots
                    .iter()
                    .filter(|slot| {
                        slot.path
                            .file_name()
                            .map(|name| name.to_string_lossy().to_lowercase().contains(&query))
                            .unwrap_or(query.is_empty())
                    })
                    .nth(index)
                    .cloned()
            });
            if let Some(slot) = slot {
                drop(state);
                start_load(&self.workspace, slot, cx);
                return Ok(());
            }
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Overview, payload)) = message {
            if let Some(LoadFinished {
                request,
                selected_path,
                journal,
                error,
            }) = payload.downcast_ref::<LoadFinished>()
            {
                if self.workspace.lock().load_request == *request {
                    if let (Some(path), Some(journal)) = (selected_path.as_ref(), journal.as_ref()) {
                        let source_sha256 = journal
                            .current()
                            .map(|plan| plan.source_sha256.clone())
                            .unwrap_or_default();
                        cx.app.set_current_save_identity(path.clone(), source_sha256);
                        cx.app.set_draft_journal(journal.clone());
                        let game = self
                            .workspace
                            .lock()
                            .selected
                            .as_ref()
                            .and_then(|save| save.slot.game_id.clone());
                        cx.app.set_selected_game(game);
                        set_workspace_draft(&self.workspace, journal);
                    } else {
                        cx.app.set_current_save(None);
                        cx.app.set_selected_game(None);
                    }
                    if let Some(error) = error {
                        cx.status = Some(format!("Сейв не загружен: {error}"));
                    } else {
                        cx.status = Some("Сейв прочитан и проверен.".to_owned());
                    }
                }
            }
            self.render(cx)?;
        }
        Ok(())
    }
}

struct ItemControls {
    label: WidgetId,
    decrease: WidgetId,
    increase: WidgetId,
    handle: Option<ItemHandle>,
}

/// Inventory screen with guarded X-Ray and S2 edits.
struct Inventory {
    workspace: Workspace,
    page: usize,
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
    last_path: Option<PathBuf>,
}

fn money_input_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 10,
        history_limit: 32,
        filter: InputFilter::Any,
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
            last_path: None,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let money_input_state;
        let Some(selected) = state.selected.as_ref() else {
            if let Some(id) = self.money_label {
                cx.tree.set_text(id, "Сначала выберите сейв на экране «Обзор».")?;
            }
            self.set_edit_controls(cx, false)?;
            if let Some(status) = self.status {
                cx.tree.set_text(status, "Выберите сейв для просмотра и правки.")?;
            }
            return Ok(());
        };
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.page = 0;
        }
        match &selected.data {
            SaveData::Xray { save, inventory } => {
                let money = save.money()?;
                let money_editable =
                    writer::capability(save.format(), writer::ChangeKind::EditMoney) == writer::Capability::Verified;
                let pending_money = state.pending_money.unwrap_or(money);
                if let Some(id) = self.money_label {
                    cx.tree.set_text(
                        id,
                        &format!(
                            "Деньги: {pending_money}{}",
                            if money_editable {
                                ""
                            } else {
                                " · запись отключена для этого формата"
                            }
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
                        (self.selected_category == "ВСЕ"
                            || xray_inventory_category(&item.category, &item.section) == self.selected_category)
                            && search_matches(&format!("{} {}", item.section, item.category), &query)
                    })
                    .collect();
                self.update_inventory_filters(cx, visible_items.len())?;
                let stack_editable =
                    writer::capability(save.format(), writer::ChangeKind::EditStacks) == writer::Capability::Verified;
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(item) = visible_items.get(start.saturating_add(offset)) {
                        let count = item.count.map_or_else(
                            || "—".to_owned(),
                            |original| {
                                state
                                    .pending_stacks
                                    .get(&ItemHandle::Xray(item.handle))
                                    .copied()
                                    .unwrap_or(u32::from(original))
                                    .to_string()
                            },
                        );
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "◇ {} · {} · {} · состояние {} · × {count} · 0x{:04X}",
                                item.section,
                                item.category,
                                item.placement.as_deref().unwrap_or("—"),
                                item.condition
                                    .map_or_else(|| "—".to_owned(), |value| format!("{:.0}%", value * 100.0)),
                                item.handle
                            ),
                        )?;
                        cx.tree.set_visible(row.label, true)?;
                        let editable = stack_editable && item.count.is_some();
                        cx.tree.set_visible(row.decrease, editable)?;
                        cx.tree.set_visible(row.increase, editable)?;
                        row.handle = Some(ItemHandle::Xray(item.handle));
                    } else {
                        row.handle = None;
                        cx.tree.set_visible(row.label, false)?;
                        cx.tree.set_visible(row.decrease, false)?;
                        cx.tree.set_visible(row.increase, false)?;
                    }
                }
                let pages = visible_items
                    .len()
                    .saturating_add(INVENTORY_PAGE_SIZE.saturating_sub(1))
                    / INVENTORY_PAGE_SIZE;
                self.page = self.page.min(pages.saturating_sub(1));
                if let Some(id) = self.previous {
                    cx.tree.set_visible(id, pages > 1 && self.page > 0)?;
                }
                if let Some(id) = self.next {
                    cx.tree
                        .set_visible(id, pages > 1 && self.page.saturating_add(1) < pages)?;
                }
                let has_changes = pending_money != money
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
                        "Изменения подготовлены. Сохранение создаст бэкап, запишет файл и повторно его прочитает.",
                    )?;
                }
            }
            SaveData::Stalker2 { save, inventory, .. } => {
                let writable = !save.index().is_legacy();
                let money = save.money();
                let pending_money = state.pending_money.unwrap_or(money);
                if let Some(id) = self.money_label {
                    cx.tree.set_text(
                        id,
                        &format!(
                            "Деньги: {pending_money}{}",
                            if writable {
                                " · S2"
                            } else {
                                " · S2 1.0.x открыт только для чтения"
                            }
                        ),
                    )?;
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
                self.update_inventory_filters(cx, visible_items.len())?;
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(item) = visible_items.get(start.saturating_add(offset)) {
                        let name = item.display_name.as_deref().unwrap_or("Неизвестный предмет");
                        let count = state
                            .pending_stacks
                            .get(&ItemHandle::Stalker2(item.handle))
                            .copied()
                            .unwrap_or(item.count);
                        let key = format!(
                            "{:02x}{:02x}{:02x}",
                            item.type_key[0], item.type_key[1], item.type_key[2]
                        );
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "◇ {name} · {} · размещение ({:?}, {:?}) · состояние {} · × {count} · {key}",
                                s2_inventory_category(item.kind_code, item.display_name.as_deref()),
                                item.x,
                                item.y,
                                item.condition
                                    .map_or_else(|| "—".to_owned(), |value| format!("{:.0}%", value * 100.0))
                            ),
                        )?;
                        cx.tree.set_visible(row.label, true)?;
                        let editable = writable && item.editable_count;
                        cx.tree.set_visible(row.decrease, editable)?;
                        cx.tree.set_visible(row.increase, editable)?;
                        row.handle = editable.then_some(ItemHandle::Stalker2(item.handle));
                    } else {
                        cx.tree.set_visible(row.label, false)?;
                        cx.tree.set_visible(row.decrease, false)?;
                        cx.tree.set_visible(row.increase, false)?;
                        row.handle = None;
                    }
                }
                let pages = visible_items
                    .len()
                    .saturating_add(INVENTORY_PAGE_SIZE.saturating_sub(1))
                    / INVENTORY_PAGE_SIZE;
                self.page = self.page.min(pages.saturating_sub(1));
                if let Some(id) = self.previous {
                    cx.tree.set_visible(id, pages > 1 && self.page > 0)?;
                }
                if let Some(id) = self.next {
                    cx.tree
                        .set_visible(id, pages > 1 && self.page.saturating_add(1) < pages)?;
                }
                let has_changes = pending_money != money
                    || state.pending_stacks.iter().any(|(handle, count)| {
                        matches!(handle, ItemHandle::Stalker2(_))
                            && inventory
                                .iter()
                                .find(|item| *handle == ItemHandle::Stalker2(item.handle))
                                .is_some_and(|item| item.count != *count)
                    })
                    || !state.pending_stash_moves.is_empty();
                if let Some(id) = self.export {
                    cx.tree.set_visible(id, writable && has_changes)?;
                    cx.tree.set_text(id, "Сохранить")?;
                }
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        if writable {
                            "Изменения сохраняются с резервной копией и проверкой повторным чтением."
                        } else {
                            "Сейв S2 1.0.x открыт только для чтения."
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
        {
            cx.tree.set_visible(id, visible)?;
        }
        for (id, _) in &self.categories {
            cx.tree.set_visible(*id, visible)?;
        }
        for row in &self.rows {
            cx.tree.set_visible(row.decrease, visible)?;
            cx.tree.set_visible(row.increase, visible)?;
            cx.tree.set_visible(row.label, visible)?;
        }
        if let Some(id) = self.previous {
            cx.tree.set_visible(id, false)?;
        }
        if let Some(id) = self.next {
            cx.tree.set_visible(id, false)?;
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
            cx.tree.set_text(id, &format!("Найдено: {count}"))?;
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
            let Ok(value) = typed.parse::<u32>() else { return Ok(()) };
            if value > 2_000_000_000 {
                return Ok(());
            }
            value
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
            cx.status = Some("Введены некорректные значения (проверьте введённые числа).".to_owned());
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

    fn save(&self, cx: &mut Context<'_>) -> Result<()> {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some("Сохранение доступно в работающем окне редактора.".to_owned());
            return Ok(());
        };
        let (selected, money, stacks, stash_moves) = {
            let state = self.workspace.lock();
            (
                state.selected.clone(),
                state.pending_money,
                state.pending_stacks.clone(),
                state.pending_stash_moves.clone(),
            )
        };
        let Some(selected) = selected else {
            cx.status = Some("Сначала выберите сейв.".to_owned());
            return Ok(());
        };
        if cx.app.has_invalid_numeric_input() {
            cx.status = Some("Введены некорректные значения (проверьте введённые числа).".to_owned());
            return Ok(());
        }
        if !cx.app.has_draft(&selected.source_sha256) && money.is_none() && stacks.is_empty() && stash_moves.is_empty()
        {
            cx.status = Some("Нет несохранённых изменений.".to_owned());
            return Ok(());
        }
        let source_sha256 = selected.source_sha256.clone();
        if let Some(plan) = cx.app.draft(&selected.source_sha256) {
            if plan.unmapped_legacy_plan.is_some() {
                cx.status =
                    Some("В черновике есть правки из другой версии редактора; сбросьте его перед записью.".to_owned());
                return Ok(());
            }
        }
        if let Some(status) = self.status {
            cx.tree.set_text(status, "Создаю резервную копию и сохраняю…")?;
        }
        self.workspace.spawn("save-write", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = commit_save_edits(&selected, money, &stacks, &stash_moves).map_err(|error| error.to_string());
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(SaveFinished { source_sha256, result }),
            ));
        });
        Ok(())
    }

    fn editor_action(&mut self, action: EditorAction, cx: &mut Context<'_>) -> Result<()> {
        if action == EditorAction::Save {
            return self.save(cx);
        }
        let Some(source_sha256) = cx.app.current_save_sha256().map(str::to_owned) else {
            cx.status = Some("Выберите сохранение для редактирования.".to_owned());
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
                cx.status = Some("Черновик сброшен.".to_owned());
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
    source_sha256: String,
    result: std::result::Result<(Arc<LoadedSave>, String), String>,
}

fn commit_save_edits(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<ItemHandle, u32>,
    stash_moves: &BTreeSet<u32>,
) -> Result<(Arc<LoadedSave>, String)> {
    commit_save_edits_to(selected, money, stacks, stash_moves, &default_backup_directory())
}

fn commit_save_edits_to(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<ItemHandle, u32>,
    stash_moves: &BTreeSet<u32>,
    backup_directory: &Path,
) -> Result<(Arc<LoadedSave>, String)> {
    let (packed, _summary) = prepare_save_edits(selected, money, stacks, stash_moves)?;
    let receipt = transaction::replace_transaction(
        &selected.slot.path,
        &selected.source_sha256,
        packed.as_slice(),
        backup_directory,
    )?;
    let mut slot = selected.slot.clone();
    let metadata = std::fs::metadata(&slot.path)?;
    slot.size = metadata.len();
    slot.last_write_time_utc = metadata.modified().unwrap_or(slot.last_write_time_utc);
    let reloaded = LoadedSave::read(slot)?;
    verify_requested_values(&reloaded, money, stacks, stash_moves)?;
    Ok((
        Arc::new(reloaded),
        format!(
            "Сохранено и повторно прочитано · бэкап {} · SHA-256 {}",
            receipt.backup_path.display(),
            receipt.output_sha256
        ),
    ))
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
    prepare_save_edits(selected, money, &stacks, &BTreeSet::new())
}

fn prepare_save_edits(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<ItemHandle, u32>,
    stash_moves: &BTreeSet<u32>,
) -> Result<(SaveBuffer, EditSummary)> {
    match &selected.data {
        SaveData::Xray { save, inventory } => {
            if !stash_moves.is_empty() {
                return Err(Error::Refused(
                    "S2 stash changes cannot be applied to an X-Ray save".to_owned(),
                ));
            }
            let current_money = save.money()?;
            let money_change = money.filter(|value| *value != current_money);
            let mut changes = Vec::new();
            if let Some(new_value) = money_change {
                changes.push(writer::Change::SetMoney {
                    target_object: save.actor_id(),
                    old_value: current_money,
                    new_value,
                });
            }
            let mut stack_count = 0_usize;
            for (handle, new_value) in stacks {
                let ItemHandle::Xray(handle) = handle else {
                    continue;
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
            if changes.is_empty() {
                return Err(Error::Refused("there are no inventory changes to save".to_owned()));
            }
            let packed = writer::apply(save, &writer::ChangeSet::new(changes))?;
            Ok((
                packed,
                EditSummary {
                    money: money_change,
                    stack_count,
                    ..EditSummary::default()
                },
            ))
        }
        SaveData::Stalker2 {
            save,
            inventory,
            stash_items,
        } => {
            if save.index().is_legacy() {
                return Err(Error::Refused("legacy S2 layouts are read-only".to_owned()));
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
            let money_change = money.filter(|value| *value != current_money);
            let mut changes = Vec::new();
            if let Some(new_value) = money_change {
                changes.push(S2Change::SetMoney(new_value));
            }
            let mut stack_count = 0_usize;
            for (handle, new_value) in stacks {
                let ItemHandle::Stalker2(handle) = handle else {
                    continue;
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
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<ItemHandle, u32>,
    stash_moves: &BTreeSet<u32>,
) -> Result<()> {
    let actual_money = match &selected.data {
        SaveData::Xray { save, .. } => save.money()?,
        SaveData::Stalker2 { save, .. } => save.money(),
    };
    if money.is_some_and(|expected| expected != actual_money) {
        return Err(Error::damaged("saved wallet value differs after read-back"));
    }
    for (handle, expected) in stacks {
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
    for handle in stash_moves {
        let moved = match &selected.data {
            SaveData::Stalker2 { save, .. } => {
                !save.stash_items()?.iter().any(|item| item.handle == *handle)
                    && save.items().iter().any(|item| item.handle == *handle)
            }
            SaveData::Xray { .. } => false,
        };
        if !moved {
            return Err(Error::damaged("saved stash transfer differs after read-back"));
        }
    }
    Ok(())
}

fn default_backup_directory() -> PathBuf {
    #[cfg(target_os = "windows")]
    let data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|home| PathBuf::from(home).join("AppData/Local")));
    #[cfg(target_os = "macos")]
    let data = std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"));
    #[cfg(all(unix, not(target_os = "macos")))]
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")));
    data.unwrap_or_else(|| std::env::temp_dir().join("StalkerSaveEditorData"))
        .join("StalkerSaveEditor")
        .join("backups")
}

impl Screen for Inventory {
    fn id(&self) -> ScreenId {
        ScreenId::Inventory
    }

    fn subtitle(&self) -> &str {
        "Состав рюкзака и подтверждённые изменения X-Ray / S2"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let inventory = style::card(cx.tree, host)?;
        style::label(cx.tree, inventory, "ИНВЕНТАРЬ", Text::Heading)?;
        let money = style::row(cx.tree, inventory)?;
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
            let button = style::button(cx.tree, money, label, Button::Secondary)?;
            self.money_buttons.push((button, amount));
        }
        let filters = style::row(cx.tree, inventory)?;
        let search_widget = cx.tree.add(
            Some(filters),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
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
                text: "Поиск предметов…".to_owned(),
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
        let chips = style::row(cx.tree, inventory)?;
        for category in INVENTORY_CATEGORIES {
            let id = cx.tree.add(
                Some(chips),
                NodeKind::Leaf,
                Style {
                    min: crate::layout::Size::new(58.0, 28.0),
                    padding: crate::layout::Edges {
                        left: 8.0,
                        top: 0.0,
                        right: 8.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
                Content::Button {
                    text: category.to_owned(),
                    style: Text::Note.style(),
                },
                style::nav(category == self.selected_category),
            )?;
            self.categories.push((id, category));
        }
        self.empty_results = Some(paragraph(
            cx.tree,
            inventory,
            "⌕\nПредметы не найдены\nИзмените поиск или категорию.",
            Text::Note,
        )?);
        if let Some(id) = self.empty_results {
            cx.tree.set_visible(id, false)?;
        }
        self.reset_filters = Some(style::button(
            cx.tree,
            inventory,
            "Сбросить фильтры",
            Button::Secondary,
        )?);
        if let Some(id) = self.reset_filters {
            cx.tree.set_visible(id, false)?;
        }
        let pages = style::row(cx.tree, inventory)?;
        self.previous = Some(style::button(cx.tree, pages, "Назад", Button::Secondary)?);
        self.next = Some(style::button(cx.tree, pages, "Дальше", Button::Secondary)?);
        for _ in 0..INVENTORY_PAGE_SIZE {
            let row = style::row(cx.tree, inventory)?;
            let label = style::label(cx.tree, row, "", Text::Body)?;
            let decrease = style::button(cx.tree, row, "−", Button::Secondary)?;
            let increase = style::button(cx.tree, row, "+", Button::Secondary)?;
            self.rows.push(ItemControls {
                label,
                decrease,
                increase,
                handle: None,
            });
        }
        self.export = Some(style::button(cx.tree, inventory, "Сохранить", Button::Primary)?);
        self.status = Some(style::label(
            cx.tree,
            inventory,
            "Изменения пока не подготовлены.",
            Text::Note,
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
        self.workspace.poll_tasks();
        if let Message::User(AppMessage::EditorAction(action)) = message {
            return self.editor_action(*action, cx);
        }
        if let (Some(widget), Some(input)) = (self.search_widget, self.search.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if let (Some(widget), Some(input)) = (self.money_input_widget, self.money_input.as_mut()) {
            input.focus(cx.tree.focused() == Some(widget), 0);
        }
        if clicked.is_some() && clicked == self.search_widget {
            if let Some(input) = self.search.as_mut() {
                input.focus(true, 0);
            }
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
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
        }) = message
        {
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
                                    Some("Введены некорректные значения (проверьте введённые числа).".to_owned())
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
        if clicked.is_some() && clicked == self.export {
            return self.save(cx);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Inventory, payload)) = message {
            if let Some(SaveFinished { source_sha256, result }) = payload.downcast_ref::<SaveFinished>() {
                match result {
                    Ok((loaded, text)) => {
                        let mut state = self.workspace.lock();
                        state.selected = Some(Arc::clone(loaded));
                        state.pending_money = None;
                        state.pending_stacks.clear();
                        state.pending_stash_moves.clear();
                        drop(state);
                        cx.app.discard_draft(source_sha256);
                        let old_empty = DraftJournal::new(vec![DraftPlan::empty(source_sha256)?], 0)?;
                        let new_journal = DraftJournal::new(vec![DraftPlan::empty(&loaded.source_sha256)?], 0)?;
                        cx.app
                            .set_current_save_identity(loaded.slot.path.clone(), loaded.source_sha256.clone());
                        cx.app.set_draft_journal(new_journal.clone());
                        set_workspace_draft(&self.workspace, &new_journal);
                        self.workspace.persist_drafts(vec![old_empty, new_journal], cx);
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, text)?;
                        }
                        cx.status = Some(text.clone());
                    }
                    Err(error) => {
                        let text = format!("Не сохранено: {error}");
                        if let Some(id) = self.status {
                            cx.tree.set_text(id, &text)?;
                        }
                        cx.status = Some(text);
                    }
                }
            }
            if let Some(DraftPersisted(result)) = payload.downcast_ref::<DraftPersisted>() {
                match result {
                    Ok(()) => cx.status = Some("Черновик сохранён.".to_owned()),
                    Err(error) => cx.status = Some(format!("Не удалось сохранить черновик: {error}")),
                }
            }
            self.render(cx)?;
        }
        Ok(())
    }
}

/// Faction information available from the selected save.
struct Factions {
    workspace: Workspace,
    text: Option<WidgetId>,
}

impl Factions {
    fn new(workspace: Workspace) -> Self {
        Self { workspace, text: None }
    }

    fn render(&self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let text = state
            .selected
            .as_ref()
            .map(|save| save.factions.as_str())
            .unwrap_or("Сначала выберите сейв на экране «Обзор».");
        if let Some(id) = self.text {
            cx.tree.set_text(id, text)?;
        }
        Ok(())
    }
}

impl Screen for Factions {
    fn id(&self) -> ScreenId {
        ScreenId::Factions
    }

    fn subtitle(&self) -> &str {
        "Только сведения, подтверждённые индексатором сейва"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ФРАКЦИИ", Text::Heading)?;
        self.text = Some(style::label(
            cx.tree,
            card,
            "Выберите сейв на экране «Обзор».",
            Text::Body,
        )?);
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
    }
}

#[derive(Clone, Copy)]
struct StashRow {
    row: WidgetId,
    label: WidgetId,
    move_button: WidgetId,
    handle: Option<u32>,
}

/// Confirmed stash contents for the selected save.
struct Stashes {
    workspace: Workspace,
    text: Option<WidgetId>,
    status: Option<WidgetId>,
    pager: Option<WidgetId>,
    previous: Option<WidgetId>,
    next: Option<WidgetId>,
    rows: Vec<StashRow>,
    page: usize,
    last_path: Option<PathBuf>,
}

impl Stashes {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            text: None,
            status: None,
            pager: None,
            previous: None,
            next: None,
            rows: Vec::new(),
            page: 0,
            last_path: None,
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (selected, pending_moves) = {
            let state = self.workspace.lock();
            (state.selected.clone(), state.pending_stash_moves.clone())
        };
        for row in &mut self.rows {
            row.handle = None;
            cx.tree.set_visible(row.row, false)?;
        }
        if let Some(id) = self.previous {
            cx.tree.set_visible(id, false)?;
        }
        if let Some(id) = self.next {
            cx.tree.set_visible(id, false)?;
        }
        if let Some(id) = self.pager {
            cx.tree.set_visible(id, false)?;
        }
        self.set_status(cx, "")?;
        let Some(selected) = selected else {
            self.set_text(cx, "Сначала выберите сейв на экране «Обзор».")?;
            return Ok(());
        };
        if self.last_path.as_ref() != Some(&selected.slot.path) {
            self.last_path = Some(selected.slot.path.clone());
            self.page = 0;
        }
        let SaveData::Stalker2 { save, stash_items, .. } = &selected.data else {
            self.set_text(cx, &selected.stashes)?;
            return Ok(());
        };
        let items = match stash_items {
            Some(Ok(items)) => items,
            Some(Err(error)) => {
                self.set_text(cx, &format!("Подтверждённые данные тайника недоступны: {error}"))?;
                return Ok(());
            }
            None => {
                self.set_text(cx, "Подтверждённый блок тайника в этом сохранении не найден.")?;
                return Ok(());
            }
        };
        if items.is_empty() {
            self.set_text(cx, "Подтверждённый тайник найден, но живых предметов в нём нет.")?;
        } else {
            let pages = items.len().div_ceil(self.rows.len().max(1));
            self.page = self.page.min(pages.saturating_sub(1));
            let start = self.page.saturating_mul(self.rows.len());
            self.set_text(
                cx,
                &format!(
                    "S2: {} предметов в подтверждённом тайнике · страница {} из {}. Отметьте перенос и сохраните его в «Инвентаре».",
                    items.len(),
                    self.page.saturating_add(1),
                    pages
                ),
            )?;
            for (offset, row) in self.rows.iter_mut().enumerate() {
                let Some(item) = items.get(start.saturating_add(offset)) else {
                    continue;
                };
                let name = item.display_name.as_deref().map(str::to_owned).unwrap_or_else(|| {
                    format!(
                        "Предмет · ключ {:02X}{:02X}{:02X}",
                        item.type_key[0], item.type_key[1], item.type_key[2]
                    )
                });
                let weight = if item.total_weight.is_finite() {
                    format!("{:.1}", item.total_weight)
                } else {
                    "неизвестен".to_owned()
                };
                cx.tree.set_text(
                    row.label,
                    &format!(
                        "{name} · кол-во {} · вес {weight} · ячейки {} · {}×{} от {},{} · 0x{:08X}",
                        item.count,
                        item.cells.len(),
                        item.width,
                        item.height,
                        item.x,
                        item.y,
                        item.handle
                    ),
                )?;
                cx.tree.set_visible(row.row, true)?;
                // S2 stash -> backpack shifts bytes without fixing outer lengths; C# 1.3.1 kept it disabled
                // (move_items unsupported). Off until proven in the game.
                let can_move =
                    S2_STASH_MOVE_ENABLED && !save.index().is_legacy() && save.unresolved_handles().is_empty();
                cx.tree.set_text(
                    row.move_button,
                    if pending_moves.contains(&item.handle) {
                        "Отменить перенос"
                    } else {
                        "В рюкзак"
                    },
                )?;
                cx.tree.set_visible(row.move_button, can_move)?;
                row.handle = Some(item.handle);
            }
            if let Some(id) = self.previous {
                cx.tree.set_visible(id, pages > 1 && self.page > 0)?;
            }
            if let Some(id) = self.next {
                cx.tree
                    .set_visible(id, pages > 1 && self.page.saturating_add(1) < pages)?;
            }
            if let Some(id) = self.pager {
                cx.tree.set_visible(id, pages > 1)?;
            }
            if !save.unresolved_handles().is_empty() {
                self.set_status(cx, "Перенос отключён: индекс сейва содержит неразрешённые ссылки.")?;
            } else if save.index().is_legacy() {
                self.set_status(cx, "S2 1.0.x доступен только для чтения.")?;
            } else if pending_moves.is_empty() {
                self.set_status(
                    cx,
                    "Отметьте предметы и примените перенос кнопкой «Сохранить» в «Инвентаре».",
                )?;
            } else {
                self.set_status(
                    cx,
                    &format!(
                        "{} предмет(ов) будет перенесено при сохранении из «Инвентаря».",
                        pending_moves.len()
                    ),
                )?;
            }
        }
        Ok(())
    }

    fn set_text(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(id) = self.text {
            cx.tree.set_text(id, text)?;
        }
        Ok(())
    }

    fn set_status(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        if let Some(id) = self.status {
            cx.tree.set_text(id, text)?;
        }
        Ok(())
    }

    fn move_item(&mut self, cx: &mut Context<'_>, handle: u32) -> Result<()> {
        let status = {
            let mut state = self.workspace.lock();
            let Some(selected) = state.selected.as_ref() else {
                cx.status = Some("Сначала выберите сейв.".to_owned());
                return Ok(());
            };
            let SaveData::Stalker2 { save, stash_items, .. } = &selected.data else {
                cx.status = Some("Перенос тайника поддерживается только для S2.".to_owned());
                return Ok(());
            };
            if save.index().is_legacy() || !save.unresolved_handles().is_empty() {
                cx.status = Some("Перенос недоступен для этого S2-сейва.".to_owned());
                return Ok(());
            }
            let Some(items) = stash_items.as_ref().and_then(|items| items.as_ref().ok()) else {
                cx.status = Some("Содержимое тайника не подтверждено индексом.".to_owned());
                return Ok(());
            };
            let Some(item) = items.iter().find(|item| item.handle == handle) else {
                cx.status = Some("Предмет больше не найден в выбранном сейве.".to_owned());
                return Ok(());
            };
            let name = item
                .display_name
                .as_deref()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("предмет 0x{:08X}", item.handle));
            if state.pending_stash_moves.remove(&handle) {
                format!("Перенос {name} отменён.")
            } else {
                state.pending_stash_moves.insert(handle);
                format!("{name} будет перенесён в рюкзак при сохранении.")
            }
        };
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
        "Подтверждённые тайники и их предметы"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ТАЙНИКИ", Text::Heading)?;
        self.text = Some(style::label(
            cx.tree,
            card,
            "Выберите сейв на экране «Обзор».",
            Text::Body,
        )?);
        self.status = Some(style::label(cx.tree, card, "", Text::Note)?);
        let pager = style::row(cx.tree, card)?;
        self.pager = Some(pager);
        self.previous = Some(style::button(cx.tree, pager, "Предыдущая", Button::Secondary)?);
        self.next = Some(style::button(cx.tree, pager, "Следующая", Button::Secondary)?);
        cx.tree.set_visible(pager, false)?;
        for _ in 0..MAXIMUM_STASH_ROWS {
            let row = style::row(cx.tree, card)?;
            let label = style::label(cx.tree, row, "", Text::Body)?;
            let move_button = style::button(cx.tree, row, "В рюкзак", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(StashRow {
                row,
                label,
                move_button,
                handle: None,
            });
        }
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.workspace.poll_tasks();
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        _message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked.is_some() && clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if let Some(handle) = self
            .rows
            .iter()
            .find(|row| Some(row.move_button) == clicked)
            .and_then(|row| row.handle)
        {
            self.move_item(cx, handle)?;
        }
        self.workspace.poll_tasks();
        Ok(())
    }
}

/// Parsed level-changer destinations in the selected save.
struct Transitions {
    workspace: Workspace,
    text: Option<WidgetId>,
}

impl Transitions {
    fn new(workspace: Workspace) -> Self {
        Self { workspace, text: None }
    }

    fn render(&self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let text = state
            .selected
            .as_ref()
            .map(|save| save.transitions.as_str())
            .unwrap_or("Сначала выберите сейв на экране «Обзор».");
        if let Some(id) = self.text {
            cx.tree.set_text(id, text)?;
        }
        Ok(())
    }
}

impl Screen for Transitions {
    fn id(&self) -> ScreenId {
        ScreenId::Transitions
    }

    fn subtitle(&self) -> &str {
        "Переходы из индексированных объектов level_changer"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::label(cx.tree, card, "ПЕРЕХОДЫ", Text::Heading)?;
        self.text = Some(style::label(
            cx.tree,
            card,
            "Выберите сейв на экране «Обзор».",
            Text::Body,
        )?);
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
    }
}

fn short_text(text: &str, limit: usize) -> String {
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
        commit_save_edits_to, prepare_save_edits, prepare_xray_edits, DraftJournal, DraftPlan, DraftStore, Inventory,
        LoadFinished, LoadedSave, Overview, S2Save, SaveBuffer, SaveSlot, Workspace,
    };
    use crate::event_loop::{channel_pair, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::{AppMessage, Context, Screen, ScreenId};
    use crate::widget::{Content, Look, Tree};
    use sse_core::Error;
    use sse_xray::Save;
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::UNIX_EPOCH;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

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

        let (output, summary) = prepare_save_edits(&loaded, Some(1_000), &BTreeMap::new(), &BTreeSet::new())?;
        assert_eq!(S2Save::from_bytes(output.as_slice())?.money(), 1_000);
        assert_eq!(summary.money, Some(1_000));
        Ok(())
    }

    #[test]
    fn s2_stash_transfer_creates_backup_and_passes_durable_read_back() -> sse_core::Result<()> {
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

        let pending_moves = BTreeSet::from([handle]);
        let (updated, status) = commit_save_edits_to(&selected, None, &BTreeMap::new(), &pending_moves, &backup)?;
        let super::SaveData::Stalker2 { save, .. } = &updated.data else {
            return Err(Error::damaged("updated S2 fixture parsed as X-Ray"));
        };
        assert!(!save.stash_items()?.iter().any(|item| item.handle == handle));
        assert!(save.items().iter().any(|item| item.handle == handle));
        assert!(status.contains("прочитан"));

        let durable = S2Save::from_bytes(&fs::read(&path)?)?;
        assert!(!durable.stash_items()?.iter().any(|item| item.handle == handle));
        assert!(durable.items().iter().any(|item| item.handle == handle));
        assert!(sse_storage::transaction::list_backups(&backup)?
            .iter()
            .any(|entry| entry.status == sse_storage::transaction::BackupStatus::Verified));
        Ok(())
    }

    #[test]
    fn s2_stash_transfer_is_staged_and_can_be_toggled_off() -> sse_core::Result<()> {
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
        let workspace = Workspace::default();
        workspace.lock().selected = Some(std::sync::Arc::new(selected));
        let (proxy, _receiver) = channel_pair::<AppMessage>();
        let mut app = sse_app::AppState::new();
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
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;

        screen.move_item(&mut cx, handle)?;
        assert!(workspace.lock().pending_stash_moves.contains(&handle));
        assert_eq!(fs::read(&path)?, original);
        screen.move_item(&mut cx, handle)?;
        assert!(!workspace.lock().pending_stash_moves.contains(&handle));
        assert_eq!(fs::read(&path)?, original);
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
        let (xray_after, _) =
            commit_save_edits_to(&xray, Some(xray_money), &BTreeMap::new(), &BTreeSet::new(), &backup)?;
        assert!(matches!(
            &xray_after.data,
            super::SaveData::Xray { save, .. } if save.money().ok() == Some(xray_money)
        ));

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
            Some(s2_money),
            &BTreeMap::new(),
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
            Some(path.as_path()),
            "background loader did not return the fixture path"
        );
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        overview.message(&mut cx, &message, None)?;
        assert_eq!(cx.app.current_save(), Some(path.as_path()));
        let source_sha256 = sse_codecs::sha256::sha256_hex(include_bytes!(
            "../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav"
        ));
        assert_eq!(cx.app.current_save_sha256(), Some(source_sha256.as_str()));
        assert!(cx.app.draft_journal(&source_sha256).is_some());
        assert!(!cx.app.has_draft(&source_sha256));
        Ok(())
    }

    #[test]
    fn inventory_draft_persists_and_global_undo_redo_updates_the_selected_save() -> sse_core::Result<()> {
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
        assert!(DraftStore::new(&draft_directory)
            .load(&source_sha256)?
            .is_some_and(|journal| journal.current().and_then(|draft| draft.money) == Some(original_money + 77)));

        inventory.editor_action(super::EditorAction::Undo, &mut cx)?;
        let _ = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| Error::System(format!("draft undo persistence timed out: {error}")))?;
        assert_eq!(cx.app.draft(&source_sha256).and_then(|draft| draft.money), None);
        assert!(DraftStore::new(&draft_directory).load(&source_sha256)?.is_none());

        inventory.editor_action(super::EditorAction::Redo, &mut cx)?;
        let _ = receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| Error::System(format!("draft redo persistence timed out: {error}")))?;
        assert_eq!(
            cx.app.draft(&source_sha256).and_then(|draft| draft.money),
            Some(original_money.saturating_add(77))
        );
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
}
