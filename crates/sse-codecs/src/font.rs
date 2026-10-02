//! Safe OpenType/TrueType/CFF font parsing, variation, and analytic coverage rasterisation.
//!
//! The parser borrows the source bytes and validates every table range before use. TrueType
//! `glyf` and CFF Type 2 outlines, cmap formats 4/12, horizontal metrics, legacy `kern`,
//! and one selected `wght` variation instance through `fvar`/`avar`/`gvar` are supported.
//! All offsets and hostile lengths are checked before use; malformed input never becomes guessed geometry.

use sse_core::{Error, Result};

const MAX_TABLES: usize = 4_096;
const MAX_GLYPHS: usize = 1_000_000;
const MAX_CONTOURS: usize = 16_384;
const MAX_POINTS: usize = 1_000_000;
const MAX_COMPOSITE_DEPTH: usize = 8;
const MAX_CFF_STACK: usize = 48;
const MAX_CFF_SUBR_DEPTH: usize = 10;
const MAX_CFF_FDS: usize = 512;
const MAX_GVAR_TUPLES: usize = 4_095;
const MAX_VARIATION_AXES: usize = 32;

/// Identifier of a glyph in the font.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphId(pub u16);

/// Global font metrics, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Ascender from the `hhea` table.
    pub ascent: f32,
    /// Descender from the `hhea` table.
    pub descent: f32,
    /// Recommended line gap from the `hhea` table.
    pub line_gap: f32,
    /// Units per em from the `head` table.
    pub units_per_em: f32,
}

/// A path sink used by [`Font::outline`].
pub trait OutlineSink {
    /// Starts a contour.
    fn move_to(&mut self, x: f32, y: f32);
    /// Adds a line segment.
    fn line_to(&mut self, x: f32, y: f32);
    /// Adds a quadratic Bézier.
    fn quad_to(&mut self, control_x: f32, control_y: f32, x: f32, y: f32);
    /// Adds a cubic Bézier.
    fn cubic_to(&mut self, control1_x: f32, control1_y: f32, control2_x: f32, control2_y: f32, x: f32, y: f32);
    /// Closes the current contour.
    fn close(&mut self);
}

/// A retained path segment accepted by [`rasterize`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    /// Starts a contour.
    MoveTo(f32, f32),
    /// Adds a line.
    LineTo(f32, f32),
    /// Adds a quadratic Bézier.
    QuadTo(f32, f32, f32, f32),
    /// Adds a cubic Bézier.
    CubicTo(f32, f32, f32, f32, f32, f32),
    /// Closes the contour.
    Close,
}

#[derive(Clone, Copy, Debug)]
struct Table {
    tag: [u8; 4],
    offset: usize,
    length: usize,
}

#[derive(Clone, Copy, Debug)]
struct Transform {
    xx: f32,
    xy: f32,
    yx: f32,
    yy: f32,
    dx: f32,
    dy: f32,
}

impl Transform {
    fn identity() -> Self {
        Self {
            xx: 1.0,
            xy: 0.0,
            yx: 0.0,
            yy: 1.0,
            dx: 0.0,
            dy: 0.0,
        }
    }

    fn point(self, x: f32, y: f32) -> (f32, f32) {
        (
            self.xx.mul_add(x, self.xy.mul_add(y, self.dx)),
            self.yx.mul_add(x, self.yy.mul_add(y, self.dy)),
        )
    }

    fn combine(self, child: Self) -> Self {
        Self {
            xx: self.xx.mul_add(child.xx, self.xy * child.yx),
            xy: self.xx.mul_add(child.xy, self.xy * child.yy),
            yx: self.yx.mul_add(child.xx, self.yy * child.yx),
            yy: self.yx.mul_add(child.xy, self.yy * child.yy),
            dx: self.xx.mul_add(child.dx, self.xy.mul_add(child.dy, self.dx)),
            dy: self.yx.mul_add(child.dx, self.yy.mul_add(child.dy, self.dy)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Point {
    x: f32,
    y: f32,
    on_curve: bool,
}

#[derive(Clone, Debug)]
struct VariationState {
    gvar: Table,
    axis_count: usize,
    coords: Vec<f32>,
    shared_tuple_count: usize,
    shared_tuples_offset: usize,
    glyph_count: usize,
    data_offset: usize,
    long_offsets: bool,
}

#[derive(Clone, Copy, Debug)]
struct CffIndex {
    count: usize,
    offsets: usize,
    data: usize,
    end: usize,
    off_size: usize,
}

impl CffIndex {
    fn object<'a>(&self, data: &'a [u8], index: usize) -> Result<&'a [u8]> {
        if index >= self.count {
            return Err(Error::damaged("CFF INDEX object is outside range"));
        }
        let a = cff_read_offset(
            data,
            checked_add(self.offsets, checked_mul(index, self.off_size)?)?,
            self.off_size,
        )?;
        let b = cff_read_offset(
            data,
            checked_add(self.offsets, checked_mul(checked_add(index, 1)?, self.off_size)?)?,
            self.off_size,
        )?;
        if a == 0 || b < a {
            return Err(Error::damaged("CFF INDEX offsets are invalid"));
        }
        let a_offset = a
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("CFF INDEX start offset underflow"))?;
        let b_offset = b
            .checked_sub(1)
            .ok_or_else(|| Error::damaged("CFF INDEX end offset underflow"))?;
        let start = checked_add(self.data, a_offset)?;
        let end = checked_add(self.data, b_offset)?;
        if start > end || end > self.end {
            return Err(Error::damaged("CFF INDEX object exceeds table"));
        }
        let length = end
            .checked_sub(start)
            .ok_or_else(|| Error::damaged("CFF INDEX object length underflow"))?;
        checked_range(data, start, length)
    }
}

#[derive(Clone, Debug)]
struct CffPrivate {
    local_subrs: Option<CffIndex>,
}

#[derive(Clone, Debug)]
struct CffState {
    char_strings: CffIndex,
    global_subrs: CffIndex,
    private: Option<CffPrivate>,
    fd_array: Vec<CffPrivate>,
    fd_select_offset: Option<usize>,
    fd_select_end: usize,
}

#[derive(Clone, Copy, Debug)]
struct Component {
    glyph: u16,
    transform: Transform,
}

/// A borrowed OpenType/TrueType font.
pub struct Font<'a> {
    data: &'a [u8],
    glyph_count: u32,
    units_per_em: u16,
    ascent: i16,
    descent: i16,
    line_gap: i16,
    number_of_h_metrics: u16,
    loca_long: bool,
    cmap_offset: usize,
    cmap_length: usize,
    glyf: Option<Table>,
    loca: Option<Table>,
    hmtx: Table,
    kern: Option<Table>,
    cff: Option<CffState>,
    variation: Option<VariationState>,
}

impl<'a> Font<'a> {
    /// Parses a `.ttf`, `.otf`, or a selected face of a `.ttc`.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed offsets/counts/ranges, and
    /// [`Error::Refused`] for an unsupported outline flavour.
    pub fn parse(data: &'a [u8], index_in_collection: u32) -> Result<Self> {
        Self::parse_impl(data, index_in_collection, None)
    }

    /// Parses a font and selects one `wght` instance from an `fvar`/`gvar` font.
    ///
    /// The requested value is clamped to the axis range. If the font is not variable or has no
    /// `wght` axis, an error is returned instead of silently using the default outline.
    pub fn parse_with_weight(data: &'a [u8], index_in_collection: u32, weight: f32) -> Result<Self> {
        Self::parse_impl(data, index_in_collection, Some(weight))
    }

    fn parse_impl(data: &'a [u8], index_in_collection: u32, weight: Option<f32>) -> Result<Self> {
        let sfnt_offset = collection_face_offset(data, index_in_collection)?;
        let num_tables = usize::from(be_u16_at(data, checked_add(sfnt_offset, 4)?)?);
        if num_tables == 0 || num_tables > MAX_TABLES {
            return Err(Error::damaged("font table count is outside the allowed range"));
        }

        let records_start = checked_add(sfnt_offset, 12)?;
        let records_bytes = checked_mul(num_tables, 16)?;
        checked_range(data, records_start, records_bytes)?;

        let mut tables = Vec::with_capacity(num_tables);
        let mut index = 0_usize;
        while index < num_tables {
            let record = checked_add(records_start, checked_mul(index, 16)?)?;
            let tag_slice = checked_range(data, record, 4)?;
            let tag = <[u8; 4]>::try_from(tag_slice).map_err(|_| Error::damaged("invalid font table tag"))?;
            let offset = usize::try_from(be_u32_at(data, checked_add(record, 8)?)?)
                .map_err(|_| Error::damaged("font table offset does not fit usize"))?;
            let length = usize::try_from(be_u32_at(data, checked_add(record, 12)?)?)
                .map_err(|_| Error::damaged("font table length does not fit usize"))?;
            checked_range(data, offset, length)?;
            tables.push(Table { tag, offset, length });
            index = checked_add(index, 1)?;
        }

        let head = required_table(&tables, *b"head")?;
        if head.length < 54 {
            return Err(Error::damaged("short head table"));
        }
        let units_per_em = be_u16_at(data, checked_add(head.offset, 18)?)?;
        if units_per_em == 0 {
            return Err(Error::damaged("font units-per-em is zero"));
        }
        let loca_format = be_i16_at(data, checked_add(head.offset, 50)?)?;
        let loca_long = match loca_format {
            0 => false,
            1 => true,
            _ => return Err(Error::damaged("unsupported loca format")),
        };

        let maxp = required_table(&tables, *b"maxp")?;
        if maxp.length < 6 {
            return Err(Error::damaged("short maxp table"));
        }
        let glyph_count = u32::from(be_u16_at(data, checked_add(maxp.offset, 4)?)?);
        if usize::try_from(glyph_count).unwrap_or(usize::MAX) > MAX_GLYPHS {
            return Err(Error::damaged("font glyph count exceeds limit"));
        }

        let hhea = required_table(&tables, *b"hhea")?;
        if hhea.length < 36 {
            return Err(Error::damaged("short hhea table"));
        }
        let ascent = be_i16_at(data, checked_add(hhea.offset, 4)?)?;
        let descent = be_i16_at(data, checked_add(hhea.offset, 6)?)?;
        let line_gap = be_i16_at(data, checked_add(hhea.offset, 8)?)?;
        let number_of_h_metrics = be_u16_at(data, checked_add(hhea.offset, 34)?)?;
        if number_of_h_metrics == 0 || u32::from(number_of_h_metrics) > glyph_count {
            return Err(Error::damaged("invalid numberOfHMetrics"));
        }
        let hmtx = required_table(&tables, *b"hmtx")?;
        let required_hmtx = checked_mul(usize::from(number_of_h_metrics), 4)?;
        if hmtx.length < required_hmtx {
            return Err(Error::damaged("short hmtx table"));
        }

        let cmap = required_table(&tables, *b"cmap")?;
        let (cmap_offset, cmap_length) = select_cmap(data, cmap)?;

        let glyf = find_table(&tables, *b"glyf");
        let loca = find_table(&tables, *b"loca");
        let cff_table = find_table(&tables, *b"CFF ");
        if glyf.is_some() != loca.is_some() {
            return Err(Error::damaged("glyf and loca must appear together"));
        }
        if glyf.is_none() && cff_table.is_none() {
            return Err(Error::Refused("font has no supported outline table".to_owned()));
        }

        if let Some(loca_table) = loca {
            let entries = usize::try_from(glyph_count)
                .ok()
                .and_then(|value| value.checked_add(1))
                .ok_or_else(|| Error::damaged("loca entry count overflow"))?;
            let width = if loca_long { 4 } else { 2 };
            let needed = checked_mul(entries, width)?;
            if loca_table.length < needed {
                return Err(Error::damaged("short loca table"));
            }
        }

        let cff = cff_table
            .map(|table| {
                parse_cff(
                    data,
                    table,
                    usize::try_from(glyph_count).map_err(|_| Error::damaged("glyph count overflow"))?,
                )
            })
            .transpose()?;
        let variation = parse_variation(
            data,
            &tables,
            usize::try_from(glyph_count).map_err(|_| Error::damaged("glyph count overflow"))?,
            weight,
        )?;

        Ok(Self {
            data,
            glyph_count,
            units_per_em,
            ascent,
            descent,
            line_gap,
            number_of_h_metrics,
            loca_long,
            cmap_offset,
            cmap_length,
            glyf,
            loca,
            hmtx,
            kern: find_table(&tables, *b"kern"),
            cff,
            variation,
        })
    }

    /// Looks up a Unicode scalar through cmap format 12 or 4.
    #[must_use]
    pub fn glyph(&self, character: char) -> Option<GlyphId> {
        let code = u32::from(character);
        match be_u16_at(self.data, self.cmap_offset).ok()? {
            4 => cmap4_lookup(self.data, self.cmap_offset, self.cmap_length, code),
            12 => cmap12_lookup(self.data, self.cmap_offset, self.cmap_length, code),
            _ => None,
        }
        .and_then(|value| u16::try_from(value).ok())
        .map(GlyphId)
    }

    /// Returns the horizontal advance in font units.
    #[must_use]
    pub fn advance(&self, glyph: GlyphId) -> f32 {
        let gid = u32::from(glyph.0);
        if gid >= self.glyph_count {
            return 0.0;
        }
        let metrics = u32::from(self.number_of_h_metrics);
        let index = gid.min(metrics.saturating_sub(1));
        let Some(index) = usize::try_from(index).ok() else {
            return 0.0;
        };
        let Some(relative) = index.checked_mul(4) else {
            return 0.0;
        };
        let Some(offset) = self.hmtx.offset.checked_add(relative) else {
            return 0.0;
        };
        be_u16_at(self.data, offset).map_or(0.0, f32::from)
    }

    /// Returns legacy `kern` format-0 horizontal kerning in font units.
    #[must_use]
    pub fn kerning(&self, a: GlyphId, b: GlyphId) -> f32 {
        self.kern
            .and_then(|table| kern_lookup(self.data, table, a.0, b.0).ok().flatten())
            .map_or(0.0, f32::from)
    }

