#!/usr/bin/env python3
"""Build the Unicode 17.0.0 grapheme and line-break table block using Python's standard library.

The input files are Unicode Character Database data files. Their original copyright and license
notices are retained in tools/ucd/17.0.0; see https://www.unicode.org/terms_of_use.html.
"""

from __future__ import annotations

import argparse
import re
import struct
from pathlib import Path


MAGIC = b"SSEUT17\0"
CODE_POINT_LIMIT = 0x110000

GRAPHEME_CLASSES = {
    "Other": 0,
    "CR": 1,
    "LF": 2,
    "Control": 3,
    "Extend": 4,
    "Regional_Indicator": 5,
    "Prepend": 6,
    "SpacingMark": 7,
    "L": 8,
    "V": 9,
    "T": 10,
    "LV": 11,
    "LVT": 12,
    "ZWJ": 13,
}

LINE_CLASSES = {
    "XX": 0,
    "BK": 1,
    "CR": 2,
    "LF": 3,
    "CM": 4,
    "NL": 5,
    "SG": 6,
    "WJ": 7,
    "ZW": 8,
    "GL": 9,
    "SP": 10,
    "B2": 11,
    "BA": 12,
    "BB": 13,
    "HY": 14,
    "CB": 15,
    "CL": 16,
    "CP": 17,
    "EX": 18,
    "IN": 19,
    "NS": 20,
    "OP": 21,
    "QU": 22,
    "IS": 23,
    "NU": 24,
    "PO": 25,
    "PR": 26,
    "SY": 27,
    "AI": 28,
    "AL": 29,
    "CJ": 30,
    "H2": 31,
    "H3": 32,
    "HL": 33,
    "ID": 34,
    "JL": 35,
    "JV": 36,
    "JT": 37,
    "RI": 38,
    "SA": 39,
    "ZWJ": 40,
    "EB": 41,
    "EM": 42,
    "AK": 43,
    "AP": 44,
    "AS": 45,
    "VF": 46,
    "VI": 47,
    "HH": 48,
}

INCB_CLASSES = {"None": 0, "Extend": 1, "Consonant": 2, "Linker": 3}


def code_point_range(value: str) -> tuple[int, int]:
    match = re.fullmatch(r"([0-9A-Fa-f]{4,6})(?:\.\.([0-9A-Fa-f]{4,6}))?", value.strip())
    if match is None:
        raise ValueError(f"invalid Unicode code point range: {value!r}")
    start = int(match.group(1), 16)
    end = int(match.group(2) or match.group(1), 16)
    if start > end or end >= CODE_POINT_LIMIT:
        raise ValueError(f"Unicode code point range outside 0..10FFFF: {value!r}")
    return start, end


def require_unicode_17(path: Path, lines: list[str]) -> None:
    if not any("17.0" in line for line in lines[:12]):
        raise ValueError(f"{path} is not a Unicode 17.0.0 data file")


