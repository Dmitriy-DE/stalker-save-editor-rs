#!/usr/bin/env python3
"""Ratchet for untranslated interface rows in crates/sse-ui/src/strings.rs.

The generated table falls back to English when a language has no translation, and a few rows keep the
Russian source in a non-Russian column. Both show up on screen as English or Russian text in another
interface language. This check counts those rows per language and fails when a count grows beyond
tools/i18n-fallback-baseline.json. Lowering a count means updating the baseline in the same change.

Usage: tools/check_i18n_fallback.py [--update]
"""
from __future__ import annotations

import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
TABLE = ROOT / "crates/sse-ui/src/strings.rs"
BASELINE = ROOT / "tools/i18n-fallback-baseline.json"
ROW = re.compile(r'^\s*\("((?:[^"\\]|\\.)*)", \[(.*?)\]\),\s*$', re.M)
VALUE = re.compile(r'"((?:[^"\\]|\\.)*)"')
LANGS = ["ru", "uk", "en", "de", "fr", "it", "es", "pl", "cs", "pt-BR", "tr", "ja", "ko", "zh-CN", "zh-TW"]


def has_cyrillic(text: str) -> bool:
    return any("а" <= ch.lower() <= "я" or ch in "ёЁіїєґ" for ch in text)


def has_latin(text: str) -> bool:
    return any(ch.isascii() and ch.isalpha() for ch in text)


def counts() -> dict[str, dict[str, int]]:
    rows = ROW.findall(TABLE.read_text(encoding="utf-8"))
    result = {lang: {"english": 0, "russian": 0} for lang in LANGS if lang not in ("ru", "en")}
    for _, raw in rows:
        values = VALUE.findall(raw)
        if len(values) != len(LANGS):
            raise SystemExit(f"{TABLE}: a row has {len(values)} values, expected {len(LANGS)}")
        ru, en = values[0], values[2]
        if not has_cyrillic(ru):
            continue
        for index, lang in enumerate(LANGS):
            if lang in ("ru", "en"):
                continue
            if values[index] == en and has_latin(en):
                result[lang]["english"] += 1
            elif values[index] == ru:
                result[lang]["russian"] += 1
    return result


def main() -> int:
    current = counts()
    if "--update" in sys.argv:
        BASELINE.write_text(json.dumps(current, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"baseline written: {BASELINE}")
        return 0
    baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
    failures = []
    for lang, kinds in current.items():
        for kind, count in kinds.items():
            allowed = baseline.get(lang, {}).get(kind, 0)
            if count > allowed:
                failures.append(f"{lang} {kind}: {count} > baseline {allowed}")
    if failures:
        print("untranslated rows grew:")
        for line in failures:
            print("  " + line)
        return 1
    print("i18n fallback ratchet passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
