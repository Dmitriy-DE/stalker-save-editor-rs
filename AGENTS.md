# Rules of this repository

This repository has shared code ownership: no crate, file or tool is reserved to a particular agent. The local Claude
integrates: it reviews and merges every pull request (owner's decision). `AGENTS.md`, the "Как работаем" section of
`TASKS.md`, the required CI checks and branch protection change only by the owner's decision. Read `PLAN.md`,
the relevant package in `TASKS.md`, and the matching section of `ACCEPTANCE.md` before changing behavior. The C# 1.3.1
contract remains the reference for exact save bytes and established user-visible behavior. The five root documents
README, AGENTS, PLAN, TASKS and ACCEPTANCE are the only project documents to add or update; do not create more `.md`
files.

## Boundaries

- Change any crate or project file required by the task, including `sse-ui`, `.github/workflows/ci.yml` and `tools/`.
- One task = one branch `wp/<id>-<slug>` from fresh `origin/main` and one pull request into `main`. Never merge your
  own pull request. Do not open a duplicate for the same task; a separately numbered follow-up after a merge gets its
  own branch and PR.
- **No third-party code.** No crates, no C or C++: only `std`. What we need we write (codecs, parsers, hashes,
  signature check, image and font code). Calls into the operating system or Steam's own library live in
  `sse-sys` (the only crate where `unsafe` is allowed).
  No `async` runtime anywhere.
- **Better, not a copy.** The C# editor is the floor: what is read from a save and what bytes are written must
  match it, because the game is the judge. Structure, speed, memory and checks are designed anew. Each task names what
  must be better and how it is measured; a line-by-line port is rejected. Where the C# code is wrong, do not copy the
  mistake: show the proof in the pull request and record the difference in `PLAN.md`.
- No new documents. What a reader must know goes into rustdoc; state goes into the pull request text.

## Correctness (each rule is a bug the C# editor shipped)

1. **Unknown stays read-only.** No write from a guessed offset, length or meaning. A writer changes only bytes whose
   meaning a fixture or a game run proves, and proves it changed nothing else.
2. **A search must be unique.** Code that finds data by scanning accepts a position only when exactly one position
   fits and everything around it agrees. (The level-changer reader took "the first offset that parses" and read
   `arbage` instead of `L02_Garbage`, with numbers from the wrong bytes.)
3. **Lossless.** Reading a save and writing it back unchanged gives the same bytes. Bytes a reader does not
   understand are kept, never rebuilt from a model.
4. **Checked everything.** `sse_core::Cursor` for reads, `checked_*` for arithmetic, `get()` for indexing. The lints
   deny the rest. A damaged file gives `Error::Damaged`, never a panic.
5. **Limits.** Every length read from a file is compared with a stated maximum before memory is allocated.
6. **Text encodings are explicit** (Windows-1251 in X-Ray, UTF-8/UTF-16 in S2). Identifiers may have capital letters.
7. **The write transaction is one piece**: fresh hash of the source, backup with a journal, durable temp file, atomic
   replace, read-back of the result. Nothing skips a step for speed and nothing retries an uncertain write.

## Memory and speed (the reason this rewrite exists)

- One owned buffer per save image (`SaveBuffer`). No copy "to be safe": borrow. A writer produces one new buffer.
- The model of a save is the buffer plus an index of offsets; values are decoded when asked for.
- Caches are bounded and say what they evict. No global mutable state.
- Nothing slow runs on the interface thread: no file, archive, hash, codec or process work. (The C# editor froze for
  3–5 s at every start because an `async` method did its work before its first real await.)
- Budgets in `PLAN.md` are CI gates, not wishes.

## Tests

- Automated tests and developer tools use a temporary `STALKER_SAVE_EDITOR_DATA` root, including logs. Confirm they
  leave the real application-data tree untouched. Fixtures live in `fixtures/`; personal saves are never committed.
- Live game and Steam tests are allowed when authorized for the task. Make a copy of each save or original game/cloud
  state first, use only the copy, restore the original state, and record exactly what was loaded or changed. Synthetic
  round-trips do not prove that a game accepts a mutation.
- Parity first: `fixtures/synthetic` has source/expected pairs and vectors from the C# editor; a port is done when it
  reproduces them byte for byte and `tools/oracle.sh` prints no difference.
- Every reader gets a damaged-input test (truncated, bit-flipped, hostile lengths) and a fuzz target.
- Paths are compared after resolving links (macOS: `/var` is `/private/var`); no assumptions about separators or case.

## Process

- A child process that is this same executable (Steam worker, hotkey helper) is dispatched before anything touches
  the interface, and an unknown worker argument exits with a usage error.
- Developer-only modes (screenshots, measurements) are separate binaries, silent, and not shipped.
- Interface text: Russian source strings as keys, 15 translations, a CI check that none is missing. No control that
  does nothing.
- Report honestly in the pull request: what was run, on what, and what was not checked.
