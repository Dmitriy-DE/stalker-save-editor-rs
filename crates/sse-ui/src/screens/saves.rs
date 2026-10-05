//! S2 save screens: discovery, overview, inventory, factions, stashes and transitions.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, EditorAction, Screen, ScreenId};
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::Message;
use crate::layout::{NodeKind, Size, Style};
use crate::raster::Color;
use crate::widget::{Content, Look, WidgetId};
use crate::widgets::table::{Header, Table};
use crate::widgets::text_input::TextInput;
use sse_core::{Error, Result, SaveBuffer};
use sse_s2::{S2Change, S2InventoryItem, S2Save, S2StashItem, S2StashLayout};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::drafts::{AddRequest, DraftJournal, DraftPlacement, DraftPlan, DraftStore};
use sse_storage::transaction::{self, EditSummary};
use sse_xray::{save::InventoryItem, writer, Save};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

const SAVE_PAGE_SIZE: usize = 10;
const INVENTORY_PAGE_SIZE: usize = 8;
const ADD_ITEM_PAGE_SIZE: usize = 8;
const MAXIMUM_STASH_ROWS: usize = 10;
const MAXIMUM_UPGRADE_ROWS: usize = 16;
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

/// S2 stash transfer stays disabled until a saved result is validated in-game.
pub(super) const S2_STASH_MOVE_ENABLED: bool = false;

#[derive(Clone)]
pub(crate) struct Workspace {
    state: Arc<Mutex<WorkspaceState>>,
    draft_directory: Arc<PathBuf>,
    backup_directory: Arc<Mutex<PathBuf>>,
    draft_generation: Arc<AtomicU64>,
    draft_latest: Arc<Mutex<BTreeMap<String, u64>>>,
    draft_write_lock: Arc<Mutex<()>>,
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

