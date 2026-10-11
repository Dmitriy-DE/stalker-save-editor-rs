//! Text drawing: the editor's bundled fonts, glyph coverage cache and single-line runs.

use crate::fallback_font::{is_cjk, SystemFaces};
use crate::raster::{Color, MaskRef, Surface};
use sse_codecs::font::{rasterize, Font, GlyphId, OutlineSink, Segment};
use sse_core::Result;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

const BODY_REGULAR: &[u8] = include_bytes!("../assets/fonts/LiberationSansNarrow-Regular.ttf");
const BODY_BOLD: &[u8] = include_bytes!("../assets/fonts/LiberationSansNarrow-Bold.ttf");
const HEADING: &[u8] = include_bytes!("../assets/fonts/Oswald-wght.ttf");

/// Upper bound of cached glyph masks before the cache is dropped and rebuilt.
const MAX_CACHED_GLYPHS: usize = 1024;
/// Subpixel horizontal positions per pixel.
const SUBPIXEL_STEPS: f32 = 4.0;

/// One of the three faces the editor uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    /// Liberation Sans Narrow, the body text of the C# editor.
    Body,
    /// Liberation Sans Narrow Bold.
    BodyBold,
    /// Oswald semi-bold, headings and navigation.
    Heading,
    /// Oswald medium, weight 500.
    HeadingMedium,
}

/// Text style: face, size in UI pixels, space between characters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    /// Font face.
    pub face: Face,
    /// Em size in pixels.
    pub size: f32,
    /// Space added between two consecutive characters, in pixels.
    pub tracking: f32,
}

impl TextStyle {
    /// Creates a style without extra spacing.
    #[must_use]
    pub const fn new(face: Face, size: f32) -> Self {
        Self {
            face,
            size,
            tracking: 0.0,
        }
    }

