//! Text-field editing model with a UTF-8 aware gap buffer.
//!
//! The model stores Unicode scalar values in a two-sided gap buffer. Caret and selection
//! positions are always kept on extended-grapheme boundaries; the segmentation implemented
//! here covers combining marks, variation selectors, emoji modifiers, regional-indicator
//! pairs, and ZWJ emoji sequences without external crates. Editing at the gap is amortised
//! O(1); navigation scans in place and does not allocate. Undo records only changed text,
//! rather than cloning the whole field after every key press.

use sse_core::{Error, Result};
use std::cmp::{max, min};
use std::collections::VecDeque;

const INITIAL_GAP_CAPACITY: usize = 32;
const DEFAULT_HISTORY_LIMIT: usize = 256;
const DEFAULT_MAX_GRAPHEMES: usize = 1_000_000;
const MAX_TEXT_SCALARS: usize = 4_000_000;

/// Whether the field accepts embedded line breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldMode {
    /// One logical line. Pasted line breaks are normalised to spaces.
    SingleLine,
    /// Multiple logical lines separated by `\n`.
    MultiLine,
}

/// Numeric input validation performed after an edit has been composed.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum InputFilter {
    /// Any Unicode text is accepted.
    #[default]
    Any,
    /// ASCII decimal digits only, optionally constrained by an inclusive range.
    Digits {
        /// Lowest accepted value.
        min: Option<u64>,
        /// Highest accepted value.
        max: Option<u64>,
        /// Whether an empty field is allowed while editing.
        allow_empty: bool,
    },
    /// Decimal money/count input converted to integer minor units without floating point.
    Money {
        /// Lowest accepted value in minor units.
        min_minor: Option<i64>,
        /// Highest accepted value in minor units.
        max_minor: Option<i64>,
        /// Number of fractional decimal places, from 0 through 9.
        decimals: u8,
        /// Whether a leading minus sign is accepted.
        allow_negative: bool,
        /// Whether an empty field is allowed while editing.
        allow_empty: bool,
    },
}

/// Immutable configuration for a text field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditConfig {
    /// Single- or multi-line behaviour.
    pub mode: FieldMode,
    /// Maximum number of grapheme clusters retained in the buffer.
    pub max_graphemes: usize,
    /// Maximum number of undo records retained.
    pub history_limit: usize,
    /// Optional numeric input validation.
    pub filter: InputFilter,
}

impl Default for EditConfig {
    fn default() -> Self {
        Self {
            mode: FieldMode::SingleLine,
            max_graphemes: DEFAULT_MAX_GRAPHEMES,
            history_limit: DEFAULT_HISTORY_LIMIT,
            filter: InputFilter::Any,
        }
    }
}

/// A directional selection. `anchor` stays fixed while Shift extends `caret`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Fixed end of a Shift-selection, as a Unicode-scalar index.
    pub anchor: usize,
    /// Active caret end, as a Unicode-scalar index.
    pub caret: usize,
}

impl Selection {
    /// A collapsed selection at `position`.
    #[must_use]
    pub const fn collapsed(position: usize) -> Self {
        Self {
            anchor: position,
            caret: position,
        }
    }

    /// Whether no text is selected.
    #[must_use]
    pub const fn is_collapsed(self) -> bool {
        self.anchor == self.caret
    }

    /// Ordered `(start, end)` scalar indices.
    #[must_use]
    pub fn ordered(self) -> (usize, usize) {
        (min(self.anchor, self.caret), max(self.anchor, self.caret))
    }
}

/// Mouse click interpretation for text selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseSelect {
    /// Place the caret at a grapheme boundary.
    Caret,
    /// Select the word at the clicked grapheme.
    Word,
    /// Select the whole logical line, including its trailing newline when present.
    Line,
}

/// Modifier state used by [`EditModel::key`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Ctrl/Command-like word or command modifier.
    pub ctrl: bool,
    /// Extend the existing selection.
    pub shift: bool,
}

/// Logical key handled by the editor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// Insert one Unicode scalar.
    Character(char),
    /// Move to the previous grapheme or word.
    Left,
    /// Move to the next grapheme or word.
    Right,
    /// Move one visual text line up by logical grapheme column.
    Up,
    /// Move one visual text line down by logical grapheme column.
    Down,
    /// Move to line start, or document start with Ctrl.
    Home,
    /// Move to line end, or document end with Ctrl.
    End,
    /// Delete the previous grapheme or word.
    Backspace,
    /// Delete the next grapheme or word.
    Delete,
    /// Insert a line break in multi-line mode.
    Enter,
    /// Select all with Ctrl.
    A,
    /// Copy with Ctrl.
    C,
    /// Cut with Ctrl.
    X,
    /// Paste with Ctrl.
    V,
    /// Undo with Ctrl.
    Z,
    /// Redo with Ctrl.
    Y,
}

/// Clipboard abstraction; UI/platform code supplies the implementation.
pub trait Clipboard {
    /// Reads clipboard text.
    ///
    /// # Errors
    /// Returns a system/platform error when clipboard access fails.
    fn read_text(&mut self) -> Result<String>;

    /// Replaces clipboard text.
    ///
    /// # Errors
    /// Returns a system/platform error when clipboard access fails.
    fn write_text(&mut self, text: &str) -> Result<()>;
}

/// Active IME text overlaid on top of an unmodified buffer range.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Composition {
    /// Buffer range that the composition visually replaces.
    pub range: Selection,
    /// Current IME composition text.
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CoalesceKind {
    None,
    Typing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct EditRecord {
    start: usize,
    deleted: String,
    inserted: String,
    before: Selection,
    after: Selection,
    coalesce: CoalesceKind,
    before_graphemes: usize,
    after_graphemes: usize,
}

/// Two-sided gap buffer. `left` is in document order; `right` is reversed so its last scalar
/// is the first scalar after the gap.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GapBuffer {
    left: Vec<char>,
    right: Vec<char>,
}

impl GapBuffer {
    fn from_text(text: &str) -> Result<Self> {
        let count = text.chars().count();
        if count > MAX_TEXT_SCALARS {
            return Err(Error::Refused("text field exceeds scalar limit".to_owned()));
        }
        let capacity = count
            .checked_add(INITIAL_GAP_CAPACITY)
            .ok_or_else(|| Error::Refused("text field capacity overflow".to_owned()))?;
        let mut left = Vec::with_capacity(capacity);
        left.extend(text.chars());
        Ok(Self {
            left,
            right: Vec::with_capacity(INITIAL_GAP_CAPACITY),
        })
    }

    fn len(&self) -> usize {
        self.left.len().saturating_add(self.right.len())
    }

    fn char_at(&self, index: usize) -> Option<char> {
        if index < self.left.len() {
            return self.left.get(index).copied();
        }
        let relative = index.checked_sub(self.left.len())?;
        let reverse = self.right.len().checked_sub(relative.checked_add(1)?)?;
        self.right.get(reverse).copied()
    }

    fn move_gap(&mut self, target: usize) -> Result<()> {
        if target > self.len() {
            return Err(Error::Damaged("caret outside text buffer".to_owned()));
        }
        while self.left.len() > target {
            let Some(value) = self.left.pop() else {
                return Err(Error::Damaged("gap move underflow".to_owned()));
            };
            self.right.push(value);
        }
        while self.left.len() < target {
            let Some(value) = self.right.pop() else {
                return Err(Error::Damaged("gap move overflow".to_owned()));
            };
            self.left.push(value);
        }
        Ok(())
    }