def property_records(
    path: Path, selector, classes: dict[str, int], default: str
) -> list[tuple[int, int, int]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    require_unicode_17(path, lines)
    records: list[tuple[int, int, int]] = []
    for line_number, line in enumerate(lines, start=1):
        body = line.split("#", maxsplit=1)[0].strip()
        if not body:
            continue
        fields = [field.strip() for field in body.split(";")]
        selected_class = selector(fields)
        if selected_class is None:
            continue
        if selected_class not in classes:
            raise ValueError(f"{path}:{line_number}: unknown property value {selected_class!r}")
        if selected_class == default:
            continue
        start, end = code_point_range(fields[0])
        records.append((start, end, classes[selected_class]))
    return merge_ranges(records, path)


def merge_ranges(records: list[tuple[int, int, int]], source: Path) -> list[tuple[int, int, int]]:
    records.sort()
    merged: list[tuple[int, int, int]] = []
    for start, end, value in records:
        if merged:
            previous_start, previous_end, previous_value = merged[-1]
            if start <= previous_end:
                raise ValueError(f"{source}: overlapping property ranges at U+{start:04X}")
            if value == previous_value and start == previous_end + 1:
                merged[-1] = (previous_start, end, value)
                continue
        merged.append((start, end, value))
    return merged


def grapheme_records(ucd: Path) -> list[tuple[int, int, int]]:
    return property_records(
        ucd / "auxiliary/GraphemeBreakProperty.txt",
        lambda fields: fields[1] if len(fields) >= 2 else None,
        GRAPHEME_CLASSES,
        "Other",
    )


def line_records(ucd: Path) -> list[tuple[int, int, int]]:
    return property_records(
        ucd / "LineBreak.txt",
        lambda fields: fields[1] if len(fields) >= 2 else None,
        LINE_CLASSES,
        "XX",
    )


def incb_records(ucd: Path) -> list[tuple[int, int, int]]:
    def select(fields: list[str]) -> str | None:
        return fields[2] if len(fields) >= 3 and fields[1] == "InCB" else None

    return property_records(ucd / "DerivedCoreProperties.txt", select, INCB_CLASSES, "None")


def pictographic_records(ucd: Path) -> list[tuple[int, int]]:
    path = ucd / "emoji/emoji-data.txt"
    lines = path.read_text(encoding="utf-8").splitlines()
    require_unicode_17(path, lines)
    records: list[tuple[int, int]] = []
    for line in lines:
        body = line.split("#", maxsplit=1)[0].strip()
        if not body:
            continue
        fields = [field.strip() for field in body.split(";")]
        if len(fields) < 2 or fields[1] != "Extended_Pictographic":
            continue
        records.append(code_point_range(fields[0]))
    records.sort()
    merged: list[tuple[int, int]] = []
    for start, end in records:
        if merged:
            previous_start, previous_end = merged[-1]
            if start <= previous_end:
                raise ValueError(f"{path}: overlapping pictographic ranges at U+{start:04X}")
            if start == previous_end + 1:
                merged[-1] = (previous_start, end)
                continue
        merged.append((start, end))
    return merged


def build_table(ucd: Path) -> tuple[bytes, tuple[int, int, int, int]]:
    grapheme = grapheme_records(ucd)
    line = line_records(ucd)
    pictographic = pictographic_records(ucd)
    incb = incb_records(ucd)
    output = bytearray(
        struct.pack("<8sIIII", MAGIC, len(grapheme), len(line), len(pictographic), len(incb))
    )
    for start, end, value in grapheme:
        output.extend(struct.pack("<IIB", start, end, value))
    for start, end, value in line:
        output.extend(struct.pack("<IIB", start, end, value))
    for start, end in pictographic:
        output.extend(struct.pack("<II", start, end))
    for start, end, value in incb:
        output.extend(struct.pack("<IIB", start, end, value))
    return bytes(output), (len(grapheme), len(line), len(pictographic), len(incb))


def render_rust_metadata(counts: tuple[int, int, int, int]) -> bytes:
    offsets = [24]
    strides = [9, 9, 8, 9]
    for count, stride in zip(counts, strides, strict=True):
        offsets.append(offsets[-1] + count * stride)
    classes = ["Some(8)", "Some(8)", "None", "Some(8)"]
    lines = ["// Generated by tools/generate_unicode_tables.py from Unicode 17.0.0 UCD."]
    lines.append("const TABLE_SECTIONS: [TableSection; 4] = [")
    for offset, count, stride, class_offset in zip(
        offsets[:-1], counts, strides, classes, strict=True
    ):
        lines.extend(
            [
                "    TableSection {",
                f"        offset: {offset},",
                f"        count: {count},",
                f"        stride: {stride},",
                f"        class_offset: {class_offset},",
                "    },",
            ]
        )
    lines.extend(["];", f"const TABLE_END: usize = {offsets[-1]};", ""])
    return "\n".join(lines).encode("utf-8")


def write_atomically(path: Path, contents: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_bytes(contents)
    temporary.replace(path)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ucd", type=Path, default=Path("tools/ucd/17.0.0/ucd"))
    parser.add_argument(
        "--output", type=Path, default=Path("crates/sse-ui/assets/unicode17-tables.bin")
    )
    parser.add_argument(
        "--metadata-output",
        type=Path,
        default=Path("crates/sse-ui/src/unicode_table_meta.rs"),
    )
    args = parser.parse_args()
    try:
        data, counts = build_table(args.ucd)
        metadata = render_rust_metadata(counts)
        write_atomically(args.output, data)
        write_atomically(args.metadata_output, metadata)
    except (OSError, ValueError, struct.error) as error:
        parser.error(str(error))
    print(f"wrote {len(data)} bytes to {args.output} and metadata to {args.metadata_output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
