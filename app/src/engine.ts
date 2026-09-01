/**
 * Typed wrapper around the WASM engine (generated bindings in ./wasm-pkg).
 *
 * The UI never mutates document state itself: it builds serde-JSON commands
 * mirroring `chitrakar_doc::Command`, sends them through `WasmSession.apply`,
 * and re-reads rendered pixels / layer state from the engine.
 */

import init, { WasmSession } from "./wasm-pkg/chitrakar_engine";

export type NodeId = number;
export type BlendMode = "Normal" | "Multiply" | "Screen";

export type AuthoredColor =
  | { Srgb: { r: number; g: number; b: number; a: number } }
  | { Cmyk: { c: number; m: number; y: number; k: number; a: number } };

export type VectorShape =
  | { Rect: { width: number; height: number } }
  | { Ellipse: { rx: number; ry: number } };

export type Adjustment =
  | { BrightnessContrast: { brightness: number; contrast: number } }
  | { Exposure: { stops: number } }
  | {
      HueSaturation: {
        hue_degrees: number;
        saturation: number;
        lightness: number;
      };
    };

export interface Transform {
  a: number;
  b: number;
  c: number;
  d: number;
  e: number;
  f: number;
}

export interface Stroke {
  color: AuthoredColor;
  width: number;
}

export type Filter =
  | { GaussianBlur: { sigma: number } }
  | { Sharpen: { sigma: number; amount: number } };

export type MaskKind =
  | { Vector: { shape: VectorShape; transform: Transform } }
  | {
      Raster: {
        resource_id: string;
        width: number;
        height: number;
        transform: Transform;
      };
    };

export interface Mask {
  kind: MaskKind;
  invert: boolean;
}

export type NodeKind =
  | "Group"
  | {
      Vector: {
        shape: VectorShape;
        fill: AuthoredColor | null;
        stroke: Stroke | null;
      };
    }
  | { Adjustment: Adjustment }
  | { Filter: Filter };

export interface NodePayload {
  name: string;
  kind: NodeKind;
  transform: Transform;
  opacity: number;
  visible: boolean;
  blend: BlendMode;
}

export type Command =
  | { AddNode: { parent: NodeId; index: number; node: NodePayload } }
  | { RemoveNode: { id: NodeId } }
  | { SetOpacity: { id: NodeId; opacity: number } }
  | { SetVisible: { id: NodeId; visible: boolean } }
  | { SetBlendMode: { id: NodeId; blend: BlendMode } }
  | { SetTransform: { id: NodeId; transform: Transform } }
  | { SetKind: { id: NodeId; kind: NodeKind } }
  | { SetName: { id: NodeId; name: string } }
  | { SetMask: { id: NodeId; mask: Mask | null } }
  | { MoveNode: { id: NodeId; parent: NodeId; index: number } };

/** Mirror of `chitrakar_engine::LayerInfo`. */
export interface LayerInfo {
  id: NodeId;
  name: string;
  kind: "group" | "vector" | "raster" | "adjustment" | "filter";
  visible: boolean;
  opacity: number;
  blend: BlendMode;
  has_mask: boolean;
  depth: number;
  /** Slot in the parent group, painter's order (0 = bottom). */
  parent: NodeId;
  index: number;
  sibling_count: number;
}

export const identity = (tx = 0, ty = 0): Transform => ({
  a: 1,
  b: 0,
  c: 0,
  d: 1,
  e: tx,
  f: ty,
});

export function nodePayload(
  name: string,
  kind: NodeKind,
  tx = 0,
  ty = 0,
): NodePayload {
  return {
    name,
    kind,
    transform: identity(tx, ty),
    opacity: 1,
    visible: true,
    blend: "Normal",
  };
}

/** Parse "#rrggbb" into an sRGB authored color. */
export function hexColor(hex: string, alpha = 1): AuthoredColor {
  const n = parseInt(hex.slice(1), 16);
  return {
    Srgb: {
      r: ((n >> 16) & 0xff) / 255,
      g: ((n >> 8) & 0xff) / 255,
      b: (n & 0xff) / 255,
      a: alpha,
    },
  };
}

/** Render an authored color as "#rrggbb" for a color input (alpha dropped). */
export function colorToHex(color: AuthoredColor): string {
  const c =
    "Srgb" in color
      ? color.Srgb
      : // CMYK preview via the naive formula, mirroring the engine edge.
        {
          r: (1 - color.Cmyk.c) * (1 - color.Cmyk.k),
          g: (1 - color.Cmyk.m) * (1 - color.Cmyk.k),
          b: (1 - color.Cmyk.y) * (1 - color.Cmyk.k),
        };
  const h = (v: number) =>
    Math.round(Math.min(1, Math.max(0, v)) * 255)
      .toString(16)
      .padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

let wasmReady: Promise<unknown> | null = null;
let wasmMemory: WebAssembly.Memory | null = null;

/** Idempotent engine module initialization. */
export function initEngine(): Promise<unknown> {
  wasmReady ??= init().then((exports) => {
    wasmMemory = (exports as { memory: WebAssembly.Memory }).memory;
    return exports;
  });
  return wasmReady;
}

/** Linear memory of the engine module (frames are read from it in place). */
export function getWasmMemory(): WebAssembly.Memory {
  if (!wasmMemory) throw new Error("engine not initialized");
  return wasmMemory;
}

export { WasmSession };

export function sendCommand(session: WasmSession, cmd: Command): void {
  session.apply(JSON.stringify(cmd));
}

/** Apply a command as part of a live drag gesture (see Session::preview). */
export function sendPreview(session: WasmSession, cmd: Command): void {
  session.preview(JSON.stringify(cmd));
}
