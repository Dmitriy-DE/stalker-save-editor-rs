//! Dependency-free retained layout engine for `sse-ui`.
//!
//! The arena owns nodes and only stores integer handles between them. Measurement and arrangement are separate
//! passes. Each node keeps the last measured constraint and result; mutations invalidate that node and its ancestors,
//! so repeating layout with the same constraint does not descend into an unchanged subtree.
//!
//! Pixel snapping is performed on shared track boundaries rather than on rectangle widths independently. This is
//! important at fractional scale factors: adjacent row/column/grid cells therefore share the exact same snapped
//! edge and cannot develop one-physical-pixel gaps or overlaps.

use sse_core::{Error, Result};

const MAX_NODES: usize = 1_000_000;
const MAX_DEPTH: usize = 512;
const EPSILON: f32 = 0.000_1;

/// Handle of a node in [`Layout`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(usize);

impl NodeId {
    /// Returns the arena slot of this handle.
    #[must_use]
    pub fn index(self) -> usize {
        self.0
    }
}

/// Two-dimensional size in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    /// Horizontal extent.
    pub width: f32,
    /// Vertical extent.
    pub height: f32,
}

impl Size {
    /// Creates a size.
    #[must_use]
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Rectangle in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left coordinate.
    pub x: f32,
    /// Top coordinate.
    pub y: f32,
    /// Width.
    pub width: f32,
    /// Height.
    pub height: f32,
}

impl Rect {
    /// Creates a rectangle.
    #[must_use]
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

/// Four edge widths in logical pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Edges {
    /// Left edge.
    pub left: f32,
    /// Top edge.
    pub top: f32,
    /// Right edge.
    pub right: f32,
    /// Bottom edge.
    pub bottom: f32,
}

impl Edges {
    /// Same value on every edge.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    fn horizontal(self) -> f32 {
        non_negative(self.left) + non_negative(self.right)
    }

    fn vertical(self) -> f32 {
        non_negative(self.top) + non_negative(self.bottom)
    }
}

/// Width/height constraints used during measurement.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Constraints {
    /// Minimum accepted size.
    pub min: Size,
    /// Maximum accepted size. Use `f32::INFINITY` for an unconstrained axis.
    pub max: Size,
}

impl Constraints {
    /// Creates constraints and normalises negative/NaN values to safe values.
    #[must_use]
    pub fn new(min: Size, max: Size) -> Self {
        let max_width = sanitise_max(max.width);
        let max_height = sanitise_max(max.height);
        let min_width = sanitise_min(min.width).min(max_width);
        let min_height = sanitise_min(min.height).min(max_height);
        Self {
            min: Size::new(min_width, min_height),
            max: Size::new(max_width, max_height),
        }
    }

    /// Tight constraints for an exact size.
    #[must_use]
    pub fn tight(size: Size) -> Self {
        let safe = Size::new(sanitise_min(size.width), sanitise_min(size.height));
        Self {
            min: safe,
            max: safe,
        }
    }

    /// Unconstrained axes.
    #[must_use]
    pub const fn loose() -> Self {
        Self {
            min: Size::new(0.0, 0.0),
            max: Size::new(f32::INFINITY, f32::INFINITY),
        }
    }
}

/// Alignment on a cross axis or inside a grid/stack cell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// Place at the leading edge.
    #[default]
    Start,
    /// Centre in the available space.
    Center,
    /// Place at the trailing edge.
    End,
    /// Fill the available space, subject to min/max limits.
    Stretch,
}

/// Grid track sizing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Track {
    /// Exact logical-pixel size.
    Fixed(f32),
    /// Size to the largest measured child contribution in this track.
    Auto,
    /// Share remaining space proportionally. Non-positive values are treated as one fraction.
    Fraction(f32),
}

/// Optional explicit placement of a child in a grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridPlacement {
    /// Zero-based column.
    pub column: usize,
    /// Zero-based row.
    pub row: usize,
    /// Number of columns occupied. Zero is normalised to one.
    pub column_span: usize,
    /// Number of rows occupied. Zero is normalised to one.
    pub row_span: usize,
}

impl GridPlacement {
    /// Creates a one-cell placement.
    #[must_use]
    pub const fn cell(column: usize, row: usize) -> Self {
        Self {
            column,
            row,
            column_span: 1,
            row_span: 1,
        }
    }
}

/// Kind of a layout node.
#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    /// Fixed/preferred-size leaf.
    Leaf,
    /// Text measured by the callback supplied to [`Layout::measure`].
    Text(String),
    /// Horizontal flex line.
    Row,
    /// Vertical flex line.
    Column,
    /// Horizontal flow that starts a new line when the next child no longer fits.
    Wrap,
    /// Grid with explicit track definitions.
    Grid {
        /// Column tracks.
        columns: Vec<Track>,
        /// Row tracks.
        rows: Vec<Track>,
    },
    /// All children share the same content rectangle.
    Stack,
    /// One-child viewport. Enabled axes are measured unconstrained and report their full content size.
    Scroll {
        /// Permit content wider than the viewport.
        horizontal: bool,
        /// Permit content taller than the viewport.
        vertical: bool,
        /// Horizontal scroll offset in logical pixels.
        offset_x: f32,
        /// Vertical scroll offset in logical pixels.
        offset_y: f32,
    },
}

/// Per-node sizing and alignment parameters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Minimum content-box size.
    pub min: Size,
    /// Preferred content-box size.
    pub preferred: Size,
    /// Maximum content-box size.
    pub max: Size,
    /// Space outside the node.
    pub margin: Edges,
    /// Space between the node border and its children/content.
    pub padding: Edges,
    /// Horizontal/vertical gap between children.
    pub gap: Size,
    /// Cross-axis alignment used for children of this node.
    pub align_items: Align,
    /// Optional override of the parent's alignment for this node.
    pub align_self: Option<Align>,
    /// Share of positive main-axis free space.
    pub grow: f32,
    /// Share of main-axis shrink pressure.
    pub shrink: f32,
    /// Width divided by height. Non-positive or non-finite values disable the ratio.
    pub aspect_ratio: Option<f32>,
    /// Explicit grid placement when this node is a child of a grid.
    pub grid: Option<GridPlacement>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            min: Size::new(0.0, 0.0),
            preferred: Size::new(0.0, 0.0),
            max: Size::new(f32::INFINITY, f32::INFINITY),
            margin: Edges::default(),
            padding: Edges::default(),
            gap: Size::new(0.0, 0.0),
            align_items: Align::Start,
            align_self: None,
            grow: 0.0,
            shrink: 1.0,
            aspect_ratio: None,
            grid: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ConstraintKey {
    min_width: u32,
    min_height: u32,
    max_width: u32,
    max_height: u32,
}

