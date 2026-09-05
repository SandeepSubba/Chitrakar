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

// Probes are written in document pixels. The canvas is a window onto the
// page: `data-origin-x/y` say where the document's origin sits in its
// backing store and `data-frame-scale` how many of its pixels one document
// pixel takes, so every read converts through those.
const canvasPixel = (x, y) =>
  page.evaluate(([x, y]) => {
    const c = document.getElementById("engine-canvas");
    const s = Number(c.dataset.frameScale) || 1;
    const ox = Number(c.dataset.originX) || 0;
    const oy = Number(c.dataset.originY) || 0;
    const dx = Math.round(ox + (x + 0.5) * s);
    const dy = Math.round(oy + (y + 0.5) * s);
    if (dx < 0 || dy < 0 || dx >= c.width || dy >= c.height) return [0, 0, 0, 0];
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
const newDocument = async (w, h, mode, dpi) => {
  await menuClick("File", "New document…");
  await page.locator('input[aria-label="Width"]').fill(String(w));
  await page.locator('input[aria-label="Height"]').fill(String(h));
  await page.selectOption('[aria-label="Colour mode"]', mode);
  if (dpi) await page.locator('input[aria-label="Resolution"]').fill(String(dpi));
  await page.click("text=Create");
  await page.waitForTimeout(400);
};

/** Pick a tool. The shape tools share one slot in the rail — the one
 * last used sits in it — so anything else in that group is reached by
 * opening the group first, which is what a person does too. */
const pickTool = async (name) => {
  const direct = page.locator(`.toolbar > button[aria-label="${name}"]`);
  if (await direct.count()) {
    await direct.click();
  } else {
    const slot = page.locator('.tool-group > button[aria-label="' + name + '"]');
    if (await slot.count()) {
      await slot.click();
    } else {
      await page.click('button[aria-label="More shapes"]');
      await page.waitForTimeout(80);
      await page.click(`.tool-flyout button[aria-label="${name}"]`);
    }
  }
  await page.waitForTimeout(60);
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
    "File,Edit,Page,View",
  "menu bar carries File, Edit, Page and View",
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
await pickTool("Rect");
const box = await page.locator("#engine-page").boundingBox();
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
await pickTool("Ellipse");
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

// 4b. Levels: neutral at first; an input white below the pixel's value
// pushes it to white, an output white pulls everything down. Undone
// again afterwards — four gestures, four entries — so the rest of the
// suite sees the document and the history it expects.
{
  await page.selectOption('[aria-label="Add adjustment layer"]', "levels");
  await page.waitForTimeout(200);
  const neutral = await canvasPixel(500, 325);
  assert(
    (await page.locator(".panel ul li", { hasText: "Levels" }).count()) === 1 &&
      neutral.join() === after.join(),
    "a neutral levels layer changes nothing",
  );
  await page.locator(".panel ul li", { hasText: "Levels" }).click();
  await setSlider("Input white", 0.05);
  const stretched = await canvasPixel(500, 325);
  assert(
    stretched[1] > neutral[1] && stretched[1] >= 250,
    `an input white below the pixel clips it to white (g ${neutral[1]} -> ${stretched[1]})`,
  );
  await setSlider("Input white", 1);
  await setSlider("Output white", 0.2);
  const pressed = await canvasPixel(500, 325);
  assert(
    pressed[1] < neutral[1],
    `an output white pulls it down (g ${neutral[1]} -> ${pressed[1]})`,
  );
  for (let i = 0; i < 4; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(100);
  }
  assert(
    (await page.locator(".panel ul li", { hasText: "Levels" }).count()) === 0 &&
      (await canvasPixel(500, 325)).join() === after.join(),
    "and four undos take the layer and its three edits back",
  );
}

// 4b1. A layer's look travels to another layer without its shape: copy
// the style off one rect, give it to a second, and the second keeps
// being itself while taking the fill and the opacity.
{
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 1280) * b.width, b.y + (y / 720) * b.height];
  // The colour to draw with is shared with every later block, so put it
  // back before leaving.
  const wasFill = await page.inputValue('input[aria-label="Fill colour"]');
  await pickTool("Rect");
  await page.mouse.move(...at(80, 560));
  await page.mouse.down();
  await page.mouse.move(...at(280, 680), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await page.fill('input[aria-label="Fill colour"]', "#22cc55");
  await page.waitForTimeout(200);
  await pickTool("Rect");
  await page.mouse.move(...at(340, 560));
  await page.mouse.down();
  await page.mouse.move(...at(540, 680), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const green = await canvasPixel(440, 620);
  assert(green[1] > green[0] && green[1] > green[2], `a green rect (${green})`);
  // Drawing a shape does not pick it, and the opacity lives in the
  // panel, so pick the new rect by its row — the topmost one.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  // Fade it, so the style carries something besides the fill.
  await setSlider("Layer opacity", 0.5);
  await page.waitForTimeout(200);

  // Copy that look, pick the first rect, and give it the look.
  await page.keyboard.press("Control+Alt+c");
  await page.waitForTimeout(150);
  await pickTool("Move");
  await page.mouse.click(...at(180, 620));
  await page.waitForTimeout(200);
  const wasFirst = await canvasPixel(180, 620);
  await page.keyboard.press("Control+Alt+v");
  await page.waitForTimeout(250);
  const nowFirst = await canvasPixel(180, 620);
  assert(
    nowFirst.join() !== wasFirst.join(),
    `the first rect took the look (${wasFirst} -> ${nowFirst})`,
  );
  assert(
    nowFirst[1] > nowFirst[0] && nowFirst[1] > nowFirst[2],
    `and it is the green one (${nowFirst})`,
  );
  assert(
    (await canvasPixel(180, 540))[3] === 0,
    "and it kept its own shape rather than taking the other's",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(180, 620)).join() === wasFirst.join(),
    "one undo takes the whole paste back",
  );
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(120);
  }
  await page.fill('input[aria-label="Fill colour"]', wasFill);
  await page.waitForTimeout(200);
}

// 4b2. White balance and vibrance: neutral changes nothing, warming lifts
// the red and drops the blue, and vibrance moves a colour without moving
// a grey. Undone afterwards, so the rest of the suite sees what it
// expects.
{
  await page.selectOption('[aria-label="Add adjustment layer"]', "white-balance");
  await page.waitForTimeout(200);
  const neutral = await canvasPixel(500, 325);
  assert(
    (await page.locator(".panel ul li", { hasText: "White balance" }).count()) === 1 &&
      neutral.join() === after.join(),
    "a neutral white balance changes nothing",
  );
  await page.locator(".panel ul li", { hasText: "White balance" }).click();
  // The exposure layer below has pushed this pixel's blue past white —
  // it is 255 on screen but above 1.0 in linear light — so cooling it by
  // 40% still shows as 255. Red is the channel with room to move.
  await setSlider("Temperature", 0.8);
  const warm = await canvasPixel(500, 325);
  assert(
    warm[0] > neutral[0],
    `warming lifts the red (${neutral} -> ${warm})`,
  );
  await setSlider("Temperature", -0.8);
  const cool = await canvasPixel(500, 325);
  assert(
    cool[0] < neutral[0],
    `and cooling drops it below where it started (${neutral} -> ${cool})`,
  );
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(100);
  }
  assert(
    (await page.locator(".panel ul li", { hasText: "White balance" }).count()) === 0,
    "and three undos take the layer and its two edits back",
  );

  await page.selectOption('[aria-label="Add adjustment layer"]', "vibrance");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(500, 325)).join() === after.join(),
    "a neutral vibrance changes nothing",
  );
  await page.locator(".panel ul li", { hasText: "Vibrance" }).click();
  // What vibrance does to a given colour is settled by the engine's own
  // tests; what this one is for is that the slider reaches the picture.
  // A digest rather than one pixel, since the pixel this block has been
  // reading is over-exposed and already saturated, which is exactly the
  // colour vibrance is supposed to leave alone.
  const digest = () =>
    page.evaluate(() => {
      const c = document.getElementById("engine-canvas");
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      let h = 0;
      for (let i = 0; i < d.length; i += 997) h = (h * 31 + d[i]) | 0;
      return h;
    });
  const before = await digest();
  await setSlider("Vibrance", 1);
  assert((await digest()) !== before, "vibrance reaches the picture");
  for (let i = 0; i < 2; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(100);
  }
  assert(
    (await page.locator(".panel ul li", { hasText: "Vibrance" }).count()) === 0 &&
      (await digest()) === before &&
      (await canvasPixel(500, 325)).join() === after.join(),
    "and two undos take that back too",
  );
}

// 4c. Curves: the diagonal changes nothing; pressing on the graph's middle
// and dragging up adds a point and lifts the midtones in one gesture.
{
  await page.selectOption('[aria-label="Add adjustment layer"]', "curves");
  await page.waitForTimeout(200);
  const neutral = await canvasPixel(500, 325);
  assert(
    (await page.locator(".panel ul li", { hasText: "Curves" }).count()) === 1 &&
      neutral.join() === after.join(),
    "a curve on the diagonal changes nothing",
  );
  await page.locator(".panel ul li", { hasText: "Curves" }).click();
  const graph = await page.locator('[aria-label="Tone curve"]').boundingBox();
  const [cx, cy] = [graph.x + graph.width / 2, graph.y + graph.height / 2];
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx, cy - graph.height * 0.25, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const lifted = await canvasPixel(500, 325);
  assert(
    lifted[1] > neutral[1] + 20 && (await page.locator('[aria-label="Tone curve"] circle').count()) === 3,
    `pressing on the curve adds a point, and dragging it up lifts the midtones (g ${neutral[1]} -> ${lifted[1]})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert(
    (await canvasPixel(500, 325)).join() === after.join() &&
      (await page.locator('[aria-label="Tone curve"] circle').count()) === 2,
    "one undo takes the whole press-and-drag back",
  );
  // The tones the curve is about to move are drawn behind it: three
  // filled shapes for RGB, one for a single channel.
  await page.waitForTimeout(500);
  assert(
    (await page.locator('[aria-label="Tone curve"] .histogram path').count()) ===
      3,
    "the picture's tones are drawn behind the graph, one shape per channel",
  );

  // Per-channel: the picker switches the graph to one channel, and a
  // curve drawn there moves that channel alone.
  // Red, not blue: this pixel's blue is already at the top of the range,
  // so a curve could not show there — the same over-exposure that the
  // white balance block has to step around.
  const before = await canvasPixel(500, 325);
  await page.click('button[aria-label="Red channel"]');
  await page.waitForTimeout(150);
  const chan = await page.locator('[aria-label="Red curve"]').boundingBox();
  const [bx, by] = [chan.x + chan.width / 2, chan.y + chan.height / 2];
  await page.mouse.move(bx, by);
  await page.mouse.down();
  await page.mouse.move(bx, by - chan.height * 0.3, { steps: 4 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const graded = await canvasPixel(500, 325);
  assert(
    graded[0] > before[0] + 15,
    `a curve on red lifts red (${before} -> ${graded})`,
  );
  assert(
    graded[1] === before[1] && graded[2] === before[2],
    `and leaves green and blue where they were (${before} -> ${graded})`,
  );
  // The master graph still shows its own curve, with blue's behind it.
  await page.click('button[aria-label="RGB channel"]');
  await page.waitForTimeout(150);
  assert(
    (await page.locator('[aria-label="Tone curve"] .ghost').count()) === 1,
    "the channel that has a curve is drawn behind the one in hand",
  );
  await page.click('button[aria-label="Red channel"]');
  await page.waitForTimeout(500);
  assert(
    (await page.locator('[aria-label="Red curve"] .histogram path').count()) ===
      1,
    "and on one channel the tones behind are that channel's alone",
  );
  await page.click('button[aria-label="RGB channel"]');
  await page.waitForTimeout(150);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(500, 325)).join() === before.join(),
    "and one undo takes the channel curve back",
  );

  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".panel ul li", { hasText: "Curves" }).count()) === 0,
    "and the next takes the layer",
  );
}

// 4d. The three that decide what a picture is made of: black and white
// mixed by its weights, a gradient map reading a tone off a ramp, and a
// negative. Each is added, checked against the picture, and undone.
{
  const shown = async () => await canvasPixel(500, 325);
  const start = await shown();
  const grey = (px) => Math.abs(px[0] - px[1]) < 3 && Math.abs(px[1] - px[2]) < 3;
  assert(!grey(start), "the picture has colour in it to take away");

  await page.selectOption('[aria-label="Add adjustment layer"]', "black-and-white");
  await page.waitForTimeout(250);
  assert(grey(await shown()), "black and white takes the colour out");
  await page.locator(".panel ul li", { hasText: "Black & white" }).click();
  await page.waitForTimeout(150);
  const byLuma = (await shown())[0];
  // The weights are a recipe: leaning on one channel changes which
  // colours come out light, so the same pixel comes out a different grey.
  await setSlider("Red weight", 2);
  await page.waitForTimeout(200);
  const leaning = await shown();
  assert(grey(leaning), "it is still grey");
  assert(
    Math.abs(leaning[0] - byLuma) > 4,
    `and a different one (${leaning[0]} against ${byLuma})`,
  );
  await page.click("text=Plain luminance");
  await page.waitForTimeout(200);
  assert(
    Math.abs((await shown())[0] - byLuma) < 2,
    "and the button puts the plain recipe back",
  );
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(120);
  }
  assert(
    (await page.locator(".panel ul li", { hasText: "Black & white" }).count()) === 0 &&
      (await shown()).join() === start.join(),
    "and undo takes the layer and every weight with it",
  );

  await page.selectOption('[aria-label="Add adjustment layer"]', "gradient-map");
  await page.waitForTimeout(250);
  assert(grey(await shown()), "a black-to-white ramp is a monochrome map");
  await page.locator(".panel ul li", { hasText: "Gradient map" }).click();
  await page.waitForTimeout(150);
  // Colour the shadows end: every tone now reads off a ramp that starts
  // red, so the picture takes the ramp's colour rather than its own.
  await page.locator('input[aria-label="Map stop 1"]').evaluate((el) => {
    const set = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    ).set;
    set.call(el, "#ff0000");
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  });
  await page.waitForTimeout(250);
  const mapped = await shown();
  assert(mapped[0] > mapped[2] + 10, `the ramp's colour is what shows (${mapped})`);
  for (let i = 0; i < 2; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(120);
  }
  assert(
    (await page.locator(".panel ul li", { hasText: "Gradient map" }).count()) === 0 &&
      (await shown()).join() === start.join(),
    "and undo takes the map back",
  );

  await page.selectOption('[aria-label="Add adjustment layer"]', "invert");
  await page.waitForTimeout(250);
  const negative = await shown();
  // A negative on the values a device shows: the two sides of each
  // channel add up to the whole of it.
  assert(
    [0, 1, 2].every((c) => Math.abs(negative[c] + start[c] - 255) < 4),
    `each channel is turned inside out (${negative} against ${start})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".panel ul li", { hasText: "Invert" }).count()) === 0 &&
      (await shown()).join() === start.join(),
    "and undo puts the picture back",
  );
}

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
await pickTool("Move");
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
// The rest of the W3C modes are in the picker too, grouped the way an
// editor groups them. Difference of a colour with what it sits on is the
// one with an answer that can be checked without knowing either.
assert(
  (await page.locator('select[aria-label="Blend mode"] option').count()) === 16,
  "every blend mode the spec names is offered",
);
await page.selectOption('select[aria-label="Blend mode"]', "Screen");
await page.waitForTimeout(150);
const screened = await canvasPixel(500, 400);
assert(
  screened[1] > overlapBefore[1],
  `screen lightened it the other way (g ${overlapBefore[1]} -> ${screened[1]})`,
);
// Both shapes were drawn with the same fill, so difference is the mode
// with an answer that says so: a colour against itself is nothing.
await page.selectOption('select[aria-label="Blend mode"]', "Difference");
await page.waitForTimeout(150);
const same = await canvasPixel(500, 400);
assert(
  same[0] < 10 && same[1] < 10 && same[2] < 10 && same[3] === 255,
  `a colour differenced with itself is black (${same})`,
);
await page.selectOption('select[aria-label="Blend mode"]', "Multiply");
await page.waitForTimeout(150);

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
await page.setInputFiles('input[accept="image/png,image/jpeg,image/svg+xml"]', {
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
const boxBefore = await page.locator("#engine-page").boundingBox();
const pixelBeforeZoom = await canvasPixel(310, 480);
await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
await page.mouse.wheel(0, -400);
await page.waitForTimeout(150);
const boxAfter = await page.locator("#engine-page").boundingBox();
assert(boxAfter.width > boxBefore.width * 1.05, "wheel zoom enlarged canvas");
// And it enlarged the artwork, not its pixels: the engine is told to
// render more of them per document pixel, while the canvas itself stays
// the size of the viewport it covers.
const zoomedStore = await page.$eval("#engine-canvas", (c) => [
  c.width,
  Number(c.dataset.frameScale),
]);
assert(
  zoomedStore[1] > 1,
  `zooming in raised the render resolution (${zoomedStore})`,
);
assert(
  zoomedStore[0] < 1280 * 2,
  `and the canvas stayed a viewport rather than growing with it (${zoomedStore})`,
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
await pickTool("Move");
const box2 = await page.locator("#engine-page").boundingBox();
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

// 8i3. Resizing snaps too: pull a corner near the page's middle and it
// catches, with a guide, on the same lines a move would.
{
  await page.mouse.click(...toScreen(450, 400));
  await page.waitForTimeout(150);
  const se = await page.locator(".handle.se").boundingBox();
  await page.mouse.move(se.x + 5, se.y + 5);
  await page.mouse.down();
  // Aim three document pixels short of the page's vertical centre (640).
  await page.mouse.move(...toScreen(637, 500), { steps: 8 });
  await page.waitForTimeout(200);
  const vertical = await page.$$eval(".snap-overlay line", (ls) =>
    ls.filter((l) => l.getAttribute("x1") === l.getAttribute("x2")).length,
  );
  assert(vertical === 1, "a vertical guide shows while the corner is caught");
  await page.mouse.up();
  await page.waitForTimeout(250);
  // Outline points are relative to the canvas host, not the viewport.
  const host = await page.locator(".canvas-host").boundingBox();
  const rightDoc = await page
    .$eval(".sel-outline polygon", (el) =>
      Math.max(...el.getAttribute("points").split(" ").map((p) => Number(p.split(",")[0]))),
    )
    .then((x) => (x + host.x - box2.x) / sx2);
  assert(
    Math.abs(rightDoc - 640) < 1.5 && Math.abs(rightDoc - 637) > 1.5,
    `the corner settled on the page's centre line, not where it was dropped (${rightDoc})`,
  );
  assert(
    (await page.locator(".snap-overlay line").count()) === 0,
    "and the guide clears when the drag ends",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
}

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

// 8k0. And it can lie on either side of the edge, which is the
// difference between a border that eats into the fill and one that does
// not. Read three points across the rect's left edge: how far out and
// how far in the band reaches tells the three apart, without either of
// us having to know exactly where the edge fell after the resizes above.
{
  // Dark and opaque: bare canvas is transparent, and a transparent pixel
  // reads zero on every channel, which "dark" alone would take for ink.
  const band = async (x) => {
    const px = await canvasPixel(x, 400);
    return px[3] > 200 && px[0] < 80;
  };
  const across = async () => [await band(296), await band(298), await band(302)];
  const side = async (which) => {
    if (which) {
      await page.selectOption('select[aria-label="Border side"]', which);
      await page.waitForTimeout(300);
    }
    return across();
  };
  assert(
    (await side(null)).join() === "false,false,true",
    `unasked, the band lies inside the edge (${await across()})`,
  );
  assert(
    (await side("Outside")).join() === "true,true,false",
    `outside, it lies beyond the edge and not within it (${await across()})`,
  );
  assert(
    (await side("Centre")).join() === "false,true,false",
    `across, it straddles the edge (${await across()})`,
  );
  assert(
    (await side("Inside")).join() === "false,false,true",
    "and back inside where it began",
  );
}

// 8k1. That stroke can be broken up: the picker's patterns leave gaps
// along the line, and Solid puts it back. Read along the top band of the
// same rect, two pixels in from its edge.
{
  const alongEdge = async () => {
    const row = [];
    for (let x = 320; x < 380; x += 3) row.push((await canvasPixel(x, 302))[0]);
    return row;
  };
  const solid = await alongEdge();
  assert(
    solid.every((r) => r < 80),
    `the stroke runs the whole edge (${solid})`,
  );
  await page.selectOption('select[aria-label="Line pattern"]', {
    label: "Dashed",
  });
  await page.waitForTimeout(300);
  const dashed = await alongEdge();
  assert(
    dashed.some((r) => r > 150) && dashed.some((r) => r < 80),
    `dashed, it is on in places and off in others (${dashed})`,
  );
  await page.selectOption('select[aria-label="Line pattern"]', {
    label: "Solid",
  });
  await page.waitForTimeout(300);
  assert(
    (await alongEdge()).every((r) => r < 80),
    "and Solid puts the whole line back",
  );
}

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
  (await page.locator(".panel ul li .layer-kind-icon").count()) > 0,
  "layer rows say what the layer is — a picture of it, or a glyph for its kind",
);

// 8l2. Masks: inscribed ellipse mask hides the rect's corners, invert
// flips it, remove restores — all non-destructive.
// Probes sit a few document pixels inside an edge: the canvas renders at
// the size it is displayed at, so a probe on a boundary lands on a pixel
// the edge only partly covers.
px = await canvasPixel(594, 494); // rect's bottom-right corner, outside ellipse
assert(px[3] === 255, "rect corner visible before mask");
await page.click("text=Ellipse mask");
await page.waitForTimeout(200);
px = await canvasPixel(594, 494);
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
    (await canvasPixel(594, 494))[3] === 255,
    "moving the mask uncovered the corner it was hiding",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(594, 494))[3] === 0,
    "and the move undoes as one step",
  );
}
await page.check('input[aria-label="Invert mask"]');
await page.waitForTimeout(200);
px = await canvasPixel(450, 400);
assert(px[0] !== 255 || px[3] !== 255, "inverted mask hides the center");
await page.click("text=Remove");
await page.waitForTimeout(200);
px = await canvasPixel(594, 494);
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
await pickTool("Pen");
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
await pickTool("Move");
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
    (await page.locator(".align-bar:not(.combine-bar) button").count()) === 8,
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

