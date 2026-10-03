//! C# 1.3.1 visual tokens. Values are logical pixels; scale only at the UI boundary.

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
    ("amber", 0xD6A62D), ("teal", 0x70BCA6), ("blue", 0x78AFE0), ("rust", 0xE07A57),
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

/// Common card padding from StalkerTheme.Card / views.
pub const CARD_PADDING: f32 = 20.0;
/// Common card/row gap.
pub const CONTROL_GAP: f32 = 10.0;
/// Common card radius used by Rust screen helpers, matching C# view cards.
pub const CARD_RADIUS: f32 = 4.0;
/// StalkerTheme.StalkerButton / view minimum control height.
pub const BUTTON_HEIGHT: f32 = 34.0;
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
                page_title: TextRole { family: "Oswald", size: 28.0, line_height: 34.0, weight: 600 },
                section_title: TextRole { family: "Oswald", size: 18.0, line_height: 24.0, weight: 600 },
                body: TextRole { family: "Liberation Sans Narrow", size: 13.0, line_height: 18.0, weight: 400 },
                caption: TextRole { family: "Liberation Sans Narrow", size: 11.0, line_height: 15.0, weight: 400 },
                table_cell: TextRole { family: "Liberation Sans Narrow", size: 12.0, line_height: 17.0, weight: 400 },
                button: TextRole { family: "Liberation Sans Narrow", size: 12.0, line_height: 16.0, weight: 600 },
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

    /// Scale 1.0–2.0 like StalkerTheme.CurrentUiScalePercent (100–200).
    #[must_use]
    #[allow(clippy::arithmetic_side_effects)]
    pub fn scaled(self, scale: f32) -> Self {
        let factor = scale.clamp(1.0, 2.0);
        let px = |v: f32| (v * factor).round().max(1.0);
        let role = |r: TextRole| TextRole { size: px(r.size), line_height: px(r.line_height), ..r };
        let mut spacing = self.metrics.spacing;
        let mut radii = self.metrics.radii;
        let mut borders = self.metrics.borders;
        let mut controls = self.metrics.controls;
        let mut sidebar_widths = self.metrics.sidebar_widths;
        let mut sidebar_items = self.metrics.sidebar_items;
        let mut bars = self.metrics.bars;
        let mut animations_ms = self.metrics.animations_ms;
        let mut shadow = self.metrics.shadow;
        for values in [&mut spacing[..], &mut radii[..], &mut borders[..], &mut controls[..],
            &mut sidebar_widths[..], &mut sidebar_items[..], &mut bars[..], &mut animations_ms[..], &mut shadow[..]] {
            for value in values { *value = px(*value); }
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
                spacing, radii, borders, controls, sidebar_widths, sidebar_items, bars,
                scrollbar: px(self.metrics.scrollbar), animations_ms, shadow,
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
