//! S2 save screens: discovery, overview, inventory, factions, stashes and transitions.

use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::{Error, Result, SaveBuffer};
use sse_s2::{S2InventoryItem, S2Save, S2StashLayout};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::transaction::{self, EditSummary};
use sse_xray::{save::InventoryItem, writer, Save};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

const SAVE_PAGE_SIZE: usize = 10;
const INVENTORY_PAGE_SIZE: usize = 8;

/// Screens of this package.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    let workspace = Workspace::default();
    vec![
        Box::new(Overview::new(workspace.clone())),
        Box::new(Inventory::new(workspace.clone())),
        Box::new(Factions::new(workspace.clone())),
        Box::new(Stashes::new(workspace.clone())),
        Box::new(Transitions::new(workspace)),
    ]
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
    scanning: bool,
    discovery: Option<sse_storage::discovery::SaveDiscoveryResult>,
    loading: bool,
    load_request: u64,
    load_error: Option<String>,
    selected: Option<Arc<LoadedSave>>,
    pending_money: Option<u32>,
    pending_stacks: BTreeMap<u16, u16>,
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
        slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: Save,
        inventory: Vec<InventoryItem>,
    ) -> Result<Self> {
        let money = save.money()?;
        let format = save.format().id();
        let summary = format!(
            "Формат: {format}\nРазмер: {} байт\nВремя игры: {}\nДеньги: {money}\nПредметов в инвентаре: {}",
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
        slot: SaveSlot,
        packed: SaveBuffer,
        source_sha256: String,
        save: S2Save,
        inventory: Vec<S2InventoryItem>,
        stash: Option<S2StashLayout>,
    ) -> Self {
        let summary = format!(
            "Формат: S.T.A.L.K.E.R. 2\nРазмер: {} байт\nДеньги: {}\nПредметов в рюкзаке: {}\nНеопознанных ссылок: {}",
            packed.len(),
            save.money(),
            inventory.len(),
            save.unresolved_handles().len()
        );
        let factions = "Сведения о фракциях для S2 не входят в подтверждённый индекс.".to_owned();
        let stashes = describe_s2_stash(stash.as_ref());
        let transitions = "Переходы между локациями для S2 пока не индексируются.".to_owned();
        Self {
            slot,
            source_sha256,
            summary,
            factions,
            stashes,
            transitions,
            data: SaveData::Stalker2 { save, inventory },
        }
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
    std::thread::spawn(move || {
        let candidates = SaveDirectoryLocator::find_candidate_directories(None);
        let result = SaveSlotDiscovery::discover(&candidates);
        workspace.lock().discovery = Some(result);
        workspace.lock().scanning = false;
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
    });
    cx.status = Some("Ищу сейвы в обнаруженных каталогах…".to_owned());
}

fn start_load(workspace: &Workspace, slot: SaveSlot, cx: &mut Context<'_>) {
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
        state.load_request
    };
    let workspace = workspace.clone();
    std::thread::spawn(move || {
        let result = LoadedSave::read(slot);
        let mut state = workspace.lock();
        if state.load_request != request {
            return;
        }
        state.loading = false;
        match result {
            Ok(save) => {
                state.load_error = None;
                state.selected = Some(Arc::new(save));
            }
            Err(error) => {
                state.selected = None;
                state.load_error = Some(error.to_string());
            }
        }
        drop(state);
        let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
    });
    cx.status = Some("Загружаю и проверяю выбранный сейв…".to_owned());
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
        }
    }

    fn render(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let slot_count = state.discovery.as_ref().map_or(0, |result| result.slots.len());
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
        for (offset, id) in self.rows.iter().enumerate() {
            let slot = state
                .discovery
                .as_ref()
                .and_then(|result| result.slots.get(start.saturating_add(offset)));
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
                cx.tree
                    .set_text(*id, &format!("{shortened} · {game} · {} MiB", slot.size / 1_048_576))?;
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
        self.previous = Some(style::button(cx.tree, actions, "Назад", Button::Secondary)?);
        self.next = Some(style::button(cx.tree, actions, "Дальше", Button::Secondary)?);
        self.list_status = Some(style::label(cx.tree, list, "Сейвы ещё не искали.", Text::Note)?);
        for _ in 0..SAVE_PAGE_SIZE {
            let row = style::button(cx.tree, list, "", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(row);
        }
        let overview = style::card(cx.tree, host)?;
        style::label(cx.tree, overview, "ОБЗОР ВЫБРАННОГО СЕЙВА", Text::Heading)?;
        self.selected_summary = Some(style::label(cx.tree, overview, "Выберите сейв из списка.", Text::Body)?);
        Ok(())
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        if self.workspace.lock().discovery.is_none() {
            start_discovery(&self.workspace, cx);
        }
        self.render(cx)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        if clicked == self.refresh {
            start_discovery(&self.workspace, cx);
            return Ok(());
        }
        if clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked == self.next {
            self.page = self.page.saturating_add(1);
            return self.render(cx);
        }
        if let Some(offset) = clicked.and_then(|id| self.rows.iter().position(|row| *row == id)) {
            let state = self.workspace.lock();
            let index = self.page.saturating_mul(SAVE_PAGE_SIZE).saturating_add(offset);
            if let Some(slot) = state
                .discovery
                .as_ref()
                .and_then(|result| result.slots.get(index))
                .cloned()
            {
                drop(state);
                start_load(&self.workspace, slot, cx);
                return Ok(());
            }
        }
        if matches!(message, Message::User(AppMessage::ToScreen(ScreenId::Overview, _))) {
            self.render(cx)?;
        }
        Ok(())
    }
}

struct ItemControls {
    label: WidgetId,
    decrease: WidgetId,
    increase: WidgetId,
    handle: Option<u16>,
}

/// Inventory screen with guarded X-Ray edits and safe copy export.
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
                cx.tree.set_text(
                    status,
                    "Для S2 пока доступно только чтение; X-Ray правки появятся после выбора сейва.",
                )?;
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
                                    .get(&item.handle)
                                    .copied()
                                    .unwrap_or(original)
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
                        row.handle = Some(item.handle);
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
                        inventory
                            .iter()
                            .find(|item| item.handle == *handle)
                            .and_then(|item| item.count)
                            .is_some_and(|old| old != *count)
                    });
                if let Some(id) = self.export {
                    cx.tree.set_visible(id, has_changes)?;
                }
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        "Изменения пока только подготовлены. Экспорт создаёт новый файл и отдельный проверяемый бэкап.",
                    )?;
                }
            }
            SaveData::Stalker2 { save, inventory } => {
                if let Some(id) = self.money_label {
                    cx.tree
                        .set_text(id, &format!("Деньги: {} · S2 открыт только для чтения", save.money()))?;
                }
                self.set_edit_controls(cx, false)?;
                for (offset, row) in self.rows.iter_mut().enumerate() {
                    if let Some(item) =
                        inventory.get(self.page.saturating_mul(INVENTORY_PAGE_SIZE).saturating_add(offset))
                    {
                        let name = item.display_name.as_deref().unwrap_or("Неизвестный предмет");
                        cx.tree.set_text(
                            row.label,
                            &format!(
                                "{name} · кол-во {} · тип {:02X}{:02X}{:02X} · 0x{:08X}",
                                item.count, item.type_key[0], item.type_key[1], item.type_key[2], item.handle
                            ),
                        )?;
                        cx.tree.set_visible(row.label, true)?;
                    } else {
                        cx.tree.set_visible(row.label, false)?;
                    }
                    cx.tree.set_visible(row.decrease, false)?;
                    cx.tree.set_visible(row.increase, false)?;
                    row.handle = None;
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
                if let Some(id) = self.status {
                    cx.tree.set_text(
                        id,
                        "Правка S2 отключена: writer ещё не прошёл проверку по эталонным сейвам.",
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
        let SaveData::Xray { save, .. } = &selected.data else {
            return;
        };
        let current = state.pending_money.unwrap_or_else(|| save.money().unwrap_or_default());
        state.pending_money = Some(if increase {
            current.saturating_add(1_000).min(2_000_000_000)
        } else {
            current.saturating_sub(1_000)
        });
    }

    fn stage_stack(&self, handle: u16, increase: bool) {
        let mut state = self.workspace.lock();
        let Some(selected) = state.selected.as_ref() else {
            return;
        };
        let SaveData::Xray { inventory, .. } = &selected.data else {
            return;
        };
        let Some(item) = inventory.iter().find(|item| item.handle == handle) else {
            return;
        };
        let Some(original) = item.count else {
            return;
        };
        let current = state.pending_stacks.get(&handle).copied().unwrap_or(original);
        let next = if increase {
            current.saturating_add(1)
        } else {
            current.saturating_sub(1).max(1)
        };
        state.pending_stacks.insert(handle, next);
    }

    fn export(&self, cx: &mut Context<'_>) -> Result<()> {
        let Some(proxy) = cx.proxy.cloned() else {
            cx.status = Some("Экспорт доступен в работающем окне редактора.".to_owned());
            return Ok(());
        };
        let (selected, money, stacks) = {
            let state = self.workspace.lock();
            (
                state.selected.clone(),
                state.pending_money,
                state.pending_stacks.clone(),
            )
        };
        let Some(selected) = selected else {
            cx.status = Some("Сначала выберите сейв.".to_owned());
            return Ok(());
        };
        if let Some(status) = self.status {
            cx.tree.set_text(status, "Подготавливаю проверяемый экспорт…")?;
        }
        std::thread::spawn(move || {
            let result = export_xray_edits(&selected, money, &stacks)
                .map(|receipt| {
                    format!(
                        "Экспортирован {} · SHA-256 {}",
                        receipt.output_path.display(),
                        receipt.output_sha256
                    )
                })
                .unwrap_or_else(|error| format!("Экспорт не выполнен: {error}"));
            let _ = proxy.send(AppMessage::ToScreen(
                ScreenId::Inventory,
                Box::new(ExportFinished(result)),
            ));
        });
        Ok(())
    }
}

struct ExportFinished(String);

fn export_xray_edits(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<u16, u16>,
) -> Result<transaction::ExportReceipt> {
    let (packed, summary) = prepare_xray_edits(selected, money, stacks)?;
    let output_path = edited_output_path(&selected.slot.path);
    transaction::export_transaction(
        &selected.slot.path,
        &selected.source_sha256,
        packed.as_slice(),
        &output_path,
        &default_backup_directory(),
        summary,
    )
}

fn prepare_xray_edits(
    selected: &LoadedSave,
    money: Option<u32>,
    stacks: &BTreeMap<u16, u16>,
) -> Result<(SaveBuffer, EditSummary)> {
    let SaveData::Xray { save, inventory } = &selected.data else {
        return Err(Error::Refused("S2 inventory writes are not enabled".to_owned()));
    };
    let current_money = save.money()?;
    let mut changes = Vec::new();
    let money_change = money.filter(|value| *value != current_money);
    if let Some(new_value) = money_change {
        changes.push(writer::Change::SetMoney {
            target_object: save.actor_id(),
            old_value: current_money,
            new_value,
        });
    }
    let mut stack_count = 0_usize;
    for (handle, new_value) in stacks {
        let Some(item) = inventory.iter().find(|item| item.handle == *handle) else {
            continue;
        };
        let Some(old_value) = item.count else {
            continue;
        };
        if old_value != *new_value {
            changes.push(writer::Change::SetStack {
                target_object: *handle,
                old_value,
                new_value: *new_value,
            });
            stack_count = stack_count.saturating_add(1);
        }
    }
    if changes.is_empty() {
        return Err(Error::Refused("there are no inventory changes to export".to_owned()));
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

fn edited_output_path(source: &Path) -> PathBuf {
    let mut name = source
        .file_stem()
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from("save"));
    name.push("_edited");
    if let Some(extension) = source.extension() {
        name.push(".");
        name.push(extension);
    }
    source.parent().unwrap_or_else(|| Path::new(".")).join(name)
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
        "Состав рюкзака и подтверждённые изменения X-Ray"
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
        self.export = Some(style::button(
            cx.tree,
            inventory,
            "Экспортировать копию",
            Button::Primary,
        )?);
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
        if clicked == self.money_decrease {
            self.stage_money(false);
            return self.render(cx);
        }
        if clicked == self.money_increase {
            self.stage_money(true);
            return self.render(cx);
        }
        if clicked == self.previous {
            self.page = self.page.saturating_sub(1);
            return self.render(cx);
        }
        if clicked == self.next {
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
        if clicked == self.export {
            return self.export(cx);
        }
        if let Message::User(AppMessage::ToScreen(ScreenId::Inventory, payload)) = message {
            if let Some(ExportFinished(text)) = payload.downcast_ref::<ExportFinished>() {
                if let Some(id) = self.status {
                    cx.tree.set_text(id, text)?;
                }
                cx.status = Some(text.clone());
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

/// Confirmed stash information for the selected save.
struct Stashes {
    workspace: Workspace,
    text: Option<WidgetId>,
}

impl Stashes {
    fn new(workspace: Workspace) -> Self {
        Self { workspace, text: None }
    }

    fn render(&self, cx: &mut Context<'_>) -> Result<()> {
        let state = self.workspace.lock();
        let text = state
            .selected
            .as_ref()
            .map(|save| save.stashes.as_str())
            .unwrap_or("Сначала выберите сейв на экране «Обзор».");
        if let Some(id) = self.text {
            cx.tree.set_text(id, text)?;
        }
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
        self.render(cx)
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.render(cx)
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
    use super::{prepare_xray_edits, LoadedSave, S2Save, SaveBuffer, SaveSlot};
    use sse_core::Error;
    use sse_xray::Save;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::UNIX_EPOCH;

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
    fn s2_inventory_remains_read_only() -> sse_core::Result<()> {
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

        assert!(matches!(
            prepare_xray_edits(&loaded, Some(1_000), &BTreeMap::new()),
            Err(Error::Refused(_))
        ));
        Ok(())
    }
}