    /// Returns global metrics in font units.
    #[must_use]
    pub fn metrics(&self) -> Metrics {
        Metrics {
            ascent: f32::from(self.ascent),
            descent: f32::from(self.descent),
            line_gap: f32::from(self.line_gap),
            units_per_em: f32::from(self.units_per_em),
        }
    }

    /// Sends the requested outline to `sink`.
    ///
    /// # Errors
    /// Returns an error for an invalid glyph id, malformed outline or unsupported CFF outline.
    pub fn outline(&self, glyph: GlyphId, sink: &mut impl OutlineSink) -> Result<()> {
        if u32::from(glyph.0) >= self.glyph_count {
            return Err(Error::damaged("glyph id is outside the font"));
        }
        if self.glyf.is_some() {
            return self.outline_glyf(glyph.0, sink, Transform::identity(), 0);
        }
        if let Some(cff) = &self.cff {
            return outline_cff(self.data, cff, glyph.0, sink);
        }
        Err(Error::damaged("font has no outline table"))
    }

    fn outline_glyf(&self, glyph: u16, sink: &mut impl OutlineSink, transform: Transform, depth: usize) -> Result<()> {
        if depth > MAX_COMPOSITE_DEPTH {
            return Err(Error::damaged("composite glyph depth exceeds limit"));
        }
        let glyf = self.glyf.ok_or_else(|| Error::damaged("missing glyf table"))?;
        let (start, end) = self.glyph_range(glyph)?;
        if start == end {
            return Ok(());
        }
        let absolute = checked_add(glyf.offset, start)?;
        let length = end
            .checked_sub(start)
            .ok_or_else(|| Error::damaged("glyph range underflow"))?;
        let bytes = checked_range(self.data, absolute, length)?;
        if bytes.len() < 10 {
            return Err(Error::damaged("short glyph header"));
        }
        let contours = be_i16_at(bytes, 0)?;
        if contours >= 0 {
            self.outline_simple(
                glyph,
                bytes,
                usize::try_from(contours).map_err(|_| Error::damaged("contour count"))?,
                sink,
                transform,
            )
        } else {
            self.outline_composite(glyph, bytes, sink, transform, depth)
        }
    }

    fn glyph_range(&self, glyph: u16) -> Result<(usize, usize)> {
        let loca = self.loca.ok_or_else(|| Error::damaged("missing loca table"))?;
        let index = usize::from(glyph);
        let next = checked_add(index, 1)?;
        let start = if self.loca_long {
            let at = checked_add(loca.offset, checked_mul(index, 4)?)?;
            usize::try_from(be_u32_at(self.data, at)?).map_err(|_| Error::damaged("loca offset does not fit usize"))?
        } else {
            let at = checked_add(loca.offset, checked_mul(index, 2)?)?;
            checked_mul(usize::from(be_u16_at(self.data, at)?), 2)?
        };
        let end = if self.loca_long {
            let at = checked_add(loca.offset, checked_mul(next, 4)?)?;
            usize::try_from(be_u32_at(self.data, at)?).map_err(|_| Error::damaged("loca offset does not fit usize"))?
        } else {
            let at = checked_add(loca.offset, checked_mul(next, 2)?)?;
            checked_mul(usize::from(be_u16_at(self.data, at)?), 2)?
        };
        let glyf = self.glyf.ok_or_else(|| Error::damaged("missing glyf table"))?;
        if start > end || end > glyf.length {
            return Err(Error::damaged("loca points outside glyf table"));
        }
        Ok((start, end))
    }

    fn outline_simple(
        &self,
        glyph: u16,
        bytes: &[u8],
        contour_count: usize,
        sink: &mut impl OutlineSink,
        transform: Transform,
    ) -> Result<()> {
        if contour_count > MAX_CONTOURS {
            return Err(Error::damaged("glyph contour count exceeds limit"));
        }
        if contour_count == 0 {
            return Ok(());
        }
        let mut position = 10_usize;
        let mut ends = Vec::with_capacity(contour_count);
        let mut previous = None;
        let mut index = 0_usize;
        while index < contour_count {
            let end = be_u16_at(bytes, position)?;
            position = checked_add(position, 2)?;
            if previous.is_some_and(|value| end <= value) {
                return Err(Error::damaged("glyph contour endpoints are not increasing"));
            }
            previous = Some(end);
            ends.push(end);
            index = checked_add(index, 1)?;
        }
        let point_count = previous
            .map(usize::from)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| Error::damaged("simple glyph has no points"))?;
        if point_count > MAX_POINTS {
            return Err(Error::damaged("glyph point count exceeds limit"));
        }
        let instruction_length = usize::from(be_u16_at(bytes, position)?);
        position = checked_add(position, 2)?;
        position = checked_add(position, instruction_length)?;
        checked_range(bytes, position, 0)?;

        let mut flags = Vec::with_capacity(point_count);
        while flags.len() < point_count {
            let flag = *bytes
                .get(position)
                .ok_or_else(|| Error::damaged("truncated glyph flags"))?;
            position = checked_add(position, 1)?;
            flags.push(flag);
            if flag & 0x08 != 0 {
                let repeat = usize::from(
                    *bytes
                        .get(position)
                        .ok_or_else(|| Error::damaged("truncated glyph flag repeat"))?,
                );
                position = checked_add(position, 1)?;
                let total = checked_add(flags.len(), repeat)?;
                if total > point_count {
                    return Err(Error::damaged("glyph flag repeat exceeds point count"));
                }
                let mut repeated = 0_usize;
                while repeated < repeat {
                    flags.push(flag);
                    repeated = checked_add(repeated, 1)?;
                }
            }
        }

        let mut xs = Vec::with_capacity(point_count);
        let mut x = 0_i32;
        for flag in flags.iter().copied() {
            let delta = if flag & 0x02 != 0 {
                let byte = i32::from(
                    *bytes
                        .get(position)
                        .ok_or_else(|| Error::damaged("truncated glyph x coordinate"))?,
                );
                position = checked_add(position, 1)?;
                if flag & 0x10 != 0 {
                    byte
                } else {
                    byte.checked_neg().ok_or_else(|| Error::damaged("x delta overflow"))?
                }
            } else if flag & 0x10 != 0 {
                0
            } else {
                let value = i32::from(be_i16_at(bytes, position)?);
                position = checked_add(position, 2)?;
                value
            };
            x = x
                .checked_add(delta)
                .ok_or_else(|| Error::damaged("glyph x coordinate overflow"))?;
            xs.push(x);
        }

        let mut ys = Vec::with_capacity(point_count);
        let mut y = 0_i32;
        for flag in flags.iter().copied() {
            let delta = if flag & 0x04 != 0 {
                let byte = i32::from(
                    *bytes
                        .get(position)
                        .ok_or_else(|| Error::damaged("truncated glyph y coordinate"))?,
                );
                position = checked_add(position, 1)?;
                if flag & 0x20 != 0 {
                    byte
                } else {
                    byte.checked_neg().ok_or_else(|| Error::damaged("y delta overflow"))?
                }
            } else if flag & 0x20 != 0 {
                0
            } else {
                let value = i32::from(be_i16_at(bytes, position)?);
                position = checked_add(position, 2)?;
                value
            };
            y = y
                .checked_add(delta)
                .ok_or_else(|| Error::damaged("glyph y coordinate overflow"))?;
            ys.push(y);
        }

        let mut points = Vec::with_capacity(point_count);
        let mut point_index = 0_usize;
        while point_index < point_count {
            let px = *xs
                .get(point_index)
                .ok_or_else(|| Error::damaged("missing glyph x point"))?;
            let py = *ys
                .get(point_index)
                .ok_or_else(|| Error::damaged("missing glyph y point"))?;
            let flag = *flags
                .get(point_index)
                .ok_or_else(|| Error::damaged("missing glyph flag"))?;
            points.push(Point {
                x: i32_to_f32(px)?,
                y: i32_to_f32(py)?,
                on_curve: flag & 1 != 0,
            });
            point_index = checked_add(point_index, 1)?;
        }

        self.apply_gvar_simple(glyph, &mut points, &ends)?;

        let mut first = 0_usize;
        for end in ends {
            let last = usize::from(end);
            let after = checked_add(last, 1)?;
            let contour = points
                .get(first..after)
                .ok_or_else(|| Error::damaged("glyph contour range is invalid"))?;
            emit_quadratic_contour(contour, sink, transform)?;
            first = after;
        }
        Ok(())
    }

    fn outline_composite(
        &self,
        parent_glyph: u16,
        bytes: &[u8],
        sink: &mut impl OutlineSink,
        transform: Transform,
        depth: usize,
    ) -> Result<()> {
        let mut position = 10_usize;
        let next_depth = checked_add(depth, 1)?;
        let mut components = Vec::new();
        let final_flags = loop {
            if components.len() >= MAX_POINTS {
                return Err(Error::damaged("composite component count exceeds limit"));
            }
            let flags = be_u16_at(bytes, position)?;
            let glyph = be_u16_at(bytes, checked_add(position, 2)?)?;
            position = checked_add(position, 4)?;

            let words = flags & 0x0001 != 0;
            let xy_values = flags & 0x0002 != 0;
            let (arg1, arg2) = if words {
                let a = be_i16_at(bytes, position)?;
                let b = be_i16_at(bytes, checked_add(position, 2)?)?;
                position = checked_add(position, 4)?;
                (a, b)
            } else {
                let a = i16::from(i8::from_be_bytes([*bytes
                    .get(position)
                    .ok_or_else(|| Error::damaged("short composite args"))?]));
                let b_pos = checked_add(position, 1)?;
                let b = i16::from(i8::from_be_bytes([*bytes
                    .get(b_pos)
                    .ok_or_else(|| Error::damaged("short composite args"))?]));
                position = checked_add(position, 2)?;
                (a, b)
            };
            if !xy_values {
                return Err(Error::Refused(
                    "point-matched composite glyph placement is not implemented".to_owned(),
                ));
            }

            let mut child = Transform::identity();
            child.dx = f32::from(arg1);
            child.dy = f32::from(arg2);
            if flags & 0x0008 != 0 {
                let scale = f2dot14(be_i16_at(bytes, position)?);
                position = checked_add(position, 2)?;
                child.xx = scale;
                child.yy = scale;
            } else if flags & 0x0040 != 0 {
                child.xx = f2dot14(be_i16_at(bytes, position)?);
                child.yy = f2dot14(be_i16_at(bytes, checked_add(position, 2)?)?);
                position = checked_add(position, 4)?;
            } else if flags & 0x0080 != 0 {
                child.xx = f2dot14(be_i16_at(bytes, position)?);
                child.yx = f2dot14(be_i16_at(bytes, checked_add(position, 2)?)?);
                child.xy = f2dot14(be_i16_at(bytes, checked_add(position, 4)?)?);
                child.yy = f2dot14(be_i16_at(bytes, checked_add(position, 6)?)?);
                position = checked_add(position, 8)?;
            }
            components.push(Component {
                glyph,
                transform: child,
            });
            if flags & 0x0020 == 0 {
                break flags;
            }
        };

        if final_flags & 0x0100 != 0 {
            let instruction_length = usize::from(be_u16_at(bytes, position)?);
            position = checked_add(position, 2)?;
            checked_range(bytes, position, instruction_length)?;
        }

        self.apply_gvar_composite(parent_glyph, &mut components)?;
        for component in components {
            self.outline_glyf(
                component.glyph,
                sink,
                transform.combine(component.transform),
                next_depth,
            )?;
        }
        Ok(())
    }

    fn apply_gvar_simple(&self, glyph: u16, points: &mut [Point], ends: &[u16]) -> Result<()> {
        if self.variation.is_none() || points.is_empty() {
            return Ok(());
        }
        let base: Vec<(f32, f32)> = points.iter().map(|point| (point.x, point.y)).collect();
        let Some(deltas) = self.gvar_deltas(glyph, &base, Some(ends))? else {
            return Ok(());
        };
        for (point, delta) in points.iter_mut().zip(deltas) {
            point.x += delta.0;
            point.y += delta.1;
        }
        Ok(())
    }

    fn apply_gvar_composite(&self, glyph: u16, components: &mut [Component]) -> Result<()> {
        if self.variation.is_none() || components.is_empty() {
            return Ok(());
        }
        let base: Vec<(f32, f32)> = components
            .iter()
            .map(|component| (component.transform.dx, component.transform.dy))
            .collect();
        let Some(deltas) = self.gvar_deltas(glyph, &base, None)? else {
            return Ok(());
        };
        for (component, delta) in components.iter_mut().zip(deltas) {
            component.transform.dx += delta.0;
            component.transform.dy += delta.1;
        }
        Ok(())
    }

    fn gvar_deltas(
        &self,
        glyph: u16,
        base_points: &[(f32, f32)],
        contour_ends: Option<&[u16]>,
    ) -> Result<Option<Vec<(f32, f32)>>> {
        let Some(variation) = &self.variation else {
            return Ok(None);
        };
        if variation.coords.iter().all(|coord| coord.abs() <= f32::EPSILON) {
            return Ok(None);
        }
        let glyph_index = usize::from(glyph);
        if glyph_index >= variation.glyph_count {
            return Err(Error::damaged("gvar glyph is outside variation table"));
        }
        let start_rel = gvar_glyph_offset(self.data, variation, glyph_index)?;
        let end_rel = gvar_glyph_offset(self.data, variation, checked_add(glyph_index, 1)?)?;
        if end_rel < start_rel {
            return Err(Error::damaged("gvar glyph offsets are decreasing"));
        }
        if start_rel == end_rel {
            return Ok(None);
        }
        let start = checked_add(variation.data_offset, start_rel)?;
        let end = checked_add(variation.data_offset, end_rel)?;
        let table_end = checked_add(variation.gvar.offset, variation.gvar.length)?;
        if start < variation.gvar.offset || end > table_end {
            return Err(Error::damaged("gvar glyph data exceeds table"));
        }
        let tuple_word = be_u16_at(self.data, start)?;
        let tuple_count = usize::from(tuple_word & 0x0fff);
        if tuple_count > MAX_GVAR_TUPLES {
            return Err(Error::damaged("gvar tuple count exceeds limit"));
        }
        let data_rel = usize::from(be_u16_at(self.data, checked_add(start, 2)?)?);
        let serialized_start = checked_add(start, data_rel)?;
        if serialized_start > end {
            return Err(Error::damaged("gvar serialized data starts outside glyph data"));
        }

        #[derive(Clone)]
        struct TupleHeader {
            data_size: usize,
            peak: Vec<f32>,
            start: Option<Vec<f32>>,
            end: Option<Vec<f32>>,
            private_points: bool,
        }

        let mut headers = Vec::with_capacity(tuple_count);
        let mut header_pos = checked_add(start, 4)?;
        for _ in 0..tuple_count {
            let data_size = usize::from(be_u16_at(self.data, header_pos)?);
            let tuple_index = be_u16_at(self.data, checked_add(header_pos, 2)?)?;
            header_pos = checked_add(header_pos, 4)?;
            let peak = if tuple_index & 0x8000 != 0 {
                let (tuple, next) = read_tuple_coords(self.data, header_pos, variation.axis_count, end)?;
                header_pos = next;
                tuple
            } else {
                let shared = usize::from(tuple_index & 0x0fff);
                read_shared_tuple(self.data, variation, shared)?
            };
            let (intermediate_start, intermediate_end) = if tuple_index & 0x4000 != 0 {
                let (a, next) = read_tuple_coords(self.data, header_pos, variation.axis_count, end)?;
                let (b, next2) = read_tuple_coords(self.data, next, variation.axis_count, end)?;
                header_pos = next2;
                (Some(a), Some(b))
            } else {
                (None, None)
            };
            headers.push(TupleHeader {
                data_size,
                peak,
                start: intermediate_start,
                end: intermediate_end,
                private_points: tuple_index & 0x2000 != 0,
            });
        }
        if header_pos > serialized_start {
            return Err(Error::damaged("gvar tuple headers overlap serialized data"));
        }

        let total_points = checked_add(base_points.len(), 4)?;
        let (shared_points, mut tuple_data) = if tuple_word & 0x8000 != 0 {
            decode_packed_points(self.data, serialized_start, total_points, end)?
        } else {
            (None, serialized_start)
        };
        let mut accumulated = vec![(0.0_f32, 0.0_f32); total_points];
        for header in headers {
            let tuple_end = checked_add(tuple_data, header.data_size)?;
            if tuple_end > end {
                return Err(Error::damaged("gvar tuple data exceeds glyph data"));
            }
            let scalar = tuple_scalar(
                &variation.coords,
                &header.peak,
                header.start.as_deref(),
                header.end.as_deref(),
            )?;
            let (points, deltas_start) = if header.private_points {
                decode_packed_points(self.data, tuple_data, total_points, tuple_end)?
            } else {
                (shared_points.clone(), tuple_data)
            };
            let selected_count = points.as_ref().map_or(total_points, Vec::len);
            let (xs, after_x) = decode_packed_deltas(self.data, deltas_start, selected_count, tuple_end)?;
            let (ys, after_y) = decode_packed_deltas(self.data, after_x, selected_count, tuple_end)?;
            if after_y > tuple_end {
                return Err(Error::damaged("gvar packed deltas exceed tuple"));
            }
            if scalar != 0.0 {
                let mut dx = vec![None; total_points];
                let mut dy = vec![None; total_points];
                if let Some(indices) = &points {
                    for (slot, point_index) in indices.iter().copied().enumerate() {
                        let x = *xs.get(slot).ok_or_else(|| Error::damaged("gvar x delta missing"))?;
                        let y = *ys.get(slot).ok_or_else(|| Error::damaged("gvar y delta missing"))?;
                        *dx.get_mut(point_index)
                            .ok_or_else(|| Error::damaged("gvar point index outside glyph"))? = Some(x);
                        *dy.get_mut(point_index)
                            .ok_or_else(|| Error::damaged("gvar point index outside glyph"))? = Some(y);
                    }
                } else {
                    for index in 0..total_points {
                        *dx.get_mut(index).ok_or_else(|| Error::damaged("gvar x slot missing"))? =
                            xs.get(index).copied();
                        *dy.get_mut(index).ok_or_else(|| Error::damaged("gvar y slot missing"))? =
                            ys.get(index).copied();
                    }
                }
                if let Some(ends) = contour_ends {
                    iup_fill(&mut dx, base_points, ends, true)?;
                    iup_fill(&mut dy, base_points, ends, false)?;
                }
                for index in 0..total_points {
                    let x = dx.get(index).and_then(|value| *value).unwrap_or(0.0) * scalar;
                    let y = dy.get(index).and_then(|value| *value).unwrap_or(0.0) * scalar;
                    let slot = accumulated
                        .get_mut(index)
                        .ok_or_else(|| Error::damaged("gvar accumulator missing"))?;
                    slot.0 += x;
                    slot.1 += y;
                }
            }
            tuple_data = tuple_end;
        }
        accumulated.truncate(base_points.len());
        Ok(Some(accumulated))
    }
}

