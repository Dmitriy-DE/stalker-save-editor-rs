//! Cached vector icons and C#-style icon button variants.

use crate::path::{FillRule, FlattenedPath, Icon, Path, Point, StrokeStyle, Transform};
use sse_core::{Error, Result};

const MAX_CACHE: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
/// Rasterised icon mask plus the colour used as its cache key.
pub struct IconBitmap {
    /// Square side in pixels.
    pub size: u16,
    /// RGB foreground colour.
    pub color: u32,
    /// Row-major 8-bit coverage mask.
    pub alpha: Vec<u8>,
}

struct Entry {
    icon: Icon,
    bitmap: IconBitmap,
    stamp: u64,
}

/// Bounded LRU-ish cache. Path rasterisation happens once per icon/size/colour tuple.
/// Bounded cache of rasterised path icons.
pub struct IconCache {
    entries: Vec<Entry>,
    clock: u64,
}

impl IconCache {
    /// Create an empty cache.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
        }
    }

    /// Get or rasterise an icon for this size/colour tuple.
    pub fn get(&mut self, icon: Icon, size: u16, color: u32) -> Result<&IconBitmap> {
        self.clock = self.clock.saturating_add(1);
        if let Some(i) = self
            .entries
            .iter()
            .position(|e| e.icon == icon && e.bitmap.size == size && e.bitmap.color == color)
        {
            if let Some(e) = self.entries.get_mut(i) {
                e.stamp = self.clock;
            }
            return self
                .entries
                .get(i)
                .map(|e| &e.bitmap)
                .ok_or_else(|| Error::damaged("icon cache"));
        }
        if size == 0 || size > 512 {
            return Err(Error::Refused("icon size must be 1..=512".to_owned()));
        }
        let bitmap = rasterize(icon, size, color)?;
        if self.entries.len() >= MAX_CACHE {
            let victim = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, e)| e.stamp)
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.entries.remove(victim);
        }
        self.entries.push(Entry {
            icon,
            bitmap,
            stamp: self.clock,
        });
        self.entries
            .last()
            .map(|e| &e.bitmap)
            .ok_or_else(|| Error::damaged("icon cache insert"))
    }

    /// Number of cached variants.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no icon variants are cached.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for IconCache {
    fn default() -> Self {
        Self::new()
    }
}

fn rasterize(icon: Icon, size: u16, color: u32) -> Result<IconBitmap> {
    let scale = f64::from(size) / 24.0;
    let transform = Transform::scale(scale, scale);
    let outline: Option<FlattenedPath> = if icon.stroke_width() > 0.0 && !icon.path_data().is_empty() {
        Some(icon.path()?.stroke_to_fill(
            transform,
            0.25,
            StrokeStyle {
                width: icon.stroke_width() * scale,
                ..StrokeStyle::icon()
            },
        )?)
    } else {
        None
    };
    let filled: Option<FlattenedPath> = if icon.fill_data().is_empty() {
        None
    } else {
        Some(Path::parse(icon.fill_data())?.flatten(transform, 0.25, FillRule::NonZero)?)
    };
    coverage(size, color, outline.as_ref(), filled.as_ref())
}

fn coverage(
    size: u16,
    color: u32,
    outline: Option<&FlattenedPath>,
    filled: Option<&FlattenedPath>,
) -> Result<IconBitmap> {
    let side = usize::from(size);
    let count = side
        .checked_mul(side)
        .ok_or_else(|| Error::damaged("icon bitmap overflow"))?;
    let mut alpha = vec![0u8; count];
    for y in 0..side {
        for x in 0..side {
            let mut covered = 0u8;
            for sy in [0.125, 0.375, 0.625, 0.875] {
                for sx in [0.125, 0.375, 0.625, 0.875] {
                    let point = Point::new(
                        f64::from(u16::try_from(x).unwrap_or_default()) + sx,
                        f64::from(u16::try_from(y).unwrap_or_default()) + sy,
                    );
                    if outline.is_some_and(|flat| flat.contains(point))
                        || filled.is_some_and(|flat| flat.contains(point))
                    {
                        covered = covered.saturating_add(1);
                    }
                }
            }
            let i = y
                .checked_mul(side)
                .and_then(|v| v.checked_add(x))
                .ok_or_else(|| Error::damaged("icon pixel"))?;
            if let Some(p) = alpha.get_mut(i) {
                *p = u8::try_from(u32::from(covered).saturating_mul(255) / 16).unwrap_or(u8::MAX);
            }
        }
    }
    Ok(IconBitmap { size, color, alpha })
}

