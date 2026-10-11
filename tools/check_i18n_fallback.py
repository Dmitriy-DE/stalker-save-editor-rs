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

# Rows that stay equal to English on purpose: proper names of game places, factions, weapons and
# products, unit symbols, and words that are the same in the target language (OK, PM, PDA, CONS.,
# Action, Description ...). Each entry is (language, English text). Keep this list short and reviewed.
EXEMPT = {
    "fr": [
        "9×18 mm",
        "9×19 mm",
        "9×39 mm",
        "ACTION",
        "ACTIONS",
        "ART.",
        "Action",
        "Agroprom",
        "Bandage",
        "Bandits",
        "Bar",
        "CLOUD",
        "CS",
        "CS EE",
        "ChNPP",
        "Clear Sky",
        "Clear Sky (Enhanced Edition)",
        "Clear Sky EE",
        "CoP",
        "CoP EE",
        "Codecs",
        "Cordon",
        "DATE",
        "Date",
        "Description",
        "Document",
        "Duga",
        "FACTIONS",
        "Faction",
        "Factions",
        "Grenade",
        "INTERFACE",
        "Installation",
        "Journal",
        "Jupiter",
        "LOCAL",
        "Limansk",
        "Local",
        "Malachite",
        "Message",
        "Mode",
        "Module",
        "OK",
        "PARENT",
        "PDA",
        "PM",
        "POINTS",
        "Position",
        "RELATIONS",
        "Relation",
        "Rostok",
        "S.T.A.L.K.E.R. Clear Sky",
        "SA Avalanche",
        "SIRCAA",
        "SOURCE",
        "Saiga D-12",
        "SoC",
        "SoC EE",
        "Source",
        "TRANSITION",
        "TRANSITIONS",
        "TYPE",
        "Type",
        "VERSION",
        "Vodka",
        "Volume",
        "Yaniv",
        "Yantar",
        "ZONE",
        "Zaton",
        "Zone",
        "cp.",
        "stalkers {0} · mutants {1}",
        "{0:.1f} kg",
        "{0:F0} ms",
        "{0} ms",
        "{0} · {1:0.#} h",
        "{0} · {1} h",
        "— kg",
    ],
    "es": [
        "9×18 mm",
        "9×19 mm",
        "9×39 mm",
        "ART.",
        "Agroprom",
        "B",
        "Bar",
        "CONS.",
        "CS",
        "CS EE",
        "ChNPP",
        "Clear Sky",
        "Clear Sky (Enhanced Edition)",
        "Clear Sky EE",
        "CoP",
        "CoP EE",
        "DETECTOR",
        "Detector",
        "Duga",
        "EXPERIMENTAL",
        "Experimental",
        "GB",
        "Game Doctor",
        "Hospital",
        "KB",
        "LOCAL",
        "Limansk",
        "Local",
        "MB",
        "Neutral",
        "Neutral (0)",
        "No",
        "PDA",
        "PM",
        "Rostok",
        "S.T.A.L.K.E.R. Clear Sky",
        "S.T.A.L.K.E.R. Save Editor — web",
        "S.T.A.L.K.E.R.: Clear Sky",
        "S.T.A.L.K.E.R.: Clear Sky (20510)",
        "SIRCAA",
        "Saiga D-12",
        "SoC",
        "SoC EE",
        "Steam Cloud: {0} B · {1}",
        "Steam Cloud: —",
        "Vodka",
        "Yaniv",
        "Yantar",
        "Zalissya",
        "{0:.1f} KB",
        "{0:.1f} kg",
        "{0:.2f} MB",
        "{0:F0} KB",
        "{0:F0} ms",
        "{0:F1} KB",
        "{0:F1} MB",
        "{0} B",
        "{0} ms",
        "{0} · {1:0.#} h",
        "{0} · {1} h",
        "{0} · {1} · {2} · {3} bytes",
        "— kg",
    ],
    "it": [
        "9×18 mm",
        "9×19 mm",
        "9×39 mm",
        "ART.",
        "Agroprom",
        "B",
        "Bar",
        "CONS.",
        "CS",
        "CS EE",
        "ChNPP",
        "Clear Sky",
        "Clear Sky (Enhanced Edition)",
        "Clear Sky EE",
        "CoP",
        "CoP EE",
        "Cordon",
        "Duga",
        "File",
        "GB",
        "Game Doctor",
        "Jupiter",
        "KB",
        "Limansk",
        "MB",
        "Malachite",
        "No",
        "OK",
        "PDA",
        "PM",
        "Pripyat",
        "Rostok",
        "S.T.A.L.K.E.R. Clear Sky",
        "S.T.A.L.K.E.R. Save Editor — web",
        "S.T.A.L.K.E.R.: Clear Sky",
        "S.T.A.L.K.E.R.: Clear Sky (20510)",
        "SIRCAA",
        "Saiga D-12",
        "Slot",
        "Slot {0}",
        "Slot: {0}",
        "SoC",
        "SoC EE",
        "Vodka",
        "Volume",
        "Yaniv",
        "Yantar",
        "Zalissya",
        "Zaton",
        "{0:.1f} KB",
        "{0:.1f} kg",
        "{0:.2f} MB",
        "{0:F0} KB",
        "{0:F0} ms",
        "{0:F1} KB",
        "{0:F1} MB",
        "{0} B",
        "{0} ms",
        "{0} · {1:0.#} h",
        "{0} · {1} h",
        "— kg",
    ],
    "pl": [
        "9×18 mm",
        "9×19 mm",
        "9×39 mm",
        "ART.",
        "Agroprom",
        "B",
        "Bar",
        "ChNPP",
        "Duga",
        "GB",
        "Game Doctor",
        "KB",
        "MB",
        "OK",
        "PDA",
        "PM",
        "Rostok",
        "SIRCAA",
        "Saiga D-12",
        "Zalissya",
        "Zaton",
        "{0:.1f} KB",
        "{0:.1f} kg",
        "{0:.2f} MB",
        "{0:F0} KB",
        "{0:F0} ms",
        "{0:F1} KB",
        "{0:F1} MB",
        "{0} B",
        "{0} ms",
        "{0}\\nFormat: {1}",
        "— kg",
    ],
    "cs": [
        "9×18 mm",
        "9×19 mm",
        "9×39 mm",
        "ART.",
        "Agroprom",
        "B",
        "Bar",
        "CS",
        "CS EE",
        "ChNPP",
        "Clear Sky",
        "Clear Sky (Enhanced Edition)",
        "Clear Sky EE",
        "CoP",
        "CoP EE",
        "Duga",
        "GB",
        "Game Doctor",
        "Jupiter",
        "KB",
        "Limansk",
        "MB",
        "OK",
        "PDA",
        "PM",
        "Rostok",
        "S.T.A.L.K.E.R. Clear Sky",
        "S.T.A.L.K.E.R. Save Editor — web",
        "S.T.A.L.K.E.R.: Clear Sky",
        "S.T.A.L.K.E.R.: Clear Sky (20510)",
        "SIRCAA",
        "Saiga D-12",
        "SoC",
        "SoC EE",
        "Vodka",
        "Zalissya",
        "Zaton",
        "{0:.1f} KB",
        "{0:.1f} kg",
        "{0:.2f} MB",
        "{0:F0} KB",
        "{0:F0} ms",
        "{0:F1} KB",
        "{0:F1} MB",
        "{0} B",
        "{0} ms",
        "{0} · {1:0.#} h",
        "{0} · {1} h",
        "— kg",
    ],
}


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
            if values[index] == en and has_latin(en) and en not in EXEMPT.get(lang, ()):
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
