//! S3 screens: verified backups, bounded save comparison, history, and read-only diagnostics.

use super::saves::Workspace;
use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::{Result, SaveBuffer};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::transaction::{self, BackupEntry, BackupStatus};
use std::path::{Path, PathBuf};

const MAXIMUM_VISIBLE_ENTRIES: usize = 12;
const MAXIMUM_COMPARE_RANGES: usize = MAXIMUM_VISIBLE_ENTRIES;

/// Screens owned by S3.
#[must_use]
pub fn screens() -> Vec<Box<dyn Screen>> {
    screens_with_workspace(Workspace::default())
}

pub(crate) fn screens_with_workspace(workspace: Workspace) -> Vec<Box<dyn Screen>> {
    [
        (ScreenId::Backups, "Резервные копии и восстановление"),
        (ScreenId::Compare, "Сравнение двух сохранений"),
        (ScreenId::Timeline, "История экспортов и резервных копий"),
        (ScreenId::SaveDoctor, "Проверка структуры и целостности"),
    ]
    .into_iter()
    .map(|(id, subtitle)| Box::new(HistoryScreen::new(id, subtitle, workspace.clone())) as Box<dyn Screen>)
    .collect()
}

#[derive(Clone, Debug)]
enum Action {
    Restore { journal: PathBuf, source: PathBuf },
    Compare(PathBuf),
    Diagnose { path: PathBuf, format_id: Option<String> },
}

#[derive(Clone, Debug)]
struct ActionButton {
    widget: WidgetId,
    action: Action,
}

#[derive(Clone, Copy, Debug)]
struct ResultRow {
    row: WidgetId,
    label: WidgetId,
    button: WidgetId,
}

#[derive(Debug)]
enum HistoryResult {
    Backups(std::result::Result<Vec<BackupEntry>, String>),
    Saves(std::result::Result<Vec<SaveSlot>, String>),
    Compare(std::result::Result<CompareReport, String>),
    Diagnosis(std::result::Result<String, String>),
    Restored(std::result::Result<PathBuf, String>),
}

#[derive(Debug)]
struct CompareReport {
    first: PathBuf,
    second: PathBuf,
    old_size: usize,
    new_size: usize,
    ranges: Vec<crate::diff::ByteRange>,
    truncated: bool,
}

/// One S3 screen instance.
pub struct HistoryScreen {
    id: ScreenId,
    subtitle: &'static str,
    workspace: Workspace,
    results: Option<WidgetId>,
    refresh: Option<WidgetId>,
    previous_page: Option<WidgetId>,
    next_page: Option<WidgetId>,
    summary: Option<WidgetId>,
    rows: Vec<ResultRow>,
    actions: Vec<ActionButton>,
    backup_entries: Option<Vec<BackupEntry>>,
    save_entries: Option<Vec<SaveSlot>>,
    page: usize,
    compare_selection: Vec<PathBuf>,
}

impl HistoryScreen {
    fn new(id: ScreenId, subtitle: &'static str, workspace: Workspace) -> Self {
        Self {
            id,
            subtitle,
            workspace,
            results: None,
            refresh: None,
            previous_page: None,
            next_page: None,
            summary: None,
            rows: Vec::new(),
            actions: Vec::new(),
            backup_entries: None,
            save_entries: None,
            page: 0,
            compare_selection: Vec::new(),
        }
    }

