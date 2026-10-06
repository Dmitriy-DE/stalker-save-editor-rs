//! Retained widget tree with damage tracking: only what changed is laid out again and repainted.
//!
//! Widgets live in an arena in creation order; a parent is always created before its children, so arena order is
//! paint order. Every visible change (text, look, hover, geometry) adds the old and new rectangles to the damage
//! list; [`Tree::paint`] redraws only those rectangles and returns them for the backend to present.

use crate::event_loop::ImeEvent;
use crate::glyphs::{to_px, to_u32, Fonts, TextStyle};
use crate::layout::{self, Constraints, Layout, NodeId, NodeKind, Size, Style};
use crate::path::Icon;
use crate::raster::{Color, ImageFilter, ImageRef, MaskRef, Radii, Rect, Surface};
use crate::widgets::icon::IconCache;
use sse_core::{Error, Result};
use std::sync::Arc;

/// Damage rectangles kept separately before they are merged into one bounding box.
const MAX_DAMAGE_RECTS: usize = 16;
const DIALOG_DIM_ALPHA: u8 = 144;

/// Handle of a widget in a [`Tree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WidgetId(usize);

impl WidgetId {
    /// Arena index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0
    }
}

/// Shared premultiplied BGRA image data shown by an image widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Row-major premultiplied BGRA pixels.
    pub pixels: Arc<[u32]>,
}

impl ImageData {
    /// Creates image data after validating that the pixel plane matches its dimensions.
    ///
    /// # Errors
    /// Returns damage for empty dimensions or a mismatched pixel count.
    pub fn new(width: u32, height: u32, pixels: Vec<u32>) -> Result<Self> {
        let expected = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| Error::damaged("image dimensions overflow"))?;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err(Error::damaged("image dimensions do not match its pixel plane"));
        }
        Ok(Self {
            width,
            height,
            pixels: pixels.into(),
        })
    }
}

/// Horizontal placement of a widget's text inside its rectangle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlign {
    /// Left edge plus padding.
    #[default]
    Start,
    /// Centred.
    Center,
    /// Right edge minus padding.
    End,
}

/// How a widget looks. Colours come from the theme; the tree knows nothing about the palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Background fill.
    pub fill: Option<Color>,
    /// Background while the pointer is over an interactive widget.
    pub hover_fill: Option<Color>,
    /// Background while an interactive widget is held down.
    pub pressed_fill: Option<Color>,
    /// Border colour and width.
    pub border: Option<(Color, f32)>,
    /// Left accent bar (selected navigation item) colour and width.
    pub accent_bar: Option<(Color, f32)>,
    /// Corner radius.
    pub radius: f32,
    /// Text colour.
    pub text: Color,
    /// Text colour while hovered.
    pub hover_text: Option<Color>,
    /// Text alignment.
    pub align: TextAlign,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            fill: None,
            hover_fill: None,
            pressed_fill: None,
            border: None,
            accent_bar: None,
            radius: 0.0,
            text: Color::rgba(255, 255, 255, 255),
            hover_text: None,
            align: TextAlign::Start,
        }
    }
}

/// What a widget shows and whether it reacts to the pointer.
#[derive(Clone, Debug, PartialEq)]
pub enum Content {
    /// A box: background, border, children.
    Panel,
    /// A scaled image, with pixels shared across retained-tree redraws.
    Image(Option<ImageData>),
    /// One line of text.
    Label {
        /// Text.
        text: String,
        /// Font face and size.
        style: TextStyle,
    },
    /// One-line editable field rendered with the widget's fill, border and focus ring.
    Input {
        /// Current edit text.
        text: String,
        /// Font face and size.
        style: TextStyle,
    },
    /// Multi-line text wrapped to the arranged width.
    Paragraph {
        /// Text.
        text: String,
        /// Font face and size.
        style: TextStyle,
    },
    /// A clickable box with one line of text.
    Button {
        /// Text.
        text: String,
        /// Font face and size.
        style: TextStyle,
    },
    /// A clickable navigation row with a built-in vector icon.
    IconButton {
        /// Vector icon.
        icon: Icon,
        /// Optional text; empty in collapsed navigation.
        text: String,
        /// Font face and size.
        style: TextStyle,
    },
}

impl Content {
    fn text(&self) -> Option<(&str, TextStyle)> {
        match self {
            Self::Panel | Self::Image(_) => None,
            Self::Label { text, style }
            | Self::Input { text, style }
            | Self::Paragraph { text, style }
            | Self::Button { text, style }
            | Self::IconButton { text, style, .. } => Some((text.as_str(), *style)),
        }
    }

    const fn interactive(&self) -> bool {
        matches!(self, Self::Button { .. } | Self::IconButton { .. } | Self::Input { .. })
    }
}

struct Node {
    content: Content,
    look: Look,
    style: Style,
    layout: NodeId,
    parent: Option<WidgetId>,
    rect: Rect,
    visible: bool,
    enabled: bool,
    scroll_y: i32,
    clip_children: bool,
    tooltip: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InputComposition {
    widget: WidgetId,
    start: usize,
    end: usize,
    text: String,
}

/// Retained widget tree, its layout and its pending damage.
pub struct Tree {
    nodes: Vec<Node>,
    layout: Layout,
    fonts: Fonts,
    root: Option<WidgetId>,
    background: Color,
    size: (u32, u32),
    scale: f32,
    needs_layout: bool,
    damage: Vec<Rect>,
    hover: Option<WidgetId>,
    pressed: Option<WidgetId>,
    focused: Option<WidgetId>,
    input_composition: Option<InputComposition>,
    previous_dialog_focus: Option<WidgetId>,
    modal_dialog: Option<WidgetId>,
    overlay_host: Option<WidgetId>,
    icon_cache: IconCache,
    changed_inputs: Vec<WidgetId>,
    tooltip_ticks: u8,
    tooltip_visible: bool,
}

impl Tree {
    /// Creates an empty tree that paints `background` behind everything.
    #[must_use]
    pub fn new(fonts: Fonts, background: Color) -> Self {
        Self {
            nodes: Vec::new(),
            layout: Layout::new(),
            fonts,
            root: None,
            background,
            size: (0, 0),
            scale: 1.0,
            needs_layout: true,
            damage: Vec::new(),
            hover: None,
            pressed: None,
            focused: None,
            input_composition: None,
            previous_dialog_focus: None,
            modal_dialog: None,
            overlay_host: None,
            icon_cache: IconCache::new(),
            changed_inputs: Vec::new(),
            tooltip_ticks: 0,
            tooltip_visible: false,
        }
    }

    /// Fonts used for measuring and drawing.
    pub fn fonts(&mut self) -> &mut Fonts {
        &mut self.fonts
    }

    /// Registers the transparent host where screens can build non-blocking dialog widgets.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn set_overlay_host(&mut self, id: WidgetId) -> Result<()> {
        self.node(id)?;
        self.overlay_host = Some(id);
        Ok(())
    }

