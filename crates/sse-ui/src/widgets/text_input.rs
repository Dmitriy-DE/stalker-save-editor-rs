//! TextBox interaction state backed by edit::EditModel.

use crate::edit::{Clipboard, EditConfig, EditModel, Key, Modifiers, MouseSelect, Selection};
use crate::event_loop::ImeEvent;
use sse_core::Result;

/// C# TextBox background RGB.
pub const BACKGROUND: u32 = 0x1A1D17;
/// C# TextBox normal border RGB.
pub const BORDER: u32 = 0x33382F;
/// C# TextBox focus/caret RGB.
pub const FOCUS: u32 = 0xD6A62D;
const BLINK_MS: u64 = 530;

/// Stateful controller around the edit model; retained painting is handled by the widget tree.
pub struct TextInput {
    model: EditModel,
    focused: bool,
    caret_visible: bool,
    last_blink_ms: u64,
}

impl TextInput {
    /// Create a text input with the supplied edit policy.
    pub fn new(text: &str, config: EditConfig) -> Result<Self> {
        Ok(Self {
            model: EditModel::new(text, config)?,
            focused: false,
            caret_visible: false,
            last_blink_ms: 0,
        })
    }

    /// Current text.
    #[must_use]
    pub fn text(&self) -> String {
        self.model.text()
    }

    /// Text shown while an input method has uncommitted preedit text.
    #[must_use]
    pub fn display_text(&self) -> String {
        let text = self.model.text();
        let Some(composition) = self.model.composition() else {
            return text;
        };
        let (start, end) = composition.range.ordered();
        let mut chars = text.chars();
        let mut displayed = String::new();
        for _ in 0..start {
            let Some(character) = chars.next() else {
                break;
            };
            displayed.push(character);
        }
        displayed.push_str(&composition.text);
        for _ in start..end {
            let _ = chars.next();
        }
        displayed.extend(chars);
        displayed
    }

    /// Applies one platform input-method event. A commit is one undoable text edit.
    ///
    /// # Errors
    /// Returns an error when the committed text violates the edit model's structural limits.
    pub fn apply_ime_event(&mut self, event: &ImeEvent) -> Result<bool> {
        match event {
            ImeEvent::Start => {
                self.model.set_composition(String::new());
                Ok(true)
            }
            ImeEvent::Update(text) => {
                self.model.update_composition(text.clone());
                Ok(true)
            }
            ImeEvent::Commit(text) => {
                self.model.update_composition(text.clone());
                self.model.commit_composition()
            }
            ImeEvent::Cancel => {
                let changed = self.model.composition().is_some();
                self.model.cancel_composition();
                Ok(changed)
            }
        }
    }

    /// Current scalar selection.
    #[must_use]
    pub const fn selection(&self) -> Selection {
        self.model.selection()
    }

    /// Whether the field owns keyboard focus.
    #[must_use]
    pub const fn focused(&self) -> bool {
        self.focused
    }

    /// Whether the blinking caret is currently visible.
    #[must_use]
    pub const fn caret_visible(&self) -> bool {
        self.focused && self.caret_visible
    }

    /// Change focus and restart the caret blink timer.
    pub fn focus(&mut self, focused: bool, now_ms: u64) -> bool {
        let changed = self.focused != focused;
        self.focused = focused;
        self.caret_visible = focused;
        self.last_blink_ms = now_ms;
        changed
    }

