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

// Probes are written in document pixels. The canvas backing store holds
// `data-frame-scale` device pixels for each of them (it rises with zoom so
// magnified artwork is re-rendered rather than enlarged), so every read
// converts through it.
const canvasPixel = (x, y) =>
  page.evaluate(([x, y]) => {
    const c = document.getElementById("engine-canvas");
    const s = Number(c.dataset.frameScale) || 1;
    const dx = Math.min(c.width - 1, Math.floor((x + 0.5) * s));
    const dy = Math.min(c.height - 1, Math.floor((y + 0.5) * s));
    return Array.from(c.getContext("2d").getImageData(dx, dy, 1, 1).data);
  }, [x, y]);

const assert = (cond, msg) => {
  if (!cond) throw new Error("FAIL: " + msg);
  console.log("ok:", msg);
};

/** Document actions live in the menu bar, so reaching one means opening its
 * menu first. Returns the item's locator without clicking, since some steps
 * need to race the click against a download. */
const menuItem = async (menu, item) => {
  await page.click(`.menu-label:text-is("${menu}")`);
  await page.waitForTimeout(120);
  return page.locator(".menu-item", { hasText: item });
};
/** Create a document through the New-document dialog. */
const newDocument = async (w, h, mode) => {
  await menuClick("File", "New document…");
  await page.locator('input[aria-label="Width"]').fill(String(w));
  await page.locator('input[aria-label="Height"]').fill(String(h));
  await page.selectOption('[aria-label="Colour mode"]', mode);
  await page.click("text=Create");
  await page.waitForTimeout(400);
};

const menuClick = async (menu, item) => {
  (await menuItem(menu, item)).click();
  await page.waitForTimeout(200);
};

// 1. Empty document: canvas transparent, empty-state hint shown.
assert((await canvasPixel(100, 100))[3] === 0, "empty doc renders transparent");
assert(await page.isVisible("text=Drag on the canvas"), "empty layers hint");

// 1b. Chrome: tools are glyphs, document actions live in menus, and a menu
// closes the ways a menu is expected to.
assert(
  (await page.locator('button[aria-label="Rect"] svg').count()) === 1,
  "tools render an icon, not a letter",
);
assert(
  (await page.locator(".topbar .menu-label").allTextContents()).join(",") ===
    "File,Edit,View",
  "menu bar carries File, Edit and View",
);
await page.click('.menu-label:text-is("File")');
await page.waitForTimeout(120);
assert(await page.isVisible("text=Export PNG"), "File menu holds the exports");
assert(
  await page.isVisible("text=New document…"),
  "and the document actions that used to crowd the bar",
);
// Hovering a neighbour switches menus once one is open.
await page.hover('.menu-label:text-is("View")');
await page.waitForTimeout(120);
assert(
  await page.isVisible("text=Fit document to window"),
  "hovering across the bar switches menus",
);
await page.keyboard.press("Escape");
await page.waitForTimeout(120);
assert(
  (await page.locator(".menu-pop").count()) === 0,
  "escape closes the open menu",
);

// 1c. Tool shortcuts: a bare letter switches tools, but not while typing —
// a text layer's content and a layer's name both contain those letters.
await page.keyboard.press("e");
await page.waitForTimeout(100);
assert(
  await page.locator('button[aria-label="Ellipse"]').evaluate((el) =>
    el.classList.contains("active"),
  ),
  "E selects the ellipse tool",
);
await page.keyboard.press("v");
await page.waitForTimeout(100);
assert(
  await page.locator('button[aria-label="Move"]').evaluate((el) =>
    el.classList.contains("active"),
  ),
  "V returns to move",
);

// 2. Draw a rect with the Rect tool.
await page.click('button[aria-label="Rect"]');
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