    fn with_paths(draft_directory: PathBuf, backup_directory: PathBuf) -> Self {
        Self {
            state: Arc::new(Mutex::new(WorkspaceState::default())),
            draft_directory: Arc::new(draft_directory),
            backup_directory: Arc::new(Mutex::new(backup_directory)),
            draft_generation: Arc::new(AtomicU64::new(0)),
            draft_latest: Arc::new(Mutex::new(BTreeMap::new())),
            draft_write_lock: Arc::new(Mutex::new(())),
        }
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

    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn is_saving(&self) -> bool {
        self.lock().active_save_request.is_some()
    }

    pub(crate) fn begin_saving(&self) -> Option<u64> {
        let mut state = self.lock();
        if state.active_save_request.is_some() {
            return None;
        }
        let request = state.next_save_request.checked_add(1)?;
        state.next_save_request = request;
        state.active_save_request = Some(request);
        Some(request)
    }

    pub(crate) fn finish_saving(&self, request: u64) -> bool {
        let mut state = self.lock();
        if state.active_save_request != Some(request) {
            return false;
        }
        state.active_save_request = None;
        true
    }

    pub(crate) fn library_snapshot(&self) -> (bool, Option<String>, Vec<SaveSlot>) {
        let state = self.lock();
        (
            state.scanning,
            state.load_error.clone(),
            state.discovery.as_ref().map_or_else(Vec::new, |discovery| {
                discovery
                    .slots
                    .iter()
                    .filter(|slot| slot.detection_error.is_none())
                    .cloned()
                    .collect()
            }),
        )
    }

    pub(crate) fn refresh_library(&self, cx: &mut Context<'_>) {
        start_discovery(self, cx);
    }

    pub(crate) fn select_library_path(&self, path: &Path, cx: &mut Context<'_>) {
        let slot = self
            .lock()
            .discovery
            .as_ref()
            .and_then(|discovery| discovery.slots.iter().find(|slot| slot.path == path))
            .cloned();
        if let Some(slot) = slot {
            start_load(self, slot, cx);
        }
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
    pending_durability: BTreeMap<ItemHandle, u8>,
    pending_placements: BTreeMap<ItemHandle, DraftPlacement>,
    pending_upgrades: BTreeMap<ItemHandle, Vec<String>>,
    pending_removed: BTreeSet<ItemHandle>,
    pending_adds: Vec<AddRequest>,
    pending_stash_moves: BTreeSet<u32>,
    external_change: bool,
    last_file_check: u64,
    file_check_generation: u64,
    file_check_in_flight: bool,
    active_save_request: Option<u64>,
    next_save_request: u64,
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
    }
}

struct LoadedSave {
    slot: SaveSlot,
    source_sha256: String,
    info: String,
    parameters: String,
    integrity: String,
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
        let parameters = format!(
            "Деньги: {money} RU · Предметов: {} · Тайников: —\nИгровое время: {} · Персонаж: — · Здоровье: —\nРанг: — · Репутация: — · Задания: — · Убито: — · Погода: —",
            inventory.len(),
            save.game_time()
        );
        let integrity = save_integrity(
            &slot,
            &source_sha256,
            packed.len(),
            "не подтверждается отдельным полем",
            format,
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
            info,
            parameters,
            integrity,
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
        let info = save_info(&slot);
        let parameters = format!(
            "Деньги: {} RU · Предметов: {} · Тайников: {}\nИгровое время · Персонаж · Здоровье · Ранг · Репутация · Задания · Убито · Погода: —\nНеопознанных ссылок: {}",
            save.money(),
            inventory.len(),
            stash.as_ref().map_or_else(|| "—".to_owned(), |items| items.live_handles().len().to_string()),
            save.unresolved_handles().len()
        );
        let integrity = save_integrity(
            &slot,
            &source_sha256,
            packed.len(),
            if save.container().stored_crc32() == save.container().computed_crc32() {
                "OK (CRC32)"
            } else {
                "ошибка"
            },
            "S2",
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
            info,
            parameters,
            integrity,
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

fn save_info(slot: &SaveSlot) -> String {
    let game = slot.format_id.as_deref().unwrap_or(&slot.candidate_release_id);
    let filename = slot
        .path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_else(|| "без имени".into());
    format!(
        "Игра: {}\nИмя файла: {filename}\nПуть: {}",
        format_display_name(game),
        slot.path.display()
    )
}

fn save_integrity(slot: &SaveSlot, source_sha256: &str, bytes_read: usize, crc_status: &str, format: &str) -> String {
    format!(
        "Размер файла: {} байт · Изменён: {} UTC\nSHA-256: {source_sha256}\nCRC: {crc_status} · Формат: {format} · Сборка игры: —",
        bytes_read,
        display_file_time(slot.last_write_time_utc, false, true)
    )
}

pub(super) fn format_display_name(format: &str) -> &'static str {
    match format {
        "stalker-soc-ee" => "Тень Чернобыля (Enhanced Edition)",
        "stalker-soc" | "soc" => "Тень Чернобыля",
        "stalker-cs-ee" => "Чистое Небо (Enhanced Edition)",
        "stalker-cs" | "clear_sky" => "Чистое Небо",
        "stalker-cop-ee" => "Зов Припяти (Enhanced Edition)",
        "stalker-cop" | "cop" => "Зов Припяти",
        "stalker2" | "s2" => "S.T.A.L.K.E.R. 2: Сердце Чернобыля",
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
        format!("{:.1} МБ", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1_024 {
        format!("{:.0} КБ", bytes as f64 / 1_024.0)
    } else {
        format!("{bytes} Б")
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
        if state.active_save_request.is_some() {
            cx.status = Some("Нельзя сменить сейв, пока выполняется запись.".to_owned());
            return;
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
        state.external_change = false;
        state.file_check_generation = state.file_check_generation.saturating_add(1);
        state.file_check_in_flight = false;
        state.last_file_check = 0;
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

fn schedule_file_check(workspace: &Workspace, cx: &Context<'_>, seconds: u64) {
    let Some(proxy) = cx.proxy.cloned() else {
        return;
    };
    let (path, expected_size, expected_modified, source_sha256, generation) = {
        let mut state = workspace.lock();
        if state.active_save_request.is_some()
            || state.file_check_in_flight
            || seconds.saturating_sub(state.last_file_check) < 3
        {
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
        state.file_check_in_flight = true;
        state.file_check_generation = state.file_check_generation.saturating_add(1);
        (
            snapshot.0,
            snapshot.1,
            snapshot.2,
            snapshot.3,
            state.file_check_generation,
        )
    };
    let shared = workspace.clone();
    workspace.spawn("save-file-monitor", move |context| {
        if context.is_cancelled() {
            return;
        }
        let changed = std::fs::metadata(&path).map_or(true, |metadata| {
            metadata.len() != expected_size || metadata.modified().is_ok_and(|modified| modified != expected_modified)
        });
        let still_selected = {
            let mut state = shared.lock();
            let still_selected = state
                .selected
                .as_ref()
                .is_some_and(|selected| selected.slot.path == path && selected.source_sha256 == source_sha256);
            if state.file_check_generation == generation {
                state.file_check_in_flight = false;
                if still_selected {
                    state.external_change = changed;
                }
            }
            still_selected
        };
        if still_selected {
            let finished = FileCheckFinished {
                path,
                source_sha256,
                changed,
            };
            let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(finished.clone())));
            let _ = proxy.send(AppMessage::ToScreen(ScreenId::Inventory, Box::new(finished)));
        }
    });
}

fn start_reload_selected(workspace: &Workspace, cx: &mut Context<'_>) -> Result<()> {
    let Some(proxy) = cx.proxy.cloned() else {
        cx.status = Some("Повторное чтение доступно в работающем окне редактора.".to_owned());
        return Ok(());
    };
    let (path, old_sha256, request, generation, empty_journal) = {
        let mut state = workspace.lock();
        if state.active_save_request.is_some() {
            cx.status = Some("Нельзя перечитать сейв, пока выполняется запись.".to_owned());
            return Ok(());
        }
        let selected = state.selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let old_sha256 = selected.source_sha256.clone();
        let empty_journal = DraftJournal::new(vec![DraftPlan::empty(&old_sha256)?], 0)?;
        let generation = workspace
            .draft_generation
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        workspace
            .draft_latest
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(old_sha256.clone(), generation);
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
        state.external_change = false;
        state.file_check_generation = state.file_check_generation.saturating_add(1);
        state.file_check_in_flight = false;
        state.last_file_check = 0;
        (
            selected.slot.path.clone(),
            old_sha256,
            state.load_request,
            generation,
            empty_journal,
        )
    };
    cx.app.set_current_save(None);
    cx.app.discard_draft(&old_sha256);
    let write_lock = Arc::clone(&workspace.draft_write_lock);
    let latest = Arc::clone(&workspace.draft_latest);
    let directory = Arc::clone(&workspace.draft_directory);
    let shared = workspace.clone();
    workspace.spawn("save-reload", move |context| {
        if context.is_cancelled() {
            return;
        }
        let result: Result<(LoadedSave, DraftJournal)> = (|| {
            {
                let _guard = write_lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                if latest
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .get(&old_sha256)
                    .copied()
                    == Some(generation)
                {
                    DraftStore::new(directory.as_path()).save(empty_journal)?;
                }
            }
            let loaded = LoadedSave::read(slot_for_path(&path)?)?;
            let journal = load_draft_journal(directory.as_path(), &loaded.source_sha256)?;
            Ok((loaded, journal))
        })();
        let mut state = shared.lock();
        if state.load_request != request {
            return;
        }
        state.loading = false;
        let completion = match result {
            Ok((loaded, journal)) => {
                let path = loaded.slot.path.clone();
                state.selected = Some(Arc::new(loaded));
                state.load_error = None;
                LoadFinished {
                    request,
                    selected_path: Some(path),
                    journal: Some(journal),
                    error: None,
                }
            }
            Err(error) => {
                let error = error.to_string();
                state.selected = None;
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
    cx.status = Some("Сбрасываю черновик и перечитываю сейв с диска…".to_owned());
    Ok(())
}

struct LoadFinished {
    request: u64,
    selected_path: Option<PathBuf>,
    journal: Option<DraftJournal>,
    error: Option<String>,
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
                "Файл сейва изменился после открытия (игра или другая программа). Несохранённые правки относятся к старой версии.",
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

/// Save list and selected-save overview.
struct Overview {
    workspace: Workspace,
    page: usize,
    refresh: Option<WidgetId>,
    previous: Option<WidgetId>,
    next: Option<WidgetId>,
    rows: Vec<WidgetId>,
    row_containers: Vec<WidgetId>,
    list_status: Option<WidgetId>,
    selected_info: Option<WidgetId>,
    selected_parameters: Option<WidgetId>,
    selected_integrity: Option<WidgetId>,
    search_text: Option<WidgetId>,
    search_input: Option<TextInput>,
    search_query: String,
    search_focused: bool,
    external_banner_row: Option<WidgetId>,
    external_banner: Option<WidgetId>,
    external_reload: Option<WidgetId>,
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
            row_containers: Vec::new(),
            list_status: None,
            selected_info: None,
            selected_parameters: None,
            selected_integrity: None,
            search_text: None,
            search_input: None,
            search_query: String::new(),
            search_focused: false,
            external_banner_row: None,
            external_banner: None,
            external_reload: None,
        }
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
                if self.search_query.is_empty() {
                    "Поиск по имени файла…"
                } else {
                    &self.search_query
                },
            )?;
        }
        for (offset, id) in self.rows.iter().enumerate() {
            let container = self.row_containers.get(offset).copied();
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
                let shortened = short_text(&file_name, 26);
                let game = slot
                    .format_id
                    .as_deref()
                    .or(Some(slot.candidate_release_id.as_str()))
                    .unwrap_or("неизвестный формат");
                cx.tree.set_text(
                    *id,
                    &format!(
                        "{} · {shortened} · {} · {}",
                        short_text(game, 16),
                        display_size(slot.size),
                        display_file_time(slot.last_write_time_utc, true, false)
                    ),
                )?;
                cx.tree.set_visible(*id, true)?;
                if let Some(container) = container {
                    cx.tree.set_visible(container, true)?;
                }
            } else {
                cx.tree.set_visible(*id, false)?;
                if let Some(container) = container {
                    cx.tree.set_visible(container, false)?;
                }
            }
        }
        let (info, parameters, integrity) = state.selected.as_ref().map_or(
            (
                "Выберите сохранение для просмотра.".to_owned(),
                "Деньги: —\nПредметов: —\nТайников: —\nИгровое время: —".to_owned(),
                "Размер файла: —\nИзменён: —\nSHA-256: —\nФормат: —\nСборка игры: —".to_owned(),
            ),
            |save| (save.info.clone(), save.parameters.clone(), save.integrity.clone()),
        );
        if let Some(id) = self.selected_info {
            cx.tree.set_text(id, &info)?;
        }
        if let Some(id) = self.selected_parameters {
            cx.tree.set_text(id, &parameters)?;
        }
        if let Some(id) = self.selected_integrity {
            cx.tree.set_text(id, &integrity)?;
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
        let (row, banner, reload) = add_external_file_banner(cx.tree, host)?;
        self.external_banner_row = Some(row);
        self.external_banner = Some(banner);
        self.external_reload = Some(reload);
        let list = style::card(cx.tree, host)?;
        cx.tree.set_style(
            list,
            Style {
                padding: crate::layout::Edges::all(crate::theme::CARD_PADDING),
                gap: Size::new(0.0, 0.0),
                align_items: crate::layout::Align::Stretch,
                shrink: 0.0,
                ..Style::default()
            },
        )?;
        style::label(cx.tree, list, "СОХРАНЕНИЯ", Text::Heading)?;
        let actions = style::row(cx.tree, list)?;
        cx.tree.set_style(
            actions,
            Style {
                margin: crate::layout::Edges {
                    top: 4.0,
                    bottom: 4.0,
                    ..crate::layout::Edges::default()
                },
                gap: Size::new(crate::theme::CONTROL_GAP, 0.0),
                align_items: crate::layout::Align::Center,
                ..Style::default()
            },
        )?;
        self.refresh = Some(style::button(cx.tree, actions, "Найти сейвы", Button::Primary)?);
        let colors = crate::theme::current().colors;
        let search = cx.tree.add(
            Some(actions),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                min: Size::new(180.0, crate::theme::BUTTON_HEIGHT),
                padding: crate::layout::Edges {
                    left: 10.0,
                    top: 0.0,
                    right: 10.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: "Поиск по имени файла…".to_owned(),
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
        self.search_text = Some(search);
        self.search_input = Some(TextInput::new("", inventory_search_config())?);
        self.previous = Some(style::button(cx.tree, actions, "Назад", Button::Secondary)?);
        self.next = Some(style::button(cx.tree, actions, "Дальше", Button::Secondary)?);
        let headings = style::label(cx.tree, list, "ИГРА · СОХРАНЕНИЕ · РАЗМЕР · ДАТА ИЗМЕНЕНИЯ", Text::Note)?;
        cx.tree.set_style(
            headings,
            Style {
                margin: crate::layout::Edges {
                    top: 2.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;
        let status = style::label(cx.tree, list, "Сейвы ещё не искали.", Text::Note)?;
        cx.tree.set_style(
            status,
            Style {
                margin: crate::layout::Edges {
                    top: 2.0,
                    ..crate::layout::Edges::default()
                },
                ..Style::default()
            },
        )?;
        self.list_status = Some(status);
        for _ in 0..SAVE_PAGE_SIZE {
            let row_container = cx.tree.add(
                Some(list),
                NodeKind::Column,
                Style {
                    margin: crate::layout::Edges {
                        top: 4.0,
                        ..crate::layout::Edges::default()
                    },
                    ..Style::default()
                },
                Content::Panel,
                Look::default(),
            )?;
            let row = style::button(cx.tree, row_container, "", Button::Secondary)?;
            cx.tree.set_visible(row_container, false)?;
            self.row_containers.push(row_container);
            self.rows.push(row);
        }
        let overview = style::card(cx.tree, host)?;
        style::label(cx.tree, overview, "ИНФОРМАЦИЯ О СОХРАНЕНИИ", Text::Heading)?;
        self.selected_info = Some(paragraph(
            cx.tree,
            overview,
            "Выберите сохранение для просмотра.",
            Text::Body,
        )?);
        let parameters = style::card(cx.tree, host)?;
        style::label(cx.tree, parameters, "ПАРАМЕТРЫ СТАЛКЕРА", Text::Heading)?;
        self.selected_parameters = Some(paragraph(
            cx.tree,
            parameters,
            "Деньги: — · Предметов: — · Тайников: —\nИгровое время: — · Персонаж: — · Здоровье: —\nРанг: — · Репутация: — · Задания: — · Убито: — · Погода: —",
            Text::Note,
        )?);
        let integrity = style::card(cx.tree, host)?;
        style::label(cx.tree, integrity, "ЦЕЛОСТНОСТЬ И МЕТАДАННЫЕ", Text::Heading)?;
        self.selected_integrity = Some(paragraph(
            cx.tree,
            integrity,
            "Размер файла: — · Изменён: —\nSHA-256: —\nФормат: — · Сборка игры: —",
            Text::Note,
        )?);
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
        if let Message::User(AppMessage::Tick(seconds)) = message {
            schedule_file_check(&self.workspace, cx, *seconds);
        }
        if let Some(search_widget) = self.search_text {
            self.search_focused = cx.tree.focused() == Some(search_widget);
            if let Some(input) = self.search_input.as_mut() {
                input.focus(self.search_focused, 0);
            }
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
                if let Some(search_widget) = self.search_text {
                    self.search_focused = cx.tree.is_visible(search_widget);
                    if self.search_focused {
                        cx.tree.set_focus(Some(search_widget))?;
                        if let Some(input) = self.search_input.as_mut() {
                            input.focus(true, 0);
                        }
                    }
                }
                return self.render(cx);
            }
            if *keysym == 0xff09 {
                return self.render(cx);
            }
            if self.search_input.as_ref().is_some_and(TextInput::focused) {
                if matches!(*keysym, 0xff0d | 0xff1b) {
                    self.search_focused = false;
                    if let Some(input) = self.search_input.as_mut() {
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
                if let Some(input) = self.search_input.as_mut() {
                    let _ = input.key(
                        key,
                        Modifiers {
                            ctrl: *ctrl,
                            shift: *shift,
                        },
                        typed.as_deref(),
                        &mut clipboard,
                    )?;
                    self.search_query = input.text();
                    if let Some(widget) = self.search_text {
                        cx.tree.set_text(widget, &self.search_query)?;
                    }
                }
                self.page = 0;
                return self.render(cx);
            }
        }
        if clicked.is_some() && clicked == self.search_text {
            if let Some(widget) = self.search_text {
                cx.tree.set_focus(Some(widget))?;
            }
            self.search_focused = true;
            if let Some(input) = self.search_input.as_mut() {
                input.focus(true, 0);
            }
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.refresh {
            start_discovery(&self.workspace, cx);
            return Ok(());
        }
        if clicked.is_some() && clicked == self.external_reload {
            return start_reload_selected(&self.workspace, cx);
        }
        if clicked.is_some() && clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.next {
            self.page = self.page.saturating_add(1);
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
    row: WidgetId,
    label: WidgetId,
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
    candidates.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.key.cmp(&right.key))
    });
    candidates
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
    add_empty: Option<WidgetId>,
    add_previous: Option<WidgetId>,
    add_next: Option<WidgetId>,
    add_confirm: Option<WidgetId>,
    add_cancel: Option<WidgetId>,
    add_candidates: Vec<AddCandidate>,
    add_selected_key: Option<String>,
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
            add_empty: None,
            add_previous: None,
            add_next: None,
            add_confirm: None,
            add_cancel: None,
            add_candidates: Vec::new(),
            add_selected_key: None,
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
        }
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
        let money_input_state;
        let Some(selected) = state.selected.as_ref() else {
            self.add_panel_open = false;
            if let Some(panel) = self.add_panel {
                cx.tree.set_visible(panel, false)?;
            }
            if let Some(card) = self.inventory_card {
                cx.tree.set_visible(card, true)?;
            }
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
            self.selected_item = None;
            self.add_panel_open = false;
            if let Some(panel) = self.add_panel {
                cx.tree.set_visible(panel, false)?;
            }
            if let Some(card) = self.inventory_card {
                cx.tree.set_visible(card, true)?;
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
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(group) = visible_groups.get(start.saturating_add(offset)) {
                        let Some(item) = group.first().copied() else {
                            continue;
                        };
                        cx.tree.set_visible(row.row, true)?;
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
                        let condition = state
                            .pending_durability
                            .get(&ItemHandle::Xray(item.handle))
                            .map(|value| format!("{value}%"))
                            .or_else(|| item.condition.map(|value| format!("{:.0}%", value * 100.0)))
                            .unwrap_or_else(|| "—".to_owned());
                        let name = sse_catalog::SaveNaming::item_name(save.format().id(), &item.section, None);
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "◇ {} · {} · {condition} · × {count}",
                                short_text(&name, 20),
                                item.category,
                            ),
                        )?;
                        let condition_ratio = state
                            .pending_durability
                            .get(&ItemHandle::Xray(item.handle))
                            .map(|value| f32::from(*value) / 100.0)
                            .or(item.condition);
                        let condition_color = match condition_ratio {
                            Some(value) if value >= 0.75 => Color::rgba(104, 200, 144, 255),
                            Some(value) if value >= 0.4 => Color::rgba(231, 190, 91, 255),
                            Some(_) => Color::rgba(220, 103, 91, 255),
                            None => Color::rgba(145, 155, 160, 255),
                        };
                        cx.tree.set_look(
                            row.label,
                            Look {
                                text: condition_color,
                                ..Look::default()
                            },
                        )?;
                        cx.tree.set_visible(row.label, true)?;
                        cx.tree.set_visible(row.select, true)?;
                        cx.tree.set_enabled(row.select, true)?;
                        cx.tree.set_text(
                            row.select,
                            if self.selected_item == Some(ItemHandle::Xray(item.handle)) {
                                "Выбрано"
                            } else {
                                "Осмотреть"
                            },
                        )?;
                        cx.tree.set_look(
                            row.select,
                            style::nav(self.selected_item == Some(ItemHandle::Xray(item.handle))),
                        )?;
                        let editable = stack_editable && item.count.is_some();
                        cx.tree.set_visible(row.decrease, editable)?;
                        cx.tree.set_visible(row.increase, editable)?;
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
                    || !state.pending_durability.is_empty()
                    || !state.pending_placements.is_empty()
                    || !state.pending_upgrades.is_empty()
                    || !state.pending_removed.is_empty()
                    || !state.pending_adds.is_empty()
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
                    cx.tree.set_text(id, &format!("Деньги: {pending_money}"))?;
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
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(group) = visible_groups.get(start.saturating_add(offset)) {
                        let Some(item) = group.first().copied() else {
                            continue;
                        };
                        cx.tree.set_visible(row.row, true)?;
                        let name = item.display_name.as_deref().unwrap_or("Неизвестный предмет");
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
                        let category = s2_inventory_category(item.kind_code, item.display_name.as_deref());
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "◇ {} · {category} · {} · × {count} · {key}",
                                short_text(name, 18),
                                item.condition
                                    .map_or_else(|| "—".to_owned(), |value| format!("{:.0}%", value * 100.0))
                            ),
                        )?;
                        cx.tree.set_visible(row.label, true)?;
                        cx.tree.set_visible(row.select, true)?;
                        cx.tree.set_enabled(row.select, true)?;
                        cx.tree.set_text(
                            row.select,
                            if self.selected_item == Some(ItemHandle::Stalker2(item.handle)) {
                                "Выбрано"
                            } else {
                                "Осмотреть"
                            },
                        )?;
                        cx.tree.set_look(
                            row.select,
                            style::nav(self.selected_item == Some(ItemHandle::Stalker2(item.handle))),
                        )?;
                        let editable = writable && item.editable_count;
                        cx.tree.set_visible(row.decrease, editable)?;
                        cx.tree.set_visible(row.increase, editable)?;
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
                    || !state.pending_durability.is_empty()
                    || state.pending_stacks.iter().any(|(handle, count)| {
                        matches!(handle, ItemHandle::Stalker2(_))
                            && inventory
                                .iter()
                                .find(|item| *handle == ItemHandle::Stalker2(item.handle))
                                .is_some_and(|item| item.count != *count)
                    })
                    || !state.pending_stash_moves.is_empty();
                let blocked_stash_draft = !S2_STASH_MOVE_ENABLED && !state.pending_stash_moves.is_empty();
                if let Some(id) = self.export {
                    cx.tree
                        .set_visible(id, writable && has_changes && !blocked_stash_draft)?;
                    cx.tree.set_text(id, "Сохранить")?;
                }
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        if blocked_stash_draft {
                            "Черновик содержит перенос S2 из тайника, отключённый до проверки в игре. Сбросьте этот черновик, чтобы продолжить."
                        } else if writable {
                            "Изменения сохраняются с резервной копией и проверкой повторным чтением."
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
                    "Предмет не выбран\nВыберите предмет для редактирования характеристик.",
                )?;
            }
            if let Some(id) = self.inspector_condition {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, "Прочность: —")?;
            }
            if let Some(id) = self.inspector_placement {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, "Размещение: —")?;
            }
            if let Some(id) = self.inspector_upgrades {
                cx.tree.set_visible(id, true)?;
                cx.tree.set_text(id, "Модификации: —")?;
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
                    format!(
                        "{}\nКлюч: {}\nКоличество в пачке: {count}",
                        sse_catalog::SaveNaming::item_name(save.format().id(), &item.section, None),
                        item.section
                    ),
                    item.condition,
                    item.placement
                        .clone()
                        .unwrap_or_else(|| "Размещение не прочитано".to_owned()),
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
                    (Some(x), Some(y)) => format!("Рюкзак: столбец {x}, строка {y}"),
                    _ => "Размещение не прочитано".to_owned(),
                };
                (
                    format!(
                        "{}\nКлюч: {:02x}{:02x}{:02x}\nКоличество в пачке: {}",
                        item.display_name.as_deref().unwrap_or("Неизвестный предмет"),
                        item.type_key[0],
                        item.type_key[1],
                        item.type_key[2],
                        state.pending_stacks.get(&handle).copied().unwrap_or(item.count)
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
                    cx.tree.set_text(id, "Выбранный предмет отсутствует в текущем сейве.")?;
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
                .unwrap_or_else(|| "нет шкалы состояния / износа".to_owned());
            cx.tree.set_text(id, &format!("Состояние / прочность: {shown}"))?;
        }
        if let Some(id) = self.inspector_placement {
            cx.tree.set_visible(id, true)?;
            if let Some(pending) = state.pending_placements.get(&handle) {
                placement = match pending {
                    DraftPlacement::Ruck => "ruck".to_owned(),
                    DraftPlacement::Belt => "belt".to_owned(),
                    DraftPlacement::Slot(slot) => format!("slot {slot}"),
                };
            }
            cx.tree.set_text(id, &format!("Размещение: {placement}"))?;
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
                &format!("Модификации: {}", if upgrades.is_empty() { "—" } else { &upgrades }),
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
            cx.tree.set_text(id, "+ Добавить предмет")?;
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
        if let Some(card) = self.inventory_card {
            cx.tree.set_visible(card, false)?;
        }
        if let Some(panel) = self.add_panel {
            cx.tree.set_visible(panel, true)?;
        }
        self.add_panel_open = true;
        if let Some(widget) = self.add_search_widget {
            cx.tree.set_text(widget, "")?;
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
        if let Some(card) = self.inventory_card {
            cx.tree.set_visible(card, true)?;
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
        for (offset, (widget, slot)) in self.add_candidate_rows.iter_mut().enumerate() {
            if let Some(candidate) = matching.get(start.saturating_add(offset)) {
                *slot = Some(candidate.key.clone());
                let suffix = if candidate.template_available {
                    String::new()
                } else {
                    " · нет подтверждённого шаблона в сейве".to_owned()
                };
                cx.tree.set_text(
                    *widget,
                    &format!("{} · {}{suffix}", candidate.display_name, candidate.key),
                )?;
                cx.tree.set_visible(*widget, true)?;
                cx.tree.set_enabled(*widget, candidate.template_available)?;
                cx.tree.set_look(
                    *widget,
                    style::nav(self.add_selected_key.as_deref() == Some(&candidate.key)),
                )?;
            } else {
                *slot = None;
                cx.tree.set_visible(*widget, false)?;
            }
        }
        if let Some(empty) = self.add_empty {
            cx.tree.set_visible(empty, matching.is_empty())?;
            cx.tree.set_text(empty, "Предметы не найдены.")?;
        }
        if let Some(previous) = self.add_previous {
            cx.tree.set_visible(previous, pages > 1 && self.add_page > 0)?;
        }
        if let Some(next) = self.add_next {
            cx.tree
                .set_visible(next, pages > 1 && self.add_page.saturating_add(1) < pages)?;
        }
        let has_template = self.add_selected_key.as_ref().is_some_and(|key| {
            matching
                .iter()
                .any(|candidate| candidate.key == *key && candidate.template_available)
        });
        if let Some(confirm) = self.add_confirm {
            cx.tree.set_enabled(confirm, has_template)?;
        }
        if let (Some(widget), Some(quantity)) = (self.add_quantity_widget, self.add_quantity.as_ref()) {
            if !quantity.focused() {
                cx.tree.set_text(widget, &quantity.text())?;
            }
        }
        Ok(())
    }

    fn stage_add_key(&self, cx: &mut Context<'_>, item_key: &str, quantity: u32) -> Result<()> {
        let selected = self.workspace.lock().selected.clone();
        let Some(selected) = selected else {
            return Ok(());
        };
        let SaveData::Xray { save, inventory } = &selected.data else {
            cx.status = Some("Добавление доступно только для подтверждённых X-Ray форматов.".to_owned());
            return Ok(());
        };
        if writer::capability(save.format(), writer::ChangeKind::AddItems) == writer::Capability::Unsupported
            || sse_catalog::CatalogBundleReader::load_embedded()
                .get(save.format().id())
                .and_then(|bundle| bundle.items.resolve(item_key))
                .is_none()
            || !inventory.iter().any(|item| item.section == item_key)
        {
            cx.status = Some("Для этого предмета или формата нет подтверждённого шаблона добавления.".to_owned());
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
        cx.status = Some(format!(
            "Предмет {item_key} ({quantity} шт.) добавлен в очередь на запись."
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

    fn save(&self, cx: &mut Context<'_>) -> Result<()> {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some("Сохранение доступно в работающем окне редактора.".to_owned());
            return Ok(());
        };
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
                },
                state.pending_stash_moves.clone(),
            )
        };
        let Some(selected) = selected else {
            cx.status = Some("Сначала выберите сейв.".to_owned());
            return Ok(());
        };
        if !S2_STASH_MOVE_ENABLED && !stash_moves.is_empty() {
            let text = "Черновик содержит перенос S2 из тайника, отключённый до проверки в игре. Сбросьте этот черновик, чтобы продолжить.";
            if let Some(status) = self.status {
                cx.tree.set_text(status, text)?;
            }
            cx.status = Some(text.to_owned());
            return Ok(());
        }
        if cx.app.has_invalid_numeric_input() {
            cx.status = Some("Введены некорректные значения (проверьте введённые числа).".to_owned());
            return Ok(());
        }
        if !cx.app.has_draft(&selected.source_sha256) && !edits.has_changes() && stash_moves.is_empty() {
            cx.status = Some("Нет несохранённых изменений.".to_owned());
            return Ok(());
        }
        let source_sha256 = selected.source_sha256.clone();
        if let Some(plan) = cx.app.draft(&selected.source_sha256) {
            if plan.unmapped_legacy_plan.is_some() {
                cx.status =
                    Some("В черновике есть правки из другой версии редактора, которые эта версия не понимает. Сбросьте черновик, чтобы продолжить (он сохранится рядом).".to_owned());
                return Ok(());
            }
        }
        let Some(request_id) = self.workspace.begin_saving() else {
            let text = "Сохранение уже выполняется.";
            if let Some(status) = self.status {
                cx.tree.set_text(status, text)?;
            }
            cx.status = Some(text.to_owned());
            return Ok(());
        };
        let source_path = selected.slot.path.clone();
        let backup_directory = self.workspace.backup_directory();
        if let Some(status) = self.status {
            cx.tree.set_text(status, "Сохранение…")?;
        }
        self.workspace.spawn("save-write", move |context| {
            let result = if context.is_cancelled() {
                Err("Сохранение отменено.".to_owned())
            } else {
                commit_save_edits(&selected, &edits, &stash_moves, &backup_directory).map_err(|error| error.to_string())
            };
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(SaveFinished {
                    request_id,
                    source_path,
                    source_sha256,
                    result,
                }),
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
    request_id: u64,
    source_path: PathBuf,
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
    let preflight_image = packed.clone();
    let readback_image = packed.clone();
    let (receipt, mut reloaded, (size, modified)) =
        transaction::replace_transaction_with_summary_preflight_and_verifier(
            &selected.slot.path,
            &selected.source_sha256,
            packed.as_slice(),
            backup_directory,
            summary,
            |_, replacement| {
                if replacement != preflight_image.as_slice() {
                    return Err(Error::damaged("prepared save bytes changed before semantic preflight"));
                }
                let reloaded = LoadedSave::from_buffer(selected.slot.clone(), preflight_image.clone())?;
                verify_requested_values(selected, &reloaded, edits, stash_moves)?;
                Ok(reloaded)
            },
            |read_back| {
                if read_back != readback_image.as_slice() {
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
        SaveData::Xray { save, .. } => ("не подтверждается отдельным полем", save.format().id()),
        SaveData::Stalker2 { save, .. } => (
            if save.container().stored_crc32() == save.container().computed_crc32() {
                "OK (CRC32)"
            } else {
                "ошибка"
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
    Ok((
        Arc::new(reloaded),
        format!(
            "Сохранено успешно. Backup: {}",
            receipt.backup_path.file_name().map_or_else(
                || receipt.backup_path.display().to_string(),
                |name| name.to_string_lossy().into_owned()
            )
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
            let current_money = save.money()?;
            let money_change = edits.money.filter(|value| *value != current_money);
            let mut changes = Vec::new();
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
                        (
                            object.story_id.is_some_and(|value| value != u32::MAX),
                            object.spawn_story_id.is_some_and(|value| value != u32::MAX),
                            save.custom_data(object).is_some_and(|data| !data.is_empty()),
                            object.spawn_id != Some(u16::MAX),
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
        "Состав рюкзака и подтверждённые изменения X-Ray / S2"
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (row, banner, reload) = add_external_file_banner(cx.tree, host)?;
        self.external_banner_row = Some(row);
        self.external_banner = Some(banner);
        self.external_reload = Some(reload);
        let inventory = style::card(cx.tree, host)?;
        self.inventory_card = Some(inventory);
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
        let item_rows = cx.tree.add(
            Some(inventory),
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        for _ in 0..INVENTORY_PAGE_SIZE {
            let row = style::row(cx.tree, item_rows)?;
            cx.tree.set_visible(row, false)?;
            let label = style::label(cx.tree, row, "", Text::Body)?;
            let select = style::button(cx.tree, row, "Осмотреть", Button::Secondary)?;
            let decrease = style::button(cx.tree, row, "−", Button::Secondary)?;
            let increase = style::button(cx.tree, row, "+", Button::Secondary)?;
            self.rows.push(ItemControls {
                row,
                label,
                select,
                decrease,
                increase,
                handle: None,
            });
        }
        let inspector = style::card(cx.tree, inventory)?;
        style::label(cx.tree, inspector, "ВЫБРАННЫЙ ПРЕДМЕТ", Text::Heading)?;
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
            self.condition_buttons.push((button, percent));
        }
        style::label(cx.tree, inspector, "РАЗМЕЩЕНИЕ", Text::Heading)?;
        self.inspector_placement = Some(style::label(cx.tree, inspector, "Размещение: —", Text::Body)?);
        let placement_row = style::row(cx.tree, inspector)?;
        for (label, destination) in [("Рюкзак", DraftPlacement::Ruck), ("Пояс", DraftPlacement::Belt)] {
            let button = style::button(cx.tree, placement_row, label, Button::Secondary)?;
            self.placement_buttons.push((button, destination));
        }
        style::label(cx.tree, inspector, "МОДИФИКАЦИИ", Text::Heading)?;
        self.inspector_upgrades = Some(style::label(cx.tree, inspector, "Модификации: —", Text::Body)?);
        let upgrade_row = style::row(cx.tree, inspector)?;
        for _ in 0..MAXIMUM_UPGRADE_ROWS {
            let widget = style::button(cx.tree, upgrade_row, "", Button::Secondary)?;
            cx.tree.set_visible(widget, false)?;
            self.upgrade_controls.push(UpgradeControl { widget, key: None });
        }
        let edit_actions = style::row(cx.tree, inspector)?;
        self.remove_button = Some(style::button(cx.tree, edit_actions, "Удалить предмет", Button::Danger)?);
        self.add_button = Some(style::button(
            cx.tree,
            edit_actions,
            "+ Добавить предмет",
            Button::Primary,
        )?);
        self.export = Some(style::button(cx.tree, inventory, "Сохранить", Button::Primary)?);
        self.status = Some(style::label(
            cx.tree,
            inventory,
            "Изменения пока не подготовлены.",
            Text::Note,
        )?);
        let panel = style::card(cx.tree, host)?;
        self.add_panel = Some(panel);
        style::label(cx.tree, panel, "ДОБАВИТЬ ПРЕДМЕТ", Text::Heading)?;
        style::label(
            cx.tree,
            panel,
            "Доступны записи каталога с сериализованным шаблоном в этом сейве.",
            Text::Note,
        )?;
        let add_search = cx.tree.add(
            Some(panel),
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
                text: "Поиск по названию или ключу секции…".to_owned(),
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
        let add_empty = style::label(cx.tree, panel, "Предметы не найдены.", Text::Note)?;
        self.add_empty = Some(add_empty);
        cx.tree.set_visible(add_empty, false)?;
        for _ in 0..ADD_ITEM_PAGE_SIZE {
            let widget = style::button(cx.tree, panel, "", Button::Secondary)?;
            cx.tree.set_visible(widget, false)?;
            self.add_candidate_rows.push((widget, None));
        }
        let add_pages = style::row(cx.tree, panel)?;
        self.add_previous = Some(style::button(cx.tree, add_pages, "Назад", Button::Secondary)?);
        self.add_next = Some(style::button(cx.tree, add_pages, "Дальше", Button::Secondary)?);
        let quantity_row = style::row(cx.tree, panel)?;
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
        let add_actions = style::row(cx.tree, panel)?;
        self.add_confirm = Some(style::button(cx.tree, add_actions, "Добавить", Button::Primary)?);
        self.add_cancel = Some(style::button(cx.tree, add_actions, "Отмена", Button::Secondary)?);
        cx.tree.set_visible(panel, false)?;
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
                cx.status = Some("Количество должно быть от 1 до 65535 для этого формата.".to_owned());
                return Ok(());
            }
            let Some(key) = self.add_selected_key.clone() else {
                cx.status = Some("Выберите предмет с подтверждённым шаблоном добавления.".to_owned());
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
                source_path,
                source_sha256,
                result,
            }) = payload.downcast_ref::<SaveFinished>()
            {
                if !self.workspace.finish_saving(*request_id) {
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
                        state.external_change = false;
                        drop(state);
                        cx.app.discard_draft(source_sha256);
                        let old_empty = DraftJournal::new(vec![DraftPlan::empty(source_sha256)?], 0)?;
                        let new_journal = DraftJournal::new(vec![DraftPlan::empty(&loaded.source_sha256)?], 0)?;
                        cx.app
                            .set_current_save_identity(loaded.slot.path.clone(), loaded.source_sha256.clone());
                        let legacy_s2 =
                            matches!(&loaded.data, SaveData::Stalker2 { save, .. } if save.index().is_legacy());
                        cx.app.set_current_save_format(loaded.slot.format_id.clone(), legacy_s2);
                        cx.app.set_draft_journal(new_journal.clone());
                        set_workspace_draft(&self.workspace, &new_journal);
                        self.workspace.persist_drafts(vec![old_empty, new_journal], cx);
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
                        let text = format!("Не удалось сохранить: {error}");
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
            let can_move = S2_STASH_MOVE_ENABLED && !save.index().is_legacy() && save.unresolved_handles().is_empty();
            let pages = items.len().div_ceil(self.rows.len().max(1));
            self.page = self.page.min(pages.saturating_sub(1));
            let start = self.page.saturating_mul(self.rows.len());
            self.set_text(
                cx,
                &format!(
                    "S2: {} предметов в подтверждённом тайнике · страница {} из {}. {}",
                    items.len(),
                    self.page.saturating_add(1),
                    pages,
                    if can_move {
                        "Отметьте перенос и сохраните его в «Инвентаре»."
                    } else {
                        "Перенос в рюкзак отключён до проверки сохранения в игре."
                    }
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
            if !S2_STASH_MOVE_ENABLED {
                cx.status = Some("Перенос из тайника S2 отключён до проверки сохранения в игре.".to_owned());
                return Ok(());
            }
            if save.index().is_legacy() || !save.unresolved_handles().is_empty() {
                cx.status = Some("Перенос недоступен для этого S2-сейва.".to_owned());
                return Ok(());
            }
            if !state.pending_stash_moves.is_empty() && !state.pending_stash_moves.contains(&handle) {
                cx.status =
                    Some("За один раз можно перенести только один предмет. Отмените предыдущий перенос.".to_owned());
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
        add_external_file_banner, commit_save_edits_to, prepare_save_edits, prepare_xray_edits, AddRequest,
        DraftJournal, DraftPlan, DraftStore, Inventory, ItemHandle, LoadFinished, LoadedSave, Overview,
        PendingInventoryEdits, S2Save, SaveBuffer, SaveSlot, Workspace,
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
    use std::time::UNIX_EPOCH;

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn save_library_and_overview_dates_match_reference_patterns() {
        assert_eq!(super::display_file_time(UNIX_EPOCH, true, false), "01.01.70 00:00");
        assert_eq!(super::display_file_time(UNIX_EPOCH, false, true), "01.01.1970 00:00:00");
    }

    #[test]
    fn stale_save_completion_cannot_unlock_a_newer_write() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let first = workspace
            .begin_saving()
            .ok_or_else(|| Error::Refused("first test save did not start".to_owned()))?;
        assert!(workspace.finish_saving(first));
        let second = workspace
            .begin_saving()
            .ok_or_else(|| Error::Refused("second test save did not start".to_owned()))?;
        assert_ne!(first, second);

        assert!(!workspace.finish_saving(first));
        assert!(workspace.is_saving());
        assert!(workspace.finish_saving(second));
        assert!(!workspace.is_saving());
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

        let move_button = screen
            .rows
            .first()
            .map(|row| row.move_button)
            .ok_or_else(|| Error::damaged("S2 stash row was not built"))?;
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
            "\"operation\":{{\"mode\":\"replace\",\"money\":{xray_money},\"stack_count\":0}}"
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
            "\"operation\":{{\"mode\":\"replace\",\"money\":{xray_stack_money},\"stack_count\":1}}"
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
