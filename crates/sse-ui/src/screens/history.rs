//! S3 screens: verified backups, bounded save comparison, history, and read-only diagnostics.

use super::saves::{build_list_side, list_window, split_label, sync_side_widths, ListSide, RefreshOverview, Workspace};
use super::style::{self, Button, Text};
use super::{AppMessage, Context, Screen, ScreenId};
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::Message;
use crate::glyphs::{Face, TextStyle};
use crate::layout::{Edges, NodeKind, Size, Style};
use crate::process_guard::{
    format_id_for_save_file, is_windows_file_busy_error_text, running_game_for_format, SAVE_WHILE_GAME_RUNNING_WARNING,
};
use crate::widget::{Content, Look, WidgetId};
use crate::widgets::text_input::TextInput;
use sse_core::{Error, Result, SaveBuffer};
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
        (ScreenId::Backups, crate::strings::t("Резервные копии и восстановление")),
        (ScreenId::Compare, crate::strings::t("Сравнение двух сохранений")),
        (
            ScreenId::Timeline,
            crate::strings::t("История экспортов и резервных копий"),
        ),
        (
            ScreenId::SaveDoctor,
            crate::strings::t("Проверка структуры и целостности"),
        ),
    ]
    .into_iter()
    .map(|(id, subtitle)| Box::new(HistoryScreen::new(id, subtitle, workspace.clone())) as Box<dyn Screen>)
    .collect()
}