// Multi-stop: adding one changes nothing until it is moved, then it
// recolours the middle of the ramp while both ends stay put.
const midBefore = await canvasPixel(250, 200);
await page.click("text=Add stop");
await page.waitForTimeout(200);
assert(
  (await page.locator('input[aria-label^="Gradient stop"][type="color"]').count()) === 3,
  "a third stop appeared",
);
assert(
  (await page.locator(".grad-stop").count()) === 1,
  "and it gets a knob on the canvas between the two ends",
);
const midNow = await canvasPixel(250, 200);
assert(
  midNow.every((v, i) => Math.abs(v - midBefore[i]) <= 1),
  `adding a stop leaves the ramp as it was (${midBefore} -> ${midNow})`,
);
await page.locator('input[aria-label="Gradient stop 2"]').evaluate((el) => {
  const set = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value",
  ).set;
  set.call(el, "#00ff00");
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
});
await page.waitForTimeout(250);
const midAfter = await canvasPixel(250, 200);
assert(
  midAfter[1] > midAfter[0] && midAfter[1] > midAfter[2],
  `the middle stop recoloured the middle of the ramp (${midAfter})`,
);
// The ramp runs top-to-bottom here, so the untouched end is the top edge.
const endStill = await canvasPixel(250, 115);
assert(
  endStill[2] > endStill[1],
  `while the first stop stayed where it was (${endStill})`,
);
await page.click('button[aria-label="Remove gradient stop 2"]');
await page.waitForTimeout(250);
assert(
  (await page.locator('input[aria-label^="Gradient stop"][type="color"]').count()) === 2,
  "and removing it puts the ramp back to two stops",
);
assert(
  await page.isDisabled('button[aria-label="Remove gradient stop 1"]'),
  "the last two stops cannot be removed",
);

// The ramp is also draggable where it is seen: the line and its knobs sit
// on the canvas, and dragging an end re-aims the gradient.
assert(
  (await page.locator(".grad-handle").count()) === 2,
  "a linear gradient shows an end knob at each end",
);
const beforeAim = await canvasPixel(115, 200);
const fromKnob = page.locator('[data-grad="from"]');
const fk = await fromKnob.boundingBox();
await fromKnob.hover();
await page.mouse.down();
await page.mouse.move(fk.x + 160, fk.y + 5, { steps: 6 });
await page.mouse.up();
await page.waitForTimeout(250);
const afterAim = await canvasPixel(115, 200);
assert(
  JSON.stringify(afterAim) !== JSON.stringify(beforeAim),
  `dragging the start knob re-aimed the ramp (${beforeAim} -> ${afterAim})`,
);
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert(
  JSON.stringify(await canvasPixel(115, 200)) === JSON.stringify(beforeAim),
  "and the drag undoes as one step",
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
// No undo here: switching back to Solid already restored the fill the rest
// of the suite draws on, and a fixed undo count would drift every time this
// step grows.

// 3. Draw an ellipse overlapping it.
await page.click('button[aria-label="Ellipse"]');
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
await page.click('button[aria-label="Move"]');
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
const pixelBeforeZoom = await canvasPixel(310, 480);
await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
await page.mouse.wheel(0, -400);
await page.waitForTimeout(150);
const boxAfter = await page.locator("#engine-canvas").boundingBox();
assert(boxAfter.width > boxBefore.width * 1.05, "wheel zoom enlarged canvas");
// And it enlarged the artwork, not its pixels: the engine renders more of
// them, so the backing store grows with the zoom rather than being
// stretched to fill the larger box.
const zoomedStore = await page.$eval("#engine-canvas", (c) => [
  c.width,
  Number(c.dataset.frameScale),
]);
assert(
  zoomedStore[1] > 1 && zoomedStore[0] > 1280,
  `zooming in raised the render resolution (${zoomedStore})`,
);
// The picture is still the same picture at the new resolution.
{
  const px = await canvasPixel(310, 480);
  assert(
    px.every((v, i) => Math.abs(v - pixelBeforeZoom[i]) <= 4),
    `and the same document pixel still reads the same (${px} vs ${pixelBeforeZoom})`,
  );
}
await menuClick("View", "Fit document to window");
await page.waitForTimeout(150);

// 8h. Live move preview: pixels move BEFORE mouseup; Escape cancels.
// State: opaque rect on top spans (300,300)-(600,500). Probe (310,480) is
// inside the rect but outside the ellipse.
await page.click('button[aria-label="Move"]');
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

// 8h2. Snapping: a drag that lands near an alignment line is pulled onto
// it, and a guide is shown while it holds. The rect is dragged toward the
// page centre but stopped a few document pixels short.
{
  const centreOf = async () => {
    const quad = await page.$eval(".sel-outline polygon", (el) =>
      el.getAttribute("points").split(" ").map((p) => p.split(",").map(Number)),
    );
    return quad.reduce((a, p) => a + p[0], 0) / quad.length;
  };
  await page.mouse.click(...toScreen(450, 400));
  await page.waitForTimeout(150);
  const before = await centreOf();
  await page.mouse.move(...toScreen(450, 400));
  await page.mouse.down();
  await page.mouse.move(...toScreen(637, 400), { steps: 8 });
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".snap-overlay line").count()) > 0,
    "a guide appears while the drag is snapped",
  );
  await page.mouse.up();
  await page.waitForTimeout(200);
  const moved = ((await centreOf()) - before) / sx2;
  assert(
    Math.abs(moved - 187) > 0.5 && Math.abs(moved - 187) < 9,
    `the drop was pulled onto the alignment line (moved ${moved} doc px, aimed at 187)`,
  );
  assert(
    (await page.locator(".snap-overlay line").count()) === 0,
    "and the guide clears when the drag ends",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    Math.abs((await centreOf()) - before) < 1,
    "the snapped move undoes as one step",
  );
}

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

