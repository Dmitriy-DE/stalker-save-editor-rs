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

/// Builders of the redesigned controls. Numbers come from [`theme::d2`]; icons are `Icon::D2*`.
pub mod d2 {
    use super::text_upper;
    use crate::glyphs::{Face, TextStyle};
    use crate::layout::{Edges, NodeKind, Size, Style};
    use crate::path::Icon;
    use crate::raster::Color;
    use crate::theme::d2::{self as tokens, StateLook, TextSpec};
    use crate::widget::{Content, DisabledLook, Look, TextAlign, Tree, WidgetId};
    use sse_core::Result;

    /// Visual role of a button.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ButtonKind {
        /// Filled accent action.
        Primary,
        /// Neutral action with a metal border.
        Secondary,
        /// Accent outline action.
        Outline,
        /// Destructive outline action.
        Danger,
    }

    /// Height of a button.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ButtonSize {
        /// 36 pixels.
        Normal,
        /// 28 pixels.
        Small,
    }

    /// Role of a status badge.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BadgeKind {
        /// Installed, found, verified.
        Installed,
        /// Update available, draft, experimental.
        Update,
        /// File changed after install: error, conflict, damaged.
        Changed,
        /// Steam Cloud.
        Cloud,
        /// Not installed.
        NotInstalled,
        /// Save slot.
        Slot,
        /// Draft changes.
        Draft,
    }

    fn argb(value: u32) -> Color {
        let [r, g, b, a] = value.to_be_bytes();
        Color::rgba(r, g, b, a)
    }

    fn text_style(spec: TextSpec) -> TextStyle {
        let face = match (spec.face, spec.weight) {
            ("Oswald", weight) if weight >= 600 => Face::Heading,
            ("Oswald", _) => Face::HeadingMedium,
            (_, weight) if weight >= 700 => Face::BodyBold,
            _ => Face::Body,
        };
        TextStyle::new(face, spec.size).with_tracking(spec.tracking)
    }

    fn control_look(states: &[StateLook; 5], selected: bool, radius: f32) -> Look {
        let [normal, hover, pressed, selected_state, disabled] = *states;
        let base = if selected { selected_state } else { normal };
        Look {
            fill: Some(argb(base.fill)),
            hover_fill: Some(argb(hover.fill)),
            pressed_fill: Some(argb(pressed.fill)),
            border: Some((argb(base.border), 1.0)),
            hover_border: Some((argb(hover.border), 1.0)),
            pressed_border: Some((argb(pressed.border), 1.0)),
            text: argb(base.text),
            hover_text: Some(argb(hover.text)),
            pressed_text: Some(argb(pressed.text)),
            radius,
            align: TextAlign::Center,
            focus_ring: (selected_state.ring != 0 && !selected).then(|| (argb(selected_state.ring), 2.0, 2.0)),
            disabled: Some(DisabledLook {
                fill: argb(disabled.fill),
                border: argb(disabled.border),
                text: argb(disabled.text),
            }),
            ..Look::default()
        }
    }

    fn padded(left: f32, right: f32) -> Edges {
        Edges {
            left,
            top: 0.0,
            right,
            bottom: 0.0,
        }
    }

    fn min_width(tree: &Tree, label: &str, style: TextStyle, padding: Edges) -> f32 {
        (tree.measure_text(label, style) + padding.left + padding.right).ceil()
    }

    /// A button with a text label.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn button(
        tree: &mut Tree,
        parent: WidgetId,
        text: &str,
        kind: ButtonKind,
        size: ButtonSize,
    ) -> Result<WidgetId> {
        let states = match kind {
            ButtonKind::Primary => &tokens::BUTTON_PRIMARY,
            ButtonKind::Secondary => &tokens::BUTTON_SECONDARY,
            ButtonKind::Outline => &tokens::BUTTON_ACCENT_OUTLINE,
            ButtonKind::Danger => &tokens::BUTTON_DANGER,
        };
        let height = match size {
            ButtonSize::Normal => tokens::CONTROL_HEIGHT.0,
            ButtonSize::Small => tokens::CONTROL_HEIGHT_SMALL.0,
        };
        let label = text_upper(text);
        let label_style = text_style(tokens::TYPE_TAB_BUTTON);
        let padding = padded(16.0, 16.0);
        let style = Style {
            min: Size::new(min_width(tree, &label, label_style, padding).max(96.0), height),
            padding,
            shrink: 0.0,
            ..Style::default()
        };
        let look = control_look(states, false, tokens::RADIUS_BUTTON);
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            Content::Button {
                text: label,
                style: label_style,
            },
            look,
        )
    }

    fn icon_or_plain(text: String, style: TextStyle, icon: Option<Icon>) -> Content {
        match icon {
            Some(icon) => Content::IconButton { icon, text, style },
            None => Content::Button { text, style },
        }
    }

    /// A tab of the page tab strip; 36 pixels high.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn tab(tree: &mut Tree, parent: WidgetId, text: &str, icon: Option<Icon>, selected: bool) -> Result<WidgetId> {
        let label = text_upper(text);
        let label_style = text_style(tokens::TYPE_TAB_BUTTON);
        let padding = padded(tokens::TAB_PADDING.0, tokens::TAB_PADDING.0);
        let style = Style {
            min: Size::new(min_width(tree, &label, label_style, padding), tokens::TAB_HEIGHT.0),
            padding,
            shrink: 0.0,
            ..Style::default()
        };
        let look = control_look(&tokens::TAB_STATES, selected, tokens::RADIUS_BUTTON);
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            icon_or_plain(label, label_style, icon),
            look,
        )
    }

    /// A filter chip; 28 pixels high.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn chip(tree: &mut Tree, parent: WidgetId, text: &str, icon: Option<Icon>, selected: bool) -> Result<WidgetId> {
        let label = text_upper(text);
        let label_style = TextStyle::new(Face::HeadingMedium, 12.0).with_tracking(0.6);
        let padding = padded(10.0, 10.0);
        let style = Style {
            min: Size::new(
                min_width(tree, &label, label_style, padding),
                tokens::CONTROL_HEIGHT_SMALL.0,
            ),
            padding,
            shrink: 0.0,
            ..Style::default()
        };
        let look = control_look(&tokens::FILTER_CHIP_STATES, selected, tokens::RADIUS_BADGE);
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            icon_or_plain(label, label_style, icon),
            look,
        )
    }

    /// A status badge.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn badge(tree: &mut Tree, parent: WidgetId, text: &str, kind: BadgeKind) -> Result<WidgetId> {
        let colours = match kind {
            BadgeKind::Installed => tokens::BADGE_INSTALLED,
            BadgeKind::Update => tokens::BADGE_UPDATE,
            BadgeKind::Changed => tokens::BADGE_CHANGED,
            BadgeKind::Cloud => tokens::BADGE_CLOUD,
            BadgeKind::NotInstalled => tokens::BADGE_NOT_INSTALLED,
            BadgeKind::Slot => tokens::BADGE_SLOT,
            BadgeKind::Draft => tokens::BADGE_DRAFT,
        };
        let label = text_upper(text);
        let label_style = text_style(tokens::TYPE_BADGE);
        let padding = padded(8.0, 8.0);
        let style = Style {
            min: Size::new(min_width(tree, &label, label_style, padding), 18.0),
            padding,
            ..Style::default()
        };
        let look = Look {
            fill: Some(argb(colours.fill)),
            border: Some((argb(colours.border), 1.0)),
            text: argb(colours.text),
            radius: tokens::RADIUS_BADGE,
            align: TextAlign::Center,
            ..Look::default()
        };
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            Content::Label {
                text: label,
                style: label_style,
            },
            look,
        )
    }

    /// A panel that holds a block of controls.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn panel(tree: &mut Tree, parent: WidgetId) -> Result<WidgetId> {
        let style = Style {
            padding: padded(tokens::PANEL_PADDING.0, tokens::PANEL_PADDING.0),
            ..Style::default()
        };
        let look = Look {
            fill: Some(argb(tokens::PANEL)),
            border: Some((argb(tokens::BORDER), 1.0)),
            radius: tokens::RADIUS_PANEL,
            ..Look::default()
        };
        tree.add(Some(parent), NodeKind::Column, style, Content::Panel, look)
    }

    /// Panel heading: uppercase, accent colour.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn panel_title(tree: &mut Tree, parent: WidgetId, text: &str) -> Result<WidgetId> {
        let look = Look {
            text: argb(tokens::ACCENT),
            ..Look::default()
        };
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: text_upper(text),
                style: text_style(tokens::TYPE_PANEL_TITLE),
            },
            look,
        )
    }

    /// A tile with a small key above a large value.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn tile(tree: &mut Tree, parent: WidgetId, key: &str, value: &str) -> Result<WidgetId> {
        let style = Style {
            min: Size::new(tokens::TILE_MIN_WIDTH.0, tokens::TILE_MIN_HEIGHT.0),
            padding: padded(tokens::TILE_PADDING_HORIZONTAL.0, tokens::TILE_PADDING_HORIZONTAL.0),
            ..Style::default()
        };
        let look = Look {
            fill: Some(argb(tokens::PANEL_RAISED)),
            border: Some((argb(tokens::BORDER_SUBTLE), 1.0)),
            radius: tokens::RADIUS_PANEL,
            ..Look::default()
        };
        let tile = tree.add(Some(parent), NodeKind::Column, style, Content::Panel, look)?;
        tree.add(
            Some(tile),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: text_upper(key),
                style: text_style(tokens::TYPE_TILE_LABEL),
            },
            Look {
                text: argb(tokens::TEXT_MUTED),
                ..Look::default()
            },
        )?;
        tree.add(
            Some(tile),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: value.to_owned(),
                style: text_style(tokens::TYPE_VALUE_LARGE),
            },
            Look {
                text: argb(tokens::TEXT_VALUE),
                ..Look::default()
            },
        )?;
        Ok(tile)
    }

    /// A list row; 44 pixels high.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn list_row(tree: &mut Tree, parent: WidgetId, text: &str, selected: bool) -> Result<WidgetId> {
        let style = Style {
            min: Size::new(0.0, 44.0),
            padding: padded(12.0, 12.0),
            ..Style::default()
        };
        let mut look = control_look(&tokens::LIST_ROW_STATES, selected, tokens::RADIUS_BADGE);
        look.align = TextAlign::Start;
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            Content::Button {
                text: text.to_owned(),
                style: text_style(tokens::TYPE_LIST_NAME),
            },
            look,
        )
    }

    /// A text field with a placeholder; 36 pixels high.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn input(tree: &mut Tree, parent: WidgetId, placeholder: &str) -> Result<WidgetId> {
        let [normal, hover, _, _, disabled] = tokens::INPUT_STATES;
        let style = Style {
            min: Size::new(0.0, tokens::CONTROL_HEIGHT.0),
            padding: padded(12.0, 12.0),
            ..Style::default()
        };
        let look = Look {
            fill: Some(argb(normal.fill)),
            border: Some((argb(normal.border), 1.0)),
            hover_border: Some((argb(hover.border), 1.0)),
            text: argb(normal.text),
            radius: tokens::RADIUS_BUTTON,
            disabled: Some(DisabledLook {
                fill: argb(disabled.fill),
                border: argb(disabled.border),
                text: argb(disabled.text),
            }),
            ..Look::default()
        };
        tree.add(
            Some(parent),
            NodeKind::Leaf,
            style,
            Content::Input {
                text: placeholder.to_owned(),
                style: text_style(tokens::TYPE_TEXT),
            },
            look,
        )
    }

    /// A key and a value side by side.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn key_value_row(tree: &mut Tree, parent: WidgetId, key: &str, value: &str) -> Result<WidgetId> {
        let row = tree.add(
            Some(parent),
            NodeKind::Row,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        tree.add(
            Some(row),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: text_upper(key),
                style: text_style(tokens::TYPE_CAPTION),
            },
            Look {
                text: argb(tokens::TEXT_MUTED),
                ..Look::default()
            },
        )?;
        tree.add(
            Some(row),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: value.to_owned(),
                style: text_style(tokens::TYPE_TEXT),
            },
            Look {
                text: argb(tokens::TEXT_PRIMARY),
                ..Look::default()
            },
        )?;
        Ok(row)
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::glyphs::Fonts;
        use crate::widget::Tree;

        #[test]
        fn showcase_paints_every_control_in_every_state() -> Result<()> {
            let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
            let root = tree.add(
                None,
                NodeKind::Column,
                Style::default(),
                Content::Panel,
                Look::default(),
            )?;
            let panel = panel(&mut tree, root)?;
            panel_title(&mut tree, panel, "Показ элементов")?;
            let mut controls = Vec::new();
            for kind in [
                ButtonKind::Primary,
                ButtonKind::Secondary,
                ButtonKind::Outline,
                ButtonKind::Danger,
            ] {
                for size in [ButtonSize::Normal, ButtonSize::Small] {
                    controls.push(button(&mut tree, panel, "Сохранить", kind, size)?);
                }
            }
            for selected in [false, true] {
                controls.push(tab(&mut tree, panel, "Инвентарь", Some(Icon::D2Inventory), selected)?);
                controls.push(chip(&mut tree, panel, "Артефакты", Some(Icon::D2Radiation), selected)?);
                controls.push(list_row(&mut tree, panel, "pripyat_hospital.scop", selected)?);
            }
            for kind in [
                BadgeKind::Installed,
                BadgeKind::Update,
                BadgeKind::Changed,
                BadgeKind::Cloud,
                BadgeKind::NotInstalled,
                BadgeKind::Slot,
                BadgeKind::Draft,
            ] {
                badge(&mut tree, panel, "Установлено", kind)?;
            }
            tile(&mut tree, panel, "Игровое время", "48 650 RU")?;
            key_value_row(&mut tree, panel, "Версия", "2.0.0")?;
            controls.push(input(&mut tree, panel, "Поиск предметов…")?);
            for (index, id) in controls.iter().enumerate() {
                tree.set_enabled(*id, index % 2 == 1)?;
            }
            tree.resize(960, 720);
            tree.update_layout()?;
            tree.focus_next(false);
            if let Some(first) = controls.first() {
                let rect = tree.rect(*first)?;
                tree.pointer_moved(rect.x.saturating_add(4), rect.y.saturating_add(4));
            }
            let mut frame = vec![0_u32; 960 * 720];
            tree.paint(&mut frame, 960)?;
            Ok(())
        }
    }
}
