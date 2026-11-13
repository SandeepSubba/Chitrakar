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
// An arrow key moves the temperature by 50 K wherever it is, though a
// step of the slider is a few kelvin where it is warm.
for (const k of [2000, 2850, 5500, 7500, 24950]) {
  check(d.stepKelvin(k, 1) === Math.min(25000, k + 50), `${k} K steps up by 50`);
  check(d.stepKelvin(k, -1) === Math.max(2000, k - 50), `${k} K steps down by 50`);
}
check(d.stepKelvin(25000, 10) === 25000 && d.stepKelvin(2000, -10) === 2000, "and stays on the slider");
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

// Settings read back from what was kept are settings.
const clean = d.cleanSettings({ exposure: 99, contrast: "x", noise: -5, white: [100, 999], extra: 1 });
check(clean.exposure === 5 && clean.contrast === 0 && clean.noise === 0, "held to what each can be");
check(clean.white[0] === 2000 && clean.white[1] === 150, "a white balance held to the slider's ends");
check(d.cleanSettings({ white: [5000] }).white === undefined, "half a white balance is the camera's");
check(d.cleanSettings(null).exposure === 0, "nothing is the defaults");
check(d.withDefaults({ noise: 60 }).color_noise === 60, "a picture from before colour noise had its own slider keeps it following Noise");
check(d.withDefaults({ noise: 60, color_noise: 10 }).color_noise === 10, "one that has its own keeps it");
check(d.withDefaults({}).color_noise === 0, "and nothing is the default");
check(
  d.cleanSettings({ masking: 250 }).masking === 100 && d.cleanSettings({}).masking === 0,
  "masking held to 0..100, and none in a preset kept before it",
);
check(
  JSON.stringify(d.cleanSettings({ defringe: [250, -3] }).defringe) === "[100,0]" &&
    d.cleanSettings({ defringe: [0, 0] }).defringe === undefined &&
    d.cleanSettings({ defringe: "purple" }).defringe === undefined,
  "defringe held to 0..100 a colour, and none when it asks for nothing",
);
check(
  d.cleanSettings({ noise: 30 }).color_noise === 30 &&
    d.cleanSettings({ noise: 30, color_noise: 200 }).color_noise === 100 &&
    d.cleanSettings({ noise: 30, color_noise: 5 }).color_noise === 5 &&
    d.cleanSettings(null).color_noise === 0,
  "colour noise its own, held to 0..100, and following the one slider in settings from before there were two",
);
{
  // Every setting away from its default goes through as it came: a field
  // this forgot would be reset whenever a picture is developed again.
  const full = {
    white: [4800, 12],
    exposure: 0.5,
    contrast: 15,
    highlights: -40,
    shadows: 25,
    whites: -5,
    blacks: 8,
    vibrance: 20,
    saturation: -10,
    clarity: 12,
    dehaze: 6,
    sharpen: 40,
    noise: 20,
    color_noise: 35,
    masking: 30,
    uncorrected: true,
    fringe: true,
    defringe: [40, 10],
    curve: true,
    tone_curve: [[0, 0.05], [0.5, 0.55], [1, 0.97]],
    red_curve: [[0, 0], [0.4, 0.45], [1, 1]],
    green_curve: [[0, 0.02], [1, 1]],
    blue_curve: [[0, 0], [0.6, 0.55], [1, 1]],
    mixer: [[5, -20, 10], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [-8, 15, -12], [0, 0, 0], [0, 0, 0]],
    grading: {
      shadows: [210, 20, -5],
      midtones: [0, 0, 0],
      highlights: [40, 15, 3],
      global: [0, 0, 0],
      blending: 60,
      balance: -10,
    },
  };
  const sorted = (o) =>
    JSON.stringify(o, (_, v) => (v && typeof v === "object" && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort()) : v));
  const back = d.cleanSettings(full);
  const lost = Object.keys(full).filter((k) => sorted(full[k]) !== sorted(back[k]));
  check(lost.length === 0, `every setting comes through as it went (lost: ${lost.join(", ") || "none"})`);
}
check(d.cleanSettings(null).curve === true, "a new development has the camera's tone curve");
check(d.cleanSettings({ curve: false }).curve === false, "a curve left off stays off");
check(d.cleanSettings({ curve: "yes" }).curve === true, "a curve that is not a yes or a no is the default");
const presets = d.cleanPresets([
  { name: "Warm", settings: { white: [6500, 10], vibrance: 20 } },
  { name: "Warm", settings: {} },
  { name: "", settings: {} },
  "nonsense",
  { name: "Flat", settings: { contrast: -40 } },
]);
check(presets.map((p) => p.name).join() === "Warm,Flat", `a name each, once (${presets.map((p) => p.name)})`);
check(d.cleanPresets("x").length === 0, "a list or nothing");
check(d.samePreset(presets[1], { ...d.RAW_DEFAULTS, contrast: -40 }), "a preset is in hand when every setting is its");
check(!d.samePreset(presets[1], { ...d.RAW_DEFAULTS, contrast: -40, white: [5000, 0] }), "and not when the light differs");