// 8i2. Rotation: the knob above the selection turns the layer, and the
// whole turn is one undo step.
await page.mouse.click(...toScreen(450, 400)); // select the rect
await page.waitForTimeout(150);
const rot = page.locator(".rot-handle");
assert((await rot.count()) === 1, "selection offers a rotation handle");
// A corner the rect covers and the ellipse does not, so the probe reads the
// rect alone; a quarter turn about the selection centre vacates it.
const rectCorner = [590, 490];
assert((await canvasPixel(...rectCorner))[3] === 255, "corner filled before turning");
const rbox = await rot.boundingBox();
// Centre of the (now oriented) selection quad.
const quad = await page.$eval(".sel-outline polygon", (el) =>
  el.getAttribute("points").split(" ").map((p) => p.split(",").map(Number)),
);
const pivot = [
  quad.reduce((a, p) => a + p[0], 0) / quad.length,
  quad.reduce((a, p) => a + p[1], 0) / quad.length,
];
await rot.hover();
await page.mouse.down();
// Swing the knob a quarter turn about the selection centre.
await page.mouse.move(pivot[0] + (rbox.y + 6 - pivot[1]), pivot[1], { steps: 8 });
await page.mouse.up();
await page.waitForTimeout(250);
assert(
  (await canvasPixel(...rectCorner))[3] === 0,
  "the corner is vacated by the rotation",
);
// The selection box follows the layer's own axes rather than staying an
// axis-aligned box around it, so its top edge is no longer horizontal.
const turnedQuad = await page.$eval(".sel-outline polygon", (el) =>
  el.getAttribute("points").split(" ").map((p) => p.split(",").map(Number)),
);
assert(
  Math.abs(turnedQuad[0][1] - turnedQuad[1][1]) > 20,
  `the selection box turned with the layer (${JSON.stringify(turnedQuad)})`,
);
// And a corner handle sits on a corner of that quad, not of its bounding
// box. The polygon is in the canvas host's coordinates; boundingBox is in
// the viewport's, so shift one into the other before comparing.
const hostBox = await page.locator(".canvas-host").boundingBox();
const nw = await page.locator(".handle.nw").boundingBox();
assert(
  Math.abs(nw.x + nw.width / 2 - hostBox.x - turnedQuad[0][0]) < 2 &&
    Math.abs(nw.y + nw.height / 2 - hostBox.y - turnedQuad[0][1]) < 2,
  `handles sit on the turned corners (${nw.x + nw.width / 2 - hostBox.x},${
    nw.y + nw.height / 2 - hostBox.y
  } vs ${turnedQuad[0]})`,
);
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert(
  (await canvasPixel(...rectCorner))[3] === 255,
  "and the whole turn undoes as one step",
);

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
// 8k2. Duplicate and delete: the copy lands above the original, offset so
// it is visible, and Delete removes a layer from the keyboard.
{
  const before = await page.locator(".panel ul li").count();
  await page.keyboard.press("Control+d");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li").count()) === before + 1,
    "Ctrl+D added a layer",
  );
  assert(
    (await page.locator(".panel ul li", { hasText: "copy" }).count()) === 1,
    "and named it as a copy",
  );
  // The copy is selected and nudged clear of the original.
  const copyRow = page.locator(".panel ul li", { hasText: "copy" });
  assert(
    await copyRow.evaluate((el) => el.classList.contains("selected")),
    "the copy is what is selected afterwards",
  );
  await page.keyboard.press("Delete");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li").count()) === before &&
      (await page.locator(".panel ul li", { hasText: "copy" }).count()) === 0,
    "Delete removed it again",
  );
  // Both were single history steps.
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li", { hasText: "copy" }).count()) === 1,
    "undo brought the copy back in one step",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li", { hasText: "copy" }).count()) === 0,
    "and another undo removed the duplicate in one step",
  );
  // Put the selection back where the following steps expect it: deleting
  // cleared it, and undo restores nodes rather than what was selected.
  await page.locator(".panel ul li", { hasText: "Hero" }).or(
    page.locator(".panel ul li", { hasText: "Rect 1" }),
  ).first().click();
  await page.waitForTimeout(250);
}

