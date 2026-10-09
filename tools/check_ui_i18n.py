#!/usr/bin/env python3
"""Fail when a screen source string with Cyrillic bypasses localization."""

from __future__ import annotations

import re
import sys
import unittest
from pathlib import Path

TRANSLATION_CALLS = {
    "t",
    "t_in",
    "tr",
    "tr_in",
    "t_args",
    "tr_args",
}
TRANSLATED_HELPERS = {
    "label",
    "button",
    "paragraph",
    "set_text",
    "set_status",
    "report_text",
    "compact_library_button",
}
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
CYRILLIC = re.compile(r"[\u0400-\u052f]")

# These are stable filter/category tokens. Display code passes them through t().
INTERNAL_CATEGORY_LITERALS = {
    "ВСЕ",
    "ОРУЖИЕ",
    "БОЕПРИПАСЫ",
    "СНАРЯЖЕНИЕ",
    "РАСХОДНИКИ",
    "АРТЕФАКТЫ",
    "КЛЮЧИ",
    "ПРОЧЕЕ",
    "ключ",
    "оруж",
    "патрон",
    "брон",
    "экип",
    "устрой",
    "расход",
    "гранат",
    "артефакт",
    "Рюкзак",
    "Пояс",
}


def _skip_line_comment(source: str, index: int) -> int:
    newline = source.find("\n", index + 2)
    return len(source) if newline < 0 else newline + 1


def _skip_block_comment(source: str, index: int) -> int:
    depth = 1
    index += 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    return index


def _skip_trivia(source: str, index: int) -> int:
    while index < len(source):
        if source[index].isspace():
            index += 1
        elif source.startswith("//", index):
            index = _skip_line_comment(source, index)
        elif source.startswith("/*", index):
            index = _skip_block_comment(source, index)
        else:
            break
    return index


def _raw_string_start(source: str, index: int) -> tuple[int, str] | None:
    if source.startswith("br", index):
        index += 2
    elif source.startswith("r", index):
        index += 1
    else:
        return None
    hashes = 0
    while index < len(source) and source[index] == "#":
        hashes += 1
        index += 1
    if index >= len(source) or source[index] != '"':
        return None
    return index + 1, '"' + ("#" * hashes)


def _read_string(source: str, index: int) -> tuple[int, str] | None:
    raw = _raw_string_start(source, index)
    if raw is not None:
        content_start, terminator = raw
        content_end = source.find(terminator, content_start)
        if content_end < 0:
            return len(source), source[content_start:]
        return content_end + len(terminator), source[content_start:content_end]
    if source[index] != '"':
        return None
    content_start = index + 1
    cursor = content_start
    while cursor < len(source):
        if source[cursor] == "\\":
            cursor += 2
        elif source[cursor] == '"':
            return cursor + 1, source[content_start:cursor]
        else:
            cursor += 1
    return len(source), source[content_start:]


def _contains_cyrillic(content: str) -> bool:
    if CYRILLIC.search(content):
        return True
    index = 0
    while index < len(content):
        if content.startswith(r"\u{", index):
            end = content.find("}", index + 3)
            if end >= 0:
                try:
                    if CYRILLIC.match(chr(int(content[index + 3 : end], 16))):
                        return True
                except (ValueError, OverflowError):
                    pass
                index = end + 1
                continue
        index += 2 if content[index] == "\\" else 1
    return False


def find_untranslated_cyrillic(source: str) -> list[tuple[int, str]]:
    """Return Cyrillic string literals outside translated UI builders."""
    findings: list[tuple[int, str]] = []
    call_stack: list[str | None] = []
    pending_call: str | None = None
    index = 0
    while index < len(source):
        if source.startswith("//", index):
            index = _skip_line_comment(source, index)
            continue
        if source.startswith("/*", index):
            index = _skip_block_comment(source, index)
            continue

        string = _read_string(source, index)
        if string is not None:
            end, literal = string
            if (
                _contains_cyrillic(literal)
                and literal not in INTERNAL_CATEGORY_LITERALS
                and not any(name in TRANSLATION_CALLS or name in TRANSLATED_HELPERS for name in call_stack)
            ):
                findings.append((source.count("\n", 0, index) + 1, literal))
            index = end
            pending_call = None
            continue

        identifier = IDENTIFIER.match(source, index)
        if identifier is not None:
            name = identifier.group()
            cursor = _skip_trivia(source, identifier.end())
            if cursor < len(source) and source[cursor] == "!":
                cursor = _skip_trivia(source, cursor + 1)
            is_call = cursor < len(source) and source[cursor] == "("
            prefix = source[max(0, index - 128) : index]
            if name in {"set_text", "set_status"}:
                translated_helper = bool(re.search(r"\bself\s*\.\s*$", prefix))
            elif name in {"label", "button"}:
                translated_helper = bool(re.search(r"\bstyle\s*::\s*$", prefix))
            elif name in {"paragraph", "report_text", "compact_library_button"}:
                translated_helper = True
            else:
                translated_helper = name in TRANSLATION_CALLS
            pending_call = name if is_call and translated_helper else None
            index = identifier.end()
            continue

        char = source[index]
        if char == "(":
            call_stack.append(pending_call)
            pending_call = None
        elif char == ")":
            if call_stack:
                call_stack.pop()
            pending_call = None
        elif not char.isspace() and char not in "!&*::":
            pending_call = None
        index += 1
    return findings


def _production_source(source: str) -> str:
    """Drop the trailing test module without truncating test-only fields/helpers."""
    test_module = re.search(r"(?m)^[ \t]*#\[cfg\(test\)\][ \t]*\n[ \t]*mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{", source)
    return source if test_module is None else source[: test_module.start()]


def scan_sources(root: Path) -> list[tuple[Path, int, str]]:
    findings = []
    for path in sorted((root / "crates/sse-ui/src/screens").glob("*.rs")):
        source = _production_source(path.read_text(encoding="utf-8"))
        findings.extend((path, line, literal) for line, literal in find_untranslated_cyrillic(source))
    return findings


class SourceCheckTests(unittest.TestCase):
    def test_finds_unwrapped_text(self) -> None:
        self.assertEqual(find_untranslated_cyrillic('tree.set_text(id, "Ошибка")'), [(1, "Ошибка")])

    def test_accepts_translation_calls_and_shared_builders(self) -> None:
        source = 't("Ошибка"); style::label(tree, parent, "Папка", Text::Body); self.set_text(cx, "Готово");'
        self.assertEqual(find_untranslated_cyrillic(source), [])

    def test_ignores_comments_and_internal_category_tokens(self) -> None:
        source = '// "Ошибка"\nconst CATEGORY: &str = "ОРУЖИЕ";'
        self.assertEqual(find_untranslated_cyrillic(source), [])


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        findings = scan_sources(root)
    except OSError as error:
        print(f"i18n source check: {error}", file=sys.stderr)
        return 1
    if findings:
        for path, line, literal in findings:
            print(f"{path}:{line}: untranslated Cyrillic string literal: {literal!r}")
        return 1
    print("i18n source check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