// Drawn curves read back: held to the square, in order, and the straight
// line is no curve.
const curved = d.cleanSettings({ tone_curve: [[1, 1.4], [0, -1], [0.5, 0.7]] });
check(JSON.stringify(curved.tone_curve) === "[[0,0],[0.5,0.7],[1,1]]", `a curve held and put in order (${JSON.stringify(curved.tone_curve)})`);
check(d.cleanSettings({ red_curve: [[0, 0], [1, 1]] }).red_curve === undefined, "a straight curve is none");
check(d.cleanSettings({ blue_curve: [[0, 0], ["x", 1]] }).blue_curve === undefined, "a curve that is not numbers is none");
check(d.cleanSettings({ green_curve: Array.from({ length: 40 }, (_, i) => [i / 40, i / 40]) }).green_curve === undefined, "nor one of forty points");
check(
  !d.samePreset({ name: "x", settings: { ...d.RAW_DEFAULTS } }, { ...d.RAW_DEFAULTS, tone_curve: [[0, 0], [0.5, 0.6], [1, 1]] }),
  "a preset is not in hand when the curve differs",
);
// Each channel to its own ends: a picture whose red stops short of white
// and whose blue starts above black.
{
  const px = new Uint8ClampedArray(4 * 1000);
  for (let i = 0; i < 1000; i++) {
    const t = i / 999;
    px.set([Math.round(t * 200), Math.round(t * 255), Math.round(30 + t * 225), 255], i * 4);
  }
  const auto = d.autoChannels(d.histogram(px));
  check(auto.red_curve && Math.abs(auto.red_curve[1][0] - 200 / 255) < 0.01, `red stretched to where it stops (${JSON.stringify(auto.red_curve)})`);
  check(auto.blue_curve && Math.abs(auto.blue_curve[0][0] - 30 / 255) < 0.01, `blue from where it starts (${JSON.stringify(auto.blue_curve)})`);
  check(auto.green_curve === undefined, "and green, already full, left straight");
}

// The colour mixer read back: eight bands of three, held to the sliders.
const mixed = d.cleanSettings({ mixer: Array.from({ length: 8 }, (_, i) => (i === 5 ? [0, 0, -250] : [0, 0, 0])) });
check(mixed.mixer && mixed.mixer[5][2] === -100, `a mixer held to its sliders (${JSON.stringify(mixed.mixer)})`);
check(d.cleanSettings({ mixer: Array.from({ length: 8 }, () => [0, 0, 0]) }).mixer === undefined, "an untouched mixer is none");
check(d.cleanSettings({ mixer: [[1, 2, 3]] }).mixer === undefined, "and one of the wrong shape is none");
check(d.MIXER_BANDS.length === 8, "eight bands");

// Grading read back: zones held, the hue taken round the circle, and a
// grading that asks for nothing is none.
const graded = d.cleanGrading({ shadows: [-170, 140, 0], blending: 200 });
check(graded && graded.shadows[0] === 190 && graded.shadows[1] === 100 && graded.blending === 100, `grading held (${JSON.stringify(graded)})`);
check(d.cleanGrading({ balance: 40 }) === undefined, "balance alone is no grading");
check(d.cleanSettings({ grading: { highlights: [40, 30, 0] } }).grading.highlights[1] === 30, "grading kept in the settings");

if (fails.length) {
  console.error(`${fails.length} of ${ran} failed:\n  ` + fails.join("\n  "));
  process.exit(1);
}
console.log(`develop: ${ran} checks passed`);
