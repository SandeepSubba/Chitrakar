import { useCallback, useEffect, useRef, useState } from "react";
import { Icon, IconName } from "./icons";
import {
  Adjustment,
  BlendMode,
  AuthoredColor,
  Command,
  GradientStop,
  LayerInfo,
  Mask,
  NodeId,
  NodeKind,
  Transform,
  VectorShape,
  WasmSession,
  colorToHex,
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

const TOOLS = ["Move", "Rect", "Ellipse", "Pen", "Brush", "Text"] as const;
/** One letter per tool, the convention every editor shares. `v` for Move
 * because that is where the muscle memory is; `m` too, since the tool is
 * called Move here. */
const TOOL_KEYS: Record<string, (typeof TOOLS)[number]> = {
  v: "Move",
  m: "Move",
  r: "Rect",
  e: "Ellipse",
  p: "Pen",
  b: "Brush",
  t: "Text",
};

/** A glyph per layer kind, so the stack is scannable without reading the
 * type label at the end of every row. */
const KIND_ICONS: Record<string, IconName> = {
  group: "group-layer",
  vector: "rect",
  raster: "image",
  adjustment: "adjust",
  filter: "filter",
  text: "text",
};

const TOOL_HINT: Record<(typeof TOOLS)[number], string> = {
  Move: "V",
  Rect: "R",
  Ellipse: "E",
  Pen: "P",
  Brush: "B",
  Text: "T",
};

const TOOL_ICONS: Record<(typeof TOOLS)[number], IconName> = {
  Move: "move",
  Rect: "rect",
  Ellipse: "ellipse",
  Pen: "pen",
  Brush: "brush",
  Text: "text",
};
type Tool = (typeof TOOLS)[number];
const BLEND_MODES: BlendMode[] = ["Normal", "Multiply", "Screen"];
/** Minimum travel between recorded brush samples, and how far a simplified
 * stroke may stray from the one that was drawn — both in document units. */
const BRUSH_STEP = 3;
const BRUSH_TOLERANCE = 2;

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
function isTextEntry(target: EventTarget | null): boolean {
  if (target instanceof HTMLTextAreaElement) return true;
  if (target instanceof HTMLElement && target.isContentEditable) return true;
  if (!(target instanceof HTMLInputElement)) return false;
  return ["text", "number", "search", "email", "url", "tel", "password"].includes(
    target.type,
  );
}

const HANDLES = ["nw", "ne", "sw", "se"] as const;
/** Which corner of the selection quad (tl, tr, br, bl) each handle sits on. */
const HANDLE_CORNER = [0, 1, 3, 2];
type Handle = (typeof HANDLES)[number];

/** Default canvas for a new document; any size can be chosen, and an
 * opened file brings its own. */
const DOC_WIDTH = 1280;
const DOC_HEIGHT = 720;

/** Starting points offered in the new-document dialog. */
const DOC_PRESETS: [string, number, number][] = [
  ["HD 1280×720", 1280, 720],
  ["Full HD 1920×1080", 1920, 1080],
  ["Square 1080×1080", 1080, 1080],
  ["A4 at 300dpi", 2480, 3508],
  ["Postcard at 300dpi", 1748, 1240],
];
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
  /** Brush: the stroke so far, in document coordinates. */
  stroke?: [number, number][];
  /** Brush: a width multiplier per recorded sample. */
  widths?: number[];
  /** Brush: when the last sample was taken, for the speed that sets it. */
  lastAt?: number;
}

interface HandleDrag {
  corner: Handle;
  id: NodeId;
  t0: Transform;
  b0: [number, number, number, number];
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
  const [layers, setLayers] = useState<LayerInfo[]>([]);
  const [selected, setSelected] = useState<NodeId | null>(null);
  const [cmyk, setCmyk] = useState(false);
  /** Which top-level menu is open, if any. */
  const [openMenu, setOpenMenu] = useState<"file" | "edit" | "view" | null>(null);
  const openInputRef = useRef<HTMLInputElement>(null);
  const placeInputRef = useRef<HTMLInputElement>(null);
  const iccInputRef = useRef<HTMLInputElement>(null);
  const pick = (ref: React.RefObject<HTMLInputElement>) => ref.current?.click();
  const [view, setView] = useState<View>({ zoom: 1, x: 0, y: 0 });
  const [opacityDraft, setOpacityDraft] = useState<number | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const imgDataRef = useRef<ImageData | null>(null);
  const toolDragRef = useRef<ToolDrag | null>(null);
  const handleDragRef = useRef<HandleDrag | null>(null);
  const panDragRef = useRef<PanDrag | null>(null);
  const spaceRef = useRef(false);
  const shapeCount = useRef(0);
  /** Pen tool: anchors of the path being drawn, in doc coordinates. */
  const [penPoints, setPenPoints] = useState<[number, number][]>([]);
  /** Extra layers picked with ctrl/cmd-click, beyond the primary selection. */
  const [multiSel, setMultiSel] = useState<NodeId[]>([]);
  const groupCount = useRef(0);

