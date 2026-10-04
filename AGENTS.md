# Rules of this repository

You are one of several agents working in parallel. Claude integrates: it owns `sse-core`, `Cargo.toml`, CI,
`AGENTS.md`, `PLAN.md`, `TASKS.md` and `ACCEPTANCE.md`, reviews and merges every pull request. Read `PLAN.md`, then
your work package in `TASKS.md`. Any screen work: the screen's section of `ACCEPTANCE.md` (C# 1.3.1 behaviour, exact
texts, checklist) is the acceptance test; §23 lists every disk/network write and its guards, §24 the C# defects not
to copy blindly. Five documents only: README, AGENTS, PLAN, TASKS, ACCEPTANCE — no new ones.

## Boundaries

- Work only inside the crate(s) your package names. Need something in `sse-core` or in another owner's crate?
  Describe it in the pull request; do not edit it.
- One package = one branch `wp/<id>-<slug>` = one pull request into `main`. Small follow-ups are new pull requests.
- **No third-party code.** No crates, no C or C++: only `std`. What we need we write (codecs, parsers, hashes,
  signature check, image and font code). Calls into the operating system or into Steam's own library live in
  `sse-sys` (Claude's crate, the only one where `unsafe` is allowed): ask for the call you need in the pull request.
  No `async` runtime anywhere.
- **Better, not a copy.** The C# editor is the floor: what is read from a save and what bytes are written must
  match it, because the game is the judge. Structure, speed, memory and checks are designed anew. Each package names
  what must be better and how it is measured; a line-by-line port is rejected. Where the C# code is wrong, do not
  copy the mistake: show the proof in the pull request and Claude records the difference in `PLAN.md`.
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

- Tests never read the machine's real home, saves or games. Fixtures live in `fixtures/`; personal saves are never
  committed.
- Parity first: `fixtures/synthetic` has source/expected pairs and vectors from the C# editor; a port is done when it
  reproduces them byte for byte and `tools/oracle.sh` prints no difference.
- Every reader gets a damaged-input test (truncated, bit-flipped, hostile lengths) and a fuzz target.
- Paths are compared after resolving links (macOS: `/var` is `/private/var`); no assumptions about separators or case.

## Process

- A child process that is this same executable (Steam worker, hotkey helper) is dispatched before anything touches
  the interface, and an unknown worker argument exits with a usage error.
- Developer-only modes (screenshots, measurements) are separate binaries, silent, and not shipped.
- Interface text: Russian source strings as keys, 14 translations, a CI check that none is missing. No control that
  does nothing.
- Report honestly in the pull request: what was run, on what, and what was not checked.
