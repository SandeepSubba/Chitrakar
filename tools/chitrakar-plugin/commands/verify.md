---
description: Run Chitrakar's full verification gate — fmt, clippy, engine tests, and the browser smoke suite
---

Run the project's complete verification gate and report the result concisely.

Run these from the repository root, in order, and do not stop at the first
failure — collect all results so the report is complete:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cd app && npm run build`
5. `cd app && npm run test:e2e`

Notes that matter for interpreting the output:

- Steps 4 and 5 are the browser suite (~80 pixel-level assertions driving
  the built app in headless Chromium). Step 5 needs a Chromium: Playwright's
  own (`npx playwright install chromium`) or one named by
  `CHITRAKAR_CHROMIUM`.
- CMYK press-profile assertions in steps 3 and 5 self-skip unless
  `CHITRAKAR_TEST_CMYK_ICC` points at a CMYK `.icc`. A "skipped" line there
  is expected, not a failure — say so rather than reporting a gap.

Report: one line per step (pass/fail with the test counts), then the
failures in detail if any. If everything passes, say so plainly in a
sentence — no ceremony.