    /// Host where a screen can place a dialog overlay.
    #[must_use]
    pub const fn overlay_host(&self) -> Option<WidgetId> {
        self.overlay_host
    }

    /// Currently focused visible interactive widget.
    #[must_use]
    pub const fn focused(&self) -> Option<WidgetId> {
        self.focused
    }

    /// Whether keyboard focus currently belongs to a text input.
    #[must_use]
    pub fn focused_is_input(&self) -> bool {
        self.focused
            .and_then(|id| self.nodes.get(id.0))
            .is_some_and(|node| matches!(&node.content, Content::Input { .. }))
    }

    /// Moves focus to a visible button or input, or clears keyboard focus.
    ///
    /// # Errors
    /// Returns an error if `id` does not identify an eligible interactive widget.
    pub fn set_focus(&mut self, id: Option<WidgetId>) -> Result<()> {
        if let Some(id) = id {
            if !self.is_focusable(id) {
                return Err(Error::Refused("widget cannot receive keyboard focus".to_owned()));
            }
        }
        self.change_focus(id);
        Ok(())
    }

    /// Focuses the next visible button, wrapping at either end. When a dialog is open, only its buttons participate.
    pub fn focus_next(&mut self, reverse: bool) -> Option<WidgetId> {
        let count = self.nodes.len();
        if count == 0 {
            return None;
        }
        let mut index = self
            .focused
            .map_or(if reverse { 0 } else { count.saturating_sub(1) }, |id| id.0);
        for _step in 0..count {
            index = if reverse {
                index.checked_sub(1).unwrap_or_else(|| count.saturating_sub(1))
            } else {
                index.checked_add(1).filter(|next| *next < count).unwrap_or(0)
            };
            let id = WidgetId(index);
            if self.is_focusable(id) {
                let _ = self.set_focus(Some(id));
                return Some(id);
            }
        }
        None
    }

    /// First visible button inside a widget subtree.
    #[must_use]
    pub fn first_focusable_in(&self, root: WidgetId) -> Option<WidgetId> {
        (0..self.nodes.len())
            .map(WidgetId)
            .find(|id| self.is_focusable(*id) && self.within_subtree(*id, root))
    }

