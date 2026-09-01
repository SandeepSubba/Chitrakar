import { useCallback, useEffect, useRef, useState } from "react";
import {
  BlendMode,
  Command,
  LayerInfo,
  NodeId,
  Transform,
  WasmSession,
  getWasmMemory,
  hexColor,
  initEngine,
  nodePayload,
  sendCommand,
  sendPreview,
} from "./engine";

const TOOLS = ["Move", "Rect", "Ellipse"] as const;
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
    if (session.cancel_preview()) refresh(session);
  }, [session, refresh]);

  // Keyboard: undo/redo, space-to-pan, escape-to-cancel.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
      if (e.key === "Escape") cancelGesture();
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
  }, [undo, redo, cancelGesture]);

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
          { Vector: { shape, fill: hexColor(fill), stroke: null } },
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

  const addExposure = () => {
    if (!session) return;
    run({
      AddNode: {
        parent: session.root_id,
        index: topLevelCount(layers),
        node: nodePayload("Exposure +0.5", {
          Adjustment: { Exposure: { stops: 0.5 } },
        }),
      },
    });
  };

  const deleteSelected = () => {
    if (selected === null) return;
    run({ RemoveNode: { id: selected } });
    setSelected(null);
  };

  const selectedLayer = layers.find((l) => l.id === selected) ?? null;
  const resizable =
    selectedLayer?.kind === "vector" || selectedLayer?.kind === "raster";
  const selBounds =
    session && selected !== null && resizable
      ? session.bounds_of(selected)
      : null;

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
              onClick={() => setTool(t)}
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
            <button onClick={addExposure} title="Add exposure adjustment layer">
              +FX
            </button>
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
            </div>
          )}
          <ul>
            {layers.map((l) => (
              <li
                key={l.id}
                className={l.id === selected ? "selected" : ""}
                style={{ paddingLeft: `${l.depth * 14 + 2}px` }}
                onClick={() => setSelected(l.id)}
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
                <span className={l.visible ? "" : "muted"}>{l.name}</span>
                <span className="kind">{l.kind}</span>
              </li>
            ))}
            {layers.length === 0 && (
              <li className="muted empty">Drag on the canvas to add shapes</li>
            )}
          </ul>
        </aside>
      </div>
    </div>
  );
}

function topLevelCount(layers: LayerInfo[]): number {
  return layers.filter((l) => l.depth === 0).length;
}

function download(bytes: Uint8Array, name: string, type: string) {
  const url = URL.createObjectURL(new Blob([bytes as BlobPart], { type }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  URL.revokeObjectURL(url);
}
