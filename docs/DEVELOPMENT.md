# Development

For people who build the editor from source or change it. Players do not need this page: see the
[user guide](GUIDE.md). The rules every contributor follows are in [AGENTS.md](../AGENTS.md).

## Two rules to know first

- **Only `std`.** The workspace has no third-party crates and no C or C++ code. Every dependency is another crate
  of this repository. Codecs, parsers, hashes, the signature check, image and font code are written here.
- **`unsafe` only in `sse-sys`.** Calls into the operating system and into Steam's library live in that crate. The
  workspace lint `unsafe_code = "deny"` applies everywhere else.

There is also no `async` runtime, and nothing slow runs on the interface thread.

## Build from source

The toolchain is pinned in `rust-toolchain.toml` (Rust 1.99.0, with `clippy` and `rustfmt`); `rustup` installs it
by itself on the first `cargo` call. The declared minimum is Rust 1.85, and CI checks it.

```bash
cargo build --release -p sse-cli -p sse-ui
```

This gives `target/release/stalker-save` (the command line) and `target/release/sse-shell` (the editor window). The
same package also has `sse-ui-dev`, a developer-only binary for screenshots, measurements and CI layout checks; it
is not shipped.

On a machine with little memory, build one package at a time and limit the jobs, for example
`cargo build -j2 --release -p sse-ui`.

The browser edition is built by `tools/build-web.sh`: it writes `web/sse_web.wasm` (refused above 5 MiB) and
`web/item-icons.bin`. The whole `web/` folder is what gets served. The item pictures are not compiled into the
browser build; the page fetches that file after the editor has started.

Release packages are described in [PACKAGING.md](PACKAGING.md).

## Crates

| Crate | What it is responsible for |
|---|---|
| `sse-core` | Shared contracts: errors, the owned save buffer, the checked byte cursor |
| `sse-codecs` | Parsers and codecs written here: JSON, ZIP, PNG, JPEG, Kraken, LZO1X, LZHUF, hashes, the P-256 signature check |
| `sse-sys` | Operating-system adapters: files, HTTPS, Steam, windowing, hotkeys. The only crate that may use `unsafe` |
| `sse-content` | Files of an installed game: archives, LTX configs, XML string tables, DDS textures, the file tree |
| `sse-catalog` | Official names, the installed-game catalogue, translations |
| `sse-xray` | X-Ray saves (Shadow of Chernobyl, Clear Sky, Call of Pripyat and the Enhanced Editions): container, index, readers, writers, the capability registry |
| `sse-s2` | S.T.A.L.K.E.R. 2 containers, indexes and the writer for its supported changes |
| `sse-storage` | Durable file writes, backups with a journal, edit drafts |
| `sse-fixes` | The game-fix catalogue and the patching engine |
| `sse-companion` | Companion install and removal, the file protocol, exact-source hooks, hotkey models |
| `sse-steam` | Steam library discovery, the isolated worker protocol, Cloud transactions, achievements |
| `sse-lint` | Static analysis of game scripts |
| `sse-doctor` | Read-only save and crash diagnostics |
| `sse-update` | The update manifest, the signature check, streaming download, platform installers |
| `sse-app` | Application services: settings, the save session, metrics, diagnostics, paths, tasks |
| `sse-ui` | The interface toolkit, the screens, and the binaries `sse-shell` and `sse-ui-dev` |
| `sse-web` | The browser host for the same shell |
| `sse-cli` | The `stalker-save` command line |
| `sse-fuzz-smoke` | The deterministic parser corpus run by CI |

How the crates fit together and how a save is written safely: [ARCHITECTURE.md](ARCHITECTURE.md).

## Tests and the CI checks, locally