    /// Shows a widget subtree above the rest of the frame, dims the background, and confines pointer/focus input to it.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn open_dialog(&mut self, id: WidgetId) -> Result<()> {
        self.node(id)?;
        if self.modal_dialog.is_some() {
            let _ = self.close_dialog()?;
        }
        self.previous_dialog_focus = self.focused;
        self.modal_dialog = Some(id);
        self.set_visible(id, true)?;
        self.focused = None;
        if let Some(focus) = self.first_focusable_in(id) {
            self.set_focus(Some(focus))?;
        }
        self.damage_all();
        Ok(())
    }

    /// Whether a widget dialog currently owns pointer and keyboard input.
    #[must_use]
    pub const fn dialog_open(&self) -> bool {
        self.modal_dialog.is_some()
    }

    /// Active dialog root, when a widget overlay owns input.
    #[must_use]
    pub const fn dialog(&self) -> Option<WidgetId> {
        self.modal_dialog
    }

    /// Whether a widget and each of its ancestors are visible.
    #[must_use]
    pub fn is_visible(&self, id: WidgetId) -> bool {
        self.shown(id)
    }

    /// Returns the displayed text of a label, input, paragraph, or button.
    ///
    /// # Errors
    /// Returns an error for an unknown widget or a widget without text content.
    pub fn text(&self, id: WidgetId) -> Result<&str> {
        self.node(id)?
            .content
            .text()
            .map(|(text, _)| text)
            .ok_or_else(|| Error::Refused("widget has no text".to_owned()))
    }

    /// Hides the active dialog and restores focus to the widget that opened it.
    ///
    /// # Errors
    /// Returns an error from the widget tree when the dialog is hidden.
    pub fn close_dialog(&mut self) -> Result<bool> {
        let Some(dialog) = self.modal_dialog.take() else {
            return Ok(false);
        };
        let previous_focus = self.previous_dialog_focus.take();
        self.set_visible(dialog, false)?;
        let restore = previous_focus.filter(|id| self.is_focusable(*id));
        self.set_focus(restore)?;
        self.damage_all();
        Ok(true)
    }

    /// Adds a widget. `kind` is the layout container kind (`Row`, `Column`, `Stack`, `Leaf`...). The first widget
    /// added without a parent becomes the root.
    ///
    /// # Errors
    /// Returns an error for an unknown parent or a second root.
    pub fn add(
        &mut self,
        parent: Option<WidgetId>,
        kind: NodeKind,
        style: Style,
        content: Content,
        look: Look,
    ) -> Result<WidgetId> {
        let measured = self.text_style(&content, style);
        let layout = self.layout.add(kind, measured)?;
        match parent {
            Some(owner) => {
                let parent_layout = self.node(owner)?.layout;
                self.layout.append_child(parent_layout, layout)?;
            }
            None if self.root.is_some() => return Err(Error::Refused("widget tree already has a root".to_owned())),
            None => {}
        }
        let id = WidgetId(self.nodes.len());
        self.nodes.push(Node {
            content,
            look,
            style,
            layout,
            parent,
            rect: Rect::new(0, 0, 0, 0),
            visible: true,
            enabled: true,
            scroll_y: 0,
            clip_children: false,
            tooltip: None,
        });
        if parent.is_none() {
            self.root = Some(id);
        }
        self.needs_layout = true;
        Ok(id)
    }

    /// Replaces the text of a label or button.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn set_text(&mut self, id: WidgetId, text: &str) -> Result<()> {
        let node = self.node_mut(id)?;
        match &mut node.content {
            Content::Label { text: old, .. }
            | Content::Input { text: old, .. }
            | Content::Paragraph { text: old, .. }
            | Content::Button { text: old, .. }
                if old != text =>
            {
                text.clone_into(old);
            }
            _ => return Ok(()),
        }
        self.restyle(id)
    }

    /// Sets image data on an image widget and damages its current bounds.
    ///
    /// # Errors
    /// Returns damage if `id` is not an image widget.
    pub fn set_image(&mut self, id: WidgetId, image: Option<ImageData>) -> Result<()> {
        let node = self.node_mut(id)?;
        let Content::Image(current) = &mut node.content else {
            return Err(Error::damaged("widget is not an image"));
        };
        let unchanged = match (&*current, &image) {
            (Some(current), Some(next)) => {
                current.width == next.width
                    && current.height == next.height
                    && Arc::ptr_eq(&current.pixels, &next.pixels)
            }
            (None, None) => true,
            _ => false,
        };
        if unchanged {
            return Ok(());
        }
        *current = image;
        let rect = node.rect;
        self.add_damage(rect);
        Ok(())
    }

    /// Returns the image stored by an image widget.
    ///
    /// # Errors
    /// Returns damage for an unknown widget or a non-image widget.
    pub fn image(&self, id: WidgetId) -> Result<Option<&ImageData>> {
        match &self.node(id)?.content {
            Content::Image(image) => Ok(image.as_ref()),
            _ => Err(Error::damaged("widget is not an image")),
        }
    }

    /// Returns the current value of an input widget.
    ///
    /// # Errors
    /// Returns damage for an unknown widget or a widget that is not an input.
    pub fn input_text(&self, id: WidgetId) -> Result<&str> {
        match &self.node(id)?.content {
            Content::Input { text, .. } => Ok(text.as_str()),
            _ => Err(Error::damaged("widget is not an input")),
        }
    }

    /// Sets hover help for a widget. Disabled interactive widgets remain hover targets for tooltips.
    pub fn set_tooltip(&mut self, id: WidgetId, text: impl Into<String>) -> Result<()> {
        self.node_mut(id)?.tooltip = Some(text.into());
        Ok(())
    }

    /// Advances the tooltip delay by one 500 ms UI timer tick.
    pub fn tick_tooltip(&mut self) -> bool {
        let has = self
            .hover
            .and_then(|id| self.nodes.get(id.0))
            .and_then(|node| node.tooltip.as_ref())
            .is_some();
        if !has {
            self.tooltip_ticks = 0;
            self.tooltip_visible = false;
            return false;
        }
        self.tooltip_ticks = self.tooltip_ticks.saturating_add(1);
        let reveal = self.tooltip_ticks >= 1;
        let changed = reveal != self.tooltip_visible;
        self.tooltip_visible = reveal;
        changed
    }

    /// Tooltip text after the 500 ms hover delay has elapsed.
    #[must_use]
    pub fn active_tooltip(&self) -> Option<&str> {
        self.tooltip_visible
            .then(|| {
                self.hover
                    .and_then(|id| self.nodes.get(id.0))
                    .and_then(|node| node.tooltip.as_deref())
            })
            .flatten()
    }

    /// Visible button labels that exceed the current arranged width and would need ellipsis.
    ///
    /// # Errors
    /// Returns an error when layout cannot be updated.
    pub fn ellipsized_button_labels(&mut self) -> Result<Vec<String>> {
        self.update_layout()?;
        let mut labels = Vec::new();
        for index in 0..self.nodes.len() {
            let id = WidgetId(index);
            if !self.shown(id) {
                continue;
            }
            let Some(node) = self.nodes.get(index) else { continue };
            let (text, style, icon_inset) = match &node.content {
                Content::Button { text, style } => (text.as_str(), *style, 0.0),
                Content::IconButton { text, style, .. } if !text.is_empty() => (text.as_str(), *style, 24.0),
                _ => continue,
            };
            let available =
                u32_to_f32(node.rect.width) - node.style.padding.left - node.style.padding.right - icon_inset;
            if self.fonts.measure(text, style) > available.max(0.0) + 0.5 {
                labels.push(text.to_owned());
            }
        }
        Ok(labels)
    }

    /// Replaces an input widget's value and records it as changed.
    ///
    /// # Errors
    /// Returns damage for an unknown widget or a widget that is not an input.
    pub fn set_input_text(&mut self, id: WidgetId, value: &str) -> Result<()> {
        let changed = match &mut self.node_mut(id)?.content {
            Content::Input { text, .. } if text != value => {
                value.clone_into(text);
                true
            }
            Content::Input { .. } => false,
            _ => return Err(Error::damaged("widget is not an input")),
        };
        if self
            .input_composition
            .as_ref()
            .is_some_and(|composition| composition.widget == id)
        {
            self.input_composition = None;
        }
        if changed {
            self.mark_input_changed(id);
            self.restyle(id)?;
        }
        Ok(())
    }

    /// Applies an input-method event to the focused one-line input.
    ///
    /// Preedit text is displayed over the input without changing its stored value. A commit replaces the range
    /// captured when composition started; cancellation discards the overlay.
    ///
    /// # Errors
    /// Returns an error when the focused widget is not a valid input during a commit.
    pub fn apply_ime_event(&mut self, event: &ImeEvent) -> Result<bool> {
        let Some(widget) = self
            .focused
            .filter(|id| self.focused_is_input() && self.is_visible(*id))
        else {
            self.input_composition = None;
            return Ok(false);
        };
        match event {
            ImeEvent::Start => {
                let end = self.input_text(widget)?.chars().count();
                self.input_composition = Some(InputComposition {
                    widget,
                    start: end,
                    end,
                    text: String::new(),
                });
                if let Ok(rect) = self.rect(widget) {
                    self.add_damage(rect);
                }
                Ok(true)
            }
            ImeEvent::Update(text) => {
                if self
                    .input_composition
                    .as_ref()
                    .is_none_or(|composition| composition.widget != widget)
                {
                    self.apply_ime_event(&ImeEvent::Start)?;
                }
                if let Some(composition) = self.input_composition.as_mut() {
                    composition.text.clone_from(text);
                }
                if let Ok(rect) = self.rect(widget) {
                    self.add_damage(rect);
                }
                Ok(true)
            }
            ImeEvent::Commit(text) => {
                let composition = if let Some(composition) = self.input_composition.take() {
                    composition
                } else {
                    let end = self.input_text(widget)?.chars().count();
                    InputComposition {
                        widget,
                        start: end,
                        end,
                        text: String::new(),
                    }
                };
                let original = self.input_text(widget)?.to_owned();
                let committed = overlay_composition(&original, composition.start, composition.end, text);
                self.set_input_text(widget, &committed)?;
                Ok(true)
            }
            ImeEvent::Cancel => {
                let Some(composition) = self.input_composition.take() else {
                    return Ok(false);
                };
                if let Ok(rect) = self.rect(composition.widget) {
                    self.add_damage(rect);
                }
                Ok(true)
            }
        }
    }

    /// Returns the text to render for an input, including active preedit text.
    ///
    /// # Errors
    /// Returns an error for an unknown widget or a widget that is not an input.
    pub fn input_display_text(&self, id: WidgetId) -> Result<String> {
        let text = self.input_text(id)?;
        let Some(composition) = self.input_composition.as_ref().filter(|value| value.widget == id) else {
            return Ok(text.to_owned());
        };
        Ok(overlay_composition(
            text,
            composition.start,
            composition.end,
            &composition.text,
        ))
    }

    /// Applies one portable keyboard edit to the focused input.
    ///
    /// Printable text is inserted at the end of the one-line value. Backspace
    /// removes one Unicode scalar. Clipboard backends can pass pasted text via
    /// `text`; control characters and line breaks are ignored.
    pub fn edit_focused_input(&mut self, keysym: u32, text: Option<&str>) -> Result<bool> {
        let Some(id) = self.focused else {
            return Ok(false);
        };
        if !matches!(self.node(id)?.content, Content::Input { .. }) {
            return Ok(false);
        }
        let mut value = self.input_text(id)?.to_owned();
        let changed = if keysym == 0xff08 {
            value.pop().is_some()
        } else if let Some(inserted) = text {
            let filtered: String = inserted
                .chars()
                .filter(|ch| !ch.is_control() && *ch != '\n' && *ch != '\r')
                .collect();
            if filtered.is_empty() {
                false
            } else {
                value.push_str(&filtered);
                true
            }
        } else {
            false
        };
        if changed {
            self.set_input_text(id, &value)?;
        }
        Ok(changed)
    }

    /// Drains input widgets whose values changed since the previous call.
    #[must_use]
    pub fn take_changed_inputs(&mut self) -> Vec<WidgetId> {
        std::mem::take(&mut self.changed_inputs)
    }

    fn mark_input_changed(&mut self, id: WidgetId) {
        if !self.changed_inputs.contains(&id) {
            self.changed_inputs.push(id);
        }
    }

    /// Replaces how a widget looks.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn set_look(&mut self, id: WidgetId, look: Look) -> Result<()> {
        let node = self.node_mut(id)?;
        if node.look != look {
            node.look = look;
            let rect = node.rect;
            self.add_damage(rect);
        }
        Ok(())
    }

    /// Replaces one resolved colour across the retained tree.
    ///
    /// Theme changes use this to update widgets that were already built without rebuilding screen state.
    pub fn replace_color(&mut self, from: Color, to: Color) {
        if from == to {
            return;
        }
        if self.background == from {
            self.background = to;
        }
        for node in &mut self.nodes {
            let look = &mut node.look;
            for color in [
                &mut look.fill,
                &mut look.hover_fill,
                &mut look.pressed_fill,
                &mut look.hover_text,
            ] {
                if *color == Some(from) {
                    *color = Some(to);
                }
            }
            if look.text == from {
                look.text = to;
            }
            if let Some((color, width)) = look.border {
                if color == from {
                    look.border = Some((to, width));
                }
            }
            if let Some((color, width)) = look.accent_bar {
                if color == from {
                    look.accent_bar = Some((to, width));
                }
            }
        }
        self.damage_all();
    }

    /// Shows or hides a widget and its subtree. A hidden widget takes no space.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn set_visible(&mut self, id: WidgetId, visible: bool) -> Result<()> {
        let node = self.node_mut(id)?;
        if node.visible == visible {
            return Ok(());
        }
        node.visible = visible;
        if !visible {
            if self.focused.is_some_and(|focused| self.within_subtree(focused, id)) {
                self.change_focus(None);
            }
            if self.modal_dialog.is_some_and(|dialog| self.within_subtree(dialog, id)) {
                self.modal_dialog = None;
                self.previous_dialog_focus = None;
            }
        }
        self.restyle(id)
    }

    /// Replaces the layout style of a widget and invalidates its measured geometry.
    ///
    /// # Errors
    /// Returns an error for an unknown widget or a failed layout update.
    pub fn set_style(&mut self, id: WidgetId, style: Style) -> Result<()> {
        let (layout, rect, visible, content) = {
            let node = self.node(id)?;
            (node.layout, node.rect, node.visible, node.content.clone())
        };
        self.node_mut(id)?.style = style;
        let layout_style = if visible {
            self.text_style(&content, style)
        } else {
            hidden_style()
        };
        self.layout.set_style(layout, layout_style)?;
        self.add_damage(rect);
        self.needs_layout = true;
        Ok(())
    }

    /// Enables or disables pointer and keyboard interaction for a widget.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn set_enabled(&mut self, id: WidgetId, enabled: bool) -> Result<()> {
        let node = self.node_mut(id)?;
        if node.enabled == enabled {
            return Ok(());
        }
        node.enabled = enabled;
        if !enabled {
            if self.focused == Some(id) {
                self.change_focus(None);
            }
            if self.pressed == Some(id) {
                self.pressed = None;
            }
            if self.hover == Some(id) {
                self.hover = None;
            }
        }
        self.restyle(id)
    }

    /// Makes this widget a clipping viewport for descendants.
    pub fn set_clip_children(&mut self, id: WidgetId, clip_children: bool) -> Result<()> {
        let node = self.node_mut(id)?;
        if node.clip_children != clip_children {
            node.clip_children = clip_children;
            self.needs_layout = true;
            self.damage_all();
        }
        Ok(())
    }

    /// Scrolls descendants upward without changing their measured sizes.
    pub fn set_scroll_y(&mut self, id: WidgetId, offset: i32) -> Result<()> {
        let node = self.node_mut(id)?;
        let offset = offset.max(0);
        if node.scroll_y != offset {
            node.scroll_y = offset;
            self.needs_layout = true;
            self.damage_all();
        }
        Ok(())
    }

    /// Natural descendant height inside a viewport before its current scroll offset.
    pub fn content_height(&mut self, id: WidgetId) -> Result<f32> {
        self.update_layout()?;
        let viewport = self.node(id)?.rect;
        let current = self.node(id)?.scroll_y;
        let bottom = (0..self.nodes.len())
            .map(WidgetId)
            .filter(|child| *child != id && self.within_subtree(*child, id))
            .filter_map(|child| {
                self.nodes.get(child.0).map(|node| {
                    i64::from(node.rect.y)
                        .saturating_add(i64::from(node.rect.height))
                        .saturating_add(i64::from(current))
                })
            })
            .max()
            .unwrap_or(i64::from(viewport.y));
        Ok(u32_to_f32(
            u32::try_from(bottom.saturating_sub(i64::from(viewport.y)).max(0)).unwrap_or(u32::MAX),
        ))
    }

    /// Last arranged rectangle of a widget.
    ///
    /// # Errors
    /// Returns an error for an unknown widget.
    pub fn rect(&self, id: WidgetId) -> Result<Rect> {
        Ok(self.node(id)?.rect)
    }

    /// Sets the window size in pixels; everything is damaged.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.size != (width, height) {
            self.size = (width, height);
            self.needs_layout = true;
            self.damage_all();
        }
    }

    /// Sets the logical-to-framebuffer scale reported by the native window.
    ///
    /// Invalid, non-positive scales are ignored so a malformed platform event cannot poison layout geometry.
    pub fn set_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 && self.scale != scale {
            self.scale = scale;
            self.needs_layout = true;
            self.damage_all();
        }
    }

    /// Current logical-to-framebuffer scale used to snap layout boundaries.
    #[must_use]
    pub const fn scale(&self) -> f32 {
        self.scale
    }

    /// Current window size.
    #[must_use]
    pub const fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Marks the whole window for repaint (expose after being uncovered, theme change).
    pub fn damage_all(&mut self) {
        self.damage.clear();
        self.damage.push(Rect::new(0, 0, self.size.0, self.size.1));
    }

    /// Adds a damaged rectangle (window coordinates).
    pub fn add_damage(&mut self, rect: Rect) {
        let Some(mut rect) = clip(rect, self.size) else {
            return;
        };
        // Absorb every rectangle that touches the new one so the list stays disjoint-ish and short.
        let mut index = 0;
        while let Some(existing) = self.damage.get(index).copied() {
            if touches(existing, rect) {
                rect = union(existing, rect);
                self.damage.swap_remove(index);
                index = 0;
            } else {
                index = index.saturating_add(1);
            }
        }
        self.damage.push(rect);
        if self.damage.len() > MAX_DAMAGE_RECTS {
            let all = self.damage.iter().copied().fold(rect, union);
            self.damage.clear();
            self.damage.push(all);
        }
    }

    /// True when something must be laid out or painted.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.needs_layout || !self.damage.is_empty()
    }

    /// Lays the tree out again if anything structural changed; damages every widget whose rectangle moved.
    ///
    /// # Errors
    /// Returns an error from the layout engine.
    pub fn update_layout(&mut self) -> Result<()> {
        if !self.needs_layout {
            return Ok(());
        }
        self.needs_layout = false;
        let Some(root) = self.root else {
            return Ok(());
        };
        let root_layout = self.node(root)?.layout;
        let (width, height) = self.size;
        let bounds = layout::Rect::new(0.0, 0.0, u32_to_f32(width), u32_to_f32(height));
        let mut no_text = |_: &str, _: f32| Size::new(0.0, 0.0);
        self.layout.measure(
            root_layout,
            Constraints::tight(Size::new(bounds.width, bounds.height)),
            &mut no_text,
        )?;
        self.layout.arrange(root_layout, bounds, self.scale, &mut no_text)?;
        let mut scrolls: Vec<i32> = Vec::with_capacity(self.nodes.len());
        for index in 0..self.nodes.len() {
            let total = self
                .nodes
                .get(index)
                .and_then(|node| node.parent)
                .and_then(|parent| {
                    self.nodes.get(parent.0).map(|parent_node| {
                        scrolls
                            .get(parent.0)
                            .copied()
                            .unwrap_or_default()
                            .saturating_add(parent_node.scroll_y)
                    })
                })
                .unwrap_or_default();
            scrolls.push(total);
        }
        let mut moved = Vec::new();
        for (index, node) in self.nodes.iter_mut().enumerate() {
            let arranged = self.layout.rect(node.layout)?;
            let rect = Rect::new(
                to_px(arranged.x.round()),
                to_px(arranged.y.round()).saturating_sub(scrolls.get(index).copied().unwrap_or(0)),
                to_u32(arranged.width.round()),
                to_u32(arranged.height.round()),
            );
            if rect != node.rect {
                moved.push(node.rect);
                moved.push(rect);
                node.rect = rect;
            }
        }
        for rect in moved {
            self.add_damage(rect);
        }
        Ok(())
    }

    /// Repaints the damaged rectangles into a `0xAARRGGBB` frame of the tree's size and returns them.
    ///
    /// # Errors
    /// Returns an error when the frame does not match the tree size or layout fails.
    pub fn paint(&mut self, frame: &mut [u32], stride: usize) -> Result<Vec<Rect>> {
        self.update_layout()?;
        let damage = std::mem::take(&mut self.damage);
        let (width, height) = self.size;
        for area in &damage {
            let mut surface = Surface::new(frame, width, height, stride, *area)?;
            surface.fill_rect(*area, Radii::ZERO, self.background);
            if self.modal_dialog.is_some() {
                self.paint_nodes(&mut surface, *area, false);
                surface.dim(DIALOG_DIM_ALPHA);
                self.paint_nodes(&mut surface, *area, true);
            } else {
                self.paint_nodes(&mut surface, *area, false);
            }
        }
        Ok(damage)
    }

    /// The topmost visible interactive widget under a point.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<WidgetId> {
        self.hit_interactive(x, y, true)
    }

    fn hit_interactive(&self, x: i32, y: i32, enabled_only: bool) -> Option<WidgetId> {
        (0..self.nodes.len()).rev().map(WidgetId).find(|id| {
            self.nodes.get(id.0).is_some_and(|node| {
                (!enabled_only || node.enabled) && node.content.interactive() && contains(node.rect, x, y)
            }) && self.shown(*id)
                && self.clip_for(*id).is_none_or(|clip| contains(clip, x, y))
                && self.modal_dialog.is_none_or(|dialog| self.within_subtree(*id, dialog))
        })
    }

    /// Pointer moved; updates hover and damages what changed. Returns true when hover changed.
    pub fn pointer_moved(&mut self, x: i32, y: i32) -> bool {
        let hit = self.hit_interactive(x, y, false);
        if hit == self.hover {
            return false;
        }
        for id in [self.hover, hit].into_iter().flatten() {
            if let Some(rect) = self.nodes.get(id.0).map(|node| node.rect) {
                self.add_damage(rect);
            }
        }
        self.hover = hit;
        self.tooltip_ticks = 0;
        self.tooltip_visible = false;
        true
    }

    /// Pointer left the window.
    pub fn pointer_left(&mut self) {
        if let Some(rect) = self.hover.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect) {
            self.add_damage(rect);
        }
        self.hover = None;
        self.tooltip_ticks = 0;
        self.tooltip_visible = false;
    }

    /// Primary button pressed or released at a point. Returns the clicked widget on a release over the widget that
    /// received the press.
    pub fn pointer_button(&mut self, pressed: bool, x: i32, y: i32) -> Option<WidgetId> {
        self.pointer_moved(x, y);
        let hit = self.hit(x, y);
        if pressed {
            self.pressed = hit;
            if let Some(rect) = hit.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect) {
                self.add_damage(rect);
            }
            return None;
        }
        let was = self.pressed.take();
        if let Some(rect) = was.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect) {
            self.add_damage(rect);
        }
        if was.is_some() && was == hit {
            was
        } else {
            None
        }
    }

    fn shown(&self, id: WidgetId) -> bool {
        let mut current = Some(id);
        while let Some(at) = current {
            match self.nodes.get(at.0) {
                Some(node) if node.visible => current = node.parent,
                _ => return false,
            }
        }
        true
    }

    fn is_focusable(&self, id: WidgetId) -> bool {
        self.nodes
            .get(id.0)
            .is_some_and(|node| node.enabled && node.content.interactive())
            && self.shown(id)
            && self.modal_dialog.is_none_or(|dialog| self.within_subtree(id, dialog))
    }

    fn clip_for(&self, id: WidgetId) -> Option<Rect> {
        let mut current = self.nodes.get(id.0).and_then(|node| node.parent);
        let mut clip_rect: Option<Rect> = None;
        while let Some(at) = current {
            let node = self.nodes.get(at.0)?;
            if node.clip_children {
                clip_rect = Some(clip_rect.map_or(node.rect, |old| {
                    intersection(&old, node.rect).unwrap_or(Rect::new(0, 0, 0, 0))
                }));
            }
            current = node.parent;
        }
        clip_rect
    }

    fn within_subtree(&self, id: WidgetId, root: WidgetId) -> bool {
        let mut current = Some(id);
        while let Some(at) = current {
            if at == root {
                return true;
            }
            current = self.nodes.get(at.0).and_then(|node| node.parent);
        }
        false
    }

    fn change_focus(&mut self, focus: Option<WidgetId>) {
        if self.focused == focus {
            return;
        }
        self.input_composition = None;
        let old = self.focused.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect);
        self.focused = focus;
        let new = self.focused.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect);
        for rect in [old, new].into_iter().flatten() {
            self.add_damage(rect);
        }
    }

    fn paint_nodes(&mut self, surface: &mut Surface<'_>, area: Rect, dialog_only: bool) {
        let modal_dialog = self.modal_dialog;
        let mut shown = Vec::with_capacity(self.nodes.len());
        let mut clips: Vec<Option<Rect>> = Vec::with_capacity(self.nodes.len());
        let mut in_dialog = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let parent_shown = node
                .parent
                .and_then(|parent| shown.get(parent.0))
                .copied()
                .unwrap_or(true);
            shown.push(node.visible && parent_shown);

            let inherited_clip = node.parent.and_then(|parent| clips.get(parent.0)).copied().flatten();
            let clip = node
                .parent
                .and_then(|parent| self.nodes.get(parent.0))
                .map_or(inherited_clip, |parent| {
                    if parent.clip_children {
                        Some(inherited_clip.map_or(parent.rect, |old| {
                            intersection(&old, parent.rect).unwrap_or(Rect::new(0, 0, 0, 0))
                        }))
                    } else {
                        inherited_clip
                    }
                });
            clips.push(clip);

            let id = WidgetId(index);
            let parent_in_dialog = node
                .parent
                .and_then(|parent| in_dialog.get(parent.0))
                .copied()
                .unwrap_or(false);
            in_dialog.push(modal_dialog.is_some_and(|dialog| id == dialog || parent_in_dialog));
        }
        for index in 0..self.nodes.len() {
            let id = WidgetId(index);
            if modal_dialog.is_some() && in_dialog.get(index).copied().unwrap_or(false) != dialog_only {
                continue;
            }
            if !shown.get(index).copied().unwrap_or(false) {
                continue;
            }
            let Some(node) = self.nodes.get(index) else { continue };
            let clipped_area = match clips.get(index).copied().flatten() {
                Some(clip) => match intersection(&area, clip) {
                    Some(clipped) => clipped,
                    None => continue,
                },
                None => area,
            };
            if node.rect.width == 0 || node.rect.height == 0 || !touches(node.rect, clipped_area) {
                continue;
            }
            let hovered = self.hover == Some(id);
            let pressed = self.pressed == Some(id) && hovered;
            let rect = node.rect;
            let mut look = node.look;
            if self.focused == Some(id) {
                look.border = Some((Color::rgba(214, 166, 45, 255), 2.0));
            }
            let padding = node.style.padding;
            let mut content = node.content.clone();
            if let Content::Input { text, .. } = &mut content {
                if let Some(composition) = self.input_composition.as_ref().filter(|value| value.widget == id) {
                    *text = overlay_composition(text, composition.start, composition.end, &composition.text);
                }
            }
            let previous_clip = surface.replace_clip(clipped_area);
            paint_node(
                surface,
                (&mut self.fonts, &mut self.icon_cache),
                rect,
                padding,
                &look,
                &content,
                (hovered, pressed),
            );
            surface.replace_clip(previous_clip);
        }
    }

    fn restyle(&mut self, id: WidgetId) -> Result<()> {
        let node = self.node(id)?;
        let style = if node.visible {
            self.text_style(&node.content, node.style)
        } else {
            hidden_style()
        };
        let (layout, rect) = (node.layout, node.rect);
        self.layout.set_style(layout, style)?;
        self.add_damage(rect);
        self.needs_layout = true;
        Ok(())
    }

    /// The caller's style with the text size folded into the preferred size.
    fn text_style(&self, content: &Content, mut style: Style) -> Style {
        if let Some((text, text_style)) = content.text() {
            let line_height = self.fonts.line_height(text_style);
            if matches!(content, Content::Paragraph { .. }) {
                let constrained = if style.preferred.width.is_finite() && style.preferred.width > 0.0 {
                    style.preferred.width
                } else if style.max.width.is_finite() {
                    style.max.width
                } else {
                    self.fonts.measure(text, text_style) + style.padding.left + style.padding.right
                };
                let inner = (constrained - style.padding.left - style.padding.right).max(1.0);
                let lines = crate::text::break_lines(text, inner, &self.fonts.metrics(text_style));
                let count = u16::try_from(lines.len().max(1)).map_or(f32::from(u16::MAX), f32::from);
                let height = line_height * count + style.padding.top + style.padding.bottom;
                style.preferred.width = style.preferred.width.max(constrained.ceil());
                style.preferred.height = style.preferred.height.max(height.ceil());
                style.min.height = style.min.height.max(height.ceil());
            } else {
                let width = self.fonts.measure(text, text_style) + style.padding.left + style.padding.right;
                let height = line_height + style.padding.top + style.padding.bottom;
                style.preferred.width = style.preferred.width.max(width.ceil());
                style.preferred.height = style.preferred.height.max(height.ceil());
                style.min.height = style.min.height.max(height.ceil());
            }
        }
        style
    }

    fn node(&self, id: WidgetId) -> Result<&Node> {
        self.nodes
            .get(id.0)
            .ok_or_else(|| Error::Refused("unknown widget".to_owned()))
    }

    fn node_mut(&mut self, id: WidgetId) -> Result<&mut Node> {
        self.nodes
            .get_mut(id.0)
            .ok_or_else(|| Error::Refused("unknown widget".to_owned()))
    }
}

