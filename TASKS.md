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

Crate `sse-s2` (yours alone) and `sse-codecs/src/kraken/`. Reference: `Formats/Stalker2/*.cs`,
`Codecs/KrakenCodec.cs`, `tools/ooz_native.cpp` and `tools/build_ooz_native.py`, `Catalogs/Stalker2*.cs`,
`docs/knowledge/S2_FORMAT.md`, `docs/evidence` section on the legacy layout; tests under `tests/.../Formats/Stalker2`
and `Codecs`.

Deliver:

1. Kraken in safe Rust, `sse-codecs/src/kraken/` — no C++, no `unsafe`, no build script. Reference for the format:
   the `ooz` sources in `third_party/pyooz/pyooz-0.0.8.tar.gz` of the C# repository (`kraken.cpp` for decoding,
   `compr_kraken.cpp`, `compr_entropy.cpp`, `compr_match_finder.cpp` for encoding). Write it as Rust, not as
   translated C: slices and checked cursors instead of pointer arithmetic, every table size and offset checked.
   - Decoder `decompress_into(source, &mut [u8])`: writes straight into the buffer that becomes the save image.
     Proof: fixture vectors, then every S2 save of the owner decodes to the same bytes as the C# editor (Claude runs
     this part), plus mutation tests — a damaged stream is `Error::Damaged`, never a panic or an endless loop.
   - Encoder `compress(payload) -> Vec<u8>`: any valid stream our decoder and the reference decoder both accept; it
     need not equal the C++ output byte for byte, but it must stay within 5% of its size on the fixtures (the game
     reads the declared sizes). Start with the simplest level that meets this; say in the pull request which entropy
     modes are produced. Until the encoder is accepted, S2 writers are compiled but refused by capabilities.
   - Split it: decoder first as its own pull request (`wp/c3a-kraken-decode`), then the reader, then the encoder.
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
   never loaded whole and not memory-mapped: one entry is read with positional reads (`FileExt::read_at` on Unix,
   `seek_read` on Windows — both `std`, no dependency, no `unsafe`) behind a small `trait ReadAt`, so the same code
   reads from a byte slice in tests and on the web. Remove `memmap2`. Header decoding
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
   `catalogs.json`, `s2_items.json`, `s2_upgrades.json`), baked at build time into binary tables of our own layout (a `build.rs` that uses `sse_codecs::json`
   from X4; sorted keys + offsets, one string blob per language) and embedded: at run time nothing is parsed, a
   lookup is a binary search in `&'static [u8]`, and only the active language's blob is touched. No `serde`.
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

# Second wave

Common to every package below: no third-party code (`AGENTS.md`), and the pull request has a section **"Better than
the reference"** with numbers (time, memory, allocations, size) measured against the C# class it replaces.

## X4 — JSON reader and writer (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`, sample files (`game-fixes.json` excerpt, `latest.json`,
`i18n-sample.json`).

One file `json.rs` for `sse-codecs`, no dependencies:

```rust
pub enum Event<'a> { ObjectStart, ObjectEnd, ArrayStart, ArrayEnd, Key(Text<'a>), String(Text<'a>),
                     Number(&'a str), Bool(bool), Null }
pub struct Reader<'a> { /* over &'a [u8] */ }
impl<'a> Reader<'a> { pub fn new(input: &'a [u8]) -> Self; pub fn next(&mut self) -> sse_core::Result<Option<Event<'a>>>;
                      pub fn skip_value(&mut self) -> sse_core::Result<()>; }
pub struct Writer { /* appends to a Vec<u8> */ }
```

- Pull reader, no tree: `Text` borrows from the input when the string has no escapes and decodes into an owned
  buffer only when it has (`\uXXXX` with surrogate pairs, the standard escapes). No allocation per token otherwise.
- Strict RFC 8259: UTF-8 validated, no trailing commas, no comments, a BOM is skipped, nesting limited to 128,
  duplicate keys are passed through. Numbers stay text with helpers `as_i64`, `as_u64`, `as_f64` (correctly rounded
  for up to 17 significant digits; write the conversion yourself).
