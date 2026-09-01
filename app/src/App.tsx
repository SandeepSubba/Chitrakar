import { useState } from "react";
import { sendCommand } from "./engine";

interface LayerRow {
  id: number;
  name: string;
  visible: boolean;
}

const TOOLS = ["Move", "Rect", "Ellipse", "Pen", "Text"] as const;

/**
 * Editor chrome skeleton: toolbar / canvas / layers panel. The canvas is a
 * placeholder <canvas> that the engine (WASM + WebGPU) takes over in the
 * Phase 0 spike; layer state becomes engine-owned at the same time.
 */
export function App() {
  const [tool, setTool] = useState<(typeof TOOLS)[number]>("Move");
  const [layers, setLayers] = useState<LayerRow[]>([
    { id: 1, name: "Layer 1", visible: true },
  ]);

  const toggleLayer = (id: number) => {
    setLayers((prev) =>
      prev.map((l) => (l.id === id ? { ...l, visible: !l.visible } : l)),
    );
    const layer = layers.find((l) => l.id === id);
    if (layer) {
      sendCommand({ SetVisible: { id, visible: !layer.visible } });
    }
  };

  return (
    <div className="editor">
      <header className="topbar">
        <span className="brand">Chitrakar</span>
        <span className="doc-title">Untitled — RGB, 1920×1080</span>
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
        </nav>
        <main className="canvas-host">
          <canvas id="engine-canvas" width={1920} height={1080} />
        </main>
        <aside className="panel" aria-label="Layers">
          <h2>Layers</h2>
          <ul>
            {layers.map((l) => (
              <li key={l.id}>
                <button
                  className="visibility"
                  onClick={() => toggleLayer(l.id)}
                  aria-pressed={l.visible}
                  title={l.visible ? "Hide layer" : "Show layer"}
                >
                  {l.visible ? "👁" : "–"}
                </button>
                <span className={l.visible ? "" : "muted"}>{l.name}</span>
              </li>
            ))}
          </ul>
        </aside>
      </div>
    </div>
  );
}