fn parse_cff(data: &[u8], table: Table, glyph_count: usize) -> Result<CffState> {
    let table_end = checked_add(table.offset, table.length)?;
    if table.length < 4 {
        return Err(Error::damaged("short CFF header"));
    }
    let major = *data
        .get(table.offset)
        .ok_or_else(|| Error::damaged("CFF major missing"))?;
    let header_size = usize::from(
        *data
            .get(checked_add(table.offset, 2)?)
            .ok_or_else(|| Error::damaged("CFF header size missing"))?,
    );
    if major != 1 || header_size < 4 || checked_add(table.offset, header_size)? > table_end {
        return Err(Error::damaged("unsupported CFF header"));
    }
    let mut position = checked_add(table.offset, header_size)?;
    let (_, next) = parse_cff_index(data, position, table_end)?;
    position = next;
    let (top_index, next) = parse_cff_index(data, position, table_end)?;
    position = next;
    if top_index.count != 1 {
        return Err(Error::damaged("CFF must contain exactly one Top DICT"));
    }
    let (_, next) = parse_cff_index(data, position, table_end)?;
    position = next;
    let (global_subrs, _) = parse_cff_index(data, position, table_end)?;
    let top = top_index.object(data, 0)?;
    let charstrings_rel = dict_ints(top, 17, None)?
        .and_then(|values| values.first().copied())
        .ok_or_else(|| Error::damaged("CFF Top DICT has no CharStrings"))?;
    let charstrings_at = cff_rel_offset(table.offset, charstrings_rel, table_end)?;
    let (char_strings, _) = parse_cff_index(data, charstrings_at, table_end)?;
    if char_strings.count != glyph_count {
        return Err(Error::damaged("CFF CharStrings count disagrees with maxp"));
    }

    let private = parse_private_from_dict(data, top, table.offset, table_end)?;
    let fd_array_rel = dict_ints(top, 12, Some(36))?.and_then(|values| values.first().copied());
    let fd_select_rel = dict_ints(top, 12, Some(37))?.and_then(|values| values.first().copied());
    let mut fd_array = Vec::new();
    if let Some(rel) = fd_array_rel {
        let at = cff_rel_offset(table.offset, rel, table_end)?;
        let (index, _) = parse_cff_index(data, at, table_end)?;
        if index.count > MAX_CFF_FDS {
            return Err(Error::damaged("CFF FDArray exceeds limit"));
        }
        fd_array.reserve(index.count);
        for fd in 0..index.count {
            let dict = index.object(data, fd)?;
            fd_array.push(
                parse_private_from_dict(data, dict, table.offset, table_end)?
                    .unwrap_or(CffPrivate { local_subrs: None }),
            );
        }
    }
    let fd_select_offset = fd_select_rel
        .map(|rel| cff_rel_offset(table.offset, rel, table_end))
        .transpose()?;
    if fd_select_offset.is_some() != !fd_array.is_empty() {
        return Err(Error::damaged("CFF CID font needs both FDArray and FDSelect"));
    }
    if let Some(offset) = fd_select_offset {
        validate_fd_select(data, offset, table_end, glyph_count, fd_array.len())?;
    }
    Ok(CffState {
        char_strings,
        global_subrs,
        private,
        fd_array,
        fd_select_offset,
        fd_select_end: table_end,
    })
}

fn parse_cff_index(data: &[u8], offset: usize, limit: usize) -> Result<(CffIndex, usize)> {
    if checked_add(offset, 2)? > limit {
        return Err(Error::damaged("CFF INDEX count is truncated"));
    }
    let count = usize::from(be_u16_at(data, offset)?);
    if count == 0 {
        let end = checked_add(offset, 2)?;
        return Ok((
            CffIndex {
                count: 0,
                offsets: end,
                data: end,
                end,
                off_size: 1,
            },
            end,
        ));
    }
    let off_size_at = checked_add(offset, 2)?;
    let off_size = usize::from(
        *data
            .get(off_size_at)
            .ok_or_else(|| Error::damaged("CFF INDEX offSize missing"))?,
    );
    if !(1..=4).contains(&off_size) {
        return Err(Error::damaged("CFF INDEX offSize outside 1..=4"));
    }
    let offsets = checked_add(offset, 3)?;
    let offset_bytes = checked_mul(checked_add(count, 1)?, off_size)?;
    let data_start = checked_add(offsets, offset_bytes)?;
    if data_start > limit {
        return Err(Error::damaged("CFF INDEX offsets are truncated"));
    }
    let first = cff_read_offset(data, offsets, off_size)?;
    let last_at = checked_add(offsets, checked_mul(count, off_size)?)?;
    let last = cff_read_offset(data, last_at, off_size)?;
    if first != 1 || last == 0 {
        return Err(Error::damaged("CFF INDEX offsets are invalid"));
    }
    let last_offset = last
        .checked_sub(1)
        .ok_or_else(|| Error::damaged("CFF INDEX final offset underflow"))?;
    let end = checked_add(data_start, last_offset)?;
    if end > limit {
        return Err(Error::damaged("CFF INDEX data exceeds table"));
    }
    let index = CffIndex {
        count,
        offsets,
        data: data_start,
        end,
        off_size,
    };
    let mut previous = 1_usize;
    for item in 0..=count {
        let value = cff_read_offset(data, checked_add(offsets, checked_mul(item, off_size)?)?, off_size)?;
        if value < previous || value > last {
            return Err(Error::damaged("CFF INDEX offsets are not monotonic"));
        }
        previous = value;
    }
    Ok((index, end))
}

fn cff_read_offset(data: &[u8], position: usize, size: usize) -> Result<usize> {
    if !(1..=4).contains(&size) {
        return Err(Error::damaged("invalid CFF offset width"));
    }
    let bytes = checked_range(data, position, size)?;
    let mut value = 0_usize;
    for byte in bytes {
        value = value
            .checked_shl(8)
            .and_then(|v| v.checked_add(usize::from(*byte)))
            .ok_or_else(|| Error::damaged("CFF offset overflow"))?;
    }
    Ok(value)
}

fn cff_rel_offset(base: usize, value: i32, limit: usize) -> Result<usize> {
    let relative = usize::try_from(value).map_err(|_| Error::damaged("negative CFF offset"))?;
    let absolute = checked_add(base, relative)?;
    if absolute > limit {
        return Err(Error::damaged("CFF offset exceeds table"));
    }
    Ok(absolute)
}

fn dict_ints(bytes: &[u8], operator: u8, escaped: Option<u8>) -> Result<Option<Vec<i32>>> {
    let mut position = 0_usize;
    let mut stack = Vec::<i32>::new();
    while position < bytes.len() {
        let byte = *bytes
            .get(position)
            .ok_or_else(|| Error::damaged("CFF DICT byte missing"))?;
        if byte <= 21 {
            position = checked_add(position, 1)?;
            let (first, second) = if byte == 12 {
                let second = *bytes
                    .get(position)
                    .ok_or_else(|| Error::damaged("CFF escaped operator truncated"))?;
                position = checked_add(position, 1)?;
                (12, Some(second))
            } else {
                (byte, None)
            };
            if first == operator && second == escaped {
                return Ok(Some(stack));
            }
            stack.clear();
            continue;
        }
        let (number, next) = cff_dict_number(bytes, position)?;
        position = next;
        stack.push(number);
        if stack.len() > MAX_CFF_STACK {
            return Err(Error::damaged("CFF DICT operand stack exceeds limit"));
        }
    }
    Ok(None)
}

fn cff_dict_number(bytes: &[u8], position: usize) -> Result<(i32, usize)> {
    let b0 = *bytes
        .get(position)
        .ok_or_else(|| Error::damaged("CFF DICT number missing"))?;
    let mut next = checked_add(position, 1)?;
    let value = match b0 {
        28 => {
            let value = i32::from(be_i16_at(bytes, next)?);
            next = checked_add(next, 2)?;
            value
        }
        29 => {
            let value = be_i32_at(bytes, next)?;
            next = checked_add(next, 4)?;
            value
        }
        30 => {
            loop {
                let byte = *bytes
                    .get(next)
                    .ok_or_else(|| Error::damaged("CFF real number truncated"))?;
                next = checked_add(next, 1)?;
                if byte & 0x0f == 0x0f || byte >> 4 == 0x0f {
                    break;
                }
            }
            0
        }
        32..=246 => i32::from(b0)
            .checked_sub(139)
            .ok_or_else(|| Error::damaged("CFF DICT integer underflow"))?,
        247..=250 => {
            let b1 = i32::from(
                *bytes
                    .get(next)
                    .ok_or_else(|| Error::damaged("CFF DICT positive number truncated"))?,
            );
            next = checked_add(next, 1)?;
            i32::from(b0)
                .checked_sub(247)
                .and_then(|value| value.checked_mul(256))
                .and_then(|value| value.checked_add(b1))
                .and_then(|value| value.checked_add(108))
                .ok_or_else(|| Error::damaged("CFF DICT positive integer overflow"))?
        }
        251..=254 => {
            let b1 = i32::from(
                *bytes
                    .get(next)
                    .ok_or_else(|| Error::damaged("CFF DICT negative number truncated"))?,
            );
            next = checked_add(next, 1)?;
            i32::from(b0)
                .checked_sub(251)
                .and_then(|value| value.checked_mul(256))
                .and_then(i32::checked_neg)
                .and_then(|value| value.checked_sub(b1))
                .and_then(|value| value.checked_sub(108))
                .ok_or_else(|| Error::damaged("CFF DICT negative integer overflow"))?
        }
        _ => return Err(Error::damaged("invalid CFF DICT number")),
    };
    Ok((value, next))
}