// 8t2. Anchors go on and come off: double-clicking the outline puts one
// where it was clicked, alt-clicking one takes it away, and each is a
// single undo.
{
  const was = await page.locator(".anchor").count();
  // The triangle's own outline, partway along an edge.
  const edge = await page.locator('[data-anchor="0"]').boundingBox();
  const other = await page.locator('[data-anchor="1"]').boundingBox();
  const apexAt = await page.locator('[data-anchor="2"]').boundingBox();
  // Halfway along the edge and a few pixels inside it: on the outline is
  // half outside the fill, and a click that lands outside deselects the
  // layer before the double-click is over.
  const centre = {
    x: (edge.x + other.x + apexAt.x) / 3 + 5,
    y: (edge.y + other.y + apexAt.y) / 3 + 5,
  };
  const half = { x: (edge.x + other.x) / 2 + 5, y: (edge.y + other.y) / 2 + 5 };
  const mid = {
    x: half.x + (centre.x - half.x) * 0.12,
    y: half.y + (centre.y - half.y) * 0.12,
  };
  await page.mouse.dblclick(mid.x, mid.y);
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".anchor").count()) === was + 1,
    `double-click put an anchor on the outline (${was} -> ${await page
      .locator(".anchor")
      .count()})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".anchor").count()) === was,
    "and one undo takes it off again",
  );

  // Put it back, then alt-click it away.
  await page.mouse.dblclick(mid.x, mid.y);
  await page.waitForTimeout(300);
  const added = await page.locator(".anchor").count();
  const spot = await page.locator('[data-anchor="1"]').boundingBox();
  await page.keyboard.down("Alt");
  await page.mouse.click(spot.x + 5, spot.y + 5);
  await page.keyboard.up("Alt");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".anchor").count()) === added - 1,
    "alt-click takes an anchor off",
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".anchor").count()) === was,
    "and the triangle is a triangle again",
  );
}

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
await pickTool("Brush");
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
await pickTool("Move");

// 8v. Text tool: click to add a live text object, edit it via the panel.
const inkCount = (x0, y0, x1, y1) =>
  page.evaluate(([a, b, c, d]) => {
    const el = document.getElementById("engine-canvas");
    const s = Number(el.dataset.frameScale) || 1;
    const ox = Number(el.dataset.originX) || 0;
    const oy = Number(el.dataset.originY) || 0;
    [a, c] = [a, c].map((v) => Math.round(ox + v * s));
    [b, d] = [b, d].map((v) => Math.round(oy + v * s));
    const img = el.getContext("2d").getImageData(a, b, c - a, d - b).data;
    let n = 0;
    for (let i = 3; i < img.length; i += 4) if (img[i] > 0) n++;
    return n;
  }, [x0, y0, x1, y1]);

assert((await inkCount(740, 590, 1100, 710)) === 0, "text target area empty");
await pickTool("Text");
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
// 8v1. Typing on the canvas: a double-click on the block opens it for
// editing in place; keystrokes preview through the engine, Escape puts
// the old text back, Ctrl+Enter keeps the new — one history entry.
{
  await page.mouse.dblclick(box.x + 780 * sx, box.y + 625 * sy);
  await page.waitForTimeout(250);
  const editor = page.locator("textarea.inline-text");
  assert(
    (await editor.count()) === 1 && (await editor.inputValue()) === "Hello!",
    "double-clicking the block opens it for typing with its text",
  );
  await page.keyboard.press("End");
  await page.keyboard.type("!!");
  await page.waitForTimeout(250);
  const inkTyped = await inkCount(740, 590, 1200, 710);
  assert(inkTyped > inkHello, `typing on the canvas re-renders the block (${inkHello} -> ${inkTyped})`);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(250);
  assert(
    (await editor.count()) === 0 && (await inkCount(740, 590, 1200, 710)) === inkHello,
    "Escape closes the editor and puts the old text back",
  );
  await page.mouse.dblclick(box.x + 780 * sx, box.y + 625 * sy);
  await page.waitForTimeout(250);
  await page.keyboard.press("End");
  await page.keyboard.type("!!");
  await page.keyboard.press("Control+Enter");
  await page.waitForTimeout(250);
  assert(
    (await editor.count()) === 0 &&
      (await inkCount(740, 590, 1200, 710)) === inkTyped &&
      (await page.locator('textarea[aria-label="Text content"]').inputValue()) === "Hello!!!",
    "Ctrl+Enter keeps what was typed, and the panel agrees",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert(
    (await inkCount(740, 590, 1200, 710)) === inkHello,
    "and the whole typing is one undo step",
  );
}
await setSlider("Size", 96);
const inkBig = await inkCount(740, 560, 1280, 720);
assert(inkBig > inkHello * 1.5, `larger size grew the ink (${inkHello} -> ${inkBig})`);
await page.keyboard.press("Control+z"); // size gesture
await page.waitForTimeout(200);
assert(
  (await inkCount(740, 590, 1100, 710)) === inkHello,
  "size change was one undo step",
);

// 8v2. Text styling: two lines, then alignment, spacing and tracking.
{
  const setText = async (value) =>
    page.locator('textarea[aria-label="Text content"]').evaluate((el, v) => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLTextAreaElement.prototype,
        "value",
      ).set;
      setter.call(el, v);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
    }, value);
  // A long line over a short one, so alignment has slack to move.
  await setText("Hello there!\nhi");
  await page.waitForTimeout(300);
  const band = [740, 640, 1280, 720]; // the second line's band
  const tall = await inkCount(...band);
  assert(tall > 20, `the second line renders (${tall} px)`);
  // Where the short line's ink sits horizontally, as its centre of mass.
  const secondLineCentre = () =>
    page.evaluate(([a, b, c, d]) => {
      const el = document.getElementById("engine-canvas");
      const s = Number(el.dataset.frameScale) || 1;
      const ox = Number(el.dataset.originX) || 0;
      const oy = Number(el.dataset.originY) || 0;
      [a, c] = [a, c].map((v) => Math.round(ox + v * s));
      [b, d] = [b, d].map((v) => Math.round(oy + v * s));
      const img = el.getContext("2d").getImageData(a, b, c - a, d - b).data;
      let sum = 0;
      let n = 0;
      for (let i = 0; i < img.length; i += 4) {
        if (img[i + 3] > 0) {
          sum += ((i / 4) % (c - a)) / s;
          n++;
        }
      }
      return n === 0 ? null : sum / n;
    }, band);
  const atLeft = await secondLineCentre();
  await page.click('button[aria-label="Align text right"]');
  await page.waitForTimeout(300);
  const atRight = await secondLineCentre();
  assert(
    atRight - atLeft > 20,
    `right alignment pushed the short line across (${atLeft} -> ${atRight})`,
  );
  await page.click('button[aria-label="Centre text"]');
  await page.waitForTimeout(300);
  const atCentre = await secondLineCentre();
  assert(
    atCentre > atLeft + 5 && atCentre < atRight - 5,
    `and centring puts it between the two (${atLeft} / ${atCentre} / ${atRight})`,
  );
  await page.click('button[aria-label="Align text left"]');
  await page.waitForTimeout(300);

  // Line height and tracking change the block's own measured size, which
  // is what the selection outline is drawn around.
  const outline = async () => {
    const q = await page.$eval(".sel-outline polygon", (el) =>
      el.getAttribute("points").split(" ").map((p) => p.split(",").map(Number)),
    );
    const xs = q.map((p) => p[0]);
    const ys = q.map((p) => p[1]);
    return [Math.max(...xs) - Math.min(...xs), Math.max(...ys) - Math.min(...ys)];
  };
  const plain = await outline();
  await setSlider("Line height", 2);
  await page.waitForTimeout(250);
  const spaced = await outline();
  assert(
    spaced[1] > plain[1] * 1.6 && Math.abs(spaced[0] - plain[0]) < 2,
    `double spacing grew the block downwards only (${plain} -> ${spaced})`,
  );
  await setSlider("Line height", 1);
  await page.waitForTimeout(250);

  await setSlider("Letter spacing", 0.3);
  await page.waitForTimeout(250);
  const tracked = await outline();
  assert(
    tracked[0] > plain[0] * 1.2 && Math.abs(tracked[1] - plain[1]) < 2,
    `tracking grew the block sideways only (${plain} -> ${tracked})`,
  );
  await setSlider("Letter spacing", 0);
  await page.waitForTimeout(250);

  // A wrap width narrows the block and folds its words into more lines.
  await setText("the quick brown fox jumps over the lazy dog");
  await page.waitForTimeout(300);
  const single = await outline();
  await setSlider("Wrap width", 240);
  await page.waitForTimeout(300);
  const folded = await outline();
  assert(
    folded[0] < single[0] * 0.6 && folded[1] > single[1] * 1.8,
    `a wrap width folded one line into several (${single} -> ${folded})`,
  );
  await setSlider("Wrap width", 0);
  await page.waitForTimeout(300);
  const unfolded = await outline();
  // Faces beyond the bundled one are fetched and registered at startup;
  // choosing one re-sets the block, so bold comes out wider.
  await page.waitForFunction(
    () => document.querySelectorAll('select[aria-label="Font"] option').length >= 4,
    null,
    { timeout: 5000 },
  );
  await page.selectOption('select[aria-label="Font"]', "DejaVu Sans Bold");
  await page.waitForTimeout(300);
  const bold = await outline();
  assert(
    bold[0] > unfolded[0] * 1.04 && Math.abs(bold[1] - unfolded[1]) < 2,
    `bold set the block wider, not taller (${unfolded} -> ${bold})`,
  );
  await page.selectOption('select[aria-label="Font"]', "DejaVu Sans");
  await page.waitForTimeout(300);
  assert(
    Math.abs((await outline())[0] - unfolded[0]) < 2,
    "and back to the bundled face it is its old width",
  );
  // The bold toggle asks for weight rather than naming a face: the block
  // stays set in DejaVu Sans and the engine finds the family's registered
  // bold cut, so it comes out the width choosing that face by hand did.
  await page.click('button[aria-label="Bold"]');
  await page.waitForTimeout(300);
  assert(
    (await page.locator('select[aria-label="Font"]').inputValue()) === "DejaVu Sans",
    "the face is still the one that was chosen",
  );
  assert(
    Math.abs((await outline())[0] - bold[0]) < 2,
    `and the bold cut is what set it (${await outline()} vs ${bold})`,
  );
  assert(
    (await page.locator('button[aria-label="Bold"]').getAttribute("aria-pressed")) ===
      "true",
    "the toggle reads as on",
  );
  await page.click('button[aria-label="Bold"]');
  await page.waitForTimeout(300);
  assert(
    Math.abs((await outline())[0] - unfolded[0]) < 2,
    "and again takes it off",
  );
  // Style runs: with part of the text selected, a style button applies
  // to that part rather than to the block.
  {
    await setText("aaaa aaaa");
    await page.waitForTimeout(300);
    const plain = await outline();
    const select = (a, b) =>
      page
        .locator('textarea[aria-label="Text content"]')
        .evaluate(
          (el, [i, j]) => {
            el.focus();
            el.setSelectionRange(i, j);
          },
          [a, b],
        );
    await select(0, 4);
    await page.click('button[aria-label="Bold"]');
    await page.waitForTimeout(300);
    const half = await outline();
    // A toggle reports the stretch it is pointed at: moving the
    // selection changes what it reads with nothing pressed in between,
    // and what it reads is what pressing it will act on.
    await select(0, 4);
    await page.waitForTimeout(50);
    assert(
      (await page.locator('button[aria-label="Bold"]').getAttribute("aria-pressed")) ===
        "true",
      "the toggle follows the selection onto the bolded word",
    );
    await select(5, 9);
    await page.waitForTimeout(50);
    assert(
      (await page.locator('button[aria-label="Bold"]').getAttribute("aria-pressed")) ===
        "false",
      "and reads as off over the plain one",
    );
    await select(0, 9);
    await page.click('button[aria-label="Bold"]');
    await page.waitForTimeout(300);
    const whole = await outline();
    assert(
      plain[0] < half[0] && half[0] < whole[0],
      `bolding half a block sits between plain and all of it (${plain} / ${half} / ${whole})`,
    );
    // And pressing it again over the whole selection takes it back off,
    // leaving no run behind to have bent the block.
    await select(0, 9);
    await page.click('button[aria-label="Bold"]');
    await page.waitForTimeout(300);
    assert(
      Math.abs((await outline())[0] - plain[0]) < 2,
      `and takes it all off again (${await outline()} vs ${plain})`,
    );
    // A colour over a selection paints only there. Where the block sits
    // on the canvas is not this test's business, so it is checked by
    // moving the selection: colour the first word and then the second,
    // and the red ink has to move along with it.
    // Red per column of the canvas, so the page's own red can be
    // subtracted off and only the text's counted.
    const redColumns = () =>
      page.evaluate(() => {
        const el = document.getElementById("engine-canvas");
        const d = el.getContext("2d").getImageData(0, 0, el.width, el.height).data;
        const cols = new Array(el.width).fill(0);
        for (let i = 0; i < d.length; i += 4)
          if (d[i] > 128 && d[i + 1] < 100 && d[i + 2] < 100 && d[i + 3] > 128)
            cols[(i >> 2) % el.width]++;
        return cols;
      });
    // How much red a state added over the baseline, and where its
    // middle sits.
    const addedRed = (cols, base) => {
      let n = 0;
      let sum = 0;
      for (let x = 0; x < cols.length; x++) {
        const d = cols[x] - base[x];
        if (d > 0) {
          n += d;
          sum += d * x;
        }
      }
      return [n, n ? sum / n : 0];
    };
    const paintRed = async (a, b) => {
      await select(a, b);
      await page.locator('input[aria-label="Text color"]').evaluate((el) => {
        const set = Object.getOwnPropertyDescriptor(
          window.HTMLInputElement.prototype,
          "value",
        ).set;
        set.call(el, "#ff0000");
        el.dispatchEvent(new Event("input", { bubbles: true }));
        el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
      });
      await page.waitForTimeout(400);
      return redColumns();
    };
    const none = await redColumns();
    const first = addedRed(await paintRed(0, 4), none);
    assert(
      first[0] > 20,
      `colouring a selection puts red on the page (${first})`,
    );
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(300);
    const second = addedRed(await paintRed(5, 9), none);
    assert(
      Math.abs(second[0] - first[0]) < first[0] * 0.4 && second[1] > first[1] + 10,
      `and colouring the other word puts the same ink further along (${first} -> ${second})`,
    );
    // And the styling follows the text it was put on. Cutting two
    // letters off the front leaves the same four letters red: had the
    // run stayed where it was, it would have run off the shortened end
    // and coloured two of them.
    const third = addedRed(await paintRed(5, 9), none);
    await setText("aa aaaa");
    await page.waitForTimeout(400);
    const moved = addedRed(await redColumns(), none);
    assert(
      moved[0] > third[0] * 0.8,
      `the red is still on all four letters (${third} -> ${moved})`,
    );
    // Leave the block plain and nothing selected, so what follows is
    // testing the block's own styling and not a leftover run.
    await select(0, 0);
    await setText("HHHH");
    await page.waitForTimeout(300);
  }

  // A family with no bold cut registered gets one anyway: the toggle is
  // never disabled, and the rasterizer thickens the upright it has.
  await page.selectOption('select[aria-label="Font"]', "DejaVu Serif");
  await page.waitForTimeout(300);
  const serif = await outline();
  await page.click('button[aria-label="Bold"]');
  await page.waitForTimeout(300);
  const serifBold = await outline();
  assert(
    serifBold[0] > serif[0] && Math.abs(serifBold[1] - serif[1]) < 2,
    `a face with no bold twin is thickened, not left alone (${serif} -> ${serifBold})`,
  );
  await page.click('button[aria-label="Bold"]');
  await page.selectOption('select[aria-label="Font"]', "DejaVu Sans");
  await page.waitForTimeout(300);
  // Italic leans the block: with straight stems on the page, the ink's
  // centre over the top rows moves right of its centre over the bottom
  // rows, and the block widens to make room for the lean.
  {
    await setText("HHHH");
    await page.waitForTimeout(300);
    const tilt = () =>
      page.evaluate(([a, b, c, d]) => {
        const el = document.getElementById("engine-canvas");
        const s = Number(el.dataset.frameScale) || 1;
        const ox = Number(el.dataset.originX) || 0;
        const oy = Number(el.dataset.originY) || 0;
        [a, c] = [a, c].map((v) => Math.round(ox + v * s));
        [b, d] = [b, d].map((v) => Math.round(oy + v * s));
        const w = c - a;
        const img = el.getContext("2d").getImageData(a, b, w, d - b).data;
        const rows = [];
        for (let i = 0; i < img.length; i += 4) {
          if (img[i + 3] > 0) rows.push(Math.floor(i / 4 / w));
        }
        if (rows.length === 0) return null;
        const [y0, y1] = [Math.min(...rows), Math.max(...rows) + 1];
        const third = Math.floor((y1 - y0) / 3);
        const centre = (r0, r1) => {
          let sum = 0;
          let n = 0;
          for (let y = r0; y < r1; y++) {
            for (let x = 0; x < w; x++) {
              const alpha = img[(y * w + x) * 4 + 3];
              sum += (alpha * (x + 0.5)) / s;
              n += alpha;
            }
          }
          return sum / n;
        };
        return centre(y0, y0 + third) - centre(y1 - third, y1);
      }, [700, 540, 1320, 740]);
    const straight = await tilt();
    const uprightWidth = (await outline())[0];
    await page.click('button[aria-label="Italic"]');
    await page.waitForTimeout(300);
    const leaned = await tilt();
    assert(
      Math.abs(straight) < 1 && leaned > 3,
      `italic leans the stems (tilt ${straight?.toFixed(2)} -> ${leaned?.toFixed(2)})`,
    );
    assert(
      (await outline())[0] > uprightWidth + 2 &&
        (await page.getAttribute('button[aria-label="Italic"]', "aria-pressed")) === "true",
      "and the block widens to hold the lean",
    );
    await page.click('button[aria-label="Italic"]');
    await page.waitForTimeout(300);
    assert(Math.abs((await tilt()) ?? 9) < 1, "and again stands the stems up");
    // Underline adds ink below the letters; strike-through adds ink
    // without reaching lower; both come off again.
    const inkRows = () =>
      page.evaluate(([a, b, c, d]) => {
        const el = document.getElementById("engine-canvas");
        const s = Number(el.dataset.frameScale) || 1;
        const ox = Number(el.dataset.originX) || 0;
        const oy = Number(el.dataset.originY) || 0;
        [a, c] = [a, c].map((v) => Math.round(ox + v * s));
        [b, d] = [b, d].map((v) => Math.round(oy + v * s));
        const w = c - a;
        const img = el.getContext("2d").getImageData(a, b, w, d - b).data;
        let last = -1;
        let count = 0;
        for (let i = 0; i < img.length; i += 4) {
          if (img[i + 3] > 0) {
            last = Math.floor(i / 4 / w);
            count++;
          }
        }
        return [last / s, count];
      }, [700, 540, 1320, 740]);
    const [bottom, plainInk] = await inkRows();
    await page.click('button[aria-label="Underline"]');
    await page.waitForTimeout(300);
    const [underBottom, underInk] = await inkRows();
    assert(
      underBottom > bottom + 2 && underInk > plainInk,
      `underline reaches below the letters (${bottom} -> ${underBottom})`,
    );
    await page.click('button[aria-label="Underline"]');
    await page.click('button[aria-label="Strike-through"]');
    await page.waitForTimeout(300);
    const [struckBottom, struckInk] = await inkRows();
    assert(
      Math.abs(struckBottom - bottom) < 1.5 && struckInk > plainInk,
      `strike-through adds ink without reaching lower (${struckBottom} vs ${bottom})`,
    );
    await page.click('button[aria-label="Strike-through"]');
    await page.waitForTimeout(300);
    assert((await inkRows())[1] === plainInk, "and both come off again");
  }
  // A font loaded from a file joins the list and dresses the picked block.
  await page.locator('input[accept=".ttf,.otf"]').setInputFiles("public/fonts/DejaVuSerif.ttf");
  await page.waitForTimeout(600);
  assert(
    (await page.locator('select[aria-label="Font"] option', { hasText: "DejaVuSerif" }).count()) === 1 &&
      (await page.locator('select[aria-label="Font"]').inputValue()) === "DejaVuSerif",
    "a loaded font file is offered under its own name and applied",
  );
  // The face travels inside the saved document: a fresh page — its own
  // engine, a registry that has never seen the file — opens the .chitra
  // and finds the text set in it, with the face on offer by name.
  {
    const [saved] = await Promise.all([
      page.waitForEvent("download"),
      (await menuItem("File", "Save")).click(),
    ]);
    const path = await saved.path();
    const fresh = await browser.newPage({ viewport: { width: 1400, height: 900 } });
    await fresh.goto("http://localhost:8123/");
    await fresh.waitForSelector("#engine-canvas");
    await fresh.waitForTimeout(500);
    await fresh.locator('input[accept=".chitra"]').setInputFiles(path);
    await fresh.waitForTimeout(600);
    await fresh.locator(".panel ul li", { hasText: "Text" }).click();
    await fresh.waitForTimeout(200);
    assert(
      (await fresh.locator('select[aria-label="Font"] option', { hasText: "DejaVuSerif" }).count()) === 1 &&
        (await fresh.locator('select[aria-label="Font"]').inputValue()) === "DejaVuSerif",
      "the loaded face travelled inside the .chitra to a page that never loaded it",
    );
    await fresh.close();
  }
  await page.selectOption('select[aria-label="Font"]', "DejaVu Sans");
  await page.waitForTimeout(300);
  assert(
    Math.abs(unfolded[0] - single[0]) < 2 && Math.abs(unfolded[1] - single[1]) < 2,
    "and zero lets it fit its text again",
  );
  await setText("Hello!");
  await page.waitForTimeout(300);
}

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
const smallPage = await page.locator("#engine-page").boundingBox();
assert(
  Math.abs(smallPage.width / smallPage.height - 600 / 400) < 0.02,
  `the page is drawn in the document's shape (${smallPage.width}x${smallPage.height})`,
);
{
  // Screen/document conversion has to follow the new size, so a drag in
  // the middle of the canvas must paint in the middle of the document.
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(500, 300), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const inside = await canvasPixel(300, 200);
  assert(inside[3] === 255, `drag painted inside the small document (${inside})`);
  assert((await canvasPixel(20, 20))[3] === 0, "and not outside the drag");
}

