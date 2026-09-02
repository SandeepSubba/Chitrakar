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
  | { Ellipse: { rx: number; ry: number } }
  | {
      Path: {
        points: [number, number][];
        closed: boolean;
        smooth: boolean;
        /** Per-anchor bezier control offsets, [inX, inY, outX, outY]. Empty
         * means a plain polyline; when present they override `smooth`. */
        handles: [number, number, number, number][];
      };
    };

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
  /** Per-anchor multipliers in 0..1 scaling `width` along the path. Empty
   * means a constant width. */
  widths: number[];
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

export interface TextSpec {
  text: string;
  size: number;
  fill: AuthoredColor;
}

export interface GradientStop {
  offset: number;
  color: AuthoredColor;
}

/** Gradient geometry is in the shape's own bounding box, 0..1 per axis. */
export type Gradient =
  | {
      Linear: {
        from: [number, number];
        to: [number, number];
        stops: GradientStop[];
      };
    }
  | { Radial: { center: [number, number]; radius: number; stops: GradientStop[] } };

export type NodeKind =
  | "Group"
  | {
      Vector: {
        shape: VectorShape;
        fill: AuthoredColor | null;
        stroke: Stroke | null;
        /** Paints in place of `fill` when set. */
        gradient: Gradient | null;
      };
    }
  | { Adjustment: Adjustment }
  | { Filter: Filter }
  | { Text: TextSpec };

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
  | { MoveNode: { id: NodeId; parent: NodeId; index: number } }
  | { Batch: Command[] };

/** Mirror of `chitrakar_engine::LayerInfo`. */
export interface LayerInfo {
  id: NodeId;
  name: string;
  kind: "group" | "vector" | "raster" | "adjustment" | "filter" | "text";
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

/** Parse "#rrggbb" into CMYK ink values (naive device conversion) — how
 * colors are authored in CMYK documents so a press profile can drive their
 * rendering and export. */
export function hexToCmykColor(hex: string, alpha = 1): AuthoredColor {
  const n = parseInt(hex.slice(1), 16);
  const r = ((n >> 16) & 0xff) / 255;
  const g = ((n >> 8) & 0xff) / 255;
  const b = (n & 0xff) / 255;
  const k = 1 - Math.max(r, g, b);
  const den = 1 - k;
  return {
    Cmyk: {
      c: den > 0 ? (1 - r - k) / den : 0,
      m: den > 0 ? (1 - g - k) / den : 0,
      y: den > 0 ? (1 - b - k) / den : 0,
      k,
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
