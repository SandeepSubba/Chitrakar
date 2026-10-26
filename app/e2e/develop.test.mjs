// Unit tests for `src/develop.ts` — the develop window's arithmetic: the
// temperature slider's scale, the named lights, and the histogram and
// clipping read off a developed preview. Asked without a browser.
//
//   node e2e/develop.test.mjs
import { build } from "esbuild";
import { unlinkSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const tmp = join(here, "out", "develop.generated.mjs");
await build({
  entryPoints: [join(here, "..", "src", "develop.ts")],
  bundle: true,
  format: "esm",
  outfile: tmp,
  logLevel: "silent",
});
const d = await import("file://" + tmp);
unlinkSync(tmp);

let ran = 0;
const fails = [];
const check = (cond, msg) => {
  ran += 1;
  if (!cond) fails.push(msg);
};

// The temperature slider: its ends, round trips, and a step of colour
// the same size wherever it is.
check(d.sliderToKelvin(0) === 2000 && d.sliderToKelvin(1000) === 25000, "the slider runs 2000 K to 25000 K");
for (const k of [2000, 2850, 3800, 5500, 6500, 7500, 12000, 25000]) {
  check(d.sliderToKelvin(d.kelvinToSlider(k)) === k, `${k} K comes back as itself`);
}
const step = (a, b) => d.kelvinToSlider(b) - d.kelvinToSlider(a);
check(Math.abs(step(2000, 2500) - step(5000, 10000)) < 1, "2000 to 2500 K is as far as 5000 to 10000 K");
for (let p = 0; p < 1000; p += 37) check(d.sliderToKelvin(p + 37) >= d.sliderToKelvin(p), `warmer to the right at ${p}`);

// The lights, named and not.
check(d.lightOf(d.RAW_DEFAULTS) === "As shot", "no white balance is the camera's");
check(d.lightOf({ ...d.RAW_DEFAULTS, white: [2850, 0] }) === "Tungsten", "a named light by its numbers");
check(d.lightOf({ ...d.RAW_DEFAULTS, white: [2851, 0] }) === "Custom", "anything else is one's own");

// The histogram: a count per level, and what is clipped.
const px = (...pixels) => Uint8ClampedArray.from(pixels.flatMap((p) => [...p, 255]));
const h = d.histogram(px([255, 10, 10], [0, 0, 0], [0, 0, 0], [128, 128, 128]));
check(h.r[255] === 1 && h.r[0] === 2 && h.g[128] === 1 && h.b[10] === 1, "a count per level per channel");
check(h.high === 0.25 && h.low === 0.5, `clipped: a quarter highlights, half shadows (${h.high}, ${h.low})`);
check(d.histogram(new Uint8ClampedArray(0)).high === 0, "an empty picture clips nothing");

// Clipping painted: red over highlights, blue over shadows, the rest left.
const pic = px([255, 200, 100], [0, 0, 0], [40, 50, 60]);
d.markClipping(pic, true, false);
check(pic[0] === 255 && pic[1] === 0 && pic[2] === 0, "a clipped highlight goes red");
check(pic[4] === 0 && pic[5] === 0 && pic[6] === 0, "shadows left when not asked");
d.markClipping(pic, false, true);
check(pic[6] === 255 && pic[5] === 80, "a clipped shadow goes blue");
check(pic[8] === 40 && pic[9] === 50 && pic[10] === 60, "and the rest is untouched");

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`develop: ${ran} checks passed`);
