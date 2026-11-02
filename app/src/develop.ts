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
  /** Presence: contrast in the middle tones, edge-aware, -100 to 100. */
  clarity: number;
  /** Presence: the veil of air taken off (or put on), -100 to 100. */
  dehaze: number;
  /** Detail: unsharp mask on the brightness, 0 to 150. */
  sharpen: number;
  /** Detail: edge-aware smoothing, 0 to 100. */
  noise: number;
  /** Leave off the lens corrections the file carries (a Fuji's RAF). */
  uncorrected?: boolean;
  /** The tones a camera's own pictures have, under the rest: on for a new
   * development, off for one made before there was such a curve. */
  curve?: boolean;
  /** The tone curve drawn in the window, and one a channel: `[in, out]`
   * points from nought to one, as a Curves layer keeps them. Absent is
   * straight. */
  tone_curve?: [number, number][];
  red_curve?: [number, number][];
  green_curve?: [number, number][];
  blue_curve?: [number, number][];
  /** The colour mixer: hue, saturation and luminance, -100 to 100, for
   * each of the eight bands in `MIXER_BANDS`. Absent is untouched. */
  mixer?: [number, number, number][];
  /** Colour grading: `[hue, saturation, luminance]` for each zone (hue in
   * degrees, saturation 0 to 100, luminance -100 to 100), how far they
   * run into each other and where they divide. Absent is none. */
  grading?: Grading;
};

export type GradeZone = "shadows" | "midtones" | "highlights" | "global";
export type Grading = Record<GradeZone, [number, number, number]> & { blending: number; balance: number };
export const GRADE_ZONES: GradeZone[] = ["shadows", "midtones", "highlights", "global"];
export const NO_GRADING: Grading = {
  shadows: [0, 0, 0],
  midtones: [0, 0, 0],
  highlights: [0, 0, 0],
  global: [0, 0, 0],
  blending: 50,
  balance: 0,
};

/** Grading read from somewhere that might say anything: each zone three
 * numbers held to their ranges, blending and balance to theirs; a grading
 * that asks for nothing is none. */
export function cleanGrading(x: unknown): Grading | undefined {
  if (!x || typeof x !== "object") return undefined;
  const o = x as Record<string, unknown>;
  const num = (v: unknown, d: number, lo: number, hi: number) =>
    typeof v === "number" && Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : d;
  const zone = (v: unknown): [number, number, number] => {
    const a = Array.isArray(v) ? v : [];
    return [((num(a[0], 0, -1e6, 1e6) % 360) + 360) % 360, num(a[1], 0, 0, 100), num(a[2], 0, -100, 100)];
  };
  const g: Grading = {
    shadows: zone(o.shadows),
    midtones: zone(o.midtones),
    highlights: zone(o.highlights),
    global: zone(o.global),
    blending: num(o.blending, 50, 0, 100),
    balance: num(o.balance, 0, -100, 100),
  };
  return GRADE_ZONES.some((z) => g[z][1] !== 0 || g[z][2] !== 0) ? g : undefined;
}

/** The colour mixer's bands, as Lightroom names them, with a colour to
 * draw each slider in. */
export const MIXER_BANDS: [string, string][] = [
  ["Red", "#e5484d"],
  ["Orange", "#f76b15"],
  ["Yellow", "#ffc53d"],
  ["Green", "#30a46c"],
  ["Aqua", "#12a594"],
  ["Blue", "#3e63dd"],
  ["Purple", "#8e4ec6"],
  ["Magenta", "#d6409f"],
];

/** The mixer read from somewhere that might say anything: eight bands of
 * three numbers held to -100..100; anything else, or all nought, is none. */
export function cleanMixer(x: unknown): [number, number, number][] | undefined {
  if (!Array.isArray(x) || x.length !== 8) return undefined;
  const out: [number, number, number][] = [];
  for (const band of x) {
    if (!Array.isArray(band) || band.length !== 3 || !band.every((v) => typeof v === "number" && Number.isFinite(v))) {
      return undefined;
    }
    out.push(band.map((v: number) => Math.min(100, Math.max(-100, v))) as [number, number, number]);
  }
  return out.some((b) => b.some((v) => v !== 0)) ? out : undefined;
}

/** The four drawn curves' names in the settings. */
export const CURVE_KEYS = ["tone_curve", "red_curve", "green_curve", "blue_curve"] as const;
export type CurveKey = (typeof CURVE_KEYS)[number];

/** What the develop window starts from: the size, the camera's own
 * white balance as a temperature and tint, and whether the file carries
 * lens corrections. */