fn parse_private_from_dict(data: &[u8], dict: &[u8], cff_base: usize, table_end: usize) -> Result<Option<CffPrivate>> {
    let Some(values) = dict_ints(dict, 18, None)? else {
        return Ok(None);
    };
    if values.len() < 2 {
        return Err(Error::damaged("CFF Private operator needs size and offset"));
    }
    let size = usize::try_from(
        *values
            .first()
            .ok_or_else(|| Error::damaged("CFF Private size missing"))?,
    )
    .map_err(|_| Error::damaged("negative CFF Private size"))?;
    let private_offset = *values
        .get(1)
        .ok_or_else(|| Error::damaged("CFF Private offset missing"))?;
    let offset = cff_rel_offset(cff_base, private_offset, table_end)?;
    let end = checked_add(offset, size)?;
    if end > table_end {
        return Err(Error::damaged("CFF Private DICT exceeds table"));
    }
    let bytes = checked_range(data, offset, size)?;
    let local_subrs = if let Some(values) = dict_ints(bytes, 19, None)? {
        let rel = *values
            .first()
            .ok_or_else(|| Error::damaged("CFF Subrs offset missing"))?;
        let rel = usize::try_from(rel).map_err(|_| Error::damaged("negative CFF Subrs offset"))?;
        let at = checked_add(offset, rel)?;
        let (index, _) = parse_cff_index(data, at, table_end)?;
        Some(index)
    } else {
        None
    };
    Ok(Some(CffPrivate { local_subrs }))
}

fn validate_fd_select(data: &[u8], offset: usize, limit: usize, glyph_count: usize, fd_count: usize) -> Result<()> {
    let format = *data
        .get(offset)
        .ok_or_else(|| Error::damaged("CFF FDSelect format missing"))?;
    match format {
        0 => {
            let start = checked_add(offset, 1)?;
            let bytes = checked_range(data, start, glyph_count)?;
            if bytes.iter().any(|fd| usize::from(*fd) >= fd_count) {
                return Err(Error::damaged("CFF FDSelect references absent FD"));
            }
        }
        3 => {
            let ranges = usize::from(be_u16_at(data, checked_add(offset, 1)?)?);
            if ranges == 0 {
                return Err(Error::damaged("CFF FDSelect format 3 has no ranges"));
            }
            let mut position = checked_add(offset, 3)?;
            let mut previous = 0_usize;
            for range in 0..ranges {
                let first = usize::from(be_u16_at(data, position)?);
                let fd = usize::from(
                    *data
                        .get(checked_add(position, 2)?)
                        .ok_or_else(|| Error::damaged("CFF FDSelect range truncated"))?,
                );
                position = checked_add(position, 3)?;
                if (range == 0 && first != 0) || (range > 0 && first <= previous) || fd >= fd_count {
                    return Err(Error::damaged("invalid CFF FDSelect range"));
                }
                previous = first;
            }
            let sentinel = usize::from(be_u16_at(data, position)?);
            position = checked_add(position, 2)?;
            if sentinel != glyph_count || position > limit {
                return Err(Error::damaged("CFF FDSelect sentinel disagrees with glyph count"));
            }
        }
        _ => return Err(Error::Refused("unsupported CFF FDSelect format".to_owned())),
    }
    Ok(())
}

fn cff_fd_for_glyph(data: &[u8], cff: &CffState, glyph: usize) -> Result<Option<usize>> {
    let Some(offset) = cff.fd_select_offset else {
        return Ok(None);
    };
    if offset >= cff.fd_select_end {
        return Err(Error::damaged("CFF FDSelect offset exceeds table"));
    }
    let format = *data
        .get(offset)
        .ok_or_else(|| Error::damaged("CFF FDSelect format missing"))?;
    match format {
        0 => Ok(Some(usize::from(
            *data
                .get(checked_add(checked_add(offset, 1)?, glyph)?)
                .ok_or_else(|| Error::damaged("CFF FDSelect glyph missing"))?,
        ))),
        3 => {
            let ranges = usize::from(be_u16_at(data, checked_add(offset, 1)?)?);
            let mut position = checked_add(offset, 3)?;
            for range in 0..ranges {
                let first = usize::from(be_u16_at(data, position)?);
                let fd = usize::from(
                    *data
                        .get(checked_add(position, 2)?)
                        .ok_or_else(|| Error::damaged("CFF FDSelect range truncated"))?,
                );
                let next_range = checked_add(range, 1)?;
                let next = if next_range < ranges {
                    usize::from(be_u16_at(data, checked_add(position, 3)?)?)
                } else {
                    usize::from(be_u16_at(
                        data,
                        checked_add(checked_add(offset, 3)?, checked_mul(ranges, 3)?)?,
                    )?)
                };
                if glyph >= first && glyph < next {
                    return Ok(Some(fd));
                }
                position = checked_add(position, 3)?;
            }
            Err(Error::damaged("CFF FDSelect has no range for glyph"))
        }
        _ => Err(Error::Refused("unsupported CFF FDSelect format".to_owned())),
    }
}

