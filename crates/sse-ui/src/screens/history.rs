//! S3 screens: verified backups, bounded save comparison, history, and read-only diagnostics.

use super::saves::Workspace;
use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::event_loop::Message;
use crate::widget::WidgetId;
use sse_core::{Result, SaveBuffer};
use sse_storage::discovery::{SaveDirectoryLocator, SaveSlot, SaveSlotDiscovery};
use sse_storage::transaction::{self, BackupEntry, BackupStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAXIMUM_VISIBLE_ENTRIES: usize = 12;

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
    RestoreInPlace { journal: PathBuf, source: PathBuf },
    Compare(PathBuf),
    Diagnose { path: PathBuf, format_id: Option<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreMode {
    Copy,
    InPlace,
}

#[derive(Debug)]
enum RestoredSave {
    Copy(PathBuf),
    InPlace(transaction::RestoreReceipt),
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
    secondary_button: WidgetId,
}

#[derive(Debug)]
enum HistoryResult {
    Backups(std::result::Result<Vec<BackupEntry>, String>),
    Saves(std::result::Result<Vec<SaveSlot>, String>),
    Compare(std::result::Result<CompareReport, String>),
    Diagnosis(std::result::Result<String, String>),
    Restored(std::result::Result<RestoredSave, String>),
}

#[derive(Debug)]
struct CompareReport {
    first: PathBuf,
    second: PathBuf,
    differences: Vec<SemanticDifference>,
    added: usize,
    removed: usize,
    changed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticDifference {
    kind: DifferenceKind,
    label: String,
    value_a: String,
    value_b: String,
    category: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DifferenceKind {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ItemQuantity {
    label: String,
    count: u64,
    all_counts_known: bool,
    object_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticSnapshot {
    game: String,
    money: u32,
    items: BTreeMap<String, ItemQuantity>,
}

/// One S3 screen instance.
pub struct HistoryScreen {
    id: ScreenId,
    subtitle: &'static str,
    workspace: Workspace,
    results: Option<WidgetId>,
    refresh: Option<WidgetId>,
    restore_confirmation: Option<WidgetId>,
    restore_description: Option<WidgetId>,
    confirm_restore: Option<WidgetId>,
    cancel_restore: Option<WidgetId>,
    previous_page: Option<WidgetId>,
    next_page: Option<WidgetId>,
    summary: Option<WidgetId>,
    rows: Vec<ResultRow>,
    actions: Vec<ActionButton>,
    backup_entries: Option<Vec<BackupEntry>>,
    save_entries: Option<Vec<SaveSlot>>,
    page: usize,
    compare_selection: Vec<PathBuf>,
    pending_restore: Option<(PathBuf, PathBuf, RestoreMode)>,
}

impl HistoryScreen {
    fn new(id: ScreenId, subtitle: &'static str, workspace: Workspace) -> Self {
        Self {
            id,
            subtitle,
            workspace,
            results: None,
            refresh: None,
            restore_confirmation: None,
            restore_description: None,
            confirm_restore: None,
            cancel_restore: None,
            previous_page: None,
            next_page: None,
            summary: None,
            rows: Vec::new(),
            actions: Vec::new(),
            backup_entries: None,
            save_entries: None,
            page: 0,
            compare_selection: Vec::new(),
            pending_restore: None,
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
                ScreenId::Backups => HistoryResult::Backups(
                    transaction::list_backups(&default_backup_directory()).map_err(|error| error.to_string()),
                ),
                ScreenId::Compare | ScreenId::Timeline | ScreenId::SaveDoctor => {
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

    fn start_restore(
        &self,
        journal: PathBuf,
        source: PathBuf,
        mode: RestoreMode,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) {
        let Some(proxy) = proxy else {
            return;
        };
        let id = self.id;
        self.workspace.spawn("save-restore", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = match mode {
                RestoreMode::Copy => restore_to_new_path(&journal, &source).map(RestoredSave::Copy),
                RestoreMode::InPlace => transaction::restore_in_place(&journal)
                    .map(RestoredSave::InPlace)
                    .map_err(|error| error.to_string()),
            };
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Restored(result))));
        });
    }

    fn open_restore_confirmation(
        &mut self,
        cx: &mut Context<'_>,
        journal: PathBuf,
        source: PathBuf,
        mode: RestoreMode,
    ) -> Result<()> {
        self.pending_restore = Some((journal, source, mode));
        if let (Some(dialog), Some(description)) = (self.restore_confirmation, self.restore_description) {
            cx.tree.set_text(
                description,
                match mode {
                    RestoreMode::Copy => {
                        "Создать отдельный файл из проверенной копии? Исходный сейв останется без изменений."
                    }
                    RestoreMode::InPlace => {
                        "Заменить исходный сейв? Текущий файл сверяется с журналом, перед записью создаётся страховочный бэкап."
                    }
                },
            )?;
            cx.tree.open_dialog(dialog)?;
        }
        self.set_summary(cx.tree, "Подтвердите восстановление.")?;
        Ok(())
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
            cx.tree.set_visible(slot.secondary_button, false)?;
            if entry.status == BackupStatus::Verified && self.id == ScreenId::Backups {
                cx.tree.set_visible(slot.button, true)?;
                cx.tree.set_text(slot.button, "В копию…")?;
                self.actions.push(ActionButton {
                    widget: slot.button,
                    action: Action::Restore {
                        journal: entry.journal_path.clone(),
                        source: entry.source_path.clone(),
                    },
                });
                cx.tree.set_visible(slot.secondary_button, true)?;
                cx.tree.set_text(slot.secondary_button, "На место…")?;
                self.actions.push(ActionButton {
                    widget: slot.secondary_button,
                    action: Action::RestoreInPlace {
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
        if self.id == ScreenId::Timeline {
            self.render_timeline_page(cx)
        } else {
            if self.id == ScreenId::Compare {
                self.compare_selection.truncate(1);
            }
            self.render_saves_page(cx)
        }
    }

    fn render_saves_page(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let (visible, total, pages) = match self.save_entries.as_ref() {
            Some(entries) => {
                let candidates = if self.id == ScreenId::Compare {
                    match self.compare_selection.first() {
                        Some(selected_path) => {
                            let current_family = entries
                                .iter()
                                .find(|entry| &entry.path == selected_path)
                                .map(game_family);
                            entries
                                .iter()
                                .filter(|entry| &entry.path != selected_path)
                                .filter(|entry| {
                                    current_family
                                        .as_deref()
                                        .is_some_and(|family| game_family(entry) == family)
                                })
                                .cloned()
                                .collect::<Vec<_>>()
                        }
                        None => entries.clone(),
                    }
                } else {
                    entries.clone()
                };
                let total = candidates.len();
                let pages = page_count(total);
                let start = self.page.saturating_mul(MAXIMUM_VISIBLE_ENTRIES);
                let visible = candidates
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
        if self.id == ScreenId::Compare && total == 0 {
            self.set_summary(cx.tree, "Нет других сейвов этой игры для сравнения.")?;
        } else {
            self.set_summary(
                cx.tree,
                &format!(
                    "Найдено файлов: {total} · страница {} из {pages}",
                    self.page.saturating_add(1),
                ),
            )?;
        }
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
            ScreenId::Backups => self.render_backups_page(cx),
            ScreenId::Timeline => self.render_timeline_page(cx),
            ScreenId::Compare | ScreenId::SaveDoctor => self.render_saves_page(cx),
            _ => Ok(()),
        }
    }

    fn render_timeline_page(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let Some(entries) = self.save_entries.as_ref() else {
            return Ok(());
        };
        let mut ordered = entries.clone();
        ordered.sort_by(timeline_order);
        let total = ordered.len();
        let pages = page_count(total);
        let start = self.page.saturating_mul(MAXIMUM_VISIBLE_ENTRIES);
        let visible = ordered
            .iter()
            .skip(start)
            .take(MAXIMUM_VISIBLE_ENTRIES)
            .collect::<Vec<_>>();
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        self.set_summary(
            cx.tree,
            &format!(
                "Сохранений: {total} · порядок: игра, от старых к новым · страница {} из {pages}",
                self.page.saturating_add(1)
            ),
        )?;
        for (row_index, save) in visible.into_iter().enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, false)?;
            let game = save.game_id.as_deref().unwrap_or(&save.candidate_game_id);
            cx.tree.set_text(
                row.label,
                &format!(
                    "{game} · {} · {} · {} байт",
                    display_name(&save.path),
                    format_system_time(save.last_write_time_utc),
                    save.size
                ),
            )?;
        }
        Ok(())
    }

    fn render_compare(&mut self, cx: &mut Context<'_>, report: CompareReport) -> Result<()> {
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, 0)?;
        let count = report.differences.len();
        self.set_summary(
            cx.tree,
            &format!(
                "Различий: {count} · добавлено: {} · удалено: {} · изменено: {} · {} → {}",
                report.added,
                report.removed,
                report.changed,
                display_name(&report.first),
                display_name(&report.second)
            ),
        )?;
        if count == 0 {
            if let Some(row) = self.rows.first().copied() {
                cx.tree.set_visible(row.row, true)?;
                cx.tree.set_visible(row.button, false)?;
                cx.tree.set_text(row.label, "Различий в деньгах и предметах нет.")?;
            }
        }
        for (row_index, difference) in report.differences.iter().take(MAXIMUM_VISIBLE_ENTRIES).enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, false)?;
            cx.tree.set_text(
                row.label,
                &format!(
                    "{} · {} · {} → {} · {}",
                    difference_kind_text(difference.kind),
                    difference.label,
                    difference.value_a,
                    difference.value_b,
                    difference.category
                ),
            )?;
        }
        if count > MAXIMUM_VISIBLE_ENTRIES {
            self.set_summary(
                cx.tree,
                &format!(
                    "Различий: {count} · добавлено: {} · удалено: {} · изменено: {} · показаны первые {}",
                    report.added, report.removed, report.changed, MAXIMUM_VISIBLE_ENTRIES
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
            ScreenId::Timeline => "ИСТОРИЯ СОХРАНЕНИЙ",
            ScreenId::SaveDoctor => "ДИАГНОСТИКА СОХРАНЕНИЯ",
            _ => "ИСТОРИЯ",
        };
        style::label(cx.tree, card, title, Text::Heading)?;
        let row = style::row(cx.tree, card)?;
        let action = match self.id {
            ScreenId::Compare => "Найти сейвы",
            ScreenId::SaveDoctor => "Выбрать сейв",
            ScreenId::Timeline => "Обновить историю",
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
                ScreenId::Backups => "Журнал резервных копий сверяется с файлами и SHA-256; восстановление идёт в отдельный файл после подтверждения.",
                ScreenId::Compare => "Показываются только различия в читаемых значениях денег и предметов.",
                ScreenId::Timeline => "Временная последовательность строится по времени изменения файлов сейвов.",
                ScreenId::SaveDoctor => "Проверка только читает сейв. Автоматического ремонта нет.",
                _ => "",
            },
            Text::Note,
        )?;
        for _ in 0..MAXIMUM_VISIBLE_ENTRIES {
            let row = style::row(cx.tree, results)?;
            let label = style::label(cx.tree, row, "", Text::Body)?;
            let button = style::button(cx.tree, row, "Действие", Button::Secondary)?;
            let secondary_button = style::button(cx.tree, row, "Восстановить", Button::Danger)?;
            cx.tree.set_visible(row, false)?;
            self.rows.push(ResultRow {
                row,
                label,
                button,
                secondary_button,
            });
        }
        let confirmation_host = cx.tree.overlay_host().unwrap_or(host);
        let confirmation = style::card(cx.tree, confirmation_host)?;
        self.restore_confirmation = Some(confirmation);
        style::label(cx.tree, confirmation, "ВОССТАНОВИТЬ РЕЗЕРВНУЮ КОПИЮ", Text::Heading)?;
        self.restore_description = Some(style::label(
            cx.tree,
            confirmation,
            "Подтвердите создание отдельного файла из проверенной копии. Исходный сейв останется без изменений.",
            Text::Body,
        )?);
        let confirm_row = style::row(cx.tree, confirmation)?;
        self.confirm_restore = Some(style::button(
            cx.tree,
            confirm_row,
            "Подтвердить восстановление",
            Button::Primary,
        )?);
        self.cancel_restore = Some(style::button(cx.tree, confirm_row, "Отмена", Button::Secondary)?);
        cx.tree.set_visible(confirmation, false)?;
        Ok(())
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        self.workspace.poll_tasks();
        if self.pending_restore.is_some()
            && matches!(
                message,
                Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym: 0xff1b,
                    ..
                })
            )
        {
            self.pending_restore = None;
            self.set_summary(cx.tree, "Восстановление отменено.")?;
        }
        if clicked.is_some() && clicked == self.refresh {
            if cx.proxy.is_none() {
                self.set_summary(cx.tree, "В режиме headless screenshot диски не сканируются.")?;
            } else {
                self.set_summary(cx.tree, "Загрузка…")?;
                self.request_refresh(cx.proxy.cloned());
            }
        }
        if clicked.is_some() && clicked == self.previous_page && self.page > 0 {
            self.page = self.page.saturating_sub(1);
            self.render_current_page(cx)?;
        }
        if clicked.is_some() && clicked == self.next_page {
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
        if clicked.is_some() && clicked == self.cancel_restore {
            self.pending_restore = None;
            let _ = cx.tree.close_dialog()?;
            self.set_summary(cx.tree, "Восстановление отменено.")?;
        }
        if clicked.is_some() && clicked == self.confirm_restore {
            if let Some((journal, source, mode)) = self.pending_restore.take() {
                let _ = cx.tree.close_dialog()?;
                let status = match mode {
                    RestoreMode::Copy => "Проверяю журнал и восстанавливаю копию в новый файл…",
                    RestoreMode::InPlace => "Проверяю журнал и восстанавливаю сейв на место…",
                };
                self.set_summary(cx.tree, status)?;
                self.start_restore(journal, source, mode, cx.proxy.cloned());
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
                    self.open_restore_confirmation(cx, journal, source, RestoreMode::Copy)?;
                }
                Action::RestoreInPlace { journal, source } => {
                    if has_pending_edits_for_selected_source(cx.app, &source) {
                        self.set_summary(cx.tree, "Сначала сохраните или сбросьте черновик выбранного сейва.")?;
                    } else {
                        self.open_restore_confirmation(cx, journal, source, RestoreMode::InPlace)?;
                    }
                }
                Action::Compare(path) => {
                    if let Some(current) = cx.app.current_save().map(Path::to_path_buf) {
                        if current == path {
                            self.set_summary(cx.tree, "Выберите другой сейв этой игры.")?;
                        } else {
                            self.compare_selection = vec![current.clone(), path.clone()];
                            self.set_summary(cx.tree, "Сравниваю…")?;
                            self.start_compare(current, path, cx.proxy.cloned());
                        }
                    } else {
                        match self.compare_selection.first().cloned() {
                            Some(first) if first == path => {
                                self.set_summary(cx.tree, "Выберите другой сейв этой игры.")?;
                            }
                            Some(first) => {
                                let entries = self.save_entries.as_deref().unwrap_or_default();
                                let first_family = entries.iter().find(|entry| entry.path == first).map(game_family);
                                let second_family = entries.iter().find(|entry| entry.path == path).map(game_family);
                                if first_family.is_some() && first_family == second_family {
                                    self.compare_selection = vec![first.clone(), path.clone()];
                                    self.set_summary(cx.tree, "Сравниваю…")?;
                                    self.start_compare(first, path, cx.proxy.cloned());
                                } else {
                                    self.set_summary(cx.tree, "Это сейвы разных игр.")?;
                                }
                            }
                            None => {
                                self.compare_selection.push(path);
                                self.page = 0;
                                self.render_saves_page(cx)?;
                                self.set_summary(cx.tree, "Выберите второй сейв этой игры.")?;
                            }
                        }
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
                if payload.is::<()>() {
                    self.request_refresh(cx.proxy.cloned());
                }
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
                        HistoryResult::Restored(Ok(RestoredSave::Copy(path))) => {
                            self.set_summary(cx.tree, &format!("Копия восстановлена в {}", path.display()))?;
                            self.request_refresh(cx.proxy.cloned());
                            if let Some(proxy) = cx.proxy.as_ref() {
                                let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
                            }
                        }
                        HistoryResult::Restored(Ok(RestoredSave::InPlace(receipt))) => {
                            let backup = receipt.safety_backup_path.as_ref().map_or_else(
                                || "без страховочного бэкапа (исходного файла не было)".to_owned(),
                                |path| format!("страховочный бэкап: {}", path.display()),
                            );
                            self.set_summary(
                                cx.tree,
                                &format!("Восстановлено на место: {} · {backup}", receipt.save_path.display()),
                            )?;
                            self.request_refresh(cx.proxy.cloned());
                            if let Some(proxy) = cx.proxy.as_ref() {
                                let _ = proxy.send(AppMessage::ToScreen(ScreenId::Overview, Box::new(())));
                            }
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
        differences: report.differences.clone(),
        added: report.added,
        removed: report.removed,
        changed: report.changed,
    }
}

fn discover_saves() -> sse_core::Result<Vec<SaveSlot>> {
    let candidates = SaveDirectoryLocator::find_candidate_directories(None);
    Ok(SaveSlotDiscovery::discover(&candidates).slots)
}

fn game_family(slot: &SaveSlot) -> String {
    let value = slot
        .game_id
        .as_deref()
        .unwrap_or(&slot.candidate_game_id)
        .to_ascii_lowercase();
    if value == "soc" || value.contains("soc") {
        "soc".to_owned()
    } else if value == "clear_sky" || value == "cs" || value.contains("clear-sky") || value.contains("clear_sky") {
        "clear_sky".to_owned()
    } else if value == "cop" || value.contains("cop") {
        "cop".to_owned()
    } else if value == "stalker2" || value == "stalker-2" || value == "s2" {
        "stalker2".to_owned()
    } else {
        value
    }
}

fn timeline_order(left: &SaveSlot, right: &SaveSlot) -> std::cmp::Ordering {
    game_family(left)
        .cmp(&game_family(right))
        .then_with(|| {
            match (
                left.last_write_time_utc == UNIX_EPOCH,
                right.last_write_time_utc == UNIX_EPOCH,
            ) {
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ => left.last_write_time_utc.cmp(&right.last_write_time_utc),
            }
        })
        .then_with(|| left.path.cmp(&right.path))
}

fn format_system_time(value: SystemTime) -> String {
    let Ok(duration) = value.duration_since(UNIX_EPOCH) else {
        return "дата неизвестна".to_owned();
    };
    let seconds = duration.as_secs();
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let Ok(days) = i64::try_from(days) else {
        return "дата вне диапазона".to_owned();
    };
    let Some(serial_day) = days.checked_add(719_468) else {
        return "дата вне диапазона".to_owned();
    };
    let era = if serial_day >= 0 {
        serial_day
    } else {
        serial_day.saturating_sub(146_096)
    }
    .div_euclid(146_097);
    let day_of_era = serial_day.saturating_sub(era.saturating_mul(146_097));
    let year_of_era = day_of_era
        .saturating_sub(day_of_era / 1_460)
        .saturating_add(day_of_era / 36_524)
        .saturating_sub(day_of_era / 146_096)
        .div_euclid(365);
    let mut year = year_of_era.saturating_add(era.saturating_mul(400));
    let day_of_year = day_of_era.saturating_sub(
        365_i64
            .saturating_mul(year_of_era)
            .saturating_add(year_of_era / 4)
            .saturating_sub(year_of_era / 100),
    );
    let month_prime = 5_i64.saturating_mul(day_of_year).saturating_add(2).div_euclid(153);
    let day = day_of_year
        .saturating_sub(153_i64.saturating_mul(month_prime).saturating_add(2).div_euclid(5))
        .saturating_add(1);
    let month = month_prime.saturating_add(if month_prime < 10 { 3 } else { -9 });
    if month <= 2 {
        year = year.saturating_add(1);
    }
    format!(
        "{day:02}.{month:02}.{year:04} {:02}:{:02}:{:02}",
        seconds_of_day / 3_600,
        seconds_of_day % 3_600 / 60,
        seconds_of_day % 60
    )
}

fn has_pending_edits_for_selected_source(app: &sse_app::AppState, source: &Path) -> bool {
    app.current_save() == Some(source)
        && (app.has_invalid_numeric_input()
            || app
                .current_save_sha256()
                .is_some_and(|source_sha256| app.has_draft(source_sha256)))
}

fn compare_saves(first: &Path, second: &Path) -> std::result::Result<CompareReport, String> {
    let first_image = SaveBuffer::read(first).map_err(|error| error.to_string())?;
    let second_image = SaveBuffer::read(second).map_err(|error| error.to_string())?;
    compare_packed(first, first_image.as_slice(), second, second_image.as_slice())
}

fn compare_packed(
    first_path: &Path,
    first: &[u8],
    second_path: &Path,
    second: &[u8],
) -> std::result::Result<CompareReport, String> {
    let first_snapshot = semantic_snapshot(first)?;
    let second_snapshot = semantic_snapshot(second)?;
    if first_snapshot.game != second_snapshot.game {
        return Err("Это сейвы разных игр.".to_owned());
    }

    let mut differences = Vec::new();
    if first_snapshot.money != second_snapshot.money {
        differences.push(SemanticDifference {
            kind: DifferenceKind::Changed,
            label: "Деньги".to_owned(),
            value_a: first_snapshot.money.to_string(),
            value_b: second_snapshot.money.to_string(),
            category: "Персонаж",
        });
    }

    let keys = first_snapshot
        .items
        .keys()
        .chain(second_snapshot.items.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for key in keys {
        let old = first_snapshot.items.get(&key);
        let new = second_snapshot.items.get(&key);
        let kind = match (old, new) {
            (None, Some(_)) => Some(DifferenceKind::Added),
            (Some(_), None) => Some(DifferenceKind::Removed),
            (Some(old), Some(new)) if item_quantity_changed(old, new) => Some(DifferenceKind::Changed),
            _ => None,
        };
        let Some(kind) = kind else {
            continue;
        };
        let label = new.or(old).map_or_else(|| key.clone(), |item| item.label.clone());
        differences.push(SemanticDifference {
            kind,
            label,
            value_a: old.map_or_else(|| "нет".to_owned(), item_quantity_text),
            value_b: new.map_or_else(|| "нет".to_owned(), item_quantity_text),
            category: "Предметы",
        });
    }

    let added = differences
        .iter()
        .filter(|difference| difference.kind == DifferenceKind::Added)
        .count();
    let removed = differences
        .iter()
        .filter(|difference| difference.kind == DifferenceKind::Removed)
        .count();
    let changed = differences
        .iter()
        .filter(|difference| difference.kind == DifferenceKind::Changed)
        .count();
    Ok(CompareReport {
        first: first_path.to_path_buf(),
        second: second_path.to_path_buf(),
        differences,
        added,
        removed,
        changed,
    })
}

fn semantic_snapshot(packed: &[u8]) -> std::result::Result<SemanticSnapshot, String> {
    if let Ok(save) = sse_s2::S2Save::from_bytes(packed) {
        let mut snapshot = SemanticSnapshot {
            game: "stalker2".to_owned(),
            money: save.money(),
            items: BTreeMap::new(),
        };
        for item in save.items() {
            let key = format!(
                "{:02x}{:02x}{:02x}",
                item.type_key[0], item.type_key[1], item.type_key[2]
            );
            let label = item.display_name.unwrap_or_else(|| format!("Предмет {}", key));
            add_item(&mut snapshot, key, label, Some(item.count));
        }
        return Ok(snapshot);
    }

    let save = sse_xray::Save::read(packed).map_err(|error| error.to_string())?;
    let game = match save.format() {
        sse_xray::Format::Soc | sse_xray::Format::SocEe => "soc",
        sse_xray::Format::Cs | sse_xray::Format::CsEe => "clear_sky",
        sse_xray::Format::Cop | sse_xray::Format::CopEe => "cop",
    };
    let mut snapshot = SemanticSnapshot {
        game: game.to_owned(),
        money: save.money().map_err(|error| error.to_string())?,
        items: BTreeMap::new(),
    };
    for item in save.inventory().map_err(|error| error.to_string())? {
        let key = format!(
            "{}\u{1f}{}",
            item.section.to_ascii_lowercase(),
            item.category.to_ascii_lowercase()
        );
        let label = format!("{} ({})", item.category, item.section);
        add_item(&mut snapshot, key, label, item.count.map(u32::from));
    }
    Ok(snapshot)
}

fn add_item(snapshot: &mut SemanticSnapshot, key: String, label: String, count: Option<u32>) {
    let item = snapshot.items.entry(key).or_insert(ItemQuantity {
        label,
        count: 0,
        all_counts_known: true,
        object_count: 0,
    });
    item.object_count = item.object_count.saturating_add(1);
    match count {
        Some(count) if item.all_counts_known => item.count = item.count.saturating_add(u64::from(count)),
        Some(_) => {}
        None => item.all_counts_known = false,
    }
}

fn item_quantity_changed(first: &ItemQuantity, second: &ItemQuantity) -> bool {
    match (first.all_counts_known, second.all_counts_known) {
        (true, true) => first.count != second.count,
        (false, false) => first.object_count != second.object_count,
        _ => true,
    }
}

fn item_quantity_text(item: &ItemQuantity) -> String {
    if item.all_counts_known {
        format!("×{}", item.count)
    } else {
        format!("{} объектов · количество неизвестно", item.object_count)
    }
}

fn difference_kind_text(kind: DifferenceKind) -> &'static str {
    match kind {
        DifferenceKind::Added => "Добавлено",
        DifferenceKind::Removed => "Удалено",
        DifferenceKind::Changed => "Изменено",
    }
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
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    restored_output_path_at(source, timestamp)
}

fn restored_output_path_at(source: &Path, timestamp: u64) -> PathBuf {
    let name = source.file_stem().unwrap_or_default().to_string_lossy();
    let extension = source.extension().and_then(|value| value.to_str()).unwrap_or("sav");
    source.with_file_name(format!("{name}_restored_{timestamp}.{extension}"))
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
    use super::{
        compare_packed, default_backup_directory, diagnose_packed, format_system_time, page_count,
        restored_output_path_at, timeline_order, truncate, Action, ActionButton, HistoryScreen, Workspace,
    };
    use crate::event_loop::Message;
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::{AppMessage, Context, Screen, ScreenId};
    use crate::widget::{Content, Look, Tree};
    use sse_storage::discovery::SaveSlot;
    use sse_storage::transaction::{self, EditSummary};
    use std::fs;
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    const SYNTHETIC_XRAY_SAVE: &[u8] = include_bytes!("../../../../fixtures/synthetic/xray-soc-ee.sav");
    const SYNTHETIC_S2_SAVE: &[u8] =
        include_bytes!("../../../../fixtures/synthetic/writer-s2-money/s2-money-source.sav");

    static NEXT_TEMP_DIRECTORY: AtomicU64 = AtomicU64::new(1);

    struct TempDirectory(PathBuf);

    impl TempDirectory {
        fn new() -> Self {
            let id = NEXT_TEMP_DIRECTORY.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("sse-history-ui-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap_or_else(|error| panic!("create test directory: {error}"));
            Self(path)
        }
    }

    impl Drop for TempDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture_slot(path: &str, game: &str) -> SaveSlot {
        SaveSlot {
            path: PathBuf::from(path),
            candidate_game_id: game.to_owned(),
            candidate_release_id: game.to_owned(),
            size: 1,
            last_write_time_utc: UNIX_EPOCH.checked_add(Duration::from_secs(1)).unwrap_or(UNIX_EPOCH),
            format_id: Some(game.to_owned()),
            game_id: Some(game.to_owned()),
            detection_error: None,
        }
    }

    #[test]
    fn diagnostic_text_truncation_obeys_character_boundary() {
        assert_eq!(truncate("абвг", 3), "аб…");
        assert_eq!(truncate("ok", 3), "ok");
    }

    #[test]
    fn backup_destination_is_a_new_sibling_and_preserves_extension() {
        let source = Path::new("/save/game_slot.sav");
        assert_eq!(
            restored_output_path_at(source, 123),
            Path::new("/save/game_slot_restored_123.sav")
        );
        assert_eq!(
            restored_output_path_at(Path::new("/save/game_slot.scop"), 123),
            Path::new("/save/game_slot_restored_123.scop")
        );
        assert_eq!(
            restored_output_path_at(Path::new("/save/game_slot.scs"), 123),
            Path::new("/save/game_slot_restored_123.scs")
        );
    }

    #[test]
    fn in_place_restore_is_blocked_for_the_selected_save_with_pending_draft_edits() -> sse_core::Result<()> {
        let source = PathBuf::from("/saves/slot.sav");
        let source_sha256 = "ab".repeat(32);
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(source.clone(), source_sha256.clone());
        let mut draft = sse_storage::drafts::DraftPlan::empty(&source_sha256)?;
        draft.money = Some(1234);
        app.set_draft(draft);

        assert!(super::has_pending_edits_for_selected_source(&app, &source));
        assert!(!super::has_pending_edits_for_selected_source(
            &app,
            Path::new("/saves/other.sav")
        ));

        app.discard_draft(&source_sha256);
        app.set_invalid_numeric_input(true);
        assert!(super::has_pending_edits_for_selected_source(&app, &source));
        Ok(())
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

    #[test]
    fn semantic_compare_reports_money_and_item_changes_without_byte_ranges() {
        let old_money = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-source.sav");
        let new_money = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let report = compare_packed(Path::new("old.sav"), old_money, Path::new("new.sav"), new_money)
            .unwrap_or_else(|error| panic!("compare money fixtures: {error}"));
        assert_eq!(report.changed, 1);
        assert_eq!(
            report.differences.first().map(|entry| entry.label.as_str()),
            Some("Деньги")
        );

        let old_items = include_bytes!("../../../../fixtures/synthetic/writer-stacks/xray-stack-cop-source.sav");
        let new_items = include_bytes!("../../../../fixtures/synthetic/writer-stacks/xray-stack-cop-expected.sav");
        let report = compare_packed(Path::new("old.sav"), old_items, Path::new("new.sav"), new_items)
            .unwrap_or_else(|error| panic!("compare item fixtures: {error}"));
        assert!(report
            .differences
            .iter()
            .any(|entry| entry.category == "Предметы" && entry.kind == super::DifferenceKind::Changed));
    }

    #[test]
    fn compare_screen_lists_two_same_game_candidates_when_no_save_is_open() -> sse_core::Result<()> {
        let fonts = Fonts::bundled()?;
        let mut tree = Tree::new(fonts, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut app = sse_app::AppState::new();
        let mut screen = HistoryScreen::new(ScreenId::Compare, "test", Workspace::default());
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.build(&mut cx, host)?;
            screen.render_saves(
                &mut cx,
                vec![
                    fixture_slot("soc-a.sav", "soc"),
                    fixture_slot("soc-b.sav", "soc"),
                    fixture_slot("cop-a.sav", "cop"),
                ],
            )?;
        }
        assert_eq!(
            screen.actions.len(),
            3,
            "all saves must be offered before a first selection"
        );

        let first_action = screen
            .actions
            .first()
            .cloned()
            .ok_or_else(|| sse_core::Error::damaged("missing first candidate"))?;
        let first_path = match first_action.action {
            Action::Compare(path) => path,
            _ => return Err(sse_core::Error::damaged("expected compare action")),
        };
        {
            let message = Message::User(AppMessage::Tick(0));
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.message(&mut cx, &message, Some(first_action.widget))?;
        }
        assert_eq!(screen.compare_selection, vec![first_path.clone()]);
        assert_eq!(
            screen.actions.len(),
            1,
            "the next choice must be a different save from the same game"
        );
        let second_action = screen
            .actions
            .first()
            .cloned()
            .ok_or_else(|| sse_core::Error::damaged("missing second candidate"))?;
        let second_path = match second_action.action {
            Action::Compare(path) => path,
            _ => return Err(sse_core::Error::damaged("expected compare action")),
        };
        {
            let message = Message::User(AppMessage::Tick(0));
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.message(&mut cx, &message, Some(second_action.widget))?;
        }
        assert_eq!(screen.compare_selection, vec![first_path, second_path]);
        Ok(())
    }

    #[test]
    fn restore_requires_confirmation_and_restores_verified_backup_to_a_new_file() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let save_directory = temp.0.join("saves");
        let backup_directory = temp.0.join("backups");
        fs::create_dir_all(&save_directory)?;
        fs::create_dir_all(&backup_directory)?;
        let source = save_directory.join("slot.sav");
        fs::write(&source, SYNTHETIC_XRAY_SAVE)?;
        let expected_sha = sse_codecs::sha256::sha256_hex(SYNTHETIC_XRAY_SAVE);
        let replacement = include_bytes!("../../../../fixtures/synthetic/writer-money/xray-money-cop-expected.sav");
        let receipt = transaction::export_transaction(
            &source,
            &expected_sha,
            replacement,
            &save_directory.join("edited.sav"),
            &backup_directory,
            EditSummary::default(),
        )?;

        let fonts = Fonts::bundled()?;
        let mut tree = Tree::new(fonts, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut app = sse_app::AppState::new();
        let mut screen = HistoryScreen::new(ScreenId::Backups, "test", Workspace::default());
        let restore_button;
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.build(&mut cx, host)?;
            restore_button = screen
                .rows
                .first()
                .ok_or_else(|| sse_core::Error::damaged("missing restore row"))?
                .button;
            screen.actions.push(ActionButton {
                widget: restore_button,
                action: Action::Restore {
                    journal: receipt.journal_path.clone(),
                    source: source.clone(),
                },
            });
            let message = Message::User(AppMessage::Tick(0));
            screen.message(&mut cx, &message, Some(restore_button))?;
        }
        assert_eq!(
            screen.pending_restore,
            Some((receipt.journal_path.clone(), source.clone(), super::RestoreMode::Copy)),
            "the first click must only request confirmation"
        );
        assert!(tree.dialog_open(), "restore confirmation must be a modal overlay");
        assert_eq!(tree.dialog(), screen.restore_confirmation);
        assert_eq!(fs::read(&source)?, SYNTHETIC_XRAY_SAVE);
        {
            let message = Message::Window(crate::event_loop::WindowEvent::Key {
                pressed: true,
                keysym: 0xff1b,
                text: None,
                ctrl: false,
                shift: false,
            });
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.message(&mut cx, &message, None)?;
        }
        assert!(
            screen.pending_restore.is_none(),
            "Escape must discard the pending confirmation"
        );
        assert!(
            tree.dialog_open(),
            "the shell owns closing the modal after routing Escape"
        );
        tree.close_dialog()?;
        assert!(!tree.dialog_open());

        let restored = super::restore_to_new_path(&receipt.journal_path, &source).map_err(sse_core::Error::System)?;
        assert_ne!(restored, source);
        assert!(restored
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("slot_restored_")));
        assert_eq!(fs::read(&restored)?, SYNTHETIC_XRAY_SAVE);
        assert_eq!(fs::read(&source)?, SYNTHETIC_XRAY_SAVE);
        Ok(())
    }

    #[test]
    fn timeline_orders_each_game_oldest_first_and_unknown_times_last() {
        let slot = |name: &str, time: SystemTime| SaveSlot {
            path: Path::new(name).to_path_buf(),
            candidate_game_id: "cop".to_owned(),
            candidate_release_id: "stalker-cop".to_owned(),
            size: 1,
            last_write_time_utc: time,
            format_id: Some("stalker-cop".to_owned()),
            game_id: Some("cop".to_owned()),
            detection_error: None,
        };
        let earlier = slot("early.sav", UNIX_EPOCH + Duration::from_secs(10));
        let later = slot("late.sav", UNIX_EPOCH + Duration::from_secs(20));
        let unknown = slot("unknown.sav", UNIX_EPOCH);
        assert_eq!(timeline_order(&earlier, &later), std::cmp::Ordering::Less);
        assert_eq!(timeline_order(&later, &unknown), std::cmp::Ordering::Less);
        assert_eq!(format_system_time(UNIX_EPOCH), "01.01.1970 00:00:00");
        assert_eq!(
            format_system_time(UNIX_EPOCH + Duration::from_secs(86_400)),
            "02.01.1970 00:00:00"
        );
    }
}
