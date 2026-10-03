//! Virtual list/table model for very large views.
//!
//! Rows are identified by stable IDs and never moved. Sorting and filtering operate on index
//! permutations. The viewport is mapped through a Fenwick tree so fixed and corrected variable
//! heights are O(log n), and scrolling performs no allocation. Selection stores row IDs, so it
//! survives re-sorting, filtering, grouping and collapse/expand operations.

use sse_core::{Error, Result};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::ops::{Div, Mul};

/// Maximum supported source-row count.
pub const MAX_ROWS: usize = 1_000_000;
const MAX_TYPE_AHEAD_CHARS: usize = 256;
/// Stable application-owned row identifier.
pub type RowId = u64;
/// Stable application-owned group identifier.
pub type GroupId = u64;

/// Height policy used to initialise the virtual extent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RowHeight {
    /// Every data row has the same height.
    Fixed(f32),
    /// Rows start at an estimated height and can later be corrected from actual measurement.
    Variable {
        /// Initial row estimate.
        estimate: f32,
        /// Height of inserted group headers.
        group_header: f32,
    },
}

/// One item in the flattened virtual view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewItem {
    /// A collapsible group header.
    GroupHeader(GroupId),
    /// A source row.
    Row(RowId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ViewEntry {
    GroupHeader(GroupId),
    Row(usize),
}

/// Half-open viewport range over flattened view items.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VisibleRange {
    /// First item to realise.
    pub start: usize,
    /// One past the last item to realise.
    pub end: usize,
    /// Y position of `start` in content coordinates.
    pub top: f32,
    /// Total virtual content height.
    pub content_height: f32,
}

/// Direction of one table sort key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Smallest first.
    Ascending,
    /// Largest first.
    Descending,
}

/// One column in a multi-column stable sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortColumn {
    /// Caller-defined column index passed to the comparison callback.
    pub column: usize,
    /// Ordering direction.
    pub direction: SortDirection,
}

/// Progress returned by incremental filtering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterProgress {
    /// Number of sorted rows examined so far.
    pub processed: usize,
    /// Total source rows that must be examined.
    pub total: usize,
    /// Number of matching rows found so far.
    pub matches: usize,
    /// Whether the current filter has reached the end.
    pub complete: bool,
}

/// Keyboard focus movement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Navigation {
    /// Previous visible row.
    Up,
    /// Next visible row.
    Down,
    /// First visible row.
    Home,
    /// Last visible row.
    End,
    /// Move approximately one realised page upward by `rows` data rows.
    PageUp(usize),
    /// Move approximately one realised page downward by `rows` data rows.
    PageDown(usize),
}

/// Width policy for one table column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnWidth {
    /// Logical-pixel width independent of the remaining table width.
    Fixed(f32),
    /// Sample row content and use the widest measured value plus padding.
    Auto {
        /// Maximum number of source rows sampled.
        sample_rows: usize,
        /// Logical pixels added around sampled content.
        padding: f32,
    },
    /// Share remaining width proportionally with other fractional columns.
    Fraction(f32),
}

/// Column sizing state, including a persistent user resize override.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Column {
    /// Automatic/fixed/fraction sizing mode.
    pub width: ColumnWidth,
    /// Smallest resolved width.
    pub min: f32,
    /// Optional largest resolved width.
    pub max: Option<f32>,
    /// User drag override. When set, it takes priority over `width`.
    pub user_width: Option<f32>,
}

