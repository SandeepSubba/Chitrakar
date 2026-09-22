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

import { COMMANDS, isChord, type CommandKeys } from "./commands";
import { useCallback, useEffect, useState } from "react";
import { ALWAYS_SHOWN, KEYED_TOOLS, TOOL_KEYS, type Tool, type ToolKeys, isTool } from "./tools";

export type Units = "px" | "mm" | "in";
/** How the chrome is coloured: as the system says, or dark or light
 * whatever it says. */
export type Theme = "system" | "dark" | "light";
export type ExportFormat = "png" | "jpeg" | "pdf" | "svg" | "tiff";
/** A frame exports at the multiple the *frame* asks for, under a name
 * the frame gives it, so it stays a row on the menu rather than a
 * choice here — see `exportArtboard`. */
/** What leaves the document: the whole page, whatever is picked out,
 * or every region kept by name — a slice each, in one press. */
export type ExportArea = "page" | "selection" | "regions";

/** A brush kept under a name: how wide, and how soft its edge. What
 * Photoshop's and Affinity's brush panels are a panel of; here a row of
 * chips over the same two numbers, since a brush is those two numbers
 * and a name. About the person, not the file — the same brushes in
 * every document — so it lives here. */
export type BrushPreset = { name: string; size: number; softness: number };

/** An export kept by name: the four answers the export window asks
 * for, so a setup reached for often — the JPEG at 80 for the web, the
 * PNG set for an app — is one press rather than four. */
export type ExportSetup = {
  name: string;
  format: ExportFormat;
  area: ExportArea;
  scale: number;
  jpegQuality: number;
};

export const EXPORT_FORMATS: readonly ExportFormat[] = ["png", "jpeg", "pdf", "svg", "tiff"];

export type Prefs = {
  /** What the rulers and the geometry fields read in. */
  units: Units;
  /** The chrome's colours: the system's choice, or one of the two
   * outright. The page is the page under either. */
  theme: Theme;
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
  /** The tool keys rebound: the key each named tool answers to instead
   * of its default. Only what differs from the defaults is kept, so a
   * default that changes reaches everyone who did not rebind it. */
  toolKeys: ToolKeys;
  /** The command chords rebound: what each named command answers to
   * instead of the chord it shipped with. Only what differs is kept. */
  commandKeys: CommandKeys;
  /** Which of the bar's groups of buttons are shown. Everything on them
   * is on a menu as well, so a group put away costs nothing but reach. */
  barDocument: boolean;
  barSelection: boolean;
  barZoom: boolean;
  /** The brushes kept by name. */
  brushes: BrushPreset[];
  /** The exports kept by name. */
  exportSetups: ExportSetup[];
};

export const DEFAULTS: Prefs = {
  units: "px",
  theme: "system",
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
  toolKeys: {},
  commandKeys: {},
  exportSetups: [],
  barDocument: true,
  barSelection: true,
  barZoom: true,
  // Three to start with, the way every editor's panel has a few: a
  // fine hard one, a soft wide one, and one in between.
  brushes: [
    { name: "Fine", size: 4, softness: 0.1 },
    { name: "Medium", size: 24, softness: 0.5 },
    { name: "Soft", size: 60, softness: 0.9 },
  ],
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
    exportArea:
      p.exportArea === "selection" || p.exportArea === "regions" ? p.exportArea : "page",
    exportFormat: EXPORT_FORMATS.includes(p.exportFormat) ? p.exportFormat : "png",
    theme: p.theme === "dark" || p.theme === "light" ? p.theme : "system",
    grid: n(Math.round(p.grid), 0, 512, 0),
    snap: n(Math.round(p.snap), 0, 64, 6),
    nudge: n(p.nudge, 0.1, 100, 1),
    nudgeBig: n(p.nudgeBig, 0.1, 1000, 10),
    newWidth: n(Math.round(p.newWidth), 1, 8192, 1024),
    newHeight: n(Math.round(p.newHeight), 1, 8192, 768),
    newDpi: n(Math.round(p.newDpi), 1, 2400, 72),
    // Zero is not a scale but the set — 1×, 2× and 3× in one press —
    // and the window never hands it to the engine as one.
    exportScale: p.exportScale === 0 ? 0 : n(p.exportScale, 0.05, 16, 1),
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
    // Only named commands, one chord each, each chord once, and
    // nothing that only says the default again.
    commandKeys: (() => {
      const out: CommandKeys = {};
      const raw = p.commandKeys;
      if (!raw || typeof raw !== "object") return out;
      const seen = new Set<string>();
      for (const [id, chord] of Object.entries(raw as Record<string, unknown>)) {
        const known = COMMANDS.find((c) => c.id === id);
        if (!known || !isChord(chord) || seen.has(chord)) continue;
        if (known.chord === chord) continue;
        seen.add(chord);
        out[known.id] = chord;
      }
      return out;
    })(),
    // Only tools that hold a key, one character each, each key once,
    // and nothing that only says the default again.
    toolKeys: (() => {
      const out: ToolKeys = {};
      const raw = p.toolKeys;
      if (!raw || typeof raw !== "object") return out;
      const seen = new Set<string>();
      for (const [t, k] of Object.entries(raw as Record<string, unknown>)) {
        if (!isTool(t) || !KEYED_TOOLS.has(t) || typeof k !== "string") continue;
        const key = k.toLowerCase();
        if (!/^[a-z0-9]$/.test(key) || seen.has(key)) continue;
        if (TOOL_KEYS[key] === t) continue;
        seen.add(key);
        out[t] = key;
      }
      return out;
    })(),
    // Each setup a name and four answers the window can take; names
    // each once; anything else is dropped rather than offered.
    exportSetups: Array.isArray(p.exportSetups)
      ? p.exportSetups
          .filter(
            (e): e is ExportSetup =>
              !!e &&
              typeof e === "object" &&
              typeof e.name === "string" &&
              e.name.trim() !== "" &&
              EXPORT_FORMATS.includes(e.format) &&
              typeof e.scale === "number" &&
              typeof e.jpegQuality === "number",
          )
          .filter((e, i, all) => all.findIndex((o) => o.name === e.name) === i)
          .map((e) => ({
            name: e.name,
            format: e.format,
            area: e.area === "selection" || e.area === "regions" ? e.area : "page",
            scale: e.scale === 0 ? 0 : n(e.scale, 0.05, 16, 1),
            jpegQuality: n(Math.round(e.jpegQuality), 1, 100, 92),
          }))
      : [],
    // Each brush a name and two numbers within what the brush takes,
    // names each once; anything else on the list is dropped rather than
    // handed to a tool.
    brushes: Array.isArray(p.brushes)
      ? p.brushes
          .filter(
            (b): b is BrushPreset =>
              !!b &&
              typeof b === "object" &&
              typeof b.name === "string" &&
              b.name.trim() !== "" &&
              typeof b.size === "number" &&
              typeof b.softness === "number",
          )
          .filter((b, i, all) => all.findIndex((o) => o.name === b.name) === i)
          .map((b) => ({
            name: b.name,
            size: n(Math.round(b.size), 1, 200, 24),
            softness: n(b.softness, 0, 1, 0.5),
          }))
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
