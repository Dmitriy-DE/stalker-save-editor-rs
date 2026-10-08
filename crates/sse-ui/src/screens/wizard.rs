//! First-run save discovery wizard from ACCEPTANCE §1.7.
use super::style::{self, Button, Text};
use super::ScreenId;
use crate::edit::{Clipboard, EditConfig, FieldMode, InputFilter, Key, Modifiers};
use crate::event_loop::{Message, WindowEvent};
use crate::glyphs::{Face, TextStyle};
use crate::layout::{Edges, NodeKind, Size, Style};
use crate::widget::{Content, Look, Tree, WidgetId};
use crate::widgets::text_input::TextInput;
use sse_core::Result;
use sse_storage::discovery::SaveDirectoryLocator;
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
        let mut settings = sse_app::AppSettings::load(&settings_path);
        let directories = settings.save_directories.get_or_insert_with(Vec::new);
        let key = path.to_string_lossy().trim().to_lowercase();
        if directories
            .iter()
            .any(|item| item.to_string_lossy().trim().to_lowercase() == key)
        {
            return Ok(false);
        }
        directories.push(path);
        settings.save(&settings_path)?;
        Ok(true)
    }
    fn auto_search() -> Result<usize> {
        let mut added = 0_usize;
        for candidate in SaveDirectoryLocator::find_candidate_directories(None) {
            if Self::save_directory(candidate.directory_path)? {
                added = added.saturating_add(1);
            }
        }
        Ok(added)
    }
    /// Handles wizard controls and returns a requested navigation target.
    pub fn message(
        &mut self,
        tree: &mut Tree,
        message: &Message<super::AppMessage>,
        clicked: Option<WidgetId>,
        status: &mut Option<String>,
    ) -> Result<Option<ScreenId>> {
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
            let added = Self::auto_search()?;
            *status = Some(if added == 0 {
                crate::strings::t("Автопоиск завершён. Новых папок не найдено.").to_owned()
            } else {
                crate::strings::t("Автопоиск завершён. Добавлено папок: {0}.").replace("{0}", &added.to_string())
            });
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
            }
        }
        if clicked.is_some() && clicked == Some(self.browse) {
            *status = Some(crate::strings::t("Выберите папку с сохранениями").to_owned());
        }
        if clicked.is_some() && clicked == Some(self.settings) {
            return Ok(Some(ScreenId::Settings));
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
    use super::{Text, Wizard};
    use crate::glyphs::Fonts;
    use crate::layout::{NodeKind, Style};
    use crate::raster::Color;
    use crate::widget::{Content, Look, Tree};

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
}
