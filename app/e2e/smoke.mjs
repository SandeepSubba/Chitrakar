// Chitrakar end-to-end smoke suite: serves app/dist, drives the built
// editor in headless Chromium, and pixel-asserts the whole feature set —
// drawing, undo/history, masks, filters, pen paths, text, CMYK + proofing.
//
// Run with `npm run test:e2e` after `npm run build`. Environment:
// - CHITRAKAR_CHROMIUM: optional path to a Chromium binary (otherwise
//   Playwright's own installed browser is used — `npx playwright install chromium`).
// - CHITRAKAR_TEST_CMYK_ICC: optional path to a CMYK .icc profile; the
//   press-profile and soft-proofing steps are skipped without it.
import { chromium } from "playwright";
import { createServer } from "http";
import { readFile, mkdir } from "fs/promises";
import { join, extname, dirname } from "path";
import { fileURLToPath } from "url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = join(HERE, "..", "dist");
const OUT = join(HERE, "out");
await mkdir(OUT, { recursive: true });
const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
};

const server = createServer(async (req, res) => {
  const path = req.url === "/" ? "/index.html" : req.url.split("?")[0];
  try {
    const body = await readFile(join(DIST, path));
    res.writeHead(200, { "content-type": MIME[extname(path)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((r) => server.listen(8123, r));

const browser = await chromium.launch({
  executablePath: process.env.CHITRAKAR_CHROMIUM || undefined,
  args: ["--no-sandbox"],
});
const page = await browser.newPage({ viewport: { width: 1400, height: 900 }, acceptDownloads: true });
const errors = [];
page.on("pageerror", (e) => errors.push(String(e)));
page.on("console", (m) => m.type() === "error" && !m.text().includes("404") && errors.push(m.text()));

await page.goto("http://localhost:8123/");
await page.waitForSelector("#engine-canvas");
await page.waitForTimeout(500); // wasm init

const canvasPixel = (x, y) =>
  page.evaluate(([x, y]) => {
    const c = document.getElementById("engine-canvas");
    return Array.from(c.getContext("2d").getImageData(x, y, 1, 1).data);
  }, [x, y]);

const assert = (cond, msg) => {
  if (!cond) throw new Error("FAIL: " + msg);
  console.log("ok:", msg);
};

// 1. Empty document: canvas transparent, empty-state hint shown.
assert((await canvasPixel(100, 100))[3] === 0, "empty doc renders transparent");
assert(await page.isVisible("text=Drag on the canvas"), "empty layers hint");

// 2. Draw a rect with the Rect tool.
await page.click('button[title="Rect"]');
const box = await page.locator("#engine-canvas").boundingBox();
const sx = box.width / 1280, sy = box.height / 720;
const drag = async (x0, y0, x1, y1) => {
  await page.mouse.move(box.x + x0 * sx, box.y + y0 * sy);
  await page.mouse.down();
  await page.mouse.move(box.x + x1 * sx, box.y + y1 * sy, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(200);
};
await drag(100, 100, 400, 300);
let px = await canvasPixel(200, 200);
assert(px[3] === 255 && px[2] > px[0], "rect rendered with blue-ish fill");
assert(await page.isVisible("text=Rect 1"), "layer row appeared");

// 2b. Gradient fills: the rect alone on the canvas, so the ramp is readable.
await page.locator(".panel ul li", { hasText: "Rect 1" }).click();
await page.waitForTimeout(150);
await page.selectOption('[aria-label="Fill type"]', "linear");
await page.waitForTimeout(200);
const left = await canvasPixel(115, 200);
const right = await canvasPixel(385, 200);
assert(
  right[0] > left[0] + 60,
  `linear gradient ramps left to right (${left} -> ${right})`,
);
assert(left[3] === 255 && right[3] === 255, "gradient fill stays opaque");

// The angle slider re-aims the ramp: at 90 degrees it runs top to bottom,
// so a row becomes flat and a column ramps instead.
await page.locator('input[aria-label="Gradient angle"]').evaluate((el) => {
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value",
  ).set;
  setter.call(el, "90");
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("pointerup", { bubbles: true }));
});
await page.waitForTimeout(200);
const top = await canvasPixel(250, 115);
const bottom = await canvasPixel(250, 285);
assert(
  bottom[0] > top[0] + 60,
  `angle 90 ramps top to bottom (${top} -> ${bottom})`,
);
const rowL = await canvasPixel(115, 200);
const rowR = await canvasPixel(385, 200);
assert(
  Math.abs(rowL[0] - rowR[0]) < 4,
  `and a row is now flat (${rowL} vs ${rowR})`,
);

// Radial: first stop at the centre, last at the rim.
await page.selectOption('[aria-label="Fill type"]', "radial");
await page.waitForTimeout(200);
const middle = await canvasPixel(250, 200);
const corner = await canvasPixel(115, 115);
assert(
  corner[0] > middle[0] + 60,
  `radial ramps outward from the centre (${middle} -> ${corner})`,
);

// Back to a flat fill: the shape is uniform again and later steps see the
// same rect they always did.
await page.selectOption('[aria-label="Fill type"]', "solid");
await page.waitForTimeout(200);
const flatA = await canvasPixel(115, 200);
const flatB = await canvasPixel(385, 200);
assert(
  JSON.stringify(flatA) === JSON.stringify(flatB),
  `solid fill is uniform again (${flatA} vs ${flatB})`,
);
// Undo the four fill-type changes so history is where the rest expects it.
for (let i = 0; i < 4; i++) await page.keyboard.press("Control+z");
await page.waitForTimeout(200);

// 3. Draw an ellipse overlapping it.
await page.click('button[title="Ellipse"]');
await drag(300, 150, 700, 500);
assert(await page.isVisible("text=Ellipse 2"), "second layer row");

// 3b. A curved edge cannot land on pixel boundaries, so the rim has to carry
// partial coverage — this is the rasterizer's anti-aliasing, live in the app.
// Probe where the edge runs diagonally (x=641 is ~45 degrees round the rim);
// straight across the top the curve is locally flat and lands on the grid.
const rim = [];
for (let y = 190; y <= 215; y++) rim.push((await canvasPixel(641, y))[3]);
assert(
  rim.some((a) => a > 8 && a < 247),
  `ellipse rim is antialiased (alphas ${rim.join(",")})`,
);
assert(rim[0] === 0 && rim[rim.length - 1] === 255, "rim scan crosses the edge");

// 4. Add an exposure adjustment (neutral), then edit its stops in the
// properties panel — the pixel brightens via the re-editable layer.
const before = await canvasPixel(500, 325);
await page.selectOption('[aria-label="Add adjustment layer"]', "exposure");
await page.waitForTimeout(200);
assert((await page.locator(".panel ul li", { hasText: "Exposure" }).count()) === 1, "adjustment layer row appeared");
let after = await canvasPixel(500, 325);
assert(after[1] === before[1], "neutral exposure changes nothing");
await page.locator(".panel ul li", { hasText: "Exposure" }).click();
const setSlider = async (label, value) => {
  await page.locator(`input[aria-label="${label}"]`).evaluate((el, v) => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    ).set;
    setter.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  }, value);
  await page.waitForTimeout(150);
};
await setSlider("Stops", 1);
after = await canvasPixel(500, 325);
assert(after[1] > before[1], `editing exposure brightened pixel (g ${before[1]} -> ${after[1]})`);

// 5. Undo three times: stops edit, adjustment layer, ellipse all revert.
await page.keyboard.press("Control+z");
await page.waitForTimeout(100);
after = await canvasPixel(500, 325);
assert(after[1] === before[1], "undo reverted the stops edit (one gesture)");
await page.keyboard.press("Control+z");
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert((await page.locator(".panel ul li", { hasText: "Ellipse 2" }).count()) === 0, "undo removed ellipse layer");
px = await canvasPixel(650, 450);
assert(px[3] === 0, "undone ellipse no longer rendered");

// 6. Redo brings the ellipse back.
await page.keyboard.press("Control+Shift+z");
await page.waitForTimeout(200);
assert((await page.locator(".panel ul li", { hasText: "Ellipse 2" }).count()) === 1, "redo restored ellipse");

// 7. Move tool: drag the rect, top-left corner vacates.
await page.click('button[title="Move"]');
await drag(150, 150, 350, 350); // grabs rect (ellipse doesn't cover 150,150)
px = await canvasPixel(101, 101);
assert(px[3] === 0, "moved rect vacated original corner");

// 8. Hide ellipse via layers panel.
const row = page.locator(".panel ul li", { hasText: "Ellipse 2" });
await row.locator(".visibility").click();
await page.waitForTimeout(200);
px = await canvasPixel(650, 250); // inside ellipse, outside the moved rect
assert(px[3] === 0, "hidden ellipse not rendered");

// 8b. Blend mode picker: set ellipse to Multiply over the moved rect.
await row.locator(".visibility").click(); // unhide again
await page.waitForTimeout(150);
await row.click(); // select ellipse
const overlapBefore = await canvasPixel(500, 400); // rect(300-600,300-500) ∩ ellipse
await page.selectOption('select[aria-label="Blend mode"]', "Multiply");
await page.waitForTimeout(150);
const overlapAfter = await canvasPixel(500, 400);
assert(
  overlapAfter[1] < overlapBefore[1],
  `multiply darkened overlap (g ${overlapBefore[1]} -> ${overlapAfter[1]})`,
);

// 8c. Opacity slider via keyboard: lower ellipse opacity, alpha-blend shifts.
const slider = page.locator('input[aria-label="Layer opacity"]');
await slider.focus();
for (let i = 0; i < 30; i++) await page.keyboard.press("ArrowLeft");
await page.waitForTimeout(150);
const faded = await canvasPixel(650, 250); // ellipse-only region
assert(faded[3] < 255, `lowered opacity fades ellipse (a=${faded[3]})`);

// 8d. Reorder: raise the (bottom) rect above the ellipse with the ↑ button.
await page.locator(".panel ul li", { hasText: "Rect 1" }).click();
const beforeRaise = await canvasPixel(500, 400);
await page.click('button[title="Raise layer"]');
await page.waitForTimeout(150);
const afterRaise = await canvasPixel(500, 400);
assert(
  JSON.stringify(beforeRaise) !== JSON.stringify(afterRaise),
  "raising rect above semi-transparent multiply ellipse changed overlap",
);

// 8e. Place image: green 4×4 PNG appears as a raster layer at the origin.
const pngB64 = await page.evaluate(() => {
  const c = document.createElement("canvas");
  c.width = 4;
  c.height = 4;
  const g = c.getContext("2d");
  g.fillStyle = "#00ff00";
  g.fillRect(0, 0, 4, 4);
  return c.toDataURL("image/png").split(",")[1];
});
await page.setInputFiles('input[accept="image/png,image/jpeg"]', {
  name: "green.png",
  mimeType: "image/png",
  buffer: Buffer.from(pngB64, "base64"),
});
await page.waitForTimeout(300);
assert(await page.isVisible("text=green.png"), "raster layer row appeared");
px = await canvasPixel(2, 2);
assert(px[1] === 255 && px[0] === 0, "placed image pixels rendered");

await page.screenshot({ path: join(OUT, "editor2.png") });
// 8f. Undo removes the placed image again (one undo step).
await page.keyboard.press("Control+z");
await page.waitForTimeout(150);
assert((await page.locator(".panel ul li", { hasText: "green.png" }).count()) === 0, "undo removed placed image");

// 8g. Wheel zoom shrinks/grows the on-screen canvas.
const boxBefore = await page.locator("#engine-canvas").boundingBox();
await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
await page.mouse.wheel(0, -400);
await page.waitForTimeout(150);
const boxAfter = await page.locator("#engine-canvas").boundingBox();
assert(boxAfter.width > boxBefore.width * 1.05, "wheel zoom enlarged canvas");
await page.click('button[title="Fit document to window"]');
await page.waitForTimeout(150);

// 8h. Live move preview: pixels move BEFORE mouseup; Escape cancels.
// State: opaque rect on top spans (300,300)-(600,500). Probe (310,480) is
// inside the rect but outside the ellipse.
await page.click('button[title="Move"]');
const box2 = await page.locator("#engine-canvas").boundingBox();
const sx2 = box2.width / 1280, sy2 = box2.height / 720;
const toScreen = (x, y) => [box2.x + x * sx2, box2.y + y * sy2];
px = await canvasPixel(310, 480);
assert(px[3] === 255, "probe starts on the rect");
await page.mouse.move(...toScreen(450, 400));
await page.mouse.down();
await page.mouse.move(...toScreen(550, 400), { steps: 6 });
await page.waitForTimeout(150);
px = await canvasPixel(310, 480);
assert(px[3] === 0, "mid-drag: rect visibly moved before mouseup (live preview)");
await page.keyboard.press("Escape");
await page.waitForTimeout(150);
await page.mouse.up();
await page.waitForTimeout(150);
px = await canvasPixel(310, 480);
assert(px[3] === 255, "Escape cancelled the drag, rect back in place");

// 8i. Resize via the se handle: rect grows past its old right edge.
await page.mouse.click(...toScreen(450, 400)); // select rect
await page.waitForTimeout(150);
const seHandle = await page.locator(".handle.se").boundingBox();
assert(seHandle, "resize handles visible for selection");
px = await canvasPixel(650, 530);
assert(px[3] === 0, "target growth area starts empty");
await page.mouse.move(seHandle.x + 5, seHandle.y + 5);
await page.mouse.down();
await page.mouse.move(...toScreen(700, 550), { steps: 6 });
await page.mouse.up();
await page.waitForTimeout(200);
px = await canvasPixel(650, 530);
assert(px[3] === 255, "rect scaled up through the corner handle");
await page.keyboard.press("Control+z");
await page.waitForTimeout(150);
px = await canvasPixel(650, 530);
assert(px[3] === 0, "resize is a single undo step");

// 8j. Edit the selected rect's fill color through the properties panel.
await page.mouse.click(...toScreen(450, 400)); // reselect rect
await page.waitForTimeout(150);
const setColor = async (label, hex) => {
  await page.locator(`input[aria-label="${label}"]`).evaluate((el, v) => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    ).set;
    setter.call(el, v);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  }, hex);
  await page.waitForTimeout(150);
};
await setColor("Fill color", "#ff0000");
px = await canvasPixel(450, 480); // inside rect, outside ellipse
assert(px[0] === 255 && px[2] < 60, "fill recolored to red via properties");

// 8k. Enable a stroke; the border band paints dark.
await page.check('input[aria-label="Stroke enabled"]');
await page.waitForTimeout(200);
px = await canvasPixel(302, 400); // 2px inside the rect's left edge
assert(px[3] === 255 && px[0] < 80, `stroke band painted (got ${px})`);

await page.screenshot({ path: join(OUT, "editor3.png") });
// 8l. Rename via double-click.
await page.locator(".panel ul li", { hasText: "Rect 1" }).locator("span").first().dblclick();
await page.locator('input[aria-label="Layer name"]').fill("Hero");
await page.keyboard.press("Enter");
await page.waitForTimeout(150);
assert(await page.isVisible("text=Hero"), "layer renamed inline");

// 8l2. Masks: inscribed ellipse mask hides the rect's corners, invert
// flips it, remove restores — all non-destructive.
px = await canvasPixel(599, 499); // rect's bottom-right corner, outside ellipse
assert(px[3] === 255, "rect corner visible before mask");
await page.click("text=Add ellipse mask");
await page.waitForTimeout(200);
px = await canvasPixel(599, 499);
assert(px[3] === 0, "ellipse mask hides the rect corner");
px = await canvasPixel(450, 400);
assert(px[0] === 255, "rect center still visible through mask");
await page.check('input[aria-label="Invert mask"]');
await page.waitForTimeout(200);
px = await canvasPixel(450, 400);
assert(px[0] !== 255 || px[3] !== 255, "inverted mask hides the center");
await page.click("text=Remove");
await page.waitForTimeout(200);
px = await canvasPixel(599, 499);
assert(px[3] === 255, "removing the mask restores the corner");

// 8m. Blur filter layer: content bleeds past shape edges; sigma editable.
px = await canvasPixel(605, 505); // just outside the rect's corner
assert(px[3] === 0, "corner region clear before blur");
await page.selectOption('[aria-label="Add adjustment layer"]', "blur");
await page.waitForTimeout(300);
px = await canvasPixel(605, 505);
assert(px[3] > 0, `blur bled past the rect corner (a=${px[3]})`);
await page.locator(".panel ul li", { hasText: "Gaussian Blur" }).click();
await setSlider("Blur sigma", 14);
px = await canvasPixel(618, 518); // farther out
assert(px[3] > 0, `larger sigma reaches farther (a=${px[3]})`);
await page.keyboard.press("Control+z"); // sigma edit
await page.keyboard.press("Control+z"); // blur layer itself
await page.waitForTimeout(300);
px = await canvasPixel(605, 505);
assert(px[3] === 0, "undo removed the blur layer");

// 8n. Pen tool: click a triangle closed -> filled path object.
await page.click('button[title="Pen"]');
const penClick = async (x, y) => {
  await page.mouse.click(box.x + x * sx, box.y + y * sy);
  await page.waitForTimeout(80);
};
await penClick(50, 20);
await penClick(250, 20);
await penClick(150, 140);
await penClick(51, 21); // near the first anchor -> closes
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 1,
  "closed pen path became a layer",
);
px = await canvasPixel(150, 60);
assert(px[3] === 255 && px[2] > px[0], "triangle interior filled");
px = await canvasPixel(60, 120);
assert(px[3] === 0, "outside the triangle stays empty");