impl Column {
    /// Creates a column with a non-negative minimum and no user override.
    #[must_use]
    pub fn new(width: ColumnWidth, min: f32, max: Option<f32>) -> Self {
        Self {
            width,
            min: min.max(0.0),
            max,
            user_width: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Fenwick {
    tree: Vec<f32>,
}

impl Fenwick {
    fn from_values(values: &[f32]) -> Result<Self> {
        let length = values
            .len()
            .checked_add(1)
            .ok_or_else(|| Error::Damaged("Fenwick size overflow".to_owned()))?;
        let mut tree = vec![0.0_f32; length];
        for (offset, value) in values.iter().copied().enumerate() {
            if !value.is_finite() || value < 0.0 {
                return Err(Error::Damaged("invalid Fenwick source value".to_owned()));
            }
            let index = offset
                .checked_add(1)
                .ok_or_else(|| Error::Damaged("Fenwick build index overflow".to_owned()))?;
            let inherited = tree.get(index).copied().unwrap_or_default();
            let current = inherited.mul_add(1.0, value);
            let Some(slot) = tree.get_mut(index) else {
                return Err(Error::Damaged("Fenwick build slot missing".to_owned()));
            };
            *slot = current;
            let step = index & index.wrapping_neg();
            let Some(parent) = index.checked_add(step) else {
                continue;
            };
            if parent < length {
                let parent_value = tree.get(parent).copied().unwrap_or_default();
                let Some(parent_slot) = tree.get_mut(parent) else {
                    return Err(Error::Damaged("Fenwick parent slot missing".to_owned()));
                };
                *parent_slot = parent_value.mul_add(1.0, current);
            }
        }
        Ok(Self { tree })
    }

    fn len(&self) -> usize {
        self.tree.len().saturating_sub(1)
    }

    fn add(&mut self, position: usize, delta: f32) -> Result<()> {
        if !delta.is_finite() {
            return Err(Error::Damaged("non-finite Fenwick delta".to_owned()));
        }
        let mut index = position
            .checked_add(1)
            .ok_or_else(|| Error::Damaged("Fenwick index overflow".to_owned()))?;
        while index < self.tree.len() {
            let Some(slot) = self.tree.get_mut(index) else {
                return Err(Error::Damaged("Fenwick slot missing".to_owned()));
            };
            *slot = slot.mul_add(1.0, delta);
            let step = index & index.wrapping_neg();
            if step == 0 {
                break;
            }
            index = index
                .checked_add(step)
                .ok_or_else(|| Error::Damaged("Fenwick update overflow".to_owned()))?;
        }
        Ok(())
    }

    fn prefix(&self, end: usize) -> f32 {
        let mut index = end.min(self.len());
        let mut sum = 0.0_f32;
        while index > 0 {
            if let Some(value) = self.tree.get(index).copied() {
                sum = sum.mul_add(1.0, value);
            }
            let step = index & index.wrapping_neg();
            if step == 0 {
                break;
            }
            index = index.saturating_sub(step);
        }
        sum
    }

    fn total(&self) -> f32 {
        self.prefix(self.len())
    }

    fn item_at_offset(&self, offset: f32) -> Option<usize> {
        let length = self.len();
        if length == 0 {
            return None;
        }
        let target = offset.max(0.0);
        let mut bit = 1_usize;
        while let Some(next) = bit.checked_mul(2) {
            if next > length {
                break;
            }
            bit = next;
        }
        let mut index = 0_usize;
        let mut accumulated = 0.0_f32;
        while bit > 0 {
            let Some(next) = index.checked_add(bit) else {
                bit >>= 1;
                continue;
            };
            if next <= length {
                let value = self.tree.get(next).copied().unwrap_or_default();
                let candidate = accumulated.mul_add(1.0, value);
                if candidate <= target {
                    index = next;
                    accumulated = candidate;
                }
            }
            bit >>= 1;
        }
        if index >= length {
            Some(length.saturating_sub(1))
        } else {
            Some(index)
        }
    }
}

/// Virtual list/table state. Source row data stays outside this object.
#[derive(Clone, Debug)]
pub struct ListModel {
    rows: Vec<RowId>,
    id_to_index: HashMap<RowId, usize>,
    permutation: Vec<usize>,
    filtered: Vec<usize>,
    filter_active: bool,
    filter_cursor: usize,
    view: Vec<ViewEntry>,
    heights: Vec<f32>,
    measured: Vec<bool>,
    fenwick: Fenwick,
    row_height: RowHeight,
    groups: HashMap<RowId, GroupId>,
    collapsed_groups: HashSet<GroupId>,
    selected: HashSet<RowId>,
    selection_anchor: Option<RowId>,
    focus: Option<RowId>,
    columns: Vec<Column>,
    type_ahead: String,
    type_ahead_last_millis: Option<u64>,
}

impl ListModel {
    /// Creates a list from stable row IDs. Duplicate IDs are refused.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] for more than one million rows, duplicate IDs, or invalid heights.
    pub fn new(rows: Vec<RowId>, row_height: RowHeight) -> Result<Self> {
        if rows.len() > MAX_ROWS {
            return Err(Error::Refused("virtual list exceeds one million rows".to_owned()));
        }
        validate_height_policy(row_height)?;
        let mut id_to_index = HashMap::with_capacity(rows.len());
        for (index, row) in rows.iter().copied().enumerate() {
            if id_to_index.insert(row, index).is_some() {
                return Err(Error::Refused("virtual list row IDs must be unique".to_owned()));
            }
        }
        let mut permutation = Vec::with_capacity(rows.len());
        permutation.extend(0..rows.len());
        let mut model = Self {
            rows,
            id_to_index,
            permutation,
            filtered: Vec::new(),
            filter_active: false,
            filter_cursor: 0,
            view: Vec::new(),
            heights: Vec::new(),
            measured: Vec::new(),
            fenwick: Fenwick::default(),
            row_height,
            groups: HashMap::new(),
            collapsed_groups: HashSet::new(),
            selected: HashSet::new(),
            selection_anchor: None,
            focus: None,
            columns: Vec::new(),
            type_ahead: String::new(),
            type_ahead_last_millis: None,
        };
        model.rebuild_view()?;
        Ok(model)
    }

    /// Number of immutable source rows.
    #[must_use]
    pub fn source_len(&self) -> usize {
        self.rows.len()
    }

    /// Number of currently flattened items, including group headers.
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.view.len()
    }

    /// Total virtual content height.
    #[must_use]
    pub fn content_height(&self) -> f32 {
        self.fenwick.total()
    }

    /// Returns one flattened row/header without allocating.
    #[must_use]
    pub fn view_item(&self, index: usize) -> Option<ViewItem> {
        match self.view.get(index).copied()? {
            ViewEntry::GroupHeader(group) => Some(ViewItem::GroupHeader(group)),
            ViewEntry::Row(source) => self.rows.get(source).copied().map(ViewItem::Row),
        }
    }

    /// Returns the current visible range with pixel overscan. This performs no allocation.
    #[must_use]
    pub fn visible_range(&self, scroll_y: f32, viewport_height: f32, overscan: f32) -> VisibleRange {
        if self.view.is_empty() {
            return VisibleRange {
                content_height: 0.0,
                ..VisibleRange::default()
            };
        }
        let scroll = if scroll_y.is_finite() { scroll_y.max(0.0) } else { 0.0 };
        let viewport = if viewport_height.is_finite() {
            viewport_height.max(0.0)
        } else {
            0.0
        };
        let extra = if overscan.is_finite() { overscan.max(0.0) } else { 0.0 };
        let start_offset = float_sub(scroll, extra).max(0.0);
        let end_offset = scroll.mul_add(1.0, viewport).mul_add(1.0, extra);
        let start = self.fenwick.item_at_offset(start_offset).unwrap_or_default();
        let end_item = self
            .fenwick
            .item_at_offset(end_offset)
            .unwrap_or_else(|| self.view.len().saturating_sub(1));
        let end = end_item.checked_add(1).unwrap_or(self.view.len()).min(self.view.len());
        VisibleRange {
            start,
            end,
            top: self.fenwick.prefix(start),
            content_height: self.fenwick.total(),
        }
    }

    /// Corrects one realised item's estimated height and returns the adjusted scroll offset that
    /// preserves the exact item and intra-item offset currently at the viewport top.
    ///
    /// # Errors
    /// Returns an error for a missing item or invalid measured height.
    pub fn correct_height(&mut self, view_index: usize, new_height: f32, scroll_y: f32) -> Result<f32> {
        if !new_height.is_finite() || new_height <= 0.0 {
            return Err(Error::Refused("row height must be finite and positive".to_owned()));
        }
        let Some(old_height) = self.heights.get(view_index).copied() else {
            return Err(Error::Damaged("height correction outside flattened view".to_owned()));
        };
        let anchor = self.fenwick.item_at_offset(scroll_y.max(0.0)).unwrap_or_default();
        let before_top = self.fenwick.prefix(anchor);
        let within = float_sub(scroll_y.max(0.0), before_top).max(0.0);
        let delta = float_sub(new_height, old_height);
        if let Some(slot) = self.heights.get_mut(view_index) {
            *slot = new_height;
        }
        if let Some(slot) = self.measured.get_mut(view_index) {
            *slot = true;
        }
        self.fenwick.add(view_index, delta)?;
        let after_top = self.fenwick.prefix(anchor);
        Ok(after_top.mul_add(1.0, within).max(0.0))
    }

    /// Whether an item has an actual measured height rather than the initial estimate.
    #[must_use]
    pub fn is_height_measured(&self, view_index: usize) -> bool {
        self.measured.get(view_index).copied().unwrap_or(false)
    }

    /// Stable multi-column sort over an index permutation; source rows are never moved.
    ///
    /// The callback receives `(left_id, right_id, column_index)`.
    ///
    /// # Errors
    /// Returns an error only if rebuilding the virtual extent fails.
    pub fn sort_by<F>(&mut self, columns: &[SortColumn], mut compare: F) -> Result<()>
    where
        F: FnMut(RowId, RowId, usize) -> Ordering,
    {
        self.permutation.clear();
        self.permutation.extend(0..self.rows.len());
        let rows = &self.rows;
        self.permutation.sort_by(|left_index, right_index| {
            let Some(left) = rows.get(*left_index).copied() else {
                return Ordering::Equal;
            };
            let Some(right) = rows.get(*right_index).copied() else {
                return Ordering::Equal;
            };
            for column in columns.iter().copied() {
                let ordering = compare(left, right, column.column);
                if ordering != Ordering::Equal {
                    return match column.direction {
                        SortDirection::Ascending => ordering,
                        SortDirection::Descending => ordering.reverse(),
                    };
                }
            }
            Ordering::Equal
        });
        self.clear_filter_state();
        self.rebuild_view()
    }

    /// Starts a new incremental filter. Call [`Self::filter_step`] repeatedly.
    ///
    /// # Errors
    /// Returns an error only if rebuilding the empty filtered view fails.
    pub fn begin_filter(&mut self) -> Result<()> {
        self.filter_active = true;
        self.filter_cursor = 0;
        self.filtered.clear();
        self.rebuild_view()
    }

    /// Examines at most `budget` sorted rows and appends matching permutation indices.
    /// Existing selection is not changed, including selected rows temporarily hidden by the filter.
    ///
    /// # Errors
    /// Returns an error if internal permutation indices are damaged or the virtual extent cannot rebuild.
    pub fn filter_step<F>(&mut self, budget: usize, mut predicate: F) -> Result<FilterProgress>
    where
        F: FnMut(RowId) -> bool,
    {
        if !self.filter_active {
            self.begin_filter()?;
        }
        let total = self.permutation.len();
        let end = self.filter_cursor.saturating_add(budget).min(total);
        while self.filter_cursor < end {
            let Some(source_index) = self.permutation.get(self.filter_cursor).copied() else {
                return Err(Error::Damaged("filter permutation index missing".to_owned()));
            };
            let Some(row) = self.rows.get(source_index).copied() else {
                return Err(Error::Damaged("filter source index missing".to_owned()));
            };
            if predicate(row) {
                self.filtered.push(source_index);
            }
            self.filter_cursor = self
                .filter_cursor
                .checked_add(1)
                .ok_or_else(|| Error::Damaged("filter cursor overflow".to_owned()))?;
        }
        self.rebuild_view()?;
        Ok(FilterProgress {
            processed: self.filter_cursor,
            total,
            matches: self.filtered.len(),
            complete: self.filter_cursor >= total,
        })
    }

    /// Removes the filter and restores the sorted permutation.
    ///
    /// # Errors
    /// Returns an error only if rebuilding the virtual extent fails.
    pub fn clear_filter(&mut self) -> Result<()> {
        self.clear_filter_state();
        self.rebuild_view()
    }

    /// Replaces all row-to-group assignments. Rows omitted from `assignments` remain ungrouped.
    ///
    /// # Errors
    /// Unknown row IDs are refused.
    pub fn set_groups<I>(&mut self, assignments: I) -> Result<()>
    where
        I: IntoIterator<Item = (RowId, GroupId)>,
    {
        let mut groups = HashMap::new();
        for (row, group) in assignments {
            if !self.id_to_index.contains_key(&row) {
                return Err(Error::Refused("group assignment references unknown row".to_owned()));
            }
            groups.insert(row, group);
        }
        self.groups = groups;
        let groups = &self.groups;
        self.collapsed_groups
            .retain(|group| groups.values().any(|value| value == group));
        self.rebuild_view()
    }

    /// Collapses or expands one group header.
    ///
    /// # Errors
    /// Returns an error only if rebuilding the virtual extent fails.
    pub fn set_group_collapsed(&mut self, group: GroupId, collapsed: bool) -> Result<()> {
        if !self.groups.values().any(|value| *value == group) {
            return Err(Error::Refused("unknown group id".to_owned()));
        }
        if collapsed {
            self.collapsed_groups.insert(group);
        } else {
            self.collapsed_groups.remove(&group);
        }
        self.rebuild_view()
    }

    /// Whether a group is collapsed.
    #[must_use]
    pub fn group_collapsed(&self, group: GroupId) -> bool {
        self.collapsed_groups.contains(&group)
    }

    /// Applies a pointer selection to a flattened data row. Group headers are ignored.
    ///
    /// Ctrl toggles a row. Shift selects a current-view range from the stable anchor.
    pub fn select_view(&mut self, view_index: usize, ctrl: bool, shift: bool) -> bool {
        let Some(row) = self.row_at_view(view_index) else {
            return false;
        };
        if shift {
            let anchor = self.selection_anchor.or(self.focus).unwrap_or(row);
            self.select_range(anchor, row, ctrl);
            self.focus = Some(row);
            return true;
        }
        if ctrl {
            if self.selected.contains(&row) {
                self.selected.remove(&row);
            } else {
                self.selected.insert(row);
            }
            self.selection_anchor = Some(row);
            self.focus = Some(row);
            return true;
        }
        self.selected.clear();
        self.selected.insert(row);
        self.selection_anchor = Some(row);
        self.focus = Some(row);
        true
    }

    /// Selects all currently visible data rows. Hidden filtered/collapsed rows are not added.
    pub fn select_all_visible(&mut self) {
        for entry in self.view.iter().copied() {
            if let ViewEntry::Row(source) = entry {
                if let Some(row) = self.rows.get(source).copied() {
                    self.selected.insert(row);
                }
            }
        }
    }

    /// Clears selection and keyboard anchor.
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.selection_anchor = None;
        self.focus = None;
    }

    /// Stable selection membership by row ID.
    #[must_use]
    pub fn is_selected(&self, row: RowId) -> bool {
        self.selected.contains(&row)
    }

    /// Number of selected stable row IDs, including hidden rows.
    #[must_use]
    pub fn selected_count(&self) -> usize {
        self.selected.len()
    }

    /// Current stable keyboard focus row.
    #[must_use]
    pub const fn focus(&self) -> Option<RowId> {
        self.focus
    }

    /// Moves keyboard focus among visible data rows, optionally extending from the selection anchor.
    pub fn navigate(&mut self, navigation: Navigation, extend: bool) -> Option<RowId> {
        let target = match navigation {
            Navigation::Home => self.first_visible_row(),
            Navigation::End => self.last_visible_row(),
            Navigation::Up => self.relative_visible_row(false, 1),
            Navigation::Down => self.relative_visible_row(true, 1),
            Navigation::PageUp(rows) => self.relative_visible_row(false, rows.max(1)),
            Navigation::PageDown(rows) => self.relative_visible_row(true, rows.max(1)),
        }?;
        self.select_keyboard(target, extend);
        Some(target)
    }

    /// Type-ahead search over visible rows without allocating row labels.
    ///
    /// `now_millis` comes from the UI clock. Typing after `timeout_millis` starts a new prefix.
    pub fn type_ahead<'a, F>(&mut self, value: char, now_millis: u64, timeout_millis: u64, label: F) -> Option<RowId>
    where
        F: Fn(RowId) -> &'a str,
    {
        if value.is_control() {
            return None;
        }
        if self
            .type_ahead_last_millis
            .is_none_or(|previous| now_millis.saturating_sub(previous) > timeout_millis)
        {
            self.type_ahead.clear();
        }
        self.type_ahead_last_millis = Some(now_millis);
        if self.type_ahead.chars().count() >= MAX_TYPE_AHEAD_CHARS {
            self.type_ahead.clear();
        }
        self.type_ahead.push(value);
        let length = self.view.len();
        if length == 0 {
            return None;
        }
        let start = self
            .focus
            .and_then(|row| self.view_position(row))
            .and_then(|position| position.checked_add(1))
            .unwrap_or_default();
        let mut scanned = 0_usize;
        while scanned < length {
            let raw = start.checked_add(scanned).unwrap_or(length);
            let position = if raw >= length { raw.saturating_sub(length) } else { raw };
            if let Some(row) = self.row_at_view(position) {
                if starts_with_case_fold(label(row), &self.type_ahead) {
                    self.select_keyboard(row, false);
                    return Some(row);
                }
            }
            scanned = scanned.saturating_add(1);
        }
        None
    }

    /// Replaces table column definitions.
    pub fn set_columns(&mut self, columns: Vec<Column>) {
        self.columns = columns;
    }

    /// Stores a clamped user-resize override for one column.
    ///
    /// # Errors
    /// Returns an error for an invalid column index or non-finite width.
    pub fn resize_column(&mut self, index: usize, width: f32) -> Result<()> {
        if !width.is_finite() || width < 0.0 {
            return Err(Error::Refused(
                "column width must be finite and non-negative".to_owned(),
            ));
        }
        let Some(column) = self.columns.get_mut(index) else {
            return Err(Error::Refused("column index outside table".to_owned()));
        };
        column.user_width = Some(clamp_width(width, column.min, column.max));
        Ok(())
    }

    /// Clears one user-resize override.
    pub fn clear_column_resize(&mut self, index: usize) {
        if let Some(column) = self.columns.get_mut(index) {
            column.user_width = None;
        }
    }

    /// Resolves column widths. Allocation happens only for the returned width vector; this method
    /// is expected to run on layout/configuration changes, never in the scrolling hot path.
    ///
    /// `measure(column_index, row_id)` returns the content width for an auto-size sample.
    #[must_use]
    pub fn resolve_columns<F>(&self, total_width: f32, mut measure: F) -> Vec<f32>
    where
        F: FnMut(usize, RowId) -> f32,
    {
        let total = if total_width.is_finite() {
            total_width.max(0.0)
        } else {
            0.0
        };
        let mut resolved = vec![0.0_f32; self.columns.len()];
        let mut used = 0.0_f32;
        let mut fraction_total = 0.0_f32;

        for (column_index, column) in self.columns.iter().copied().enumerate() {
            if let Some(user) = column.user_width {
                let width = clamp_width(user, column.min, column.max);
                if let Some(slot) = resolved.get_mut(column_index) {
                    *slot = width;
                }
                used = used.mul_add(1.0, width);
                continue;
            }
            match column.width {
                ColumnWidth::Fixed(width) => {
                    let width = clamp_width(width.max(0.0), column.min, column.max);
                    if let Some(slot) = resolved.get_mut(column_index) {
                        *slot = width;
                    }
                    used = used.mul_add(1.0, width);
                }
                ColumnWidth::Auto { sample_rows, padding } => {
                    let mut widest = 0.0_f32;
                    let mut sampled = 0_usize;
                    for entry in self.view.iter().copied() {
                        if sampled >= sample_rows {
                            break;
                        }
                        let ViewEntry::Row(source) = entry else {
                            continue;
                        };
                        let Some(row) = self.rows.get(source).copied() else {
                            continue;
                        };
                        let measured = measure(column_index, row);
                        if measured.is_finite() {
                            widest = widest.max(measured.max(0.0));
                        }
                        sampled = sampled.saturating_add(1);
                    }
                    let width = widest.mul_add(1.0, padding.max(0.0));
                    let width = clamp_width(width, column.min, column.max);
                    if let Some(slot) = resolved.get_mut(column_index) {
                        *slot = width;
                    }
                    used = used.mul_add(1.0, width);
                }
                ColumnWidth::Fraction(weight) => {
                    fraction_total = fraction_total.mul_add(1.0, weight.max(0.0));
                }
            }
        }

        let remaining = float_sub(total, used).max(0.0);
        if fraction_total > 0.0 {
            for (column_index, column) in self.columns.iter().copied().enumerate() {
                if column.user_width.is_some() {
                    continue;
                }
                let ColumnWidth::Fraction(weight) = column.width else {
                    continue;
                };
                let share = remaining.mul(weight.max(0.0).div(fraction_total));
                let width = clamp_width(share, column.min, column.max);
                if let Some(slot) = resolved.get_mut(column_index) {
                    *slot = width;
                }
            }
        }
        resolved
    }

    fn clear_filter_state(&mut self) {
        self.filter_active = false;
        self.filter_cursor = 0;
        self.filtered.clear();
    }

    fn rebuild_view(&mut self) -> Result<()> {
        {
            let effective = if self.filter_active {
                self.filtered.as_slice()
            } else {
                self.permutation.as_slice()
            };
            let rows = &self.rows;
            let groups = &self.groups;
            let collapsed_groups = &self.collapsed_groups;
            let view = &mut self.view;
            view.clear();
            view.reserve(effective.len());

            if groups.is_empty() {
                view.extend(effective.iter().copied().map(ViewEntry::Row));
            } else {
                let mut group_order = Vec::<GroupId>::new();
                let mut group_positions = HashMap::<GroupId, usize>::new();
                let mut buckets = Vec::<Vec<usize>>::new();
                let mut ungrouped = Vec::<usize>::new();
                for source in effective.iter().copied() {
                    let Some(row) = rows.get(source).copied() else {
                        return Err(Error::Damaged("view source index outside rows".to_owned()));
                    };
                    let Some(group) = groups.get(&row).copied() else {
                        ungrouped.push(source);
                        continue;
                    };
                    let bucket = if let Some(position) = group_positions.get(&group).copied() {
                        position
                    } else {
                        let position = buckets.len();
                        group_order.push(group);
                        buckets.push(Vec::new());
                        group_positions.insert(group, position);
                        position
                    };
                    let Some(values) = buckets.get_mut(bucket) else {
                        return Err(Error::Damaged("group bucket missing".to_owned()));
                    };
                    values.push(source);
                }
                for (position, group) in group_order.iter().copied().enumerate() {
                    view.push(ViewEntry::GroupHeader(group));
                    if collapsed_groups.contains(&group) {
                        continue;
                    }
                    let Some(values) = buckets.get(position) else {
                        return Err(Error::Damaged("group bucket order damaged".to_owned()));
                    };
                    view.extend(values.iter().copied().map(ViewEntry::Row));
                }
                view.extend(ungrouped.into_iter().map(ViewEntry::Row));
            }
        }

        self.heights.clear();
        self.measured.clear();
        self.heights.reserve(self.view.len());
        self.measured.reserve(self.view.len());
        for entry in self.view.iter().copied() {
            self.heights.push(self.initial_height(entry));
            self.measured.push(false);
        }
        self.fenwick = Fenwick::from_values(&self.heights)?;
        Ok(())
    }

    fn initial_height(&self, entry: ViewEntry) -> f32 {
        match (self.row_height, entry) {
            (RowHeight::Fixed(height), ViewEntry::Row(_)) => height,
            (RowHeight::Fixed(height), ViewEntry::GroupHeader(_)) => height,
            (RowHeight::Variable { estimate, .. }, ViewEntry::Row(_)) => estimate,
            (RowHeight::Variable { group_header, .. }, ViewEntry::GroupHeader(_)) => group_header,
        }
    }

    fn row_at_view(&self, view_index: usize) -> Option<RowId> {
        let ViewEntry::Row(source) = self.view.get(view_index).copied()? else {
            return None;
        };
        self.rows.get(source).copied()
    }

    fn view_position(&self, row: RowId) -> Option<usize> {
        self.view.iter().position(|entry| match *entry {
            ViewEntry::Row(source) => self.rows.get(source).copied() == Some(row),
            ViewEntry::GroupHeader(_) => false,
        })
    }

    fn select_range(&mut self, anchor: RowId, row: RowId, additive: bool) {
        let Some(anchor_position) = self.view_position(anchor) else {
            if !additive {
                self.selected.clear();
            }
            self.selected.insert(row);
            self.selection_anchor = Some(row);
            return;
        };
        let Some(row_position) = self.view_position(row) else {
            return;
        };
        if !additive {
            self.selected.clear();
        }
        let start = anchor_position.min(row_position);
        let end = anchor_position.max(row_position);
        let mut position = start;
        loop {
            if let Some(value) = self.row_at_view(position) {
                self.selected.insert(value);
            }
            if position >= end {
                break;
            }
            let Some(next) = position.checked_add(1) else {
                break;
            };
            position = next;
        }
        self.selection_anchor = Some(anchor);
    }

    fn select_keyboard(&mut self, row: RowId, extend: bool) {
        if extend {
            let anchor = self.selection_anchor.or(self.focus).unwrap_or(row);
            self.select_range(anchor, row, false);
            self.selection_anchor = Some(anchor);
        } else {
            self.selected.clear();
            self.selected.insert(row);
            self.selection_anchor = Some(row);
        }
        self.focus = Some(row);
    }

    fn first_visible_row(&self) -> Option<RowId> {
        self.view.iter().find_map(|entry| match *entry {
            ViewEntry::Row(source) => self.rows.get(source).copied(),
            ViewEntry::GroupHeader(_) => None,
        })
    }

    fn last_visible_row(&self) -> Option<RowId> {
        self.view.iter().rev().find_map(|entry| match *entry {
            ViewEntry::Row(source) => self.rows.get(source).copied(),
            ViewEntry::GroupHeader(_) => None,
        })
    }

    fn relative_visible_row(&self, down: bool, rows: usize) -> Option<RowId> {
        let Some(focus) = self.focus else {
            return if down {
                self.first_visible_row()
            } else {
                self.last_visible_row()
            };
        };
        let Some(mut position) = self.view_position(focus) else {
            return if down {
                self.first_visible_row()
            } else {
                self.last_visible_row()
            };
        };
        let mut remaining = rows.max(1);
        let mut last = Some(focus);
        loop {
            if down {
                let Some(next) = position.checked_add(1) else {
                    break;
                };
                if next >= self.view.len() {
                    break;
                }
                position = next;
            } else {
                if position == 0 {
                    break;
                }
                position = position.saturating_sub(1);
            }
            if let Some(row) = self.row_at_view(position) {
                last = Some(row);
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    return Some(row);
                }
            }
        }
        last
    }
}

