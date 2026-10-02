//! Safe OpenType/TrueType font parsing and analytic coverage rasterisation.
//!
//! The parser borrows the source bytes and validates every table range before use. TrueType
//! `glyf` outlines (simple and XY-positioned composite glyphs), cmap formats 4/12, horizontal
//! metrics and legacy `kern` format 0 are supported. CFF/CFF2 outlines are deliberately
//! reported as unsupported until the Type 2 interpreter is wired in; callers never receive
//! guessed geometry.

use sse_core::{Error, Result};

const MAX_TABLES: usize = 4_096;
const MAX_GLYPHS: usize = 1_000_000;
const MAX_CONTOURS: usize = 16_384;
const MAX_POINTS: usize = 1_000_000;
const MAX_COMPOSITE_DEPTH: usize = 8;

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
    cff: Option<Table>,
}

impl<'a> Font<'a> {
    /// Parses a `.ttf`, `.otf`, or a selected face of a `.ttc`.
    ///
    /// # Errors
    /// Returns [`Error::Damaged`] for malformed offsets/counts/ranges, and
    /// [`Error::Refused`] for an unsupported outline flavour.
    pub fn parse(data: &'a [u8], index_in_collection: u32) -> Result<Self> {
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
        let cff = find_table(&tables, *b"CFF ");
        if glyf.is_some() != loca.is_some() {
            return Err(Error::damaged("glyf and loca must appear together"));
        }
        if glyf.is_none() && cff.is_none() {
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
        if self.cff.is_some() {
            return Err(Error::Refused(
                "CFF Type 2 outlines are not implemented in this package revision".to_owned(),
            ));
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
                bytes,
                usize::try_from(contours).map_err(|_| Error::damaged("contour count"))?,
                sink,
                transform,
            )
        } else {
            self.outline_composite(bytes, sink, transform, depth)
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
        bytes: &[u8],
        sink: &mut impl OutlineSink,
        transform: Transform,
        depth: usize,
    ) -> Result<()> {
        let mut position = 10_usize;
        let next_depth = checked_add(depth, 1)?;
        loop {
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

            self.outline_glyf(glyph, sink, transform.combine(child), next_depth)?;
            if flags & 0x0020 == 0 {
                if flags & 0x0100 != 0 {
                    let instruction_length = usize::from(be_u16_at(bytes, position)?);
                    position = checked_add(position, 2)?;
                    checked_range(bytes, position, instruction_length)?;
                }
                break;
            }
        }
        Ok(())
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
                let middle = checked_add(
                    low,
                    high.checked_sub(low)
                        .unwrap_or_default()
                        .checked_div(2)
                        .unwrap_or_default(),
                )?;
                let pair = checked_add(pairs_start, checked_mul(middle, 6)?)?;
                let pair_right = checked_add(pair, 2)?;
                let right = u32::from(be_u16_at(data, pair_right)?);
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

fn i32_to_f32(value: i32) -> Result<f32> {
    let narrowed = i16::try_from(value).map_err(|_| Error::damaged("glyph coordinate outside i16 range"))?;
    Ok(f32::from(narrowed))
}

fn f2dot14(value: i16) -> f32 {
    f32::from(value) / 16_384.0
}

/// Rasterises path coverage into `out` using analytic polygon clipping after Bézier flattening.
///
/// The output is row-major 8-bit coverage. `out` is left unchanged when its size does not match
/// `width * height`. Coordinates are in output pixel space and quarter-pixel positioning is
/// naturally preserved because no coordinate quantisation occurs.
///
/// Curves are flattened geometrically (not supersampled), then each contour is clipped to each
/// affected pixel and its signed area contributes to the non-zero fill.
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

    let contours = flatten_contours(outline);
    let mut y = 0_u32;
    while y < height {
        let mut x = 0_u32;
        while x < width {
            let x0 = f64::from(x);
            let y0 = f64::from(y);
            let x1 = f64::from(x.saturating_add(1));
            let y1 = f64::from(y.saturating_add(1));
            let mut signed_area = 0.0_f64;
            for contour in &contours {
                let clipped = clip_polygon(contour, x0, y0, x1, y1);
                signed_area += polygon_signed_area(&clipped);
            }
            let coverage = signed_area.abs().clamp(0.0, 1.0);
            let byte = coverage_to_u8(coverage);
            let Some(row) = usize::try_from(y)
                .ok()
                .and_then(|row| row.checked_mul(usize::try_from(width).ok()?))
            else {
                return;
            };
            let Some(index) = usize::try_from(x).ok().and_then(|column| row.checked_add(column)) else {
                return;
            };
            if let Some(slot) = out.get_mut(index) {
                *slot = byte;
            } else {
                return;
            }
            x = match x.checked_add(1) {
                Some(value) => value,
                None => return,
            };
        }
        y = match y.checked_add(1) {
            Some(value) => value,
            None => return,
        };
    }
}

fn coverage_to_u8(coverage: f64) -> u8 {
    if coverage <= 0.0 {
        return 0;
    }
    if coverage >= 1.0 {
        return 255;
    }
    let target = coverage * 255.0;
    let mut value = 0_u16;
    while value < 255 {
        let next = value.saturating_add(1);
        let threshold = (f64::from(value) + f64::from(next)) * 0.5;
        if target < threshold {
            break;
        }
        value = next;
    }
    u8::try_from(value).unwrap_or(255)
}

type DPoint = (f64, f64);

fn flatten_contours(outline: &[Segment]) -> Vec<Vec<DPoint>> {
    let mut contours = Vec::<Vec<DPoint>>::new();
    let mut current = Vec::<DPoint>::new();
    let mut cursor = (0.0_f64, 0.0_f64);
    let mut start = cursor;

    for segment in outline.iter().copied() {
        match segment {
            Segment::MoveTo(x, y) => {
                finish_contour(&mut contours, &mut current);
                cursor = (f64::from(x), f64::from(y));
                start = cursor;
                current.push(cursor);
            }
            Segment::LineTo(x, y) => {
                cursor = (f64::from(x), f64::from(y));
                current.push(cursor);
            }
            Segment::QuadTo(cx, cy, x, y) => {
                let p0 = cursor;
                let p1 = (f64::from(cx), f64::from(cy));
                let p2 = (f64::from(x), f64::from(y));
                let steps = 24_u32;
                let mut step = 1_u32;
                while step <= steps {
                    let t = f64::from(step) / f64::from(steps);
                    let one = 1.0 - t;
                    let px = one * one * p0.0 + 2.0 * one * t * p1.0 + t * t * p2.0;
                    let py = one * one * p0.1 + 2.0 * one * t * p1.1 + t * t * p2.1;
                    current.push((px, py));
                    step = match step.checked_add(1) {
                        Some(value) => value,
                        None => break,
                    };
                }
                cursor = p2;
            }
            Segment::CubicTo(c1x, c1y, c2x, c2y, x, y) => {
                let p0 = cursor;
                let p1 = (f64::from(c1x), f64::from(c1y));
                let p2 = (f64::from(c2x), f64::from(c2y));
                let p3 = (f64::from(x), f64::from(y));
                let steps = 32_u32;
                let mut step = 1_u32;
                while step <= steps {
                    let t = f64::from(step) / f64::from(steps);
                    let one = 1.0 - t;
                    let px = one * one * one * p0.0
                        + 3.0 * one * one * t * p1.0
                        + 3.0 * one * t * t * p2.0
                        + t * t * t * p3.0;
                    let py = one * one * one * p0.1
                        + 3.0 * one * one * t * p1.1
                        + 3.0 * one * t * t * p2.1
                        + t * t * t * p3.1;
                    current.push((px, py));
                    step = match step.checked_add(1) {
                        Some(value) => value,
                        None => break,
                    };
                }
                cursor = p3;
            }
            Segment::Close => {
                if current.last().copied() != Some(start) {
                    current.push(start);
                }
                finish_contour(&mut contours, &mut current);
                cursor = start;
            }
        }
    }
    finish_contour(&mut contours, &mut current);
    contours
}

fn finish_contour(contours: &mut Vec<Vec<DPoint>>, current: &mut Vec<DPoint>) {
    if current.len() >= 3 {
        if current.first().copied() != current.last().copied() {
            if let Some(first) = current.first().copied() {
                current.push(first);
            }
        }
        contours.push(core::mem::take(current));
    } else {
        current.clear();
    }
}

#[derive(Clone, Copy)]
enum ClipEdge {
    Left(f64),
    Right(f64),
    Top(f64),
    Bottom(f64),
}

fn clip_polygon(poly: &[DPoint], x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<DPoint> {
    let mut value = poly.to_vec();
    for edge in [
        ClipEdge::Left(x0),
        ClipEdge::Right(x1),
        ClipEdge::Top(y0),
        ClipEdge::Bottom(y1),
    ] {
        value = clip_edge(&value, edge);
        if value.is_empty() {
            break;
        }
    }
    value
}

fn clip_edge(poly: &[DPoint], edge: ClipEdge) -> Vec<DPoint> {
    if poly.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(poly.len());
    let Some(mut previous) = poly.last().copied() else {
        return out;
    };
    let mut previous_inside = inside(previous, edge);
    for current in poly.iter().copied() {
        let current_inside = inside(current, edge);
        if current_inside != previous_inside {
            out.push(intersection(previous, current, edge));
        }
        if current_inside {
            out.push(current);
        }
        previous = current;
        previous_inside = current_inside;
    }
    out
}

fn inside(point: DPoint, edge: ClipEdge) -> bool {
    match edge {
        ClipEdge::Left(x) => point.0 >= x,
        ClipEdge::Right(x) => point.0 <= x,
        ClipEdge::Top(y) => point.1 >= y,
        ClipEdge::Bottom(y) => point.1 <= y,
    }
}

fn intersection(a: DPoint, b: DPoint, edge: ClipEdge) -> DPoint {
    match edge {
        ClipEdge::Left(x) | ClipEdge::Right(x) => {
            let denominator = b.0 - a.0;
            if denominator == 0.0 {
                return (x, a.1);
            }
            let t = (x - a.0) / denominator;
            (x, (b.1 - a.1).mul_add(t, a.1))
        }
        ClipEdge::Top(y) | ClipEdge::Bottom(y) => {
            let denominator = b.1 - a.1;
            if denominator == 0.0 {
                return (a.0, y);
            }
            let t = (y - a.1) / denominator;
            ((b.0 - a.0).mul_add(t, a.0), y)
        }
    }
}

fn polygon_signed_area(poly: &[DPoint]) -> f64 {
    if poly.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0_f64;
    let mut index = 0_usize;
    while index < poly.len() {
        let next = match index.checked_add(1) {
            Some(value) if value < poly.len() => value,
            _ => 0,
        };
        let Some(a) = poly.get(index).copied() else {
            break;
        };
        let Some(b) = poly.get(next).copied() else {
            break;
        };
        sum += a.0 * b.1 - b.0 * a.1;
        index = match index.checked_add(1) {
            Some(value) => value,
            None => break,
        };
    }
    sum * 0.5
}

#[cfg(test)]
mod tests {
    use super::{rasterize, Segment};

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
}