// 8o. Pen tool: Enter finishes an open path as stroked line art.
await penClick(400, 20);
await penClick(500, 120);
await penClick(600, 20);
await page.keyboard.press("Enter");
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 2,
  "open pen path became a layer",
);
px = await canvasPixel(500, 118);
assert(px[3] === 255, "polyline vertex stroked");
px = await canvasPixel(500, 60);
assert(px[3] === 0, "open path interior not filled");

// 8p. Escape abandons an in-progress pen path.
await penClick(700, 40);
await penClick(760, 100);
await page.keyboard.press("Escape");
await page.waitForTimeout(150);
await page.click('button[title="Move"]');
assert(
  (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 2,
  "escape discarded the in-progress path",
);

// 8q. Group the two pen paths (ctrl-click multi-select), one undo ungroups.
await page.locator(".panel ul li", { hasText: "Path" }).first().click();
await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({ modifiers: ["Control"] });
await page.click('button[title="Group selected layers (ctrl-click to select several)"]');
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Group 1" }).count()) === 1,
  "group layer created",
);
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Group 1" }).count()) === 0 &&
    (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 2,
  "one undo dissolved the whole grouping",
);

// 8r. Ungroup via the button.
await page.locator(".panel ul li", { hasText: "Path" }).first().click();
await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({ modifiers: ["Control"] });
await page.click('button[title="Group selected layers (ctrl-click to select several)"]');
await page.waitForTimeout(200);
await page.locator(".panel ul li", { hasText: "Group 2" }).click();
await page.click('button[title="Ungroup selected group"]');
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Group 2" }).count()) === 0 &&
    (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 2,
  "ungroup dissolved the group",
);

