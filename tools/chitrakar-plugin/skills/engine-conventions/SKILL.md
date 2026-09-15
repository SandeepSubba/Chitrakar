---
name: engine-conventions
description: Chitrakar's engine invariants — how to add or change document nodes, commands, rendering, and file format fields without breaking undo, incremental rendering, or old files. Use when editing anything under core/ (doc, render, color, codecs, engine) or the app/ UI that talks to the engine.
---

# Chitrakar engine conventions

The architecture holds together because of a few invariants. Breaking one
doesn't fail loudly — it produces subtly wrong undo, stale pixels, or files
that stop opening. Follow these when touching `core/` or the UI that drives it.

## Every mutation is an invertible Command

Document state changes only through `chitrakar_doc::Command`, applied via
`Document::apply`, which **returns the inverse**. Undo/redo is just the
inverse stack — there is no separate undo path to keep in sync.

Adding a command means adding its inverse in the same match arm. If the
inverse needs data the forward command discards (like a removed subtree),
carry that data in the inverse variant, as `RemoveNode` -> `RestoreSubtree`
does. `Batch` applies several commands atomically, rolling back the applied
ones if a later step fails, and inverts as the reversed inverses.

Add the new variant to `Session::command_target` (which node it dirties) and
`Session::describe` (its history label), or history and invalidation quietly
go wrong.

## Drag-style edits use preview/commit/cancel

Never emit one command per pointer move — that floods history. Use the
Session gesture API: `preview()` applies to the document live and re-renders
but records nothing; `commit_preview()` seals the whole gesture as **one**
history entry; `cancel_preview()` restores the pre-gesture state. The first
preview of a gesture captures the inverse that undoes all of it.

Slider edits in the properties panel are gestures too: preview on input,
commit on release/blur.

## Dirty tracking decides what re-renders

The engine computes the affected region from `node_bounds` before and after
each command, and re-renders only that. Consequences when you add a feature:

- If a node type can paint outside its anchor box (a centered path stroke, a
  smooth path's spline overshoot), `node_bounds` must include that overhang
  or you get trails of stale pixels.
- Adjustment and filter layers report `Bounds::Everything` — they act on
  everything composited below them.
- Filters read pixel *neighborhoods*. The dirty region grows by
  `filter_reach`, and `render_cached` computes a further-padded region into
  scratch, copying back only the exact region — the padding ring clamps
  against stale surroundings and must be discarded, never written.

The engine test `assert_cache_matches_fresh` compares the incremental cache
against a full render, byte for byte. Any new node kind or filter needs that
assertion somewhere in its tests; it is the only thing standing between you
and pixels that are subtly wrong only after an edit.

## Source pixels and resources are immutable

A `RasterObject` references a content-addressed resource; edits are
parameter stacks evaluated at render time. Never mutate resource bytes —
add nodes or parameters instead. This is what makes the editor
non-destructive rather than merely undoable.

## Old .chitra files must keep opening

New node kinds and fields are **additive**, with `#[serde(default)]` on
anything new. Never rename or repurpose an existing field. The manifest
carries `FORMAT_VERSION`; readers refuse a newer major rather than
misinterpreting it. Resource bytes and ICC profiles live as separate entries
in the ZIP, not in the manifest.

## The CPU renderer is the correctness reference

`core/render` is a scalar reference implementation, deliberately: it defines
what "correct" means. A future wgpu/vello backend gets validated against it
pixel-for-pixel (llvmpipe makes that runnable in CI — see
`docs/spikes/gpu-rendering.md`). Don't optimize the CPU path into something
whose output you can no longer trust as ground truth.

Compositing is premultiplied linear f32 throughout; conversion to encoded
sRGB happens only at the display/export edge. Blend, mask, and adjustment
math belongs in linear light.

## Color goes through the CMS, not by hand

`core/color/src/cms.rs` (moxcms) owns ICC work: import normalization, CMYK
press profiles, soft proofing, separation for print. Authored CMYK values
are preserved on objects; they resolve through the document's press profile
when one is loaded, and fall back to the naive device formula otherwise.
Soft proofing is display-only — it happens at presentation encode, never in
the document or exports.

## Assets need license-clean provenance

Fonts, profiles, and images committed to the repo ship with their license
(see `core/render/assets/DejaVuSans-LICENSE.txt`). Real CMYK press profiles
are *not* license-clean: tests that need one self-skip unless
`CHITRAKAR_TEST_CMYK_ICC` points at a local file.

## Before committing

`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, and in `app/`: `npm run build && npm run test:e2e`.
Rebuild `app/` whenever engine or UI changed, or the browser suite tests a
stale bundle. Extend the browser suite when UI behavior changes — it has
caught real bugs that unit tests could not.
