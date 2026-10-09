#!/usr/bin/env python3
"""Find Cyrillic Rust string literals that bypass the UI translation helpers."""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

TRANSLATION_CALLS = {"t", "t_in", "tr", "tr_in", "t_args", "tr_args"}
DEFAULT_FILES = ("crates/sse-ui/src/screens/wizard.rs",)
IDENTIFIER = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
CYRILLIC = re.compile(r"[\u0400-\u052f]")


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
        if content[index] == "\\":
            index += 2
        else:
            index += 1
    return False


def find_untranslated_cyrillic(source: str) -> list[tuple[int, str]]:
    """Return (line, literal) pairs outside t()/tr() translation calls."""
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
            if _contains_cyrillic(literal) and not any(name in TRANSLATION_CALLS for name in call_stack):
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
            pending_call = name if cursor < len(source) and source[cursor] == "(" else None
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="*", help="Rust source files to check")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    files = [Path(value) if Path(value).is_absolute() else root / value for value in (args.files or DEFAULT_FILES)]
    failed = False
    for path in files:
        try:
            source = path.read_text(encoding="utf-8")
        except OSError as error:
            print(f"{path}: {error}", file=sys.stderr)
            failed = True
            continue
        for line, literal in find_untranslated_cyrillic(source):
            print(f"{path}:{line}: untranslated Cyrillic string literal: {literal!r}")
            failed = True
    if not failed:
        print(f"i18n source check passed ({len(files)} files)")
        return 0
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