fn overlay_composition(text: &str, start: usize, end: usize, composition: &str) -> String {
    let mut chars = text.chars();
    let mut displayed = String::new();
    for _ in 0..start {
        let Some(character) = chars.next() else { break };
        displayed.push(character);
    }
    displayed.push_str(composition);
    for _ in start..end {
        let _ = chars.next();
    }
    displayed.extend(chars);
    displayed
}

fn paint_node(
    surface: &mut Surface<'_>,
    resources: (&mut Fonts, &mut IconCache),
    rect: Rect,
    padding: layout::Edges,
    look: &Look,
    content: &Content,
    (hovered, pressed): (bool, bool),
) {
    let radii = Radii::all(f64::from(look.radius));
    let interactive = content.interactive();
    let fill = if interactive && pressed {
        look.pressed_fill.or(look.hover_fill).or(look.fill)
    } else if interactive && hovered {
        look.hover_fill.or(look.fill)
    } else {
        look.fill
    };
    if let Some(color) = fill {
        surface.fill_rect(rect, radii, color);
    }
    if !matches!(content, Content::Image(_)) {
        if let Some((color, width)) = look.border {
            surface.border(rect, radii, f64::from(width), color);
        }
    }
    if let Some((color, width)) = look.accent_bar {
        surface.fill_rect(
            Rect::new(rect.x, rect.y, to_u32(width), rect.height),
            Radii::ZERO,
            color,
        );
    }
    if let Content::Image(Some(image)) = content {
        if let Ok(source) = ImageRef::new(&image.pixels, image.width, image.height, image.width as usize) {
            let x = rect.x.saturating_add(to_px(padding.left));
            let y = rect.y.saturating_add(to_px(padding.top));
            let destination = Rect::new(
                x,
                y,
                rect.width.saturating_sub(to_u32(padding.left + padding.right)),
                rect.height.saturating_sub(to_u32(padding.top + padding.bottom)),
            );
            surface.blit_image(source, destination, ImageFilter::Bilinear);
        }
        if let Some((color, width)) = look.border {
            surface.border(rect, radii, f64::from(width), color);
        }
        return;
    }
    if matches!(content, Content::Image(None)) {
        if let Some((color, width)) = look.border {
            surface.border(rect, radii, f64::from(width), color);
        }
        return;
    }
    if let Content::IconButton { icon, .. } = content {
        let size = u16::try_from(rect.height.min(18)).unwrap_or(18);
        if let Ok(bitmap) = resources.1.get(*icon, size, look.text.to_u32()) {
            if let Ok(mask) = MaskRef::new(&bitmap.alpha, u32::from(size), u32::from(size), usize::from(size)) {
                let x = rect.x.saturating_add(to_px(padding.left.max(8.0)));
                let y = rect
                    .y
                    .saturating_add(i32::try_from(rect.height.saturating_sub(u32::from(size)) / 2).unwrap_or(0));
                surface.blit_mask(mask, x, y, Color::from_u32(bitmap.color));
            }
        }
    }
    let Some((text, style)) = content.text() else {
        return;
    };
    let color = if interactive && hovered {
        look.hover_text.unwrap_or(look.text)
    } else {
        look.text
    };
    let line = resources.0.line_height(style);
    let icon_inset = if matches!(content, Content::IconButton { text, .. } if !text.is_empty()) {
        24.0
    } else {
        0.0
    };
    let left = i32_to_f32(rect.x) + padding.left + icon_inset;
    if matches!(content, Content::Paragraph { .. }) {
        let inner_width = (u32_to_f32(rect.width) - padding.left - padding.right).max(1.0);
        let lines = crate::text::break_lines(text, inner_width, &resources.0.metrics(style));
        let mut baseline = i32_to_f32(rect.y) + padding.top + resources.0.ascent(style);
        for wrapped in lines {
            if let Some(slice) = text.get(wrapped.start..wrapped.end) {
                resources.0.draw(surface, slice, left, baseline, style, look.text);
                if wrapped.append_hyphen {
                    let x = left + resources.0.measure(slice, style);
                    resources.0.draw(surface, "-", x, baseline, style, look.text);
                }
            }
            baseline += line;
        }
        return;
    }
    let text_width = resources.0.measure(text, style);
    let right = i32_to_f32(rect.x) + u32_to_f32(rect.width) - padding.right;
    let x = match look.align {
        TextAlign::Start => left,
        TextAlign::Center => left + (right - left - text_width).max(0.0) / 2.0,
        TextAlign::End => (right - text_width).max(left),
    };
    let top = i32_to_f32(rect.y) + padding.top;
    let inner = u32_to_f32(rect.height) - padding.top - padding.bottom;
    let baseline = top + (inner - line) / 2.0 + resources.0.ascent(style);
    resources.0.draw(surface, text, x, baseline, style, color);
}

