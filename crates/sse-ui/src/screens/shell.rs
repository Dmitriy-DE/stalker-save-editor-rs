//! The editor frame: grouped sidebar, header, content host, status line; builds screens lazily and routes messages.

use super::style::{self, rgb, Text};
use super::{registry, AppMessage, Context, Group, Screen, ScreenId};
use crate::event_loop::{App, Flow, Message, Proxy, WindowEvent};
use crate::glyphs::{to_px, Face, TextStyle};
use crate::layout::{Align, Edges, NodeKind, Size, Style};
use crate::widget::{Content, Look, Tree, WidgetId};
use crate::widgets::scroll::ScrollView;
use sse_core::Result;
use std::path::Path;

const KEY_ESCAPE: u32 = 0xff1b;
const KEY_TAB: u32 = 0xff09;
const KEY_RETURN: u32 = 0xff0d;
const KEY_UP: u32 = 0xff52;
const KEY_DOWN: u32 = 0xff54;

/// The editor frame and its screens.
pub struct Shell {
    screens: Vec<Box<dyn Screen>>,
    hosts: Vec<Option<WidgetId>>,
    nav: Vec<WidgetId>,
    content: WidgetId,
    scroll: ScrollView,
    scroll_bar: WidgetId,
    title: WidgetId,
    subtitle: WidgetId,
    breadcrumb: WidgetId,
    edition: WidgetId,
    undo: WidgetId,
    redo: WidgetId,
    reset: WidgetId,
    open_button: WidgetId,
    refresh: WidgetId,
    save: WidgetId,
    reports_banner: WidgetId,
    reports_ok: WidgetId,
    reports_off: WidgetId,
    status: WidgetId,
    selected: usize,
    proxy: Option<Proxy<AppMessage>>,
    app: sse_app::state::AppState,
    wizard: super::wizard::Wizard,
}

fn padded(left: f32, top: f32, right: f32, bottom: f32) -> Edges {
    Edges {
        left,
        top,
        right,
        bottom,
    }
}

fn startup_language(settings: &sse_app::AppSettings) -> String {
    if let Ok(value) = std::env::var("STALKER_EDITOR_LANG") {
        if !value.trim().is_empty() {
            return value;
        }
    }
    if let Some(value) = settings.language.as_deref() {
        return value.to_owned();
    }
    let system = std::env::var("LC_ALL")
        .ok()
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("LC_MESSAGES").ok().filter(|value| !value.is_empty()))
        .or_else(|| std::env::var("LANG").ok().filter(|value| !value.is_empty()));
    let Some(system) = system else { return "en".to_owned() };
    let normalized = system.split('.').next().unwrap_or(&system).replace('_', "-");
    let base = normalized.split('-').next().unwrap_or(&normalized);
    if crate::strings::LANGUAGES
        .iter()
        .any(|code| code.eq_ignore_ascii_case(&normalized) || code.eq_ignore_ascii_case(base))
    {
        normalized
    } else {
        "en".to_owned()
    }
}