impl ConstraintKey {
    fn of(value: Constraints) -> Self {
        Self {
            min_width: value.min.width.to_bits(),
            min_height: value.min.height.to_bits(),
            max_width: value.max.width.to_bits(),
            max_height: value.max.height.to_bits(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct MeasureCache {
    constraint: ConstraintKey,
    revision: u64,
    size: Size,
}

#[derive(Clone, Debug)]
struct Node {
    kind: NodeKind,
    style: Style,
    parent: Option<NodeId>,
    children: Vec<NodeId>,
    rect: Rect,
    content_size: Size,
    revision: u64,
    cache: Option<MeasureCache>,
}

impl Node {
    fn new(kind: NodeKind, style: Style) -> Self {
        Self {
            kind,
            style,
            parent: None,
            children: Vec::new(),
            rect: Rect::default(),
            content_size: Size::default(),
            revision: 1,
            cache: None,
        }
    }
}

/// Retained tree/arena layout engine.
#[derive(Debug, Default)]
pub struct Layout {
    nodes: Vec<Node>,
    revision_clock: u64,
    measure_visits: u64,
    cache_hits: u64,
}

impl Layout {
    /// Creates an empty arena.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            nodes: Vec::new(),
            revision_clock: 1,
            measure_visits: 0,
            cache_hits: 0,
        }
    }

    /// Adds a detached node and returns its stable handle.
    ///
    /// # Errors
    /// Refuses to grow beyond one million nodes.
    pub fn add(&mut self, kind: NodeKind, style: Style) -> Result<NodeId> {
        if self.nodes.len() >= MAX_NODES {
            return Err(Error::Refused("layout node limit reached".to_owned()));
        }
        let id = NodeId(self.nodes.len());
        self.nodes.push(Node::new(kind, sanitise_style(style)));
        Ok(id)
    }

    /// Makes `child` the last child of `parent`.
    ///
    /// A node has at most one parent. Cycles are refused.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for unknown handles and [`Error::Refused`] for cycles/multiple parents.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<()> {
        self.require(parent)?;
        self.require(child)?;
        if parent == child || self.is_ancestor(child, parent)? {
            return Err(Error::Refused("layout cycle refused".to_owned()));
        }
        if self.node(child)?.parent.is_some() {
            return Err(Error::Refused("layout node already has a parent".to_owned()));
        }
        self.node_mut(child)?.parent = Some(parent);
        self.node_mut(parent)?.children.push(child);
        self.invalidate(parent)?;
        Ok(())
    }

    /// Replaces the style and invalidates the affected subtree path.
    ///
    /// # Errors
    /// Returns an error for an unknown handle.
    pub fn set_style(&mut self, id: NodeId, style: Style) -> Result<()> {
        self.node_mut(id)?.style = sanitise_style(style);
        self.invalidate(id)
    }

    /// Replaces the node kind and invalidates cached measurements.
    ///
    /// # Errors
    /// Returns an error for an unknown handle.
    pub fn set_kind(&mut self, id: NodeId, kind: NodeKind) -> Result<()> {
        self.node_mut(id)?.kind = kind;
        self.invalidate(id)
    }

    /// Invalidates all cached text/container measurements, for example after changing the font scale.
    pub fn invalidate_all(&mut self) {
        self.revision_clock = self.revision_clock.saturating_add(1);
        let revision = self.revision_clock;
        for node in &mut self.nodes {
            node.revision = revision;
            node.cache = None;
        }
    }

    /// Measures `root` under `constraints`.
    ///
    /// `text` receives the node text and available content width and returns its measured content size. The callback
    /// is not called when the node's same-constraint result is cached.
    ///
    /// # Errors
    /// Returns an error for invalid handles, excessive tree depth, malformed grids, or non-finite callback results.
    pub fn measure<F>(&mut self, root: NodeId, constraints: Constraints, text: &mut F) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        self.require(root)?;
        self.measure_node(root, Constraints::new(constraints.min, constraints.max), text, 0)
    }

