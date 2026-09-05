/**
 * Typed wrapper around the WASM engine (generated bindings in ./wasm-pkg).
 *
 * The UI never mutates document state itself: it builds serde-JSON commands
 * mirroring `chitrakar_doc::Command`, sends them through `WasmSession.apply`,
 * and re-reads rendered pixels / layer state from the engine.
 */

import init, {
  WasmSession,
  display_p3_profile,
} from "./wasm-pkg/chitrakar_engine";

export type NodeId = number;
/** The W3C compositing modes, which is what SVG's mix-blend-mode and
 * PDF's /BM name too. */
export type BlendMode =
  | "Normal"
  | "Multiply"
  | "Screen"
  | "Overlay"
  | "Darken"
  | "Lighten"
  | "ColorDodge"
  | "ColorBurn"
  | "HardLight"
  | "SoftLight"
  | "Difference"
  | "Exclusion"
  | "Hue"
  | "Saturation"
  | "Color"
  | "Luminosity";

export type AuthoredColor =
  | { Srgb: { r: number; g: number; b: number; a: number } }
  | { Cmyk: { c: number; m: number; y: number; k: number; a: number } };

export type VectorShape =
  | { Rect: { width: number; height: number; radius: number } }
  | { Ellipse: { rx: number; ry: number } }
  | {
      Path: {
        points: [number, number][];
        closed: boolean;
        smooth: boolean;
        /** Per-anchor bezier control offsets, [inX, inY, outX, outY]. Empty
         * means a plain polyline; when present they override `smooth`. */
        handles: [number, number, number, number][];
        /** Extra closed rings, filled even-odd with the main one — what a
         * boolean operation leaves behind when it makes a hole. */
        subpaths: [number, number][][];
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
    }
  | {
      Levels: {
        in_black: number;
        in_white: number;
        gamma: number;
        out_black: number;
        out_white: number;
      };
    }
  | {
      /** A master curve every channel goes through, and a curve of its
       * own for each channel, run after it. An empty channel list is the
       * identity. */
      Curves: {
        points: [number, number][];
        red: [number, number][];
        green: [number, number][];
        blue: [number, number][];
      };
    }
  | { WhiteBalance: { temperature: number; tint: number } }
  | { Vibrance: { amount: number } }
  /** Monochrome, mixed by hand: the weights are normalized by their own
   * total, so moving one changes the mix and not the brightness. */
  | { BlackAndWhite: { red: number; green: number; blue: number } }
  /** Every tone replaced by the colour at its own place along a ramp. */
  | { GradientMap: { stops: GradientStop[] } }
  /** Turned inside out, on the values a device shows. */
  | { Invert: { amount: number } }
  /** Hue, saturation and lightness asked of one band of colour at a
   * time — six triples in red, yellow, green, cyan, blue, magenta
   * order, each a hue shift, a saturation change and a lightness
   * change. */
  | { SelectiveHsl: { bands: [number, number, number][] } }
  /** The two ends of the tone range moved without touching the middle:
   * shadows above zero opens up what is dark, highlights above zero
   * pulls back what is bright, and both run to -1 for the opposite. */
  | { ShadowsHighlights: { shadows: number; highlights: number } };

/** How much each channel contributes to brightness: the Rec. 709
 * weights, and the default recipe for a black-and-white conversion. */
export const LUMA: [number, number, number] = [0.2126, 0.7152, 0.0722];

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
  /** Lengths on and off in turn, repeating, in the shape's own units.
   * Empty is a solid line. */
  dash: number[];
  /** How the line ends where it stops, and at either end of every dash.
   * Paths only: a rect's or an ellipse's stroke is a band inside a
   * closed outline, which never stops. */
  cap: StrokeCap;
  /** How the line turns a corner. Paths only, for the same reason. */
  join: StrokeJoin;
  /** Which side of the outline the band lies on, for a rect or an
   * ellipse. `null` is what the shape has always been stroked as, which
   * is a band inside the edge. A path is stroked down the middle of its
   * line whatever this says. */
  align: StrokeAlign | null;
  /** What sits at the line's first point, and at its last. An open path
   * only, and where the line stops rather than where each dash does. */
  start_marker: Marker;
  end_marker: Marker;
}

export type Marker = "None" | "Arrow" | "Bar" | "Dot";

export type StrokeAlign = "Inside" | "Centre" | "Outside";
export type StrokeCap = "Butt" | "Round" | "Square";
export type StrokeJoin = "Miter" | "Round" | "Bevel";

export type Filter =
  | { GaussianBlur: { sigma: number } }
  | { Sharpen: { sigma: number; amount: number } }
  /** Squares of one colour each, the average of what they covered. The
   * grid is anchored in the document, so a block stays the same block
   * when the page is panned or zoomed. */
  | { Pixelate: { size: number } }
  /** Grain: a function of where a speck sits in the document and of the
   * seed, so the same page grains the same way every time it is drawn. */
  | { Noise: { amount: number; grain: number; mono: boolean; seed: number } }
  /** The corners taken down (or brought up), measured from the middle of
   * the page in document units so it sits on the picture. */
  | { Vignette: { amount: number; radius: number; softness: number } };

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

/** A live effect drawn around a layer, from the layer's own composite.
 * Offsets and blur are in the layer's parent space, so an effect follows
 * the group it is in. */
