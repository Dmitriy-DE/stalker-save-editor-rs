//! First-run save discovery wizard from ACCEPTANCE §1.7.
use super::style::{self, Button, Text};
use super::ScreenId;
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::{Message, Proxy, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{Edges, NodeKind, Size, Style};
use crate::widget::{Content, Look, Tree, WidgetId};
use crate::widgets::text_input::TextInput;
use sse_core::Result;
use sse_storage::discovery::{SaveDirectoryDiscoveryOptions, SaveDirectoryLocator};
use std::path::PathBuf;
struct EmptyClipboard;
impl Clipboard for EmptyClipboard {
    fn read_text(&mut self) -> Result<String> {
        Ok(String::new())
    }
    fn write_text(&mut self, _text: &str) -> Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WizardAction {
    AutoSearch,
    Browse,
    DirectoryAdded,
    Navigate(ScreenId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WizardTaskKind {
    AutoSearch,
    Browse,
}

impl WizardTaskKind {
    pub(super) fn failure_prefix(self) -> &'static str {
        self.failure_prefix_in(crate::strings::current_language())
    }

    fn failure_prefix_in(self, language: &str) -> &'static str {
        crate::strings::t_in(
            language,
            match self {
                Self::AutoSearch => "Не удалось найти папки с сохранениями",
                Self::Browse => "Не удалось открыть выбор папки",
            },
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WizardWorkResult {
    AutoSearch(usize),
    Directory(Option<PathBuf>),
}

pub(super) struct WizardTaskFinished {
    pub(super) request: u64,
    pub(super) kind: WizardTaskKind,
    pub(super) result: std::result::Result<WizardWorkResult, String>,
}

pub(super) fn spawn_wizard_task<F>(
    proxy: Proxy<super::AppMessage>,
    request: u64,
    kind: WizardTaskKind,
    work: F,
) -> std::io::Result<()>
where
    F: FnOnce() -> std::result::Result<WizardWorkResult, String> + Send + 'static,
{
    std::thread::Builder::new()
        .name("sse-wizard-discovery".to_owned())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .map_err(|_| crate::strings::t("задача мастера завершилась аварийно").to_owned())
                .and_then(|result| result);
            let _ = proxy.send(super::AppMessage::ToScreen(
                ScreenId::Overview,
                Box::new(WizardTaskFinished { request, kind, result }),
            ));
        })
        .map(|_| ())
}

/// Session-scoped first-run wizard. Skip is intentionally not persisted.
pub struct Wizard {
    host: WidgetId,
    #[cfg(test)]
    intro: WidgetId,
    path_input: WidgetId,
    auto: WidgetId,
    browse: WidgetId,
    add: WidgetId,
    settings: WidgetId,
    skip: WidgetId,
    input: TextInput,
    skipped: bool,
}
impl Wizard {
    /// Builds the hidden wizard once.
    pub fn build(tree: &mut Tree, parent: WidgetId) -> Result<Self> {
        Self::build_for_language(tree, parent, crate::strings::current_language())
    }

    fn build_for_language(tree: &mut Tree, parent: WidgetId, language: &str) -> Result<Self> {
        let host = style::card(tree, parent)?;
        style::label(
            tree,
            host,
            crate::strings::t_in(language, "МАСТЕР ПЕРВОГО ЗАПУСКА"),
            Text::Heading,
        )?;
        let _intro = tree.add(
            Some(host),
            NodeKind::Leaf,
            Style {
                preferred: Size::new(720.0, 0.0),
                min: Size::new(320.0, 0.0),
                max: Size::new(720.0, f32::INFINITY),
                ..Style::default()
            },
            Content::Paragraph {
                text: crate::strings::t_in(language, "Сохранения S.T.A.L.K.E.R. не были найдены в стандартных каталогах.\nУкажите папку с файлами сохранений (savedgames или SaveGames) или запустите автоматический поиск на диске.").to_owned(),
                style: Text::Body.style(),
            },
            Look { text: Text::Body.color(), ..Look::default() },
        )?;
        let auto = style::button(
            tree,
            host,
            crate::strings::t_in(language, "АВТОПОИСК ПАПОК НА ДИСКЕ"),
            Button::Primary,
        )?;
        style::label(
            tree,
            host,
            crate::strings::t_in(language, "— ИЛИ УКАЗАТЬ ПУТЬ ВРУЧНУЮ —"),
            Text::Note,
        )?;
        let colors = crate::theme::current().colors;
        let path_input = tree.add(
            Some(host),
            NodeKind::Leaf,
            Style {
                min: Size::new(320.0, crate::theme::BUTTON_HEIGHT),
                padding: Edges {
                    left: 12.0,
                    top: 0.0,
                    right: 12.0,
                    bottom: 0.0,
                },
                ..Style::default()
            },
            Content::Input {
                text: crate::strings::t_in(language, "Путь к папке с сейвами…").to_owned(),
                style: TextStyle::new(Face::Body, 16.0),
            },
            Look {
                fill: Some(style::rgb(colors.background[4])),
                border: Some((style::rgb(colors.borders[1]), 1.0)),
                radius: crate::theme::BUTTON_RADIUS,
                text: style::rgb(colors.text[2]),
                ..Look::default()
            },
        )?;
        let actions = style::row(tree, host)?;
        let browse = style::button(
            tree,
            actions,
            crate::strings::t_in(language, "Обзор…"),
            Button::Secondary,
        )?;
        let add = style::button(
            tree,
            actions,
            crate::strings::t_in(language, "Добавить"),
            Button::Primary,
        )?;
        let settings = style::button(
            tree,
            host,
            crate::strings::t_in(language, "Перейти в настройки"),
            Button::Secondary,
        )?;
        let skip = style::button(
            tree,
            host,
            crate::strings::t_in(language, "Пропустить"),
            Button::Secondary,
        )?;
        tree.set_visible(host, false)?;
        Ok(Self {
            host,
            #[cfg(test)]
            intro: _intro,
            path_input,
            auto,
            browse,
            add,
            settings,
            skip,
            input: TextInput::new(
                "",
                EditConfig {
                    mode: FieldMode::SingleLine,
                    max_graphemes: 4096,
                    history_limit: 16,
                    filter: InputFilter::Any,
                },
            )?,
            skipped: false,
        })
    }
    fn eligible(screen: ScreenId) -> bool {
        matches!(
            screen,
            ScreenId::Games
                | ScreenId::Overview
                | ScreenId::Inventory
                | ScreenId::Factions
                | ScreenId::Stashes
                | ScreenId::Transitions
                | ScreenId::Backups
        )
    }

    /// Returns whether the first-run wizard currently replaces the selected screen.
    #[must_use]
    pub fn is_showing(&self, tree: &Tree) -> bool {
        tree.is_visible(self.host)
    }

    /// Synchronizes visibility and hides the normal screen host while active.
    pub fn sync(
        &self,
        tree: &mut Tree,
        app: &sse_app::state::AppState,
        screen: ScreenId,
        screen_host: Option<WidgetId>,
    ) -> Result<()> {
        let visible = !self.skipped && app.recent_saves().is_empty() && Self::eligible(screen);
        tree.set_visible(self.host, visible)?;
        if let Some(host) = screen_host {
            tree.set_visible(host, !visible)?;
        }
        Ok(())
    }
    fn save_directory(path: PathBuf) -> Result<bool> {
        let settings_path = sse_app::default_settings_path();
        let mut settings = sse_app::AppSettings::load(&settings_path)?;
        let directories = settings.save_directories.get_or_insert_with(Vec::new);
        let key = path.to_string_lossy().trim().to_lowercase();
        if directories
            .iter()
            .any(|item| item.to_string_lossy().trim().to_lowercase() == key)
        {
            return Ok(false);
        }
        directories.push(path);
        super::submit_settings_write(
            sse_app::settings_writer::SettingsPatch::SaveDirectories(directories.clone()),
            None,
        )?;
        Ok(true)
    }
    pub(super) fn auto_search() -> Result<usize> {
        let settings_path = sse_app::default_settings_path();
        let mut settings = sse_app::AppSettings::load(&settings_path)?;
        let options = SaveDirectoryDiscoveryOptions {
            custom_save_directories: settings.save_directories.clone(),
            ..SaveDirectoryDiscoveryOptions::default()
        };
        let directories = settings.save_directories.get_or_insert_with(Vec::new);
        let mut known: std::collections::HashSet<String> = directories
            .iter()
            .map(|path| path.to_string_lossy().trim().to_lowercase())
            .collect();
        let mut added = 0_usize;
        for candidate in SaveDirectoryLocator::find_candidate_directories(Some(&options)) {
            let key = candidate.directory_path.to_string_lossy().trim().to_lowercase();
            if known.insert(key) {
                directories.push(candidate.directory_path);
                added = added.saturating_add(1);
            }
        }
        if added > 0 {
            super::submit_settings_write(
                sse_app::settings_writer::SettingsPatch::SaveDirectories(directories.clone()),
                None,
            )?;
        }
        Ok(added)
    }

    pub(super) fn set_busy(&self, tree: &mut Tree, busy: bool) -> Result<()> {
        tree.set_enabled(self.auto, !busy)?;
        tree.set_enabled(self.browse, !busy)?;
        tree.set_enabled(self.add, !busy)
    }

    pub(super) fn set_directory_input(&mut self, tree: &mut Tree, path: PathBuf) -> Result<()> {
        let text = path.to_string_lossy().into_owned();
        self.input = TextInput::new(
            &text,
            EditConfig {
                mode: FieldMode::SingleLine,
                max_graphemes: 4096,
                history_limit: 16,
                filter: InputFilter::Any,
            },
        )?;
        tree.set_input_text(self.path_input, &text)
    }

    /// Handles wizard controls and returns deferred shell actions when needed.
    pub fn message(
        &mut self,
        tree: &mut Tree,
        message: &Message<super::AppMessage>,
        clicked: Option<WidgetId>,
        status: &mut Option<String>,
    ) -> Result<Option<WizardAction>> {
        self.input.focus(tree.focused() == Some(self.path_input), 0);
        if clicked.is_some() && clicked == Some(self.path_input) {
            self.input.focus(true, 0);
        }
        if let Message::Window(WindowEvent::Ime(event)) = message {
            if self.input.focused() {
                self.input.apply_ime_event(event)?;
                tree.set_input_text(self.path_input, &self.input.display_text())?;
            }
            return Ok(None);
        }
        if clicked.is_some() && clicked == Some(self.auto) {
            return Ok(Some(WizardAction::AutoSearch));
        }
        if clicked.is_some() && clicked == Some(self.add) {
            let path = self.input.text();
            let trimmed = path.trim();
            if !trimmed.is_empty() && Self::save_directory(PathBuf::from(trimmed))? {
                self.input = TextInput::new(
                    "",
                    EditConfig {
                        mode: FieldMode::SingleLine,
                        max_graphemes: 4096,
                        history_limit: 16,
                        filter: InputFilter::Any,
                    },
                )?;
                tree.set_text(self.path_input, crate::strings::t("Путь к папке с сейвами…"))?;
                return Ok(Some(WizardAction::DirectoryAdded));
            }
        }
        if clicked.is_some() && clicked == Some(self.browse) {
            return Ok(Some(WizardAction::Browse));
        }
        if clicked.is_some() && clicked == Some(self.settings) {
            return Ok(Some(WizardAction::Navigate(ScreenId::Settings)));
        }
        if clicked.is_some() && clicked == Some(self.skip) {
            self.skipped = true;
            tree.set_visible(self.host, false)?;
            *status = Some(crate::strings::t("ВЫБЕРИТЕ СОХРАНЕНИЕ").to_owned());
        }
        if let Message::Window(WindowEvent::Key {
            pressed: true,
            keysym,
            text,
            ctrl,
            shift,
        }) = message
        {
            if *keysym == 0xff09 || (*ctrl && matches!(*keysym, 0x46 | 0x66 | 0x53 | 0x73)) {
                return Ok(None);
            }
            if matches!(*keysym, 0xff0d | 0xff1b) && self.input.focused() {
                self.input.focus(false, 0);
                tree.set_focus(None)?;
                return Ok(None);
            }
            if self.input.focused() {
                let key = match *keysym {
                    0xff08 => Key::Backspace,
                    0xffff => Key::Delete,
                    0xff51 => Key::Left,
                    0xff53 => Key::Right,
                    0xff50 => Key::Home,
                    0xff57 => Key::End,
                    value if *ctrl && matches!(value, 0x61 | 0x41) => Key::A,
                    value if *ctrl && matches!(value, 0x7a | 0x5a) => Key::Z,
                    _ => Key::Character(text.unwrap_or('\0')),
                };
                let typed = text.map(|character| character.to_string());
                let mut clipboard = EmptyClipboard;
                if self.input.key(
                    key,
                    Modifiers {
                        ctrl: *ctrl,
                        shift: *shift,
                    },
                    typed.as_deref(),
                    &mut clipboard,
                )? {
                    let shown = self.input.text();
                    tree.set_text(
                        self.path_input,
                        if shown.is_empty() {
                            crate::strings::t("Путь к папке с сейвами…")
                        } else {
                            &shown
                        },
                    )?;
                }
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::{spawn_wizard_task, Text, Wizard, WizardAction, WizardTaskFinished, WizardTaskKind, WizardWorkResult};
    use crate::event_loop::Message;
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::screens::AppMessage;
    use crate::widget::{Content, Look, Tree};

    #[test]
    fn wizard_task_failure_prefix_uses_the_selected_language() {
        assert_eq!(
            WizardTaskKind::AutoSearch.failure_prefix_in("en"),
            "Could not find save folders"
        );
        assert_eq!(
            WizardTaskKind::Browse.failure_prefix_in("uk"),
            crate::strings::t_in("uk", "Не удалось открыть выбор папки")
        );
    }

    #[test]
    fn wizard_intro_reserves_height_for_its_narrowest_layout() -> sse_core::Result<()> {
        let result = (|| {
            let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(12, 13, 10, 255));
            let root = tree.add(
                None,
                NodeKind::Column,
                Style::default(),
                Content::Panel,
                Look::default(),
            )?;
            let wizard = Wizard::build_for_language(&mut tree, root, "ru")?;
            tree.set_visible(wizard.host, true)?;
            tree.resize(940, 600);
            let mut frame = vec![0_u32; 940 * 600];
            tree.paint(&mut frame, 940)?;

            let intro = tree.rect(wizard.intro)?;
            let fonts = Fonts::bundled()?;
            let text = crate::strings::t_in(
                "ru",
                "Сохранения S.T.A.L.K.E.R. не были найдены в стандартных каталогах.\nУкажите папку с файлами сохранений (savedgames или SaveGames) или запустите автоматический поиск на диске.",
            );
            let lines = crate::text::break_lines(text, 320.0, &fonts.metrics(Text::Body.style()));
            let mut required_height = 0.0_f64;
            for _ in &lines {
                required_height += f64::from(fonts.line_height(Text::Body.style()));
            }
            required_height = required_height.ceil();
            assert!(
                f64::from(intro.height) >= required_height,
                "intro has {} px for text requiring {required_height:.0} px at the minimum width",
                intro.height
            );
            let auto = tree.rect(wizard.auto)?;
            assert!(
                i64::from(auto.y) >= i64::from(intro.y) + i64::from(intro.height),
                "auto-search button must follow the intro"
            );
            Ok(())
        })();
        result
    }

    #[test]
    fn browse_button_requests_a_folder_picker() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let root = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut wizard = Wizard::build(&mut tree, root)?;
        let browse = wizard.browse;
        let mut status = None;

        let action = wizard.message(
            &mut tree,
            &Message::User(AppMessage::Tick(0)),
            Some(browse),
            &mut status,
        )?;

        assert_eq!(
            action,
            Some(WizardAction::Browse),
            "Browse must request a directory picker instead of only changing the status line"
        );
        Ok(())
    }

    #[test]
    fn selected_directory_is_written_to_the_visible_and_editable_input() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let root = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut wizard = Wizard::build(&mut tree, root)?;
        let path = std::env::temp_dir().join("selected-save-directory");
        let expected = path.to_string_lossy().into_owned();

        wizard.set_directory_input(&mut tree, path)?;

        assert_eq!(tree.input_text(wizard.path_input)?, expected);
        assert_eq!(wizard.input.text(), expected);
        Ok(())
    }

    #[test]
    fn wizard_discovery_runs_off_ui_thread_and_returns_its_result() -> sse_core::Result<()> {
        use std::sync::mpsc;
        use std::time::Duration;

        let (proxy, receiver) = crate::event_loop::channel_pair::<AppMessage>();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let ui_thread = std::thread::current().id();
        spawn_wizard_task(proxy, 41, WizardTaskKind::AutoSearch, move || {
            started_tx
                .send(std::thread::current().id())
                .map_err(|error| error.to_string())?;
            release_rx
                .recv_timeout(Duration::from_secs(2))
                .map_err(|error| error.to_string())?;
            Ok(WizardWorkResult::AutoSearch(3))
        })?;

        let worker_thread = started_rx
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        assert_ne!(worker_thread, ui_thread);
        release_tx
            .send(())
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        let message = receiver
            .recv_timeout(Duration::from_secs(1))
            .map_err(|error| sse_core::Error::System(error.to_string()))?;
        let Message::User(AppMessage::ToScreen(super::super::ScreenId::Overview, payload)) = message else {
            return Err(sse_core::Error::Refused(
                "wizard worker result was not routed to the shell".to_owned(),
            ));
        };
        let finished = payload
            .downcast_ref::<WizardTaskFinished>()
            .ok_or_else(|| sse_core::Error::Refused("wizard worker returned the wrong payload type".to_owned()))?;
        assert_eq!(finished.request, 41);
        assert_eq!(finished.kind, WizardTaskKind::AutoSearch);
        assert_eq!(finished.result, Ok(WizardWorkResult::AutoSearch(3)));
        Ok(())
    }

    #[test]
    fn auto_search_button_returns_a_deferred_work_request() -> sse_core::Result<()> {
        let mut tree = Tree::new(Fonts::bundled()?, Color::rgba(0, 0, 0, 255));
        let root = tree.add(
            None,
            NodeKind::Column,
            Style::default(),
            Content::Panel,
            Look::default(),
        )?;
        let mut wizard = Wizard::build(&mut tree, root)?;
        let auto = wizard.auto;
        let mut status = None;

        let action = wizard.message(&mut tree, &Message::User(AppMessage::Tick(0)), Some(auto), &mut status)?;

        assert_eq!(
            action,
            Some(WizardAction::AutoSearch),
            "auto search must return work to the shell instead of scanning in the click handler"
        );
        Ok(())
    }
}