export type RawInfo = {
  width: number;
  height: number;
  asShot: [number, number];
  lens?: boolean;
  /** The lens profile the corrections came from (lensfun's name for the
   * lens), where the file carried none of its own. */
  lensProfile?: string;
};

export const RAW_DEFAULTS: RawSettings = {
  exposure: 0,
  contrast: 0,
  highlights: 0,
  shadows: 0,
  whites: 0,
  blacks: 0,
  vibrance: 0,
  saturation: 0,
  clarity: 0,
  dehaze: 0,
  sharpen: 0,
  noise: 0,
  curve: true,
};

/** Settings kept by name, to be given to another photograph in one
 * press: a look, or a way a camera's files want developing. About the
 * person, so kept in `prefs` rather than in a document. */
export type DevelopPreset = { name: string; settings: RawSettings };

/** What each setting can be, as the engine holds it to. */
const RANGES: Record<Exclude<keyof RawSettings, "white" | "uncorrected" | "curve" | "mixer" | "grading" | CurveKey>, [number, number]> = {
  exposure: [-5, 5],
  contrast: [-100, 100],
  highlights: [-100, 100],
  shadows: [-100, 100],
  whites: [-100, 100],
  blacks: [-100, 100],
  vibrance: [-100, 100],
  saturation: [-100, 100],
  clarity: [-100, 100],
  dehaze: [-100, 100],
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
  if (o.uncorrected === true) out.uncorrected = true;
  if (typeof o.curve === "boolean") out.curve = o.curve;
  for (const k of CURVE_KEYS) {
    const points = cleanCurve(o[k]);
    if (points) out[k] = points;
  }
  const mixer = cleanMixer(o.mixer);
  if (mixer) out.mixer = mixer;
  const grading = cleanGrading(o.grading);
  if (grading) out.grading = grading;
  return out;
}

/** A drawn curve read from somewhere that might say anything: two to
 * thirty-two points, each two numbers held to nought to one, in order of
 * their input; anything else, or the straight line, is no curve. */
export function cleanCurve(x: unknown): [number, number][] | undefined {
  if (!Array.isArray(x) || x.length < 2 || x.length > 32) return undefined;
  const points: [number, number][] = [];
  for (const p of x) {
    if (!Array.isArray(p) || p.length !== 2 || !p.every((v) => typeof v === "number" && Number.isFinite(v))) {
      return undefined;
    }
    points.push([Math.min(1, Math.max(0, p[0])), Math.min(1, Math.max(0, p[1]))]);
  }
  points.sort((a, b) => a[0] - b[0]);
  return points.every(([a, b]) => Math.abs(a - b) < 1e-6) ? undefined : points;
}

/** Each channel taken to its own ends, from the developed preview's
 * histogram: a thousandth of the picture left outside at each end, so a
 * speck of dust is not what decides where a channel's black is. What
 * pulls a colour cast out without being told what is grey — a channel
 * curve each, two points, the plainest curve that says it; a channel
 * with nothing to stretch is left straight. */
export function autoChannels(h: Histogram): Partial<Record<CurveKey, [number, number][]>> {
  const ends = (bins: Uint32Array): [number, number] | null => {
    let total = 0;
    for (const n of bins) total += n;
    if (total === 0) return null;
    const cut = total / 1000;
    let lo = 0;
    for (let seen = 0; lo < 255 && seen + bins[lo] <= cut; lo++) seen += bins[lo];
    let hi = 255;
    for (let seen = 0; hi > 0 && seen + bins[hi] <= cut; hi--) seen += bins[hi];
    return hi - lo >= 8 ? [lo / 255, hi / 255] : null;
  };
  const out: Partial<Record<CurveKey, [number, number][]>> = {};
  for (const [key, bins] of [["red_curve", h.r], ["green_curve", h.g], ["blue_curve", h.b]] as const) {
    const e = ends(bins);
    if (e && (e[0] > 0.004 || e[1] < 0.996)) out[key] = [[e[0], 0], [e[1], 1]];
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
  return (
    keys.every((k) => p.settings[k] === s[k]) &&
    white(p.settings.white, s.white) &&
    !!p.settings.uncorrected === !!s.uncorrected &&
    !!p.settings.curve === !!s.curve &&
    CURVE_KEYS.every((k) => JSON.stringify(p.settings[k] ?? null) === JSON.stringify(s[k] ?? null)) &&
    JSON.stringify(p.settings.mixer ?? null) === JSON.stringify(s.mixer ?? null) &&
    JSON.stringify(p.settings.grading ?? null) === JSON.stringify(s.grading ?? null)
  );
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
