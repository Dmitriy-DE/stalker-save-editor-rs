//! C# 1.3.1 visual tokens. Values are logical pixels; scale only at the UI boundary.

use std::sync::atomic::{AtomicU8, Ordering};

static CURRENT_THEME: AtomicU8 = AtomicU8::new(0);
static CURRENT_ACCENT: AtomicU8 = AtomicU8::new(0);

/// RGB token.
pub type Rgb = u32;
/// StalkerTheme.BgBase.
pub const BG_BASE: Rgb = 0x0C0D0A;
/// StalkerTheme.BgPanel.
pub const BG_PANEL: Rgb = 0x101311;
/// StalkerTheme.BgElevated.
pub const BG_ELEVATED: Rgb = 0x151814;
/// StalkerTheme.BgHover.
pub const BG_HOVER: Rgb = 0x23261F;
/// StalkerTheme.BgInput.
pub const BG_INPUT: Rgb = 0x1A1D17;
/// StalkerTheme.PlateBg.
pub const PLATE_BG: Rgb = 0x161A14;
/// StalkerTheme.BorderSubtle.
pub const BORDER_SUBTLE: Rgb = 0x242922;
/// StalkerTheme.Border.
pub const BORDER: Rgb = 0x33382F;
/// StalkerTheme.BorderMetal; also ScrollBarThumbFill.
pub const BORDER_METAL: Rgb = 0x3D4837;
/// Hover fill of destructive buttons (design D1).
pub const ERROR_SURFACE: Rgb = 0x2A1B1A;
/// StalkerTheme accent, zone/amber.
pub const ACCENT: Rgb = 0xD6A62D;
/// StalkerTheme.BrushAccentDim, zone/amber.
pub const ACCENT_DIM: Rgb = 0x8F6F22;
/// StalkerTheme.BrushAccentHover, zone/amber.
pub const ACCENT_HOVER: Rgb = 0xE5B53C;
/// StalkerTheme.BrushAccentForeground, zone/amber.
pub const ACCENT_FOREGROUND: Rgb = 0x0C0D0A;
/// StalkerTheme.Rust.
pub const RUST: Rgb = 0xA9532F;
/// StalkerTheme.Success.
pub const SUCCESS: Rgb = 0x7BCB62;
/// StalkerTheme.Warning.
pub const WARNING: Rgb = 0xD6A62D;
/// StalkerTheme.Danger.
pub const ERROR: Rgb = 0xD85A45;
/// StalkerTheme.TextPrimary.
pub const TEXT_PRIMARY: Rgb = 0xD8D2BE;
/// StalkerTheme.TextSecondary.
pub const TEXT_SECONDARY: Rgb = 0xA29D90;
/// StalkerTheme.TextMuted and disabled foreground.
pub const TEXT_DISABLED: Rgb = 0x716F67;
/// StalkerTheme.TextKhaki.
pub const TEXT_KHAKI: Rgb = 0xD8BA8C;
/// C# selection resources use BrushAccentDim.
pub const SELECTION: Rgb = ACCENT_DIM;
/// StalkerTheme.BrushBorderFocus.
pub const FOCUS_RING: Rgb = ACCENT;

/// ApplyAppearance dark accent variants.
pub const ACCENTS: [(&str, Rgb); 4] = [
    ("amber", 0xD6A62D),
    ("teal", 0x70BCA6),
    ("blue", 0x78AFE0),
    ("rust", 0xE07A57),
];

/// Colour tokens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Colors {
    /// Background layers.
    pub background: [Rgb; 6],
    /// Subtle, normal and metal borders.
    pub borders: [Rgb; 3],
    /// Primary, secondary, disabled and khaki text.
    pub text: [Rgb; 4],
    /// Accent, dim, hover and foreground.
    pub accent: [Rgb; 4],
    /// Success, warning and error.
    pub state: [Rgb; 3],
    /// Selection fill.
    pub selection: Rgb,
    /// Focus ring.
    pub focus_ring: Rgb,
}

