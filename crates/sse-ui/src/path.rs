//! SVG path parsing, flattening, transforms, stroke expansion and built-in vector icons.
//!
//! The parser accepts SVG path-data commands `M L H V C S Q T A Z` in both
//! absolute and relative forms. Quadratic curves and elliptical arcs are
//! normalised to cubic Béziers while parsing, so the flattening hot path only
//! handles lines and cubics.

use sse_core::{Error, Result};
use std::f64::consts::{FRAC_PI_2, PI};

const MAX_COMMANDS: usize = 1_000_000;
const MAX_SEGMENTS: usize = 2_000_000;
const MAX_FLATTEN_DEPTH: u8 = 24;
const EPSILON: f64 = 1.0e-12;

/// A two-dimensional point in SVG user-space coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// Creates a point.
    #[must_use]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn finite(self) -> Result<Self> {
        if self.x.is_finite() && self.y.is_finite() {
            Ok(self)
        } else {
            Err(Error::damaged("non-finite path coordinate"))
        }
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y)
    }

    fn scale(self, value: f64) -> Self {
        Self::new(self.x * value, self.y * value)
    }

    fn dot(self, other: Self) -> f64 {
        self.x.mul_add(other.x, self.y * other.y)
    }

    fn cross(self, other: Self) -> f64 {
        self.x.mul_add(other.y, -(self.y * other.x))
    }

    fn length(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn normalised(self) -> Option<Self> {
        let length = self.length();
        if length <= EPSILON || !length.is_finite() {
            None
        } else {
            Some(self.scale(1.0 / length))
        }
    }

    fn left_normal(self) -> Self {
        Self::new(-self.y, self.x)
    }

    fn distance(self, other: Self) -> f64 {
        self.sub(other).length()
    }
}

/// A line segment consumed by the coverage rasteriser.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// Segment start.
    pub from: Point,
    /// Segment end.
    pub to: Point,
}

/// Fill rule used when rasterising path edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FillRule {
    /// Alternate inside/outside at every edge crossing.
    EvenOdd,
    /// Use signed winding count; any non-zero winding is inside.
    NonZero,
}

/// A 2D affine transform.
///
/// Coordinates are transformed as:
/// `x' = a*x + c*y + e`, `y' = b*x + d*y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    /// Matrix element a.
    pub a: f64,
    /// Matrix element b.
    pub b: f64,
    /// Matrix element c.
    pub c: f64,
    /// Matrix element d.
    pub d: f64,
    /// Translation x.
    pub e: f64,
    /// Translation y.
    pub f: f64,
}

impl Transform {
    /// Identity transform.
    #[must_use]
    pub const fn identity() -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 }
    }

    /// Translation.
    #[must_use]
    pub const fn translate(x: f64, y: f64) -> Self {
        Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y }
    }

    /// Non-uniform scale.
    #[must_use]
    pub const fn scale(x: f64, y: f64) -> Self {
        Self { a: x, b: 0.0, c: 0.0, d: y, e: 0.0, f: 0.0 }
    }

    /// Counter-clockwise rotation in radians.
    #[must_use]
    pub fn rotate(radians: f64) -> Self {
        let (sine, cosine) = radians.sin_cos();
        Self { a: cosine, b: sine, c: -sine, d: cosine, e: 0.0, f: 0.0 }
    }

    /// Applies the transform to one point.
    #[must_use]
    pub fn apply(self, point: Point) -> Point {
        Point::new(
            self.a.mul_add(point.x, self.c.mul_add(point.y, self.e)),
            self.b.mul_add(point.x, self.d.mul_add(point.y, self.f)),
        )
    }

    /// Returns a transform that applies `self` and then `next`.
    #[must_use]
    pub fn then(self, next: Self) -> Self {
        Self {
            a: next.a.mul_add(self.a, next.c * self.b),
            b: next.b.mul_add(self.a, next.d * self.b),
            c: next.a.mul_add(self.c, next.c * self.d),
            d: next.b.mul_add(self.c, next.d * self.d),
            e: next.a.mul_add(self.e, next.c.mul_add(self.f, next.e)),
            f: next.b.mul_add(self.e, next.d.mul_add(self.f, next.f)),
        }
    }

    fn finite(self) -> Result<Self> {
        if self.a.is_finite() && self.b.is_finite() && self.c.is_finite() && self.d.is_finite()
            && self.e.is_finite() && self.f.is_finite()
        {
            Ok(self)
        } else {
            Err(Error::damaged("non-finite affine transform"))
        }
    }
}

/// Line cap used by stroke expansion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineCap {
    /// End exactly at the path endpoint.
    Butt,
    /// Extend by half the stroke width.
    Square,
    /// Circular cap.
    Round,
}

/// Line join used by stroke expansion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LineJoin {
    /// Intersect the outside offset edges up to the miter limit.
    Miter,
    /// Connect outside offset corners directly.
    Bevel,
    /// Circular join.
    Round,
}

/// Parameters for converting a stroked centerline to filled edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeStyle {
    /// Stroke width in user-space units.
    pub width: f64,
    /// Open-subpath cap.
    pub cap: LineCap,
    /// Corner join.
    pub join: LineJoin,
    /// Maximum miter length divided by half-width.
    pub miter_limit: f64,
}

impl StrokeStyle {
    /// The common interface-icon style: two-unit round stroke.
    #[must_use]
    pub const fn icon() -> Self {
        Self { width: 2.0, cap: LineCap::Round, join: LineJoin::Round, miter_limit: 4.0 }
    }
}

#[derive(Clone, Copy, Debug)]
enum Command {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}

/// Parsed SVG path.
///
/// Curves are kept until flattening; arcs and quadratics are represented as
/// cubic Béziers to keep one curve path through transforms and flattening.
#[derive(Clone, Debug, Default)]
pub struct Path {
    commands: Vec<Command>,
}

impl Path {
    /// Parses SVG path data.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed syntax, non-finite numbers,
    /// invalid arc flags or excessive input.
    pub fn parse(data: &str) -> Result<Self> {
        Parser::new(data).parse()
    }

    /// Returns the number of normalised path commands.
    #[must_use]
    pub fn command_count(&self) -> usize {
        self.commands.len()
    }

    /// Returns a transformed copy without flattening curves.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for a non-finite transform.
    pub fn transformed(&self, transform: Transform) -> Result<Self> {
        let transform = transform.finite()?;
        let mut commands = Vec::with_capacity(self.commands.len());
        for command in &self.commands {
            let mapped = match *command {
                Command::Move(point) => Command::Move(transform.apply(point)),
                Command::Line(point) => Command::Line(transform.apply(point)),
                Command::Cubic(a, b, c) => Command::Cubic(
                    transform.apply(a),
                    transform.apply(b),
                    transform.apply(c),
                ),
                Command::Close => Command::Close,
            };
            commands.push(mapped);
        }
        Ok(Self { commands })
    }