    fn replace(&mut self, start: usize, end: usize, inserted: &str) -> Result<String> {
        if start > end || end > self.len() {
            return Err(Error::Damaged("edit range outside text buffer".to_owned()));
        }
        self.move_gap(start)?;
        let remove_count = end.saturating_sub(start);
        let mut deleted = String::new();
        for _ in 0..remove_count {
            let Some(value) = self.right.pop() else {
                return Err(Error::Damaged("edit range ended inside gap".to_owned()));
            };
            deleted.push(value);
        }
        let inserted_count = inserted.chars().count();
        let new_len = self
            .len()
            .checked_add(inserted_count)
            .ok_or_else(|| Error::Refused("text field length overflow".to_owned()))?;
        if new_len > MAX_TEXT_SCALARS {
            return Err(Error::Refused("text field exceeds scalar limit".to_owned()));
        }
        self.left.extend(inserted.chars());
        Ok(deleted)
    }

    fn append_range(&self, start: usize, end: usize, out: &mut String) -> Result<()> {
        if start > end || end > self.len() {
            return Err(Error::Damaged("selection outside text buffer".to_owned()));
        }
        let mut index = start;
        while index < end {
            let Some(value) = self.char_at(index) else {
                return Err(Error::Damaged("selection crossed gap incorrectly".to_owned()));
            };
            out.push(value);
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::Damaged("selection index overflow".to_owned()))?;
        }
        Ok(())
    }

    fn slice_string(&self, start: usize, end: usize) -> Result<String> {
        let mut out = String::new();
        self.append_range(start, end, &mut out)?;
        Ok(out)
    }

    fn collect_text(&self) -> String {
        let mut out = String::with_capacity(self.len());
        out.extend(self.left.iter().copied());
        out.extend(self.right.iter().rev().copied());
        out
    }
}

/// Stateful text editor model.
#[derive(Clone, Debug)]
pub struct EditModel {
    buffer: GapBuffer,
    selection: Selection,
    config: EditConfig,
    history: VecDeque<EditRecord>,
    history_position: usize,
    preferred_column: Option<usize>,
    composition: Option<Composition>,
    horizontal_scroll: f32,
    grapheme_count: usize,
    paste_refusal: Option<PasteRefusal>,
}

/// Why a paste inserted nothing, so the field can tell the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteRefusal {
    /// The clipboard text does not fit in the remaining space.
    TooLong,
    /// The text fits in length but the field's filter rejects it.
    NotAllowed,
}

impl EditModel {
    /// Creates an editor with validated initial text.
    ///
    /// # Errors
    /// Returns [`Error::Refused`] when the initial text violates configured limits/filtering.
    pub fn new(initial: &str, config: EditConfig) -> Result<Self> {
        if config.max_graphemes == 0 {
            return Err(Error::Refused("max_graphemes must be non-zero".to_owned()));
        }
        if let InputFilter::Money { decimals, .. } = &config.filter {
            if *decimals > 9 {
                return Err(Error::Refused("money decimals must be <= 9".to_owned()));
            }
        }
        let initial = normalise_for_mode(initial, config.mode);
        let grapheme_count = grapheme_count_str(&initial);
        if grapheme_count > config.max_graphemes {
            return Err(Error::Refused("initial text exceeds grapheme limit".to_owned()));
        }
        if !filter_accepts(&config.filter, &initial) {
            return Err(Error::Refused("initial text rejected by input filter".to_owned()));
        }
        let buffer = GapBuffer::from_text(&initial)?;
        let caret = buffer.len();
        Ok(Self {
            buffer,
            selection: Selection::collapsed(caret),
            config,
            history: VecDeque::new(),
            history_position: 0,
            preferred_column: None,
            composition: None,
            horizontal_scroll: 0.0,
            grapheme_count,
            paste_refusal: None,
        })
    }

    /// Current text. This is the only whole-buffer allocation required for ordinary display.
    #[must_use]
    pub fn text(&self) -> String {
        self.buffer.collect_text()
    }

    /// Current directional selection.
    #[must_use]
    pub const fn selection(&self) -> Selection {
        self.selection
    }

    /// Current IME overlay, if any. The underlying buffer is unchanged until commit.
    #[must_use]
    pub fn composition(&self) -> Option<&Composition> {
        self.composition.as_ref()
    }

    /// Horizontal scroll offset maintained by [`Self::ensure_caret_visible`].
    #[must_use]
    pub const fn horizontal_scroll(&self) -> f32 {
        self.horizontal_scroll
    }

    /// Number of grapheme clusters in the buffer.
    #[must_use]
    pub const fn grapheme_count(&self) -> usize {
        self.grapheme_count
    }

    /// Places the caret at a grapheme number, clamped to the document.
    pub fn set_caret_grapheme(&mut self, grapheme: usize, extend: bool) {
        let target = scalar_for_grapheme(&self.buffer, grapheme);
        self.apply_movement(target, extend);
        self.preferred_column = None;
    }

    /// Selects by a mouse click expressed as a grapheme number.
    pub fn mouse_select(&mut self, grapheme: usize, kind: MouseSelect, extend: bool) {
        let position = scalar_for_grapheme(&self.buffer, grapheme);
        match kind {
            MouseSelect::Caret => self.apply_movement(position, extend),
            MouseSelect::Word => {
                let (start, end) = word_range(&self.buffer, position);
                self.selection = Selection {
                    anchor: start,
                    caret: end,
                };
            }
            MouseSelect::Line => {
                let start = line_start(&self.buffer, position);
                let mut end = line_end(&self.buffer, position);
                if matches!(self.buffer.char_at(end), Some('\n')) {
                    end = end.checked_add(1).unwrap_or(end);
                }
                self.selection = Selection {
                    anchor: start,
                    caret: end,
                };
            }
        }
        self.preferred_column = None;
        self.composition = None;
    }

    /// Inserts/replaces text using field limits and validation.
    ///
    /// Returns `Ok(false)` when the candidate is syntactically valid editing input but rejected
    /// by the configured maximum length or numeric filter.
    ///
    /// # Errors
    /// Returns an error only for structural overflow/damaged internal ranges.
    pub fn insert_text(&mut self, text: &str) -> Result<bool> {
        let text = normalise_for_mode(text, self.config.mode);
        self.replace_selection(&text, CoalesceKind::Typing)
    }

    /// Starts or replaces an IME composition overlay over the current selection.
    pub fn set_composition(&mut self, text: impl Into<String>) {
        let (start, end) = self.selection.ordered();
        self.composition = Some(Composition {
            range: Selection {
                anchor: start,
                caret: end,
            },
            text: text.into(),
        });
    }

    /// Changes the active IME overlay text without touching undo history.
    pub fn update_composition(&mut self, text: impl Into<String>) {
        if let Some(composition) = self.composition.as_mut() {
            composition.text = text.into();
        } else {
            self.set_composition(text);
        }
    }

    /// Commits the active IME overlay as one undoable edit.
    ///
    /// # Errors
    /// Returns an error for an invalid overlay range or structural overflow.
    pub fn commit_composition(&mut self) -> Result<bool> {
        let Some(composition) = self.composition.take() else {
            return Ok(false);
        };
        self.selection = composition.range;
        let text = normalise_for_mode(&composition.text, self.config.mode);
        self.replace_selection(&text, CoalesceKind::None)
    }

    /// Discards an active IME overlay.
    pub fn cancel_composition(&mut self) {
        self.composition = None;
    }

