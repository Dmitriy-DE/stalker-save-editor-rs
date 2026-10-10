#!/usr/bin/env python3
"""Import translations from the C# 1.3.1 locales for rows that show English or Russian fallback text.

For every row of crates/sse-ui/src/strings.rs and every non-Russian, non-English language whose value is a
fallback (the English text, or the Russian source for Cyrillic keys), the C# locale is consulted:

- exact match: the C# value is used when its placeholders ({0}, {1}, ...) are the same set as the key's;
- normalized match (case, whitespace, trailing punctuation): not applied; written to a review list instead.

Accepted values go into tools/i18n-extra.json as partial entries (only that language). The generator
falls back to the table for every other language, so nothing is removed.

Usage:
  tools/import_csharp_i18n.py <path-to-C#-i18n-dir> [--apply] [--review FILE]
Without --apply the script only reports counts.
"""
from __future__ import annotations

import argparse
import collections
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
TABLE = ROOT / "crates/sse-ui/src/strings.rs"
EXTRA = ROOT / "tools/i18n-extra.json"
ROW = re.compile(r'^\s*\("((?:[^"\\]|\\.)*)", \[(.*?)\]\),\s*$', re.M)
VALUE = re.compile(r'"((?:[^"\\]|\\.)*)"')
PLACEHOLDER = re.compile(r"\{(\d+)\}")
LANGS = ["ru", "uk", "en", "de", "fr", "it", "es", "pl", "cs", "pt-BR", "tr", "ja", "ko", "zh-CN", "zh-TW"]
FILL_LANGS = ["uk", "de", "fr", "es", "it", "pl", "cs"]
CSHARP_FILE = {"zh-CN": "zh-CN", "zh-TW": "zh-TW", "pt-BR": "pt-BR"}


def unescape(raw: str) -> str:
    return raw.replace('\\"', '"').replace("\\\\", "\\").replace("\\n", "\n")


def cyrillic(text: str) -> bool:
    return any("а" <= ch.lower() <= "я" or ch in "ёЁіїєґ" for ch in text)


def latin(text: str) -> bool:
    return any(ch.isascii() and ch.isalpha() for ch in text)


def placeholders(text: str) -> list[str]:
    return sorted(set(PLACEHOLDER.findall(text)))


def normalize(text: str) -> str:
    return re.sub(r"[\s.!?…:;]+$", "", re.sub(r"\s+", " ", text.strip())).lower()


def load_table() -> list[tuple[str, dict[str, str]]]:
    rows = []
    for key_raw, values_raw in ROW.findall(TABLE.read_text(encoding="utf-8")):
        values = [unescape(v) for v in VALUE.findall(values_raw)]
        if len(values) != len(LANGS):
            raise SystemExit(f"{TABLE}: row {key_raw[:40]!r} has {len(values)} values")
        rows.append((unescape(key_raw), dict(zip(LANGS, values))))
    return rows


def load_csharp(directory: pathlib.Path, lang: str) -> dict[str, str]:
    path = directory / f"{CSHARP_FILE.get(lang, lang)}.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    return {k: v for k, v in data.items() if isinstance(v, str) and v}


def untrustworthy(key: str, lang: str, value: str, english: str | None = None) -> bool:
    """A C# value that is still the Russian source, or Russian text in a non-Russian language."""
    if value == key:
        return True
    if lang not in ("uk",) and cyrillic(value):
        return True
    return english is not None and value == english and latin(value)


def is_fallback(key: str, lang: str, values: dict[str, str]) -> bool:
    ru, en, current = key, values["en"], values[lang]
    if cyrillic(ru) and latin(en) and current == en:
        return True
    return cyrillic(ru) and current == ru


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("csharp_dir", type=pathlib.Path)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--review", type=pathlib.Path)
    args = parser.parse_args()

    rows = load_table()
    csharp = {lang: load_csharp(args.csharp_dir, lang) for lang in FILL_LANGS}
    accepted: dict[str, dict[str, str]] = collections.defaultdict(dict)
    review: list[dict[str, str]] = []
    counts = collections.Counter()
    fallback_total = collections.Counter()

    for key, values in rows:
        for lang in FILL_LANGS:
            if not is_fallback(key, lang, values):
                continue
            fallback_total[lang] += 1
            exact = csharp[lang].get(key)
            if exact is not None:
                if placeholders(exact) != placeholders(key):
                    counts[(lang, "placeholder-mismatch")] += 1
                    continue
                if untrustworthy(key, lang, exact, values["en"]):
                    counts[(lang, "untrustworthy")] += 1
                    continue
                accepted[key][lang] = exact
                counts[(lang, "exact")] += 1
                continue
            normalized_key = normalize(key)
            for source_key, source_value in csharp[lang].items():
                if normalize(source_key) == normalized_key and placeholders(source_value) == placeholders(key):
                    review.append({"lang": lang, "key": key, "csharp_key": source_key, "value": source_value})
                    counts[(lang, "review")] += 1
                    break
            else:
                counts[(lang, "missing")] += 1

    print("fallback rows per language:", dict(fallback_total))
    for lang in FILL_LANGS:
        got = counts[(lang, "exact")]
        print(
            f"{lang}: exact={got} review={counts[(lang, 'review')]} "
            f"placeholder-mismatch={counts[(lang, 'placeholder-mismatch')]} "
            f"untrustworthy={counts[(lang, 'untrustworthy')]} "
            f"left={fallback_total[lang] - got}"
        )

    if args.review:
        args.review.write_text(json.dumps(review, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(f"review list: {args.review} ({len(review)} entries)")

    if not args.apply:
        print("dry run: nothing written (use --apply)")
        return 0

    extras = json.loads(EXTRA.read_text(encoding="utf-8"), object_pairs_hook=collections.OrderedDict)
    new_keys = [k for k in accepted if k not in extras]
    for key, per_lang in accepted.items():
        entry = extras.setdefault(key, collections.OrderedDict())
        for lang, value in per_lang.items():
            entry[lang] = value
    # Keep new partial entries mid-file, as the owner asked for new translation rows.
    items = list(extras.items())
    existing = [(k, v) for k, v in items if k not in set(new_keys)]
    added = [(k, extras[k]) for k in new_keys]
    mid = len(existing) // 2
    ordered = collections.OrderedDict(existing[:mid] + added + existing[mid:])
    EXTRA.write_text(json.dumps(ordered, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"applied {sum(len(v) for v in accepted.values())} values to {EXTRA}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
