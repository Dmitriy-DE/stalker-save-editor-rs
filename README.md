# S.T.A.L.K.E.R. Save Editor — Rust

Second generation of the save editor: one native application without a managed runtime. The released C# editor
(`Dmitriy-DE/S.T.A.L.K.E.R.-Save-Editor`, 1.3.x) stays the reference: every reader and writer here must answer
exactly like it before it replaces it.

| File | What it is |
|---|---|
| `AGENTS.md` | rules for everyone who writes code here (people and agents) |
| `PLAN.md` | architecture, budgets, phases, who owns what, state of each work package |
| `TASKS.md` | work packages as ready-to-paste task texts |

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
SSE_ORACLE=/path/to/stalker-save-editor-cli tools/oracle.sh        # parity with the C# editor
```
