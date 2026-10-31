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
cargo test --workspace                      # engine tests (~620)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
cd app && npm run dev                       # browser dev on :5173 (builds wasm first)
cd app && npm run build                     # wasm + typecheck + bundle
cd app && npm test                          # unit tests, then the browser suite
cd shells/tauri/src-tauri && ../../../app/node_modules/.bin/tauri dev    # desktop
cd shells/tauri/src-tauri && cargo test     # the shell's own (needs webkit2gtk-4.1 dev libs)
```

Every WebAssembly module (engine, PDF reader, raw reader) is built with
SIMD: `.cargo/config.toml` gives the wasm targets `+simd128`, which the
compiler uses to run the per-pixel loops four lanes at a time. A browser
without it (before Chrome 91, Firefox 89, Safari 16.4) is told so by
`initEngine` (`wasmSupported` in `src/engine.ts`, smoke 11g2) rather than
left blank. A `RUSTFLAGS` in the environment replaces those flags, so a
build that sets one must carry `-C target-feature=+simd128` itself.
`node tools/bench/engine/run.mjs --io` times a photograph's way in and
out (decode, save, a raw opened and developed); without `--io`, frames.

In the desktop shell a document has a file: Save writes back to it,
Save as and Open use the system's panels, exports ask where (a folder
for several files), and File › Open recent keeps `prefs.recent`.
`app/src/files.ts` is the one place that knows whether it is in a
browser or the shell; the shell's commands are in
`shells/tauri/src-tauri/src/lib.rs`, and smoke block 9ca drives all of
it through a stand-in bridge.

The shell can also draw the page's frames on the graphics card
(`prefs.gpuFrames`, off by default): `core/present` keeps a mirror of
the document there, fed by `app/src/gpuFrames.ts` through the commands
in `shells/tauri/src-tauri/src/gpu.rs`. The webview's engine stays the
source of truth. The mirror only answers when its frame would be the
processor's, and in every other case the page draws its own.

CMYK-profile tests self-skip unless `CHITRAKAR_TEST_CMYK_ICC` points at a
real CMYK .icc (e.g. ghostscript's default_cmyk.icc) — profiles aren't
license-clean to commit.

"Pick out the subject" tries three things in order and falls through
quietly. The system's own model where there is one: on macOS the shell
compiles `shells/tauri/src-tauri/swift/subject.swift` (Vision's subject
lifting) and hands the matte to `Session::pick_matte`; no weights are
shipped because it is already on the machine, and a build with no
`swiftc` just warns. Then a U^2-Net ONNX model, run through `tract`, if
one has been put at `~/.cache/chitrakar/u2net.onnx` or named by
`CHITRAKAR_SUBJECT_MODEL` — looked for and never committed: it is 176 MB,
and the 4.6 MB version that would fit was measured on the same
photographs and is not good enough to ship. Then the engine's own
colour-based `Session::pick_subject`, which is what the browser gets and
what answers when nothing else can.

`app/e2e/runs.test.mjs` unit-tests `src/runs.ts` — the byte arithmetic
behind styling part of a text block — without a browser: `npm run
test:unit` in `app/`, a second or two, and it is where a change to runs
should be tried first. It transpiles the TypeScript with the esbuild
that vite already brings, so there is nothing to install. Most of it is
a property over random strings and random edits, with emoji and accents
in the alphabet on purpose. `app/e2e/measure.test.mjs` runs beside it the same way, over
`src/measure.ts`: the distances the Move tool shows with alt held; and
`app/e2e/findReplace.test.mjs` over `src/findReplace.ts` (Edit › Find
and replace), bundled with the `runs.ts` it carries styling through; and
`app/e2e/touch.test.mjs` over `src/touch.ts`: two- and three-finger taps
(undo, redo) and the Shift/Alt/Ctrl a touch screen holds on the canvas,
which are worn onto every pointer event in the window's capture phase so
no handler has to know they exist; and `app/e2e/files.test.mjs` over
the naming in `src/files.ts`, which reads a file's name out of an
Android `content://` address as well as a path — the shell opens and
writes those through the fs plugin (`files::is_address`); and
`app/e2e/spacing.test.mjs` over `src/spacing.ts`: the equal-spacing
guides the Move tool catches after the edges and middles (smoke 9cb);
and `app/e2e/imports.test.mjs` over `src/imports.ts`; and
`app/e2e/develop.test.mjs` over `src/develop.ts`, the develop window's
temperature scale, histogram and clipping.

On a phone (under 640 wide or 500 tall) the menus are a drawer behind
one Menus button and the bar is one row (smoke 11a); the suite's
`menuItem` goes through the drawer whenever there is no menubar. On a
touch screen a long press opens the canvas's menu and a pen rejects the
palm (smoke 11b). The shell compiles for Android and iOS (CI jobs
`shell-android`, `shell-ios`), and CI's `android-apk` generates the
Android project with `tauri android init` and keeps an optimised APK,
signed with a throwaway key made on the runner, as an artifact; nothing under `gen/android` or `gen/apple` is committed.

