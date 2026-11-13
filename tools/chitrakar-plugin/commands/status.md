---
description: Where Chitrakar stands — current state, branch, recent work, and what's next
---

Orient in this project and summarize for someone picking it up right now.

1. Read `docs/PLAN.md` section §0 "Where things stand" (the handoff block at
   the top — read just that section, not the whole roadmap).
2. Run `git status --short --branch` and `git log --oneline -8`.

Then report, briefly:

- The branch, and whether the working tree is clean.
- What works today (from §0 — condense, don't paste it wholesale).
- What the last few commits did.
- The next item on the priority list, and anything in §0's "known limits"
  that bears on it.

Keep it under fifteen lines. If the working tree is dirty or §0 looks stale
against the recent commits, say so — a drifted handoff block is worth
fixing before more work lands on top of it.