    /// Flattens the path to line segments.
    ///
    /// `tolerance` is the maximum geometric deviation in transformed output
    /// coordinates. The returned fill rule is kept alongside the edges.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for non-positive tolerance, non-finite
    /// geometry or an expansion above the hard segment limit.
    pub fn flatten(
        &self,
        transform: Transform,
        tolerance: f64,
        fill_rule: FillRule,
    ) -> Result<FlattenedPath> {
        let polylines = self.flatten_subpaths(transform, tolerance)?;
        let mut segments = Vec::new();
        for polyline in polylines {
            append_polyline_segments(&polyline, &mut segments)?;
        }
        Ok(FlattenedPath { segments, fill_rule })
    }

    /// Expands the path stroke to filled line-segment primitives.
    ///
    /// The expansion is the union of segment rectangles plus explicit join
    /// and cap primitives. All primitives use the same counter-clockwise
    /// winding, so non-zero filling performs the union without boolean path
    /// operations or a second geometry allocation.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for invalid style/tolerance/transform or
    /// excessive expanded geometry.
    pub fn stroke_to_fill(
        &self,
        transform: Transform,
        tolerance: f64,
        style: StrokeStyle,
    ) -> Result<FlattenedPath> {
        validate_tolerance(tolerance)?;
        if !style.width.is_finite() || style.width <= 0.0 {
            return Err(Error::damaged("stroke width must be finite and positive"));
        }
        if !style.miter_limit.is_finite() || style.miter_limit < 1.0 {
            return Err(Error::damaged("miter limit must be finite and at least one"));
        }

        let half = style.width * 0.5;
        let polylines = self.flatten_subpaths(transform, tolerance)?;
        let mut segments = Vec::new();
        for polyline in polylines {
            stroke_polyline(&polyline, half, tolerance, style, &mut segments)?;
        }
        Ok(FlattenedPath { segments, fill_rule: FillRule::NonZero })
    }

    fn flatten_subpaths(&self, transform: Transform, tolerance: f64) -> Result<Vec<Polyline>> {
        validate_tolerance(tolerance)?;
        let transform = transform.finite()?;
        let mut result = Vec::new();
        let mut points = Vec::new();
        let mut current = Point::default();
        let mut start = Point::default();
        let mut have_current = false;

        for command in &self.commands {
            match *command {
                Command::Move(point) => {
                    finish_polyline(&mut result, &mut points, false)?;
                    current = transform.apply(point).finite()?;
                    start = current;
                    points.push(current);
                    have_current = true;
                }
                Command::Line(point) => {
                    if !have_current {
                        return Err(Error::damaged("line before move"));
                    }
                    current = transform.apply(point).finite()?;
                    push_distinct(&mut points, current)?;
                }
                Command::Cubic(c1, c2, end) => {
                    if !have_current {
                        return Err(Error::damaged("curve before move"));
                    }
                    let cubic = Cubic {
                        p0: current,
                        p1: transform.apply(c1).finite()?,
                        p2: transform.apply(c2).finite()?,
                        p3: transform.apply(end).finite()?,
                        depth: 0,
                    };
                    flatten_cubic(cubic, tolerance, &mut points)?;
                    current = cubic.p3;
                }
                Command::Close => {
                    if !have_current {
                        return Err(Error::damaged("close before move"));
                    }
                    finish_polyline(&mut result, &mut points, true)?;
                    current = start;
                    points.push(current);
                    have_current = true;
                }
            }
        }
        finish_polyline(&mut result, &mut points, false)?;
        Ok(result)
    }
}

/// Flattened line geometry plus the rule with which those edges are filled.
#[derive(Clone, Debug, PartialEq)]
pub struct FlattenedPath {
    /// Line segments.
    pub segments: Vec<Segment>,
    /// Fill rule for the segments.
    pub fill_rule: FillRule,
}

impl FlattenedPath {
    /// Tests a point with the stored fill rule.
    ///
    /// This is mainly useful for tests and software hit-testing; the coverage
    /// rasteriser can consume [`Self::segments`] directly.
    #[must_use]
    pub fn contains(&self, point: Point) -> bool {
        let mut winding = 0_i32;
        let mut parity = false;
        for segment in &self.segments {
            let from = segment.from;
            let to = segment.to;
            let crosses = (from.y <= point.y && to.y > point.y)
                || (to.y <= point.y && from.y > point.y);
            if !crosses {
                continue;
            }
            let dy = to.y - from.y;
            if dy.abs() <= EPSILON {
                continue;
            }
            let t = (point.y - from.y) / dy;
            let x = (to.x - from.x).mul_add(t, from.x);
            if x <= point.x {
                continue;
            }
            parity = !parity;
            let side = to.sub(from).cross(point.sub(from));
            if to.y > from.y && side > 0.0 {
                winding = winding.saturating_add(1);
            } else if to.y < from.y && side < 0.0 {
                winding = winding.saturating_sub(1);
            }
        }
        match self.fill_rule {
            FillRule::EvenOdd => parity,
            FillRule::NonZero => winding != 0,
        }
    }
}

#[derive(Clone, Debug)]
struct Polyline {
    points: Vec<Point>,
    closed: bool,
}

#[derive(Clone, Copy, Debug)]
struct Cubic {
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    depth: u8,
}

fn validate_tolerance(tolerance: f64) -> Result<()> {
    if tolerance.is_finite() && tolerance > 0.0 {
        Ok(())
    } else {
        Err(Error::damaged("flatten tolerance must be finite and positive"))
    }
}

fn finish_polyline(result: &mut Vec<Polyline>, points: &mut Vec<Point>, closed: bool) -> Result<()> {
    if points.len() >= 2 {
        if result.len() >= MAX_COMMANDS {
            return Err(Error::Refused("too many flattened subpaths".to_owned()));
        }
        result.push(Polyline { points: std::mem::take(points), closed });
    } else {
        points.clear();
    }
    Ok(())
}

fn push_distinct(points: &mut Vec<Point>, point: Point) -> Result<()> {
    if points.len() >= MAX_SEGMENTS {
        return Err(Error::Refused("flattened point limit exceeded".to_owned()));
    }
    if points.last().is_none_or(|last| last.distance(point) > EPSILON) {
        points.push(point);
    }
    Ok(())
}

fn append_segment(out: &mut Vec<Segment>, from: Point, to: Point) -> Result<()> {
    if out.len() >= MAX_SEGMENTS {
        return Err(Error::Refused("segment limit exceeded".to_owned()));
    }
    if from.distance(to) > EPSILON {
        out.push(Segment { from, to });
    }
    Ok(())
}