#[derive(Clone, Debug)]
enum Action {
    Restore {
        journal: PathBuf,
        source: PathBuf,
        backup: PathBuf,
    },
    RestoreInPlace {
        journal: PathBuf,
        source: PathBuf,
        backup: PathBuf,
    },
    Compare(PathBuf),
    Diagnose {
        path: PathBuf,
        format_id: Option<String>,
    },
    #[cfg(feature = "native-ui")]
    OpenGameFix {
        game_id: String,
        fix_id: String,
    },
    RepairQuests {
        path: PathBuf,
        format_id: Option<String>,
        expected_sha256: String,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoreMode {
    Copy,
    InPlace,
}

enum GuardedHistoryOperation {
    Restore {
        journal: PathBuf,
        source: PathBuf,
        backup: PathBuf,
        mode: RestoreMode,
    },
    QuestRepair {
        path: PathBuf,
        format_id: String,
        expected_sha256: String,
    },
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
    card: WidgetId,
    label: WidgetId,
    meta: WidgetId,
    button: WidgetId,
    secondary_button: WidgetId,
    select: WidgetId,
}

/// Sets a row's text. A row of the list has two lines: the text before the first " · " is the title, the rest is the
/// detail line. A row without its own detail line (the doctor's) writes its whole text to the one label.
fn set_row_text(tree: &mut crate::widget::Tree, row: ResultRow, text: &str) -> Result<()> {
    if row.meta == row.label {
        return tree.set_text(row.label, text);
    }
    // A line is cut to what a row shows; the full text is in the side panel.
    let (title, meta) = split_label(text);
    tree.set_text(row.label, &truncate(title, 64))?;
    tree.set_text(row.meta, &truncate(meta, 96))
}

#[derive(Debug)]
enum HistoryResult {
    Backups(std::result::Result<Vec<BackupEntry>, String>),
    Saves(std::result::Result<Vec<SaveSlot>, String>),
    Compare(std::result::Result<CompareReport, String>),
    Diagnosis {
        request_id: u64,
        result: std::result::Result<(PathBuf, DiagnosisReport), String>,
    },
    QuestRepaired {
        request_id: sse_app::SaveOperationId,
        result: std::result::Result<QuestRepairCompletion, String>,
    },
    Restored {
        result: std::result::Result<RestoredSave, String>,
    },
    ProcessCheck {
        request_id: u64,
        result: std::result::Result<bool, String>,
    },
}

#[derive(Debug, Clone)]
struct DiagnosisReport {
    summary: String,
    source_sha256: String,
    format_id: Option<String>,
    quest_states: Vec<sse_doctor::QuestTaskState>,
    can_repair_quests: bool,
}

#[derive(Debug, Clone)]
struct QuestRepairCompletion {
    path: PathBuf,
    backup_path: PathBuf,
    maintenance_warning: Option<String>,
    report: std::result::Result<DiagnosisReport, String>,
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
    refresh: Option<WidgetId>,
    restore_confirmation: Option<WidgetId>,
    restore_confirmation_title: Option<WidgetId>,
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
    pending_restore: Option<(PathBuf, PathBuf, PathBuf, RestoreMode)>,
    pending_guarded_operation: Option<(u64, GuardedHistoryOperation)>,
    next_process_check_id: u64,
    process_check_complete: bool,
    diagnosis_request_id: u64,
    diagnosed_selection: Option<(PathBuf, String)>,
    doctor_open_save: Option<WidgetId>,
    doctor_path_input: Option<WidgetId>,
    doctor_check: Option<WidgetId>,
    doctor_path: Option<TextInput>,
    doctor_clipboard: HistoryClipboard,
    diagnosis_running: bool,
    side: Option<ListSide>,
    selected_row: Option<usize>,
    side_action: Option<WidgetId>,
    side_secondary: Option<WidgetId>,
    side_actions: Option<WidgetId>,
    /// The side panel's key–value lines for each backup row on the page: file, time, status, size, path.
    backup_details: Vec<[String; 5]>,
}

impl HistoryScreen {
    /// The save doctor's heading, description and file controls: a card above the results.
    fn build_doctor_controls(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let card = style::card(cx.tree, host)?;
        style::d2::panel_title(cx.tree, card, doctor_title())?;
        if self.id == ScreenId::SaveDoctor {
            super::saves::paragraph(cx.tree, card, doctor_description(), Text::Note)?;
            let path_row = style::row(cx.tree, card)?;
            style::label(cx.tree, path_row, crate::strings::t("Файл"), Text::Note)?;
            let colors = crate::theme::current().colors;
            let path_input = cx.tree.add(
                Some(path_row),
                NodeKind::Leaf,
                Style {
                    min: Size::new(360.0, crate::theme::BUTTON_HEIGHT),
                    padding: Edges {
                        left: 12.0,
                        top: 0.0,
                        right: 12.0,
                        bottom: 0.0,
                    },
                    ..Style::default()
                },
                Content::Input {
                    text: String::new(),
                    style: TextStyle::new(Face::Body, 16.0),
                },
                Look {
                    fill: Some(style::rgb(colors.background[4])),
                    border: Some((style::rgb(colors.borders[1]), 1.0)),
                    radius: crate::theme::BUTTON_RADIUS,
                    text: style::rgb(colors.text[0]),
                    ..Look::default()
                },
            )?;
            self.doctor_path_input = Some(path_input);
            self.doctor_path = Some(TextInput::new("", doctor_path_edit_config())?);
            let doctor_actions = style::row(cx.tree, card)?;
            self.doctor_check = Some(style::button(
                cx.tree,
                doctor_actions,
                crate::strings::t("ПРОВЕРИТЬ"),
                Button::Primary,
            )?);
            self.doctor_open_save = Some(style::button(
                cx.tree,
                doctor_actions,
                crate::strings::t("Открыть"),
                Button::Secondary,
            )?);
            if let Some(check) = self.doctor_check {
                cx.tree.set_enabled(check, false)?;
            }
        }
        Ok(())
    }

    fn new(id: ScreenId, subtitle: &'static str, workspace: Workspace) -> Self {
        Self {
            side: None,
            selected_row: None,
            side_action: None,
            side_secondary: None,
            side_actions: None,
            backup_details: Vec::new(),
            id,
            subtitle,
            workspace,
            refresh: None,
            restore_confirmation: None,
            restore_confirmation_title: None,
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
            pending_guarded_operation: None,
            next_process_check_id: 0,
            process_check_complete: false,
            diagnosis_request_id: 0,
            diagnosed_selection: None,
            doctor_open_save: None,
            doctor_path_input: None,
            doctor_check: None,
            doctor_path: None,
            doctor_clipboard: HistoryClipboard::default(),
            diagnosis_running: false,
        }
    }

    fn set_doctor_path(&mut self, tree: &mut crate::widget::Tree, value: &str) -> Result<()> {
        let Some(widget) = self.doctor_path_input else {
            return Ok(());
        };
        let value = value.trim();
        let current = self.doctor_path.as_ref().map_or_else(String::new, TextInput::text);
        if current != value {
            self.doctor_path = Some(TextInput::new(value, doctor_path_edit_config())?);
            self.diagnosis_request_id = self.diagnosis_request_id.saturating_add(1);
            self.diagnosed_selection = None;
            self.diagnosis_running = false;
            self.clear_results(tree)?;
            self.set_summary(tree, crate::strings::t("Путь изменён. Нажмите «ПРОВЕРИТЬ СОХРАНЕНИЕ»."))?;
        }
        tree.set_input_text(widget, value)?;
        if let Some(check) = self.doctor_check {
            tree.set_enabled(check, !value.is_empty() && !self.diagnosis_running)?;
        }
        Ok(())
    }

    fn sync_doctor_check(&self, tree: &mut crate::widget::Tree) -> Result<()> {
        let has_path = self
            .doctor_path
            .as_ref()
            .and_then(|input| manual_save_path(&input.text()))
            .is_some();
        if let Some(check) = self.doctor_check {
            tree.set_enabled(check, has_path && !self.diagnosis_running)?;
        }
        Ok(())
    }

    fn edit_doctor_path(
        &mut self,
        cx: &mut Context<'_>,
        keysym: u32,
        text: Option<char>,
        ctrl: bool,
        shift: bool,
    ) -> Result<()> {
        let key = match keysym {
            0xff08 => Key::Backspace,
            0xffff => Key::Delete,
            0xff51 => Key::Left,
            0xff53 => Key::Right,
            0xff50 => Key::Home,
            0xff57 => Key::End,
            value if ctrl && matches!(value, 0x61 | 0x41) => Key::A,
            value if ctrl && matches!(value, 0x63 | 0x43) => Key::C,
            value if ctrl && matches!(value, 0x76 | 0x56) => Key::V,
            value if ctrl && matches!(value, 0x78 | 0x58) => Key::X,
            _ => Key::Character(text.unwrap_or('\0')),
        };
        let typed = text.map(|character| character.to_string());
        let changed = {
            let Some(input) = self.doctor_path.as_mut() else {
                return Ok(());
            };
            input.key(
                key,
                Modifiers { ctrl, shift },
                typed.as_deref(),
                &mut self.doctor_clipboard,
            )?
        };
        if changed {
            let value = self.doctor_path.as_ref().map_or_else(String::new, TextInput::text);
            if let Some(widget) = self.doctor_path_input {
                cx.tree.set_input_text(widget, &value)?;
            }
            self.diagnosis_request_id = self.diagnosis_request_id.saturating_add(1);
            self.diagnosed_selection = None;
            self.diagnosis_running = false;
            self.clear_results(cx.tree)?;
            self.set_summary(
                cx.tree,
                crate::strings::t("Путь изменён. Нажмите «ПРОВЕРИТЬ СОХРАНЕНИЕ»."),
            )?;
            self.sync_doctor_check(cx.tree)?;
        }
        Ok(())
    }

    fn check_doctor_path(&mut self, cx: &mut Context<'_>) -> Result<()> {
        let value = self.doctor_path.as_ref().map_or_else(String::new, TextInput::text);
        let Some(path) = manual_save_path(&value) else {
            self.set_summary(cx.tree, crate::strings::t("Укажите путь к файлу сохранения."))?;
            self.sync_doctor_check(cx.tree)?;
            return Ok(());
        };
        if self.diagnosis_running {
            return Ok(());
        }
        let file = display_name(&path);
        let summary = crate::strings::tr_in(Some(crate::strings::current_language()), "Проверяю {0}…", &[&file]);
        self.set_summary(cx.tree, &summary)?;
        if self.start_diagnosis(path, None, cx.proxy.cloned())? {
            self.diagnosis_running = true;
            self.sync_doctor_check(cx.tree)?;
        }
        Ok(())
    }

    fn request_refresh(&self, proxy: Option<crate::event_loop::Proxy<AppMessage>>) -> Result<()> {
        let Some(proxy) = proxy else {
            return Ok(());
        };
        let id = self.id;
        let backup_directory = self.workspace.backup_directory();
        self.workspace.spawn("history-refresh", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = match id {
                ScreenId::Backups => HistoryResult::Backups(
                    transaction::list_backups(&backup_directory).map_err(|error| error.to_string()),
                ),
                ScreenId::Compare | ScreenId::Timeline | ScreenId::SaveDoctor => {
                    HistoryResult::Saves(discover_saves().map_err(|error| error.to_string()))
                }
                _ => return,
            };
            proxy.send(AppMessage::ToScreen(id, Box::new(result)));
        })
    }

    fn start_compare(
        &self,
        first: PathBuf,
        second: PathBuf,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) -> Result<()> {
        let Some(proxy) = proxy else {
            return Ok(());
        };
        let id = self.id;
        self.workspace.spawn("save-compare", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = compare_saves(&first, &second);
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Compare(result))));
        })
    }

    fn start_diagnosis(
        &mut self,
        path: PathBuf,
        format_id: Option<String>,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) -> Result<bool> {
        let Some(proxy) = proxy else {
            return Ok(false);
        };
        self.diagnosis_request_id = self.diagnosis_request_id.saturating_add(1);
        let request_id = self.diagnosis_request_id;
        let id = self.id;
        self.workspace.spawn("save-diagnosis", move |context| {
            if context.is_cancelled() {
                return;
            }
            let result = diagnose_save(&path, format_id.as_deref());
            proxy.send(AppMessage::ToScreen(
                id,
                Box::new(HistoryResult::Diagnosis { request_id, result }),
            ));
        })?;
        self.diagnosis_running = true;
        Ok(true)
    }

    fn start_quest_repair_write(
        &self,
        path: PathBuf,
        expected_sha256: String,
        format_id: Option<String>,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) -> Result<bool> {
        let Some(proxy) = proxy else {
            return Ok(false);
        };
        let Some(save_guard) = self.workspace.session().begin_save(&path) else {
            return Ok(false);
        };
        let request_id = save_guard.id();
        let backup_directory = self.workspace.backup_directory();
        let id = self.id;
        self.workspace.spawn("quest-repair", move |context| {
            let result = if context.is_cancelled() {
                Err(crate::strings::t("Операция ремонта отменена.").to_owned())
            } else {
                repair_quest_save(&path, &expected_sha256, &backup_directory).map(
                    |(backup_path, maintenance_warning)| {
                        let report = diagnose_save(&path, format_id.as_deref())
                            .map(|(_, report)| report)
                            .map_err(|error| error.to_string());
                        QuestRepairCompletion {
                            path,
                            backup_path,
                            maintenance_warning,
                            report,
                        }
                    },
                )
            };
            drop(save_guard);
            proxy.send(AppMessage::ToScreen(
                id,
                Box::new(HistoryResult::QuestRepaired { request_id, result }),
            ));
        })?;
        Ok(true)
    }

    fn start_restore_write(
        &self,
        journal: PathBuf,
        source: PathBuf,
        mode: RestoreMode,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) -> Result<bool> {
        let Some(proxy) = proxy else {
            return Ok(false);
        };
        let restore_guard = if mode == RestoreMode::InPlace {
            let Some(guard) = self.workspace.session().begin_restore(&source) else {
                return Ok(false);
            };
            Some(guard)
        } else {
            None
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
            drop(restore_guard);
            proxy.send(AppMessage::ToScreen(id, Box::new(HistoryResult::Restored { result })));
        })?;
        Ok(true)
    }

    fn start_process_check(&mut self, cx: &mut Context<'_>, operation: GuardedHistoryOperation) -> Result<bool> {
        let Some(proxy) = cx.proxy.cloned() else {
            return Ok(false);
        };
        let Some(request_id) = self.next_process_check_id.checked_add(1) else {
            self.set_summary(cx.tree, crate::strings::t("Исчерпан номер проверки запущенной игры."))?;
            return Ok(false);
        };
        self.next_process_check_id = request_id;
        let (format_id, backup_path) = match &operation {
            GuardedHistoryOperation::Restore { backup, .. } => (None, Some(backup.clone())),
            GuardedHistoryOperation::QuestRepair { format_id, .. } => (Some(format_id.clone()), None),
        };
        self.pending_guarded_operation = Some((request_id, operation));
        self.process_check_complete = false;
        if let Some(title) = self.restore_confirmation_title {
            if matches!(
                self.pending_guarded_operation.as_ref().map(|(_, operation)| operation),
                Some(GuardedHistoryOperation::QuestRepair { .. })
            ) {
                cx.tree.set_text(title, crate::strings::t("ПРОВЕРКА ЗАПУЩЕННОЙ ИГРЫ"))?;
            }
        }
        if let (Some(dialog), Some(description), Some(continue_button)) = (
            self.restore_confirmation,
            self.restore_description,
            self.confirm_restore,
        ) {
            cx.tree.set_text(
                description,
                crate::strings::t("Проверяю, запущена ли игра для выбранного сейва…"),
            )?;
            cx.tree.set_text(continue_button, crate::strings::t("Проверка…"))?;
            cx.tree.set_enabled(continue_button, false)?;
            if cx.tree.dialog() != Some(dialog) {
                cx.tree.open_dialog(dialog)?;
            }
        }
        self.set_summary(cx.tree, crate::strings::t("Проверяю запущенную игру…"))?;
        let id = self.id;
        if let Err(error) = self.workspace.spawn("save-process-check", move |context| {
            let result = if context.is_cancelled() {
                Err("process check was cancelled".to_owned())
            } else {
                let detected_format = match (format_id.as_deref(), backup_path.as_deref()) {
                    (Some(format_id), _) => Ok(format_id.to_owned()),
                    (None, Some(path)) => format_id_for_save_file(path),
                    (None, None) => Err("save format is unavailable".to_owned()),
                };
                detected_format.and_then(|format_id| running_game_for_format(&format_id))
            };
            proxy.send(AppMessage::ToScreen(
                id,
                Box::new(HistoryResult::ProcessCheck { request_id, result }),
            ));
        }) {
            self.pending_guarded_operation = None;
            self.process_check_complete = false;
            let _ = cx.tree.close_dialog()?;
            let summary = crate::strings::tr_in(
                Some(crate::strings::current_language()),
                "Не удалось начать проверку запущенной игры: {0}",
                &[&error],
            );
            self.set_summary(cx.tree, &summary)?;
            return Ok(false);
        }
        Ok(true)
    }

    fn start_guarded_operation_write(
        &self,
        operation: GuardedHistoryOperation,
        proxy: Option<crate::event_loop::Proxy<AppMessage>>,
    ) -> Result<bool> {
        match operation {
            GuardedHistoryOperation::Restore {
                journal,
                source,
                backup: _,
                mode,
            } => self.start_restore_write(journal, source, mode, proxy),
            GuardedHistoryOperation::QuestRepair {
                path,
                format_id,
                expected_sha256,
            } => self.start_quest_repair_write(path, expected_sha256, Some(format_id), proxy),
        }
    }

    fn open_restore_confirmation(
        &mut self,
        cx: &mut Context<'_>,
        journal: PathBuf,
        source: PathBuf,
        backup: PathBuf,
        mode: RestoreMode,
    ) -> Result<()> {
        self.pending_restore = Some((journal, source, backup, mode));
        if let (Some(dialog), Some(description)) = (self.restore_confirmation, self.restore_description) {
            cx.tree.set_text(
                description,
                match mode {
                    RestoreMode::Copy => {
                        crate::strings::t("Создать отдельный файл из проверенной копии? Исходный сейв останется без изменений.")
                    }
                    RestoreMode::InPlace => {
                        crate::strings::t("Заменить исходный сейв? Текущий файл сверяется с журналом, перед записью создаётся страховочный бэкап.")
                    }
                },
            )?;
            cx.tree.open_dialog(dialog)?;
        }
        self.set_summary(cx.tree, crate::strings::t("Подтвердите восстановление."))?;
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
        let rows = self.page_rows(cx.tree);
        self.backup_details.clear();
        let (visible, total, restorable, pages) = match self.backup_entries.as_ref() {
            Some(entries) => {
                let total = entries.len();
                let restorable = entries
                    .iter()
                    .filter(|entry| matches!(entry.status, BackupStatus::Verified | BackupStatus::Interrupted))
                    .count();
                let pages = page_count(total, rows);
                let start = self.page.saturating_mul(rows);
                let visible = entries.iter().skip(start).take(rows).cloned().collect::<Vec<_>>();
                (visible, total, restorable, pages)
            }
            None => return Ok(()),
        };
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        let current_page = self.page.saturating_add(1);
        let summary = crate::strings::tr_in(
            Some(crate::strings::current_language()),
            "Записей: {0} · восстановимо: {1} · страница {2} из {3}",
            &[&total, &restorable, &current_page, &pages],
        );
        self.set_summary(cx.tree, &summary)?;
        for (row_index, entry) in visible.into_iter().enumerate() {
            let file = display_name(&entry.source_path);
            let status = match entry.status {
                BackupStatus::Verified => crate::strings::t("проверен"),
                BackupStatus::Missing => crate::strings::t("файл отсутствует"),
                BackupStatus::Interrupted => crate::strings::t("прервано, копия проверена"),
                BackupStatus::Corrupt => crate::strings::t("ошибка проверки"),
            };
            let Some(slot) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(slot.row, true)?;
            // The time and size are the backup file's own; the path is the save's, cut to fit the panel.
            let metadata = std::fs::metadata(&entry.backup_path).ok();
            let time = metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .map_or_else(|| crate::strings::t("дата неизвестна").to_owned(), format_system_time);
            let size = metadata.as_ref().map_or(0, |metadata| metadata.len());
            let source = truncate(&entry.source_path.to_string_lossy(), 48);
            if self.backup_details.len() <= row_index {
                self.backup_details.resize_with(row_index.saturating_add(1), || {
                    [
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                        String::new(),
                    ]
                });
            }
            if let Some(detail) = self.backup_details.get_mut(row_index) {
                *detail = [
                    file.clone(),
                    time.clone(),
                    status.to_owned(),
                    format!("{size} B"),
                    source,
                ];
            }
            set_row_text(cx.tree, slot, &format!("{file} · {status} · {time}"))?;
            cx.tree.set_visible(slot.button, false)?;
            cx.tree.set_visible(slot.secondary_button, false)?;
            if matches!(entry.status, BackupStatus::Verified | BackupStatus::Interrupted)
                && self.id == ScreenId::Backups
            {
                cx.tree.set_visible(slot.button, true)?;
                cx.tree.set_text(slot.button, crate::strings::t("В копию…"))?;
                self.actions.push(ActionButton {
                    widget: slot.button,
                    action: Action::Restore {
                        journal: entry.journal_path.clone(),
                        source: entry.source_path.clone(),
                        backup: entry.backup_path.clone(),
                    },
                });
                if matches!(entry.operation_mode.as_deref(), Some("replace" | "restore")) {
                    cx.tree.set_visible(slot.secondary_button, true)?;
                    cx.tree
                        .set_text(slot.secondary_button, crate::strings::t("На место…"))?;
                    self.actions.push(ActionButton {
                        widget: slot.secondary_button,
                        action: Action::RestoreInPlace {
                            journal: entry.journal_path.clone(),
                            source: entry.source_path.clone(),
                            backup: entry.backup_path,
                        },
                    });
                }
            }
            if let Some(error) = entry.error {
                let error = truncate(&error, 80);
                let detail = crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "{0} · {1}: {2}",
                    &[&file, &status, &error],
                );
                set_row_text(cx.tree, slot, &detail)?;
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
        let rows = self.page_rows(cx.tree);
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
                let pages = page_count(total, rows);
                let start = self.page.saturating_mul(rows);
                let visible = candidates.iter().skip(start).take(rows).cloned().collect::<Vec<_>>();
                (visible, total, pages)
            }
            None => return Ok(()),
        };
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        if self.id == ScreenId::Compare && total == 0 {
            self.set_summary(cx.tree, crate::strings::t("Нет других сейвов этой игры для сравнения."))?;
        } else {
            let current_page = self.page.saturating_add(1);
            let summary = crate::strings::tr_in(
                Some(crate::strings::current_language()),
                "Найдено файлов: {0} · страница {1} из {2}",
                &[&total, &current_page, &pages],
            );
            self.set_summary(cx.tree, &summary)?;
        }
        for (row_index, save_slot) in visible.into_iter().enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, true)?;
            let filename = display_name(&save_slot.path);
            let format = save_slot
                .format_id
                .as_deref()
                .unwrap_or(crate::strings::t("не распознано"));
            set_row_text(cx.tree, row, &format!("{filename} · {format}"))?;
            let button_text = if self.id == ScreenId::Compare {
                crate::strings::t("Выбрать")
            } else {
                crate::strings::t("Проверить")
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

    /// Rows per page: on the list layout the rows that fit the window, on the doctor's layout the fixed count.
    fn page_rows(&self, tree: &crate::widget::Tree) -> usize {
        match self.side {
            Some(_) => list_window(tree).min(MAXIMUM_VISIBLE_ENTRIES),
            None => MAXIMUM_VISIBLE_ENTRIES,
        }
    }

    fn update_page_controls(&self, tree: &mut crate::widget::Tree, pages: usize) -> Result<()> {
        let multi_page = pages > 1;
        if let Some(list) = self.side.as_ref() {
            tree.set_visible(list.pages, multi_page)?;
        }
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
        let rows = self.page_rows(cx.tree);
        let Some(entries) = self.save_entries.as_ref() else {
            return Ok(());
        };
        let mut ordered = entries.clone();
        ordered.sort_by(timeline_order);
        let total = ordered.len();
        let pages = page_count(total, rows);
        let start = self.page.saturating_mul(rows);
        let visible = ordered.iter().skip(start).take(rows).collect::<Vec<_>>();
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, pages)?;
        let current_page = self.page.saturating_add(1);
        let summary = crate::strings::tr_in(
            Some(crate::strings::current_language()),
            "Сохранений: {0} · порядок: игра, от старых к новым · страница {1} из {2}",
            &[&total, &current_page, &pages],
        );
        self.set_summary(cx.tree, &summary)?;
        for (row_index, save) in visible.into_iter().enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, false)?;
            let game = save.game_id.as_deref().unwrap_or(&save.candidate_game_id);
            set_row_text(
                cx.tree,
                row,
                &crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "{0} · {1} · {2} · {3} байт",
                    &[
                        &display_name(&save.path),
                        &game,
                        &format_system_time(save.last_write_time_utc),
                        &save.size,
                    ],
                ),
            )?;
        }
        Ok(())
    }

    fn render_compare(&mut self, cx: &mut Context<'_>, report: CompareReport) -> Result<()> {
        let rows = self.page_rows(cx.tree);
        self.clear_results(cx.tree)?;
        self.update_page_controls(cx.tree, 0)?;
        let count = report.differences.len();
        let first = display_name(&report.first);
        let second = display_name(&report.second);
        let summary = crate::strings::tr_in(
            Some(crate::strings::current_language()),
            "Различий: {0} · добавлено: {1} · удалено: {2} · изменено: {3} · {4} → {5}",
            &[&count, &report.added, &report.removed, &report.changed, &first, &second],
        );
        self.set_summary(cx.tree, &summary)?;
        if count == 0 {
            if let Some(row) = self.rows.first().copied() {
                cx.tree.set_visible(row.row, true)?;
                cx.tree.set_visible(row.button, false)?;
                set_row_text(cx.tree, row, crate::strings::t("Различий в деньгах и предметах нет."))?;
            }
        }
        for (row_index, difference) in report.differences.iter().take(rows).enumerate() {
            let Some(row) = self.rows.get(row_index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            cx.tree.set_visible(row.button, false)?;
            let kind = crate::strings::t(difference_kind_text(difference.kind));
            let label = crate::strings::t(&difference.label);
            let category = crate::strings::t(difference.category);
            let language = crate::strings::current_language();
            let value_a = localized_comparison_value(&difference.value_a, language);
            let value_b = localized_comparison_value(&difference.value_b, language);
            set_row_text(
                cx.tree,
                row,
                &format!("{} · {} · {} → {} · {}", kind, label, value_a, value_b, category),
            )?;
        }
        if count > rows {
            self.set_summary(
                cx.tree,
                &crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "Различий: {0} · добавлено: {1} · удалено: {2} · изменено: {3} · показаны первые {4}",
                    &[&count, &report.added, &report.removed, &report.changed, &rows],
                ),
            )?;
        }
        Ok(())
    }

    fn render_diagnosis(&mut self, cx: &mut Context<'_>, path: &Path, report: DiagnosisReport) -> Result<()> {
        self.clear_results(cx.tree)?;
        self.set_summary(cx.tree, &report.summary)?;
        let mut repair_button_added = false;
        for (index, state) in report.quest_states.iter().enumerate() {
            let Some(row) = self.rows.get(index).copied() else {
                break;
            };
            cx.tree.set_visible(row.row, true)?;
            set_row_text(
                cx.tree,
                row,
                &format!("{} · {}", quest_title(state.id), quest_detail(state)),
            )?;
            cx.tree.set_visible(row.button, false)?;
            cx.tree.set_visible(row.secondary_button, false)?;
            if report.can_repair_quests && !repair_button_added && state.status == sse_doctor::QuestTaskStatus::Broken {
                cx.tree.set_text(row.button, crate::strings::t("ИСПРАВИТЬ КВЕСТЫ"))?;
                cx.tree.set_visible(row.button, true)?;
                self.actions.push(ActionButton {
                    widget: row.button,
                    action: Action::RepairQuests {
                        path: path.to_path_buf(),
                        format_id: report.format_id.clone(),
                        expected_sha256: report.source_sha256.clone(),
                    },
                });
                repair_button_added = true;
            }
            #[cfg(feature = "native-ui")]
            if state.status == sse_doctor::QuestTaskStatus::Broken && state.needs_preventing_fix {
                if let Some(fix_id) = state.preventing_fix_id {
                    if let Some(definition) = sse_fixes::GameFixCatalog::try_get(fix_id) {
                        cx.tree
                            .set_text(row.secondary_button, crate::strings::t("УСТАНОВИТЬ ИСПРАВЛЕНИЕ ИГРЫ"))?;
                        cx.tree.set_visible(row.secondary_button, true)?;
                        self.actions.push(ActionButton {
                            widget: row.secondary_button,
                            action: Action::OpenGameFix {
                                game_id: definition.game.id().to_owned(),
                                fix_id: fix_id.to_owned(),
                            },
                        });
                    }
                }
            }
        }
        Ok(())
    }

    fn render_diagnosis_error(&mut self, tree: &mut crate::widget::Tree, error: &str) -> Result<()> {
        self.clear_results(tree)?;
        self.set_summary(tree, crate::strings::t("Ошибка"))?;
        if let Some(row) = self.rows.first().copied() {
            tree.set_visible(row.row, true)?;
            set_row_text(tree, row, &doctor_error_detail(error))?;
            tree.set_visible(row.button, false)?;
            tree.set_visible(row.secondary_button, false)?;
        }
        Ok(())
    }

    fn render_quest_repair(&mut self, cx: &mut Context<'_>, completion: QuestRepairCompletion) -> Result<()> {
        let QuestRepairCompletion {
            path,
            backup_path,
            maintenance_warning,
            report,
        } = completion;
        let backup_name = backup_path.file_name().map_or_else(
            || backup_path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let mut status = match report {
            Ok(report) => {
                self.render_diagnosis(cx, &path, report)?;
                crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "КВЕСТЫ ИСПРАВЛЕНЫ. Резервная копия: {0}",
                    &[&backup_name],
                )
            }
            Err(error) => {
                self.clear_results(cx.tree)?;
                let error = truncate(&error, 120);
                crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "КВЕСТЫ ИСПРАВЛЕНЫ. Резервная копия: {0} · повторная проверка: {1}",
                    &[&backup_name, &error],
                )
            }
        };
        if let Some(warning) = maintenance_warning {
            status.push_str(&crate::strings::tr_in(
                Some(crate::strings::current_language()),
                " · ротация старых копий не завершена: {0}",
                &[&warning],
            ));
        }
        self.set_summary(cx.tree, &status)?;
        Ok(())
    }
}

