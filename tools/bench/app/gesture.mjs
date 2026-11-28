// Drives the editor itself in headless Chromium — the page, React and
// the engine together — through a drag or a run of wheel zooms, and
// says how long a step took and how many steps were drafts. Against the
// dev server (`npm run dev` in app/, http://localhost:5173/) for a quick
// look, or the production build (`npm run build` in app/, then
// `node serve-dist.mjs` here, http://localhost:8800/) for the numbers.
//
//   node tools/bench/app/gesture.mjs [url] [photo|<file.chitra>]
//     GESTURE=pan|wheel   (pan)     THROTTLE=4   (Chromium's CPU, as a phone's)
//     PROF=1              (where the time went, by self time)
//     CHITRAKAR_CHROMIUM  (browser path override)
import { chromium } from "../../../app/node_modules/playwright/index.mjs";
import { readFile } from 'node:fs/promises';
const url = process.argv[2] || 'http://localhost:5173/';
const doc = process.argv[3] || 'photo';
const chitra = doc === 'photo' ? null : await readFile(doc);
const PROF = process.env.PROF === '1';
const b = await chromium.launch({ executablePath: process.env.CHITRAKAR_CHROMIUM, args: ['--no-sandbox'] });
for (let round = 0; round < (PROF ? 1 : 2); round++) {
  const ctx = await b.newContext({ viewport: { width: 1280, height: 800 } });
  const page = await ctx.newPage();
  await page.goto(url);
  await page.waitForSelector('#engine-canvas');
  await page.waitForTimeout(1500);
  if (doc === 'photo') {
    await page.evaluate(async () => {
      const W = 4032, H = 3024;
      const c = new OffscreenCanvas(W, H); const g = c.getContext('2d');
      const grad = g.createLinearGradient(0, 0, W, H); grad.addColorStop(0, '#203050'); grad.addColorStop(0.5, '#e0a040'); grad.addColorStop(1, '#40a0c0');
      g.fillStyle = grad; g.fillRect(0, 0, W, H);
      for (let i = 0; i < 400; i++) { g.fillStyle = `hsl(${i * 37 % 360},60%,50%)`; g.fillRect((i * 97) % W, (i * 61) % H, 80, 50); }
      const blob = await c.convertToBlob({ type: 'image/png' });
      const file = new File([blob], 'photo.png', { type: 'image/png' });
      const dt = new DataTransfer(); dt.items.add(file);
      document.querySelector('.canvas-host').dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
    });
    await page.waitForTimeout(6000);
  } else {
    await page.evaluate((bytes) => {
      const file = new File([new Uint8Array(bytes)], 'page.chitra');
      const dt = new DataTransfer(); dt.items.add(file);
      document.querySelector('.canvas-host').dispatchEvent(new DragEvent('drop', { bubbles: true, cancelable: true, dataTransfer: dt }));
    }, [...chitra]);
    await page.waitForTimeout(2500);
  }
  if (process.env.THROTTLE) { const c = await page.context().newCDPSession(page); await c.send('Emulation.setCPUThrottlingRate', { rate: Number(process.env.THROTTLE) }); }
  const host = await page.locator('.canvas-host').boundingBox();
  const drafts = { seen: 0, steps: 0 };
  page.__draft = async () => { drafts.steps++; if (await page.evaluate(() => document.getElementById('engine-canvas').dataset.draft === '1')) drafts.seen++; };
  page.__drafts = drafts;
  const frame = () => page.evaluate(() => new Promise((res) => requestAnimationFrame(() => requestAnimationFrame(res))));
  const drag = async (n) => {
    await page.keyboard.down('Space'); await page.waitForTimeout(150);
    const [sx, sy] = [host.x + 400, host.y + 400];
    await page.mouse.move(sx, sy); await page.mouse.down();
    const t0 = Date.now();
    for (let i = 1; i <= n; i++) { await page.mouse.move(sx + i * 7, sy + i * 3); await frame(); await page.__draft(); }
    const ms = (Date.now() - t0) / n;
    await page.mouse.up(); await page.keyboard.up('Space');
    return ms;
  };
  const wheel = async (n) => {
    const r = host;
    const t0 = Date.now();
    for (let i = 0; i < n; i++) { await page.mouse.move(r.x + 300, r.y + 300); await page.mouse.wheel(0, i % 2 ? 60 : -60); await frame(); await page.__draft(); }
    return (Date.now() - t0) / n;
  };
  const gesture = process.env.GESTURE === 'wheel' ? wheel : drag;
  await gesture(10);
  const drag_ = drag; void drag_;
  let cdp;
  if (PROF) { cdp = await page.context().newCDPSession(page); await cdp.send('Profiler.enable'); await cdp.send('Profiler.setSamplingInterval', { interval: 200 }); await cdp.send('Profiler.start'); }
  const ms = await gesture(40);
  console.log(`round ${round}: ${ms.toFixed(1)} ms per ${process.env.GESTURE || 'pan'} step; drafts ${page.__drafts.seen}/${page.__drafts.steps}`);
  if (PROF) {
    const { profile } = await cdp.send('Profiler.stop');
    const self = new Map(); const byId = new Map(profile.nodes.map((n) => [n.id, n]));
    const dt = profile.timeDeltas; const counts = new Map();
    profile.samples.forEach((id, i) => counts.set(id, (counts.get(id) || 0) + (dt[i] || 0)));
    for (const [id, t] of counts) { const n = byId.get(id); const k = n.callFrame.functionName || n.callFrame.url; self.set(k, (self.get(k) || 0) + t); }
    console.log([...self].sort((a, b) => b[1] - a[1]).slice(0, 14).map(([k, t]) => `${(t / 1000).toFixed(0).padStart(7)} ms  ${k.slice(0, 100)}`).join('\n'));
  }
  await ctx.close();
}
await b.close();