fn append_polyline_segments(polyline: &Polyline, out: &mut Vec<Segment>) -> Result<()> {
    let mut iter = polyline.points.iter().copied();
    let Some(first) = iter.next() else {
        return Ok(());
    };
    let mut previous = first;
    for point in iter {
        append_segment(out, previous, point)?;
        previous = point;
    }
    if polyline.closed {
        append_segment(out, previous, first)?;
    }
    Ok(())
}

fn flatten_cubic(cubic: Cubic, tolerance: f64, points: &mut Vec<Point>) -> Result<()> {
    let mut stack = Vec::with_capacity(16);
    stack.push(cubic);
    while let Some(curve) = stack.pop() {
        if cubic_flat_enough(curve, tolerance) || curve.depth >= MAX_FLATTEN_DEPTH {
            push_distinct(points, curve.p3)?;
            continue;
        }
        let next_depth = curve.depth.checked_add(1).ok_or_else(|| Error::damaged("curve depth overflow"))?;
        let p01 = midpoint(curve.p0, curve.p1);
        let p12 = midpoint(curve.p1, curve.p2);
        let p23 = midpoint(curve.p2, curve.p3);
        let p012 = midpoint(p01, p12);
        let p123 = midpoint(p12, p23);
        let p0123 = midpoint(p012, p123);
        if stack.len() >= MAX_SEGMENTS {
            return Err(Error::Refused("cubic subdivision limit exceeded".to_owned()));
        }
        stack.push(Cubic { p0: p0123, p1: p123, p2: p23, p3: curve.p3, depth: next_depth });
        stack.push(Cubic { p0: curve.p0, p1: p01, p2: p012, p3: p0123, depth: next_depth });
    }
    Ok(())
}

