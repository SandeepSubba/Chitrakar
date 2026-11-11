import { defaultClientConditions, defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync, readdirSync, writeFileSync, existsSync } from "node:fs";
import type { Plugin } from "vite";

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

/** Every file the build put in assets/, listed beside it for the service
 * worker (`public/sw.js`): kept files of this build are never pruned,
 * whether or not this visit loaded them. A list made from what the page
 * loaded lost the raw reader, the PDF reader and the workers, which
 * load later or from a worker, and they failed offline. */
function listAssets(): Plugin {
  let out = "dist";
  return {
    name: "chitrakar-list-assets",
    apply: "build",
    configResolved(config) {
      out = config.build.outDir;
    },
    closeBundle() {
      const dir = `${out}/assets`;
      if (!existsSync(dir)) return;
      // And the threaded engine a raw file is developed with, which is
      // served as it is rather than bundled: left off the list, offline
      // a raw developed on one thread, seven times slower, and nothing
      // said why.
      const threads: string[] = [];
      const walk = (sub: string) => {
        const at = `${out}/${sub}`;
        if (!existsSync(at)) return;
        for (const e of readdirSync(at, { withFileTypes: true })) {
          if (e.isDirectory()) walk(`${sub}/${e.name}`);
          else if (/\.(js|wasm)$/.test(e.name)) threads.push(`${sub}/${e.name}`);
        }
      };
      walk("wasm-threads");
      writeFileSync(
        `${out}/build-assets.json`,
        JSON.stringify([...readdirSync(dir).map((f) => `assets/${f}`), ...threads]),
      );
    },
  };
}

export default defineConfig({
  plugins: [react(), listAssets()],
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
