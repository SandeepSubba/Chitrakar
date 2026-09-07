# Chitrakar — context for Claude Code sessions

Multiplatform non-destructive photo + vector editor. Rust engine compiled
natively **and** to WASM; TypeScript/React UI; Tauri 2 shells for
desktop/mobile.

**Resuming work? Read `docs/PLAN.md` §0 "Where things stand" first** — it
says what works today, how to verify, and what's next, so a fresh session
needs nothing else. The rest of PLAN.md is the roadmap (✅ = done),
`docs/spikes/` holds the moxcms-CMS and wgpu-GPU decisions with data, and
`git log --oneline` is a faithful build diary. Keep §0 current when you
finish a chunk of work.

## Architecture in one breath

The UI never touches pixels or the document: it sends serde-JSON `Command`s
over the WASM boundary (`core/engine/src/wasm.rs`) and presents frames the
engine renders. The document (`core/doc`) is a scene graph of live
parameterized nodes — vector shapes/paths, rasters referencing immutable
content-addressed resources, text, adjustment/filter layers, masks — and
every mutation is an invertible `Command` (undo = inverse stack; `Batch` is
atomic-with-rollback). Rendering (`core/render`) composites in premultiplied
linear f32 with dirty-region caching from node bounds; color (`core/color`)
holds the moxcms ICC pipeline (import normalization, CMYK press profiles,
soft proofing). `.chitra` files are ZIP: JSON manifest + PNG resources +
ICC profile (`core/codecs`).

## Conventions that matter

- Every mutation goes through a `Command` with a correct inverse. Drag-style
  edits use the Session preview/commit/cancel gesture API: previews update
  the document live, history records ONE entry per gesture.
- Dirty tracking: engine computes affected bounds pre+post per command.
  Filters and Batches conservatively invalidate the whole canvas.
- Source pixels and resources are immutable; only references are edits.
- Keep the CPU renderer the correctness reference; a future wgpu backend
  gets validated against it (llvmpipe makes that CI-able).
- Old `.chitra` files must keep loading: new node kinds/fields are additive
  with `#[serde(default)]`.

## Commands

```sh
cargo test --workspace                      # engine tests (~338)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cd app && npm run dev                       # browser dev on :5173 (builds wasm first)
cd app && npm run build                     # wasm + typecheck + bundle
cd app && npm test                          # unit tests, then the browser suite
cd shells/tauri/src-tauri && ../../../app/node_modules/.bin/tauri dev    # desktop
```

CMYK-profile tests self-skip unless `CHITRAKAR_TEST_CMYK_ICC` points at a
real CMYK .icc (e.g. ghostscript's default_cmyk.icc) — profiles aren't
license-clean to commit.

`app/e2e/runs.test.mjs` unit-tests `src/runs.ts` — the byte arithmetic
behind styling part of a text block — without a browser: `npm run
test:unit` in `app/`, a second or two, and it is where a change to runs
should be tried first. It transpiles the TypeScript with the esbuild
that vite already brings, so there is nothing to install. Most of it is
a property over random strings and random edits, with emoji and accents
in the alphabet on purpose.

The Playwright smoke suite lives at `app/e2e/smoke.mjs` (~888 pixel-level
assertions driving the built app in headless Chromium; it has caught real
bugs). Run `npm run build && npm run test:e2e` in `app/`. Extend it whenever
UI behavior changes. While writing one, `node e2e/one.mjs 9af` (or
`node e2e/one.mjs "Colour balance"`) runs a single block against the
harness alone — seconds instead of the full run — but the suite is
still the gate. A failure names the block it happened in and prints
what the editor looked like — the layers and which were picked — and
puts the browser and the port away, so the next run does not die on
EADDRINUSE about something else; a screenshot lands in `e2e/out/`. Env:
`CHITRAKAR_CHROMIUM` (browser path override), `CHITRAKAR_TEST_CMYK_ICC`
(enables the press-profile/proofing steps).

## Selections

A selection is a region picked out of the page, kept in the document as a
`Mask` (`Document::selection`, `Command::SetSelection`). It is not a
stencil pixels are cut through — this editor is non-destructive — it is a
region handed to a layer as the part of it that shows, which is why it is
a mask and why `Session::mask_from_selection` is a carry between spaces
rather than a conversion. Add/subtract/intersect go through the shape
booleans in `chitrakar_render::boolean`. A brush painted inside a region
carries it on the stroke (`PaintStroke::clip`), so it stays confined
after the region is let go of — under the clip the stroke is whole.

## Claude Code plugin

`tools/chitrakar-plugin/` packages this workflow: `/chitrakar:verify` (the
full gate in one step), `/chitrakar:status`, `/chitrakar:ship`, an
`engine-conventions` skill that loads when editing `core/`, and a
SessionStart hook reporting live branch/dirty state. Install with
`/plugin marketplace add SandeepSubba/Chitrakar` then
`/plugin install chitrakar@chitrakar`.

## Working agreement

Before committing: fmt + clippy clean + all tests green, and rebuild
`app/` if engine/UI changed. Push to the active feature branch. Update
`docs/PLAN.md` ✅ marks when completing roadmap items. Don't commit fonts,
profiles, or assets without license-clean provenance (see
core/render/assets/DejaVuSans-LICENSE.txt as the pattern).