    /// Keeps an externally measured caret X position inside the visible horizontal viewport.
    /// Invalid/non-finite geometry is ignored.
    pub fn ensure_caret_visible(&mut self, caret_x: f32, viewport_width: f32, margin: f32) {
        if !caret_x.is_finite() || !viewport_width.is_finite() || !margin.is_finite() {
            return;
        }
        if viewport_width <= 0.0 || margin < 0.0 {
            return;
        }
        let left_limit = self.horizontal_scroll.mul_add(1.0, margin);
        if caret_x < left_limit {
            self.horizontal_scroll = float_sub(caret_x, margin).max(0.0);
            return;
        }
        let usable = float_sub(viewport_width, margin).max(0.0);
        let right_limit = self.horizontal_scroll.mul_add(1.0, usable);
        if caret_x > right_limit {
            self.horizontal_scroll = float_sub(caret_x, usable).max(0.0);
        }
    }

    /// Executes one logical key command.
    ///
    /// # Errors
    /// Clipboard commands forward clipboard failures; editing errors report structural overflow.
    pub fn key<C: Clipboard>(&mut self, key: Key, modifiers: Modifiers, clipboard: &mut C) -> Result<bool> {
        if modifiers.ctrl {
            match key {
                Key::A => {
                    self.selection = Selection {
                        anchor: 0,
                        caret: self.buffer.len(),
                    };
                    self.break_coalescing();
                    return Ok(true);
                }
                Key::C => return self.copy(clipboard),
                Key::X => return self.cut(clipboard),
                Key::V => return self.paste(clipboard),
                Key::Z => return self.undo(),
                Key::Y => return self.redo(),
                _ => {}
            }
        }

        match key {
            Key::Character(value) if !modifiers.ctrl => {
                let mut encoded = String::new();
                encoded.push(value);
                self.insert_text(&encoded)
            }
            Key::Enter if !modifiers.ctrl && self.config.mode == FieldMode::MultiLine => self.insert_text("\n"),
            Key::Left => {
                self.move_left(modifiers.ctrl, modifiers.shift);
                Ok(true)
            }
            Key::Right => {
                self.move_right(modifiers.ctrl, modifiers.shift);
                Ok(true)
            }
            Key::Up => {
                self.move_vertical(false, modifiers.shift);
                Ok(true)
            }
            Key::Down => {
                self.move_vertical(true, modifiers.shift);
                Ok(true)
            }
            Key::Home => {
                let target = if modifiers.ctrl {
                    0
                } else {
                    line_start(&self.buffer, self.selection.caret)
                };
                self.apply_movement(target, modifiers.shift);
                self.break_coalescing();
                Ok(true)
            }
            Key::End => {
                let target = if modifiers.ctrl {
                    self.buffer.len()
                } else {
                    line_end(&self.buffer, self.selection.caret)
                };
                self.apply_movement(target, modifiers.shift);
                self.break_coalescing();
                Ok(true)
            }
            Key::Backspace => self.backspace(modifiers.ctrl),
            Key::Delete => self.delete_forward(modifiers.ctrl),
            _ => Ok(false),
        }
    }

    /// Copies selected text.
    ///
    /// # Errors
    /// Forwards clipboard failures.
    pub fn copy<C: Clipboard>(&self, clipboard: &mut C) -> Result<bool> {
        let (start, end) = self.selection.ordered();
        if start == end {
            return Ok(false);
        }
        let text = self.buffer.slice_string(start, end)?;
        clipboard.write_text(&text)?;
        Ok(true)
    }

    /// Copies then deletes selected text as one undoable edit.
    ///
    /// # Errors
    /// Forwards clipboard failures or editing errors.
    pub fn cut<C: Clipboard>(&mut self, clipboard: &mut C) -> Result<bool> {
        let (start, end) = self.selection.ordered();
        if start == end {
            return Ok(false);
        }
        let text = self.buffer.slice_string(start, end)?;
        clipboard.write_text(&text)?;
        self.replace_selection("", CoalesceKind::None)
    }

    /// Inserts clipboard text.
    ///
    /// # Errors
    /// Forwards clipboard failures or editing errors.
    ///
    /// Text that does not fit the limit is cut at the longest prefix that does. Only a paste that
    /// inserts nothing returns `Ok(false)`, so the caller can tell a refused paste from a shortened one.
    ///
    /// # Errors
    /// Returns an error only if the clipboard fails or the buffer and history disagree.
    pub fn paste<C: Clipboard>(&mut self, clipboard: &mut C) -> Result<bool> {
        let text = clipboard.read_text()?;
        let text = normalise_for_mode(&text, self.config.mode);
        let (start, end) = self.selection.ordered();
        self.paste_refusal = None;
        if self.candidate_allowed(start, end, &text)?.is_some() {
            return self.replace_selection(&text, CoalesceKind::None);
        }
        let mut ends: Vec<usize> = text.char_indices().skip(1).map(|(index, _)| index).collect();
        ends.push(text.len());
        // Binary search for the longest accepted prefix; acceptance only grows with length up to the limit.
        let (mut low, mut high) = (0, ends.len());
        while low < high {
            let middle = low.saturating_add(high.saturating_sub(low) / 2);
            let prefix = text.get(..ends.get(middle).copied().unwrap_or(0)).unwrap_or("");
            if self.candidate_allowed(start, end, prefix)?.is_some() {
                low = middle.saturating_add(1);
            } else {
                high = middle;
            }
        }
        let accepted = low
            .checked_sub(1)
            .and_then(|index| ends.get(index))
            .copied()
            .unwrap_or(0);
        let prefix = text.get(..accepted).unwrap_or("");
        if prefix.is_empty() {
            let removed = end.saturating_sub(start);
            let after = self
                .grapheme_count
                .saturating_sub(removed)
                .saturating_add(text.chars().count());
            self.paste_refusal = Some(if after > self.config.max_graphemes {
                PasteRefusal::TooLong
            } else {
                PasteRefusal::NotAllowed
            });
            self.break_coalescing();
            return Ok(false);
        }
        self.replace_selection(prefix, CoalesceKind::None)
    }

    /// Takes the reason the last paste inserted nothing, if any.
    pub fn take_paste_refusal(&mut self) -> Option<PasteRefusal> {
        self.paste_refusal.take()
    }

    /// Undoes one coalesced edit.
    ///
    /// # Errors
    /// Returns an error only if internal history ranges no longer match the buffer.
    pub fn undo(&mut self) -> Result<bool> {
        if self.history_position == 0 {
            return Ok(false);
        }
        let index = self.history_position.saturating_sub(1);
        let Some(record) = self.history.get(index).cloned() else {
            return Err(Error::Damaged("undo history index missing".to_owned()));
        };
        let inserted_len = record.inserted.chars().count();
        let end = record
            .start
            .checked_add(inserted_len)
            .ok_or_else(|| Error::Damaged("undo range overflow".to_owned()))?;
        self.buffer.replace(record.start, end, &record.deleted)?;
        self.grapheme_count = record.before_graphemes;
        self.selection = record.before;
        self.history_position = index;
        self.composition = None;
        self.preferred_column = None;
        Ok(true)
    }

    /// Redoes one edit.
    ///
    /// # Errors
    /// Returns an error only if internal history ranges no longer match the buffer.
    pub fn redo(&mut self) -> Result<bool> {
        let Some(record) = self.history.get(self.history_position).cloned() else {
            return Ok(false);
        };
        let deleted_len = record.deleted.chars().count();
        let end = record
            .start
            .checked_add(deleted_len)
            .ok_or_else(|| Error::Damaged("redo range overflow".to_owned()))?;
        self.buffer.replace(record.start, end, &record.inserted)?;
        self.grapheme_count = record.after_graphemes;
        self.selection = record.after;
        self.history_position = self
            .history_position
            .checked_add(1)
            .ok_or_else(|| Error::Damaged("redo history overflow".to_owned()))?;
        self.composition = None;
        self.preferred_column = None;
        Ok(true)
    }