- `Writer`: objects, arrays, strings with minimal escaping, integers, compact and two-space indented output that is
  stable (the C# editor's files must stay readable by it and ours by the C# editor).
- Tests: the attached files read and rewritten compact → same events; every truncation of one file is an error;
  depth bomb; lone surrogates; a 10 MiB string without allocation when it has no escapes; the classic accept/reject
  cases of the JSON test suite written out by hand (at least 60).

## X5 — ECDSA P-256 signature check (ChatGPT)

Attachments: `UpdateSignature.cs`, the public key (`update-public-key.pem`), `latest.json` + `latest.json.sig` of
release 1.3.1, `sha256.rs`, `cursor.rs`, `error.rs`, `lints.toml`.

One file `p256.rs` for `sse-codecs`: verification only, no secrets, so constant time is not required — correctness
is.

```rust
pub struct PublicKey { /* affine point */ }
impl PublicKey { pub fn from_pem(text: &str) -> sse_core::Result<Self>;      // SubjectPublicKeyInfo, uncompressed point
                 pub fn verify(&self, message_sha256: &[u8; 32], signature: &[u8]) -> bool; }
```

- The signature encoding is whatever `UpdateSignature.cs` checks (state which: DER `SEQUENCE{r,s}` or raw `r‖s`;
  accept exactly that). Reject `r` or `s` equal to zero or ≥ n, a point not on the curve, the point at infinity,
  non-canonical DER.
- Field and scalar arithmetic on `[u64; 4]` written by hand (Montgomery or Solinas reduction, Jacobian points,
  Shamir's trick for `u1·G + u2·Q`). No `unsafe`, no indexing with `[]` outside fixed-size arrays with constant
  indexes, checked arithmetic or explicit `wrapping_*`/`carrying` helpers.
- Tests: the attached real manifest verifies and any flipped bit of manifest or signature does not; the NIST
  CAVP `SigVer` P-256/SHA-256 vectors (at least 15, typed into the test); the Wycheproof edge cases for
  `ecdsa_secp256r1_sha256` you can reproduce from memory, each named.

## X6 — Fonts: TrueType and CFF outlines to coverage (ChatGPT)

Attachments: `LiberationSansNarrow-Regular.ttf`, `Oswald[wght].ttf`, `cursor.rs`, `error.rs`, `lints.toml`.

One module `font.rs` (+ `raster.rs` if you prefer two files) for the interface crate. No dependencies.

```rust
pub struct Font<'a> { /* borrows the file */ }
impl<'a> Font<'a> {
    pub fn parse(data: &'a [u8], index_in_collection: u32) -> sse_core::Result<Self>;   // .ttf, .otf, .ttc
    pub fn glyph(&self, character: char) -> Option<GlyphId>;                           // cmap formats 4 and 12
    pub fn advance(&self, glyph: GlyphId) -> f32;  pub fn kerning(&self, a: GlyphId, b: GlyphId) -> f32;  // kern table
    pub fn metrics(&self) -> Metrics;                                                   // ascent, descent, line gap, units per em
    pub fn outline(&self, glyph: GlyphId, sink: &mut impl OutlineSink) -> sse_core::Result<()>;
}
pub fn rasterize(outline: &[Segment], width: u32, height: u32, out: &mut [u8]);        // 8-bit coverage, non-zero rule
```

- Outlines: `glyf` (simple and composite glyphs, quadratic) and `CFF ` (Type 2 charstrings with subroutines, cubic) —
  system CJK fonts are CFF inside `.ttc`. Variable fonts: read the default instance and, for `Oswald[wght]`, apply
  `fvar`/`gvar` deltas for one requested weight (if `gvar` is too much for one sitting, say so and deliver without).
- Rasteriser: signed-area accumulation (one pass over edges, one prefix sum), exact coverage, no supersampling.
  Sub-pixel horizontal positioning in quarters of a pixel.
- Every table offset and length is checked; a damaged font is an error, never a panic or an endless loop
  (composite depth ≤ 8, charstring stack and call depth as the specification limits).
- Tests: glyph ids and advances of 20 named characters of the attached fonts (Latin, Cyrillic, punctuation) typed in
  as constants you computed and say how; coverage of a rasterised square and circle within 1/255 of the exact area;
  truncation of the font at 200 evenly spaced lengths never panics.

## X7 — Windows minidump reader (ChatGPT)

Attachments: `CrashDumpReader.cs`, `CrashDumpReaderTests.cs` (its `BuildDump` makes the synthetic dumps: port it into your tests), `cursor.rs`, `error.rs`, `lints.toml`.

One file `minidump.rs`: header and stream directory, streams `SystemInfo`, `Exception` (code, address, thread),
`ModuleList` (name, base, size, timestamp, version), `ThreadList` with the faulting thread's context for x86 and
x64, `MemoryList`/`Memory64List` enough to read the stack. A function that walks the faulting stack for return
addresses inside known modules and returns `module+offset` frames exactly as `CrashDumpReader` does. Everything is a
view over the input slice — no copies of streams. Limits on every count; hostile directory entries (overlapping,
beyond the file) are errors. Tests: dumps built as the C# tests build them, against the values the C# tests assert, truncation at
every 64th byte, a dump claiming 2³² modules.

## X8 — Inflate and PNG decoder (ChatGPT)

Attachments: two icons from the C# editor's asset pack, `cursor.rs`, `error.rs`, `lints.toml`.

Two files: `inflate.rs` (`pub fn inflate_zlib(input: &[u8], maximum: usize) -> Result<Vec<u8>>`: stored, fixed and
dynamic blocks, Adler-32 checked, table-driven Huffman decoding with a two-level table, output limit enforced before
growth) and `png.rs` (`pub fn decode(input) -> Result<Image>` to RGBA8: colour types 0, 2, 3, 4, 6, bit depths 1–16
reduced to 8, `tRNS`, the five filters, Adam7; CRC of every chunk checked; dimensions limited to 16 384). Used by
the build step that bakes the icon atlas and by the previews of S2 saves, so decoding speed matters: say how many
MiB/s you measured or estimate. Tests: the attached icons decode to stated dimensions and to a stated FNV-1a hash of
the pixels (compute it and say how); hand-made streams for each block type; truncation everywhere; a zip bomb stops
at the limit.

## X9 — LZO1X: a real compressor (ChatGPT, after X1)

Attachments: your accepted `lzo1x.rs`, `lzo1x-extended-m4.lzo` / `.raw` (a stream with real matches), `cursor.rs`,
`error.rs`, `lints.toml`.

The C# editor never compressed: it wrote the whole image as one literal run, so an edited save is larger than its
unpacked image. Add to `lzo1x.rs`:

```rust
pub fn compress_fast(payload: &[u8]) -> Vec<u8>;   // real LZO1X-1 class compression
```

keeping `compress` (literal-only) as it is. Any stream is acceptable that the standard LZO1X decoder — the game's —
decodes to `payload`: M1–M4 matches, a hash table of 2^14 entries on the stack or in one allocation, one pass, no
`unsafe`, no `[]` indexing. Output must never exceed `payload.len() + payload.len()/16 + 67`. Tests: round trip
through `decompress` for 2 000 fixed-seed buffers (sizes 0…300 000, repetitive, random, mixed, long zero runs —
saves are full of them); every match distance class (≤ 0x800, ≤ 0x4000, ≤ 0xBFFF) produced at least once (assert by
parsing your own output); ratio on the attached `.raw` no worse than the attached `.lzo`; speed stated.

## X10 — Ogg Vorbis decoder (ChatGPT)

Attachments: three `.ogg` menu sounds of the games, `cursor.rs`, `error.rs`, `lints.toml`.

The C# editor shipped 8.8 MiB of game sounds and decoded them with NVorbis. With our own decoder the editor plays the
sounds **straight from the installed game's archives** and ships none. One module (`ogg.rs` + `vorbis.rs`):
`pub fn decode(input: &[u8], maximum_samples: usize) -> Result<Pcm>` with `Pcm { channels, rate, samples: Vec<i16> }`
interleaved. Ogg pages with CRC checked, Vorbis I identification/comment/setup headers, floor 1 (floor 0 may be
refused with `Error::Refused` — say whether the attached files use it), residue 0/1/2, codebooks with lookup types
1 and 2, channel coupling, inverse MDCT written by you (a straightforward O(n log n) one), windowing and overlap-add.
Tests: the attached files decode to the sample count and an FNV-1a hash of the PCM that you state and explain how
you obtained (a reference decoder you ran, or a careful hand check of the first packet if you cannot run one —
say which); truncation at 100 points; a header claiming 2³¹ codebook entries is refused before allocating.

## X11 — X11 wire protocol (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`.

Pure encoding and decoding of the X11 core protocol, MIT-SHM and XKB core-keymap requests our window needs —
**no socket and no `unsafe` in your code**: the transport is `trait Transport { fn send(&mut self, &[u8]); fn
receive(&mut self, &mut [u8]) }`. Deliver: connection setup with `.Xauthority` parsing (MIT-MAGIC-COOKIE-1) and the
setup reply (screens, visuals, depths, maximum request length, BIG-REQUESTS), requests CreateWindow, ChangeProperty
(WM_NAME, _NET_WM_NAME, WM_PROTOCOLS + WM_DELETE_WINDOW, _NET_WM_ICON), MapWindow, ConfigureWindow, CreateGC,
PutImage split to the maximum request length, ShmAttach/ShmPutImage, InternAtom, GetKeyboardMapping, selections
(clipboard copy and paste with INCR for big text), cursor shapes; events Expose (merged into damage rectangles),
ConfigureNotify, Key/Button/Motion, FocusIn/Out, ClientMessage, SelectionRequest/Notify; errors decoded with names.
Keysym → `char` table (Latin-1, Cyrillic, Greek, keypad, function keys) generated by you into a static table.
Tests against byte sequences you derive from the protocol specification, with a scripted fake transport.

## X12 — Wayland client protocol (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`.

Same shape as X11: marshalling only, transport trait that also carries file descriptors as opaque `u32` handles
(the real socket and `memfd` live in our `sse-sys`). Deliver: wire format (header, int/uint/fixed/string/array/
object/new_id/fd), object id allocation, registry binding, `wl_compositor`, `wl_surface` (attach, damage_buffer,
frame callback, commit, buffer scale and `wp_fractional_scale_v1`), `wl_shm` pools, `xdg_wm_base` + `xdg_surface` +
`xdg_toplevel` (configure/ack, close, min size, title), `zxdg_decoration_manager_v1`, `wl_seat` keyboard (keymap fd,
enter/leave/key/modifiers/repeat_info), pointer (enter/motion/button/axis), `wl_data_device` for clipboard text.
Plus an **XKB text keymap parser** (the format the compositor sends): enough of `xkb_keycodes`, `xkb_types`,
`xkb_symbols`, `xkb_compat` to map keycode + modifiers (Shift, Lock, level 3, group switch for Russian) to keysym,
and the keysym → `char` table. Tests: hand-built message bytes both ways; a real-world keymap text you reproduce
from memory of `us,ru` layouts (state how faithful it is).

## X13 — Text layout and search folding (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`, `i18n-sample.json` (strings in 4 languages).

`text.rs` for `sse-ui`, given only glyph advances (`trait Metrics { fn advance(&self, char) -> f32; fn kerning(&self,
char, char) -> f32 }`): line breaking for Latin, Cyrillic, CJK (break between ideographs, kinsoku: no line-start
`、。」）` etc.), Korean by words, Turkish/Polish fine; soft hyphen; hard breaks; width-limited ellipsis in the
middle (for file paths) and at the end; caret positions and hit testing by grapheme cluster (implement the extended
grapheme cluster rules for the scripts above, emoji ZWJ sequences treated as one cluster). Search folding:
case-insensitive and accent-insensitive comparison (`ё`=`е`, Polish/Czech/Turkish diacritics, Turkish dotted I
rules by locale), full-width ↔ half-width digits and Latin, so a search for `ак-74` finds `АК-74`. Tests: tables of
cases per language, at least 80.

## X14 — Raster primitives (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`.

`raster.rs` for `sse-ui` over a premultiplied BGRA `&mut [u32]` with a stride and a clip rectangle: solid and
gradient fill of rectangles with per-corner radius and exact anti-aliased edges, borders of any width, coverage-mask
blit with colour (text), image blit with bilinear scaling and a high-quality downscale (box filter of exact
coverage) for thumbnails, alpha blending in linear-light or sRGB (choose and justify), drop shadow (separable box
blur ×3 approximating Gaussian, cached by size), dimming overlay. Everything clipped, no allocation per call except
the shadow cache. Measure and state throughput for a 1920×1080 frame of mixed content; write the inner loops so the
compiler vectorises them (chunks of 8, no bounds checks inside thanks to slices of exact length). Tests: coverage of
a rounded rectangle against the analytic area within 1/255; clipping at all edges; blending identities.

## X15 — Lua 5.1 parser (ChatGPT)

Attachments: two scripts of our companion mod, `cursor.rs`, `error.rs`, `lints.toml`.

`lua.rs`: lexer and parser of Lua 5.1 (the language of all X-Ray scripts, including long strings/comments with
levels, numeric forms, escapes, `...`, method calls, the 5.1 grammar exactly) into a compact AST in one arena
(indices, not boxes), every node with byte span and line. Errors carry line and column as `luac` reports them.
X-Ray scripts are Windows-1251 text: the lexer works on bytes and never assumes UTF-8. Used by `sse-lint` (G7) for
reference checks and later possibly by our own interpreter. Tests: the attached scripts parse; a table of 60 small
programs with their expected AST printed in a compact S-expression form; every truncation of one script is an
error, never a panic; nesting depth limited (200) against stack exhaustion. State parse speed in MiB/s.

## X16 — Kraken decoder, independent implementation (ChatGPT)

Attachments: `kraken.cpp`, `bits_rev_table.h` (the open `ooz` decoder — a description of the format, not code to
translate), `synthetic-s2.sav` + `synthetic-s2.raw`, `cursor.rs`, `error.rs`, `lints.toml`.

Codex writes the same decoder in the repository; yours is the second, independent one: the two are run against each
other on every save and on mutated streams, and any disagreement is a bug in one of them. `kraken.rs`:
`pub fn decompress_into(source: &[u8], output: &mut [u8]) -> Result<()>` — safe Rust, slices and checked cursors
instead of pointers, every table size, offset and length checked, no scratch allocation larger than 2× one 256 KiB
chunk. Cover what Kraken streams of game saves use: block header, quantum header, memcpy/memset quanta, the entropy
array types (raw, Huffman 2- and 4-way with the new and old code-length coding, RLE, tANS, recursive/multi-array),
LZ runs with the two modes, offset coding with the extended-offset scheme. Mermaid/Selkie/Leviathan/LZNA/Bitknit:
`Error::Refused`. A damaged stream is `Error::Damaged` — never a panic, an out-of-bounds copy or a loop that does
not advance. Say how the container in `synthetic-s2.sav` is framed if you can tell; otherwise test the pieces with
streams you build by hand (memcpy and memset quanta, a raw-entropy LZ block).

## X17 — Window on Windows (ChatGPT)

Attachments: `error.rs`. This file is for `sse-sys`, the one crate where `unsafe` is allowed: declarations of
`user32`/`gdi32`/`dwmapi`/`shcore`/`kernel32` functions **written by hand** (`extern "system"`, no `windows` or
`winapi` crate), every `unsafe` block with a `// SAFETY:` line.

`window_win32.rs`: a window that shows a BGRA frame buffer — `RegisterClassExW`, `CreateWindowExW`, per-monitor-v2
DPI awareness and `WM_DPICHANGED`, message loop that sleeps when idle (`MsgWaitForMultipleObjects` with a wake
event for worker threads), `WM_PAINT` with `SetDIBitsToDevice` of only the damaged rectangles, resize without
flicker (`WM_ERASEBKGND` handled), minimum size, dark title bar (`DwmSetWindowAttribute` 20), window icon from RGBA,
keyboard (`WM_KEYDOWN`, `WM_CHAR` with UTF-16 surrogates), mouse with wheel and capture, cursor shapes, clipboard
text both ways, `RegisterHotKey`, file-open and folder dialogs through `IFileOpenDialog` (COM by hand), high-contrast
and reduced-motion queries. API as a trait `Window { fn present(&mut self, frame, damage); fn next_event(&mut self,
timeout) -> Event; … }` you define. It will be compiled by CI on Windows, not by you: write conservatively and list
every function with its documented signature in a table at the top.

## X18 — Window on macOS (ChatGPT)

Same contract as X17 for `window_macos.rs`: Objective-C runtime by hand (`objc_msgSend`, `sel_registerName`,
`objc_getClass`, a class allocated with `objc_allocateClassPair` for the view and the delegate), `NSApplication`
without a nib, `NSWindow` with a layer-backed view whose layer contents is a `CGImage` made from our BGRA buffer
(or `IOSurface`, justify), backing scale factor changes, events through `nextEventMatchingMask` with a timeout,
key text via `interpretKeyEvents`/`insertText:`, pasteboard text, `NSOpenPanel`, menu bar with Quit/Copy/Paste so
the standard shortcuts work, dark appearance. arm64 and x86_64 calling conventions for `objc_msgSend` noted.

## X19 — Sound output (ChatGPT)

Attachments: `cursor.rs`, `error.rs`, `lints.toml`.

Three small backends behind `trait Output { fn play(&mut self, pcm: &[i16], channels, rate, volume) }`, short UI
sounds only, never blocking the caller: (1) Linux — the **PulseAudio native protocol** over its Unix socket
(PipeWire speaks it too): cookie auth, tagstruct marshalling, `CREATE_PLAYBACK_STREAM`, writes, drain; pure safe
code over a transport trait, with tests on hand-built packets. (2) Windows — `waveOutOpen`/`waveOutWrite`
declarations by hand (for `sse-sys`, `unsafe` with SAFETY lines). (3) macOS — `AudioQueue` by hand. If no server
answers, sound is silently off.

## X20 — HTTPS through the operating system (ChatGPT)

For `sse-sys`. `trait Fetch { fn get(&mut self, url, range_from: u64, sink: &mut dyn FnMut(&[u8]) -> bool) ->
Result<Response> }` with status, content length and redirects followed only to `https`. Windows: WinHTTP by hand
(`WinHttpOpen`, `Connect`, `OpenRequest`, `SendRequest`, `ReceiveResponse`, `QueryHeaders`, `ReadData`), system
proxy settings respected. Linux and macOS: the system's `libcurl` loaded at run time (`dlopen` of `libcurl.so.4` /
`libcurl.4.dylib`, `curl_easy_*` declared by hand, write callback), with a clear error when it is absent. Timeouts,
a size limit, cancellation from the sink. No TLS code of our own, certificate checks never disabled. Also a
`file://` and an in-memory implementation for tests.

## X21 — ZIP and deflate (ChatGPT)

Attachments: your `inflate.rs` if X8 is done (else write inflate here), `cursor.rs`, `error.rs`, `lints.toml`.

`deflate.rs`: compressor with lazy matching, hash chains, dynamic Huffman blocks (length-limited codes by package-
merge or a correct heuristic), levels fast/default; output valid for any inflater and within 5% of zlib level 6 on
text (state what you measured or estimated). `zip.rs`: reader (central directory, ZIP64, stored + deflate, CRC
checked, names in UTF-8 and CP437, **path traversal refused**: absolute paths, `..`, drive letters, symlinks) and a
streaming writer with stable output (fixed timestamps) for reproducible packages. Tests: round trips, hand-made
archives, a zip bomb stopped by the limit, traversal names.

## X22 — Difference of bytes and of lines (ChatGPT)

`diff.rs`: (1) Myers O(ND) line diff with a linear-space variant and a cut-off heuristic, producing unified hunks —
for showing what a game fix changes in a script (Windows-1251 bytes, CRLF/LF preserved exactly); (2) patch
application with context matching that refuses when the context is not unique; (3) byte-range diff of two save
images of equal or different length (changed ranges merged when closer than N bytes) for the Compare screen and for
proving a writer touched nothing else; (4) three-way merge of line edits for scripts patched by two fixes, conflicts
reported, never guessed. Tests: 60 cases incl. empty files, no trailing newline, 1 MiB inputs with a time bound.

## X23 — Numbers, dates, plurals in 15 languages (ChatGPT)

Attachments: `i18n-sample.json`.

`locale.rs` for: ru, uk, en, de, fr, es, it, pl, cs, tr, pt-BR, ja, ko, zh-CN, zh-TW. CLDR plural categories
(cardinal) as functions; number formatting (grouping and decimal separators, non-breaking spaces where the locale
uses them), sizes (КБ/МБ vs KB/MB vs 千字节 as each locale writes them), money with the in-game `RU` suffix, dates
and relative times ("3 минуты назад", "昨天"), list joining ("a, b и c"). Tables typed in by you from CLDR; a test
table of at least 10 cases per language. No allocation for numbers (write into a `&mut String`).

## X24 — Atlas packing (ChatGPT)

`atlas.rs`: pack N rectangles (icons from 16×16 to 512×256) into pages of a maximum size with MaxRects (best short
side fit) and a skyline fallback, optional 1-pixel padding, deterministic output for the same input, duplicates
merged by a caller-supplied pixel hash, incremental add for the glyph cache with eviction of least-recently-used
shelves. Report the fill ratio on a generated set that mimics 600 icons; tests for no overlap and bounds.

## X25 — Vector icons (ChatGPT)

`path.rs` for `sse-ui`: parser of SVG path data (`M L H V C S Q T A Z`, relative forms, implicit repeats, arcs
converted to cubics), stroke to fill (width, butt/round/square caps, miter/round/bevel joins), transforms, flatten
with a tolerance, output as line segments for the coverage rasteriser of X6 (`Segment { from, to }`), even-odd and
non-zero. Plus **28 interface icons as path strings you draw yourself** on a 24×24 grid, one visual style, 2 px
strokes: saves, inventory, stash, map/transitions, factions, backup, compare, timeline, doctor, games, fixes,
wrench, companion, trophy, cloud, book, shield/capabilities, update, settings, search, add, delete, undo, redo,
save, folder, warning, info. Tests for the parser and the arc conversion.

## X26 — Layout engine (ChatGPT)

`layout.rs` for `sse-ui`, pure arithmetic over a tree in an arena: row, column, wrap, grid with fixed/auto/fraction
tracks, stack; per node min/preferred/max size, margin, padding, gap, alignment, grow/shrink factors, aspect ratio,
text nodes measured through a callback (width → height); two passes (measure, arrange), results cached by
constraint so an unchanged subtree costs nothing; pixel snapping at a fractional scale factor without gaps or
overlaps; scroll containers report content size. Tests: 80 layouts with expected rectangles, incl. the three window
widths 940/1260/1920 at scales 1, 1.25, 1.5, 2.

## X27 — Text field model (ChatGPT)

`edit.rs`: the logic of a single-line and a multi-line text field without drawing: buffer (gap buffer or rope —
justify), caret and selection by grapheme cluster, word and line movement, Home/End, selection by mouse (double
click word, triple line), insert/delete/backspace with Ctrl variants, clipboard cut/copy/paste through a trait,
undo/redo with coalescing of typing, maximum length, input filters (digits only with range, for money and counts),
IME composition range as an optional overlay, horizontal scroll to keep the caret visible. Tests as scripts of key
presses with expected text and selection, Cyrillic and CJK included.

## X28 — Virtual list and table model (ChatGPT)

`list.rs`: model for lists of up to a million rows: fixed and variable row heights (prefix sums in a Fenwick tree,
estimated then corrected heights without scroll jumps — scroll anchoring), visible range for a viewport with
overscan, selection (single, Ctrl, Shift ranges, select all) kept stable under sort and filter, sort by several
columns with stable order over an index permutation (the data is never moved), incremental filter, column widths
(fixed, auto by sampled content, fraction, user resize, minimum), keyboard navigation with type-ahead, grouping
with collapsible headers (inventory piles). No allocation while scrolling. Tests as scripts with expected ranges.

## X29 — Regular expressions, linear time (ChatGPT)

Attachments: `CrashSignatureCatalog.cs` (the patterns we must run), `cursor.rs`, `error.rs`, `lints.toml`.

`regex.rs`: the subset the attached patterns use and a little more — literals, classes, `\d \s \w \b`, `.`,
alternation, groups (capturing and not), `* + ? {m,n}` greedy and lazy, anchors, case-insensitive flag (ASCII and
Cyrillic). Compiled to a Thompson NFA and run by a Pike VM or a lazily built DFA: **time linear in the input, no
backtracking**, bounded memory. A `Set` that compiles all catalogue patterns together, extracts their required
literals into one Aho-Corasick automaton as a prefilter, and scans a log stream chunk by chunk (matches across
chunk borders handled). Tests: every attached pattern against a positive and a negative line; classic pathological
patterns (`(a+)+b`) finish in linear time; 50 MiB/s or better stated for the prefiltered scan.

---

## C4 — Checks and crash analysis (Codex)

New crate `sse-doctor` (yours alone). Reference: `Core/Diagnostics/SaveDoctor.cs`, `QuestDoctor.cs`,
`CrashLogAnalyzer.cs`, `CrashLogDiscovery.cs`, `CrashSignatureCatalog.cs` + its data, `CrashDumpReader.cs` (the
reader itself arrives as `sse_codecs::minidump` from X7; until then a trait), `GameDoctor.cs`,
`GameBuildFingerprint.cs`, `EnvironmentDoctor.cs`, `Cli/Program.Diagnostics.cs`; tests under `tests/.../Diagnostics`;
`docs/DIAGNOSTICS.md`.

Deliver:

1. **One rule engine.** A rule is data + one function over the save index: id, severity, what it looked at, what it
   found, and — when a repair is proven — the `ChangeSet` that repairs it. Save Doctor, the command line and the
   pre-write check of the transaction run the same rules; the C# editor had three separate implementations.
   Rules run over the index without building objects, and independent rules run on a scoped thread pool.
2. Save Doctor and Quest Doctor rules of the C# editor, each with its fixture; Quest Doctor repairs through info
   portions exactly where the C# code allows them.
3. Crash logs: discovery in the game folders, the signature catalogue read as is (including `any.*` signatures and
   the repair-installation advice), a matcher that compiles the catalogue once into one automaton instead of
   testing signatures one by one, stack frames from minidumps as `module+offset`.
4. Game Doctor: build fingerprints of the six games, environment checks.
5. `sse-cli`: `doctor save|quests|crash|game` with the C# arguments, output and exit codes.

Better than the reference: all rules on a 5 MiB save in ≤ 30 ms; a 50 MiB crash log matched in one pass with
bounded memory (streamed, not loaded); one implementation instead of three.

## C5 — Steam: cloud and achievements (Codex)

New crate `sse-steam` (yours alone). Reference: everything in `src/StalkerSaveEditor.Steam/`,
`Host/CloudServiceAdapter.cs`, `Host/SteamAchievementsAdapter.cs`, `Core/Storage/SteamLibraryFolderLocator.cs`; tests
under `tests/StalkerSaveEditor.Steam.Tests`; fixtures `cloud-transaction/`.

Deliver:

1. Locating Steam, its libraries and the Auto-Cloud roots (VDF through `sse_codecs::vdf` from X3).
2. The native part behind a trait `SteamApi` with two implementations: the real one calls Steam's own library
   through `sse-sys` (describe in the pull request the exact functions and signatures you need there — Claude adds
   them; you write none of the `unsafe`), and a scripted fake for tests.
3. The worker process: the same executable started with a worker argument, parsed **before** anything else in
   `main`; an unknown worker argument is a usage error; the worker never opens a window, plays a sound or reads
   settings. The protocol between editor and worker is length-prefixed binary frames of our own, not JSON lines.
4. Cloud: list, download, the write transaction of `SteamCloudWriteTransaction` (fresh hash, backup, write,
   read-back, and **no automatic retry of an uncertain write**), Auto-Cloud writer.
5. Achievements: read, set, clear with the confirmations the C# adapter requires.

Better than the reference: the worker is the editor's own binary in a mode that maps no interface code (measure its
memory: ≤ 8 MiB); a hung Steam call cannot hang the editor (timeout + kill, state reported as uncertain).

