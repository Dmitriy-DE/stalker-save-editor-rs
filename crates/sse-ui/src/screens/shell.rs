//! The editor frame: grouped sidebar, header, content host, status line; builds screens lazily and routes messages.

use super::style::{self, rgb, Text};
use super::{registry, AppMessage, Context, Group, Screen, ScreenId};
use crate::event_loop::{App, Flow, Message, Proxy, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::widget::{Content, Look, Tree, WidgetId};
use sse_core::Result;
use std::path::Path;

const KEY_ESCAPE: u32 = 0xff1b;
const KEY_UP: u32 = 0xff52;
const KEY_DOWN: u32 = 0xff54;

/// The editor frame and its screens.
pub struct Shell {
    screens: Vec<Box<dyn Screen>>,
    hosts: Vec<Option<WidgetId>>,
    nav: Vec<WidgetId>,
    content: WidgetId,
    title: WidgetId,
    subtitle: WidgetId,
    status: WidgetId,
    selected: usize,
    proxy: Option<Proxy<AppMessage>>,
    app: sse_app::state::AppState,
}

fn padded(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges {
        left,
        top,
        right,
        bottom,
    }
}

impl Shell {
    /// Builds the frame into an empty tree and shows the first screen.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn build(tree: &mut Tree, proxy: Option<Proxy<AppMessage>>) -> Result<Self> {
        let root_style = Style {
            align_items: Align::Stretch,
            ..Style::default()
        };
        let root = tree.add(None, NodeKind::Row, root_style, Content::Panel, Look::default())?;

        let sidebar_style = Style {
            preferred: Size::new(232.0, 0.0),
            min: Size::new(232.0, 0.0),
            shrink: 0.0,
            padding: padded(0.0, 18.0, 0.0, 12.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let sidebar_look = Look {
            fill: Some(rgb(style::BG_PANEL)),
            border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
            ..Look::default()
        };
        let sidebar = tree.add(
            Some(root),
            NodeKind::Column,
            sidebar_style,
            Content::Panel,
            sidebar_look,
        )?;
        let brand = Style {
            padding: padded(20.0, 0.0, 20.0, 0.0),
            ..Style::default()
        };
        tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            brand,
            Content::Label {
                text: "S.T.A.L.K.E.R.".to_owned(),
                style: TextStyle::new(Face::Heading, 22.0),
            },
            Look {
                text: rgb(style::ACCENT),
                ..Look::default()
            },
        )?;
        tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            Style {
                padding: padded(20.0, 0.0, 20.0, 6.0),
                ..Style::default()
            },
            Content::Label {
                text: "РЕДАКТОР СОХРАНЕНИЙ".to_owned(),
                style: TextStyle::new(Face::Heading, 12.0),
            },
            Look {
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let screens = registry();
        let mut nav = Vec::with_capacity(screens.len());
        let mut group: Option<Group> = None;
        for screen in &screens {
            let id = screen.id();
            if group != Some(id.group()) {
                group = Some(id.group());
                tree.add(
                    Some(sidebar),
                    NodeKind::Leaf,
                    Style {
                        padding: padded(20.0, 12.0, 20.0, 4.0),
                        ..Style::default()
                    },
                    Content::Label {
                        text: id.group().caption().to_owned(),
                        style: TextStyle::new(Face::Heading, 11.0),
                    },
                    Look {
                        text: rgb(style::TEXT_MUTED),
                        ..Look::default()
                    },
                )?;
            }
            let item = Style {
                min: Size::new(0.0, 30.0),
                padding: padded(22.0, 0.0, 12.0, 0.0),
                ..Style::default()
            };
            let content = Content::Button {
                text: id.title().to_owned(),
                style: TextStyle::new(Face::Heading, 14.0),
            };
            nav.push(tree.add(Some(sidebar), NodeKind::Leaf, item, content, style::nav(nav.is_empty()))?);
        }
        tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            Style {
                grow: 1.0,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let version = Content::Label {
            text: format!("{} · Rust", env!("CARGO_PKG_VERSION")),
            style: Text::Note.style(),
        };
        tree.add(
            Some(sidebar),
            NodeKind::Leaf,
            brand,
            version,
            Look {
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let main_style = Style {
            grow: 1.0,
            align_items: Align::Stretch,
            ..Style::default()
        };
        let main = tree.add(
            Some(root),
            NodeKind::Column,
            main_style,
            Content::Panel,
            Look::default(),
        )?;
        let header_style = Style {
            padding: padded(32.0, 24.0, 32.0, 16.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let header = tree.add(
            Some(main),
            NodeKind::Column,
            header_style,
            Content::Panel,
            Look::default(),
        )?;
        let title = style::label(tree, header, "", Text::Title)?;
        let subtitle = tree.add(
            Some(header),
            NodeKind::Leaf,
            Style::default(),
            Content::Label {
                text: String::new(),
                style: TextStyle::new(Face::Body, 16.0),
            },
            Look {
                text: rgb(style::TEXT_SECONDARY),
                ..Look::default()
            },
        )?;
        let content_style = Style {
            grow: 1.0,
            margin: padded(32.0, 0.0, 32.0, 20.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let content = tree.add(
            Some(main),
            NodeKind::Column,
            content_style,
            Content::Panel,
            Look::default(),
        )?;
        let status = tree.add(
            Some(main),
            NodeKind::Leaf,
            Style {
                min: Size::new(0.0, 28.0),
                padding: padded(32.0, 0.0, 32.0, 0.0),
                ..Style::default()
            },
            Content::Label {
                text: "Готово".to_owned(),
                style: Text::Note.style(),
            },
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                text: rgb(style::TEXT_MUTED),
                ..Look::default()
            },
        )?;

        let hosts = vec![None; screens.len()];
        let mut shell = Self {
            screens,
            hosts,
            nav,
            content,
            title,
            subtitle,
            status,
            selected: 0,
            proxy,
            app: sse_app::state::AppState::new(),
        };
        shell.show(tree, 0)?;
        Ok(shell)
    }

    /// Shared application state.
    #[must_use]
    pub fn app(&self) -> &sse_app::state::AppState {
        &self.app
    }

    /// Currently shown screen.
    #[must_use]
    pub fn current(&self) -> Option<ScreenId> {
        self.screens.get(self.selected).map(|screen| screen.id())
    }

    /// Switches to a screen by id.
    ///
    /// # Errors
    /// Returns an error from the widget tree or the screen.
    pub fn open(&mut self, tree: &mut Tree, id: ScreenId) -> Result<()> {
        match self.screens.iter().position(|screen| screen.id() == id) {
            Some(index) => self.select(tree, index),
            None => Ok(()),
        }
    }

    /// Opens a save through the selected screen's background-aware save hook.
    ///
    /// # Errors
    /// Returns an error if the screen cannot read or parse the save.
    pub fn open_save(&mut self, tree: &mut Tree, path: &Path) -> Result<bool> {
        self.open(tree, ScreenId::Overview)?;
        let Some(index) = self.screens.iter().position(|screen| screen.id() == ScreenId::Overview) else {
            return Ok(false);
        };
        let mut cx = Context {
            tree,
            proxy: self.proxy.as_ref(),
            status: None,
        };
        let opened = self
            .screens
            .get_mut(index)
            .map(|screen| screen.open_save(&mut cx, path))
            .transpose()?
            .unwrap_or(false);
        if let Some(status) = cx.status {
            cx.tree.set_text(self.status, &status)?;
        }
        Ok(opened)
    }

    fn select(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        if index == self.selected || index >= self.screens.len() {
            return Ok(());
        }
        if let Some(old) = self.nav.get(self.selected) {
            tree.set_look(*old, style::nav(false))?;
        }
        if let Some(host) = self.hosts.get(self.selected).copied().flatten() {
            tree.set_visible(host, false)?;
        }
        if let Some(new) = self.nav.get(index) {
            tree.set_look(*new, style::nav(true))?;
        }
        self.selected = index;
        self.show(tree, index)
    }

    fn show(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        let (Some(screen), Some(slot)) = (self.screens.get_mut(index), self.hosts.get_mut(index)) else {
            return Ok(());
        };
        tree.set_text(self.title, screen.id().title())?;
        tree.set_text(self.subtitle, screen.subtitle())?;
        let mut cx = Context {
            tree,
            proxy: self.proxy.as_ref(),
            status: None,
            app: &mut self.app,
        };
        match *slot {
            Some(host) => cx.tree.set_visible(host, true)?,
            None => {
                let host_style = Style {
                    grow: 1.0,
                    gap: Size::new(0.0, 16.0),
                    align_items: Align::Stretch,
                    ..Style::default()
                };
                let host = cx.tree.add(
                    Some(self.content),
                    NodeKind::Column,
                    host_style,
                    Content::Panel,
                    Look::default(),
                )?;
                *slot = Some(host);
                screen.build(&mut cx, host)?;
            }
        }
        screen.shown(&mut cx)?;
        let status = cx.status.take();
        if let Some(text) = status {
            tree.set_text(self.status, &text)?;
        }
        Ok(())
    }

    fn route(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<()> {
        let mut status = None;
        for (index, screen) in self.screens.iter_mut().enumerate() {
            if self.hosts.get(index).copied().flatten().is_none() {
                continue;
            }
            let wanted = match message {
                Message::User(AppMessage::Tick(_)) => true,
                Message::User(AppMessage::ToScreen(id, _)) => *id == screen.id(),
                Message::Window(_) => index == self.selected,
            };
            if wanted {
                let mut cx = Context {
                    tree,
                    proxy: self.proxy.as_ref(),
                    status: None,
                    app: &mut self.app,
                };
                screen.message(&mut cx, message, clicked)?;
                status = cx.status.or(status);
            }
        }
        if let Some(text) = status {
            tree.set_text(self.status, &text)?;
        }
        Ok(())
    }

    fn handle(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Result<Flow> {
        if let Some(index) = clicked.and_then(|id| self.nav.iter().position(|nav| *nav == id)) {
            self.select(tree, index)?;
            return Ok(Flow::Continue);
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            ctrl: false,
            ..
        }) = message
        {
            let count = self.nav.len();
            match *keysym {
                KEY_ESCAPE => return Ok(Flow::Exit),
                KEY_UP => self.select(tree, self.selected.checked_sub(1).unwrap_or(count.saturating_sub(1)))?,
                KEY_DOWN => {
                    let next = self.selected.saturating_add(1);
                    self.select(tree, if next >= count { 0 } else { next })?;
                }
                _ => {}
            }
        }
        self.route(tree, message, clicked)?;
        Ok(Flow::Continue)
    }
}

impl App<AppMessage> for Shell {
    fn message(&mut self, tree: &mut Tree, message: &Message<AppMessage>, clicked: Option<WidgetId>) -> Flow {
        match self.handle(tree, message, clicked) {
            Ok(flow) => flow,
            Err(error) => {
                let _ = tree.set_text(self.status, &format!("Ошибка: {error}"));
                Flow::Continue
            }
        }
    }
}
