#!/usr/bin/env python3
"""Write the item icon pack file that the browser edition fetches after it starts.

The desktop builds compile the pictures in; the browser build leaves them out of the WebAssembly file and reads
this pack instead. Layout (read by `sse_ui::item_icons::install_pack`): the magic `SSEICON1`, a little-endian
u32 count, then per picture in path order: u16 path length, the path (`xray/<key>.png`, `s2/<key>.png`), u32 byte
length, the PNG bytes. The output depends only on the pictures, so it is the same on every run.
"""

from __future__ import annotations

import struct
import sys
from pathlib import Path

MAGIC = b"SSEICON1"
MAX_PACK_BYTES = 64 * 1024 * 1024
MAX_PACK_ENTRIES = 65_536
FOLDERS = ("s2", "xray")


def pack(source: Path) -> tuple[bytes, int]:
    entries: list[tuple[bytes, bytes]] = []
    for folder in FOLDERS:
        for picture in (source / folder).glob("*.png"):
            entries.append((f"{folder}/{picture.name}".encode("utf-8"), picture.read_bytes()))
    if not entries:
        raise SystemExit(f"no pictures under {source}")
    if len(entries) > MAX_PACK_ENTRIES:
        raise SystemExit("too many pictures for one pack")
    entries.sort(key=lambda entry: entry[0])
    out = bytearray(MAGIC)
    out += struct.pack("<I", len(entries))
    for path, data in entries:
        out += struct.pack("<H", len(path)) + path + struct.pack("<I", len(data)) + data
    if len(out) > MAX_PACK_BYTES:
        raise SystemExit("the pack is larger than the editor accepts")
    return bytes(out), len(entries)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} <item-icons directory> <output file>", file=sys.stderr)
        return 2
    data, count = pack(Path(sys.argv[1]))
    Path(sys.argv[2]).write_bytes(data)
    print(f"Item icons: {count} pictures, {len(data)} bytes ({sys.argv[2]})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
