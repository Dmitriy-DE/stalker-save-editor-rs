//! Shared look of the screens: the C# dark palette and builders for the common blocks.
//!
//! Screens use these helpers instead of their own colours so the theme can change in one place (X36 tokens will
//! replace the constants here).

use crate::glyphs::{Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::raster::Color;
use crate::widget::{Content, Look, TextAlign, Tree, WidgetId};
use sse_core::Result;

/// Window background.
pub const BG_BASE: u32 = 0x0C0D0A;
/// Sidebar and status line.
pub const BG_PANEL: u32 = 0x101311;
/// Cards.
pub const BG_ELEVATED: u32 = 0x151814;
/// Hover.
pub const BG_HOVER: u32 = 0x23261F;
/// Input fields.
pub const BG_INPUT: u32 = 0x1A1D17;
/// Thin separators.
pub const BORDER_SUBTLE: u32 = 0x242922;
/// Borders.
pub const BORDER: u32 = 0x33382F;
/// Accent (amber).
pub const ACCENT: u32 = 0xD6A62D;
/// Accent hover.
pub const ACCENT_HOVER: u32 = 0xE5B53C;
/// Text on accent.
pub const ACCENT_FOREGROUND: u32 = 0x0C0D0A;
/// Success.
pub const SUCCESS: u32 = 0x7BCB62;
/// Danger.
pub const DANGER: u32 = 0xD85A45;
/// Main text.
pub const TEXT_PRIMARY: u32 = 0xD8D2BE;
/// Secondary text.
pub const TEXT_SECONDARY: u32 = 0xA29D90;
/// Muted text.
pub const TEXT_MUTED: u32 = 0x716F67;
/// Khaki values.
pub const TEXT_KHAKI: u32 = 0xD8BA8C;

/// Opaque colour from `0xRRGGBB`.
#[must_use]
pub fn rgb(value: u32) -> Color {
    let [_, r, g, b] = value.to_be_bytes();
    Color::rgba(r, g, b, 255)
}

/// Text roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Text {
    /// Screen title.
    Title,
    /// Card heading.
    Heading,
    /// Body text.
    Body,
    /// Value (khaki).
    Value,
    /// Small muted note.
    Note,
}

impl Text {
    /// Font and size.
    #[must_use]
    pub const fn style(self) -> TextStyle {
        match self {
            Self::Title => TextStyle::new(Face::Heading, 30.0),
            Self::Heading => TextStyle::new(Face::Heading, 18.0),
            Self::Body | Self::Value => TextStyle::new(Face::Body, 16.0),
            Self::Note => TextStyle::new(Face::Body, 13.0),
        }
    }

    /// Colour.
    #[must_use]
    pub fn color(self) -> Color {
        rgb(match self {
            Self::Title | Self::Heading | Self::Body => TEXT_PRIMARY,
            Self::Value => TEXT_KHAKI,
            Self::Note => TEXT_MUTED,
        })
    }
}

/// Button roles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    /// Amber main action.
    Primary,
    /// Outlined secondary action.
    Secondary,
    /// Red destructive action.
    Danger,
}

/// A card: elevated panel with border, a column of children with gaps.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn card(tree: &mut Tree, parent: WidgetId) -> Result<WidgetId> {
    let style = Style {
        padding: Edges::all(20.0),
        gap: Size::new(0.0, 10.0),
        align_items: Align::Stretch,
        ..Style::default()
    };
    let look = Look {
        fill: Some(rgb(BG_ELEVATED)),
        border: Some((rgb(BORDER_SUBTLE), 1.0)),
        radius: 4.0,
        ..Look::default()
    };
    tree.add(Some(parent), NodeKind::Column, style, Content::Panel, look)
}

/// A horizontal row with gaps.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn row(tree: &mut Tree, parent: WidgetId) -> Result<WidgetId> {
    let style = Style {
        gap: Size::new(10.0, 0.0),
        align_items: Align::Center,
        ..Style::default()
    };
    tree.add(Some(parent), NodeKind::Row, style, Content::Panel, Look::default())
}

/// One line of text.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn label(tree: &mut Tree, parent: WidgetId, text: &str, role: Text) -> Result<WidgetId> {
    let content = Content::Label {
        text: text.to_owned(),
        style: role.style(),
    };
    let look = Look {
        text: role.color(),
        ..Look::default()
    };
    tree.add(Some(parent), NodeKind::Leaf, Style::default(), content, look)
}

/// A button.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn button(tree: &mut Tree, parent: WidgetId, text: &str, role: Button) -> Result<WidgetId> {
    let style = Style {
        min: Size::new(96.0, 34.0),
        padding: Edges {
            left: 16.0,
            top: 0.0,
            right: 16.0,
            bottom: 0.0,
        },
        ..Style::default()
    };
    let (fill, hover, foreground, border) = match role {
        Button::Primary => (Some(ACCENT), ACCENT_HOVER, ACCENT_FOREGROUND, None),
        Button::Secondary => (None, BG_HOVER, TEXT_PRIMARY, Some(BORDER)),
        Button::Danger => (None, BG_HOVER, DANGER, Some(DANGER)),
    };
    let look = Look {
        fill: fill.map(rgb),
        hover_fill: Some(rgb(hover)),
        border: border.map(|color| (rgb(color), 1.0)),
        radius: 3.0,
        text: rgb(foreground),
        align: TextAlign::Center,
        ..Look::default()
    };
    let content = Content::Button {
        text: text_upper(text),
        style: TextStyle::new(Face::Heading, 14.0),
    };
    tree.add(Some(parent), NodeKind::Leaf, style, content, look)
}

/// Sidebar item look.
#[must_use]
pub fn nav(selected: bool) -> Look {
    Look {
        fill: selected.then(|| rgb(BG_ELEVATED)),
        hover_fill: Some(rgb(BG_HOVER)),
        pressed_fill: Some(rgb(BG_ELEVATED)),
        accent_bar: selected.then(|| (rgb(ACCENT), 3.0)),
        text: rgb(if selected { TEXT_PRIMARY } else { TEXT_SECONDARY }),
        hover_text: Some(rgb(TEXT_PRIMARY)),
        ..Look::default()
    }
}

fn text_upper(text: &str) -> String {
    text.chars().flat_map(char::to_uppercase).collect()
}
