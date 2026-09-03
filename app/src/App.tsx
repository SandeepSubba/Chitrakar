import { useCallback, useEffect, useRef, useState } from "react";
import { Icon, IconName } from "./icons";
import {
  Adjustment,
  BlendMode,
  AuthoredColor,
  Command,
  Effect,
  EffectKind,
  GradientStop,
  LayerInfo,
  Mask,
  NodeId,
  NodeKind,
  Transform,
  VectorShape,
  WasmSession,
  colorToHex,
  effectBody,
  effectKind,
  getWasmMemory,
  hexColor,
  hexToCmykColor,
  initEngine,
  nodePayload,
  sendCommand,
  sendPreview,
} from "./engine";

/** The colour a ramp shows partway between two stops.
 *
 * The engine blends stops in linear light like every other blend, so this
 * has to as well: interpolating the encoded sRGB values instead lands a
 * visibly different colour at the midpoint, and inserting a stop there
 * would bend a ramp that should have been left alone. CMYK stops resolve
 * through the press profile in the engine, which the UI cannot reproduce
 * without it, so those interpolate by ink — close enough to place a stop,
 * and it is the same ink the flat-fill editor authors. */
function mixAuthored(
  a: AuthoredColor,
  b: AuthoredColor,
  t: number,
): AuthoredColor {
  const at = (x: number, y: number) => x + (y - x) * t;
  if ("Srgb" in a && "Srgb" in b) {
    const lin = (v: number) =>
      v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
    const enc = (v: number) =>
      v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055;
    const mix = (x: number, y: number) => enc(at(lin(x), lin(y)));
    return {
      Srgb: {
        r: mix(a.Srgb.r, b.Srgb.r),
        g: mix(a.Srgb.g, b.Srgb.g),
        b: mix(a.Srgb.b, b.Srgb.b),
        a: at(a.Srgb.a, b.Srgb.a),
      },
    };
  }
  if ("Cmyk" in a && "Cmyk" in b) {
    return {
      Cmyk: {
        c: at(a.Cmyk.c, b.Cmyk.c),
        m: at(a.Cmyk.m, b.Cmyk.m),
        y: at(a.Cmyk.y, b.Cmyk.y),
        k: at(a.Cmyk.k, b.Cmyk.k),
        a: at(a.Cmyk.a, b.Cmyk.a),
      },
    };
  }
  return a;
}

/** `outer` applied after `inner` — the nesting rule, matching the engine's
 * Transform::compose. */
function composeT(outer: Transform, inner: Transform): Transform {
  return {
    a: outer.a * inner.a + outer.c * inner.b,
    b: outer.b * inner.a + outer.d * inner.b,
    c: outer.a * inner.c + outer.c * inner.d,
    d: outer.b * inner.c + outer.d * inner.d,
    e: outer.a * inner.e + outer.c * inner.f + outer.e,
    f: outer.b * inner.e + outer.d * inner.f + outer.f,
  };
}

/** A path's handles, padded to one per anchor so callers can index freely.
 * Stored empty when nothing is curved, which is what keeps older files (and
 * plain polylines) free of the field entirely. */
function withHandles(path: Extract<VectorShape, { Path: unknown }>["Path"]) {
  const zero = () => [0, 0, 0, 0] as [number, number, number, number];
  return path.points.map((_, i) => path.handles[i] ?? zero());
}

/** Explicit handles that reproduce exactly what the path already draws, so
 * converting hands you controls without changing the shape under you.
 *
 * A straight segment is a cubic whose controls lie on the chord at thirds,
 * and a Catmull-Rom spline is a cubic whose controls are a sixth of the
 * neighbour-to-neighbour span — so which seed preserves the shape depends on
 * whether `smooth` was on. */
function seedHandles(
  path: Extract<VectorShape, { Path: unknown }>["Path"],
): [number, number, number, number][] {
  const pts = path.points;
  const n = pts.length;
  const at = (i: number) =>
    path.closed ? pts[((i % n) + n) % n] : pts[Math.min(Math.max(i, 0), n - 1)];
  return pts.map((p, i) => {
    const [prev, next] = [at(i - 1), at(i + 1)];
    if (path.smooth) {
      const tx = (next[0] - prev[0]) / 6;
      const ty = (next[1] - prev[1]) / 6;
      return [-tx, -ty, tx, ty];
    }
    return [
      (prev[0] - p[0]) / 3,
      (prev[1] - p[1]) / 3,
      (next[0] - p[0]) / 3,
      (next[1] - p[1]) / 3,
    ];
  });
}

const TOOLS = [
  "Move",
  "Frame",
  "Rect",
  "Ellipse",
  "Pen",
  "Brush",
  "Paint",
  "Clone",
  "Text",
  "Crop",
  "Eyedropper",
] as const;
/** One letter per tool, the convention every editor shares. `v` for Move
 * because that is where the muscle memory is; `m` too, since the tool is
 * called Move here. */
const TOOL_KEYS: Record<string, (typeof TOOLS)[number]> = {
  v: "Move",
  m: "Move",
  f: "Frame",
  r: "Rect",
  e: "Ellipse",
  p: "Pen",
  b: "Brush",
  n: "Paint",
  s: "Clone",
  t: "Text",
  c: "Crop",
  i: "Eyedropper",
};

/** A glyph per layer kind, so the stack is scannable without reading the
 * type label at the end of every row. */
/** The layer kinds that hold other layers — what a row can be dropped
 * into, and what the panel walks down through. */
const HOLDS_CHILDREN = new Set(["group", "artboard"]);

const KIND_ICONS: Record<string, IconName> = {
  group: "group-layer",
  artboard: "frame",
  vector: "rect",
  raster: "image",
  adjustment: "adjust",
  filter: "filter",
  text: "text",
  paint: "paint",
  clone: "clone",
};

const TOOL_HINT: Record<(typeof TOOLS)[number], string> = {
  Move: "V",
  Frame: "F",
  Rect: "R",
  Ellipse: "E",
  Pen: "P",
  Brush: "B",
  Paint: "N",
  Clone: "S",
  Text: "T",
  Crop: "C",
  Eyedropper: "I",
};

const TOOL_ICONS: Record<(typeof TOOLS)[number], IconName> = {
  Move: "move",
  Frame: "frame",
  Rect: "rect",
  Ellipse: "ellipse",
  Pen: "pen",
  Brush: "brush",
  Paint: "paint",
  Clone: "clone",
  Text: "text",
  Crop: "crop",
  Eyedropper: "eyedropper",
};
type Tool = (typeof TOOLS)[number];
const BLEND_MODES: BlendMode[] = ["Normal", "Multiply", "Screen"];
/** Minimum travel between recorded brush samples, and how far a simplified
 * stroke may stray from the one that was drawn — both in document units. */
const BRUSH_STEP = 3;
/** How many pixels a layer's picture in the panel is across. Twice the
 * size it is shown at, so it stays crisp on a display that has the
 * pixels for it. */
const THUMB = 36;
const BRUSH_TOLERANCE = 2;

/** The colour the page shows at a document point, as a hex string, or
 * null where the page shows nothing. The composite rather than the layer
 * under the cursor: what the eye is pointing at is what it takes. */
function colorUnder(
  session: WasmSession,
  x: number,
  y: number,
): string | null {
  const c = session.color_at(x, y);
  if (c.length !== 4 || c[3] === 0) return null;
  return `#${[c[0], c[1], c[2]]
    .map((v) => v.toString(16).padStart(2, "0"))
    .join("")}`;
}

/** Ramer-Douglas-Peucker, returning the *indices* it keeps so per-sample
 * data (the widths) can be carried along with the points. A raw stroke is
 * hundreds of points at screen resolution, which renders the same and
 * cannot be edited by hand; this keeps the shape within `tol` of what was
 * drawn and leaves a handful of anchors to grab afterwards. */
function simplifyStroke(pts: [number, number][], tol: number): number[] {
  const walk = (lo: number, hi: number): number[] => {
    let worst = 0;
    let at = lo;
    for (let i = lo + 1; i < hi; i++) {
      const d = pointToSegment(pts[i], pts[lo], pts[hi]);
      if (d > worst) {
        worst = d;
        at = i;
      }
    }
    if (worst <= tol) return [lo, hi];
    return [...walk(lo, at).slice(0, -1), ...walk(at, hi)];
  };
  if (pts.length < 3) return pts.map((_, i) => i);
  return walk(0, pts.length - 1);
}

/** Stroke width from how fast the cursor is travelling, in document units
 * per millisecond: slow strokes lay down a full-width line, fast ones thin
 * out. Clamped so a flick still leaves a visible mark. */
function speedWidth(speed: number): number {
  return Math.max(0.25, Math.min(1, 1 - speed * 0.35));
}

function pointToSegment(
  p: [number, number],
  a: [number, number],
  b: [number, number],
): number {
  const [dx, dy] = [b[0] - a[0], b[1] - a[1]];
  const len2 = dx * dx + dy * dy;
  const t =
    len2 < 1e-9
      ? 0
      : Math.max(
          0,
          Math.min(1, ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2),
        );
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}

/** Is this event going into something that takes text? Only then should a
 * bare letter or Delete mean a character rather than a shortcut. A checkbox
 * or a slider is focusable and ignores both, so treating every input as
 * text entry silently disables the shortcuts after any panel click. */
/** The draft of the open document, kept in IndexedDB so it survives a
 * closed tab and a crash. One record; a .chitra with images can run past
 * what localStorage holds, and IndexedDB takes bytes as they are. */
const DRAFT_DB = "chitrakar";
const DRAFT_STORE = "drafts";
function draftDb(): Promise<IDBDatabase | null> {
  return new Promise((resolve) => {
    try {
      const req = indexedDB.open(DRAFT_DB, 1);
      req.onupgradeneeded = () => req.result.createObjectStore(DRAFT_STORE);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => resolve(null);
    } catch {
      resolve(null);
    }
  });
}
async function putDraft(bytes: Uint8Array): Promise<void> {
  const db = await draftDb();
  if (!db) return;
  db.transaction(DRAFT_STORE, "readwrite")
    .objectStore(DRAFT_STORE)
    .put(bytes, "current");
}
async function getDraft(): Promise<Uint8Array | null> {
  const db = await draftDb();
  if (!db) return null;
  return new Promise((resolve) => {
    const req = db
      .transaction(DRAFT_STORE)
      .objectStore(DRAFT_STORE)
      .get("current");
    req.onsuccess = () =>
      resolve(req.result instanceof Uint8Array ? req.result : null);
    req.onerror = () => resolve(null);
  });
}
/** The draft's name, kept beside it so a restored document is still
 * called what it was called. */
async function putDraftName(name: string): Promise<void> {
  const db = await draftDb();
  if (!db) return;
  db.transaction(DRAFT_STORE, "readwrite")
    .objectStore(DRAFT_STORE)
    .put(name, "name");
}
async function getDraftName(): Promise<string | null> {
  const db = await draftDb();
  if (!db) return null;
  return new Promise((resolve) => {
    const req = db
      .transaction(DRAFT_STORE)
      .objectStore(DRAFT_STORE)
      .get("name");
    req.onsuccess = () =>
      resolve(typeof req.result === "string" ? req.result : null);
    req.onerror = () => resolve(null);
  });
}
async function clearDraft(): Promise<void> {
  const db = await draftDb();
  if (!db) return;
  const store = db
    .transaction(DRAFT_STORE, "readwrite")
    .objectStore(DRAFT_STORE);
  store.delete("current");
  store.delete("name");
}

function isTextEntry(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  if (!(target instanceof HTMLInputElement)) return false;
  return [
    "text",
    "number",
    "search",
    "email",
    "url",
    "tel",
    "password",
  ].includes(target.type);
}

const HANDLES = ["nw", "ne", "sw", "se"] as const;
/** Which corner of the selection quad (tl, tr, br, bl) each handle sits on. */
const HANDLE_CORNER = [0, 1, 3, 2];
type Handle = (typeof HANDLES)[number];

/** Default canvas for a new document; any size can be chosen, and an
 * opened file brings its own. */
const DOC_WIDTH = 1280;
const DOC_HEIGHT = 720;

/** Starting points offered in the new-document dialog: name, pixels, and
 * the resolution that gives them their size on paper. */
const DOC_PRESETS: [string, number, number, number][] = [
  ["HD 1280×720", 1280, 720, 72],
  ["Full HD 1920×1080", 1920, 1080, 72],
  ["Square 1080×1080", 1080, 1080, 72],
  ["A4 at 300dpi", 2480, 3508, 300],
  ["Postcard at 300dpi", 1748, 1240, 300],
];

/** Units the rulers and the geometry fields can read in. Pixels are the
 * document's own; the others go through its resolution. */
type Units = "px" | "mm" | "in";
const UNIT_LABELS: Record<Units, string> = {
  px: "Pixels",
  mm: "Millimetres",
  in: "Inches",
};
/** How many of the unit one document pixel is. */
function perPixel(units: Units, dpi: number): number {
  return units === "px" ? 1 : units === "mm" ? 25.4 / dpi : 1 / dpi;
}
/** Tick spacings a ruler chooses between, per unit. */
const UNIT_TICKS: Record<Units, number[]> = {
  px: [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000],
  mm: [0.5, 1, 2, 5, 10, 25, 50, 100, 250, 500, 1000],
  in: [0.125, 0.25, 0.5, 1, 2, 5, 10, 25, 50, 100],
};
/** A length in the unit, trimmed to what the unit can tell apart. */
function inUnits(px: number, units: Units, dpi: number): number {
  const v = px * perPixel(units, dpi);
  const places = units === "px" ? 2 : units === "mm" ? 2 : 3;
  return Math.round(v * 10 ** places) / 10 ** places;
}
const MIN_ZOOM = 0.05;
const MAX_ZOOM = 8;
const MIN_SIZE = 2; // doc pixels, resize clamp

interface View {
  zoom: number;
  x: number;
  y: number;
}

interface ToolDrag {
  tool: Tool;
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  moved: boolean;
  /** Move tool: the node being dragged and its full starting transform. */
  target?: NodeId;
  t0?: Transform;
  /** Move tool: every layer travelling with the drag, each with its own
   * starting transform, since each sits in its own parent space. */
  moving?: { id: NodeId; t0: Transform }[];
  /** Brush: the stroke so far, in document coordinates. */
  stroke?: [number, number][];
  /** Brush: a width multiplier per recorded sample. */
  widths?: number[];
  /** Brush: when the last sample was taken, for the speed that sets it. */
  lastAt?: number;
  /** Move: the dragged layer's document-space box when the drag began, and
   * the edges and centres worth snapping it to. */
  b0?: [number, number, number, number];
  snapX?: number[];
  snapY?: number[];
}

/** Effect names as the panel shows them, in the order the picker offers. */
const EFFECT_LABELS: Record<EffectKind, string> = {
  DropShadow: "Drop shadow",
  Outline: "Outline",
  InnerShadow: "Inner shadow",
};

/** The tunable numbers of each effect: [field, label, min, max, step]. */
const EFFECT_FIELDS: Record<
  EffectKind,
  [string, string, number, number, number][]
> = {
  DropShadow: [
    ["dx", "X", -60, 60, 1],
    ["dy", "Y", -60, 60, 1],
    ["blur", "Blur", 0, 40, 0.5],
    ["opacity", "Strength", 0, 1, 0.01],
  ],
  Outline: [
    ["width", "Width", 0.5, 40, 0.5],
    ["opacity", "Strength", 0, 1, 0.01],
  ],
  InnerShadow: [
    ["dx", "X", -40, 40, 1],
    ["dy", "Y", -40, 40, 1],
    ["blur", "Blur", 0, 30, 0.5],
    ["opacity", "Strength", 0, 1, 0.01],
  ],
};

/** A newly added effect, with defaults that read as the effect's name at
 * a glance rather than as nothing at all. */
function newEffect(kind: EffectKind, color: AuthoredColor): Effect {
  switch (kind) {
    case "Outline":
      return { Outline: { width: 4, color, opacity: 1 } };
    case "InnerShadow":
      return { InnerShadow: { dx: 4, dy: 4, blur: 6, color, opacity: 0.5 } };
    default:
      return { DropShadow: { dx: 6, dy: 6, blur: 6, color, opacity: 0.45 } };
  }
}

/** Alignment guides currently showing, in document coordinates. */
interface Guides {
  x: number[];
  y: number[];
}

/** Thickness of the rulers, in CSS pixels. */
const RULER = 18;

/** A guide the user placed, mirroring `chitrakar_doc::Guide`. */
type DocGuide = { Vertical: number } | { Horizontal: number };

const guideAt = (g: DocGuide) => ("Vertical" in g ? g.Vertical : g.Horizontal);
const guideIsVertical = (g: DocGuide) => "Vertical" in g;

/** How close, in screen pixels, an edge has to come before it snaps. A
 * fixed screen distance rather than a document one, so snapping feels the
 * same however far you are zoomed in. */
const SNAP_PX = 6;

/** The three lines a box offers on each axis: its edges and its middle. */
const snapLines = (lo: number, hi: number) => [lo, (lo + hi) / 2, hi];

/** Nudge `moving` onto the nearest of `targets`, if one is within `tol`.
 * Returns the correction to apply and the line it landed on. */
function snapAxis(
  moving: number[],
  targets: number[],
  tol: number,
): { delta: number; guide: number | null } {
  let best = { delta: 0, guide: null as number | null };
  let bestGap = tol;
  for (const m of moving) {
    for (const t of targets) {
      const gap = Math.abs(t - m);
      if (gap < bestGap) {
        bestGap = gap;
        best = { delta: t - m, guide: t };
      }
    }
  }
  return best;
}

interface HandleDrag {
  corner: Handle;
  id: NodeId;
  t0: Transform;
  b0: [number, number, number, number];
  /** Lines the dragged corner can catch on, as for a move. */
  snapX?: number[];
  snapY?: number[];
}

interface PanDrag {
  pointerX: number;
  pointerY: number;
  viewX: number;
  viewY: number;
}

const toTransform = (v: ArrayLike<number>): Transform => ({
  a: v[0],
  b: v[1],
  c: v[2],
  d: v[3],
  e: v[4],
  f: v[5],
});

