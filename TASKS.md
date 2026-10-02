# Work packages

Each package is a task text to hand to its executor as is. Reference code: the C# editor at tag `v1.3.1`,
`https://github.com/Dmitriy-DE/S.T.A.L.K.E.R.-Save-Editor` (paths below are in that repository). Rules: `AGENTS.md`.
State of each package: the table at the end of `PLAN.md`.

Every package ends the same way: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
and `cargo test --workspace` pass; a pull request from `wp/<id>-<slug>` says what was run and what was not checked.

---

## X1 — LZO1X codec (ChatGPT; no repository access needed)

Attachments the owner uploads with this text: `Lzo1xCodec.cs`, six `lzo1x-*.lzo` / `lzo1x-*.raw` pairs,
`cursor.rs`, `error.rs`, the lint section of `Cargo.toml`.

Write one Rust file, `lzo1x.rs`, for the crate `sse-codecs`:

```rust
pub fn decompress(stream: &[u8], expected_size: usize) -> sse_core::Result<Vec<u8>>;
pub fn compress(payload: &[u8]) -> Vec<u8>;
```

- `decompress` reproduces `Lzo1xCodec.Decompress`: the same accepted streams, the same refusals (truncated stream,
  back-reference before the start, output longer or shorter than `expected_size`, missing end marker). One output
  allocation of `expected_size`; no intermediate copy. A damaged stream returns `Error::Damaged`, never a panic.
- `compress` produces **byte for byte** what `Lzo1xCodec.Compress` produces: the writers' fixtures compare whole
  files, so a better compressor is a failed task.
- No `unsafe`, no indexing with `[]`, no unchecked arithmetic (the attached lints deny them), no dependencies.
- Tests in the same file: each attached pair both ways; round trip of 1 000 pseudo-random buffers (a fixed-seed
  generator written in the test, sizes 0…70 000, mixed repetitive and random data); every truncation of one vector
  is an error; a hostile stream claiming a 4 GiB literal is an error without allocating it.
- Return the file and a note of anything in the C# code that looked like a bug rather than behaviour to copy.

## X2 — LZHUF, archive descrambling, CRC32 (ChatGPT)

Attachments: `XRayArchiveHeaderCodec.cs`, the `xray-archive/` fixture (`synthetic.db`, `manifest.json`,
`entry-*.bin`), `cursor.rs`, `error.rs`, `lints.toml`.

Two Rust files for `sse-codecs`:

```rust
// lzhuf.rs
pub fn decode(code: &[u8]) -> sse_core::Result<Vec<u8>>;                 // = XRayArchiveHeaderCodec.DecodeLzhuf
pub fn descramble(data: &[u8], world_wide: bool) -> Vec<u8>;             // = DecryptScramble
// crc32.rs
pub fn crc32(data: &[u8]) -> u32;                                        // IEEE, polynomial 0xEDB88320, table built at compile time
```

- The same output as the C# code for every input it accepts and an error where it refuses. The declared output size
  is checked against a maximum of 64 MiB before allocating.
- Tests: the fixture's header table decoded and compared with `manifest.json`; truncation at every byte of the coded
  table is an error or a shorter valid result exactly as in C#; CRC of the standard vector `123456789` is
  `0xCBF43926`.
- Same constraints as X1: no `unsafe`, no `[]` indexing, checked arithmetic, no dependencies.

## X3 — Steam VDF reader (ChatGPT)

Attachments: `SteamVdfParser.cs`, `steam-vdf/libraryfolders.vdf`, `steam-vdf/unterminated.vdf`,
`golden/steam-vdf/libraryfolders.json`, `cursor.rs`, `error.rs`, `lints.toml`.

One file `vdf.rs`: a reader of Valve's text KeyValues format with the same result as `SteamVdfParser` — nested
sections, quoted keys and values, escapes, comments, the same limits on depth and size, the same refusals
(`unterminated.vdf` is an error). API: `pub fn parse(text: &str) -> sse_core::Result<Node>` with `Node` giving
ordered children and case-insensitive lookup as the C# class does. Tests: the fixture against the golden JSON
(write the comparison by hand, no JSON dependency: assert the library paths and app ids the golden file lists),
depth bomb, 10 MiB of one unterminated string.