    fn replace_selection(&mut self, inserted: &str, coalesce: CoalesceKind) -> Result<bool> {
        let (start, end) = self.selection.ordered();
        let Some(after_graphemes) = self.candidate_allowed(start, end, inserted)? else {
            self.break_coalescing();
            return Ok(false);
        };
        let before = self.selection;
        let before_graphemes = self.grapheme_count;
        let deleted = self.buffer.replace(start, end, inserted)?;
        if deleted.is_empty() && inserted.is_empty() {
            return Ok(false);
        }
        self.grapheme_count = after_graphemes;
        let inserted_len = inserted.chars().count();
        let caret = start
            .checked_add(inserted_len)
            .ok_or_else(|| Error::Damaged("caret overflow after edit".to_owned()))?;
        self.selection = Selection::collapsed(caret);
        let record = EditRecord {
            start,
            deleted,
            inserted: inserted.to_owned(),
            before,
            after: self.selection,
            coalesce,
            before_graphemes,
            after_graphemes,
        };
        self.record_history(record)?;
        self.composition = None;
        self.preferred_column = None;
        Ok(true)
    }

    fn candidate_allowed(&self, start: usize, end: usize, inserted: &str) -> Result<Option<usize>> {
        let left = if start == 0 {
            0
        } else {
            previous_grapheme(&self.buffer, start)
        };
        let right = if end >= self.buffer.len() {
            self.buffer.len()
        } else {
            next_grapheme(&self.buffer, end)
        };
        let old_local = grapheme_count_range(&self.buffer, left, right);
        let mut local = String::with_capacity(inserted.len().saturating_add(16));
        self.buffer.append_range(left, start, &mut local)?;
        local.push_str(inserted);
        self.buffer.append_range(end, right, &mut local)?;
        let new_local = grapheme_count_str(&local);
        let retained = self.grapheme_count.saturating_sub(old_local);
        let Some(candidate_count) = retained.checked_add(new_local) else {
            return Ok(None);
        };
        if candidate_count > self.config.max_graphemes {
            return Ok(None);
        }
        if matches!(&self.config.filter, InputFilter::Any) {
            return Ok(Some(candidate_count));
        }
        let mut candidate = String::new();
        let mut index = 0_usize;
        while index < start {
            let Some(value) = self.buffer.char_at(index) else {
                return Err(Error::Damaged("filter prefix outside buffer".to_owned()));
            };
            candidate.push(value);
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::Damaged("filter prefix overflow".to_owned()))?;
        }
        candidate.push_str(inserted);
        index = end;
        while index < self.buffer.len() {
            let Some(value) = self.buffer.char_at(index) else {
                return Err(Error::Damaged("filter suffix outside buffer".to_owned()));
            };
            candidate.push(value);
            index = index
                .checked_add(1)
                .ok_or_else(|| Error::Damaged("filter suffix overflow".to_owned()))?;
        }
        if filter_accepts(&self.config.filter, &candidate) {
            Ok(Some(candidate_count))
        } else {
            Ok(None)
        }
    }

    fn record_history(&mut self, record: EditRecord) -> Result<()> {
        if self.config.history_limit == 0 {
            self.history.clear();
            self.history_position = 0;
            return Ok(());
        }
        self.history.truncate(self.history_position);
        let can_coalesce = record.coalesce == CoalesceKind::Typing
            && record.deleted.is_empty()
            && self.history.back().is_some_and(|previous| {
                if previous.coalesce != CoalesceKind::Typing || !previous.deleted.is_empty() {
                    return false;
                }
                let previous_inserted = previous.inserted.chars().count();
                previous
                    .start
                    .checked_add(previous_inserted)
                    .is_some_and(|end| end == record.start && previous.after == record.before)
            });
        if can_coalesce {
            let Some(previous) = self.history.back_mut() else {
                return Err(Error::Damaged("coalescing history disappeared".to_owned()));
            };
            previous.inserted.push_str(&record.inserted);
            previous.after = record.after;
            previous.after_graphemes = record.after_graphemes;
            self.history_position = self.history.len();
            return Ok(());
        }
        if self.history.len() >= self.config.history_limit {
            let _ = self.history.pop_front();
            self.history_position = self.history_position.saturating_sub(1);
        }
        self.history.push_back(record);
        self.history_position = self.history.len();
        Ok(())
    }

    fn break_coalescing(&mut self) {
        if let Some(last) = self.history.back_mut() {
            last.coalesce = CoalesceKind::None;
        }
        self.preferred_column = None;
        self.composition = None;
    }

    fn apply_movement(&mut self, target: usize, extend: bool) {
        let target = target.min(self.buffer.len());
        if extend {
            self.selection.caret = target;
        } else {
            self.selection = Selection::collapsed(target);
        }
        self.composition = None;
    }

    fn move_left(&mut self, word: bool, extend: bool) {
        let target = if !extend && !self.selection.is_collapsed() {
            self.selection.ordered().0
        } else if word {
            previous_word_boundary(&self.buffer, self.selection.caret)
        } else {
            previous_grapheme(&self.buffer, self.selection.caret)
        };
        self.apply_movement(target, extend);
        self.break_coalescing();
    }

    fn move_right(&mut self, word: bool, extend: bool) {
        let target = if !extend && !self.selection.is_collapsed() {
            self.selection.ordered().1
        } else if word {
            next_word_boundary(&self.buffer, self.selection.caret)
        } else {
            next_grapheme(&self.buffer, self.selection.caret)
        };
        self.apply_movement(target, extend);
        self.break_coalescing();
    }

    fn move_vertical(&mut self, down: bool, extend: bool) {
        if self.config.mode == FieldMode::SingleLine {
            if down {
                self.move_right(false, extend);
            } else {
                self.move_left(false, extend);
            }
            return;
        }
        let caret = self.selection.caret;
        let start = line_start(&self.buffer, caret);
        let column = self
            .preferred_column
            .unwrap_or_else(|| grapheme_count_range(&self.buffer, start, caret));
        self.preferred_column = Some(column);
        let target = if down {
            let end = line_end(&self.buffer, caret);
            if end >= self.buffer.len() {
                self.buffer.len()
            } else {
                let next_start = end.checked_add(1).unwrap_or(end);
                let next_end = line_end(&self.buffer, next_start);
                advance_graphemes(&self.buffer, next_start, next_end, column)
            }
        } else if start == 0 {
            0
        } else {
            let previous_line_end = start.saturating_sub(1);
            let previous_start = line_start(&self.buffer, previous_line_end);
            advance_graphemes(&self.buffer, previous_start, previous_line_end, column)
        };
        self.apply_movement(target, extend);
        if let Some(last) = self.history.back_mut() {
            last.coalesce = CoalesceKind::None;
        }
    }

    fn backspace(&mut self, word: bool) -> Result<bool> {
        if !self.selection.is_collapsed() {
            return self.replace_selection("", CoalesceKind::None);
        }
        let caret = self.selection.caret;
        if caret == 0 {
            return Ok(false);
        }
        let start = if word {
            previous_word_boundary(&self.buffer, caret)
        } else {
            previous_grapheme(&self.buffer, caret)
        };
        self.selection = Selection { anchor: start, caret };
        self.replace_selection("", CoalesceKind::None)
    }

    fn delete_forward(&mut self, word: bool) -> Result<bool> {
        if !self.selection.is_collapsed() {
            return self.replace_selection("", CoalesceKind::None);
        }
        let caret = self.selection.caret;
        if caret >= self.buffer.len() {
            return Ok(false);
        }
        let end = if word {
            next_word_boundary(&self.buffer, caret)
        } else {
            next_grapheme(&self.buffer, caret)
        };
        self.selection = Selection {
            anchor: caret,
            caret: end,
        };
        self.replace_selection("", CoalesceKind::None)
    }
}