/// One C# text role.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextRole {
    /// Font family.
    pub family: &'static str,
    /// Font size.
    pub size: f32,
    /// Line height.
    pub line_height: f32,
    /// CSS-like weight.
    pub weight: u16,
}

/// Typography from StalkerTheme fonts and repeated view sizes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Typography {
    /// Page title: Oswald.
    pub page_title: TextRole,
    /// Section title: Oswald.
    pub section_title: TextRole,
    /// Body: Liberation Sans Narrow.
    pub body: TextRole,
    /// Caption.
    pub caption: TextRole,
    /// Table/list cell.
    pub table_cell: TextRole,
    /// StalkerButton text.
    pub button: TextRole,
}

/// Repeated metrics from StalkerTheme.cs, MainWindow*.cs and Views/*.cs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Spacing scale used by StackPanel and padding.
    pub spacing: [f32; 9],
    /// CornerRadius values used by common controls.
    pub radii: [f32; 5],
    /// BorderThickness values.
    pub borders: [f32; 2],
    /// Button/input/row heights.
    pub controls: [f32; 3],
    /// Collapsed/expanded sidebar widths.
    pub sidebar_widths: [f32; 2],
    /// Collapsed/expanded sidebar item heights.
    pub sidebar_items: [f32; 2],
    /// Top/status bar heights.
    pub bars: [f32; 2],
    /// Scrollbar thickness.
    pub scrollbar: f32,
    /// Fast/normal animation durations in ms.
    pub animations_ms: [f32; 2],
    /// Overlay shadow blur/y offset.
    pub shadow: [f32; 2],
}

/// Common card padding (design D1: 14–16 instead of 20).
pub const CARD_PADDING: f32 = 16.0;
/// Common card/row gap.
pub const CONTROL_GAP: f32 = 10.0;
/// Common card radius used by Rust screen helpers, matching C# view cards.
pub const CARD_RADIUS: f32 = 4.0;
/// StalkerTheme.StalkerButton / view minimum control height (design D1: 36).
pub const BUTTON_HEIGHT: f32 = 36.0;
/// Common button radius used by view buttons.
pub const BUTTON_RADIUS: f32 = 3.0;

/// Complete theme.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    /// Colours.
    pub colors: Colors,
    /// Text roles.
    pub typography: Typography,
    /// Geometry.
    pub metrics: Metrics,
}

/// Stable C# appearance ids.
pub const THEMES: [(&str, &str); 3] = [("zone", "Зона (тёмная)"), ("clear-sky", "Чистое небо"), ("day", "День")];
/// Stable C# accent ids.
pub const ACCENT_IDS: [&str; 4] = ["amber", "teal", "blue", "rust"];
/// Display names from ACCEPTANCE §22.
pub const ACCENT_NAMES: [&str; 4] = ["Янтарный", "Бирюзовый", "Синий", "Ржавый"];

/// Applies the selected C# appearance to subsequently built widgets.
pub fn apply_appearance(theme_id: &str, accent_id: &str) {
    let theme = match theme_id {
        "day" => 1,
        "clear-sky" => 2,
        _ => 0,
    };
    let accent = match accent_id {
        "teal" => 1,
        "blue" => 2,
        "rust" => 3,
        _ => 0,
    };
    CURRENT_THEME.store(theme, Ordering::Relaxed);
    CURRENT_ACCENT.store(accent, Ordering::Relaxed);
}

/// Current stable theme id.
#[must_use]
pub fn current_theme_id() -> &'static str {
    match CURRENT_THEME.load(Ordering::Relaxed) {
        1 => "day",
        2 => "clear-sky",
        _ => "zone",
    }
}

/// Current stable accent id.
#[must_use]
pub fn current_accent_id() -> &'static str {
    match CURRENT_ACCENT.load(Ordering::Relaxed) {
        1 => "teal",
        2 => "blue",
        3 => "rust",
        _ => "amber",
    }
}