    /// Arranges the already-retained tree into `rect` and snaps it to the requested physical-pixel scale.
    ///
    /// Measurement is performed as needed and therefore benefits from the same caches. Scale must be finite and
    /// positive; invalid scales are refused instead of silently producing NaNs.
    ///
    /// # Errors
    /// Returns an error for invalid handles, excessive tree depth, malformed grids, or invalid scale.
    pub fn arrange<F>(&mut self, root: NodeId, rect: Rect, scale: f32, text: &mut F) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        self.require(root)?;
        if !scale.is_finite() || scale <= 0.0 {
            return Err(Error::Refused("layout scale must be finite and positive".to_owned()));
        }
        let safe = Rect::new(
            finite_or_zero(rect.x),
            finite_or_zero(rect.y),
            non_negative(rect.width),
            non_negative(rect.height),
        );
        self.arrange_node(root, safe, scale, text, 0)
    }

    /// Last arranged rectangle of a node.
    ///
    /// # Errors
    /// Returns an error for an unknown handle.
    pub fn rect(&self, id: NodeId) -> Result<Rect> {
        Ok(self.node(id)?.rect)
    }

    /// Full content extent of a scroll container after arrangement; for other nodes this is the arranged content box.
    ///
    /// # Errors
    /// Returns an error for an unknown handle.
    pub fn content_size(&self, id: NodeId) -> Result<Size> {
        Ok(self.node(id)?.content_size)
    }

    /// Number of nodes actually visited by the measurement algorithm since the last counter reset.
    #[must_use]
    pub const fn measure_visits(&self) -> u64 {
        self.measure_visits
    }

    /// Number of same-constraint measurement cache hits since the last counter reset.
    #[must_use]
    pub const fn cache_hits(&self) -> u64 {
        self.cache_hits
    }

    /// Resets instrumentation counters without touching layout state.
    pub fn reset_counters(&mut self) {
        self.measure_visits = 0;
        self.cache_hits = 0;
    }

    fn measure_node<F>(
        &mut self,
        id: NodeId,
        constraints: Constraints,
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        if depth > MAX_DEPTH {
            return Err(Error::Refused("layout tree is too deep".to_owned()));
        }
        let key = ConstraintKey::of(constraints);
        let revision = self.node(id)?.revision;
        if let Some(cache) = self.node(id)?.cache {
            if cache.revision == revision && cache.constraint == key {
                self.cache_hits = self.cache_hits.saturating_add(1);
                return Ok(cache.size);
            }
        }
        self.measure_visits = self.measure_visits.saturating_add(1);

        let kind = self.node(id)?.kind.clone();
        let style = self.node(id)?.style;
        let children = self.node(id)?.children.clone();
        let inner_constraints = content_constraints(constraints, style);
        let next_depth = depth
            .checked_add(1)
            .ok_or_else(|| Error::Refused("layout depth overflow".to_owned()))?;
        let content = match kind {
            NodeKind::Leaf => preferred_in(style, inner_constraints),
            NodeKind::Text(value) => {
                let width = if style.preferred.width > EPSILON {
                    preferred_axis(
                        style.preferred.width,
                        style.min.width,
                        style.max.width,
                        inner_constraints.min.width,
                        inner_constraints.max.width,
                    )
                } else {
                    inner_constraints.max.width
                };
                let measured = text(&value, width);
                if !valid_size(measured) {
                    return Err(Error::Damaged("text measurer returned a non-finite size".to_owned()));
                }
                apply_content_style(measured, style, inner_constraints)
            }
            NodeKind::Row => self.measure_linear(&children, style, inner_constraints, true, text, next_depth)?,
            NodeKind::Column => {
                self.measure_linear(&children, style, inner_constraints, false, text, next_depth)?
            }
            NodeKind::Wrap => self.measure_wrap(&children, style, inner_constraints, text, next_depth)?,
            NodeKind::Grid { columns, rows } => {
                self.measure_grid(&children, style, inner_constraints, &columns, &rows, text, next_depth)?
            }
            NodeKind::Stack => self.measure_stack(&children, style, inner_constraints, text, next_depth)?,
            NodeKind::Scroll {
                horizontal,
                vertical,
                ..
            } => self.measure_scroll(
                &children,
                style,
                inner_constraints,
                horizontal,
                vertical,
                text,
                next_depth,
            )?,
        };
        let with_padding = Size::new(
            content.width + style.padding.horizontal(),
            content.height + style.padding.vertical(),
        );
        let result = clamp_size(with_padding, constraints);
        self.node_mut(id)?.cache = Some(MeasureCache {
            constraint: key,
            revision,
            size: result,
        });
        Ok(result)
    }

    fn measure_linear<F>(
        &mut self,
        children: &[NodeId],
        style: Style,
        constraints: Constraints,
        horizontal: bool,
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let mut main = 0.0_f32;
        let mut cross = 0.0_f32;
        let mut count = 0_usize;
        for child in children {
            let child_style = self.node(*child)?.style;
            let child_constraints = if horizontal {
                Constraints::new(Size::new(0.0, 0.0), Size::new(f32::INFINITY, constraints.max.height))
            } else {
                Constraints::new(Size::new(0.0, 0.0), Size::new(constraints.max.width, f32::INFINITY))
            };
            let size = self.measure_node(*child, child_constraints, text, depth)?;
            let outer = outer_size(size, child_style.margin);
            if horizontal {
                main += outer.width;
                cross = cross.max(outer.height);
            } else {
                main += outer.height;
                cross = cross.max(outer.width);
            }
            count = count
                .checked_add(1)
                .ok_or_else(|| Error::Refused("layout child count overflow".to_owned()))?;
        }
        let gaps = gap_total(count, if horizontal { style.gap.width } else { style.gap.height });
        main += gaps;
        let raw = if horizontal {
            Size::new(main, cross)
        } else {
            Size::new(cross, main)
        };
        Ok(apply_content_style(raw, style, constraints))
    }

    fn measure_wrap<F>(
        &mut self,
        children: &[NodeId],
        style: Style,
        constraints: Constraints,
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let available = constraints.max.width;
        let finite_width = available.is_finite();
        let mut line_width = 0.0_f32;
        let mut line_height = 0.0_f32;
        let mut total_height = 0.0_f32;
        let mut max_width = 0.0_f32;
        let mut line_count = 0_usize;
        for child in children {
            let child_style = self.node(*child)?.style;
            let size = self.measure_node(*child, Constraints::loose(), text, depth)?;
            let outer = outer_size(size, child_style.margin);
            let gap = if line_count == 0 { 0.0 } else { non_negative(style.gap.width) };
            let candidate = line_width + gap + outer.width;
            if finite_width && line_count != 0 && candidate > available + EPSILON {
                max_width = max_width.max(line_width);
                if total_height > 0.0 {
                    total_height += non_negative(style.gap.height);
                }
                total_height += line_height;
                line_width = outer.width;
                line_height = outer.height;
                line_count = 1;
            } else {
                line_width = candidate;
                line_height = line_height.max(outer.height);
                line_count = line_count
                    .checked_add(1)
                    .ok_or_else(|| Error::Refused("layout wrap count overflow".to_owned()))?;
            }
        }
        if line_count != 0 {
            max_width = max_width.max(line_width);
            if total_height > 0.0 {
                total_height += non_negative(style.gap.height);
            }
            total_height += line_height;
        }
        Ok(apply_content_style(
            Size::new(max_width, total_height),
            style,
            constraints,
        ))
    }

    fn measure_stack<F>(
        &mut self,
        children: &[NodeId],
        style: Style,
        constraints: Constraints,
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let mut size = Size::default();
        for child in children {
            let child_style = self.node(*child)?.style;
            let measured = self.measure_node(*child, constraints, text, depth)?;
            let outer = outer_size(measured, child_style.margin);
            size.width = size.width.max(outer.width);
            size.height = size.height.max(outer.height);
        }
        Ok(apply_content_style(size, style, constraints))
    }

    fn measure_scroll<F>(
        &mut self,
        children: &[NodeId],
        style: Style,
        constraints: Constraints,
        horizontal: bool,
        vertical: bool,
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        if children.len() > 1 {
            return Err(Error::Refused("scroll container accepts at most one child".to_owned()));
        }
        let mut content = Size::default();
        if let Some(child) = children.first().copied() {
            let child_constraints = Constraints::new(
                Size::new(0.0, 0.0),
                Size::new(
                    if horizontal { f32::INFINITY } else { constraints.max.width },
                    if vertical { f32::INFINITY } else { constraints.max.height },
                ),
            );
            let child_style = self.node(child)?.style;
            content = outer_size(self.measure_node(child, child_constraints, text, depth)?, child_style.margin);
        }
        let viewport = Size::new(
            if constraints.max.width.is_finite() {
                constraints.max.width
            } else {
                content.width
            },
            if constraints.max.height.is_finite() {
                constraints.max.height
            } else {
                content.height
            },
        );
        Ok(apply_content_style(viewport, style, constraints))
    }

    fn measure_grid<F>(
        &mut self,
        children: &[NodeId],
        style: Style,
        constraints: Constraints,
        columns: &[Track],
        rows: &[Track],
        text: &mut F,
        depth: usize,
    ) -> Result<Size>
    where
        F: FnMut(&str, f32) -> Size,
    {
        if columns.is_empty() || rows.is_empty() {
            return Err(Error::Refused("grid needs at least one row and column".to_owned()));
        }
        let intrinsic = self.grid_intrinsic(children, columns, rows, text, depth)?;
        let column_sizes = resolve_tracks(columns, &intrinsic.columns, constraints.max.width, style.gap.width)?;
        let row_sizes = resolve_tracks(rows, &intrinsic.rows, constraints.max.height, style.gap.height)?;
        let width = sum_values(&column_sizes) + gap_total(column_sizes.len(), style.gap.width);
        let height = sum_values(&row_sizes) + gap_total(row_sizes.len(), style.gap.height);
        Ok(apply_content_style(Size::new(width, height), style, constraints))
    }

    fn grid_intrinsic<F>(
        &mut self,
        children: &[NodeId],
        columns: &[Track],
        rows: &[Track],
        text: &mut F,
        depth: usize,
    ) -> Result<GridIntrinsic>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let mut column_auto = vec![0.0_f32; columns.len()];
        let mut row_auto = vec![0.0_f32; rows.len()];
        for (position, child) in children.iter().copied().enumerate() {
            let placement = self.grid_placement(child, position, columns.len(), rows.len())?;
            let child_style = self.node(child)?.style;
            let measured = self.measure_node(child, Constraints::loose(), text, depth)?;
            let outer = outer_size(measured, child_style.margin);
            if placement.column_span == 1 {
                if matches!(columns.get(placement.column), Some(Track::Auto)) {
                    let slot = column_auto
                        .get_mut(placement.column)
                        .ok_or_else(|| Error::Damaged("grid column outside track table".to_owned()))?;
                    *slot = slot.max(outer.width);
                }
            }
            if placement.row_span == 1 {
                if matches!(rows.get(placement.row), Some(Track::Auto)) {
                    let slot = row_auto
                        .get_mut(placement.row)
                        .ok_or_else(|| Error::Damaged("grid row outside track table".to_owned()))?;
                    *slot = slot.max(outer.height);
                }
            }
        }
        Ok(GridIntrinsic {
            columns: column_auto,
            rows: row_auto,
        })
    }

    fn arrange_node<F>(
        &mut self,
        id: NodeId,
        rect: Rect,
        scale: f32,
        text: &mut F,
        depth: usize,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        if depth > MAX_DEPTH {
            return Err(Error::Refused("layout tree is too deep".to_owned()));
        }
        let style = self.node(id)?.style;
        let own = inset_rect(rect, style.margin);
        self.node_mut(id)?.rect = own;
        let content_rect = inset_rect(own, style.padding);
        self.node_mut(id)?.content_size = Size::new(content_rect.width, content_rect.height);
        let kind = self.node(id)?.kind.clone();
        let children = self.node(id)?.children.clone();
        let next_depth = depth
            .checked_add(1)
            .ok_or_else(|| Error::Refused("layout depth overflow".to_owned()))?;
        match kind {
            NodeKind::Leaf | NodeKind::Text(_) => Ok(()),
            NodeKind::Row => {
                self.arrange_linear(&children, content_rect, style, true, scale, text, next_depth)
            }
            NodeKind::Column => {
                self.arrange_linear(&children, content_rect, style, false, scale, text, next_depth)
            }
            NodeKind::Wrap => self.arrange_wrap(&children, content_rect, style, scale, text, next_depth),
            NodeKind::Grid { columns, rows } => self.arrange_grid(
                &children,
                GridArrange {
                    rect: content_rect,
                    style,
                    columns: &columns,
                    rows: &rows,
                    scale,
                    depth: next_depth,
                },
                text,
            ),
            NodeKind::Stack => self.arrange_stack(&children, content_rect, style, scale, text, next_depth),
            NodeKind::Scroll {
                horizontal,
                vertical,
                offset_x,
                offset_y,
            } => self.arrange_scroll(
                &children,
                ScrollArrange {
                    rect: content_rect,
                    horizontal,
                    vertical,
                    offset_x,
                    offset_y,
                    scale,
                    depth: next_depth,
                },
                text,
            ),
        }
    }

    fn arrange_linear<F>(
        &mut self,
        children: &[NodeId],
        rect: Rect,
        style: Style,
        horizontal: bool,
        scale: f32,
        text: &mut F,
        depth: usize,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let mut items = Vec::<LinearItem>::with_capacity(children.len());
        let mut used = 0.0_f32;
        let mut total_grow = 0.0_f32;
        let mut total_shrink = 0.0_f32;
        for child in children {
            let child_style = self.node(*child)?.style;
            let max = if horizontal {
                Size::new(f32::INFINITY, rect.height)
            } else {
                Size::new(rect.width, f32::INFINITY)
            };
            let measured = self.measure_node(*child, Constraints::new(Size::default(), max), text, depth)?;
            let outer = outer_size(measured, child_style.margin);
            let base = if horizontal { outer.width } else { outer.height };
            used += base;
            total_grow += non_negative(child_style.grow);
            total_shrink += non_negative(child_style.shrink) * base.max(1.0);
            items.push(LinearItem {
                id: *child,
                measured,
                style: child_style,
                main: base,
            });
        }
        let gap = if horizontal { style.gap.width } else { style.gap.height };
        used += gap_total(items.len(), gap);
        let available = if horizontal { rect.width } else { rect.height };
        let free = available - used;
        for item in &mut items {
            if free > EPSILON && total_grow > EPSILON {
                item.main += free * non_negative(item.style.grow) / total_grow;
            } else if free < -EPSILON && total_shrink > EPSILON {
                let pressure = non_negative(item.style.shrink) * item.main.max(1.0);
                item.main = (item.main + free * pressure / total_shrink).max(0.0);
            }
        }

        let geometry = snapped_item_geometry(rect, &items, non_negative(gap), horizontal, scale)?;
        for (index, item) in items.iter().enumerate() {
            let start = *geometry
                .starts
                .get(index)
                .ok_or_else(|| Error::Damaged("missing linear start boundary".to_owned()))?;
            let end = *geometry
                .ends
                .get(index)
                .ok_or_else(|| Error::Damaged("missing linear end boundary".to_owned()))?;
            let cell = if horizontal {
                Rect::new(start, rect.y, (end - start).max(0.0), rect.height)
            } else {
                Rect::new(rect.x, start, rect.width, (end - start).max(0.0))
            };
            let child_rect = align_in_cell(cell, item.measured, item.style, style.align_items, horizontal);
            self.arrange_node(
                item.id,
                snap_rect_within(child_rect, cell, scale),
                scale,
                text,
                depth,
            )?;
        }
        Ok(())
    }

    fn arrange_wrap<F>(
        &mut self,
        children: &[NodeId],
        rect: Rect,
        style: Style,
        scale: f32,
        text: &mut F,
        depth: usize,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let mut lines = Vec::<WrapLine>::new();
        let mut line = WrapLine::default();
        for child in children {
            let child_style = self.node(*child)?.style;
            let measured = self.measure_node(*child, Constraints::loose(), text, depth)?;
            let outer = outer_size(measured, child_style.margin);
            let gap = if line.items.is_empty() { 0.0 } else { non_negative(style.gap.width) };
            let candidate = line.width + gap + outer.width;
            if !line.items.is_empty() && candidate > rect.width + EPSILON {
                lines.push(line);
                line = WrapLine::default();
            }
            if !line.items.is_empty() {
                line.width += non_negative(style.gap.width);
            }
            line.width += outer.width;
            line.height = line.height.max(outer.height);
            line.items.push(LinearItem {
                id: *child,
                measured,
                style: child_style,
                main: outer.width,
            });
        }
        if !line.items.is_empty() {
            lines.push(line);
        }

        let mut y = rect.y;
        for current in &lines {
            let y0 = snap_position(y, scale);
            let y1 = snap_position((y + current.height).min(rect.bottom()), scale);
            let line_rect = Rect::new(rect.x, y0, rect.width, (y1 - y0).max(0.0));
            let geometry = snapped_item_geometry(
                line_rect,
                &current.items,
                non_negative(style.gap.width),
                true,
                scale,
            )?;
            for (index, item) in current.items.iter().enumerate() {
                let start = *geometry
                    .starts
                    .get(index)
                    .ok_or_else(|| Error::Damaged("missing wrap start boundary".to_owned()))?;
                let end = *geometry
                    .ends
                    .get(index)
                    .ok_or_else(|| Error::Damaged("missing wrap end boundary".to_owned()))?;
                let cell = Rect::new(start, y0, (end - start).max(0.0), (y1 - y0).max(0.0));
                let child_rect = align_in_cell(cell, item.measured, item.style, style.align_items, true);
                self.arrange_node(
                    item.id,
                    snap_rect_within(child_rect, cell, scale),
                    scale,
                    text,
                    depth,
                )?;
            }
            y = y + current.height + non_negative(style.gap.height);
        }
        Ok(())
    }

    fn arrange_stack<F>(
        &mut self,
        children: &[NodeId],
        rect: Rect,
        style: Style,
        scale: f32,
        text: &mut F,
        depth: usize,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        for child in children {
            let child_style = self.node(*child)?.style;
            let measured = self.measure_node(
                *child,
                Constraints::new(Size::default(), Size::new(rect.width, rect.height)),
                text,
                depth,
            )?;
            let horizontal = child_style.align_self.unwrap_or(style.align_items);
            let vertical = child_style.align_self.unwrap_or(style.align_items);
            let child_rect = align_two_axes(rect, measured, child_style, horizontal, vertical);
            self.arrange_node(
                *child,
                snap_rect_within(child_rect, rect, scale),
                scale,
                text,
                depth,
            )?;
        }
        Ok(())
    }

    fn arrange_scroll<F>(
        &mut self,
        children: &[NodeId],
        args: ScrollArrange,
        text: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let ScrollArrange {
            rect,
            horizontal,
            vertical,
            offset_x,
            offset_y,
            scale,
            depth,
        } = args;
        if children.len() > 1 {
            return Err(Error::Refused("scroll container accepts at most one child".to_owned()));
        }
        let Some(child) = children.first().copied() else {
            return Ok(());
        };
        let child_style = self.node(child)?.style;
        let max = Size::new(
            if horizontal { f32::INFINITY } else { rect.width },
            if vertical { f32::INFINITY } else { rect.height },
        );
        let measured = self.measure_node(child, Constraints::new(Size::default(), max), text, depth)?;
        let outer = outer_size(measured, child_style.margin);
        let parent = self
            .node(child)?
            .parent
            .ok_or_else(|| Error::Damaged("scroll child lost its parent".to_owned()))?;
        self.node_mut(parent)?.content_size = outer;
        let max_x = (outer.width - rect.width).max(0.0);
        let max_y = (outer.height - rect.height).max(0.0);
        let x = rect.x - non_negative(offset_x).min(max_x);
        let y = rect.y - non_negative(offset_y).min(max_y);
        let child_rect = Rect::new(
            x,
            y,
            if horizontal { outer.width } else { rect.width },
            if vertical { outer.height } else { rect.height },
        );
        self.arrange_node(child, snap_rect(child_rect, scale), scale, text, depth)
    }

    fn arrange_grid<F>(
        &mut self,
        children: &[NodeId],
        args: GridArrange<'_>,
        text: &mut F,
    ) -> Result<()>
    where
        F: FnMut(&str, f32) -> Size,
    {
        let GridArrange {
            rect,
            style,
            columns,
            rows,
            scale,
            depth,
        } = args;
        if columns.is_empty() || rows.is_empty() {
            return Err(Error::Refused("grid needs at least one row and column".to_owned()));
        }
        let intrinsic = self.grid_intrinsic(children, columns, rows, text, depth)?;
        let column_sizes = resolve_tracks(columns, &intrinsic.columns, rect.width, style.gap.width)?;
        let row_sizes = resolve_tracks(rows, &intrinsic.rows, rect.height, style.gap.height)?;
        let x_geometry = track_geometry(rect.x, rect.width, &column_sizes, style.gap.width, scale)?;
        let y_geometry = track_geometry(rect.y, rect.height, &row_sizes, style.gap.height, scale)?;

        for (position, child) in children.iter().copied().enumerate() {
            let placement = self.grid_placement(child, position, columns.len(), rows.len())?;
            let column_end = placement
                .column
                .checked_add(placement.column_span)
                .ok_or_else(|| Error::Refused("grid span overflow".to_owned()))?;
            let row_end = placement
                .row
                .checked_add(placement.row_span)
                .ok_or_else(|| Error::Refused("grid span overflow".to_owned()))?;
            let last_column = column_end
                .checked_sub(1)
                .ok_or_else(|| Error::Refused("grid column span underflow".to_owned()))?;
            let last_row = row_end
                .checked_sub(1)
                .ok_or_else(|| Error::Refused("grid row span underflow".to_owned()))?;
            let x0 = *x_geometry
                .starts
                .get(placement.column)
                .ok_or_else(|| Error::Damaged("grid column start missing".to_owned()))?;
            let x1 = *x_geometry
                .ends
                .get(last_column)
                .ok_or_else(|| Error::Damaged("grid column end missing".to_owned()))?;
            let y0 = *y_geometry
                .starts
                .get(placement.row)
                .ok_or_else(|| Error::Damaged("grid row start missing".to_owned()))?;
            let y1 = *y_geometry
                .ends
                .get(last_row)
                .ok_or_else(|| Error::Damaged("grid row end missing".to_owned()))?;
            let cell = Rect::new(
                x0,
                y0,
                (x1 - x0).max(0.0),
                (y1 - y0).max(0.0),
            );
            let child_style = self.node(child)?.style;
            let measured = self.measure_node(
                child,
                Constraints::new(Size::default(), Size::new(cell.width, cell.height)),
                text,
                depth,
            )?;
            let align = child_style.align_self.unwrap_or(style.align_items);
            let child_rect = align_two_axes(cell, measured, child_style, align, align);
            self.arrange_node(
                child,
                snap_rect_within(child_rect, cell, scale),
                scale,
                text,
                depth,
            )?;
        }
        Ok(())
    }

    fn grid_placement(
        &self,
        child: NodeId,
        position: usize,
        column_count: usize,
        row_count: usize,
    ) -> Result<GridPlacement> {
        if column_count == 0 || row_count == 0 {
            return Err(Error::Refused("grid track table is empty".to_owned()));
        }
        let explicit = self.node(child)?.style.grid;
        let placement = if let Some(mut value) = explicit {
            value.column_span = value.column_span.max(1);
            value.row_span = value.row_span.max(1);
            value
        } else {
            GridPlacement {
                column: position
                    .checked_rem(column_count)
                    .ok_or_else(|| Error::Refused("grid column divisor is zero".to_owned()))?,
                row: position
                    .checked_div(column_count)
                    .ok_or_else(|| Error::Refused("grid column divisor is zero".to_owned()))?,
                column_span: 1,
                row_span: 1,
            }
        };
        let column_end = placement
            .column
            .checked_add(placement.column_span)
            .ok_or_else(|| Error::Refused("grid column span overflow".to_owned()))?;
        let row_end = placement
            .row
            .checked_add(placement.row_span)
            .ok_or_else(|| Error::Refused("grid row span overflow".to_owned()))?;
        if column_end > column_count || row_end > row_count {
            return Err(Error::Refused("grid placement outside track table".to_owned()));
        }
        Ok(placement)
    }

    fn invalidate(&mut self, start: NodeId) -> Result<()> {
        self.revision_clock = self.revision_clock.saturating_add(1);
        let revision = self.revision_clock;
        let mut cursor = Some(start);
        let mut depth = 0_usize;
        while let Some(id) = cursor {
            if depth > MAX_DEPTH {
                return Err(Error::Refused("layout parent chain is too deep".to_owned()));
            }
            let parent = self.node(id)?.parent;
            let node = self.node_mut(id)?;
            node.revision = revision;
            node.cache = None;
            cursor = parent;
            depth = depth
                .checked_add(1)
                .ok_or_else(|| Error::Refused("layout depth overflow".to_owned()))?;
        }
        Ok(())
    }

    fn is_ancestor(&self, possible_ancestor: NodeId, node: NodeId) -> Result<bool> {
        let mut cursor = Some(node);
        let mut depth = 0_usize;
        while let Some(id) = cursor {
            if id == possible_ancestor {
                return Ok(true);
            }
            if depth > MAX_DEPTH {
                return Err(Error::Refused("layout parent chain is too deep".to_owned()));
            }
            cursor = self.node(id)?.parent;
            depth = depth
                .checked_add(1)
                .ok_or_else(|| Error::Refused("layout depth overflow".to_owned()))?;
        }
        Ok(false)
    }

    fn require(&self, id: NodeId) -> Result<()> {
        self.node(id).map(|_| ())
    }

    fn node(&self, id: NodeId) -> Result<&Node> {
        self.nodes
            .get(id.0)
            .ok_or_else(|| Error::Damaged(format!("unknown layout node {}", id.0)))
    }

    fn node_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.nodes
            .get_mut(id.0)
            .ok_or_else(|| Error::Damaged(format!("unknown layout node {}", id.0)))
    }
}