fn normalise_for_mode(text: &str, mode: FieldMode) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(value) = chars.next() {
        if value == '\r' {
            if chars.peek().is_some_and(|next| *next == '\n') {
                let _ = chars.next();
            }
            out.push(if mode == FieldMode::MultiLine { '\n' } else { ' ' });
        } else if value == '\n' {
            out.push(if mode == FieldMode::MultiLine { '\n' } else { ' ' });
        } else {
            out.push(value);
        }
    }
    out
}

fn filter_accepts(filter: &InputFilter, text: &str) -> bool {
    match filter {
        InputFilter::Any => true,
        InputFilter::Digits { min, max, allow_empty } => {
            if text.is_empty() {
                return *allow_empty;
            }
            if !text.chars().all(|value| value.is_ascii_digit()) {
                return false;
            }
            let Ok(value) = text.parse::<u64>() else {
                return false;
            };
            min.is_none_or(|lower| value >= lower) && max.is_none_or(|upper| value <= upper)
        }
        InputFilter::Money {
            min_minor,
            max_minor,
            decimals,
            allow_negative,
            allow_empty,
        } => {
            if text.is_empty() {
                return *allow_empty;
            }
            let Some(value) = parse_money_minor(text, *decimals, *allow_negative) else {
                return false;
            };
            min_minor.is_none_or(|lower| value >= lower) && max_minor.is_none_or(|upper| value <= upper)
        }
    }
}