fn outline_cff(data: &[u8], cff: &CffState, glyph: u16, sink: &mut impl OutlineSink) -> Result<()> {
    let glyph_index = usize::from(glyph);
    let charstring = cff.char_strings.object(data, glyph_index)?;
    let local_subrs = if let Some(fd) = cff_fd_for_glyph(data, cff, glyph_index)? {
        cff.fd_array
            .get(fd)
            .ok_or_else(|| Error::damaged("CFF FD index outside FDArray"))?
            .local_subrs
    } else {
        cff.private.as_ref().and_then(|private| private.local_subrs)
    };
    let mut interpreter = Type2Interpreter {
        data,
        global_subrs: cff.global_subrs,
        local_subrs,
        sink,
        stack: Vec::with_capacity(MAX_CFF_STACK),
        x: 0.0,
        y: 0.0,
        hints: 0,
        contour_open: false,
    };
    let result = interpreter.run(charstring, 0)?;
    if result == Type2Exit::Return {
        return Err(Error::damaged("top-level CFF charstring returned from subroutine"));
    }
    if interpreter.contour_open {
        interpreter.sink.close();
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Type2Exit {
    End,
    Return,
}

struct Type2Interpreter<'a, 'b, S: OutlineSink> {
    data: &'a [u8],
    global_subrs: CffIndex,
    local_subrs: Option<CffIndex>,
    sink: &'b mut S,
    stack: Vec<f32>,
    x: f32,
    y: f32,
    hints: usize,
    contour_open: bool,
}

impl<S: OutlineSink> Type2Interpreter<'_, '_, S> {
    fn run(&mut self, code: &[u8], depth: usize) -> Result<Type2Exit> {
        if depth > MAX_CFF_SUBR_DEPTH {
            return Err(Error::damaged("CFF subroutine depth exceeds limit"));
        }
        let mut position = 0_usize;
        while position < code.len() {
            let byte = *code
                .get(position)
                .ok_or_else(|| Error::damaged("CFF charstring byte missing"))?;
            if byte >= 32 || byte == 28 || byte == 255 {
                let (value, next) = type2_number(code, position)?;
                position = next;
                if self.stack.len() >= MAX_CFF_STACK {
                    return Err(Error::damaged("CFF charstring stack exceeds limit"));
                }
                self.stack.push(value);
                continue;
            }
            position = checked_add(position, 1)?;
            match byte {
                1 | 3 | 18 | 23 => self.consume_stems()?,
                4 => {
                    self.drop_optional_width(1)?;
                    let dy = self.only_arg()?;
                    self.move_to(0.0, dy);
                }
                5 => self.rlineto()?,
                6 => self.hv_lines(true)?,
                7 => self.hv_lines(false)?,
                8 => self.rrcurveto()?,
                10 => {
                    let raw = self.pop_int()?;
                    let local = self
                        .local_subrs
                        .ok_or_else(|| Error::damaged("CFF callsubr without local Subrs"))?;
                    let index = biased_subr_index(raw, local.count)?;
                    let subr = local.object(self.data, index)?;
                    if self.run(subr, checked_add(depth, 1)?)? == Type2Exit::End {
                        return Ok(Type2Exit::End);
                    }
                }
                11 => return Ok(Type2Exit::Return),
                12 => {
                    let op = *code
                        .get(position)
                        .ok_or_else(|| Error::damaged("CFF escaped operator truncated"))?;
                    position = checked_add(position, 1)?;
                    self.escape(op)?;
                }
                14 => {
                    self.stack.clear();
                    return Ok(Type2Exit::End);
                }
                19 | 20 => {
                    self.consume_stems()?;
                    let mask = checked_add(self.hints, 7)? / 8;
                    position = checked_add(position, mask)?;
                    if position > code.len() {
                        return Err(Error::damaged("CFF hint mask is truncated"));
                    }
                }
                21 => {
                    self.drop_optional_width(2)?;
                    let args = self.take_args()?;
                    self.move_to(cff_arg(&args, 0)?, cff_arg(&args, 1)?);
                }
                22 => {
                    self.drop_optional_width(1)?;
                    let dx = self.only_arg()?;
                    self.move_to(dx, 0.0);
                }
                24 => self.rcurveline()?,
                25 => self.rlinecurve()?,
                26 => self.vvcurveto()?,
                27 => self.hhcurveto()?,
                29 => {
                    let raw = self.pop_int()?;
                    let index = biased_subr_index(raw, self.global_subrs.count)?;
                    let subr = self.global_subrs.object(self.data, index)?;
                    if self.run(subr, checked_add(depth, 1)?)? == Type2Exit::End {
                        return Ok(Type2Exit::End);
                    }
                }
                30 => self.hvcurveto(false)?,
                31 => self.hvcurveto(true)?,
                _ => return Err(Error::Refused(format!("unsupported CFF Type 2 operator {byte}"))),
            }
        }
        Ok(Type2Exit::End)
    }

    fn consume_stems(&mut self) -> Result<()> {
        if self.stack.len() % 2 == 1 {
            self.stack.remove(0);
        }
        if self.stack.len() % 2 != 0 {
            return Err(Error::damaged("CFF stem operands are not pairs"));
        }
        self.hints = checked_add(self.hints, self.stack.len() / 2)?;
        self.stack.clear();
        Ok(())
    }

    fn drop_optional_width(&mut self, required: usize) -> Result<()> {
        if self.stack.len() == checked_add(required, 1)? {
            self.stack.remove(0);
        }
        if self.stack.len() != required {
            return Err(Error::damaged("CFF moveto operand count is invalid"));
        }
        Ok(())
    }

    fn only_arg(&mut self) -> Result<f32> {
        if self.stack.len() != 1 {
            return Err(Error::damaged("CFF operator expects one operand"));
        }
        self.stack.pop().ok_or_else(|| Error::damaged("CFF operand missing"))
    }

    fn take_args(&mut self) -> Result<Vec<f32>> {
        Ok(core::mem::take(&mut self.stack))
    }

    #[allow(clippy::cast_possible_truncation)]
    fn pop_int(&mut self) -> Result<i32> {
        let value = self
            .stack
            .pop()
            .ok_or_else(|| Error::damaged("CFF subroutine index missing"))?;
        if !value.is_finite() || value.fract() != 0.0 || value < i32::MIN as f32 || value > i32::MAX as f32 {
            return Err(Error::damaged("CFF subroutine index is not an integer"));
        }
        Ok(value as i32)
    }

    fn ensure_contour(&self) -> Result<()> {
        if self.contour_open {
            Ok(())
        } else {
            Err(Error::damaged("CFF drawing operator before moveto"))
        }
    }

    fn move_to(&mut self, dx: f32, dy: f32) {
        if self.contour_open {
            self.sink.close();
        }
        self.x += dx;
        self.y += dy;
        self.sink.move_to(self.x, self.y);
        self.contour_open = true;
        self.stack.clear();
    }

    fn line_by(&mut self, dx: f32, dy: f32) -> Result<()> {
        self.ensure_contour()?;
        self.x += dx;
        self.y += dy;
        self.sink.line_to(self.x, self.y);
        Ok(())
    }

    fn curve_by(&mut self, d1: (f32, f32), d2: (f32, f32), d3: (f32, f32)) -> Result<()> {
        self.ensure_contour()?;
        let c1 = (self.x + d1.0, self.y + d1.1);
        let c2 = (c1.0 + d2.0, c1.1 + d2.1);
        self.x = c2.0 + d3.0;
        self.y = c2.1 + d3.1;
        self.sink.cubic_to(c1.0, c1.1, c2.0, c2.1, self.x, self.y);
        Ok(())
    }

    fn rlineto(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 2 || args.len() % 2 != 0 {
            return Err(Error::damaged("CFF rlineto operands invalid"));
        }
        for pair in args.chunks_exact(2) {
            self.line_by(cff_arg(pair, 0)?, cff_arg(pair, 1)?)?;
        }
        Ok(())
    }
    fn hv_lines(&mut self, horizontal: bool) -> Result<()> {
        let args = self.take_args()?;
        if args.is_empty() {
            return Err(Error::damaged("CFF line operands missing"));
        }
        let mut h = horizontal;
        for value in args {
            if h {
                self.line_by(value, 0.0)?;
            } else {
                self.line_by(0.0, value)?;
            }
            h = !h;
        }
        Ok(())
    }
    fn rrcurveto(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 6 || args.len() % 6 != 0 {
            return Err(Error::damaged("CFF rrcurveto operands invalid"));
        }
        for c in args.chunks_exact(6) {
            self.curve_by(
                (cff_arg(c, 0)?, cff_arg(c, 1)?),
                (cff_arg(c, 2)?, cff_arg(c, 3)?),
                (cff_arg(c, 4)?, cff_arg(c, 5)?),
            )?;
        }
        Ok(())
    }
    fn rcurveline(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 8 || args.len().saturating_sub(2) % 6 != 0 {
            return Err(Error::damaged("CFF rcurveline operands invalid"));
        }
        let split = args
            .len()
            .checked_sub(2)
            .ok_or_else(|| Error::damaged("CFF rcurveline split underflow"))?;
        let curves = args
            .get(..split)
            .ok_or_else(|| Error::damaged("CFF rcurveline range invalid"))?;
        for c in curves.chunks_exact(6) {
            self.curve_by(
                (cff_arg(c, 0)?, cff_arg(c, 1)?),
                (cff_arg(c, 2)?, cff_arg(c, 3)?),
                (cff_arg(c, 4)?, cff_arg(c, 5)?),
            )?;
        }
        self.line_by(cff_arg(&args, split)?, cff_arg(&args, checked_add(split, 1)?)?)
    }

    fn rlinecurve(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 8 || args.len().saturating_sub(6) % 2 != 0 {
            return Err(Error::damaged("CFF rlinecurve operands invalid"));
        }
        let split = args
            .len()
            .checked_sub(6)
            .ok_or_else(|| Error::damaged("CFF rlinecurve split underflow"))?;
        let lines = args
            .get(..split)
            .ok_or_else(|| Error::damaged("CFF rlinecurve line range invalid"))?;
        for l in lines.chunks_exact(2) {
            self.line_by(cff_arg(l, 0)?, cff_arg(l, 1)?)?;
        }
        let c = args
            .get(split..)
            .ok_or_else(|| Error::damaged("CFF rlinecurve curve range invalid"))?;
        self.curve_by(
            (cff_arg(c, 0)?, cff_arg(c, 1)?),
            (cff_arg(c, 2)?, cff_arg(c, 3)?),
            (cff_arg(c, 4)?, cff_arg(c, 5)?),
        )
    }

    fn hhcurveto(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 4 {
            return Err(Error::damaged("CFF hhcurveto operands missing"));
        }
        let mut index = 0_usize;
        let mut dy1 = 0.0;
        if args.len() % 4 == 1 {
            dy1 = cff_arg(&args, 0)?;
            index = 1;
        }
        while checked_add(index, 3)? < args.len() {
            self.curve_by(
                (cff_arg(&args, index)?, dy1),
                (
                    cff_arg(&args, checked_add(index, 1)?)?,
                    cff_arg(&args, checked_add(index, 2)?)?,
                ),
                (cff_arg(&args, checked_add(index, 3)?)?, 0.0),
            )?;
            dy1 = 0.0;
            index = checked_add(index, 4)?;
        }
        if index != args.len() {
            return Err(Error::damaged("CFF hhcurveto operands invalid"));
        }
        Ok(())
    }

    fn vvcurveto(&mut self) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 4 {
            return Err(Error::damaged("CFF vvcurveto operands missing"));
        }
        let mut index = 0_usize;
        let mut dx1 = 0.0;
        if args.len() % 4 == 1 {
            dx1 = cff_arg(&args, 0)?;
            index = 1;
        }
        while checked_add(index, 3)? < args.len() {
            self.curve_by(
                (dx1, cff_arg(&args, index)?),
                (
                    cff_arg(&args, checked_add(index, 1)?)?,
                    cff_arg(&args, checked_add(index, 2)?)?,
                ),
                (0.0, cff_arg(&args, checked_add(index, 3)?)?),
            )?;
            dx1 = 0.0;
            index = checked_add(index, 4)?;
        }
        if index != args.len() {
            return Err(Error::damaged("CFF vvcurveto operands invalid"));
        }
        Ok(())
    }

    fn hvcurveto(&mut self, starts_horizontal: bool) -> Result<()> {
        let args = self.take_args()?;
        if args.len() < 4 {
            return Err(Error::damaged("CFF hv/vhcurveto operands missing"));
        }
        let mut index = 0_usize;
        let mut horizontal = starts_horizontal;
        while checked_add(index, 3)? < args.len() {
            let remaining = args
                .len()
                .checked_sub(index)
                .ok_or_else(|| Error::damaged("CFF curve index overflow"))?;
            let extra = remaining == 5;
            let extra_delta = if extra {
                cff_arg(&args, checked_add(index, 4)?)?
            } else {
                0.0
            };
            if horizontal {
                self.curve_by(
                    (cff_arg(&args, index)?, 0.0),
                    (
                        cff_arg(&args, checked_add(index, 1)?)?,
                        cff_arg(&args, checked_add(index, 2)?)?,
                    ),
                    (extra_delta, cff_arg(&args, checked_add(index, 3)?)?),
                )?;
            } else {
                self.curve_by(
                    (0.0, cff_arg(&args, index)?),
                    (
                        cff_arg(&args, checked_add(index, 1)?)?,
                        cff_arg(&args, checked_add(index, 2)?)?,
                    ),
                    (cff_arg(&args, checked_add(index, 3)?)?, extra_delta),
                )?;
            }
            index = checked_add(index, if extra { 5 } else { 4 })?;
            horizontal = !horizontal;
        }
        if index != args.len() {
            return Err(Error::damaged("CFF hv/vhcurveto operands invalid"));
        }
        Ok(())
    }

    fn escape(&mut self, op: u8) -> Result<()> {
        let args = self.take_args()?;
        match op {
            34 => {
                if args.len() != 7 {
                    return Err(Error::damaged("CFF hflex operands invalid"));
                }
                let dy2 = cff_arg(&args, 2)?;
                self.curve_by(
                    (cff_arg(&args, 0)?, 0.0),
                    (cff_arg(&args, 1)?, dy2),
                    (cff_arg(&args, 3)?, 0.0),
                )?;
                self.curve_by(
                    (cff_arg(&args, 4)?, 0.0),
                    (cff_arg(&args, 5)?, -dy2),
                    (cff_arg(&args, 6)?, 0.0),
                )
            }
            35 => {
                if args.len() != 13 {
                    return Err(Error::damaged("CFF flex operands invalid"));
                }
                self.curve_by(
                    (cff_arg(&args, 0)?, cff_arg(&args, 1)?),
                    (cff_arg(&args, 2)?, cff_arg(&args, 3)?),
                    (cff_arg(&args, 4)?, cff_arg(&args, 5)?),
                )?;
                self.curve_by(
                    (cff_arg(&args, 6)?, cff_arg(&args, 7)?),
                    (cff_arg(&args, 8)?, cff_arg(&args, 9)?),
                    (cff_arg(&args, 10)?, cff_arg(&args, 11)?),
                )
            }
            36 => {
                if args.len() != 9 {
                    return Err(Error::damaged("CFF hflex1 operands invalid"));
                }
                let dy6 = -(cff_arg(&args, 1)? + cff_arg(&args, 3)? + cff_arg(&args, 7)?);
                self.curve_by(
                    (cff_arg(&args, 0)?, cff_arg(&args, 1)?),
                    (cff_arg(&args, 2)?, cff_arg(&args, 3)?),
                    (cff_arg(&args, 4)?, 0.0),
                )?;
                self.curve_by(
                    (cff_arg(&args, 5)?, 0.0),
                    (cff_arg(&args, 6)?, cff_arg(&args, 7)?),
                    (cff_arg(&args, 8)?, dy6),
                )
            }
            37 => {
                if args.len() != 11 {
                    return Err(Error::damaged("CFF flex1 operands invalid"));
                }
                let dx = cff_arg(&args, 0)?
                    + cff_arg(&args, 2)?
                    + cff_arg(&args, 4)?
                    + cff_arg(&args, 6)?
                    + cff_arg(&args, 8)?;
                let dy = cff_arg(&args, 1)?
                    + cff_arg(&args, 3)?
                    + cff_arg(&args, 5)?
                    + cff_arg(&args, 7)?
                    + cff_arg(&args, 9)?;
                let last = if dx.abs() > dy.abs() {
                    (cff_arg(&args, 10)?, -dy)
                } else {
                    (-dx, cff_arg(&args, 10)?)
                };
                self.curve_by(
                    (cff_arg(&args, 0)?, cff_arg(&args, 1)?),
                    (cff_arg(&args, 2)?, cff_arg(&args, 3)?),
                    (cff_arg(&args, 4)?, cff_arg(&args, 5)?),
                )?;
                self.curve_by(
                    (cff_arg(&args, 6)?, cff_arg(&args, 7)?),
                    (cff_arg(&args, 8)?, cff_arg(&args, 9)?),
                    last,
                )
            }
            _ => Err(Error::Refused(format!("unsupported CFF Type 2 escaped operator {op}"))),
        }
    }
}

fn cff_arg(args: &[f32], index: usize) -> Result<f32> {
    args.get(index)
        .copied()
        .ok_or_else(|| Error::damaged("CFF operand missing"))
}

fn biased_subr_index(raw: i32, count: usize) -> Result<usize> {
    let bias = if count < 1_240 {
        107_i32
    } else if count < 33_900 {
        1_131
    } else {
        32_768
    };
    let value = raw
        .checked_add(bias)
        .ok_or_else(|| Error::damaged("CFF subroutine index overflow"))?;
    let index = usize::try_from(value).map_err(|_| Error::damaged("negative CFF subroutine index"))?;
    if index >= count {
        return Err(Error::damaged("CFF subroutine index outside INDEX"));
    }
    Ok(index)
}

fn type2_number(bytes: &[u8], position: usize) -> Result<(f32, usize)> {
    let b0 = *bytes
        .get(position)
        .ok_or_else(|| Error::damaged("CFF Type 2 number missing"))?;
    let mut next = checked_add(position, 1)?;
    let value = match b0 {
        28 => {
            let v = f32::from(be_i16_at(bytes, next)?);
            next = checked_add(next, 2)?;
            v
        }
        32..=246 => f32::from(b0) - 139.0,
        247..=250 => {
            let b1 = f32::from(
                *bytes
                    .get(next)
                    .ok_or_else(|| Error::damaged("CFF positive number truncated"))?,
            );
            next = checked_add(next, 1)?;
            (f32::from(b0) - 247.0) * 256.0 + b1 + 108.0
        }
        251..=254 => {
            let b1 = f32::from(
                *bytes
                    .get(next)
                    .ok_or_else(|| Error::damaged("CFF negative number truncated"))?,
            );
            next = checked_add(next, 1)?;
            -((f32::from(b0) - 251.0) * 256.0) - b1 - 108.0
        }
        255 => {
            let raw = be_i32_at(bytes, next)?;
            next = checked_add(next, 4)?;
            (raw as f32) / 65536.0
        }
        _ => return Err(Error::damaged("invalid CFF Type 2 number")),
    };
    Ok((value, next))
}

fn parse_variation(
    data: &[u8],
    tables: &[Table],
    glyph_count: usize,
    weight: Option<f32>,
) -> Result<Option<VariationState>> {
    let Some(requested) = weight else {
        return Ok(None);
    };
    if !requested.is_finite() {
        return Err(Error::damaged("requested font weight is not finite"));
    }
    let fvar = required_table(tables, *b"fvar")?;
    let gvar = required_table(tables, *b"gvar")?;
    if fvar.length < 16 || be_u16_at(data, fvar.offset)? != 1 {
        return Err(Error::damaged("unsupported fvar header"));
    }
    let axes_rel = usize::from(be_u16_at(data, checked_add(fvar.offset, 4)?)?);
    let axis_count = usize::from(be_u16_at(data, checked_add(fvar.offset, 8)?)?);
    let axis_size = usize::from(be_u16_at(data, checked_add(fvar.offset, 10)?)?);
    if axis_count == 0 || axis_count > MAX_VARIATION_AXES || axis_size < 20 {
        return Err(Error::damaged("invalid fvar axis array"));
    }
    let axes_start = checked_add(fvar.offset, axes_rel)?;
    let axes_bytes = checked_mul(axis_count, axis_size)?;
    if checked_add(axes_start, axes_bytes)? > checked_add(fvar.offset, fvar.length)? {
        return Err(Error::damaged("fvar axes exceed table"));
    }
    let mut coords = vec![0.0_f32; axis_count];
    let mut found_weight = false;
    for axis in 0..axis_count {
        let record = checked_add(axes_start, checked_mul(axis, axis_size)?)?;
        let tag = checked_range(data, record, 4)?;
        if tag == b"wght" {
            let min = fixed_16_16(be_i32_at(data, checked_add(record, 4)?)?);
            let default = fixed_16_16(be_i32_at(data, checked_add(record, 8)?)?);
            let max = fixed_16_16(be_i32_at(data, checked_add(record, 12)?)?);
            if !(min <= default && default <= max) || min == max {
                return Err(Error::damaged("invalid wght fvar range"));
            }
            let value = requested.clamp(min, max);
            let normalized = if value < default {
                let denominator = default - min;
                if denominator == 0.0 {
                    0.0
                } else {
                    (value - default) / denominator
                }
            } else if value > default {
                let denominator = max - default;
                if denominator == 0.0 {
                    0.0
                } else {
                    (value - default) / denominator
                }
            } else {
                0.0
            };
            *coords
                .get_mut(axis)
                .ok_or_else(|| Error::damaged("fvar axis slot missing"))? = normalized.clamp(-1.0, 1.0);
            found_weight = true;
        }
    }
    if !found_weight {
        return Err(Error::Refused("variable font has no wght axis".to_owned()));
    }
    if let Some(avar) = find_table(tables, *b"avar") {
        apply_avar(data, avar, &mut coords)?;
    }

    if gvar.length < 20 || be_u16_at(data, gvar.offset)? != 1 {
        return Err(Error::damaged("unsupported gvar header"));
    }
    let gvar_axis_count = usize::from(be_u16_at(data, checked_add(gvar.offset, 4)?)?);
    let shared_tuple_count = usize::from(be_u16_at(data, checked_add(gvar.offset, 6)?)?);
    let shared_rel = usize::try_from(be_u32_at(data, checked_add(gvar.offset, 8)?)?)
        .map_err(|_| Error::damaged("gvar shared tuple offset overflow"))?;
    let gvar_glyph_count = usize::from(be_u16_at(data, checked_add(gvar.offset, 12)?)?);
    let flags = be_u16_at(data, checked_add(gvar.offset, 14)?)?;
    let data_rel = usize::try_from(be_u32_at(data, checked_add(gvar.offset, 16)?)?)
        .map_err(|_| Error::damaged("gvar data offset overflow"))?;
    if gvar_axis_count != axis_count || gvar_glyph_count != glyph_count {
        return Err(Error::damaged("gvar dimensions disagree with fvar/maxp"));
    }
    let shared_tuples_offset = checked_add(gvar.offset, shared_rel)?;
    let shared_bytes = checked_mul(checked_mul(shared_tuple_count, axis_count)?, 2)?;
    if checked_add(shared_tuples_offset, shared_bytes)? > checked_add(gvar.offset, gvar.length)? {
        return Err(Error::damaged("gvar shared tuples exceed table"));
    }
    let long_offsets = flags & 1 != 0;
    let offset_width = if long_offsets { 4 } else { 2 };
    let offsets_bytes = checked_mul(checked_add(glyph_count, 1)?, offset_width)?;
    if checked_add(checked_add(gvar.offset, 20)?, offsets_bytes)? > checked_add(gvar.offset, gvar.length)? {
        return Err(Error::damaged("gvar glyph offsets exceed table"));
    }
    let data_offset = checked_add(gvar.offset, data_rel)?;
    if data_offset > checked_add(gvar.offset, gvar.length)? {
        return Err(Error::damaged("gvar data offset exceeds table"));
    }
    Ok(Some(VariationState {
        gvar,
        axis_count,
        coords,
        shared_tuple_count,
        shared_tuples_offset,
        glyph_count,
        data_offset,
        long_offsets,
    }))
}

fn apply_avar(data: &[u8], avar: Table, coords: &mut [f32]) -> Result<()> {
    if avar.length < 8 || be_u16_at(data, avar.offset)? != 1 {
        return Err(Error::damaged("unsupported avar header"));
    }
    let axis_count = usize::from(be_u16_at(data, checked_add(avar.offset, 6)?)?);
    if axis_count != coords.len() {
        return Err(Error::damaged("avar axis count disagrees with fvar"));
    }
    let table_end = checked_add(avar.offset, avar.length)?;
    let mut position = checked_add(avar.offset, 8)?;
    for axis in 0..axis_count {
        let pair_count = usize::from(be_u16_at(data, position)?);
        position = checked_add(position, 2)?;
        if pair_count < 2 || checked_add(position, checked_mul(pair_count, 4)?)? > table_end {
            return Err(Error::damaged("invalid avar segment map"));
        }
        let coord = *coords
            .get(axis)
            .ok_or_else(|| Error::damaged("avar coordinate missing"))?;
        let mut previous_from = f32::NEG_INFINITY;
        let mut previous_to = 0.0_f32;
        let mut mapped = None;
        for pair in 0..pair_count {
            let at = checked_add(position, checked_mul(pair, 4)?)?;
            let from = f2dot14(be_i16_at(data, at)?);
            let to = f2dot14(be_i16_at(data, checked_add(at, 2)?)?);
            if from < previous_from {
                return Err(Error::damaged("avar segment coordinates are not sorted"));
            }
            if pair == 0 && coord <= from {
                mapped = Some(to);
            } else if mapped.is_none() && coord <= from {
                let span = from - previous_from;
                mapped = Some(if span == 0.0 {
                    to
                } else {
                    previous_to + (to - previous_to) * ((coord - previous_from) / span)
                });
            }
            previous_from = from;
            previous_to = to;
        }
        if mapped.is_none() {
            mapped = Some(previous_to);
        }
        *coords
            .get_mut(axis)
            .ok_or_else(|| Error::damaged("avar coordinate slot missing"))? = mapped.unwrap_or(coord);
        position = checked_add(position, checked_mul(pair_count, 4)?)?;
    }
    Ok(())
}

fn gvar_glyph_offset(data: &[u8], variation: &VariationState, index: usize) -> Result<usize> {
    if index > variation.glyph_count {
        return Err(Error::damaged("gvar glyph offset index outside array"));
    }
    let table = checked_add(variation.gvar.offset, 20)?;
    if variation.long_offsets {
        let at = checked_add(table, checked_mul(index, 4)?)?;
        usize::try_from(be_u32_at(data, at)?).map_err(|_| Error::damaged("gvar glyph offset overflow"))
    } else {
        let at = checked_add(table, checked_mul(index, 2)?)?;
        checked_mul(usize::from(be_u16_at(data, at)?), 2)
    }
}

fn read_tuple_coords(data: &[u8], position: usize, axis_count: usize, limit: usize) -> Result<(Vec<f32>, usize)> {
    let bytes = checked_mul(axis_count, 2)?;
    let end = checked_add(position, bytes)?;
    if end > limit {
        return Err(Error::damaged("gvar tuple coordinates are truncated"));
    }
    let mut tuple = Vec::with_capacity(axis_count);
    for axis in 0..axis_count {
        tuple.push(f2dot14(be_i16_at(data, checked_add(position, checked_mul(axis, 2)?)?)?));
    }
    Ok((tuple, end))
}

fn read_shared_tuple(data: &[u8], variation: &VariationState, index: usize) -> Result<Vec<f32>> {
    if index >= variation.shared_tuple_count {
        return Err(Error::damaged("gvar shared tuple index outside table"));
    }
    let tuple_bytes = checked_mul(variation.axis_count, 2)?;
    let position = checked_add(variation.shared_tuples_offset, checked_mul(index, tuple_bytes)?)?;
    let (tuple, _) = read_tuple_coords(
        data,
        position,
        variation.axis_count,
        checked_add(variation.gvar.offset, variation.gvar.length)?,
    )?;
    Ok(tuple)
}

fn tuple_scalar(coords: &[f32], peak: &[f32], start: Option<&[f32]>, end: Option<&[f32]>) -> Result<f32> {
    if coords.len() != peak.len()
        || start.is_some_and(|v| v.len() != coords.len())
        || end.is_some_and(|v| v.len() != coords.len())
    {
        return Err(Error::damaged("gvar tuple coordinate dimensions disagree"));
    }
    let mut scalar = 1.0_f32;
    for axis in 0..coords.len() {
        let coord = *coords
            .get(axis)
            .ok_or_else(|| Error::damaged("variation coordinate missing"))?;
        let peak_coord = *peak
            .get(axis)
            .ok_or_else(|| Error::damaged("peak coordinate missing"))?;
        if peak_coord == 0.0 {
            continue;
        }
        let axis_scalar = if let (Some(starts), Some(ends)) = (start, end) {
            let low = *starts
                .get(axis)
                .ok_or_else(|| Error::damaged("intermediate start missing"))?;
            let high = *ends
                .get(axis)
                .ok_or_else(|| Error::damaged("intermediate end missing"))?;
            if coord < low || coord > high || peak_coord < low || peak_coord > high {
                0.0
            } else if coord == peak_coord {
                1.0
            } else if coord < peak_coord {
                let d = peak_coord - low;
                if d == 0.0 {
                    0.0
                } else {
                    (coord - low) / d
                }
            } else {
                let d = high - peak_coord;
                if d == 0.0 {
                    0.0
                } else {
                    (high - coord) / d
                }
            }
        } else if coord == 0.0 || coord.signum() != peak_coord.signum() {
            0.0
        } else if coord.abs() >= peak_coord.abs() {
            1.0
        } else {
            coord / peak_coord
        };
        scalar *= axis_scalar.clamp(0.0, 1.0);
        if scalar == 0.0 {
            break;
        }
    }
    Ok(scalar)
}

fn decode_packed_points(
    data: &[u8],
    position: usize,
    point_limit: usize,
    limit: usize,
) -> Result<(Option<Vec<usize>>, usize)> {
    if position >= limit {
        return Err(Error::damaged("gvar packed points are truncated"));
    }
    let first = *data
        .get(position)
        .ok_or_else(|| Error::damaged("gvar point count missing"))?;
    let mut cursor = checked_add(position, 1)?;
    let count = if first & 0x80 != 0 {
        let second = *data
            .get(cursor)
            .ok_or_else(|| Error::damaged("gvar point count truncated"))?;
        cursor = checked_add(cursor, 1)?;
        (usize::from(first & 0x7f) << 8) | usize::from(second)
    } else {
        usize::from(first)
    };
    if count == 0 {
        return Ok((None, cursor));
    }
    if count > point_limit {
        return Err(Error::damaged("gvar point count exceeds glyph"));
    }
    let mut points = Vec::with_capacity(count);
    let mut current = 0_usize;
    while points.len() < count {
        if cursor >= limit {
            return Err(Error::damaged("gvar packed point run truncated"));
        }
        let control = *data
            .get(cursor)
            .ok_or_else(|| Error::damaged("gvar point run missing"))?;
        cursor = checked_add(cursor, 1)?;
        let run = usize::from(control & 0x7f) + 1;
        if checked_add(points.len(), run)? > count {
            return Err(Error::damaged("gvar point run exceeds declared count"));
        }
        for _ in 0..run {
            let delta = if control & 0x80 != 0 {
                let value = usize::from(be_u16_at(data, cursor)?);
                cursor = checked_add(cursor, 2)?;
                value
            } else {
                let value = usize::from(
                    *data
                        .get(cursor)
                        .ok_or_else(|| Error::damaged("gvar point delta missing"))?,
                );
                cursor = checked_add(cursor, 1)?;
                value
            };
            current = checked_add(current, delta)?;
            if current >= point_limit || cursor > limit {
                return Err(Error::damaged("gvar point index outside glyph"));
            }
            points.push(current);
        }
    }
    Ok((Some(points), cursor))
}

fn decode_packed_deltas(data: &[u8], position: usize, count: usize, limit: usize) -> Result<(Vec<f32>, usize)> {
    let mut cursor = position;
    let mut values = Vec::with_capacity(count);
    while values.len() < count {
        if cursor >= limit {
            return Err(Error::damaged("gvar packed delta run truncated"));
        }
        let control = *data
            .get(cursor)
            .ok_or_else(|| Error::damaged("gvar delta run missing"))?;
        cursor = checked_add(cursor, 1)?;
        let run = usize::from(control & 0x3f) + 1;
        if checked_add(values.len(), run)? > count {
            return Err(Error::damaged("gvar delta run exceeds declared count"));
        }
        if control & 0x80 != 0 {
            values.resize(checked_add(values.len(), run)?, 0.0);
            continue;
        }
        for _ in 0..run {
            let value = if control & 0x40 != 0 {
                let value = f32::from(be_i16_at(data, cursor)?);
                cursor = checked_add(cursor, 2)?;
                value
            } else {
                let byte = *data
                    .get(cursor)
                    .ok_or_else(|| Error::damaged("gvar byte delta missing"))?;
                cursor = checked_add(cursor, 1)?;
                f32::from(i8::from_be_bytes([byte]))
            };
            if cursor > limit {
                return Err(Error::damaged("gvar packed deltas exceed tuple"));
            }
            values.push(value);
        }
    }
    Ok((values, cursor))
}

fn iup_fill(values: &mut [Option<f32>], base: &[(f32, f32)], ends: &[u16], x_axis: bool) -> Result<()> {
    if values.len() < base.len() {
        return Err(Error::damaged("IUP delta array is shorter than points"));
    }
    let mut first = 0_usize;
    for end in ends.iter().copied() {
        let last = usize::from(end);
        if last >= base.len() || first > last {
            return Err(Error::damaged("IUP contour endpoint outside points"));
        }
        let touched: Vec<usize> = (first..=last)
            .filter(|index| values.get(*index).and_then(|value| *value).is_some())
            .collect();
        if touched.len() == 1 {
            let touched_index = *touched
                .first()
                .ok_or_else(|| Error::damaged("IUP touched point missing"))?;
            let delta = values.get(touched_index).and_then(|value| *value).unwrap_or(0.0);
            for index in first..=last {
                if let Some(slot) = values.get_mut(index) {
                    *slot = Some(delta);
                }
            }
        } else if touched.len() >= 2 {
            for pair in 0..touched.len() {
                let a = *touched
                    .get(pair)
                    .ok_or_else(|| Error::damaged("IUP first touched point missing"))?;
                let next_pair = checked_add(pair, 1)?
                    .checked_rem(touched.len())
                    .ok_or_else(|| Error::damaged("IUP touched-point modulo by zero"))?;
                let b = *touched
                    .get(next_pair)
                    .ok_or_else(|| Error::damaged("IUP second touched point missing"))?;
                let da = values.get(a).and_then(|value| *value).unwrap_or(0.0);
                let db = values.get(b).and_then(|value| *value).unwrap_or(0.0);
                let pa = *base.get(a).ok_or_else(|| Error::damaged("IUP base point missing"))?;
                let pb = *base.get(b).ok_or_else(|| Error::damaged("IUP base point missing"))?;
                let ca = if x_axis { pa.0 } else { pa.1 };
                let cb = if x_axis { pb.0 } else { pb.1 };
                let mut index = if a == last { first } else { checked_add(a, 1)? };
                while index != b {
                    let p = *base
                        .get(index)
                        .ok_or_else(|| Error::damaged("IUP base point missing"))?;
                    let c = if x_axis { p.0 } else { p.1 };
                    let delta = iup_interpolate(c, ca, da, cb, db);
                    if let Some(slot) = values.get_mut(index) {
                        if slot.is_none() {
                            *slot = Some(delta);
                        }
                    }
                    index = if index == last { first } else { checked_add(index, 1)? };
                }
            }
        }
        for index in first..=last {
            if let Some(slot) = values.get_mut(index) {
                if slot.is_none() {
                    *slot = Some(0.0);
                }
            }
        }
        first = checked_add(last, 1)?;
    }
    Ok(())
}

fn iup_interpolate(c: f32, c1: f32, d1: f32, c2: f32, d2: f32) -> f32 {
    if c1 == c2 {
        return if d1 == d2 { d1 } else { 0.0 };
    }
    let (low_c, low_d, high_c, high_d) = if c1 < c2 { (c1, d1, c2, d2) } else { (c2, d2, c1, d1) };
    if c <= low_c {
        low_d
    } else if c >= high_c {
        high_d
    } else {
        low_d + (high_d - low_d) * ((c - low_c) / (high_c - low_c))
    }
}

fn collection_face_offset(data: &[u8], index: u32) -> Result<usize> {
    if data.starts_with(b"ttcf") {
        if data.len() < 12 {
            return Err(Error::damaged("short TTC header"));
        }
        let count = be_u32_at(data, 8)?;
        if index >= count {
            return Err(Error::damaged("TTC face index is outside collection"));
        }
        let relative = checked_mul(
            usize::try_from(index).map_err(|_| Error::damaged("TTC face index overflow"))?,
            4,
        )?;
        let entry = checked_add(12, relative)?;
        let offset = usize::try_from(be_u32_at(data, entry)?)
            .map_err(|_| Error::damaged("TTC face offset does not fit usize"))?;
        checked_range(data, offset, 12)?;
        Ok(offset)
    } else {
        if index != 0 {
            return Err(Error::damaged("non-collection font only has face zero"));
        }
        checked_range(data, 0, 12)?;
        Ok(0)
    }
}

fn select_cmap(data: &[u8], cmap: Table) -> Result<(usize, usize)> {
    if cmap.length < 4 {
        return Err(Error::damaged("short cmap table"));
    }
    let count = usize::from(be_u16_at(data, checked_add(cmap.offset, 2)?)?);
    let records = checked_mul(count, 8)?;
    if cmap.length < checked_add(4, records)? {
        return Err(Error::damaged("short cmap encoding records"));
    }
    let mut best: Option<(u8, usize, usize)> = None;
    let mut index = 0_usize;
    while index < count {
        let record = checked_add(cmap.offset, checked_add(4, checked_mul(index, 8)?)?)?;
        let platform = be_u16_at(data, record)?;
        let encoding = be_u16_at(data, checked_add(record, 2)?)?;
        let relative = usize::try_from(be_u32_at(data, checked_add(record, 4)?)?)
            .map_err(|_| Error::damaged("cmap subtable offset overflow"))?;
        if relative >= cmap.length {
            return Err(Error::damaged("cmap subtable outside cmap"));
        }
        let offset = checked_add(cmap.offset, relative)?;
        let format = be_u16_at(data, offset)?;
        let (length, rank) = match format {
            12 => {
                let length = usize::try_from(be_u32_at(data, checked_add(offset, 4)?)?)
                    .map_err(|_| Error::damaged("cmap12 length overflow"))?;
                let rank = if platform == 3 && encoding == 10 { 4 } else { 3 };
                (length, rank)
            }
            4 => {
                let length = usize::from(be_u16_at(data, checked_add(offset, 2)?)?);
                let rank = if platform == 3 { 2 } else { 1 };
                (length, rank)
            }
            _ => {
                index = checked_add(index, 1)?;
                continue;
            }
        };
        let end = checked_add(relative, length)?;
        if end > cmap.length {
            return Err(Error::damaged("cmap subtable length outside cmap"));
        }
        if best.is_none_or(|(current, _, _)| rank > current) {
            best = Some((rank, offset, length));
        }
        index = checked_add(index, 1)?;
    }
    best.map(|(_, offset, length)| (offset, length))
        .ok_or_else(|| Error::Refused("font has no cmap format 4 or 12".to_owned()))
}

fn cmap12_lookup(data: &[u8], offset: usize, length: usize, code: u32) -> Option<u32> {
    if length < 16 {
        return None;
    }
    let groups = usize::try_from(be_u32_at(data, offset.checked_add(12)?).ok()?).ok()?;
    let bytes = groups.checked_mul(12)?;
    if 16_usize.checked_add(bytes)? > length {
        return None;
    }
    let mut low = 0_usize;
    let mut high = groups;
    while low < high {
        let middle = low.checked_add(high.checked_sub(low)?.checked_div(2)?)?;
        let group = offset.checked_add(16)?.checked_add(middle.checked_mul(12)?)?;
        let start = be_u32_at(data, group).ok()?;
        let end = be_u32_at(data, group.checked_add(4)?).ok()?;
        if code < start {
            high = middle;
        } else if code > end {
            low = middle.checked_add(1)?;
        } else {
            let glyph = be_u32_at(data, group.checked_add(8)?).ok()?;
            return glyph.checked_add(code.checked_sub(start)?);
        }
    }
    None
}

fn cmap4_lookup(data: &[u8], offset: usize, length: usize, code: u32) -> Option<u32> {
    let code16 = u16::try_from(code).ok()?;
    if length < 16 {
        return None;
    }
    let seg_count = usize::from(be_u16_at(data, offset.checked_add(6)?).ok()?).checked_div(2)?;
    if seg_count == 0 {
        return None;
    }
    let end_codes = offset.checked_add(14)?;
    let start_codes = end_codes.checked_add(seg_count.checked_mul(2)?)?.checked_add(2)?;
    let deltas = start_codes.checked_add(seg_count.checked_mul(2)?)?;
    let range_offsets = deltas.checked_add(seg_count.checked_mul(2)?)?;
    if range_offsets
        .checked_add(seg_count.checked_mul(2)?)?
        .checked_sub(offset)?
        > length
    {
        return None;
    }

    let mut segment = 0_usize;
    while segment < seg_count {
        let end = be_u16_at(data, end_codes.checked_add(segment.checked_mul(2)?)?).ok()?;
        if code16 <= end {
            let start = be_u16_at(data, start_codes.checked_add(segment.checked_mul(2)?)?).ok()?;
            if code16 < start {
                return None;
            }
            let delta = be_i16_at(data, deltas.checked_add(segment.checked_mul(2)?)?).ok()?;
            let ro_word = range_offsets.checked_add(segment.checked_mul(2)?)?;
            let range = be_u16_at(data, ro_word).ok()?;
            if range == 0 {
                let value = u32::from(code16).wrapping_add_signed(i32::from(delta)) & 0xFFFF;
                return Some(value);
            }
            let code_delta = usize::from(code16.checked_sub(start)?);
            let glyph_at = ro_word
                .checked_add(usize::from(range))?
                .checked_add(code_delta.checked_mul(2)?)?;
            if glyph_at.checked_add(2)?.checked_sub(offset)? > length {
                return None;
            }
            let glyph = be_u16_at(data, glyph_at).ok()?;
            if glyph == 0 {
                return Some(0);
            }
            let value = u32::from(glyph).wrapping_add_signed(i32::from(delta)) & 0xFFFF;
            return Some(value);
        }
        segment = segment.checked_add(1)?;
    }
    None
}

fn emit_quadratic_contour(points: &[Point], sink: &mut impl OutlineSink, transform: Transform) -> Result<()> {
    if points.is_empty() {
        return Ok(());
    }
    let first = *points.first().ok_or_else(|| Error::damaged("empty contour"))?;
    let last = *points.last().ok_or_else(|| Error::damaged("empty contour"))?;
    let start = if first.on_curve {
        first
    } else if last.on_curve {
        last
    } else {
        midpoint(first, last)
    };
    let (sx, sy) = transform.point(start.x, start.y);
    sink.move_to(sx, sy);

    let mut index = if first.on_curve { 1_usize } else { 0_usize };
    while index < points.len() {
        let current = *points
            .get(index)
            .ok_or_else(|| Error::damaged("contour point missing"))?;
        if current.on_curve {
            let (x, y) = transform.point(current.x, current.y);
            sink.line_to(x, y);
            index = checked_add(index, 1)?;
            continue;
        }

        let next_index = checked_add(index, 1)?;
        let next = points.get(next_index).copied().unwrap_or(start);
        let end = if next.on_curve { next } else { midpoint(current, next) };
        let (cx, cy) = transform.point(current.x, current.y);
        let (ex, ey) = transform.point(end.x, end.y);
        sink.quad_to(cx, cy, ex, ey);
        index = if next.on_curve {
            checked_add(index, 2)?
        } else {
            checked_add(index, 1)?
        };
    }
    sink.close();
    Ok(())
}

fn midpoint(a: Point, b: Point) -> Point {
    Point {
        x: (a.x + b.x) * 0.5,
        y: (a.y + b.y) * 0.5,
        on_curve: true,
    }
}

fn kern_lookup(data: &[u8], table: Table, left: u16, right: u16) -> Result<Option<i16>> {
    if table.length < 4 {
        return Err(Error::damaged("short kern table"));
    }
    let version = be_u16_at(data, table.offset)?;
    if version != 0 {
        return Ok(None);
    }
    let count = usize::from(be_u16_at(data, checked_add(table.offset, 2)?)?);
    let mut position = checked_add(table.offset, 4)?;
    let table_end = checked_add(table.offset, table.length)?;
    let mut index = 0_usize;
    while index < count {
        if checked_add(position, 6)? > table_end {
            return Err(Error::damaged("short kern subtable header"));
        }
        let length = usize::from(be_u16_at(data, checked_add(position, 2)?)?);
        if length < 6 || checked_add(position, length)? > table_end {
            return Err(Error::damaged("invalid kern subtable length"));
        }
        let coverage = be_u16_at(data, checked_add(position, 4)?)?;
        let format = coverage.checked_shr(8).unwrap_or_default();
        let horizontal = coverage & 1 != 0;
        if format == 0 && horizontal {
            let body = checked_add(position, 6)?;
            if length < 14 {
                return Err(Error::damaged("short kern format 0"));
            }
            let pairs = usize::from(be_u16_at(data, body)?);
            let pairs_start = checked_add(body, 8)?;
            let pairs_bytes = checked_mul(pairs, 6)?;
            if checked_add(pairs_start, pairs_bytes)? > checked_add(position, length)? {
                return Err(Error::damaged("kern pairs exceed subtable"));
            }
            let wanted = u32::from(left)
                .checked_shl(16)
                .and_then(|value| value.checked_add(u32::from(right)))
                .ok_or_else(|| Error::damaged("kern key overflow"))?;
            let mut low = 0_usize;
            let mut high = pairs;
            while low < high {
                let middle = checked_add(low, high.saturating_sub(low).checked_div(2).unwrap_or_default())?;
                let pair = checked_add(pairs_start, checked_mul(middle, 6)?)?;
                let right = u32::from(be_u16_at(data, checked_add(pair, 2)?)?);
                let key = u32::from(be_u16_at(data, pair)?)
                    .checked_shl(16)
                    .and_then(|value| value.checked_add(right))
                    .ok_or_else(|| Error::damaged("kern pair key overflow"))?;
                if key < wanted {
                    low = checked_add(middle, 1)?;
                } else if key > wanted {
                    high = middle;
                } else {
                    return Ok(Some(be_i16_at(data, checked_add(pair, 4)?)?));
                }
            }
        }
        position = checked_add(position, length)?;
        index = checked_add(index, 1)?;
    }
    Ok(None)
}

fn find_table(tables: &[Table], tag: [u8; 4]) -> Option<Table> {
    tables.iter().copied().find(|table| table.tag == tag)
}

fn required_table(tables: &[Table], tag: [u8; 4]) -> Result<Table> {
    find_table(tables, tag).ok_or_else(|| {
        Error::damaged(format!(
            "required font table {:?} is missing",
            String::from_utf8_lossy(&tag)
        ))
    })
}

fn checked_range(data: &[u8], offset: usize, length: usize) -> Result<&[u8]> {
    let end = checked_add(offset, length)?;
    data.get(offset..end)
        .ok_or_else(|| Error::damaged("font range is outside input"))
}

fn checked_add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(|| Error::damaged("font offset overflow"))
}

