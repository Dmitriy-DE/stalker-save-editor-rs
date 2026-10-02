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

## X2 — LZHUF and CRC32 (ChatGPT)

Same form as X1. Attachments: `XRayArchiveHeaderCodec.cs`, `xray-archive/` fixture, the CRC32 routine of
`Stalker2SaveReader.cs`. Files `lzhuf.rs` (`decode`, plus the archive header descrambling) and `crc32.rs`, with the
fixture as tests and the same damaged-input rules.

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

`sse-xray` writers (money, stacks, durability, placement, upgrades, factions, add, delete, stash transfer, info
portions, relocation) as changes applied to one working copy of the image, and `sse-storage` (durable write, atomic
replace, backup with journal, read-back, drafts with a bounded history). Reference: the `XRay*Writer.cs` files,
`Editing/`, `Backups/`, `Storage/AtomicFile.cs`. Done when every `writer-*` fixture pair is reproduced byte for byte
and an edit followed by its undo gives the original image on every fixture.

## C3 — S.T.A.L.K.E.R. 2 saves and Kraken (Codex, after C2)

`sse-s2` and the Kraken binding in `sse-codecs` (the vendored C++ `tools/ooz_native.cpp`, linked statically, the only
`unsafe` in the workspace, behind one safe function that decodes into a caller-owned buffer). Reference:
`Formats/Stalker2/`, `Codecs/KrakenCodec.cs`, `docs/knowledge/S2_FORMAT.md`.

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

## G2 — Names and catalogues (Gemini, after G1)

Item, upgrade, faction, level and stash names from `catalog_names.json` and from an installed game; the interface
translations (`i18n/*.json`, Russian keys) with the completeness check. Reference: `Core/Catalogs/`,
`Desktop/Services/PlaceNames.cs`, `SaveNaming.cs`, `I18nService.cs`.

## G3 — Game fixes (Gemini, after G2)

`sse-fixes`: the catalogue `game-fixes.json` as data, install / remove / state by exact source hash, as
`Core/Patching/`. Done when `tools/fix_realcheck.sh` of the C# repository, pointed at the Rust command line, installs
and removes every fix on copies of the six installs.

---

## U0 — Interface toolkit decision (Claude)

Two throwaway prototypes of one screen (save list with thumbnails, inventory table of 2 000 rows with icons, detail
panel, the current dark theme, Russian + Chinese text, scale 100–200%): Slint and egui. Measure idle memory, cold
start, binary size, scroll smoothness; compare screenshots with the C# editor. The choice and the numbers go into
`PLAN.md`; the prototypes are deleted.