impl HistoryScreen {
    /// The restore confirmation overlay, shared by both layouts.
    fn build_restore_confirmation(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let confirmation_host = cx.tree.overlay_host().unwrap_or(host);
        let confirmation = style::card(cx.tree, confirmation_host)?;
        self.restore_confirmation = Some(confirmation);
        self.restore_confirmation_title = Some(style::label(
            cx.tree,
            confirmation,
            crate::strings::t("ВОССТАНОВИТЬ РЕЗЕРВНУЮ КОПИЮ"),
            Text::Heading,
        )?);
        self.restore_description = Some(style::label(
            cx.tree,
            confirmation,
            crate::strings::t(
                "Подтвердите создание отдельного файла из проверенной копии. Исходный сейв останется без изменений.",
            ),
            Text::Body,
        )?);
        let confirm_row = style::row(cx.tree, confirmation)?;
        self.confirm_restore = Some(style::button(
            cx.tree,
            confirm_row,
            crate::strings::t("Подтвердить восстановление"),
            Button::Primary,
        )?);
        self.cancel_restore = Some(style::button(
            cx.tree,
            confirm_row,
            crate::strings::t("Отмена"),
            Button::Secondary,
        )?);
        cx.tree.set_visible(confirmation, false)?;
        Ok(())
    }