// 8l. Rename via double-click.
await page.locator(".panel ul li", { hasText: "Rect 1" }).locator(".layer-name").dblclick();
await page.locator('input[aria-label="Layer name"]').fill("Hero");
await page.keyboard.press("Enter");
await page.waitForTimeout(150);
assert(await page.isVisible("text=Hero"), "layer renamed inline");
assert(
  await page.locator('button[aria-label="Move"]').evaluate((el) =>
    el.classList.contains("active"),
  ),
  "typing a layer name did not switch tools under the cursor",
);
assert(
  (await page.locator(".panel ul li .layer-kind-icon svg").count()) > 0,
  "layer rows carry a kind glyph",
);

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
// The mask is editable where it applies: drag it and the hole moves with
// it, as one undo step.
assert(
  (await page.locator(".mask-move").count()) === 1 &&
    (await page.locator(".mask-handle").count()) === 4,
  "the mask gets a move knob and corner handles",
);
{
  const knob = page.locator(".mask-move");
  const kb = await knob.boundingBox();
  await knob.hover();
  await page.mouse.down();
  await page.mouse.move(kb.x + kb.width / 2 + 140, kb.y + kb.height / 2 + 110, {
    steps: 6,
  });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert(
    (await canvasPixel(599, 499))[3] === 255,
    "moving the mask uncovered the corner it was hiding",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(599, 499))[3] === 0,
    "and the move undoes as one step",
  );
}
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
await page.click('button[aria-label="Pen"]');
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
await page.click('button[aria-label="Move"]');
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