    fn request_refresh(&self, proxy: Option<crate::event_loop::Proxy<AppMessage>>) {
        let Some(proxy) = proxy else {
            return;
        };
        let id = self.id;
        self.workspace.spawn("history-refresh", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = match id {
                ScreenId::Backups | ScreenId::Timeline => HistoryResult::Backups(
                    transaction::list_backups(&default_backup_directory()).map_err(|error| error.to_string()),
                ),
                ScreenId::Compare | ScreenId::SaveDoctor => {
                    HistoryResult::Saves(discover_saves().map_err(|error| error.to_string()))
                }
                _ => return,
            };
            proxy.send(AppMessage::ToScreen(id, Box::new(result)));
        });
    }

    fn start_compare(&self, first: PathBuf, second: PathBuf, proxy: Option<crate::event_loop::Proxy<AppMessage>>) {
        let Some(proxy) = proxy else {
            return;
        };
        let id = self.id;
        self.workspace.spawn("save-compare", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = compare_saves(&first, &second);
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Compare(result))));
        });
    }

    fn start_diagnosis(
        &self,
        path: PathBuf,
        format_id: Option<String>,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) {
        let Some(proxy) = proxy else {
            return;
        };
        let id = self.id;
        self.workspace.spawn("save-diagnosis", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = diagnose_save(&path, format_id.as_deref());
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Diagnosis(result))));
        });
    }

    fn start_restore(&self, journal: PathBuf, source: PathBuf, proxy: Option<crate::event_loop::Proxy<AppMessage>>) {
        let Some(proxy) = proxy else {
            return;
        };
        let id = self.id;
        self.workspace.spawn("save-restore", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = restore_to_new_path(&journal, &source);
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Restored(result))));
        });
    }

    fn set_summary(&self, tree: &mut crate::widget::Tree, value: &str) -> Result<()> {
        if let Some(id) = self.summary {
            tree.set_text(id, value)?;
        }
        Ok(())
    }

    fn clear_results(&mut self, tree: &mut crate::widget::Tree) -> Result<()> {
        self.actions.clear();
        for row in &self.rows {
            tree.set_visible(row.row, false)?;
        }
        Ok(())
    }

    fn render_backups(&mut self, cx: &mut Context<'_>, entries: Vec<BackupEntry>) -> Result<()> {
        self.backup_entries = Some(entries);
        self.page = 0;
        self.render_backups_page(cx)
    }

    fn render_backups_page(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (visible, total, verified, pages) = match self.backup_entries.as_ref() {
            Some(entries) => {
                let total = entries.len();
                let verified = entries
                    .iter()
                    .filter(|entry| entry.status == BackupStatus::Verified)
                    .count();
                let pages = page_count(total);
                let start = self.page.saturating_mul(MAXIMUM_VISIBLE_ENTRIES);
                let visible = entries
                    .iter()
                    .skip(start)
                    .take(MAXIMUM_VISIBLE_ENTRIES)
                    .cloned()
                    .collect::<Vec<_>>();
                (visible, total, verified, pages)
            }
            None => return Ok(()),
        };
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        self.set_summary(
            cx.tree,
            &format!(
                "Записей: {total} · проверено: {verified} · страница {} из {pages}",
                self.page.saturating_add(1)
            ),
        )?;
        for (row_index, entry) in visible.into_iter().enumerate() {
            let file = display_name(&entry.source_path);
            let status = match entry.status {
                BackupStatus::Verified => "проверен",
                BackupStatus::Missing => "файл отсутствует",
                BackupStatus::Corrupt => "ошибка проверки",
            };
            let Some(slot) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(slot.row, true)?;
            cx.tree.set_text(slot.label, &format!("{file} · {status}"))?;
            cx.tree.set_visible(slot.button, false)?;
            if entry.status == BackupStatus::Verified && self.id == ScreenId::Backups {
                cx.tree.set_visible(slot.button, true)?;
                cx.tree.set_text(slot.button, "Восстановить копию…")?;
                self.actions.push(ActionButton {
                    widget: slot.button,
                    action: Action::Restore {
                        journal: entry.journal_path,
                        source: entry.source_path,
                    },
                });
            }
            if let Some(error) = entry.error {
                cx.tree
                    .set_text(slot.label, &format!("{file} · {status}: {}", truncate(&error, 80)))?;
            }
        }
        Ok(())
    }

    fn render_saves(&mut self, cx: &mut Context<'_>, slots: Vec<SaveSlot>) -> Result<()> {
        self.save_entries = Some(slots);
        self.page = 0;
        self.compare_selection.clear();
        self.render_saves_page(cx)
    }

    fn render_saves_page(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (visible, total, pages) = match self.save_entries.as_ref() {
            Some(entries) => {
                let total = entries.len();
                let pages = page_count(total);
                let start = self.page.saturating_mul(MAXIMUM_VISIBLE_ENTRIES);
                let visible = entries
                    .iter()
                    .skip(start)
                    .take(MAXIMUM_VISIBLE_ENTRIES)
                    .cloned()
                    .collect::<Vec<_>>();
                (visible, total, pages)
            }
            None => return Ok(()),
        };
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        self.set_summary(
            cx.tree,
            &format!(
                "Найдено файлов: {total} · страница {} из {pages}",
                self.page.saturating_add(1),
            ),
        )?;
        for (row_index, save_slot) in visible.into_iter().enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, true)?;
            let filename = display_name(&save_slot.path);
            let format = save_slot.format_id.as_deref().unwrap_or("не распознано");
            cx.tree.set_text(row.label, &format!("{filename} · {format}"))?;
            let button_text = if self.id == ScreenId::Compare {
                "Выбрать"
            } else {
                "Проверить"
            };
            if self.id == ScreenId::SaveDoctor && save_slot.format_id.is_none() {
                cx.tree.set_visible(row.button, false)?;
            } else {
                cx.tree.set_visible(row.button, true)?;
                cx.tree.set_text(row.button, button_text)?;
                self.actions.push(ActionButton {
                    widget: row.button,
                    action: if self.id == ScreenId::Compare {
                        Action::Compare(save_slot.path)
                    } else {
                        Action::Diagnose {
                            path: save_slot.path,
                            format_id: save_slot.format_id,
                        }
                    },
                });
            }
        }
        Ok(())
    }

    fn update_page_controls(&self, tree: &mut crate::widget::Tree, pages: usize) -> Result<()> {
        let multi_page = pages > 1;
        if let Some(previous) = self.previous_page {
            tree.set_visible(previous, multi_page && self.page > 0)?;
        }
        if let Some(next) = self.next_page {
            tree.set_visible(next, multi_page && self.page.saturating_add(1) < pages)?;
        }
        Ok(())
    }

    fn render_current_page(&mut self, cx: &mut Context<'_>) -> Result<()> {
        match self.id {
            ScreenId::Backups | ScreenId::Timeline => self.render_backups_page(cx),
            ScreenId::Compare | ScreenId::SaveDoctor => self.render_saves_page(cx),
            _ => Ok(()),
        }
    }

    fn render_compare(&mut self, cx: &mut Context<'_>, report: CompareReport) -> Result<()> {
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, 0)?;
        let count = report.ranges.len();
        self.set_summary(
            cx.tree,
            &format!(
                "{} → {} · packed {} → {} байт · диапазонов: {}{}",
                display_name(&report.first),
                display_name(&report.second),
                report.old_size,
                report.new_size,
                count,
                if report.truncated {
                    " (показан лимит)"
                } else {
                    ""
                }
            ),
        )?;
        if count == 0 {
            if let Some(row) = self.rows.first().copied() {
                cx.tree.set_visible(row.row, true)?;
                cx.tree.set_visible(row.button, false)?;
                cx.tree.set_text(row.label, "Байты совпадают.")?;
            }
        }
        for (row_index, range) in report.ranges.into_iter().enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, false)?;
            cx.tree.set_text(
                row.label,
                &format!(
                    "A {:08X}..{:08X} → B {:08X}..{:08X}",
                    range.old_start, range.old_end, range.new_start, range.new_end
                ),
            )?;
        }
        Ok(())
    }

    fn render_diagnosis(&self, cx: &mut Context<'_>, text: &str) -> Result<()> {
        self.set_summary(cx.tree, text)?;
        Ok(())
    }
}

