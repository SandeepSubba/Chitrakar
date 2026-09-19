#!/usr/bin/env bash
# Surfaces the live repo state a session cannot get from CLAUDE.md:
# which branch, whether work is uncommitted, and what landed last.
# Stays silent outside a git repo so it is harmless anywhere else.
set -uo pipefail

git rev-parse --is-inside-work-tree >/dev/null 2>&1 || exit 0
branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null) || exit 0
dirty=$(git status --porcelain 2>/dev/null | wc -l | tr -d ' ')
last=$(git log --oneline -1 2>/dev/null)

if [ "$dirty" -gt 0 ]; then
  state="$dirty uncommitted change(s) — check before building on top"
else
  state="clean"
fi

cat <<MSG
Chitrakar: on branch '$branch' ($state). Last commit: $last
Resuming? Read docs/PLAN.md section 0 "Where things stand" for what works,
how to verify, and what is next. /chitrakar:status summarizes it,
/chitrakar:verify runs the full gate.
MSG
