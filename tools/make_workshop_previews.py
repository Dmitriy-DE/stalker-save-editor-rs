#!/usr/bin/env python3
"""Draw the Steam Workshop preview pictures of the three Enhanced Edition companion packages.

Everything comes from this repository: the application icon (`packaging/icons/stalker-save-editor.svg`, redrawn
here from its own shapes), the interface palette (`crates/sse-ui/src/theme.rs`, the `d2` tokens) and the bundled
fonts. No GSC logo or artwork is used. The output is the same on every run:

    python3 tools/make_workshop_previews.py

writes `crates/sse-companion/assets/workshop/preview-{soc,cs,cop}.{png,jpg}` in English and
`preview-{soc,cs,cop}-{ru,uk}.{png,jpg}` in Russian and Ukrainian (1024×1024, well under 1 MB).
Needs Pillow.
"""

from __future__ import annotations

import re
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
ICON = ROOT / "packaging/icons/stalker-save-editor.svg"
FONTS = ROOT / "crates/sse-ui/assets/fonts"
OUTPUT = ROOT / "crates/sse-companion/assets/workshop"

SIZE = 1024
SCALE = 2  # drawn twice as large and scaled down, for smooth edges

# crates/sse-ui/src/theme.rs, `d2`
BG_BASE = (0x0C, 0x0D, 0x0A)
PANEL_RAISED = (0x15, 0x18, 0x14)
BORDER = (0x33, 0x38, 0x2F)
BORDER_METAL = (0x3D, 0x48, 0x37)
TEXT_PRIMARY = (0xD8, 0xD2, 0xBE)
TEXT_SECONDARY = (0xA2, 0x9D, 0x90)
TEXT_MUTED = (0x71, 0x6F, 0x67)
ACCENT = (0xD6, 0xA6, 0x2D)

GAMES = {
    "soc": {"en": "SHADOW OF CHORNOBYL", "ru": "ТЕНЬ ЧЕРНОБЫЛЯ", "uk": "ТІНЬ ЧОРНОБИЛЯ"},
    "cs": {"en": "CLEAR SKY", "ru": "ЧИСТОЕ НЕБО", "uk": "ЧИСТЕ НЕБО"},
    "cop": {"en": "CALL OF PRYPIAT", "ru": "ЗОВ ПРИПЯТИ", "uk": "ПОКЛИК ПРИП'ЯТІ"},
}

# The product name stays as it is; the second line and the footer follow the language of the page.
WORDS = {
    "en": ("COMPANION", "In-game menu: Esc, then F1  ·  unofficial fan mod"),
    "ru": ("КОМПАНЬОН", "Меню в игре: Esc, затем F1  ·  неофициальный мод"),
    "uk": ("КОМПАНЬЙОН", "Меню у грі: Esc, потім F1  ·  неофіційний мод"),
}


def icon_star() -> list[tuple[float, float]]:
    """The star of the application icon, in its 256×256 view box."""
    path = re.search(r'<path d="M([^"]+)z"', ICON.read_text(encoding="utf-8"))
    if path is None:
        raise SystemExit("the icon has no star path")
    start, steps = path.group(1).split("l", 1)
    numbers = [float(value) for value in re.findall(r"-?\d+(?:\.\d+)?", steps)]
    x, y = (float(value) for value in start.split())
    points = [(x, y)]
    for dx, dy in zip(numbers[0::2], numbers[1::2]):
        x, y = x + dx, y + dy
        points.append((x, y))
    return points


def draw_icon(draw: ImageDraw.ImageDraw, left: float, top: float, side: float) -> None:
    """The application icon: tile, ring, star and hub, as in the SVG."""
    unit = side / 256

    def at(x: float, y: float) -> tuple[float, float]:
        return left + x * unit, top + y * unit

    draw.rounded_rectangle([at(0, 0), at(256, 256)], radius=36 * unit, fill=PANEL_RAISED, outline=BORDER_METAL,
                           width=round(2 * unit))
    draw.ellipse([at(128 - 78, 128 - 78), at(128 + 78, 128 + 78)], outline=ACCENT, width=round(12 * unit))
    draw.polygon([at(x, y) for x, y in icon_star()], fill=ACCENT)
    draw.ellipse([at(128 - 24, 128 - 24), at(128 + 24, 128 + 24)], fill=PANEL_RAISED)


def centred(draw: ImageDraw.ImageDraw, y: float, text: str, font: ImageFont.FreeTypeFont, fill, spacing: float = 0):
    width = sum(draw.textlength(char, font=font) for char in text) + spacing * (len(text) - 1)
    x = (SIZE * SCALE - width) / 2
    for char in text:
        draw.text((x, y), char, font=font, fill=fill)
        x += draw.textlength(char, font=font) + spacing


def preview(game_title: str, language: str) -> Image.Image:
    second_line, footer = WORDS[language]
    s = SCALE
    image = Image.new("RGB", (SIZE * s, SIZE * s), BG_BASE)
    draw = ImageDraw.Draw(image)
    draw.rectangle([24 * s, 24 * s, (SIZE - 24) * s, (SIZE - 24) * s], outline=BORDER, width=2 * s)
    draw.rectangle([24 * s, 24 * s, (SIZE - 24) * s, 32 * s], fill=ACCENT)

    def oswald(size: int, weight: int) -> ImageFont.FreeTypeFont:
        font = ImageFont.truetype(str(FONTS / "Oswald-wght.ttf"), size * s)
        font.set_variation_by_axes([weight])
        return font

    narrow = ImageFont.truetype(str(FONTS / "LiberationSansNarrow-Regular.ttf"), 34 * s)

    draw_icon(draw, (SIZE - 360) / 2 * s, 96 * s, 360 * s)
    centred(draw, 500 * s, "SAVE EDITOR", oswald(104, 700), TEXT_PRIMARY, spacing=6 * s)
    centred(draw, 632 * s, second_line, oswald(64, 500), ACCENT, spacing=22 * s)
    draw.line([(SIZE / 2 - 150) * s, 748 * s, (SIZE / 2 + 150) * s, 748 * s], fill=BORDER_METAL, width=2 * s)
    centred(draw, 772 * s, game_title, oswald(52, 600), TEXT_PRIMARY, spacing=4 * s)
    centred(draw, 846 * s, "ENHANCED EDITION", oswald(30, 400), TEXT_SECONDARY, spacing=8 * s)
    centred(draw, 924 * s, footer, narrow, TEXT_MUTED)
    return image.resize((SIZE, SIZE), Image.LANCZOS)


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for game, titles in GAMES.items():
        for language, title in titles.items():
            suffix = "" if language == "en" else f"-{language}"
            image = preview(title, language)
            image.save(OUTPUT / f"preview-{game}{suffix}.png", optimize=True)
            image.save(OUTPUT / f"preview-{game}{suffix}.jpg", quality=90, optimize=True, subsampling=0)
            for extension in ("png", "jpg"):
                path = OUTPUT / f"preview-{game}{suffix}.{extension}"
                print(f"{path.relative_to(ROOT)}: {path.stat().st_size} bytes")


if __name__ == "__main__":
    main()
