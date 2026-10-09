import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from check_i18n_source import find_untranslated_cyrillic


class FindUntranslatedCyrillicTests(unittest.TestCase):
    def test_reports_cyrillic_string_outside_translation_call(self):
        self.assertEqual(
            [(1, "русский")],
            find_untranslated_cyrillic('let text = "русский";'),
        )

    def test_accepts_string_inside_translation_call(self):
        self.assertEqual([], find_untranslated_cyrillic('let text = crate::strings::t("Русский");'))

    def test_accepts_comments_between_translation_name_and_call(self):
        source = 'let text = crate::strings::t /* translated key */ ("Русский");'
        self.assertEqual([], find_untranslated_cyrillic(source))

    def test_accepts_format_string_nested_in_translation_call(self):
        source = 'service.tr_in(Some(language), format!("Найдено {0}", count), &[]);'
        self.assertEqual([], find_untranslated_cyrillic(source))

    def test_ignores_comments_and_reports_raw_string(self):
        source = '// "Комментарий"\nlet text = r###"Русский"###;'
        self.assertEqual([(2, "Русский")], find_untranslated_cyrillic(source))

    def test_ignores_nested_block_comments(self):
        source = '/* "Комментарий" /* "вложенный" */ */ let text = "English";'
        self.assertEqual([], find_untranslated_cyrillic(source))

    def test_detects_cyrillic_unicode_escape(self):
        self.assertEqual([(1, r"\u{0410}")], find_untranslated_cyrillic(r'let text = "\u{0410}";'))

    def test_lifetimes_do_not_hide_later_ui_strings(self):
        source = "fn label(value: &'static str) { let text = \"Русский\"; let marker = 'x'; }"
        self.assertEqual([(1, "Русский")], find_untranslated_cyrillic(source))

    def test_wizard_has_no_untranslated_cyrillic_literals(self):
        wizard = Path(__file__).resolve().parents[2] / "crates/sse-ui/src/screens/wizard.rs"
        self.assertEqual([], find_untranslated_cyrillic(wizard.read_text(encoding="utf-8")))


if __name__ == "__main__":
    unittest.main()