fn checked_mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(|| Error::damaged("font size overflow"))
}

fn be_u16_at(data: &[u8], offset: usize) -> Result<u16> {
    let bytes = checked_range(data, offset, 2)?;
    let array = <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged("short u16"))?;
    Ok(u16::from_be_bytes(array))
}

fn be_i16_at(data: &[u8], offset: usize) -> Result<i16> {
    let bytes = checked_range(data, offset, 2)?;
    let array = <[u8; 2]>::try_from(bytes).map_err(|_| Error::damaged("short i16"))?;
    Ok(i16::from_be_bytes(array))
}

fn be_u32_at(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = checked_range(data, offset, 4)?;
    let array = <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("short u32"))?;
    Ok(u32::from_be_bytes(array))
}

fn be_i32_at(data: &[u8], offset: usize) -> Result<i32> {
    let bytes = checked_range(data, offset, 4)?;
    let array = <[u8; 4]>::try_from(bytes).map_err(|_| Error::damaged("short i32"))?;
    Ok(i32::from_be_bytes(array))
}

fn fixed_16_16(value: i32) -> f32 {
    value as f32 / 65_536.0
}

fn i32_to_f32(value: i32) -> Result<f32> {
    let narrowed = i16::try_from(value).map_err(|_| Error::damaged("glyph coordinate outside i16 range"))?;
    Ok(f32::from(narrowed))
}

