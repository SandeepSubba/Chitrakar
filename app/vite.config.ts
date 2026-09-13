import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Tauri dev server contract: fixed port, fail rather than drift.
  server: {
    port: 5173,
    strictPort: true,
  },
  // Relative asset paths so the bundle works from Tauri's frontendDist.
  base: "./",
  clearScreen: false,
});