export function App() {
  const [session, setSession] = useState<WasmSession | null>(null);
  const [tool, setTool] = useState<Tool>("Move");
  const [fill, setFill] = useState("#6c8cff");
  const [brushSize, setBrushSize] = useState(8);
  /** The paint brush: how wide it is in document pixels, how much of that
   * width its edge fades over, and whether it is rubbing paint out
   * instead of laying it down. */
  const [paintSize, setPaintSize] = useState(24);
  const [paintSoftness, setPaintSoftness] = useState(0.5);
  const [erasing, setErasing] = useState(false);
  /** Whether the clone brush heals: laying the source's texture down in
   * the colour of the place it lands, rather than as it found it. */
  const [healing, setHealing] = useState(true);
  /** Where the brush is hovering, in the canvas's own coordinates, so the
   * ring that shows how big it is can sit under the pointer. Null when the
   * pointer is not over the canvas, or the brush is not the tool in hand. */
  const [brushAt, setBrushAt] = useState<[number, number] | null>(null);
  /** Where the clone tool reads from, in document units. Alt-click sets
   * it; until it is set the tool has nothing to lift and says so. */
  const [cloneFrom, setCloneFrom] = useState<[number, number] | null>(null);
  /** A small picture of each layer, by id, for the panel. Regenerated a
   * breath after the document settles rather than on every frame: a drag
   * refreshes the layer list many times a second and none of those frames
   * is worth a re-render of the whole stack. */
  const [thumbs, setThumbs] = useState<Record<number, string>>({});
  /** And a picture of each layer's mask, fitted the same way, so the row
   * shows what is being let through as well as what is under it. */
  const [maskThumbs, setMaskThumbs] = useState<Record<number, string>>({});
  /** The live document's pixel size. Every screen/document conversion goes
   * through this rather than a constant, so an opened file of any size
   * lands on a canvas that fits it. */
  const [docSize, setDocSize] = useState<[number, number]>([
    DOC_WIDTH,
    DOC_HEIGHT,
  ]);
  /** The same size, for callbacks that must stay referentially stable:
   * fitView is a dependency of newDocument, which is a dependency of the
   * one-shot startup effect, so reading state there would rebuild the
   * document on every render. */
  const docSizeRef = useRef<[number, number]>([DOC_WIDTH, DOC_HEIGHT]);
  const setDocumentSize = useCallback((w: number, h: number) => {
    docSizeRef.current = [w, h];
    setDocSize([w, h]);
  }, []);
  const [newDocOpen, setNewDocOpen] = useState(false);
  /** The sheet of keys and gestures, opened with "?" or from the View
   * menu: half of what this editor can do is a gesture nobody would
   * guess at, and a menu cannot show a gesture. */
  const [showKeys, setShowKeys] = useState(false);
  const [layers, setLayers] = useState<LayerInfo[]>([]);
  const [selected, setSelected] = useState<NodeId | null>(null);
  const [cmyk, setCmyk] = useState(false);
  /** Which top-level menu is open, if any. */
  const [openMenu, setOpenMenu] = useState<"file" | "edit" | "view" | null>(
    null,
  );
  const openInputRef = useRef<HTMLInputElement>(null);
  const placeInputRef = useRef<HTMLInputElement>(null);
  const iccInputRef = useRef<HTMLInputElement>(null);
  const fontInputRef = useRef<HTMLInputElement>(null);
  const pick = (ref: React.RefObject<HTMLInputElement>) => ref.current?.click();
  const [view, setView] = useState<View>({ zoom: 1, x: 0, y: 0 });
  /** The open document's resolution, and the units the rulers and the
   * geometry fields read in — the latter remembered across visits. */
  const [docDpi, setDocDpi] = useState(72);
  const [units, setUnitsState] = useState<Units>(() => {
    try {
      const saved = localStorage.getItem("chitrakar.units");
      return saved === "mm" || saved === "in" ? saved : "px";
    } catch {
      return "px";
    }
  });
  const setUnits = (u: Units) => {
    setUnitsState(u);
    try {
      localStorage.setItem("chitrakar.units", u);
    } catch {
      // Not remembered, then.
    }
  };
  /** The viewport's size in CSS pixels. The canvas covers it, and the
   * engine composites only what fits — so a print-sized page costs a
   * screenful of pixels to show, not nine megapixels. */
  const [viewport, setViewport] = useState<[number, number]>([1, 1]);
  const dpr = typeof window === "undefined" ? 1 : window.devicePixelRatio || 1;
  const [opacityDraft, setOpacityDraft] = useState<number | null>(null);
  /** Whether a paste event followed the last Ctrl+V; see the keydown. */
  const pasteSeen = useRef(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const imgDataRef = useRef<ImageData | null>(null);
  const toolDragRef = useRef<ToolDrag | null>(null);
  const handleDragRef = useRef<HandleDrag | null>(null);
  const panDragRef = useRef<PanDrag | null>(null);
  const spaceRef = useRef(false);
  const shapeCount = useRef(0);
  const frameCount = useRef(0);
  const paintCount = useRef(0);
  /** Where the last paint sample landed, for the speed the width follows. */
  const lastPaint = useRef<[number, number]>([0, 0]);
  /** Where the last stroke ended, so shift-clicking can draw a straight
   * line on from there — what a brush does everywhere else. */
  const strokeEnd = useRef<[number, number] | null>(null);
  /** Pen tool: anchors of the path being drawn, in doc coordinates. */
  const [penPoints, setPenPoints] = useState<[number, number][]>([]);
  /** A draft from an earlier visit, offered back until it is taken or
   * thrown away; and the draft-writer, a breath after the last change.
   * `saveTick` is bumped on every refresh, so the draft follows the
   * document. */
  const [recoverable, setRecoverable] = useState<Uint8Array | null>(null);
  const [saveTick, setSaveTick] = useState(0);
  /** What this document is called: the name it was opened under, or the
   * one typed in the bar. Every save and every export is named after it,
   * so a file keeps its name through a session. */
  const [docName, setDocName] = useState("untitled");
  const draftName = useRef<string | null>(null);
  useEffect(() => {
    getDraft().then(
      (bytes) => bytes && bytes.length > 0 && setRecoverable(bytes),
    );
    getDraftName().then((name) => (draftName.current = name));
  }, []);
  useEffect(() => {
    // Nothing to keep until there is something on the page: the empty
    // document every visit starts with must not overwrite a draft that
    // has not been taken back yet.
    if (!session || saveTick === 0 || layers.length === 0) return;
    const t = setTimeout(() => {
      try {
        putDraft(session.save());
        putDraftName(docName);
      } catch {
        // A draft that cannot be written is not worth an alert.
      }
    }, 1500);
    return () => clearTimeout(t);
  }, [session, saveTick, layers.length, docName]);

  /** Faces a text block can be set in. The bundled one is always there;
   * the rest are fetched from /fonts once per page load and registered
   * with the engine, which keeps them for good. */
  const [fontNames, setFontNames] = useState<string[]>(["DejaVu Sans"]);
  const fontsLoaded = useRef(false);
  useEffect(() => {
    if (!session || fontsLoaded.current) return;
    fontsLoaded.current = true;
    const faces: [string, string][] = [
      ["DejaVu Sans Bold", "/fonts/DejaVuSans-Bold.ttf"],
      ["DejaVu Serif", "/fonts/DejaVuSerif.ttf"],
      ["DejaVu Sans Mono", "/fonts/DejaVuSansMono.ttf"],
      ["DejaVu Sans Mono Oblique", "/fonts/DejaVuSansMono-Oblique.ttf"],
    ];
    Promise.all(
      faces.map(async ([name, url]) => {
        try {
          const res = await fetch(url);
          if (!res.ok) return;
          WasmSession.register_font(
            name,
            new Uint8Array(await res.arrayBuffer()),
          );
        } catch {
          // A face that will not load is simply not offered.
        }
      }),
    ).then(() =>
      setFontNames(JSON.parse(WasmSession.font_names()) as string[]),
    );
  }, [session]);

  /** Alignment guides drawn while a drag is snapped to something. */
  const [guides, setGuides] = useState<Guides>({ x: [], y: [] });
  /** The guides the user has placed, read back from the document. */
  const [docGuides, setDocGuides] = useState<DocGuide[]>([]);
  const [showGuides, setShowGuides] = useState(true);
  /** A guide being dragged: out of a ruler (index null) or an existing one
   * being moved. `at` is where it currently sits, in document units. */
  const [guideDrag, setGuideDrag] = useState<{
    vertical: boolean;
    index: number | null;
    at: number;
  } | null>(null);
  /** The crop frame being dragged, in host coordinates: [x0, y0, x1, y1]. */
  const [cropRect, setCropRect] = useState<
    [number, number, number, number] | null
  >(null);
  /** Extra layers picked with ctrl/cmd-click, beyond the primary selection. */
  const [multiSel, setMultiSel] = useState<NodeId[]>([]);
  const groupCount = useRef(0);

  const refresh = useCallback(
    (s: WasmSession) => {
      setSaveTick((n) => n + 1);
      const canvas = canvasRef.current;
      if (canvas) {
        const ctx = canvas.getContext("2d")!;
        const dirty = s.render_frame();
        if (
          !imgDataRef.current ||
          imgDataRef.current.width !== s.frame_width() ||
          imgDataRef.current.height !== s.frame_height()
        ) {
          imgDataRef.current = new ImageData(s.frame_width(), s.frame_height());
        }
        if (dirty.length === 4) {
          // Zero-copy view into wasm memory; ImageData needs its own buffer.
          const view = new Uint8ClampedArray(
            getWasmMemory().buffer,
            s.frame_ptr(),
            s.frame_len(),
          );
          imgDataRef.current.data.set(view);
          ctx.putImageData(
            imgDataRef.current,
            0,
            0,
            dirty[0],
            dirty[1],
            dirty[2],
            dirty[3],
          );
        }
      }
      setLayers(JSON.parse(s.layers_json()) as LayerInfo[]);
      setDocGuides(JSON.parse(s.guides_json()) as DocGuide[]);
      // The page's size is document state like anything else — undoing a
      // crop changes it — so it is read back here rather than only being
      // written where a crop or a new document sets it.
      if (
        s.width !== docSizeRef.current[0] ||
        s.height !== docSizeRef.current[1]
      ) {
        setDocumentSize(s.width, s.height);
      }
    },
    [setDocumentSize],
  );

  const fitView = useCallback(() => {
    const host = hostRef.current;
    if (!host) return;
    const zoom =
      Math.min(
        host.clientWidth / docSizeRef.current[0],
        host.clientHeight / docSizeRef.current[1],
      ) * 0.9;
    setView({
      zoom,
      x: (host.clientWidth - docSizeRef.current[0] * zoom) / 2,
      y: (host.clientHeight - docSizeRef.current[1] * zoom) / 2,
    });
  }, []);

  /** Zoom about the centre of the viewport, so the View menu behaves like
   * the wheel does under the cursor. */
  const zoomBy = useCallback((factor: number) => {
    const host = hostRef.current;
    if (!host) return;
    setView((v) => {
      const zoom = Math.min(8, Math.max(0.05, v.zoom * factor));
      const k = zoom / v.zoom;
      const [cx, cy] = [host.clientWidth / 2, host.clientHeight / 2];
      return { zoom, x: cx - (cx - v.x) * k, y: cy - (cy - v.y) * k };
    });
  }, []);

  /** Set the zoom outright, keeping the viewport's centre where it is. */
  const zoomTo = useCallback((zoom: number) => {
    const host = hostRef.current;
    if (!host) return;
    setView((v) => {
      const k = Math.min(8, Math.max(0.05, zoom)) / v.zoom;
      const [cx, cy] = [host.clientWidth / 2, host.clientHeight / 2];
      return {
        zoom: v.zoom * k,
        x: cx - (cx - v.x) * k,
        y: cy - (cy - v.y) * k,
      };
    });
  }, []);

  const newDocument = useCallback(
    (useCmyk: boolean, width = DOC_WIDTH, height = DOC_HEIGHT, dpi = 72) => {
      const s = new WasmSession(width, height, useCmyk);
      s.set_dpi(dpi);
      setDocDpi(dpi);
      setSession(s);
      setDocumentSize(width, height);
      setCmyk(useCmyk);
      setSelected(null);
      setHasIcc(false);
      setProofing(false);
      setGamutWarn(false);
      shapeCount.current = 0;
      paintCount.current = 0;
      setCloneFrom(null);
      setDocName("untitled");
      refresh(s);
      fitView();
    },
    [refresh, fitView, setDocumentSize],
  );

  useEffect(() => {
    let cancelled = false;
    initEngine().then(() => {
      if (!cancelled) newDocument(false);
    });
    return () => {
      cancelled = true;
    };
  }, [newDocument]);

  const run = useCallback(
    (cmd: Command) => {
      if (!session) return;
      sendCommand(session, cmd);
      refresh(session);
    },
    [session, refresh],
  );

  const preview = useCallback(
    (cmd: Command) => {
      if (!session) return;
      sendPreview(session, cmd);
      refresh(session);
    },
    [session, refresh],
  );

  /** After an undo or redo, point the selection at the layer it touched —
   * undoing a delete brings the layer back and should bring the selection
   * back with it — and drop it when that layer is gone. */
  const followHistory = useCallback(
    (s: WasmSession) => {
      const alive = new Set(
        (JSON.parse(s.layers_json()) as LayerInfo[]).map((l) => l.id as NodeId),
      );
      const touched = s.last_touched_node();
      // A selection that survived the step is left alone: undoing an edit
      // to one layer while another is picked should not steal the pick.
      // Only a selection the step took away — a delete undone, an add
      // redone — is pointed at what came back.
      setSelected((prev) =>
        prev !== null && alive.has(prev)
          ? prev
          : touched === undefined
            ? null
            : touched,
      );
      setMultiSel((prev) => prev.filter((id) => alive.has(id)));
      refresh(s);
    },
    [refresh],
  );

  const undo = useCallback(() => {
    if (session?.undo()) followHistory(session);
  }, [session, followHistory]);
  const redo = useCallback(() => {
    if (session?.redo()) followHistory(session);
  }, [session, followHistory]);

  /** Pick every top-level layer. */
  const selectAll = useCallback(() => {
    const top = layers.filter((l) => l.depth === 0).map((l) => l.id as NodeId);
    if (top.length === 0) return;
    setSelected(top[0]);
    setMultiSel(top);
  }, [layers]);

  const deselect = useCallback(() => {
    setSelected(null);
    setMultiSel([]);
  }, []);

  const cancelGesture = useCallback(() => {
    if (!session) return;
    toolDragRef.current = null;
    handleDragRef.current = null;
    setPenPoints([]);
    if (session.cancel_preview()) refresh(session);
  }, [session, refresh]);

  /** Commit the pen path being drawn: closed shapes get the current fill,
   * open polylines get a stroke. Anchors normalize to a (0,0) origin with
   * the offset carried by the node transform. */
  const finishPath = useCallback(
    (closed: boolean) => {
      if (!session) return;
      setPenPoints((pts) => {
        if (pts.length < (closed ? 3 : 2)) return pts;
        const minX = Math.min(...pts.map((p) => p[0]));
        const minY = Math.min(...pts.map((p) => p[1]));
        const points = pts.map(
          (p) => [p[0] - minX, p[1] - minY] as [number, number],
        );
        shapeCount.current += 1;
        run({
          AddNode: {
            parent: session.root_id,
            index: topLevelCount(layers),
            node: nodePayload(
              `Path ${shapeCount.current}`,
              {
                Vector: {
                  shape: {
                    Path: {
                      points,
                      closed,
                      smooth: false,
                      handles: [],
                      subpaths: [],
                    },
                  },
                  fill: closed
                    ? cmyk
                      ? hexToCmykColor(fill)
                      : hexColor(fill)
                    : null,
                  stroke: closed
                    ? null
                    : { color: hexColor(fill), width: 4, widths: [] },
                  gradient: null,
                },
              },
              minX,
              minY,
            ),
          },
        });
        return [];
      });
    },
    [session, layers, fill, cmyk, run],
  );

  // Keyboard: tool shortcuts, undo/redo, space-to-pan, escape-to-cancel.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      // A single letter is a tool switch, but only when it isn't being typed
      // into something: a text layer's content is edited in a textarea and a
      // layer is renamed in an input, and both contain the letters below.
      const typing =
        isTextEntry(e.target) || e.target instanceof HTMLSelectElement;
      if (!typing && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const shortcut = TOOL_KEYS[e.key.toLowerCase()];
        if (shortcut) {
          e.preventDefault();
          setTool(shortcut);
          setPenPoints([]);
        }
        // Brackets resize the brush, as they do everywhere else. By a
        // ratio rather than a step, so the small sizes stay adjustable
        // and the large ones do not take forever.
        if (e.key === "[" || e.key === "]") {
          e.preventDefault();
          setPaintSize((size) => {
            // Rounded away from where it started, so a small brush can
            // still be made smaller rather than rounding back to itself.
            const next =
              e.key === "]" ? Math.ceil(size * 1.25) : Math.floor(size / 1.25);
            return Math.max(1, Math.min(200, next));
          });
        }
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
      // Escape cancels whatever is in flight; with nothing in flight it
      // drops the selection, which is what every editor does with it.
      if (e.key === "Escape") {
        if (showKeys) {
          setShowKeys(false);
          return;
        }
        cancelGesture();
        deselect();
      }
      if (!typing && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a") {
        e.preventDefault();
        selectAll();
      }
      // "?" opens the sheet of shortcuts, and closes it again.
      if (!typing && e.key === "?") {
        e.preventDefault();
        setShowKeys((open) => !open);
      }
      if (e.key === "Enter" && !(e.target instanceof HTMLInputElement)) {
        finishPath(false); // pen tool: finish as an open (stroked) path
      }
      if (e.code === "Space" && !(e.target instanceof HTMLInputElement)) {
        spaceRef.current = true;
        e.preventDefault();
      }
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.code === "Space") spaceRef.current = false;
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
    };
    // `showKeys` is in here because Escape reads it: without it the
    // listener would keep a closure from before the sheet opened.
  }, [undo, redo, cancelGesture, finishPath, selectAll, deselect, showKeys]);

  // Keep the viewport measurement in step with the element it describes.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const measure = () =>
      setViewport(([w, h]) =>
        w === host.clientWidth && h === host.clientHeight
          ? [w, h]
          : [host.clientWidth, host.clientHeight],
      );
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(host);
    return () => observer.disconnect();
  }, []);

  // Tell the engine what the canvas is looking at, then repaint. The
  // canvas is a window onto the page now, so its backing store follows the
  // viewport rather than the document, and every pan or zoom is a fresh
  // frame — which is cheap, because it is only ever a screenful.
  useEffect(() => {
    if (!session) return;
    session.set_viewport(
      view.zoom * dpr,
      view.x * dpr,
      view.y * dpr,
      Math.round(viewport[0] * dpr),
      Math.round(viewport[1] * dpr),
    );
    // Resizing the element clears its backing store, and the engine tracks
    // what it has already presented, so it has to be told.
    session.invalidate();
    refresh(session);
  }, [session, view, viewport, dpr, docSize, refresh]);

  // Wheel zoom toward the cursor. Attached manually: React wheel listeners
  // are passive, and we must preventDefault to stop page scroll.
  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const rect = host.getBoundingClientRect();
      const cx = e.clientX - rect.left;
      const cy = e.clientY - rect.top;
      setView((v) => {
        const zoom = Math.min(
          MAX_ZOOM,
          Math.max(MIN_ZOOM, v.zoom * Math.exp(-e.deltaY * 0.0015)),
        );
        // Keep the document point under the cursor fixed.
        return {
          zoom,
          x: cx - ((cx - v.x) / v.zoom) * zoom,
          y: cy - ((cy - v.y) / v.zoom) * zoom,
        };
      });
    };
    host.addEventListener("wheel", onWheel, { passive: false });
    return () => host.removeEventListener("wheel", onWheel);
  }, []);

  /** Pointer position in document pixels. The canvas covers the whole
   * viewport, so the page's position within it is the view, not the
   * element's box. */
  const docPoint = (e: {
    clientX: number;
    clientY: number;
  }): [number, number] => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return [
      (e.clientX - rect.left - view.x) / view.zoom,
      (e.clientY - rect.top - view.y) / view.zoom,
    ];
  };

  /** Invert an affine, or null when it collapses space. */
  const inverseOf = (t: Transform) => {
    const det = t.a * t.d - t.b * t.c;
    if (Math.abs(det) < 1e-9) return null;
    return (x: number, y: number): [number, number] => {
      const [ux, uy] = [x - t.e, y - t.f];
      return [(t.d * ux - t.c * uy) / det, (t.a * uy - t.b * ux) / det];
    };
  };

  /** A pointer position in the space the selected layer's transform is
   * written against — its parent's. Identity for a top-level layer; for one
   * inside a moved group, this is what keeps a drag from fighting the
   * group's transform. */
  const layerPoint = (
    e: { clientX: number; clientY: number },
    id: NodeId | null = selected,
    /** Treat the coordinates as document units already, not screen ones. */
    inDoc = false,
  ): [number, number] => {
    const [x, y] = inDoc ? [e.clientX, e.clientY] : docPoint(e);
    if (!session || id === null) return [x, y];
    const inv = inverseOf(toTransform(session.parent_space_of(id)));
    return inv ? inv(x, y) : [x, y];
  };

  /** A displacement carried into a layer's parent space: a vector, so only
   * the linear part of the space applies. */
  const layerVector = (
    id: NodeId,
    dx: number,
    dy: number,
  ): [number, number] => {
    if (!session) return [dx, dy];
    const t = toTransform(session.parent_space_of(id));
    const det = t.a * t.d - t.b * t.c;
    if (Math.abs(det) < 1e-9) return [dx, dy];
    return [(t.d * dx - t.c * dy) / det, (t.a * dy - t.b * dx) / det];
  };

  /** Is `id` inside the currently selected group? The layers list is
   * depth-first, so a group's subtree is the run of rows after it with a
   * greater depth. */
  const inSelectedGroup = (id: NodeId) => {
    if (selected === null || id === selected) return false;
    const at = layers.findIndex((l) => l.id === selected);
    if (at < 0 || !HOLDS_CHILDREN.has(layers[at].kind)) return false;
    for (let i = at + 1; i < layers.length; i++) {
      if (layers[i].depth <= layers[at].depth) break;
      if (layers[i].id === id) return true;
    }
    return false;
  };

  /** Whether `id` travels with `target` — it is the node itself, one of its
   * ancestors, or one of its descendants. Such a layer is no use to snap
   * against, because it moves too. */
  const relatedTo = (id: NodeId, target: NodeId): boolean => {
    if (id === target) return true;
    const parentOf = (n: NodeId) =>
      layers.find((l) => l.id === n)?.parent as NodeId | undefined;
    const climbs = (from: NodeId, to: NodeId) => {
      let up = parentOf(from);
      // The root is its own parent, so bound the walk by the tree's depth.
      for (let hops = 0; up !== undefined && hops <= layers.length; hops++) {
        if (up === to) return true;
        const next = parentOf(up);
        if (next === up) return false;
        up = next;
      }
      return false;
    };
    return climbs(id, target) || climbs(target, id);
  };

  /** Turn a document-space displacement into one SetTransform per layer,
   * each expressed in that layer's own parent space. Layers that would not
   * actually move are left out. */
  const translateAll = (
    moving: { id: NodeId; t0: Transform }[],
    mx: number,
    my: number,
  ): Command[] => {
    const out: Command[] = [];
    for (const { id, t0 } of moving) {
      const [dx, dy] = layerVector(id, mx, my);
      if (dx === 0 && dy === 0) continue;
      out.push({
        SetTransform: { id, transform: { ...t0, e: t0.e + dx, f: t0.f + dy } },
      });
    }
    return out;
  };

  /** The lines a drag can catch on: the page's own edges and middle, and
   * the same three from every layer that is not travelling with the drag
   * (its ancestors and descendants move too, so they are no use). */
  const snapTargets = (moving: NodeId[]): [number[], number[]] => {
    const xs = snapLines(0, docSize[0]);
    const ys = snapLines(0, docSize[1]);
    // A guide is placed to be snapped to; that is the whole point of one.
    for (const g of docGuides) {
      (guideIsVertical(g) ? xs : ys).push(guideAt(g));
    }
    if (!session) return [xs, ys];
    for (const layer of layers) {
      const id = layer.id as NodeId;
      if (moving.some((m) => relatedTo(id, m))) continue;
      const b = session.bounds_of(id);
      if (b.length !== 4) continue;
      xs.push(...snapLines(b[0], b[0] + b[2]));
      ys.push(...snapLines(b[1], b[1] + b[3]));
    }
    return [xs, ys];
  };

  /** The document-space box around a set of layers, as two corners.
   * `bounds_of` answers [x, y, w, h]; everything here works in corners. */
  const unionBounds = (
    ids: NodeId[],
  ): [number, number, number, number] | null => {
    if (!session) return null;
    let acc: [number, number, number, number] | null = null;
    for (const id of ids) {
      const b = session.bounds_of(id);
      if (b.length !== 4) continue;
      const box: [number, number, number, number] = [
        b[0],
        b[1],
        b[0] + b[2],
        b[1] + b[3],
      ];
      acc = acc
        ? [
            Math.min(acc[0], box[0]),
            Math.min(acc[1], box[1]),
            Math.max(acc[2], box[2]),
            Math.max(acc[3], box[3]),
          ]
        : box;
    }
    return acc;
  };

  const isPanTrigger = (e: React.PointerEvent) =>
    e.button === 1 || (e.button === 0 && spaceRef.current);

  const onHostPointerDown = (e: React.PointerEvent) => {
    if (!isPanTrigger(e)) return;
    panDragRef.current = {
      pointerX: e.clientX,
      pointerY: e.clientY,
      viewX: view.x,
      viewY: view.y,
    };
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    e.preventDefault();
  };

  const onHostPointerMove = (e: React.PointerEvent) => {
    if ((tool === "Paint" || tool === "Clone") && canvasRef.current) {
      const rect = canvasRef.current.getBoundingClientRect();
      setBrushAt([e.clientX - rect.left, e.clientY - rect.top]);
    }
    const pan = panDragRef.current;
    if (!pan) return;
    setView((v) => ({
      ...v,
      x: pan.viewX + (e.clientX - pan.pointerX),
      y: pan.viewY + (e.clientY - pan.pointerY),
    }));
  };

  const onHostPointerUp = () => {
    panDragRef.current = null;
  };

  useEffect(() => {
    if (!session) return;
    const id = setTimeout(() => {
      const canvas = document.createElement("canvas");
      canvas.width = THUMB;
      canvas.height = THUMB;
      const ctx = canvas.getContext("2d");
      if (!ctx) return;
      const draw = (px: Uint8Array) => {
        ctx.clearRect(0, 0, THUMB, THUMB);
        ctx.putImageData(
          new ImageData(new Uint8ClampedArray(px), THUMB, THUMB),
          0,
          0,
        );
        return canvas.toDataURL();
      };
      const next: Record<number, string> = {};
      const masks: Record<number, string> = {};
      for (const l of layers) {
        try {
          const px = session.thumbnail(l.id, THUMB);
          if (px.length === THUMB * THUMB * 4) next[l.id] = draw(px);
          if (l.has_mask) {
            const mp = session.mask_thumbnail(l.id, THUMB);
            if (mp.length === THUMB * THUMB * 4) masks[l.id] = draw(mp);
          }
        } catch {
          continue;
        }
      }
      setThumbs(next);
      setMaskThumbs(masks);
    }, 350);
    return () => clearTimeout(id);
  }, [session, layers, saveTick]);

  /** The ring follows the brush; nothing else needs it, and a stale ring
   * left behind by a tool change or a pointer leaving the canvas would
   * read as a brush that is still there. */
  useEffect(() => {
    if (tool !== "Paint" && tool !== "Clone") setBrushAt(null);
  }, [tool]);

  /** Commit the guide list, as one history entry. */
  const setGuidesDoc = (next: DocGuide[]) =>
    run({ SetGuides: { guides: next } });

  /** Start dragging a guide: out of a ruler when `index` is null, or an
   * existing one when it is not. The pointer is followed on the window so
   * the drag survives crossing the canvas, the panel, or a ruler. */
  const startGuideDrag = (
    vertical: boolean,
    index: number | null,
    e: React.PointerEvent,
  ) => {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    const host = hostRef.current;
    if (!host) return;
    const rect = host.getBoundingClientRect();
    const positionOf = (ev: { clientX: number; clientY: number }) =>
      vertical
        ? (ev.clientX - rect.left - view.x) / view.zoom
        : (ev.clientY - rect.top - view.y) / view.zoom;
    // Dropping a guide back on a ruler is how one is thrown away, so the
    // gesture has to know where the rulers are.
    const overRuler = (ev: { clientX: number; clientY: number }) =>
      vertical ? ev.clientX - rect.left < RULER : ev.clientY - rect.top < RULER;
    setGuideDrag({ vertical, index, at: positionOf(e) });
    const onMove = (ev: PointerEvent) =>
      setGuideDrag((g) => (g ? { ...g, at: positionOf(ev) } : g));
    const onUp = (ev: PointerEvent) => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      setGuideDrag(null);
      const at = Math.round(positionOf(ev) * 100) / 100;
      const limit = vertical ? docSizeRef.current[0] : docSizeRef.current[1];
      const keep = !overRuler(ev) && at >= 0 && at <= limit;
      const next = docGuides.filter((_, i) => i !== index);
      if (keep) {
        next.push(vertical ? { Vertical: at } : { Horizontal: at });
      } else if (index === null) {
        return; // dragged out of a ruler and dropped nowhere
      }
      setGuidesDoc(next);
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };

  /** A double-click on a picked text block opens it for typing in place. */
  const onCanvasDoubleClick = (e: React.MouseEvent) => {
    if (tool !== "Move" || !session || selected === null) return;
    // Double-clicking a picked path's outline puts an anchor on it,
    // which is what the gesture means in every vector editor. A block of
    // text has its own meaning for it, and takes precedence.
    if (
      selectedKind &&
      typeof selectedKind === "object" &&
      "Vector" in selectedKind &&
      "Path" in selectedKind.Vector.shape
    ) {
      const [x, y] = docPoint(e);
      try {
        // Near the outline, in document units: an anchor goes on the
        // path, so a double-click well inside the shape is not asking
        // for one and falls through to what it otherwise means.
        session.insert_anchor(selected, x, y, 10 / view.zoom);
      } catch {
        return;
      }
      refresh(session);
      return;
    }
    if (!inlineText) beginInlineText();
  };

  /** A rubber band being dragged on empty canvas, in document
   * coordinates, and whether it adds to the selection it started from. */
  const marqueeRef = useRef<{
    x0: number;
    y0: number;
    add: NodeId[];
    band: [number, number, number, number];
  } | null>(null);
  const [marquee, setMarquee] = useState<
    [number, number, number, number] | null
  >(null);

  /** Every top-level layer the band touches, topmost first. Nested layers
   * are not offered: a band picks the objects on the page, and a group is
   * one object. Locked layers, and the adjustment and filter layers that
   * have no box of their own, are not picked. */
  const layersInBand = (band: [number, number, number, number]): NodeId[] => {
    if (!session) return [];
    const [x0, y0, x1, y1] = band;
    return layers
      .filter(
        (l) =>
          l.depth === 0 &&
          !l.locked &&
          l.visible &&
          l.kind !== "adjustment" &&
          l.kind !== "filter",
      )
      .filter((l) => {
        const b = session.bounds_of(l.id);
        if (b.length !== 4) return false;
        return b[0] < x1 && b[0] + b[2] > x0 && b[1] < y1 && b[1] + b[3] > y0;
      })
      .map((l) => l.id);
  };

  /** A brush stroke in flight: the engine holds the stroke itself, this
   * only remembers that one is being drawn and how thick the last sample
   * was, so the width eases rather than jumping sample to sample. */
  const paintingRef = useRef<{ width: number; at: number } | null>(null);

  /** The layer a paint stroke should land on: the picked one when it is a
   * paint layer, and a fresh one when it is not — so the first stroke of a
   * session makes its own layer and every stroke after it joins that one. */
  const paintTarget = (s: WasmSession, clone: boolean): NodeId | null => {
    const want = clone ? "clone" : "paint";
    const picked = layers.find((l) => l.id === selected);
    if (picked && picked.kind === want && !picked.locked) return picked.id;
    paintCount.current += 1;
    try {
      const id = (
        clone
          ? s.add_clone_layer(`Clone ${paintCount.current}`)
          : s.add_paint_layer(`Paint ${paintCount.current}`)
      ) as NodeId;
      setSelected(id);
      setMultiSel([]);
      return id;
    } catch {
      return null;
    }
  };

  const onCanvasPointerDown = (e: React.PointerEvent) => {
    if (!session || isPanTrigger(e) || e.button !== 0) return;
    e.stopPropagation();
    const [x, y] = docPoint(e);
    if (tool === "Clone") {
      // Alt sets where the clone reads from, which is what that key does
      // in every editor that has this tool.
      if (e.altKey) {
        setCloneFrom([x, y]);
        return;
      }
      if (cloneFrom === null) {
        alert("Alt-click the place to clone from first.");
        return;
      }
      const layer = paintTarget(session, true);
      if (layer === null) return;
      try {
        session.paint_begin(
          layer,
          x,
          y,
          paintSize / 2,
          JSON.stringify(cmyk ? hexToCmykColor(fill) : hexColor(fill)),
          paintSoftness,
          false,
          false,
        );
        session.paint_source(cloneFrom[0] - x, cloneFrom[1] - y, healing);
      } catch (err) {
        alert(`Clone: ${err}`);
        return;
      }
      paintingRef.current = { width: 1, at: performance.now() };
      lastPaint.current = [x, y];
      (e.target as Element).setPointerCapture?.(e.pointerId);
      refresh(session);
      return;
    }
    if (tool === "Paint") {
      // Alt takes the colour under the brush rather than laying any: the
      // sampling every paint tool puts under that key, so the brush does
      // not have to be put down to pick a colour up.
      if (e.altKey) {
        const hex = colorUnder(session, x, y);
        if (hex) setFill(hex);
        return;
      }
      // Rubbing at a layer that is not a paint layer takes a piece out
      // of the layer rather than painting over it: the stroke goes into
      // a mask, so the layer itself is untouched and the brush puts the
      // piece back. Once a layer has such a mask the brush keeps working
      // on it, which is how what was rubbed out is painted back in.
      const picked = layers.find((l) => l.id === selected);
      const onMask =
        !!picked &&
        picked.kind !== "paint" &&
        !picked.locked &&
        (erasing || picked.painted_mask);
      let layer: NodeId | null;
      if (onMask) {
        try {
          if (!session.ensure_painted_mask(picked.id)) {
            alert(`${picked.name} already carries a mask of another kind.`);
            return;
          }
        } catch (err) {
          alert(`Mask: ${err}`);
          return;
        }
        layer = picked.id;
      } else {
        layer = paintTarget(session, false);
      }
      if (layer === null) return;
      // Shift starts the stroke where the last one ended and runs it
      // straight to here, which is how a brush draws a line it could not
      // have drawn by hand.
      const from = e.shiftKey ? strokeEnd.current : null;
      const start = from ?? [x, y];
      try {
        session.paint_begin(
          layer,
          start[0],
          start[1],
          paintSize / 2,
          JSON.stringify(cmyk ? hexToCmykColor(fill) : hexColor(fill)),
          paintSoftness,
          erasing,
          onMask,
        );
        if (from) session.paint_extend(x, y, paintSize / 2);
      } catch (err) {
        alert(`Paint: ${err}`);
        return;
      }
      strokeEnd.current = [x, y];
      if (from) {
        // The line is the whole gesture; there is nothing to drag.
        session.commit_preview();
        refresh(session);
        return;
      }
      paintingRef.current = { width: 1, at: performance.now() };
      lastPaint.current = [x, y];
      (e.target as Element).setPointerCapture?.(e.pointerId);
      refresh(session);
      return;
    }
    if (tool === "Text") {
      shapeCount.current += 1;
      run({
        AddNode: {
          parent: session.root_id,
          index: topLevelCount(layers),
          node: nodePayload(
            `Text ${shapeCount.current}`,
            {
              Text: {
                text: "Text",
                size: 48,
                fill: cmyk ? hexToCmykColor(fill) : hexColor(fill),
                align: "Left",
                line_height: 1,
                letter_spacing: 0,
                width: 0,
                font: "",
                italic: false,
                underline: false,
                strike: false,
                along: null,
                along_offset: 0,
              },
            },
            x,
            y,
          ),
        },
      });
      setTool("Move");
      return;
    }
    if (tool === "Pen") {
      // Clicking the first anchor again closes the path.
      const closeRadius = 8 / view.zoom;
      const first = penPoints[0];
      if (
        penPoints.length >= 3 &&
        first &&
        Math.hypot(x - first[0], y - first[1]) < closeRadius
      ) {
        finishPath(true);
      } else {
        setPenPoints((pts) => [...pts, [x, y]]);
      }
      return;
    }
    const drag: ToolDrag = {
      tool,
      startX: x,
      startY: y,
      lastX: x,
      lastY: y,
      moved: false,
      stroke: tool === "Brush" ? [[x, y]] : undefined,
      widths: tool === "Brush" ? [1] : undefined,
      lastAt: performance.now(),
    };
    if (tool === "Eyedropper") {
      // Take the colour the page shows there — the composite, not the
      // layer under the cursor, which is what the eye is pointing at —
      // and make it the colour to draw with. With a shape or a block of
      // text picked, it becomes that layer's fill as well.
      const hex = colorUnder(session, x, y);
      if (hex) {
        setFill(hex);
        const colour = cmyk ? hexToCmykColor(hex) : hexColor(hex);
        if (selectedKind && typeof selectedKind === "object") {
          if ("Vector" in selectedKind) {
            setKind(
              {
                Vector: {
                  ...selectedKind.Vector,
                  fill: colour,
                  gradient: null,
                },
              },
              false,
            );
          } else if ("Text" in selectedKind) {
            setKind({ Text: { ...selectedKind.Text, fill: colour } }, false);
          }
        }
      }
      setTool("Move");
      return;
    }
    if (tool === "Move") {
      const hit = session.hit_test(x, y);
      if (hit === undefined) {
        // Empty canvas: drag a band over what should be picked. Shift
        // keeps what is already picked and adds to it.
        marqueeRef.current = {
          x0: x,
          y0: y,
          add: e.shiftKey ? selectionSet : [],
          band: [x, y, x, y],
        };
        setMarquee([x, y, x, y]);
        if (!e.shiftKey) {
          setSelected(null);
          setMultiSel([]);
        }
        (e.target as Element).setPointerCapture(e.pointerId);
        return;
      }
      // Dragging inside a selected group moves the group. Hit testing only
      // ever reports leaves, so without this a group could be selected in
      // the panel and still not be draggable — you would grab whichever
      // child happened to be under the cursor.
      let target = inSelectedGroup(hit) ? selected! : hit;
      // Grabbing any member of a multi-selection drags the whole of it;
      // grabbing anything else starts a fresh single selection.
      // A locked layer among them stays where it is; the target itself is
      // never locked, since the pick fell through anything that was.
      let together =
        selectionSet.length > 1 && selectionSet.includes(target)
          ? movableSelection
          : [target];
      if (together.length === 1) setMultiSel([]);
      // Alt drags a copy and leaves the original where it was: the copies
      // are made where they stand, and the drag carries them off. Two
      // history entries — the move, then the duplicate under it.
      if (e.altKey) {
        try {
          const copies = Array.from(
            session.duplicate_nodes(new Float64Array(together), false),
          );
          target = copies[Math.max(0, together.indexOf(target))];
          together = copies;
          setMultiSel(copies.length > 1 ? copies : []);
          refresh(session);
        } catch (err) {
          alert(`Duplicate: ${err}`);
        }
      }
      drag.target = target;
      drag.t0 = toTransform(session.transform_of(target));
      drag.moving = together.map((id) => ({
        id,
        t0: toTransform(session.transform_of(id)),
      }));
      // Collect the snap lines once, while nothing is moving. What snaps
      // is the box around everything being dragged, so several layers
      // align as the one shape they look like.
      const moved = unionBounds(together);
      if (moved) {
        drag.b0 = moved;
        [drag.snapX, drag.snapY] = snapTargets(together);
      }
      setSelected(target);
    }
    toolDragRef.current = drag;
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onCanvasPointerMove = (e: React.PointerEvent) => {
    const band = marqueeRef.current;
    if (band) {
      const [x, y] = docPoint(e);
      // The gesture keeps the band itself; the state is only what the
      // overlay draws, and a release that lands before React has painted
      // must still pick what the pointer actually swept.
      band.band = [
        Math.min(band.x0, x),
        Math.min(band.y0, y),
        Math.max(band.x0, x),
        Math.max(band.y0, y),
      ];
      setMarquee(band.band);
      return;
    }
    const painting = paintingRef.current;
    if (painting && session) {
      const [px, py] = docPoint(e);
      const now = performance.now();
      // The same reading the vector brush takes: a pen's real pressure,
      // and a mouse's speed standing in for it.
      const step = Math.hypot(px - lastPaint.current[0], py - lastPaint.current[1]);
      const want =
        e.pointerType === "pen" && e.pressure > 0
          ? 0.25 + e.pressure * 0.75
          : speedWidth(step / Math.max(1, now - painting.at));
      painting.width += (want - painting.width) * 0.35;
      painting.at = now;
      lastPaint.current = [px, py];
      try {
        session.paint_extend(px, py, (paintSize / 2) * painting.width);
      } catch {
        /* the layer went away under the stroke; the gesture ends below */
      }
      refresh(session);
      return;
    }
    const drag = toolDragRef.current;
    if (!drag) return;
    [drag.lastX, drag.lastY] = docPoint(e);
    if (drag.stroke) {
      // Only keep points that add something: a stroke sampled at screen
      // resolution carries hundreds of anchors nobody can edit.
      const last = drag.stroke[drag.stroke.length - 1];
      const step = Math.hypot(drag.lastX - last[0], drag.lastY - last[1]);
      if (step >= BRUSH_STEP) {
        const now = performance.now();
        // A pen reports real pressure; a mouse always reads 0.5, so fall
        // back to speed — a fast stroke thins out, the way ink does.
        const w =
          e.pointerType === "pen" && e.pressure > 0
            ? 0.25 + e.pressure * 0.75
            : speedWidth(step / Math.max(1, now - (drag.lastAt ?? now)));
        const prev = drag.widths?.[drag.widths.length - 1] ?? 1;
        // Ease toward the new width: sample-to-sample jitter would show up
        // as a lumpy edge.
        drag.widths?.push(prev + (w - prev) * 0.35);
        drag.lastAt = now;
        drag.stroke.push([drag.lastX, drag.lastY]);
        drag.moved = true;
        setPenPoints([...drag.stroke]); // live line while drawing
      }
    }
    if (drag.tool === "Crop") {
      // Show the frame where it is being dragged, in host coordinates so
      // it sits over the canvas without a transform of its own.
      const toHost = (x: number, y: number): [number, number] => [
        view.x + x * view.zoom,
        view.y + y * view.zoom,
      ];
      const [ax, ay] = toHost(drag.startX, drag.startY);
      const [bx, by] = toHost(drag.lastX, drag.lastY);
      setCropRect([
        Math.min(ax, bx),
        Math.min(ay, by),
        Math.max(ax, bx),
        Math.max(ay, by),
      ]);
      drag.moved = true;
      return;
    }

    // Move tool: live preview while dragging.
    if (drag.tool === "Move" && drag.target !== undefined && drag.t0) {
      // Snap the layer's edges and centre onto the page's and the other
      // layers', in document space, before the delta is carried into the
      // layer's own space. Ctrl (or Cmd) drags free of it.
      let [mx, my] = [drag.lastX - drag.startX, drag.lastY - drag.startY];
      const snapping =
        drag.b0 && drag.snapX && drag.snapY && !(e.ctrlKey || e.metaKey);
      const next: Guides = { x: [], y: [] };
      if (snapping) {
        const tol = SNAP_PX / view.zoom;
        const b = drag.b0!;
        const sx = snapAxis(snapLines(b[0] + mx, b[2] + mx), drag.snapX!, tol);
        const sy = snapAxis(snapLines(b[1] + my, b[3] + my), drag.snapY!, tol);
        mx += sx.delta;
        my += sy.delta;
        if (sx.guide !== null) next.x.push(sx.guide);
        if (sy.guide !== null) next.y.push(sy.guide);
      }
      // Only re-render when the guides actually change: this runs on every
      // pointer sample.
      setGuides((g) =>
        g.x[0] === next.x[0] && g.y[0] === next.y[0] ? g : next,
      );
      // The delta is in document space; each layer wants it in its own
      // parent's, which differ once groups turn or scale.
      const moves = translateAll(drag.moving ?? [], mx, my);
      if (moves.length > 0) {
        drag.moved = true;
        preview(moves.length === 1 ? moves[0] : { Batch: moves });
      }
    }
  };

  /** The look of the picked layer, kept as JSON so it outlives the
   * document it was taken from — the same way the layer clipboard does.
   * Null until something has been copied. */
  const styleClip = useRef<string | null>(null);

  const copyStyle = () => {
    if (!session || selected === null) return;
    try {
      styleClip.current = session.copy_style(selected);
    } catch (err) {
      alert(`Copy style: ${err}`);
    }
  };

  const pasteStyle = () => {
    if (!session || styleClip.current === null) return;
    const ids = selectionSet;
    if (ids.length === 0) return;
    try {
      session.paste_style(styleClip.current, new Float64Array(ids));
    } catch (err) {
      alert(`Paste style: ${err}`);
      return;
    }
    refresh(session);
  };

  const onCanvasPointerUp = () => {
    if (paintingRef.current && session) {
      paintingRef.current = null;
      strokeEnd.current = lastPaint.current;
      session.commit_preview();
      refresh(session);
      return;
    }
    const band = marqueeRef.current;
    if (band) {
      marqueeRef.current = null;
      const rect = band.band;
      setMarquee(null);
      // A band with no width or height is a click on empty canvas, which
      // has already cleared the selection.
      if (rect && (rect[2] - rect[0] > 1 || rect[3] - rect[1] > 1)) {
        const caught = layersInBand(rect);
        const picked = [
          ...band.add,
          ...caught.filter((id) => !band.add.includes(id)),
        ];
        // The topmost of them leads, so the panel and the properties
        // follow the layer a second click would grab.
        setSelected(picked.length > 0 ? picked[picked.length - 1] : null);
        setMultiSel(picked.length > 1 ? picked : []);
      }
      return;
    }
    const drag = toolDragRef.current;
    toolDragRef.current = null;
    setGuides({ x: [], y: [] });
    setCropRect(null);
    if (!drag || !session) return;

    if (drag.tool === "Move") {
      // The document already holds the previewed position; seal the gesture.
      if (drag.moved && session.commit_preview()) refresh(session);
      return;
    }

    if (drag.tool === "Brush") {
      setPenPoints([]);
      const kept = simplifyStroke(drag.stroke ?? [], BRUSH_TOLERANCE);
      if (kept.length < 2) return;
      const pts = kept.map((i) => drag.stroke![i]);
      const widths = kept.map((i) => drag.widths?.[i] ?? 1);
      // Anchors are stored relative to the stroke's own origin, like every
      // other path, so the node's transform carries its position.
      const minX = Math.min(...pts.map((p) => p[0]));
      const minY = Math.min(...pts.map((p) => p[1]));
      shapeCount.current += 1;
      run({
        AddNode: {
          parent: session.root_id,
          index: topLevelCount(layers),
          node: nodePayload(
            `Stroke ${shapeCount.current}`,
            {
              Vector: {
                shape: {
                  Path: {
                    subpaths: [],
                    points: pts.map(
                      (p) => [p[0] - minX, p[1] - minY] as [number, number],
                    ),
                    closed: false,
                    // Smoothed, so a hand-drawn line reads as a curve and
                    // stays editable as a handful of anchors.
                    smooth: true,
                    handles: [],
                  },
                },
                fill: null,
                stroke: {
                  color: cmyk ? hexToCmykColor(fill) : hexColor(fill),
                  width: brushSize,
                  widths,
                },
                gradient: null,
              },
            },
            minX,
            minY,
          ),
        },
      });
      return;
    }

    // Shape tools: commit the dragged bounds as a new object.
    const x0 = Math.min(drag.startX, drag.lastX);
    const y0 = Math.min(drag.startY, drag.lastY);
    const w = Math.abs(drag.lastX - drag.startX);
    const h = Math.abs(drag.lastY - drag.startY);
    if (w < MIN_SIZE || h < MIN_SIZE) return;
    if (drag.tool === "Crop") {
      // Crop to the dragged rectangle, clamped to the page: the document
      // becomes that rectangle and every layer shifts with it, so what was
      // framed stays framed. Both corners are clamped, not just the
      // origin — a frame started off the page would otherwise keep the
      // width it was dragged with and come out bigger than it looked.
      const clamp = (v: number, hi: number) => Math.min(Math.max(v, 0), hi);
      const cx = Math.round(clamp(x0, docSize[0]));
      const cy = Math.round(clamp(y0, docSize[1]));
      const cw = Math.round(clamp(x0 + w, docSize[0])) - cx;
      const ch = Math.round(clamp(y0 + h, docSize[1])) - cy;
      if (cw < 1 || ch < 1) return;
      try {
        session.resize_canvas(cw, ch, -cx, -cy);
      } catch (err) {
        alert(`Crop: ${err}`);
        return;
      }
      setDocumentSize(cw, ch);
      refresh(session);
      fitView();
      setTool("Move");
      return;
    }
    if (drag.tool === "Frame") {
      // A frame is put on the page whole: its size is what was dragged,
      // and it goes under everything already there so it does not cover
      // loose layers. White ground, which is what a page is.
      frameCount.current += 1;
      try {
        const id = session.add_artboard(
          `Artboard ${frameCount.current}`,
          Math.round(x0),
          Math.round(y0),
          Math.round(w),
          Math.round(h),
          JSON.stringify(hexColor("#ffffff")),
        );
        setSelected(id);
        setMultiSel([]);
        refresh(session);
        setTool("Move");
      } catch (err) {
        alert(`Frame: ${err}`);
      }
      return;
    }
    shapeCount.current += 1;
    // Drawn inside a frame, it goes into that frame — in the frame's own
    // coordinates, so it lands under the cursor rather than at the
    // frame's offset. Both corners are carried across, so a frame that
    // has been scaled gets a shape the size it was dragged.
    const board = session.frame_at(x0, y0);
    const inside = board >= 0 ? session.point_inside(board, x0, y0) : [];
    const far = board >= 0 ? session.point_inside(board, x0 + w, y0 + h) : [];
    const local = inside.length === 2 && far.length === 2;
    const [ox, oy] = local ? [inside[0], inside[1]] : [x0, y0];
    const [lw, lh] = local
      ? [Math.abs(far[0] - inside[0]), Math.abs(far[1] - inside[1])]
      : [w, h];
    const shape =
      drag.tool === "Rect"
        ? { Rect: { width: lw, height: lh, radius: 0 } }
        : { Ellipse: { rx: lw / 2, ry: lh / 2 } };
    run({
      AddNode: {
        parent: local ? board : session.root_id,
        index: local ? session.child_count(board) : topLevelCount(layers),
        node: nodePayload(
          `${drag.tool} ${shapeCount.current}`,
          {
            Vector: {
              shape,
              // CMYK documents author ink values so the press profile
              // (and later export) drives their rendering.
              fill: cmyk ? hexToCmykColor(fill) : hexColor(fill),
              stroke: null,
              gradient: null,
            },
          },
          ox,
          oy,
        ),
      },
    });
  };

  // Resize handles: drag scales the node, anchored at the opposite corner.
  const onHandlePointerDown = (e: React.PointerEvent, corner: Handle) => {
    if (!session || selected === null || !selLocal) return;
    e.stopPropagation();
    const [snapX, snapY] = snapTargets([selected]);
    handleDragRef.current = {
      corner,
      id: selected,
      t0: toTransform(session.transform_of(selected)),
      b0: selLocal,
      snapX,
      snapY,
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onHandlePointerMove = (e: React.PointerEvent) => {
    const drag = handleDragRef.current;
    if (!drag || !session) return;
    if (e.buttons === 0) return;
    // The corner follows the cursor, so snapping the cursor snaps the
    // corner — done in document space, where the lines are, and before
    // the point is carried into the layer's own space.
    let [px, py] = docPoint(e);
    const next: Guides = { x: [], y: [] };
    if (drag.snapX && drag.snapY && !(e.ctrlKey || e.metaKey)) {
      const tol = SNAP_PX / view.zoom;
      const sx = snapAxis([px], drag.snapX, tol);
      const sy = snapAxis([py], drag.snapY, tol);
      px += sx.delta;
      py += sy.delta;
      if (sx.guide !== null) next.x.push(sx.guide);
      if (sy.guide !== null) next.y.push(sy.guide);
    }
    setGuides((g) => (g.x[0] === next.x[0] && g.y[0] === next.y[0] ? g : next));
    // Resize happens in the layer's own space: bring the cursor there,
    // hold the opposite corner still, and scale about it. Doing it in
    // document space would stretch a rotated layer along the wrong axes.
    const [dx, dy] = layerPoint({ clientX: px, clientY: py }, drag.id, true);
    const t = drag.t0;
    const det = t.a * t.d - t.b * t.c;
    if (Math.abs(det) < 1e-9) return;
    const [ux, uy] = [dx - t.e, dy - t.f];
    const lx = (t.d * ux - t.c * uy) / det;
    const ly = (t.a * uy - t.b * ux) / det;

    const [x0, y0, x1, y1] = drag.b0;
    const west = drag.corner === "nw" || drag.corner === "sw";
    const north = drag.corner === "nw" || drag.corner === "ne";
    // The corner that stays put, in local units.
    const [fx, fy] = [west ? x1 : x0, north ? y1 : y0];
    const span = (a: number, b: number) => Math.max(MIN_SIZE, Math.abs(a - b));
    const sx = span(lx, fx) / span(west ? x0 : x1, fx);
    const sy = span(ly, fy) / span(north ? y0 : y1, fy);

    // T' = T0 . scale(sx, sy) about (fx, fy), composed in local space.
    const [tx, ty] = [(1 - sx) * fx, (1 - sy) * fy];
    preview({
      SetTransform: {
        id: drag.id,
        transform: {
          a: t.a * sx,
          b: t.b * sx,
          c: t.c * sy,
          d: t.d * sy,
          e: t.a * tx + t.c * ty + t.e,
          f: t.b * tx + t.d * ty + t.f,
        },
      },
    });
  };

  const onHandlePointerUp = () => {
    handleDragRef.current = null;
    setGuides({ x: [], y: [] });
    if (session?.commit_preview()) refresh(session);
  };

  const ADJUSTMENT_PRESETS: Record<string, { name: string; kind: NodeKind }> = {
    exposure: {
      name: "Exposure",
      kind: { Adjustment: { Exposure: { stops: 0 } } },
    },
    "brightness-contrast": {
      name: "Brightness/Contrast",
      kind: {
        Adjustment: { BrightnessContrast: { brightness: 0, contrast: 0 } },
      },
    },
    "hue-saturation": {
      name: "Hue/Saturation",
      kind: {
        Adjustment: {
          HueSaturation: { hue_degrees: 0, saturation: 0, lightness: 0 },
        },
      },
    },
    "white-balance": {
      name: "White balance",
      kind: { Adjustment: { WhiteBalance: { temperature: 0, tint: 0 } } },
    },
    vibrance: {
      name: "Vibrance",
      kind: { Adjustment: { Vibrance: { amount: 0 } } },
    },
    levels: {
      name: "Levels",
      kind: {
        Adjustment: {
          Levels: {
            in_black: 0,
            in_white: 1,
            gamma: 1,
            out_black: 0,
            out_white: 1,
          },
        },
      },
    },
    curves: {
      name: "Curves",
      kind: {
        Adjustment: {
          Curves: {
            points: [
              [0, 0],
              [1, 1],
            ],
          },
        },
      },
    },
    blur: {
      name: "Gaussian Blur",
      kind: { Filter: { GaussianBlur: { sigma: 4 } } },
    },
    sharpen: {
      name: "Sharpen",
      kind: { Filter: { Sharpen: { sigma: 1.5, amount: 0.5 } } },
    },
  };

  /** Add an adjustment or filter. A key on its own puts it at the top,
   * over everything; `only:` in front scopes it to the picked layer, which
   * the engine does by grouping the two — a group isolates, and that is
   * the whole trick. */
  const addAdjustment = (value: string) => {
    const only = value.startsWith("only:");
    const preset = ADJUSTMENT_PRESETS[only ? value.slice(5) : value];
    if (!session || !preset) return;
    if (only && selected !== null) {
      try {
        const group = session.adjust_node(
          selected,
          JSON.stringify(nodePayload(preset.name, preset.kind)),
        );
        // Pick the adjustment itself, so its controls are right there.
        const rows = JSON.parse(session.layers_json()) as LayerInfo[];
        const at = rows.findIndex((l) => l.id === group);
        const inside = rows
          .slice(at + 1)
          .find((l) => l.parent === group && l.kind !== "group");
        setSelected(inside ? inside.id : group);
        setMultiSel([]);
        refresh(session);
      } catch (err) {
        alert(`Adjust: ${err}`);
      }
      return;
    }
    run({
      AddNode: {
        parent: session.root_id,
        index: topLevelCount(layers),
        node: nodePayload(preset.name, preset.kind),
      },
    });
  };

  /** Commit the running slider/drag gesture as one undo step. */
  const endGesture = () => {
    if (session?.commit_preview()) refresh(session);
  };

  const [renaming, setRenaming] = useState<{
    id: NodeId;
    value: string;
  } | null>(null);

  /** Dragging a row of the layers list to reorder: which layer, the row
   * under the pointer and where it would land relative to it. Pointer
   * driven rather than HTML drag-and-drop, so it works the same with a
   * mouse, a touch, and a test's synthetic pointer. */
  const [layerDrag, setLayerDrag] = useState<{
    id: NodeId;
    over: NodeId | null;
    where: "above" | "below" | "into";
  } | null>(null);
  const layerDragRef = useRef<{
    id: NodeId;
    startY: number;
    active: boolean;
    over: NodeId | null;
    where: "above" | "below" | "into";
  } | null>(null);
  const layerListRef = useRef<HTMLUListElement>(null);
  /** When a drag last ended, so the click the release brings with it does
   * not also pick the row. Timed rather than a flag that waits to be
   * cleared: a drop can move the row out from under the pointer, and then
   * no click follows at all — a flag left standing would eat someone's
   * next click, minutes later. */
  const layerDragDone = useRef(0);
  const onRowPointerDown = (e: React.PointerEvent, id: NodeId) => {
    if (e.button !== 0 || renaming) return;
    layerDragRef.current = {
      id,
      startY: e.clientY,
      active: false,
      over: null,
      where: "above",
    };
    const onMove = (ev: PointerEvent) => {
      const drag = layerDragRef.current;
      if (!drag) return;
      if (!drag.active) {
        if (Math.abs(ev.clientY - drag.startY) < 4) return;
        drag.active = true;
      }
      let over: NodeId | null = null;
      let where: "above" | "below" | "into" = "above";
      for (const row of layerListRef.current?.querySelectorAll<HTMLLIElement>(
        "li[data-id]",
      ) ?? []) {
        const r = row.getBoundingClientRect();
        if (ev.clientY < r.top || ev.clientY >= r.bottom) continue;
        over = Number(row.dataset.id);
        const t = (ev.clientY - r.top) / r.height;
        // The middle of a group's (or a frame's) row drops into it; the
        // edges of any row drop beside it.
        where =
          HOLDS_CHILDREN.has(row.dataset.kind ?? "") && t > 0.3 && t < 0.7
            ? "into"
            : t < 0.5
              ? "above"
              : "below";
      }
      drag.over = over;
      drag.where = where;
      setLayerDrag({ id: drag.id, over, where });
    };
    const onUp = () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      const drag = layerDragRef.current;
      layerDragRef.current = null;
      setLayerDrag(null);
      // The drop is read from the ref, not a state updater: an updater is
      // meant to be pure, and React may run it twice.
      if (drag?.active && drag.over !== null) {
        layerDragDone.current = Date.now();
        dropLayer(drag.id, drag.over, drag.where);
      }
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
  };
  /** Move `id` to where a drop over `over` asked for, as one MoveNode. The
   * list runs top-first, so "above" a row is the slot past its index in
   * painter's order, and a move within one group accounts for the slot
   * the layer leaves behind. */
  const dropLayer = (
    id: NodeId,
    over: NodeId,
    where: "above" | "below" | "into",
  ) => {
    if (!session || id === over) return;
    const me = layers.find((l) => l.id === id);
    const target = layers.find((l) => l.id === over);
    if (!me || !target) return;
    let parent: NodeId;
    let index: number;
    if (where === "into") {
      parent = target.id;
      index = layers.filter((l) => l.parent === target.id).length;
      if (me.parent === parent) index -= 1;
    } else {
      parent = target.parent;
      index = where === "above" ? target.index + 1 : target.index;
      if (me.parent === parent && me.index < index) index -= 1;
    }
    // Never into its own subtree.
    for (
      let cur: NodeId | undefined = parent;
      cur !== undefined && cur !== session.root_id;
    ) {
      if (cur === id) return;
      cur = layers.find((l) => l.id === cur)?.parent;
    }
    if (parent === me.parent && index === me.index) return;
    try {
      // Through the engine rather than a plain MoveNode: changing
      // parents changes the space the layer's transform is written in,
      // and the engine undoes that so the layer does not jump when it is
      // dropped into a group or a frame that sits away from the origin.
      session.reparent(id, parent, index);
      refresh(session);
    } catch (err) {
      alert(`Move: ${err}`);
    }
  };

  /** Typing into a text block on the canvas: which block, and the text as
   * typed so far. Every keystroke previews through the engine and the
   * block records one history entry when the editor closes; Escape puts
   * the old text back. */
  const [inlineText, setInlineText] = useState<{
    id: NodeId;
    value: string;
  } | null>(null);
  const inlineRef = useRef<HTMLTextAreaElement>(null);
  const beginInlineText = () => {
    if (
      !session ||
      !selectedLayer ||
      !selectedKind ||
      typeof selectedKind !== "object" ||
      !("Text" in selectedKind)
    )
      return;
    setInlineText({ id: selectedLayer.id, value: selectedKind.Text.text });
    setTimeout(() => {
      inlineRef.current?.focus();
      inlineRef.current?.select();
    }, 0);
  };
  const typeInlineText = (value: string) => {
    if (
      !inlineText ||
      !selectedKind ||
      typeof selectedKind !== "object" ||
      !("Text" in selectedKind)
    )
      return;
    setInlineText({ ...inlineText, value });
    preview({
      SetKind: {
        id: inlineText.id,
        kind: { Text: { ...selectedKind.Text, text: value } },
      },
    });
  };
  const closeInlineText = (keep: boolean) => {
    if (!inlineText) return;
    setInlineText(null);
    if (!session) return;
    if (keep) {
      endGesture();
    } else if (session.cancel_preview()) {
      refresh(session);
    }
  };

  const commitRename = () => {
    if (renaming && renaming.value.trim()) {
      run({ SetName: { id: renaming.id, name: renaming.value.trim() } });
    }
    setRenaming(null);
  };

  const deleteSelected = () => {
    if (selected === null) return;
    run({ RemoveNode: { id: selected } });
    setSelected(null);
  };

  const copySelected = () => {
    if (!session || selected === null) return;
    session.copy_node(selected);
  };

  const pasteClipboard = () => {
    if (!session) return;
    const id = session.paste();
    if (id === undefined) return;
    setSelected(id);
    setMultiSel([]);
    refresh(session);
  };

  // Layer shortcuts get their own listener, declared after the actions it
  // calls: a closure over a `const` declared further down still throws
  // when it runs, and TypeScript does not flag that inside a callback.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const typing = isTextEntry(e.target);
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "d") {
        e.preventDefault();
        duplicateSelected();
      }
      // With alt, the clipboard keys carry the layer's look rather than
      // the layer: the pairing every editor uses for it.
      if ((e.metaKey || e.ctrlKey) && e.altKey && !typing) {
        const k = e.key.toLowerCase();
        if (k === "c") {
          e.preventDefault();
          copyStyle();
          return;
        }
        if (k === "v") {
          e.preventDefault();
          pasteStyle();
          return;
        }
        if (k === "g") {
          e.preventDefault();
          clipSelection();
          return;
        }
      }
      if ((e.metaKey || e.ctrlKey) && !typing) {
        const k = e.key.toLowerCase();
        if (k === "c") {
          e.preventDefault();
          copySelected();
        }
        if (k === "x") {
          e.preventDefault();
          copySelected();
          deleteSelected();
        }
        // Ctrl+V is left to the paste event, which can see the clipboard —
        // with a fallback: a webview that never fires it on a non-editable
        // target (WKWebView) still gets the in-app paste, a beat later,
        // and one that does fire it has already been served by then.
        if (k === "v") {
          pasteSeen.current = false;
          window.setTimeout(() => {
            if (!pasteSeen.current) pasteClipboard();
          }, 80);
        }
      }
      if (!typing && (e.key === "Delete" || e.key === "Backspace")) {
        e.preventDefault();
        deleteSelected();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  /** All picked layers: primary selection plus ctrl-clicked extras. */
  const selectionSet =
    selected === null
      ? multiSel
      : [selected, ...multiSel.filter((id) => id !== selected)];
  /** What a move, an alignment or a flip acts on: a locked layer is
   * picked and read like any other, but nothing shifts it. */
  const movableSelection = selectionSet.filter(
    (id) => !layers.find((l) => l.id === id)?.locked,
  );

  /** Align or distribute everything picked. Enabled only with two or more,
   * which is the only case where either word means anything. */
  const alignSelection = (mode: string) => {
    if (!session || movableSelection.length < 2) return;
    try {
      session.align_nodes(new Float64Array(movableSelection), mode);
      refresh(session);
    } catch (err) {
      alert(`Align: ${err}`);
    }
  };

  /** Arrow keys nudge whatever is picked; shift makes it a coarse step.
   * Declared after `selectionSet` for the same reason the layer shortcuts
   * are declared after their actions. */
  useEffect(() => {
    const STEPS: Record<string, [number, number]> = {
      ArrowLeft: [-1, 0],
      ArrowRight: [1, 0],
      ArrowUp: [0, -1],
      ArrowDown: [0, 1],
    };
    const onKey = (e: KeyboardEvent) => {
      const step = STEPS[e.key];
      // Any form control gets its arrows first: they step a slider, open a
      // select, move a caret. Only a bare canvas nudges the layer.
      const inControl =
        e.target instanceof HTMLInputElement ||
        e.target instanceof HTMLSelectElement ||
        isTextEntry(e.target);
      if (!step || !session || inControl || selectionSet.length === 0) {
        return;
      }
      e.preventDefault();
      const k = e.shiftKey ? 10 : 1;
      const moving = movableSelection.map((id) => ({
        id,
        t0: toTransform(session.transform_of(id)),
      }));
      const cmds = translateAll(moving, step[0] * k, step[1] * k);
      if (cmds.length > 0) run(cmds.length === 1 ? cmds[0] : { Batch: cmds });
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  /** Mirror the picked layers about their shared box, one history
   * entry: a pair flips as a pair, a lone layer flips in place. */
  const flipSelection = (horizontal: boolean) => {
    if (!session || movableSelection.length === 0) return;
    try {
      session.flip_nodes(new Float64Array(movableSelection), horizontal);
      refresh(session);
    } catch (err) {
      alert(`Flip: ${err}`);
    }
  };

  /** Combine the picked shapes into one compound path. */
  const combineSelection = (op: string) => {
    if (!session || selectionSet.length < 2) return;
    try {
      const id = session.boolean_nodes(new Float64Array(selectionSet), op);
      setSelected(id);
      setMultiSel([]);
      refresh(session);
    } catch (err) {
      alert(`Combine: ${err}`);
    }
  };

  const BOOLEAN_BUTTONS: [string, IconName, string][] = [
    ["union", "union", "Unite shapes"],
    ["subtract", "subtract", "Subtract the shapes above"],
    ["intersect", "intersect", "Keep only the overlap"],
    ["exclude", "exclude", "Keep everything but the overlap"],
  ];

  /** Copy the picked layer and take it out, as the shortcut does. */
  const cutSelected = () => {
    copySelected();
    deleteSelected();
  };

  /** Frame the picked layers, or the whole page when nothing is picked. */
  const zoomToSelection = () => {
    const host = hostRef.current;
    if (!host) return;
    const box = selectionSet.length > 0 ? unionBounds(selectionSet) : null;
    if (!box) {
      fitView();
      return;
    }
    const [w, h] = [Math.max(1, box[2] - box[0]), Math.max(1, box[3] - box[1])];
    const zoom = Math.min(
      8,
      Math.max(
        0.05,
        Math.min(host.clientWidth / w, host.clientHeight / h) * 0.8,
      ),
    );
    setView({
      zoom,
      x: host.clientWidth / 2 - (box[0] + w / 2) * zoom,
      y: host.clientHeight / 2 - (box[1] + h / 2) * zoom,
    });
  };

  const ALIGN_BUTTONS: [string, IconName, string][] = [
    ["left", "alignLeft", "Align left edges"],
    ["center-h", "alignCenterH", "Align horizontal centres"],
    ["right", "alignRight", "Align right edges"],
    ["top", "alignTop", "Align top edges"],
    ["middle-v", "alignMiddleV", "Align vertical centres"],
    ["bottom", "alignBottom", "Align bottom edges"],
    ["distribute-h", "distributeH", "Space evenly across"],
    ["distribute-v", "distributeV", "Space evenly down"],
  ];

  const groupSelection = () => {
    if (!session || selectionSet.length === 0) return;
    try {
      groupCount.current += 1;
      const id = session.group_nodes(
        new Float64Array(selectionSet),
        `Group ${groupCount.current}`,
      );
      setSelected(id);
      setMultiSel([]);
      refresh(session);
    } catch (err) {
      alert(`Group: ${err}`);
    }
  };

  /** Confine the picked layers to the layer below each of them, or let
   * them out again — one entry however many are picked. The whole
   * selection follows the first one's state, so a mixed selection ends up
   * agreeing rather than each flipping past the others. The bottom layer
   * of a parent has nothing under it and is left alone. */
  const clipSelection = () => {
    if (!session || selectionSet.length === 0) return;
    const rows = layers.filter(
      (l) => selectionSet.includes(l.id) && l.index > 0,
    );
    if (rows.length === 0) return;
    const clipped = !rows[0].clipped;
    const cmds = rows.map((l) => ({
      SetClipped: { id: l.id, clipped },
    }));
    run(cmds.length === 1 ? cmds[0] : { Batch: cmds });
  };

  const ungroupSelection = () => {
    if (!session || selected === null) return;
    try {
      session.ungroup_node(selected);
      setSelected(null);
      setMultiSel([]);
      refresh(session);
    } catch (err) {
      alert(`Ungroup: ${err}`);
    }
  };

  /** Anchor editing: index of the path anchor being dragged, with the
   * path's points and transform captured at drag start (each move derives
   * from these, so renormalization never accumulates drift). */
  const anchorDragRef = useRef<{
    idx: number;
    vector: Extract<NodeKind, { Vector: unknown }>["Vector"];
    t0: Transform;
  } | null>(null);

  const onAnchorPointerDown = (e: React.PointerEvent, idx: number) => {
    if (!session || selected === null || !selectedKind) return;
    if (typeof selectedKind !== "object" || !("Vector" in selectedKind)) return;
    e.stopPropagation();
    // Alt takes the anchor off rather than moving it — the gesture that
    // pairs with double-clicking the outline to put one on.
    if (e.altKey) {
      try {
        session.remove_anchor(selected, idx);
      } catch (err) {
        alert(`Remove anchor: ${err}`);
        return;
      }
      refresh(session);
      return;
    }
    anchorDragRef.current = {
      idx,
      vector: JSON.parse(JSON.stringify(selectedKind.Vector)),
      t0: toTransform(session.transform_of(selected)),
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onAnchorPointerMove = (e: React.PointerEvent) => {
    const drag = anchorDragRef.current;
    if (!drag || !session || selected === null) return;
    if (e.buttons === 0) return;
    if (!("Path" in drag.vector.shape)) return;
    const [dx, dy] = layerPoint(e);
    const { t0 } = drag;
    const path = drag.vector.shape.Path;
    const pts = path.points.map((p) => [...p] as [number, number]);
    pts[drag.idx] = [(dx - t0.e) / t0.a, (dy - t0.f) / t0.d];
    // Keep anchors normalized to a (0,0) origin; the shift moves into the
    // node transform so bounds and handles stay correct.
    const minX = Math.min(...pts.map((p) => p[0]));
    const minY = Math.min(...pts.map((p) => p[1]));
    const norm = pts.map((p) => [p[0] - minX, p[1] - minY] as [number, number]);
    preview({
      Batch: [
        {
          SetKind: {
            id: selected,
            kind: {
              Vector: {
                ...drag.vector,
                shape: {
                  Path: { ...path, points: norm },
                },
              },
            },
          },
        },
        {
          SetTransform: {
            id: selected,
            transform: {
              ...t0,
              e: t0.e + minX * t0.a,
              f: t0.f + minY * t0.d,
            },
          },
        },
      ],
    });
  };

  const onAnchorPointerUp = () => {
    anchorDragRef.current = null;
    if (session?.commit_preview()) refresh(session);
  };

  /** Curve-handle editing. Same capture-at-drag-start discipline as
   * anchors; unlike anchors, handles are offsets from their anchor, so
   * nothing needs renormalizing and the transform is left alone. */
  const curveDragRef = useRef<{
    idx: number;
    /** 0 = incoming handle, 2 = outgoing (index into the [in, out] pair). */
    side: 0 | 2;
    vector: Extract<NodeKind, { Vector: unknown }>["Vector"];
    t0: Transform;
  } | null>(null);

  const onCurveHandleDown = (
    e: React.PointerEvent,
    idx: number,
    side: 0 | 2,
  ) => {
    if (!session || selected === null || !selectedKind) return;
    if (typeof selectedKind !== "object" || !("Vector" in selectedKind)) return;
    e.stopPropagation();
    curveDragRef.current = {
      idx,
      side,
      vector: JSON.parse(JSON.stringify(selectedKind.Vector)),
      t0: toTransform(session.transform_of(selected)),
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onCurveHandleMove = (e: React.PointerEvent) => {
    const drag = curveDragRef.current;
    if (!drag || !session || selected === null) return;
    // A pointermove with no button down is a hover, not a drag. Without
    // this a hover over any handle after a drag re-applies that drag's
    // captured state, quietly undoing whatever happened since.
    if (e.buttons === 0) return;

    if (!("Path" in drag.vector.shape)) return;
    const [dx, dy] = layerPoint(e);
    const { t0, idx, side } = drag;
    const path = drag.vector.shape.Path;
    const anchor = path.points[idx];
    // Offsets are in the path's own space, like the anchors themselves.
    const ox = (dx - t0.e) / t0.a - anchor[0];
    const oy = (dy - t0.f) / t0.d - anchor[1];
    const handles = withHandles(path).map(
      (h, i) => (i === idx ? [...h] : h) as [number, number, number, number],
    );
    handles[idx][side] = ox;
    handles[idx][side + 1] = oy;
    // The pair points opposite ways so the curve stays smooth through its
    // anchor, which is what dragging one of a pair is understood to mean.
    // Alt breaks the pairing and moves only the handle under the cursor —
    // how a corner gets made.
    if (!e.altKey) {
      handles[idx][side === 0 ? 2 : 0] = -ox;
      handles[idx][side === 0 ? 3 : 1] = -oy;
    }
    preview({
      SetKind: {
        id: selected,
        kind: {
          Vector: {
            ...drag.vector,
            shape: { Path: { ...path, handles } },
          },
        },
      },
    });
  };

  /** Rotation: the knob above the selection turns the node about the
   * centre of its bounds. Composed onto the transform captured at drag
   * start, so the gesture is one rotation rather than an accumulation of
   * small ones that drift. */
  const rotateDragRef = useRef<{
    t0: Transform;
    centre: [number, number];
    start: number;
  } | null>(null);

  const onRotatePointerDown = (e: React.PointerEvent) => {
    if (!session || selected === null || !selBounds || selBounds.length !== 4)
      return;
    e.stopPropagation();
    const inv = inverseOf(toTransform(session.parent_space_of(selected)));
    const docCentre: [number, number] = [
      selBounds[0] + selBounds[2] / 2,
      selBounds[1] + selBounds[3] / 2,
    ];
    const centre: [number, number] = inv ? inv(...docCentre) : docCentre;
    const [px, py] = layerPoint(e);
    rotateDragRef.current = {
      t0: toTransform(session.transform_of(selected)),
      centre,
      start: Math.atan2(py - centre[1], px - centre[0]),
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onRotatePointerMove = (e: React.PointerEvent) => {
    const drag = rotateDragRef.current;
    if (!drag || !session || selected === null) return;
    if (e.buttons === 0) return;
    const [px, py] = layerPoint(e);
    const [cx, cy] = drag.centre;
    let d = Math.atan2(py - cy, px - cx) - drag.start;
    // Shift snaps to 15 degrees, the usual courtesy for straightening up.
    if (e.shiftKey) d = Math.round(d / (Math.PI / 12)) * (Math.PI / 12);
    const [cos, sin] = [Math.cos(d), Math.sin(d)];
    const t = drag.t0;
    preview({
      SetTransform: {
        id: selected,
        transform: {
          a: cos * t.a - sin * t.b,
          b: sin * t.a + cos * t.b,
          c: cos * t.c - sin * t.d,
          d: sin * t.c + cos * t.d,
          e: cos * (t.e - cx) - sin * (t.f - cy) + cx,
          f: sin * (t.e - cx) + cos * (t.f - cy) + cy,
        },
      },
    });
  };

  const onRotatePointerUp = () => {
    rotateDragRef.current = null;
    if (session?.commit_preview()) refresh(session);
  };

  /** Gradient geometry on the canvas: the ends of a linear ramp, the centre
   * and rim of a radial one, and each stop's position along the line.
   * Coordinates are the gradient's own — the shape's box, normalized — so
   * dragging maps the cursor back through the layer and its box. */
  const gradDragRef = useRef<{
    part: "from" | "to" | "centre" | "radius" | number;
    vector: Extract<NodeKind, { Vector: unknown }>["Vector"];
  } | null>(null);

  /** A pointer position in gradient coordinates: 0..1 across the selected
   * layer's own bounding box. */
  const gradPoint = (e: {
    clientX: number;
    clientY: number;
  }): [number, number] => {
    const [px, py] = layerPoint(e);
    if (!session || selected === null || !selLocal) return [0, 0];
    const inv = inverseOf(toTransform(session.transform_of(selected)));
    const [lx, ly] = inv ? inv(px, py) : [px, py];
    const [x0, y0, x1, y1] = selLocal;
    const span = (a: number, b: number) => (Math.abs(b - a) < 1e-6 ? 1 : b - a);
    return [(lx - x0) / span(x0, x1), (ly - y0) / span(y0, y1)];
  };

  const onGradHandleDown = (
    e: React.PointerEvent,
    part: "from" | "to" | "centre" | "radius" | number,
  ) => {
    if (!session || selected === null || !selectedKind) return;
    if (typeof selectedKind !== "object" || !("Vector" in selectedKind)) return;
    e.stopPropagation();
    gradDragRef.current = {
      part,
      vector: JSON.parse(JSON.stringify(selectedKind.Vector)),
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onGradHandleMove = (e: React.PointerEvent) => {
    const drag = gradDragRef.current;
    if (!drag || !session || selected === null) return;
    if (e.buttons === 0) return;
    const g = drag.vector.gradient;
    if (!g) return;
    const [u, v] = gradPoint(e);
    let next = g;
    if ("Linear" in g) {
      const lin = g.Linear;
      if (drag.part === "from") next = { Linear: { ...lin, from: [u, v] } };
      else if (drag.part === "to") next = { Linear: { ...lin, to: [u, v] } };
      else if (typeof drag.part === "number") {
        // Project the cursor onto the ramp's line: a stop only has a
        // position along it, not a position in the plane.
        const [dx, dy] = [lin.to[0] - lin.from[0], lin.to[1] - lin.from[1]];
        const len2 = dx * dx + dy * dy;
        const t =
          len2 < 1e-9
            ? 0
            : ((u - lin.from[0]) * dx + (v - lin.from[1]) * dy) / len2;
        const stops = lin.stops.map((st, i) =>
          i === drag.part ? { ...st, offset: Math.min(1, Math.max(0, t)) } : st,
        );
        next = { Linear: { ...lin, stops } };
      }
    } else {
      const rad = g.Radial;
      if (drag.part === "centre") next = { Radial: { ...rad, center: [u, v] } };
      else if (drag.part === "radius") {
        const r = Math.hypot(u - rad.center[0], v - rad.center[1]);
        next = { Radial: { ...rad, radius: Math.max(0.02, r) } };
      }
    }
    preview({
      SetKind: {
        id: selected,
        kind: { Vector: { ...drag.vector, gradient: next } },
      },
    });
  };

  const onGradHandleUp = () => {
    gradDragRef.current = null;
    if (session?.commit_preview()) refresh(session);
  };

  /** Mask editing on the canvas. A mask created here is an ellipse with a
   * translation, so its box in the layer's parent space is simply
   * [e, e+2rx] x [f, f+2ry] — which is what these drags rewrite. */
  const maskDragRef = useRef<{
    corner: Handle | "move";
    mask: Mask;
    box: [number, number, number, number];
    start: [number, number];
  } | null>(null);

  const maskBox = (m: Mask | null): [number, number, number, number] | null => {
    if (!m || !("Vector" in m.kind)) return null;
    const { shape, transform } = m.kind.Vector;
    if (!("Ellipse" in shape)) return null;
    return [
      transform.e,
      transform.f,
      transform.e + shape.Ellipse.rx * 2,
      transform.f + shape.Ellipse.ry * 2,
    ];
  };

  const onMaskHandleDown = (e: React.PointerEvent, corner: Handle | "move") => {
    if (!session || selected === null || !selectedMask) return;
    const box = maskBox(selectedMask);
    if (!box) return;
    e.stopPropagation();
    maskDragRef.current = {
      corner,
      mask: JSON.parse(JSON.stringify(selectedMask)),
      box,
      start: layerPoint(e),
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onMaskHandleMove = (e: React.PointerEvent) => {
    const drag = maskDragRef.current;
    if (!drag || !session || selected === null) return;
    if (e.buttons === 0) return;
    const [px, py] = layerPoint(e);
    let [x0, y0, x1, y1] = drag.box;
    if (drag.corner === "move") {
      const [dx, dy] = [px - drag.start[0], py - drag.start[1]];
      [x0, y0, x1, y1] = [x0 + dx, y0 + dy, x1 + dx, y1 + dy];
    } else {
      const west = drag.corner === "nw" || drag.corner === "sw";
      const north = drag.corner === "nw" || drag.corner === "ne";
      const [fx, fy] = [west ? x1 : x0, north ? y1 : y0];
      [x0, x1] = [Math.min(px, fx), Math.max(px, fx)];
      [y0, y1] = [Math.min(py, fy), Math.max(py, fy)];
    }
    preview({
      SetMask: {
        id: selected,
        mask: {
          ...drag.mask,
          kind: {
            Vector: {
              shape: {
                Ellipse: {
                  rx: Math.max(MIN_SIZE, x1 - x0) / 2,
                  ry: Math.max(MIN_SIZE, y1 - y0) / 2,
                },
              },
              transform: { a: 1, b: 0, c: 0, d: 1, e: x0, f: y0 },
            },
          },
        },
      },
    });
  };

  const onMaskHandleUp = () => {
    maskDragRef.current = null;
    if (session?.commit_preview()) refresh(session);
  };

  const onCurveHandleUp = () => {
    handleDragRef.current = null;
    if (session?.commit_preview()) refresh(session);
  };

  const jumpHistory = (delta: number) => {
    if (!session || delta === 0) return;
    session.jump(delta);
    setSelected(null);
    setMultiSel([]);
    refresh(session);
  };

  const selectedLayer = layers.find((l) => l.id === selected) ?? null;
  // Something has to be under a layer for it to be clipped to it, so the
  // bottom-most layer of a parent cannot be.
  const clippable = layers.some(
    (l) => selectionSet.includes(l.id) && l.index > 0,
  );
  // Adjustment and filter layers act on everything below them and have no
  // box of their own; everything else, groups included, can be moved,
  // scaled and turned.
  const resizable =
    selectedLayer !== null &&
    selectedLayer.kind !== "adjustment" &&
    selectedLayer.kind !== "filter";
  /** A locked layer keeps its outline — it is still the picked layer —
   * but offers nothing to grab: it is not to be moved from the canvas. */
  const movable = resizable && !selectedLayer.locked;
  /** Handles sit over the layer they belong to, which is exactly where a
   * brush wants to paint, so the brush takes them off the canvas while it
   * is the tool in hand. The outline stays: it still says what is picked. */
  const grabbable = movable && tool !== "Paint" && tool !== "Clone";
  const selBounds =
    session && selected !== null && resizable
      ? session.bounds_of(selected)
      : null;
  /** The selection box in the layer's own axes: its local bounds mapped
   * through its transform, as four screen-space corners. Drawn instead of
   * an axis-aligned box so it turns with a rotated layer, and so the resize
   * handles sit on the corners they actually move. */
  let selQuad: [number, number][] | null = null;
  let selLocal: [number, number, number, number] | null = null;
  /** Maps a point in the selected layer's own space to screen. */
  let selToScreen: ((x: number, y: number) => [number, number]) | null = null;
  /** The selected layer's parent space — the space its own transform, and
   * its mask's, are written against. */
  let selParent: Transform | null = null;
  if (session && selected !== null && resizable) {
    const lb = session.local_bounds_of(selected);
    if (lb.length === 4) {
      selLocal = [lb[0], lb[1], lb[2], lb[3]];
      // Draw against the document, so a layer inside a moved group is
      // outlined where it actually is.
      selParent = toTransform(session.parent_space_of(selected));
      const t = composeT(
        selParent,
        toTransform(session.transform_of(selected)),
      );
      const toScreen = (x: number, y: number): [number, number] => [
        view.x + (t.a * x + t.c * y + t.e) * view.zoom,
        view.y + (t.b * x + t.d * y + t.f) * view.zoom,
      ];
      selToScreen = toScreen;
      selQuad = [
        toScreen(lb[0], lb[1]),
        toScreen(lb[2], lb[1]),
        toScreen(lb[2], lb[3]),
        toScreen(lb[0], lb[3]),
      ];
    }
  }

  let selectedKind: NodeKind | null = null;
  let selectedMask: Mask | null = null;
  let selectedEffects: Effect[] = [];
  if (session && selectedLayer) {
    try {
      selectedKind = JSON.parse(
        session.kind_json(selectedLayer.id),
      ) as NodeKind;
      selectedMask = JSON.parse(
        session.mask_json(selectedLayer.id),
      ) as Mask | null;
      selectedEffects = JSON.parse(
        session.effects_json(selectedLayer.id),
      ) as Effect[];
    } catch {
      selectedKind = null;
    }
  }

  /** Replace the selected layer's effect list. Slider drags preview, so a
   * whole drag is one history entry. */
  const setEffects = (effects: Effect[], gesture = false) => {
    if (!selectedLayer) return;
    const cmd: Command = { SetEffects: { id: selectedLayer.id, effects } };
    if (gesture) preview(cmd);
    else run(cmd);
  };

  /** The effect list with two entries exchanged. */
  const swapEffects = (a: number, b: number): Effect[] => {
    const next = [...selectedEffects];
    [next[a], next[b]] = [next[b], next[a]];
    return next;
  };

  /** Rewrite one field of the effect at `at`, keeping its variant. */
  const tuneEffect = (
    at: number,
    patch: Record<string, number | AuthoredColor>,
    gesture = false,
  ) => {
    const current = selectedEffects[at];
    if (!current) return;
    const kind = effectKind(current);
    const next = [...selectedEffects];
    next[at] = { [kind]: { ...effectBody(current), ...patch } } as Effect;
    setEffects(next, gesture);
  };

  /** The selected layer's visual box in document pixels, [x, y, w, h] —
   * what the geometry fields show and edit. */
  const selBox = ((): [number, number, number, number] | null => {
    if (!session || selected === null) return null;
    const b = session.bounds_of(selected);
    return b.length === 4 ? [b[0], b[1], b[2], b[3]] : null;
  })();

  /** Type an exact position or size. Moving is a translation in the
   * layer's own parent space; resizing scales it about its own top-left,
   * exactly as dragging the south-east handle does. For a turned layer the
   * box is the one around it, so a size typed there is the size of that
   * box rather than of the layer's own axes. */
  const setGeometry = (field: "x" | "y" | "w" | "h", value: number) => {
    if (!session || selected === null || !selBox) return;
    const [x, y, w, h] = selBox;
    const t0 = toTransform(session.transform_of(selected));
    if (field === "x" || field === "y") {
      const [dx, dy] = layerVector(
        selected,
        field === "x" ? value - x : 0,
        field === "y" ? value - y : 0,
      );
      if (dx === 0 && dy === 0) return;
      run({
        SetTransform: {
          id: selected,
          transform: { ...t0, e: t0.e + dx, f: t0.f + dy },
        },
      });
      return;
    }
    const from = field === "w" ? w : h;
    if (from <= 0 || value <= 0 || !selLocal) return;
    const ratio = value / from;
    const [sx, sy] = field === "w" ? [ratio, 1] : [1, ratio];
    const [fx, fy] = [selLocal[0], selLocal[1]];
    const [tx, ty] = [(1 - sx) * fx, (1 - sy) * fy];
    run({
      SetTransform: {
        id: selected,
        transform: {
          a: t0.a * sx,
          b: t0.b * sx,
          c: t0.c * sy,
          d: t0.d * sy,
          e: t0.a * tx + t0.c * ty + t0.e,
          f: t0.b * tx + t0.d * ty + t0.f,
        },
      },
    });
  };

  /** Copy the selected layer and its contents, and select the copy — the
   * copy is what you want to move next. */
  const duplicateSelected = () => {
    if (!session || selected === null) return;
    try {
      const copy = session.duplicate_node(selected);
      setSelected(copy);
      setMultiSel([]);
      refresh(session);
    } catch (err) {
      alert(`Duplicate: ${err}`);
    }
  };

  /** Attach an ellipse mask inscribed in the layer's current bounds. */
  const addMask = (kind: "ellipse" | "rect") => {
    if (!session || !selectedLayer) return;
    // A mask is written in the layer's parent space, so inscribe it in the
    // layer's bounds *there* — mapping its own box through its own
    // transform — rather than in document bounds, which only agree while
    // the layer sits at the top level.
    const lb = session.local_bounds_of(selectedLayer.id);
    if (lb.length !== 4) return;
    const t = toTransform(session.transform_of(selectedLayer.id));
    const xs = [
      t.a * lb[0] + t.c * lb[1] + t.e,
      t.a * lb[2] + t.c * lb[3] + t.e,
    ];
    const ys = [
      t.b * lb[0] + t.d * lb[1] + t.f,
      t.b * lb[2] + t.d * lb[3] + t.f,
    ];
    const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
    const [y0, y1] = [Math.min(...ys), Math.max(...ys)];
    const [w, h] = [x1 - x0, y1 - y0];
    run({
      SetMask: {
        id: selectedLayer.id,
        mask: {
          kind: {
            Vector: {
              shape:
                kind === "rect"
                  ? { Rect: { width: w, height: h, radius: 0 } }
                  : { Ellipse: { rx: w / 2, ry: h / 2 } },
              transform: { a: 1, b: 0, c: 0, d: 1, e: x0, f: y0 },
            },
          },
          invert: false,
        },
      },
    });
  };

  /** Make the picked shape the mask of the layer under it: the shape
   * itself goes away and its outline becomes what shows of the layer.
   * A mask is written in its owner's parent space, and both layers share
   * a parent, so the shape's own transform carries over unchanged. */
  const maskWithSelectedShape = () => {
    if (!session || selected === null || !selectedKind) return;
    if (typeof selectedKind !== "object" || !("Vector" in selectedKind)) return;
    const parent = layers.find((l) => l.id === selected)?.parent;
    const siblings = layers.filter((l) => l.parent === parent);
    // The panel lists topmost first, so the layer below is the next row.
    const at = siblings.findIndex((l) => l.id === selected);
    const below = siblings[at + 1];
    if (!below) {
      alert("Masking needs a layer underneath the shape.");
      return;
    }
    run({
      Batch: [
        {
          SetMask: {
            id: below.id,
            mask: {
              kind: {
                Vector: {
                  shape: selectedKind.Vector.shape,
                  transform: toTransform(session.transform_of(selected)),
                },
              },
              invert: false,
            },
          },
        },
        { RemoveNode: { id: selected } },
      ],
    });
    setSelected(below.id);
  };

  let history: { past: string[]; future: string[] } = { past: [], future: [] };
  if (session) {
    try {
      history = JSON.parse(session.history_json());
    } catch {
      // keep empty history on parse issues
    }
  }

  const setKind = (kind: NodeKind, gesture: boolean) => {
    if (!selectedLayer) return;
    const cmd: Command = { SetKind: { id: selectedLayer.id, kind } };
    if (gesture) preview(cmd);
    else run(cmd);
  };

  /** Reorder within the parent group: +1 raises toward the top. */
  const reorderSelected = (direction: 1 | -1) => {
    if (!selectedLayer) return;
    run({
      MoveNode: {
        id: selectedLayer.id,
        parent: selectedLayer.parent,
        index: selectedLayer.index + direction,
      },
    });
  };

  /** Opacity and blend reach every picked layer, not only the one whose
   * properties the panel happens to be showing — one history entry for
   * the lot, named after how many it touched. */
  const commitOpacity = () => {
    if (opacityDraft !== null && session && selectionSet.length > 0) {
      session.set_opacity_of(new Float64Array(selectionSet), opacityDraft);
      refresh(session);
    }
    setOpacityDraft(null);
  };

  const setBlendOfSelection = (blend: BlendMode) => {
    if (!session || selectionSet.length === 0) return;
    session.set_blend_of(new Float64Array(selectionSet), blend);
    refresh(session);
  };

  /** The document's name as a file's: trimmed, and never empty. */
  const fileName = () =>
    docName.trim().replace(/[\\/:*?"<>|]/g, "-") || "untitled";

  const saveFile = () => {
    if (!session) return;
    download(session.save(), `${fileName()}.chitra`, "application/zip");
  };

  const exportPng = () => {
    if (!session) return;
    download(session.export_png(), `${fileName()}.png`, "image/png");
  };

  /** PNG at a multiple of the document's size — the @2x/@3x a screen
   * asset wants, re-solved rather than upsampled. */
  const exportPngAt = (scale: number) => {
    if (!session) return;
    download(
      session.export_png_at(scale, 0, 0, 0, 0),
      `${fileName()}@${scale}x.png`,
      "image/png",
    );
  };

  /** PNG of just the picked layers' box, at document resolution. */
  const exportSelectionPng = () => {
    if (!session || selectionSet.length === 0) return;
    const box = unionBounds(selectionSet);
    if (!box) return;
    const [x, y, w, h] = [box[0], box[1], box[2] - box[0], box[3] - box[1]];
    try {
      download(
        session.export_png_at(1, x, y, w, h),
        `${fileName()}-selection.png`,
        "image/png",
      );
    } catch (err) {
      alert(`Export: ${err}`);
    }
  };

  /** Every frame on the page, each as its own PNG at its own size —
   * what having frames is for. Named after the frame, so a page of them
   * comes out as a set of named pictures. */
  const exportArtboards = () => {
    if (!session) return;
    const boards = layers.filter((l) => l.kind === "artboard");
    if (boards.length === 0) return;
    try {
      for (const board of boards) {
        download(
          session.export_artboard_png(board.id, 1),
          `${fileName()} - ${board.name}.png`,
          "image/png",
        );
      }
    } catch (err) {
      alert(`Export: ${err}`);
    }
  };

  const exportJpeg = () => {
    if (!session) return;
    download(session.export_jpeg(92), `${fileName()}.jpg`, "image/jpeg");
  };

  const exportPdf = () => {
    if (!session) return;
    try {
      download(session.export_pdf(), `${fileName()}.pdf`, "application/pdf");
    } catch (err) {
      alert(`PDF export: ${err}`);
    }
  };

  const exportTiff = () => {
    if (!session) return;
    try {
      download(session.export_cmyk_tiff(), `${fileName()}.tif`, "image/tiff");
    } catch (err) {
      alert(`CMYK TIFF export: ${err}`);
    }
  };

  const exportSvg = () => {
    if (!session) return;
    download(
      new TextEncoder().encode(session.export_svg()),
      `${fileName()}.svg`,
      "image/svg+xml",
    );
  };

  /** The picked layers' box as a PNG on the system clipboard, for pasting
   * into other applications — the in-app clipboard carries layers, which
   * nothing outside can read. Pasting it back here places it as an image,
   * which is what a picture on the clipboard is. */
  const copyAsImage = async () => {
    if (!session || selectionSet.length === 0) return;
    const box = unionBounds(selectionSet);
    if (!box) return;
    const [x, y, w, h] = [box[0], box[1], box[2] - box[0], box[3] - box[1]];
    if (!(w > 0 && h > 0)) return;
    try {
      const png = session.export_png_at(1, x, y, w, h);
      const blob = new Blob([png as BlobPart], { type: "image/png" });
      await navigator.clipboard.write([
        new ClipboardItem({ "image/png": blob }),
      ]);
    } catch (err) {
      alert(`Copy as image: ${err}`);
    }
  };

  /** Open a .chitra from its bytes, whether chosen or dropped. */
  const openDocumentBytes = (bytes: Uint8Array, name?: string) => {
    try {
      const s = WasmSession.open(bytes);
      // Faces the file carried are registered by the open; offer them.
      setFontNames(JSON.parse(WasmSession.font_names()) as string[]);
      if (name) setDocName(name.replace(/\.chitra$/i, ""));
      setDocDpi(s.dpi);
      setSession(s);
      setDocumentSize(s.width, s.height);
      setCmyk(s.cmyk);
      setSelected(null);
      setHasIcc(s.has_cmyk_profile);
      setProofing(false);
      setGamutWarn(false);
      refresh(s);
      fitView();
    } catch (err) {
      alert(`Could not open document: ${err}`);
    }
  };

  const openFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    file
      .arrayBuffer()
      .then((buf) => openDocumentBytes(new Uint8Array(buf), file.name));
  };

  /** Bring an image file in as a layer and pick it — whichever way it
   * arrived: the file dialog, a drop on the canvas, or a paste. */
  const placeImageFile = useCallback(
    (file: File) => {
      if (!session) return;
      // An SVG comes in as shapes, anything else as pixels.
      const vector = file.type === "image/svg+xml" || /\.svg$/i.test(file.name);
      file.arrayBuffer().then((buf) => {
        try {
          const bytes = new Uint8Array(buf);
          const id = vector
            ? session.place_svg(bytes, file.name || "Pasted drawing")
            : session.place_image(bytes, file.name || "Pasted image");
          setSelected(id);
          setMultiSel([]);
          refresh(session);
        } catch (err) {
          alert(`Could not place image: ${err}`);
        }
      });
    },
    [session, refresh],
  );

  const placeImage = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (file) placeImageFile(file);
  };

  /** Files dropped on the canvas: images become layers, a .chitra opens.
   * A drop that holds both opens the document and leaves the images —
   * placing them would put them into the session the open just replaced. */
  const onHostDrop = (e: React.DragEvent) => {
    e.preventDefault();
    const files = Array.from(e.dataTransfer.files);
    const doc = files.find((f) => f.name.toLowerCase().endsWith(".chitra"));
    if (doc) {
      doc
        .arrayBuffer()
        .then((buf) => openDocumentBytes(new Uint8Array(buf), doc.name));
      return;
    }
    for (const file of files) {
      if (file.type.startsWith("image/")) placeImageFile(file);
    }
  };

  // Paste is handled at the event rather than the keystroke, because only
  // the event knows what the clipboard holds: an image from another app
  // becomes a layer, anything else falls back to the in-app clipboard.
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      if (isTextEntry(e.target)) return;
      pasteSeen.current = true;
      const image = Array.from(e.clipboardData?.files ?? []).find((f) =>
        f.type.startsWith("image/"),
      );
      e.preventDefault();
      if (image) placeImageFile(image);
      else pasteClipboard();
    };
    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  });

  const [hasIcc, setHasIcc] = useState(false);
  const [proofing, setProofing] = useState(false);
  const [gamutWarn, setGamutWarn] = useState(false);

  const applyProofing = (proof: boolean, gamut: boolean) => {
    if (!session) return;
    try {
      session.set_proofing(proof, gamut);
      setProofing(proof);
      setGamutWarn(gamut);
      refresh(session);
    } catch (err) {
      alert(`Soft proofing: ${err}`);
    }
  };

  /** Register a font file under its own name, offer it in the Text panel,
   * and set the picked text layer in it — which is what loading one is
   * usually for. A font stays for the page's lifetime, and a saved .chitra
   * carries the faces its text is set in, so the document reads the same
   * wherever it is opened next. */
  const loadFont = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file || !session) return;
    const name = file.name.replace(/\.(ttf|otf)$/i, "");
    file.arrayBuffer().then((buf) => {
      try {
        WasmSession.register_font(name, new Uint8Array(buf));
      } catch (err) {
        alert(`Could not load font: ${err}`);
        return;
      }
      setFontNames(JSON.parse(WasmSession.font_names()) as string[]);
      if (
        selectedKind &&
        typeof selectedKind === "object" &&
        "Text" in selectedKind
      ) {
        run({
          SetKind: {
            id: selectedLayer!.id,
            kind: { Text: { ...selectedKind.Text, font: name } },
          },
        });
      }
    });
  };

  const loadIccProfile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file || !session) return;
    file.arrayBuffer().then((buf) => {
      try {
        session.set_cmyk_profile(new Uint8Array(buf));
        setHasIcc(true);
        refresh(session);
      } catch (err) {
        alert(`Could not use ICC profile: ${err}`);
      }
    });
  };

  return (
    <div className="editor">
      {recoverable && (
        <div className="recover" role="status">
          <span>A draft from your last session is here.</span>
          <button
            type="button"
            onClick={() => {
              const bytes = recoverable;
              setRecoverable(null);
              openDocumentBytes(bytes, draftName.current ?? undefined);
            }}
          >
            Restore
          </button>
          <button
            type="button"
            onClick={() => {
              setRecoverable(null);
              clearDraft();
            }}
          >
            Discard
          </button>
        </div>
      )}
      <header className="topbar">
        <span className="brand">Chitrakar</span>
        <nav className="menubar" aria-label="Main menu">
          <MenuButton
            label="File"
            open={openMenu === "file"}
            onOpen={() => setOpenMenu(openMenu === "file" ? null : "file")}
            onHover={() => openMenu && setOpenMenu("file")}
            onClose={() => setOpenMenu(null)}
          >
            <MenuItem icon="newDoc" onClick={() => setNewDocOpen(true)}>
              New document…
            </MenuItem>
            <hr />
            <MenuItem icon="open" onClick={() => pick(openInputRef)}>
              Open…
            </MenuItem>
            <MenuItem icon="image" onClick={() => pick(placeInputRef)}>
              Place image…
            </MenuItem>
            <MenuItem icon="text" onClick={() => pick(fontInputRef)}>
              Load font…
            </MenuItem>
            <MenuItem icon="save" onClick={saveFile}>
              Save
            </MenuItem>
            <hr />
            <MenuItem icon="export" onClick={exportPng}>
              Export PNG
            </MenuItem>
            <MenuItem icon="export" onClick={() => exportPngAt(2)} hint="@2x">
              Export PNG at 2×
            </MenuItem>
            <MenuItem icon="export" onClick={() => exportPngAt(3)} hint="@3x">
              Export PNG at 3×
            </MenuItem>
            {selectionSet.length > 0 && (
              <MenuItem icon="export" onClick={exportSelectionPng}>
                Export selection as PNG
              </MenuItem>
            )}
            {layers.some((l) => l.kind === "artboard") && (
              <MenuItem
                icon="frame"
                onClick={exportArtboards}
                hint="one each"
              >
                Export every artboard
              </MenuItem>
            )}
            <MenuItem icon="export" onClick={exportJpeg} hint="flattened">
              Export JPEG
            </MenuItem>
            <MenuItem icon="export" onClick={exportSvg}>
              Export SVG
            </MenuItem>
            <MenuItem
              icon="export"
              onClick={exportPdf}
              hint={hasIcc ? "CMYK" : "sRGB"}
            >
              Export PDF
            </MenuItem>
            {hasIcc && (
              <MenuItem icon="export" onClick={exportTiff} hint="CMYK">
                Export TIFF
              </MenuItem>
            )}
            <hr />
            <MenuItem
              icon={hasIcc ? "check" : "profile"}
              onClick={() => pick(iccInputRef)}
            >
              {hasIcc ? "Replace press profile…" : "Load press profile…"}
            </MenuItem>
          </MenuButton>

          <MenuButton
            label="Edit"
            open={openMenu === "edit"}
            onOpen={() => setOpenMenu(openMenu === "edit" ? null : "edit")}
            onHover={() => openMenu && setOpenMenu("edit")}
            onClose={() => setOpenMenu(null)}
          >
            <MenuItem icon="undo" onClick={undo} hint="Ctrl+Z">
              Undo
            </MenuItem>
            <MenuItem icon="redo" onClick={redo} hint="Ctrl+Shift+Z">
              Redo
            </MenuItem>
            <MenuItem icon="cut" onClick={cutSelected} hint="Ctrl+X">
              Cut
            </MenuItem>
            <MenuItem icon="copy" onClick={copySelected} hint="Ctrl+C">
              Copy
            </MenuItem>
            {selectionSet.length > 0 && (
              <MenuItem icon="copy" onClick={copyAsImage}>
                Copy as image
              </MenuItem>
            )}
            <MenuItem icon="paste" onClick={pasteClipboard} hint="Ctrl+V">
              Paste
            </MenuItem>
            <MenuItem
              icon="duplicate"
              onClick={duplicateSelected}
              hint="Ctrl+D"
            >
              Duplicate
            </MenuItem>
            <MenuItem icon="copy" onClick={copyStyle} hint="Ctrl+Alt+C">
              Copy style
            </MenuItem>
            <MenuItem icon="paste" onClick={pasteStyle} hint="Ctrl+Alt+V">
              Paste style
            </MenuItem>
            <MenuItem icon="flipH" onClick={() => flipSelection(true)}>
              Flip horizontal
            </MenuItem>
            <MenuItem icon="flipV" onClick={() => flipSelection(false)}>
              Flip vertical
            </MenuItem>
            <MenuItem icon="trash" onClick={deleteSelected} hint="Del">
              Delete
            </MenuItem>
            <MenuItem icon="selectAll" onClick={selectAll} hint="Ctrl+A">
              Select all
            </MenuItem>
            <MenuItem icon="check" onClick={deselect} hint="Esc">
              Deselect
            </MenuItem>
          </MenuButton>

          <MenuButton
            label="View"
            open={openMenu === "view"}
            onOpen={() => setOpenMenu(openMenu === "view" ? null : "view")}
            onHover={() => openMenu && setOpenMenu("view")}
            onClose={() => setOpenMenu(null)}
          >
            {(["px", "mm", "in"] as Units[]).map((u) => (
              <MenuItem
                key={u}
                icon="units"
                onClick={() => setUnits(u)}
                hint={units === u ? "✓" : undefined}
              >
                {UNIT_LABELS[u]}
              </MenuItem>
            ))}
            <hr />
            <MenuItem icon="fit" onClick={fitView}>
              Fit document to window
            </MenuItem>
            <MenuItem icon="text" onClick={() => setShowKeys(true)} hint="?">
              Keys and gestures
            </MenuItem>
            <MenuItem icon="zoomIn" onClick={() => zoomBy(1.25)}>
              Zoom in
            </MenuItem>
            <MenuItem icon="zoomOut" onClick={() => zoomBy(0.8)}>
              Zoom out
            </MenuItem>
            <MenuItem icon="actualSize" onClick={() => zoomTo(1)}>
              Actual size
            </MenuItem>
            <MenuItem icon="selectAll" onClick={zoomToSelection}>
              Zoom to selection
            </MenuItem>
            <MenuItem
              icon={showGuides ? "check" : "fit"}
              onClick={() => setShowGuides((v) => !v)}
            >
              {showGuides ? "Hide guides" : "Show guides"}
            </MenuItem>
            <MenuItem icon="trash" onClick={() => setGuidesDoc([])}>
              Clear guides
            </MenuItem>
          </MenuButton>
        </nav>

        <span className="spacer" />

        {hasIcc && (
          <div className="chrome-group" role="group" aria-label="Soft proofing">
            <button
              className={proofing ? "chrome-button toggled" : "chrome-button"}
              onClick={() => applyProofing(!proofing, false)}
              title="Soft proof: preview what the press can reproduce"
              aria-pressed={proofing}
            >
              <Icon name="proof" />
              Proof
            </button>
            <button
              className={gamutWarn ? "chrome-button toggled" : "chrome-button"}
              onClick={() =>
                gamutWarn
                  ? applyProofing(proofing, false)
                  : applyProofing(true, true)
              }
              title="Mark out-of-gamut pixels grey"
              aria-pressed={gamutWarn}
            >
              <Icon name="gamut" />
              Gamut
            </button>
          </div>
        )}

        <div className="chrome-group" role="group" aria-label="History">
          <button
            className="chrome-button icon-only"
            onClick={undo}
            title="Undo (Ctrl+Z)"
          >
            <Icon name="undo" />
          </button>
          <button
            className="chrome-button icon-only"
            onClick={redo}
            title="Redo (Ctrl+Shift+Z)"
          >
            <Icon name="redo" />
          </button>
        </div>

        <input
          className="doc-name"
          value={docName}
          onChange={(e) => setDocName(e.target.value)}
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          spellCheck={false}
          title="What this document is called; every save and export is named after it"
          aria-label="Document name"
        />
        <span className="doc-chip">
          {hasIcc && (
            <span className="icc-badge" title="A CMYK press profile is loaded">
              ICC ✓
            </span>
          )}
          {cmyk ? "CMYK" : "RGB"}, {docSize[0]}×{docSize[1]}
          {units !== "px" && (
            <>
              {" "}
              ({inUnits(docSize[0], units, docDpi)}×
              {inUnits(docSize[1], units, docDpi)} {units})
            </>
          )}{" "}
          · {docDpi} dpi · {Math.round(view.zoom * 100)}%
        </span>

        {/* The file inputs live here, outside the menus, so they stay mounted
            whether or not a menu is open; the menu items just click them. */}
        <input
          ref={openInputRef}
          type="file"
          accept=".chitra"
          onChange={openFile}
          hidden
        />
        <input
          ref={placeInputRef}
          type="file"
          accept="image/png,image/jpeg,image/svg+xml"
          onChange={placeImage}
          hidden
        />
        <input
          ref={iccInputRef}
          type="file"
          accept=".icc,.icm"
          onChange={loadIccProfile}
          hidden
        />
        <input
          ref={fontInputRef}
          type="file"
          accept=".ttf,.otf"
          onChange={loadFont}
          hidden
        />
      </header>
      {showKeys && <KeysDialog onClose={() => setShowKeys(false)} />}
      {newDocOpen && (
        <NewDocDialog
          onCancel={() => setNewDocOpen(false)}
          onCreate={(w, h, useCmyk, dpi) => {
            setNewDocOpen(false);
            newDocument(useCmyk, w, h, dpi);
          }}
        />
      )}
      <div className="workspace">
        <nav className="toolbar" aria-label="Tools">
          {TOOLS.map((t) => (
            <button
              key={t}
              className={t === tool ? "tool active" : "tool"}
              onClick={() => {
                setTool(t);
                setPenPoints([]);
              }}
              title={`${t} (${TOOL_HINT[t]})`}
              aria-label={t}
            >
              <Icon name={TOOL_ICONS[t]} size={20} />
            </button>
          ))}
          <input
            type="color"
            value={fill}
            onChange={(e) => setFill(e.target.value)}
            title="Fill color"
            aria-label="Fill colour"
            className="fill-swatch"
          />
          {(tool === "Paint" || tool === "Clone") && (
            <>
              <input
                type="number"
                min={1}
                max={200}
                value={paintSize}
                onChange={(e) =>
                  setPaintSize(
                    Math.max(1, Math.min(200, Number(e.target.value))),
                  )
                }
                title="Brush width"
                aria-label="Paint width"
                className="brush-size"
              />
              <input
                type="range"
                min={0}
                max={100}
                value={Math.round(paintSoftness * 100)}
                onChange={(e) => setPaintSoftness(Number(e.target.value) / 100)}
                title="How soft the brush's edge is"
                aria-label="Brush softness"
                className="paint-softness"
              />
              {tool === "Clone" && (
                <button
                  className={`icon-button${healing ? " active" : ""}`}
                  onClick={() => setHealing((on) => !on)}
                  title="Heal: lay the texture down in the colour of the place it lands"
                  aria-label="Heal"
                  aria-pressed={healing}
                >
                  <Icon name="heal" />
                </button>
              )}
              {tool === "Paint" && (
              <button
                className={`icon-button${erasing ? " active" : ""}`}
                onClick={() => setErasing((on) => !on)}
                title="Rub paint out — on a paint layer its own paint, on any other layer a piece out of the layer itself"
                aria-label="Erase"
                aria-pressed={erasing}
              >
                <Icon name="eraser" />
              </button>
              )}
            </>
          )}
          {tool === "Brush" && (
            <input
              type="number"
              min={1}
              max={64}
              value={brushSize}
              onChange={(e) =>
                setBrushSize(Math.max(1, Math.min(64, Number(e.target.value))))
              }
              title="Brush width"
              aria-label="Brush width"
              className="brush-size"
            />
          )}
        </nav>
        <main
          className={`canvas-host${
            tool === "Paint" || tool === "Clone" ? " painting" : ""
          }`}
          ref={hostRef}
          onDragOver={(e) => e.preventDefault()}
          onDrop={onHostDrop}
          onPointerDown={onHostPointerDown}
          onPointerMove={onHostPointerMove}
          onPointerUp={onHostPointerUp}
          onPointerLeave={() => setBrushAt(null)}
        >
          {/* Rulers along the top and left edges, marked in document
              units and following the view. Dragging out of one places a
              guide; dropping a guide back on one throws it away. */}
          {(() => {
            // Ticks are chosen in the unit the rulers read in, then
            // placed in pixels: a step is the first that puts ticks at
            // least sixty screen pixels apart.
            const unitPx = 1 / perPixel(units, docDpi);
            const candidates = UNIT_TICKS[units];
            const step =
              candidates.find((s) => s * unitPx * view.zoom >= 60) ??
              candidates[candidates.length - 1];
            const ticks = (vertical: boolean) => {
              const span = vertical ? viewport[1] : viewport[0];
              const origin = vertical ? view.y : view.x;
              const first =
                Math.floor(-origin / view.zoom / unitPx / step) * step;
              const last = (span - origin) / view.zoom / unitPx;
              const out: number[] = [];
              for (let v = first; v <= last; v += step)
                out.push(Math.round(v * 1000) / 1000);
              return out;
            };
            const ruler = (vertical: boolean) => (
              <div
                className={vertical ? "ruler ruler-left" : "ruler ruler-top"}
                aria-label={vertical ? "Vertical ruler" : "Horizontal ruler"}
                onPointerDown={(e) => startGuideDrag(vertical, null, e)}
              >
                <svg>
                  {ticks(vertical).map((v) => {
                    const at =
                      (vertical ? view.y : view.x) + v * unitPx * view.zoom;
                    return vertical ? (
                      <g key={v}>
                        <line x1={RULER - 5} y1={at} x2={RULER} y2={at} />
                        {/* Turned a quarter to read up the ruler. The
                            rotation puts the glyphs to the left of their
                            anchor, so the anchor sits clear of the edge or
                            they fall off it. */}
                        <text
                          x={RULER - 6}
                          y={at + 3}
                          transform={`rotate(-90 ${RULER - 6} ${at + 3})`}
                        >
                          {v}
                        </text>
                      </g>
                    ) : (
                      <g key={v}>
                        <line x1={at} y1={RULER - 5} x2={at} y2={RULER} />
                        <text x={at + 3} y={RULER - 7}>
                          {v}
                        </text>
                      </g>
                    );
                  })}
                </svg>
              </div>
            );
            return (
              <>
                {ruler(false)}
                {ruler(true)}
                <div className="ruler-corner" />
              </>
            );
          })()}
          {/* Guides: the ones placed, plus the one being dragged. */}
          {showGuides && (
            <svg className="guide-overlay" aria-hidden="true">
              {docGuides.map((g, i) => {
                const vertical = guideIsVertical(g);
                const at =
                  (vertical ? view.x : view.y) + guideAt(g) * view.zoom;
                const hidden = guideDrag?.index === i;
                const ends = {
                  x1: vertical ? at : 0,
                  y1: vertical ? 0 : at,
                  x2: vertical ? at : viewport[0],
                  y2: vertical ? viewport[1] : at,
                };
                return hidden ? null : (
                  <g key={`${vertical}${i}`}>
                    {/* A hairline is hard to catch with a pointer, so the
                        line that takes the grab is wider and invisible. */}
                    <line
                      className="guide-hit"
                      {...ends}
                      data-guide={
                        vertical ? `v${guideAt(g)}` : `h${guideAt(g)}`
                      }
                      onPointerDown={(e) => startGuideDrag(vertical, i, e)}
                    />
                    <line className="guide" {...ends} />
                  </g>
                );
              })}
              {guideDrag && (
                <line
                  className="guide dragging"
                  x1={
                    guideDrag.vertical ? view.x + guideDrag.at * view.zoom : 0
                  }
                  y1={
                    guideDrag.vertical ? 0 : view.y + guideDrag.at * view.zoom
                  }
                  x2={
                    guideDrag.vertical
                      ? view.x + guideDrag.at * view.zoom
                      : viewport[0]
                  }
                  y2={
                    guideDrag.vertical
                      ? viewport[1]
                      : view.y + guideDrag.at * view.zoom
                  }
                />
              )}
            </svg>
          )}
          {/* The page itself: white, with the shadow that lifts it off
              the desk. The canvas no longer is the page — it is a window
              onto it, covering the whole viewport — so the page's own
              extent has to be drawn. */}
          <div
            id="engine-page"
            style={{
              left: view.x,
              top: view.y,
              width: docSize[0] * view.zoom,
              height: docSize[1] * view.zoom,
            }}
          />
          <canvas
            id="engine-canvas"
            ref={canvasRef}
            width={Math.round(viewport[0] * dpr)}
            height={Math.round(viewport[1] * dpr)}
            /* Where the document's origin sits in the backing store, and
               how many of its pixels one document pixel takes — published
               so anything reading it can convert into it. */
            data-origin-x={view.x * dpr}
            data-origin-y={view.y * dpr}
            data-frame-scale={view.zoom * dpr}
            style={{ width: viewport[0], height: viewport[1] }}
            onDoubleClick={onCanvasDoubleClick}
            onPointerDown={onCanvasPointerDown}
            onPointerMove={onCanvasPointerMove}
            onPointerUp={onCanvasPointerUp}
          />
          {cropRect && (
            <svg className="crop-overlay" aria-hidden="true">
              {/* Four panels dimming everything the crop would discard,
                  which is how the framing reads at a glance. */}
              {(
                [
                  [0, 0, "100%", cropRect[1]],
                  [0, cropRect[3], "100%", "100%"],
                  [0, cropRect[1], cropRect[0], cropRect[3] - cropRect[1]],
                  [cropRect[2], cropRect[1], "100%", cropRect[3] - cropRect[1]],
                ] as [number, number, number | string, number | string][]
              ).map(([x, y, w, h], i) => (
                <rect
                  key={i}
                  className="crop-shade"
                  x={x}
                  y={y}
                  width={w}
                  height={h}
                />
              ))}
              <rect
                className="crop-frame"
                x={cropRect[0]}
                y={cropRect[1]}
                width={cropRect[2] - cropRect[0]}
                height={cropRect[3] - cropRect[1]}
              />
            </svg>
          )}
          {marquee && (
            <svg className="marquee-overlay" aria-hidden="true">
              <rect
                x={view.x + marquee[0] * view.zoom}
                y={view.y + marquee[1] * view.zoom}
                width={(marquee[2] - marquee[0]) * view.zoom}
                height={(marquee[3] - marquee[1]) * view.zoom}
              />
            </svg>
          )}
          {/* How big the brush is, and how much of that is its fade: the
              outer ring is where it stops, the inner one where its solid
              core ends. An eraser's ring is dashed, since what it does to
              the canvas is the opposite of what the colour says. */}
          {tool === "Clone" && cloneFrom && (
            <svg className="brush-ring clone-source" aria-hidden="true">
              <circle
                cx={view.x + cloneFrom[0] * view.zoom}
                cy={view.y + cloneFrom[1] * view.zoom}
                r={Math.max(3, (paintSize / 2) * view.zoom)}
              />
              <path
                d={`M${view.x + cloneFrom[0] * view.zoom - 7} ${
                  view.y + cloneFrom[1] * view.zoom
                } h14 M${view.x + cloneFrom[0] * view.zoom} ${
                  view.y + cloneFrom[1] * view.zoom - 7
                } v14`}
              />
            </svg>
          )}
          {(tool === "Paint" || tool === "Clone") && brushAt && (
            <svg
              className={`brush-ring${erasing ? " erasing" : ""}`}
              aria-hidden="true"
            >
              <circle
                cx={brushAt[0]}
                cy={brushAt[1]}
                r={Math.max(1, (paintSize / 2) * view.zoom)}
              />
              {paintSoftness > 0.05 && (
                <circle
                  className="core"
                  cx={brushAt[0]}
                  cy={brushAt[1]}
                  r={Math.max(
                    0.5,
                    (paintSize / 2) * (1 - paintSoftness) * view.zoom,
                  )}
                />
              )}
            </svg>
          )}
          {(guides.x.length > 0 || guides.y.length > 0) && (
            <svg className="snap-overlay" aria-hidden="true">
              {guides.x.map((x) => (
                <line
                  key={`x${x}`}
                  x1={view.x + x * view.zoom}
                  y1={view.y}
                  x2={view.x + x * view.zoom}
                  y2={view.y + docSize[1] * view.zoom}
                />
              ))}
              {guides.y.map((y) => (
                <line
                  key={`y${y}`}
                  x1={view.x}
                  y1={view.y + y * view.zoom}
                  x2={view.x + docSize[0] * view.zoom}
                  y2={view.y + y * view.zoom}
                />
              ))}
            </svg>
          )}
          {penPoints.length > 0 && (
            <svg className="pen-overlay" aria-hidden="true">
              <polyline
                points={penPoints
                  .map(
                    (p) =>
                      `${view.x + p[0] * view.zoom},${view.y + p[1] * view.zoom}`,
                  )
                  .join(" ")}
              />
              {/* Only the pen closes a path by clicking its first anchor;
                  a brush stroke reuses the preview line but not that mark. */}
              {tool === "Pen" && (
                <circle
                  className="pen-first"
                  cx={view.x + penPoints[0][0] * view.zoom}
                  cy={view.y + penPoints[0][1] * view.zoom}
                  r={5}
                />
              )}
            </svg>
          )}
          {session &&
            selected !== null &&
            selectedKind &&
            typeof selectedKind === "object" &&
            "Vector" in selectedKind &&
            "Path" in selectedKind.Vector.shape &&
            (() => {
              const t = toTransform(session.transform_of(selected));
              const path = selectedKind.Vector.shape.Path;
              const handles = withHandles(path);
              const screen = (x: number, y: number) => ({
                left: view.x + (t.e + x * t.a) * view.zoom,
                top: view.y + (t.f + y * t.d) * view.zoom,
              });
              return path.points.flatMap((p, i) => {
                const at = screen(p[0], p[1]);
                const dots = [
                  <div
                    key={`a${i}`}
                    className="anchor"
                    data-anchor={i}
                    style={{ left: at.left - 5, top: at.top - 5 }}
                    onPointerDown={(e) => onAnchorPointerDown(e, i)}
                    onPointerMove={onAnchorPointerMove}
                    onPointerUp={onAnchorPointerUp}
                  />,
                ];
                // Only handles that are actually out from their anchor get a
                // dot: one sitting on the anchor would cover it and swallow
                // its drags. "Convert to curves" is what brings them out.
                for (const side of [0, 2] as const) {
                  const [ox, oy] = [handles[i][side], handles[i][side + 1]];
                  if (ox === 0 && oy === 0) continue;
                  const hx = p[0] + ox;
                  const hy = p[1] + oy;
                  const h = screen(hx, hy);
                  dots.push(
                    <div
                      key={`h${i}-${side}`}
                      className="curve-handle"
                      data-handle={`${i}-${side}`}
                      style={{ left: h.left - 4, top: h.top - 4 }}
                      onPointerDown={(e) => onCurveHandleDown(e, i, side)}
                      onPointerMove={onCurveHandleMove}
                      onPointerUp={onCurveHandleUp}
                    />,
                  );
                }
                return dots;
              });
            })()}
          {inlineText &&
            session &&
            selected === inlineText.id &&
            selLocal &&
            selParent &&
            selectedKind &&
            typeof selectedKind === "object" &&
            "Text" in selectedKind &&
            (() => {
              // The editor lives in the block's own coordinates: one CSS
              // matrix — the layer's transform through its parents and
              // the view — puts it over the block however the block is
              // turned or scaled, and sizes its type with it.
              const t = composeT(
                selParent,
                toTransform(session.transform_of(selected)),
              );
              const z = view.zoom;
              const spec = selectedKind.Text;
              const fill = spec.fill;
              const caret =
                "Srgb" in fill
                  ? `rgb(${fill.Srgb.r * 255}, ${fill.Srgb.g * 255}, ${fill.Srgb.b * 255})`
                  : "currentColor";
              return (
                <textarea
                  ref={inlineRef}
                  className="inline-text"
                  aria-label="Text on canvas"
                  value={inlineText.value}
                  spellCheck={false}
                  style={{
                    transform: `matrix(${t.a * z}, ${t.b * z}, ${t.c * z}, ${t.d * z}, ${view.x + t.e * z}, ${view.y + t.f * z})`,
                    width:
                      Math.max(spec.width, selLocal[2] - selLocal[0]) +
                      spec.size,
                    height: selLocal[3] - selLocal[1] + spec.size,
                    fontFamily: `"${spec.font || "DejaVu Sans"}", sans-serif`,
                    // The document's size is the ascent-to-descent height;
                    // CSS wants the em, which in these faces is 0.86 of it.
                    fontSize: spec.size * 0.86,
                    lineHeight: `${spec.size * spec.line_height}px`,
                    letterSpacing: spec.letter_spacing * spec.size,
                    textAlign:
                      spec.align === "Center"
                        ? "center"
                        : spec.align === "Right"
                          ? "right"
                          : "left",
                    whiteSpace: spec.width > 0 ? "pre-wrap" : "pre",
                    fontStyle: spec.italic ? "italic" : "normal",
                    caretColor: caret,
                  }}
                  onChange={(e) => typeInlineText(e.target.value)}
                  onBlur={() => closeInlineText(true)}
                  onKeyDown={(e) => {
                    if (e.key === "Escape") {
                      e.preventDefault();
                      closeInlineText(false);
                    } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                      e.preventDefault();
                      closeInlineText(true);
                    }
                    e.stopPropagation();
                  }}
                />
              );
            })()}
          {selQuad && (
            <>
              <svg className="sel-outline" aria-hidden="true">
                <polygon points={selQuad.map((p) => p.join(",")).join(" ")} />
              </svg>
              {/* The knob sits off the top edge along the box's own normal,
                  so it stays above the layer however the layer is turned.
                  A locked layer has none: nothing about it turns. */}
              {grabbable &&
                (() => {
                  const [tl, tr, , bl] = selQuad;
                  const mid = [(tl[0] + tr[0]) / 2, (tl[1] + tr[1]) / 2];
                  const down = [bl[0] - tl[0], bl[1] - tl[1]];
                  const len = Math.hypot(down[0], down[1]) || 1;
                  const knob = [
                    mid[0] - (down[0] / len) * 26,
                    mid[1] - (down[1] / len) * 26,
                  ];
                  return (
                    <>
                      <svg className="sel-outline" aria-hidden="true">
                        <line
                          x1={mid[0]}
                          y1={mid[1]}
                          x2={knob[0]}
                          y2={knob[1]}
                        />
                      </svg>
                      <div
                        className="rot-handle"
                        data-handle="rotate"
                        title="Rotate (hold Shift to snap)"
                        aria-label="Rotate layer"
                        style={{ left: knob[0] - 6, top: knob[1] - 6 }}
                        onPointerDown={onRotatePointerDown}
                        onPointerMove={onRotatePointerMove}
                        onPointerUp={onRotatePointerUp}
                      />
                    </>
                  );
                })()}
              {/* The mask, drawn and edited where it applies. Dashed and
                  in its own colour, because it is not the layer's outline
                  and confusing the two makes both useless. */}
              {(() => {
                const box = maskBox(selectedMask);
                if (!box || !selParent) return null;
                const p = selParent;
                const at = (x: number, y: number): [number, number] => [
                  view.x + (p.a * x + p.c * y + p.e) * view.zoom,
                  view.y + (p.b * x + p.d * y + p.f) * view.zoom,
                ];
                const [x0, y0, x1, y1] = box;
                const quad: [number, number][] = [
                  at(x0, y0),
                  at(x1, y0),
                  at(x1, y1),
                  at(x0, y1),
                ];
                const centre = at((x0 + x1) / 2, (y0 + y1) / 2);
                const knob = (
                  key: string,
                  pt: [number, number],
                  corner: Handle | "move",
                ) => (
                  <div
                    key={key}
                    className={corner === "move" ? "mask-move" : "mask-handle"}
                    data-mask={key}
                    style={{ left: pt[0] - 5, top: pt[1] - 5 }}
                    onPointerDown={(e) => onMaskHandleDown(e, corner)}
                    onPointerMove={onMaskHandleMove}
                    onPointerUp={onMaskHandleUp}
                  />
                );
                return (
                  <>
                    <svg className="sel-outline" aria-hidden="true">
                      <ellipse
                        className="mask-outline"
                        cx={(quad[0][0] + quad[2][0]) / 2}
                        cy={(quad[0][1] + quad[2][1]) / 2}
                        rx={Math.abs(quad[1][0] - quad[0][0]) / 2}
                        ry={Math.abs(quad[3][1] - quad[0][1]) / 2}
                      />
                    </svg>
                    {knob("move", centre, "move")}
                    {HANDLES.map((c, i) => knob(c, quad[HANDLE_CORNER[i]], c))}
                  </>
                );
              })()}
              {/* Gradient geometry, dragged where it is seen rather than
                  through sliders in the panel. */}
              {(() => {
                const kind = selectedKind;
                if (!selToScreen || !selLocal || kind === null) return null;
                if (typeof kind !== "object" || !("Vector" in kind))
                  return null;
                const g = kind.Vector.gradient;
                if (!g) return null;
                const [x0, y0, x1, y1] = selLocal;
                const at = (u: number, v: number) =>
                  selToScreen!(x0 + u * (x1 - x0), y0 + v * (y1 - y0));
                const knob = (
                  key: string,
                  p: [number, number],
                  part: "from" | "to" | "centre" | "radius" | number,
                  cls: string,
                ) => (
                  <div
                    key={key}
                    className={cls}
                    data-grad={key}
                    style={{ left: p[0] - 5, top: p[1] - 5 }}
                    onPointerDown={(e) => onGradHandleDown(e, part)}
                    onPointerMove={onGradHandleMove}
                    onPointerUp={onGradHandleUp}
                  />
                );
                if ("Linear" in g) {
                  const { from, to, stops } = g.Linear;
                  const [a, b] = [at(from[0], from[1]), at(to[0], to[1])];
                  return (
                    <>
                      <svg className="sel-outline" aria-hidden="true">
                        <line
                          className="grad-line"
                          x1={a[0]}
                          y1={a[1]}
                          x2={b[0]}
                          y2={b[1]}
                        />
                      </svg>
                      {knob("from", a, "from", "grad-handle")}
                      {knob("to", b, "to", "grad-handle")}
                      {stops
                        .slice(1, -1)
                        .map((st, i) =>
                          knob(
                            `stop${i + 1}`,
                            at(
                              from[0] + (to[0] - from[0]) * st.offset,
                              from[1] + (to[1] - from[1]) * st.offset,
                            ),
                            i + 1,
                            "grad-stop",
                          ),
                        )}
                    </>
                  );
                }
                const { center, radius } = g.Radial;
                const c = at(center[0], center[1]);
                const rim = at(center[0] + radius, center[1]);
                return (
                  <>
                    <svg className="sel-outline" aria-hidden="true">
                      <line
                        className="grad-line"
                        x1={c[0]}
                        y1={c[1]}
                        x2={rim[0]}
                        y2={rim[1]}
                      />
                    </svg>
                    {knob("centre", c, "centre", "grad-handle")}
                    {knob("radius", rim, "radius", "grad-handle")}
                  </>
                );
              })()}
              {grabbable &&
                HANDLES.map((c, i) => (
                  <div
                    key={c}
                    className={`handle ${c}`}
                    data-handle={c}
                    style={{
                      left: selQuad![HANDLE_CORNER[i]][0] - 5,
                      top: selQuad![HANDLE_CORNER[i]][1] - 5,
                    }}
                    onPointerDown={(e) => onHandlePointerDown(e, c)}
                    onPointerMove={onHandlePointerMove}
                    onPointerUp={onHandlePointerUp}
                  />
                ))}
            </>
          )}
        </main>
        <aside className="panel" aria-label="Layers">
          <div className="panel-head">
            <h2>Layers</h2>
            <select
              className="add-adjustment"
              value=""
              onChange={(e) => addAdjustment(e.target.value)}
              title="Add adjustment layer"
              aria-label="Add adjustment layer"
            >
              <option value="" disabled>
                +FX
              </option>
              <optgroup label="Over everything below">
                {Object.entries(ADJUSTMENT_PRESETS).map(([key, p]) => (
                  <option key={key} value={key}>
                    {p.name}
                  </option>
                ))}
              </optgroup>
              {selectedLayer &&
                selectedLayer.kind !== "adjustment" &&
                selectedLayer.kind !== "filter" && (
                  <optgroup label={`Only on ${selectedLayer.name}`}>
                    {Object.entries(ADJUSTMENT_PRESETS).map(([key, p]) => (
                      <option key={`only:${key}`} value={`only:${key}`}>
                        {p.name}
                      </option>
                    ))}
                  </optgroup>
                )}
            </select>
            <button
              onClick={() => reorderSelected(1)}
              disabled={
                !selectedLayer ||
                selectedLayer.index >= selectedLayer.sibling_count - 1
              }
              title="Raise layer"
              aria-label="Raise layer"
            >
              <Icon name="raise" size={16} />
            </button>
            <button
              onClick={() => reorderSelected(-1)}
              disabled={!selectedLayer || selectedLayer.index === 0}
              title="Lower layer"
              aria-label="Lower layer"
            >
              <Icon name="lower" size={16} />
            </button>
            <button
              onClick={groupSelection}
              disabled={selectionSet.length === 0}
              title="Group selected layers (ctrl-click to select several)"
              aria-label="Group selected layers (ctrl-click to select several)"
            >
              <Icon name="group" size={16} />
            </button>
            <button
              onClick={ungroupSelection}
              disabled={selectedLayer?.kind !== "group"}
              title="Ungroup selected group"
              aria-label="Ungroup selected group"
            >
              <Icon name="ungroup" size={16} />
            </button>
            <button
              onClick={clipSelection}
              disabled={!clippable}
              aria-pressed={selectedLayer?.clipped ?? false}
              title="Clip to the layer below (Ctrl+Alt+G)"
              aria-label="Clip to the layer below"
            >
              <Icon name="clip" size={16} />
            </button>
            <button
              onClick={duplicateSelected}
              disabled={selected === null}
              title="Duplicate layer (Ctrl+D)"
              aria-label="Duplicate layer"
            >
              <Icon name="duplicate" size={16} />
            </button>
            <button
              onClick={deleteSelected}
              disabled={selected === null}
              title="Delete selected layer (Del)"
              aria-label="Delete selected layer"
            >
              <Icon name="trash" size={16} />
            </button>
          </div>
          {selectionSet.length >= 2 && (
            <div className="align-bar" role="group" aria-label="Align layers">
              {ALIGN_BUTTONS.map(([mode, icon, label]) => (
                <button
                  key={mode}
                  onClick={() => alignSelection(mode)}
                  title={label}
                  aria-label={label}
                >
                  <Icon name={icon} size={16} />
                </button>
              ))}
            </div>
          )}
          {selectionSet.length >= 2 && (
            <div
              className="align-bar combine-bar"
              role="group"
              aria-label="Combine shapes"
            >
              {BOOLEAN_BUTTONS.map(([op, icon, label]) => (
                <button
                  key={op}
                  onClick={() => combineSelection(op)}
                  title={label}
                  aria-label={label}
                >
                  <Icon name={icon} size={16} />
                </button>
              ))}
            </div>
          )}
          {selectedLayer && (
            <div className="layer-props">
              {selBox && (
                <div className="geometry" aria-label="Position and size">
                  {(
                    [
                      ["x", "X", selBox[0]],
                      ["y", "Y", selBox[1]],
                      ["w", "W", selBox[2]],
                      ["h", "H", selBox[3]],
                    ] as const
                  ).map(([field, label, value]) => (
                    <label key={field}>
                      {label}
                      <input
                        type="number"
                        step={units === "px" ? "1" : "0.01"}
                        // Keyed by the value so a committed edit re-reads
                        // from the document rather than keeping a stale
                        // draft: the box moves for reasons other than this
                        // field (a drag, an undo, an align). Shown and
                        // read in the chosen units.
                        key={`${field}${units}${Math.round(value * 100)}`}
                        defaultValue={inUnits(value, units, docDpi)}
                        onKeyDown={(e) => {
                          e.stopPropagation();
                          if (e.key === "Enter") e.currentTarget.blur();
                        }}
                        onBlur={(e) => {
                          const v = Number(e.currentTarget.value);
                          if (Number.isFinite(v))
                            setGeometry(field, v / perPixel(units, docDpi));
                        }}
                        aria-label={
                          field === "w" || field === "h"
                            ? `${label} size`
                            : `${label} position`
                        }
                      />
                    </label>
                  ))}
                </div>
              )}
              <label>
                Opacity{" "}
                {Math.round((opacityDraft ?? selectedLayer.opacity) * 100)}%
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={opacityDraft ?? selectedLayer.opacity}
                  onChange={(e) => setOpacityDraft(Number(e.target.value))}
                  onPointerUp={commitOpacity}
                  onKeyUp={commitOpacity}
                  onBlur={commitOpacity}
                  aria-label="Layer opacity"
                />
              </label>
              <label>
                Blend
                <select
                  value={selectedLayer.blend}
                  onChange={(e) =>
                    setBlendOfSelection(e.target.value as BlendMode)
                  }
                  aria-label="Blend mode"
                >
                  {BLEND_MODES.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </select>
              </label>
              {selectedKind && (
                <KindProps
                  kind={selectedKind}
                  onEdit={setKind}
                  onGestureEnd={endGesture}
                  cmyk={cmyk}
                  fonts={fontNames}
                  shapes={layers
                    .filter(
                      (l) => l.kind === "vector" && l.id !== selectedLayer.id,
                    )
                    .map((l) => ({ id: l.id, name: l.name }))}
                  onAlong={(shape) => {
                    if (!session || !selectedLayer) return;
                    if (shape === null) {
                      if (
                        selectedKind &&
                        typeof selectedKind === "object" &&
                        "Text" in selectedKind
                      ) {
                        setKind(
                          { Text: { ...selectedKind.Text, along: null } },
                          false,
                        );
                      }
                      return;
                    }
                    try {
                      session.text_along(selectedLayer.id, shape);
                      refresh(session);
                    } catch (err) {
                      alert(`Text on path: ${err}`);
                    }
                  }}
                />
              )}
              <div className="effects">
                {selectedEffects.map((effect, at) => {
                  const kind = effectKind(effect);
                  const body = effectBody(effect) as Record<string, number>;
                  return (
                    <div className="effect" key={`${kind}${at}`}>
                      <div className="row effect-head">
                        <span>{EFFECT_LABELS[kind]}</span>
                        <input
                          type="color"
                          value={colorToHex(
                            (effect as never)[kind]["color"] as AuthoredColor,
                          )}
                          onChange={(e) =>
                            tuneEffect(at, {
                              color: cmyk
                                ? hexToCmykColor(e.target.value)
                                : hexColor(e.target.value),
                            })
                          }
                          aria-label={`${EFFECT_LABELS[kind]} colour`}
                        />
                        {/* Order matters: an outline under a drop shadow
                            is not the same picture as one over it. */}
                        <button
                          className="mask-button"
                          disabled={at === 0}
                          aria-label={`Move ${EFFECT_LABELS[kind]} down`}
                          onClick={() => setEffects(swapEffects(at, at - 1))}
                        >
                          <Icon name="lower" size={14} />
                        </button>
                        <button
                          className="mask-button"
                          disabled={at === selectedEffects.length - 1}
                          aria-label={`Move ${EFFECT_LABELS[kind]} up`}
                          onClick={() => setEffects(swapEffects(at, at + 1))}
                        >
                          <Icon name="raise" size={14} />
                        </button>
                        <button
                          className="mask-button"
                          aria-label={`Remove ${EFFECT_LABELS[kind]}`}
                          onClick={() =>
                            setEffects(
                              selectedEffects.filter((_, i) => i !== at),
                            )
                          }
                        >
                          Remove
                        </button>
                      </div>
                      {EFFECT_FIELDS[kind].map(
                        ([field, label, min, max, step]) => (
                          <label key={field}>
                            {label} {(body[field] ?? 0).toFixed(2)}
                            <input
                              type="range"
                              min={min}
                              max={max}
                              step={step}
                              value={body[field] ?? 0}
                              onChange={(e) =>
                                tuneEffect(
                                  at,
                                  { [field]: Number(e.target.value) },
                                  true,
                                )
                              }
                              onPointerUp={endGesture}
                              onKeyUp={endGesture}
                              onBlur={endGesture}
                              aria-label={`${EFFECT_LABELS[kind]} ${label.toLowerCase()}`}
                            />
                          </label>
                        ),
                      )}
                    </div>
                  );
                })}
                <label className="row">
                  Add effect
                  <select
                    value=""
                    onChange={(e) => {
                      const kind = e.target.value as EffectKind;
                      if (!kind) return;
                      e.target.value = "";
                      setEffects([
                        ...selectedEffects,
                        newEffect(
                          kind,
                          cmyk
                            ? hexToCmykColor("#000000")
                            : hexColor("#000000"),
                        ),
                      ]);
                    }}
                    aria-label="Add effect"
                  >
                    <option value="">Choose…</option>
                    {(Object.keys(EFFECT_LABELS) as EffectKind[]).map((k) => (
                      <option key={k} value={k}>
                        {EFFECT_LABELS[k]}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              {selectedMask === null ? (
                <div className="row mask-row">
                  <button
                    className="mask-button"
                    onClick={() => addMask("ellipse")}
                  >
                    Ellipse mask
                  </button>
                  <button
                    className="mask-button"
                    onClick={() => addMask("rect")}
                  >
                    Rect mask
                  </button>
                  {selectedKind &&
                    typeof selectedKind === "object" &&
                    "Vector" in selectedKind && (
                      <button
                        className="mask-button"
                        onClick={maskWithSelectedShape}
                      >
                        Mask below
                      </button>
                    )}
                </div>
              ) : (
                <label className="row">
                  <input
                    type="checkbox"
                    checked={selectedMask.invert}
                    onChange={(e) =>
                      run({
                        SetMask: {
                          id: selectedLayer.id,
                          mask: { ...selectedMask!, invert: e.target.checked },
                        },
                      })
                    }
                    aria-label="Invert mask"
                  />
                  Invert mask
                  <button
                    className="mask-button"
                    onClick={() =>
                      run({ SetMask: { id: selectedLayer.id, mask: null } })
                    }
                  >
                    Remove
                  </button>
                </label>
              )}
            </div>
          )}
          <ul ref={layerListRef}>
            {layers.map((l) => (
              <li
                key={l.id}
                data-id={l.id}
                data-kind={l.kind}
                className={[
                  l.id === selected
                    ? "selected"
                    : multiSel.includes(l.id)
                      ? "multi"
                      : "",
                  layerDrag?.over === l.id && layerDrag.id !== l.id
                    ? `drop-${layerDrag.where}`
                    : "",
                  layerDrag?.id === l.id ? "dragging" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
                style={{ paddingLeft: `${l.depth * 14 + 2}px` }}
                onPointerDown={(e) => onRowPointerDown(e, l.id)}
                onClick={(e) => {
                  if (Date.now() - layerDragDone.current < 300) return;
                  if (e.ctrlKey || e.metaKey) {
                    // Toggle in the multi-selection; primary stays put.
                    if (l.id === selected) return;
                    setMultiSel((prev) =>
                      prev.includes(l.id)
                        ? prev.filter((id) => id !== l.id)
                        : [...prev, l.id],
                    );
                  } else {
                    setSelected(l.id);
                    setMultiSel([]);
                  }
                }}
              >
                <button
                  className="visibility"
                  onClick={(e) => {
                    e.stopPropagation();
                    run({ SetVisible: { id: l.id, visible: !l.visible } });
                  }}
                  aria-pressed={l.visible}
                  title={l.visible ? "Hide layer" : "Show layer"}
                >
                  <Icon name={l.visible ? "eye" : "eyeOff"} size={15} />
                </button>
                <button
                  className="lock-toggle"
                  onClick={(e) => {
                    e.stopPropagation();
                    run({ SetLocked: { id: l.id, locked: !l.locked } });
                  }}
                  aria-pressed={l.locked}
                  title={l.locked ? "Unlock layer" : "Lock layer"}
                  aria-label={l.locked ? "Unlock layer" : "Lock layer"}
                >
                  <Icon name={l.locked ? "lock" : "unlock"} size={14} />
                </button>
                {/* A layer confined to the one below it says so with a
                    hook pointing down at it, the way the panel of every
                    editor that has clipping does. */}
                {l.clipped && (
                  <span
                    className="clip-mark"
                    title="Clipped to the layer below"
                    aria-label="Clipped to the layer below"
                  >
                    <Icon name="clip" size={12} />
                  </span>
                )}
                {/* What the layer holds, when it holds anything: a
                    small picture of it on its own. An adjustment or a
                    filter is a change to what is under it and has none,
                    so those keep the glyph that says which they are. */}
                <span className="layer-kind-icon" title={l.kind}>
                  {thumbs[l.id] ? (
                    <img className="layer-thumb" src={thumbs[l.id]} alt="" />
                  ) : (
                    <Icon name={KIND_ICONS[l.kind] ?? "rect"} size={15} />
                  )}
                </span>
                {renaming?.id === l.id ? (
                  <input
                    className="rename"
                    value={renaming.value}
                    autoFocus
                    onChange={(e) =>
                      setRenaming({ id: l.id, value: e.target.value })
                    }
                    onBlur={commitRename}
                    onKeyDown={(e) => {
                      e.stopPropagation();
                      if (e.key === "Enter") commitRename();
                      if (e.key === "Escape") setRenaming(null);
                    }}
                    aria-label="Layer name"
                  />
                ) : (
                  <span
                    className={l.visible ? "layer-name" : "layer-name muted"}
                    onDoubleClick={() =>
                      setRenaming({ id: l.id, value: l.name })
                    }
                  >
                    {l.name}
                  </span>
                )}
                {l.has_mask && (
                  <span
                    className="kind"
                    title="What this layer's mask lets through"
                  >
                    {maskThumbs[l.id] ? (
                      <img
                        className="layer-thumb mask-thumb"
                        src={maskThumbs[l.id]}
                        alt=""
                      />
                    ) : (
                      <Icon name="mask" size={14} />
                    )}
                  </span>
                )}
                {l.has_effects && (
                  <span className="kind" title="This layer has effects">
                    <Icon name="shadow" size={14} />
                  </span>
                )}
              </li>
            ))}
            {layers.length === 0 && (
              <li className="muted empty">Drag on the canvas to add shapes</li>
            )}
          </ul>
          {(history.past.length > 0 || history.future.length > 0) && (
            <div className="history" aria-label="History">
              <h2>History</h2>
              <ol>
                {history.past.map((label, i) => (
                  <li key={`p${i}`}>
                    <button
                      className={
                        i === history.past.length - 1 ? "current" : undefined
                      }
                      onClick={() => jumpHistory(i + 1 - history.past.length)}
                    >
                      {label}
                    </button>
                  </li>
                ))}
                {history.future.map((label, i) => (
                  <li key={`f${i}`} className="future">
                    <button onClick={() => jumpHistory(i + 1)}>{label}</button>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}

function topLevelCount(layers: LayerInfo[]): number {
  return layers.filter((l) => l.depth === 0).length;
}

/** What this editor answers to, in the order someone would meet it.
 * Gestures sit beside keys because most of these are gestures. */
const KEY_HELP: [string, [string, string][]][] = [
  [
    "Tools",
    [
      ["V, M", "Move"],
      ["F", "Frame (an artboard: a page within the page)"],
      ["R, E", "Rectangle, ellipse"],
      ["P, B", "Pen, brush"],
      ["N", "Paint (a brush that lays pixels)"],
      ["S", "Clone (paint with what is already there)"],
      ["Alt-click (clone)", "Set the place to clone from"],
      [
        "Heal (clone)",
        "Lay the texture down in the colour of the place it lands",
      ],
      ["[  ]", "Thinner, thicker brush"],
      ["Alt-click (brush)", "Take the colour under the brush"],
      ["Shift-click (brush)", "Paint a straight line on from the last stroke"],
      [
        "Erase + drag",
        "On a paint layer, rubs out its paint; on any other, takes a piece out of it (and the brush puts it back)",
      ],
      ["T", "Text"],
      ["C", "Crop"],
      ["I", "Eyedropper — take the colour under the cursor"],
    ],
  ],
  [
    "Picking",
    [
      ["Drag on bare canvas", "A band picks everything it touches"],
      ["Shift-drag a band", "Adds to what is picked"],
      ["Ctrl-click a layer", "Adds one layer to the selection"],
      ["Double-click text", "Type into it on the canvas"],
      ["Escape", "Let go of the selection"],
    ],
  ],
  [
    "Moving",
    [
      ["Drag", "Move; the edges snap to the page and to other layers"],
      ["Ctrl while dragging", "Ignore the snapping"],
      ["Alt-drag", "Leave the original and carry a copy"],
      ["Arrow keys", "Nudge a pixel; shift for ten"],
      ["Drag a layer row", "Reorder, or drop into a group"],
    ],
  ],
  [
    "The view",
    [
      ["Wheel", "Zoom about the cursor"],
      ["Space-drag, middle-drag", "Pan"],
      ["Drag out of a ruler", "Place a guide; drop it back to remove it"],
    ],
  ],
  [
    "The document",
    [
      ["Ctrl+Z, Ctrl+Shift+Z", "Undo, redo"],
      ["Ctrl+C, Ctrl+X, Ctrl+V", "Copy, cut, paste"],
      ["Ctrl+D", "Duplicate"],
      ["Ctrl+Alt+C / V", "Copy, paste a layer's look"],
      ["Ctrl+Alt+G", "Clip to the layer below"],
      ["Double-click a path", "Put an anchor on its outline"],
      ["Alt-click an anchor", "Take it off"],
      ["Ctrl+A", "Select all"],
      ["Delete", "Delete the picked layers"],
      ["?", "This sheet"],
    ],
  ],
];

/** The sheet of keys and gestures. */
function KeysDialog({ onClose }: { onClose: () => void }) {
  return (
    <div className="modal-scrim" onPointerDown={onClose}>
      <div
        className="modal keys"
        role="dialog"
        aria-label="Keys and gestures"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <h2>Keys and gestures</h2>
        <div className="keys-columns">
          {KEY_HELP.map(([group, rows]) => (
            <section key={group}>
              <h3>{group}</h3>
              <dl>
                {rows.map(([key, what]) => (
                  <div key={key}>
                    <dt>{key}</dt>
                    <dd>{what}</dd>
                  </div>
                ))}
              </dl>
            </section>
          ))}
        </div>
        <div className="modal-actions">
          <button className="mask-button primary" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/** New-document dialog: presets for the sizes people actually start from,
 * and the two fields underneath for everything else. Colour mode is chosen
 * here because it decides how every fill in the document is authored, and
 * changing it afterwards would mean reinterpreting them all. */
function NewDocDialog({
  onCreate,
  onCancel,
}: {
  onCreate: (w: number, h: number, cmyk: boolean, dpi: number) => void;
  onCancel: () => void;
}) {
  const [w, setW] = useState(DOC_WIDTH);
  const [h, setH] = useState(DOC_HEIGHT);
  const [dpi, setDpi] = useState(72);
  const [mode, setMode] = useState("rgb");

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onCreate(w, h, mode === "cmyk", dpi);
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [w, h, mode, dpi, onCreate, onCancel]);

  const size = (label: string, value: number, set: (v: number) => void) => (
    <label className="row">
      {label}
      <input
        type="number"
        min={1}
        max={8192}
        value={value}
        onChange={(e) =>
          set(Math.max(1, Math.min(8192, Math.round(Number(e.target.value)))))
        }
        aria-label={label}
      />
    </label>
  );

  return (
    <div className="modal-scrim" onPointerDown={onCancel}>
      <div
        className="modal"
        role="dialog"
        aria-label="New document"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <h2>New document</h2>
        <div className="preset-list">
          {DOC_PRESETS.map(([name, pw, ph, pdpi]) => (
            <button
              key={name}
              className={
                w === pw && h === ph && dpi === pdpi
                  ? "preset active"
                  : "preset"
              }
              onClick={() => {
                setW(pw);
                setH(ph);
                setDpi(pdpi);
              }}
            >
              {name}
            </button>
          ))}
        </div>
        {size("Width", w, setW)}
        {size("Height", h, setH)}
        <label className="row">
          Resolution
          <input
            type="number"
            min={1}
            max={2400}
            value={dpi}
            onChange={(e) =>
              setDpi(
                Math.max(1, Math.min(2400, Math.round(Number(e.target.value)))),
              )
            }
            aria-label="Resolution"
          />
          <span className="hint">
            dpi · {inUnits(w, "mm", dpi)} × {inUnits(h, "mm", dpi)} mm
          </span>
        </label>
        <label className="row">
          Colour
          <select
            value={mode}
            onChange={(e) => setMode(e.target.value)}
            aria-label="Colour mode"
          >
            <option value="rgb">RGB</option>
            <option value="cmyk">CMYK</option>
          </select>
        </label>
        <div className="modal-actions">
          <button className="mask-button" onClick={onCancel}>
            Cancel
          </button>
          <button
            className="mask-button primary"
            onClick={() => onCreate(w, h, mode === "cmyk", dpi)}
          >
            Create
          </button>
        </div>
      </div>
    </div>
  );
}

/** One top-level menu: a label in the bar and the popup it owns.
 *
 * Menu bars have a convention worth honouring — once one menu is open,
 * moving across the bar switches to its neighbour without another click —
 * so opening is a click but switching is a hover, which is what `onHover`
 * is for. Escape and any click outside close it. */
function MenuButton({
  label,
  open,
  onOpen,
  onHover,
  onClose,
  children,
}: {
  label: string;
  open: boolean;
  onOpen: () => void;
  onHover: () => void;
  onClose: () => void;
  children: React.ReactNode;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", away);
      document.removeEventListener("keydown", key);
    };
  }, [open, onClose]);

  return (
    <div className="menu" ref={ref}>
      <button
        className={open ? "menu-label open" : "menu-label"}
        onClick={onOpen}
        onPointerEnter={onHover}
        aria-haspopup="menu"
        aria-expanded={open}
      >
        {label}
      </button>
      {open && (
        <div className="menu-pop" role="menu" onClick={onClose}>
          {children}
        </div>
      )}
    </div>
  );
}

/** A row in a menu: glyph, label, and an optional hint on the right for a
 * shortcut or the mode an action will use. */
function MenuItem({
  icon,
  onClick,
  hint,
  children,
}: {
  icon: IconName;
  onClick: () => void;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <button className="menu-item" role="menuitem" onClick={onClick}>
      <Icon name={icon} size={16} />
      <span className="menu-item-label">{children}</span>
      {hint && <span className="menu-item-hint">{hint}</span>}
    </button>
  );
}

/** The curve a set of points implies, sampled for drawing: the same
 * monotone cubic the renderer tabulates (Fritsch–Carlson), sorted, flat
 * past the outer points, identity for fewer than two. */
function curveSamples(points: [number, number][], n = 64): [number, number][] {
  const pts = [...points]
    .map(
      ([x, y]) =>
        [Math.min(1, Math.max(0, x)), Math.min(1, Math.max(0, y))] as [
          number,
          number,
        ],
    )
    .sort((a, b) => a[0] - b[0])
    .filter(
      (p, i, all) =>
        i === all.length - 1 || Math.abs(all[i + 1][0] - p[0]) >= 1e-6,
    );
  const k = pts.length;
  const at = (x: number): number => {
    if (k < 2) return x;
    if (x <= pts[0][0]) return pts[0][1];
    if (x >= pts[k - 1][0]) return pts[k - 1][1];
    const h = pts.slice(0, -1).map((p, i) => pts[i + 1][0] - p[0]);
    const d = pts.slice(0, -1).map((p, i) => (pts[i + 1][1] - p[1]) / h[i]);
    const m = pts.map((_, i) =>
      i === 0
        ? d[0]
        : i === k - 1
          ? d[k - 2]
          : d[i - 1] * d[i] > 0
            ? (d[i - 1] + d[i]) / 2
            : 0,
    );
    for (let i = 0; i < k - 1; i++) {
      if (d[i] === 0) {
        m[i] = m[i + 1] = 0;
        continue;
      }
      const [a, b] = [m[i] / d[i], m[i + 1] / d[i]];
      const r = a * a + b * b;
      if (r > 9) {
        const t = 3 / Math.sqrt(r);
        m[i] = t * a * d[i];
        m[i + 1] = t * b * d[i];
      }
    }
    let s = 0;
    while (pts[s + 1][0] < x) s++;
    const t = (x - pts[s][0]) / h[s];
    const [t2, t3] = [t * t, t * t * t];
    const y =
      (2 * t3 - 3 * t2 + 1) * pts[s][1] +
      (t3 - 2 * t2 + t) * h[s] * m[s] +
      (-2 * t3 + 3 * t2) * pts[s + 1][1] +
      (t3 - t2) * h[s] * m[s + 1];
    return Math.min(1, Math.max(0, y));
  };
  return Array.from({ length: n + 1 }, (_, i) => [i / n, at(i / n)]);
}

/** A tone curve to draw on: press anywhere to add a point and drag it in
 * the same gesture, drag a point to move it (it stays between its
 * neighbours), double-click one to take it away. The document previews
 * while the pointer is down and records one entry when it lifts. */
function CurveEditor({
  points,
  onEdit,
  onGestureEnd,
}: {
  points: [number, number][];
  onEdit: (points: [number, number][], gesture: boolean) => void;
  onGestureEnd: () => void;
}) {
  const SIZE = 160;
  const svgRef = useRef<SVGSVGElement>(null);
  /** The gesture's own copy of the points and which one it holds, so a
   * move that lands before React has re-rendered the props still edits
   * the right point. */
  const dragging = useRef<{
    index: number;
    points: [number, number][];
    previewing: boolean;
  } | null>(null);
  const clamp01 = (v: number) => Math.min(1, Math.max(0, v));
  const local = (
    e: React.PointerEvent | React.MouseEvent,
  ): [number, number] => {
    const r = svgRef.current!.getBoundingClientRect();
    return [
      clamp01((e.clientX - r.left) / r.width),
      clamp01(1 - (e.clientY - r.top) / r.height),
    ];
  };
  const nearest = (p: [number, number]) => {
    let best = -1;
    let dist = 0.06;
    points.forEach(([x, y], i) => {
      const d = Math.hypot(x - p[0], y - p[1]);
      if (d < dist) {
        best = i;
        dist = d;
      }
    });
    return best;
  };
  const sorted = (pts: [number, number][]) =>
    [...pts].sort((a, b) => a[0] - b[0]);
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const p = local(e);
    let index = nearest(p);
    let next = sorted(points);
    if (index < 0) {
      next = sorted([...next, p]);
      index = next.findIndex((q) => q === p);
    } else {
      index = next.findIndex((q) => q === points[index]);
    }
    dragging.current = {
      index,
      points: next,
      previewing: index < 0 || next.length > points.length,
    };
    svgRef.current!.setPointerCapture(e.pointerId);
    // A new point is previewed at once; pressing an existing one previews
    // nothing until it moves, so a click on a point is not an edit.
    if (dragging.current.previewing) onEdit(next, true);
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const drag = dragging.current;
    if (!drag) return;
    drag.previewing = true;
    const p = local(e);
    const { index: i } = drag;
    const next = [...drag.points];
    const lo = i > 0 ? next[i - 1][0] + 0.005 : 0;
    const hi = i < next.length - 1 ? next[i + 1][0] - 0.005 : 1;
    next[i] = [Math.min(hi, Math.max(lo, p[0])), p[1]];
    drag.points = next;
    onEdit(next, true);
  };
  const onPointerUp = () => {
    const drag = dragging.current;
    if (!drag) return;
    dragging.current = null;
    if (drag.previewing) onGestureEnd();
  };
  const onDoubleClick = (e: React.MouseEvent) => {
    const i = nearest(local(e));
    if (i < 0 || points.length <= 2) return;
    onEdit(
      points.filter((_, j) => j !== i),
      false,
    );
  };
  const px = (v: number) => v * SIZE;
  const py = (v: number) => (1 - v) * SIZE;
  const line = curveSamples(points)
    .map(
      ([x, y], i) =>
        `${i === 0 ? "M" : "L"}${px(x).toFixed(1)},${py(y).toFixed(1)}`,
    )
    .join(" ");
  return (
    <div className="curve-row">
      <svg
        ref={svgRef}
        className="curve-editor"
        viewBox={`0 0 ${SIZE} ${SIZE}`}
        role="img"
        aria-label="Tone curve"
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onDoubleClick={onDoubleClick}
      >
        {[0.25, 0.5, 0.75].map((v) => (
          <g key={v} className="grid">
            <line x1={px(v)} y1={0} x2={px(v)} y2={SIZE} />
            <line x1={0} y1={py(v)} x2={SIZE} y2={py(v)} />
          </g>
        ))}
        <line className="grid" x1={0} y1={SIZE} x2={SIZE} y2={0} />
        <path className="line" d={line} />
        {points.map(([x, y], i) => (
          <circle key={i} className="pt" cx={px(x)} cy={py(y)} r={4} />
        ))}
      </svg>
      <button
        type="button"
        onClick={() =>
          onEdit(
            [
              [0, 0],
              [1, 1],
            ],
            false,
          )
        }
        title="Back to the diagonal"
      >
        Reset curve
      </button>
    </div>
  );
}

interface KindPropsProps {
  kind: NodeKind;
  /** gesture=true routes through preview (live, uncommitted). */
  onEdit: (kind: NodeKind, gesture: boolean) => void;
  onGestureEnd: () => void;
  /** Document colour mode, so new colours are authored as ink in a CMYK
   * document exactly like the shape tools do. */
  cmyk: boolean;
  /** Faces a text block may be set in, bundled one first. */
  fonts: string[];
  /** Shape layers a text block could be set along, and what to do when
   * one is picked (null takes the text off its path). */
  shapes: { id: number; name: string }[];
  onAlong: (shape: number | null) => void;
}

/** Parameter editors for the selected node's kind — the panel that makes
 * every layer's settings revisitable (the non-destructive contract). */
function KindProps({
  kind,
  onEdit,
  onGestureEnd,
  cmyk,
  fonts,
  shapes,
  onAlong,
}: KindPropsProps) {
  if (typeof kind !== "object") return null;

  const slider = (
    label: string,
    value: number,
    min: number,
    max: number,
    step: number,
    set: (v: number) => NodeKind,
  ) => (
    <label key={label}>
      {label} {value.toFixed(2)}
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onEdit(set(Number(e.target.value)), true)}
        onPointerUp={onGestureEnd}
        onKeyUp={onGestureEnd}
        onBlur={onGestureEnd}
        aria-label={label}
      />
    </label>
  );

  if ("Adjustment" in kind) {
    const adj = kind.Adjustment;
    const wrap = (a: Adjustment): NodeKind => ({ Adjustment: a });
    if ("Exposure" in adj) {
      return slider("Stops", adj.Exposure.stops, -3, 3, 0.05, (v) =>
        wrap({ Exposure: { stops: v } }),
      );
    }
    if ("BrightnessContrast" in adj) {
      const p = adj.BrightnessContrast;
      return (
        <>
          {slider("Brightness", p.brightness, -1, 1, 0.01, (v) =>
            wrap({ BrightnessContrast: { ...p, brightness: v } }),
          )}
          {slider("Contrast", p.contrast, -1, 1, 0.01, (v) =>
            wrap({ BrightnessContrast: { ...p, contrast: v } }),
          )}
        </>
      );
    }
    if ("HueSaturation" in adj) {
      const p = adj.HueSaturation;
      return (
        <>
          {slider("Hue", p.hue_degrees, -180, 180, 1, (v) =>
            wrap({ HueSaturation: { ...p, hue_degrees: v } }),
          )}
          {slider("Saturation", p.saturation, -1, 1, 0.01, (v) =>
            wrap({ HueSaturation: { ...p, saturation: v } }),
          )}
          {slider("Lightness", p.lightness, -1, 1, 0.01, (v) =>
            wrap({ HueSaturation: { ...p, lightness: v } }),
          )}
        </>
      );
    }
    if ("WhiteBalance" in adj) {
      const p = adj.WhiteBalance;
      return (
        <>
          {slider("Temperature", p.temperature, -1, 1, 0.01, (v) =>
            wrap({ WhiteBalance: { ...p, temperature: v } }),
          )}
          {slider("Tint", p.tint, -1, 1, 0.01, (v) =>
            wrap({ WhiteBalance: { ...p, tint: v } }),
          )}
        </>
      );
    }
    if ("Vibrance" in adj) {
      return slider("Vibrance", adj.Vibrance.amount, -1, 1, 0.01, (v) =>
        wrap({ Vibrance: { amount: v } }),
      );
    }
    if ("Curves" in adj) {
      return (
        <CurveEditor
          points={adj.Curves.points}
          onEdit={(points, gesture) =>
            onEdit(wrap({ Curves: { points } }), gesture)
          }
          onGestureEnd={onGestureEnd}
        />
      );
    }
    if ("Levels" in adj) {
      const p = adj.Levels;
      const set = (patch: Partial<typeof p>) =>
        wrap({ Levels: { ...p, ...patch } });
      return (
        <>
          {slider("Input black", p.in_black, 0, 1, 0.01, (v) =>
            set({ in_black: v }),
          )}
          {slider("Input white", p.in_white, 0, 1, 0.01, (v) =>
            set({ in_white: v }),
          )}
          {slider("Gamma", p.gamma, 0.2, 3, 0.02, (v) => set({ gamma: v }))}
          {slider("Output black", p.out_black, 0, 1, 0.01, (v) =>
            set({ out_black: v }),
          )}
          {slider("Output white", p.out_white, 0, 1, 0.01, (v) =>
            set({ out_white: v }),
          )}
        </>
      );
    }
    return null;
  }

  if ("Filter" in kind) {
    const filter = kind.Filter;
    if ("GaussianBlur" in filter) {
      return slider(
        "Blur sigma",
        filter.GaussianBlur.sigma,
        0,
        50,
        0.5,
        (v) => ({
          Filter: { GaussianBlur: { sigma: v } },
        }),
      );
    }
    if ("Sharpen" in filter) {
      const p = filter.Sharpen;
      return (
        <>
          {slider("Sharpen sigma", p.sigma, 0.5, 20, 0.5, (v) => ({
            Filter: { Sharpen: { ...p, sigma: v } },
          }))}
          {slider("Amount", p.amount, 0, 3, 0.05, (v) => ({
            Filter: { Sharpen: { ...p, amount: v } },
          }))}
        </>
      );
    }
    return null;
  }

  if ("Text" in kind) {
    const t = kind.Text;
    return (
      <>
        <label>
          Text
          <textarea
            value={t.text}
            rows={2}
            onChange={(e) =>
              onEdit({ Text: { ...t, text: e.target.value } }, true)
            }
            onBlur={onGestureEnd}
            aria-label="Text content"
          />
        </label>
        {slider("Size", t.size, 8, 200, 1, (v) => ({
          Text: { ...t, size: v },
        }))}
        <div className="align-bar" role="group" aria-label="Text alignment">
          {(
            [
              ["Left", "alignLeft", "Align text left"],
              ["Center", "alignCenterH", "Centre text"],
              ["Right", "alignRight", "Align text right"],
            ] as const
          ).map(([mode, icon, title]) => (
            <button
              key={mode}
              className={(t.align ?? "Left") === mode ? "active" : undefined}
              title={title}
              aria-label={title}
              onClick={() => onEdit({ Text: { ...t, align: mode } }, false)}
            >
              <Icon name={icon} />
            </button>
          ))}
        </div>
        {slider("Line height", t.line_height ?? 1, 0.5, 3, 0.05, (v) => ({
          Text: { ...t, line_height: v },
        }))}
        {slider(
          "Letter spacing",
          t.letter_spacing ?? 0,
          -0.1,
          0.5,
          0.01,
          (v) => ({
            Text: { ...t, letter_spacing: v },
          }),
        )}
        {/* A row of its own rather than one label around both controls:
            a label labels one control, and a toggle sharing it would be
            what the label points at. */}
        <div className="row">
          <label htmlFor="text-font">Font</label>
          {(() => {
            // A face and its "… Bold" twin, when the registry has both:
            // the toggle just swaps the name, which is all bold is here.
            const current = t.font || fonts[0];
            const base = current.replace(/ Bold$/, "");
            const heavy = `${base} Bold`;
            const isBold = current.endsWith(" Bold");
            const available = fonts.includes(heavy) && fonts.includes(base);
            return (
              <button
                className={isBold ? "active" : undefined}
                disabled={!available}
                title={available ? "Bold" : "No bold face for this font"}
                aria-label="Bold"
                aria-pressed={isBold}
                onClick={() =>
                  onEdit({ Text: { ...t, font: isBold ? base : heavy } }, false)
                }
              >
                <strong>B</strong>
              </button>
            );
          })()}
          {/* Italic always works: the face's oblique twin when there is
              one, a lean the rasterizer synthesizes when there is not. */}
          <button
            className={t.italic ? "active" : undefined}
            title="Italic"
            aria-label="Italic"
            aria-pressed={!!t.italic}
            onClick={() => onEdit({ Text: { ...t, italic: !t.italic } }, false)}
          >
            <em>I</em>
          </button>
          <button
            className={t.underline ? "active" : undefined}
            title="Underline"
            aria-label="Underline"
            aria-pressed={!!t.underline}
            onClick={() =>
              onEdit({ Text: { ...t, underline: !t.underline } }, false)
            }
          >
            <u>U</u>
          </button>
          <button
            className={t.strike ? "active" : undefined}
            title="Strike-through"
            aria-label="Strike-through"
            aria-pressed={!!t.strike}
            onClick={() => onEdit({ Text: { ...t, strike: !t.strike } }, false)}
          >
            <s>S</s>
          </button>
          <select
            id="text-font"
            value={t.font || fonts[0]}
            onChange={(e) =>
              onEdit({ Text: { ...t, font: e.target.value } }, false)
            }
            aria-label="Font"
          >
            {fonts.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </div>
        {/* A guide to run along: picking a shape copies its outline into
            the block, so the block stands alone afterwards. */}
        <div className="row">
          <label htmlFor="text-along">Along</label>
          <select
            id="text-along"
            aria-label="Along"
            value={t.along ? "on" : ""}
            onChange={(e) =>
              onAlong(e.target.value === "" ? null : Number(e.target.value))
            }
          >
            <option value="">None</option>
            {t.along && <option value="on">This path</option>}
            {shapes.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </div>
        {t.along &&
          slider("Path offset", t.along_offset, 0, 2000, 1, (v) => ({
            Text: { ...t, along_offset: v },
          }))}
        {/* Zero is a block that fits its text; anything else wraps to it. */}
        {slider("Wrap width", t.width ?? 0, 0, 2000, 10, (v) => ({
          Text: { ...t, width: v },
        }))}
        <label className="row">
          Color
          <input
            type="color"
            value={colorToHex(t.fill)}
            onChange={(e) =>
              onEdit({ Text: { ...t, fill: hexColor(e.target.value) } }, true)
            }
            onBlur={onGestureEnd}
            aria-label="Text color"
          />
        </label>
      </>
    );
  }

  if ("Vector" in kind) {
    const v = kind.Vector;
    const patch = (changes: Partial<typeof v>): NodeKind => ({
      Vector: { ...v, ...changes },
    });

    const authored = (hex: string) =>
      cmyk ? hexToCmykColor(hex) : hexColor(hex);
    const grad = v.gradient;
    const gradStops = grad
      ? "Linear" in grad
        ? grad.Linear.stops
        : grad.Radial.stops
      : [];
    const fillKind = !grad ? "solid" : "Linear" in grad ? "linear" : "radial";
    // A gradient's ends are the shape's own fill and white, so switching
    // fill type starts from what is already on screen.
    const startingStops = (): GradientStop[] => [
      { offset: 0, color: v.fill ?? authored("#6c8cff") },
      { offset: 1, color: authored("#ffffff") },
    ];
    /** Endpoints of a linear ramp at `deg`, across the shape's box. */
    const endpoints = (
      deg: number,
    ): { from: [number, number]; to: [number, number] } => {
      const rad = (deg * Math.PI) / 180;
      const [dx, dy] = [Math.cos(rad) / 2, Math.sin(rad) / 2];
      return { from: [0.5 - dx, 0.5 - dy], to: [0.5 + dx, 0.5 + dy] };
    };
    const angleOf = (g: typeof grad): number => {
      if (!g || !("Linear" in g)) return 0;
      const { from, to } = g.Linear;
      const deg =
        (Math.atan2(to[1] - from[1], to[0] - from[0]) * 180) / Math.PI;
      return Math.round(deg < 0 ? deg + 360 : deg);
    };
    const setFillKind = (next: string) => {
      const stops = gradStops.length ? gradStops : startingStops();
      if (next === "solid") return patch({ gradient: null });
      if (next === "linear")
        return patch({ gradient: { Linear: { ...endpoints(0), stops } } });
      return patch({
        gradient: { Radial: { center: [0.5, 0.5], radius: 0.5, stops } },
      });
    };
    /** Put a new stop list back into whichever gradient kind is set. */
    const withStops = (stops: GradientStop[]): NodeKind => {
      if (grad && "Linear" in grad)
        return patch({ gradient: { Linear: { ...grad.Linear, stops } } });
      if (grad && "Radial" in grad)
        return patch({ gradient: { Radial: { ...grad.Radial, stops } } });
      return patch({});
    };
    const setStop = (i: number, changes: Partial<GradientStop>): NodeKind =>
      withStops(gradStops.map((s, j) => (j === i ? { ...s, ...changes } : s)));
    /** Insert a stop in the widest gap, coloured by what the ramp already
     * shows there, so adding one changes nothing until it is moved. */
    const addStop = (): NodeKind => {
      let at = 0;
      for (let i = 1; i < gradStops.length; i++) {
        if (
          gradStops[i].offset - gradStops[i - 1].offset >
          gradStops[at + 1].offset - gradStops[at].offset
        )
          at = i - 1;
      }
      const [a, b] = [gradStops[at], gradStops[at + 1]];
      const stop: GradientStop = {
        offset: (a.offset + b.offset) / 2,
        color: mixAuthored(a.color, b.color, 0.5),
      };
      return withStops([
        ...gradStops.slice(0, at + 1),
        stop,
        ...gradStops.slice(at + 1),
      ]);
    };
    const removeStop = (i: number): NodeKind =>
      withStops(gradStops.filter((_, j) => j !== i));
    // A preview of the ramp itself, so the stop list is readable as a whole.
    // sRGB interpolation here against the engine's linear-light blend: this
    // is a locator strip, not a proof of the render.
    const rampCss = `linear-gradient(90deg, ${gradStops
      .map((s) => `${colorToHex(s.color)} ${Math.round(s.offset * 100)}%`)
      .join(", ")})`;

    return (
      <>
        <label className="row">
          Fill type
          <select
            value={fillKind}
            onChange={(e) => onEdit(setFillKind(e.target.value), false)}
            aria-label="Fill type"
          >
            <option value="solid">Solid</option>
            <option value="linear">Linear gradient</option>
            <option value="radial">Radial gradient</option>
          </select>
        </label>
        {grad && (
          <>
            <div
              className="ramp-preview"
              style={{ background: rampCss }}
              aria-hidden="true"
            />
            {gradStops.map((stop, i) => (
              <div className="stop-row" key={i}>
                <input
                  type="color"
                  value={colorToHex(stop.color)}
                  onChange={(e) =>
                    onEdit(
                      setStop(i, { color: authored(e.target.value) }),
                      true,
                    )
                  }
                  onBlur={onGestureEnd}
                  aria-label={`Gradient stop ${i + 1}`}
                />
                <input
                  type="range"
                  min={0}
                  max={1}
                  step={0.01}
                  value={stop.offset}
                  onChange={(e) =>
                    onEdit(setStop(i, { offset: Number(e.target.value) }), true)
                  }
                  onPointerUp={onGestureEnd}
                  onKeyUp={onGestureEnd}
                  onBlur={onGestureEnd}
                  aria-label={`Gradient stop ${i + 1} position`}
                />
                <button
                  className="stop-remove"
                  disabled={gradStops.length <= 2}
                  onClick={() => onEdit(removeStop(i), false)}
                  title={
                    gradStops.length <= 2
                      ? "A gradient needs at least two stops"
                      : `Remove stop ${i + 1}`
                  }
                  aria-label={`Remove gradient stop ${i + 1}`}
                >
                  <Icon name="trash" size={14} />
                </button>
              </div>
            ))}
            <button
              className="mask-button"
              onClick={() => onEdit(addStop(), false)}
            >
              Add stop
            </button>
            {"Linear" in grad
              ? slider("Gradient angle", angleOf(grad), 0, 359, 1, (deg) =>
                  patch({
                    gradient: {
                      Linear: { ...endpoints(deg), stops: gradStops },
                    },
                  }),
                )
              : slider(
                  "Gradient radius",
                  grad.Radial.radius,
                  0.05,
                  1.5,
                  0.05,
                  (r) =>
                    patch({
                      gradient: { Radial: { ...grad.Radial, radius: r } },
                    }),
                )}
          </>
        )}
        <label className="row">
          <input
            type="checkbox"
            checked={v.fill !== null}
            onChange={(e) =>
              onEdit(
                patch({ fill: e.target.checked ? hexColor("#6c8cff") : null }),
                false,
              )
            }
            aria-label="Fill enabled"
          />
          Fill
          {!grad && v.fill && !("Cmyk" in v.fill) && (
            <input
              type="color"
              value={colorToHex(v.fill)}
              onChange={(e) =>
                onEdit(patch({ fill: hexColor(e.target.value) }), true)
              }
              onBlur={onGestureEnd}
              aria-label="Fill color"
            />
          )}
        </label>
        {!grad && v.fill && "Cmyk" in v.fill && (
          <>
            {(["c", "m", "y", "k"] as const).map((ch) => {
              const ink = (v.fill as { Cmyk: Record<string, number> }).Cmyk;
              return slider(
                `${ch.toUpperCase()} ink`,
                ink[ch],
                0,
                1,
                0.01,
                (val) =>
                  patch({
                    fill: { Cmyk: { ...ink, [ch]: val } } as typeof v.fill,
                  }),
              );
            })}
          </>
        )}
        <label className="row">
          <input
            type="checkbox"
            checked={v.stroke !== null}
            onChange={(e) =>
              onEdit(
                patch({
                  stroke: e.target.checked
                    ? { color: hexColor("#1a1a1e"), width: 4, widths: [] }
                    : null,
                }),
                false,
              )
            }
            aria-label="Stroke enabled"
          />
          Stroke
          {v.stroke && (
            <input
              type="color"
              value={colorToHex(v.stroke.color)}
              onChange={(e) =>
                onEdit(
                  patch({
                    stroke: { ...v.stroke!, color: hexColor(e.target.value) },
                  }),
                  true,
                )
              }
              onBlur={onGestureEnd}
              aria-label="Stroke color"
            />
          )}
        </label>
        {v.stroke &&
          slider("Stroke width", v.stroke.width, 1, 50, 1, (w) =>
            patch({ stroke: { ...v.stroke!, width: w } }),
          )}
        {"Rect" in v.shape &&
          (() => {
            const rect = v.shape.Rect;
            // Half the shorter side is where the corners meet and the rect
            // becomes a capsule; past that there is nothing left to round.
            const most = Math.max(1, Math.min(rect.width, rect.height) / 2);
            return slider(
              "Corner radius",
              rect.radius ?? 0,
              0,
              most,
              0.5,
              (r) => patch({ shape: { Rect: { ...rect, radius: r } } }),
            );
          })()}
        {"Path" in v.shape &&
          (() => {
            const path = ("Path" in v.shape && v.shape.Path) as Extract<
              VectorShape,
              { Path: unknown }
            >["Path"];
            const curved = path.handles.some((h) => h.some((v) => v !== 0));
            return (
              <>
                <label className="row">
                  <input
                    type="checkbox"
                    checked={path.smooth}
                    disabled={curved}
                    onChange={(e) =>
                      onEdit(
                        patch({
                          shape: {
                            Path: {
                              ...path,
                              smooth: e.target.checked,
                              handles: path.handles,
                            },
                          },
                        }),
                        false,
                      )
                    }
                    aria-label="Smooth path"
                  />
                  Smooth
                </label>
                {curved ? (
                  <>
                    <p className="muted">
                      Curve handles are set, so they define the shape. Drag them
                      on the canvas; hold Alt to move one on its own.
                    </p>
                    <button
                      className="mask-button"
                      onClick={() =>
                        onEdit(
                          patch({ shape: { Path: { ...path, handles: [] } } }),
                          false,
                        )
                      }
                    >
                      Straighten curves
                    </button>
                  </>
                ) : (
                  <button
                    className="mask-button"
                    onClick={() =>
                      onEdit(
                        patch({
                          shape: {
                            Path: { ...path, handles: seedHandles(path) },
                          },
                        }),
                        false,
                      )
                    }
                  >
                    Convert to curves
                  </button>
                )}
              </>
            );
          })()}
      </>
    );
  }

  return null;
}

function download(bytes: Uint8Array, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}