fn f2dot14(value: i16) -> f32 {
    f32::from(value) / 16_384.0
}

/// Rasterises a path into 8-bit non-zero-rule coverage.
///
/// Curves are flattened adaptively, then every edge is integrated once into signed per-cell
/// area plus a winding delta. A single prefix sum across each scanline turns those edge
/// contributions into pixel coverage. There is no per-pixel polygon clipping and no
/// supersampling. Quarter-pixel positioning is preserved because coordinates stay fractional.
pub fn rasterize(outline: &[Segment], width: u32, height: u32, out: &mut [u8]) {
    let Some(pixel_count_u32) = width.checked_mul(height) else {
        return;
    };
    let Ok(pixel_count) = usize::try_from(pixel_count_u32) else {
        return;
    };
    if out.len() != pixel_count {
        return;
    }
    out.fill(0);
    if width == 0 || height == 0 {
        return;
    }
    let Ok(w) = usize::try_from(width) else {
        return;
    };
    let Ok(h) = usize::try_from(height) else {
        return;
    };
    let Some(stride) = w.checked_add(1) else {
        return;
    };
    let Some(cell_count) = stride.checked_mul(h) else {
        return;
    };
    let mut cells = vec![CellAcc::default(); cell_count];

    let mut cursor = (0.0_f32, 0.0_f32);
    let mut start_point = cursor;
    let mut contour_open = false;
    for segment in outline.iter().copied() {
        match segment {
            Segment::MoveTo(x, y) => {
                if contour_open && cursor != start_point {
                    accumulate_edge(cursor, start_point, w, h, stride, &mut cells);
                }
                cursor = (x, y);
                start_point = cursor;
                contour_open = true;
            }
            Segment::LineTo(x, y) => {
                let next = (x, y);
                accumulate_edge(cursor, next, w, h, stride, &mut cells);
                cursor = next;
            }
            Segment::QuadTo(cx, cy, x, y) => {
                let next = (x, y);
                flatten_quad(cursor, (cx, cy), next, 0, w, h, stride, &mut cells);
                cursor = next;
            }
            Segment::CubicTo(c1x, c1y, c2x, c2y, x, y) => {
                let next = (x, y);
                flatten_cubic(cursor, (c1x, c1y), (c2x, c2y), next, 0, w, h, stride, &mut cells);
                cursor = next;
            }
            Segment::Close => {
                if contour_open && cursor != start_point {
                    accumulate_edge(cursor, start_point, w, h, stride, &mut cells);
                }
                cursor = start_point;
                contour_open = false;
            }
        }
    }
    if contour_open && cursor != start_point {
        accumulate_edge(cursor, start_point, w, h, stride, &mut cells);
    }

    for y in 0..h {
        let Some(row) = y.checked_mul(stride) else {
            return;
        };
        let Some(out_row) = y.checked_mul(w) else {
            return;
        };
        let mut winding = 0.0_f32;
        for x in 0..w {
            let Some(index) = row.checked_add(x) else {
                return;
            };
            let Some(cell) = cells.get(index).copied() else {
                return;
            };
            winding += cell.cover;
            let coverage = (winding + cell.area).abs().clamp(0.0, 1.0);
            let Some(out_index) = out_row.checked_add(x) else {
                return;
            };
            let Some(slot) = out.get_mut(out_index) else {
                return;
            };
            *slot = coverage_to_u8(coverage);
        }
    }
}