#[derive(Debug)]
struct GridIntrinsic {
    columns: Vec<f32>,
    rows: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
struct LinearItem {
    id: NodeId,
    measured: Size,
    style: Style,
    main: f32,
}

#[derive(Clone, Copy, Debug)]
struct ScrollArrange {
    rect: Rect,
    horizontal: bool,
    vertical: bool,
    offset_x: f32,
    offset_y: f32,
    scale: f32,
    depth: usize,
}

#[derive(Clone, Copy, Debug)]
struct GridArrange<'a> {
    rect: Rect,
    style: Style,
    columns: &'a [Track],
    rows: &'a [Track],
    scale: f32,
    depth: usize,
}

#[derive(Debug, Default)]
struct WrapLine {
    items: Vec<LinearItem>,
    width: f32,
    height: f32,
}

fn sanitise_style(mut style: Style) -> Style {
    style.min = Size::new(sanitise_min(style.min.width), sanitise_min(style.min.height));
    style.preferred = Size::new(
        sanitise_min(style.preferred.width),
        sanitise_min(style.preferred.height),
    );
    style.max = Size::new(sanitise_max(style.max.width), sanitise_max(style.max.height));
    style.max.width = style.max.width.max(style.min.width);
    style.max.height = style.max.height.max(style.min.height);
    style.margin = sanitise_edges(style.margin);
    style.padding = sanitise_edges(style.padding);
    style.gap = Size::new(non_negative(style.gap.width), non_negative(style.gap.height));
    style.grow = non_negative(style.grow);
    style.shrink = non_negative(style.shrink);
    style.aspect_ratio = style
        .aspect_ratio
        .filter(|value| value.is_finite() && *value > EPSILON);
    style
}

