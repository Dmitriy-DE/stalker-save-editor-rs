//! Virtual sortable table controller over list::ListModel.

use crate::list::{ListModel, RowHeight, RowId, SortColumn, SortDirection, ViewItem, VisibleRange};
use sse_core::Result;
use std::cmp::Ordering;

/// Header state for one sortable column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    /// Header label.
    pub label: String,
    /// Whether clicking this header sorts.
    pub sortable: bool,
    /// Active direction, or none when this column is not a sort key.
    pub direction: Option<SortDirection>,
}

/// Virtual table: data stays outside; only RowIds and an index permutation live here.
pub struct Table {
    model: ListModel,
    headers: Vec<Header>,
    sort: Vec<SortColumn>,
    hover: Option<RowId>,
}

impl Table {
    /// Create a virtual table over stable row identifiers.
    pub fn new(rows: Vec<RowId>, row_height: f32, headers: Vec<Header>) -> Result<Self> {
        Ok(Self {
            model: ListModel::new(rows, RowHeight::Fixed(row_height))?,
            headers,
            sort: Vec::new(),
            hover: None,
        })
    }

    /// Range that must be realised for the current viewport.
    #[must_use]
    pub fn visible_range(&self, scroll_y: f32, viewport_height: f32) -> VisibleRange {
        self.model.visible_range(scroll_y, viewport_height, 80.0)
    }

    /// Resolve one virtual-view index to a data row.
    #[must_use]
    pub fn visible_row(&self, index: usize) -> Option<RowId> {
        match self.model.view_item(index) {
            Some(ViewItem::Row(id)) => Some(id),
            _ => None,
        }
    }

    /// Change the hovered virtual row.
    pub fn hover_view(&mut self, index: Option<usize>) -> bool {
        let next = index.and_then(|i| self.visible_row(i));
        let changed = next != self.hover;
        self.hover = next;
        changed
    }

    /// Stable id of the hovered row.
    #[must_use]
    pub const fn hovered(&self) -> Option<RowId> {
        self.hover
    }

    /// Apply single/Ctrl/Shift selection to a virtual row.
    pub fn select_view(&mut self, index: usize, ctrl: bool, shift: bool) -> bool {
        self.model.select_view(index, ctrl, shift)
    }

    /// Whether a stable row id is selected.
    #[must_use]
    pub fn selected(&self, row: RowId) -> bool {
        self.model.is_selected(row)
    }

    /// Click a header: first click ascending, repeated click toggles. Shift adds a secondary stable key.
    pub fn header_click<F>(&mut self, column: usize, shift: bool, compare: F) -> Result<bool>
    where
        F: FnMut(RowId, RowId, usize) -> Ordering,
    {
        let Some(header) = self.headers.get(column) else {
            return Ok(false);
        };
        if !header.sortable {
            return Ok(false);
        }
        let next = match header.direction {
            Some(SortDirection::Ascending) => SortDirection::Descending,
            _ => SortDirection::Ascending,
        };
        if !shift {
            self.sort.clear();
            for h in &mut self.headers {
                h.direction = None;
            }
        }
        if let Some(existing) = self.sort.iter_mut().find(|s| s.column == column) {
            existing.direction = next;
        } else {
            self.sort.push(SortColumn {
                column,
                direction: next,
            });
        }
        if let Some(h) = self.headers.get_mut(column) {
            h.direction = Some(next);
        }
        self.model.sort_by(&self.sort, compare)?;
        Ok(true)
    }

    /// Header states in display order.
    #[must_use]
    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    /// Number of items in the current sorted/filtered view.
    #[must_use]
    pub fn view_len(&self) -> usize {
        self.model.view_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_realise_only_viewport() {
        let rows = (0..10_000u64).collect();
        let t = Table::new(
            rows,
            40.0,
            vec![Header {
                label: "ID".into(),
                sortable: true,
                direction: None,
            }],
        )
        .unwrap();
        let r = t.visible_range(20_000.0, 400.0);
        assert!(r.end - r.start < 20);
        assert_eq!(t.view_len(), 10_000);
    }

    #[test]
    fn header_sorts_and_selection_tracks_id() {
        let mut t = Table::new(
            vec![3, 1, 2],
            40.0,
            vec![Header {
                label: "ID".into(),
                sortable: true,
                direction: None,
            }],
        )
        .unwrap();
        t.select_view(0, false, false);
        t.header_click(0, false, |a, b, _| a.cmp(&b)).unwrap();
        assert!(t.selected(3));
        assert_eq!(t.visible_row(0), Some(1));
    }
}