  const refresh = useCallback((s: WasmSession) => {
    const canvas = canvasRef.current;
    if (canvas) {
      const ctx = canvas.getContext("2d")!;
      const dirty = s.render_frame();
      if (
        !imgDataRef.current ||
        imgDataRef.current.width !== s.width ||
        imgDataRef.current.height !== s.height
      ) {
        imgDataRef.current = new ImageData(s.width, s.height);
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
  }, []);

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

  const newDocument = useCallback(
    (useCmyk: boolean, width = DOC_WIDTH, height = DOC_HEIGHT) => {
      const s = new WasmSession(width, height, useCmyk);
      setSession(s);
      setDocumentSize(width, height);
      setCmyk(useCmyk);
      setSelected(null);
      setHasIcc(false);
      setProofing(false);
      setGamutWarn(false);
      shapeCount.current = 0;
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

  const undo = useCallback(() => {
    if (session?.undo()) refresh(session);
  }, [session, refresh]);
  const redo = useCallback(() => {
    if (session?.redo()) refresh(session);
  }, [session, refresh]);

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
                  shape: { Path: { points, closed, smooth: false, handles: [] } },
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
      const typing = isTextEntry(e.target) || e.target instanceof HTMLSelectElement;
      if (!typing && !e.metaKey && !e.ctrlKey && !e.altKey) {
        const shortcut = TOOL_KEYS[e.key.toLowerCase()];
        if (shortcut) {
          e.preventDefault();
          setTool(shortcut);
          setPenPoints([]);
        }
      }
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
      if (e.key === "Escape") cancelGesture();
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
  }, [undo, redo, cancelGesture, finishPath]);

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

  /** Pointer position in document pixels. */
  const docPoint = (e: { clientX: number; clientY: number }): [number, number] => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return [
      ((e.clientX - rect.left) / rect.width) * docSize[0],
      ((e.clientY - rect.top) / rect.height) * docSize[1],
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
  ): [number, number] => {
    const [x, y] = docPoint(e);
    if (!session || id === null) return [x, y];
    const inv = inverseOf(toTransform(session.parent_space_of(id)));
    return inv ? inv(x, y) : [x, y];
  };

  /** A displacement carried into a layer's parent space: a vector, so only
   * the linear part of the space applies. */
  const layerVector = (id: NodeId, dx: number, dy: number): [number, number] => {
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
    if (at < 0 || layers[at].kind !== "group") return false;
    for (let i = at + 1; i < layers.length; i++) {
      if (layers[i].depth <= layers[at].depth) break;
      if (layers[i].id === id) return true;
    }
    return false;
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

  const onCanvasPointerDown = (e: React.PointerEvent) => {
    if (!session || isPanTrigger(e) || e.button !== 0) return;
    e.stopPropagation();
    const [x, y] = docPoint(e);
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
    if (tool === "Move") {
      const hit = session.hit_test(x, y);
      if (hit === undefined) {
        setSelected(null);
        return;
      }
      // Dragging inside a selected group moves the group. Hit testing only
      // ever reports leaves, so without this a group could be selected in
      // the panel and still not be draggable — you would grab whichever
      // child happened to be under the cursor.
      const target = inSelectedGroup(hit) ? selected! : hit;
      drag.target = target;
      drag.t0 = toTransform(session.transform_of(target));
      setSelected(target);
    }
    toolDragRef.current = drag;
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onCanvasPointerMove = (e: React.PointerEvent) => {
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
    // Move tool: live preview while dragging.
    if (drag.tool === "Move" && drag.target !== undefined && drag.t0) {
      const [dx, dy] = layerVector(
        drag.target,
        drag.lastX - drag.startX,
        drag.lastY - drag.startY,
      );
      if (dx !== 0 || dy !== 0) {
        drag.moved = true;
        preview({
          SetTransform: {
            id: drag.target,
            transform: { ...drag.t0, e: drag.t0.e + dx, f: drag.t0.f + dy },
          },
        });
      }
    }
  };

  const onCanvasPointerUp = () => {
    const drag = toolDragRef.current;
    toolDragRef.current = null;
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
    shapeCount.current += 1;
    const shape =
      drag.tool === "Rect"
        ? { Rect: { width: w, height: h } }
        : { Ellipse: { rx: w / 2, ry: h / 2 } };
    run({
      AddNode: {
        parent: session.root_id,
        index: topLevelCount(layers),
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
          x0,
          y0,
        ),
      },
    });
  };

  // Resize handles: drag scales the node, anchored at the opposite corner.
  const onHandlePointerDown = (e: React.PointerEvent, corner: Handle) => {
    if (!session || selected === null || !selLocal) return;
    e.stopPropagation();
    handleDragRef.current = {
      corner,
      id: selected,
      t0: toTransform(session.transform_of(selected)),
      b0: selLocal,
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onHandlePointerMove = (e: React.PointerEvent) => {
    const drag = handleDragRef.current;
    if (!drag || !session) return;
    if (e.buttons === 0) return;
    // Resize happens in the layer's own space: bring the cursor there,
    // hold the opposite corner still, and scale about it. Doing it in
    // document space would stretch a rotated layer along the wrong axes.
    const [dx, dy] = layerPoint(e);
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
    if (session?.commit_preview()) refresh(session);
  };

  const ADJUSTMENT_PRESETS: Record<string, { name: string; kind: NodeKind }> = {
    exposure: { name: "Exposure", kind: { Adjustment: { Exposure: { stops: 0 } } } },
    "brightness-contrast": {
      name: "Brightness/Contrast",
      kind: { Adjustment: { BrightnessContrast: { brightness: 0, contrast: 0 } } },
    },
    "hue-saturation": {
      name: "Hue/Saturation",
      kind: {
        Adjustment: {
          HueSaturation: { hue_degrees: 0, saturation: 0, lightness: 0 },
        },
      },
    },
    blur: { name: "Gaussian Blur", kind: { Filter: { GaussianBlur: { sigma: 4 } } } },
    sharpen: {
      name: "Sharpen",
      kind: { Filter: { Sharpen: { sigma: 1.5, amount: 0.5 } } },
    },
  };

  const addAdjustment = (key: string) => {
    const preset = ADJUSTMENT_PRESETS[key];
    if (!session || !preset) return;
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

  const [renaming, setRenaming] = useState<{ id: NodeId; value: string } | null>(
    null,
  );

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
        if (k === "v") {
          e.preventDefault();
          pasteClipboard();
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
  const gradPoint = (e: { clientX: number; clientY: number }): [number, number] => {
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
  // Adjustment and filter layers act on everything below them and have no
  // box of their own; everything else, groups included, can be moved,
  // scaled and turned.
  const resizable =
    selectedLayer !== null &&
    selectedLayer.kind !== "adjustment" &&
    selectedLayer.kind !== "filter";
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
      const t = composeT(selParent, toTransform(session.transform_of(selected)));
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
  if (session && selectedLayer) {
    try {
      selectedKind = JSON.parse(session.kind_json(selectedLayer.id)) as NodeKind;
      selectedMask = JSON.parse(session.mask_json(selectedLayer.id)) as Mask | null;
    } catch {
      selectedKind = null;
    }
  }

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
  const addMask = () => {
    if (!session || !selectedLayer) return;
    // A mask is written in the layer's parent space, so inscribe it in the
    // layer's bounds *there* — mapping its own box through its own
    // transform — rather than in document bounds, which only agree while
    // the layer sits at the top level.
    const lb = session.local_bounds_of(selectedLayer.id);
    if (lb.length !== 4) return;
    const t = toTransform(session.transform_of(selectedLayer.id));
    const xs = [t.a * lb[0] + t.c * lb[1] + t.e, t.a * lb[2] + t.c * lb[3] + t.e];
    const ys = [t.b * lb[0] + t.d * lb[1] + t.f, t.b * lb[2] + t.d * lb[3] + t.f];
    const [x0, x1] = [Math.min(...xs), Math.max(...xs)];
    const [y0, y1] = [Math.min(...ys), Math.max(...ys)];
    run({
      SetMask: {
        id: selectedLayer.id,
        mask: {
          kind: {
            Vector: {
              shape: { Ellipse: { rx: (x1 - x0) / 2, ry: (y1 - y0) / 2 } },
              transform: { a: 1, b: 0, c: 0, d: 1, e: x0, f: y0 },
            },
          },
          invert: false,
        },
      },
    });
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

  const commitOpacity = () => {
    if (opacityDraft !== null && selectedLayer) {
      run({ SetOpacity: { id: selectedLayer.id, opacity: opacityDraft } });
    }
    setOpacityDraft(null);
  };

  const saveFile = () => {
    if (!session) return;
    download(session.save(), "untitled.chitra", "application/zip");
  };

  const exportPng = () => {
    if (!session) return;
    download(session.export_png(), "untitled.png", "image/png");
  };

  const exportJpeg = () => {
    if (!session) return;
    download(session.export_jpeg(92), "untitled.jpg", "image/jpeg");
  };

  const exportPdf = () => {
    if (!session) return;
    try {
      download(session.export_pdf(), "untitled.pdf", "application/pdf");
    } catch (err) {
      alert(`PDF export: ${err}`);
    }
  };

  const exportTiff = () => {
    if (!session) return;
    try {
      download(session.export_cmyk_tiff(), "untitled.tif", "image/tiff");
    } catch (err) {
      alert(`CMYK TIFF export: ${err}`);
    }
  };

  const exportSvg = () => {
    if (!session) return;
    download(
      new TextEncoder().encode(session.export_svg()),
      "untitled.svg",
      "image/svg+xml",
    );
  };

  const openFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    file.arrayBuffer().then((buf) => {
      const s = WasmSession.open(new Uint8Array(buf));
      setSession(s);
      setDocumentSize(s.width, s.height);
      setCmyk(s.cmyk);
      setSelected(null);
      setHasIcc(s.has_cmyk_profile);
      setProofing(false);
      setGamutWarn(false);
      refresh(s);
      fitView();
    });
  };

  const placeImage = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file || !session) return;
    file.arrayBuffer().then((buf) => {
      session.place_image(new Uint8Array(buf), file.name);
      refresh(session);
    });
  };

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
            <MenuItem icon="save" onClick={saveFile}>
              Save
            </MenuItem>
            <hr />
            <MenuItem icon="export" onClick={exportPng}>
              Export PNG
            </MenuItem>
            <MenuItem
              icon="export"
              onClick={exportJpeg}
              hint="flattened"
            >
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
          </MenuButton>

          <MenuButton
            label="View"
            open={openMenu === "view"}
            onOpen={() => setOpenMenu(openMenu === "view" ? null : "view")}
            onHover={() => openMenu && setOpenMenu("view")}
            onClose={() => setOpenMenu(null)}
          >
            <MenuItem icon="fit" onClick={fitView}>
              Fit document to window
            </MenuItem>
            <MenuItem icon="zoomIn" onClick={() => zoomBy(1.25)}>
              Zoom in
            </MenuItem>
            <MenuItem icon="zoomOut" onClick={() => zoomBy(0.8)}>
              Zoom out
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
                gamutWarn ? applyProofing(proofing, false) : applyProofing(true, true)
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
          <button className="chrome-button icon-only" onClick={undo} title="Undo (Ctrl+Z)">
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

        <span className="doc-chip">
          {hasIcc && (
            <span className="icc-badge" title="A CMYK press profile is loaded">
              ICC ✓
            </span>
          )}
          {cmyk ? "CMYK" : "RGB"}, {docSize[0]}×{docSize[1]} ·{" "}
          {Math.round(view.zoom * 100)}%
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
          accept="image/png,image/jpeg"
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
      </header>
      {newDocOpen && (
        <NewDocDialog
          onCancel={() => setNewDocOpen(false)}
          onCreate={(w, h, useCmyk) => {
            setNewDocOpen(false);
            newDocument(useCmyk, w, h);
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
            className="fill-swatch"
          />
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
          className="canvas-host"
          ref={hostRef}
          onPointerDown={onHostPointerDown}
          onPointerMove={onHostPointerMove}
          onPointerUp={onHostPointerUp}
        >
          <canvas
            id="engine-canvas"
            ref={canvasRef}
            width={docSize[0]}
            height={docSize[1]}
            style={{
              transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`,
            }}
            onPointerDown={onCanvasPointerDown}
            onPointerMove={onCanvasPointerMove}
            onPointerUp={onCanvasPointerUp}
          />
          {penPoints.length > 0 && (
            <svg className="pen-overlay" aria-hidden="true">
              <polyline
                points={penPoints
                  .map((p) => `${view.x + p[0] * view.zoom},${view.y + p[1] * view.zoom}`)
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
          {selQuad && (
            <>
              <svg className="sel-outline" aria-hidden="true">
                <polygon points={selQuad.map((p) => p.join(",")).join(" ")} />
              </svg>
              {/* The knob sits off the top edge along the box's own normal,
                  so it stays above the layer however the layer is turned. */}
              {(() => {
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
                      <line x1={mid[0]} y1={mid[1]} x2={knob[0]} y2={knob[1]} />
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
                if (typeof kind !== "object" || !("Vector" in kind)) return null;
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
                      {stops.slice(1, -1).map((st, i) =>
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
              {HANDLES.map((c, i) => (
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
              {Object.entries(ADJUSTMENT_PRESETS).map(([key, p]) => (
                <option key={key} value={key}>
                  {p.name}
                </option>
              ))}
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
          {selectedLayer && (
            <div className="layer-props">
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
                    run({
                      SetBlendMode: {
                        id: selectedLayer.id,
                        blend: e.target.value as BlendMode,
                      },
                    })
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
                />
              )}
              {selectedMask === null ? (
                <button className="mask-button" onClick={addMask}>
                  Add ellipse mask
                </button>
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
          <ul>
            {layers.map((l) => (
              <li
                key={l.id}
                className={
                  l.id === selected
                    ? "selected"
                    : multiSel.includes(l.id)
                      ? "multi"
                      : ""
                }
                style={{ paddingLeft: `${l.depth * 14 + 2}px` }}
                onClick={(e) => {
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
                <span className="layer-kind-icon" title={l.kind}>
                  <Icon name={KIND_ICONS[l.kind] ?? "rect"} size={15} />
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
                  <span className="kind" title="This layer has a mask">
                    <Icon name="mask" size={14} />
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

/** New-document dialog: presets for the sizes people actually start from,
 * and the two fields underneath for everything else. Colour mode is chosen
 * here because it decides how every fill in the document is authored, and
 * changing it afterwards would mean reinterpreting them all. */
function NewDocDialog({
  onCreate,
  onCancel,
}: {
  onCreate: (w: number, h: number, cmyk: boolean) => void;
  onCancel: () => void;
}) {
  const [w, setW] = useState(DOC_WIDTH);
  const [h, setH] = useState(DOC_HEIGHT);
  const [mode, setMode] = useState("rgb");

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onCancel();
      if (e.key === "Enter") onCreate(w, h, mode === "cmyk");
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [w, h, mode, onCreate, onCancel]);

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
          {DOC_PRESETS.map(([name, pw, ph]) => (
            <button
              key={name}
              className={w === pw && h === ph ? "preset active" : "preset"}
              onClick={() => {
                setW(pw);
                setH(ph);
              }}
            >
              {name}
            </button>
          ))}
        </div>
        {size("Width", w, setW)}
        {size("Height", h, setH)}
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
            onClick={() => onCreate(w, h, mode === "cmyk")}
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

interface KindPropsProps {
  kind: NodeKind;
  /** gesture=true routes through preview (live, uncommitted). */
  onEdit: (kind: NodeKind, gesture: boolean) => void;
  onGestureEnd: () => void;
  /** Document colour mode, so new colours are authored as ink in a CMYK
   * document exactly like the shape tools do. */
  cmyk: boolean;
}

/** Parameter editors for the selected node's kind — the panel that makes
 * every layer's settings revisitable (the non-destructive contract). */
function KindProps({ kind, onEdit, onGestureEnd, cmyk }: KindPropsProps) {
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
    return null;
  }

  if ("Filter" in kind) {
    const filter = kind.Filter;
    if ("GaussianBlur" in filter) {
      return slider("Blur sigma", filter.GaussianBlur.sigma, 0, 50, 0.5, (v) => ({
        Filter: { GaussianBlur: { sigma: v } },
      }));
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
    const endpoints = (deg: number): { from: [number, number]; to: [number, number] } => {
      const rad = (deg * Math.PI) / 180;
      const [dx, dy] = [Math.cos(rad) / 2, Math.sin(rad) / 2];
      return { from: [0.5 - dx, 0.5 - dy], to: [0.5 + dx, 0.5 + dy] };
    };
    const angleOf = (g: typeof grad): number => {
      if (!g || !("Linear" in g)) return 0;
      const { from, to } = g.Linear;
      const deg = (Math.atan2(to[1] - from[1], to[0] - from[0]) * 180) / Math.PI;
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
                    onEdit(setStop(i, { color: authored(e.target.value) }), true)
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
              : slider("Gradient radius", grad.Radial.radius, 0.05, 1.5, 0.05, (r) =>
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
                            Path: { ...path, smooth: e.target.checked, handles: path.handles },
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
                      Curve handles are set, so they define the shape. Drag
                      them on the canvas; hold Alt to move one on its own.
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
                          shape: { Path: { ...path, handles: seedHandles(path) } },
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