---

---

## C1 — X-Ray saves: container, registry, readers (Codex)

Crate `sse-xray` (yours alone). Reference: `src/StalkerSaveEditor.Core/Formats/XRay/XRayContainer.cs`,
`XRayChunk.cs`, `XRayTrilogyReader*.cs`, `XRayTrilogySave.cs`, `XRayRelationRegistry.cs`, `XRayLevelChangerReader.cs`,
`XRayRelocation.cs` (reading part), `XRayProgressReader.cs`, `XRayWeatherReader.cs`, `Formats/Enhanced/`,
`Inspection/SaveInspector.cs`; their tests under `tests/StalkerSaveEditor.Core.Tests/Formats/XRay` and
`docs/knowledge/XRAY_FORMAT.md`.

Deliver:

1. `Container`: header (signature, version 3/5/6, unpacked size ≤ 512 MiB checked before allocating), chunk table
   over the unpacked image. Until X1 lands, unpacking is a function the caller passes in; tests use the `.raw`
   fixtures next to each `.sav`. Do not write an LZO codec.
2. `Save`: the unpacked image (`SaveBuffer`) plus an **index** — for every registry object its name, section,
   id, parent, version and the offsets of its record, state, update and client data. No object graph: inventory
   items, stashes, level changers, relations, tasks, weather are views that decode from the image when asked.
3. Format detection for the six formats (`stalker-soc`, `-cs`, `-cop` and the three `-ee`), by content, with the
   same answers as the C# reader for every fixture.
4. Level-changer destinations with the rule of `XRayRelocation.FindDestination` at `v1.3.1`: the block is accepted
   only where the restrictor shape list ends and only when exactly one position fits. Capital letters in level names
   are valid.
5. In `sse-cli`: commands `info` and `inventory` whose output is identical to the C# command line (you may add the
   two commands to `sse-cli/src/main.rs`; nothing else there).

Done when: every `xray-*.sav`/`.raw` fixture and `fixtures/golden/xray-*-vectors.json` passes; `tools/oracle.sh`
reports no difference (the oracle binary is the `stalker-save-editor-cli` of release 1.3.1); every reader has a
truncation test and a bit-flip test over a fixture; reading one save allocates the image once (assert it in a test
with a counting allocator).

Not in this package: any writer, S2, file access outside tests.

## C2 — X-Ray writers and the write transaction (Codex, after C1)

Crates `sse-xray` (writers) and `sse-storage` (yours alone). Reference: `Formats/XRay/XRay*Writer.cs`,
`XRayRelocation.cs` (`Prepare`), `Editing/EditPlan.cs`, `EditKind.cs`, `EditService.cs`, `PreparedEdit.cs`,
`DraftStore.cs`, `Backups/LocalSaveReplacement.cs`, `LocalSaveStorage.cs`, `Storage/AtomicFile.cs`,
`Capabilities/`; tests under `tests/.../Formats/XRay`, `Editing`, `Backups`, `Storage`.

Deliver:

1. `ChangeSet` (in `sse-xray`; Claude moves the shared part to `sse-core` when S2 needs it): money, stack counts,
   durability, placement, upgrades, faction relations and player faction, item add (clone of a template), item
   delete, stash take/put, info portions, actor relocation. A change names its target by object id and carries the
   old value where the C# plan does.
2. One function `apply(&Save, &ChangeSet) -> Result<SaveBuffer>`: one working copy of the unpacked image, all
   changes applied to it, repacked once. No stage-by-stage re-parsing and no intermediate images (the C# pipeline
   made one per kind of edit). After applying, the result is re-read and compared with the intended state, and a
   byte comparison proves that nothing outside the ranges the changes own was touched.
3. Capabilities: which kinds of change each of the six formats allows, identical to
   `fixtures/golden/capabilities`. An unsupported or unproven change is `Error::Refused` before any work.