export type Effect =
  | {
      DropShadow: {
        dx: number;
        dy: number;
        blur: number;
        color: AuthoredColor;
        opacity: number;
      };
    }
  | { Outline: { width: number; color: AuthoredColor; opacity: number } }
  | {
      InnerShadow: {
        dx: number;
        dy: number;
        blur: number;
        color: AuthoredColor;
        opacity: number;
      };
    };

/** The variant name of an effect — its single key. */
export type EffectKind = "DropShadow" | "Outline" | "InnerShadow";

export function effectKind(e: Effect): EffectKind {
  return Object.keys(e)[0] as EffectKind;
}

/** The parameters an effect carries, whichever variant it is. */
export function effectBody(e: Effect): Record<string, number | AuthoredColor> {
  return Object.values(e)[0];
}

export type TextAlign = "Left" | "Center" | "Right";

/** A stretch of a block set differently from the rest: the same choices
 * the block itself makes, made again over a range of its text, with
 * `null` meaning "however the block does it". Offsets are into the
 * text's UTF-8 bytes, which is how the engine counts. */
export interface StyleRun {
  start: number;
  end: number;
  fill?: AuthoredColor | null;
  bold?: boolean | null;
  italic?: boolean | null;
  underline?: boolean | null;
  strike?: boolean | null;
  font?: string | null;
}

export interface TextSpec {
  text: string;
  size: number;
  fill: AuthoredColor;
  /** Where each line sits inside the block's own width. */
  align: TextAlign;
  /** Multiple of the font's natural line height. */
  line_height: number;
  /** Tracking, in ems, so it follows the size. */
  letter_spacing: number;
  /** Wrap width in document pixels; zero fits the text instead. */
  width: number;
  /** The face's registered name; empty is the bundled DejaVu Sans. */
  font: string;
  /** Italic: the face's oblique twin when one is registered, else a
   * lean the rasterizer synthesizes. */
  italic: boolean;
  /** Bold: the family's "… Bold" cut when one is registered, else a
   * thickening the rasterizer synthesizes. */
  bold: boolean;
  /** A line under each line of text, and one through it. */
  underline: boolean;
  strike: boolean;
  /** Stretches of the text set differently from the rest of it. */
  runs: StyleRun[];
  /** A guide in the block's own space to set the text along, and how far
   * along it the text starts. */
  along: VectorShape | null;
  along_offset: number;
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

/** What a layer does when the frame around it is given a new size. */
export type Pin = "Start" | "End" | "Middle" | "Stretch";
export interface Pinning {
  x: Pin;
  y: Pin;
}

/** One colour in the document's palette. */
export interface Swatch {
  name: string;
  color: AuthoredColor;
}

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
  | { Text: TextSpec }
  | { Instance: { of: NodeId } }
  | {
      /** A frame: a group with a size of its own that cuts its contents
       * to that box and grounds it. */
      Artboard: {
        width: number;
        height: number;
        background: AuthoredColor | null;
        /** How many pixels a pixel of the frame exports as: which
         * multiple belongs to which frame is a property of the frame. A
         * file written before this reads as 1. */
        export_scale: number;
      };
    };

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
  | { SetLocked: { id: NodeId; locked: boolean } }
  | { SetClipped: { id: NodeId; clipped: boolean } }
  | { SetPinning: { id: NodeId; pinned: Pinning } }
  | { SetBlendMode: { id: NodeId; blend: BlendMode } }
  | { SetTransform: { id: NodeId; transform: Transform } }
  | { SetKind: { id: NodeId; kind: NodeKind } }
  | { SetName: { id: NodeId; name: string } }
  | { SetMask: { id: NodeId; mask: Mask | null } }
  | { SetEffects: { id: NodeId; effects: Effect[] } }
  | { SetGuides: { guides: ({ Vertical: number } | { Horizontal: number })[] } }
  | { SetSwatches: { swatches: Swatch[] } }
  | { MoveNode: { id: NodeId; parent: NodeId; index: number } }
  /** Turn the page by any angle about its own middle and give it the size
   * it should have afterwards — what straightening a crooked horizon is
   * made of. `Session::straighten_size` answers the size. */
  | { StraightenCanvas: { degrees: number; width: number; height: number } }
  | { Batch: Command[] };

/** Mirror of `chitrakar_engine::LayerInfo`. */
export interface LayerInfo {
  id: NodeId;
  name: string;
  kind:
    | "group"
    | "vector"
    | "raster"
    | "adjustment"
    | "filter"
    | "text"
    | "paint"
    | "clone"
    | "artboard"
    | "instance";
  visible: boolean;
  opacity: number;
  blend: BlendMode;
  has_mask: boolean;
  painted_mask: boolean;
  has_effects: boolean;
  /** Cannot be picked or moved on the canvas. */
  locked: boolean;
  /** Shows only where the layer below it does, and goes when it goes. */
  clipped: boolean;
  /** What it does when the frame around it is resized. */
  pinned: Pinning;
  /** The layer this one is a live copy of, or 0 when it is not a copy. */
  copies: NodeId;
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

/** The Display P3 profile's own bytes, for a screen known to be P3 —
 * most of what Apple has shipped for years — without hunting down an
 * .icc file for it. */
export { display_p3_profile };