/// Current C# palette with the selected accent.
#[must_use]
pub fn current() -> Theme {
    palette(current_theme_id(), current_accent_id())
}

/// Exact C# 1.3.1 palette for a stable theme/accent id.
#[must_use]
pub fn palette(theme_id: &str, accent_id: &str) -> Theme {
    let mut theme = match theme_id {
        "day" => Theme::with_colors(
            [0xD8D8D0, 0xF0F0E9, 0xFAFAF4, 0xE4E6DD, 0xFFFFFF, 0xE8E9E1],
            [0xC5C9BE, 0xAEB5A8, 0x8F998D],
            [0x202720, 0x4B564E, 0x68736B, 0x46584D],
            [0x347345, 0x8A610C, 0xA43F32],
        ),
        "clear-sky" => Theme::with_colors(
            [0x0C1517, 0x111D1F, 0x17272A, 0x203437, 0x172326, 0x182629],
            [0x263739, 0x35484A, 0x456064],
            [0xDCE5DC, 0xAABBB7, 0x7F928D, 0xBED1C4],
            [0x82CA8B, 0xD5AA52, 0xE06C59],
        ),
        _ => Theme::dark(),
    };
    let (accent, dim, hover, foreground) = match (theme_id, accent_id) {
        ("day", "teal") => (0x176B5E, 0x5C9287, 0x337D71, 0xFFFFFF),
        ("day", "blue") => (0x285F8D, 0x678BA5, 0x42729B, 0xFFFFFF),
        ("day", "rust") => (0x9B452B, 0xB17A66, 0xA75B44, 0xFFFFFF),
        ("day", _) => (0x8A610C, 0xA68C53, 0x987429, 0xFFFFFF),
        ("clear-sky", "teal") => (0x70BCA6, 0x4C8073, 0x7BC1AD, 0x10130F),
        ("clear-sky", "blue") => (0x78AFE0, 0x517898, 0x83B5E2, 0xFFFFFF),
        ("clear-sky", "rust") => (0xE07A57, 0x945640, 0xE28564, 0xFFFFFF),
        ("clear-sky", _) => (0xD6A62D, 0x8D7225, 0xD9AD3E, 0xFFFFFF),
        (_, "teal") => (0x70BCA6, 0x4C7D6E, 0x7BC1AD, 0x10130F),
        (_, "blue") => (0x78AFE0, 0x517593, 0x83B5E2, 0xFFFFFF),
        (_, "rust") => (0xE07A57, 0x94533B, 0xE28564, 0xFFFFFF),
        _ => (0xD6A62D, 0x8D6F20, 0xD9AD3E, 0xFFFFFF),
    };
    theme.colors.accent = [accent, dim, hover, foreground];
    theme.colors.selection = dim;
    theme.colors.focus_ring = accent;
    theme
}

impl Theme {
    /// Shipped C# zone/amber theme.
    #[must_use]
    pub const fn dark() -> Self {
        Self {
            colors: Colors {
                background: [BG_BASE, BG_PANEL, BG_ELEVATED, BG_HOVER, BG_INPUT, PLATE_BG],
                borders: [BORDER_SUBTLE, BORDER, BORDER_METAL],
                text: [TEXT_PRIMARY, TEXT_SECONDARY, TEXT_DISABLED, TEXT_KHAKI],
                accent: [ACCENT, ACCENT_DIM, ACCENT_HOVER, ACCENT_FOREGROUND],
                state: [SUCCESS, WARNING, ERROR],
                selection: SELECTION,
                focus_ring: FOCUS_RING,
            },
            typography: Typography {
                page_title: TextRole {
                    family: "Oswald",
                    size: 28.0,
                    line_height: 34.0,
                    weight: 600,
                },
                section_title: TextRole {
                    family: "Oswald",
                    size: 18.0,
                    line_height: 24.0,
                    weight: 600,
                },
                body: TextRole {
                    family: "Liberation Sans Narrow",
                    size: 13.0,
                    line_height: 18.0,
                    weight: 400,
                },
                caption: TextRole {
                    family: "Liberation Sans Narrow",
                    size: 11.0,
                    line_height: 15.0,
                    weight: 400,
                },
                table_cell: TextRole {
                    family: "Liberation Sans Narrow",
                    size: 12.0,
                    line_height: 17.0,
                    weight: 400,
                },
                button: TextRole {
                    family: "Liberation Sans Narrow",
                    size: 12.0,
                    line_height: 16.0,
                    weight: 600,
                },
            },
            metrics: Metrics {
                spacing: [2.0, 4.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 24.0],
                radii: [1.0, 2.0, 3.0, 4.0, 6.0],
                borders: [1.0, 2.0],
                controls: [34.0, 34.0, 40.0],
                sidebar_widths: [72.0, 240.0],
                sidebar_items: [46.0, 46.0],
                bars: [54.0, 33.0],
                scrollbar: 10.0,
                animations_ms: [120.0, 180.0],
                shadow: [24.0, 8.0],
            },
        }
    }

