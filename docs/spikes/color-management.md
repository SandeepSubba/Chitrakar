# Spike: ICC engine — moxcms vs lcms2

**Decision: moxcms** (pure-Rust CMS, `moxcms = "0.9"`), adopted in
`chitrakar-color::cms`.

## Method

Head-to-head on the same machine (x86-64 Linux container, release builds),
converting identical inputs through both engines:

- Display P3 → sRGB, 8-bit RGBA, relative colorimetric.
- CMYK → sRGB using a real press profile (Artifex CMYK SWOP,
  `default_cmyk.icc` from ghostpdl — used as test data only, not
  redistributed in this repo).
- Throughput: one 1920×1080 RGBA frame through the P3→sRGB transform.
- WASM: `cargo check --target wasm32-unknown-unknown` for each.

## Results

| Criterion | moxcms 0.9 | lcms2 6.2 (liblcms2 C) |
|---|---|---|
| CMYK C=100 → sRGB | (0, 175, 239) | (0, 176, 240) |
| CMYK rich black → sRGB | (33, 36, 37) | (34, 36, 37) |
| Max channel diff, 2M random P3→sRGB pixels | **1 / 255** | (reference) |
| 1080p frame P3→sRGB | **7.6 ms** | 34.0 ms |
| wasm32-unknown-unknown | **compiles clean** | fails (cc cannot build C without a wasm sysroot) |
| Extra WASM size (engine module) | ~380 KB raw / ~150 KB gzip | n/a |

## Why moxcms

1. **WASM is non-negotiable** for the one-engine architecture, and lcms2's C
   core does not build for `wasm32-unknown-unknown` with stock tooling.
   This alone decides it; the rest is upside.
2. **Correctness**: within 1 LSB of lcms2 (the de-facto reference) on both
   RGB and CMYK transforms.
3. **Speed**: ~4.4× faster on bulk 8-bit transforms in this test.
4. Pure Rust: no `unsafe` FFI surface, one toolchain, same code on every
   target.

Risks accepted: moxcms is younger than lcms2; exotic profile constructs
(device links, abstract profiles) are less battle-tested. Mitigation: all
profile parsing is treated as untrusted input with graceful fallback (bad
profile ⇒ naive conversion, never a crash), and the CMS sits behind our own
`chitrakar_color::cms` API so the backend could be swapped.

## What landed with the spike

- `chitrakar_color::cms`: import normalization (`normalize_rgba8_to_srgb`)
  and `CmykCms` (parsed press profile with a cached CMYK→sRGB f32 transform).
- Import honors embedded ICC profiles: PNG/JPEG pixels tagged with an RGB
  profile are converted to sRGB once at the decode edge.
- CMYK documents can carry a press profile (stored as `profiles/cmyk.icc`
  in `.chitra`); authored CMYK ink values render through it, falling back
  to the naive device formula without one. The UI loads a profile via the
  ICC button on CMYK documents; shapes drawn in CMYK documents author real
  ink values.

## Testing note

Real CMYK press profiles are not license-clean to commit. Tests that need
one are self-skipping: set `CHITRAKAR_TEST_CMYK_ICC=/path/to/profile.icc`
(e.g. ghostscript's `default_cmyk.icc`) to run the full CMYK assertions
locally. RGB-profile tests are fully self-contained — moxcms can synthesize
and `encode()` Display P3, so the import-normalization path is always
exercised in CI.

## Still open (Phase 3 remainder)

- Soft proofing (working → press → monitor) with gamut warning.
- Monitor profiles for display transform.
- CMYK export (TIFF/PDF with embedded profile) and rendering-intent UI.
- Import of CMYK-encoded JPEG/TIFF source images.