## C6 — Companion mod: install, protocol, hot keys (Codex)

New crate `sse-companion` (yours alone). Reference: `Core/Companion/*.cs`, `Core/Hotkeys/*.cs`,
`Desktop/Services/CompanionServiceAdapter.cs`, `mods/companion/` (the Lua mod itself is copied unchanged into
`assets/companion/` — do not edit the Lua), `tools/pack_companion_ee.py`, `tools/check_companion.sh`,
`docs/COMPANION.md`; tests under `tests/.../Companion`, `Hotkeys`.

Deliver:

1. Installer for SoC, CS, CoP, the three Enhanced Editions (archive packing as `pack_companion_ee.py` does — in
   Rust) and S.T.A.L.K.E.R. 2 (UE4SS mod folder): journal, exact restore on removal, the previous copy kept until
   the new one is in place, state files readable by the C# editor and vice versa. Installation happens only on an
   explicit call — nothing installs by itself.
2. Hook patcher with the exact-source rule of `CompanionHookPatcher`.
3. Protocol client: commands and replies through the files the mod watches, timeouts, a stale reply is never taken
   for a fresh one.
4. Hot keys: layouts, game-window matching, the Windows backend and the X11 backend. The X11 helper is the same
   executable in a helper mode (same rule as the Steam worker). OS calls go through `sse-sys` (ask Claude for them).