    /// Returns the same style with the given spacing between characters.
    #[must_use]
    pub const fn with_tracking(self, tracking: f32) -> Self {
        Self { tracking, ..self }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphKey {
    face: Face,
    glyph: u16,
    size_bits: u32,
    subpixel: u8,
    /// Drawn from a system font (a CJK glyph the bundled faces lack), not from the bundled face.
    system: bool,
}

struct GlyphMask {
    left: i32,
    top: i32,
    width: u32,
    height: u32,
    coverage: Vec<u8>,
}

/// Parsed fonts plus a coverage cache. One per window.
pub struct Fonts {
    body: Font<'static>,
    bold: Font<'static>,
    heading: Font<'static>,
    heading_medium: Font<'static>,
    cache: HashMap<GlyphKey, GlyphMask>,
    /// System fonts for CJK glyphs, loaded on first need; `None` inside when this machine has none.
    system: OnceLock<Option<SystemFaces>>,
    /// Set when a CJK glyph had no system font to come from; read once by [`Fonts::take_system_font_missing`].
    system_missing: AtomicBool,
}

impl Fonts {
    /// Parses the bundled fonts.
    ///
    /// # Errors
    /// Returns an error if a bundled font fails to parse.
    pub fn bundled() -> Result<Self> {
        Ok(Self {
            body: Font::parse(BODY_REGULAR, 0)?,
            bold: Font::parse(BODY_BOLD, 0)?,
            heading: Font::parse_with_weight(HEADING, 0, 600.0)?,
            heading_medium: Font::parse_with_weight(HEADING, 0, 500.0)?,
            cache: HashMap::new(),
            system: OnceLock::new(),
            system_missing: AtomicBool::new(false),
        })
    }

    /// The system font's glyph for a CJK character the bundled faces lack. Loads the system font on first need only.
    fn system_glyph(&self, character: char) -> Option<(&Font<'static>, GlyphId)> {
        if !is_cjk(character) {
            return None;
        }
        let faces = self
            .system
            .get_or_init(|| SystemFaces::load(crate::strings::current_language()));
        let found = faces.as_ref().and_then(|faces| faces.glyph(character));
        if found.is_none() {
            self.system_missing.store(true, Ordering::Relaxed);
        }
        found
    }

    /// The glyph of `character` and the system font it comes from, if it is not in the bundled `face`.
    fn source(&self, face: Face, character: char) -> (GlyphId, Option<&Font<'static>>) {
        let bundled = self.font(face);
        match bundled.glyph(character) {
            Some(glyph) if glyph.0 != 0 => (glyph, None),
            _ => match self.system_glyph(character) {
                Some((system, glyph)) => (glyph, Some(system)),
                None => (bundled.glyph(character).unwrap_or(GlyphId(0)), None),
            },
        }
    }

    /// Pixels per font unit for the font a glyph comes from.
    fn unit_scale(&self, style: TextStyle, system: Option<&Font<'static>>) -> f32 {
        match system {
            Some(font) => {
                let units = font.metrics().units_per_em;
                if units > 0.0 {
                    style.size / units
                } else {
                    0.0
                }
            }
            None => self.scale(style),
        }
    }

    /// Whether the system fallback font has been loaded. Russian text must leave this false.
    #[cfg(test)]
    fn system_font_loaded(&self) -> bool {
        self.system.get().is_some_and(Option::is_some)
    }

    /// Whether a CJK glyph was asked for and no system font had it since the last call. The caller reports it once.
    #[must_use]
    pub fn take_system_font_missing(&self) -> bool {
        self.system_missing.swap(false, Ordering::Relaxed)
    }

    fn font(&self, face: Face) -> &Font<'static> {
        match face {
            Face::Body => &self.body,
            Face::BodyBold => &self.bold,
            Face::Heading => &self.heading,
            Face::HeadingMedium => &self.heading_medium,
        }
    }

    fn scale(&self, style: TextStyle) -> f32 {
        let units = self.font(style.face).metrics().units_per_em;
        if units > 0.0 {
            style.size / units
        } else {
            0.0
        }
    }

    /// Ascent above the baseline in pixels.
    #[must_use]
    pub fn ascent(&self, style: TextStyle) -> f32 {
        self.font(style.face).metrics().ascent * self.scale(style)
    }

    /// Recommended line height in pixels.
    #[must_use]
    pub fn line_height(&self, style: TextStyle) -> f32 {
        let metrics = self.font(style.face).metrics();
        (metrics.ascent - metrics.descent + metrics.line_gap) * self.scale(style)
    }

    /// Width of a single line in pixels.
    #[must_use]
    pub fn measure(&self, text: &str, style: TextStyle) -> f32 {
        let bundled = self.font(style.face);
        let mut width = 0.0_f32;
        let mut previous: Option<(GlyphId, bool)> = None;
        for character in text.chars() {
            let (glyph, system) = self.source(style.face, character);
            let font = system.unwrap_or(bundled);
            let scale = self.unit_scale(style, system);
            if let Some((left, left_system)) = previous {
                // Kerning only within one font: a system glyph has no pair with a bundled one.
                let kerning = if !left_system && system.is_none() {
                    bundled.kerning(left, glyph) * scale
                } else {
                    0.0
                };
                width += kerning + style.tracking;
            }
            width += font.advance(glyph) * scale;
            previous = Some((glyph, system.is_some()));
        }
        width
    }

    /// A [`crate::text::Metrics`] view of one style for line breaking and carets.
    #[must_use]
    pub fn metrics(&self, style: TextStyle) -> StyleMetrics<'_> {
        StyleMetrics { fonts: self, style }
    }

    /// Draws one line with its baseline at `baseline_y`.
    pub fn draw(
        &mut self,
        surface: &mut Surface<'_>,
        text: &str,
        x: f32,
        baseline_y: f32,
        style: TextStyle,
        color: Color,
    ) {
        if self.cache.len() > MAX_CACHED_GLYPHS {
            self.cache.clear();
        }
        let mut pen = x;
        let mut previous: Option<(GlyphId, bool)> = None;
        let baseline = to_px(baseline_y.round());
        for character in text.chars() {
            let (glyph, system, scale, advance, kerning) = {
                let (glyph, system) = self.source(style.face, character);
                let scale = self.unit_scale(style, system);
                let font = system.unwrap_or_else(|| self.font(style.face));
                let kerning = match previous {
                    Some((left, left_system)) if !left_system && system.is_none() => {
                        self.font(style.face).kerning(left, glyph)
                    }
                    _ => 0.0,
                };
                (glyph, system.is_some(), scale, font.advance(glyph), kerning)
            };
            pen += kerning * scale;
            if previous.is_some() {
                pen += style.tracking;
            }
            previous = Some((glyph, system));
            if !character.is_whitespace() {
                let whole = pen.floor();
                let fraction = ((pen - whole) * SUBPIXEL_STEPS)
                    .floor()
                    .clamp(0.0, SUBPIXEL_STEPS - 1.0);
                let key = GlyphKey {
                    face: style.face,
                    glyph: glyph.0,
                    size_bits: style.size.to_bits(),
                    subpixel: to_u8(fraction),
                    system,
                };
                if !self.cache.contains_key(&key) {
                    let mask = {
                        let (_, source) = self.source(style.face, character);
                        let font = source.unwrap_or_else(|| self.font(style.face));
                        Self::render_glyph(font, key, fraction / SUBPIXEL_STEPS, scale)
                    };
                    self.cache.insert(key, mask);
                }
                if let Some(mask) = self.cache.get(&key) {
                    if let Ok(reference) = MaskRef::new(&mask.coverage, mask.width, mask.height, to_usize(mask.width)) {
                        let gx = to_px(whole).saturating_add(mask.left);
                        let gy = baseline.saturating_sub(mask.top);
                        surface.blit_mask(reference, gx, gy, color);
                    }
                }
            }
            pen += advance * scale;
        }
    }

