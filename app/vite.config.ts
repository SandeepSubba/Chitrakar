import { defaultClientConditions, defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";

/** The app's version, from the one place it is written, for the About
 * window. */
const { version } = JSON.parse(readFileSync("./package.json", "utf8"));

/** The two headers that make the page cross-origin isolated, which is
 * what lets the develop worker's threaded engine share its memory
 * (`src/developer.ts`). The shell sends them too (tauri.conf.json), and
 * the service worker adds them where a host does not. */
const ISOLATION = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  // Tauri dev server contract: fixed port, fail rather than drift.
  server: {
    port: 5173,
    strictPort: true,
    headers: ISOLATION,
  },
  preview: { headers: ISOLATION },
  // Relative asset paths so the bundle works from Tauri's frontendDist.
  base: "./",
  clearScreen: false,
  // The raw reader's worker loads its module with a dynamic import, which
  // a worker bundled as a classic script cannot do.
  worker: { format: "es" },
  // onnxruntime-web (the subject model's runtime) as its separate files
  // rather than one bundle: it starts its threads by spawning its own
  // script, and bundled into the worker that script would be the worker.
  resolve: { conditions: ["onnxruntime-web-use-extern-wasm", ...defaultClientConditions] },
});