Better than the reference: nothing here ever runs on the interface thread (the C# adapter froze the start for
3–5 s); the helper speaks the X11 wire protocol over the socket itself — no Xlib.

---

## G4 — Finding saves, previews, icon atlas (Gemini)

In `sse-storage` (module `discovery`, agreed with Codex: you own that module only) and `sse-content` (previews,
icons). Reference: `Core/Storage/SaveDirectoryLocator.cs`, `SaveSlotDiscovery.cs`, `Core/Inspection/SavePreviewReader.cs`,
`Desktop/Services/ItemIconService.cs`, `Desktop/Assets/Icons` + `icon-aliases.json` + `PROVENANCE.json`,
`tools/import_catalog_assets.py`.

Deliver:

1. Save discovery for all games and stores (Steam, GOG, Game Pass paths for S2), with the C# rules and fixtures.
2. **Library index**: a file of our own binary layout (path, size, time, hash of the header, format, the few facts
   the list shows) so the list of several hundred saves is on screen without opening one save; entries are
   revalidated by size + time and re-read in the background by a bounded pool. Corrupt index → rebuilt, never fatal.
3. Previews: X-Ray preview images (DDS next to the save) and S2 thumbnails to RGBA at the list's size, bounded cache
   with a stated memory limit.
4. Icon atlas: a build step (`build.rs` or `xtask`-style binary inside the crate) that reads the PNG icons
   (`sse_codecs::png` from X8), removes duplicates by pixel hash, packs them into atlas pages of our own format
   (pixels compressed with our LZO), and a run-time reader that maps a name (with aliases) to a rectangle and
   decodes only the page asked for. Game atlases (`ui_icon_equipment.dds`) cut through G1's DDS.