    /// Rasterises one glyph's outline from `font`, the font the glyph came from.
    fn render_glyph(font: &Font<'static>, key: GlyphKey, offset_x: f32, scale: f32) -> GlyphMask {
        let mut sink = Collect {
            segments: Vec::new(),
            scale,
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        };
        let empty = GlyphMask {
            left: 0,
            top: 0,
            width: 0,
            height: 0,
            coverage: Vec::new(),
        };
        if font.outline(GlyphId(key.glyph), &mut sink).is_err() || sink.segments.is_empty() {
            return empty;
        }
        // Outline space: y up, scaled. Mask space: y down, origin at the pixel-aligned bounding box.
        let left = (sink.min_x + offset_x).floor();
        let top = sink.max_y.ceil();
        let width = to_u32((sink.max_x + offset_x).ceil() - left);
        let height = to_u32(top - sink.min_y.floor());
        let Some(cells) = width.checked_mul(height) else {
            return empty;
        };
        if width == 0 || height == 0 || cells > 1 << 20 {
            return empty;
        }
        let dx = offset_x - left;
        let segments: Vec<Segment> = sink
            .segments
            .iter()
            .map(|segment| match *segment {
                Segment::MoveTo(x, y) => Segment::MoveTo(x + dx, top - y),
                Segment::LineTo(x, y) => Segment::LineTo(x + dx, top - y),
                Segment::QuadTo(cx, cy, x, y) => Segment::QuadTo(cx + dx, top - cy, x + dx, top - y),
                Segment::CubicTo(ax, ay, bx, by, x, y) => {
                    Segment::CubicTo(ax + dx, top - ay, bx + dx, top - by, x + dx, top - y)
                }
                Segment::Close => Segment::Close,
            })
            .collect();
        let mut coverage = vec![0_u8; to_usize(cells)];
        rasterize(&segments, width, height, &mut coverage);
        GlyphMask {
            left: to_px(left),
            top: to_px(top),
            width,
            height,
            coverage,
        }
    }
}

/// Text metrics of one style, for [`crate::text`] algorithms.
pub struct StyleMetrics<'a> {
    fonts: &'a Fonts,
    style: TextStyle,
}

impl crate::text::Metrics for StyleMetrics<'_> {
    fn advance(&self, character: char) -> f32 {
        let (glyph, system) = self.fonts.source(self.style.face, character);
        let font = system.unwrap_or_else(|| self.fonts.font(self.style.face));
        font.advance(glyph) * self.fonts.unit_scale(self.style, system)
    }

    fn kerning(&self, left: char, right: char) -> f32 {
        // Kerning only inside the bundled face: a system glyph has no pair with a bundled one.
        let (a, a_system) = self.fonts.source(self.style.face, left);
        let (b, b_system) = self.fonts.source(self.style.face, right);
        if a_system.is_some() || b_system.is_some() {
            return 0.0;
        }
        let font = self.fonts.font(self.style.face);
        font.kerning(a, b) * self.fonts.scale(self.style)
    }

    fn tracking(&self) -> f32 {
        self.style.tracking
    }
}

