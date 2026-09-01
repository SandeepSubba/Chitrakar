import { useCallback, useEffect, useRef, useState } from "react";
import {
  Adjustment,
  BlendMode,
  Command,
  LayerInfo,
  Mask,
  NodeId,
  NodeKind,
  Transform,
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

const TOOLS = ["Move", "Rect", "Ellipse", "Pen"] as const;
type Tool = (typeof TOOLS)[number];
const BLEND_MODES: BlendMode[] = ["Normal", "Multiply", "Screen"];
const HANDLES = ["nw", "ne", "sw", "se"] as const;
type Handle = (typeof HANDLES)[number];

const DOC_WIDTH = 1280;
const DOC_HEIGHT = 720;
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
  const [layers, setLayers] = useState<LayerInfo[]>([]);
  const [selected, setSelected] = useState<NodeId | null>(null);
  const [cmyk, setCmyk] = useState(false);
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
      Math.min(host.clientWidth / DOC_WIDTH, host.clientHeight / DOC_HEIGHT) *
      0.9;
    setView({
      zoom,
      x: (host.clientWidth - DOC_WIDTH * zoom) / 2,
      y: (host.clientHeight - DOC_HEIGHT * zoom) / 2,
    });
  }, []);

  const newDocument = useCallback(
    (useCmyk: boolean) => {
      const s = new WasmSession(DOC_WIDTH, DOC_HEIGHT, useCmyk);
      setSession(s);
      setCmyk(useCmyk);
      setSelected(null);
      setHasIcc(false);
      setProofing(false);
      setGamutWarn(false);
      shapeCount.current = 0;
      refresh(s);
      fitView();
    },
    [refresh, fitView],
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
                  shape: { Path: { points, closed } },
                  fill: closed
                    ? cmyk
                      ? hexToCmykColor(fill)
                      : hexColor(fill)
                    : null,
                  stroke: closed
                    ? null
                    : { color: hexColor(fill), width: 4 },
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

  // Keyboard: undo/redo, space-to-pan, escape-to-cancel.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
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
      ((e.clientX - rect.left) / rect.width) * DOC_WIDTH,
      ((e.clientY - rect.top) / rect.height) * DOC_HEIGHT,
    ];
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
    };
    if (tool === "Move") {
      const hit = session.hit_test(x, y);
      if (hit === undefined) {
        setSelected(null);
        return;
      }
      drag.target = hit;
      drag.t0 = toTransform(session.transform_of(hit));
      setSelected(hit);
    }
    toolDragRef.current = drag;
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onCanvasPointerMove = (e: React.PointerEvent) => {
    const drag = toolDragRef.current;
    if (!drag) return;
    [drag.lastX, drag.lastY] = docPoint(e);
    // Move tool: live preview while dragging.
    if (drag.tool === "Move" && drag.target !== undefined && drag.t0) {
      const dx = drag.lastX - drag.startX;
      const dy = drag.lastY - drag.startY;
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
    if (!session || selected === null) return;
    e.stopPropagation();
    const b = session.bounds_of(selected);
    if (b.length !== 4) return;
    handleDragRef.current = {
      corner,
      id: selected,
      t0: toTransform(session.transform_of(selected)),
      b0: [b[0], b[1], b[2], b[3]],
    };
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onHandlePointerMove = (e: React.PointerEvent) => {
    const drag = handleDragRef.current;
    if (!drag || !session) return;
    const [cx, cy] = docPoint(e);
    const [bx, by, bw, bh] = drag.b0;
    const west = drag.corner === "nw" || drag.corner === "sw";
    const north = drag.corner === "nw" || drag.corner === "ne";
    const newW = Math.max(MIN_SIZE, west ? bx + bw - cx : cx - bx);
    const newH = Math.max(MIN_SIZE, north ? by + bh - cy : cy - by);
    preview({
      SetTransform: {
        id: drag.id,
        transform: {
          ...drag.t0,
          a: (drag.t0.a * newW) / bw,
          d: (drag.t0.d * newH) / bh,
          e: west ? bx + bw - newW : drag.t0.e,
          f: north ? by + bh - newH : drag.t0.f,
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

  const jumpHistory = (delta: number) => {
    if (!session || delta === 0) return;
    session.jump(delta);
    setSelected(null);
    setMultiSel([]);
    refresh(session);
  };

  const selectedLayer = layers.find((l) => l.id === selected) ?? null;
  const resizable =
    selectedLayer?.kind === "vector" || selectedLayer?.kind === "raster";
  const selBounds =
    session && selected !== null && resizable
      ? session.bounds_of(selected)
      : null;
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

  /** Attach an ellipse mask inscribed in the layer's current bounds. */
  const addMask = () => {
    if (!session || !selectedLayer) return;
    const b = session.bounds_of(selectedLayer.id);
    if (b.length !== 4) return;
    run({
      SetMask: {
        id: selectedLayer.id,
        mask: {
          kind: {
            Vector: {
              shape: { Ellipse: { rx: b[2] / 2, ry: b[3] / 2 } },
              transform: { a: 1, b: 0, c: 0, d: 1, e: b[0], f: b[1] },
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

  const openFile = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    file.arrayBuffer().then((buf) => {
      const s = WasmSession.open(new Uint8Array(buf));
      setSession(s);
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
        <button onClick={() => newDocument(false)}>New RGB</button>
        <button onClick={() => newDocument(true)}>New CMYK</button>
        <label className="file-button">
          Open
          <input type="file" accept=".chitra" onChange={openFile} hidden />
        </label>
        <label className="file-button" title="Place image">
          Place
          <input
            type="file"
            accept="image/png,image/jpeg"
            onChange={placeImage}
            hidden
          />
        </label>
        <label
          className="file-button"
          title="Load a CMYK press profile (ICC) for ink preview and proofing"
        >
          {hasIcc ? "ICC ✓" : "ICC…"}
          <input type="file" accept=".icc,.icm" onChange={loadIccProfile} hidden />
        </label>
        {hasIcc && (
          <>
            <button
              className={proofing ? "toggled" : ""}
              onClick={() => applyProofing(!proofing, false)}
              title="Soft proof: preview what the press can reproduce"
            >
              Proof
            </button>
            <button
              className={gamutWarn ? "toggled" : ""}
              onClick={() =>
                gamutWarn ? applyProofing(proofing, false) : applyProofing(true, true)
              }
              title="Mark out-of-gamut pixels grey"
            >
              Gamut
            </button>
          </>
        )}
        <button onClick={saveFile}>Save</button>
        <button onClick={exportPng}>Export PNG</button>
        <span className="spacer" />
        <button onClick={undo} title="Ctrl+Z">
          Undo
        </button>
        <button onClick={redo} title="Ctrl+Shift+Z">
          Redo
        </button>
        <button onClick={fitView} title="Fit document to window">
          Fit
        </button>
        <span className="doc-title">
          {cmyk ? "CMYK" : "RGB"}, {DOC_WIDTH}×{DOC_HEIGHT} ·{" "}
          {Math.round(view.zoom * 100)}%
        </span>
      </header>
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
              title={t}
            >
              {t[0]}
            </button>
          ))}
          <input
            type="color"
            value={fill}
            onChange={(e) => setFill(e.target.value)}
            title="Fill color"
            className="fill-swatch"
          />
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
            width={DOC_WIDTH}
            height={DOC_HEIGHT}
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
              <circle
                className="pen-first"
                cx={view.x + penPoints[0][0] * view.zoom}
                cy={view.y + penPoints[0][1] * view.zoom}
                r={5}
              />
            </svg>
          )}
          {selBounds && selBounds.length === 4 && (
            <div
              className="sel-overlay"
              style={{
                left: view.x + selBounds[0] * view.zoom,
                top: view.y + selBounds[1] * view.zoom,
                width: selBounds[2] * view.zoom,
                height: selBounds[3] * view.zoom,
              }}
            >
              {HANDLES.map((c) => (
                <div
                  key={c}
                  className={`handle ${c}`}
                  data-handle={c}
                  onPointerDown={(e) => onHandlePointerDown(e, c)}
                  onPointerMove={onHandlePointerMove}
                  onPointerUp={onHandlePointerUp}
                />
              ))}
            </div>
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
            >
              ↑
            </button>
            <button
              onClick={() => reorderSelected(-1)}
              disabled={!selectedLayer || selectedLayer.index === 0}
              title="Lower layer"
            >
              ↓
            </button>
            <button
              onClick={groupSelection}
              disabled={selectionSet.length === 0}
              title="Group selected layers (ctrl-click to select several)"
            >
              ⧉
            </button>
            <button
              onClick={ungroupSelection}
              disabled={selectedLayer?.kind !== "group"}
              title="Ungroup selected group"
            >
              ⧎
            </button>
            <button
              onClick={deleteSelected}
              disabled={selected === null}
              title="Delete selected layer"
            >
              🗑
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
                  {l.visible ? "👁" : "–"}
                </button>
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
                    className={l.visible ? "" : "muted"}
                    onDoubleClick={() =>
                      setRenaming({ id: l.id, value: l.name })
                    }
                  >
                    {l.name}
                  </span>
                )}
                <span className="kind">
                  {l.has_mask ? "◐ " : ""}
                  {l.kind}
                </span>
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

interface KindPropsProps {
  kind: NodeKind;
  /** gesture=true routes through preview (live, uncommitted). */
  onEdit: (kind: NodeKind, gesture: boolean) => void;
  onGestureEnd: () => void;
}

/** Parameter editors for the selected node's kind — the panel that makes
 * every layer's settings revisitable (the non-destructive contract). */
function KindProps({ kind, onEdit, onGestureEnd }: KindPropsProps) {
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

  if ("Vector" in kind) {
    const v = kind.Vector;
    const patch = (changes: Partial<typeof v>): NodeKind => ({
      Vector: { ...v, ...changes },
    });
    return (
      <>
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
          {v.fill && !("Cmyk" in v.fill) && (
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
        {v.fill && "Cmyk" in v.fill && (
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
                    ? { color: hexColor("#1a1a1e"), width: 4 }
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