Better than the reference: icon pack ≤ 3.5 MiB (was 7); idle memory of the icon service ≤ 4 MiB; 333 saves listed
in ≤ 100 ms with a warm index.

## G5 — Updates (Gemini)

New crate `sse-update` (yours alone). Reference: `src/StalkerSaveEditor.Updater/*.cs`, `Host/UpdateServiceAdapter.cs`,
`tools/release/publish_release.py`, `docs/PACKAGING.md`; tests under `tests/.../Updater`.

Deliver:

1. Manifest `latest.json` read with `sse_codecs::json`; signature checked with `sse_codecs::p256` (X5) against the
   embedded public key. **The format and the key do not change**: editor 1.3.x must update into 2.0 through this
   channel and 2.0 must read what `publish_release.py` writes today.
2. Detection of how this copy was installed (deb, AppImage, portable, Windows installer, macOS bundle) as
   `UpdateInstallationDetector`.
3. Download through a trait `Fetch` (the real implementation is the operating system's HTTP stack in `sse-sys`;
   describe what you need, Claude writes it): resumable, size and SHA-256 checked while streaming, never held in
   memory whole.
4. Install per platform with the result shown, as release 1.3.1 does (Linux: no silent quit).
5. `sse-cli`: `update check|download`.

Better than the reference: a package is verified while it downloads (one pass, constant memory); a manifest with a
valid signature but an older version is refused (downgrade protection — check what C# does and report).

## G6 — Game environment and packages (Gemini)

Module `toolkit` in `sse-fixes` and the folder `packaging/`. Reference: `Core/Diagnostics/ToolkitInstallAudit.cs`,
`Desktop/ViewModels/ToolkitEnvironmentViewModel.cs` (logic only), `Stalker2ModToggle.cs`, `docs/PACKAGING.md`,
`.github/workflows/release-packages.yml`, `packaging/` of the C# repository.

Deliver:

1. Environment of an installation: snapshots, profiles, the managed `user.ltx`, the install audit, S2 mod toggle.
   Every function takes the installation explicitly — no "current installation" state (the C# screen wrote the
   `user.ltx` of the previously selected game).
2. Packages built by scripts in the repository with nothing downloaded at build time: `.deb`, AppImage, Windows
   installer + portable zip, macOS `.dmg`. Sounds are a separate optional package.
3. Size gates in CI: the job fails when a package exceeds the budget of `PLAN.md`.

Better than the reference: package sizes; build of all packages from a clean checkout in ≤ 5 minutes.

## G7 — Checks of game scripts (Gemini)

New crate `sse-lint` (yours alone). Reference: `tools/check_condlists.py`, `check_condfuncs.py`, `check_dialogs.py`,
`check_infos.py`, `check_logic_refs.py`, `check_module_calls.py`, `check_trade_items.py`, `lua_globals.py`,
`spawn_diff.py`, `ee_diff.py`, `tools/fix_regress.py`, `fix_realcheck.sh`.

Deliver the checkers as a library over `sse-content`'s file tree (so they see a game exactly as it runs: loose
files over archives) with a small Lua **lexer** of our own for the reference checks (not an interpreter), and
`sse-cli lint <game folder>`. The same library answers in the editor: "what do the installed mods break". The
fix regression (`fix_regress`) becomes a test that builds original and patched trees with `sse-fixes` and runs the
checkers on both.