// 8q2. Align: with two layers picked, the strip appears and lines them up.
{
  await page.locator(".panel ul li", { hasText: "Path" }).first().click();
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".align-bar").count()) === 0,
    "one layer offers nothing to align",
  );
  await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({
    modifiers: ["Control"],
  });
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".align-bar button").count()) === 8,
    "a multi-selection offers align and distribute",
  );
  // Where the two paths' left edges are before and after.
  const leftEdge = async (nth) => {
    await page.locator(".panel ul li", { hasText: "Path" }).nth(nth).click();
    await page.waitForTimeout(150);
    const q = await page.$eval(".sel-outline polygon", (el) =>
      el.getAttribute("points").split(" ").map((p) => Number(p.split(",")[0])),
    );
    return Math.min(...q);
  };
  const gap = async () => Math.abs((await leftEdge(0)) - (await leftEdge(1)));
  const beforeGap = await gap();
  assert(beforeGap > 5, `the two paths start at different edges (${beforeGap})`);
  await page.locator(".panel ul li", { hasText: "Path" }).first().click();
  await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({
    modifiers: ["Control"],
  });
  await page.waitForTimeout(150);
  await page.click('button[aria-label="Align left edges"]');
  await page.waitForTimeout(250);
  const afterGap = await gap();
  // Not exactly zero: alignment works on visual bounds, which include a
  // stroked path's overhang, while this probe reads the anchor outline —
  // so a stroked layer and a filled one settle a stroke-width apart.
  assert(
    afterGap < beforeGap * 0.05,
    `aligning brought their left edges together (${beforeGap} -> ${afterGap})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  const undone = await gap();
  assert(
    Math.abs(undone - beforeGap) < 2,
    "and the whole alignment undoes as one step",
  );
}

// 8q3. Arrow keys nudge whatever is picked — one layer, or a whole
// multi-selection moving together as one edit.
{
  const leftOf = async (nth) => {
    await page.locator(".panel ul li", { hasText: "Path" }).nth(nth).click();
    await page.waitForTimeout(150);
    const q = await page.$eval(".sel-outline polygon", (el) =>
      el.getAttribute("points").split(" ").map((p) => Number(p.split(",")[0])),
    );
    return Math.min(...q);
  };
  const start = [await leftOf(0), await leftOf(1)];
  await page.locator(".panel ul li", { hasText: "Path" }).first().click();
  await page.waitForTimeout(150);
  await page.keyboard.press("Shift+ArrowRight");
  await page.waitForTimeout(250);
  const one = [await leftOf(0), await leftOf(1)];
  assert(
    one[0] - start[0] > 5 && Math.abs(one[1] - start[1]) < 1,
    `shift-arrow nudged the picked layer alone (${start} -> ${one})`,
  );
  // Now both: one press, both move, one history entry.
  await page.locator(".panel ul li", { hasText: "Path" }).first().click();
  await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({
    modifiers: ["Control"],
  });
  await page.waitForTimeout(150);
  await page.keyboard.press("Shift+ArrowRight");
  await page.waitForTimeout(250);
  const both = [await leftOf(0), await leftOf(1)];
  assert(
    both[0] - one[0] > 5 && Math.abs(both[1] - one[1] - (both[0] - one[0])) < 1,
    `one press moved the whole selection by the same step (${one} -> ${both})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  const undone = [await leftOf(0), await leftOf(1)];
  assert(
    Math.abs(undone[0] - one[0]) < 1 && Math.abs(undone[1] - one[1]) < 1,
    `the whole nudge undoes as one step (${undone})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  const back = [await leftOf(0), await leftOf(1)];
  assert(
    Math.abs(back[0] - start[0]) < 1 && Math.abs(back[1] - start[1]) < 1,
    `and so does the single-layer one (${start} -> ${back})`,
  );
}

// 8r. Ungroup via the button.
await page.locator(".panel ul li", { hasText: "Path" }).first().click();
await page.locator(".panel ul li", { hasText: "Path" }).nth(1).click({ modifiers: ["Control"] });
await page.click('button[title="Group selected layers (ctrl-click to select several)"]');
await page.waitForTimeout(200);
await page.locator(".panel ul li", { hasText: "Group 2" }).click();
await page.waitForTimeout(200);

// 8r2. A group moves as a unit, and dissolving it leaves its contents put.
// A point on one of the paths, which the group carries with it.
const onPath = [150, 60];
assert((await canvasPixel(...onPath))[3] > 0, "path ink before moving the group");
assert(
  (await page.locator(".sel-outline polygon").count()) === 1,
  "a selected group gets a selection box of its own",
);
await drag(150, 60, 150, 170);
assert(
  (await canvasPixel(...onPath))[3] === 0,
  "the group's contents moved with it",
);
const movedProbe = [150, 170];
const afterMove = await canvasPixel(...movedProbe);
assert(afterMove[3] > 0, `and arrived at the new position (${afterMove})`);

// Put it back, so the rest of the suite sees the shapes where it left
// them. (That dissolving a moved group leaves its contents put is pinned
// by a native test, which can assert the transforms rather than pixels.)
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert(
  (await canvasPixel(...onPath))[3] > 0,
  "and the move undoes as one step",
);

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

// Handles come in pairs that point opposite ways through their anchor, so
// dragging one swings the other; Alt breaks that pairing.
// Work on whichever pair sits clear of the tool rail, since a handle dragged
// out to the left would be unclickable.
const pairIndex = await page.$$eval(".curve-handle", (els) => {
  const byAnchor = {};
  for (const el of els) {
    const [i, side] = el.dataset.handle.split("-");
    (byAnchor[i] ??= {})[side] = el.getBoundingClientRect().x;
  }
  let best = null;
  for (const [i, sides] of Object.entries(byAnchor)) {
    if (sides["0"] === undefined || sides["2"] === undefined) continue;
    const clearance = Math.min(sides["0"], sides["2"]);
    if (!best || clearance > best.clearance) best = { i, clearance };
  }
  return best.i;
});
const centre = async (sel) => {
  const b = await page.locator(sel).boundingBox();
  return [b.x + b.width / 2, b.y + b.height / 2];
};
/** How far each handle of the chosen anchor sits from the anchor itself. */
const arms = async () => {
  const a = await centre(`.anchor[data-anchor="${pairIndex}"]`);
  const i = await centre(`[data-handle="${pairIndex}-0"]`);
  const o = await centre(`[data-handle="${pairIndex}-2"]`);
  return [
    [i[0] - a[0], i[1] - a[1]],
    [o[0] - a[0], o[1] - a[1]],
  ];
};
const opposite = ([i, o]) =>
  Math.abs(i[0] + o[0]) < 3 && Math.abs(i[1] + o[1]) < 3;

const outHandle = page.locator(`[data-handle="${pairIndex}-2"]`);
let ob = await outHandle.boundingBox();
await outHandle.hover(); // actionability check: fail loudly if it is covered
await page.mouse.down();
await page.mouse.move(ob.x + ob.width / 2 + 40, ob.y + ob.height / 2 - 40, {
  steps: 5,
});
await page.mouse.up();
await page.waitForTimeout(250);
assert(
  opposite(await arms()),
  `dragging one handle keeps the pair opposite (${JSON.stringify(await arms())})`,
);

// Alt-drag its partner: it moves alone and the pair stops being mirrored.
const inHandle = page.locator(`[data-handle="${pairIndex}-0"]`);
const outBefore = (await arms())[1];
const ib = await inHandle.boundingBox();
await page.keyboard.down("Alt");
await inHandle.hover();
await page.mouse.down();
await page.mouse.move(ib.x + ib.width / 2 + 45, ib.y + ib.height / 2 + 30, {
  steps: 5,
});
await page.mouse.up();
await page.keyboard.up("Alt");
await page.waitForTimeout(250);
const armsAfterAlt = await arms();
assert(
  Math.abs(armsAfterAlt[1][0] - outBefore[0]) < 3 &&
    Math.abs(armsAfterAlt[1][1] - outBefore[1]) < 3,
  `alt-drag left the other handle where it was (${outBefore} -> ${armsAfterAlt[1]})`,
);
assert(!opposite(armsAfterAlt), "and the pair is no longer mirrored");
await page.keyboard.press("Control+z");
await page.keyboard.press("Control+z");
await page.waitForTimeout(250);

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

// 8u3. Brush: a freehand drag becomes a stroked path — editable anchors,
// not baked pixels — simplified down from the raw samples.
await page.click('button[aria-label="Brush"]');
await page.locator('input[aria-label="Brush width"]').fill("10");
await page.waitForTimeout(150);
const strokeAt = [420, 640];
assert((await canvasPixel(...strokeAt))[3] === 0, "brush area starts empty");
await page.mouse.move(...toScreen(300, 620));
await page.mouse.down();
for (const [x, y] of [
  [340, 630],
  [380, 645],
  [420, 640],
  [460, 620],
  [500, 600],
]) {
  await page.mouse.move(...toScreen(x, y), { steps: 3 });
}
await page.mouse.up();
await page.waitForTimeout(300);
assert(
  (await page.locator(".panel ul li", { hasText: "Stroke" }).count()) === 1,
  "the stroke became a layer",
);
const painted = await canvasPixel(...strokeAt);
assert(painted[3] > 0, `ink follows the drag (${painted})`);
assert(
  (await canvasPixel(420, 700))[3] === 0,
  "and nowhere the brush did not go",
);
// It is a path, so it has anchors to grab — and far fewer than the samples.
const anchors = await page.locator(".anchor").count();
assert(
  anchors >= 2 && anchors <= 8,
  `the stroke simplifies to a handful of anchors (${anchors})`,
);
// Width follows the hand: a slow stretch lays down more ink than a flick,
// so the band is measurably wider at the slow end.
const bandAt = async (x) => {
  let n = 0;
  for (let y = 560; y < 700; y++) if ((await canvasPixel(x, y))[3] > 128) n++;
  return n;
};
await page.mouse.move(...toScreen(200, 600));
await page.mouse.down();
for (let x = 220; x <= 340; x += 20) {
  await page.mouse.move(...toScreen(x, 600), { steps: 8 }); // slow
}
for (let x = 380; x <= 620; x += 60) {
  await page.mouse.move(...toScreen(x, 600), { steps: 1 }); // fast
}
await page.mouse.up();
await page.waitForTimeout(300);
const slowBand = await bandAt(260);
const fastBand = await bandAt(560);
assert(
  slowBand > fastBand,
  `a slow stroke lays down more ink than a flick (${slowBand} vs ${fastBand})`,
);
await page.keyboard.press("Control+z");
await page.waitForTimeout(200);

await page.keyboard.press("Control+z");
await page.waitForTimeout(200);
assert(
  (await page.locator(".panel ul li", { hasText: "Stroke" }).count()) === 0 &&
    (await canvasPixel(...strokeAt))[3] === 0,
  "and the whole stroke undoes as one step",
);
await page.click('button[aria-label="Move"]');

// 8v. Text tool: click to add a live text object, edit it via the panel.
const inkCount = (x0, y0, x1, y1) =>
  page.evaluate(([a, b, c, d]) => {
    const el = document.getElementById("engine-canvas");
    const s = Number(el.dataset.frameScale) || 1;
    [a, b, c, d] = [a, b, c, d].map((v) => Math.round(v * s));
    const img = el.getContext("2d").getImageData(a, b, c - a, d - b).data;
    let n = 0;
    for (let i = 3; i < img.length; i += 4) if (img[i] > 0) n++;
    return n;
  }, [x0, y0, x1, y1]);

assert((await inkCount(740, 590, 1100, 710)) === 0, "text target area empty");
await page.click('button[aria-label="Text"]');
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
  (await menuItem("File", "Export SVG")).click(),
]);
const svgPath = await svgDl.path();
const svgText = await readFile(svgPath, "utf8");
assert(svgText.startsWith("<svg "), "SVG root element");
assert(svgText.includes("<rect") && svgText.includes("<path"), "shapes exported as vectors");
assert(svgText.includes("<text") && svgText.includes("Hello!"), "text exported live");
console.log("ok: SVG export contains live vector markup");

// 8w2. Export JPEG: a real JPEG, with the canvas flattened onto white.
const [jpegDl] = await Promise.all([
  page.waitForEvent("download"),
  (await menuItem("File", "Export JPEG")).click(),
]);
const jpegBytes = await readFile(await jpegDl.path());
assert(
  jpegBytes[0] === 0xff && jpegBytes[1] === 0xd8,
  "JPEG starts with the SOI marker",
);
assert(
  jpegBytes.subarray(-2).equals(Buffer.from([0xff, 0xd9])),
  "and ends with EOI",
);
assert(jpegBytes.length > 2000, `JPEG carries image data (${jpegBytes.length} bytes)`);

// 8x. A document is not always 1280x720: create a small one, check the
// canvas is sized to it and that a drag lands where it was aimed.
await newDocument(600, 400, "rgb");
assert(
  await page.isVisible("text=RGB, 600×400"),
  "the status chip reports the document's real size",
);
const smallCanvas = await page.$eval("#engine-canvas", (c) => [c.width, c.height]);
assert(
  smallCanvas[0] >= 600 &&
    Math.abs(smallCanvas[0] / smallCanvas[1] - 600 / 400) < 0.02,
  `the canvas carries the document's shape at or above its resolution (${smallCanvas})`,
);
{
  // Screen/document conversion has to follow the new size, so a drag in
  // the middle of the canvas must paint in the middle of the document.
  const b = await page.locator("#engine-canvas").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await page.click('button[aria-label="Rect"]');
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(500, 300), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const inside = await canvasPixel(300, 200);
  assert(inside[3] === 255, `drag painted inside the small document (${inside})`);
  assert((await canvasPixel(20, 20))[3] === 0, "and not outside the drag");
}

// 8x2. Drop shadow: a live effect on the selected layer. The rect spans
// (100,100)-(500,300) in this 600x400 document, so just past its
// bottom-right corner is empty ground for the shadow to land on.
{
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  assert((await canvasPixel(503, 200))[3] === 0, "ground beside the rect is clear");
  await page.click("text=Add drop shadow");
  await page.waitForTimeout(300);
  await page.screenshot({ path: join(OUT, "drop-shadow.png") });
  const shade = await canvasPixel(503, 200);
  assert(shade[3] > 40 && shade[0] < 120, `a shadow fell to the right (${shade})`);
  assert(
    (await canvasPixel(300, 200))[3] === 255,
    "and the layer itself is unchanged",
  );
  assert(
    (await page.locator('.panel ul li [title="This layer has effects"]').count()) === 1,
    "the layer row marks that it carries effects",
  );
  // Aiming it the other way moves the shadow with it.
  const xSlider = page.locator('input[aria-label="Shadow X"]');
  await xSlider.fill("-30");
  await xSlider.dispatchEvent("pointerup");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(503, 200))[3] === 0,
    "re-aiming the shadow vacated where it was",
  );
  const behind = await canvasPixel(80, 200);
  assert(behind[3] > 20, `and it now falls to the left (${behind})`);
  await page.click("text=Remove");
  await page.waitForTimeout(300);
  assert((await canvasPixel(80, 200))[3] === 0, "removing it clears the ground");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert((await canvasPixel(80, 200))[3] > 20, "and undo brings the shadow back");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert(
    (await page.locator('.panel ul li [title="This layer has effects"]').count()) === 0,
    "undo unwinds the shadow entirely",
  );
}