fn midpoint(a: Point, b: Point) -> Point {
    Point::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

fn cubic_flat_enough(cubic: Cubic, tolerance: f64) -> bool {
    let chord = cubic.p3.sub(cubic.p0);
    let length = chord.length();
    if length <= EPSILON {
        return cubic.p1.distance(cubic.p0).max(cubic.p2.distance(cubic.p0)) <= tolerance;
    }
    let d1 = chord.cross(cubic.p1.sub(cubic.p0)).abs() / length;
    let d2 = chord.cross(cubic.p2.sub(cubic.p0)).abs() / length;
    d1.max(d2) <= tolerance
}

fn stroke_polyline(
    polyline: &Polyline,
    half: f64,
    tolerance: f64,
    style: StrokeStyle,
    out: &mut Vec<Segment>,
) -> Result<()> {
    let points = deduplicate_points(&polyline.points);
    if points.len() < 2 {
        return Ok(());
    }

    let segment_count = if polyline.closed { points.len() } else { points.len().saturating_sub(1) };
    for index in 0..segment_count {
        let Some((from, to)) = segment_points(&points, index, polyline.closed) else {
            continue;
        };
        if let Some(tangent) = to.sub(from).normalised() {
            append_segment_rectangle(from, to, tangent, half, out)?;
        }
    }

    let join_count = if polyline.closed { points.len() } else { points.len().saturating_sub(2) };
    let join_start = if polyline.closed { 0 } else { 1 };
    for offset in 0..join_count {
        let index = join_start.checked_add(offset).ok_or_else(|| Error::damaged("join index overflow"))?;
        append_join(&points, index, polyline.closed, half, tolerance, style, out)?;
    }

    if !polyline.closed {
        let first = points.first().copied().ok_or_else(|| Error::damaged("stroke start missing"))?;
        let second = points.get(1).copied().ok_or_else(|| Error::damaged("stroke second point missing"))?;
        let last = points.last().copied().ok_or_else(|| Error::damaged("stroke end missing"))?;
        let before_last_index = points.len().checked_sub(2).ok_or_else(|| Error::damaged("stroke endpoint index underflow"))?;
        let before_last = points.get(before_last_index).copied().ok_or_else(|| Error::damaged("stroke penultimate point missing"))?;
        let start_tangent = second.sub(first).normalised().ok_or_else(|| Error::damaged("zero-length stroke start"))?;
        let end_tangent = last.sub(before_last).normalised().ok_or_else(|| Error::damaged("zero-length stroke end"))?;
        append_cap(first, start_tangent.scale(-1.0), half, tolerance, style.cap, out)?;
        append_cap(last, end_tangent, half, tolerance, style.cap, out)?;
    }
    Ok(())
}

fn deduplicate_points(points: &[Point]) -> Vec<Point> {
    let mut result = Vec::with_capacity(points.len());
    for point in points {
        if result.last().is_none_or(|last: &Point| last.distance(*point) > EPSILON) {
            result.push(*point);
        }
    }
    result
}

fn segment_points(points: &[Point], index: usize, closed: bool) -> Option<(Point, Point)> {
    let from = points.get(index).copied()?;
    let next = index.checked_add(1)?;
    let to = if next < points.len() {
        points.get(next).copied()?
    } else if closed {
        points.first().copied()?
    } else {
        return None;
    };
    Some((from, to))
}

fn append_segment_rectangle(from: Point, to: Point, tangent: Point, half: f64, out: &mut Vec<Segment>) -> Result<()> {
    let normal = tangent.left_normal().scale(half);
    let a = from.sub(normal);
    let b = to.sub(normal);
    let c = to.add(normal);
    let d = from.add(normal);
    append_polygon(out, &[a, b, c, d])
}

fn append_cap(
    center: Point,
    outward: Point,
    half: f64,
    tolerance: f64,
    cap: LineCap,
    out: &mut Vec<Segment>,
) -> Result<()> {
    match cap {
        LineCap::Butt => Ok(()),
        LineCap::Round => append_circle(center, half, tolerance, out),
        LineCap::Square => {
            let normal = outward.left_normal().scale(half);
            let edge = center.add(outward.scale(half));
            let a = center.sub(normal);
            let b = edge.sub(normal);
            let c = edge.add(normal);
            let d = center.add(normal);
            append_polygon(out, &[a, b, c, d])
        }
    }
}

fn append_join(
    points: &[Point],
    index: usize,
    closed: bool,
    half: f64,
    tolerance: f64,
    style: StrokeStyle,
    out: &mut Vec<Segment>,
) -> Result<()> {
    let current = points.get(index).copied().ok_or_else(|| Error::damaged("join point missing"))?;
    let previous = if index == 0 {
        if closed { points.last().copied() } else { None }
    } else {
        points.get(index.saturating_sub(1)).copied()
    }
    .ok_or_else(|| Error::damaged("join previous point missing"))?;

    let next_index = index.checked_add(1).ok_or_else(|| Error::damaged("join next index overflow"))?;
    let next = if next_index < points.len() {
        points.get(next_index).copied()
    } else if closed {
        points.first().copied()
    } else {
        None
    }
    .ok_or_else(|| Error::damaged("join next point missing"))?;

    let incoming = current.sub(previous).normalised().ok_or_else(|| Error::damaged("zero-length incoming join segment"))?;
    let outgoing = next.sub(current).normalised().ok_or_else(|| Error::damaged("zero-length outgoing join segment"))?;
    let turn = incoming.cross(outgoing);
    if turn.abs() <= EPSILON {
        return Ok(());
    }
    if style.join == LineJoin::Round {
        return append_circle(current, half, tolerance, out);
    }

    let outer_sign = if turn > 0.0 { -1.0 } else { 1.0 };
    let previous_outer = current.add(incoming.left_normal().scale(half * outer_sign));
    let next_outer = current.add(outgoing.left_normal().scale(half * outer_sign));
    if style.join == LineJoin::Miter {
        if let Some(miter) = line_intersection(previous_outer, incoming, next_outer, outgoing) {
            let ratio = miter.distance(current) / half;
            if ratio.is_finite() && ratio <= style.miter_limit {
                return append_polygon(out, &[current, previous_outer, miter, next_outer]);
            }
        }
    }
    append_polygon(out, &[current, previous_outer, next_outer])
}

fn line_intersection(a: Point, ad: Point, b: Point, bd: Point) -> Option<Point> {
    let denominator = ad.cross(bd);
    if denominator.abs() <= EPSILON {
        return None;
    }
    let t = b.sub(a).cross(bd) / denominator;
    Some(a.add(ad.scale(t)))
}

fn append_circle(center: Point, radius: f64, tolerance: f64, out: &mut Vec<Segment>) -> Result<()> {
    if radius <= EPSILON {
        return Ok(());
    }
    let ratio = (1.0 - tolerance.min(radius) / radius).clamp(-1.0, 1.0);
    let angle = (2.0 * ratio.acos()).clamp(PI / 64.0, FRAC_PI_2);
    let target = (2.0 * PI / angle).ceil().clamp(4.0, 256.0);
    let mut steps = 4_usize;
    while f64::from(u32::try_from(steps).map_err(|_| Error::damaged("circle step conversion failed"))?) < target {
        steps = steps
            .checked_add(1)
            .ok_or_else(|| Error::damaged("circle subdivision count overflow"))?;
    }
    let steps_u32 = u32::try_from(steps).map_err(|_| Error::damaged("circle step conversion failed"))?;
    let mut previous = Point::new(center.x + radius, center.y);
    for index in 1..=steps {
        let index_u32 = u32::try_from(index).map_err(|_| Error::damaged("circle index conversion failed"))?;
        let theta = 2.0 * PI * f64::from(index_u32) / f64::from(steps_u32);
        let (sine, cosine) = theta.sin_cos();
        let point = Point::new(radius.mul_add(cosine, center.x), radius.mul_add(sine, center.y));
        append_segment(out, previous, point)?;
        previous = point;
    }
    Ok(())
}

fn append_polygon(out: &mut Vec<Segment>, points: &[Point]) -> Result<()> {
    if points.len() < 3 {
        return Ok(());
    }
    let area = signed_area(points);
    if area.abs() <= EPSILON {
        return Ok(());
    }
    if area > 0.0 {
        append_polygon_order(out, points.iter().copied())
    } else {
        append_polygon_order(out, points.iter().rev().copied())
    }
}

fn append_polygon_order<I>(out: &mut Vec<Segment>, points: I) -> Result<()>
where
    I: IntoIterator<Item = Point>,
{
    let mut iter = points.into_iter();
    let Some(first) = iter.next() else {
        return Ok(());
    };
    let mut previous = first;
    for point in iter {
        append_segment(out, previous, point)?;
        previous = point;
    }
    append_segment(out, previous, first)
}

fn signed_area(points: &[Point]) -> f64 {
    let Some(first) = points.first().copied() else {
        return 0.0;
    };
    let mut sum = 0.0;
    let mut previous = first;
    for point in points.iter().copied().skip(1) {
        sum += previous.cross(point);
        previous = point;
    }
    sum += previous.cross(first);
    sum * 0.5
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LastCurve {
    None,
    Cubic,
    Quadratic,
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    path: Path,
    current: Point,
    subpath_start: Point,
    has_current: bool,
    last_command: Option<u8>,
    last_curve: LastCurve,
    last_cubic_control: Point,
    last_quadratic_control: Point,
}

impl<'a> Parser<'a> {
    fn new(data: &'a str) -> Self {
        Self {
            bytes: data.as_bytes(),
            position: 0,
            path: Path::default(),
            current: Point::default(),
            subpath_start: Point::default(),
            has_current: false,
            last_command: None,
            last_curve: LastCurve::None,
            last_cubic_control: Point::default(),
            last_quadratic_control: Point::default(),
        }
    }

    fn parse(mut self) -> Result<Path> {
        while {
            self.skip_separators();
            self.position < self.bytes.len()
        } {
            let command = self.read_or_repeat_command()?;
            self.execute(command)?;
        }
        Ok(self.path)
    }

    fn execute(&mut self, command: u8) -> Result<()> {
        let relative = command.is_ascii_lowercase();
        match command.to_ascii_uppercase() {
            b'M' => self.parse_move(relative)?,
            b'L' => self.parse_line(relative)?,
            b'H' => self.parse_horizontal(relative)?,
            b'V' => self.parse_vertical(relative)?,
            b'C' => self.parse_cubic(relative)?,
            b'S' => self.parse_smooth_cubic(relative)?,
            b'Q' => self.parse_quadratic(relative)?,
            b'T' => self.parse_smooth_quadratic(relative)?,
            b'A' => self.parse_arc(relative)?,
            b'Z' => self.close()?,
            _ => return Err(self.error("unsupported SVG path command")),
        }
        Ok(())
    }

    fn read_or_repeat_command(&mut self) -> Result<u8> {
        let byte = self.peek().ok_or_else(|| self.error("missing SVG command"))?;
        if byte.is_ascii_alphabetic() {
            self.position = self.position.checked_add(1).ok_or_else(|| self.error("SVG parser position overflow"))?;
            if !matches!(
                byte.to_ascii_uppercase(),
                b'M' | b'L' | b'H' | b'V' | b'C' | b'S' | b'Q' | b'T' | b'A' | b'Z'
            ) {
                return Err(self.error("unknown SVG path command"));
            }
            self.last_command = Some(byte);
            Ok(byte)
        } else {
            let command = self.last_command.ok_or_else(|| self.error("path data starts without a command"))?;
            if command.to_ascii_uppercase() == b'Z' {
                return Err(self.error("numbers cannot implicitly repeat close-path"));
            }
            Ok(command)
        }
    }

    fn parse_move(&mut self, relative: bool) -> Result<()> {
        let first = self.read_point(relative)?;
        self.push(Command::Move(first))?;
        self.current = first;
        self.subpath_start = first;
        self.has_current = true;
        self.reset_controls();
        self.last_command = Some(if relative { b'l' } else { b'L' });
        while self.has_number() {
            let point = self.read_point(relative)?;
            self.line_to(point)?;
        }
        Ok(())
    }

    fn parse_line(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let point = self.read_point(relative)?;
            self.line_to(point)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("line command needs coordinates")) }
    }

    fn parse_horizontal(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let value = self.number()?;
            let x = if relative { self.current.x + value } else { value };
            self.line_to(Point::new(x, self.current.y).finite()?)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("horizontal command needs a coordinate")) }
    }

    fn parse_vertical(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let value = self.number()?;
            let y = if relative { self.current.y + value } else { value };
            self.line_to(Point::new(self.current.x, y).finite()?)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("vertical command needs a coordinate")) }
    }

    fn parse_cubic(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let c1 = self.read_point(relative)?;
            let c2 = self.read_point(relative)?;
            let end = self.read_point(relative)?;
            self.cubic_to(c1, c2, end)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("cubic command needs six coordinates")) }
    }

    fn parse_smooth_cubic(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let c1 = if self.last_curve == LastCurve::Cubic {
                reflect(self.last_cubic_control, self.current)
            } else {
                self.current
            };
            let c2 = self.read_point(relative)?;
            let end = self.read_point(relative)?;
            self.cubic_to(c1, c2, end)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("smooth cubic command needs four coordinates")) }
    }

    fn parse_quadratic(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let control = self.read_point(relative)?;
            let end = self.read_point(relative)?;
            self.quadratic_to(control, end)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("quadratic command needs four coordinates")) }
    }

    fn parse_smooth_quadratic(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let control = if self.last_curve == LastCurve::Quadratic {
                reflect(self.last_quadratic_control, self.current)
            } else {
                self.current
            };
            let end = self.read_point(relative)?;
            self.quadratic_to(control, end)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("smooth quadratic command needs two coordinates")) }
    }

    fn parse_arc(&mut self, relative: bool) -> Result<()> {
        self.require_current()?;
        let mut any = false;
        while self.has_number() {
            let rx = self.number()?.abs();
            let ry = self.number()?.abs();
            let rotation = self.number()?;
            let large = self.flag()?;
            let sweep = self.flag()?;
            let end = self.read_point(relative)?;
            self.arc_to(rx, ry, rotation, large, sweep, end)?;
            any = true;
        }
        if any { Ok(()) } else { Err(self.error("arc command needs seven parameters")) }
    }

    fn close(&mut self) -> Result<()> {
        self.require_current()?;
        self.push(Command::Close)?;
        self.current = self.subpath_start;
        self.reset_controls();
        self.has_current = true;
        Ok(())
    }

    fn line_to(&mut self, point: Point) -> Result<()> {
        self.push(Command::Line(point))?;
        self.current = point;
        self.reset_controls();
        Ok(())
    }

    fn cubic_to(&mut self, c1: Point, c2: Point, end: Point) -> Result<()> {
        self.push(Command::Cubic(c1, c2, end))?;
        self.current = end;
        self.last_cubic_control = c2;
        self.last_curve = LastCurve::Cubic;
        Ok(())
    }

    fn quadratic_to(&mut self, control: Point, end: Point) -> Result<()> {
        let one_third = 1.0 / 3.0;
        let two_thirds = 2.0 / 3.0;
        let c1 = self.current.scale(one_third).add(control.scale(two_thirds));
        let c2 = end.scale(one_third).add(control.scale(two_thirds));
        self.push(Command::Cubic(c1, c2, end))?;
        self.current = end;
        self.last_quadratic_control = control;
        self.last_curve = LastCurve::Quadratic;
        Ok(())
    }

    fn arc_to(
        &mut self,
        rx: f64,
        ry: f64,
        rotation_degrees: f64,
        large_arc: bool,
        sweep: bool,
        end: Point,
    ) -> Result<()> {
        let start = self.current;
        self.reset_controls();
        if start.distance(end) <= EPSILON {
            self.current = end;
            return Ok(());
        }
        if rx <= EPSILON || ry <= EPSILON {
            return self.line_to(end);
        }
        for cubic in arc_to_cubics(start, end, rx, ry, rotation_degrees, large_arc, sweep)? {
            self.push(Command::Cubic(cubic.p1, cubic.p2, cubic.p3))?;
        }
        self.current = end;
        self.reset_controls();
        Ok(())
    }

    fn push(&mut self, command: Command) -> Result<()> {
        if self.path.commands.len() >= MAX_COMMANDS {
            return Err(Error::Refused("SVG path command limit exceeded".to_owned()));
        }
        self.path.commands.push(command);
        Ok(())
    }

    fn read_point(&mut self, relative: bool) -> Result<Point> {
        let x = self.number()?;
        let y = self.number()?;
        let point = Point::new(x, y);
        if relative { self.current.add(point).finite() } else { point.finite() }
    }

    fn flag(&mut self) -> Result<bool> {
        self.skip_separators();
        let byte = self.peek().ok_or_else(|| self.error("missing arc flag"))?;
        match byte {
            b'0' => {
                self.position = self.position.checked_add(1).ok_or_else(|| self.error("arc flag position overflow"))?;
                Ok(false)
            }
            b'1' => {
                self.position = self.position.checked_add(1).ok_or_else(|| self.error("arc flag position overflow"))?;
                Ok(true)
            }
            _ => Err(self.error("arc flag must be 0 or 1")),
        }
    }

    fn number(&mut self) -> Result<f64> {
        self.skip_separators();
        let start = self.position;
        if matches!(self.peek(), Some(b'+') | Some(b'-')) {
            self.bump()?;
        }
        let mut digits = 0_usize;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.bump()?;
            digits = digits.saturating_add(1);
        }
        if self.peek() == Some(b'.') {
            self.bump()?;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump()?;
                digits = digits.saturating_add(1);
            }
        }
        if digits == 0 {
            return Err(self.error("SVG number has no digits"));
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            self.bump()?;
            if matches!(self.peek(), Some(b'+') | Some(b'-')) {
                self.bump()?;
            }
            let mut exponent_digits = 0_usize;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.bump()?;
                exponent_digits = exponent_digits.saturating_add(1);
            }
            if exponent_digits == 0 {
                return Err(self.error("SVG exponent has no digits"));
            }
        }
        let slice = self.bytes.get(start..self.position).ok_or_else(|| self.error("SVG number slice invalid"))?;
        let text = std::str::from_utf8(slice).map_err(|_| self.error("SVG path data is not UTF-8"))?;
        let value = text.parse::<f64>().map_err(|_| self.error("invalid SVG number"))?;
        if value.is_finite() { Ok(value) } else { Err(self.error("non-finite SVG number")) }
    }

    fn has_number(&mut self) -> bool {
        self.skip_separators();
        matches!(self.peek(), Some(b'+') | Some(b'-') | Some(b'.') | Some(b'0'..=b'9'))
    }

    fn skip_separators(&mut self) {
        loop {
            match self.peek() {
                Some(byte) if byte.is_ascii_whitespace() || byte == b',' => self.position = self.position.saturating_add(1),
                _ => break,
            }
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn bump(&mut self) -> Result<()> {
        self.position = self.position.checked_add(1).ok_or_else(|| self.error("SVG parser position overflow"))?;
        Ok(())
    }

    fn require_current(&self) -> Result<()> {
        if self.has_current { Ok(()) } else { Err(self.error("drawing command before move")) }
    }

    fn reset_controls(&mut self) {
        self.last_curve = LastCurve::None;
    }

    fn error(&self, message: &str) -> Error {
        Error::damaged(format!("{message} at byte {}", self.position))
    }
}

fn reflect(control: Point, around: Point) -> Point {
    around.scale(2.0).sub(control)
}

fn arc_to_cubics(
    start: Point,
    end: Point,
    mut rx: f64,
    mut ry: f64,
    rotation_degrees: f64,
    large_arc: bool,
    sweep: bool,
) -> Result<Vec<Cubic>> {
    if !rx.is_finite() || !ry.is_finite() || !rotation_degrees.is_finite() {
        return Err(Error::damaged("non-finite SVG arc parameter"));
    }
    rx = rx.abs();
    ry = ry.abs();
    if rx <= EPSILON || ry <= EPSILON || start.distance(end) <= EPSILON {
        return Ok(Vec::new());
    }

    let phi = rotation_degrees.rem_euclid(360.0) * PI / 180.0;
    let (sin_phi, cos_phi) = phi.sin_cos();
    let delta = start.sub(end).scale(0.5);
    let x1p = cos_phi.mul_add(delta.x, sin_phi * delta.y);
    let y1p = (-sin_phi).mul_add(delta.x, cos_phi * delta.y);

    let mut rx_sq = rx * rx;
    let mut ry_sq = ry * ry;
    let x_sq = x1p * x1p;
    let y_sq = y1p * y1p;
    let lambda = x_sq / rx_sq + y_sq / ry_sq;
    if lambda > 1.0 {
        let scale = lambda.sqrt();
        rx *= scale;
        ry *= scale;
        rx_sq = rx * rx;
        ry_sq = ry * ry;
    }

    let numerator = (rx_sq * ry_sq - rx_sq * y_sq - ry_sq * x_sq).max(0.0);
    let denominator = rx_sq.mul_add(y_sq, ry_sq * x_sq);
    let sign = if large_arc == sweep { -1.0 } else { 1.0 };
    let coefficient = if denominator <= EPSILON { 0.0 } else { sign * (numerator / denominator).sqrt() };
    let cxp = coefficient * (rx * y1p / ry);
    let cyp = coefficient * (-ry * x1p / rx);
    let midpoint = start.add(end).scale(0.5);
    let center = Point::new(
        cos_phi.mul_add(cxp, (-sin_phi).mul_add(cyp, midpoint.x)),
        sin_phi.mul_add(cxp, cos_phi.mul_add(cyp, midpoint.y)),
    );

    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = uy.atan2(ux);
    let mut sweep_angle = signed_angle(Point::new(ux, uy), Point::new(vx, vy));
    if !sweep && sweep_angle > 0.0 {
        sweep_angle -= 2.0 * PI;
    } else if sweep && sweep_angle < 0.0 {
        sweep_angle += 2.0 * PI;
    }

    let turns = sweep_angle.abs() / FRAC_PI_2;
    let count = if turns <= 1.0 {
        1_usize
    } else if turns <= 2.0 {
        2_usize
    } else if turns <= 3.0 {
        3_usize
    } else {
        4_usize
    };
    let count_u32 = u32::try_from(count).map_err(|_| Error::damaged("arc segment count conversion failed"))?;
    let step = sweep_angle / f64::from(count_u32);
    let mut result = Vec::with_capacity(count);
    let mut angle = theta1;
    let mut curve_start = start;

    for index in 0..count {
        let last = index.checked_add(1) == Some(count);
        let next_angle = if last { theta1 + sweep_angle } else { angle + step };
        let delta_angle = next_angle - angle;
        let alpha = 4.0 / 3.0 * (delta_angle * 0.25).tan();
        let unit0 = Point::new(angle.cos(), angle.sin());
        let unit1 = Point::new(next_angle.cos(), next_angle.sin());
        let derivative0 = Point::new(-unit0.y, unit0.x);
        let derivative1 = Point::new(-unit1.y, unit1.x);
        let c1 = ellipse_map(unit0.add(derivative0.scale(alpha)), center, rx, ry, cos_phi, sin_phi);
        let c2 = ellipse_map(unit1.sub(derivative1.scale(alpha)), center, rx, ry, cos_phi, sin_phi);
        let curve_end = if last { end } else { ellipse_map(unit1, center, rx, ry, cos_phi, sin_phi) };
        result.push(Cubic { p0: curve_start, p1: c1, p2: c2, p3: curve_end, depth: 0 });
        curve_start = curve_end;
        angle = next_angle;
    }
    Ok(result)
}

fn ellipse_map(unit: Point, center: Point, rx: f64, ry: f64, cos_phi: f64, sin_phi: f64) -> Point {
    let x = rx * unit.x;
    let y = ry * unit.y;
    Point::new(
        cos_phi.mul_add(x, (-sin_phi).mul_add(y, center.x)),
        sin_phi.mul_add(x, cos_phi.mul_add(y, center.y)),
    )
}

fn signed_angle(a: Point, b: Point) -> f64 {
    a.cross(b).atan2(a.dot(b))
}

/// Built-in 24×24 interface icons.
///
/// Every icon uses the same two-unit round stroke and is intentionally stored
/// as editable SVG path data rather than pre-flattened points.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Icon {
    /// Multiple save slots.
    Saves,
    /// Player inventory.
    Inventory,
    /// Personal stash.
    Stash,
    /// Map and transitions.
    MapTransitions,
    /// Factions.
    Factions,
    /// Backup.
    Backup,
    /// Compare.
    Compare,
    /// Timeline.
    Timeline,
    /// Save doctor.
    Doctor,
    /// Games.
    Games,
    /// Fix collection.
    Fixes,
    /// Wrench/tooling.
    Wrench,
    /// Companion.
    Companion,
    /// Trophy/achievement.
    Trophy,
    /// Cloud.
    Cloud,
    /// Book/documentation.
    Book,
    /// Shield/capabilities.
    ShieldCapabilities,
    /// Update.
    Update,
    /// Settings.
    Settings,
    /// Search.
    Search,
    /// Add.
    Add,
    /// Delete.
    Delete,
    /// Undo.
    Undo,
    /// Redo.
    Redo,
    /// Save.
    Save,
    /// Folder.
    Folder,
    /// Warning.
    Warning,
    /// Information.
    Info,
}

