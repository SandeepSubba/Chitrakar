/** Everything the app remembers about how you like to work.
 *
 * These were four localStorage keys under two different spellings
 * (`chitrakar:grid`, `chitrakar.units`) set from four places, and the
 * only way to change any of them was to find the one menu row that did
 * it. They are one object under one key now, which is what lets a
 * preferences window be a form over a value rather than a pile of
 * wires — and what stops the next setting from inventing a fifth
 * convention.
 *
 * What is *not* here: the panel's width and the toolbar's position.
 * Those are written on every frame of a drag, and a JSON blob rewritten
 * sixty times a second to remember a drag is not a preference, it is a
 * leak. They stay on their own keys.
 *
 * A preference is about the person, never the document: units, the grid
 * you work over, what export you reach for. Anything the file should
 * still say when it is opened on another machine belongs in the
 * document and goes through a `Command`.
 */

import { useCallback, useEffect, useState } from "react";
import { ALWAYS_SHOWN, isTool, type Tool } from "./tools";

export type Units = "px" | "mm" | "in";
export type ExportFormat = "png" | "jpeg" | "pdf" | "svg" | "tiff";
/** A frame exports at the multiple the *frame* asks for, under a name
 * the frame gives it, so it stays a row on the menu rather than a
 * choice here — see `exportArtboard`. */
export type ExportArea = "page" | "selection";

export type Prefs = {
  /** What the rulers and the geometry fields read in. */
  units: Units;
  /** How far apart the grid's lines are, in document pixels; 0 for none. */
  grid: number;
  /** Whether guides are drawn. */
  showGuides: boolean;
  /** How near a thing has to come before it catches, in screen pixels. */
  snap: number;
  /** How far an arrow key moves a layer, and how far with shift held. */
  nudge: number;
  nudgeBig: number;
  /** Whether the open document is kept in the browser between visits. */
  keepDraft: boolean;
  /** What a new document starts as, when nothing else is asked for. */
  newWidth: number;
  newHeight: number;
  newDpi: number;
  /** What the export window opens on. */
  exportFormat: ExportFormat;
  exportArea: ExportArea;
  exportScale: number;
  /** JPEG quality, 1–100. */
  jpegQuality: number;
  /** How far the edge of a picked region is softened, in pixels. */
  feather: number;
  /** How readily the subject pick lets go of a colour, 0–1. */
  subjectTolerance: number;
  /** The tools put away from the rail. They are still there — behind
   * the slot at the rail's end, and on their keys — but not in the way.
   * A list of what is *hidden* rather than what is shown, so a tool
   * this version adds appears for someone whose settings predate it. */
  hiddenTools: Tool[];
  /** Which of the bar's groups of buttons are shown. Everything on them
   * is on a menu as well, so a group put away costs nothing but reach. */
  barDocument: boolean;
  barSelection: boolean;
  barZoom: boolean;
};

export const DEFAULTS: Prefs = {
  units: "px",
  grid: 0,
  showGuides: true,
  snap: 6,
  nudge: 1,
  nudgeBig: 10,
  keepDraft: true,
  newWidth: 1024,
  newHeight: 768,
  newDpi: 72,
  exportFormat: "png",
  exportArea: "page",
  exportScale: 1,
  jpegQuality: 92,
  feather: 0,
  subjectTolerance: 0.5,
  hiddenTools: [],
  barDocument: true,
  barSelection: true,
  barZoom: true,
};

const KEY = "chitrakar:prefs";

/** The keys this replaced. Read once, so that a grid and a unit chosen
 * before any of this existed are still there afterwards — a preferences
 * window that silently resets what you had is worse than none. */
function legacy(): Partial<Prefs> {
  const was: Partial<Prefs> = {};
  try {
    const u = localStorage.getItem("chitrakar.units");
    if (u === "mm" || u === "in" || u === "px") was.units = u;
    const g = Number(localStorage.getItem("chitrakar:grid"));
    if (Number.isFinite(g) && g > 0) was.grid = g;
  } catch {
    // A browser that will not say is a browser with nothing to carry over.
  }
  return was;
}

/** Whatever is worth keeping out of a stored blob, defaults for the
 * rest. Every field is checked against the default's own type rather
 * than trusted: this is JSON from a disk that another version of the
 * app wrote, and a string where a number belongs would otherwise reach
 * the engine. */
export function readPrefs(): Prefs {
  let stored: unknown = null;
  try {
    stored = JSON.parse(localStorage.getItem(KEY) ?? "null");
  } catch {
    stored = null;
  }
  const from = { ...legacy(), ...(stored && typeof stored === "object" ? stored : {}) };
  const out = { ...DEFAULTS };
  for (const k of Object.keys(DEFAULTS) as (keyof Prefs)[]) {
    const v = (from as Record<string, unknown>)[k];
    if (typeof v === typeof DEFAULTS[k]) (out as Record<string, unknown>)[k] = v;
  }
  return clamp(out);
}

/** Preferences that cannot hurt anything downstream. A number typed
 * into a field arrives here before it reaches the engine, so this is
 * where a grid of -4 or a quality of 900 stops. */
export function clamp(p: Prefs): Prefs {
  const n = (v: number, lo: number, hi: number, fallback: number) =>
    Number.isFinite(v) ? Math.min(hi, Math.max(lo, v)) : fallback;
  return {
    ...p,
    // A value written by a version that offered more than these two
    // must not reach the window as an area it cannot show.
    exportArea: p.exportArea === "selection" ? "selection" : "page",
    grid: n(Math.round(p.grid), 0, 512, 0),
    snap: n(Math.round(p.snap), 0, 64, 6),
    nudge: n(p.nudge, 0.1, 100, 1),
    nudgeBig: n(p.nudgeBig, 0.1, 1000, 10),
    newWidth: n(Math.round(p.newWidth), 1, 8192, 1024),
    newHeight: n(Math.round(p.newHeight), 1, 8192, 768),
    newDpi: n(Math.round(p.newDpi), 1, 2400, 72),
    exportScale: n(p.exportScale, 0.05, 16, 1),
    jpegQuality: n(Math.round(p.jpegQuality), 1, 100, 92),
    feather: n(p.feather, 0, 500, 0),
    subjectTolerance: n(p.subjectTolerance, 0, 1, 0.5),
    // Only names that are tools, each once, and never the one tool the
    // rail cannot do without. `typeof []` is "object" and so is
    // `typeof null`, which is why the field-by-field read above is not
    // enough here.
    hiddenTools: Array.isArray(p.hiddenTools)
      ? p.hiddenTools.filter(
          (t, i, all) => isTool(t) && t !== ALWAYS_SHOWN && all.indexOf(t) === i,
        )
      : [],
  };
}

export function writePrefs(p: Prefs) {
  try {
    localStorage.setItem(KEY, JSON.stringify(p));
  } catch {
    // Out of room, or a browser told not to. The app works the same
    // this session; it just starts over on the next one.
  }
}

/** The preferences, and a way to change one or many of them. Held in
 * one state so a window that sets six at once is one render, and
 * written out whenever it changes. */
export function usePrefs() {
  const [prefs, setAll] = useState<Prefs>(readPrefs);
  useEffect(() => writePrefs(prefs), [prefs]);
  const set = useCallback(
    (patch: Partial<Prefs>) => setAll((p) => clamp({ ...p, ...patch })),
    [],
  );
  const reset = useCallback(() => setAll({ ...DEFAULTS }), []);
  return { prefs, set, reset };
}
