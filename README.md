# Chitrakar

A modern, multiplatform photo + vector editing app with fully non-destructive
objects and layers, and real color management (RGB and CMYK, ICC-correct).

**Targets:** Windows, macOS, Linux (shipping); iPadOS, iOS, Android (shells
pending).

**Stack:** Rust engine (document model, a CPU renderer that compiles natively
and to WASM, a wgpu backend growing beside it and checked against that
renderer pixel for pixel, moxcms ICC color pipeline, codecs) + Tauri 2
shells + TypeScript/React UI. The UI never touches pixels: it sends commands over
the WASM boundary and presents the frames the engine renders.

**Today:** shapes, pen and brush paths with bezier handles and booleans,
placed images, live text (shaped by the font, any face loaded, bold /
italic / underline, wrapped, aligned, set along a path, typed on the
canvas), adjustment and filter layers (exposure, brightness/contrast,
hue/saturation, levels, curves, blur, sharpen), masks, live effects
(shadows, outlines), groups, alignment, snapping, guides and rulers in
pixels or millimetres, crop, flip, rotate, a labelled undo history,
CMYK documents with press profiles and soft proofing, autosaved drafts,
and export to PNG, JPEG, SVG, CMYK TIFF and PDF (live vectors and text,
ink as ink). `docs/PLAN.md` §0 says exactly where things stand.

See [docs/PLAN.md](docs/PLAN.md) for the full architecture and roadmap.

## Development

```sh
# Rust core (document model, renderer, color, codecs, engine)
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings

# UI (TypeScript + React + Vite)
cd app && npm install
npm run build      # typecheck + production bundle
npm run dev        # dev server on :5173

# Tauri desktop shell (needs the Tauri Linux/macOS/Windows prerequisites:
# https://v2.tauri.app/start/prerequisites/)
cd shells/tauri/src-tauri && cargo check

# Desktop app: dev window / installable bundles (deb, rpm, AppImage on
# Linux; dmg on macOS; msi/nsis on Windows)
cd shells/tauri/src-tauri
../../../app/node_modules/.bin/tauri dev
../../../app/node_modules/.bin/tauri build
```

Tagging `v*` (or manually dispatching the "Release builds" workflow) builds
installers for Windows, macOS (Intel + Apple Silicon), and Linux in CI.

Repository layout: `core/` (Rust engine workspace) · `app/` (shared UI) ·
`shells/tauri/` (desktop + mobile shells) · `docs/` (plan, ADRs).