// 8x3. Dragging one member of a multi-selection carries the rest. Rects
// are used because the drag has to start on the layer it grabs, and a
// rect's centre is reliably on it.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-canvas").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await page.click('button[aria-label="Rect"]');
  for (const [x0, y0, x1, y1] of [
    [60, 60, 180, 160],
    [300, 60, 420, 160],
  ]) {
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  }
  await page.click('button[aria-label="Move"]');
  await page.locator(".panel ul li").first().click();
  await page.locator(".panel ul li").nth(1).click({ modifiers: ["Control"] });
  await page.waitForTimeout(200);
  assert((await canvasPixel(120, 110))[3] === 255, "first rect is where it was drawn");
  assert((await canvasPixel(360, 110))[3] === 255, "and so is the second");
  // Grab the middle of one of them and pull straight down.
  await page.mouse.move(...at(120, 110));
  await page.mouse.down();
  await page.mouse.move(...at(120, 240), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert((await canvasPixel(120, 240))[3] === 255, "the grabbed rect came along");
  assert(
    (await canvasPixel(360, 240))[3] === 255,
    "and so did the one that was only ctrl-clicked",
  );
  assert((await canvasPixel(360, 110))[3] === 0, "leaving its old place empty");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(360, 110))[3] === 255 && (await canvasPixel(120, 240))[3] === 0,
    "and the pair moves back in one undo",
  );
}

