# Game fixes

Game fixes correct bugs in the retail game files. The catalogue, the engine and the screen are in `crates/sse-fixes`
(catalogue and engine), `crates/sse-ui/src/screens/games.rs` (screen) and `sse-cli fixes` (command line).

## The catalogue

The catalogue is `crates/sse-fixes/data/game-fixes.json`. `build.rs` compresses it (raw DEFLATE) and embeds it in the
binary. Dataset version `2026.10.1` (`DATASET_VERSION` in `catalog.rs`); 149 definitions.

Each definition holds:

- `id`, `title`, `game` (one of the X-Ray games, their Enhanced Editions, or `Stalker2`), `version` (the fix's own
  version, e.g. `1.0.0`);
- `supportedSteamBuildIds`: the game builds the fix was checked against;
- `category`: `Essential` (50), `Recommended` (82), `Community` (16) or `Optional` (1);
- `maturity`: `Validated`, `Experimental` or `ResearchOnly`;
- `dependsOn` and `conflictsWith`;
- `implementation`: `ExactTextReplacement` (147), `Structured` (1) or `Overlay` (1);
- `textPatches`: each patch names a file, the exact text it expects, its replacement, and the SHA-256 of the file it
  expects;
- `verificationState`: `RetailFilesVerified` for all 149 entries. This records that the expected hashes were checked
  against retail files. It does not record a game run of the patched files.

Per game, the screen lists: Shadow of Chernobyl 35 (Enhanced Edition 20), Clear Sky 76 with 1 experimental (Enhanced
Edition 51), Call of Pripyat 36 (Enhanced Edition 25), S.T.A.L.K.E.R. 2 none. The Enhanced Edition entries are variants
built at load time from the retail definitions and the Enhanced Edition hashes; the counts are asserted in
`crates/sse-fixes/tests/catalog_tests.rs`.

## Installing a fix

`GameFixEngine::install` in `crates/sse-fixes/src/engine.rs`:

1. Refuses when the game for the fix is running. The check reads the process list. An install or uninstall that cannot
   check the process list is refused as well.
2. Checks that the expected file contents are present before changing them. A text patch applies only when the file
   matches its expected SHA-256 and contains the expected text exactly.
3. Writes the changed files through `AtomicFileWriter`, and records a journal of what it changed.
4. On failure, restores the files it had already changed.

A journal lets an interrupted install be recovered (`recover_interrupted`). Each installed fix has a manifest that
lists the files it manages; uninstall uses that manifest.

Some fixes need a file that is not in the program, such as a whole-file overlay. The engine downloads it only during
the install the user explicitly starts, from the asset host `save-editor-downloads.save-editor.workers.dev`, with a size
limit and HTTPS only.

## Presets

`fixes preset` and `fixes apply-preset` take one of `essential`, `recommended` or `all-safe`. The screen has the same
presets. A preset runs the same install path as a single fix, and a preset may contain only safe fixes for the selected
game.

## Command line

```bash
stalker-save fixes list [--game TARGET] [--json]
stalker-save fixes state TARGET GAME_DIR --json
stalker-save fixes install ID GAME_DIR
stalker-save fixes remove ID GAME_DIR
stalker-save fixes extract TARGET GAME_DIR OUT_DIR [--archives-only] [PREFIX...]
```

## What is not verified

The fixes are checked against file hashes and exact text. `verificationState` records the hash check against retail
files only. A game run that shows a fix working is recorded in `TASKS.md`, not in this document. Synthetic tests do
not show that the game accepts a change.
