/** The develop window's arithmetic, kept apart from the window so it can
 * be tried without a browser (`e2e/develop.test.mjs`): the settings and
 * their defaults, the named lights, the temperature slider's scale, and
 * the histogram and clipping read off the developed preview. */

/** How a camera's raw file is developed: `RawDevelop` in the engine. */
export type RawSettings = {
  /** Kelvin and tint; absent for the camera's own. */
  white?: [number, number];
  exposure: number;
  contrast: number;
  highlights: number;
  shadows: number;
  whites: number;
  blacks: number;
  vibrance: number;
  saturation: number;
  /** Detail: unsharp mask on the brightness, 0 to 150. */
  sharpen: number;
  /** Detail: edge-aware smoothing, 0 to 100. */
  noise: number;
};

/** What the develop window starts from: the size and the camera's own
 * white balance as a temperature and tint. */
export type RawInfo = { width: number; height: number; asShot: [number, number] };

export const RAW_DEFAULTS: RawSettings = {
  exposure: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  whites: 0,
  blacks: 0,
  vibrance: 0,
  saturation: 0,
  sharpen: 0,
  noise: 0,
};

/** Settings kept by name, to be given to another photograph in one
 * press: a look, or a way a camera's files want developing. About the
 * person, so kept in `prefs` rather than in a document. */
export type DevelopPreset = { name: string; settings: RawSettings };

/** What each setting can be, as the engine holds it to. */
const RANGES: Record<Exclude<keyof RawSettings, "white">, [number, number]> = {
  exposure: [-5, 5],
  contrast: [-100, 100],
  highlights: [-100, 100],
  shadows: [-100, 100],
  whites: [-100, 100],
  blacks: [-100, 100],
  vibrance: [-100, 100],
  saturation: [-100, 100],
  sharpen: [0, 150],
  noise: [0, 100],
};

/** Settings read from somewhere that might say anything (a kept preset,
 * a file): each a number within what it can be, else its default; a
 * white balance of two numbers, else the camera's own. */
export function cleanSettings(x: unknown): RawSettings {
  const o = (x && typeof x === "object" ? x : {}) as Record<string, unknown>;
  const out = { ...RAW_DEFAULTS };
  for (const [k, [lo, hi]] of Object.entries(RANGES) as [keyof typeof RANGES, [number, number]][]) {
    const v = o[k];
    if (typeof v === "number" && Number.isFinite(v)) out[k] = Math.min(hi, Math.max(lo, v));
  }
  const w = o.white;
  if (Array.isArray(w) && w.length === 2 && w.every((v) => typeof v === "number" && Number.isFinite(v))) {
    out.white = [Math.min(25000, Math.max(2000, w[0])), Math.min(150, Math.max(-150, w[1]))];
  }
  return out;
}

/** Kept presets as they are read back: a name each, once, and settings
 * that are settings. */
export function cleanPresets(x: unknown): DevelopPreset[] {
  if (!Array.isArray(x)) return [];
  const seen = new Set<string>();
  const out: DevelopPreset[] = [];
  for (const p of x) {
    if (!p || typeof p !== "object") continue;
    const { name, settings } = p as { name?: unknown; settings?: unknown };
    if (typeof name !== "string" || name.trim() === "" || seen.has(name)) continue;
    seen.add(name);
    out.push({ name, settings: cleanSettings(settings) });
  }
  return out;
}

/** Whether settings are a preset's: every one the same. */
export function samePreset(p: DevelopPreset, s: RawSettings): boolean {
  const keys = Object.keys(RANGES) as (keyof typeof RANGES)[];
  const white = (a?: [number, number], b?: [number, number]) =>
    (!a && !b) || (!!a && !!b && a[0] === b[0] && a[1] === b[1]);
  return keys.every((k) => p.settings[k] === s[k]) && white(p.settings.white, s.white);
}

/** Lightroom's names for a light, and what they are. */
export const LIGHTS: [string, [number, number] | null][] = [
  ["As shot", null],
  ["Daylight", [5500, 0]],
  ["Cloudy", [6500, 0]],
  ["Shade", [7500, 0]],
  ["Tungsten", [2850, 0]],
  ["Fluorescent", [3800, 20]],
  ["Flash", [5500, 0]],
];

