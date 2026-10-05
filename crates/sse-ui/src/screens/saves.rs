//! S2 save screens: discovery, overview, inventory, factions, stashes and transitions.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::layout::{NodeKind, Style};
use crate::widget::{Content, Look, WidgetId};
use crate::widgets::table::{Header, Table};
use sse_core::{Error, Result, SaveBuffer};
use sse_s2::{S2Change, S2InventoryItem, S2Save, S2StashItem, S2StashLayout};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::transaction::{self, EditSummary};
use sse_xray::{save::InventoryItem, writer, Save};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

const SAVE_PAGE_SIZE: usize = 10;
const INVENTORY_PAGE_SIZE: usize = 8;
const MAXIMUM_STASH_ROWS: usize = 10;

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

#[derive(Clone, Default)]
pub(crate) struct Workspace(Arc<Mutex<WorkspaceState>>);

impl Workspace {
    fn lock(&self) -> MutexGuard<'_, WorkspaceState> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
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

struct LoadedSave {
    slot: SaveSlot,
    source_sha256: String,
    packed_size: u64,
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
        Self::from_buffer(slot, packed)
    }

    fn from_buffer(slot: SaveSlot, packed: SaveBuffer) -> Result<Self> {
        let source_sha256 = sse_codecs::sha256::sha256_hex(packed.as_slice());
        match S2Save::from_bytes(packed.as_slice()) {
            Ok(save) => {
                let inventory = save.items();
                let stash = save.stash().ok();
                Self::from_s2(slot, packed, source_sha256, save, inventory, stash)
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
        }
    }

    fn from_xray(
        mut slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: Save,
        inventory: Vec<InventoryItem>,
    ) -> Result<Self> {
        let packed_size = u64::try_from(packed.len())
            .map_err(|_| Error::damaged("X-Ray packed size does not fit the save metadata"))?;
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
        let factions = match save.player_faction() {
            Some(id) => format!("Фракция игрока: ID {id}\nРедактирование отношений недоступно в текущем индексаторе."),
            None => "Идентификатор фракции игрока не подтверждён этим сохранением.".to_owned(),
        };
        let stashes = describe_xray_stashes(&save);
        let transitions = describe_xray_transitions(&save);
        let mut loaded = Self {
            slot,
            source_sha256,
            packed_size,
            summary: String::new(),
            factions,
            stashes,
            transitions,
            data: SaveData::Xray { save, inventory },
        };
        loaded.refresh_summary()?;
        Ok(loaded)
    }

    fn from_s2(
        mut slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: S2Save,
        inventory: Vec<S2InventoryItem>,
        stash: Option<S2StashLayout>,
    ) -> Result<Self> {
        let packed_size =
            u64::try_from(packed.len()).map_err(|_| Error::damaged("S2 packed size does not fit the save metadata"))?;
        slot.game_id = Some("stalker2".to_owned());
        slot.candidate_game_id = "stalker2".to_owned();
        slot.candidate_release_id = "stalker2".to_owned();
        slot.format_id = Some("stalker2".to_owned());
        let factions =
            "Фракции S2 доступны только для чтения; отношения и принадлежность пока не индексируются.".to_owned();
        let stashes = describe_s2_stash(stash.as_ref());
        let transitions = "Переходы S2 доступны только для чтения, но их формат пока не индексируется.".to_owned();
        let stash_items = stash
            .as_ref()
            .map(|_| save.stash_items().map_err(|error| error.to_string()));
        let mut loaded = Self {
            slot,
            source_sha256,
            packed_size,
            summary: String::new(),
            factions,
            stashes,
            transitions,
            data: SaveData::Stalker2 {
                save,
                inventory,
                stash_items,
            },
        };
        loaded.refresh_summary()?;
        Ok(loaded)
    }

    fn refresh_summary(&mut self) -> Result<()> {
        self.summary = match &self.data {
            SaveData::Xray { save, inventory } => {
                let format = save.format().id();
                format!(
                    "Игра: {format}\nЛокация: не подтверждена текущим индексатором\nИзменён (Unix UTC): {}\nРазмер: {} байт\nФормат: {format}\nCRC: контейнер X-Ray не хранит CRC\nВремя игры: {}\nДеньги: {}\nПредметов в инвентаре: {}",
                    unix_time(self.slot.last_write_time_utc),
                    self.packed_size,
                    save.game_time(),
                    save.money()?,
                    inventory.len()
                )
            }
            SaveData::Stalker2 { save, inventory, .. } => format!(
                "Игра: S.T.A.L.K.E.R. 2\nЛокация: не подтверждена текущим индексатором\nИзменён (Unix UTC): {}\nРазмер: {} байт\nФормат: S2\nCRC32: {:08X} — проверен\nДеньги: {}\nПредметов в рюкзаке: {}\nНеопознанных ссылок: {}",
                unix_time(self.slot.last_write_time_utc),
                self.packed_size,
                save.container().stored_crc32(),
                save.money(),
                inventory.len(),
                save.unresolved_handles().len()
            ),
        };
        Ok(())
    }

    fn update_file_metadata(&mut self, size: u64, modified: std::time::SystemTime) -> Result<()> {
        self.slot.size = size;
        self.slot.last_write_time_utc = modified;
        self.packed_size = size;
        self.refresh_summary()
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
    workspace.clone().spawn("save-load", move |context| {
        if context.is_cancelled() {
            return;
        }
        let result = slot().and_then(LoadedSave::read);
        let mut state = workspace.lock();
        if state.load_request != request {
            return;
        }
        state.loading = false;
        let completion = match result {
            Ok(save) => {
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
    error: Option<String>,
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
                error,
            }) = payload.downcast_ref::<LoadFinished>()
            {
                if self.workspace.lock().load_request == *request {
                    cx.app.set_current_save(selected_path.clone());
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
    money_decrease: Option<WidgetId>,
    money_increase: Option<WidgetId>,
    previous: Option<WidgetId>,
    next: Option<WidgetId>,
    export: Option<WidgetId>,
    status: Option<WidgetId>,
    rows: Vec<ItemControls>,
    last_path: Option<PathBuf>,
}

impl Inventory {
    fn new(workspace: Workspace) -> Self {
        Self {
            workspace,
            page: 0,
            money_label: None,
            money_decrease: None,
            money_increase: None,
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
                if let Some(id) = self.money_decrease {
                    cx.tree.set_visible(id, money_editable)?;
                }
                if let Some(id) = self.money_increase {
                    cx.tree.set_visible(id, money_editable)?;
                }
                let stack_editable =
                    writer::capability(save.format(), writer::ChangeKind::EditStacks) == writer::Capability::Verified;
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(item) = inventory.get(start.saturating_add(offset)) {
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
                                "{} · {} · кол-во {count} · 0x{:04X}",
                                item.section, item.category, item.handle
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
                let pages = inventory.len().saturating_add(INVENTORY_PAGE_SIZE.saturating_sub(1)) / INVENTORY_PAGE_SIZE;
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
                for id in [self.money_decrease, self.money_increase].into_iter().flatten() {
                    cx.tree.set_visible(id, writable)?;
                }
                let start = self.page.saturating_mul(INVENTORY_PAGE_SIZE);
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(item) = inventory.get(start.saturating_add(offset)) {
                        let name = item.display_name.as_deref().unwrap_or("Неизвестный предмет");
                        let count = state
                            .pending_stacks
                            .get(&ItemHandle::Stalker2(item.handle))
                            .copied()
                            .unwrap_or(item.count);
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "{name} · кол-во {count} · тип {:02X}{:02X}{:02X} · 0x{:08X}",
                                item.type_key[0], item.type_key[1], item.type_key[2], item.handle
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
                let pages = inventory.len().saturating_add(INVENTORY_PAGE_SIZE.saturating_sub(1)) / INVENTORY_PAGE_SIZE;
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
        Ok(())
    }

    fn set_edit_controls(&self, cx: &mut Context<'_>, visible: bool) -> Result<()> {
        for id in [self.money_decrease, self.money_increase, self.export]
            .into_iter()
            .flatten()
        {
            cx.tree.set_visible(id, visible)?;
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

    fn stage_money(&self, increase: bool) {
        let mut state = self.workspace.lock();
        let Some(selected) = state.selected.as_ref() else {
            return;
        };
        let current_money = match &selected.data {
            SaveData::Xray { save, .. } => save.money().ok(),
            SaveData::Stalker2 { save, .. } if !save.index().is_legacy() => Some(save.money()),
            SaveData::Stalker2 { .. } => None,
        };
        let Some(current_money) = current_money else {
            return;
        };
        let current = state.pending_money.unwrap_or(current_money);
        state.pending_money = Some(if increase {
            current.saturating_add(1_000).min(2_000_000_000)
        } else {
            current.saturating_sub(1_000)
        });
    }

    fn stage_stack(&self, handle: ItemHandle, increase: bool) {
        let mut state = self.workspace.lock();
        let Some(selected) = state.selected.as_ref() else {
            return;
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
            return;
        };
        let current = state.pending_stacks.get(&handle).copied().unwrap_or(original);
        let next = if increase {
            let maximum = match handle {
                ItemHandle::Xray(_) => u32::from(u16::MAX),
                ItemHandle::Stalker2(_) => 10_000_000,
            };
            current.saturating_add(1).min(maximum)
        } else {
            current.saturating_sub(1).max(1)
        };
        state.pending_stacks.insert(handle, next);
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
                Box::new(SaveFinished(result)),
            ));
        });
        Ok(())
    }
}

struct SaveFinished(std::result::Result<(Arc<LoadedSave>, String), String>);

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
    let prepared = packed.clone();
    let (receipt, mut reloaded) = transaction::replace_transaction_with_preflight(
        &selected.slot.path,
        &selected.source_sha256,
        packed.as_slice(),
        backup_directory,
        |_, replacement| {
            if replacement != prepared.as_slice() {
                return Err(Error::damaged(
                    "prepared save bytes changed before transaction preflight",
                ));
            }
            verify_prepared_output(selected, prepared, money, stacks, stash_moves)
        },
    )?;
    let metadata = std::fs::metadata(&selected.slot.path)?;
    let modified = metadata.modified().unwrap_or(reloaded.slot.last_write_time_utc);
    reloaded.update_file_metadata(metadata.len(), modified)?;
    Ok((
        Arc::new(reloaded),
        format!(
            "Сохранено и повторно прочитано · бэкап {} · SHA-256 {}",
            receipt.backup_path.display(),
            receipt.output_sha256
        ),
    ))
}

fn verify_prepared_output(
    selected: &LoadedSave,
    packed: SaveBuffer,
    money: Option<u32>,
    stacks: &BTreeMap<ItemHandle, u32>,
    stash_moves: &BTreeSet<u32>,
) -> Result<LoadedSave> {
    let prepared = LoadedSave::from_buffer(selected.slot.clone(), packed)?;
    verify_requested_values(&prepared, money, stacks, stash_moves)?;
    Ok(prepared)
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
        self.money_decrease = Some(style::button(cx.tree, money, "−1000", Button::Secondary)?);
        self.money_increase = Some(style::button(cx.tree, money, "+1000", Button::Secondary)?);
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
        if let Message::Window(crate::event_loop::WindowEvent::Key {
            pressed: true,
            ctrl: true,
            keysym,
            ..
        }) = message
        {
            if matches!(*keysym, 0x53 | 0x73) {
                return self.save(cx);
            }
        }
        if clicked.is_some() && clicked == self.money_decrease {
            self.stage_money(false);
            return self.render(cx);
        }
        if clicked.is_some() && clicked == self.money_increase {
            self.stage_money(true);
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
                    self.stage_stack(handle, clicked == Some(row.increase));
                }
                return self.render(cx);
            }
        }
        if clicked.is_some() && clicked == self.export {
            return self.save(cx);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Inventory, payload)) = message {
            if let Some(SaveFinished(result)) = payload.downcast_ref::<SaveFinished>() {
                match result {
                    Ok((loaded, text)) => {
                        let mut state = self.workspace.lock();
                        state.selected = Some(Arc::clone(loaded));
                        state.pending_money = None;
                        state.pending_stacks.clear();
                        state.pending_stash_moves.clear();
                        drop(state);
                        cx.app.set_current_save(Some(loaded.slot.path.clone()));
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
        commit_save_edits_to, prepare_save_edits, prepare_xray_edits, verify_prepared_output, LoadFinished, LoadedSave,
        Overview, S2Save, SaveBuffer, SaveSlot, Workspace,
    };
    use crate::event_loop::{channel_pair, Message};
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
    fn prepared_semantic_mismatch_is_rejected_before_transaction() -> sse_core::Result<()> {
        let source = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let expected = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let loaded = load_xray(source, "xray-money-cop-source.sav", "stalker-cop", "cop")?;

        let result = verify_prepared_output(
            &loaded,
            SaveBuffer::from_vec(expected.to_vec()),
            Some(1),
            &BTreeMap::new(),
            &BTreeSet::new(),
        );
        let error = match result {
            Ok(_) => return Err(Error::damaged("prepared output accepted an unexpected money value")),
            Err(error) => error,
        };

        assert!(error.to_string().contains("saved wallet value differs after read-back"));
        Ok(())
    }

    #[test]
    fn s2_prepared_semantic_mismatch_is_rejected_before_transaction() -> sse_core::Result<()> {
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
        )?;
        let (prepared, _) = prepare_save_edits(&loaded, Some(1_000), &BTreeMap::new(), &BTreeSet::new())?;

        assert!(
            verify_prepared_output(&loaded, prepared, Some(999), &BTreeMap::new(), &BTreeSet::new(),)
                .is_err_and(|error| error.to_string().contains("saved wallet value differs after read-back"))
        );
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
        )?;

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
        let mut overview = Overview::new(Workspace::default());
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
        Ok(())
    }
}
