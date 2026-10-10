# Architecture

## Crates

The workspace has no third-party crates. Every crate depends only on other crates in this repository and on the
standard library.

| Crate | Role |
|---|---|
| `sse-core` | Shared contracts: errors, the owned save buffer, the checked byte cursor |
| `sse-codecs` | Parsers and codecs written here: JSON, ZIP, PNG, DDS, LZHUF, hashes, P-256 signature check |
| `sse-sys` | Operating-system adapters: files, HTTPS, Steam, windowing, hotkeys. The only crate that may use `unsafe` |
| `sse-content` | Files of an installed game: archives, LTX configs, XML string tables, DDS textures, file tree |
| `sse-catalog` | Official names, installed-game catalogue, translations |
| `sse-xray` | X-Ray saves (SoC, CS, CoP and the Enhanced Editions): container, index, readers, writers |
| `sse-s2` | S.T.A.L.K.E.R. 2 containers and indexes; writer for the four supported changes |
| `sse-storage` | Durable file writes, backups with a journal, edit drafts |
| `sse-fixes` | Game-fix catalogue and the patching engine |
| `sse-companion` | Companion install and removal, file protocol, exact-source hooks, hotkey models |
| `sse-steam` | Steam library discovery, isolated worker protocol, Cloud transactions, achievements |
| `sse-lint` | Static analysis of game scripts |
| `sse-doctor` | Read-only save and crash diagnostics |
| `sse-update` | Update manifest, signature check, streaming download, platform installers |
| `sse-app` | Application services: settings, save session, metrics, diagnostics, paths, tasks |
| `sse-ui` | The interface toolkit, screens, and the binaries `sse-shell` and `sse-ui-dev` |
| `sse-web` | Browser host for the same shell |
| `sse-cli` | The `stalker-save` command line |
| `sse-fuzz-smoke` | Deterministic parser corpus run by CI |

Boundaries that the code enforces:

- Every interface (desktop, web, command line) uses the shared editor service in `sse-app`. Mutation logic is not
  duplicated in the interface layers.
- Only `sse-sys` contains `unsafe` code. Other crates forbid it or do not use it.
- Format detection is based on the file content. File names are not used to decide the format.
- Write capabilities are explicit and per format. A field is written only when the capability registry allows it.

## Save write path

`stalker-save edit ... --in-place` and the interface's save action use the same path: `replace_transaction` in
`crates/sse-storage/src/transaction.rs`.

1. **Refuse unsafe targets.** The save must not be a symbolic link or read-only. The backup directory must lie
   outside the save's own directory.
2. **Check the source hash.** The current file's SHA-256 must equal the hash recorded when the edit was prepared.
   If it differs, the write is refused ("source changed since preparation").
3. **Preflight.** The caller's preflight runs on the original and the replacement bytes before any file is changed.
4. **Backup and journal.** Three files are created with create-only writes and flushed:
   - `<name>_ORIGINAL.sav`: a copy of the original bytes;
   - `<name>_EDITED.sav`: the replacement bytes;
   - `<name>_ORIGINAL.json`: the journal, status `prepared`, with both hashes and the paths.
5. **Temporary output.** The replacement is written to a temporary file in the save's directory, and the original
   permissions are copied to it.
6. **Recheck, then replace.** The source hash is read again, because another writer could have changed the file while
   metadata was copied. The temporary file then replaces the save only if the hash still matches. The directory is
   synced.
7. **Read back.** The save is read again. Its bytes must equal the replacement, and the caller's verification must
   pass. The journal is rewritten with status `verified` through a temporary file and then replaces the old journal.
8. **Rollback on failure.** If any step after the replacement fails, the code checks that the save still holds the
   edited hash. It writes the original bytes to a temporary file and replaces the save only if the hash still matches.
   It then reads the save back and compares it with the original. If the restore cannot be verified, the error says so
   and the backup files stay on disk.
9. **Maintenance.** After a successful write, verified backup sets beyond 100 are pruned. A pruning failure is reported
   as a warning; the write itself stays successful.

The journal is limited to 2 MiB. `restore_in_place` and `restore_backup` restore a save from a journal; the `backups`
command lists and restores them.

S.T.A.L.K.E.R. 2 writes follow the same path. `sse-s2` produces the replacement bytes, and the S2 verification
re-reads wallet, stack counts and the CRC.

Uncertain outcomes are never retried automatically. A Steam Cloud write is a separate transaction that checks the
file hash before it sends the payload; the worker sets its `payload_sent` flag before the bytes leave the process.

## Interface layer

- `sse-ui` draws every screen with its own toolkit (`crates/sse-ui/src/`). Background work runs in threads; the
  update checks catch panics from their work and report them as errors.
- `sse-ui/src/screens/saves.rs` holds the save and inventory screens and calls `replace_transaction` for writes.
  `services.rs` holds the Achievements, Cloud, Companion and Updates sections. `games.rs` holds the game-fix screen.
  `history.rs` lists backups.
- Interface text is in `crates/sse-ui/src/strings.rs` and `tools/i18n-extra.json`. `tools/check_ui_i18n.py` checks
  Cyrillic literals in Rust sources.

## Non-goals of the current code

- No async runtime. Background work uses standard threads.
- The program's own HTTPS requests go to: the update manifest and the packages it lists; fix asset files, only when
  the user installs a fix that needs them; the metrics endpoint, only after consent; and the report endpoint, only
  after the user confirms a report. See [DIAGNOSTICS.md](DIAGNOSTICS.md) and [PACKAGING.md](PACKAGING.md).