#[derive(Clone, Copy, Default)]
struct CellAcc {
    area: f32,
    cover: f32,
}

#[allow(clippy::cast_possible_truncation)]
fn coverage_to_u8(coverage: f32) -> u8 {
    if coverage <= 0.0 {
        0
    } else if coverage >= 1.0 {
        255
    } else {
        (coverage.mul_add(255.0, 0.5)) as u8
    }
}

const FLATNESS: f32 = 1.0 / 32.0;
const MAX_FLATTEN_DEPTH: usize = 10;

#[allow(clippy::too_many_arguments)]
fn flatten_quad(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    depth: usize,
    w: usize,
    h: usize,
    stride: usize,
    cells: &mut [CellAcc],
) {
    if depth >= MAX_FLATTEN_DEPTH || point_line_distance_sq(p1, p0, p2) <= FLATNESS * FLATNESS {
        accumulate_edge(p0, p2, w, h, stride, cells);
        return;
    }
    let a = mid2(p0, p1);
    let b = mid2(p1, p2);
    let m = mid2(a, b);
    let Some(next_depth) = depth.checked_add(1) else {
        return;
    };
    flatten_quad(p0, a, m, next_depth, w, h, stride, cells);
    flatten_quad(m, b, p2, next_depth, w, h, stride, cells);
}

#[allow(clippy::too_many_arguments)]
fn flatten_cubic(
    p0: (f32, f32),
    p1: (f32, f32),
    p2: (f32, f32),
    p3: (f32, f32),
    depth: usize,
    w: usize,
    h: usize,
    stride: usize,
    cells: &mut [CellAcc],
) {
    let flat = point_line_distance_sq(p1, p0, p3).max(point_line_distance_sq(p2, p0, p3));
    if depth >= MAX_FLATTEN_DEPTH || flat <= FLATNESS * FLATNESS {
        accumulate_edge(p0, p3, w, h, stride, cells);
        return;
    }
    let a = mid2(p0, p1);
    let b = mid2(p1, p2);
    let c = mid2(p2, p3);
    let d = mid2(a, b);
    let e = mid2(b, c);
    let m = mid2(d, e);
    let Some(next_depth) = depth.checked_add(1) else {
        return;
    };
    flatten_cubic(p0, a, d, m, next_depth, w, h, stride, cells);
    flatten_cubic(m, e, c, p3, next_depth, w, h, stride, cells);
}
fn mid2(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5)
}
fn point_line_distance_sq(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let len = dx * dx + dy * dy;
    if len <= f32::EPSILON {
        return (p.0 - a.0).powi(2) + (p.1 - a.1).powi(2);
    }
    let cross = dx * (a.1 - p.1) - (a.0 - p.0) * dy;
    (cross * cross) / len
}

#[allow(clippy::cast_possible_truncation)]
fn bounded_floor_to_usize(value: f32, limit: usize) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= limit as f32 {
        return limit;
    }
    value.floor() as usize
}

#[allow(clippy::cast_possible_truncation)]
fn bounded_ceil_to_usize(value: f32, limit: usize) -> usize {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    if value >= limit as f32 {
        return limit;
    }
    value.ceil() as usize
}

fn accumulate_edge(a: (f32, f32), b: (f32, f32), w: usize, h: usize, stride: usize, cells: &mut [CellAcc]) {
    if !a.0.is_finite() || !a.1.is_finite() || !b.0.is_finite() || !b.1.is_finite() || a.1 == b.1 {
        return;
    }
    let min_y = a.1.min(b.1).max(0.0);
    let max_y = a.1.max(b.1).min(h as f32);
    if min_y >= max_y {
        return;
    }
    let mut row = bounded_floor_to_usize(min_y, h);
    let last = bounded_ceil_to_usize(max_y, h).saturating_sub(1);
    while row <= last {
        let low = min_y.max(row as f32);
        let high = max_y.min(row as f32 + 1.0);
        if low < high {
            let t0 = (low - a.1) / (b.1 - a.1);
            let t1 = (high - a.1) / (b.1 - a.1);
            let x0 = (b.0 - a.0).mul_add(t0, a.0);
            let x1 = (b.0 - a.0).mul_add(t1, a.0);
            if b.1 > a.1 {
                accumulate_strip_piece((x0, low), (x1, high), row, w, stride, cells);
            } else {
                accumulate_strip_piece((x1, high), (x0, low), row, w, stride, cells);
            }
        }
        if row == last {
            break;
        }
        let Some(next_row) = row.checked_add(1) else {
            return;
        };
        row = next_row;
    }
}

fn accumulate_strip_piece(a: (f32, f32), b: (f32, f32), row: usize, w: usize, stride: usize, cells: &mut [CellAcc]) {
    let dx = b.0 - a.0;
    if dx.abs() <= f32::EPSILON {
        add_cell_piece(a, b, row, w, stride, cells);
        return;
    }
    let step = if dx > 0.0 { 1.0 } else { -1.0 };
    let mut boundary = if dx > 0.0 { a.0.floor() + 1.0 } else { a.0.ceil() - 1.0 };
    let mut prev = a;
    loop {
        let inside = if dx > 0.0 { boundary < b.0 } else { boundary > b.0 };
        if !inside {
            break;
        }
        let t = (boundary - a.0) / dx;
        let p = (boundary, (b.1 - a.1).mul_add(t, a.1));
        add_cell_piece(prev, p, row, w, stride, cells);
        prev = p;
        boundary += step;
    }
    add_cell_piece(prev, b, row, w, stride, cells);
}

fn add_cell_piece(a: (f32, f32), b: (f32, f32), row: usize, w: usize, stride: usize, cells: &mut [CellAcc]) {
    let dy = b.1 - a.1;
    if dy == 0.0 {
        return;
    }
    let avg_x = (a.0 + b.0) * 0.5;
    let cell = avg_x.floor();
    let Some(row_start) = row.checked_mul(stride) else {
        return;
    };
    if cell < 0.0 {
        if let Some(slot) = cells.get_mut(row_start) {
            slot.cover += dy;
        }
        return;
    }
    let cell = bounded_floor_to_usize(cell, w);
    if cell >= w {
        return;
    }
    let Some(cell_index) = row_start.checked_add(cell) else {
        return;
    };
    let Some(next_cell) = cell.checked_add(1) else {
        return;
    };
    let next_boundary = next_cell as f32;
    if let Some(slot) = cells.get_mut(cell_index) {
        slot.area += dy * (next_boundary - avg_x);
    }
    let Some(next_index) = row_start.checked_add(next_cell) else {
        return;
    };
    if let Some(slot) = cells.get_mut(next_index) {
        slot.cover += dy;
    }
}

#[cfg(test)]
mod tests {
    use super::{rasterize, Font, GlyphId, OutlineSink, Segment};
    use std::time::Instant;

    const CHARS: [char; 20] = [
        'A', 'a', 'Z', 'z', '0', '9', '.', ',', '!', '?', 'А', 'Б', 'В', 'а', 'б', 'в', 'Ё', 'ё', 'Ж', 'ж',
    ];
    const LIBERATION: [(u16, f32); 20] = [
        (36, 1120.0),
        (68, 934.0),
        (61, 1026.0),
        (93, 840.0),
        (19, 934.0),
        (28, 934.0),
        (17, 467.0),
        (15, 467.0),
        (4, 467.0),
        (34, 934.0),
        (428, 1120.0),
        (429, 1102.0),
        (430, 1120.0),
        (460, 934.0),
        (461, 962.0),
        (462, 892.0),
        (413, 1120.0),
        (493, 934.0),
        (434, 1550.0),
        (466, 1123.0),
    ];
    const OSWALD: [(u16, f32); 20] = [
        (1, 492.0),
        (218, 409.0),
        (214, 417.0),
        (432, 347.0),
        (682, 517.0),
        (691, 502.0),
        (705, 188.0),
        (706, 187.0),
        (710, 208.0),
        (712, 483.0),
        (445, 492.0),
        (446, 494.0),
        (447, 524.0),
        (563, 409.0),
        (564, 438.0),
        (565, 415.0),
        (457, 407.0),
        (575, 421.0),
        (458, 676.0),
        (576, 579.0),
    ];

    fn parsed(bytes: &[u8]) -> Font<'_> {
        match Font::parse(bytes, 0) {
            Ok(font) => font,
            Err(error) => panic!("font parse failed: {error:?}"),
        }
    }

    fn assert_metrics(bytes: &[u8], expected: &[(u16, f32); 20]) {
        let font = parsed(bytes);
        for (index, character) in CHARS.iter().copied().enumerate() {
            let Some((glyph, advance)) = expected.get(index).copied() else {
                panic!("missing expected font case");
            };
            assert_eq!(font.glyph(character), Some(GlyphId(glyph)), "glyph for {character}");
            assert_eq!(font.advance(GlyphId(glyph)), advance, "advance for {character}");
        }
    }

    #[test]
    fn real_liberation_ids_and_advances() {
        assert_metrics(
            include_bytes!("../../../fixtures/fonts/LiberationSansNarrow-Regular.ttf"),
            &LIBERATION,
        );
    }

    #[test]
    fn real_oswald_ids_and_advances() {
        assert_metrics(include_bytes!("../../../fixtures/fonts/Oswald[wght].ttf"), &OSWALD);
    }

    struct Path(Vec<Segment>);
    impl OutlineSink for Path {
        fn move_to(&mut self, x: f32, y: f32) {
            self.0.push(Segment::MoveTo(x, y));
        }
        fn line_to(&mut self, x: f32, y: f32) {
            self.0.push(Segment::LineTo(x, y));
        }
        fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
            self.0.push(Segment::QuadTo(cx, cy, x, y));
        }
        fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
            self.0.push(Segment::CubicTo(c1x, c1y, c2x, c2y, x, y));
        }
        fn close(&mut self) {
            self.0.push(Segment::Close);
        }
    }

    #[test]
    fn oswald_weight_changes_outline() {
        let bytes = include_bytes!("../../../fixtures/fonts/Oswald[wght].ttf");
        let default = parsed(bytes);
        let heavy = match Font::parse_with_weight(bytes, 0, 700.0) {
            Ok(font) => font,
            Err(error) => panic!("variable font parse failed: {error:?}"),
        };
        let glyph = default.glyph('A').unwrap_or(GlyphId(0));
        let mut a = Path(Vec::new());
        let mut b = Path(Vec::new());
        if let Err(error) = default.outline(glyph, &mut a) {
            panic!("default outline failed: {error:?}");
        }
        if let Err(error) = heavy.outline(glyph, &mut b) {
            panic!("varied outline failed: {error:?}");
        }
        assert_ne!(a.0, b.0);
    }

    #[test]
    fn rasterises_exact_axis_aligned_square() {
        let path = [
            Segment::MoveTo(0.25, 0.25),
            Segment::LineTo(1.75, 0.25),
            Segment::LineTo(1.75, 1.75),
            Segment::LineTo(0.25, 1.75),
            Segment::Close,
        ];
        let mut out = [0_u8; 4];
        rasterize(&path, 2, 2, &mut out);
        for value in out {
            assert!((i16::from(value) - 143).abs() <= 1);
        }
    }

    #[test]
    fn wrong_output_size_is_untouched() {
        let path = [Segment::MoveTo(0.0, 0.0), Segment::Close];
        let mut out = [17_u8; 3];
        rasterize(&path, 2, 2, &mut out);
        assert_eq!(out, [17, 17, 17]);
    }

    #[test]
    #[ignore = "microbenchmark; run with cargo test --release rasterizer_16px_benchmark -- --ignored --nocapture"]
    fn rasterizer_16px_benchmark() {
        let path = [
            Segment::MoveTo(2.0, 1.0),
            Segment::CubicTo(14.0, 1.0, 14.0, 15.0, 2.0, 15.0),
            Segment::CubicTo(8.0, 12.0, 8.0, 4.0, 2.0, 1.0),
            Segment::Close,
        ];
        let mut out = [0_u8; 16 * 16];
        let iterations = 50_000_u32;
        let start = Instant::now();
        for _ in 0..iterations {
            rasterize(&path, 16, 16, &mut out);
            std::hint::black_box(&out);
        }
        let nanos = start.elapsed().as_nanos() / u128::from(iterations);
        eprintln!("X6b rasterizer 16px: {nanos} ns/glyph");
    }
}