// 8x1b. Corner radius: the rect's corners round off and its sides stay.
{
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  // The rect spans (100,100)-(500,300) here.
  assert((await canvasPixel(103, 103))[3] === 255, "the corner starts square");
  await setSlider("Corner radius", 60);
  await page.waitForTimeout(300);
  assert((await canvasPixel(103, 103))[3] === 0, "the corner is rounded away");
  assert((await canvasPixel(300, 103))[3] === 255, "the top edge stays flush");
  assert((await canvasPixel(103, 200))[3] === 255, "and so does the left edge");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert((await canvasPixel(103, 103))[3] === 255, "and it undoes as one step");
}

// 8x2. Drop shadow: a live effect on the selected layer. The rect spans
// (100,100)-(500,300) in this 600x400 document, so just past its
// bottom-right corner is empty ground for the shadow to land on.
{
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  assert((await canvasPixel(503, 200))[3] === 0, "ground beside the rect is clear");
  await page.selectOption('select[aria-label="Add effect"]', "DropShadow");
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
  const xSlider = page.locator('input[aria-label="Drop shadow x"]');
  await xSlider.fill("-30");
  await xSlider.dispatchEvent("pointerup");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(503, 200))[3] === 0,
    "re-aiming the shadow vacated where it was",
  );
  const behind = await canvasPixel(80, 200);
  assert(behind[3] > 20, `and it now falls to the left (${behind})`);
  await page.click('button[aria-label="Remove Drop shadow"]');
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

  // Outline: a band hugging the shape, and effects stack rather than
  // replacing one another.
  await page.selectOption('select[aria-label="Add effect"]', "Outline");
  await page.waitForTimeout(300);
  const band = await canvasPixel(502, 200); // two pixels outside the edge
  assert(band[3] > 200 && band[0] < 120, `an outline band appeared (${band})`);
  assert((await canvasPixel(515, 200))[3] === 0, "and stops at the width given");
  assert(
    (await canvasPixel(300, 200))[0] > 100,
    "the layer shows through above its own outline",
  );
  await setSlider("Outline width", 20);
  await page.waitForTimeout(300);
  const wider = await canvasPixel(515, 200);
  assert(wider[3] > 200 && wider[0] < 120, `a wider outline reaches further out (${wider})`);
  await page.selectOption('select[aria-label="Add effect"]', "InnerShadow");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".effect").count()) === 2,
    "both effects are listed, stacked",
  );
  assert(
    (await canvasPixel(515, 200))[3] > 200,
    "and adding one did not replace the other",
  );
  // Order matters: an outline under an inner shadow is a different
  // picture from one over it, so the stack can be rearranged.
  const stackOrder = () =>
    page.$$eval(".effect .effect-head span", (els) => els.map((e) => e.textContent));
  assert(
    (await stackOrder()).join() === "Outline,Inner shadow",
    `the stack lists them in order (${await stackOrder()})`,
  );
  await page.click('button[aria-label="Move Outline up"]');
  await page.waitForTimeout(300);
  assert(
    (await stackOrder()).join() === "Inner shadow,Outline",
    `moving one up rearranged the stack (${await stackOrder()})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await stackOrder()).join() === "Outline,Inner shadow",
    "and the rearrangement undoes",
  );
  await page.click('button[aria-label="Remove Outline"]');
  await page.waitForTimeout(300);
  assert((await page.locator(".effect").count()) === 1, "removing one leaves the other");
  assert((await canvasPixel(515, 200))[3] === 0, "and the band is gone");
  await page.click('button[aria-label="Remove Inner shadow"]');
  await page.waitForTimeout(300);
  assert(
    (await page.locator('.panel ul li [title="This layer has effects"]').count()) === 0,
    "the layer carries no effects again",
  );
}

// 8x3. Dragging one member of a multi-selection carries the rest. Rects
// are used because the drag has to start on the layer it grabs, and a
// rect's centre is reliably on it.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
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
  await pickTool("Move");
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

// 8x1c. Masks beyond the inscribed ellipse: a rectangle, and a shape
// used as the mask of the layer under it.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const drawRect = async (x0, y0, x1, y1) => {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  };
  await drawRect(100, 100, 500, 300);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  // An ellipse mask cuts the corners; a rectangle one does not.
  await page.click("text=Ellipse mask");
  await page.waitForTimeout(300);
  assert((await canvasPixel(110, 110))[3] === 0, "the ellipse mask cut the corner");
  await page.click("text=Remove");
  await page.waitForTimeout(250);
  await page.click("text=Rect mask");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(110, 110))[3] === 255,
    "a rectangle mask keeps the corner it inscribes",
  );
  assert((await canvasPixel(300, 200))[3] === 255, "and the middle with it");
  await page.click("text=Remove");
  await page.waitForTimeout(250);

  // Draw a second shape over it and make it the mask of the one below.
  await drawRect(150, 150, 250, 250);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "two layers before masking",
  );
  await page.click("text=Mask below");
  await page.waitForTimeout(350);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "the shape became the mask and left the stack",
  );
  assert((await canvasPixel(200, 200))[3] === 255, "what the shape covered shows");
  assert((await canvasPixel(400, 200))[3] === 0, "and what it did not is hidden");
  assert(
    (await page
      .locator('.panel ul li [title="What this layer\'s mask lets through"]')
      .count()) === 1,
    "the row marks the mask it now carries",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === 2 &&
      (await canvasPixel(400, 200))[3] === 255,
    "and the whole thing undoes as one step",
  );
}

// 8x2a. The Edit and View menus carry what the shortcuts do, so it can
// be found without knowing it; select-all and zoom-to-selection work.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  for (const [x0, y0, x1, y1] of [
    [60, 60, 160, 160],
    [400, 250, 500, 350],
  ]) {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  }
  await pickTool("Move");
  await page.waitForTimeout(150);
  for (const item of ["Cut", "Copy", "Paste", "Duplicate", "Delete", "Select all"]) {
    const found = await menuItem("Edit", item);
    assert((await found.count()) >= 1, `the Edit menu offers ${item}`);
    await page.keyboard.press("Escape");
  }
  await menuClick("Edit", "Select all");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 2,
    "select all picked both layers",
  );
  assert(
    (await page.locator('button[aria-label="Unite shapes"]').count()) === 1,
    "and a two-layer selection offers what two-layer selections offer",
  );
  // The shortcut does the same thing the menu item does.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 0,
    "escape dropped it again",
  );
  await page.keyboard.press("Control+a");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 2,
    "ctrl+a picked both layers",
  );
  // Zoom to selection frames both, so the page no longer fits the window.
  const fitted = await page.locator("#engine-page").boundingBox();
  await menuClick("View", "Zoom to selection");
  await page.waitForTimeout(300);
  const framed = await page.locator("#engine-page").boundingBox();
  assert(
    framed.width > fitted.width * 1.1,
    `zoom to selection framed the picked layers (${fitted.width} -> ${framed.width})`,
  );
  await menuClick("View", "Actual size");
  await page.waitForTimeout(300);
  assert(
    await page.isVisible("text=· 100%"),
    "actual size reports one hundred per cent",
  );
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li.selected").count()) === 0,
    "escape let the selection go",
  );
  await menuClick("View", "Fit document to window");
  await page.waitForTimeout(200);
}

// 8x2b. Rulers and guides: drag one out of a ruler, snap to it, put it
// back. Guides are document state, so they undo.
{
  await newDocument(600, 400, "rgb");
  const host = await page.locator(".canvas-host").boundingBox();
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  assert(
    (await page.locator(".ruler").count()) === 2,
    "the canvas has a ruler on each edge",
  );
  assert((await page.locator(".guide-overlay .guide").count()) === 0, "and no guides yet");
  // Drag a vertical guide out of the left ruler to document x = 300.
  await page.mouse.move(host.x + 8, host.y + 200);
  await page.mouse.down();
  await page.mouse.move(...at(300, 200), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".guide-overlay .guide").count()) === 1,
    "dragging out of a ruler placed a guide",
  );
  // A layer dropped near it is pulled onto it.
  await pickTool("Rect");
  await page.mouse.move(...at(80, 80));
  await page.mouse.down();
  await page.mouse.move(...at(180, 180), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const leftField = page.locator('input[aria-label="X position"]');
  // Aim the rect's left edge three document pixels short of the guide.
  await page.mouse.move(...at(130, 130));
  await page.mouse.down();
  await page.mouse.move(...at(347, 130), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const landed = Number(await leftField.inputValue());
  assert(
    Math.abs(landed - 300) < 1.5,
    `the drop caught the guide rather than where it was aimed (${landed})`,
  );
  // Throw the guide away by dragging it back onto the ruler.
  // A guide line has no area, so grab it by the position it reports.
  const guideX = await page.$eval(".guide-overlay .guide-hit", (el) =>
    Number(el.getAttribute("x1")),
  );
  await page.mouse.move(host.x + guideX, host.y + 250);
  await page.mouse.down();
  await page.mouse.move(host.x + 6, host.y + 200, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".guide-overlay .guide").count()) === 0,
    "and dropping it back on the ruler threw it away",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".guide-overlay .guide").count()) === 1,
    "which undoes like any other edit",
  );
  await page.screenshot({ path: join(OUT, "guides.png") });
}

// 8x3a. Typed geometry: the position and size fields place a layer
// exactly, and each edit is one history entry.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(200, 180), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const field = (label) => page.locator(`input[aria-label="${label}"]`);
  const geometry = async () =>
    Promise.all(
      ["X position", "Y position", "W size", "H size"].map((l) =>
        field(l).inputValue().then(Number),
      ),
    );
  const drawn = await geometry();
  assert(
    Math.abs(drawn[0] - 100) < 2 && Math.abs(drawn[2] - 100) < 2,
    `the fields report where it was drawn (${drawn})`,
  );
  // Type an exact position: the shape moves and the fields agree.
  await field("X position").fill("300");
  await field("X position").press("Enter");
  await page.waitForTimeout(300);
  assert((await canvasPixel(350, 140))[3] === 255, "typing X moved the layer");
  assert((await canvasPixel(150, 140))[3] === 0, "and vacated where it was");
  // Then an exact size, anchored at the corner the position names.
  await field("W size").fill("200");
  await field("W size").press("Enter");
  await page.waitForTimeout(300);
  const resized = await geometry();
  assert(
    Math.abs(resized[0] - 300) < 2 && Math.abs(resized[2] - 200) < 2,
    `typing W resized it about its own corner (${resized})`,
  );
  assert((await canvasPixel(490, 140))[3] === 255, "the layer reaches its new width");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await canvasPixel(490, 140))[3] === 0, "the resize undoes on its own");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await canvasPixel(150, 140))[3] === 255, "and so does the move");
}

// 8x3b. Booleans: two overlapping rects combine into one compound path,
// and subtracting an enclosed one punches a hole.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const drawRect = async (x0, y0, x1, y1) => {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  };
  await drawRect(100, 100, 300, 300);
  await drawRect(200, 200, 400, 340);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.locator(".panel ul li").nth(1).click({ modifiers: ["Control"] });
  await page.waitForTimeout(200);
  assert(
    (await page.locator('button[aria-label="Unite shapes"]').count()) === 1,
    "a multi-selection offers the boolean operations",
  );
  await page.click('button[aria-label="Unite shapes"]');
  await page.waitForTimeout(400);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "uniting left a single layer",
  );
  assert((await canvasPixel(150, 150))[3] === 255, "the first shape survives");
  assert((await canvasPixel(350, 320))[3] === 255, "and so does the second");
  assert((await canvasPixel(450, 150))[3] === 0, "and nothing outside them");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "and the union undoes as one step",
  );

  // Subtract an enclosed shape: the result is one layer with a hole in it.
  await newDocument(600, 400, "rgb");
  await drawRect(100, 100, 500, 300);
  await drawRect(250, 170, 350, 230);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.locator(".panel ul li").nth(1).click({ modifiers: ["Control"] });
  await page.waitForTimeout(200);
  await page.click('button[aria-label="Subtract the shapes above"]');
  await page.waitForTimeout(400);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "subtracting left a single layer",
  );
  assert((await canvasPixel(150, 200))[3] === 255, "the ring is filled");
  assert((await canvasPixel(300, 200))[3] === 0, "and the middle is a hole");
  assert((await canvasPixel(50, 200))[3] === 0, "outside is still outside");
}

// 8x4. Crop: drag a frame with the crop tool and the page becomes it,
// with the picture still where it was inside the frame.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(200, 150));
  await page.mouse.down();
  await page.mouse.move(...at(400, 250), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert((await canvasPixel(300, 200))[3] === 255, "a rect in the middle");

  await pickTool("Crop");
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(500, 300), { steps: 8 });
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".crop-frame").count()) === 1,
    "the crop frame shows while dragging",
  );
  await page.mouse.up();
  await page.waitForTimeout(400);
  assert(
    await page.isVisible("text=RGB, 400×200"),
    "the page became the cropped rectangle",
  );
  assert(
    (await page.locator(".crop-frame").count()) === 0,
    "and the frame clears when the crop lands",
  );
  // The rect was at (200,150)-(400,250); after cropping from (100,100) it
  // sits at (100,50)-(300,150) on the new page.
  assert((await canvasPixel(200, 100))[3] === 255, "the picture stayed framed");
  assert((await canvasPixel(20, 20))[3] === 0, "and its surroundings came with it");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(400);
  assert(await page.isVisible("text=RGB, 600×400"), "undo restores the page");
  assert((await canvasPixel(300, 200))[3] === 255, "and puts the picture back");

  // A frame dragged from off the page crops to the page's own edge, so
  // the document comes out the size of what was actually framed rather
  // than the size the pointer travelled. The page is back to 600x400, and
  // the view is wherever the last crop left it, so both are re-read.
  await menuClick("View", "Fit document to window");
  await page.waitForTimeout(250);
  const cropped = await page.locator("#engine-page").boundingBox();
  await pickTool("Crop");
  // Just left of the page but still over the canvas, clear of the ruler.
  await page.mouse.move(cropped.x - 20, cropped.y + (100 / 400) * cropped.height);
  await page.mouse.down();
  await page.mouse.move(
    cropped.x + (200 / 600) * cropped.width,
    cropped.y + (150 / 400) * cropped.height,
    { steps: 8 },
  );
  await page.mouse.up();
  await page.waitForTimeout(400);
  assert(
    await page.isVisible("text=RGB, 200×50"),
    "a crop started off the page stops at its edge",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(400);

}

// 8x5. Images from anywhere: dropped on the canvas or pasted from another
// application, they become layers. And undo after a delete brings the
// selection back with the layer.
{
  await newDocument(600, 400, "rgb");
  // A 40x30 solid red PNG made in the page, handed over as a File the way
  // a drop or a paste would hand it.
  const makeImage = () =>
    page.evaluateHandle(async () => {
      const c = document.createElement("canvas");
      c.width = 40;
      c.height = 30;
      const g = c.getContext("2d");
      g.fillStyle = "#ff0000";
      g.fillRect(0, 0, 40, 30);
      const blob = await new Promise((r) => c.toBlob(r, "image/png"));
      return new File([blob], "shot.png", { type: "image/png" });
    });
  const dropped = await makeImage();
  await page.evaluate((file) => {
    const dt = new DataTransfer();
    dt.items.add(file);
    const host = document.querySelector(".canvas-host");
    host.dispatchEvent(new DragEvent("drop", { bubbles: true, cancelable: true, dataTransfer: dt }));
  }, dropped);
  await page.waitForTimeout(400);
  assert(
    (await page.locator(".panel ul li", { hasText: "shot.png" }).count()) === 1,
    "a dropped image file became a layer named after the file",
  );
  assert((await canvasPixel(10, 10))[0] > 200, "and its pixels landed at the origin");
  assert(
    (await page.locator(".panel ul li.selected").count()) === 1,
    "and it is picked, ready to move",
  );

  const pasted = await makeImage();
  await page.evaluate((file) => {
    const dt = new DataTransfer();
    dt.items.add(file);
    window.dispatchEvent(new ClipboardEvent("paste", { bubbles: true, cancelable: true, clipboardData: dt }));
  }, pasted);
  await page.waitForTimeout(400);
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "a pasted image became a second layer",
  );

  // Delete the picked layer, undo, and the selection is back on it.
  await page.keyboard.press("Delete");
  await page.waitForTimeout(250);
  assert((await page.locator(".panel ul li").count()) === 1, "delete took it away");
  assert((await page.locator(".panel ul li.selected").count()) === 0, "nothing picked");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await page.locator(".panel ul li").count()) === 2, "undo brought it back");
  assert(
    (await page.locator(".panel ul li.selected").count()) === 1,
    "and the selection came back with it",
  );
}

// 8x6. An adjustment scoped to one layer: two rects, darken only one.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  for (const [x0, y0, x1, y1] of [[50, 50, 250, 250], [350, 50, 550, 250]]) {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  }
  await pickTool("Move");
  await page.locator(".panel ul li").first().click(); // the right-hand rect, drawn last
  await page.waitForTimeout(200);
  const leftBefore = await canvasPixel(150, 150);
  const rightBefore = await canvasPixel(450, 150);
  assert(
    (await page.locator('select[aria-label="Add adjustment layer"] optgroup').count()) === 2,
    "with a layer picked, +FX offers to scope to it",
  );
  await page.selectOption('select[aria-label="Add adjustment layer"]', "only:exposure");
  await page.waitForTimeout(400);
  assert(
    (await page.locator(".panel ul li").count()) === 4 &&
      (await page.locator(".panel ul li", { hasText: "+ Exposure" }).count()) === 1,
    "the layer and its adjustment sit together in a group named for both",
  );
  await setSlider("Stops", -3);
  await page.waitForTimeout(300);
  const leftAfter = await canvasPixel(150, 150);
  const rightAfter = await canvasPixel(450, 150);
  assert(
    rightAfter[2] < rightBefore[2] * 0.6,
    `the adjusted rect darkened (${rightBefore} -> ${rightAfter})`,
  );
  assert(
    leftAfter.join() === leftBefore.join(),
    `and the other rect did not (${leftBefore} -> ${leftAfter})`,
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === 2 &&
      (await canvasPixel(450, 150)).join() === rightBefore.join(),
    "two undos: the slider, then the whole arrangement",
  );
}

// 8x6b. Clipping to the layer below: the upper layer shows only where the
// lower one does, goes when it goes, and an adjustment clipped the same
// way changes only what the layer under it covers.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const draw = async (x0, y0, x1, y1) => {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  };
  await draw(100, 100, 300, 300);
  await draw(50, 50, 550, 350);
  await pickTool("Move");
  assert(
    (await canvasPixel(450, 200))[3] === 255,
    "the upper rect covers the page on its own",
  );
  // The top row of the panel is the layer drawn last.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(150);
  await page.click('button[aria-label="Clip to the layer below"]');
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(200, 200))[3] === 255,
    "clipped, it still covers what it is clipped to",
  );
  assert(
    (await canvasPixel(450, 200))[3] === 0,
    "and shows nothing past that layer's edge",
  );
  assert(
    (await page.locator(".clip-mark").count()) === 1,
    "the row says so with a hook",
  );

  // It goes when the layer under it goes.
  await page.locator(".panel ul li").nth(1).locator("button.visibility").click();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(200, 200))[3] === 0,
    "hiding the layer below takes the clipped layer with it",
  );
  await page.locator(".panel ul li").nth(1).locator("button.visibility").click();
  await page.waitForTimeout(300);

  // Ctrl+Alt+G is the same switch, so it lets the layer out again.
  await page.keyboard.press("Control+Alt+g");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(450, 200))[3] === 255 &&
      (await page.locator(".clip-mark").count()) === 0,
    "the shortcut releases it",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(450, 200))[3] === 0,
    "and clipping is one undo step",
  );
}

{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  for (const [x0, y0, x1, y1] of [
    [50, 50, 250, 250],
    [350, 50, 550, 250],
  ]) {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(200);
  }
  await pickTool("Move");
  const leftBefore = await canvasPixel(150, 150);
  const rightBefore = await canvasPixel(450, 150);
  await page.selectOption(
    'select[aria-label="Add adjustment layer"]',
    "exposure",
  );
  await page.waitForTimeout(300);
  // An adjustment over everything below lands on top without being
  // picked; its controls come with picking it.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await setSlider("Stops", -3);
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(150, 150))[2] < leftBefore[2] * 0.6 &&
      (await canvasPixel(450, 150))[2] < rightBefore[2] * 0.6,
    "unclipped, the adjustment darkens both rects",
  );
  await page.click('button[aria-label="Clip to the layer below"]');
  await page.waitForTimeout(400);
  assert(
    (await canvasPixel(150, 150)).join() === leftBefore.join(),
    "clipped, it lets the rect it is not over alone",
  );
  assert(
    (await canvasPixel(450, 150))[2] < rightBefore[2] * 0.6,
    "and still darkens the one below it",
  );
}

// 8x6c. Artboards: a frame dragged out of the Frame tool is a page within
// the page — it grounds what is in it, cuts it to its box, takes what is
// drawn inside it, and exports at its own size on its own.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const drag = async (x0, y0, x1, y1) => {
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
  };
  await pickTool("Frame");
  await drag(100, 100, 300, 300);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "the frame is a layer",
  );
  const white = await canvasPixel(200, 200);
  assert(
    white.slice(0, 3).join() === "255,255,255" && white[3] === 255,
    `the frame grounds its box (${white})`,
  );
  assert(
    (await canvasPixel(50, 50))[3] === 0,
    "and only its box: the page around it is bare",
  );

  // A shape drawn inside the frame goes into the frame, where it was drawn.
  await pickTool("Rect");
  await drag(150, 150, 250, 250);
  const rows = await page.locator(".panel ul li").count();
  assert(rows === 2, `the rect is a layer too (${rows})`);
  assert(
    (await page.locator(".panel ul li").first().getAttribute("style")) !==
      (await page.locator(".panel ul li").nth(1).getAttribute("style")),
    "and it is indented under the frame, not beside it",
  );
  const painted = await canvasPixel(200, 200);
  assert(
    painted.slice(0, 3).join() !== "255,255,255",
    `the rect landed where it was drawn (${painted})`,
  );

  // The frame folds shut in the panel, taking what is in it with it,
  // and a folded frame is still a frame — it exports on its own.
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "both rows are listed",
  );
  await page.click('button[aria-label^="Fold "]');
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li").count()) === 1,
    "folded, the frame's contents are out of the list",
  );
  await page.click('button[aria-label^="Open "]');
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "and opening it brings them back",
  );
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const [oneBoard] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export this artboard")).click(),
  ]);
  {
    const bytes = await readFile(await oneBoard.path());
    assert(
      bytes.readUInt32BE(16) === 200 && bytes.readUInt32BE(20) === 200,
      `the picked frame exports at its own size (${bytes.readUInt32BE(16)}x${bytes.readUInt32BE(20)})`,
    );
  }

  // Dragged out past the frame's edge, the rect is cut at the edge. The
  // panel runs top-first and the frame owns the rect, so the frame is
  // the first row and the rect the second.
  await pickTool("Move");
  await page.locator(".panel ul li").nth(1).click();
  await page.waitForTimeout(200);
  await page.mouse.move(...at(200, 200));
  await page.mouse.down();
  await page.mouse.move(...at(300, 200), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const kept = await canvasPixel(270, 200);
  assert(
    kept[3] === 255 && kept.slice(0, 3).join() !== "255,255,255",
    `the part still inside the frame is drawn (${kept})`,
  );
  const gone = await canvasPixel(320, 200);
  assert(
    gone[3] === 0,
    `and what left the frame is not drawn outside it (${gone})`,
  );
}

// 8x6d. A frame is resized, not scaled: dragging its corner changes how
// many pixels it is, and what is in it stays the size it was. Its ground
// can be turned off, which makes it a window onto the page.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const drag = async (x0, y0, x1, y1) => {
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
  };
  await pickTool("Frame");
  await drag(100, 100, 300, 300);
  await pickTool("Rect");
  await drag(120, 120, 180, 180);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(250);
  const size = () => page.locator('input[aria-label="W size"]').inputValue();
  assert((await size()) === "200", `the frame is 200 wide (${await size()})`);
  const inner = await canvasPixel(150, 150);

  // Pull the south-east corner out, shift held so the drag is free of
  // the frame's proportions. The frame gets bigger; the rect in it does
  // not.
  const se = await page.locator(".handle.se").boundingBox();
  await page.keyboard.down("Shift");
  await page.mouse.move(se.x + se.width / 2, se.y + se.height / 2);
  await page.mouse.down();
  await page.mouse.move(...at(500, 380), { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await page.waitForTimeout(350);
  const grown = Number(await size());
  assert(grown > 380, `the frame grew to ${grown}`);
  assert(
    (await canvasPixel(150, 150)).join() === inner.join(),
    "and the rect in it is where and what it was",
  );
  assert(
    (await canvasPixel(190, 150)).slice(0, 3).join() === "255,255,255",
    "still its own size — past its edge is the frame's ground again",
  );
  assert(
    (await canvasPixel(400, 350)).slice(0, 3).join() === "255,255,255",
    "the ground reaches the frame's new edge",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await size()) === "200",
    `one undo puts the frame back (${await size()})`,
  );

  // Pinned to the right, a layer keeps its distance from that edge when
  // the frame grows; pinned to both sides it takes up the difference.
  await page.locator(".panel ul li").nth(1).click();
  await page.waitForTimeout(200);
  assert(
    (await page.locator('select[aria-label="Pinned across"]').count()) === 1,
    "a layer inside a frame is asked what it holds on to",
  );
  // Dragged-out geometry lands on fractions of a pixel, so these read
  // within a pixel rather than exactly.
  const near = (a, b) => Math.abs(Number(a) - b) < 1.5;
  const rectX = () =>
    page.locator('input[aria-label="X position"]').inputValue();
  const rectW = () => page.locator('input[aria-label="W size"]').inputValue();
  assert(near(await rectX(), 120), `the rect starts at 120 (${await rectX()})`);
  await page.selectOption('select[aria-label="Pinned across"]', "End");
  await page.waitForTimeout(200);
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const fw = page.locator('input[aria-label="W size"]');
  await fw.fill("300");
  await fw.press("Enter");
  await page.waitForTimeout(300);
  await page.locator(".panel ul li").nth(1).click();
  await page.waitForTimeout(200);
  assert(
    near(await rectX(), 220),
    `pinned right, it moved with the edge (${await rectX()})`,
  );
  assert(near(await rectW(), 60), `and kept its size (${await rectW()})`);
  await page.selectOption('select[aria-label="Pinned across"]', "Stretch");
  await page.waitForTimeout(200);
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const fw2 = page.locator('input[aria-label="W size"]');
  await fw2.fill("400");
  await fw2.press("Enter");
  await page.waitForTimeout(300);
  await page.locator(".panel ul li").nth(1).click();
  await page.waitForTimeout(200);
  assert(
    near(await rectW(), 160),
    `pinned to both sides, it took up the difference (${await rectW()})`,
  );
  // Back to where the rest of the block expects the frame.
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  assert(
    near(await size(), 200),
    `four undos put the frame and its pins back (${await size()})`,
  );

  // Typed, the width is the frame's own size too.
  const w = page.locator('input[aria-label="W size"]');
  await w.fill("260");
  await w.press("Enter");
  await page.waitForTimeout(300);
  assert((await size()) === "260", `typed size takes (${await size()})`);
  assert(
    (await canvasPixel(150, 150)).join() === inner.join(),
    "and still does not touch what is inside",
  );

  // A preset gives the frame a named size, in pixels the document's
  // resolution works out for paper.
  await page.selectOption('select[aria-label="Frame size preset"]', {
    label: "Desktop 1920 × 1080",
  });
  await page.waitForTimeout(300);
  assert(
    (await size()) === "1920",
    `a preset sets the frame's size (${await size()})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await size()) === "260", "and it is one undo");

  // No ground: the frame becomes a window onto the page.
  await page.uncheck('input[aria-label="Frame has a ground"]');
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(250, 250))[3] === 0,
    "with no ground the frame shows the page through it",
  );
  assert(
    (await canvasPixel(150, 150))[3] === 255,
    "and what is in it is still drawn",
  );
}

