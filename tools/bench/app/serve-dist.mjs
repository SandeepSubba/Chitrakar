// Serves app/dist on a port, for the gesture bench against the
// production build (`npm run build` in app/ first): node serve-dist.mjs [port]
import http from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
const root = join(dirname(fileURLToPath(import.meta.url)), "../../../app/dist");
const port = Number(process.argv[2] || 8800);
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css", ".json": "application/json", ".webmanifest": "application/manifest+json", ".svg": "image/svg+xml", ".png": "image/png", ".ttf": "font/ttf", ".woff2": "font/woff2" };
http.createServer(async (q, r) => {
  const p0 = q.url.split("?")[0];
  const p = join(root, p0 === "/" ? "/index.html" : p0);
  try { const body = await readFile(p); r.writeHead(200, { "content-type": types[extname(p)] || "application/octet-stream" }); r.end(body); } catch { r.writeHead(404).end(); }
}).listen(port, () => console.log(`serving app/dist on ${port}`));
