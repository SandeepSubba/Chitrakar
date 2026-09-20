import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";

/** The app's version, from the one place it is written, for the About
 * window. */
const { version } = JSON.parse(readFileSync("./package.json", "utf8"));

export default defineConfig({
  plugins: [react()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  // Tauri dev server contract: fixed port, fail rather than drift.
  server: {
    port: 5173,
    strictPort: true,
  },
  // Relative asset paths so the bundle works from Tauri's frontendDist.
  base: "./",
  clearScreen: false,
});
