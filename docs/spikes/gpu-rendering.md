# Spike: wgpu viability for the GPU render path

**Result: viable, including in headless CI.** wgpu 23 initializes and
renders correctly on Mesa's software Vulkan driver (llvmpipe) with no GPU
and no display server.

## Setup

- Environment: Linux container, no GPU, no X/Wayland. `mesa-vulkan-drivers`
  provides llvmpipe (LLVM 20.1.2).
- wgpu 23 + pollster; offscreen `Rgba8Unorm` target, full render-to-texture
  + buffer readback round trip.

## Findings

| Check | Result |
|---|---|
| Adapter discovery | llvmpipe via Vulkan backend, `DeviceType::Cpu` |
| Clear + triangle draw correctness | pixel-verified (clear color at corners, fill color at center) |
| 1280×720 draw + full readback | **~3.0 ms/frame** on software Vulkan |

Even the *software* Vulkan path round-trips a full canvas in ~3ms — far
under a 60fps budget — so on real GPUs (desktop Metal/Vulkan/DX12, WebGPU in
the browser) the wgpu path has ample headroom, and the same code is testable
in plain CI containers by installing `mesa-vulkan-drivers`.

## Implications for the render architecture

- The planned backend split stands: keep the scalar CPU renderer as the
  correctness reference, add a wgpu backend validated against it
  pixel-by-pixel (llvmpipe in CI makes that comparison automatable).
- Vector rasterization on GPU should come from **vello** (wgpu-based) per
  the plan; this spike de-risks the wgpu substrate underneath it.
- The blur/sharpen filters and adjustment layers map to compute passes;
  the tiled cache design carries over (tile textures instead of CPU tiles).
- WebGPU-in-webview availability per mobile platform remains the open half
  of Spike 1 and still needs on-device verification.

## Reproducing

```sh
apt-get install mesa-vulkan-drivers
cargo run --release   # in the spike crate: adapter print, pixel asserts, timing
```
