// Times each layer of layers.json over a 12-megapixel photograph in
// headless Chromium, through the WASM engine alone (no page, no React).
//
//   node tools/bench/engine/run.mjs [pkg-dir] [--only name] [--prof]
//
// `pkg-dir` is a wasm-pack output (default app/src/wasm-pkg: build it
// with `npm run build:wasm` in app/). `--only` keeps the layers whose
// name contains the text; `--prof` prints where the time went instead,
// by self time, from Chromium's own sampling profiler.
import { chromium } from "../../../app/node_modules/playwright/index.mjs";
import http from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const only = args.includes("--only") ? args[args.indexOf("--only") + 1] : null;
const pkg = resolve(
  args.find((a, i) => !a.startsWith("--") && args[i - 1] !== "--only") || join(here, "../../../app/src/wasm-pkg"),
);
const prof = args.includes("--prof");
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json" };
const srv = http.createServer(async (q, r) => {
  const p0 = q.url.split("?")[0];
  if (p0 === "/favicon.ico") { r.writeHead(204).end(); return; }
  const p = p0.startsWith("/pkg/") ? join(pkg, p0.slice(5)) : join(here, p0);
  if (process.env.BENCH_DEBUG) console.log("serve", p0, "->", p);
  try { const body = await readFile(p); r.writeHead(200, { "content-type": types[extname(p)] || "application/octet-stream" }); r.end(body); } catch { r.writeHead(404).end(); }
});
await new Promise((r) => srv.listen(0, r));
const port = srv.address().port;
const b = await chromium.launch({ executablePath: process.env.CHITRAKAR_CHROMIUM, args: ["--no-sandbox"] });
const page = await b.newPage();
page.on("pageerror", (e) => console.log("page error:", e.message));
page.on("console", (m) => { if (m.type() === "error" || process.env.BENCH_DEBUG) console.log("page:", m.text()); });
let cdp;
if (prof) { cdp = await page.context().newCDPSession(page); await cdp.send("Profiler.enable"); await cdp.send("Profiler.setSamplingInterval", { interval: 200 }); await cdp.send("Profiler.start"); }
// `--io`: the photograph on its way in and out (io.html) rather than
// frames.
const pageName = args.includes("--io") ? "io.html" : "page.html";
await page.goto(`http://localhost:${port}/${pageName}${only ? `?only=${encodeURIComponent(only)}` : ""}`);
await page.waitForFunction(() => window.result, null, { timeout: 1_800_000 });
for (const [n, t] of await page.evaluate(() => window.result)) console.log(n.padEnd(22), t);
if (prof) {
  const { profile } = await cdp.send("Profiler.stop");
  const self = new Map(); const byId = new Map(profile.nodes.map((n) => [n.id, n]));
  const dt = profile.timeDeltas; const counts = new Map();
  profile.samples.forEach((id, i) => counts.set(id, (counts.get(id) || 0) + (dt[i] || 0)));
  for (const [id, t] of counts) { const n = byId.get(id); const k = n.callFrame.functionName || n.callFrame.url; self.set(k, (self.get(k) || 0) + t); }
  console.log([...self].sort((a, b) => b[1] - a[1]).slice(0, 25).map(([k, t]) => `${(t / 1000).toFixed(0).padStart(7)} ms  ${k.slice(0, 110)}`).join("\n"));
}
await b.close(); srv.close();
