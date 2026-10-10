# In-game companion

The companion is a set of Lua scripts that run inside the game. The editor writes requests into a folder, and the mod
answers with replies in the same folder. The code is in `crates/sse-companion`, with the installer in `installer.rs`.

## Installation is always the user's choice

- Nothing is installed without the user starting it. The crate describes the install as explicit.
- The install is manifest-backed: the manifest records the files the install placed, and removal uses it.
- The target mod directory must be absent or already owned by that manifest. Otherwise the install is refused.

## What is installed where

| Game | Mechanism | Source |
|---|---|---|
| Shadow of Chernobyl, Clear Sky, Call of Pripyat | Scripts in `gamedata/`, plus a per-game `fsgame` configuration file | `bundled.rs` embeds the payloads at build time (`install_bundled`) |
| Enhanced Editions | Steam Workshop packages (`sse-companion/src/workshop.rs` holds the item IDs and detects an existing install, read-only) | Packages are built with `sse-ui-dev --package-ee` |
| S.T.A.L.K.E.R. 2 | A UE4SS mod in the game's `Mods` directory (`install_stalker2` takes that directory) | See `bundled.rs` |

For X-Ray games the installer rewrites the configuration paths of its payload to the folder of the chosen game.

## Hooks into existing scripts

Some features need a line added to a game script that already exists. The hook code (`hook.rs`) changes a script only
when the exact source text around the insertion point is found. If that text is missing, the install is refused.

## Request and reply protocol

The mod and the editor exchange small text files in a shared folder (`protocol.rs`). The mod reads a request, runs it,
and writes a reply. Every file is capped at 1 MiB (`MAX_COMPANION_FILE_BYTES`). Each request kind has its own argument
checks in `protocol.rs`.

## Hotkeys

`hotkeys.rs` parses a hotkey layout, and `hotkey_runtime.rs` matches key presses to the game window. The shell has
two modes for this: `sse-shell --hotkey-helper` runs the hotkey helper, and `sse-shell --companion` runs the companion
command line.

## What is not verified

The installer and protocol are covered by tests on files and fixtures. The companion has not been run in a running
game build of this version; the open game checks are listed in `TASKS.md` (A15).
