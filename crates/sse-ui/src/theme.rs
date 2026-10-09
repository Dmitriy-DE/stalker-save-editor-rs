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

/// Design D2 tokens: colours, control states, type and metrics from `STYLE-DATA.js.txt`.
///
/// Colours are `0xRRGGBBAA` (opaque colours carry `FF`; `transparent` is `0x00000000`). The state arrays keep
/// the order given in the spec, listed on each constant.
pub mod d2 {
    /// Fill, border and text colour of one control state, and the focus ring colour (0 when there is none).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct StateLook {
        /// Fill colour.
        pub fill: u32,
        /// Border colour.
        pub border: u32,
        /// Text colour.
        pub text: u32,
        /// Focus ring colour, 0 when the state draws no ring.
        pub ring: u32,
    }

    /// Text input state: fill, border, text, placeholder/value colour and icon colour.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct InputLook {
        /// Fill colour.
        pub fill: u32,
        /// Border colour.
        pub border: u32,
        /// Text colour.
        pub text: u32,
        /// Icon colour.
        pub icon: u32,
    }

    /// Badge colours: text, border and fill.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct BadgeLook {
        /// Text colour.
        pub text: u32,
        /// Border colour.
        pub border: u32,
        /// Fill colour.
        pub fill: u32,
    }

    /// One text role: face, weight, size and line height in pixels, tracking in pixels, colour.
    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct TextSpec {
        /// Font family name.
        pub face: &'static str,
        /// CSS-like weight.
        pub weight: u16,
        /// Font size.
        pub size: f32,
        /// Line height.
        pub line_height: f32,
        /// Letter spacing.
        pub tracking: f32,
        /// Text colour.
        pub color: u32,
    }

    // ---- colours ----

    // ФОН И ЗАТЕМНЕНИЕ АРТА
    /// фон окна, пока арт не загружен (`#0C0D0A`).
    pub const BG_BASE: u32 = 0x0C0D0AFF;
    /// меню и строка состояния (`#0A0B09`).
    pub const BG_SIDEBAR: u32 = 0x0A0B09FF;
    /// поверх art-sidebar (`#0A0B09B3`).
    pub const SCRIM_SIDEBAR: u32 = 0x0A0B09B3;
    /// верхняя полоса 44 (`#0C0D0A99`).
    pub const SCRIM_TOPBAR: u32 = 0x0C0D0A99;
    /// поверх art-header, зона заголовка (`#0C0D0A33`).
    pub const SCRIM_HEADER: u32 = 0x0C0D0A33;
    /// поверх art-window ниже шапки (`#0C0D0AB3`).
    pub const SCRIM_CONTENT: u32 = 0x0C0D0AB3;

    // ПАНЕЛИ И ЭЛЕМЕНТЫ
    /// панели и карточки поверх арта (`#101311E6`).
    pub const PANEL: u32 = 0x101311E6;
    /// плитки и блоки внутри панели (`#151814F2`).
    pub const PANEL_RAISED: u32 = 0x151814F2;
    /// вторичная кнопка, вкладка (`#151814CC`).
    pub const CONTROL: u32 = 0x151814CC;
    /// наведение на кнопку и вкладку (`#23261F`).
    pub const HOVER: u32 = 0x23261FFF;
    /// наведение на строку списка (`#23261FCC`).
    pub const ROW_HOVER: u32 = 0x23261FCC;
    /// поля ввода и списки выбора (`#1A1D17`).
    pub const INPUT: u32 = 0x1A1D17FF;

    // РАМКИ
    /// разделители внутри панели (`#242922`).
    pub const BORDER_SUBTLE: u32 = 0x242922FF;
    /// панели, вкладки, поля (`#33382F`).
    pub const BORDER: u32 = 0x33382FFF;
    /// кнопки, плашки (`#3D4837`).
    pub const BORDER_METAL: u32 = 0x3D4837FF;
    /// наведение на кнопку (`#716F67`).
    pub const BORDER_STRONG: u32 = 0x716F67FF;

    // ТЕКСТ
    /// основной (`#D8D2BE`).
    pub const TEXT_PRIMARY: u32 = 0xD8D2BEFF;
    /// подписи, неактивные вкладки (`#A29D90`).
    pub const TEXT_SECONDARY: u32 = 0xA29D90FF;
    /// примечания, ключи секций (`#716F67`).
    pub const TEXT_MUTED: u32 = 0x716F67FF;
    /// недоступные элементы (`#4D4C46`).
    pub const TEXT_DISABLED: u32 = 0x4D4C46FF;
    /// значения: деньги, счётчики (`#D8BA8C`).
    pub const TEXT_VALUE: u32 = 0xD8BA8CFF;
    /// текст на янтаре (`#0C0D0A`).
    pub const TEXT_ON_ACCENT: u32 = 0x0C0D0AFF;

    // АКЦЕНТ
    /// основная кнопка, выбранное (`#D6A62D`).
    pub const ACCENT: u32 = 0xD6A62DFF;
    /// наведение; текст акцентом (`#E5B53C`).
    pub const ACCENT_HOVER: u32 = 0xE5B53CFF;
    /// нажатие (`#B88E22`).
    pub const ACCENT_PRESSED: u32 = 0xB88E22FF;
    /// фон выбранной строки (`#D6A62D1A`).
    pub const ACCENT_TINT: u32 = 0xD6A62D1A;
    /// наведение на контурную кнопку (`#D6A62D26`).
    pub const ACCENT_TINT_STRONG: u32 = 0xD6A62D26;
    /// рамка плашки черновика (`#D6A62D66`).
    pub const ACCENT_BORDER: u32 = 0xD6A62D66;

    // СОСТОЯНИЯ
    /// ОК, установлено, ≥75% (`#7BCB62`).
    pub const SUCCESS: u32 = 0x7BCB62FF;
    /// фон плашки (`#7BCB621F`).
    pub const SUCCESS_FILL: u32 = 0x7BCB621F;
    /// ошибка, удаление, <40% (`#D85A45`).
    pub const DANGER: u32 = 0xD85A45FF;
    /// фон плашки (`#D85A451F`).
    pub const DANGER_FILL: u32 = 0xD85A451F;
    /// Steam Cloud (`#78AFE0`).
    pub const INFO: u32 = 0x78AFE0FF;
    /// фон плашки (`#78AFE01F`).
    pub const INFO_FILL: u32 = 0x78AFE01F;

    // ---- buttons: order normal, hover, pressed, focus, disabled ----

    /// Основная button (label «СОХРАНИТЬ», icon save, weight 600).
    /// Order: normal, hover, pressed, focus, disabled.
    pub const BUTTON_PRIMARY: [StateLook; 5] = [
        // заливка #D6A62D · текст #0C0D0A
        StateLook {
            fill: 0xD6A62DFF,
            border: 0xD6A62DFF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // заливка #E5B53C
        StateLook {
            fill: 0xE5B53CFF,
            border: 0xE5B53CFF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // заливка #B88E22
        StateLook {
            fill: 0xB88E22FF,
            border: 0xB88E22FF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // кольцо 2 · #D6A62D · зазор 2
        StateLook {
            fill: 0xD6A62DFF,
            border: 0xD6A62DFF,
            text: 0x0C0D0AFF,
            ring: 0xD6A62DFF,
        },
        // заливка #D6A62D33 · текст #8F7A45
        StateLook {
            fill: 0xD6A62D33,
            border: 0xD6A62D33,
            text: 0x8F7A45FF,
            ring: 0,
        },
    ];

    /// Вторичная button (label «ОТМЕНИТЬ», icon undo, weight 500).
    /// Order: normal, hover, pressed, focus, disabled.
    pub const BUTTON_SECONDARY: [StateLook; 5] = [
        // заливка #151814CC · рамка #3D4837
        StateLook {
            fill: 0x151814CC,
            border: 0x3D4837FF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #23261F · рамка #716F67
        StateLook {
            fill: 0x23261FFF,
            border: 0x716F67FF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #1A1D17 · рамка #D6A62D
        StateLook {
            fill: 0x1A1D17FF,
            border: 0xD6A62DFF,
            text: 0xE5B53CFF,
            ring: 0,
        },
        // кольцо 2 · #D6A62D
        StateLook {
            fill: 0x151814CC,
            border: 0x3D4837FF,
            text: 0xD8D2BEFF,
            ring: 0xD6A62DFF,
        },
        // заливка #15181466 · рамка #242922
        StateLook {
            fill: 0x15181466,
            border: 0x242922FF,
            text: 0x4D4C46FF,
            ring: 0,
        },
    ];

    /// Контурная акцент button (label «ПРОВЕРИТЬ», icon check, weight 500).
    /// Order: normal, hover, pressed, focus, disabled.
    pub const BUTTON_ACCENT_OUTLINE: [StateLook; 5] = [
        // рамка #D6A62D · текст #E5B53C
        StateLook {
            fill: 0x151814CC,
            border: 0xD6A62DFF,
            text: 0xE5B53CFF,
            ring: 0,
        },
        // заливка #D6A62D26
        StateLook {
            fill: 0xD6A62D26,
            border: 0xE5B53CFF,
            text: 0xE5B53CFF,
            ring: 0,
        },
        // заливка #D6A62D40
        StateLook {
            fill: 0xD6A62D40,
            border: 0xB88E22FF,
            text: 0xE5B53CFF,
            ring: 0,
        },
        // кольцо 2
        StateLook {
            fill: 0x151814CC,
            border: 0xD6A62DFF,
            text: 0xE5B53CFF,
            ring: 0xD6A62DFF,
        },
        // рамка #D6A62D40
        StateLook {
            fill: 0x00000000,
            border: 0xD6A62D40,
            text: 0xD6A62D66,
            ring: 0,
        },
    ];

    /// Опасная button (label «УДАЛИТЬ», icon trash, weight 500).
    /// Order: normal, hover, pressed, focus, disabled.
    pub const BUTTON_DANGER: [StateLook; 5] = [
        // рамка и текст #D85A45
        StateLook {
            fill: 0x00000000,
            border: 0xD85A45FF,
            text: 0xD85A45FF,
            ring: 0,
        },
        // заливка #D85A4526
        StateLook {
            fill: 0xD85A4526,
            border: 0xD85A45FF,
            text: 0xE87563FF,
            ring: 0,
        },
        // заливка #D85A4540
        StateLook {
            fill: 0xD85A4540,
            border: 0xD85A45FF,
            text: 0xE87563FF,
            ring: 0,
        },
        // кольцо 2 · #D6A62D
        StateLook {
            fill: 0x00000000,
            border: 0xD85A45FF,
            text: 0xD85A45FF,
            ring: 0xD6A62DFF,
        },
        // рамка #D85A4540
        StateLook {
            fill: 0x00000000,
            border: 0xD85A4540,
            text: 0xD85A4566,
            ring: 0,
        },
    ];

    // ---- elements: order normal, hover, pressed, selected/focus, disabled ----

    /// ВКЛАДКА · 36 (height 36, Oswald 13).
    /// Order as in the spec.
    pub const TAB_STATES: [StateLook; 5] = [
        // заливка #151814CC · рамка #33382F · текст #A29D90
        StateLook {
            fill: 0x151814CC,
            border: 0x33382FFF,
            text: 0xA29D90FF,
            ring: 0,
        },
        // заливка #23261F · рамка #3D4837
        StateLook {
            fill: 0x23261FFF,
            border: 0x3D4837FF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // рамка #D6A62D
        StateLook {
            fill: 0x1A1D17FF,
            border: 0xD6A62DFF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #D6A62D · текст #0C0D0A
        StateLook {
            fill: 0xD6A62DFF,
            border: 0xD6A62DFF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // текст #4D4C46
        StateLook {
            fill: 0x15181466,
            border: 0x242922FF,
            text: 0x4D4C46FF,
            ring: 0,
        },
    ];

    /// ПУНКТ МЕНЮ · 48 (height 48, Oswald 16).
    /// Order as in the spec.
    pub const MENU_ITEM_STATES: [StateLook; 5] = [
        // без заливки · текст и значок #A29D90
        StateLook {
            fill: 0x00000000,
            border: 0x00000000,
            text: 0xA29D90FF,
            ring: 0,
        },
        // заливка #23261FCC
        StateLook {
            fill: 0x23261FCC,
            border: 0x00000000,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #D6A62D26
        StateLook {
            fill: 0xD6A62D26,
            border: 0x00000000,
            text: 0xE5B53CFF,
            ring: 0,
        },
        // заливка #D6A62D · текст #0C0D0A
        StateLook {
            fill: 0xD6A62DFF,
            border: 0xD6A62DFF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // текст #4D4C46
        StateLook {
            fill: 0x00000000,
            border: 0x00000000,
            text: 0x4D4C46FF,
            ring: 0,
        },
    ];

    /// СТРОКА СПИСКА · 44 / 70 (height 44, Liberation Sans Narrow 14).
    /// Order as in the spec.
    pub const LIST_ROW_STATES: [StateLook; 5] = [
        // без заливки
        StateLook {
            fill: 0x00000000,
            border: 0x00000000,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #23261FCC
        StateLook {
            fill: 0x23261FCC,
            border: 0x00000000,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #D6A62D2E
        StateLook {
            fill: 0xD6A62D2E,
            border: 0x00000000,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #D6A62D1A · рамка #D6A62D
        StateLook {
            fill: 0xD6A62D1A,
            border: 0xD6A62DFF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // текст #4D4C46
        StateLook {
            fill: 0x00000000,
            border: 0x00000000,
            text: 0x4D4C46FF,
            ring: 0,
        },
    ];

    /// ФИЛЬТР-ЧИП · 28 (height 28, Oswald 12).
    /// Order as in the spec.
    pub const FILTER_CHIP_STATES: [StateLook; 5] = [
        // скругление 2
        StateLook {
            fill: 0x151814CC,
            border: 0x33382FFF,
            text: 0xA29D90FF,
            ring: 0,
        },
        // заливка #23261F
        StateLook {
            fill: 0x23261FFF,
            border: 0x3D4837FF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // рамка #D6A62D
        StateLook {
            fill: 0x1A1D17FF,
            border: 0xD6A62DFF,
            text: 0xD8D2BEFF,
            ring: 0,
        },
        // заливка #D6A62D
        StateLook {
            fill: 0xD6A62DFF,
            border: 0xD6A62DFF,
            text: 0x0C0D0AFF,
            ring: 0,
        },
        // текст #4D4C46
        StateLook {
            fill: 0x15181466,
            border: 0x242922FF,
            text: 0x4D4C46FF,
            ring: 0,
        },
    ];

    // ---- inputs: order normal, hover, focus, error, disabled ----

    /// Text input states (normal, hover, focus, error, disabled).
    /// Order as in the spec.
    pub const INPUT_STATES: [InputLook; 5] = [
        // заливка #1A1D17 · рамка #33382F · подсказка #716F67 · значок #716F67
        InputLook {
            fill: 0x1A1D17FF,
            border: 0x33382FFF,
            text: 0x716F67FF,
            icon: 0x716F67FF,
        },
        // рамка #3D4837 · значок #A29D90
        InputLook {
            fill: 0x1A1D17FF,
            border: 0x3D4837FF,
            text: 0x716F67FF,
            icon: 0xA29D90FF,
        },
        // рамка 1 · #D6A62D, курсор #E5B53C · значок #A29D90
        InputLook {
            fill: 0x1A1D17FF,
            border: 0xD6A62DFF,
            text: 0xD8D2BEFF,
            icon: 0xA29D90FF,
        },
        // рамка #D85A45 · причина под полем 12 · #D85A45 · значок #D85A45
        InputLook {
            fill: 0x1A1D17FF,
            border: 0xD85A45FF,
            text: 0xD8D2BEFF,
            icon: 0xD85A45FF,
        },
        // текст #4D4C46 · значок #4D4C46
        InputLook {
            fill: 0x15181466,
            border: 0x242922FF,
            text: 0x4D4C46FF,
            icon: 0x4D4C46FF,
        },
    ];

    // ---- badges ----
    /// УСТАНОВЛЕНО: успех: установлено, найдено, проверенная сборка.
    pub const BADGE_INSTALLED: BadgeLook = BadgeLook {
        text: 0x7BCB62FF,
        border: 0x7BCB6266,
        fill: 0x7BCB621F,
    };
    /// ОБНОВЛЕНИЕ ДОСТУПНО: внимание: обновление, черновик, экспериментально.
    pub const BADGE_UPDATE: BadgeLook = BadgeLook {
        text: 0xD6A62DFF,
        border: 0xD6A62D66,
        fill: 0xD6A62D1F,
    };
    /// ФАЙЛ ИЗМЕНЁН ПОСЛЕ УСТАНОВКИ: ошибка, конфликт, повреждён.
    pub const BADGE_CHANGED: BadgeLook = BadgeLook {
        text: 0xD85A45FF,
        border: 0xD85A4566,
        fill: 0xD85A451F,
    };
    /// STEAM CLOUD: источник: облако.
    pub const BADGE_CLOUD: BadgeLook = BadgeLook {
        text: 0x78AFE0FF,
        border: 0x78AFE066,
        fill: 0x78AFE01F,
    };
    /// НЕ УСТАНОВЛЕНО: нейтрально.
    pub const BADGE_NOT_INSTALLED: BadgeLook = BadgeLook {
        text: 0xA29D90FF,
        border: 0x3D4837FF,
        fill: 0x00000000,
    };
    /// СЛОТ: размещение, источник установки (Steam / GOG).
    pub const BADGE_SLOT: BadgeLook = BadgeLook {
        text: 0xD8BA8CFF,
        border: 0x3D4837FF,
        fill: 0x00000000,
    };
    /// Черновик: 3 действ.: бейдж черновика: Liberation 12, высота 24.
    pub const BADGE_DRAFT: BadgeLook = BadgeLook {
        text: 0xE5B53CFF,
        border: 0xD6A62D66,
        fill: 0xD6A62D1A,
    };

    // ---- type ----
    /// Заголовок страницы — Oswald 600 · 36/42 · 28/32 · трекинг 1 · заглавные. Sample: «СОХРАНЕНИЯ».
    pub const TYPE_PAGE_TITLE: TextSpec = TextSpec {
        face: "Oswald",
        weight: 600,
        size: 36.0,
        line_height: 42.0,
        tracking: 1.0,
        color: 0xD8D2BEFF,
    };
    /// Подзаголовок страницы — Liberation Sans Narrow 400 · 15/20. Sample: «Библиотека, черновик правок и проверка перед записью.».
    pub const TYPE_PAGE_SUBTITLE: TextSpec = TextSpec {
        face: "Liberation Sans Narrow",
        weight: 400,
        size: 15.0,
        line_height: 20.0,
        tracking: 0.0,
        color: 0xA29D90FF,
    };
    /// Пункт меню — Oswald 500 · 16/20 · трекинг 0,6 · заглавные. Sample: «ЭНЦИКЛОПЕДИЯ».
    pub const TYPE_MENU_ITEM: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 16.0,
        line_height: 20.0,
        tracking: 0.6,
        color: 0xA29D90FF,
    };
    /// Заголовок панели — Oswald 500 · 14/18 · трекинг 1,2 · заглавные · акцент. Sample: «ПАРАМЕТРЫ СТАЛКЕРА».
    pub const TYPE_PANEL_TITLE: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 14.0,
        line_height: 18.0,
        tracking: 1.2,
        color: 0xD6A62DFF,
    };
    /// Вкладка / кнопка — Oswald 500 · 13/16 и 14/16 · трекинг 0,6. Sample: «УСТАНОВИТЬ ВЫБРАННОЕ».
    pub const TYPE_TAB_BUTTON: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 14.0,
        line_height: 16.0,
        tracking: 0.6,
        color: 0xD8D2BEFF,
    };
    /// Крупное значение — Oswald 500 · 22/26 · khaki. Sample: «48 650 RU».
    pub const TYPE_VALUE_LARGE: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 22.0,
        line_height: 26.0,
        tracking: 0.0,
        color: 0xD8BA8CFF,
    };
    /// Имя файла / предмета — Oswald 500 · 22/28 (шапка) · 20/25 (инспектор). Sample: «pripyat_hospital.scop».
    pub const TYPE_FILE_NAME: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 22.0,
        line_height: 28.0,
        tracking: 0.0,
        color: 0xD8D2BEFF,
    };
    /// Текст — Liberation Sans Narrow 400 · 14/18. Sample: «Задание выдаёт командир Долга, но в записи указана группировка «Чистое Небо».».
    pub const TYPE_TEXT: TextSpec = TextSpec {
        face: "Liberation Sans Narrow",
        weight: 400,
        size: 14.0,
        line_height: 18.0,
        tracking: 0.0,
        color: 0xD8D2BEFF,
    };
    /// Имя в строке списка — Liberation Sans Narrow 700 · 14/18. Sample: «zaton_autosave.scop».
    pub const TYPE_LIST_NAME: TextSpec = TextSpec {
        face: "Liberation Sans Narrow",
        weight: 700,
        size: 14.0,
        line_height: 18.0,
        tracking: 0.0,
        color: 0xD8D2BEFF,
    };
    /// Подпись — Liberation Sans Narrow 400 · 13/17 · 12/15. Sample: «wpn_ak74 · 26.09.25 13:50 · 2.4 МБ».
    pub const TYPE_CAPTION: TextSpec = TextSpec {
        face: "Liberation Sans Narrow",
        weight: 400,
        size: 13.0,
        line_height: 17.0,
        tracking: 0.0,
        color: 0x716F67FF,
    };
    /// Метка плитки — Liberation Sans Narrow 400 · 12/15 · трекинг 0,6. Sample: «ИГРОВОЕ ВРЕМЯ».
    pub const TYPE_TILE_LABEL: TextSpec = TextSpec {
        face: "Liberation Sans Narrow",
        weight: 400,
        size: 12.0,
        line_height: 15.0,
        tracking: 0.6,
        color: 0x716F67FF,
    };
    /// Плашка — Oswald 500 · 11/14 · трекинг 0,6 · заглавные. Sample: «УСТАНОВЛЕНО».
    pub const TYPE_BADGE: TextSpec = TextSpec {
        face: "Oswald",
        weight: 500,
        size: 11.0,
        line_height: 14.0,
        tracking: 0.6,
        color: 0x7BCB62FF,
    };

    // ---- metrics: (standard, compact) in logical pixels ----
    /// Side menu width (`248` standard, `64` compact: icons only; the collapse button is 48×48).
    pub const SIDEBAR_WIDTH: (f32, f32) = (248.0, 64.0);
    /// Logo block in the side menu (`96` / `72`); the sign is 44 / 36.
    pub const SIDEBAR_LOGO_BLOCK: (f32, f32) = (96.0, 72.0);
    /// Menu item height (`48` / `48×48`).
    pub const MENU_ITEM_HEIGHT: (f32, f32) = (48.0, 48.0);
    /// Menu item padding (`14`; the spec gives no compact value, so the standard one is kept).
    pub const MENU_ITEM_PADDING: (f32, f32) = (14.0, 14.0);
    /// Menu item icon size (`22`; no compact value given, standard kept).
    pub const MENU_ITEM_ICON: (f32, f32) = (22.0, 22.0);
    /// Gap between menu items (`4`; no compact value given, standard kept).
    pub const MENU_ITEM_GAP: (f32, f32) = (4.0, 4.0);
    /// Top bar height (`44` / `44`).
    pub const TOP_BAR_HEIGHT: (f32, f32) = (44.0, 44.0);
    /// Header height with the title block (`108` / `80`).
    pub const HEADER_HEIGHT: (f32, f32) = (108.0, 80.0);
    /// Content padding, left/right/bottom (`24` / `16`).
    pub const CONTENT_PADDING: (f32, f32) = (24.0, 16.0);
    /// Gap between panels: `16` in columns, `12` in a stack; compact `12`.
    pub const PANEL_GAP: (f32, f32) = (16.0, 12.0);
    /// Save library column width (`300` / `248`; `340` at ≥2200 logical).
    pub const LIBRARY_WIDTH: (f32, f32) = (300.0, 248.0);
    /// Save library row height (`70` / `70`).
    pub const LIBRARY_ROW_HEIGHT: (f32, f32) = (70.0, 70.0);
    /// Library snapshot width (`96` / `72`).
    pub const LIBRARY_PREVIEW_WIDTH: (f32, f32) = (96.0, 72.0);
    /// Library snapshot height (`54` / `41`).
    pub const LIBRARY_PREVIEW_HEIGHT: (f32, f32) = (54.0, 41.0);
    /// Open save header height (`87` / `87`); its snapshot is 112×63.
    pub const SAVE_HEADER_HEIGHT: (f32, f32) = (87.0, 87.0);
    /// Tab height (`36` / `36`).
    pub const TAB_HEIGHT: (f32, f32) = (36.0, 36.0);
    /// Tab horizontal padding (`14` / `9`).
    pub const TAB_PADDING: (f32, f32) = (14.0, 9.0);
    /// Gap between tabs (`6`; no compact value given, standard kept).
    pub const TAB_GAP: (f32, f32) = (6.0, 6.0);
    /// Panel padding (`16` / `16`).
    pub const PANEL_PADDING: (f32, f32) = (16.0, 16.0);
    /// Value tile minimum width (`176` / `176`).
    pub const TILE_MIN_WIDTH: (f32, f32) = (176.0, 176.0);
    /// Value tile minimum height (`64` / `64`).
    pub const TILE_MIN_HEIGHT: (f32, f32) = (64.0, 64.0);
    /// Value tile vertical padding (`10` / `10`).
    pub const TILE_PADDING_VERTICAL: (f32, f32) = (10.0, 10.0);
    /// Value tile horizontal padding (`12` / `12`).
    pub const TILE_PADDING_HORIZONTAL: (f32, f32) = (12.0, 12.0);
    /// Gap between value tiles (`8`; auto-fill grid).
    pub const TILE_GAP: (f32, f32) = (8.0, 8.0);
    /// Table row height (`44` / `44`).
    pub const TABLE_ROW_HEIGHT: (f32, f32) = (44.0, 44.0);
    /// Table header height (`36` / `36`).
    pub const TABLE_HEADER_HEIGHT: (f32, f32) = (36.0, 36.0);
    /// Item inspector width (`360` / `296`).
    pub const INSPECTOR_WIDTH: (f32, f32) = (360.0, 296.0);
    /// Gap between inspector sections, which are split by a `#242922` rule (`12` / `12`).
    pub const INSPECTOR_SECTION_GAP: (f32, f32) = (12.0, 12.0);
    /// Game install list width (`440` / `340`).
    pub const GAMES_LIST_WIDTH: (f32, f32) = (440.0, 340.0);
    /// Game cover width in the list (`128` / `96`).
    pub const GAMES_COVER_WIDTH: (f32, f32) = (128.0, 96.0);
    /// Game cover height in the list (`72` / `54`).
    pub const GAMES_COVER_HEIGHT: (f32, f32) = (72.0, 54.0);
    /// Fix detail width (`440` / `340`).
    pub const FIX_DETAIL_WIDTH: (f32, f32) = (440.0, 340.0);
    /// Button, input and tab height (`36` / `36`).
    pub const CONTROL_HEIGHT: (f32, f32) = (36.0, 36.0);
    /// Small button height (`28`).
    pub const CONTROL_HEIGHT_SMALL: (f32, f32) = (28.0, 28.0);
    /// Status bar height (`30` / `30`).
    pub const STATUS_BAR_HEIGHT: (f32, f32) = (30.0, 30.0);
    /// Corner radius of badges and chips (`2`).
    pub const RADIUS_BADGE: f32 = 2.0;
    /// Corner radius of buttons (`3`).
    pub const RADIUS_BUTTON: f32 = 3.0;
    /// Corner radius of panels (`4`).
    pub const RADIUS_PANEL: f32 = 4.0;
    /// Spacing scale: 4 · 6 · 8 · 10 · 12 · 14 · 16 · 20 · 24.
    pub const SPACING_SCALE: [f32; 9] = [4.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0];

    // ---- layout thresholds (logical width, inclusive) ----
    /// Compact layout starts at this width.
    pub const LAYOUT_COMPACT_MIN: u32 = 940;
    /// Compact layout ends at this width (`1599`).
    pub const LAYOUT_COMPACT_MAX: u32 = 1599;
    /// Standard layout starts at this width.
    pub const LAYOUT_STANDARD_MIN: u32 = 1600;
    /// Standard layout ends at this width (`2199`).
    pub const LAYOUT_STANDARD_MAX: u32 = 2199;
    /// Wide layout starts at this width.
    pub const LAYOUT_WIDE_MIN: u32 = 2200;

    #[cfg(test)]
    mod state_tests {
        use super::*;

        #[test]
        fn focus_ring_only_on_button_focus_state() {
            for states in [
                &BUTTON_PRIMARY,
                &BUTTON_SECONDARY,
                &BUTTON_ACCENT_OUTLINE,
                &BUTTON_DANGER,
            ] {
                assert_eq!(states[3].ring, ACCENT);
                assert_eq!(states.iter().filter(|look| look.ring != 0).count(), 1);
            }
            assert!(TAB_STATES.iter().all(|look| look.ring == 0));
        }

        #[test]
        fn input_states_carry_icon_colour() {
            assert_eq!(INPUT_STATES[3].icon, 0xD85A45FF);
            assert_eq!(INPUT_STATES[0].icon, TEXT_MUTED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Theme;

    #[test]
    fn d2_colour_tokens_match_the_style_sheet() {
        use super::d2::*;
        // Name, value and constant, in the order of the colour groups of STYLE-DATA.js.txt.
        let expected: [(&str, u32); 34] = [
            ("bg_base", 0x0C0D0AFF),
            ("bg_sidebar", 0x0A0B09FF),
            ("scrim_sidebar", 0x0A0B09B3),
            ("scrim_topbar", 0x0C0D0A99),
            ("scrim_header", 0x0C0D0A33),
            ("scrim_content", 0x0C0D0AB3),
            ("panel", 0x101311E6),
            ("panel_raised", 0x151814F2),
            ("control", 0x151814CC),
            ("hover", 0x23261FFF),
            ("row_hover", 0x23261FCC),
            ("input", 0x1A1D17FF),
            ("border_subtle", 0x242922FF),
            ("border", 0x33382FFF),
            ("border_metal", 0x3D4837FF),
            ("border_strong", 0x716F67FF),
            ("text_primary", 0xD8D2BEFF),
            ("text_secondary", 0xA29D90FF),
            ("text_muted", 0x716F67FF),
            ("text_disabled", 0x4D4C46FF),
            ("text_value", 0xD8BA8CFF),
            ("text_on_accent", 0x0C0D0AFF),
            ("accent", 0xD6A62DFF),
            ("accent_hover", 0xE5B53CFF),
            ("accent_pressed", 0xB88E22FF),
            ("accent_tint", 0xD6A62D1A),
            ("accent_tint_strong", 0xD6A62D26),
            ("accent_border", 0xD6A62D66),
            ("success", 0x7BCB62FF),
            ("success_fill", 0x7BCB621F),
            ("danger", 0xD85A45FF),
            ("danger_fill", 0xD85A451F),
            ("info", 0x78AFE0FF),
            ("info_fill", 0x78AFE01F),
        ];
        let actual: [u32; 34] = [
            BG_BASE,
            BG_SIDEBAR,
            SCRIM_SIDEBAR,
            SCRIM_TOPBAR,
            SCRIM_HEADER,
            SCRIM_CONTENT,
            PANEL,
            PANEL_RAISED,
            CONTROL,
            HOVER,
            ROW_HOVER,
            INPUT,
            BORDER_SUBTLE,
            BORDER,
            BORDER_METAL,
            BORDER_STRONG,
            TEXT_PRIMARY,
            TEXT_SECONDARY,
            TEXT_MUTED,
            TEXT_DISABLED,
            TEXT_VALUE,
            TEXT_ON_ACCENT,
            ACCENT,
            ACCENT_HOVER,
            ACCENT_PRESSED,
            ACCENT_TINT,
            ACCENT_TINT_STRONG,
            ACCENT_BORDER,
            SUCCESS,
            SUCCESS_FILL,
            DANGER,
            DANGER_FILL,
            INFO,
            INFO_FILL,
        ];
        for ((name, want), got) in expected.iter().zip(actual.iter()) {
            assert_eq!(want, got, "{name}");
        }
    }

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