Better than the reference: a whole game checked in ≤ 2 s (the Python tools take minutes); usable by players, not
only by us.

---

## U0 — Interface toolkit decision (Claude) — done

Measured Slint, egui and our own; the numbers and the choice (our own `sse-ui`) are in `PLAN.md`.

## U1 — `sse-ui` (Claude)

Retained tree of widgets with damage tracking (nothing is drawn when nothing changed), software canvas (rectangles,
rounded corners, images, 8-bit coverage text from X6's font code, glyph atlas with a stated limit), layout (rows,
columns, grid, wrap), virtual list and table as the only lists, scroll, text field with selection and clipboard,
buttons, check boxes, sliders, drop-downs, tabs, tooltips, dialogs that never block, keyboard focus, the theme of
the C# editor (colours, Oswald headings, accents), interface scale. Window trait with the X11 backend (wire
protocol, shared-memory images). A screenshot binary (developer-only, silent) that renders any screen to a file
without a window: this is how screens are accepted and tested in CI.

U2–U5 (window on Windows, Wayland, macOS, canvas on the web) and S1–S5 (screens) get their texts when U1 is merged:
they are written against its widget set.

## Экраны S1–S5: каркас

Каркас лежит в `crates/sse-ui/src/screens/`:
- `mod.rs` — `ScreenId` (20 экранов как в меню C#), трейт `Screen`, `AppMessage`;
- `shell.rs` — рамка;
- `style.rs` — палитра и готовые блоки: `card`, `row`, `label`, `button`.

У каждого пакета свой файл, чужие файлы не трогать:

| Пакет | Файл |
|---|---|
| S2 | `saves.rs` |
| S3 | `history.rs` |
| S4 | `games.rs` |
| S5 | `services.rs` |
| S1 | `app.rs` |

Заглушку `Placeholder` замени своим типом, реализующим `Screen`. Образец — `Settings` в `app.rs`:
- виджеты строятся один раз, их id хранятся в полях;
- клики приходят в `message`;
- медленную работу делай в потоке, результат возвращай через `cx.proxy` → `AppMessage::ToScreen(id, Box::new(..))`.

Снимок экрана: `sse-shell --screenshot out.png 1280x860 N`, где N — номер в `ScreenId::ALL`.

## План до конца проекта (2026-10-05)

Claude уходит на несколько дней. Агенты берут задачи отсюда по порядку, без ожидания. Каждый пункт — отдельная
ветка `wp/<id>` и PR в `main` от свежего `main`; следующий пункт — только после вливания предыдущего или от
`main`, если пункты не пересекаются. В PR: чек-лист раздела `ACCEPTANCE.md`, что проверено и что нет.
Владелец вливает PR, если CI зелёный на трёх ОС. Правила — `AGENTS.md`.

Общие правила, которые чаще всего нарушали:
- действие только при `clicked.is_some()`; запись — только с подтверждением;
- ничего медленного в потоке интерфейса;
- проверять запись против эталона (C# CLI 1.3.1, ooz), а не только своим декодером;
- не открывать несколько PR, правящих одни и те же файлы: вливаемость важнее скорости.

### Кодекс — сейвы, запись, ядро, sse-sys

- K1. **Один PR вместо #158–#161** (и `wp/p0-backup-path-containment`): проверка до записи (побайтно вне
  changed_ranges + смысловая), S2 владение handle и прочность, блокировка записи и закрытия окна, черновики по
  SHA, папка бэкапов через canonicalize. Свести с текущим API транзакции (сводка + проверка после записи) так,
  чтобы остались ВСЕ защиты. После вливания — закрыть #158–#161.
- K2. Один путь данных везде, включая CLI: `sse_app::paths` + `settings.backup_directory` +
  `STALKER_SAVE_EDITOR_DATA` (сейчас CLI пишет бэкапы в `~/.local/share/...` всегда).
- K3. X-Ray запись по XRAY-WRITER-REVIEW: добавление (сброс story_id/spawn_story_id/name_replace/custom data
  в клоне, шаблон той же секции/clsid, отказ `0xFFFF`), прочность (отказ без доказанной позиции UPDATE),
  размещение (занятость слота, пояс), удаление (отказ при story_id ≠ −1), обратное чтение вне ожидаемых диапазонов.
- K4. Инвентарь §3 и «Добавить предмет» §4 в окне, затем Фракции §5, Тайники §6 (X-Ray), Переходы §7,
  Бэкапы §8 с «Восстановить на место», Библиотека §1.5, баннер «Файл изменён» §1.3, верхняя панель с
  Отменить/Вернуть/Сбросить/Открыть…/Обновить/СОХРАНИТЬ и Ctrl+Z/Y/S.
- K5. Steam FFI по `ACCEPTANCE.md` Часть IV (в libsteam_api владельца нет `SteamAPI_Init` — `SteamAPI_InitFlat`;
  RemoteStorage v016, UserStats v013), рабочий процесс, поле `stage` в ошибках; подключить облако трилогии и
  достижения. Запись в облако на реальном Steam — только после подтверждения владельца.
- K6. sse-sys: `output/windows.rs` — `waveOutReset` до освобождения буфера; `window_win32.rs` — состояние без
  алиасинга `Box`; `// SAFETY:` у каждого unsafe.
- K7. Доктор сохранения §11 (SaveDoctor, QuestDoctor, 7 квестов, ремонт через общий путь записи).
- K8. X35: размер ≤ ×1,15 к файлам игры, скорость ≥ 50 МиБ/с (корректность — 23 вектора, 2000 буферов).
- K9. Окно Wayland; один exe с режимами (Часть III §1); горячие клавиши компаньона (Часть III §5.4);
  протокол компаньона (ping/info/list_inventory/give/S2).
- K10. Диагностика (Часть II §13, без отправки), бюджеты в CI (старт ≤ 0,2 с, простой ≤ 30 МБ, смена экрана
  ≤ 16 мс, большой сейв ≤ 0,5 с), settings.json совместимый с C#, фаззинг новых форматов.
- K11. Веб-версия заново от main (без цикла зависимостей: sse-sys не зависит от sse-web).

### ChatGPT — экраны вне сейвов, кодеки, упаковка

- G1. #120 переводы — перебазировать и довести: все тексты через `strings::t()`, дополнение каталога на 15 языков.
- G2. Подсказки (tooltip, 500 мс, причины недоступности дословно), звуки интерфейса из файлов игры, анимации
  (в простое 0 кадров).
- G3. Тексты кнопок и статусов дословно как в C# на всех своих экранах; ↑/↓ не листают при фокусе в поле.
- G4. Эталонные снимки (golden) всех 20 экранов, тест с допуском.
- G5. Упаковка: иконки, .desktop + AppStream, Info.plist, ресурсы Windows; tools/package.sh. Подпись не делать.
- G6. Подключение к API Кодекса по мере появления: «В сохранение»/«В игру» в Энциклопедии, команды и инспектор
  Компаньона, кнопки верхней панели — не делать фальшивых действий, пока API нет.

### Владелец

- Проверка в игре по INGAME-TESTPLAN (сначала деньги и пачки во всех играх и S2 — записанное Rust).
- Вливание PR с зелёным CI; ревизии вторым Claude по архиву исходников.

### Состояние на 2026-10-05 (вечер)

Влиты K1 (#171) и G1–G6 (#120). Ревизия второго Claude (FINAL-REVIEW, main после #170) дала новые пункты ниже;
часть из них K1 мог уже закрыть — перед началом пункта проверить по коду и написать в PR, что было закрыто раньше.

### Кодекс — новые пункты (после K2, до K4)

- K12. **P0** X-Ray клон без сюжетных полей: разобрать в STATE `story_id`, `spawn_story_id`, custom data, в SPAWN
  `name_replace`, spawn id; в `clone_template_record` сбрасывать их (значения «нет» — как в C# `XRayAddWriter`);
  сверять в обратном чтении; тест «шаблон со story_id → клон без». Пока не готово — окно отказывает в добавлении,
  если у шаблона есть story_id.
- K13. X-Ray правки из окна: прочность — отказ без доказанной позиции UPDATE и сверка UPDATE; размещение — отказ
  при занятом слоте/нет места на поясе; удаление — отказ при story_id; CLI `allocate_object_id` и писатель —
  отказ `0xFFFF`; явный `TEMPLATE:KEY` — только тот же раздел. (Это K3, подробнее.)
- K14. Закрытие окна во время записи (если K1 не закрыл): не выходить при `is_saving()`, закрыть после
  `SaveFinished`; при выходе дождаться `draft-save`/`draft-reset` (≤ 2 с).
- K15. S2: запасной поток — заголовок `CC 06` на каждые 0x40000 байт + тест образа > 256 КиБ; `SetStackCount` —
  только handle из `owned_handles`; прочность оружия — единственный кандидат во всём окне; побайтная проверка вне
  `changed_ranges` до упаковки; `can_write()` = false для 1.0.x.
- K16. `GameFixEngine::update` одной транзакцией: при ошибке — откат к старой версии файлов и манифеста; тест
  с отказом на N-м файле.
- K17. Компаньон: отказ поверх файлов активных Game Fix; `bind_stalker.script`/`ui_main_menu.script` читать из
  архивов игры через `sse-content`, если распакованных нет.
- K18. Черновики: удаление старого черновика после сохранения — без проверки поколения (`saves.rs` persist_drafts);
  расширенные поля (durability/placements/upgrades/s2StashTakes) — только в схеме 3, схему 2 C# должен читать;
  «Восстановить на место» — отказ при `is_saving()`.
- K19. Подключить готовые писатели в окно (после K12–K13): Фракции §5 (`SetFactionRelation`), Тайники X-Ray §6
  (`MoveItem`), Перенос §7 (`RelocateActor`, с подтверждением), ремонт квестов §11.
- K2 уточнение: одна функция `sse_app::paths::backup_directory(&settings)`; убрать три копии
  `default_backup_directory` (saves.rs, history.rs, sse-cli); поле «Папка бэкапов» в Настройках должно работать.

### ChatGPT — новые пункты

- G7. «Открыть…»: системный выбор файла через `sse-sys` (Windows `IFileOpenDialog`, Linux portal/zenity, macOS
  `NSOpenPanel`) или убрать кнопку. Сейчас заглушка в `shell.rs`.
- G8. Облако «ЗАПИСАТЬ» (до Steam FFI): локальный файл — пара облачного по имени (ACCEPTANCE-HOST §2.2), а не
  текущий сейв; бэкапы — в `<данные>/backups`, не в папку сейва.
- G9. Настройки: один поток-владелец записи `settings.json` (последнее значение побеждает) вместо `load→save` в
  каждом потоке.
- G10. Среда игры: подтверждение удаления снимка и профиля; восстановление снимка и применение профиля — в фоне.
- G11. Доделать за Gemini: `sse-lint` чистый на `screens/games.rs` (ветка Gemini `wp/g7e-lint-clean-games` не
  опубликована — делать заново от main).
- G3 остаток: убрать из интерфейса служебные английские слова (`Shell`, `file-picker`, `AppState`).
- Golden-тест зависит от домашней папки (экран «Обзор» видит настоящие сейвы): изолировать HOME/данные в тесте.

### Владелец — добавлено

- После K12: в игре добавление квестового и обычного предмета (INGAME-TESTPLAN §4) на ТЧ/ЧН/ЗП.
- После K15: S2 деньги, пачки, прочность в игре.

### Состояние на 2026-10-05 (ночь) — после K1-CHECK

Влиты: K1 #171, K2 #180, G1–G6 #120, G7–G11 и остаток G3 #173–#178, изоляция golden #179.
K1 уже закрыл: закрытие окна во время записи, проверку до записи, S2 владение/прочность/«только ожидаемое»,
`can_write()` для 1.0.x. Поэтому K14 и K15 сужены (ниже).

**Кодекс — порядок дальше:** K12 (P0) → K13 → K15 → K18 → K19 → K4 → K5 → K6 → K7 → K8 → K10.
- K14 (сужено, P2): откладывать закрытие окна и во время «Восстановить на место» (общий флаг занятости для
  записи и восстановления); при закрытии дождаться `draft-save`/`draft-reset` (≤ 2 с).
- K15 (сужено): только запасной поток S2 — `CC 06` на каждые 0x40000 байт (`sse-s2/src/lib.rs` около :2058) +
  тест образа > 256 КиБ с принудительным запасным путём.
- K18 дополнение: одна `Arc<[u8]>` на `packed`/`preflight_image`/`readback_image` в `commit_save_edits_to`
  (сейчас до ~5 копий образа S2); восстановление при `is_saving()` — отказ, и наоборот.
- K13 дополнение: X-Ray пересобранные чанки (OBJECT, отношения) помечаются изменёнными целиком — для клона,
  удаления и апгрейдов добавить сверку полей записей в обратном чтении (иначе P0-1 не ловится проверкой).

**ChatGPT — забирает у Кодекса то, что не трогает сейвы:**
- G12. Таблица перевода отказов писателей на русский (как в C#): ни одна английская строка из sse-s2/sse-xray/
  sse-storage не доходит до статуса окна. Проверить, что окно не предлагает править пачки S2 вне сетки рюкзака.
- G13 (= K16). `GameFixEngine::update` одной транзакцией, откат к старой версии; тест отказа на N-м файле.
- G14 (= K17). Компаньон: отказ поверх файлов активных Game Fix; скрипты из архивов игры через `sse-content`.
- G15 (= K14 сужено). Только если Кодекс ещё не взял: закрытие окна во время восстановления + ожидание черновиков.
  Перед началом проверить, нет ли открытого PR Кодекса на `event_loop.rs`/`history.rs`.
- G16 (= K9). Окно Wayland; один exe с режимами (Часть III §1); горячие клавиши и протокол компаньона.
- G17 (= K11). Веб-версия заново от main, без цикла зависимостей.
- G18. Бюджеты в CI из K10 (старт ≤ 0,2 с, простой ≤ 30 МБ, смена экрана ≤ 16 мс), фаззинг новых форматов.

### Состояние на 2026-10-05 (поздно) — после G12–G18 и K12/K13/K15

Влиты G12–G18 (#182–#191), K12/K13/K15. main был сломан (G16–G18 влиты без CI: ошибки компиляции, ci.yml) —
починено в #192. **G16 Wayland удалён**: он тянул крейт `minifb` и 81 зависимость с crates.io — нарушение
правила «только std». Режим `--companion` в одном exe оставлен.
Проверено на сейвах владельца (копии): чтение = C# 331/333; деньги X-Ray побайтно = C# 74/74; добавление
предмета X-Ray (явный шаблон) — C# видит +1 предмет 67/67. В игре — не проверено.

**Правило для всех:** PR без зелёного CI на трёх ОС не вливать. Если CI не запустился — значит сломан ci.yml или
сборка, а не «GitHub не создал run». Никаких зависимостей вне workspace (Cargo.lock без `source = "registry`).

**Кодекс дальше:** K18 → K19 → K4 → K5 → K6 → K7 → K8 → K10 (диагностика, settings.json как в C#).
- K12 доделка: CLI `--add KEY=N` без шаблона отказывает при нескольких кандидатах; выбирать сам по K12-SPEC §5
  (без story_id/spawn_story_id/custom data, затем любой той же секции).

**ChatGPT дальше:**
- G16 заново: Wayland без крейтов — свой клиент wire-протокола по unix-сокету (`wl_compositor`, `xdg_shell`,
  `wl_shm` через memfd/mmap в `sse-sys`, `wl_seat` для мыши/клавиатуры), иначе X11/XWayland как сейчас.
- G19. Скорость смены экрана: сейчас 40–60 мс в release при бюджете 16 мс (`sse-shell --ci-budget`). Найти, что
  перестраивается при `Shell::open`, кэшировать; после — вернуть шаг бюджета в CI блокирующим (убрать
  `continue-on-error`).
- G20. Простой ≤ 30 МБ и старт ≤ 0,2 с — замерить тем же `--ci-budget` и довести.