4. `sse-storage`: durable write (temp file, flush to disk, atomic rename, directory flush where the OS has it),
   the replace transaction (fresh SHA-256 of the source must equal the one the edit was prepared for → backup with a
   journal entry → write → read back and verify → on any failure the original stays), backup listing and restore,
   drafts (untouched state + the latest 100 change sets, bounded file size). File formats of the journal and of the
   drafts stay readable by the C# editor and vice versa (fixtures `drafts/`, tests in `Backups`).
5. `sse-cli`: `set-money`, `set-stack`, `edit` with the C# command line's arguments, output and exit codes.

Done when: every `writer-*` and `xray-stashes` / `xray-level-changer` fixture pair is reproduced **byte for byte**
(packed files once `sse_codecs::lzo1x` is in `main`; until then compare unpacked images and mark the packed
comparisons `#[ignore = "needs X1"]`); change followed by its inverse returns the original image on every fixture;
a failure injected at each step of the transaction (a test file system) leaves the original file intact; an edit of
a save allocates the image at most three times (source, working copy, verification), asserted with a counting
allocator.

## C3 — S.T.A.L.K.E.R. 2 saves and Kraken (Codex, after C2)

Crate `sse-s2` (yours alone) and `sse-codecs/src/kraken.rs` + its `build.rs`. Reference: `Formats/Stalker2/*.cs`,
`Codecs/KrakenCodec.cs`, `tools/ooz_native.cpp` and `tools/build_ooz_native.py`, `Catalogs/Stalker2*.cs`,
`docs/knowledge/S2_FORMAT.md`, `docs/evidence` section on the legacy layout; tests under `tests/.../Formats/Stalker2`
and `Codecs`.

Deliver:

1. Kraken: the vendored C++ compiled by `cc` and linked statically; one safe wrapper each way
   (`decompress_into(source, &mut [u8])`, `compress(payload) -> Vec<u8>`). This is the only `unsafe` in the workspace:
   one module with `#![allow(unsafe_code)]`, every call checked for sizes before and after. The decoder writes
   straight into the buffer that becomes the save image — no work buffer copied afterwards.
2. Container: header, CRC32 (`sse_codecs::crc32`, or a local one marked for replacement until X2 lands), sizes
   checked before allocating.
3. Reader as image + index: wallet, inventory records with names (name tables, both layouts), item state, stashes,
   the legacy 1.0.x layout read-only exactly as `Stalker2InventoryLayout.IsLegacy` decides.
4. Writers: money, stacks (with the rule that refuses reducing a kind-8 item to one **before** packing), durability,
   stash to backpack, as changes on one working copy; verification decodes the result once into a temporary buffer
   and compares ranges — it does not build a second parsed save.
5. `sse-cli`: the S2 branches of `info`, `inventory`, `set-money`, `set-stack`, `edit`.

Done when: `synthetic-s2*` and every `writer-s2-*` fixture pair is reproduced byte for byte; `tools/oracle.sh`
shows no difference; truncation and bit-flip tests for the container and the readers; peak memory of a money edit on
a synthetic 64 MiB image is at most three images plus the packed input and output (counting allocator).

---

## G1 — Files of an installed game (Gemini)

Crate `sse-content` (yours alone). Reference: `src/StalkerSaveEditor.Core/Formats/XRay/XRayArchive*.cs`,
`Content/GameFileTree.cs`, `Content/LtxDocument.cs`, `Content/XRayStringTables.cs`, `Content/DdsImage.cs` and their
tests under `tests/StalkerSaveEditor.Core.Tests/Content` and `.../Formats/XRay` (archive tests).

Deliver:

1. Archive reader for `.db*` / `.xdb*`: header table, entries by path, one entry's bytes on demand. The archive is
   memory-mapped (`memmap2` is the one dependency allowed here); nothing is read until asked for. Header decoding
   (LZHUF, descrambling) arrives with X2: until then take it as a function parameter and test with the decoded table
   of the `xray-archive` fixture.