fn parse_money_minor(text: &str, decimals: u8, allow_negative: bool) -> Option<i64> {
    if decimals > 9 {
        return None;
    }
    let mut negative = false;
    let mut seen_digit = false;
    let mut seen_separator = false;
    let mut whole = 0_i64;
    let mut fraction = 0_i64;
    let mut fractional_digits = 0_u8;
    let mut first = true;
    for value in text.chars() {
        if first && value == '-' {
            if !allow_negative {
                return None;
            }
            negative = true;
            first = false;
            continue;
        }
        first = false;
        if value == '.' || value == ',' {
            if seen_separator || decimals == 0 {
                return None;
            }
            seen_separator = true;
            continue;
        }
        let digit = value.to_digit(10)?;
        if !value.is_ascii_digit() {
            return None;
        }
        seen_digit = true;
        let digit = i64::from(digit);
        if seen_separator {
            if fractional_digits >= decimals {
                return None;
            }
            fraction = fraction.checked_mul(10)?.checked_add(digit)?;
            fractional_digits = fractional_digits.checked_add(1)?;
        } else {
            whole = whole.checked_mul(10)?.checked_add(digit)?;
        }
    }
    if !seen_digit {
        return None;
    }
    let mut scale = 1_i64;
    for _ in 0..decimals {
        scale = scale.checked_mul(10)?;
    }
    let missing = decimals.saturating_sub(fractional_digits);
    for _ in 0..missing {
        fraction = fraction.checked_mul(10)?;
    }
    let value = whole.checked_mul(scale)?.checked_add(fraction)?;
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

fn float_sub(left: f32, right: f32) -> f32 {
    left.mul_add(1.0, right.copysign(-1.0))
}

fn is_combining(value: char) -> bool {
    matches!(
        u32::from(value),
        0x0300..=0x036F
            | 0x0483..=0x0489
            | 0x0591..=0x05BD
            | 0x05BF
            | 0x05C1..=0x05C2
            | 0x05C4..=0x05C5
            | 0x0610..=0x061A
            | 0x064B..=0x065F
            | 0x0670
            | 0x06D6..=0x06ED
            | 0x0711
            | 0x0730..=0x074A
            | 0x07A6..=0x07B0
            | 0x07EB..=0x07F3
            | 0x0816..=0x082D
            | 0x0859..=0x085B
            | 0x08D3..=0x0903
            | 0x093A..=0x094D
            | 0x0951..=0x0957
            | 0x0962..=0x0963
            | 0x0981..=0x0983
            | 0x09BC
            | 0x09BE..=0x09CD
            | 0x09D7
            | 0x09E2..=0x09E3
            | 0x0A01..=0x0A03
            | 0x0A3C
            | 0x0A3E..=0x0A4D
            | 0x0A51
            | 0x0A70..=0x0A71
            | 0x0A75
            | 0x0ABC
            | 0x0ABE..=0x0ACD
            | 0x0AE2..=0x0AE3
            | 0x0B01..=0x0B03
            | 0x0B3C
            | 0x0B3E..=0x0B4D
            | 0x0B56..=0x0B57
            | 0x0B62..=0x0B63
            | 0x0BBE..=0x0BCD
            | 0x0BD7
            | 0x0C00..=0x0C04
            | 0x0C3E..=0x0C56
            | 0x0C62..=0x0C63
            | 0x0C81..=0x0C83
            | 0x0CBC
            | 0x0CBE..=0x0CD6
            | 0x0CE2..=0x0CE3
            | 0x0D00..=0x0D03
            | 0x0D3B..=0x0D4D
            | 0x0D57
            | 0x0D62..=0x0D63
            | 0x0D82..=0x0D83
            | 0x0DCF..=0x0DDF
            | 0x0E31
            | 0x0E34..=0x0E3A
            | 0x0E47..=0x0E4E
            | 0x0EB1
            | 0x0EB4..=0x0EBC
            | 0x0EC8..=0x0ECD
            | 0x0F18..=0x0F19
            | 0x0F35
            | 0x0F37
            | 0x0F39
            | 0x0F71..=0x0F84
            | 0x0F86..=0x0F87
            | 0x0F8D..=0x0FBC
            | 0x102B..=0x103E
            | 0x1056..=0x1059
            | 0x105E..=0x1060
            | 0x1062..=0x1064
            | 0x1067..=0x106D
            | 0x1071..=0x1074
            | 0x1082..=0x108D
            | 0x109A..=0x109D
            | 0x135D..=0x135F
            | 0x1712..=0x1715
            | 0x1732..=0x1734
            | 0x1752..=0x1753
            | 0x1772..=0x1773
            | 0x17B4..=0x17D3
            | 0x17DD
            | 0x180B..=0x180D
            | 0x1885..=0x1886
            | 0x18A9
            | 0x1920..=0x192B
            | 0x1930..=0x193B
            | 0x1A17..=0x1A1B
            | 0x1A55..=0x1A7F
            | 0x1AB0..=0x1ACE
            | 0x1B00..=0x1B04
            | 0x1B34..=0x1B44
            | 0x1B6B..=0x1B73
            | 0x1B80..=0x1B82
            | 0x1BA1..=0x1BAD
            | 0x1BE6..=0x1BF3
            | 0x1C24..=0x1C37
            | 0x1CD0..=0x1CD2
            | 0x1CD4..=0x1CE8
            | 0x1CED
            | 0x1CF2..=0x1CF4
            | 0x1CF7..=0x1CF9
            | 0x1DC0..=0x1DFF
            | 0x20D0..=0x20FF
            | 0x2CEF..=0x2CF1
            | 0x2D7F
            | 0x2DE0..=0x2DFF
            | 0x302A..=0x302F
            | 0x3099..=0x309A
            | 0xA66F
            | 0xA674..=0xA67D
            | 0xA69E..=0xA69F
            | 0xA6F0..=0xA6F1
            | 0xA802
            | 0xA806
            | 0xA80B
            | 0xA823..=0xA827
            | 0xA880..=0xA881
            | 0xA8B4..=0xA8C5
            | 0xA8E0..=0xA8F1
            | 0xA8FF
            | 0xA926..=0xA92D
            | 0xA947..=0xA953
            | 0xA980..=0xA983
            | 0xA9B3..=0xA9C0
            | 0xA9E5
            | 0xAA29..=0xAA36
            | 0xAA43
            | 0xAA4C..=0xAA4D
            | 0xAA7B..=0xAA7D
            | 0xAAB0
            | 0xAAB2..=0xAAB4
            | 0xAAB7..=0xAAB8
            | 0xAABE..=0xAABF
            | 0xAAC1
            | 0xAAEB..=0xAAEF
            | 0xAAF5..=0xAAF6
            | 0xABE3..=0xABEA
            | 0xABEC..=0xABED
            | 0xFB1E
            | 0xFE00..=0xFE0F
            | 0xFE20..=0xFE2F
            | 0x101FD
            | 0x102E0
            | 0x10376..=0x1037A
            | 0x10A01..=0x10A03
            | 0x10A05..=0x10A06
            | 0x10A0C..=0x10A0F
            | 0x10A38..=0x10A3A
            | 0x10A3F
            | 0x10AE5..=0x10AE6
            | 0x11000..=0x11002
            | 0x11038..=0x11046
            | 0x1107F..=0x11082
            | 0x110B0..=0x110BA
            | 0x11100..=0x11102
            | 0x11127..=0x11134
            | 0x11145..=0x11146
            | 0x11173
            | 0x11180..=0x11182
            | 0x111B3..=0x111C0
            | 0x111CA..=0x111CC
            | 0x1122C..=0x11237
            | 0x1123E
            | 0x112DF..=0x112EA
            | 0x11300..=0x11303
            | 0x1133B..=0x1134D
            | 0x11357
            | 0x11362..=0x11363
            | 0x11435..=0x11446
            | 0x114B0..=0x114C3
            | 0x115AF..=0x115B5
            | 0x115B8..=0x115C0
            | 0x11630..=0x11640
            | 0x116AB..=0x116B7
            | 0x1171D..=0x1172B
            | 0x1182C..=0x1183A
            | 0x11930..=0x11935
            | 0x11937..=0x11938
            | 0x1193B..=0x1193E
            | 0x11940
            | 0x11942..=0x11943
            | 0x119D1..=0x119D7
            | 0x119DA..=0x119E0
            | 0x119E4
            | 0x11A01..=0x11A0A
            | 0x11A33..=0x11A39
            | 0x11A3B..=0x11A3E
            | 0x11A47
            | 0x11A51..=0x11A5B
            | 0x11A8A..=0x11A99
            | 0x11C2F..=0x11C36
            | 0x11C38..=0x11C3F
            | 0x11C92..=0x11CA7
            | 0x11CA9..=0x11CB6
            | 0x11D31..=0x11D36
            | 0x11D3A
            | 0x11D3C..=0x11D3D
            | 0x11D3F..=0x11D45
            | 0x11D47
            | 0x11D8A..=0x11D8E
            | 0x11D90..=0x11D91
            | 0x11D93..=0x11D97
            | 0x11EF3..=0x11EF6
            | 0x16AF0..=0x16AF4
            | 0x16B30..=0x16B36
            | 0x16F4F
            | 0x16F51..=0x16F87
            | 0x16F8F..=0x16F92
            | 0x16FE4
            | 0x1BC9D..=0x1BC9E
            | 0x1D165..=0x1D169
            | 0x1D16D..=0x1D172
            | 0x1D17B..=0x1D182
            | 0x1D185..=0x1D18B
            | 0x1D1AA..=0x1D1AD
            | 0x1D242..=0x1D244
            | 0x1DA00..=0x1DA36
            | 0x1DA3B..=0x1DA6C
            | 0x1DA75
            | 0x1DA84
            | 0x1DA9B..=0x1DA9F
            | 0x1DAA1..=0x1DAAF
            | 0x1E000..=0x1E02A
            | 0x1E130..=0x1E136
            | 0x1E2AE
            | 0x1E2EC..=0x1E2EF
            | 0x1E8D0..=0x1E8D6
            | 0x1E944..=0x1E94A
            | 0xE0100..=0xE01EF
    )
}

fn is_emoji_modifier(value: char) -> bool {
    matches!(u32::from(value), 0x1F3FB..=0x1F3FF)
}

fn is_regional_indicator(value: char) -> bool {
    matches!(u32::from(value), 0x1F1E6..=0x1F1FF)
}

fn is_extend(value: char) -> bool {
    is_combining(value) || is_emoji_modifier(value)
}

fn next_grapheme(buffer: &GapBuffer, start: usize) -> usize {
    let len = buffer.len();
    if start >= len {
        return len;
    }
    let Some(first) = buffer.char_at(start) else {
        return len;
    };
    let mut index = start.checked_add(1).unwrap_or(len);
    if is_regional_indicator(first) && buffer.char_at(index).is_some_and(is_regional_indicator) {
        index = index.checked_add(1).unwrap_or(len).min(len);
    }
    while let Some(value) = buffer.char_at(index) {
        if is_extend(value) {
            index = index.checked_add(1).unwrap_or(len).min(len);
            continue;
        }
        if value == '\u{200D}' {
            let zwj_end = index.checked_add(1).unwrap_or(len).min(len);
            if zwj_end >= len {
                index = zwj_end;
                break;
            }
            index = zwj_end.checked_add(1).unwrap_or(len).min(len);
            continue;
        }
        break;
    }
    index.min(len)
}

fn previous_grapheme(buffer: &GapBuffer, position: usize) -> usize {
    if position == 0 {
        return 0;
    }
    let target = position.min(buffer.len());
    let mut previous = 0_usize;
    let mut current = 0_usize;
    while current < target {
        previous = current;
        let next = next_grapheme(buffer, current);
        if next >= target || next <= current {
            break;
        }
        current = next;
    }
    previous
}

fn grapheme_count_buffer(buffer: &GapBuffer) -> usize {
    grapheme_count_range(buffer, 0, buffer.len())
}

fn grapheme_count_range(buffer: &GapBuffer, start: usize, end: usize) -> usize {
    let mut count = 0_usize;
    let mut position = start.min(end).min(buffer.len());
    let end = end.min(buffer.len());
    while position < end {
        let next = next_grapheme(buffer, position).min(end);
        if next <= position {
            break;
        }
        count = count.saturating_add(1);
        position = next;
    }
    count
}

fn grapheme_count_str(text: &str) -> usize {
    let Ok(buffer) = GapBuffer::from_text(text) else {
        return usize::MAX;
    };
    grapheme_count_buffer(&buffer)
}

fn scalar_for_grapheme(buffer: &GapBuffer, grapheme: usize) -> usize {
    let mut position = 0_usize;
    let mut count = 0_usize;
    while position < buffer.len() && count < grapheme {
        let next = next_grapheme(buffer, position);
        if next <= position {
            break;
        }
        position = next;
        count = count.saturating_add(1);
    }
    position
}

fn classify_word(value: char) -> u8 {
    if value.is_whitespace() {
        0
    } else if value.is_alphanumeric() || value == '_' || is_cjk(value) {
        1
    } else {
        2
    }
}

fn is_cjk(value: char) -> bool {
    matches!(
        u32::from(value),
        0x2E80..=0x2FDF
            | 0x3040..=0x30FF
            | 0x31F0..=0x31FF
            | 0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xAC00..=0xD7AF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
    )
}

fn previous_word_boundary(buffer: &GapBuffer, position: usize) -> usize {
    let mut cursor = position.min(buffer.len());
    if cursor == 0 {
        return 0;
    }
    cursor = previous_grapheme(buffer, cursor);
    while cursor > 0 {
        let Some(value) = buffer.char_at(cursor) else {
            break;
        };
        if classify_word(value) != 0 {
            break;
        }
        cursor = previous_grapheme(buffer, cursor);
    }
    let Some(value) = buffer.char_at(cursor) else {
        return cursor;
    };
    let class = classify_word(value);
    while cursor > 0 {
        let previous = previous_grapheme(buffer, cursor);
        let Some(previous_value) = buffer.char_at(previous) else {
            break;
        };
        if classify_word(previous_value) != class {
            break;
        }
        cursor = previous;
    }
    cursor
}

fn next_word_boundary(buffer: &GapBuffer, position: usize) -> usize {
    let len = buffer.len();
    let mut cursor = position.min(len);
    if cursor >= len {
        return len;
    }
    let Some(value) = buffer.char_at(cursor) else {
        return len;
    };
    let class = classify_word(value);
    while cursor < len {
        let Some(current) = buffer.char_at(cursor) else {
            break;
        };
        if classify_word(current) != class {
            break;
        }
        let next = next_grapheme(buffer, cursor);
        if next <= cursor {
            return len;
        }
        cursor = next;
    }
    while cursor < len {
        let Some(current) = buffer.char_at(cursor) else {
            break;
        };
        if classify_word(current) != 0 {
            break;
        }
        let next = next_grapheme(buffer, cursor);
        if next <= cursor {
            return len;
        }
        cursor = next;
    }
    cursor
}

fn word_range(buffer: &GapBuffer, position: usize) -> (usize, usize) {
    if buffer.len() == 0 {
        return (0, 0);
    }
    let mut start = position.min(buffer.len());
    if start == buffer.len() {
        start = previous_grapheme(buffer, start);
    }
    let Some(value) = buffer.char_at(start) else {
        return (start, start);
    };
    let class = classify_word(value);
    let mut left = start;
    while left > 0 {
        let previous = previous_grapheme(buffer, left);
        let Some(previous_value) = buffer.char_at(previous) else {
            break;
        };
        if classify_word(previous_value) != class {
            break;
        }
        left = previous;
    }
    let mut right = next_grapheme(buffer, start);
    while right < buffer.len() {
        let Some(next_value) = buffer.char_at(right) else {
            break;
        };
        if classify_word(next_value) != class {
            break;
        }
        let next = next_grapheme(buffer, right);
        if next <= right {
            break;
        }
        right = next;
    }
    (left, right)
}

fn line_start(buffer: &GapBuffer, position: usize) -> usize {
    let mut cursor = position.min(buffer.len());
    while cursor > 0 {
        let previous = cursor.saturating_sub(1);
        if matches!(buffer.char_at(previous), Some('\n')) {
            break;
        }
        cursor = previous;
    }
    cursor
}

fn line_end(buffer: &GapBuffer, position: usize) -> usize {
    let mut cursor = position.min(buffer.len());
    while cursor < buffer.len() {
        if matches!(buffer.char_at(cursor), Some('\n')) {
            break;
        }
        cursor = cursor.checked_add(1).unwrap_or(buffer.len());
    }
    cursor.min(buffer.len())
}

fn advance_graphemes(buffer: &GapBuffer, start: usize, end: usize, count: usize) -> usize {
    let mut cursor = start.min(end).min(buffer.len());
    let end = end.min(buffer.len());
    let mut moved = 0_usize;
    while cursor < end && moved < count {
        let next = next_grapheme(buffer, cursor).min(end);
        if next <= cursor {
            break;
        }
        cursor = next;
        moved = moved.saturating_add(1);
    }
    cursor
}

#[cfg(test)]
mod tests {
    use super::{
        float_sub, Clipboard, EditConfig, EditModel, FieldMode, InputFilter, Key, Modifiers, MouseSelect, Selection,
    };
    use sse_core::Result;

    #[derive(Default)]
    struct MemoryClipboard {
        text: String,
    }

    impl Clipboard for MemoryClipboard {
        fn read_text(&mut self) -> Result<String> {
            Ok(self.text.clone())
        }

        fn write_text(&mut self, text: &str) -> Result<()> {
            self.text.clear();
            self.text.push_str(text);
            Ok(())
        }
    }

    fn model(initial: &str, mode: FieldMode) -> EditModel {
        let config = EditConfig {
            mode,
            ..EditConfig::default()
        };
        match EditModel::new(initial, config) {
            Ok(value) => value,
            Err(error) => panic!("failed to create editor: {error}"),
        }
    }

    fn key(model: &mut EditModel, key: Key, ctrl: bool, shift: bool, clipboard: &mut MemoryClipboard) {
        let result = model.key(key, Modifiers { ctrl, shift }, clipboard);
        if let Err(error) = result {
            panic!("key failed: {error}");
        }
    }

    #[test]
    fn typing_is_coalesced_into_one_undo_record() {
        let mut editor = model("", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard::default();
        for value in "hello".chars() {
            key(&mut editor, Key::Character(value), false, false, &mut clipboard);
        }
        assert_eq!(editor.text(), "hello");
        assert_eq!(editor.undo(), Ok(true));
        assert_eq!(editor.text(), "");
        assert_eq!(editor.redo(), Ok(true));
        assert_eq!(editor.text(), "hello");
    }

    #[test]
    fn combining_mark_moves_as_one_grapheme() {
        let mut editor = model("a\u{0301}b", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard::default();
        key(&mut editor, Key::Left, false, false, &mut clipboard);
        assert_eq!(editor.selection(), Selection::collapsed(2));
        key(&mut editor, Key::Left, false, false, &mut clipboard);
        assert_eq!(editor.selection(), Selection::collapsed(0));
        key(&mut editor, Key::Delete, false, false, &mut clipboard);
        assert_eq!(editor.text(), "b");
    }

    #[test]
    fn emoji_zwj_sequence_is_one_grapheme() {
        let mut editor = model("👩\u{200D}💻!", FieldMode::SingleLine);
        editor.set_caret_grapheme(1, false);
        assert_eq!(editor.selection().caret, 3);
        let mut clipboard = MemoryClipboard::default();
        key(&mut editor, Key::Backspace, false, false, &mut clipboard);
        assert_eq!(editor.text(), "!");
    }

    #[test]
    fn cyrillic_ctrl_word_navigation_and_delete() {
        let mut editor = model("Привет мир", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard::default();
        key(&mut editor, Key::Left, true, false, &mut clipboard);
        assert_eq!(editor.selection().caret, 7);
        key(&mut editor, Key::Backspace, true, false, &mut clipboard);
        assert_eq!(editor.text(), "мир");
    }

    #[test]
    fn cjk_double_click_selects_word_run() {
        let mut editor = model("测试 数据", FieldMode::SingleLine);
        editor.mouse_select(1, MouseSelect::Word, false);
        assert_eq!(editor.selection(), Selection { anchor: 0, caret: 2 });
    }

    #[test]
    fn triple_click_selects_line_and_newline() {
        let mut editor = model("one\ntwo\nthree", FieldMode::MultiLine);
        editor.mouse_select(5, MouseSelect::Line, false);
        assert_eq!(editor.selection(), Selection { anchor: 4, caret: 8 });
    }

    #[test]
    fn up_down_keep_logical_grapheme_column() {
        let mut editor = model("abcd\nxy\n12345", FieldMode::MultiLine);
        let mut clipboard = MemoryClipboard::default();
        editor.set_caret_grapheme(3, false);
        key(&mut editor, Key::Down, false, false, &mut clipboard);
        assert_eq!(editor.selection().caret, 7);
        key(&mut editor, Key::Down, false, false, &mut clipboard);
        assert_eq!(editor.selection().caret, 11);
        key(&mut editor, Key::Up, false, false, &mut clipboard);
        assert_eq!(editor.selection().caret, 7);
    }

    #[test]
    fn single_line_paste_normalises_newlines() {
        let mut editor = model("a", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard {
            text: "b\nc\r\nd".to_owned(),
        };
        assert_eq!(editor.paste(&mut clipboard), Ok(true));
        assert_eq!(editor.text(), "ab c d");
    }

    #[test]
    fn paste_longer_than_the_limit_keeps_the_part_that_fits() {
        let config = EditConfig {
            max_graphemes: 5,
            ..EditConfig::default()
        };
        let mut editor = match EditModel::new("ab", config) {
            Ok(value) => value,
            Err(error) => panic!("failed to create editor: {error}"),
        };
        let mut clipboard = MemoryClipboard {
            text: "cdefgh".to_owned(),
        };
        assert_eq!(editor.paste(&mut clipboard), Ok(true));
        assert_eq!(editor.text(), "abcde");
        assert_eq!(
            editor.paste(&mut clipboard),
            Ok(false),
            "a full field refuses the whole paste"
        );
        assert_eq!(editor.text(), "abcde");
    }

    #[test]
    fn refused_paste_says_why_it_was_refused() {
        let config = EditConfig {
            max_graphemes: 3,
            ..EditConfig::default()
        };
        let mut full = match EditModel::new("abc", config) {
            Ok(value) => value,
            Err(error) => panic!("failed to create editor: {error}"),
        };
        let mut clipboard = MemoryClipboard { text: "x".to_owned() };
        full.set_caret_grapheme(3, false);
        assert_eq!(full.paste(&mut clipboard), Ok(false));
        assert_eq!(full.take_paste_refusal(), Some(super::PasteRefusal::TooLong));
        assert_eq!(full.take_paste_refusal(), None, "the reason is reported once");

        let digits = EditConfig {
            filter: InputFilter::Digits {
                min: Some(0),
                max: None,
                allow_empty: true,
            },
            ..EditConfig::default()
        };
        let mut numeric = match EditModel::new("", digits) {
            Ok(value) => value,
            Err(error) => panic!("failed to create editor: {error}"),
        };
        clipboard.text = "abc".to_owned();
        assert_eq!(numeric.paste(&mut clipboard), Ok(false));
        assert_eq!(numeric.take_paste_refusal(), Some(super::PasteRefusal::NotAllowed));
    }

    #[test]
    fn cut_copy_paste_round_trip() {
        let mut editor = model("alpha beta", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard::default();
        editor.mouse_select(2, MouseSelect::Word, false);
        assert_eq!(editor.copy(&mut clipboard), Ok(true));
        assert_eq!(clipboard.text, "alpha");
        assert_eq!(editor.cut(&mut clipboard), Ok(true));
        assert_eq!(editor.text(), " beta");
        editor.set_caret_grapheme(editor.grapheme_count(), false);
        assert_eq!(editor.paste(&mut clipboard), Ok(true));
        assert_eq!(editor.text(), " betaalpha");
    }

    #[test]
    fn digits_filter_enforces_range_and_allows_intermediate_empty() {
        let config = EditConfig {
            mode: FieldMode::SingleLine,
            max_graphemes: 4,
            history_limit: 16,
            filter: InputFilter::Digits {
                min: Some(0),
                max: Some(100),
                allow_empty: true,
            },
        };
        let mut editor = match EditModel::new("10", config) {
            Ok(value) => value,
            Err(error) => panic!("editor: {error}"),
        };
        assert_eq!(editor.insert_text("1"), Ok(false));
        assert_eq!(editor.text(), "10");
        editor.mouse_select(0, MouseSelect::Line, false);
        assert_eq!(editor.insert_text("99"), Ok(true));
        assert_eq!(editor.text(), "99");
    }

    #[test]
    fn money_filter_uses_exact_minor_units() {
        let config = EditConfig {
            mode: FieldMode::SingleLine,
            max_graphemes: 16,
            history_limit: 16,
            filter: InputFilter::Money {
                min_minor: Some(-10_000),
                max_minor: Some(10_000),
                decimals: 2,
                allow_negative: true,
                allow_empty: true,
            },
        };
        let mut editor = match EditModel::new("", config) {
            Ok(value) => value,
            Err(error) => panic!("editor: {error}"),
        };
        assert_eq!(editor.insert_text("-12.34"), Ok(true));
        assert_eq!(editor.text(), "-12.34");
        assert_eq!(editor.insert_text("5"), Ok(false));
    }

    #[test]
    fn maximum_length_counts_graphemes_not_scalars() {
        let config = EditConfig {
            mode: FieldMode::SingleLine,
            max_graphemes: 2,
            history_limit: 8,
            filter: InputFilter::Any,
        };
        let mut editor = match EditModel::new("a\u{0301}", config) {
            Ok(value) => value,
            Err(error) => panic!("editor: {error}"),
        };
        assert_eq!(editor.insert_text("界"), Ok(true));
        assert_eq!(editor.insert_text("x"), Ok(false));
        assert_eq!(editor.text(), "a\u{0301}界");
    }

    #[test]
    fn ime_overlay_does_not_touch_buffer_until_commit() {
        let mut editor = model("ab", FieldMode::SingleLine);
        editor.set_caret_grapheme(1, false);
        editor.set_composition("Ж");
        assert_eq!(editor.text(), "ab");
        assert_eq!(editor.commit_composition(), Ok(true));
        assert_eq!(editor.text(), "aЖb");
        assert_eq!(editor.undo(), Ok(true));
        assert_eq!(editor.text(), "ab");
    }

    #[test]
    fn horizontal_scroll_keeps_measured_caret_inside_view() {
        let mut editor = model("abcdef", FieldMode::SingleLine);
        editor.ensure_caret_visible(120.0, 100.0, 8.0);
        assert!(float_sub(editor.horizontal_scroll(), 28.0).abs() < 0.001);
        editor.ensure_caret_visible(10.0, 100.0, 8.0);
        assert!(float_sub(editor.horizontal_scroll(), 2.0).abs() < 0.001);
    }

    #[test]
    fn shift_navigation_preserves_anchor() {
        let mut editor = model("abc", FieldMode::SingleLine);
        let mut clipboard = MemoryClipboard::default();
        key(&mut editor, Key::Left, false, true, &mut clipboard);
        key(&mut editor, Key::Left, false, true, &mut clipboard);
        assert_eq!(editor.selection(), Selection { anchor: 3, caret: 1 });
    }
}