// 8x6e. A live copy: it draws whatever the layer it follows holds, where
// the copy is, so changing the original changes the copy — and moving the
// original moves only the original.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(60, 60));
  await page.mouse.down();
  await page.mouse.move(...at(160, 160), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  // Drawing does not pick; the copy button acts on what is picked.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const original = await canvasPixel(100, 100);
  assert(original[3] === 255, `something to copy (${original})`);

  await page.click('button[aria-label="Make a live copy"]');
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === 2,
    "the copy is a layer of its own",
  );
  assert(
    (await page.locator(".copy-of").count()) === 1,
    "and the panel says what it follows",
  );
  // It starts on top of the original; drag it clear.
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(400, 100), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const moved = await canvasPixel(400, 100);
  assert(
    moved.join() === original.join(),
    `the copy draws what the original does (${original} -> ${moved})`,
  );
  assert(
    (await canvasPixel(100, 100)).join() === original.join(),
    "and the original is still where it was",
  );

  // Change the original's colour: the copy changes with it.
  await page.locator(".panel ul li").nth(1).click();
  await page.waitForTimeout(200);
  await setColor("Fill color", "#00ff00");
  await page.waitForTimeout(350);
  const green = await canvasPixel(100, 100);
  assert(
    green[1] > green[0] && green[1] > green[2],
    `the original went green (${green})`,
  );
  assert(
    (await canvasPixel(400, 100)).join() === green.join(),
    `and the copy went with it (${await canvasPixel(400, 100)})`,
  );

  // Moving the original leaves the copy where it was put.
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(100, 300), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(400, 100)).join() === green.join(),
    "moving the original moved only the original",
  );
  assert((await canvasPixel(100, 300))[3] === 255, "which did move");

  // A copy can differ where it has to: give it one of the original's
  // layers as its own, change that, and everything else still follows.
  await newDocument(600, 400, "rgb");
  const b2 = await page.locator("#engine-page").boundingBox();
  const at2 = (x, y) => [
    b2.x + (x / 600) * b2.width,
    b2.y + (y / 400) * b2.height,
  ];
  const box = async (x0, y0, x1, y1) => {
    await pickTool("Rect");
    await page.mouse.move(...at2(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at2(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
  };
  await box(40, 40, 100, 100);
  await box(120, 40, 180, 100);
  await pickTool("Move");
  await menuClick("Edit", "Select all");
  await page.waitForTimeout(150);
  await page.click('button[aria-label="Group selected layers (ctrl-click to select several)"]');
  await page.waitForTimeout(250);
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(150);
  await page.click('button[aria-label="Make a live copy"]');
  await page.waitForTimeout(300);
  // Move the copy clear of the original.
  await page.mouse.move(...at2(70, 70));
  await page.mouse.down();
  await page.mouse.move(...at2(370, 70), { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(370, 70))[3] === 255 &&
      (await canvasPixel(450, 70))[3] === 255,
    "the copy draws both of the original's layers",
  );
  const rows = await page
    .locator('[aria-label="Follows the original in"] .row')
    .count();
  assert(rows === 2, `the panel offers both of them (${rows})`);

  // Take the second one for the copy's own and hide it: the copy loses
  // that layer and keeps the other, and the original keeps both.
  await page.click('button[aria-label^="Give this copy its own"] >> nth=1');
  await page.waitForTimeout(300);
  await page.locator(".panel ul li").nth(1).locator("button.visibility").click();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(450, 70))[3] === 0,
    "the copy's own layer went when it was hidden",
  );
  assert(
    (await canvasPixel(370, 70))[3] === 255,
    "and the one it still shares stayed",
  );
  assert(
    (await canvasPixel(150, 70))[3] === 255,
    "the original kept both of its own",
  );
  // Following again brings it back.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await page.click('button[aria-label^="Follow the original"]');
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(450, 70))[3] === 255,
    "and following the original again brings it back",
  );
}


