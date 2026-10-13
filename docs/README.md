# Documentation

Every page in this folder, with one line about each.

## For players

| Page | What is in it |
|---|---|
| [GUIDE.md](GUIDE.md) | The user guide in English: installing, the first edit, every screen, backups, fixes, the companion, troubleshooting. |
| [GUIDE.uk.md](GUIDE.uk.md) | Посібник користувача українською: the same guide in Ukrainian, with the Ukrainian button names and screenshots. |
| [GUIDE.ru.md](GUIDE.ru.md) | Руководство пользователя на русском: the same guide in Russian, with the Russian button names and screenshots. |
| [CAPABILITIES.md](CAPABILITIES.md) | What can be edited in each game, cell by cell, with the reason for every refusal. |
| [RELEASE_NOTES_2.0.0.md](RELEASE_NOTES_2.0.0.md) | Release notes for 2.0.0 (English, Russian, Ukrainian): what is new, known limitations, what is not verified. |
| [RELEASE_NOTES_2.0.1.md](RELEASE_NOTES_2.0.1.md) | Release notes for 2.0.1 (English, Russian, Ukrainian): the fix for upgrading from 1.3.x. |
| [RELEASE_NOTES_2.0.2.md](RELEASE_NOTES_2.0.2.md) | Release notes for 2.0.2 (English, Russian, Ukrainian): the fix for Linux, where the editor did not start on GNOME with Wayland. |

## For developers

| Page | What is in it |
|---|---|
| [DEVELOPMENT.md](DEVELOPMENT.md) | Building from source, the crates, running the tests and CI checks locally, adding an interface string, the command line. |
| [ARCHITECTURE.md](ARCHITECTURE.md) | The crates and their boundaries, and the path a save takes when it is written. |
| [GAME_FIXES.md](GAME_FIXES.md) | The fix catalogue, how a fix is installed and removed, presets, the `fixes` commands. |
| [COMPANION.md](COMPANION.md) | The in-game companion: what is installed where, hooks into game scripts, the request and reply protocol, hotkeys. |
| [WORKSHOP.md](WORKSHOP.md) | Steam Workshop packages of the companion for the Enhanced Editions: staging, checking, uploading. |
| [DIAGNOSTICS.md](DIAGNOSTICS.md) | Logs, the doctor commands, error reports and performance metrics, and what leaves the computer. |
| [PACKAGING.md](PACKAGING.md) | How the Linux, Windows and macOS packages and the update manifest are built, and which CI job does what. |
| [RELEASE.md](RELEASE.md) | The release procedure step by step, and who does each step. |
| [THIRD_PARTY_ASSETS.md](THIRD_PARTY_ASSETS.md) | Images, sounds and fonts in the repository that the authors did not create, with their sources and terms (in Russian). |

## Pictures

`images/guide/<language>/` holds the screenshots of the user guide for `en`, `uk` and `ru`: one set of 23 pictures
per language, 1366×768, taken from the program on the test saves of `fixtures/synthetic/`. How they are taken is in
[DEVELOPMENT.md](DEVELOPMENT.md#screenshots).

## In the repository root

[README.md](../README.md) is the page for players. [PLAN.md](../PLAN.md), [TASKS.md](../TASKS.md),
[ACCEPTANCE.md](../ACCEPTANCE.md) and [AGENTS.md](../AGENTS.md) hold the plan, open work, acceptance criteria and
the rules for contributors.