impl Shell {
    /// Builds the frame into an empty tree and shows the first screen.
    ///
    /// # Errors
    /// Returns an error from the widget tree.
    pub fn build(tree: &mut Tree, proxy: Option<Proxy<AppMessage>>) -> Result<Self> {
        let settings = sse_app::AppSettings::load(&sse_app::default_settings_path());
        let language = startup_language(&settings);
        crate::strings::set_language(Some(&language));
        let root_style = Style {
            align_items: Align::Stretch,
            ..Style::default()
        };
        let root = tree.add(None, NodeKind::Stack, root_style, Content::Panel, Look::default())?;
        let frame = tree.add(
            Some(root),
            NodeKind::Row,
            Style {
                grow: 1.0,
                align_self: Some(Align::Stretch),
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;

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
            Some(frame),
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
                text: crate::strings::t(id.title()).to_owned(),
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
            Some(frame),
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
        let top = style::row(tree, header)?;
        style::label(tree, top, "S.T.A.L.K.E.R. SAVE EDITOR", Text::Title)?;
        let edition = style::label(tree, top, "X-Ray / S2", Text::Value)?;
        let undo = style::button(tree, top, "Отменить", style::Button::Secondary)?;
        let redo = style::button(tree, top, "Вернуть", style::Button::Secondary)?;
        let reset = style::button(tree, top, "Сбросить", style::Button::Secondary)?;
        let open_button = style::button(tree, top, "Открыть…", style::Button::Secondary)?;
        let refresh = style::button(tree, top, "Обновить", style::Button::Secondary)?;
        let save = style::button(tree, top, "СОХРАНИТЬ", style::Button::Primary)?;
        let breadcrumb = style::label(tree, header, "", Text::Note)?;
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
        let reports_banner = tree.add(
            Some(main),
            NodeKind::Row,
            Style {
                min: Size::new(0.0, 44.0),
                padding: padded(16.0, 6.0, 16.0, 6.0),
                align_items: Align::Center,
                ..Style::default()
            },
            Content::Panel,
            Look {
                fill: Some(rgb(style::BG_PANEL)),
                border: Some((rgb(style::BORDER_SUBTLE), 1.0)),
                ..Look::default()
            },
        )?;
        style::label(
            tree,
            reports_banner,
            "Редактор раз в сутки и после сбоя отправляет разработчику журнал работы, чтобы находить ошибки. Пути, имена и Steam ID из него вырезаются, сейвы не отправляются.",
            Text::Note,
        )?;
        let reports_ok = style::button(tree, reports_banner, "Понятно", style::Button::Secondary)?;
        let reports_off = style::button(tree, reports_banner, "Не отправлять", style::Button::Secondary)?;
        tree.set_visible(reports_banner, !settings.reports_notice_shown)?;

        let viewport = tree.add(
            Some(main),
            NodeKind::Row,
            Style {
                grow: 1.0,
                align_items: Align::Stretch,
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        let content_style = Style {
            grow: 1.0,
            margin: padded(32.0, 0.0, 12.0, 20.0),
            align_items: Align::Stretch,
            ..Style::default()
        };
        let content = tree.add(
            Some(viewport),
            NodeKind::Column,
            content_style,
            Content::Panel,
            Look::default(),
        )?;
        tree.set_clip_children(content, true)?;
        let scroll_bar = tree.add(
            Some(viewport),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(10.0, 0.0),
                min: Size::new(10.0, 0.0),
                margin: padded(0.0, 4.0, 10.0, 20.0),
                ..Style::default()
            },
            Content::Label {
                text: "▐".to_owned(),
                style: TextStyle::new(Face::Body, 10.0),
            },
            Look {
                text: rgb(crate::widgets::scroll::THUMB),
                ..Look::default()
            },
        )?;
        let wizard = super::wizard::Wizard::build(tree, content)?;
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

        let overlay_host = tree.add(
            Some(root),
            NodeKind::Stack,
            Style {
                align_items: Align::Center,
                align_self: Some(Align::Stretch),
                ..Style::default()
            },
            Content::Panel,
            Look::default(),
        )?;
        tree.set_overlay_host(overlay_host)?;

        let hosts = vec![None; screens.len()];
        let mut shell = Self {
            screens,
            hosts,
            nav,
            content,
            scroll: ScrollView::new(),
            scroll_bar,
            title,
            subtitle,
            breadcrumb,
            edition,
            undo,
            redo,
            reset,
            open_button,
            refresh,
            save,
            reports_banner,
            reports_ok,
            reports_off,
            status,
            selected: 0,
            proxy,
            app: sse_app::state::AppState::new(),
            wizard,
        };
        shell.show(tree, 0)?;
        Ok(shell)
    }

    /// Shared application state.
    #[must_use]
    pub fn app(&self) -> &sse_app::state::AppState {
        &self.app
    }

    /// Installs the event proxy after a headless shell has been built.
    pub fn set_proxy(&mut self, proxy: Proxy<AppMessage>) {
        self.proxy = Some(proxy);
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
            app: &mut self.app,
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
        if tree.dialog_open() {
            let _ = tree.close_dialog()?;
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
        self.scroll.scroll_to(0.0);
        tree.set_scroll_y(self.content, 0)?;
        self.show(tree, index)
    }

    fn show(&mut self, tree: &mut Tree, index: usize) -> Result<()> {
        let (Some(screen), Some(slot)) = (self.screens.get_mut(index), self.hosts.get_mut(index)) else {
            return Ok(());
        };
        tree.set_text(self.title, crate::strings::t(screen.id().title()))?;
        tree.set_text(self.subtitle, crate::strings::t(screen.subtitle()))?;
        tree.set_text(
            self.breadcrumb,
            &format!(
                "{} / {}",
                crate::strings::t(screen.id().group().caption()),
                crate::strings::t(screen.id().title())
            ),
        )?;
        tree.set_text(self.edition, self.app.selected_game().unwrap_or("X-Ray / S2"))?;
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
        let screen_id = screen.id();
        let screen_host = *slot;
        let status = cx.status.take();
        drop(cx);
        self.wizard.sync(tree, &self.app, screen_id, screen_host)?;
        if let Some(text) = status {
            tree.set_text(self.status, &text)?;
        }
        if !tree.dialog_open() {
            if let Some(host) = screen_host {
                if let Some(focus) = tree.first_focusable_in(host) {
                    tree.set_focus(Some(focus))?;
                }
            }
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
        if let Message::Window(WindowEvent::Wheel { delta }) = message {
            let viewport = tree.rect(self.content)?;
            let content_height = tree.content_height(self.content)?;
            self.scroll.set_extent(content_height, viewport.height as f32);
            if self.scroll.wheel(-(*delta as f32)) {
                tree.set_scroll_y(self.content, to_px(self.scroll.offset_y().round()))?;
            }
            tree.set_visible(self.scroll_bar, self.scroll.thumb().is_some())?;
            return Ok(Flow::Continue);
        }
        let mut wizard_status = None;
        if let Some(target) = self.wizard.message(tree, message, clicked, &mut wizard_status)? {
            self.open(tree, target)?;
            return Ok(Flow::Continue);
        }
        if let Some(text) = wizard_status {
            tree.set_text(self.status, &text)?;
        }
        if clicked.is_some() && clicked == Some(self.reports_ok) {
            let mut settings = sse_app::AppSettings::load(&sse_app::default_settings_path());
            settings.reports_notice_shown = true;
            settings.save(&sse_app::default_settings_path())?;
            tree.set_visible(self.reports_banner, false)?;
            tree.set_text(self.status, "Настройки отчётов сохранены.")?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.reports_off) {
            let mut settings = sse_app::AppSettings::load(&sse_app::default_settings_path());
            settings.reports_notice_shown = true;
            settings.send_reports = false;
            settings.save(&sse_app::default_settings_path())?;
            tree.set_visible(self.reports_banner, false)?;
            tree.set_text(self.status, "Отправка отчётов отключена.")?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && [self.undo, self.redo, self.reset, self.save].contains(&clicked.unwrap_or(self.undo)) {
            tree.set_text(
                self.status,
                "Действие недоступно: AppState ещё не предоставляет журнал черновика и транзакционный save API.",
            )?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.open_button) {
            tree.set_text(
                self.status,
                "Открыть… недоступно: системный file-picker ещё не подключён к Shell.",
            )?;
            return Ok(Flow::Continue);
        }
        if clicked.is_some() && clicked == Some(self.refresh) {
            tree.set_text(
                self.status,
                "Обновление библиотеки недоступно: AppState ещё не предоставляет refresh/cancel API.",
            )?;
            return Ok(Flow::Continue);
        }
        if let Some(index) = clicked.and_then(|id| self.nav.iter().position(|nav| *nav == id)) {
            if let Some(nav) = self.nav.get(index).copied() {
                tree.set_focus(Some(nav))?;
            }
            self.select(tree, index)?;
            return Ok(Flow::Continue);
        }
        if let Some(clicked) = clicked {
            tree.set_focus(Some(clicked))?;
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            ctrl,
            shift,
            ..
        }) = message
        {
            if *keysym == KEY_ESCAPE {
                self.route(tree, message, None)?;
                if tree.dialog_open() {
                    let _ = tree.close_dialog()?;
                } else {
                    tree.set_focus(None)?;
                }
                return Ok(Flow::Continue);
            }
            if *keysym == KEY_TAB {
                let _ = tree.focus_next(*shift);
                self.route(tree, message, None)?;
                return Ok(Flow::Continue);
            }
            if *keysym == KEY_RETURN {
                if let Some(index) = tree
                    .focused()
                    .and_then(|focus| self.nav.iter().position(|nav| *nav == focus))
                {
                    self.select(tree, index)?;
                    return Ok(Flow::Continue);
                }
                let target = tree.focused().or_else(|| {
                    self.hosts
                        .get(self.selected)
                        .copied()
                        .flatten()
                        .and_then(|host| tree.first_focusable_in(host))
                });
                if let Some(target) = target {
                    tree.set_focus(Some(target))?;
                }
                self.route(tree, message, target)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x46 | 0x66) {
                if let Some(index) = self.screens.iter().position(|screen| screen.id() == ScreenId::Overview) {
                    self.select(tree, index)?;
                }
                self.route(tree, message, None)?;
                return Ok(Flow::Continue);
            }
            if !tree.dialog_open() && *ctrl && matches!(*keysym, 0x53 | 0x73) {
                if let Some(index) = self
                    .screens
                    .iter()
                    .position(|screen| screen.id() == ScreenId::Inventory)
                {
                    self.select(tree, index)?;
                }
                self.route(tree, message, None)?;
                return Ok(Flow::Continue);
            }
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            ctrl: false,
            ..
        }) = message
        {
            if !tree.dialog_open() {
                let count = self.nav.len();
                match *keysym {
                    KEY_UP => self.select(tree, self.selected.checked_sub(1).unwrap_or(count.saturating_sub(1)))?,
                    KEY_DOWN => {
                        let next = self.selected.saturating_add(1);
                        self.select(tree, if next >= count { 0 } else { next })?;
                    }
                    _ => {}
                }
            }
        }
        self.route(tree, message, clicked)?;
        if let Some(screen) = self.screens.get(self.selected) {
            let host = self.hosts.get(self.selected).copied().flatten();
            self.wizard.sync(tree, &self.app, screen.id(), host)?;
        }
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

#[cfg(test)]
mod tests {
    use super::Shell;
    use crate::event_loop::{Flow, Message, WindowEvent};
    use crate::glyphs::Fonts;
    use crate::raster::Color;
    use crate::widget::Tree;

    #[test]
    fn escape_does_not_close_the_application() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff1b,
            text: None,
            ctrl: false,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        Ok(())
    }

    #[test]
    fn tab_moves_visible_keyboard_focus_and_repaints_its_ring() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        tree.resize(940, 700);
        let mut frame = vec![0_u32; 940 * 700];
        tree.paint(&mut frame, 940)?;
        let before = frame.clone();
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: false,
        });
        shell.handle(&mut tree, &message, None)?;
        tree.paint(&mut frame, 940)?;
        assert!(
            frame.iter().zip(&before).any(|(after, before)| after != before),
            "Tab must show a keyboard focus ring"
        );
        Ok(())
    }

    #[test]
    fn ctrl_s_opens_inventory_and_keeps_the_shell_running() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('s'),
            text: None,
            ctrl: true,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        assert_eq!(shell.current(), Some(super::super::ScreenId::Inventory));
        Ok(())
    }

    #[test]
    fn ctrl_f_does_not_focus_search_hidden_by_the_first_run_wizard() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let search = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: u32::from('f'),
            text: None,
            ctrl: true,
            shift: false,
        });
        shell.handle(&mut tree, &search, None)?;
        assert_eq!(tree.focused(), None);
        Ok(())
    }

    #[test]
    fn escape_closes_a_widget_dialog_without_exiting() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let overlay = tree
            .overlay_host()
            .ok_or_else(|| sse_core::Error::damaged("missing dialog overlay"))?;
        let dialog = tree.add(
            Some(overlay),
            crate::layout::NodeKind::Column,
            crate::layout::Style::default(),
            crate::widget::Content::Panel,
            crate::widget::Look::default(),
        )?;
        tree.open_dialog(dialog)?;
        let message = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff1b,
            text: None,
            ctrl: false,
            shift: false,
        });
        assert!(matches!(shell.handle(&mut tree, &message, None)?, Flow::Continue));
        assert!(!tree.dialog_open());
        Ok(())
    }

    #[test]
    fn shift_tab_returns_to_the_previous_visible_control() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let mut shell = Shell::build(&mut tree, None)?;
        let forward = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: false,
        });
        shell.handle(&mut tree, &forward, None)?;
        let first = tree.focused();
        shell.handle(&mut tree, &forward, None)?;
        let second = tree.focused();
        let backwards = Message::Window(WindowEvent::Key {
            pressed: true,
            keysym: 0xff09,
            text: None,
            ctrl: false,
            shift: true,
        });
        shell.handle(&mut tree, &backwards, None)?;
        assert_ne!(first, second);
        assert_eq!(tree.focused(), first);
        Ok(())
    }
}