fn hidden_style() -> Style {
    Style {
        preferred: Size::new(0.0, 0.0),
        max: Size::new(0.0, 0.0),
        shrink: 1.0,
        ..Style::default()
    }
}

fn intersection(a: &Rect, b: Rect) -> Option<Rect> {
    let x0 = i64::from(a.x).max(i64::from(b.x));
    let y0 = i64::from(a.y).max(i64::from(b.y));
    let x1 = i64::from(a.x)
        .saturating_add(i64::from(a.width))
        .min(i64::from(b.x).saturating_add(i64::from(b.width)));
    let y1 = i64::from(a.y)
        .saturating_add(i64::from(a.height))
        .min(i64::from(b.y).saturating_add(i64::from(b.height)));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect::new(
        i32::try_from(x0).ok()?,
        i32::try_from(y0).ok()?,
        u32::try_from(x1.saturating_sub(x0)).ok()?,
        u32::try_from(y1.saturating_sub(y0)).ok()?,
    ))
}

fn clip(rect: Rect, size: (u32, u32)) -> Option<Rect> {
    let x0 = i64::from(rect.x).max(0);
    let y0 = i64::from(rect.y).max(0);
    let x1 = i64::from(rect.x)
        .saturating_add(i64::from(rect.width))
        .min(i64::from(size.0));
    let y1 = i64::from(rect.y)
        .saturating_add(i64::from(rect.height))
        .min(i64::from(size.1));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(Rect::new(
        i32::try_from(x0).ok()?,
        i32::try_from(y0).ok()?,
        u32::try_from(x1.saturating_sub(x0)).ok()?,
        u32::try_from(y1.saturating_sub(y0)).ok()?,
    ))
}

