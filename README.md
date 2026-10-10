# S.T.A.L.K.E.R. Save Editor — Rust

Second generation of the save editor: one native application without a managed runtime. The released C# editor
(`Dmitriy-DE/S.T.A.L.K.E.R.-Save-Editor`, 1.3.x) stays the reference: every reader and writer here must answer
exactly like it before it replaces it.

## Current status — 2026-10-10

- A18, A10, A14, A16 and A17 implementation PRs are merged: #335/#503, #336, #456, #466 and #474/#477.
- A15 game and companion live checks remain incomplete. The desktop-control API currently exposes no native apps
  (`apps=[]`); no new game session or in-game load was run. Per-game preflight and the exact blocker are in `TASKS.md`.
- A13-56–58 translation fixes are merged in #514; all applicable CI checks passed, including Windows installer and
  portable builds.
- A20 Linux playback merged in #512; the installed-game audio-source follow-up is open in #519. A22/S2 follow-ups
  for download error text and refused stash items are open in #520. D2 S11 is open in #517, which currently reports
  no CI checks.
- Rust 2.0 has not been released. Workshop publication and release signing remain owner actions.

| File | What it is |
|---|---|
| `AGENTS.md` | rules for everyone who writes code here (people and agents) |
| `PLAN.md` | architecture, budgets, implementation stages, current status link |
| `TASKS.md` | work packages as ready-to-paste task texts |

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
SSE_ORACLE=/path/to/stalker-save-editor-cli tools/oracle.sh        # parity with the C# editor
cargo run -p sse-ui --bin sse-ui-dev -- --metrics-report <dir>   # table from downloaded metrics/*.json (download with wrangler first)
```