impl Icon {
    /// Returns hand-authored SVG path data on a 24×24 grid.
    #[must_use]
    pub const fn path_data(self) -> &'static str {
        match self {
            Self::Saves => "M6 3h11l3 3v13a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z M8 3v6h8V3 M8 15h8 M8 18h6",
            Self::Inventory => "M4 7h16v13H4z M8 7V4h8v3 M8 11h3v3H8z M13 11h3v3h-3z M8 16h3v2H8z M13 16h3v2h-3z",
            Self::Stash => "M3 8h18v12H3z M5 8l2-4h10l2 4 M8 12h8 M10 16h4",
            Self::MapTransitions => "M4 5l5-2 6 2 5-2v16l-5 2-6-2-5 2z M9 3v16 M15 5v16 M12 10h6 M15 7l3 3-3 3",
            Self::Factions => "M8 10a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M16 11a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M3 20c0-4 2-7 5-7s5 3 5 7 M11 20c0-3 2-6 5-6s5 3 5 6",
            Self::Backup => "M5 7h14v13H5z M8 7V4h8v3 M8 15a4 4 0 1 0 1-3 M8 12v4h4",
            Self::Compare => "M7 4v16 M17 4v16 M4 8h6 M14 8h6 M4 16h6 M14 16h6 M8 5l-2 3 2 3 M16 13l2 3-2 3",
            Self::Timeline => "M4 6h16 M4 12h16 M4 18h16 M8 3v6 M14 9v6 M18 15v6",
            Self::Doctor => "M9 4h6v5h5v6h-5v5H9v-5H4V9h5z",
            Self::Games => "M7 9h10a4 4 0 0 1 4 4v3a3 3 0 0 1-5 2l-2-2h-4l-2 2a3 3 0 0 1-5-2v-3a4 4 0 0 1 4-4z M7 13h4 M9 11v4 M16 12h.01 M18 14h.01",
            Self::Fixes => "M5 5l4 4 M15 15l4 4 M14 5a5 5 0 0 0 5 5l-9 9a3 3 0 0 1-4-4l9-9a5 5 0 0 0-1-1z",
            Self::Wrench => "M14 5a5 5 0 0 0 5 5l-9 9a3 3 0 0 1-4-4l9-9 M16 4l-3 3 4 4 3-3",
            Self::Companion => "M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8z M5 21c0-5 3-7 7-7s7 2 7 7 M4 9l-2 2 2 2 M20 9l2 2-2 2",
            Self::Trophy => "M7 4h10v5a5 5 0 0 1-10 0z M7 6H4v2a4 4 0 0 0 4 4 M17 6h3v2a4 4 0 0 1-4 4 M12 14v4 M8 21h8 M10 18h4",
            Self::Cloud => "M7 18h11a4 4 0 0 0 0-8 6 6 0 0 0-11-2 5 5 0 0 0 0 10z",
            Self::Book => "M4 5a3 3 0 0 1 3-2h5v17H7a3 3 0 0 0-3 2z M20 5a3 3 0 0 0-3-2h-5v17h5a3 3 0 0 1 3 2z",
            Self::ShieldCapabilities => "M12 3l8 3v5c0 5-3 8-8 10-5-2-8-5-8-10V6z M8 12l3 3 5-6",
            Self::Update => "M20 7v5h-5 M4 17v-5h5 M6 9a7 7 0 0 1 12-3l2 2 M18 15a7 7 0 0 1-12 3l-2-2",
            Self::Settings => "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z M12 3v2 M12 19v2 M3 12h2 M19 12h2 M5.6 5.6L7 7 M17 17l1.4 1.4 M18.4 5.6L17 7 M7 17l-1.4 1.4",
            Self::Search => "M10 4a6 6 0 1 0 0 12 6 6 0 0 0 0-12z M14.5 14.5L20 20",
            Self::Add => "M12 5v14 M5 12h14",
            Self::Delete => "M5 7h14 M9 7V4h6v3 M7 7l1 13h8l1-13 M10 10v7 M14 10v7",
            Self::Undo => "M9 7L4 12l5 5 M5 12h8a6 6 0 0 1 6 6",
            Self::Redo => "M15 7l5 5-5 5 M19 12h-8a6 6 0 0 0-6 6",
            Self::Save => "M5 3h12l3 3v15H4V4a1 1 0 0 1 1-1z M8 3v6h8V3 M8 15h8v6H8z",
            Self::Folder => "M3 7h7l2 2h9v11H3z M3 7V5h7l2 2",
            Self::Warning => "M12 3l10 18H2z M12 9v5 M12 18h.01",
            Self::Info => "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z M12 10v7 M12 7h.01",
        }
    }

    /// Parses this icon as a path.
    ///
    /// # Errors
    /// Only returns an error if an embedded icon path is accidentally changed
    /// to invalid SVG data.
    pub fn path(self) -> Result<Path> {
        Path::parse(self.path_data())
    }
}