struct Collect {
    segments: Vec<Segment>,
    scale: f32,
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl Collect {
    fn point(&mut self, x: f32, y: f32) -> (f32, f32) {
        let (sx, sy) = (x * self.scale, y * self.scale);
        self.min_x = self.min_x.min(sx);
        self.min_y = self.min_y.min(sy);
        self.max_x = self.max_x.max(sx);
        self.max_y = self.max_y.max(sy);
        (sx, sy)
    }
}

impl OutlineSink for Collect {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.segments.push(Segment::MoveTo(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.point(x, y);
        self.segments.push(Segment::LineTo(x, y));
    }

    fn quad_to(&mut self, control_x: f32, control_y: f32, x: f32, y: f32) {
        let (cx, cy) = self.point(control_x, control_y);
        let (x, y) = self.point(x, y);
        self.segments.push(Segment::QuadTo(cx, cy, x, y));
    }

    fn cubic_to(&mut self, control1_x: f32, control1_y: f32, control2_x: f32, control2_y: f32, x: f32, y: f32) {
        let (ax, ay) = self.point(control1_x, control1_y);
        let (bx, by) = self.point(control2_x, control2_y);
        let (x, y) = self.point(x, y);
        self.segments.push(Segment::CubicTo(ax, ay, bx, by, x, y));
    }

    fn close(&mut self) {
        self.segments.push(Segment::Close);
    }
}

/// Rounds toward zero and clamps a finite pixel value into `i32`; NaN becomes 0.
#[must_use]
pub fn to_px(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation)]
    let clamped = value.clamp(-1_000_000.0, 1_000_000.0) as i32;
    clamped
}

/// Clamps a pixel extent into `u32` (negative and NaN become 0).
#[must_use]
pub fn to_u32(value: f32) -> u32 {
    u32::try_from(to_px(value)).unwrap_or(0)
}

fn to_u8(value: f32) -> u8 {
    u8::try_from(to_px(value)).unwrap_or(0)
}

fn to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fonts() -> Fonts {
        Fonts::bundled().unwrap_or_else(|error| panic!("{error:?}"))
    }

    #[test]
    fn tracking_widens_a_line_by_gaps_between_characters() {
        let fonts = fonts();
        let plain = TextStyle::new(Face::Body, 14.0);
        let spaced = plain.with_tracking(2.0);
        let width = fonts.measure("ABC", spaced) - fonts.measure("ABC", plain);
        assert!((width - 4.0).abs() < 1.0e-3);
    }

    #[test]
    fn medium_heading_is_narrower_than_semi_bold() {
        let fonts = fonts();
        let medium = fonts.measure("ЗАГОЛОВОК", TextStyle::new(Face::HeadingMedium, 16.0));
        let semi_bold = fonts.measure("ЗАГОЛОВОК", TextStyle::new(Face::Heading, 16.0));
        assert!(medium > 0.0 && (medium - semi_bold).abs() > 1.0e-3);
    }
}

#[cfg(test)]
mod system_fallback_tests {
    use super::*;
    use crate::fallback_font::{is_cjk, SystemFaces};

    fn style() -> TextStyle {
        TextStyle::new(Face::Body, 14.0)
    }

    #[test]
    fn only_cjk_scripts_count_as_cjk() {
        assert!(is_cjk('日') && is_cjk('か') && is_cjk('カ') && is_cjk('한') && is_cjk('中') && is_cjk('！'));
        assert!(!is_cjk('Ж') && !is_cjk('а') && !is_cjk('A') && !is_cjk('—') && !is_cjk('·'));
    }

    #[test]
    fn russian_text_never_loads_the_system_font() -> Result<()> {
        let fonts = Fonts::bundled()?;
        let width = fonts.measure("Сохранения · Инвентарь — 9×18 мм → КБ", style());
        assert!(width > 0.0);
        assert!(
            !fonts.system_font_loaded(),
            "Cyrillic and symbols must come from the bundled faces"
        );
        assert!(!fonts.take_system_font_missing());
        Ok(())
    }

    #[test]
    fn a_missing_or_damaged_system_font_does_not_panic() {
        assert!(SystemFaces::from_bytes(b"this is not a font file").is_none());
        assert!(SystemFaces::from_bytes(&[]).is_none());
    }

    #[test]
    fn a_cjk_glyph_is_found_in_a_system_font_when_this_machine_has_one() -> Result<()> {
        let Some(system) = SystemFaces::load("ja") else {
            eprintln!("no system CJK font on this machine; the lookup test is skipped");
            return Ok(());
        };
        assert!(system.glyph('日').is_some(), "the system font must supply a CJK glyph");
        assert!(
            system.glyph('\u{E000}').is_none(),
            "a private-use character is in no font"
        );
        let fonts = Fonts::bundled()?;
        let with_cjk = fonts.measure("日本語", style());
        assert!(with_cjk > 0.0, "a CJK label gets a width from the system font");
        assert!(fonts.system_font_loaded());
        Ok(())
    }
}