// 8y. The clipboard survives the document it was copied from: copy a
// shape, start a fresh document, paste it back.
{
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await page.keyboard.press("Control+c");
  await page.waitForTimeout(150);
  await newDocument(600, 400, "rgb");
  assert(
    (await page.locator(".panel ul li.empty").count()) === 1,
    "the new document starts empty",
  );
  await page.keyboard.press("Control+v");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "paste brought the copied layer into the new document",
  );
  // It renders, rather than arriving as an empty node.
  const shot = await page.evaluate(() => {
    const c = document.getElementById("engine-canvas");
    const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
    let ink = 0;
    for (let i = 3; i < d.length; i += 4) if (d[i] > 0) ink++;
    return ink;
  });
  assert(shot > 100, `and it renders (${shot} covered pixels)`);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li.empty").count()) === 1,
    "and the paste undoes as one step",
  );
}

// 9. CMYK doc smoke: new doc, draw, still renders.
await newDocument(1280, 720, "cmyk");
await page.click('button[aria-label="Rect"]');
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
await page.click('button[aria-label="Move"]');
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
await newDocument(1280, 720, "rgb");
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
await page.click('button[aria-label="Rect"]');
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
  (await menuItem("File", "Export TIFF")).click(),
]);
const tiffBytes = await readFile(await tiffDl.path());
const marker = tiffBytes.subarray(0, 2).toString("latin1");
assert(marker === "II" || marker === "MM", `TIFF byte-order marker (${marker})`);
assert(tiffBytes.includes(iccBytes.subarray(0, 256)), "press profile embedded in TIFF");
assert(tiffBytes.length > 10000, `TIFF carries pixel data (${tiffBytes.length} bytes)`);

// 9f. PDF export, CMYK-separated because a press profile is loaded.
const [pdfDl] = await Promise.all([
  page.waitForEvent("download"),
  (await menuItem("File", "Export PDF")).click(),
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