/// All built-in icons in stable display order.
pub const ALL_ICONS: [Icon; 28] = [
    Icon::Saves, Icon::Inventory, Icon::Stash, Icon::MapTransitions, Icon::Factions, Icon::Backup,
    Icon::Compare, Icon::Timeline, Icon::Doctor, Icon::Games, Icon::Fixes, Icon::Wrench,
    Icon::Companion, Icon::Trophy, Icon::Cloud, Icon::Book, Icon::ShieldCapabilities, Icon::Update,
    Icon::Settings, Icon::Search, Icon::Add, Icon::Delete, Icon::Undo, Icon::Redo, Icon::Save,
    Icon::Folder, Icon::Warning, Icon::Info,
];

#[cfg(test)]
mod tests {
    use super::{arc_to_cubics, FillRule, Icon, LineCap, LineJoin, Path, Point, StrokeStyle, Transform, ALL_ICONS};

    fn approx(left: f64, right: f64, epsilon: f64) -> bool {
        (left - right).abs() <= epsilon
    }

    #[test]
    fn parser_accepts_all_command_families_and_implicit_repeats() {
        let data = concat!(
            "M1 2 3 4 L5 6 7 8 H9 10 V11 12 ",
            "C13 14 15 16 17 18 19 20 21 22 23 24 ",
            "S25 26 27 28 Q29 30 31 32 T33 34 ",
            "A5 6 30 0 1 40 41 z"
        );
        let path = Path::parse(data);
        assert!(path.is_ok());
        assert!(path.as_ref().is_ok_and(|value| value.command_count() >= 13));
    }