    /// Layout of the backups, compare and timeline screens: the list of entries to the left, the chosen entry and its
    /// actions in the side panel. The entries' own buttons are kept hidden; the side panel's buttons stand for them.
    fn build_list_layout(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        let (title, empty, action) = match self.id {
            ScreenId::SaveDoctor => (
                crate::strings::t("РЕЗУЛЬТАТЫ ПРОВЕРКИ"),
                crate::strings::t("Выберите результат слева."),
                crate::strings::t("Список сейвов"),
            ),
            ScreenId::Backups => (
                crate::strings::t("БЭКАПЫ И ВОССТАНОВЛЕНИЕ"),
                crate::strings::t("Журнал резервных копий сверяется с файлами и SHA-256; восстановление идёт в отдельный файл после подтверждения."),
                crate::strings::t("Обновить"),
            ),
            ScreenId::Compare => (
                crate::strings::t("СРАВНЕНИЕ СОХРАНЕНИЙ"),
                crate::strings::t("Показываются только различия в читаемых значениях денег и предметов."),
                crate::strings::t("Найти сейвы"),
            ),
            ScreenId::Timeline => (
                crate::strings::t("ИСТОРИЯ СОХРАНЕНИЙ"),
                crate::strings::t("Временная последовательность строится по времени изменения файлов сейвов."),
                crate::strings::t("Обновить историю"),
            ),
            _ => (
                crate::strings::t("ИСТОРИЯ"),
                crate::strings::t("Нажмите кнопку, чтобы прочитать локальные данные."),
                crate::strings::t("Обновить"),
            ),
        };
        let keys = [
            crate::strings::t("Файл"),
            crate::strings::t("Время"),
            crate::strings::t("Статус"),
            crate::strings::t("Размер"),
            crate::strings::t("Путь"),
        ]
        .map(str::to_owned);
        let list = build_list_side(
            cx,
            host,
            title,
            crate::strings::t("ВЫБРАННАЯ ЗАПИСЬ"),
            &keys,
            (
                empty,
                crate::strings::t("Нажмите кнопку, чтобы прочитать локальные данные."),
            ),
            Some(action),
        )?;
        // The entries' own buttons live in a hidden host: a row shows them only through the side panel.
        // Hidden children still reserve their parent's gap, so the host sits last in the side panel: its gap only
        // shortens the panel's free room, not the list's.
        let inspector = cx.tree.children(list.side).first().copied().unwrap_or(host);
        let hidden = cx.tree.add(
            Some(inspector),
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_visible(hidden, false)?;
        self.rows.clear();
        for row in &list.rows {
            let button = style::button(cx.tree, hidden, "", Button::Secondary)?;
            let secondary_button = style::button(cx.tree, hidden, "", Button::Danger)?;
            self.rows.push(ResultRow {
                row: row.stack,
                card: row.card,
                label: row.title,
                meta: row.meta,
                button,
                secondary_button,
                select: row.select,
            });
        }
        // The two actions of a backup are long labels: one above the other, each as wide as the side panel. The
        // column takes the row's place in the inspector; the row itself stays hidden.
        let inspector = cx.tree.children(list.side).first().copied().unwrap_or(host);
        let stacked = cx.tree.add(
            Some(inspector),
            NodeKind::Column,
            Style {
                gap: Size::new(0.0, crate::theme::CONTROL_GAP),
                align_items: crate::layout::Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        cx.tree.set_visible(stacked, false)?;
        cx.tree.set_visible(list.actions, false)?;
        let side_action = style::d2::button(
            cx.tree,
            stacked,
            "",
            style::d2::ButtonKind::Primary,
            style::d2::ButtonSize::Normal,
        )?;
        let side_secondary = style::d2::button(
            cx.tree,
            stacked,
            "",
            style::d2::ButtonKind::Secondary,
            style::d2::ButtonSize::Normal,
        )?;
        self.side_actions = Some(stacked);
        self.side_action = Some(side_action);
        self.side_secondary = Some(side_secondary);
        self.refresh = list.action_button;
        self.previous_page = Some(list.previous);
        self.next_page = Some(list.next);
        self.summary = Some(list.note);
        self.side = Some(list);
        self.build_restore_confirmation(cx, host)?;
        self.sync_side(cx.tree)
    }

    /// Shows the chosen entry in the side panel and its actions as the entry's own buttons are now.
    fn sync_side(&self, tree: &mut crate::widget::Tree) -> Result<()> {
        let Some(list) = self.side.as_ref() else {
            return Ok(());
        };
        sync_side_widths(tree, list)?;
        let shown: Vec<bool> = self.rows.iter().map(|row| tree.is_visible(row.row)).collect();
        let selected = self
            .selected_row
            .filter(|index| shown.get(*index).copied().unwrap_or(false));
        // The header counter is the number of entries the screen holds, not of rows on this page.
        let total = match (&self.backup_entries, &self.save_entries) {
            (Some(entries), _) => entries.len(),
            (None, Some(_)) if self.id == ScreenId::Compare => self.actions.len(),
            (None, Some(entries)) => entries.len(),
            (None, None) => shown.iter().filter(|visible| **visible).count(),
        };
        tree.set_text(list.count, &total.to_string())?;
        for (index, row) in self.rows.iter().enumerate() {
            let chosen = Some(index) == selected;
            tree.set_look(
                row.card,
                if chosen {
                    crate::widget::Look {
                        fill: Some(style::d2::argb(crate::theme::d2::ACCENT_TINT)),
                        border: Some((style::d2::argb(crate::theme::d2::ACCENT), 1.0)),
                        radius: crate::theme::d2::RADIUS_BADGE,
                        ..crate::widget::Look::default()
                    }
                } else {
                    crate::widget::Look::default()
                },
            )?;
        }
        let Some(index) = selected else {
            for id in &list.kv_rows {
                tree.set_visible(*id, false)?;
            }
            tree.set_visible(list.detail, false)?;
            for id in [self.side_action, self.side_secondary].into_iter().flatten() {
                tree.set_visible(id, false)?;
            }
            tree.set_visible(self.side_actions.unwrap_or(list.actions), false)?;
            tree.set_visible(list.empty, true)?;
            return Ok(());
        };
        let Some(row) = self.rows.get(index).copied() else {
            return Ok(());
        };
        let title = tree.text(row.label)?.to_owned();
        let detail = tree.text(row.meta)?.to_owned();
        if let Some(fields) = self.backup_details.get(index).filter(|_| self.id == ScreenId::Backups) {
            // A backup shows its own record: the file, time, status, size and path, one key–value line each.
            for (value, text) in list.kv_values.iter().zip(fields.iter()) {
                tree.set_text(*value, text)?;
            }
            for id in &list.kv_rows {
                tree.set_visible(*id, true)?;
            }
            tree.set_visible(list.detail, false)?;
        } else {
            if let Some(value) = list.kv_values.first() {
                tree.set_text(*value, &truncate(&title, 36))?;
            }
            // The file name is the key line; the details wrap in the paragraph below it, inside the panel.
            for (position, id) in list.kv_rows.iter().enumerate() {
                tree.set_visible(*id, position == 0)?;
            }
            tree.set_text(list.detail, &detail)?;
            tree.set_visible(list.detail, true)?;
        }
        tree.set_visible(list.empty, false)?;
        // The entry's buttons sit in a hidden host, so their own visibility is not enough: an action exists when the
        // screen offers it for this row.
        let primary = self.actions.iter().any(|action| action.widget == row.button);
        let secondary = self.actions.iter().any(|action| action.widget == row.secondary_button);
        if let Some(action) = self.side_action {
            // The label is set after the build, so it is written in capitals here, like the build's own labels.
            let text = tree.text(row.button)?.to_uppercase();
            tree.set_text(action, &text)?;
            tree.set_enabled(action, tree.is_enabled(row.button)?)?;
            tree.set_visible(action, primary)?;
        }
        if let Some(action) = self.side_secondary {
            let text = tree.text(row.secondary_button)?.to_uppercase();
            tree.set_text(action, &text)?;
            tree.set_enabled(action, tree.is_enabled(row.secondary_button)?)?;
            tree.set_visible(action, secondary)?;
        }
        tree.set_visible(self.side_actions.unwrap_or(list.actions), primary || secondary)?;
        Ok(())
    }

    /// The side panel's buttons act for the chosen entry: they stand for its own buttons.
    fn side_click(&self, clicked: Option<WidgetId>) -> Option<WidgetId> {
        let side_button = clicked.is_some() && (clicked == self.side_action || clicked == self.side_secondary);
        if !side_button {
            return clicked;
        }
        let row = self.selected_row.and_then(|index| self.rows.get(index))?;
        if clicked == self.side_action {
            return Some(row.button);
        }
        Some(row.secondary_button)
    }
}

impl HistoryScreen {
    fn message_inner(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        self.workspace.poll_tasks();
        if let Message::User(AppMessage::ToScreen(target, payload)) = message {
            if *target == self.id {
                if let Some(HistoryResult::ProcessCheck { request_id, result }) =
                    payload.downcast_ref::<HistoryResult>()
                {
                    let is_current = self
                        .pending_guarded_operation
                        .as_ref()
                        .is_some_and(|(pending_id, _)| pending_id == request_id);
                    if is_current {
                        match result {
                            Ok(false) => {
                                let Some((_, operation)) = self.pending_guarded_operation.take() else {
                                    return Ok(());
                                };
                                self.process_check_complete = false;
                                let _ = cx.tree.close_dialog()?;
                                if !self.start_guarded_operation_write(operation, cx.proxy.cloned())? {
                                    let status = if self.workspace.is_saving() {
                                        crate::strings::t("Дождитесь завершения записи сейва.")
                                    } else if self.workspace.is_restoring() {
                                        crate::strings::t("Восстановление сейва уже выполняется.")
                                    } else {
                                        crate::strings::t("Не удалось начать операцию с сейвом.")
                                    };
                                    self.set_summary(cx.tree, status)?;
                                }
                            }
                            Ok(true) => {
                                self.process_check_complete = true;
                                if let Some(description) = self.restore_description {
                                    cx.tree.set_text(description, SAVE_WHILE_GAME_RUNNING_WARNING)?;
                                }
                                if let Some(continue_button) = self.confirm_restore {
                                    cx.tree
                                        .set_text(continue_button, crate::strings::t("Всё равно сохранить"))?;
                                    cx.tree.set_enabled(continue_button, true)?;
                                }
                                self.set_summary(cx.tree, SAVE_WHILE_GAME_RUNNING_WARNING)?;
                            }
                            Err(error) => {
                                self.pending_guarded_operation = None;
                                self.process_check_complete = false;
                                let _ = cx.tree.close_dialog()?;
                                let summary = crate::strings::tr_in(
                                    Some(crate::strings::current_language()),
                                    "Не удалось проверить запущенную игру; операция отменена: {0}",
                                    &[error],
                                );
                                self.set_summary(cx.tree, &summary)?;
                            }
                        }
                    }
                    return Ok(());
                }
            }
        }
        if self.pending_guarded_operation.is_some() {
            let escape = matches!(
                message,
                Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym: 0xff1b,
                    ..
                })
            );
            if escape || clicked.is_some_and(|id| Some(id) == self.cancel_restore) {
                self.pending_guarded_operation = None;
                self.process_check_complete = false;
                if !escape {
                    let _ = cx.tree.close_dialog()?;
                }
                self.set_summary(cx.tree, crate::strings::t("Операция отменена."))?;
                return Ok(());
            }
            if clicked.is_some_and(|id| Some(id) == self.confirm_restore) && self.process_check_complete {
                let Some((_, operation)) = self.pending_guarded_operation.take() else {
                    return Ok(());
                };
                self.process_check_complete = false;
                let _ = cx.tree.close_dialog()?;
                if !self.start_guarded_operation_write(operation, cx.proxy.cloned())? {
                    let status = if self.workspace.is_saving() {
                        crate::strings::t("Дождитесь завершения записи сейва.")
                    } else if self.workspace.is_restoring() {
                        crate::strings::t("Восстановление сейва уже выполняется.")
                    } else {
                        crate::strings::t("Не удалось начать операцию с сейвом.")
                    };
                    self.set_summary(cx.tree, status)?;
                }
                return Ok(());
            }
            return Ok(());
        }
        if self.id == ScreenId::SaveDoctor {
            if let (Some(widget), Some(input)) = (self.doctor_path_input, self.doctor_path.as_mut()) {
                if clicked.is_some() && clicked == Some(widget) {
                    cx.tree.set_focus(Some(widget))?;
                    input.focus(true, 0);
                } else {
                    input.focus(cx.tree.focused() == Some(widget), 0);
                }
                if input.focused() {
                    if let Message::Window(crate::event_loop::WindowEvent::Key {
                        pressed: true,
                        keysym,
                        text,
                        ctrl,
                        shift,
                    }) = message
                    {
                        if *keysym == 0xff0d {
                            self.check_doctor_path(cx)?;
                            return Ok(());
                        }
                        if *keysym == 0xff1b {
                            input.focus(false, 0);
                            cx.tree.set_focus(None)?;
                            return Ok(());
                        }
                        self.edit_doctor_path(cx, *keysym, *text, *ctrl, *shift)?;
                        return Ok(());
                    }
                }
            }
            if clicked.is_some() && clicked == self.doctor_check {
                self.check_doctor_path(cx)?;
                return Ok(());
            }
            if clicked.is_some() && clicked == self.doctor_open_save {
                if let Some(proxy) = cx.proxy.cloned() {
                    let _ = proxy.send(AppMessage::OpenSavePicker {
                        return_to: ScreenId::SaveDoctor,
                    });
                    self.set_summary(cx.tree, crate::strings::t("Открываю выбор файла сохранения…"))?;
                } else {
                    self.set_summary(
                        cx.tree,
                        crate::strings::t("Выбор файла доступен только в рабочем окне."),
                    )?;
                }
                return Ok(());
            }
        }
        if self.pending_restore.is_some() {
            let escape = matches!(
                message,
                Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym: 0xff1b,
                    ..
                })
            );
            if escape {
                self.pending_restore = None;
                self.set_summary(cx.tree, crate::strings::t("Восстановление отменено."))?;
                return Ok(());
            }
            if clicked.is_some_and(|id| Some(id) == self.cancel_restore) {
                self.pending_restore = None;
                let _ = cx.tree.close_dialog()?;
                self.set_summary(cx.tree, crate::strings::t("Восстановление отменено."))?;
                return Ok(());
            }
            if clicked.is_some_and(|id| Some(id) == self.confirm_restore) {
                if let Some((journal, source, backup, mode)) = self.pending_restore.take() {
                    let operation = GuardedHistoryOperation::Restore {
                        journal,
                        source,
                        backup,
                        mode,
                    };
                    if !self.start_process_check(cx, operation)? {
                        let _ = cx.tree.close_dialog()?;
                        self.set_summary(cx.tree, crate::strings::t("Не удалось проверить запущенную игру."))?;
                    }
                }
                return Ok(());
            }
            return Ok(());
        }
        if clicked.is_some() && clicked == self.refresh {
            if cx.proxy.is_none() {
                self.set_summary(
                    cx.tree,
                    crate::strings::t("В режиме headless screenshot диски не сканируются."),
                )?;
            } else {
                self.set_summary(cx.tree, crate::strings::t("Загрузка…"))?;
                self.request_refresh(cx.proxy.cloned())?;
            }
        }
        if clicked.is_some() && clicked == self.previous_page && self.page > 0 {
            self.page = self.page.saturating_sub(1);
            self.render_current_page(cx)?;
        }
        if clicked.is_some() && clicked == self.next_page {
            let rows = self.page_rows(cx.tree);
            let total_pages = self.backup_entries.as_ref().map_or_else(
                || {
                    self.save_entries
                        .as_ref()
                        .map_or(0, |entries| page_count(entries.len(), rows))
                },
                |entries| page_count(entries.len(), rows),
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
                Action::Restore {
                    journal,
                    source,
                    backup,
                } => {
                    self.open_restore_confirmation(cx, journal, source, backup, RestoreMode::Copy)?;
                }
                Action::RestoreInPlace {
                    journal,
                    source,
                    backup,
                } => {
                    if self.workspace.is_saving() {
                        self.set_summary(
                            cx.tree,
                            crate::strings::t("Дождитесь завершения сохранения, чтобы восстановить сейв на место."),
                        )?;
                    } else if has_pending_edits_for_selected_source(cx.app, &source) {
                        self.set_summary(
                            cx.tree,
                            crate::strings::t("Сначала сохраните или сбросьте черновик выбранного сейва."),
                        )?;
                    } else {
                        self.open_restore_confirmation(cx, journal, source, backup, RestoreMode::InPlace)?;
                    }
                }
                Action::Compare(path) => {
                    if let Some(current) = cx.app.current_save().map(Path::to_path_buf) {
                        if current == path {
                            self.set_summary(cx.tree, crate::strings::t("Выберите другой сейв этой игры."))?;
                        } else {
                            self.compare_selection = vec![current.clone(), path.clone()];
                            self.set_summary(cx.tree, crate::strings::t("Сравниваю…"))?;
                            self.start_compare(current, path, cx.proxy.cloned())?;
                        }
                    } else {
                        match self.compare_selection.first().cloned() {
                            Some(first) if first == path => {
                                self.set_summary(cx.tree, crate::strings::t("Выберите другой сейв этой игры."))?;
                            }
                            Some(first) => {
                                let entries = self.save_entries.as_deref().unwrap_or_default();
                                let first_family = entries.iter().find(|entry| entry.path == first).map(game_family);
                                let second_family = entries.iter().find(|entry| entry.path == path).map(game_family);
                                if first_family.is_some() && first_family == second_family {
                                    self.compare_selection = vec![first.clone(), path.clone()];
                                    self.set_summary(cx.tree, crate::strings::t("Сравниваю…"))?;
                                    self.start_compare(first, path, cx.proxy.cloned())?;
                                } else {
                                    self.set_summary(cx.tree, crate::strings::t("Это сейвы разных игр."))?;
                                }
                            }
                            None => {
                                self.compare_selection.push(path);
                                self.page = 0;
                                self.render_saves_page(cx)?;
                                self.set_summary(cx.tree, crate::strings::t("Выберите второй сейв этой игры."))?;
                            }
                        }
                    }
                }
                Action::Diagnose { path, format_id } => {
                    self.set_doctor_path(cx.tree, &path.to_string_lossy())?;
                    let file = display_name(&path);
                    let summary =
                        crate::strings::tr_in(Some(crate::strings::current_language()), "Проверяю {0}…", &[&file]);
                    self.set_summary(cx.tree, &summary)?;
                    self.start_diagnosis(path, format_id, cx.proxy.cloned())?;
                    self.sync_doctor_check(cx.tree)?;
                }
                #[cfg(feature = "native-ui")]
                Action::OpenGameFix { game_id, fix_id } => {
                    if let Some(proxy) = cx.proxy.cloned() {
                        let _ = proxy.send(AppMessage::OpenGameFix {
                            game_id: game_id.clone(),
                            fix_id: fix_id.clone(),
                        });
                        self.set_summary(cx.tree, crate::strings::t("Открываю связанное исправление игры…"))?;
                    } else {
                        self.set_summary(
                            cx.tree,
                            crate::strings::t("Исправление игры можно открыть только в рабочем окне."),
                        )?;
                    }
                }
                Action::RepairQuests {
                    path,
                    format_id,
                    expected_sha256,
                } => {
                    let Some(format_id) = format_id else {
                        self.set_summary(
                            cx.tree,
                            crate::strings::t("Не удалось определить формат сейва для проверки запущенной игры."),
                        )?;
                        return Ok(());
                    };
                    // The repair rewrites the save and reloads it, which drops unsaved edits of the same save.
                    if has_pending_edits_for_selected_source(cx.app, &path) {
                        self.set_summary(
                            cx.tree,
                            crate::strings::t("Сначала сохраните или сбросьте черновик выбранного сейва."),
                        )?;
                        return Ok(());
                    }
                    if !self.start_process_check(
                        cx,
                        GuardedHistoryOperation::QuestRepair {
                            path,
                            format_id,
                            expected_sha256,
                        },
                    )? {
                        let status = if cx.proxy.is_none() {
                            crate::strings::t("Ремонт доступен только в работающем окне.")
                        } else if self.workspace.is_restoring() {
                            crate::strings::t("Дождитесь завершения восстановления сейва.")
                        } else if self.workspace.is_saving() {
                            crate::strings::t("Запись сейва уже выполняется.")
                        } else {
                            crate::strings::t("Не удалось проверить запущенную игру.")
                        };
                        self.set_summary(cx.tree, status)?;
                    }
                }
            }
        }
        if let Message::User(AppMessage::ToScreen(target, payload)) = message {
            if *target == self.id {
                if payload.is::<()>() {
                    self.request_refresh(cx.proxy.cloned())?;
                }
                if let Some(payload) = payload.downcast_ref::<HistoryResult>() {
                    match payload {
                        HistoryResult::Backups(Ok(entries)) => self.render_backups(cx, entries.clone())?,
                        HistoryResult::Backups(Err(error)) => {
                            let error = truncate(error, 160);
                            let summary = crate::strings::tr_in(
                                Some(crate::strings::current_language()),
                                "Ошибка: {0}",
                                &[&error],
                            );
                            self.set_summary(cx.tree, &summary)?
                        }
                        HistoryResult::Saves(Ok(slots)) => self.render_saves(cx, slots.clone())?,
                        HistoryResult::Saves(Err(error)) => {
                            let error = truncate(error, 160);
                            let summary = crate::strings::tr_in(
                                Some(crate::strings::current_language()),
                                "Ошибка: {0}",
                                &[&error],
                            );
                            self.set_summary(cx.tree, &summary)?
                        }
                        HistoryResult::Compare(Ok(report)) => self.render_compare(cx, clone_report(report))?,
                        HistoryResult::Compare(Err(error)) => {
                            let error = if error == crate::strings::t_in("ru", "Это сейвы разных игр.")
                            {
                                crate::strings::t(error).to_owned()
                            } else {
                                truncate(error, 160)
                            };
                            let summary = crate::strings::tr_in(
                                Some(crate::strings::current_language()),
                                "Ошибка сравнения: {0}",
                                &[&error],
                            );
                            self.set_summary(cx.tree, &summary)?
                        }
                        HistoryResult::Diagnosis { request_id, result } if *request_id == self.diagnosis_request_id => {
                            self.diagnosis_running = false;
                            if let Ok((path, report)) = result {
                                self.diagnosed_selection = Some((path.clone(), report.source_sha256.clone()));
                            }
                            self.sync_doctor_check(cx.tree)?;
                            match result {
                                Ok((path, report)) => self.render_diagnosis(cx, path, report.clone())?,
                                Err(error) => self.render_diagnosis_error(cx.tree, error)?,
                            }
                        }
                        HistoryResult::Diagnosis { .. } => {}
                        HistoryResult::ProcessCheck { .. } => {}
                        HistoryResult::QuestRepaired { request_id, result } => {
                            if !self.workspace.session().is_latest_operation(*request_id) {
                                return Ok(());
                            }
                            match result {
                                Ok(completion) => {
                                    let path = completion.path.clone();
                                    self.render_quest_repair(cx, completion.clone())?;
                                    if let Some(proxy) = cx.proxy.as_ref() {
                                        let reload_path =
                                            (cx.app.current_save() == Some(path.as_path())).then_some(path);
                                        let _ = proxy.send(AppMessage::ToScreen(
                                            ScreenId::Overview,
                                            Box::new(RefreshOverview { path: reload_path }),
                                        ));
                                    }
                                }
                                Err(error) => {
                                    if is_windows_file_busy_error_text(error) {
                                        self.set_summary(cx.tree, SAVE_WHILE_GAME_RUNNING_WARNING)?;
                                    } else {
                                        self.set_summary(
                                            cx.tree,
                                            &crate::strings::tr_in(
                                                Some(crate::strings::current_language()),
                                                "Не удалось сохранить: {0}",
                                                &[&truncate(error, 160)],
                                            ),
                                        )?;
                                    }
                                }
                            }
                        }
                        HistoryResult::Restored { result } => {
                            // A result that is no longer the latest operation still shows its outcome, so a
                            // failed or finished restore is never hidden by a newer operation.
                            match result {
                                Ok(RestoredSave::Copy(path)) => {
                                    let path = path.display().to_string();
                                    let summary = crate::strings::tr_in(
                                        Some(crate::strings::current_language()),
                                        "Копия восстановлена в {0}",
                                        &[&path],
                                    );
                                    self.set_summary(cx.tree, &summary)?;
                                    self.request_refresh(cx.proxy.cloned())?;
                                    if let Some(proxy) = cx.proxy.as_ref() {
                                        let _ = proxy.send(AppMessage::ToScreen(
                                            ScreenId::Overview,
                                            Box::new(RefreshOverview { path: None }),
                                        ));
                                    }
                                }
                                Ok(RestoredSave::InPlace(receipt)) => {
                                    let backup = receipt.safety_backup_path.as_ref().map_or_else(
                                        || {
                                            crate::strings::t("без страховочного бэкапа (исходного файла не было)")
                                                .to_owned()
                                        },
                                        |path| {
                                            crate::strings::tr_in(
                                                Some(crate::strings::current_language()),
                                                "страховочный бэкап: {0}",
                                                &[&path.display()],
                                            )
                                        },
                                    );
                                    let save_path = receipt.save_path.display().to_string();
                                    self.set_summary(
                                        cx.tree,
                                        &crate::strings::tr_in(
                                            Some(crate::strings::current_language()),
                                            "Восстановлено на место: {0} · {1}",
                                            &[&save_path, &backup],
                                        ),
                                    )?;
                                    self.request_refresh(cx.proxy.cloned())?;
                                    if let Some(proxy) = cx.proxy.as_ref() {
                                        let reload_path = (cx.app.current_save() == Some(receipt.save_path.as_path()))
                                            .then(|| receipt.save_path.clone());
                                        let _ = proxy.send(AppMessage::ToScreen(
                                            ScreenId::Overview,
                                            Box::new(RefreshOverview { path: reload_path }),
                                        ));
                                    }
                                }
                                Err(error) => {
                                    let text = if is_windows_file_busy_error_text(error) {
                                        SAVE_WHILE_GAME_RUNNING_WARNING.to_owned()
                                    } else {
                                        crate::strings::tr_in(
                                            Some(crate::strings::current_language()),
                                            "Не восстановлено: {0}",
                                            &[&truncate(error, 160)],
                                        )
                                    };
                                    self.set_summary(cx.tree, &text)?;
                                }
                            }
                        }
                    }
                }
            }
        }
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
        if self.id == ScreenId::Backups {
            self.request_refresh(cx.proxy.cloned())?;
        }
        if self.id == ScreenId::Compare {
            let selected = cx.app.current_save().map(Path::to_path_buf);
            if self.compare_selection.first().cloned() != selected {
                self.compare_selection = selected.into_iter().collect();
            }
        } else if self.id == ScreenId::SaveDoctor {
            let selected = cx
                .app
                .current_save()
                .zip(cx.app.current_save_sha256())
                .map(|(path, sha256)| (path.to_path_buf(), sha256.to_owned()));
            if selected != self.diagnosed_selection {
                if let Some((path, source_sha256)) = selected {
                    let format_id = cx.app.current_save_format().map(str::to_owned);
                    self.set_doctor_path(cx.tree, &path.to_string_lossy())?;
                    let file = display_name(&path);
                    let summary =
                        crate::strings::tr_in(Some(crate::strings::current_language()), "Проверяю {0}…", &[&file]);
                    self.set_summary(cx.tree, &summary)?;
                    if self.start_diagnosis(path.clone(), format_id, cx.proxy.cloned())? {
                        self.diagnosed_selection = Some((path, source_sha256));
                    }
                    self.sync_doctor_check(cx.tree)?;
                } else if self.diagnosed_selection.is_some() || self.diagnosis_running {
                    self.diagnosis_request_id = self.diagnosis_request_id.saturating_add(1);
                    self.diagnosed_selection = None;
                    self.diagnosis_running = false;
                    self.clear_results(cx.tree)?;
                    self.set_summary(cx.tree, crate::strings::t("ВЫБЕРИТЕ ФАЙЛ СОХРАНЕНИЯ ДЛЯ ПРОВЕРКИ."))?;
                    self.sync_doctor_check(cx.tree)?;
                }
            }
        }
        self.sync_side(cx.tree)
    }

    fn build(&mut self, cx: &mut Context<'_>, host: WidgetId) -> Result<()> {
        // The save doctor has its file controls above the results; every screen of this family then has the list layout.
        if self.id == ScreenId::SaveDoctor {
            self.build_doctor_controls(cx, host)?;
        }
        self.build_list_layout(cx, host)
    }

    fn message(
        &mut self,
        cx: &mut Context<'_>,
        message: &Message<AppMessage>,
        clicked: Option<WidgetId>,
    ) -> Result<()> {
        // A row's select button only chooses the entry; the side panel's buttons act for the chosen one.
        if let Some(index) = clicked.and_then(|id| self.rows.iter().position(|row| row.select == id)) {
            self.selected_row = Some(index);
            return self.sync_side(cx.tree);
        }
        let clicked = self.side_click(clicked);
        self.message_inner(cx, message, clicked)?;
        self.sync_side(cx.tree)
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
    let discovery_options = super::save_directory_discovery_options();
    let candidates = SaveDirectoryLocator::find_candidate_directories(Some(&discovery_options));
    let result = SaveSlotDiscovery::discover(&candidates);
    for failure in &result.worker_failures {
        sse_app::diagnostics::warn(failure);
    }
    Ok(result.slots)
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

pub(super) fn format_system_time(value: SystemTime) -> String {
    let Ok(duration) = value.duration_since(UNIX_EPOCH) else {
        return crate::strings::t("дата неизвестна").to_owned();
    };
    let seconds = duration.as_secs();
    let days = seconds / 86_400;
    let seconds_of_day = seconds % 86_400;
    let Ok(days) = i64::try_from(days) else {
        return crate::strings::t("дата вне диапазона").to_owned();
    };
    let Some(serial_day) = days.checked_add(719_468) else {
        return crate::strings::t("дата вне диапазона").to_owned();
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
        return Err(crate::strings::t_in("ru", "Это сейвы разных игр.").to_owned());
    }

    let mut differences = Vec::new();
    if first_snapshot.money != second_snapshot.money {
        differences.push(SemanticDifference {
            kind: DifferenceKind::Changed,
            label: crate::strings::t_in("ru", "Деньги").to_owned(),
            value_a: first_snapshot.money.to_string(),
            value_b: second_snapshot.money.to_string(),
            category: crate::strings::t_in("ru", "Персонаж"),
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
            value_a: old.map_or_else(|| crate::strings::t_in("ru", "нет").to_owned(), item_quantity_text),
            value_b: new.map_or_else(|| crate::strings::t_in("ru", "нет").to_owned(), item_quantity_text),
            category: crate::strings::t_in("ru", "Предметы"),
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
            money: save.money().map_err(|error| error.to_string())?,
            items: BTreeMap::new(),
        };
        for item in save.items() {
            let key = format!(
                "{:02x}{:02x}{:02x}",
                item.type_key[0], item.type_key[1], item.type_key[2]
            );
            let label = item
                .display_name
                .unwrap_or_else(|| crate::strings::tr_in(Some("ru"), "Предмет {0}", &[&key]));
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

fn localized_comparison_value(value: &str, language: &str) -> String {
    if value == crate::strings::t_in("ru", "нет") {
        return crate::strings::t_in(language, value).to_owned();
    }
    if let Some(count) = value.strip_suffix(crate::strings::t_in("ru", " объектов · количество неизвестно"))
    {
        return crate::strings::tr_in(Some(language), "{0} объектов · количество неизвестно", &[&count]);
    }
    value.to_owned()
}

fn item_quantity_text(item: &ItemQuantity) -> String {
    if item.all_counts_known {
        format!("×{}", item.count)
    } else {
        crate::strings::tr_in(
            Some("ru"),
            "{0} объектов · количество неизвестно",
            &[&item.object_count],
        )
    }
}

fn difference_kind_text(kind: DifferenceKind) -> &'static str {
    match kind {
        DifferenceKind::Added => crate::strings::t_in("ru", "Добавлено"),
        DifferenceKind::Removed => crate::strings::t_in("ru", "Удалено"),
        DifferenceKind::Changed => crate::strings::t_in("ru", "Изменено"),
    }
}

fn diagnose_save(path: &Path, format_id: Option<&str>) -> std::result::Result<(PathBuf, DiagnosisReport), String> {
    let packed = SaveBuffer::read(path).map_err(|error| error.to_string())?;
    diagnose_packed(packed.as_slice(), format_id).map(|report| (path.to_path_buf(), report))
}

fn diagnose_packed(packed: &[u8], format_id: Option<&str>) -> std::result::Result<DiagnosisReport, String> {
    let source_sha256 = sse_codecs::sha256::sha256_hex(packed);
    if format_id.is_none() || format_id == Some("stalker2") {
        match sse_s2::S2Save::from_bytes(packed) {
            Ok(save) => {
                let container = save.container();
                let items = save.items();
                let stored_crc = format!("{:08X}", container.stored_crc32());
                let computed_crc = format!("{:08X}", container.computed_crc32());
                let money = save
                    .money()
                    .map_or_else(|error| error.to_string(), |value| value.to_string());
                let item_count = items.len();
                let stash = crate::strings::t(if save.stash().is_ok() {
                    "найден"
                } else {
                    "не найден"
                });
                let warning_count = save.warnings().len();
                let unresolved_count = save.unresolved_handles().len();
                let summary = crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "S2: {0} байт · CRC {1}/{2} · деньги {3} · предметов {4} · тайник {5} · предупреждений {6} · неразрешённых ссылок {7}. Правила Quest Doctor для S2 недоступны.",
                    &[
                        &packed.len(),
                        &stored_crc,
                        &computed_crc,
                        &money,
                        &item_count,
                        &stash,
                        &warning_count,
                        &unresolved_count,
                    ],
                );
                return Ok(DiagnosisReport {
                    summary,
                    source_sha256,
                    format_id: Some("stalker2".to_owned()),
                    quest_states: Vec::new(),
                    can_repair_quests: false,
                });
            }
            Err(error) if format_id == Some("stalker2") => return Err(error.to_string()),
            Err(_) => {}
        }
    }
    if format_id.is_none() || format_id.is_some_and(|id| id.starts_with("stalker-")) {
        match sse_xray::Save::read(packed) {
            Ok(save) => {
                let inventory = save.inventory().map_err(|error| error.to_string())?;
                let quests = sse_doctor::analyze_quests_from_save(&save);
                let broken = quests
                    .states
                    .iter()
                    .filter(|state| state.status == sse_doctor::QuestTaskStatus::Broken)
                    .count();
                let quest_summary = if !quests.quest_states_available {
                    crate::strings::t("Нет проверенных правил квестов для этого формата.").to_owned()
                } else if broken > 0 {
                    crate::strings::tr_in(
                        Some(crate::strings::current_language()),
                        "Подтверждённо сломанных квестов: {0}; доступен ремонт.",
                        &[&broken],
                    )
                } else {
                    crate::strings::t("Подтверждённых сломанных квестов не найдено.").to_owned()
                };
                let format_id = save.format().id();
                let packed_bytes = packed.len();
                let registry_count = save.registry_objects().len();
                let inventory_count = inventory.len();
                let money = save.money().map_err(|error| error.to_string())?;
                let game_time = save.game_time();
                let summary = crate::strings::tr_in(
                    Some(crate::strings::current_language()),
                    "X-Ray {0}: упакованный файл {1} байт · реестр {2} · предметов инвентаря {3} · деньги {4} · игровой тик {5}. Структура прочитана. {6}",
                    &[
                        &format_id,
                        &packed_bytes,
                        &registry_count,
                        &inventory_count,
                        &money,
                        &game_time,
                        &quest_summary,
                    ],
                );
                return Ok(DiagnosisReport {
                    summary,
                    source_sha256,
                    format_id: Some(save.format().id().to_owned()),
                    quest_states: quests.states,
                    can_repair_quests: broken > 0,
                });
            }
            Err(error) if format_id.is_some() => return Err(error.to_string()),
            Err(_) => {}
        }
    }
    Err(crate::strings::t("формат не распознан; структурная проверка не запускалась").to_owned())
}

fn repair_quest_save(
    path: &Path,
    expected_source_sha256: &str,
    backup_directory: &Path,
) -> std::result::Result<(PathBuf, Option<String>), String> {
    let original = SaveBuffer::read(path).map_err(|error| error.to_string())?;
    let actual_sha256 = sse_codecs::sha256::sha256_hex(original.as_slice());
    if actual_sha256 != expected_source_sha256 {
        return Err(crate::strings::t("Файл изменился после проверки. Проверьте его ещё раз.").to_owned());
    }
    let Some(prepared) = sse_doctor::prepare_quest_repair(original.as_slice()).map_err(|error| error.to_string())?
    else {
        return Err(crate::strings::t("НЕТ ПОДТВЕРЖДЁННЫХ СЛОМАННЫХ КВЕСТОВ.").to_owned());
    };
    let preflight_image = prepared.clone();
    let request =
        transaction::ReplacementRequest::new(path, expected_source_sha256, prepared.as_slice(), backup_directory);
    let (receipt, (), ()) = transaction::replace_transaction(
        &transaction::StdFileSystem,
        request,
        move |_, replacement| {
            if replacement != preflight_image.as_slice() {
                return Err(Error::damaged(
                    "prepared quest repair changed before transaction preflight",
                ));
            }
            sse_doctor::verify_quest_repair(replacement)
        },
        sse_doctor::verify_quest_repair,
    )
    .map_err(|error| error.to_string())?;
    Ok((receipt.backup_path, receipt.maintenance_warning))
}

fn quest_title(id: &str) -> &'static str {
    match id {
        "cs.wild-napr-dead" => crate::strings::t("КВЕСТ: ЗАДАНИЯ НАПРА НА БАРАХОЛКЕ ПОСЛЕ ЕГО СМЕРТИ"),
        "cs.wolf-dead" => crate::strings::t("КВЕСТ: ЗАДАНИЯ ВОЛКА ПОСЛЕ ЕГО СМЕРТИ"),
        "cs.hog-dead" => crate::strings::t("КВЕСТ: СЮЖЕТ НА АРМЕЙСКИХ СКЛАДАХ ПОСЛЕ СМЕРТИ КАБАНА"),
        "soc.mole-dead" => crate::strings::t("КВЕСТ: ВСТРЕЧА С КРОТОМ НА АГРОПРОМЕ ПОСЛЕ ЕГО СМЕРТИ"),
        "soc.prisoner-dead" => crate::strings::t("КВЕСТ: ПЛЕННЫЙ ДОЛГОВЕЦ В ТЁМНОЙ ДОЛИНЕ ПОСЛЕ ЕГО СМЕРТИ"),
        "soc.courier-dead" => crate::strings::t("КВЕСТ: КУРЬЕР СВОБОДЫ ПОСЛЕ ЕГО СМЕРТИ"),
        "soc.informer-dead" => crate::strings::t("КВЕСТ: ИНФОРМАТОР СВОБОДЫ ПОСЛЕ ЕГО СМЕРТИ"),
        _ => crate::strings::t("КВЕСТ"),
    }
}

fn quest_detail(state: &sse_doctor::QuestTaskState) -> String {
    let language = crate::strings::current_language();
    let detail = match (state.status, state.reason) {
        (sse_doctor::QuestTaskStatus::Ok, "alive") => crate::strings::t_in(language, "✓ NPC ЖИВ.").to_owned(),
        (sse_doctor::QuestTaskStatus::Ok, _) => crate::strings::t_in(language, "✓ ФЛАГ УЖЕ ВЫДАН.").to_owned(),
        (sse_doctor::QuestTaskStatus::Broken, _) => {
            let missing_info = state
                .missing_info
                .unwrap_or(crate::strings::t_in(language, "неизвестный"));
            crate::strings::tr_in(
                Some(language),
                "⚠ NPC МЁРТВ, НО ФЛАГ {0} НЕ ВЫДАН: ЗАДАНИЕ ЗАВИСНЕТ.",
                &[&missing_info],
            )
        }
        (sse_doctor::QuestTaskStatus::Unknown, "too-late") => crate::strings::t_in(
            language,
            "× NPC МЁРТВ БЕЗ ФЛАГА, НО СЮЖЕТ УЖЕ ПОШЁЛ ДАЛЬШЕ: ДОБАВЛЕНИЕ ФЛАГА НЕ ПОМОЖЕТ. НУЖЕН БОЛЕЕ РАННИЙ СЕЙВ.",
        )
        .to_owned(),
        (sse_doctor::QuestTaskStatus::Unknown, _) => {
            crate::strings::t_in(language, "? NPC НЕ НАЙДЕН ИЛИ НЕ ЧИТАЕТСЯ; СОСТОЯНИЕ НЕИЗВЕСТНО.").to_owned()
        }
    };
    if state.needs_preventing_fix {
        if let Some(fix_id) = state.preventing_fix_id {
            return crate::strings::tr_in(
                Some(language),
                "{0} ЧТОБЫ ИГРА УВИДЕЛА ФЛАГ, УСТАНОВИТЕ ИСПРАВЛЕНИЕ ИГРЫ {1}.",
                &[&detail, &fix_id],
            );
        }
    }
    detail
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

fn page_count(total: usize, rows: usize) -> usize {
    let rows = rows.max(1);
    total
        .saturating_add(rows.saturating_sub(1))
        .checked_div(rows)
        .unwrap_or(0)
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

fn doctor_title() -> &'static str {
    crate::strings::t("ДОКТОР СОХРАНЕНИЯ")
}

fn doctor_description() -> &'static str {
    crate::strings::t(
        "Проверка читаемости формата. Семантические правила и ремонт доступны только при наличии проверенных данных.\nИСПРАВИТЬ КВЕСТЫ использует общий путь записи с бэкапом и обратным чтением.",
    )
}

fn doctor_error_detail(error: &str) -> String {
    let language = crate::strings::current_language();
    if let Some(detail) = error.strip_prefix(crate::strings::t_in(language, "формат не распознан;")) {
        let detail = truncate(detail.trim(), 120);
        crate::strings::tr_in(
            Some(language),
            "× СТРУКТУРНАЯ ПРОВЕРКА — ФАЙЛ НЕ РАСПОЗНАН ИЛИ ПОВРЕЖДЁН: {0}",
            &[&detail],
        )
    } else {
        let detail = truncate(error, 160);
        crate::strings::tr_in(Some(language), "× СТРУКТУРНАЯ ПРОВЕРКА — {0}", &[&detail])
    }
}

fn manual_save_path(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    (!value.is_empty()).then(|| PathBuf::from(value))
}

fn doctor_path_edit_config() -> EditConfig {
    EditConfig {
        mode: FieldMode::SingleLine,
        max_graphemes: 4096,
        history_limit: 32,
        filter: InputFilter::Any,
    }
}

#[derive(Default)]
struct HistoryClipboard(String);

impl Clipboard for HistoryClipboard {
    fn read_text(&mut self) -> Result<String> {
        Ok(self.0.clone())
    }

    fn write_text(&mut self, text: &str) -> Result<()> {
        text.clone_into(&mut self.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        compare_packed, diagnose_packed, format_system_time, page_count, quest_detail, restored_output_path_at,
        timeline_order, truncate, Action, ActionButton, DiagnosisReport, HistoryScreen, Workspace,
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

    #[test]
    fn history_screen_titles_have_english_and_ukrainian_translations() {
        let titles = [
            (
                "Резервные копии и восстановление",
                "Backups and recovery",
                "Резервні копії та відновлення",
            ),
            (
                "Сравнение двух сохранений",
                "Compare two saves",
                "Порівняння двох збережень",
            ),
            (
                "История экспортов и резервных копий",
                "Export and backup history",
                "Історія експорту й резервних копій",
            ),
            (
                "Проверка структуры и целостности",
                "Structure and integrity check",
                "Перевірка структури та цілісності",
            ),
        ];

        for (key, english, ukrainian) in titles {
            assert_eq!(crate::strings::t_in("en", key), english, "English: {key}");
            assert_eq!(crate::strings::t_in("uk", key), ukrainian, "Ukrainian: {key}");
        }
    }

    #[test]
    fn backup_page_summary_translates_each_count_and_page_number() {
        let service = sse_catalog::I18nService::instance();
        let args: [&dyn std::fmt::Display; 4] = [&7, &5, &1, &2];

        assert_eq!(
            service.tr_in(
                Some("en"),
                "Записей: {0} · восстановимо: {1} · страница {2} из {3}",
                &args,
            ),
            "Entries: 7 · restorable: 5 · page 1 of 2"
        );
        assert_eq!(
            service.tr_in(
                Some("uk"),
                "Записей: {0} · восстановимо: {1} · страница {2} из {3}",
                &args,
            ),
            "Записів: 7 · можна відновити: 5 · сторінка 1 з 2"
        );
    }

    #[test]
    fn history_screen_controls_have_english_and_ukrainian_translations() {
        let labels = [
            (
                "БЭКАПЫ И ВОССТАНОВЛЕНИЕ",
                "BACKUPS AND RECOVERY",
                "РЕЗЕРВНІ КОПІЇ ТА ВІДНОВЛЕННЯ",
            ),
            ("СРАВНЕНИЕ СОХРАНЕНИЙ", "SAVE COMPARISON", "ПОРІВНЯННЯ ЗБЕРЕЖЕНЬ"),
            ("ИСТОРИЯ СОХРАНЕНИЙ", "SAVE HISTORY", "ІСТОРІЯ ЗБЕРЕЖЕНЬ"),
            ("ДОКТОР СОХРАНЕНИЯ", "SAVE DOCTOR", "ДІАГНОСТИКА ЗБЕРЕЖЕННЯ"),
            ("Файл", "File", "Файл"),
            ("Найти сейвы", "Find saves", "Знайти збереження"),
            ("Список сейвов", "Save list", "Список збережень"),
            ("Обновить историю", "Refresh history", "Оновити історію"),
            (
                "Нажмите кнопку, чтобы прочитать локальные данные.",
                "Click the button to read local data.",
                "Натисніть кнопку, щоб прочитати локальні дані.",
            ),
            ("Назад", "Back", "Назад"),
            ("Дальше", "Next", "Далі"),
        ];

        for (key, english, ukrainian) in labels {
            assert_eq!(crate::strings::t_in("en", key), english, "English: {key}");
            assert_eq!(crate::strings::t_in("uk", key), ukrainian, "Ukrainian: {key}");
        }
    }

    #[test]
    fn history_operation_statuses_and_errors_are_translated() {
        let statuses = [
            ("Операция отменена.", "Operation cancelled.", "Операцію скасовано."),
            (
                "Восстановление отменено.",
                "Restore cancelled.",
                "Відновлення скасовано.",
            ),
            ("Загрузка…", "Loading…", "Завантаження…"),
            (
                "Не удалось проверить запущенную игру.",
                "Could not check whether the game is running.",
                "Не вдалося перевірити, чи запущена гра.",
            ),
        ];
        for (key, english, ukrainian) in statuses {
            assert_eq!(crate::strings::t_in("en", key), english, "English: {key}");
            assert_eq!(crate::strings::t_in("uk", key), ukrainian, "Ukrainian: {key}");
        }

        let service = sse_catalog::I18nService::instance();
        let detail = "permission denied";
        assert_eq!(
            service.tr_in(Some("en"), "Ошибка: {0}", &[&detail]),
            "Error: permission denied"
        );
        assert_eq!(
            service.tr_in(Some("uk"), "Ошибка: {0}", &[&detail]),
            "Помилка: permission denied"
        );
    }

    #[test]
    fn quest_repair_status_keeps_the_backup_name_when_translated() {
        let service = sse_catalog::I18nService::instance();
        let backup_name = "slot.sav.bak";
        assert_eq!(
            service.tr_in(Some("en"), "КВЕСТЫ ИСПРАВЛЕНЫ. Резервная копия: {0}", &[&backup_name],),
            "QUESTS REPAIRED. Backup: slot.sav.bak"
        );
        assert_eq!(
            service.tr_in(Some("uk"), "КВЕСТЫ ИСПРАВЛЕНЫ. Резервная копия: {0}", &[&backup_name],),
            "КВЕСТИ ВІДНОВЛЕНО. Резервна копія: slot.sav.bak"
        );
    }

    #[test]
    fn diagnostic_templates_translate_statuses_and_preserve_values() {
        for (key, english, ukrainian) in [
            ("дата неизвестна", "date unknown", "дату не визначено"),
            ("дата вне диапазона", "date out of range", "дата поза діапазоном"),
            (
                "Нет проверенных правил квестов для этого формата.",
                "No verified quest rules are available for this format.",
                "Для цього формату немає перевірених правил квестів.",
            ),
            (
                "Подтверждённых сломанных квестов не найдено.",
                "No confirmed broken quests were found.",
                "Підтверджених зламаних квестів не знайдено.",
            ),
        ] {
            assert_eq!(crate::strings::t_in("en", key), english, "English: {key}");
            assert_eq!(crate::strings::t_in("uk", key), ukrainian, "Ukrainian: {key}");
        }

        let service = sse_catalog::I18nService::instance();
        let args: [&dyn std::fmt::Display; 8] = [&12, &"000000AB", &"000000CD", &500, &9, &"found", &2, &1];
        assert_eq!(
            service.tr_in(
                Some("en"),
                "S2: {0} байт · CRC {1}/{2} · деньги {3} · предметов {4} · тайник {5} · предупреждений {6} · неразрешённых ссылок {7}. Правила Quest Doctor для S2 недоступны.",
                &args,
            ),
            "S2: 12 bytes · CRC 000000AB/000000CD · money 500 · items 9 · stash found · warnings 2 · unresolved links 1. Quest Doctor rules for S2 are unavailable."
        );

        let flag = "esc_wolf_dead";
        assert_eq!(
            service.tr_in(
                Some("en"),
                "⚠ NPC МЁРТВ, НО ФЛАГ {0} НЕ ВЫДАН: ЗАДАНИЕ ЗАВИСНЕТ.",
                &[&flag],
            ),
            "⚠ NPC IS DEAD, BUT FLAG esc_wolf_dead WAS NOT SET: THE QUEST WILL STALL."
        );
        assert_eq!(
            service.tr_in(
                Some("uk"),
                "⚠ NPC МЁРТВ, НО ФЛАГ {0} НЕ ВЫДАН: ЗАДАНИЕ ЗАВИСНЕТ.",
                &[&flag],
            ),
            "⚠ NPC МЕРТВИЙ, АЛЕ ПРАПОРЕЦЬ esc_wolf_dead НЕ ВСТАНОВЛЕНО: КВЕСТ ЗАВИСНЕ."
        );
    }

    #[test]
    fn comparison_values_are_localized_without_changing_report_data() {
        assert_eq!(super::localized_comparison_value("нет", "en"), "none");
        assert_eq!(super::localized_comparison_value("нет", "uk"), "немає");
        assert_eq!(
            super::localized_comparison_value("3 объектов · количество неизвестно", "en"),
            "3 objects · quantity unknown"
        );
        assert_eq!(
            super::localized_comparison_value("3 объектов · количество неизвестно", "uk"),
            "3 об’єктів · кількість невідома"
        );
    }

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
    fn doctor_screen_uses_acceptance_heading_and_describes_repair() {
        let language = crate::strings::current_language();
        assert_eq!(
            super::doctor_title(),
            crate::strings::t_in(language, "ДОКТОР СОХРАНЕНИЯ")
        );
        let description = super::doctor_description();
        assert!(description.contains(crate::strings::t_in(language, "ИСПРАВИТЬ КВЕСТЫ")));
        assert!(description.contains('\n'));
    }

    #[test]
    fn doctor_open_save_button_requests_the_shared_picker_and_returns_to_doctor() -> sse_core::Result<()> {
        let mut screen = HistoryScreen::new(ScreenId::SaveDoctor, "test", Workspace::default());
        let mut app = sse_app::state::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let (proxy, receiver) = crate::event_loop::channel_pair::<AppMessage>();
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let open = screen
            .doctor_open_save
            .ok_or_else(|| sse_core::Error::damaged("doctor open-save button was not built"))?;

        screen.message(&mut cx, &Message::User(AppMessage::Tick(0)), Some(open))?;

        assert!(matches!(
            receiver.recv_timeout(Duration::from_secs(1)),
            Ok(Message::User(AppMessage::OpenSavePicker {
                return_to: ScreenId::SaveDoctor
            }))
        ));
        Ok(())
    }

    #[test]
    fn doctor_error_uses_acceptance_structure_row() {
        let language = crate::strings::current_language();
        let plain = super::doctor_error_detail("file not found");
        assert!(plain.starts_with('×'));
        assert!(plain.ends_with("file not found"));
        let unrecognized = sse_catalog::I18nService::instance().tr_in(
            Some(language),
            "формат не распознан; структурная проверка не запускалась",
            &[],
        );
        let detail = super::doctor_error_detail(&unrecognized);
        assert!(detail.starts_with('×'));
        assert!(
            detail.contains("structural check was not run") || detail.contains("структурная проверка не запускалась")
        );
        assert!(super::doctor_error_detail(&"x".repeat(200)).chars().count() <= 190);
    }

    #[test]
    fn manual_save_path_rejects_blank_input_and_trims_whitespace() {
        assert_eq!(super::manual_save_path("  	 "), None);
        assert_eq!(
            super::manual_save_path("  /tmp/slot.sav  "),
            Some(PathBuf::from("/tmp/slot.sav"))
        );
    }

    #[test]
    fn quest_detail_shows_the_preventing_game_fix() {
        let state = sse_doctor::QuestTaskState {
            id: "cs.wolf-dead",
            title: "Wolf's tasks after his death",
            status: sse_doctor::QuestTaskStatus::Broken,
            reason: "dead-without-flag",
            missing_info: Some("esc_wolf_dead"),
            preventing_fix_id: Some("cs.quest.wolf-offline-cancellation"),
            needs_preventing_fix: true,
            detail: "NPC is dead, but the flag is absent.",
            references: &[],
        };
        assert!(quest_detail(&state).ends_with("cs.quest.wolf-offline-cancellation."));
    }

    #[test]
    fn manual_doctor_path_runs_and_applies_a_background_check() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("manual-fixture.sav");
        fs::write(&path, SYNTHETIC_XRAY_SAVE)?;
        let workspace = Workspace::default();
        let mut screen = HistoryScreen::new(ScreenId::SaveDoctor, "test", workspace);
        let mut app = sse_app::state::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let (proxy, receiver) = crate::event_loop::channel_pair::<AppMessage>();
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        let path_input = screen
            .doctor_path_input
            .ok_or_else(|| sse_core::Error::damaged("doctor path input was not built"))?;
        let check = screen
            .doctor_check
            .ok_or_else(|| sse_core::Error::damaged("doctor check button was not built"))?;
        cx.tree.set_focus(Some(path_input))?;
        for character in path.to_string_lossy().chars() {
            screen.message(
                &mut cx,
                &Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym: u32::from(character),
                    text: Some(character),
                    ctrl: false,
                    shift: false,
                }),
                None,
            )?;
        }
        for keysym in [0x61, 0x63, 0x76] {
            screen.message(
                &mut cx,
                &Message::Window(crate::event_loop::WindowEvent::Key {
                    pressed: true,
                    keysym,
                    text: None,
                    ctrl: true,
                    shift: false,
                }),
                None,
            )?;
        }
        assert_eq!(cx.tree.input_text(path_input)?, path.to_string_lossy());
        cx.tree.set_focus(Some(check))?;
        screen.message(&mut cx, &Message::User(AppMessage::Tick(0)), Some(check))?;
        assert!(screen.diagnosis_running);

        let message = receiver
            .recv_timeout(Duration::from_secs(2))
            .map_err(|error| sse_core::Error::damaged(format!("doctor worker did not reply: {error}")))?;
        if let Message::User(AppMessage::ToScreen(_, payload)) = &message {
            if let Some(super::HistoryResult::Diagnosis { result, .. }) = payload.downcast_ref::<super::HistoryResult>()
            {
                assert!(result.is_ok(), "manual path diagnosis failed: {result:?}");
            }
        }
        screen.message(&mut cx, &message, None)?;
        assert!(!screen.diagnosis_running);
        assert_eq!(
            screen.diagnosed_selection.as_ref().map(|(selected, _)| selected),
            Some(&path)
        );
        Ok(())
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
    fn quest_repair_is_blocked_for_the_selected_save_with_pending_draft_edits() -> sse_core::Result<()> {
        let source = PathBuf::from("/saves/slot.sav");
        let source_sha256 = "cd".repeat(32);
        let mut app = sse_app::AppState::new();
        app.set_current_save_identity(source.clone(), source_sha256.clone());
        let mut draft = sse_storage::drafts::DraftPlan::empty(&source_sha256)?;
        draft.money = Some(42);
        app.set_draft(draft);

        // The repair checks the same predicate as the in-place restore before it starts.
        assert!(super::has_pending_edits_for_selected_source(&app, &source));
        app.discard_draft(&source_sha256);
        assert!(!super::has_pending_edits_for_selected_source(&app, &source));
        Ok(())
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
    fn in_place_restore_does_not_start_while_a_save_is_active() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let path = PathBuf::from("fixture.sav");
        let save_guard = workspace
            .session()
            .begin_save(&path)
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        let screen = HistoryScreen::new(ScreenId::Backups, "test", workspace.clone());
        let (proxy, _receiver) = crate::event_loop::channel_pair::<AppMessage>();

        assert!(!screen.start_restore_write(
            PathBuf::from("fixture-journal.json"),
            path,
            super::RestoreMode::InPlace,
            Some(proxy),
        )?);
        assert!(!workspace.is_restoring());
        drop(save_guard);
        Ok(())
    }

    #[test]
    fn shared_backup_directory_defaults_under_application_data_directory() {
        let settings = sse_app::AppSettings::default();
        assert_eq!(
            sse_app::paths::backup_directory(&settings),
            sse_app::paths::default_data_directory().join("backups")
        );
    }

    #[test]
    fn list_pages_cover_all_entries_without_an_empty_tail_page() {
        assert_eq!(page_count(0, 12), 0);
        assert_eq!(page_count(12, 12), 1);
        assert_eq!(page_count(13, 12), 2);
        assert_eq!(page_count(24, 12), 2);
        assert_eq!(page_count(13, 7), 2);
    }

    #[test]
    fn doctor_identifies_synthetic_xray_save() {
        let report = diagnose_packed(SYNTHETIC_XRAY_SAVE, None)
            .unwrap_or_else(|error| panic!("diagnose X-Ray fixture: {error}"));
        assert!(report.summary.starts_with("X-Ray stalker-soc-ee:"));
        assert!(
            report.summary.contains("Structure read successfully") || report.summary.contains("Структура прочитана")
        );
        assert!(!report.can_repair_quests);
        assert!(report.quest_states.is_empty());
    }

    #[test]
    fn doctor_refuses_to_guess_a_save_format() {
        assert!(diagnose_packed(b"not a supported save", None).is_err());
    }

    #[test]
    fn doctor_identifies_synthetic_s2_save_and_reports_crc() {
        let report =
            diagnose_packed(SYNTHETIC_S2_SAVE, None).unwrap_or_else(|error| panic!("diagnose S2 fixture: {error}"));
        assert!(report.summary.starts_with("S2:"));
        assert!(report.summary.contains("CRC"));
        assert!(report.summary.contains("Quest Doctor"));
        assert!(report.summary.contains("S2"));
        assert!(!report.can_repair_quests);
        assert!(report.quest_states.is_empty());
    }

    #[test]
    fn doctor_refuses_repair_without_a_proven_broken_quest_and_leaves_the_save_untouched() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let path = temp.0.join("unknown-quest-state.sav");
        let original = SYNTHETIC_XRAY_SAVE;
        fs::write(&path, original)?;
        let expected_sha256 = sse_codecs::sha256::sha256_hex(original);

        let result = super::repair_quest_save(&path, &expected_sha256, &temp.0.join("backups"));

        assert!(result.is_err());
        assert_eq!(fs::read(&path)?, original);
        assert!(!temp.0.join("backups").exists());
        Ok(())
    }

    #[test]
    fn doctor_shows_repair_only_for_a_proven_broken_quest() -> sse_core::Result<()> {
        let mut screen = HistoryScreen::new(ScreenId::SaveDoctor, "test", Workspace::default());
        let mut app = sse_app::state::AppState::new();
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
        let host = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let (proxy, _receiver) = crate::event_loop::channel_pair::<AppMessage>();
        let mut cx = Context {
            tree: &mut tree,
            proxy: Some(&proxy),
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.render_diagnosis(
            &mut cx,
            Path::new("fixture.sav"),
            DiagnosisReport {
                summary: "Подтверждённо сломанный квест.".to_owned(),
                source_sha256: "ab".repeat(32),
                format_id: Some("stalker-cs".to_owned()),
                quest_states: vec![sse_doctor::QuestTaskState {
                    id: "cs.wolf-dead",
                    title: "Wolf's tasks after his death",
                    status: sse_doctor::QuestTaskStatus::Broken,
                    reason: "dead-without-flag",
                    missing_info: Some("esc_wolf_dead"),
                    preventing_fix_id: Some("cs.quest.wolf-offline-cancellation"),
                    needs_preventing_fix: true,
                    detail: "Wolf is dead, but his offline-death cancellation flag is missing.",
                    references: &[],
                }],
                can_repair_quests: true,
            },
        )?;

        let row = screen
            .rows
            .first()
            .ok_or_else(|| sse_core::Error::damaged("quest state row was not built"))?;
        assert!(cx.tree.is_visible(row.row));
        // The row's own buttons sit in a hidden host; the screen's action list says which of them is offered.
        assert!(screen.actions.iter().any(|action| action.widget == row.button));
        assert!(matches!(
            screen.actions.first().map(|action| &action.action),
            Some(Action::RepairQuests { .. })
        ));
        #[cfg(feature = "native-ui")]
        {
            assert!(screen
                .actions
                .iter()
                .any(|action| action.widget == row.secondary_button));
            assert!(matches!(
                screen.actions.get(1).map(|action| &action.action),
                Some(Action::OpenGameFix { game_id, fix_id })
                    if game_id == "cs" && fix_id == "cs.quest.wolf-offline-cancellation"
            ));
            screen.message(&mut cx, &Message::User(AppMessage::Tick(0)), Some(row.secondary_button))?;
            assert!(matches!(
                _receiver.recv_timeout(Duration::from_secs(1)),
                Ok(Message::User(AppMessage::OpenGameFix { game_id, fix_id }))
                    if game_id == "cs" && fix_id == "cs.quest.wolf-offline-cancellation"
            ));
        }
        #[cfg(not(feature = "native-ui"))]
        assert!(!cx.tree.is_visible(row.secondary_button));
        Ok(())
    }

    #[test]
    fn stale_restore_failure_is_still_reported() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let session = workspace.session();
        let path = PathBuf::from("restore-old.sav");
        let first = session
            .begin_restore(&path)
            .ok_or_else(|| sse_core::Error::Refused("first restore did not start".to_owned()))?;
        let first_id = first.id();
        drop(first);
        let _second = session
            .begin_restore(&path)
            .ok_or_else(|| sse_core::Error::Refused("second restore did not start".to_owned()))?;
        assert!(!session.is_latest_operation(first_id));

        let mut screen = HistoryScreen::new(ScreenId::Backups, "test", workspace.clone());
        let mut app = sse_app::state::AppState::new();
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
        screen.message(
            &mut cx,
            &Message::User(AppMessage::ToScreen(
                ScreenId::Backups,
                Box::new(super::HistoryResult::Restored {
                    result: Err("disk full".to_owned()),
                }),
            )),
            None,
        )?;
        let summary = screen
            .summary
            .ok_or_else(|| sse_core::Error::Refused("no summary widget".to_owned()))?;
        assert!(
            tree.text(summary)?.contains("disk full"),
            "the outcome of a superseded restore must still be shown"
        );
        Ok(())
    }

    #[test]
    fn stale_diagnosis_result_cannot_replace_the_newer_report() -> sse_core::Result<()> {
        let mut screen = HistoryScreen::new(ScreenId::SaveDoctor, "test", Workspace::default());
        screen.diagnosis_request_id = 2;
        let mut app = sse_app::state::AppState::new();
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
        screen.message(
            &mut cx,
            &Message::User(AppMessage::ToScreen(
                ScreenId::SaveDoctor,
                Box::new(super::HistoryResult::Diagnosis {
                    request_id: 1,
                    result: Ok((
                        PathBuf::from("stale.sav"),
                        DiagnosisReport {
                            summary: "stale report".to_owned(),
                            source_sha256: "ab".repeat(32),
                            format_id: Some("stalker-cs".to_owned()),
                            quest_states: vec![sse_doctor::QuestTaskState {
                                id: "cs.wolf-dead",
                                title: "Wolf's tasks after his death",
                                status: sse_doctor::QuestTaskStatus::Broken,
                                reason: "dead-without-flag",
                                missing_info: Some("esc_wolf_dead"),
                                preventing_fix_id: None,
                                needs_preventing_fix: false,
                                detail: "The actor is dead.",
                                references: &[],
                            }],
                            can_repair_quests: true,
                        },
                    )),
                }),
            )),
            None,
        )?;

        assert!(screen.actions.is_empty());
        assert!(screen.rows.iter().all(|row| !cx.tree.is_visible(row.row)));
        Ok(())
    }

    #[test]
    fn quest_repair_cannot_start_while_another_save_is_active() -> sse_core::Result<()> {
        let workspace = Workspace::default();
        let path = PathBuf::from("fixture.sav");
        let save_guard = workspace
            .session()
            .begin_save(&path)
            .ok_or_else(|| sse_core::Error::Refused("test save did not start".to_owned()))?;
        let screen = HistoryScreen::new(ScreenId::SaveDoctor, "test", workspace.clone());
        let (proxy, _receiver) = crate::event_loop::channel_pair::<AppMessage>();

        assert!(!screen.start_quest_repair_write(path, "00".repeat(32), None, Some(proxy),)?);
        assert!(workspace.is_saving());
        drop(save_guard);
        Ok(())
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
    fn compare_row_then_side_action_chooses_the_same_candidate() -> sse_core::Result<()> {
        // The row only chooses a save; the side panel's one action does what the entry's own button did.
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
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.render_saves(
            &mut cx,
            vec![fixture_slot("soc-a.sav", "soc"), fixture_slot("soc-b.sav", "soc")],
        )?;
        let entry = screen
            .actions
            .first()
            .cloned()
            .ok_or_else(|| sse_core::Error::damaged("missing first candidate"))?;
        let first_path = match entry.action {
            Action::Compare(path) => path,
            _ => return Err(sse_core::Error::damaged("expected compare action")),
        };
        let position = screen
            .rows
            .iter()
            .position(|row| row.button == entry.widget)
            .ok_or_else(|| sse_core::Error::damaged("the candidate has no row"))?;
        let select = screen
            .rows
            .get(position)
            .map(|row| row.select)
            .ok_or_else(|| sse_core::Error::damaged("no select button for the row"))?;
        let side_action = screen
            .side_action
            .ok_or_else(|| sse_core::Error::damaged("the side panel has no action"))?;
        let pointer = Message::Window(crate::event_loop::WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        assert!(
            cx.tree.is_visible(side_action),
            "the side action is hidden after choosing a candidate"
        );
        assert!(
            cx.tree.is_enabled(side_action)?,
            "the side action is off after choosing a candidate"
        );
        assert!(
            screen.compare_selection.is_empty(),
            "choosing a row must not start the comparison"
        );
        screen.message(&mut cx, &pointer, Some(side_action))?;
        assert_eq!(
            screen.compare_selection,
            vec![first_path],
            "the side action chose another candidate"
        );
        Ok(())
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
    fn backup_row_then_side_action_asks_the_same_confirmation() -> sse_core::Result<()> {
        // The row only chooses the backup; the side panel's restore action asks for the same confirmation as the row's own.
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
        let mut cx = Context {
            tree: &mut tree,
            proxy: None,
            status: None,
            app: &mut app,
        };
        screen.build(&mut cx, host)?;
        screen.render_backups(&mut cx, transaction::list_backups(&backup_directory)?)?;
        let select = screen
            .rows
            .first()
            .map(|row| row.select)
            .ok_or_else(|| sse_core::Error::damaged("no backup row"))?;
        let side_action = screen
            .side_action
            .ok_or_else(|| sse_core::Error::damaged("the side panel has no action"))?;
        let pointer = Message::Window(crate::event_loop::WindowEvent::PointerLeft);
        screen.message(&mut cx, &pointer, Some(select))?;
        assert!(
            cx.tree.is_visible(side_action),
            "the side action is hidden after choosing a backup"
        );
        assert!(
            screen.pending_restore.is_none(),
            "choosing a row must not ask for a restore"
        );
        screen.message(&mut cx, &pointer, Some(side_action))?;
        let (journal, _, _, _) = screen
            .pending_restore
            .clone()
            .ok_or_else(|| sse_core::Error::damaged("the side action did not ask for the restore"))?;
        assert_eq!(journal, receipt.journal_path);
        assert_eq!(
            fs::read(&source)?,
            SYNTHETIC_XRAY_SAVE,
            "nothing is written before the confirmation"
        );
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
                    backup: receipt.backup_path.clone(),
                },
            });
            let message = Message::User(AppMessage::Tick(0));
            screen.message(&mut cx, &message, Some(restore_button))?;
        }
        assert_eq!(
            screen.pending_restore,
            Some((
                receipt.journal_path.clone(),
                source.clone(),
                receipt.backup_path.clone(),
                super::RestoreMode::Copy
            )),
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
    fn steam_cloud_backup_offers_copy_restore_without_in_place_action() -> sse_core::Result<()> {
        let temp = TempDirectory::new();
        let backup_directory = temp.0.join("backups");
        let receipt = transaction::write_cloud_recovery_artifacts(
            &backup_directory,
            4500,
            "_appdata_/savedgames/slot.sav",
            b"cloud original bytes",
            b"prepared upload bytes",
        )?;
        let entries = transaction::list_backups(&backup_directory)?;
        assert!(entries.iter().any(|entry| entry.journal_path == receipt.journal_path));

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
        let mut screen = HistoryScreen::new(
            ScreenId::Backups,
            "test",
            Workspace::with_backup_directory(backup_directory),
        );
        {
            let mut cx = Context {
                tree: &mut tree,
                proxy: None,
                status: None,
                app: &mut app,
            };
            screen.build(&mut cx, host)?;
            screen.render_backups(&mut cx, entries)?;
        }

        assert_eq!(screen.actions.len(), 1);
        assert!(matches!(
            screen.actions.first().map(|action| &action.action),
            Some(Action::Restore { .. })
        ));
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