// 8s. History panel: jump to the first edit and back to the newest.
assert(await page.isVisible("text=History"), "history panel present");
const firstEntry = page.locator(".history button").first();
assert(
  (await firstEntry.textContent()).includes("Add Rect 1"),
  "oldest entry is the first draw",
);
await firstEntry.click();
await page.waitForTimeout(300);
px = await canvasPixel(650, 250);
assert(px[3] === 0, "jumped back: ellipse never drawn yet");
px = await canvasPixel(200, 200);
assert(px[3] === 255, "jumped back: first rect present at origin position");
await page.locator(".history button").last().click();
await page.waitForTimeout(300);
assert(
  (await page.locator(".panel ul li", { hasText: "Path" }).count()) === 2,
  "jumped forward: full timeline restored",
);

// 8t. Anchor editing: drag the triangle's apex; one undo reverts it.
await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click(); // triangle
await page.waitForTimeout(200);
assert(
  (await page.locator(".anchor").count()) === 3,
  "three anchor handles shown for the triangle",
);
px = await canvasPixel(150, 180);
assert(px[3] === 0, "below the apex starts empty");
const apex = await page.locator('[data-anchor="2"]').boundingBox();
await page.mouse.move(apex.x + 5, apex.y + 5);
await page.mouse.down();
await page.mouse.move(box.x + 150 * sx, box.y + 220 * sy, { steps: 6 });
await page.mouse.up();
await page.waitForTimeout(200);
px = await canvasPixel(150, 180);
assert(px[3] === 255, "dragged apex stretched the triangle");
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
px = await canvasPixel(150, 180);
assert(px[3] === 0, "anchor drag is one undo step");