// 8x7. Export at a scale, and export just the selection. The PNG's IHDR
// says how big the picture came out.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(300, 250), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const pngSize = async (dl) => {
    const bytes = await readFile(await dl.path());
    return [bytes.readUInt32BE(16), bytes.readUInt32BE(20)];
  };
  const [twoX] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export PNG at 2×")).click(),
  ]);
  assert((await pngSize(twoX)).join("x") === "1200x800", "2x export is twice the page");
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  // Units: the geometry fields and rulers can read in millimetres or
  // inches through the document's resolution (72 dpi here), and a value
  // typed in those units lands in pixels.
  await page.waitForTimeout(200);
  await menuClick("View", "Millimetres");
  await page.waitForTimeout(200);
  const wMm = Number(await page.locator('input[aria-label="W size"]').inputValue());
  assert(Math.abs(wMm - 70.56) < 0.05, `200px at 72dpi reads as 70.56 mm (${wMm})`);
  assert(
    (await page.locator(".topbar").innerText()).includes("mm"),
    "the status line says the page's size on paper",
  );
  const rulerLabels = await page.locator(".ruler-top text").allTextContents();
  assert(
    rulerLabels.every((l) => Number(l) <= 300),
    `ruler ticks are millimetres now (${rulerLabels.slice(0, 5)})`,
  );
  await page.locator('input[aria-label="W size"]').fill("35.28");
  await page.locator('input[aria-label="W size"]').press("Enter");
  await page.waitForTimeout(250);
  await menuClick("View", "Pixels");
  await page.waitForTimeout(200);
  const wPx = Number(await page.locator('input[aria-label="W size"]').inputValue());
  assert(Math.abs(wPx - 100) < 0.6, `35.28 mm typed is 100 px (${wPx})`);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  await page.waitForTimeout(200);
  const [sel] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export selection as PNG")).click(),
  ]);
  const size = await pngSize(sel);
  assert(
    Math.abs(size[0] - 200) <= 1 && Math.abs(size[1] - 150) <= 1,
    `the selection export is the rect's own size (${size})`,
  );
  // The same picture goes out to other applications through the system
  // clipboard, as a PNG of the selection's box.
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"], {
    origin: "http://localhost:8123",
  });
  await menuClick("Edit", "Copy as image");
  await page.waitForTimeout(400);
  const onClipboard = await page.evaluate(async () => {
    const items = await navigator.clipboard.read();
    const item = items.find((i) => i.types.includes("image/png"));
    if (!item) return null;
    const bitmap = await createImageBitmap(await item.getType("image/png"));
    return [bitmap.width, bitmap.height];
  });
  assert(
    onClipboard && Math.abs(onClipboard[0] - 200) <= 1 && Math.abs(onClipboard[1] - 150) <= 1,
    `Copy as image put a PNG of the selection on the system clipboard (${onClipboard})`,
  );

  // 8x8. Flip: a pair mirrors about the box the two of them span, so
  // they trade sides; vertical trades top for bottom; undo puts it back.
  await pickTool("Ellipse");
  await page.mouse.move(...at(400, 300));
  await page.mouse.down();
  await page.mouse.move(...at(500, 380), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await menuClick("Edit", "Select all");
  await page.waitForTimeout(150);
  const filled = async (x, y) => (await canvasPixel(x, y))[3] === 255;
  assert((await filled(150, 175)) && !(await filled(150, 340)), "the rect is left, the ellipse right");
  await menuClick("Edit", "Flip horizontal");
  await page.waitForTimeout(250);
  assert(
    !(await filled(150, 175)) && (await filled(150, 340)) && (await filled(450, 175)),
    "flipped horizontally, they trade sides",
  );
  await menuClick("Edit", "Flip vertical");
  await page.waitForTimeout(250);
  assert(
    (await filled(150, 140)) && !(await filled(150, 340)) && (await filled(450, 300)),
    "flipped vertically, they trade rows",
  );
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(120);
  }
  assert(
    (await filled(150, 175)) && !(await filled(150, 340)) && !(await filled(450, 340)),
    "three undos: both flips and the ellipse are gone",
  );

  // 8x9. Text on a path: a block set along the ellipse's outline leaves
  // its own spot and rings the ellipse; undo takes it off again.
  await pickTool("Ellipse");
  await page.mouse.move(...at(400, 300));
  await page.mouse.down();
  await page.mouse.move(...at(500, 380), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Text");
  await page.mouse.click(...at(60, 350));
  await page.waitForTimeout(300);
  await pickTool("Move");
  await page.mouse.click(...at(75, 365));
  await page.waitForTimeout(200);
  assert((await inkCount(40, 340, 260, 420)) > 50, "the block sits where it was placed");
  const guide = await page
    .locator('select[aria-label="Along"] option', { hasText: "Ellipse" })
    .first()
    .getAttribute("value");
  await page.selectOption('select[aria-label="Along"]', guide);
  await page.waitForTimeout(300);
  assert(
    (await inkCount(40, 340, 260, 420)) === 0 && (await inkCount(370, 240, 570, 440)) > 50,
    "set along the ellipse, the text leaves its spot and rings it",
  );
  assert(
    (await page.locator('select[aria-label="Along"]').inputValue()) === "on" &&
      (await page.locator('input[aria-label="Path offset"]').count()) === 1,
    "the panel says so and offers an offset",
  );
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(120);
  }
  assert(
    (await inkCount(370, 240, 570, 440)) === 0 && (await inkCount(40, 340, 260, 420)) === 0,
    "three undos: the attachment, the text and the ellipse are gone",
  );

  // 8x10. Reordering by drag: carry the top row below the one under it
  // and the stack turns over; one undo turns it back.
  await pickTool("Ellipse");
  await page.mouse.move(...at(400, 300));
  await page.mouse.down();
  await page.mouse.move(...at(500, 380), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const names = async () =>
    (await page.locator(".panel ul li .layer-name").allTextContents()).map((n) => n.trim());
  const before = await names();
  assert(before.length === 2 && before[0].startsWith("Ellipse"), `the ellipse is on top (${before})`);
  const rows = page.locator(".panel ul li");
  const [top, bottom] = [await rows.nth(0).boundingBox(), await rows.nth(1).boundingBox()];
  await page.mouse.move(top.x + 40, top.y + top.height / 2);
  await page.mouse.down();
  await page.mouse.move(top.x + 40, bottom.y + bottom.height * 0.85, { steps: 6 });
  assert(
    (await rows.nth(1).getAttribute("class")).includes("drop-below"),
    "a line shows where the row would land",
  );
  await page.mouse.up();
  await page.waitForTimeout(250);
  const after = await names();
  assert(
    after[0] === before[1] && after[1] === before[0],
    `dropped below the rect, the ellipse goes under it (${after})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert((await names()).join() === before.join(), "one undo turns the stack back");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);

  // 8x11. Placing an SVG brings its shapes in as a group of editable
  // layers, drawn where the file drew them; one undo takes it all back.
  const beforeSvg = await names();
  const mark = `<svg xmlns="http://www.w3.org/2000/svg" width="120" height="100">
    <rect id="box" x="10" y="10" width="40" height="30" fill="#00aa00"/>
    <circle cx="80" cy="25" r="15" fill="#ff8800"/></svg>`;
  await page.setInputFiles('input[accept="image/png,image/jpeg,image/svg+xml"]', {
    name: "mark.svg",
    mimeType: "image/svg+xml",
    buffer: Buffer.from(mark),
  });
  await page.waitForTimeout(400);
  const placed = await names();
  assert(
    placed[0] === "mark.svg" && placed.includes("box") && placed.length === beforeSvg.length + 3,
    `the SVG is a group with its shapes as layers (${placed})`,
  );
  const green = await canvasPixel(30, 25);
  assert(green[1] > 150 && green[0] < 60, `the rect is drawn where the file put it (${green})`);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await names()).join() === beforeSvg.join() && (await canvasPixel(30, 25))[3] === 0,
    "one undo takes the whole drawing back",
  );

  // 8x12. Locking: a locked layer is not picked on the canvas and offers
  // no handles; unlocking gives it back.
  await pickTool("Move");
  await page.mouse.click(...at(200, 175));
  await page.waitForTimeout(200);
  assert((await page.locator(".handle").count()) > 0, "the rect is picked and handled to begin with");
  await page.click('button[aria-label="Lock layer"]');
  await page.waitForTimeout(150);
  await page.keyboard.press("Escape");
  await page.mouse.click(...at(200, 175));
  await page.waitForTimeout(200);
  assert((await page.locator(".sel-outline").count()) === 0, "a click on a locked layer picks nothing");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".sel-outline").count()) >= 1 && (await page.locator(".handle").count()) === 0,
    "picked from the panel it shows its outline but no handles",
  );
  await page.click('button[aria-label="Unlock layer"]');
  await page.waitForTimeout(150);
  await page.keyboard.press("Escape");
  await page.mouse.click(...at(200, 175));
  await page.waitForTimeout(200);
  assert((await page.locator(".handle").count()) > 0, "unlocked, it is picked and handled again");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  // A drop that carries a row out from under the pointer swallows the
  // click that follows it — but only that one: the next click picks.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".panel ul li.selected").count()) === 1,
    "a row still picks after an earlier drag",
  );

  // 8x13. A band dragged over empty canvas picks everything it touches;
  // a locked layer stays out of it, and shift adds to what is picked.
  await pickTool("Ellipse");
  await page.mouse.move(...at(380, 60));
  await page.mouse.down();
  await page.mouse.move(...at(500, 160), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
  // From a bare corner across both shapes.
  await page.mouse.move(...at(560, 380));
  await page.mouse.down();
  await page.mouse.move(...at(400, 200), { steps: 4 });
  assert((await page.locator(".marquee-overlay").count()) === 1, "the band is drawn while dragging");
  await page.mouse.move(...at(80, 80), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 2 &&
      (await page.locator(".marquee-overlay").count()) === 0,
    "the band picked both layers and let go",
  );
  // Locked layers are not caught: lock the rect and band over both again.
  await page.keyboard.press("Escape");
  await page.locator(".panel ul li", { hasText: "Rect 1" }).locator(".lock-toggle").click();
  await page.waitForTimeout(150);
  await page.keyboard.press("Escape");
  await page.mouse.move(...at(560, 380));
  await page.mouse.down();
  await page.mouse.move(...at(80, 80), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const caught = await page.locator(".panel ul li.selected, .panel ul li.multi").count();
  assert(caught === 1, `the locked rect stays out of the band (${caught} picked)`);
  await page.locator(".panel ul li", { hasText: "Rect 1" }).locator(".lock-toggle").click();
  await page.waitForTimeout(150);
  // A click on bare canvas clears the selection, as it always did.
  await page.mouse.click(...at(560, 380));
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 0,
    "a click on bare canvas still clears the selection",
  );
  // A locked layer is not shifted by an alignment either.
  await page.locator(".panel ul li", { hasText: "Rect 1" }).locator(".lock-toggle").click();
  await page.waitForTimeout(150);
  await page.mouse.move(...at(560, 380));
  await page.mouse.down();
  await page.mouse.move(...at(80, 80), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(200);
  await page.locator(".panel ul li", { hasText: "Rect 1" }).click({ modifiers: ["Control"] });
  await page.waitForTimeout(150);
  const rectInk = await inkCount(100, 100, 300, 250);
  await page.click('button[aria-label="Align left edges"]');
  await page.waitForTimeout(250);
  assert(
    (await inkCount(100, 100, 300, 250)) === rectInk,
    "aligning a selection leaves its locked layer where it was",
  );
  for (let i = 0; i < 5; i++) {
    await page.keyboard.press("Control+z"); // align, lock ×3, the ellipse
    await page.waitForTimeout(120);
  }

  // 8x14. The eyedropper takes the colour the page shows and draws with
  // it — and gives it to the picked layer.
  const setSwatch = async (hex) =>
    page.locator('input[aria-label="Fill colour"]').evaluate((el, v) => {
      const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
      setter.call(el, v);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    }, hex);
  await setSwatch("#00cc44");
  await pickTool("Ellipse");
  await page.mouse.move(...at(380, 60));
  await page.mouse.down();
  await page.mouse.move(...at(520, 170), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await setSwatch("#1122cc");
  await pickTool("Rect");
  await page.mouse.move(...at(80, 260));
  await page.mouse.down();
  await page.mouse.move(...at(240, 380), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const blue = await canvasPixel(160, 320);
  assert(blue[2] > 150 && blue[1] < 80, `the new rect is blue (${blue})`);
  await pickTool("Move");
  await page.mouse.click(...at(160, 320)); // pick the blue rect
  await page.waitForTimeout(200);
  await page.keyboard.press("i");
  await page.waitForTimeout(120);
  assert(
    (await page.getAttribute('button[aria-label="Eyedropper"]', "class")).includes("active"),
    "the letter picks up the eyedropper",
  );
  await page.mouse.click(...at(450, 115)); // on the green ellipse
  await page.waitForTimeout(300);
  assert(
    (await page.locator('input[aria-label="Fill colour"]').inputValue()) === "#00cc44",
    "it takes the colour the page shows there",
  );
  const picked = await canvasPixel(160, 320);
  assert(
    picked[1] > 150 && picked[2] < 90 && picked[3] === 255,
    `and gives it to the picked layer (${picked})`,
  );
  assert(
    (await page.getAttribute('button[aria-label="Move"]', "class")).includes("active"),
    "and hands the Move tool back",
  );
  // 8x14b. Opacity and blend reach every picked layer at once.
  await page.keyboard.press("Escape");
  await page.mouse.move(...at(560, 380));
  await page.mouse.down();
  await page.mouse.move(...at(80, 80), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".panel ul li.selected, .panel ul li.multi").count()) === 2,
    "both layers picked",
  );
  await page.locator('input[aria-label="Layer opacity"]').evaluate((el) => {
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, "value").set;
    setter.call(el, "0.4");
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
    el.dispatchEvent(new FocusEvent("focusout", { bubbles: true }));
  });
  await page.waitForTimeout(300);
  const [faded, alsoFaded] = [await canvasPixel(160, 320), await canvasPixel(450, 115)];
  assert(
    Math.abs(faded[3] - 102) < 8 && Math.abs(alsoFaded[3] - 102) < 8,
    `the slider faded both (${faded[3]}, ${alsoFaded[3]})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(200);
  assert(
    (await canvasPixel(160, 320))[3] === 255 && (await canvasPixel(450, 115))[3] === 255,
    "and one undo brings both back",
  );
  // Clicking a member of a multi-selection keeps the whole of it, so
  // let go of both before picking the one to alt-drag.
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
  await page.mouse.click(...at(160, 320));
  await page.waitForTimeout(200);

  // 8x15. Alt-dragging a layer takes a copy and leaves the original.
  await page.mouse.move(...at(160, 320));
  await page.mouse.down({ button: "left" });
  await page.keyboard.down("Alt");
  await page.mouse.up();
  await page.mouse.move(...at(160, 320));
  await page.mouse.down();
  await page.mouse.move(...at(420, 320), { steps: 8 });
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.waitForTimeout(300);
  const afterAlt = (await page.locator(".panel ul li .layer-name").allTextContents()).map((n) =>
    n.trim(),
  );
  assert(
    afterAlt.filter((n) => n.endsWith("copy")).length === 1,
    `alt-drag made a copy (${afterAlt})`,
  );
  assert(
    (await canvasPixel(160, 320))[3] === 255 && (await canvasPixel(420, 320))[3] === 255,
    "the original stayed and the copy came along",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert(
    (await canvasPixel(420, 320))[3] === 0 && (await canvasPixel(160, 320))[3] === 255,
    "one undo takes the drag back",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".panel ul li .layer-name").allTextContents()).every((n) => !n.trim().endsWith("copy")),
    "and the next takes the copy",
  );

  await page.keyboard.press("Control+z");
  await page.waitForTimeout(150);
  assert((await canvasPixel(160, 320))[2] > 150, "one undo puts the layer's own colour back");
  await page.keyboard.press("Control+z"); // the rect
  await page.keyboard.press("Control+z"); // the ellipse
  await page.waitForTimeout(200);
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
await pickTool("Rect");
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
await pickTool("Move");
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
await pickTool("Rect");
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
assert(
  pdfText.includes(" re\n") && pdfText.includes("/CS0 cs") && pdfText.includes("/OutputIntents"),
  "PDF carries live paths in ink, with the profile as its output intent",
);
}

// 9e2. A document's resolution sizes its page in print: at 300 dpi the
// 600×400 page is two inches by one and a third — 144 by 96 points.
{
  await newDocument(600, 400, "rgb", 300);
  assert((await page.locator(".topbar").innerText()).includes("300 dpi"), "the status line carries the dpi");
  await pickTool("Rect");
  const b = await page.locator("#engine-page").boundingBox();
  await page.mouse.move(b.x + b.width * 0.2, b.y + b.height * 0.2);
  await page.mouse.down();
  await page.mouse.move(b.x + b.width * 0.6, b.y + b.height * 0.6, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export PDF")).click(),
  ]);
  const text = (await readFile(await dl.path())).toString("latin1");
  assert(
    text.includes("/MediaBox [0 0 144.000 96.000]") && text.includes(" re\n"),
    "the PDF page is sized by the resolution, with the rect live on it",
  );
}


