#!/usr/bin/env python3
"""Compare Rust UI screenshots with the pinned C# reference renderer.

The comparison ignores antialiased edge pixels (font/rasterizer implementation detail)
and fails when more than 0.5% of the remaining pixels differ by >8 in any RGB channel.
"""

from __future__ import annotations

import argparse
import struct
import sys
import zlib
from pathlib import Path

PNG = b"\x89PNG\r\n\x1a\n"


def paeth(a: int, b: int, c: int) -> int:
    p = a + b - c
    pa = abs(p - a)
    pb = abs(p - b)
    pc = abs(p - c)
    if pa <= pb and pa <= pc:
        return a
    if pb <= pc:
        return b
    return c


def read_png(path: Path) -> tuple[int, int, bytearray]:
    data = path.read_bytes()
    if not data.startswith(PNG):
        raise ValueError(f"{path}: not a PNG")
    pos = len(PNG)
    width = height = bit_depth = color_type = None
    payload = bytearray()
    while pos + 12 <= len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            width, height, bit_depth, color_type, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", body
            )
            if bit_depth != 8 or compression != 0 or filtering != 0 or interlace != 0:
                raise ValueError(f"{path}: unsupported PNG encoding")
        elif kind == b"IDAT":
            payload.extend(body)
        elif kind == b"IEND":
            break
    if width is None or height is None or color_type not in (2, 6):
        raise ValueError(f"{path}: expected 8-bit RGB/RGBA PNG")
    channels = 3 if color_type == 2 else 4
    row_bytes = width * channels
    raw = zlib.decompress(bytes(payload))
    expected = height * (row_bytes + 1)
    if len(raw) != expected:
        raise ValueError(f"{path}: unexpected decompressed size {len(raw)} != {expected}")
    decoded = bytearray(height * row_bytes)
    previous = bytearray(row_bytes)
    src = 0
    for y in range(height):
        filter_type = raw[src]
        src += 1
        row = bytearray(raw[src : src + row_bytes])
        src += row_bytes
        for x in range(row_bytes):
            left = row[x - channels] if x >= channels else 0
            up = previous[x]
            upper_left = previous[x - channels] if x >= channels else 0
            if filter_type == 1:
                row[x] = (row[x] + left) & 0xFF
            elif filter_type == 2:
                row[x] = (row[x] + up) & 0xFF
            elif filter_type == 3:
                row[x] = (row[x] + ((left + up) // 2)) & 0xFF
            elif filter_type == 4:
                row[x] = (row[x] + paeth(left, up, upper_left)) & 0xFF
            elif filter_type != 0:
                raise ValueError(f"{path}: unsupported PNG filter {filter_type}")
        start = y * row_bytes
        decoded[start : start + row_bytes] = row
        previous = row
    rgb = bytearray(width * height * 3)
    for pixel in range(width * height):
        source = pixel * channels
        target = pixel * 3
        rgb[target : target + 3] = decoded[source : source + 3]
    return width, height, rgb


def is_edge(rgb: bytearray, width: int, height: int, x: int, y: int, tolerance: int) -> bool:
    here = (y * width + x) * 3
    for nx, ny in ((x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)):
        if nx < 0 or ny < 0 or nx >= width or ny >= height:
            continue
        other = (ny * width + nx) * 3
        if max(abs(rgb[here + c] - rgb[other + c]) for c in range(3)) > tolerance:
            return True
    return False


def compare(reference: Path, actual: Path, tolerance: int, max_ratio: float) -> bool:
    rw, rh, ref = read_png(reference)
    aw, ah, got = read_png(actual)
    if (rw, rh) != (aw, ah):
        print(f"{actual.name}: size mismatch {(aw, ah)} != {(rw, rh)}", file=sys.stderr)
        return False
    compared = 0
    different = 0
    for y in range(rh):
        for x in range(rw):
            # C# uses Skia while Rust uses the own rasterizer. Ignore antialiased edges,
            # but compare the flat interiors around them so geometry/color shifts still fail.
            if is_edge(ref, rw, rh, x, y, tolerance) or is_edge(got, rw, rh, x, y, tolerance):
                continue
            offset = (y * rw + x) * 3
            compared += 1
            if max(abs(ref[offset + c] - got[offset + c]) for c in range(3)) > tolerance:
                different += 1
    ratio = different / max(compared, 1)
    print(
        f"{actual.name}: {different}/{compared} unmasked pixels differ "
        f"({ratio:.4%}), limit {max_ratio:.4%}"
    )
    return ratio <= max_ratio


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--reference-dir", type=Path, required=True)
    parser.add_argument("--actual-dir", type=Path, required=True)
    parser.add_argument("--tolerance", type=int, default=8)
    parser.add_argument("--max-ratio", type=float, default=0.005)
    parser.add_argument("names", nargs="+")
    args = parser.parse_args()
    ok = True
    for name in args.names:
        ok = compare(
            args.reference_dir / f"{name}.png",
            args.actual_dir / f"{name}.png",
            args.tolerance,
            args.max_ratio,
        ) and ok
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