/** Which named light settings are, "As shot" or "Custom" else. */
export function lightOf(s: RawSettings): string {
  if (!s.white) return "As shot";
  const [k, t] = s.white;
  return LIGHTS.find(([, w]) => w && w[0] === k && w[1] === t)?.[0] ?? "Custom";
}

/** Temperature rides its slider in mireds (a million over kelvin), where
 * a step is a step of colour: 2000 K and 2500 K are as far apart as
 * 5000 K and 10000 K. The slider runs 0 to 1000. */
const MIRED_WARM = 1e6 / 2000;
const MIRED_COOL = 1e6 / 25000;
export const kelvinToSlider = (k: number) =>
  ((MIRED_WARM - 1e6 / k) / (MIRED_WARM - MIRED_COOL)) * 1000;
/** And back, to the nearest 50 K. */
export const sliderToKelvin = (p: number) =>
  Math.round(1e6 / (MIRED_WARM - (p / 1000) * (MIRED_WARM - MIRED_COOL)) / 50) * 50;

/** A picture's histogram, a count per level per channel, and how much of
 * it is clipped: any channel at 255 (a highlight with nothing left in
 * it), or every channel at 0 (a shadow with nothing in it). */
export type Histogram = {
  r: Uint32Array;
  g: Uint32Array;
  b: Uint32Array;
  /** Fractions of the picture. */
  high: number;
  low: number;
};

export function histogram(rgba: Uint8ClampedArray | Uint8Array): Histogram {
  const r = new Uint32Array(256);
  const g = new Uint32Array(256);
  const b = new Uint32Array(256);
  let high = 0;
  let low = 0;
  const n = Math.floor(rgba.length / 4);
  for (let i = 0; i < n * 4; i += 4) {
    const [pr, pg, pb] = [rgba[i], rgba[i + 1], rgba[i + 2]];
    r[pr]++;
    g[pg]++;
    b[pb]++;
    if (pr === 255 || pg === 255 || pb === 255) high++;
    else if (pr === 0 && pg === 0 && pb === 0) low++;
  }
  return { r, g, b, high: n ? high / n : 0, low: n ? low / n : 0 };
}

/** Paint what is clipped over a picture, in place: highlights red, shadows
 * blue — the colours every raw developer has used for it. */
export function markClipping(rgba: Uint8ClampedArray | Uint8Array, high: boolean, low: boolean) {
  for (let i = 0; i < rgba.length; i += 4) {
    const [pr, pg, pb] = [rgba[i], rgba[i + 1], rgba[i + 2]];
    if (high && (pr === 255 || pg === 255 || pb === 255)) {
      rgba[i] = 255;
      rgba[i + 1] = 0;
      rgba[i + 2] = 0;
    } else if (low && pr === 0 && pg === 0 && pb === 0) {
      rgba[i] = 0;
      rgba[i + 1] = 80;
      rgba[i + 2] = 255;
    }
  }
}

/** The histogram drawn: each channel as a filled curve, added together
 * so where all three agree it is white, as Lightroom draws it. Heights
 * are taken against the tallest level but the two ends, which a clipped
 * picture piles up in and would flatten everything else. */
export function drawHistogram(ctx: CanvasRenderingContext2D, h: Histogram, w: number, ht: number) {
  ctx.clearRect(0, 0, w, ht);
  let top = 1;
  for (const ch of [h.r, h.g, h.b]) for (let i = 1; i < 255; i++) top = Math.max(top, ch[i]);
  ctx.globalCompositeOperation = "lighter";
  const colours: [Uint32Array, string][] = [
    [h.r, "rgba(230, 60, 60, 0.85)"],
    [h.g, "rgba(60, 200, 80, 0.85)"],
    [h.b, "rgba(70, 110, 240, 0.85)"],
  ];
  for (const [ch, colour] of colours) {
    ctx.fillStyle = colour;
    ctx.beginPath();
    ctx.moveTo(0, ht);
    for (let i = 0; i < 256; i++) {
      // The square root: a level with a hundredth as many pixels still shows.
      const v = Math.min(1, Math.sqrt(ch[i] / top));
      ctx.lineTo((i / 255) * w, ht - v * ht);
    }
    ctx.lineTo(w, ht);
    ctx.closePath();
    ctx.fill();
  }
  ctx.globalCompositeOperation = "source-over";
}