fn validate_height_policy(policy: RowHeight) -> Result<()> {
    match policy {
        RowHeight::Fixed(height) if height.is_finite() && height > 0.0 => Ok(()),
        RowHeight::Variable { estimate, group_header }
            if estimate.is_finite() && estimate > 0.0 && group_header.is_finite() && group_header > 0.0 =>
        {
            Ok(())
        }
        _ => Err(Error::Refused("row heights must be finite and positive".to_owned())),
    }
}

fn clamp_width(value: f32, minimum: f32, maximum: Option<f32>) -> f32 {
    let mut value = value.max(minimum.max(0.0));
    if let Some(maximum) = maximum.filter(|maximum| maximum.is_finite()) {
        value = value.min(maximum.max(minimum));
    }
    value
}

fn float_sub(left: f32, right: f32) -> f32 {
    left.mul_add(1.0, right.copysign(-1.0))
}

fn starts_with_case_fold(text: &str, prefix: &str) -> bool {
    let mut text_folded = text.chars().flat_map(char::to_lowercase);
    let mut prefix_folded = prefix.chars().flat_map(char::to_lowercase);
    loop {
        match prefix_folded.next() {
            None => return true,
            Some(expected) => {
                let Some(actual) = text_folded.next() else {
                    return false;
                };
                if actual != expected {
                    return false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        float_sub, Column, ColumnWidth, ListModel, Navigation, RowHeight, SortColumn, SortDirection, ViewItem, MAX_ROWS,
    };
    use std::cmp::Ordering;

    fn ids(count: usize) -> Vec<u64> {
        (0..count).filter_map(|value| u64::try_from(value).ok()).collect()
    }

    fn model(count: usize, height: RowHeight) -> ListModel {
        match ListModel::new(ids(count), height) {
            Ok(value) => value,
            Err(error) => panic!("list creation failed: {error}"),
        }
    }

    #[test]
    fn fixed_height_visible_range_has_pixel_overscan() {
        let list = model(100, RowHeight::Fixed(20.0));
        let range = list.visible_range(200.0, 100.0, 20.0);
        assert_eq!((range.start, range.end), (9, 17));
        assert!(float_sub(range.top, 180.0).abs() < 0.001);
        assert!(float_sub(range.content_height, 2000.0).abs() < 0.001);
    }

    #[test]
    fn corrected_height_preserves_top_scroll_anchor() {
        let mut list = model(
            20,
            RowHeight::Variable {
                estimate: 10.0,
                group_header: 14.0,
            },
        );
        let adjusted = match list.correct_height(2, 30.0, 55.0) {
            Ok(value) => value,
            Err(error) => panic!("height correction failed: {error}"),
        };
        assert!(float_sub(adjusted, 75.0).abs() < 0.001);
        let range = list.visible_range(adjusted, 20.0, 0.0);
        assert_eq!(range.start, 5);
        assert!(list.is_height_measured(2));
    }

    #[test]
    fn correction_below_anchor_does_not_move_scroll() {
        let mut list = model(
            20,
            RowHeight::Variable {
                estimate: 10.0,
                group_header: 14.0,
            },
        );
        let adjusted = match list.correct_height(12, 50.0, 55.0) {
            Ok(value) => value,
            Err(error) => panic!("height correction failed: {error}"),
        };
        assert!(float_sub(adjusted, 55.0).abs() < 0.001);
    }

    #[test]
    fn stable_multi_column_sort_moves_only_permutation() {
        let mut list = model(6, RowHeight::Fixed(10.0));
        let keys = [
            SortColumn {
                column: 0,
                direction: SortDirection::Ascending,
            },
            SortColumn {
                column: 1,
                direction: SortDirection::Descending,
            },
        ];
        let result = list.sort_by(&keys, |left, right, column| match column {
            0 => (left % 2).cmp(&(right % 2)),
            1 => left.cmp(&right),
            _ => Ordering::Equal,
        });
        assert_eq!(result, Ok(()));
        let actual: Vec<_> = (0..list.view_len()).filter_map(|index| list.view_item(index)).collect();
        assert_eq!(
            actual,
            vec![
                ViewItem::Row(4),
                ViewItem::Row(2),
                ViewItem::Row(0),
                ViewItem::Row(5),
                ViewItem::Row(3),
                ViewItem::Row(1),
            ]
        );
    }

    #[test]
    fn selection_survives_sort_and_filter() {
        let mut list = model(10, RowHeight::Fixed(12.0));
        assert!(list.select_view(7, false, false));
        assert!(list.is_selected(7));
        let sort = [SortColumn {
            column: 0,
            direction: SortDirection::Descending,
        }];
        assert_eq!(list.sort_by(&sort, |left, right, _| left.cmp(&right)), Ok(()));
        assert!(list.is_selected(7));
        assert_eq!(list.begin_filter(), Ok(()));
        let progress = match list.filter_step(10, |row| row % 2 == 0) {
            Ok(value) => value,
            Err(error) => panic!("filter failed: {error}"),
        };
        assert!(progress.complete);
        assert!(list.is_selected(7));
        assert_eq!(list.selected_count(), 1);
    }

    #[test]
    fn incremental_filter_processes_only_budget() {
        let mut list = model(20, RowHeight::Fixed(10.0));
        assert_eq!(list.begin_filter(), Ok(()));
        let first = match list.filter_step(5, |row| row % 3 == 0) {
            Ok(value) => value,
            Err(error) => panic!("filter failed: {error}"),
        };
        assert_eq!((first.processed, first.matches, first.complete), (5, 2, false));
        assert_eq!(list.view_len(), 2);
        let second = match list.filter_step(100, |row| row % 3 == 0) {
            Ok(value) => value,
            Err(error) => panic!("filter failed: {error}"),
        };
        assert_eq!((second.processed, second.matches, second.complete), (20, 7, true));
    }

    #[test]
    fn ctrl_and_shift_selection_scripts() {
        let mut list = model(10, RowHeight::Fixed(10.0));
        assert!(list.select_view(2, false, false));
        assert!(list.select_view(4, true, false));
        assert_eq!(list.selected_count(), 2);
        assert!(list.select_view(6, false, true));
        assert_eq!(list.selected_count(), 3);
        assert!(list.is_selected(4));
        assert!(list.is_selected(5));
        assert!(list.is_selected(6));
    }

    #[test]
    fn grouping_inserts_one_header_and_collapse_keeps_selection() {
        let mut list = model(
            6,
            RowHeight::Variable {
                estimate: 10.0,
                group_header: 15.0,
            },
        );
        assert_eq!(
            list.set_groups([(0, 10), (1, 10), (2, 10), (3, 20), (4, 20), (5, 20)]),
            Ok(())
        );
        assert_eq!(list.view_len(), 8);
        assert_eq!(list.view_item(0), Some(ViewItem::GroupHeader(10)));
        assert!(list.select_view(2, false, false));
        assert!(list.is_selected(1));
        assert_eq!(list.set_group_collapsed(10, true), Ok(()));
        assert_eq!(list.view_len(), 5);
        assert!(list.is_selected(1));
        assert!(list.group_collapsed(10));
    }

    #[test]
    fn keyboard_navigation_skips_group_headers() {
        let mut list = model(4, RowHeight::Fixed(10.0));
        assert_eq!(list.set_groups([(0, 1), (1, 1), (2, 2), (3, 2)]), Ok(()));
        assert_eq!(list.navigate(Navigation::Home, false), Some(0));
        assert_eq!(list.navigate(Navigation::Down, false), Some(1));
        assert_eq!(list.navigate(Navigation::Down, false), Some(2));
        assert_eq!(list.navigate(Navigation::End, false), Some(3));
        assert_eq!(list.navigate(Navigation::Up, false), Some(2));
    }

    #[test]
    fn typeahead_wraps_and_is_case_insensitive_for_cyrillic() {
        let mut list = model(4, RowHeight::Fixed(10.0));
        let labels = ["Alpha", "Болт", "Винтовка", "Ящик"];
        assert_eq!(list.navigate(Navigation::End, false), Some(3));
        let found = list.type_ahead('б', 100, 1000, |row| {
            usize::try_from(row)
                .ok()
                .and_then(|index| labels.get(index).copied())
                .unwrap_or("")
        });
        assert_eq!(found, Some(1));
    }

    #[test]
    fn typeahead_timeout_starts_new_prefix() {
        let mut list = model(3, RowHeight::Fixed(10.0));
        let labels = ["Alpha", "Beta", "Bravo"];
        assert_eq!(
            list.type_ahead('b', 100, 500, |row| {
                usize::try_from(row)
                    .ok()
                    .and_then(|index| labels.get(index).copied())
                    .unwrap_or("")
            }),
            Some(1)
        );
        assert_eq!(
            list.type_ahead('r', 200, 500, |row| {
                usize::try_from(row)
                    .ok()
                    .and_then(|index| labels.get(index).copied())
                    .unwrap_or("")
            }),
            Some(2)
        );
        assert_eq!(
            list.type_ahead('a', 1000, 500, |row| {
                usize::try_from(row)
                    .ok()
                    .and_then(|index| labels.get(index).copied())
                    .unwrap_or("")
            }),
            Some(0)
        );
    }

    #[test]
    fn columns_resolve_fixed_auto_fraction_and_user_resize() {
        let mut list = model(5, RowHeight::Fixed(10.0));
        list.set_columns(vec![
            Column::new(ColumnWidth::Fixed(100.0), 50.0, None),
            Column::new(
                ColumnWidth::Auto {
                    sample_rows: 3,
                    padding: 10.0,
                },
                40.0,
                Some(120.0),
            ),
            Column::new(ColumnWidth::Fraction(1.0), 20.0, None),
            Column::new(ColumnWidth::Fraction(2.0), 20.0, None),
        ]);
        let widths = list.resolve_columns(400.0, |column, row| {
            if column == 1 {
                match row {
                    0 => 50.0,
                    1 => 70.0,
                    _ => 60.0,
                }
            } else {
                0.0
            }
        });
        assert_eq!(widths, vec![100.0, 80.0, 73.333336, 146.66667]);
        assert_eq!(list.resize_column(2, 90.0), Ok(()));
        let resized = list.resolve_columns(400.0, |_, _| 0.0);
        assert_eq!(resized.get(2).copied(), Some(90.0));
    }

    #[test]
    fn select_all_visible_does_not_add_filtered_out_rows() {
        let mut list = model(12, RowHeight::Fixed(10.0));
        assert_eq!(list.begin_filter(), Ok(()));
        assert!(list.filter_step(12, |row| row < 4).is_ok());
        list.select_all_visible();
        assert_eq!(list.selected_count(), 4);
        assert!(!list.is_selected(8));
    }

    #[test]
    #[ignore = "capacity regression allocates state for one million rows"]
    fn million_rows_are_accepted_but_one_more_is_refused() {
        let rows = ids(MAX_ROWS);
        let list = ListModel::new(rows, RowHeight::Fixed(1.0));
        assert!(list.is_ok());
        let too_many = MAX_ROWS.saturating_add(1);
        let result = ListModel::new(ids(too_many), RowHeight::Fixed(1.0));
        assert!(result.is_err());
    }

    #[test]
    fn viewport_query_does_not_change_capacities() {
        let list = model(1000, RowHeight::Fixed(18.0));
        let before_view = list.view.capacity();
        let before_heights = list.heights.capacity();
        let before_tree = list.fenwick.tree.capacity();
        for offset in [0.0, 100.0, 5000.0, 17000.0] {
            let _ = list.visible_range(offset, 720.0, 180.0);
        }
        assert_eq!(list.view.capacity(), before_view);
        assert_eq!(list.heights.capacity(), before_heights);
        assert_eq!(list.fenwick.tree.capacity(), before_tree);
    }
}