The workflow is `.github/workflows/ci.yml`. The main checks of its `check` job:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
python3 tools/check_data_isolation.py -- cargo test --workspace
```

`tools/check_data_isolation.py` runs a command with a temporary application-data folder and fails if the command
wrote into the real one. Tests and developer tools must use a temporary `STALKER_SAVE_EDITOR_DATA` root.

To run less than the whole workspace, name the package: `cargo test -p sse-xray`.

Interface and translation checks (Linux job):

```bash
# performance budgets of the interface
python3 tools/check_data_isolation.py -- cargo run --release -p sse-ui --bin sse-ui-dev -- --ci-budget
# buttons and text that do not fit, in every language
python3 tools/check_data_isolation.py -- cargo run --release -p sse-ui --bin sse-ui-dev -- --ci-i18n-buttons
python3 tools/check_data_isolation.py -- cargo run --release -p sse-ui --bin sse-ui-dev -- --ci-text-overflow fixtures/synthetic/xray-clear-sky.sav
# Cyrillic source strings that bypass the translation helpers
python3 -m unittest discover -s tools -p check_ui_i18n.py
python3 tools/check_ui_i18n.py
python3 tools/check_data_isolation.py -- python3 -m unittest discover -s tools/tests
python3 tools/check_i18n_source.py
```

Parser fuzz smoke:

```bash
python3 tools/check_data_isolation.py -- cargo test -p sse-fuzz-smoke --release -- --nocapture
```

The browser edition has its own job: `sh tools/build-web.sh`, then `cargo test --locked -p sse-web`.

Parity with the C# 1.3.1 editor is checked by hand, not in CI: `tools/oracle.sh` compares output with the C#
command-line editor when `SSE_ORACLE` points at it.

Synthetic round-trip tests do not prove that a game accepts an edit. What has and has not been loaded in the games
is listed in `ACCEPTANCE.md`, part V.

### Screenshots

`sse-ui-dev --screenshot OUT.png [WxH] [NAV] [--open SAVE] ...` paints one screen into a PNG without opening a
window. `NAV` is the index of the screen in `ScreenId::ALL` (`crates/sse-ui/src/screens/mod.rs`). The interface
language is taken from `STALKER_EDITOR_LANG`. The pictures of the user guide in `docs/images/guide/` were taken this
way at 1366×768 on the saves from `fixtures/synthetic/`, with `HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME` and
`STALKER_SAVE_EDITOR_DATA` pointing into a temporary folder, so that no personal path gets into a picture.

## Adding an interface string and its translations

Interface text uses **Russian source strings as keys**. There are 15 languages (`LANGUAGES` in
`crates/sse-ui/src/strings.rs`): `ru`, `uk`, `en`, `de`, `fr`, `it`, `es`, `pl`, `cs`, `pt-BR`, `tr`, `ja`, `ko`,
`zh-CN`, `zh-TW`.

1. In the screen code, wrap the Russian text in a translation helper: `crate::strings::t("Сохранить")`, or `tr` /
   `tr_in` for text with `{0}` placeholders. A Cyrillic literal outside these helpers fails
   `tools/check_ui_i18n.py` and `tools/check_i18n_source.py`.
2. Add the key to `tools/i18n-extra.json` with all 15 translations. A key that exists only there must have every
   language; the generator refuses it otherwise.
3. Regenerate the table: `python3 tools/generate_strings.py <folder with the C# editor's i18n catalogs>`. It
   rewrites `crates/sse-ui/src/strings.rs`; do not edit that file by hand. The base catalogs come from the C# 1.3.1
   editor, with `crates/sse-catalog/i18n` as a fallback. Running the generator against `crates/sse-catalog` alone
   produces a different table, so the C# catalog folder is needed.
4. Run the translation checks above. `tools/check_i18n_fallback.py` counts rows that still show English or Russian
   in another language and fails when a count grows beyond `tools/i18n-fallback-baseline.json`.

Identifiers (item keys, file names, script names) are never translated.

## Command line

```text
stalker-save version
stalker-save info <save>
stalker-save inventory <save> [--all]
stalker-save audit [path]
stalker-save doctor save|crash|game|quests ...
stalker-save set-money|set-stack|edit <save> ... (-o OUTPUT | --in-place) [--backup-dir DIR]
stalker-save backups list|restore ...
stalker-save fixes list|state|status|preset|apply-preset|install|update|remove|extract ...
stalker-save lint <game-folder> [--check NAME] [--config SUBDIR] [--json]
stalker-save update check|download|verify-manifest ...
```

Write commands take `--in-place` for a journaled replacement, or `-o`/`--output` for a new file. The two cannot be
combined. Exit codes: `0` done, `2` wrong arguments, `3` refused (unsupported or unsafe), `4` unreadable or damaged
input, `5` file or system error.

## What is verified, and where it stops

- **Automated, in CI:** formatting, clippy with warnings denied, the workspace tests on Linux, Windows and macOS,
  parser fuzz smoke tests, interface performance budgets, translation layout and source checks, and acceptance
  screenshots.
- **By hand:** parity with the C# editor through `tools/oracle.sh`.
- **Not verified:** writes from this version have not been loaded in the games, one Call of Pripyat save excepted.
  The Windows and macOS builds were never started by a person. See `ACCEPTANCE.md`, part V, and `TASKS.md`.

## Where things are written down

- [PLAN.md](../PLAN.md): the plan and the budgets that CI enforces.
- [TASKS.md](../TASKS.md): open work.
- [ACCEPTANCE.md](../ACCEPTANCE.md): acceptance criteria; part V is the owner's final check list.
- [AGENTS.md](../AGENTS.md): the rules of the repository.
- [All documentation pages](README.md).
