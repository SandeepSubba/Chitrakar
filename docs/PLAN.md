# Chitrakar — Architecture & Roadmap

Chitrakar ("painter") is a modern, multiplatform photo + vector editing app built around
two non-negotiable principles:

1. **Non-destructive everything** — the document is a tree of live objects (shapes,
   images, adjustments, filters, masks). Pixels are only ever *rendered*, never baked.
   Any edit can be revisited or removed at any time.
2. **Real color management** — documents can be RGB or CMYK, with ICC-profile-correct
   import, display (soft proofing), and export. This is designed into the pixel
   pipeline from day one, not bolted on.

**Target platforms:** Windows, macOS, Linux, iPadOS, iOS, Android.

---

## 0. Where things stand (read this first)

*Handoff block — keep it current; it exists so a fresh session can resume
without reading anything else.*

- **Branch:** `claude/multiplatform-photo-vector-editor-enghs5`.
- **Working today:** a real editor. Draw rects (square or round-cornered),
  ellipses and pen paths (straight
  or smooth), brush freehand strokes that land as editable paths and swell
  with pressure or slow strokes, place images, add live text; move/scale with handles and live
  drag preview, turn with a rotation knob, flip a selection about its
  own box; adjustment layers (exposure, brightness/contrast, hue/sat, levels,
  curves drawn on a graph in the panel) and
  filter layers (gaussian blur, sharpen) — over everything below, or
  scoped to one layer, which groups the two so the group's isolation does
  the confining — masks on any layer — an
  inscribed ellipse or rectangle, or any shape handed down to the layer
  below — dragged and resized on the canvas, groups,
  reorder, duplicate/copy/cut/paste (subtree and pixels included, across
  documents), delete, align and distribute a multi-selection, combine
  shapes with union/subtract/intersect/exclude into a compound path that
  can carry holes, opacity/blend,
  rename, labelled history with jump-to-state.
  Transforms nest: a group moves, scales and turns as a unit, and
  dissolving one folds its transform into its children. Documents are any
  size, chosen from presets or typed, in RGB or CMYK, and the crop tool
  re-frames one after the fact — the page becomes the dragged rectangle
  and the picture stays where it was inside it.
  Edges are anti-aliased: rect fills analytically, path fills by a scanline
  rasterizer (exact horizontally), the rest by coverage sampling, and vector
  mask edges feather the same way. Placed images sample bilinearly in
  premultiplied linear space, with the image outline antialiased too.
  Fills can be linear or radial gradients with any number of stops,
  authored in the shape's own box so they follow it, aimed by dragging their
  ends and stops on the canvas, and exported as live SVG gradients. Paths carry bezier
  handles you drag on canvas — alt-drag breaks a pair to make a corner —
  and converting from straight or smooth preserves the shape; they export
  as real cubic segments.
  Any layer can carry live effects — drop shadow, outline and inner
  shadow, stacked in any combination, all cast from the layer's
  silhouette so they follow every edit.
  Dragging a layer snaps its edges and centre to the page's and to the
  other layers', showing a guide on the line it caught; ctrl/cmd drags
  free of it — and the same lines catch a resize handle. A ctrl-clicked
  multi-selection moves as one — by drag or by
  arrow key (shift for a coarse step) — in a single history entry. Exact
  placement is typed: the panel carries X/Y/W/H in document pixels.
  Rulers run along the canvas edges; dragging out of one places a guide
  that layers snap to and that saves with the document, and dropping it
  back throws it away.
  Zooming in re-renders rather than magnifies: the engine composites a
  viewport at the resolution the canvas is displayed at, so outlines,
  gradients and glyphs are re-solved instead of interpolated, and a page
  costs a screenful of pixels to show however big it is.
  Text is shaped by the font (rustybuzz: ligatures, kerning, combining
  marks; complex scripts once a font for them is registered), can be set
  in DejaVu Sans, Sans Bold, Serif or Sans Mono — the bundled face is in
  the wasm, the rest are fetched from `app/public/fonts` at startup and
  registered with the engine, so adding a face is dropping a file there;
  File › Load font… registers any TrueType/OpenType file for the page's
  lifetime, a Bold toggle swaps a face for its "… Bold" twin, an Italic
  toggle sets it in its "… Oblique"/"… Italic" twin when one is registered
  (Sans Mono's is shipped) and leans the outlines itself when none is, and
  a saved
  `.chitra` carries the faces its text is set in (all but the bundled one)
  under `fonts/`, registering them on open, so a document reads the same
  wherever it is opened next —
  rasterizes at
  the size it is actually seen at, and carries alignment, line spacing,
  tracking and a wrap width for paragraph text.
  Color: embedded ICC honored on import, CMYK documents with press profiles,
  soft proofing + gamut warning. Files: `.chitra` save/open; export PNG (at 1x, 2x or 3x, or of just
  the selection — rendered at that size, not upsampled), JPEG, SVG,
  CMYK TIFF, PDF. Desktop app packages (deb verified locally; CI builds
  Win/macOS/Linux installers on a `v*` tag).
- **Renderer performance:** the transform inverse is solved once per shape
  (`Inverse`), not per coverage sample — a boundary pixel asks for up to
  twenty-one. Groups only paint where their contents can land, and a group
  that nothing inside reads the backdrop through (opacity 1, Normal, no
  mask, no blended/adjustment/filter descendant) skips its isolation
  surface entirely. Together those took a full A4/300dpi render from
  ~370 ms to ~240 ms (fat LTO in the release profile later took another tenth
  off every figure and 6% off the wasm bundle), and one plain folder in an A4 document from ~250 ms
  of overhead to none. What remains at that size is mostly memory
  bandwidth: the surface is 16 bytes a pixel (139 MB at A4), and each
  full-canvas pass over it costs ~55 ms. `cargo test --release -p
  chitrakar-render -- --ignored --nocapture --test-threads=1` runs the
  timing probes; the next win there is avoiding the whole-canvas
  allocation an isolated group still makes. Interactive rendering goes
  through `Session::set_viewport`, which composites only what the canvas
  can see: an A4 page at 300dpi shown on screen is 15 ms rather than
  169 ms, and the zoom is no longer capped by what a full-page surface
  would cost. A placed photo shown smaller than
  its own resolution is box-filtered over the texels each device pixel
  really covers (up to four taps an axis), so shrinking one settles
  instead of crawling.
- **Verify before committing:** `cargo test --workspace` (~160),
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all`,
  and in `app/`: `npm run build && npm run test:e2e` (~316 browser
  assertions). Both suites self-skip CMYK-profile steps unless
  `CHITRAKAR_TEST_CMYK_ICC` points at a CMYK .icc. The toolchain is pinned
  in `rust-toolchain.toml` and CI installs from it, so the clippy that runs
  locally is the clippy that runs in CI; bump it deliberately.
- **Next up (rough priority):**
  1. wgpu/vello GPU backend, validated pixel-for-pixel against the CPU
     reference renderer (llvmpipe makes this CI-able — see
     docs/spikes/gpu-rendering.md).
  2. Mobile shells: `tauri android init` / `ios init` (needs SDKs, so it
     wants a machine with Xcode/Android Studio).
  3. Depth: underline/strike-through, text on a path, and another
     review pass over the last stretch of commits (each pass so far has
     found real defects).
- **Chrome:** document actions live in a File/Edit/View menu bar — Edit
  carries cut/copy/paste/duplicate/delete and select-all beside undo, View
  carries fit/zoom/actual-size/zoom-to-selection and the guide toggles, so
  none of it depends on knowing the shortcut; the tool
  rail, layer actions and top-bar toggles are icons from `app/src/icons.tsx`
  (one stroke weight, one 24-unit grid, drawn in currentColor). Accent is
  reserved for state — active tool, open menu, live toggle, selected layer.
  Tools have single-letter shortcuts (V/M, R, E, P, T), suppressed while
  typing. Layer rows carry a kind glyph and a mask marker.
- **Tooling:** `tools/chitrakar-plugin/` is a Claude Code plugin bundling
  the verification gate, status, ship, the engine conventions skill, and a
  SessionStart hook (install: `/plugin marketplace add
  SandeepSubba/Chitrakar`, `/plugin install chitrakar@chitrakar`).
- **Known limits, deliberately:** the in-app clipboard carries layers
  between documents; out to other applications a selection goes as a
  picture (Edit › Copy as image puts a PNG of its box on the system
  clipboard), and images from other applications come *in*, by paste or
  by dropping a file on the canvas (a dropped .chitra opens); effects
  come in three kinds;
  and export flattens them like everything else; SVG export sets each
  line the renderer set — wrapped, aligned by text-anchor, on its real
  baseline, at the em the face is scaled to — though a synthesized
  italic lean and a loaded font are the reader's to supply; a mask is an ellipse, a rectangle, or
  another shape handed down to the layer below, moved and resized on
  canvas but not reshaped there; PDF export is live where PDF has the
  words (paths, solid fills and strokes, groups, images, opacity, blend,
  text in embedded faces, subset to the glyphs used) and the engine's
  pixels where it does not (gradients, effects, masks, varying strokes;
  an adjustment or filter flattens what is under it), and TIFF is the
  composite; a boolean
  operation flattens curves to line segments and declines outlines that
  touch or overlap exactly, rather than guessing.

---

## 1. Tech stack

| Piece | Choice | Why |
|---|---|---|
| Engine | **Rust** (`chitrakar-core` workspace) | Memory-safe, fast, compiles natively for all 6 targets *and* to WASM; one engine codebase forever. |
| App shell | **Tauri 2** | Single shell framework covering desktop *and* iOS/Android; native menus, file dialogs, small binaries. |
| UI | **TypeScript + React** (webview) | One UI codebase across all platforms; responsive layout adapts desktop ⇄ tablet ⇄ phone. |
| GPU rendering | **wgpu** (vectors via **vello**, raster ops via compute shaders) | Portable over Vulkan/Metal/DX12/GLES — and over WebGPU when the engine runs as WASM. |
| Color management | **ICC-based CMS**: `lcms2` (battle-tested) with `moxcms` (pure Rust) evaluated as the WASM-friendly alternative | Correct RGB/CMYK conversions, monitor profiles, soft proofing. |
| Codecs | `image`/`zune` crates (PNG, JPEG, TIFF), `resvg`/`usvg` (SVG import), custom exporters | Pure Rust ⇒ works on every target including WASM. |

### How the engine reaches the screen

The engine is one Rust crate compiled two ways:

- **WASM build (MVP path):** the engine runs *inside* the webview and renders through
  WebGPU (fallback: WebGL2/canvas readback). UI ⇄ engine calls are plain in-process
  bindings — no IPC serialization on the hot path. This works identically in every
  Tauri shell and keeps one render path to debug.
- **Native build (optimization path, later):** the same crate runs in the Tauri host
  process rendering with wgpu directly to a native surface composited with the webview.
  We switch per-platform only where WASM/WebGPU proves insufficient (likely candidates:
  older Android webviews, very large documents).

**Risk to validate first (Phase 0 spike):** WebGPU availability in each platform's
webview (WKWebView on iOS, Android System WebView, WebView2, WebKitGTK). The fallback
ladder (WebGL2 → software render + blit) must be proven before we commit the MVP to it.

---

## 2. Document model (the heart of the app)

```
Document
├─ metadata: color mode (RGB | CMYK), working profile, dpi, dimensions
├─ resources: embedded source images (immutable, content-addressed)
└─ root: Group
   ├─ VectorObject      — path/shape parameters, fills, strokes (all editable)
   ├─ RasterObject      — reference to immutable source pixels + its own
   │                       non-destructive edit stack (crop, transform, adjustments)
   ├─ AdjustmentLayer   — curves, levels, HSL, exposure… applies to everything below
   ├─ FilterEffect      — gaussian blur, sharpen… attached to an object or a group
   ├─ Group             — nesting, blend mode, opacity, clipping
   └─ Mask              — raster or vector mask attachable to any node
```

Key rules:

- **Source pixels are immutable.** A RasterObject points at a resource; edits are
  parameter stacks evaluated at render time.
- **Rendering is a pull-based graph evaluation** with per-node caching: a node re-renders
  only when its parameters or inputs change. Caches are tiled (e.g. 256×256 tiles) so
  editing one region doesn't invalidate the whole canvas.
- **Edits are commands.** Every mutation goes through a command object → free undo/redo,
  and later a path to collaborative editing (commands are serializable).
- **Working pixel format:** 32-bit float, premultiplied, linear light, in the document's
  working space. Blending happens in linear; display transform is the last step.

### File format: `.chitra`

A ZIP container (same family as `.ora`/`.sketch`):

```
document.chitra
├─ manifest.json     — versioned schema: full node tree + parameters
├─ resources/        — original embedded images, untouched bytes
├─ profiles/         — embedded ICC profiles
└─ thumbnails/       — preview renders
```

Human-diffable manifest, originals preserved byte-for-byte, forward-compatible via
schema version + "unknown node" passthrough (unknown future node types survive
open→save round trips).

---

## 3. Color pipeline (RGB + CMYK)

No GPU API understands CMYK — so the CMS is part of the engine, not the platform.

```
import:  decode → assign/honor embedded ICC profile → convert to working space
edit:    all compositing in linear float, working space
display: working space → monitor profile (or soft-proof: working → CMYK press
         profile → monitor, with gamut warning overlay)
export:  working space → target profile (sRGB PNG/JPEG, CMYK TIFF/PDF), profile embedded
```

- **RGB documents:** working space = linear form of the chosen profile (sRGB default;
  Display P3 / Adobe RGB selectable).
- **CMYK documents:** native CMYK values are preserved on objects where they were
  authored (a "C:100 M:0 Y:0 K:0" fill stays those numbers); compositing happens in a
  linear RGB proxy space with the document's press profile (e.g. FOGRA39, GRACoL)
  driving display and export. This is the Affinity/Photoshop-style compromise that keeps
  editing fast *and* output correct.
- Soft proofing and per-document rendering intent (perceptual/relative colorimetric)
  are first-class UI, not buried settings.

---

## 4. Repository layout

```
chitrakar/
├─ core/                  # Rust workspace
│  ├─ doc/                # document model, commands, undo, .chitra I/O
│  ├─ render/             # render graph, tiling, wgpu/vello backends
│  ├─ color/              # CMS wrapper, profiles, pixel formats
│  ├─ codecs/             # import/export (PNG, JPEG, TIFF, SVG, PDF)
│  └─ engine/             # public API: the one crate the shells embed
│                         #   (cdylib for native, wasm-bindgen for WASM)
├─ app/                   # TypeScript UI (React) — tools, panels, canvas host
├─ shells/tauri/          # Tauri 2 config for desktop + iOS + Android
└─ docs/                  # this plan, ADRs, format spec
```

---

## 5. Roadmap

### Phase 0 — Foundations & risk spikes (small)
- ✅ Scaffold Rust workspace, Tauri 2 app, React UI, CI (fmt/clippy/test + desktop builds).
- ✅ Desktop packaging: app icons generated for every platform, bundling enabled
  (deb/rpm/AppImage, dmg, msi/nsis), a Linux .deb built and inspected locally,
  and a release workflow producing installers for Windows, macOS (Intel +
  Apple Silicon), and Linux on version tags or manual dispatch.
- **Spike 1:** WASM engine + WebGPU triangle→texture inside Tauri webview on desktop,
  iOS Simulator, Android emulator. Decide the fallback ladder with data.
  - ✅ *First half proven:* engine compiles to WASM (wasm-bindgen), runs in-browser,
    renders to canvas via `putImageData`; full editor loop (draw/undo/hide/save)
    verified headless in Chromium. WebGPU-in-webview per platform still open.
  - ✅ *Native wgpu proven headless* (docs/spikes/gpu-rendering.md): wgpu 23 on
    llvmpipe software Vulkan renders pixel-correct at ~3ms per 1280×720
    draw+readback — the GPU backend is developable and CI-testable against
    the CPU reference renderer.
- **Spike 2:** ✅ lcms2 vs moxcms — **moxcms chosen** (compiles to wasm where
  lcms2's C core cannot, ~4.4× faster, matches lcms2 within 1/255 on RGB and
  CMYK press-profile transforms). Full numbers: docs/spikes/color-management.md.

### Phase 1 — Core editor (vector + raster objects)
- ✅ Document model, command/undo system, `.chitra` save/load (manifest-only container;
  embedded resources arrive with raster support).
- Cached incremental rendering ✅: the engine keeps a composite cache, computes
  dirty regions from node bounds per command, and re-renders/re-encodes only
  those pixels (adjustment layers dirty everything below, by design). Per-node
  tile caches refine this later. Canvas pan/zoom ✅ (wheel zoom toward cursor,
  space/middle-drag pan, fit-to-window).
- Live gestures ✅: preview/commit/cancel in the engine — drags update the
  document each pointer move, history records one undo step per gesture,
  Escape cancels. Transforms support scale (shear/rotation with the GPU path).
- Vector: rect/ellipse ✅ drawn interactively; polygon paths ✅ — even-odd
  fill, centered stroke for open polylines (line art), hit testing and
  stroke-aware bounds — drawn with the pen tool ✅ (click anchors, click the
  first anchor to close as a filled shape, Enter finishes an open stroked
  path, Escape abandons, dashed live preview). Bezier segments and anchor
  editing pending; gradient fills pending.
- Raster: place PNG/JPEG as RasterObject ✅ (content-addressed resource pool,
  pixels embedded as PNGs in .chitra, undoable placement, move + hit test);
  scale/rotate pending.
- Layer panel: hide ✅, select ✅, delete ✅, reorder ✅ (MoveNode command:
  reorder + reparent with subtree-cycle protection), opacity slider ✅,
  blend-mode picker ✅; grouping UI pending.
- Selection ✅ (hit test + move tool with live preview); corner resize
  handles ✅ (anchored scaling); rotation handles pending.

### Phase 2 — Non-destructive power
- Adjustment layers: brightness/contrast ✅, exposure ✅, hue/saturation ✅
  (feColorMatrix-style hue rotation + luminance-relative saturation) — all
  re-editable via the properties panel with live slider preview, one undo
  step per gesture. Levels ✅ (input range, gamma, output range, in linear
  light) and Curves ✅ (monotone cubic through points in the display
  encoding, tabulated once per pass; press-to-add-and-drag graph editor).
- Vector styling ✅ first pass: fill and inner stroke (color + width),
  editable on existing objects; stroke-only shapes hit-test on the band.
  Gradients pending. Layer rename ✅ (SetName command, inline edit).
- Filter effects: gaussian blur ✅ and sharpen (unsharp mask) ✅ as
  non-destructive layers — CPU path uses three iterated box blurs
  (O(pixels) per pass, W3C feGaussianBlur approximation) in premultiplied
  linear; parameters live-edit via the panel. While any filter layer exists,
  incremental invalidation falls back to whole-canvas (neighborhood reads at
  region edges); padded region rendering and the GPU compute path refine
  this later.
- Masks ✅ first pass: a mask attaches to any node — vector masks (hard shape
  coverage) and raster masks (luminance × alpha, transform-sampled) modulate
  a shape's/image's paint, a group's composite, and an adjustment's or
  filter's strength; invert supported; UI adds an inscribed ellipse mask
  with invert/remove. Paintable raster masks await the brush engine;
  on-canvas mask geometry editing and clipping groups pending.
- Full undo/redo history panel ✅: every edit records a human-readable label
  (from the forward command and the touched layer's name); the panel lists
  past and undone-future edits and clicking jumps the document to that point.
- Grouping ✅: Batch command (atomic multi-command with rollback, one undo
  step); group ctrl-click-selected same-parent layers into a new group,
  ungroup dissolves in place — both single history entries.

### Phase 3 — Color management & export
- ICC import honoring embedded profiles ✅ (PNG/JPEG pixels tagged with an
  RGB profile normalize to sRGB at the decode edge via moxcms).
- CMYK document mode ✅ with press profiles ✅: documents carry an ICC press
  profile (persisted in .chitra as profiles/cmyk.icc, loadable in the UI);
  authored CMYK ink renders through it, naive formula as fallback; shapes
  drawn in CMYK documents author real ink values with C/M/Y/K ink sliders ✅.
- Soft proofing ✅ + gamut warning ✅: display-only round trip through the
  press profile at the presentation-encode step (exports stay unproofed);
  out-of-gamut pixels mark neutral grey. Monitor profiles and rendering-
  intent selection pending.
- Export: PNG ✅ (sRGB composite), SVG ✅ (live vector markup — shapes,
  paths, groups with opacity/blend, embedded rasters, text; CMYK colors
  resolve through the press profile; adjustments/filters/masks noted as
  omitted), CMYK TIFF ✅ (composite separated into ink through the press
  profile, composited over paper white, 4-channel TIFF with that profile
  embedded; refuses rather than guessing when no profile is loaded).
  and PDF ✅ (one page sized from the document dpi; live paths, solid
  fills and inner/centred strokes, nested groups, image XObjects with
  soft masks, opacity and blend as graphics states, text as text (each
  face embedded once as a CID font addressed by glyph id, glyphs where
  the shaper put them so kerning and ligatures survive, a synthesized
  italic as a text-matrix skew, and a ToUnicode map so the words can be
  found and copied — checked by reading them back through Ghostscript;
  the face travels as a TrueType subset of the glyphs used, ids kept),
  and the engine's pixels — oversampled towards 300 dpi and trimmed to
  their ink — for what PDF cannot say; with a press
  profile authored ink is written as ink, sRGB is separated through the
  profile, and the profile is both the ICCBased colour space and the
  page's output intent; a Ghostscript test checks the page against the
  CPU renderer). JPEG ✅.

### Phase 4 — Mobile shells
- Tauri iOS/Android builds; responsive UI: collapsible panels → bottom toolbars.
- Touch + Apple Pencil/stylus input (pressure into the input pipeline early, ahead of
  brush tools).
- Platform file integration (Files app, Android SAF, share sheets).

### Phase 5 — Depth (ongoing)
- Pen tool + full path editing; boolean operations on shapes.
- Text objects ✅ first pass: live TextSpec nodes (string, size, color as
  document state; glyphs rasterize at render time via ab_glyph + bundled
  DejaVu Sans, kerned per-glyph layout with newline support), blitted through
  the node transform with mask/opacity/blend support; Text tool click-places,
  panel edits content/size/color with gesture preview; resize handles work.
  Proper shaping (`rustybuzz`/`parley`), font choice, and weights pending.
- Brush engine for raster painting; healing/clone as non-destructive ops.
- Live effects (drop shadow, outline), styles, symbols/components.
- Later bets enabled by the architecture: collaboration (serializable commands),
  plugin API (WASM sandboxed), web build (engine already compiles to WASM).

---

## 6. Guiding decisions (mini-ADRs)

1. **One engine, two compilations (WASM + native)** — never fork the engine per platform.
2. **Linear float compositing** — correctness first; 8-bit preview paths only as a
   measured optimization.
3. **Tiled, cached, pull-based rendering** — the non-negotiable for non-destructive
   editing at interactive speed.
4. **Immutable sources + parameter stacks + commands** — undo, history, and future
   collaboration all fall out of this one choice.
5. **ZIP+JSON container format** — inspectable, versionable, resilient; binary-only
   formats are a trap at this stage.
6. **UI in the webview, pixels in the engine** — the UI never touches pixel buffers;
   it sends commands and displays engine-rendered textures.