/// C# button visual role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconButtonKind {
    /// Filled accent action.
    Primary,
    /// Neutral elevated action.
    Secondary,
    /// Destructive outlined action.
    Danger,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Palette resolved for an icon-button role.
pub struct IconButtonLook {
    /// Normal fill RGB.
    pub fill: u32,
    /// Hover fill RGB.
    pub hover: u32,
    /// Border RGB.
    pub border: u32,
    /// Icon/text RGB.
    pub foreground: u32,
}

/// Resolve a C#-style button role to colours.
#[must_use]
pub const fn button_look(kind: IconButtonKind) -> IconButtonLook {
    match kind {
        IconButtonKind::Primary => IconButtonLook {
            fill: 0xD6A62D,
            hover: 0xE5B53C,
            border: 0xD6A62D,
            foreground: 0x0C0D0A,
        },
        IconButtonKind::Secondary => IconButtonLook {
            fill: 0x151814,
            hover: 0x23261F,
            border: 0x33382F,
            foreground: 0xD8D2BE,
        },
        IconButtonKind::Danger => IconButtonLook {
            fill: 0x151814,
            hover: 0x3A211C,
            border: 0xD85A45,
            foreground: 0xD85A45,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_reuses_and_bounds() {
        let mut c = IconCache::new();
        let a = c
            .get(Icon::Save, 24, 0xffffff)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .alpha
            .clone();
        let b = c
            .get(Icon::Save, 24, 0xffffff)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .alpha
            .clone();
        assert_eq!(a, b);
        assert_eq!(c.len(), 1);
        assert!(a.iter().any(|v| *v != 0));
    }

    #[test]
    fn variants_are_distinct() {
        assert_ne!(
            button_look(IconButtonKind::Primary),
            button_look(IconButtonKind::Danger)
        );
    }

    #[test]
    fn d2_icons_rasterise_with_pixels_at_22_and_16() {
        use crate::path::D2_ICONS;
        let mut cache = IconCache::new();
        for icon in D2_ICONS {
            for size in [22_u16, 16] {
                let bitmap = cache
                    .get(icon, size, 0xD8D2BE)
                    .unwrap_or_else(|error| panic!("{icon:?}: {error:?}"));
                assert_eq!(bitmap.alpha.len(), usize::from(size) * usize::from(size));
                assert!(
                    bitmap.alpha.iter().any(|value| *value > 0),
                    "{icon:?} at {size} is empty"
                );
            }
        }
    }

    #[test]
    fn filled_square_at_center_gives_full_coverage() {
        let square = Path::parse("M0 0H24V24H0Z")
            .and_then(|path| path.flatten(Transform::identity(), 0.25, FillRule::NonZero))
            .unwrap_or_else(|error| panic!("{error:?}"));
        let bitmap = coverage(24, 0xffffff, None, Some(&square)).unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(bitmap.alpha.get(12 * 24 + 12).copied(), Some(255));
    }

    #[test]
    fn more_dots_are_filled_discs() {
        let mut cache = IconCache::new();
        let bitmap = cache
            .get(Icon::D2More, 24, 0xffffff)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(bitmap.alpha.get(4 * 24 + 11).copied(), Some(255));
        assert_eq!(bitmap.alpha.first().copied(), Some(0));
    }

    #[test]
    fn outline_stroke_width_two_has_strong_coverage() {
        let mut cache = IconCache::new();
        let bitmap = cache
            .get(Icon::Search, 24, 0xffffff)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(bitmap.alpha.iter().any(|&v| v >= 200));
    }
}
