//! Retained widget tree with damage tracking: only what changed is laid out again and repainted.
//!
//! Widgets live in an arena in creation order; a parent is always created before its children, so arena order is
//! paint order. Every visible change (text, look, hover, geometry) adds the old and new rectangles to the damage
//! list; [`Tree::paint`] redraws only those rectangles and returns them for the backend to present.

use crate::glyphs::{to_px, to_u32, Fonts, TextStyle};
use crate::layout::{self, Constraints, Layout, NodeId, NodeKind, Size, Style};
use crate::raster::{Color, Radii, Rect, Surface};
use sse_core::{Error, Result};

/// Damage rectangles kept separately before they are merged into one bounding box.
const MAX_DAMAGE_RECTS: usize = 16;

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
    /// One line of text.
    Label {
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
}

impl Content {
    fn text(&self) -> Option<(&str, TextStyle)> {
        match self {
            Self::Panel => None,
            Self::Label { text, style } | Self::Button { text, style } => Some((text.as_str(), *style)),
        }
    }

    const fn interactive(&self) -> bool {
        matches!(self, Self::Button { .. })
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
        }
    }

    /// Fonts used for measuring and drawing.
    pub fn fonts(&mut self) -> &mut Fonts {
        &mut self.fonts
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
            Content::Label { text: old, .. } | Content::Button { text: old, .. } if old != text => {
                text.clone_into(old);
            }
            _ => return Ok(()),
        }
        self.restyle(id)
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
        self.restyle(id)
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
        let mut moved = Vec::new();
        for node in &mut self.nodes {
            let arranged = self.layout.rect(node.layout)?;
            let rect = Rect::new(
                to_px(arranged.x.round()),
                to_px(arranged.y.round()),
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
            for index in 0..self.nodes.len() {
                let id = WidgetId(index);
                if !self.shown(id) {
                    continue;
                }
                let Some(node) = self.nodes.get(index) else { continue };
                if node.rect.width == 0 || node.rect.height == 0 || !touches(node.rect, *area) {
                    continue;
                }
                let hovered = self.hover == Some(id);
                let pressed = self.pressed == Some(id) && hovered;
                let rect = node.rect;
                let look = node.look;
                let padding = node.style.padding;
                let content = node.content.clone();
                paint_node(
                    &mut surface,
                    &mut self.fonts,
                    rect,
                    padding,
                    &look,
                    &content,
                    (hovered, pressed),
                );
            }
        }
        Ok(damage)
    }

    /// The topmost visible interactive widget under a point.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<WidgetId> {
        (0..self.nodes.len()).rev().map(WidgetId).find(|id| {
            self.nodes
                .get(id.0)
                .is_some_and(|node| node.content.interactive() && contains(node.rect, x, y))
                && self.shown(*id)
        })
    }

    /// Pointer moved; updates hover and damages what changed. Returns true when hover changed.
    pub fn pointer_moved(&mut self, x: i32, y: i32) -> bool {
        let hit = self.hit(x, y);
        if hit == self.hover {
            return false;
        }
        for id in [self.hover, hit].into_iter().flatten() {
            if let Some(rect) = self.nodes.get(id.0).map(|node| node.rect) {
                self.add_damage(rect);
            }
        }
        self.hover = hit;
        true
    }

    /// Pointer left the window.
    pub fn pointer_left(&mut self) {
        if let Some(rect) = self.hover.and_then(|id| self.nodes.get(id.0)).map(|node| node.rect) {
            self.add_damage(rect);
        }
        self.hover = None;
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
            let width = self.fonts.measure(text, text_style) + style.padding.left + style.padding.right;
            let height = self.fonts.line_height(text_style) + style.padding.top + style.padding.bottom;
            style.preferred.width = style.preferred.width.max(width.ceil());
            style.preferred.height = style.preferred.height.max(height.ceil());
            style.min.height = style.min.height.max(height.ceil());
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

fn paint_node(
    surface: &mut Surface<'_>,
    fonts: &mut Fonts,
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
    if let Some((color, width)) = look.border {
        surface.border(rect, radii, f64::from(width), color);
    }
    if let Some((color, width)) = look.accent_bar {
        surface.fill_rect(
            Rect::new(rect.x, rect.y, to_u32(width), rect.height),
            Radii::ZERO,
            color,
        );
    }
    let Some((text, style)) = content.text() else {
        return;
    };
    let color = if interactive && hovered {
        look.hover_text.unwrap_or(look.text)
    } else {
        look.text
    };
    let text_width = fonts.measure(text, style);
    let line = fonts.line_height(style);
    let left = i32_to_f32(rect.x) + padding.left;
    let right = i32_to_f32(rect.x) + u32_to_f32(rect.width) - padding.right;
    let x = match look.align {
        TextAlign::Start => left,
        TextAlign::Center => left + (right - left - text_width).max(0.0) / 2.0,
        TextAlign::End => (right - text_width).max(left),
    };
    let top = i32_to_f32(rect.y) + padding.top;
    let inner = u32_to_f32(rect.height) - padding.top - padding.bottom;
    let baseline = top + (inner - line) / 2.0 + fonts.ascent(style);
    fonts.draw(surface, text, x, baseline, style, color);
}

fn hidden_style() -> Style {
    Style {
        preferred: Size::new(0.0, 0.0),
        max: Size::new(0.0, 0.0),
        shrink: 1.0,
        ..Style::default()
    }
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
