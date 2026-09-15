# Chitrakar plugin for Claude Code

Packages this project's workflow so any Claude Code session — on your
machine, in the cloud, or a teammate's — works the same way.

## What's inside

| Component | What it does |
|---|---|
| `/chitrakar:verify` | Runs the whole gate: `cargo fmt --check`, clippy with `-D warnings`, the engine tests, then the UI build and the browser smoke suite. Reports pass/fail per step. |
| `/chitrakar:status` | Reads `docs/PLAN.md` §0 plus git state and says where the project stands and what's next. |
| `/chitrakar:ship` | Verify → update the docs → commit → push. Refuses to commit past a failing gate. |
| `engine-conventions` skill | The invariants that keep the engine coherent: invertible commands, the gesture API, dirty-region rules, additive serde for file compatibility, the CPU renderer as correctness reference. Loads automatically when working under `core/`. |
| SessionStart hook | Prints the live branch, whether the tree is dirty, and the last commit — the dynamic state `CLAUDE.md` can't carry. |

## Install

From a clone of this repo (or anywhere, once it's pushed):

```
/plugin marketplace add SandeepSubba/Chitrakar
/plugin install chitrakar@chitrakar
```

## Why a plugin and not just CLAUDE.md

`CLAUDE.md` carries knowledge; a plugin carries *behavior*. The commands make
verification a single step no one skips, the skill loads its detail only when
engine code is actually being edited (rather than costing tokens every turn),
and the hook reports state that changes between sessions.