// 9g. The document's name: typed in the bar, it names every file that
// leaves — the save, and the exports.
{
  await page.locator('input[aria-label="Document name"]').fill("blue-mark");
  await page.waitForTimeout(150);
  const [saved] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Save")).click(),
  ]);
  assert(saved.suggestedFilename() === "blue-mark.chitra", `the save carries the name (${saved.suggestedFilename()})`);
  const [png] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export PNG")).first().click(),
  ]);
  assert(png.suggestedFilename() === "blue-mark.png", `and so does the export (${png.suggestedFilename()})`);
  // Opening a file takes its name; a new document starts over.
  await page.locator('input[accept=".chitra"]').setInputFiles({
    name: "logo.chitra",
    mimeType: "application/zip",
    buffer: await readFile(await saved.path()),
  });
  await page.waitForTimeout(600);
  assert(
    (await page.locator('input[aria-label="Document name"]').inputValue()) === "logo",
    "an opened file brings its own name",
  );
  await newDocument(400, 300, "rgb");
  assert(
    (await page.locator('input[aria-label="Document name"]').inputValue()) === "untitled",
    "and a new document starts over",
  );
  // Leave something on the page for the recovery step that follows.
  await pickTool("Rect");
  const nameBox = await page.locator("#engine-page").boundingBox();
  await page.mouse.move(nameBox.x + nameBox.width * 0.2, nameBox.y + nameBox.height * 0.2);
  await page.mouse.down();
  await page.mouse.move(nameBox.x + nameBox.width * 0.7, nameBox.y + nameBox.height * 0.7, { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
}

// 9h. The sheet of keys and gestures: "?" opens it, Escape closes it,
// and the View menu has it too.
{
  await page.keyboard.press("?");
  await page.waitForTimeout(200);
  const sheet = page.locator('[aria-label="Keys and gestures"]');
  assert((await sheet.count()) === 1, "? opens the sheet");
  const text = await sheet.innerText();
  assert(
    text.includes("Eyedropper") && text.includes("Alt-drag") && text.includes("band"),
    "it says what the gestures do",
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  assert((await sheet.count()) === 0, "and Escape closes it");
  await menuClick("View", "Keys and gestures");
  await page.waitForTimeout(200);
  assert((await sheet.count()) === 1, "the View menu opens it too");
  await page.click('[aria-label="Keys and gestures"] >> text=Close');
  await page.waitForTimeout(200);
  assert((await sheet.count()) === 0, "and Close closes it");
}

// 9i. The paint brush: a stroke lays pixels on a layer of its own, the
// eraser takes them back off, and the whole stroke is one undo.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Paint");
  await page.waitForTimeout(150);
  await page.fill('input[aria-label="Paint width"]', "40");
  await page.waitForTimeout(100);
  await page.mouse.move(...at(120, 200));
  await page.mouse.down();
  await page.mouse.move(...at(280, 200), { steps: 10 });
  await page.mouse.move(...at(440, 200), { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li .layer-name").allTextContents()).some((n) =>
      n.trim().startsWith("Paint"),
    ),
    "the stroke made a paint layer",
  );
  const onIt = await canvasPixel(280, 200);
  assert(onIt[3] > 200, `it painted along the line (${onIt})`);
  assert((await canvasPixel(280, 320))[3] === 0, "and nowhere else");
  // Its edge fades rather than stopping dead: down the column through
  // the stroke, some pixel is neither bare page nor full paint.
  const column = [];
  for (let y = 168; y <= 232; y += 2) column.push((await canvasPixel(280, y))[3]);
  assert(
    column.some((a) => a > 10 && a < 245),
    `the brush's edge is soft (${column.join(",")})`,
  );

  // A second stroke joins the same layer rather than making another.
  const rows = await page.locator(".panel ul li").count();
  await page.mouse.move(...at(120, 260));
  await page.mouse.down();
  await page.mouse.move(...at(440, 260), { steps: 12 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li").count()) === rows,
    "a second stroke joins the layer the first one made",
  );
  assert((await canvasPixel(280, 260))[3] > 200, "and it is painted too");

  // Shift-click runs a straight line on from where the last stroke ended.
  await page.mouse.move(...at(120, 320));
  await page.mouse.down();
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert((await canvasPixel(280, 320))[3] === 0, "nothing along that line yet");
  await page.keyboard.down("Shift");
  await page.mouse.move(...at(440, 320));
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up("Shift");
  await page.waitForTimeout(300);
  const painted = await canvasPixel(280, 320);
  assert(painted[3] > 200, `shift-click ran a line from the last dab (${painted})`);

  // Alt-click takes the colour under the brush without laying any.
  await page.fill('input[aria-label="Fill colour"]', "#123456");
  await page.waitForTimeout(150);
  await page.keyboard.down("Alt");
  await page.mouse.move(...at(280, 320));
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.waitForTimeout(300);
  assert(
    (await page.inputValue('input[aria-label="Fill colour"]')) !== "#123456",
    "alt-click took the colour it was over",
  );
  const still = await canvasPixel(280, 320);
  assert(
    still[0] === painted[0] && still[1] === painted[1] && still[2] === painted[2],
    `and laid none of its own (${still} against ${painted})`,
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await canvasPixel(280, 320))[3] === 0, "the line and the dab undo away");

  // The eraser takes paint off this layer and leaves the page bare.
  await page.click('button[aria-label="Erase"]');
  await page.waitForTimeout(100);
  await page.mouse.move(...at(280, 200));
  await page.mouse.down();
  await page.mouse.move(...at(280, 205), { steps: 3 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const rubbed = await canvasPixel(280, 200);
  assert(rubbed[3] < 60, `the eraser took the paint off (${rubbed})`);
  assert((await canvasPixel(160, 200))[3] > 200, "and left the rest of the stroke");

  // However many points it gathered, a stroke is one undo.
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert((await canvasPixel(280, 200))[3] > 200, "one undo puts the erased paint back");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert((await canvasPixel(280, 260))[3] === 0, "the next takes the second stroke");
  assert((await canvasPixel(280, 200))[3] > 200, "and leaves the first");
  await page.click('button[aria-label="Erase"]');

  // The ring shows how big the brush is and follows the pointer; the
  // brackets resize it, and both rings resize with it.
  await page.mouse.move(...at(300, 100));
  await page.waitForTimeout(150);
  const ring = page.locator(".brush-ring circle").first();
  assert((await ring.count()) === 1, "the brush has a ring under the pointer");
  const wide = Number(await ring.getAttribute("r"));
  await page.mouse.move(...at(320, 120));
  await page.waitForTimeout(100);
  const moved = await page.locator(".brush-ring circle").first().getAttribute("cx");
  await page.keyboard.press("[");
  await page.keyboard.press("[");
  await page.waitForTimeout(150);
  const thin = Number(await page.locator(".brush-ring circle").first().getAttribute("r"));
  assert(thin < wide, `"[" thinned the brush (${wide} -> ${thin})`);
  assert(
    Number(await page.inputValue('input[aria-label="Paint width"]')) < 40,
    "and the width field says so",
  );
  await page.keyboard.press("]");
  await page.waitForTimeout(150);
  assert(
    Number(await page.locator(".brush-ring circle").first().getAttribute("r")) > thin,
    '"]" thickens it again',
  );
  await page.mouse.move(...at(200, 200));
  await page.waitForTimeout(100);
  assert(
    (await page.locator(".brush-ring circle").first().getAttribute("cx")) !== moved,
    "and the ring follows the pointer",
  );
  await pickTool("Move");
  await page.waitForTimeout(150);
  assert(
    (await page.locator(".brush-ring").count()) === 0,
    "putting the brush down takes the ring away",
  );

  // Rubbing at a layer that is not a paint layer takes a piece out of it
  // through a mask, and the brush puts the piece back.
  await pickTool("Rect");
  await page.mouse.move(...at(60, 300));
  await page.mouse.down();
  await page.mouse.move(...at(540, 380), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert((await canvasPixel(300, 340))[3] === 255, "a solid rect to rub at");
  // The brush works on the picked layer, so pick the rect.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  const beforeRub = await page.locator(".panel ul li").count();
  await pickTool("Paint");
  await page.click('button[aria-label="Erase"]');
  await page.waitForTimeout(150);
  await page.mouse.move(...at(300, 340));
  await page.mouse.down();
  await page.mouse.move(...at(305, 342), { steps: 2 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(300, 340))[3] < 60,
    "the rub took a piece out of the rect",
  );
  assert((await canvasPixel(100, 340))[3] === 255, "and left the rest of it");
  assert(
    (await page.locator(".panel ul li").count()) === beforeRub,
    "through a mask on the rect, not a new paint layer",
  );
  await page.click('button[aria-label="Erase"]');
  await page.waitForTimeout(150);
  await page.mouse.move(...at(300, 340));
  await page.mouse.down();
  await page.mouse.move(...at(303, 341), { steps: 2 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await canvasPixel(300, 340))[3] > 200,
    "and the brush puts the piece back",
  );
  // The row shows what the mask lets through, beside the layer itself.
  await page.waitForTimeout(700);
  const maskShot = page.locator(".panel ul li .mask-thumb").first();
  assert((await maskShot.count()) === 1, "the mask has a picture of its own");
  assert(
    (await maskShot.getAttribute("src")).startsWith("data:image/png"),
    "and it is a picture, not a glyph",
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);

  // The clone brush paints with what is already on the page: alt-click
  // says where to read from, and a stroke lifts that colour to where it
  // is drawn — following the source rather than keeping a copy of it.
  await pickTool("Clone");
  await page.waitForTimeout(150);
  const cloneRows = await page.locator(".panel ul li").count();
  await page.keyboard.down("Alt");
  await page.mouse.move(...at(160, 200));
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.waitForTimeout(200);
  assert(
    (await page.locator(".clone-source").count()) === 1,
    "alt-click marks the place it will read from",
  );
  const lifted = await canvasPixel(160, 200);
  assert(lifted[3] > 200, `there is something to lift (${lifted})`);
  assert((await canvasPixel(160, 360))[3] === 0, "and bare page to lift it onto");
  await page.mouse.move(...at(160, 360));
  await page.mouse.down();
  await page.mouse.move(...at(200, 360), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const cloned = await canvasPixel(170, 360);
  assert(
    cloned[3] > 150 &&
      Math.abs(cloned[0] - lifted[0]) < 40 &&
      Math.abs(cloned[1] - lifted[1]) < 40,
    `the clone laid down what its source shows (${lifted} -> ${cloned})`,
  );
  assert(
    (await page.locator(".panel ul li").count()) === cloneRows + 1,
    "on a layer of its own",
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert((await canvasPixel(170, 360))[3] === 0, "and it undoes away");

  // Healing lays the source's texture down in the colour of the place it
  // lands. Give it somewhere to land that is a different colour from the
  // source, and the same stroke comes out differently with it on and off.
  const beforeHeal = await page.inputValue('input[aria-label="Fill colour"]');
  const rowsBeforeHeal = await page.locator(".panel ul li").count();
  await pickTool("Paint");
  await page.fill('input[aria-label="Fill colour"]', "#dddddd");
  await page.waitForTimeout(150);
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(500, 100), { steps: 10 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const pale = await canvasPixel(300, 100);
  assert(pale[0] > 180 && pale[3] > 200, `a pale stroke to land on (${pale})`);

  await pickTool("Clone");
  await page.waitForTimeout(150);
  await page.keyboard.down("Alt");
  await page.mouse.move(...at(160, 200));
  await page.mouse.down();
  await page.mouse.up();
  await page.keyboard.up("Alt");
  await page.waitForTimeout(200);
  // What the source holds, rather than a colour assumed from earlier
  // steps: the colour carried this far depends on which optional blocks
  // ran, so the test reads it instead of naming it.
  const source = await canvasPixel(160, 200);
  const dab = async () => {
    await page.mouse.move(...at(300, 100));
    await page.mouse.down();
    await page.mouse.move(...at(306, 100), { steps: 3 });
    await page.mouse.up();
    await page.waitForTimeout(300);
    const px = await canvasPixel(300, 100);
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(250);
    return px;
  };
  const healed = await dab();
  await page.click('button[aria-label="Heal"]');
  await page.waitForTimeout(150);
  const stamped = await dab();
  assert(
    Math.abs(healed[0] - pale[0]) < 40,
    `healing kept the colour it landed in (${pale} -> ${healed})`,
  );
  assert(
    stamped.slice(0, 3).every((v, i) => Math.abs(v - source[i]) < 40) &&
      stamped.slice(0, 3).some((v, i) => Math.abs(v - pale[i]) > 30),
    `and with it off the source comes over as it is (${source} -> ${stamped})`,
  );
  await page.click('button[aria-label="Heal"]');
  // Take the whole trial back — the strokes and the layers they made —
  // by undoing until the stack is where it was, rather than by counting
  // entries, which is easy to get wrong and hard to notice.
  for (let i = 0; i < 8; i++) {
    if (
      (await page.locator(".panel ul li").count()) === rowsBeforeHeal &&
      (await canvasPixel(300, 100))[3] === 0
    )
      break;
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(150);
  }
  assert(
    (await page.locator(".panel ul li").count()) === rowsBeforeHeal &&
      (await canvasPixel(300, 100))[3] === 0,
    "and the whole trial undoes away",
  );
  await page.fill('input[aria-label="Fill colour"]', beforeHeal);
  await pickTool("Move");
  await page.waitForTimeout(150);

  // The panel shows what each layer holds, not only what it is called.
  await page.waitForTimeout(700);
  const thumb = page.locator(".panel ul li .layer-thumb").first();
  assert((await thumb.count()) === 1, "the paint layer has a picture of itself");
  const src = await thumb.getAttribute("src");
  assert(src.startsWith("data:image/png"), "and it is a picture, not a glyph");
  // It is a picture of that layer: the stroke's colour is in it, and it
  // is not a blank square.
  const ink = await page.evaluate(
    (url) =>
      new Promise((done) => {
        const img = new Image();
        img.onload = () => {
          const c = document.createElement("canvas");
          c.width = img.width;
          c.height = img.height;
          const x = c.getContext("2d");
          x.drawImage(img, 0, 0);
          const d = x.getImageData(0, 0, c.width, c.height).data;
          let opaque = 0;
          for (let i = 3; i < d.length; i += 4) if (d[i] > 200) opaque++;
          done({ opaque, total: d.length / 4 });
        };
        img.src = url;
      }),
    src,
  );
  assert(
    ink.opaque > 20 && ink.opaque < ink.total,
    `the picture holds the layer's ink and its bare parts (${ink.opaque} of ${ink.total})`,
  );
}

// 9j. The document's palette: colours kept by name, saved with the file,
// clicked to draw with and to give to the picked layer.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  assert(
    (await page.locator(".palette .swatch:not(.add)").count()) === 0,
    "a new document starts with no palette",
  );
  await setColor("Fill colour", "#ff0066");
  await page.click('button[aria-label="Add to the palette"]');
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".palette .swatch:not(.add)").count()) === 1,
    "the colour being drawn with goes into the palette",
  );

  // Draw something, then give it a palette colour.
  await pickTool("Rect");
  await page.mouse.move(...at(100, 100));
  await page.mouse.down();
  await page.mouse.move(...at(300, 250), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await setColor("Fill colour", "#00aaff");
  await page.click('button[aria-label="Add to the palette"]');
  await page.waitForTimeout(250);
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await page.locator(".palette .swatch:not(.add)").first().click();
  await page.waitForTimeout(300);
  const px = await canvasPixel(200, 175);
  assert(
    px[0] > 200 && px[1] < 60 && px[2] > 60 && px[2] < 160,
    `clicking a palette colour gives it to the picked layer (${px})`,
  );

  // Alt-click takes one out, and the whole palette saves with the file.
  await page.locator(".palette .swatch:not(.add)").first().click({
    modifiers: ["Alt"],
  });
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".palette .swatch:not(.add)").count()) === 1,
    "alt-click takes a colour out of the palette",
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  assert(
    (await page.locator(".palette .swatch:not(.add)").count()) === 2,
    "and one undo puts it back",
  );
}

// 9j2. Where a line stops and how it turns: the two pickers change the
// shape of the ink at the end of a line and at its corner, not just the
// markup. On a page of its own, so the probes have bare paper around them.
{
  await newDocument(600, 400, "rgb");
  const page_ = await page.locator("#engine-page").boundingBox();
  const [ux, uy] = [page_.width / 600, page_.height / 400];
  const click = async (x, y) => {
    await page.mouse.click(page_.x + x * ux, page_.y + y * uy);
    await page.waitForTimeout(80);
  };
  // An elbow: right along the top, then down. Twenty-four wide, so an
  // end and a corner are a dozen pixels of ink rather than two.
  await pickTool("Pen");
  await click(150, 120);
  await click(330, 120);
  await click(330, 280);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(250);
  await pickTool("Move");
  await page.locator(".panel ul li", { hasText: "Path" }).first().click();
  await page.waitForTimeout(200);
  const width = page.locator('input[aria-label="Stroke width"]');
  await width.fill("24");
  await width.evaluate((el) => el.blur());
  await page.waitForTimeout(250);

  const ink = async (x, y) => (await canvasPixel(x, y))[3] > 128;
  const pick = async (what, value) => {
    await page.selectOption(`select[aria-label="Line ${what}"]`, value);
    await page.waitForTimeout(250);
  };
  assert(await ink(240, 120), "the line itself is drawn");
  // Six past the last point, down the middle of the line; and out at the
  // corner a squared end would have, which is nine past and nine across.
  const [pastEnd, endCorner] = [[330, 286], [339, 289]];
  // Just outside the turn, and out where only a miter reaches.
  const [byTheCorner, thePoint] = [[333, 117], [339, 111]];

  assert(await ink(...pastEnd), "a round end reaches past the last point");
  assert(!(await ink(...endCorner)), "but has no corner out at the side");
  await pick("ends", "Butt");
  assert(!(await ink(...pastEnd)), "a flat end stops on the last point");
  await pick("ends", "Square");
  assert(await ink(...pastEnd), "a square end reaches past it");
  assert(await ink(...endCorner), "and has a corner");

  assert(await ink(...byTheCorner), "the turn is filled");
  assert(!(await ink(...thePoint)), "a round corner goes no further");
  await pick("corners", "Miter");
  assert(await ink(...thePoint), "a mitred one is carried out to a point");
  await pick("corners", "Bevel");
  assert(!(await ink(...thePoint)), "a bevelled one is cut across");
  assert(await ink(...byTheCorner), "and still fills the turn");

  // A line can point at something. The head is sized from the line, so
  // it reaches well past where the line itself stops.
  // The tip is on the line's last point and the head reaches back along
  // the line from there, three widths long and one and a half either
  // side — so it is wider than the line well before the tip.
  await pick("ends", "Butt");
  assert(!(await ink(315, 240)), "the line is only as wide as it is");
  await page.selectOption('select[aria-label="Line end"]', "Arrow");
  await page.waitForTimeout(300);
  assert(await ink(315, 240), "a head is wider than the line it is on");
  assert(await ink(350, 220), "and wider still further from its tip");
  assert(!(await ink(300, 260)), "narrowing to the tip rather than filling a box");
  await page.selectOption('select[aria-label="Line end"]', "None");
  await page.waitForTimeout(300);
  assert(!(await ink(315, 240)), "and it comes off again");

  // A rect's stroke is a band inside a closed outline: it never stops
  // and its corners are its own, so neither question is asked of it.
  await pickTool("Rect");
  await page.mouse.move(page_.x + 400 * ux, page_.y + 300 * uy);
  await page.mouse.down();
  await page.mouse.move(page_.x + 550 * ux, page_.y + 380 * uy, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  await pickTool("Move");
  await page.locator(".panel ul li", { hasText: "Rect" }).first().click();
  await page.waitForTimeout(200);
  await page.check('input[aria-label="Stroke enabled"]');
  await page.waitForTimeout(200);
  assert(
    (await page.locator('input[aria-label="Stroke width"]').count()) === 1 &&
      (await page.locator('select[aria-label="Line ends"]').count()) === 0,
    "a rect's stroke is asked neither question",
  );
}

// 9j3. The page turns. A wide page with a mark in one corner stands up
// on its end, the mark goes round with it, and it undoes.
{
  await newDocument(600, 300, "rgb");
  const at = await page.locator("#engine-page").boundingBox();
  const [ux, uy] = [at.width / 600, at.height / 300];
  await pickTool("Rect");
  await page.mouse.move(at.x + 20 * ux, at.y + 20 * uy);
  await page.mouse.down();
  await page.mouse.move(at.x + 80 * ux, at.y + 60 * uy, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const inked = (x, y) => canvasPixel(x, y).then((px) => px[3] > 128);
  const size = () =>
    page.locator("#engine-page").evaluate((el) => [
      Math.round(el.getBoundingClientRect().width),
      Math.round(el.getBoundingClientRect().height),
    ]);
  const [w0, h0] = await size();
  assert(w0 > h0, `the page starts wider than it is tall (${w0}x${h0})`);
  assert(await inked(50, 40), "with a mark in its top left");
  assert(!(await inked(550, 40)), "and nothing in its top right");

  await menuClick("Page", "Turn right");
  await page.waitForTimeout(400);
  const [w1, h1] = await size();
  assert(h1 > w1, `turned right it stands on its end (${w1}x${h1})`);
  // The page is 300x600 now, and what was top-left is top-right: the
  // mark sits within twenty of the right-hand edge.
  assert(await inked(260, 50), "the mark went round to the right");
  assert(!(await inked(50, 50)), "and left where it was");

  await menuClick("Page", "Turn upside down");
  await page.waitForTimeout(400);
  assert(await inked(40, 550), "upside down puts it at the bottom left");

  for (let i = 0; i < 2; i++) {
    await page.keyboard.press("Control+z");
    await page.waitForTimeout(300);
  }
  // The page's shape, not the size it is drawn at: the view was fitted
  // to the page standing up and stays where the last fit put it.
  const [w2, h2] = await size();
  assert(
    Math.abs(w2 / h2 - w0 / h0) < 0.02,
    `two undos put the page back the way it was (${w2}x${h2} against ${w0}x${h0})`,
  );
  assert(await inked(50, 40), "with its mark back in the corner");
  assert(!(await inked(550, 40)), "and nothing where the turn had put it");
}

// 9j4. The other half of cropping: the page is given room around the
// picture rather than taken in to it, with one of its nine points
// staying where it is. And the page mirrors.
{
  await newDocument(400, 300, "rgb");
  const at = await page.locator("#engine-page").boundingBox();
  const [ux, uy] = [at.width / 400, at.height / 300];
  await pickTool("Rect");
  await page.mouse.move(at.x + 20 * ux, at.y + 20 * uy);
  await page.mouse.down();
  await page.mouse.move(at.x + 80 * ux, at.y + 80 * uy, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  const inked = (x, y) => canvasPixel(x, y).then((px) => px[3] > 128);
  assert(await inked(50, 50), "a mark near the page's top left");

  // Anchored at the top left, a bigger page adds its room to the right
  // and below — so the mark does not move at all.
  await menuClick("Page", "Canvas size…");
  await page.waitForTimeout(200);
  assert(
    await page.isVisible('[role="dialog"][aria-label="Canvas size"]'),
    "the canvas-size dialog opens",
  );
  await page.locator('input[aria-label="Canvas width"]').fill("600");
  await page.locator('input[aria-label="Canvas height"]').fill("500");
  await page.click('button[aria-label="Anchor left top"]');
  await page.click('button[aria-label="Resize the page"]');
  await page.waitForTimeout(400);
  assert(await inked(50, 50), "anchored top left, the mark stays put");
  assert(!(await inked(550, 450)), "and the new room is bare page");

  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  // Anchored in the middle, the same growth is shared all round, so the
  // mark moves by half of it: a hundred across and a hundred down.
  await menuClick("Page", "Canvas size…");
  await page.waitForTimeout(200);
  await page.locator('input[aria-label="Canvas width"]').fill("600");
  await page.locator('input[aria-label="Canvas height"]').fill("500");
  await page.click('button[aria-label="Anchor centre middle"]');
  await page.click('button[aria-label="Resize the page"]');
  await page.waitForTimeout(400);
  assert(await inked(150, 150), "anchored in the middle, it moved with it");
  assert(!(await inked(50, 50)), "and left where it was");

  // Mirroring keeps the page's size and crosses what is on it. The mark
  // sits 100..180 across a 600-wide page, so it lands at 420..500.
  await menuClick("Page", "Mirror left to right");
  await page.waitForTimeout(400);
  assert(await inked(450, 150), "the mark crossed to the other side");
  assert(!(await inked(150, 150)), "and is not where it was");
  await menuClick("Page", "Mirror left to right");
  await page.waitForTimeout(400);
  assert(await inked(150, 150), "twice over is where it started");
}

// 9j5. A dragged corner keeps the shape's proportions, and shift lets go
// of them. Letting go of a photograph a little squashed is a mistake
// nobody notices until it is printed, so that is the way round.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  await pickTool("Rect");
  await page.mouse.move(...at(50, 50));
  await page.mouse.down();
  await page.mouse.move(...at(250, 150), { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(250);
  const size = async () => [
    Number(await page.locator('input[aria-label="W size"]').inputValue()),
    Number(await page.locator('input[aria-label="H size"]').inputValue()),
  ];
  const [w0, h0] = await size();
  assert(
    Math.abs(w0 - 200) < 3 && Math.abs(h0 - 100) < 3,
    `a rect twice as wide as it is tall (${w0}x${h0})`,
  );

  // Pull the corner out and well down: the height has to come along
  // rather than following the cursor.
  const pull = async (x, y, shift) => {
    const se = await page.locator(".handle.se").boundingBox();
    if (shift) await page.keyboard.down("Shift");
    await page.mouse.move(se.x + se.width / 2, se.y + se.height / 2);
    await page.mouse.down();
    await page.mouse.move(...at(x, y), { steps: 8 });
    await page.mouse.up();
    if (shift) await page.keyboard.up("Shift");
    await page.waitForTimeout(350);
  };
  await pull(450, 350, false);
  const [w1, h1] = await size();
  assert(
    Math.abs(w1 / h1 - w0 / h0) < 0.05,
    `the shape kept its proportions (${w1}x${h1}, was ${w0}x${h0})`,
  );
  assert(w1 > w0 + 20, `and it did grow (${w1} from ${w0})`);
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);

  // Alt holds the shape's middle rather than its far corner, so the box
  // grows both ways at once: the same pull gives twice the size and
  // leaves the middle where it was.
  const middle = async () => {
    const [x, y] = [
      Number(await page.locator('input[aria-label="X position"]').inputValue()),
      Number(await page.locator('input[aria-label="Y position"]').inputValue()),
    ];
    const [w, h] = await size();
    return [x + w / 2, y + h / 2];
  };
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(250);
  const wasMiddle = await middle();
  await page.keyboard.down("Alt");
  await pull(350, 250, false);
  await page.keyboard.up("Alt");
  const nowMiddle = await middle();
  assert(
    Math.abs(nowMiddle[0] - wasMiddle[0]) < 6 &&
      Math.abs(nowMiddle[1] - wasMiddle[1]) < 6,
    `alt keeps the middle where it was (${nowMiddle} against ${wasMiddle})`,
  );
  const [aw, ah] = await size();
  assert(
    Math.abs(aw / ah - w0 / h0) < 0.05 && aw > w0,
    `and it still keeps its proportions (${aw}x${ah})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);

  // The same drag with shift held follows the cursor on both axes, so
  // the shape comes out a different one.
  await pull(450, 350, true);
  const [w2, h2] = await size();
  assert(
    Math.abs(w2 - 400) < 6 && Math.abs(h2 - 300) < 6,
    `shift takes the corner to the cursor (${w2}x${h2})`,
  );
  assert(
    Math.abs(w2 / h2 - w0 / h0) > 0.2,
    "which is a shape of its own, not the one it started as",
  );
}

// 9j6. Shift squares off what is being dragged out, and holds a pen
// segment to an eighth of a turn. Nothing has proportions of its own yet
// when it is being drawn, so shift is what asks for the one shape worth
// naming — which is the other way round from resizing something that has.
{
  await newDocument(600, 400, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 600) * b.width, b.y + (y / 400) * b.height];
  const drawn = async (tool, x0, y0, x1, y1, shift, alt) => {
    await pickTool(tool);
    if (shift) await page.keyboard.down("Shift");
    if (alt) await page.keyboard.down("Alt");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 8 });
    await page.mouse.up();
    if (shift) await page.keyboard.up("Shift");
    if (alt) await page.keyboard.up("Alt");
    await page.waitForTimeout(300);
    await pickTool("Move");
    await page.locator(".panel ul li").first().click();
    await page.waitForTimeout(250);
    return [
      Number(await page.locator('input[aria-label="W size"]').inputValue()),
      Number(await page.locator('input[aria-label="H size"]').inputValue()),
    ];
  };

  const [w, h] = await drawn("Rect", 60, 60, 300, 180, false);
  assert(
    Math.abs(w - 240) < 4 && Math.abs(h - 120) < 4,
    `dragged out, a rect is the box that was dragged (${w}x${h})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  const [sw, sh] = await drawn("Rect", 60, 60, 300, 180, true);
  assert(
    Math.abs(sw - sh) < 4 && Math.abs(sw - 240) < 4,
    `with shift it is a square, on the longer side (${sw}x${sh})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  const [cw, ch] = await drawn("Ellipse", 60, 60, 300, 180, true);
  assert(
    Math.abs(cw - ch) < 4,
    `and an ellipse comes out a circle (${cw}x${ch})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);
  // Alt draws out from the middle, which is how a circle is put on a
  // target rather than beside one: the same drag makes twice the box,
  // centred on where it began.
  const [mw, mh] = await drawn("Rect", 160, 110, 260, 160, false, true);
  assert(
    Math.abs(mw - 200) < 4 && Math.abs(mh - 100) < 4,
    `alt draws out from the middle, so it is twice the drag (${mw}x${mh})`,
  );
  const [mx, my] = [
    Number(await page.locator('input[aria-label="X position"]').inputValue()),
    Number(await page.locator('input[aria-label="Y position"]').inputValue()),
  ];
  assert(
    Math.abs(mx + mw / 2 - 160) < 4 && Math.abs(my + mh / 2 - 110) < 4,
    `centred on where the drag began (${mx + mw / 2},${my + mh / 2})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);

  // A pen segment held to 45°: clicked well off the diagonal, the anchor
  // lands on it. The path is two anchors, finished with Enter, so its
  // box is the segment's own.
  await pickTool("Pen");
  await page.mouse.click(...at(100, 300));
  await page.waitForTimeout(120);
  await page.keyboard.down("Shift");
  await page.mouse.click(...at(300, 340));
  await page.keyboard.up("Shift");
  await page.waitForTimeout(120);
  await page.keyboard.press("Enter");
  await page.waitForTimeout(300);
  await pickTool("Move");
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(250);
  const [pw, ph] = [
    Number(await page.locator('input[aria-label="W size"]').inputValue()),
    Number(await page.locator('input[aria-label="H size"]').inputValue()),
  ];
  // 200 across and 40 down is nearest the level eighth, so the segment
  // lies flat at the length it was dragged — 204 — inside a box the
  // stroke's own width reaches four past at either end.
  assert(
    Math.abs(pw - 212) < 4 && ph < 12,
    `the segment went level rather than where it was clicked (${pw}x${ph})`,
  );
}

// 9j7. Two filters that have to be functions of the page rather than of
// the window: a grid of squares anchored in the document, and grain
// settled by where a speck sits on it.
{
  await newDocument(400, 300, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  // Two halves, so a block over the join has an average neither has.
  for (const [x0, x1, hex] of [
    [20, 210, "#ff0000"],
    [210, 380, "#0000ff"],
  ]) {
    // The colour is set before the drag, so each rect is drawn in it
    // rather than having to be picked again afterwards.
    await page.fill('input[aria-label="Fill colour"]', hex);
    await page.waitForTimeout(150);
    await pickTool("Rect");
    await page.mouse.move(...at(x0, 40));
    await page.mouse.down();
    await page.mouse.move(...at(x1, 260), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(250);
    await pickTool("Move");
  }
  const px = (x, y) => canvasPixel(x, y);
  const flat = await px(100, 150);
  assert(flat[0] > 200 && flat[2] < 60, `the left half is red (${flat})`);

  await page.selectOption('[aria-label="Add adjustment layer"]', "pixelate");
  await page.waitForTimeout(300);
  await page.locator(".panel ul li", { hasText: "Pixelate" }).click();
  await page.waitForTimeout(200);
  await setSlider("Block size", 40);
  await page.waitForTimeout(300);
  // The grid starts at the page's own origin, so a block runs 200..240
  // while the two halves meet at 210: that block straddles the join and
  // comes out neither red nor blue.
  const joined = await px(210, 150);
  assert(
    joined[0] > 40 && joined[2] > 40,
    `the block over the join carries both halves (${joined})`,
  );
  // And one block is one colour throughout.
  const [a1, a2] = [await px(84, 150), await px(110, 150)];
  assert(
    Math.abs(a1[0] - a2[0]) < 4 && Math.abs(a1[2] - a2[2]) < 4,
    `one block is flat (${a1} vs ${a2})`,
  );
  await page.keyboard.press("Control+z");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li", { hasText: "Pixelate" }).count()) === 0,
    "and it undoes",
  );

  // Grain: a flat colour stops being flat, and the same page grains the
  // same way twice — which a redraw between the two readings would show.
  await page.selectOption('[aria-label="Add adjustment layer"]', "noise");
  await page.waitForTimeout(300);
  await page.locator(".panel ul li", { hasText: "Noise" }).click();
  await page.waitForTimeout(200);
  await setSlider("Noise amount", 0.6);
  await page.waitForTimeout(300);
  // Read along a row of the page by its own coordinates, whatever the
  // view is doing, so the same document points are read each time.
  const along = () =>
    page.evaluate(() => {
      const c = document.getElementById("engine-canvas");
      const d = c.getContext("2d").getImageData(0, 0, c.width, c.height).data;
      const s = Number(c.dataset.frameScale) || 1;
      const ox = Number(c.dataset.originX) || 0;
      const oy = Number(c.dataset.originY) || 0;
      const row = [];
      for (let x = 150; x < 205; x++) {
        const dx = Math.round(ox + (x + 0.5) * s);
        const dy = Math.round(oy + (150 + 0.5) * s);
        row.push(d[(dy * c.width + dx) * 4]);
      }
      return row;
    });
  const grained = await along();
  const mean = grained.reduce((a, v) => a + v, 0) / grained.length;
  const varies =
    grained.reduce((a, v) => a + Math.abs(v - mean), 0) / grained.length;
  assert(varies > 3, `a flat colour comes out grained (spread ${varies})`);
  // Zoom in and read the same document points again. Grain settled by
  // where a speck sits on the page comes back the same; grain settled by
  // the screen would be a fresh sprinkling at every zoom.
  await menuClick("View", "Zoom in");
  await page.waitForTimeout(500);
  const closer = await along();
  const same = grained.filter((v, i) => Math.abs(v - closer[i]) <= 3).length;
  assert(
    same > grained.length * 0.85,
    `the page grains by the page, not by the view (${same} of ${grained.length} unchanged)`,
  );
}

// 9j8. Frames become the pages of one PDF: each page its frame's own
// size, in the order the frames sit on the document. It is offered only
// when there are frames to make pages of.
{
  await newDocument(400, 300, "rgb");
  assert(
    !(await (await menuItem("File", "Export PDF of the frames")).count()),
    "with no frames there are no pages to offer",
  );
  await page.keyboard.press("Escape");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  for (const [x0, x1] of [
    [20, 140],
    [200, 380],
  ]) {
    await pickTool("Frame");
    await page.mouse.move(...at(x0, 40));
    await page.mouse.down();
    await page.mouse.move(...at(x1, 200), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(300);
  }
  await pickTool("Move");
  const [dl] = await Promise.all([
    page.waitForEvent("download"),
    (await menuItem("File", "Export PDF of the frames")).click(),
  ]);
  const pdf = await readFile(await dl.path(), "latin1");
  assert(pdf.startsWith("%PDF-1.7"), "a PDF came out");
  assert(pdf.includes("/Count 2"), "two frames, two pages");
  // The first frame is 120 by 160 document pixels, the second 180 by
  // 160; at 72 dpi a document pixel is a point.
  assert(
    pdf.includes("/MediaBox [0 0 120.000 160.000]") &&
      pdf.includes("/MediaBox [0 0 180.000 160.000]"),
    "each page is its own frame's size",
  );
}

// 9j9. The shape tools share a slot, a polygon and a star are drawn from
// it, the toolbar can be carried off its edge, and the panel is as wide
// as it is dragged to be.
{
  await newDocument(400, 300, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  const drag = async (x0, y0, x1, y1) => {
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(300);
  };
  const ink = (x, y) => canvasPixel(x, y).then((px) => px[3] > 128);

  // The rail holds one shape at a time, and taking another out of the
  // group puts that one in the slot.
  assert(
    (await page.locator('.toolbar .tool-group > button[aria-label="Rect"]').count()) === 1,
    "the rail's shape slot starts on the rect",
  );
  await pickTool("Polygon");
  assert(
    (await page.locator('.toolbar .tool-group > button[aria-label="Polygon"]').count()) === 1,
    "and holds whichever shape was taken out of the group",
  );

  // A polygon of five sides, inscribed in the box that was dragged: its
  // top point is on the middle of the top edge, so the box's own top
  // corners are bare.
  await page.locator('input[aria-label="Sides"]').fill("5");
  await page.waitForTimeout(150);
  await drag(100, 60, 300, 260);
  assert(await ink(200, 70), "the polygon's top point is on the middle");
  assert(!(await ink(110, 70)), "and its box's corner is bare");
  assert(await ink(200, 180), "with a filled middle");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);

  // A star of the same count reaches the same points but is cut in
  // between them, so the middle of an edge is bare where a polygon's is
  // not.
  await pickTool("Star");
  await drag(100, 60, 300, 260);
  assert(await ink(200, 70), "the star's top point reaches as far");
  assert(await ink(200, 180), "and it is filled at the middle");
  assert(!(await ink(120, 120)), "but cut away between its points");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);

  // A line is the drag itself, stroked, from end to end.
  await pickTool("Line");
  await drag(80, 240, 320, 240);
  assert(await ink(200, 240), "the line is drawn along the drag");
  assert(!(await ink(200, 200)), "and is a line, not the box around it");
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(250);

  // The toolbar is carried off its edge by its grip, and put back.
  const railAt = async () => (await page.locator(".toolbar").boundingBox()).x;
  const home = await railAt();
  const grip = await page.locator('button[aria-label="Move the toolbar"]').boundingBox();
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(grip.x + 320, grip.y + 140, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  assert((await railAt()) > home + 200, "the toolbar went where it was carried");
  assert(
    await page.locator(".toolbar").evaluate((el) => el.classList.contains("floating")),
    "and floats rather than sitting in the row",
  );
  await page.dblclick('button[aria-label="Move the toolbar"]');
  await page.waitForTimeout(250);
  assert(
    Math.abs((await railAt()) - home) < 2,
    "double-clicking the grip docks it again",
  );

  // The panel's own edge sets how wide it is.
  const panelWidth = async () =>
    (await page.locator(".panel").boundingBox()).width;
  const was = await panelWidth();
  const edge = await page.locator('[aria-label="Panel width"]').boundingBox();
  await page.mouse.move(edge.x + edge.width / 2, edge.y + 100);
  await page.mouse.down();
  await page.mouse.move(edge.x - 90, edge.y + 100, { steps: 8 });
  await page.mouse.up();
  await page.waitForTimeout(250);
  const now = await panelWidth();
  assert(
    now > was + 60,
    `the panel is as wide as its edge was dragged (${was} to ${now})`,
  );

  // Leave a layer behind: the recovery block that follows needs a
  // document with something in it to lose.
  await pickTool("Rect");
  await drag(60, 60, 340, 240);
}

// 9k. A shape being drawn catches the same lines a shape being moved
// does — here the box of the rect drawn before it. Ctrl draws free of
// them and shift, which asks for an exact shape, wins over them.
{
  await newDocument(400, 300, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  // Four screen pixels in document units: inside the six the snap
  // reaches, so a drag aimed this far off a line still lands on it.
  const near = (4 * 400) / b.width;
  const drawRect = async (x0, y0, x1, y1, mod, mid) => {
    await pickTool("Rect");
    if (mod) await page.keyboard.down(mod);
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    if (mid) await mid();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    if (mod) await page.keyboard.up(mod);
    await page.waitForTimeout(300);
  };
  // Where the newest layer starts, which is the top row of the panel.
  const startsAt = async () => {
    await pickTool("Move");
    await page.locator(".panel ul li").first().click();
    await page.waitForTimeout(200);
    return Number(
      await page.locator('input[aria-label="X position"]').inputValue(),
    );
  };

  await drawRect(100, 100, 160, 200);
  assert(Math.abs((await startsAt()) - 100) < 1.5, "a rect to catch on");

  // The handles are the move tool's own: with a shape tool up, a drag
  // from the picked layer's corner draws rather than resizes it.
  assert(
    (await page.locator("[data-handle]").count()) === 5,
    "the move tool offers four corners and a knob to turn by",
  );
  await pickTool("Rect");
  assert(
    (await page.locator("[data-handle]").count()) === 0,
    "and a shape tool puts them away",
  );

  // Aimed a few pixels shy of that rect's right edge, the next one
  // starts exactly on it — and says so while it is being dragged.
  let sawGuide = 0;
  await drawRect(160 + near, 120, 300, 260, null, async () => {
    // Pass a few pixels off the page's own middle on the way: the corner
    // being drawn catches both of its lines and says so.
    await page.mouse.move(...at(200 + near, 150 + near), { steps: 4 });
    await page.waitForTimeout(150);
    sawGuide = await page.locator(".snap-overlay line").count();
  });
  assert(
    sawGuide === 2,
    `a guide on each line the corner being drawn caught (${sawGuide})`,
  );
  assert(
    (await page.locator(".snap-overlay line").count()) === 0,
    "and clears when the drag ends",
  );
  const caught = await startsAt();
  assert(
    Math.abs(caught - 160) < 1.5,
    `the drawn corner caught the edge beside it (${caught})`,
  );

  // Ctrl draws free of the lines: the same drag stays where it was
  // aimed.
  await drawRect(160 + near, 120, 300, 260, "Control");
  const free = await startsAt();
  assert(
    free > 160 + near - 1.5 && free < 160 + near + 1.5,
    `ctrl drew free of the edge (${free} against ${(160 + near).toFixed(2)})`,
  );

  // Shift squares the box off, and wins over the lines: the corner it
  // is dragged to would have caught the rect beside it, and comes out on
  // the square instead. Where the drag began is not in question, so it
  // still catches its own lines.
  await drawRect(110, 30, 160 + near, 220, "Shift");
  const left = await startsAt();
  const [w, h] = [
    Number(await page.locator('input[aria-label="W size"]').inputValue()),
    Number(await page.locator('input[aria-label="H size"]').inputValue()),
  ];
  assert(
    Math.abs(left - 110) < 1.5 && Math.abs(w - h) < 1.5 && Math.abs(w - 190) < 2,
    `shift squared the box rather than catching the edge (${left}, ${w}x${h})`,
  );
}

// 9l. The keys for the view and for the order of things: zoom in, out,
// fit and actual size, and a layer carried to the front or the back in
// one step rather than one step per layer in the way.
{
  await newDocument(400, 300, "rgb");
  const pageWidth = async () =>
    (await page.locator("#engine-page").boundingBox()).width;
  await page.keyboard.press("Control+1");
  await page.waitForTimeout(250);
  const actual = await pageWidth();
  assert(
    Math.abs(actual - 400) < 2,
    `ctrl+1 shows the page's own pixels (${actual})`,
  );
  await page.keyboard.press("Control+=");
  await page.waitForTimeout(250);
  const zoomed = await pageWidth();
  assert(zoomed > actual * 1.2, `ctrl+= zoomed in (${actual} -> ${zoomed})`);
  await page.keyboard.press("Control+-");
  await page.waitForTimeout(250);
  assert(
    Math.abs((await pageWidth()) - actual) < 2,
    "and ctrl+- zoomed back out to where it was",
  );
  await page.keyboard.press("Control+0");
  await page.waitForTimeout(300);
  const fitted = await pageWidth();
  assert(
    fitted > actual * 1.5,
    `ctrl+0 fit the page to the window (${fitted})`,
  );

  // Three rects, each over the last, all crossing one point.
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  const paint = async (hex, x0, y0, x1, y1) => {
    await page.keyboard.press("Escape"); // colour goes to the picked layer
    await setColor("Fill colour", hex);
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(300);
  };
  await paint("#ff0000", 50, 50, 250, 250);
  await paint("#00cc00", 100, 80, 300, 220);
  await paint("#0000ff", 150, 100, 350, 200);
  const over = () => canvasPixel(200, 150);
  const reads = (px, [r, g, bl]) =>
    Math.abs(px[0] - r) < 40 && Math.abs(px[1] - g) < 40 && Math.abs(px[2] - bl) < 40;
  assert(reads(await over(), [0, 0, 255]), `the last drawn is on top (${await over()})`);

  // The red one is the bottom row; carried to the front it is what shows.
  await pickTool("Move");
  await page.locator(".panel ul li").last().click();
  await page.waitForTimeout(200);
  await page.keyboard.press("Control+Shift+BracketRight");
  await page.waitForTimeout(300);
  assert(
    reads(await over(), [255, 0, 0]),
    `ctrl+shift+] brought the bottom layer to the front (${await over()})`,
  );
  await page.keyboard.press("Control+z");
  await page.waitForTimeout(300);
  assert(
    reads(await over(), [0, 0, 255]),
    "and one undo puts the order back",
  );

  // The blue one is the top row; sent to the back the green one shows.
  await page.locator(".panel ul li").first().click();
  await page.waitForTimeout(200);
  await page.keyboard.press("Control+Shift+BracketLeft");
  await page.waitForTimeout(300);
  assert(
    reads(await over(), [0, 204, 0]),
    `ctrl+shift+[ sent the top layer to the back (${await over()})`,
  );
}

// 9m. Two fingers are the view's: a pinch zooms about the point they
// began around, their middle carries the page, and whatever one finger
// had begun is let go of rather than left half-drawn. There is no wheel
// and no space bar on a tablet, so this is the only way to get about.
{
  await newDocument(400, 300, "rgb");
  await page.keyboard.press("Control+1");
  await page.waitForTimeout(250);
  // Synthetic pointers have no capture to take; the real ones on a
  // tablet do.
  await page.evaluate(() => {
    window.__capture = [
      Element.prototype.setPointerCapture,
      Element.prototype.releasePointerCapture,
    ];
    Element.prototype.setPointerCapture = () => {};
    Element.prototype.releasePointerCapture = () => {};
  });
  const finger = (type, id, x, y) =>
    page.evaluate(
      ([type, id, x, y]) => {
        document.getElementById("engine-canvas").dispatchEvent(
          new PointerEvent(type, {
            bubbles: true,
            cancelable: true,
            composed: true,
            pointerId: id,
            pointerType: "touch",
            isPrimary: id === 1,
            buttons: type === "pointerup" ? 0 : 1,
            clientX: x,
            clientY: y,
          }),
        );
      },
      [type, id, x, y],
    );
  const pageBox = () => page.locator("#engine-page").boundingBox();
  const before = await pageBox();
  const [cx, cy] = [before.x + before.width / 2, before.y + before.height / 2];
  assert(
    Math.abs(before.width - 400) < 2,
    `the page is at its own size to begin with (${before.width})`,
  );

  // Two fingers, a hundred apart, spread to two hundred: twice the zoom,
  // about the middle they began around, which does not move.
  await finger("pointerdown", 1, cx - 50, cy);
  await finger("pointerdown", 2, cx + 50, cy);
  await finger("pointermove", 1, cx - 100, cy);
  await finger("pointermove", 2, cx + 100, cy);
  await page.waitForTimeout(300);
  const spread = await pageBox();
  await finger("pointerup", 1, cx - 100, cy);
  await finger("pointerup", 2, cx + 100, cy);
  await page.waitForTimeout(250);
  assert(
    Math.abs(spread.width - 800) < 8,
    `spreading two fingers doubled the zoom (${spread.width})`,
  );
  assert(
    Math.abs(spread.x + spread.width / 2 - cx) < 4 &&
      Math.abs(spread.y + spread.height / 2 - cy) < 4,
    "and the point they began around stayed under them",
  );

  // Both fingers together carry the page rather than resizing it.
  await finger("pointerdown", 1, cx - 100, cy);
  await finger("pointerdown", 2, cx + 100, cy);
  await finger("pointermove", 1, cx - 40, cy);
  await finger("pointermove", 2, cx + 160, cy);
  await page.waitForTimeout(300);
  const panned = await pageBox();
  await finger("pointerup", 1, cx - 40, cy);
  await finger("pointerup", 2, cx + 160, cy);
  await page.waitForTimeout(250);
  assert(
    Math.abs(panned.x - (spread.x + 60)) < 6 &&
      Math.abs(panned.width - spread.width) < 8,
    `two fingers together carried the page (${panned.x} against ${spread.x + 60})`,
  );

  // A rect begun with one finger is let go of when the second lands.
  await page.keyboard.press("Control+0");
  await page.waitForTimeout(250);
  await pickTool("Rect");
  // Named rows, not every row: an empty document's panel holds a line of
  // advice, which a first layer replaces rather than joins.
  const rows = await page.locator(".panel ul li .layer-name").count();
  const box = await pageBox();
  const [px, py] = [box.x + box.width / 4, box.y + box.height / 4];
  await finger("pointerdown", 1, px, py);
  await finger("pointermove", 1, px + 40, py + 40);
  await finger("pointerdown", 2, px + 140, py + 40);
  await finger("pointermove", 1, px + 20, py + 40);
  await finger("pointermove", 2, px + 240, py + 40);
  await page.waitForTimeout(250);
  await finger("pointerup", 1, px + 20, py + 40);
  await finger("pointerup", 2, px + 240, py + 40);
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li .layer-name").count()) === rows,
    "the rect the pinch interrupted was let go of, not drawn",
  );
  await page.evaluate(() => {
    [Element.prototype.setPointerCapture, Element.prototype.releasePointerCapture] =
      window.__capture;
  });

  // Leave a layer behind for the block that follows, drawn the way a
  // mouse draws one — on a page fitted to the window again, since the
  // pinch above left it several times its size.
  await page.keyboard.press("Control+0");
  await page.waitForTimeout(300);
  const fit = await pageBox();
  await page.mouse.move(fit.x + 40, fit.y + 40);
  await page.mouse.down();
  await page.mouse.move(fit.x + 200, fit.y + 160, { steps: 6 });
  await page.mouse.up();
  await page.waitForTimeout(300);
  assert(
    (await page.locator(".panel ul li .layer-name").count()) === rows + 1,
    "and a rect drawn with a mouse still lands",
  );
}

// 9n. The right-click menu: what can be done with what was pointed at,
// where it was pointed at. Right-clicking something not already picked
// picks it, and bare canvas offers what needs no layer at all.
{
  await newDocument(400, 300, "rgb");
  const b = await page.locator("#engine-page").boundingBox();
  const at = (x, y) => [b.x + (x / 400) * b.width, b.y + (y / 300) * b.height];
  const menu = page.locator(".context-menu");
  const drawRect = async (x0, y0, x1, y1) => {
    await pickTool("Rect");
    await page.mouse.move(...at(x0, y0));
    await page.mouse.down();
    await page.mouse.move(...at(x1, y1), { steps: 6 });
    await page.mouse.up();
    await page.waitForTimeout(300);
  };
  await drawRect(40, 40, 160, 160);
  await drawRect(220, 40, 340, 160);

  // Bare canvas: no layer in hand, so the menu offers the things that
  // need none.
  await pickTool("Move");
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
  await page.mouse.click(...at(200, 260), { button: "right" });
  await page.waitForTimeout(200);
  assert((await menu.count()) === 1, "right-click opens a menu");
  const bare = await menu.innerText();
  assert(
    bare.includes("Select all") && !bare.includes("Bring to front"),
    `with nothing picked it offers what needs nothing (${bare.replace(/\s+/g, " ")})`,
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(200);
  assert((await menu.count()) === 0, "and Escape puts it away");

  // Right-clicking the second rect picks it, and the menu is about it.
  await page.mouse.click(...at(280, 100), { button: "right" });
  await page.waitForTimeout(250);
  const picked = Number(
    await page.locator('input[aria-label="X position"]').inputValue(),
  );
  assert(
    Math.abs(picked - 220) < 1.5,
    `right-clicking a layer picks it (${picked})`,
  );
  const onIt = await menu.innerText();
  assert(
    onIt.includes("Bring to front") && onIt.includes("Delete"),
    "and the menu is about the layer",
  );
  // The menu sits where the pointer asked for it.
  const where = await menu.boundingBox();
  const [mx, my] = at(280, 100);
  assert(
    Math.abs(where.x - mx) < 3 && Math.abs(where.y - my) < 3,
    `the menu opened where the click was (${where.x},${where.y} against ${mx},${my})`,
  );

  // Choosing an item does what it says: this one is under the other, and
  // comes out over it.
  const rows = await page.locator(".panel ul li .layer-name").count();
  await menu.locator("text=Bring to front").click();
  await page.waitForTimeout(300);
  assert((await menu.count()) === 0, "choosing an item closes the menu");
  assert(
    (await page.locator(".panel ul li .layer-name").first().innerText()).includes(
      "Rect 2",
    ),
    "and it did what it said",
  );
  assert(
    (await page.locator(".panel ul li .layer-name").count()) === rows,
    "with nothing added or lost",
  );

  // A menu asked for hard against the corner stays inside the window
  // rather than hanging off it where nothing could reach it.
  const win = await page.evaluate(() => [window.innerWidth, window.innerHeight]);
  const host = await page.locator(".canvas-host").boundingBox();
  await page.mouse.click(host.x + host.width - 4, host.y + host.height - 4, {
    button: "right",
  });
  await page.waitForTimeout(250);
  const corner = await menu.boundingBox();
  assert(
    corner.x + corner.width <= win[0] && corner.y + corner.height <= win[1],
    `a menu in the corner stays in the window (${corner.x + corner.width} of ${win[0]}, ${corner.y + corner.height} of ${win[1]})`,
  );
  await page.keyboard.press("Escape");
  await page.waitForTimeout(150);
}

// 10. Recovery: a draft of the document is kept as it changes, and a
// fresh visit offers it back — restored, the layers and the ink return.
{
  const rowsBefore = await page.locator(".panel ul li .layer-name").count();
  assert(rowsBefore > 0, "something to lose");
  await page.waitForTimeout(2200); // the draft is written a breath after the last change
  await page.reload();
  await page.waitForSelector("#engine-canvas");
  await page.waitForTimeout(600);
  assert(await page.isVisible(".recover"), "a fresh visit offers the draft back");
  assert(
    (await page.locator(".panel ul li .layer-name").count()) === 0,
    "and nothing is restored until asked",
  );
  await page.click(".recover >> text=Restore");
  await page.waitForTimeout(600);
  assert(
    (await page.locator(".panel ul li .layer-name").count()) === rowsBefore &&
      (await page.locator(".recover").count()) === 0,
    `Restore brings the document back (${rowsBefore} layers)`,
  );
}

await page.screenshot({ path: join(OUT, "editor-final.png") });
assert(errors.length === 0, "no page errors: " + JSON.stringify(errors));

console.log("\nALL SMOKE TESTS PASSED");
await browser.close();
server.close();
process.exit(0);
