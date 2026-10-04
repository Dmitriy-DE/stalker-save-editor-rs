#!/usr/bin/env python3
"""Generate crates/sse-ui/src/strings.rs from C# 1.3.1 i18n/*.json."""
from __future__ import annotations
import json
import pathlib
import sys

LANGS = ["ru","uk","en","de","fr","it","es","pl","cs","pt-BR","tr","ja","ko","zh-CN","zh-TW"]

def q(value: str) -> str:
    return json.dumps(value, ensure_ascii=False)

def main() -> int:
    if len(sys.argv) != 2:
        print("usage: tools/generate_strings.py <directory-containing-i18n>", file=sys.stderr)
        return 2
    root = pathlib.Path(sys.argv[1])
    source = root / "i18n" if (root / "i18n").is_dir() else root
    catalogs = {}
    keys = set()
    for lang in LANGS:
        path = source / f"{lang}.json"
        with path.open("r", encoding="utf-8") as handle:
            catalog = json.load(handle)
        if not isinstance(catalog, dict):
            raise ValueError(f"{path}: expected object")
        catalogs[lang] = catalog
        keys.update(catalog)
    rows = []
    for key in sorted(keys):
        values = []
        for lang in LANGS:
            value = key if lang == "ru" else catalogs[lang].get(key)
            if not isinstance(value, str) or not value:
                value = catalogs["en"].get(key) if lang != "en" else None
            if not isinstance(value, str) or not value:
                value = key
            values.append(value)
        rows.append(f"    ({q(key)}, [" + ", ".join(q(v) for v in values) + "]),")
    header = """//! Generated from C# 1.3.1 localization. Do not edit by hand.
use std::sync::atomic::{AtomicU8, Ordering};
pub const LANGUAGES: [&str; 15] = [\"ru\",\"uk\",\"en\",\"de\",\"fr\",\"it\",\"es\",\"pl\",\"cs\",\"pt-BR\",\"tr\",\"ja\",\"ko\",\"zh-CN\",\"zh-TW\"];
static CURRENT_LANGUAGE: AtomicU8 = AtomicU8::new(0);
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
pub fn set_language(code: Option<&str>) {
    let index = code.map_or(0, language_index);
    CURRENT_LANGUAGE.store(u8::try_from(index).unwrap_or(0), Ordering::Relaxed);
}
#[must_use]
pub fn current_language() -> &'static str {
    LANGUAGES.get(usize::from(CURRENT_LANGUAGE.load(Ordering::Relaxed))).copied().unwrap_or("ru")
}
#[must_use]
pub fn t(key: &str) -> &str { t_in(current_language(), key) }
#[must_use]
pub fn t_in<'a>(language: &str, key: &'a str) -> &'a str {
    let lang = language_index(language);
    match STRINGS.binary_search_by(|entry| entry.0.cmp(key)) {
        Ok(index) => STRINGS.get(index).and_then(|entry| entry.1.get(lang)).copied().filter(|v| !v.is_empty()).unwrap_or(key),
        Err(_) => key,
    }
}
type Row = (&'static str, [&'static str; 15]);
static STRINGS: &[Row] = &[
"""
    footer = """];

#[cfg(test)]
mod tests {
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
}
"""
    output = pathlib.Path("crates/sse-ui/src/strings.rs")
    output.write_text(header + "\n".join(rows) + "\n" + footer, encoding="utf-8")
    print(f"generated {output} with {len(rows)} keys")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