    const fn with_colors(background: [Rgb; 6], borders: [Rgb; 3], text: [Rgb; 4], state: [Rgb; 3]) -> Self {
        let base = Self::dark();
        Self {
            colors: Colors {
                background,
                borders,
                text,
                state,
                ..base.colors
            },
            ..base
        }
    }

    /// Scale 1.0–2.0 like StalkerTheme.CurrentUiScalePercent (100–200).
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn scaled(self, scale: f32) -> Self {
        let factor = scale.clamp(1.0, 2.0);
        let px = |v: f32| (v * factor).round().max(1.0);
        let role = |r: TextRole| TextRole {
            size: px(r.size),
            line_height: px(r.line_height),
            ..r
        };
        let mut spacing = self.metrics.spacing;
        let mut radii = self.metrics.radii;
        let mut borders = self.metrics.borders;
        let mut controls = self.metrics.controls;
        let mut sidebar_widths = self.metrics.sidebar_widths;
        let mut sidebar_items = self.metrics.sidebar_items;
        let mut bars = self.metrics.bars;
        let mut animations_ms = self.metrics.animations_ms;
        let mut shadow = self.metrics.shadow;
        for value in &mut spacing {
            *value = px(*value);
        }
        for value in &mut radii {
            *value = px(*value);
        }
        for value in &mut borders {
            *value = px(*value);
        }
        for value in &mut controls {
            *value = px(*value);
        }
        for value in &mut sidebar_widths {
            *value = px(*value);
        }
        for value in &mut sidebar_items {
            *value = px(*value);
        }
        for value in &mut bars {
            *value = px(*value);
        }
        for value in &mut animations_ms {
            *value = px(*value);
        }
        for value in &mut shadow {
            *value = px(*value);
        }
        Self {
            colors: self.colors,
            typography: Typography {
                page_title: role(self.typography.page_title),
                section_title: role(self.typography.section_title),
                body: role(self.typography.body),
                caption: role(self.typography.caption),
                table_cell: role(self.typography.table_cell),
                button: role(self.typography.button),
            },
            metrics: Metrics {
                spacing,
                radii,
                borders,
                controls,
                sidebar_widths,
                sidebar_items,
                bars,
                scrollbar: px(self.metrics.scrollbar),
                animations_ms,
                shadow,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn scale_is_clamped_and_pixel_aligned() {
        for scale in [1.0_f32, 1.25, 1.5, 1.75, 2.0] {
            let theme = Theme::dark().scaled(scale);
            assert!(theme.metrics.spacing.iter().all(|v| v.fract() == 0.0 && *v >= 1.0));
            assert!(theme.metrics.controls.iter().all(|v| v.fract() == 0.0));
            assert_eq!(theme.typography.body.size.fract(), 0.0);
        }
        assert_eq!(Theme::dark().scaled(0.5), Theme::dark().scaled(1.0));
        assert_eq!(Theme::dark().scaled(3.0), Theme::dark().scaled(2.0));
    }
}