// 8u. Smooth toggle bows the path outside its straight chords.
px = await canvasPixel(92, 80); // just left of the sharp left edge
assert(px[3] === 0, "outside the straight edge before smoothing");
await page.check('input[aria-label="Smooth path"]');
await page.waitForTimeout(200);
px = await canvasPixel(92, 80);
assert(px[3] > 0, "smooth spline bows past the chord");
await page.uncheck('input[aria-label="Smooth path"]');
await page.waitForTimeout(150);

// 8u2. Curve handles: converting keeps the shape and hands over its
// controls, then dragging a handle bends the outline where it was straight.
const straightAt = await canvasPixel(92, 80);
assert(straightAt[3] === 0, "still straight before converting");
assert(
  (await page.locator(".curve-handle").count()) === 0,
  "a straight path shows no curve handles",
);
await page.click("text=Convert to curves");
await page.waitForTimeout(200);
const handleCount = await page.locator(".curve-handle").count();
assert(handleCount > 0, `converting exposes handles (${handleCount})`);
assert(
  (await canvasPixel(92, 80))[3] === 0,
  "converting hands over controls without changing the shape",
);
assert(
  await page.isDisabled('input[aria-label="Smooth path"]'),
  "smooth is disabled once explicit handles define the shape",
);

// Drag one handle far to the left; the outline follows it out there.
const handle = page.locator(".curve-handle").first();
const hbox = await handle.boundingBox();
await page.mouse.move(hbox.x + 4, hbox.y + 4);
await page.mouse.down();
await page.mouse.move(hbox.x - 120, hbox.y + 40, { steps: 6 });
await page.mouse.up();
await page.waitForTimeout(200);
const bent = await canvasPixel(92, 80);
assert(bent[3] > 0, `dragging a handle bent the outline (got ${bent})`);