    #[test]
    fn relative_commands_accumulate_from_current_point() {
        let flat = Path::parse("M10 10 l5 0 0 5 h-5 v-5 z")
            .and_then(|value| value.flatten(Transform::identity(), 0.1, FillRule::NonZero));
        assert!(flat.as_ref().is_ok_and(|value| value.segments.len() == 4));
    }

    #[test]
    fn number_parser_handles_compact_signs_decimals_and_exponents() {
        let flat = Path::parse("M.5-.5L1e1-2E0")
            .and_then(|value| value.flatten(Transform::identity(), 0.01, FillRule::NonZero));
        let segment = flat.ok().and_then(|value| value.segments.first().copied());
        assert!(segment.is_some_and(|value| approx(value.to.x, 10.0, 1.0e-9)));
        assert!(segment.is_some_and(|value| approx(value.to.y, -2.0, 1.0e-9)));
    }

    #[test]
    fn malformed_arc_flags_are_rejected() {
        assert!(Path::parse("M0 0 A10 10 0 2 0 20 0").is_err());
        assert!(Path::parse("M0 0 A10 10 0 0 x 20 0").is_err());
    }

    #[test]
    fn smooth_cubic_reflects_previous_control() {
        let flat = Path::parse("M0 0 C0 10 10 10 10 0 S20-10 20 0")
            .and_then(|value| value.flatten(Transform::identity(), 0.05, FillRule::NonZero));
        assert!(flat.as_ref().is_ok_and(|value| value.segments.len() > 4));
    }