2. File tree of a game: loose `gamedata` files over archives, later archives over earlier ones, as the C# tree does.
3. LTX documents: sections, inheritance, `#include`, the same answers as `LtxDocument` for its tests.
4. String tables (XML, Windows-1251 / 1250 / UTF-8 detected as the C# code does) → id to text per language.
5. DDS (DXT1/3/5, uncompressed) → RGBA8 and a rectangle cut out of an atlas.

Done when the C# tests of these classes are reproduced as Rust tests on the same fixtures, each parser has
truncation and hostile-length tests, and opening a 1 GiB archive (synthetic, generated in the test's temp folder)
keeps the process under 20 MiB.

Not in this package: catalogues, names, icons cache, anything that knows about saves.

## G2 — Names, catalogues, translations (Gemini, after G1)

New crate `sse-catalog` (yours alone; add it to the workspace members only by creating the folder — the workspace
globs `crates/*`). Reference: `Core/Catalogs/*.cs` and `Catalogs/Data/*.json`, `Content/InstalledGameCatalogBuilder.cs`,
`Content/GameContentService.cs`, `Desktop/Services/PlaceNames.cs`, `SaveNaming.cs`, `I18nService.cs`,
`I18nCompletenessChecker.cs`, `Desktop/i18n/*.json`, `tools/generate_place_names.py`; tests under
`tests/.../Catalogs`, `Content`, `Desktop/PlaceNamesTests.cs`, `TranslationTests`.

Deliver:

1. The shipped catalogues as data files copied from the C# repository unchanged (`catalog_names.json`,
   `catalogs.json`, `s2_items.json`, `s2_upgrades.json`), embedded compressed and parsed on first use into compact
   tables (interned strings, no per-entry maps of 13 languages: one table per language loaded on demand). `serde` +
   `serde_json` are allowed here.
2. Name resolution with the C# order: the games' own name in the interface language → the installed game's
   catalogue → the section key. Kinds: items, upgrades, factions, levels, stashes. `PlaceNames` rules reproduced
   (personal boxes, numbered boxes, camp boxes, level by object prefix, level aliases).
3. Catalogue of an installed game built from `sse-content` (items with name, category, weight, cost, icon
   rectangle; upgrades per item), cached on disk keyed by the game build, as `GameContentService` does.
4. Translations: Russian source string → text in one of 14 languages, plural forms as `I18nService`; a checker that
   fails when any language lacks a key, exposed as a function and as a test.

Done when the C# tests of these classes pass as Rust tests on the same data, and loading every shipped catalogue
keeps the process under 15 MiB with only the active language's names in memory.

## G3 — Game fixes (Gemini, after G2)

New crate `sse-fixes` (yours alone). Reference: `Core/Patching/GameFixCatalog.cs`, `GameFixModels.cs`,
`GameFixEngine*.cs`, `GameFixContentStore.cs`, `GameFileSystem.cs`, `Patching/Data/game-fixes.json`,
`docs/GAME_FIXES.md`; tests under `tests/.../Patching`; `tools/fix_regress.py`, `tools/fix_realcheck.sh`.

Deliver:

1. The catalogue `game-fixes.json` read as is (it stays the single source for both editors): definitions, text
   patches with expected file hash, anchors, code pages, `retailOnly`, the Enhanced Edition hash table, presets.
2. The engine: state of a fix in an installation (not installed / installed / changed by someone else / outdated),
   install and remove with the original kept and restored, exact-source-hash rule (a file that is not the expected
   one is never patched), the journal and recovery after an interrupted run, state files readable by the C# editor
   and vice versa.
3. `sse-cli`: `fixes list|state|install|remove|preset|extract` with the C# arguments and output.

Done when: the C# engine tests are reproduced; the counts per game equal the catalogue's (SoC 35, CS 75, CoP 36 and
the EE variants 20 / 51 / 25); `tools/fix_realcheck.sh` of the C# repository, pointed at the Rust command line,
installs and removes every fix on copies of the six installs (the owner or Claude runs this part: it needs the
games).

---

## U0 — Interface toolkit decision (Claude)

Two throwaway prototypes of one screen (save list with thumbnails, inventory table of 2 000 rows with icons, detail
panel, the current dark theme, Russian + Chinese text, scale 100–200%): Slint and egui. Measure idle memory, cold
start, binary size, scroll smoothness; compare screenshots with the C# editor. The choice and the numbers go into
`PLAN.md`; the prototypes are deleted.
