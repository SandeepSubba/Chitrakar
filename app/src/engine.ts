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

export type NodeKind =
  | "Group"
  | {
      Vector: {
        shape: VectorShape;
        fill: AuthoredColor | null;
        stroke: null;
      };
    }
  | { Adjustment: Adjustment };

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
  | { MoveNode: { id: NodeId; parent: NodeId; index: number } };

/** Mirror of `chitrakar_engine::LayerInfo`. */
export interface LayerInfo {
  id: NodeId;
  name: string;
  kind: "group" | "vector" | "raster" | "adjustment";
  visible: boolean;
  opacity: number;
  blend: BlendMode;
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