fn edges(rect: Rect) -> (i64, i64, i64, i64) {
    let x = i64::from(rect.x);
    let y = i64::from(rect.y);
    (
        x,
        y,
        x.saturating_add(i64::from(rect.width)),
        y.saturating_add(i64::from(rect.height)),
    )
}

fn touches(a: Rect, b: Rect) -> bool {
    let (ax0, ay0, ax1, ay1) = edges(a);
    let (bx0, by0, bx1, by1) = edges(b);
    ax0 < bx1 && bx0 < ax1 && ay0 < by1 && by0 < ay1
}

fn union(a: Rect, b: Rect) -> Rect {
    let (ax0, ay0, ax1, ay1) = edges(a);
    let (bx0, by0, bx1, by1) = edges(b);
    let (x0, y0, x1, y1) = (ax0.min(bx0), ay0.min(by0), ax1.max(bx1), ay1.max(by1));
    Rect::new(
        i32::try_from(x0).unwrap_or(0),
        i32::try_from(y0).unwrap_or(0),
        u32::try_from(x1.saturating_sub(x0)).unwrap_or(0),
        u32::try_from(y1.saturating_sub(y0)).unwrap_or(0),
    )
}

fn contains(rect: Rect, x: i32, y: i32) -> bool {
    let (x0, y0, x1, y1) = edges(rect);
    let (x, y) = (i64::from(x), i64::from(y));
    x >= x0 && x < x1 && y >= y0 && y < y1
}

