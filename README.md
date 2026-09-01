# Chitrakar

A modern, multiplatform photo + vector editing app with fully non-destructive
objects and layers, and real color management (RGB and CMYK, ICC-correct).

**Targets:** Windows, macOS, Linux, iPadOS, iOS, Android.

**Stack:** Rust engine (document model, GPU render graph via wgpu/vello, ICC color
pipeline) + Tauri 2 shells + TypeScript/React UI.

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
```

Repository layout: `core/` (Rust engine workspace) · `app/` (shared UI) ·
`shells/tauri/` (desktop + mobile shells) · `docs/` (plan, ADRs).