// One undo step for the whole drag, one more for the conversion.
await page.keyboard.press("Control+z");
await page.waitForTimeout(150);
assert(
  (await canvasPixel(92, 80))[3] === 0,
  "the handle drag undoes as a single step",
);
await page.keyboard.press("Control+z");
await page.waitForTimeout(150);
assert(
  (await page.locator(".curve-handle").count()) === 0,
  "and the conversion undoes too",
);

// 8v. Text tool: click to add a live text object, edit it via the panel.
const inkCount = (x0, y0, x1, y1) =>
  page.evaluate(([a, b, c, d]) => {
    const ctx = document.getElementById("engine-canvas").getContext("2d");
    const img = ctx.getImageData(a, b, c - a, d - b).data;
    let n = 0;
    for (let i = 3; i < img.length; i += 4) if (img[i] > 0) n++;
    return n;
  }, [x0, y0, x1, y1]);

assert((await inkCount(740, 590, 1100, 710)) === 0, "text target area empty");
await page.click('button[title="Text"]');
await page.mouse.click(box.x + 750 * sx, box.y + 600 * sy);
await page.waitForTimeout(300);
assert(
  (await page.locator(".panel ul li", { hasText: "Text" }).count()) === 1,
  "text layer row appeared",
);
const inkDefault = await inkCount(740, 590, 1100, 710);
assert(inkDefault > 50, `glyph ink rendered (${inkDefault} px)`);