The crash-recovery draft lives in IndexedDB in pieces: `current` is the
document without its pictures and originals (`save_skeleton`), written a
breath after each edit; each picture's PNG and each original is a
`part:<name>` record written once; `parts` lists what `current` needs, and
`restore_part` puts them back on Restore (smoke 10). A picture's PNG is
made once per session (`WrittenPictures`) for drafts and Save alike.

In a browser the built page is an installable app:
`app/public/manifest.webmanifest` (share target, file handlers) and a
service worker, `app/public/sw.js`, registered by `src/offline.ts` only
in a production build outside the shell. It keeps what a visit loads
so the editor opens offline — content-named `assets/` cache-first,
everything else network-first — and takes shares posted to
`share-target`, opening the page with `?shared` (`takeShared`). Drops,
shares and launches all go through `takeFiles` in App.tsx. Smoke
11e–11g; the suite's main page runs under the worker too.

The Playwright smoke suite lives at `app/e2e/smoke.mjs` (~1400 pixel-level
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
after the region is let go of — under the clip the stroke is whole. That
holds for a mask brushed by hand as much as for a layer: a mask is
brushed with the same tool and the same strokes, so it is confined the
same way, and both paths read the clip.

## Colour by name

A document's palette (`Document::swatches`, `Command::SetSwatches`) is
reached for rather than copied from: `AuthoredColor::Named { name, means }`
is a colour standing for an entry. It carries what the name means as well
as the name, so it is still a colour away from the document — exported, or
in a palette the name has been taken out of — and no renderer or exporter
has to learn about palettes: they read past the name with
`AuthoredColor::flat()`.

The live half is `Document::settle_swatches`, which points every named
colour at what the palette now says. `Document::unsettled_by` decides when:
a palette change unsettles everything, and a command that brings colours
in on a layer unsettles that layer (a layer arriving from a document with
a palette of its own can carry another file's idea of a name). It is only
as good as `Node::each_color_mut` is complete, which is why that walk
matches every `NodeKind` and `Effect` exhaustively rather than falling
through — a new kind of layer will not compile until it says whether it
holds a colour.

Styles (`Document::styles`, `Command::SetStyles`, a `KeptStyle` holding
a `Look`) are the palette's shape for a whole look — fill, stroke,
gradient, effects, opacity, blend, and how text is set when taken off
text — kept by name and given to layers by name through
`Session::apply_style`. A layer given one follows it (`Node::style`,
`Command::SetStyleLink`): keeping the style again under the same name
gives its new look to every follower in the same step, forgetting it
lets them go with their look, and the panel's Detach lets one go. A style keeps its colours *flat*
(`keep_style` reads past every name), so it is outside the settling
above by design: it names a look, not the entries the look was made
from.

## Settings, and getting a picture out

`app/src/prefs.ts` is the one place the app remembers how you like to
work: a single `chitrakar:prefs` object, read back field by field
against the defaults' own types and clamped on the way in, so a number
typed into a window cannot reach the engine as something it will not
take. It replaced four `localStorage` keys under two spellings, and it
reads the old two once so nothing anybody had chosen is lost. A
preference is about the *person* — units, grid, snap distance, what
export you reach for. Anything the file should still say on another
machine belongs in the document and goes through a `Command`. The
panel's width and the toolbar's position deliberately stay on their own
keys: they are written on every frame of a drag.

`PreferencesDialog.tsx` (Ctrl+,) is a rail of nine groups over that
object; its Appearance page picks the theme (`prefs.theme`: system, dark
or light — every chrome colour is a token on `:root` in `styles.css`,
the light set sits under `:root[data-theme="light"]`, and the app
resolves "system" before setting the attribute; what is drawn over the
picture keeps its own colours), an accent of the person's own
(`prefs.accent`, laid on the root as `--accent` with `--on-accent`
worked out by `onColour`), a colour for sliders and ticks
(`prefs.sliderColor` → `--slider`, which `accent-color` reads; it
follows the accent when unset) and the interface size (`prefs.uiScale`:
the root font size, the panel's drawn width, and `data-scale` on the
root so the bar gives things up at proportionally wider windows); its Tools page decides which tools are on the rail (a tool put
away keeps its key and waits behind a "…" slot at the rail's end) and
which button groups the bar shows; `prefs.brushes` is the brushes kept
by name, shown as chips over the paint tools' row. `prefs.recentColors` is the
last eight colours used, a round-chipped row under the palette. `app/src/tools.ts` is the one table
of tools — names, keys, icons, the rail's five sections — that the rail,
the keyboard handler and that page all read; the keys are defaults, and
`boundKeys(prefs.toolKeys)` is what is read at run time, so a tool key
rebound on the Tools page (press its key button, then the key; a key
another tool holds changes hands; Backspace restores) reaches the
handler, the tooltips and the keys sheet alike. `app/src/commands.ts` is
the same idea for the *commands* — save, group, undo, send to the back:
one table of chords (`mod+shift+e`, modifiers in a fixed order) that the
two keydown handlers, the menus and the preferences' Keys page all read,
with `boundChords(prefs.commandKeys, prefs.menuKeys)` laying a person's
rebindings over the defaults. Any other menu row can be given a chord
too (`prefs.menuKeys`, by the row's id; `giveChord` in commands.ts
moves a chord between commands and rows, `claimable` keeps a bare
letter for the tools), run through the same `findEntry` the system
menu bar uses; the keys sheet lists them under "Your keys". `ExportDialog.tsx` (Ctrl+Shift+E) replaced thirteen File-menu
rows with format × area × scale, and shows the file's real size — it
encodes the bytes rather than estimating, and the same encode is what
gets written, and is decoded again for the picture beside the settings
(a PDF or TIFF shows the page standing in); a WebP is lossless and
goes everywhere a PNG does (`Raster` in `core/codecs`), though only its
encoder is in the engine — placing a WebP, GIF or BMP goes through the
browser's decoder (`app/src/picture.ts`); `Set` writes @1x, @2x and
@3x in one press; `prefs.exportSetups` keeps a window's answers by name
as chips above the formats; the area "Each kept region" writes a file per
region the document keeps by name (`Session::kept_region_png` reads one
where it is kept, so exporting moves neither the selection nor history). Above four megapixels it waits to be asked. Two exports
are still menu rows and should stay that way: a frame's export multiple
and its name are the *document's* (`export_scale`), which the window's
own scale cannot express.

## Getting other programs' files in

`app/src/imports.ts` decides the road (`importKind`: by first bytes, and
by name for a camera's raw file, which is a TIFF inside), read by Place,
Open (a non-`.chitra` opens as a new document named after it), drops,
shares and the shell's "Open with". The readers are the engine's, in
`core/codecs`: `tiff_import.rs` (bits, CMYK, ICC), `psd.rs` (our own:
layers, groups, masks, blend modes; what it cannot keep goes to
`Session::import_notes` and Photoshop's composite comes in hidden beside
the layers), `core/pdfin` (its own WASM module, `chitrakar-pdf-in`,
which `app/src/pdf.ts` fetches only when a PDF is first opened: `hayro`
draws a page at the dpi the `PdfDialog` asks for, PDF-compatible `.ai`
too, and the engine is handed the pixels by `place_rgba`) and
`core/rawin` (`rawler` reads every camera's format, CR3 included; built
for WASI by `npm run build:wasm-raw` and run in a worker,
`app/src/rawWorker.ts` + `src/raw.ts`, since rawler relies on catching
its own panics and a WASM module cannot — the worker is retired on any
failure; it hands the engine the mosaic packed, `raw::unpack`). A raw
file opens in the develop window (`RawDevelopDialog`):
`develop.rs` keeps the sensor's values (`RawPhoto`) and develops them
with `RawDevelop` — white balance in kelvin and tint through the
camera's matrix, exposure, highlights, shadows, whites, blacks,
contrast, vibrance, saturation, and Detail (sharpening, noise
reduction, judged in a 1:1 square from `develop_square`) — with a
block-averaged `RawPreview` for the sliders; settings kept by name
(`prefs.developPresets`) and given to other picked raw layers as one
step (`stage_redevelop` + `commit_staged_develop`). X-Trans has a
demosaic of its own (`x_trans_region`, judged by the ignored
`demosaic_against_truth`); a Fuji's RAF carries its lens's corrections,
which `core/rawin` reads (`fuji_lens`), and a DNG its opcodes
(`dng_opcodes`; `core/codecs/src/opcodes.rs`: gain maps, a vignette, a
warp), which develop applies unless `RawDevelop::uncorrected` says not
to (a DNG's list 2, the sensor's shading, always). New developments lay
a base curve fitted to cameras' own JPEGs under the settings
(`RawDevelop::curve`, `base_curve`); a picture developed before it has
none. Kept sensor values have room above white (`WHITE` is half their
range), which clipped highlights are rebuilt into once
(`rebuild_highlights`, judged by the ignored `highlights_against_truth`). Placed, the raw file is kept in the document
(`Document::originals`, `originals/` in the zip) and the settings on
the picture (`RasterRef::develop`), so Develop again is a `SetKind`
with one undo (smoke 9cn). HEIC/AVIF are the browser's to decode, and
a HEIC that will not decode says how to get a JPEG. hayro ships Foxit's
BSD standard fonts and rawler is LGPL-2.1, which a GPL-3.0 project
can carry. Smoke 9cm builds each file byte by byte. Going the other
way, the Export window writes a layered PSD (`Session::export_psd`,
`psd_export.rs`): each layer drawn alone with its opacity, blend,
visibility, clipping and mask (as Photoshop's own layer mask) written
beside it, groups as groups, and
adjustments as stamps of what they make of what is under them (smoke
9co).

## Measuring

`tools/bench/` (see its README) times the editor in headless Chromium:
`engine/run.mjs` every adjustment and filter over a 12-megapixel
photograph through the WASM engine alone, and `app/gesture.mjs` a drag
or a run of wheel zooms through the page itself, with `THROTTLE=4` for
a phone's speed and `PROF=1` for where the time went. For looking at an
app change by hand or driving it from a script, keep `npm run dev`
running on :5173 rather than rebuilding each time; only the smoke suite
needs `dist`, and refuses one older than any source.

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