    /// Advance the caret timer; true when the caret needs repaint.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        if !self.focused {
            return false;
        }
        if now_ms.saturating_sub(self.last_blink_ms) < BLINK_MS {
            return false;
        }
        self.last_blink_ms = now_ms;
        self.caret_visible = !self.caret_visible;
        true
    }

    /// Apply single/double/triple-click selection at a grapheme hit.
    pub fn mouse(&mut self, grapheme: usize, clicks: u8, shift: bool) {
        let kind = match clicks {
            2 => MouseSelect::Word,
            3..=u8::MAX => MouseSelect::Line,
            _ => MouseSelect::Caret,
        };
        self.model.mouse_select(grapheme, kind, shift);
        self.caret_visible = true;
    }

    /// text is the platform WindowEvent::Key.text payload, preserving keyboard layout and composed text.
    pub fn key<C: Clipboard>(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        text: Option<&str>,
        clipboard: &mut C,
    ) -> Result<bool> {
        self.caret_visible = true;
        if !modifiers.ctrl {
            if let Some(value) = text {
                if !value.is_empty() {
                    return self.model.insert_text(value);
                }
            }
        }
        self.model.key(key, modifiers, clipboard)
    }

    /// Paste through the platform clipboard.
    pub fn paste<C: Clipboard>(&mut self, c: &mut C) -> Result<bool> {
        self.model.paste(c)
    }

    /// Reason the last paste inserted nothing, taken once.
    pub fn take_paste_refusal(&mut self) -> Option<crate::edit::PasteRefusal> {
        self.model.take_paste_refusal()
    }

    /// Cut the current selection through the platform clipboard.
    pub fn cut<C: Clipboard>(&mut self, c: &mut C) -> Result<bool> {
        self.model.cut(c)
    }

    /// Copy the current selection through the platform clipboard.
    pub fn copy<C: Clipboard>(&self, c: &mut C) -> Result<bool> {
        self.model.copy(c)
    }

    /// Undo the latest edit-model transaction.
    pub fn undo(&mut self) -> Result<bool> {
        self.model.undo()
    }
}

/// Status text for a paste that inserted nothing, for any text field.
#[must_use]
pub fn paste_refusal_message(refusal: crate::edit::PasteRefusal) -> &'static str {
    match refusal {
        crate::edit::PasteRefusal::TooLong => {
            crate::strings::t("Вставка не поместилась: текст длиннее оставшегося места в поле.")
        }
        crate::edit::PasteRefusal::NotAllowed => {
            crate::strings::t("Вставка не принята: текст не подходит под формат поля.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{FieldMode, InputFilter};
    use crate::event_loop::ImeEvent;

    struct Clip(String);

    impl Clipboard for Clip {
        fn read_text(&mut self) -> Result<String> {
            Ok(self.0.clone())
        }

        fn write_text(&mut self, t: &str) -> Result<()> {
            self.0 = t.to_owned();
            Ok(())
        }
    }

    #[test]
    fn text_payload_and_shortcuts() {
        let mut t = TextInput::new(
            "",
            EditConfig {
                mode: FieldMode::SingleLine,
                max_graphemes: 20,
                history_limit: 20,
                filter: InputFilter::Any,
            },
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        let mut c = Clip(String::new());
        assert!(t
            .key(Key::Character('x'), Modifiers::default(), Some("Ж"), &mut c)
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert_eq!(t.text(), "Ж");
        assert!(t
            .key(
                Key::A,
                Modifiers {
                    ctrl: true,
                    shift: false,
                },
                None,
                &mut c,
            )
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert!(t
            .key(
                Key::C,
                Modifiers {
                    ctrl: true,
                    shift: false,
                },
                None,
                &mut c,
            )
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert_eq!(c.0, "Ж");
    }

    #[test]
    fn ime_preedit_is_visible_without_mutating_text_until_commit() {
        let mut input = TextInput::new("x", EditConfig::default()).unwrap_or_else(|error| panic!("{error:?}"));

        assert!(input
            .apply_ime_event(&ImeEvent::Start)
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert!(input
            .apply_ime_event(&ImeEvent::Update("かな".to_owned()))
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert_eq!(input.text(), "x");
        assert_eq!(input.display_text(), "xかな");

        assert!(input
            .apply_ime_event(&ImeEvent::Commit("仮名".to_owned()))
            .unwrap_or_else(|error| panic!("{error:?}")));
        assert_eq!(input.text(), "x仮名");
        assert!(input.undo().unwrap_or_else(|error| panic!("{error:?}")));
        assert_eq!(input.text(), "x");
    }

    #[test]
    fn caret_blinks_only_when_focused() {
        let mut t = TextInput::new("", EditConfig::default()).unwrap_or_else(|error| panic!("{error:?}"));
        assert!(t.focus(true, 10));
        assert!(!t.tick(100));
        assert!(t.tick(600));
        assert!(!t.caret_visible());
    }
}
