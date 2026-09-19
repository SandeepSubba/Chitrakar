---
description: Verify, commit, and push a chunk of Chitrakar work — refuses to ship if the gate fails
---

Ship the current work. The gate is not optional: nothing gets committed
until it is green.

1. **Verify.** Run the full gate (as `/chitrakar:verify` does): `cargo fmt
   --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo
   test --workspace`, and in `app/`: `npm run build && npm run test:e2e`.
   Rebuild `app/` whenever engine or UI code changed — a stale bundle means
   the browser suite tested the previous build.
2. **Stop if anything fails.** Report what failed and fix it, or explain why
   it cannot be fixed now. Do not commit past a red gate.
3. **Update the docs the work touched:** `docs/PLAN.md` ✅ marks for
   completed roadmap items, and §0 "Where things stand" so it matches
   reality — what now works, and what the next item is.
4. **Commit** with a message that explains *why* the change is shaped the
   way it is, not just what changed. Note any deliberate limitation and any
   test that proves the interesting behavior.
5. **Push** to the active feature branch with `git push -u origin <branch>`.

Anything the user passed as arguments describes what was built — use it to
inform the commit message: $ARGUMENTS