fn u32_to_f32(value: u32) -> f32 {
    u16::try_from(value).map_or(65_535.0, f32::from)
}

fn i32_to_f32(value: i32) -> f32 {
    i16::try_from(value).map_or(if value < 0 { -32_768.0 } else { 32_767.0 }, f32::from)
}

#[cfg(test)]
mod tests {
    use super::{Content, ImageData, Look, Tree, WidgetId};
    use crate::event_loop::ImeEvent;
    use crate::glyphs::{Face, Fonts, TextStyle};
    use crate::layout::{Align, NodeKind, Size, Style};
    use crate::raster::Color;

    #[test]
    fn image_widget_paints_shared_premultiplied_pixels() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let image = tree.add(
            None,
            NodeKind::Leaf,
            Style {
                preferred: Size::new(1.0, 1.0),
                min: Size::new(1.0, 1.0),
                ..Style::default()
            },
            Content::Image(None),
            Look::default(),
        )?;
        tree.set_image(
            image,
            Some(ImageData::new(1, 1, vec![Color::rgba(255, 0, 0, 255).to_u32()])?),
        )?;
        tree.resize(1, 1);
        let mut frame = vec![0_u32; 1];
        tree.paint(&mut frame, 1)?;
        assert_eq!(frame, vec![Color::rgba(255, 0, 0, 255).to_u32()]);
        Ok(())
    }

    #[test]
    fn image_data_rejects_invalid_dimensions() {
        assert!(ImageData::new(2, 2, vec![0_u32; 3]).is_err());
        assert!(ImageData::new(0, 2, Vec::new()).is_err());
    }

    fn dialog_tree() -> sse_core::Result<(Tree, WidgetId, WidgetId, WidgetId)> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let root = tree.add(None, NodeKind::Stack, Style::default(), Content::Panel, Look::default())?;
        let background = tree.add(
            Some(root),
            NodeKind::Leaf,
            Style {
                align_self: Some(Align::Stretch),
                ..Style::default()
            },
            Content::Button {
                text: "Содержимое".to_owned(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look {
                fill: Some(Color::rgba(120, 120, 120, 255)),
                ..Look::default()
            },
        )?;
        let overlay = tree.add(
            Some(root),
            NodeKind::Stack,
            Style {
                align_items: Align::Center,
                align_self: Some(Align::Stretch),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        tree.set_overlay_host(overlay)?;
        let dialog = tree.add(
            Some(overlay),
            NodeKind::Column,
            Style {
                preferred: Size::new(180.0, 100.0),
                ..Style::default()
            },
            Content::Panel,
            Look {
                fill: Some(Color::rgba(32, 32, 32, 255)),
                ..Look::default()
            },
        )?;
        let action = tree.add(
            Some(dialog),
            NodeKind::Leaf,
            Style::default(),
            Content::Button {
                text: "Подтвердить".to_owned(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?;
        tree.set_visible(dialog, false)?;
        tree.resize(400, 300);
        Ok((tree, background, dialog, action))
    }

    #[test]
    fn dialog_dims_background_and_confines_pointer_and_focus() -> sse_core::Result<()> {
        let (mut tree, background, dialog, action) = dialog_tree()?;
        tree.set_focus(Some(background))?;
        let mut frame = vec![0_u32; 400 * 300];
        tree.paint(&mut frame, 400)?;
        let before = frame.get(4_010).copied().unwrap_or_default();

        tree.open_dialog(dialog)?;
        tree.paint(&mut frame, 400)?;
        let after = frame.get(4_010).copied().unwrap_or_default();
        assert!(after < before, "the backdrop should be dimmed");
        assert_eq!(tree.focused(), Some(action));
        assert_eq!(tree.hit(10, 10), None, "background controls must not receive clicks");

        let rect = tree.rect(action)?;
        assert_eq!(tree.hit(rect.x + 1, rect.y + 1), Some(action));
        assert!(tree.close_dialog()?);
        assert_eq!(tree.focused(), Some(background));
        assert_eq!(tree.hit(10, 10), Some(background));
        Ok(())
    }
    #[test]
    fn input_set_get_and_changed_queue() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let input = tree.add(
            None,
            NodeKind::Leaf,
            Style::default(),
            Content::Input {
                text: String::new(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?;
        assert_eq!(tree.input_text(input)?, "");
        tree.set_input_text(input, "profile")?;
        assert_eq!(tree.input_text(input)?, "profile");
        assert_eq!(tree.take_changed_inputs(), vec![input]);
        assert!(tree.take_changed_inputs().is_empty());
        Ok(())
    }

    #[test]
    fn focused_input_displays_preedit_without_committing_and_cancels_it() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let input = tree.add(
            None,
            NodeKind::Leaf,
            Style::default(),
            Content::Input {
                text: "x".to_owned(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?;
        tree.set_focus(Some(input))?;
        assert!(tree.apply_ime_event(&ImeEvent::Start)?);
        assert!(tree.apply_ime_event(&ImeEvent::Update("かな".to_owned()))?);
        assert_eq!(tree.input_text(input)?, "x");
        assert_eq!(tree.input_display_text(input)?, "xかな");
        assert!(tree.apply_ime_event(&ImeEvent::Cancel)?);
        assert_eq!(tree.input_display_text(input)?, "x");

        assert!(tree.apply_ime_event(&ImeEvent::Start)?);
        assert!(tree.apply_ime_event(&ImeEvent::Update("かな".to_owned()))?);
        assert!(tree.apply_ime_event(&ImeEvent::Commit("仮名".to_owned()))?);
        assert_eq!(tree.input_text(input)?, "x仮名");
        Ok(())
    }

    #[test]
    fn focused_input_accepts_characters_backspace_and_inserted_text() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let input = tree.add(
            None,
            NodeKind::Leaf,
            Style::default(),
            Content::Input {
                text: String::new(),
                style: TextStyle::new(Face::Body, 14.0),
            },
            Look::default(),
        )?;
        tree.set_focus(Some(input))?;
        assert!(tree.edit_focused_input(u32::from('a'), Some("a"))?);
        assert!(tree.edit_focused_input(u32::from('b'), Some("b"))?);
        assert_eq!(tree.input_text(input)?, "ab");
        assert!(tree.edit_focused_input(0xff08, None)?);
        assert_eq!(tree.input_text(input)?, "a");
        assert!(tree.edit_focused_input(0, Some(" вставка"))?);
        assert_eq!(tree.input_text(input)?, "a вставка");
        assert_eq!(tree.take_changed_inputs(), vec![input]);
        Ok(())
    }

    #[test]
    fn disabled_widget_cannot_be_focused_or_clicked() -> sse_core::Result<()> {
        let (mut tree, background, _, _) = dialog_tree()?;
        tree.set_focus(Some(background))?;
        tree.set_enabled(background, false)?;
        assert_ne!(tree.focused(), Some(background));
        assert!(tree.set_focus(Some(background)).is_err());
        let rect = tree.rect(background)?;
        assert_eq!(tree.hit(rect.x + 1, rect.y + 1), None);
        Ok(())
    }
}
