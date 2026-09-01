import { useCallback, useEffect, useRef, useState } from "react";
import {
  Command,
  LayerInfo,
  NodeId,
  WasmSession,
  hexColor,
  identity,
  initEngine,
  nodePayload,
  sendCommand,
} from "./engine";

const TOOLS = ["Move", "Rect", "Ellipse"] as const;
type Tool = (typeof TOOLS)[number];

const DOC_WIDTH = 1280;
const DOC_HEIGHT = 720;

interface DragState {
  tool: Tool;
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  /** Move tool: the node being dragged and its starting translation. */
  target?: NodeId;
  origin?: [number, number];
}

export function App() {
  const [session, setSession] = useState<WasmSession | null>(null);
  const [tool, setTool] = useState<Tool>("Move");
  const [fill, setFill] = useState("#6c8cff");
  const [layers, setLayers] = useState<LayerInfo[]>([]);
  const [selected, setSelected] = useState<NodeId | null>(null);
  const [cmyk, setCmyk] = useState(false);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dragRef = useRef<DragState | null>(null);
  const shapeCount = useRef(0);

  const refresh = useCallback((s: WasmSession) => {
    const canvas = canvasRef.current;
    if (canvas) {
      const ctx = canvas.getContext("2d")!;
      const pixels = new Uint8ClampedArray(s.render_rgba());
      ctx.putImageData(new ImageData(pixels, s.width, s.height), 0, 0);
    }
    setLayers(JSON.parse(s.layers_json()) as LayerInfo[]);
  }, []);

  const newDocument = useCallback(
    (useCmyk: boolean) => {
      const s = new WasmSession(DOC_WIDTH, DOC_HEIGHT, useCmyk);
      setSession(s);
      setCmyk(useCmyk);
      setSelected(null);
      shapeCount.current = 0;
      refresh(s);
    },
    [refresh],
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

  const undo = useCallback(() => {
    if (session?.undo()) refresh(session);
  }, [session, refresh]);
  const redo = useCallback(() => {
    if (session?.redo()) refresh(session);
  }, [session, refresh]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);

  /** Pointer position in document pixels. */
  const docPoint = (e: React.PointerEvent): [number, number] => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return [
      ((e.clientX - rect.left) / rect.width) * DOC_WIDTH,
      ((e.clientY - rect.top) / rect.height) * DOC_HEIGHT,
    ];
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (!session) return;
    const [x, y] = docPoint(e);
    const drag: DragState = { tool, startX: x, startY: y, lastX: x, lastY: y };
    if (tool === "Move") {
      const hit = session.hit_test(x, y);
      if (hit === undefined) {
        setSelected(null);
        return;
      }
      drag.target = hit;
      drag.origin = session.translation_of(hit) as unknown as [number, number];
      setSelected(hit);
    }
    dragRef.current = drag;
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const drag = dragRef.current;
    if (!drag) return;
    [drag.lastX, drag.lastY] = docPoint(e);
  };

  const onPointerUp = () => {
    const drag = dragRef.current;
    dragRef.current = null;
    if (!drag || !session) return;

    if (drag.tool === "Move" && drag.target !== undefined && drag.origin) {
      const [dx, dy] = [drag.lastX - drag.startX, drag.lastY - drag.startY];
      if (dx === 0 && dy === 0) return;
      run({
        SetTransform: {
          id: drag.target,
          transform: identity(drag.origin[0] + dx, drag.origin[1] + dy),
        },
      });
      return;
    }

    // Shape tools: commit the dragged bounds as a new object.
    const x0 = Math.min(drag.startX, drag.lastX);
    const y0 = Math.min(drag.startY, drag.lastY);
    const w = Math.abs(drag.lastX - drag.startX);
    const h = Math.abs(drag.lastY - drag.startY);
    if (w < 2 || h < 2) return;
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
        <button onClick={saveFile}>Save</button>
        <button onClick={exportPng}>Export PNG</button>
        <span className="spacer" />
        <button onClick={undo} title="Ctrl+Z">
          Undo
        </button>
        <button onClick={redo} title="Ctrl+Shift+Z">
          Redo
        </button>
        <span className="doc-title">
          Untitled — {cmyk ? "CMYK" : "RGB"}, {DOC_WIDTH}×{DOC_HEIGHT}
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
        <main className="canvas-host">
          <canvas
            id="engine-canvas"
            ref={canvasRef}
            width={DOC_WIDTH}
            height={DOC_HEIGHT}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
          />
        </main>
        <aside className="panel" aria-label="Layers">
          <div className="panel-head">
            <h2>Layers</h2>
            <button onClick={addExposure} title="Add exposure adjustment layer">
              +FX
            </button>
            <button
              onClick={deleteSelected}
              disabled={selected === null}
              title="Delete selected layer"
            >
              🗑
            </button>
          </div>
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
