#!/usr/bin/env python3
"""Generate the static UI localization table from C# and checked-in Rust catalogs."""
from __future__ import annotations
import json
import pathlib
import sys

LANGS = ["ru","uk","en","de","fr","it","es","pl","cs","pt-BR","tr","ja","ko","zh-CN","zh-TW"]

def q(value: str) -> str:
    """Rust string literal; control, format and other invisible characters are written as \\u{..} escapes."""
    import unicodedata
    out = []
    for ch in value:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif unicodedata.category(ch) in ("Cc", "Cf", "Zl", "Zp") or (unicodedata.category(ch) == "Zs" and ch != " "):
            out.append("\\u{%x}" % ord(ch))
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'

def main() -> int:
    if len(sys.argv) != 2:
        print("usage: tools/generate_strings.py <directory-containing-i18n>", file=sys.stderr)
        return 2
    root = pathlib.Path(sys.argv[1])
    source = root / "i18n" if (root / "i18n").is_dir() else root
    fallback_source = pathlib.Path(__file__).resolve().parents[1] / "crates/sse-catalog/i18n"
    catalogs = {}
    fallback_catalogs = {}
    keys = set()
    for lang in LANGS:
        path = source / f"{lang}.json"
        with path.open("r", encoding="utf-8") as handle:
            catalog = json.load(handle)
        if not isinstance(catalog, dict):
            raise ValueError(f"{path}: expected object")
        catalogs[lang] = catalog
        keys.update(catalog)
        fallback_path = fallback_source / f"{lang}.json"
        with fallback_path.open("r", encoding="utf-8") as handle:
            fallback_catalog = json.load(handle)
        if not isinstance(fallback_catalog, dict):
            raise ValueError(f"{fallback_path}: expected object")
        fallback_catalogs[lang] = fallback_catalog
        keys.update(fallback_catalog)
    extras_path = pathlib.Path(__file__).with_name("i18n-extra.json")
    extras = json.loads(extras_path.read_text(encoding="utf-8")) if extras_path.exists() else {}
    if not isinstance(extras, dict):
        raise ValueError(f"{extras_path}: expected object")
    keys.update(extras)
    rows = []
    for key in sorted(keys):
        values = []
        for lang in LANGS:
            extra = extras.get(key, {})
            value = extra.get(lang) if isinstance(extra, dict) else None
            if not isinstance(value, str) or not value:
                value = key if lang == "ru" else catalogs[lang].get(key)
            if not isinstance(value, str) or not value:
                value = fallback_catalogs[lang].get(key)
            if not isinstance(value, str) or not value:
                value = catalogs["en"].get(key) if lang != "en" else None
            if not isinstance(value, str) or not value:
                value = fallback_catalogs["en"].get(key) if lang != "en" else None
            if not isinstance(value, str) or not value:
                value = key
            values.append(value)
        if key in extras and any(not isinstance(extras[key].get(lang), str) or not extras[key].get(lang) for lang in LANGS):
            raise ValueError(f"{extras_path}: Rust-only key {key!r} must provide all 15 translations")
        rows.append(f"    ({q(key)}, [" + ", ".join(q(v) for v in values) + "]),")
    header = """//! Generated from C# and checked-in Rust localization catalogs. Do not edit by hand.
use std::sync::atomic::{AtomicU8, Ordering};
/// Interface languages, Russian first (it is the key language).
pub const LANGUAGES: [&str; 15] = [\"ru\",\"uk\",\"en\",\"de\",\"fr\",\"it\",\"es\",\"pl\",\"cs\",\"pt-BR\",\"tr\",\"ja\",\"ko\",\"zh-CN\",\"zh-TW\"];
static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(0);
/// Index of a language code in [`LANGUAGES`]; unknown codes fall back to Russian.
#[must_use]
pub fn language_index(code: &str) -> usize {
    let clean = code.trim().replace('_', "-");
    let lower = clean.to_ascii_lowercase();
    if lower.starts_with("zh") { return if lower.contains("tw") || lower.contains("hk") || lower.contains("hant") { 14 } else { 13 }; }
    if lower.starts_with("pt") { return 9; }
    LANGUAGES.iter().position(|v| v.eq_ignore_ascii_case(&clean))
        .or_else(|| lower.split('-').next().and_then(|base| LANGUAGES.iter().position(|v| v.eq_ignore_ascii_case(base))))
        .unwrap_or(0)
}
/// Sets the interface language (`None` = Russian).
pub fn set_language(code: Option<&str>) {
    let index = code.map_or(0, language_index);
    CURRENT_LANGUAGE.store(u8::try_from(index).unwrap_or(0), Ordering::Relaxed);
}
/// Current interface language code.
#[must_use]
pub fn current_language() -> &'static str {
    LANGUAGES.get(usize::from(CURRENT_LANGUAGE.load(Ordering::Relaxed))).copied().unwrap_or("ru")
}
/// Translation of a Russian key into the current language; unknown keys are returned as is.
#[must_use]
pub fn t(key: &str) -> &str { t_in(current_language(), key) }
/// Translation of a Russian key into `language`.
#[must_use]
pub fn t_in<'a>(language: &str, key: &'a str) -> &'a str {
    let lang = language_index(language);
    match STRINGS.binary_search_by(|entry| entry.0.cmp(key)) {
        Ok(index) => STRINGS.get(index).and_then(|entry| entry.1.get(lang)).copied().filter(|v| !v.is_empty()).unwrap_or(key),
        Err(_) => key,
    }
}
/// Translates a key and formats positional placeholders without loading runtime JSON catalogs.
#[must_use]
pub fn tr_in(code: Option<&str>, key: &str, args: &[&dyn std::fmt::Display]) -> String {
    format_placeholders(t_in(code.unwrap_or("ru"), key), args)
}

/// Replaces `{N}` and `{N:spec}` in one pass. Inserted text is never scanned again, so an argument
/// that itself looks like a placeholder is printed verbatim. Unknown or out-of-range indices stay literal.
#[must_use]
pub fn format_placeholders(pattern: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut out = String::with_capacity(pattern.len());
    let mut rest = pattern;
    while let Some(open) = rest.find('{') {
        out.push_str(rest.get(..open).unwrap_or(""));
        let tail = rest.get(open..).unwrap_or("");
        match placeholder(tail, args.len()) {
            Some((index, consumed)) => {
                if let Some(arg) = args.get(index) {
                    out.push_str(&arg.to_string());
                }
                rest = tail.get(consumed..).unwrap_or("");
            }
            None => {
                out.push('{');
                rest = tail.get(1..).unwrap_or("");
            }
        }
    }
    out.push_str(rest);
    out
}

/// Parses a placeholder at the start of `tail` (which begins with `{`): returns the argument index and
/// the number of bytes it occupies.
fn placeholder(tail: &str, count: usize) -> Option<(usize, usize)> {
    let inner = tail.get(1..)?;
    let digits = inner.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    let index: usize = inner.get(..digits)?.parse().ok()?;
    if index >= count {
        return None;
    }
    let after = inner.get(digits..)?;
    let close = match after.as_bytes().first() {
        Some(b'}') => 0,
        Some(b':') => after.find('}')?,
        _ => return None,
    };
    Some((index, 1_usize.saturating_add(digits).saturating_add(close).saturating_add(1)))
}
type Row = (&'static str, [&'static str; 15]);
static STRINGS: &[Row] = &[
"""
    footer = """];

#[cfg(test)]
mod tests {
    #![allow(clippy::indexing_slicing)]
    use super::*;
    #[test]
    fn every_key_has_fifteen_non_empty_values() {
        assert!(!STRINGS.is_empty());
        for (key, values) in STRINGS {
            assert!(!key.is_empty());
            assert_eq!(values.len(), 15);
            assert!(values.iter().all(|value| !value.is_empty()), "empty translation for {key}");
        }
    }
    #[test]
    fn russian_is_source() {
        for (key, values) in STRINGS { assert_eq!(values.first().copied(), Some(*key)); }
    }
    #[test]
    fn argument_that_looks_like_a_placeholder_is_printed_verbatim() {
        let file = "{0:x}.sav";
        assert_eq!(tr_in(Some("en"), "Checking {0}", &[&file]), "Checking {0:x}.sav");
        assert_eq!(format_placeholders("{0}", &[&"{0:}"]), "{0:}");
    }

    #[test]
    fn argument_is_not_substituted_into_a_later_argument() {
        let first = "{1}";
        let second = "second";
        assert_eq!(format_placeholders("A {0} B {1}", &[&first, &second]), "A {1} B second");
    }

    #[test]
    fn unknown_or_unterminated_placeholders_stay_literal() {
        assert_eq!(format_placeholders("x {2} y", &[&"a"]), "x {2} y");
        assert_eq!(format_placeholders("open {0:X", &[&"a"]), "open {0:X");
        assert_eq!(format_placeholders("{{}} {0}", &[&"a"]), "{{}} a");
    }

    #[test]
    fn translated_placeholders_are_formatted() {
        assert_eq!(
            tr_in(Some("en"), "Настройки не сохранены: {0}", &[&"bad input"]),
            "Settings not saved: bad input"
        );
        assert_eq!(tr_in(Some("en"), "Unknown {0}", &[&"value"]), "Unknown value");
    }
    #[test]
    fn translated_placeholders_with_format_specifiers_are_formatted() {
        assert_eq!(tr_in(Some("en"), "Value {0:X4}", &[&17]), "Value 17");
    }
    #[test]
    fn shell_translations_match_runtime_catalogs_for_all_languages() {
        let catalogs = sse_catalog::I18nService::new();
        let detail = "settings.json is unchanged: invalid path";
        let detail_args: [&dyn std::fmt::Display; 1] = [&detail];
        let count = 3_usize;
        let count_args: [&dyn std::fmt::Display; 1] = [&count];
        for language in LANGUAGES {
            for key in [
                "Выберите сохранение для редактирования.",
                "Нет несохранённых изменений.",
                "Отправлять анонимные отчёты об ошибках?",
            ] {
                assert_eq!(tr_in(Some(language), key, &[]), catalogs.tr_in(Some(language), key, &[]));
            }
            let warning = "Настройки не сохранены: {0}";
            assert_eq!(
                tr_in(Some(language), warning, &detail_args),
                catalogs.tr_in(Some(language), warning, &detail_args),
                "settings warning in {language}"
            );
            let draft = "Черновик: {0} действ.";
            assert_eq!(
                tr_in(Some(language), draft, &count_args),
                catalogs.tr_in(Some(language), draft, &count_args),
                "draft badge in {language}"
            );
        }
    }
}
"""
    output = pathlib.Path("crates/sse-ui/src/strings.rs")
    output.write_text(header + "\n".join(rows) + "\n" + footer, encoding="utf-8")
    print(f"generated {output} with {len(rows)} keys")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