// Select it and edit content + size through the properties panel.
await page.mouse.click(box.x + 760 * sx, box.y + 620 * sy); // Move tool auto-active
await page.waitForTimeout(200);
await page.locator('textarea[aria-label="Text content"]').evaluate((el) => {
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLTextAreaElement.prototype,
    "value",
  ).set;
  setter.call(el, "Hello!");
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
});
await page.waitForTimeout(300);
const inkHello = await inkCount(740, 590, 1100, 710);
assert(
  inkHello !== inkDefault && inkHello > 50,
  `edited text re-rendered (${inkDefault} -> ${inkHello} px)`,
);
await setSlider("Size", 96);
const inkBig = await inkCount(740, 560, 1280, 720);
assert(inkBig > inkHello * 1.5, `larger size grew the ink (${inkHello} -> ${inkBig})`);
await page.keyboard.press("Control+z"); // size gesture
await page.waitForTimeout(200);
assert(
  (await inkCount(740, 590, 1100, 710)) === inkHello,
  "size change was one undo step",
);

await page.screenshot({ path: join(OUT, "editor5.png") });

// 8w. Export SVG: the download carries live vector markup.
const [svgDl] = await Promise.all([
  page.waitForEvent("download"),
  page.click("text=Export SVG"),
]);
const svgPath = await svgDl.path();
const svgText = await readFile(svgPath, "utf8");
assert(svgText.startsWith("<svg "), "SVG root element");
assert(svgText.includes("<rect") && svgText.includes("<path"), "shapes exported as vectors");
assert(svgText.includes("<text") && svgText.includes("Hello!"), "text exported live");
console.log("ok: SVG export contains live vector markup");

