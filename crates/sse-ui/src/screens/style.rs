//! Shared look of the screens: the C# dark palette and builders for the common blocks.
//!
//! Screens use these helpers instead of their own colours so the theme can change in one place (X36 tokens will
//! replace the constants here).

use crate::glyphs::{Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::raster::Color;
use crate::theme;
pub use crate::theme::TEXT_DISABLED as TEXT_MUTED;
pub use crate::theme::{ACCENT, BG_PANEL, BORDER_SUBTLE, TEXT_SECONDARY};
use crate::widget::{Content, Look, TextAlign, Tree, WidgetId};
use sse_core::Result;

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
        let colors = theme::current().colors;
        rgb(match self {
            Self::Title | Self::Heading | Self::Body => colors.text[0],
            Self::Value => colors.text[3],
            Self::Note => colors.text[2],
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
        padding: Edges::all(theme::CARD_PADDING),
        gap: Size::new(0.0, theme::CONTROL_GAP),
        align_items: Align::Stretch,
        ..Style::default()
    };
    let look = Look {
        fill: Some(rgb(theme::current().colors.background[2])),
        border: Some((rgb(theme::current().colors.borders[0]), 1.0)),
        radius: theme::CARD_RADIUS,
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
        gap: Size::new(theme::CONTROL_GAP, 0.0),
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

/// One-line editable input field.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn input(tree: &mut Tree, parent: WidgetId, value: &str) -> Result<WidgetId> {
    let colors = theme::current().colors;
    tree.add(
        Some(parent),
        NodeKind::Leaf,
        Style {
            min: Size::new(120.0, theme::BUTTON_HEIGHT),
            padding: Edges {
                left: 10.0,
                top: 0.0,
                right: 10.0,
                bottom: 0.0,
            },
            grow: 1.0,
            ..Style::default()
        },
        Content::Input {
            text: value.to_owned(),
            style: TextStyle::new(Face::Body, 14.0),
        },
        Look {
            fill: Some(rgb(colors.background[3])),
            border: Some((rgb(colors.borders[1]), 1.0)),
            radius: theme::BUTTON_RADIUS,
            text: rgb(colors.text[0]),
            ..Look::default()
        },
    )
}

/// A button.
///
/// # Errors
/// Returns an error from the widget tree.
pub fn button(tree: &mut Tree, parent: WidgetId, text: &str, role: Button) -> Result<WidgetId> {
    let padding = Edges {
        left: 16.0,
        top: 0.0,
        right: 16.0,
        bottom: 0.0,
    };
    let label = text_upper(text);
    let label_style = TextStyle::new(Face::Heading, 14.0);
    let min_width = (tree.measure_text(&label, label_style) + padding.left + padding.right)
        .ceil()
        .max(96.0);
    let style = Style {
        min: Size::new(min_width, theme::BUTTON_HEIGHT),
        padding,
        shrink: 0.0,
        ..Style::default()
    };
    let colors = theme::current().colors;
    let (fill, hover, foreground, border) = match role {
        Button::Primary => (Some(colors.accent[0]), colors.accent[2], colors.accent[3], None),
        Button::Secondary => (None, colors.background[3], colors.text[0], Some(colors.borders[1])),
        Button::Danger => (None, colors.background[3], colors.state[2], Some(colors.state[2])),
    };
    let look = Look {
        fill: fill.map(rgb),
        hover_fill: Some(rgb(hover)),
        border: border.map(|color| (rgb(color), 1.0)),
        radius: theme::BUTTON_RADIUS,
        text: rgb(foreground),
        align: TextAlign::Center,
        ..Look::default()
    };
    let content = Content::Button {
        text: label,
        style: label_style,
    };
    tree.add(Some(parent), NodeKind::Leaf, style, content, look)
}

/// Sidebar item look.
#[must_use]
pub fn nav(selected: bool) -> Look {
    let colors = theme::current().colors;
    Look {
        fill: selected.then(|| rgb(colors.background[2])),
        hover_fill: Some(rgb(colors.background[3])),
        pressed_fill: Some(rgb(colors.background[2])),
        accent_bar: selected.then(|| (rgb(colors.accent[0]), 3.0)),
        text: rgb(if selected { colors.text[0] } else { colors.text[1] }),
        hover_text: Some(rgb(colors.text[0])),
        ..Look::default()
    }
}

fn text_upper(text: &str) -> String {
    text.chars().flat_map(char::to_uppercase).collect()
}