fn sanitise_edges(value: Edges) -> Edges {
    Edges {
        left: non_negative(value.left),
        top: non_negative(value.top),
        right: non_negative(value.right),
        bottom: non_negative(value.bottom),
    }
}

fn sanitise_min(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn sanitise_max(value: f32) -> f32 {
    if value.is_nan() || value < 0.0 {
        0.0
    } else {
        value
    }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn non_negative(value: f32) -> f32 {
    if value.is_finite() { value.max(0.0) } else { 0.0 }
}

fn valid_size(value: Size) -> bool {
    value.width.is_finite() && value.height.is_finite() && value.width >= 0.0 && value.height >= 0.0
}

fn preferred_axis(preferred: f32, style_min: f32, style_max: f32, min: f32, max: f32) -> f32 {
    let low = non_negative(style_min).max(non_negative(min));
    let high = sanitise_max(style_max).min(sanitise_max(max)).max(low);
    non_negative(preferred).clamp(low, high)
}

fn preferred_in(style: Style, constraints: Constraints) -> Size {
    apply_content_style(style.preferred, style, constraints)
}

fn apply_content_style(size: Size, style: Style, constraints: Constraints) -> Size {
    let mut width = preferred_axis(
        size.width,
        style.min.width,
        style.max.width,
        constraints.min.width,
        constraints.max.width,
    );
    let mut height = preferred_axis(
        size.height,
        style.min.height,
        style.max.height,
        constraints.min.height,
        constraints.max.height,
    );
    if let Some(ratio) = style.aspect_ratio {
        let by_width = width / ratio;
        let by_height = height * ratio;
        let height_min = style.min.height.max(constraints.min.height);
        let height_max = style.max.height.min(constraints.max.height);
        let width_min = style.min.width.max(constraints.min.width);
        let width_max = style.max.width.min(constraints.max.width);
        if by_width >= height_min - EPSILON && by_width <= height_max + EPSILON {
            height = by_width.clamp(height_min, height_max.max(height_min));
        } else if by_height >= width_min - EPSILON && by_height <= width_max + EPSILON {
            width = by_height.clamp(width_min, width_max.max(width_min));
        }
    }
    Size::new(width, height)
}

fn clamp_size(size: Size, constraints: Constraints) -> Size {
    Size::new(
        non_negative(size.width).clamp(constraints.min.width, constraints.max.width),
        non_negative(size.height).clamp(constraints.min.height, constraints.max.height),
    )
}

fn content_constraints(outer: Constraints, style: Style) -> Constraints {
    let horizontal = style.padding.horizontal();
    let vertical = style.padding.vertical();
    let min = Size::new(
        (outer.min.width - horizontal).max(0.0),
        (outer.min.height - vertical).max(0.0),
    );
    let max = Size::new(
        subtract_if_finite(outer.max.width, horizontal),
        subtract_if_finite(outer.max.height, vertical),
    );
    Constraints::new(min, max)
}

fn subtract_if_finite(value: f32, subtract: f32) -> f32 {
    if value.is_finite() {
        (value - subtract).max(0.0)
    } else {
        value
    }
}

fn outer_size(size: Size, margin: Edges) -> Size {
    Size::new(size.width + margin.horizontal(), size.height + margin.vertical())
}

fn gap_total(count: usize, gap: f32) -> f32 {
    let separators = count.saturating_sub(1);
    usize_to_f32(separators) * non_negative(gap)
}

fn usize_to_f32(value: usize) -> f32 {
    // Layout counts are bounded by MAX_NODES (1,000,000), exactly representable in f32.
    u32::try_from(value).map_or(f32::MAX, |small| small as f32)
}

fn sum_values(values: &[f32]) -> f32 {
    values.iter().copied().fold(0.0_f32, |sum, value| sum + value)
}

fn resolve_tracks(tracks: &[Track], auto: &[f32], available: f32, gap: f32) -> Result<Vec<f32>> {
    if tracks.len() != auto.len() {
        return Err(Error::Damaged("grid track/intrinsic size mismatch".to_owned()));
    }
    let mut sizes = Vec::with_capacity(tracks.len());
    let mut fixed_auto = 0.0_f32;
    let mut fraction_weight = 0.0_f32;
    for (position, track) in tracks.iter().copied().enumerate() {
        match track {
            Track::Fixed(value) => {
                let safe = non_negative(value);
                fixed_auto += safe;
                sizes.push(safe);
            }
            Track::Auto => {
                let safe = auto.get(position).copied().map_or(0.0, non_negative);
                fixed_auto += safe;
                sizes.push(safe);
            }
            Track::Fraction(weight) => {
                fraction_weight += fraction_weight_value(weight);
                sizes.push(0.0);
            }
        }
    }
    let usable = if available.is_finite() {
        (available - gap_total(tracks.len(), gap)).max(0.0)
    } else {
        fixed_auto
    };
    let remaining = (usable - fixed_auto).max(0.0);
    for (position, track) in tracks.iter().copied().enumerate() {
        if let Track::Fraction(weight) = track {
            let slot = sizes
                .get_mut(position)
                .ok_or_else(|| Error::Damaged("grid track slot missing".to_owned()))?;
            *slot = if available.is_finite() && fraction_weight > EPSILON {
                remaining * fraction_weight_value(weight) / fraction_weight
            } else {
                0.0
            };
        }
    }
    Ok(sizes)
}

fn fraction_weight_value(value: f32) -> f32 {
    if value.is_finite() && value > EPSILON { value } else { 1.0 }
}

fn inset_rect(rect: Rect, edges: Edges) -> Rect {
    let left = non_negative(edges.left).min(rect.width);
    let top = non_negative(edges.top).min(rect.height);
    let remaining_width = (rect.width - left).max(0.0);
    let remaining_height = (rect.height - top).max(0.0);
    let right = non_negative(edges.right).min(remaining_width);
    let bottom = non_negative(edges.bottom).min(remaining_height);
    Rect::new(
        rect.x + left,
        rect.y + top,
        (rect.width - left - right).max(0.0),
        (rect.height - top - bottom).max(0.0),
    )
}

fn snap_position(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

fn snap_boundary(value: f32, outer_start: f32, outer_end: f32, scale: f32) -> f32 {
    if (value - outer_start).abs() <= EPSILON {
        outer_start
    } else if (value - outer_end).abs() <= EPSILON {
        outer_end
    } else {
        snap_position(value, scale)
    }
}

fn snap_rect(rect: Rect, scale: f32) -> Rect {
    let left = snap_position(rect.x, scale);
    let top = snap_position(rect.y, scale);
    let right = snap_position(rect.right(), scale);
    let bottom = snap_position(rect.bottom(), scale);
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn snap_rect_within(rect: Rect, bounds: Rect, scale: f32) -> Rect {
    let left = snap_boundary(rect.x, bounds.x, bounds.right(), scale);
    let top = snap_boundary(rect.y, bounds.y, bounds.bottom(), scale);
    let right = snap_boundary(rect.right(), bounds.x, bounds.right(), scale);
    let bottom = snap_boundary(rect.bottom(), bounds.y, bounds.bottom(), scale);
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

fn align_in_cell(
    cell: Rect,
    measured: Size,
    style: Style,
    parent_align: Align,
    horizontal_main: bool,
) -> Rect {
    let align = style.align_self.unwrap_or(parent_align);
    if horizontal_main {
        let outer_height = aligned_outer_extent(
            measured.height,
            style.margin.top,
            style.margin.bottom,
            cell.height,
            align,
        );
        let y = aligned_position(cell.y, cell.height, outer_height, align);
        Rect::new(cell.x, y, cell.width, outer_height)
    } else {
        let outer_width = aligned_outer_extent(
            measured.width,
            style.margin.left,
            style.margin.right,
            cell.width,
            align,
        );
        let x = aligned_position(cell.x, cell.width, outer_width, align);
        Rect::new(x, cell.y, outer_width, cell.height)
    }
}

fn align_two_axes(rect: Rect, measured: Size, style: Style, horizontal: Align, vertical: Align) -> Rect {
    let outer_width = aligned_outer_extent(
        measured.width,
        style.margin.left,
        style.margin.right,
        rect.width,
        horizontal,
    );
    let outer_height = aligned_outer_extent(
        measured.height,
        style.margin.top,
        style.margin.bottom,
        rect.height,
        vertical,
    );
    let x = aligned_position(rect.x, rect.width, outer_width, horizontal);
    let y = aligned_position(rect.y, rect.height, outer_height, vertical);
    Rect::new(x, y, outer_width, outer_height)
}

fn aligned_outer_extent(
    measured: f32,
    margin_before: f32,
    margin_after: f32,
    available: f32,
    align: Align,
) -> f32 {
    if align == Align::Stretch {
        return non_negative(available);
    }
    let margins = non_negative(margin_before) + non_negative(margin_after);
    (non_negative(measured) + margins).min(non_negative(available))
}

fn aligned_position(origin: f32, available: f32, extent: f32, align: Align) -> f32 {
    match align {
        Align::Start | Align::Stretch => origin,
        Align::Center => origin + (available - extent) * 0.5,
        Align::End => origin + (available - extent),
    }
}

fn snapped_item_geometry(
    rect: Rect,
    items: &[LinearItem],
    gap: f32,
    horizontal: bool,
    scale: f32,
) -> Result<AxisGeometry> {
    let mut starts = Vec::with_capacity(items.len());
    let mut ends = Vec::with_capacity(items.len());
    let mut cursor = if horizontal { rect.x } else { rect.y };
    let outer_start = cursor;
    let outer_end = if horizontal { rect.right() } else { rect.bottom() };
    for (position, item) in items.iter().enumerate() {
        starts.push(snap_boundary(cursor, outer_start, outer_end, scale));
        cursor += item.main;
        ends.push(snap_boundary(cursor, outer_start, outer_end, scale));
        let next = position
            .checked_add(1)
            .ok_or_else(|| Error::Refused("boundary index overflow".to_owned()))?;
        if next < items.len() {
            cursor += gap;
        }
    }
    Ok(AxisGeometry { starts, ends })
}

fn track_geometry(
    origin: f32,
    extent: f32,
    sizes: &[f32],
    gap: f32,
    scale: f32,
) -> Result<AxisGeometry> {
    let mut starts = Vec::with_capacity(sizes.len());
    let mut ends = Vec::with_capacity(sizes.len());
    let mut cursor = origin;
    let outer_end = origin + non_negative(extent);
    for (position, size) in sizes.iter().copied().enumerate() {
        starts.push(snap_boundary(cursor, origin, outer_end, scale));
        cursor += size;
        ends.push(snap_boundary(cursor, origin, outer_end, scale));
        let next = position
            .checked_add(1)
            .ok_or_else(|| Error::Refused("track boundary index overflow".to_owned()))?;
        if next < sizes.len() {
            cursor += non_negative(gap);
        }
    }
    Ok(AxisGeometry { starts, ends })
}

#[derive(Debug)]
struct AxisGeometry {
    starts: Vec<f32>,
    ends: Vec<f32>,
}

#[cfg(test)]
mod tests {
    use super::{
        Align, Constraints, Edges, GridPlacement, Layout, NodeKind, Rect, Size, Style, Track,
    };
    use sse_core::Error;

    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() <= 0.001,
            "expected {expected}, got {actual}"
        );
    }

    fn rect_close(actual: Rect, expected: Rect) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
        close(actual.width, expected.width);
        close(actual.height, expected.height);
    }

    fn fixed(width: f32, height: f32) -> Style {
        Style {
            min: Size::new(width, height),
            preferred: Size::new(width, height),
            max: Size::new(width, height),
            ..Style::default()
        }
    }

    fn measure_text(value: &str, width: f32) -> Size {
        let glyph_width = 8.0_f32;
        let line_height = 16.0_f32;
        let chars = u32::try_from(value.chars().count()).map_or(0_u32, |count| count);
        let natural = (chars as f32) * glyph_width;
        if !width.is_finite() || width <= 0.0 {
            return Size::new(natural, line_height);
        }
        let lines = (natural / width.max(glyph_width)).ceil().max(1.0);
        Size::new(natural.min(width), lines * line_height)
    }

    #[test]
    fn row_grow_and_shrink_are_predictable() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(NodeKind::Row, Style::default())?;
        let mut flex = fixed(100.0, 20.0);
        flex.max.width = f32::INFINITY;
        flex.grow = 1.0;
        flex.shrink = 1.0;
        let a = layout.add(NodeKind::Leaf, flex)?;
        let b = layout.add(NodeKind::Leaf, flex)?;
        layout.append_child(root, a)?;
        layout.append_child(root, b)?;
        layout.arrange(root, Rect::new(0.0, 0.0, 300.0, 20.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(a)?, Rect::new(0.0, 0.0, 150.0, 20.0));
        rect_close(layout.rect(b)?, Rect::new(150.0, 0.0, 150.0, 20.0));
        layout.arrange(root, Rect::new(0.0, 0.0, 120.0, 20.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(a)?, Rect::new(0.0, 0.0, 60.0, 20.0));
        rect_close(layout.rect(b)?, Rect::new(60.0, 0.0, 60.0, 20.0));
        Ok(())
    }

    #[test]
    fn column_margin_padding_gap_and_alignment() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root_style = Style {
            padding: Edges::all(10.0),
            gap: Size::new(0.0, 5.0),
            align_items: Align::Center,
            ..Style::default()
        };
        let root = layout.add(NodeKind::Column, root_style)?;
        let mut child_style = fixed(20.0, 10.0);
        child_style.margin = Edges::all(2.0);
        let child = layout.add(NodeKind::Leaf, child_style)?;
        layout.append_child(root, child)?;
        layout.arrange(root, Rect::new(0.0, 0.0, 100.0, 50.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(child)?, Rect::new(40.0, 12.0, 20.0, 10.0));
        Ok(())
    }

    #[test]
    fn wrap_creates_expected_lines() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(
            NodeKind::Wrap,
            Style {
                gap: Size::new(5.0, 3.0),
                ..Style::default()
            },
        )?;
        let mut ids = Vec::new();
        for _ in 0..4 {
            let child = layout.add(NodeKind::Leaf, fixed(40.0, 10.0))?;
            layout.append_child(root, child)?;
            ids.push(child);
        }
        layout.arrange(root, Rect::new(0.0, 0.0, 90.0, 30.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(*ids.first().ok_or_else(|| Error::Damaged("test id".to_owned()))?)?, Rect::new(0.0, 0.0, 40.0, 10.0));
        rect_close(layout.rect(*ids.get(1).ok_or_else(|| Error::Damaged("test id".to_owned()))?)?, Rect::new(45.0, 0.0, 40.0, 10.0));
        rect_close(layout.rect(*ids.get(2).ok_or_else(|| Error::Damaged("test id".to_owned()))?)?, Rect::new(0.0, 13.0, 40.0, 10.0));
        rect_close(layout.rect(*ids.get(3).ok_or_else(|| Error::Damaged("test id".to_owned()))?)?, Rect::new(45.0, 13.0, 40.0, 10.0));
        Ok(())
    }

    #[test]
    fn grid_fixed_auto_fraction_tracks() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(
            NodeKind::Grid {
                columns: vec![Track::Fixed(20.0), Track::Auto, Track::Fraction(1.0)],
                rows: vec![Track::Fixed(20.0), Track::Fraction(1.0)],
            },
            Style {
                gap: Size::new(5.0, 5.0),
                align_items: Align::Stretch,
                ..Style::default()
            },
        )?;
        let a = layout.add(NodeKind::Leaf, fixed(10.0, 10.0))?;
        let b = layout.add(NodeKind::Leaf, fixed(30.0, 10.0))?;
        let c = layout.add(NodeKind::Leaf, fixed(10.0, 10.0))?;
        let d = layout.add(NodeKind::Leaf, fixed(10.0, 10.0))?;
        for child in [a, b, c, d] {
            layout.append_child(root, child)?;
        }
        layout.arrange(root, Rect::new(0.0, 0.0, 100.0, 60.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(a)?, Rect::new(0.0, 0.0, 20.0, 20.0));
        rect_close(layout.rect(b)?, Rect::new(25.0, 0.0, 30.0, 20.0));
        rect_close(layout.rect(c)?, Rect::new(60.0, 0.0, 40.0, 20.0));
        rect_close(layout.rect(d)?, Rect::new(0.0, 25.0, 20.0, 35.0));
        Ok(())
    }

    #[test]
    fn explicit_grid_span_uses_shared_boundaries() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(
            NodeKind::Grid {
                columns: vec![Track::Fraction(1.0), Track::Fraction(1.0), Track::Fraction(1.0)],
                rows: vec![Track::Fixed(20.0)],
            },
            Style {
                gap: Size::new(2.0, 0.0),
                align_items: Align::Stretch,
                ..Style::default()
            },
        )?;
        let style = Style {
            grid: Some(GridPlacement {
                column: 1,
                row: 0,
                column_span: 2,
                row_span: 1,
            }),
            ..Style::default()
        };
        let child = layout.add(NodeKind::Leaf, style)?;
        layout.append_child(root, child)?;
        layout.arrange(root, Rect::new(0.0, 0.0, 100.0, 20.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(child)?, Rect::new(34.0, 0.0, 66.0, 20.0));
        Ok(())
    }

    #[test]
    fn stack_alignment_and_aspect_ratio() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(NodeKind::Stack, Style { align_items: Align::Center, ..Style::default() })?;
        let child = layout.add(
            NodeKind::Leaf,
            Style {
                preferred: Size::new(40.0, 10.0),
                max: Size::new(100.0, 100.0),
                aspect_ratio: Some(2.0),
                ..Style::default()
            },
        )?;
        layout.append_child(root, child)?;
        layout.arrange(root, Rect::new(0.0, 0.0, 100.0, 80.0), 1.0, &mut measure_text)?;
        rect_close(layout.rect(child)?, Rect::new(30.0, 30.0, 40.0, 20.0));
        Ok(())
    }

    #[test]
    fn text_measurement_is_cached_by_constraint() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(NodeKind::Column, Style::default())?;
        let text_node = layout.add(NodeKind::Text("cached".to_owned()), Style::default())?;
        layout.append_child(root, text_node)?;
        let constraints = Constraints::new(Size::default(), Size::new(100.0, 100.0));
        let mut calls = 0_u32;
        let mut callback = |value: &str, width: f32| {
            calls = calls.saturating_add(1);
            measure_text(value, width)
        };
        let _ = layout.measure(root, constraints, &mut callback)?;
        let first_calls = calls;
        layout.reset_counters();
        let _ = layout.measure(root, constraints, &mut callback)?;
        assert_eq!(calls, first_calls);
        assert_eq!(layout.measure_visits(), 0);
        assert_eq!(layout.cache_hits(), 1);
        Ok(())
    }

    #[test]
    fn child_mutation_invalidates_ancestors_only() -> Result<(), Error> {
        let mut layout = Layout::new();
        let root = layout.add(NodeKind::Row, Style::default())?;
        let left = layout.add(NodeKind::Column, Style::default())?;
        let right = layout.add(NodeKind::Leaf, fixed(10.0, 10.0))?;
        let inner = layout.add(NodeKind::Leaf, fixed(10.0, 10.0))?;
        layout.append_child(root, left)?;
        layout.append_child(root, right)?;
        layout.append_child(left, inner)?;
        let constraints = Constraints::new(Size::default(), Size::new(100.0, 100.0));
        let _ = layout.measure(root, constraints, &mut measure_text)?;
        layout.set_style(inner, fixed(20.0, 10.0))?;
        layout.reset_counters();
        let _ = layout.measure(root, constraints, &mut measure_text)?;
        assert_eq!(layout.measure_visits(), 3);
        assert!(layout.cache_hits() >= 1);
        Ok(())
    }

    #[test]
    fn scroll_reports_unclipped_content_size() -> Result<(), Error> {
        let mut layout = Layout::new();
        let scroll = layout.add(
            NodeKind::Scroll {
                horizontal: false,
                vertical: true,
                offset_x: 0.0,
                offset_y: 30.0,
            },
            Style::default(),
        )?;
        let child = layout.add(NodeKind::Leaf, fixed(80.0, 200.0))?;
        layout.append_child(scroll, child)?;
        layout.arrange(scroll, Rect::new(0.0, 0.0, 80.0, 50.0), 1.0, &mut measure_text)?;
        let content = layout.content_size(scroll)?;
        close(content.width, 80.0);
        close(content.height, 200.0);
        rect_close(layout.rect(child)?, Rect::new(0.0, -30.0, 80.0, 200.0));
        Ok(())
    }

    #[test]
    fn fractional_scales_keep_adjacent_cells_touching() -> Result<(), Error> {
        for scale in [1.0_f32, 1.25, 1.5, 2.0] {
            let mut layout = Layout::new();
            let root = layout.add(NodeKind::Row, Style::default())?;
            let mut ids = Vec::new();
            for _ in 0..7 {
                let child = layout.add(
                    NodeKind::Leaf,
                    Style {
                        grow: 1.0,
                        max: Size::new(f32::INFINITY, f32::INFINITY),
                        ..Style::default()
                    },
                )?;
                layout.append_child(root, child)?;
                ids.push(child);
            }
            layout.arrange(root, Rect::new(0.0, 0.0, 101.0, 20.0), scale, &mut measure_text)?;
            for pair in ids.windows(2) {
                let left = layout.rect(*pair.first().ok_or_else(|| Error::Damaged("test pair".to_owned()))?)?;
                let right = layout.rect(*pair.get(1).ok_or_else(|| Error::Damaged("test pair".to_owned()))?)?;
                close(left.x + left.width, right.x);
            }
            let last = layout.rect(*ids.last().ok_or_else(|| Error::Damaged("test id".to_owned()))?)?;
            close(last.x + last.width, 101.0);
        }
        Ok(())
    }

    #[test]
    fn required_window_width_and_scale_matrix_has_expected_rectangles() -> Result<(), Error> {
        let widths = [940.0_f32, 1260.0, 1920.0];
        let scales = [1.0_f32, 1.25, 1.5, 2.0];
        for width in widths {
            for scale in scales {
                let mut layout = Layout::new();
                let root = layout.add(NodeKind::Row, Style::default())?;
                let sidebar = layout.add(NodeKind::Leaf, fixed(240.0, 760.0))?;
                let content = layout.add(
                    NodeKind::Leaf,
                    Style {
                        min: Size::new(0.0, 760.0),
                        preferred: Size::new(0.0, 760.0),
                        max: Size::new(f32::INFINITY, 760.0),
                        grow: 1.0,
                        ..Style::default()
                    },
                )?;
                layout.append_child(root, sidebar)?;
                layout.append_child(root, content)?;
                layout.arrange(root, Rect::new(0.0, 0.0, width, 760.0), scale, &mut measure_text)?;
                rect_close(layout.rect(sidebar)?, Rect::new(0.0, 0.0, 240.0, 760.0));
                rect_close(layout.rect(content)?, Rect::new(240.0, 0.0, width - 240.0, 760.0));
            }
        }
        Ok(())
    }

    #[test]
    fn eighty_table_layouts_match_expected_rectangles() -> Result<(), Error> {
        // 80 deterministic row layouts: each case has an independently calculable exact expected split.
        for case in 0_u32..80_u32 {
            let width = 200.0 + (case as f32) * 3.0;
            let left_remainder = case.checked_rem(9).map_or(0_u32, |value| value);
            let gap_remainder = case.checked_rem(5).map_or(0_u32, |value| value);
            let left_width = 20.0 + (left_remainder as f32);
            let gap = gap_remainder as f32;
            let mut layout = Layout::new();
            let root = layout.add(
                NodeKind::Row,
                Style {
                    gap: Size::new(gap, 0.0),
                    ..Style::default()
                },
            )?;
            let left = layout.add(NodeKind::Leaf, fixed(left_width, 24.0))?;
            let right = layout.add(
                NodeKind::Leaf,
                Style {
                    min: Size::new(0.0, 24.0),
                    preferred: Size::new(0.0, 24.0),
                    max: Size::new(f32::INFINITY, 24.0),
                    grow: 1.0,
                    ..Style::default()
                },
            )?;
            layout.append_child(root, left)?;
            layout.append_child(root, right)?;
            layout.arrange(root, Rect::new(0.0, 0.0, width, 24.0), 1.0, &mut measure_text)?;
            rect_close(layout.rect(left)?, Rect::new(0.0, 0.0, left_width, 24.0));
            rect_close(
                layout.rect(right)?,
                Rect::new(left_width + gap, 0.0, width - left_width - gap, 24.0),
            );
        }
        Ok(())
    }

    #[test]
    fn malformed_tree_requests_are_refused() -> Result<(), Error> {
        let mut layout = Layout::new();
        let a = layout.add(NodeKind::Row, Style::default())?;
        let b = layout.add(NodeKind::Row, Style::default())?;
        layout.append_child(a, b)?;
        assert!(matches!(layout.append_child(b, a), Err(Error::Refused(_))));
        assert!(matches!(layout.append_child(a, b), Err(Error::Refused(_))));
        let empty_grid = layout.add(NodeKind::Grid { columns: Vec::new(), rows: Vec::new() }, Style::default())?;
        assert!(matches!(layout.measure(empty_grid, Constraints::loose(), &mut measure_text), Err(Error::Refused(_))));
        Ok(())
    }
}