// 9. CMYK doc smoke: new doc, draw, still renders.
await page.click("text=New CMYK");
await page.click('button[title="Rect"]');
await drag(50, 50, 200, 200);
px = await canvasPixel(100, 100);
assert(px[3] === 255, "CMYK document renders shapes");
assert(await page.isVisible("text=CMYK, 1280"), "CMYK mode shown in title");

// 9b. Loading a CMYK press profile changes how authored ink renders.
// (Requires CHITRAKAR_TEST_CMYK_ICC; profiles aren't license-clean to commit.)
const iccPath = process.env.CHITRAKAR_TEST_CMYK_ICC;
if (!iccPath) {
  console.log("skipped: CMYK press profile + soft proofing steps (set CHITRAKAR_TEST_CMYK_ICC)");
} else {
const iccBytes = await readFile(iccPath);
const naivePx = await canvasPixel(100, 100);
await page.setInputFiles('input[accept=".icc,.icm"]', {
  name: "swop.icc",
  mimeType: "application/vnd.iccprofile",
  buffer: iccBytes,
});
await page.waitForTimeout(400);
assert(await page.isVisible("text=ICC ✓"), "profile accepted and indicated");
const profiledPx = await canvasPixel(100, 100);
assert(
  JSON.stringify(naivePx) !== JSON.stringify(profiledPx),
  `press profile changed CMYK rendering (${naivePx} -> ${profiledPx})`,
);

// 9c. CMYK fills expose ink sliders; cranking K darkens the shape.
await page.click('button[title="Move"]');
await page.mouse.click(box.x + 100 * sx, box.y + 100 * sy);
await page.waitForTimeout(150);
assert(
  (await page.locator('input[aria-label="K ink"]').count()) === 1,
  "CMYK fill shows ink sliders",
);
await setSlider("K ink", 1);
px = await canvasPixel(100, 100);
assert(
  px[0] < 80 && px[1] < 80 && px[2] < 80,
  `100% K renders near-black through the press profile (${px})`,
);

// 9d. Soft proofing on an RGB document with the same profile.
await page.click("text=New RGB");
await page.waitForTimeout(200);
await page.setInputFiles('input[accept=".icc,.icm"]', {
  name: "swop.icc",
  mimeType: "application/vnd.iccprofile",
  buffer: iccBytes,
});
await page.waitForTimeout(300);
await page.locator(".fill-swatch").evaluate((el) => {
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value",
  ).set;
  setter.call(el, "#0000ff");
  el.dispatchEvent(new Event("input", { bubbles: true }));
});
await page.click('button[title="Rect"]');
await drag(50, 50, 200, 200);
px = await canvasPixel(100, 100);
assert(px[2] === 255 && px[0] === 0, "pure blue before proofing");
await page.click('button[title="Soft proof: preview what the press can reproduce"]');
await page.waitForTimeout(300);
px = await canvasPixel(100, 100);
assert(px[2] < 255 || px[0] > 0, `proofing shifted unprintable blue (${px})`);
await page.click('button[title="Mark out-of-gamut pixels grey"]');
await page.waitForTimeout(300);
px = await canvasPixel(100, 100);
assert(
  px[0] === 128 && px[1] === 128 && px[2] === 128,
  `gamut warning marks pure blue grey (${px})`,
);
await page.screenshot({ path: join(OUT, "editor4.png") });
// Export must be unproofed: proofing is display-only.
await page.click('button[title="Soft proof: preview what the press can reproduce"]');
await page.waitForTimeout(300);
px = await canvasPixel(100, 100);
assert(px[2] === 255, "proof off restores true pixels");