    #[test]
    fn quadratic_and_smooth_quadratic_flatten() {
        let flat = Path::parse("M0 0 Q10 20 20 0 T40 0")
            .and_then(|value| value.flatten(Transform::identity(), 0.1, FillRule::NonZero));
        assert!(flat.as_ref().is_ok_and(|value| value.segments.len() > 4));
    }

    #[test]
    fn quarter_circle_arc_becomes_one_cubic_with_exact_endpoints() {
        let curves = arc_to_cubics(Point::new(1.0, 0.0), Point::new(0.0, 1.0), 1.0, 1.0, 0.0, false, true);
        assert!(curves.as_ref().is_ok_and(|value| value.len() == 1));
        let curve = curves.ok().and_then(|value| value.first().copied());
        assert!(curve.is_some_and(|value| approx(value.p0.x, 1.0, 1.0e-9)));
        assert!(curve.is_some_and(|value| approx(value.p0.y, 0.0, 1.0e-9)));
        assert!(curve.is_some_and(|value| approx(value.p3.x, 0.0, 1.0e-9)));
        assert!(curve.is_some_and(|value| approx(value.p3.y, 1.0, 1.0e-9)));
    }

    #[test]
    fn large_arc_is_split_into_multiple_cubics() {
        let curves = arc_to_cubics(Point::new(1.0, 0.0), Point::new(0.0, 1.0), 1.0, 1.0, 0.0, true, true);
        assert!(curves.as_ref().is_ok_and(|value| value.len() == 3));
    }

    #[test]
    fn arc_radii_are_scaled_when_endpoints_do_not_fit() {
        let curves = arc_to_cubics(Point::new(0.0, 0.0), Point::new(100.0, 0.0), 10.0, 10.0, 0.0, false, true);
        let end = curves.ok().and_then(|value| value.last().copied());
        assert!(end.is_some_and(|value| approx(value.p3.x, 100.0, 1.0e-9)));
        assert!(end.is_some_and(|value| approx(value.p3.y, 0.0, 1.0e-9)));
    }

    #[test]
    fn transform_is_applied_before_flatness_test() {
        let path = Path::parse("M0 0 C0 1 1 1 1 0");
        let small = path.as_ref().ok().and_then(|value| value.flatten(Transform::identity(), 0.1, FillRule::NonZero).ok());
        let scaled = path.as_ref().ok().and_then(|value| value.flatten(Transform::scale(100.0, 100.0), 0.1, FillRule::NonZero).ok());
        assert!(small.zip(scaled).is_some_and(|(a, b)| b.segments.len() > a.segments.len()));
    }

    #[test]
    fn fill_rules_differ_for_double_wound_shape() {
        let path = Path::parse("M0 0L10 0L10 10L0 10Z M0 0L10 0L10 10L0 10Z");
        let non_zero = path.as_ref().ok().and_then(|value| value.flatten(Transform::identity(), 0.1, FillRule::NonZero).ok());
        let even_odd = path.as_ref().ok().and_then(|value| value.flatten(Transform::identity(), 0.1, FillRule::EvenOdd).ok());
        assert!(non_zero.as_ref().is_some_and(|value| value.contains(Point::new(5.0, 5.0))));
        assert!(even_odd.as_ref().is_some_and(|value| !value.contains(Point::new(5.0, 5.0))));
    }

    #[test]
    fn stroke_caps_have_expected_extent() {
        let round = Path::parse("M5 12L19 12").ok().and_then(|value| {
            value.stroke_to_fill(
                Transform::identity(),
                0.05,
                StrokeStyle { width: 2.0, cap: LineCap::Round, join: LineJoin::Round, miter_limit: 4.0 },
            ).ok()
        });
        assert!(round.as_ref().is_some_and(|value| value.contains(Point::new(4.2, 12.0))));
        assert!(round.as_ref().is_some_and(|value| !value.contains(Point::new(3.5, 12.0))));
    }

    #[test]
    fn miter_join_expands_outside_corner() {
        let stroke = Path::parse("M4 18L12 6L20 18").ok().and_then(|value| {
            value.stroke_to_fill(
                Transform::identity(),
                0.05,
                StrokeStyle { width: 2.0, cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 8.0 },
            ).ok()
        });
        assert!(stroke.as_ref().is_some_and(|value| !value.segments.is_empty()));
    }

    #[test]
    fn all_twenty_eight_icons_parse_and_stroke() {
        assert_eq!(ALL_ICONS.len(), 28);
        for icon in ALL_ICONS {
            let path = icon.path();
            assert!(path.is_ok(), "{icon:?}");
            let stroke = path.ok().and_then(|value| {
                value.stroke_to_fill(Transform::identity(), 0.1, StrokeStyle::icon()).ok()
            });
            assert!(stroke.as_ref().is_some_and(|value| !value.segments.is_empty()), "{icon:?}");
        }
    }

    #[test]
    fn icon_path_strings_use_the_24_grid_convention() {
        let flat = Icon::Add.path().and_then(|path| path.flatten(Transform::identity(), 0.1, FillRule::NonZero));
        assert!(flat.as_ref().is_ok_and(|value| value.segments.len() == 2));
    }
}