impl Screen for HistoryScreen {
    fn id(&self) -> ScreenId {
        self.id
    }

    fn subtitle(&self) -> &str {
        self.subtitle
    }

    fn shown(&mut self, cx: &mut Context<'_>) -> Result<()> {
        self.workspace.poll_tasks();
        if self.id == ScreenId::Compare {
            let selected = cx.app.current_save().map(Path::to_path_buf);
            if self.compare_selection.first().cloned() != selected {
                self.compare_selection = selected.into_iter().collect();
            }
        }
        Ok(())
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        let title = match self.id {
            ScreenId::Backups => "БЭКАПЫ И ВОССТАНОВЛЕНИЕ",
            ScreenId::Compare => "СРАВНЕНИЕ СОХРАНЕНИЙ",
            ScreenId::Timeline => "ИСТОРИЯ ЭКСПОРТОВ",
            ScreenId::SaveDoctor => "ДИАГНОСТИКА СОХРАНЕНИЯ",
            _ => "ИСТОРИЯ",
        };
        style::label(cx.tree, card, title, Text::Heading)?;
        let row = style::row(cx.tree, card)?;
        let action = match self.id {
            ScreenId::Compare => "Найти сейвы",
            ScreenId::SaveDoctor => "Выбрать сейв",
            _ => "Обновить",
        };
        self.refresh = Some(style::button(cx.tree, row, action, Button::Primary)?);
        self.previous_page = Some(style::button(cx.tree, row, "Назад", Button::Secondary)?);
        self.next_page = Some(style::button(cx.tree, row, "Дальше", Button::Secondary)?);
        if let Some(previous) = self.previous_page {
            cx.tree.set_visible(previous, false)?;
        }
        if let Some(next) = self.next_page {
            cx.tree.set_visible(next, false)?;
        }
        self.summary = Some(style::label(
            cx.tree,
            row,
            "Нажмите кнопку, чтобы прочитать локальные данные.",
            Text::Note,
        )?);
        self.results = Some(style::card(cx.tree, host)?);
        let results = self.results.unwrap_or(card);
        style::label(
            cx.tree,
            results,
            match self.id {
                ScreenId::Backups => {
                    "Восстановление создаёт новый файл рядом с исходным и никогда его не перезаписывает."
                }
                ScreenId::Compare => "Сравниваются упакованные байты; показываются первые 12 диапазонов.",
                ScreenId::Timeline => "Журнал хранит путь, статус резервной копии и результат предыдущего экспорта.",
                ScreenId::SaveDoctor => "Проверка только читает сейв. Автоматического ремонта нет.",
                _ => "",
            },
            Text::Note,
        )?;
        for _ in 0..MAXIMUM_VISIBLE_ENTRIES {
            let row = style::row(cx.tree, results)?;
            let label = style::label(cx.tree, row, "", Text::Body)?;
            let button = style::button(cx.tree, row, "Действие", Button::Secondary)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(ResultRow { row, label, button });
        }
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        self.workspace.poll_tasks();
        if clicked == self.refresh {
            if cx.proxy.is_none() {
                self.set_summary(cx.tree, "В режиме headless screenshot диски не сканируются.")?;
            } else {
                self.set_summary(cx.tree, "Загрузка…")?;
                self.request_refresh(cx.proxy.cloned());
            }
        }
        if clicked == self.previous_page && self.page > 0 {
            self.page = self.page.saturating_sub(1);
            self.render_current_page(cx)?;
        }
        if clicked == self.next_page {
            let total_pages = self.backup_entries.as_ref().map_or_else(
                || {
                    self.save_entries
                        .as_ref()
                        .map_or(0, |entries| page_count(entries.len()))
                },
                |entries| page_count(entries.len()),
            );
            if self.page.saturating_add(1) < total_pages {
                self.page = self.page.saturating_add(1);
                self.render_current_page(cx)?;
            }
        }
        if let Some(action) = self
            .actions
            .iter()
            .find(|action| Some(action.widget) == clicked)
            .map(|value| value.action.clone())
        {
            match action {
                Action::Restore { journal, source } => {
                    self.set_summary(cx.tree, "Проверяю журнал и создаю отдельный файл…")?;
                    self.start_restore(journal, source, cx.proxy.cloned());
                }
                Action::Compare(path) => {
                    if self.compare_selection.len() >= 2 {
                        self.compare_selection.clear();
                    }
                    if self.compare_selection.last() == Some(&path) {
                        self.compare_selection.clear();
                        self.compare_selection.push(path);
                    } else {
                        self.compare_selection.push(path);
                    }
                    if self.compare_selection.len() == 2 {
                        let first = self.compare_selection.first().cloned();
                        let second = self.compare_selection.get(1).cloned();
                        if let (Some(first), Some(second)) = (first, second) {
                            self.set_summary(cx.tree, "Сравниваю…")?;
                            self.start_compare(first, second, cx.proxy.cloned());
                        }
                    } else {
                        self.set_summary(cx.tree, "Выберите второй сейв для сравнения.")?;
                    }
                }
                Action::Diagnose { path, format_id } => {
                    self.set_summary(cx.tree, &format!("Проверяю {}…", display_name(&path)))?;
                    self.start_diagnosis(path, format_id, cx.proxy.cloned());
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(target, payload)) = message {
            if *target == self.id {
                if let Some(payload) = payload.downcast_ref::<HistoryResult>() {
                    match payload {
                        HistoryResult::Backups(Ok(entries)) => self.render_backups(cx, entries.clone())?,
                        HistoryResult::Backups(Err(error)) => {
                            self.set_summary(cx.tree, &format!("Ошибка: {}", truncate(error, 160)))?
                        }
                        HistoryResult::Saves(Ok(slots)) => self.render_saves(cx, slots.clone())?,
                        HistoryResult::Saves(Err(error)) => {
                            self.set_summary(cx.tree, &format!("Ошибка: {}", truncate(error, 160)))?
                        }
                        HistoryResult::Compare(Ok(report)) => self.render_compare(cx, clone_report(report))?,
                        HistoryResult::Compare(Err(error)) => {
                            self.set_summary(cx.tree, &format!("Ошибка сравнения: {}", truncate(error, 160)))?
                        }
                        HistoryResult::Diagnosis(Ok(text)) => self.render_diagnosis(cx, text)?,
                        HistoryResult::Diagnosis(Err(error)) => {
                            self.set_summary(cx.tree, &format!("Ошибка чтения: {}", truncate(error, 160)))?
                        }
                        HistoryResult::Restored(Ok(path)) => {
                            self.set_summary(cx.tree, &format!("Копия восстановлена в {}", path.display()))?
                        }
                        HistoryResult::Restored(Err(error)) => {
                            self.set_summary(cx.tree, &format!("Не восстановлено: {}", truncate(error, 160)))?
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

fn clone_report(report: &CompareReport) -> CompareReport {
    CompareReport {
        first: report.first.clone(),
        second: report.second.clone(),
        old_size: report.old_size,
        new_size: report.new_size,
        ranges: report.ranges.clone(),
        truncated: report.truncated,
    }
}

fn discover_saves() -> sse_core::Result<Vec<SaveSlot>> {
    let candidates = SaveDirectoryLocator::find_candidate_directories(None);
    Ok(SaveSlotDiscovery::discover(&candidates).slots)
}

fn compare_saves(first: &Path, second: &Path) -> std::result::Result<CompareReport, String> {
    let first_image = SaveBuffer::read(first).map_err(|error| error.to_string())?;
    let second_image = SaveBuffer::read(second).map_err(|error| error.to_string())?;
    let bounded = crate::diff::byte_ranges_bounded(
        first_image.as_slice(),
        second_image.as_slice(),
        8,
        MAXIMUM_COMPARE_RANGES,
    );
    Ok(CompareReport {
        first: first.to_path_buf(),
        second: second.to_path_buf(),
        old_size: first_image.len(),
        new_size: second_image.len(),
        ranges: bounded.ranges,
        truncated: bounded.truncated,
    })
}

fn diagnose_save(path: &Path, format_id: Option<&str>) -> std::result::Result<String, String> {
    let packed = SaveBuffer::read(path).map_err(|error| error.to_string())?;
    diagnose_packed(packed.as_slice(), format_id)
}

fn diagnose_packed(packed: &[u8], format_id: Option<&str>) -> std::result::Result<String, String> {
    if format_id == Some("stalker2") {
        let save = sse_s2::S2Save::from_bytes(packed).map_err(|error| error.to_string())?;
        let container = save.container();
        let items = save.items();
        return Ok(format!(
            "S2: {} байт · CRC {:08X}/{:08X} · деньги {} · предметов {} · тайник {} · предупреждений {} · неразрешённых ссылок {}. Только диагностика; запись S2 не поддержана.",
            packed.len(),
            container.stored_crc32(),
            container.computed_crc32(),
            save.money(),
            items.len(),
            if save.stash().is_ok() { "найден" } else { "не найден" },
            save.warnings().len(),
            save.unresolved_handles().len(),
        ));
    }
    if format_id.is_some_and(|id| id.starts_with("stalker-")) {
        let save = sse_xray::Save::read(packed).map_err(|error| error.to_string())?;
        let inventory = save.inventory().map_err(|error| error.to_string())?;
        return Ok(format!(
            "X-Ray {}: упакованный файл {} байт · реестр {} · предметов инвентаря {} · деньги {} · игровой тик {}. Структура прочитана; ремонт не выполнялся.",
            save.format().id(), packed.len(), save.registry_objects().len(), inventory.len(), save.money().map_err(|error| error.to_string())?, save.game_time(),
        ));
    }
    Err("формат не распознан; структурная проверка не запускалась".to_owned())
}

fn restore_to_new_path(journal: &Path, source: &Path) -> std::result::Result<PathBuf, String> {
    let output = restored_output_path(source);
    transaction::restore_backup(journal, &output).map_err(|error| error.to_string())
}

fn restored_output_path(source: &Path) -> PathBuf {
    let name = source.file_stem().unwrap_or_default().to_string_lossy();
    let extension = source.extension().and_then(|value| value.to_str()).unwrap_or("sav");
    source.with_file_name(format!("{name}_restored.{extension}"))
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

fn page_count(total: usize) -> usize {
    total.saturating_add(MAXIMUM_VISIBLE_ENTRIES.saturating_sub(1)) / MAXIMUM_VISIBLE_ENTRIES
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn truncate(value: &str, maximum: usize) -> String {
    if value.chars().count() <= maximum {
        return value.to_owned();
    }
    let mut result = value.chars().take(maximum.saturating_sub(1)).collect::<String>();
    result.push('…');
    result
}

#[cfg(test)]
mod tests {
    use super::{default_backup_directory, diagnose_packed, page_count, restored_output_path, truncate};
    use std::path::Path;

    const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../../fixtures/synthetic/xray-soc-ee.sav");
    const SYNTHETIC_S2_SAVE: &[u8] =
        include_bytes!("../../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");

    #[test]
    fn diagnostic_text_truncation_obeys_character_boundary() {
        assert_eq!(truncate("абвг", 3), "аб…");
        assert_eq!(truncate("ok", 3), "ok");
    }

    #[test]
    fn backup_destination_is_a_new_sibling_and_preserves_extension() {
        let source = Path::new("/save/game_slot.sav");
        assert_eq!(restored_output_path(source), Path::new("/save/game_slot_restored.sav"));
    }

    #[test]
    fn default_backup_directory_ends_in_application_backup_location() {
        assert!(default_backup_directory().ends_with("StalkerSaveEditor/backups"));
    }

    #[test]
    fn list_pages_cover_all_entries_without_an_empty_tail_page() {
        assert_eq!(page_count(0), 0);
        assert_eq!(page_count(12), 1);
        assert_eq!(page_count(13), 2);
        assert_eq!(page_count(24), 2);
    }

    #[test]
    fn doctor_identifies_synthetic_xray_save() {
        let report = diagnose_packed(SYNTHETIC_XRAY_SAVE, Some("stalker-soc-ee")).unwrap_or_else(|error| error);
        assert!(report.starts_with("X-Ray stalker-soc-ee:"));
        assert!(report.contains("Структура прочитана"));
    }

    #[test]
    fn doctor_refuses_to_guess_a_save_format() {
        assert!(diagnose_packed(SYNTHETIC_XRAY_SAVE, None).is_err());
    }

    #[test]
    fn doctor_identifies_synthetic_s2_save_and_reports_crc() {
        let report = diagnose_packed(SYNTHETIC_S2_SAVE, Some("stalker2")).unwrap_or_else(|error| error);
        assert!(report.starts_with("S2:"));
        assert!(report.contains("CRC"));
        assert!(report.contains("запись S2 не поддержана"));
    }
}