// 9e. Print handoff: CMYK TIFF export separated through the profile.
const [tiffDl] = await Promise.all([
  page.waitForEvent("download"),
  page.click("text=Export TIFF"),
]);
const tiffBytes = await readFile(await tiffDl.path());
const marker = tiffBytes.subarray(0, 2).toString("latin1");
assert(marker === "II" || marker === "MM", `TIFF byte-order marker (${marker})`);
assert(tiffBytes.includes(iccBytes.subarray(0, 256)), "press profile embedded in TIFF");
assert(tiffBytes.length > 10000, `TIFF carries pixel data (${tiffBytes.length} bytes)`);

// 9f. PDF export, CMYK-separated because a press profile is loaded.
const [pdfDl] = await Promise.all([
  page.waitForEvent("download"),
  page.click("text=Export PDF"),
]);
const pdfBytes = await readFile(await pdfDl.path());
assert(pdfBytes.subarray(0, 8).toString("latin1") === "%PDF-1.7", "PDF header");
assert(pdfBytes.subarray(-6).toString("latin1") === "%%EOF\n", "PDF trailer");
const pdfText = pdfBytes.toString("latin1");
assert(pdfText.includes("/ICCBased"), "PDF carries an ICC colorspace");
assert(pdfText.includes("/N 4"), "PDF image is 4-channel CMYK");
}

await page.screenshot({ path: join(OUT, "editor-final.png") });
assert(errors.length === 0, "no page errors: " + JSON.stringify(errors));

console.log("\nALL SMOKE TESTS PASSED");
await browser.close();
server.close();
process.exit(0);
