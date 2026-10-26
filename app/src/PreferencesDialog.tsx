/** Everything the app remembers about how you like to work, in one
 * window.
 *
 * These settings existed before this did — they were just scattered
 * across the menus that happened to use them. Units, the grid and the
 * monitor profile were three separate stretches of the View menu; the
 * softness of a picked edge was an unlabelled number on a rail; how far
 * an arrow key moves a layer was a `1` written into the keyboard
 * handler and reachable from nowhere at all. A setting you cannot find
 * is a setting you do not have.
 *
 * The shape is Photoshop's and Affinity's both: a rail of categories on
 * the left, that category's settings on the right. It earns the extra
 * furniture at six groups — a flat list of eighteen controls is a
 * search problem, and every one of these apps arrived at the same
 * answer for the same reason.
 *
 * What is deliberately *not* here: anything about the open document.
 * Its size, its resolution, its press profile and its guides are the
 * file's own and travel with it; a preference is about the person and
 * stays on this machine. The one place they touch is "New documents",
 * which is not the document's size but the size the next one starts at.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import { Icon, type IconName } from "./icons";
import { isTauri } from "./nativeMenu";
import {
  COMMANDS,
  COMMAND_GROUPS,
  boundChords,
  chordFromEvent,
  chordLabel,
  claimable,
  giveChord,
  restoreChord,
  type CommandId,
} from "./commands";
import { DEFAULTS, UI_SCALES, type ExportFormat, type FingerDoes, type Prefs, type Theme, type Units } from "./prefs";
import {
  ALWAYS_SHOWN,
  KEYED_TOOLS,
  RAIL,
  SELECT_TOOLS,
  SHAPE_TOOLS,
  TOOL_ABOUT,
  TOOL_ICONS,
  TOOL_KEYS,
  boundKeys,
  type Tool,
  type ToolKeys,
} from "./tools";

export type PrefGroup =
  | "general"
  | "appearance"
  | "tools"
  | "keys"
  | "guides"
  | "selection"
  | "colour"
  | "new"
  | "export";
type Group = PrefGroup;

const GROUPS: { id: Group; label: string; icon: IconName }[] = [
  { id: "general", label: "General", icon: "units" },
  { id: "appearance", label: "Appearance", icon: "eye" },
  { id: "tools", label: "Tools", icon: "brush" },
  { id: "keys", label: "Keys", icon: "text" },
  { id: "guides", label: "Guides & grid", icon: "fit" },
  { id: "selection", label: "Selection", icon: "marquee" },
  { id: "colour", label: "Colour", icon: "proof" },
  { id: "new", label: "New documents", icon: "newDoc" },
  { id: "export", label: "Export", icon: "export" },
];

/** Colours to reach for as an accent or for the sliders: chosen to
 * read on both themes' panels. */
const ACCENTS: [string, string][] = [
  ["Blue", "#5b7cfa"],
  ["Violet", "#9b72f2"],
  ["Teal", "#1fa79b"],
  ["Green", "#3fa35b"],
  ["Amber", "#e0a530"],
  ["Orange", "#ee7337"],
  ["Rose", "#e0517c"],
  ["Graphite", "#7d8494"],
];

const SCALE_NAMES: Record<number, string> = {
  0.9: "Compact",
  1: "Standard",
  1.125: "Large",
  1.25: "Larger",
};

const UNIT_NAMES: Record<Units, string> = {
  px: "Pixels",
  mm: "Millimetres",
  in: "Inches",
};

const FORMAT_NAMES: Record<ExportFormat, string> = {
  png: "PNG",
  jpeg: "JPEG",
  webp: "WebP",
  pdf: "PDF",
  svg: "SVG",
  tiff: "TIFF (CMYK)",
};

/** The rail's sections as the window lists them: the shared slots
 * opened out into the tools they hold, so each tool is its own row. */
const RAIL_ROWS: readonly (readonly Tool[])[] = RAIL.map((section) =>
  section.flatMap((t) =>
    t === "Select" ? [...SELECT_TOOLS] : t === "Rect" ? [...SHAPE_TOOLS] : [t],
  ),
);

export function PreferencesDialog({
  initialGroup,
  prefs,
  setPrefs,
  resetPrefs,
  hasScreenIcc,
  onLoadScreenIcc,
  onDisplayP3,
  onClearScreenIcc,
  menuRows = [],
  onClose,
}: {
  initialGroup?: PrefGroup;
  /** Every menu row a person may give a chord of their own, with the
   * way to it through the menus. */
  menuRows?: { id: string; path: string; label: string }[];
  prefs: Prefs;
  setPrefs: (p: Partial<Prefs>) => void;
  resetPrefs: () => void;
  hasScreenIcc: boolean;
  onLoadScreenIcc: () => void;
  onDisplayP3: () => void;
  onClearScreenIcc: () => void;
  onClose: () => void;
}) {
  const [group, setGroup] = useState<Group>(initialGroup ?? "general");
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);

  /** The tool keys as they stand, and which tool is waiting for a key
   * to be pressed for it. */
  const keys = useMemo(() => boundKeys(prefs.toolKeys), [prefs.toolKeys]);
  const [listening, setListening] = useState<Tool | null>(null);
  /** The chords as they stand, and which command or menu row is
   * waiting for one: `cmd:<id>` or `row:<id>`. */
  const chords = useMemo(
    () => boundChords(prefs.commandKeys, prefs.menuKeys),
    [prefs.commandKeys, prefs.menuKeys],
  );
  const [catching, setCatching] = useState<string | null>(null);
  /** Why the last chord pressed was not taken, said under the list. */
  const [refused, setRefused] = useState<string | null>(null);
  /** Words to find a menu row by. */
  const [rowQuery, setRowQuery] = useState("");
  useEffect(() => {
    if (!catching) return;
    const command = catching.startsWith("cmd:") ? (catching.slice(4) as CommandId) : null;
    const target: { command: CommandId } | { row: string } = command
      ? { command }
      : { row: catching.slice(4) };
    // Ahead of everyone else, so the chord pressed is caught rather than
    // obeyed: this window is reached by one of the chords being rebound.
    const key = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setCatching(null);
        return;
      }
      // A modifier on its own is the start of a chord, not a chord.
      if (["Control", "Meta", "Alt", "Shift"].includes(e.key)) return;
      let next;
      if (e.key === "Backspace") {
        // Back to what it shipped with — a command's chord, taken back
        // from whoever holds it now; a menu row ships with none.
        next = command
          ? restoreChord(command, prefs.commandKeys, prefs.menuKeys)
          : giveChord(target, "", prefs.commandKeys, prefs.menuKeys);
      } else if (e.key === "Delete") {
        // No chord at all.
        next = giveChord(target, "", prefs.commandKeys, prefs.menuKeys);
      } else {
        const pressed = chordFromEvent(e);
        if (!claimable(pressed)) {
          // A letter on its own is a tool's, a bracket the brush's: say
          // so and keep listening rather than taking it.
          setRefused(
            pressed
              ? `${chordLabel(pressed)} on its own belongs to the tools — hold Ctrl or Alt with it.`
              : "That key cannot be given; try a letter, a digit or a sign with Ctrl or Alt.",
          );
          return;
        }
        next = giveChord(target, pressed, prefs.commandKeys, prefs.menuKeys);
      }
      setRefused(null);
      setPrefs({ commandKeys: next.commandKeys, menuKeys: next.menuKeys });
      setCatching(null);
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [catching, prefs.commandKeys, prefs.menuKeys, setPrefs]);
  useEffect(() => {
    if (!listening) return;
    const tool = listening;
    // On the window and ahead of everyone else, so the letter pressed
    // rebinds the tool rather than picking one, and Escape ends the
    // listening rather than the window.
    const key = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.key === "Escape") {
        setListening(null);
        return;
      }
      const pressed = e.key.toLowerCase();
      const next: ToolKeys = { ...prefs.toolKeys };
      if (e.key === "Backspace" || e.key === "Delete") {
        // Back to the default — which is what was asked for, so a
        // rebinding of another tool that holds that key gives it up
        // and falls back to its own default.
        delete next[tool];
        const shipped = Object.entries(TOOL_KEYS).find(([, t]) => t === tool)?.[0];
        for (const t of Object.keys(next) as Tool[]) {
          if (next[t] === shipped) delete next[t];
        }
      } else if (/^[a-z0-9]$/.test(pressed)) {
        // A key another tool holds changes hands: that tool gets the
        // key this one had, so nothing is left unreachable by the
        // swap. A key nobody holds is simply taken.
        const holder = keys.byKey[pressed];
        const had = keys.hint[tool].toLowerCase();
        if (holder && holder !== tool) {
          if (had) next[holder] = had;
          else delete next[holder];
        }
        next[tool] = pressed;
      } else {
        // A modifier or an arrow is not a tool key; keep listening.
        return;
      }
      // A rebinding that only says the default again is not kept.
      for (const t of Object.keys(next) as Tool[]) {
        if (next[t] && TOOL_KEYS[next[t]!] === t) delete next[t];
      }
      setPrefs({ toolKeys: next });
      setListening(null);
    };
    window.addEventListener("keydown", key, true);
    return () => window.removeEventListener("keydown", key, true);
  }, [listening, keys, prefs.toolKeys, setPrefs]);

  /** A whole number typed into a field. The value is only taken when it
   * is one: a field cleared to type a new number reads as NaN for a
   * keystroke, and clamping that to the minimum fights the typing. */
  const num = (
    label: string,
    value: number,
    set: (v: number) => void,
    opts: { min?: number; max?: number; step?: number; hint?: string } = {},
  ) => (
    <label className="row">
      {label}
      <input
        type="number"
        min={opts.min ?? 0}
        max={opts.max}
        step={opts.step ?? 1}
        value={value}
        onChange={(e) => {
          const v = Number(e.target.value);
          if (Number.isFinite(v)) set(v);
        }}
        aria-label={label}
      />
      {opts.hint && <span className="hint">{opts.hint}</span>}
    </label>
  );

  const check = (
    label: string,
    value: boolean,
    set: (v: boolean) => void,
    hint?: string,
  ) => (
    <label className="row prefs-check">
      <input
        type="checkbox"
        checked={value}
        onChange={(e) => set(e.target.checked)}
        aria-label={label}
      />
      <span>{label}</span>
      {hint && <span className="hint">{hint}</span>}
    </label>
  );

  /** The accent the theme in force has of its own, read off the
   * stylesheet with the person's taken away for a moment — what the
   * colour picker starts from when nothing is chosen. */
  const themeAccent = useMemo(() => {
    const root = document.documentElement;
    const held = root.style.getPropertyValue("--accent");
    root.style.removeProperty("--accent");
    const own = getComputedStyle(root).getPropertyValue("--accent").trim();
    if (held) root.style.setProperty("--accent", held);
    return /^#[0-9a-f]{6}$/i.test(own) ? own : "#6c8cff";
    // Read again when the theme changes under it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [prefs.theme]);

  /** A row of colours to choose from, the first being none of them:
   * the theme's own, or the accent's. */
  const swatchRow = (value: string, set: (c: string) => void, name: string, none: string) => (
    <>
      <button
        type="button"
        className={value === "" ? "prefs-swatch none active" : "prefs-swatch none"}
        aria-pressed={value === ""}
        aria-label={`${name}: ${none}`}
        title={none}
        onClick={() => set("")}
      />
      {ACCENTS.map(([label, hex]) => (
        <button
          key={hex}
          type="button"
          className={value === hex ? "prefs-swatch active" : "prefs-swatch"}
          style={{ background: hex }}
          aria-pressed={value === hex}
          aria-label={`${name}: ${label}`}
          title={label}
          onClick={() => set(hex)}
        />
      ))}
    </>
  );

  /** One command or menu row and the button that takes a chord for it. */
  const chordRow = (key: string, label: string, chord: string, path?: string) => (
    <label className="row prefs-check prefs-tool" key={key}>
      <span>
        {label}
        {path && <span className="prefs-path"> · {path}</span>}
      </span>
      <button
        type="button"
        className={catching === key ? "prefs-key listening" : "prefs-key"}
        aria-label={`${label} chord`}
        title={
          catching === key
            ? "Press the keys for it; Escape leaves it as it is"
            : `The chord that runs ${label}: press to change it`
        }
        onClick={(e) => {
          e.preventDefault();
          setRefused(null);
          setCatching(catching === key ? null : key);
        }}
      >
        {catching === key ? "press…" : chord ? chordLabel(chord) : "none"}
      </button>
    </label>
  );

  return (
    <div className="modal-scrim" onPointerDown={onClose}>
      <div
        className="modal prefs-modal"
        role="dialog"
        aria-label="Preferences"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <h2>Preferences</h2>
        <div className="prefs-body">
          <nav className="prefs-rail" aria-label="Preference groups">
            {GROUPS.map((g) => (
              <button
                key={g.id}
                className={g.id === group ? "prefs-tab active" : "prefs-tab"}
                aria-pressed={g.id === group}
                onClick={() => setGroup(g.id)}
              >
                <Icon name={g.icon} />
                <span>{g.label}</span>
              </button>
            ))}
          </nav>

          <div className="prefs-pane">
            {group === "appearance" && (
              <>
                <label className="row">
                  Theme
                  <select
                    value={prefs.theme}
                    onChange={(e) => setPrefs({ theme: e.target.value as Theme })}
                    aria-label="Theme"
                  >
                    <option value="system">As the system</option>
                    <option value="dark">Dark</option>
                    <option value="light">Light</option>
                  </select>
                  <span className="hint">the windows and panels; the page is the page</span>
                </label>
                <div className="row prefs-colours" role="group" aria-label="Accent">
                  <span className="prefs-colours-name">Accent</span>
                  <div className="prefs-swatches">
                    {swatchRow(prefs.accent, (c) => setPrefs({ accent: c }), "Accent", "The theme's own")}
                    <input
                      type="color"
                      value={prefs.accent || themeAccent}
                      onChange={(e) => setPrefs({ accent: e.target.value })}
                      aria-label="Accent colour"
                      title="Any colour"
                    />
                  </div>
                </div>
                <p className="modal-aside">
                  The active tool, the picked layer, the buttons that say
                  yes. What is drawn over the picture keeps its own white
                  and black.
                </p>
                <div className="row prefs-colours" role="group" aria-label="Sliders and ticks">
                  <span className="prefs-colours-name">Sliders and ticks</span>
                  <div className="prefs-swatches">
                    {swatchRow(
                      prefs.sliderColor,
                      (c) => setPrefs({ sliderColor: c }),
                      "Slider colour",
                      "The same as the accent",
                    )}
                    <input
                      type="color"
                      value={prefs.sliderColor || prefs.accent || themeAccent}
                      onChange={(e) => setPrefs({ sliderColor: e.target.value })}
                      aria-label="Slider colour"
                      title="Any colour"
                    />
                  </div>
                </div>
                <label className="row">
                  Interface size
                  <select
                    value={prefs.uiScale}
                    onChange={(e) => setPrefs({ uiScale: Number(e.target.value) })}
                    aria-label="Interface size"
                  >
                    {UI_SCALES.map((s) => (
                      <option key={s} value={s}>
                        {SCALE_NAMES[s] ?? `${Math.round(s * 100)}%`}
                      </option>
                    ))}
                  </select>
                  <span className="hint">the menus, panels and windows; not the page</span>
                </label>
                {/* What the choices look like together, live. */}
                <div className="prefs-preview" aria-label="Preview" role="group">
                  <button type="button" className="mask-button primary" tabIndex={-1}>
                    Done
                  </button>
                  <span className="prefs-preview-row">Picked layer</span>
                  <input type="range" min={0} max={100} defaultValue={60} tabIndex={-1} aria-hidden />
                  <input type="checkbox" defaultChecked tabIndex={-1} aria-hidden />
                </div>
              </>
            )}

            {group === "general" && (
              <>
                <label className="row">
                  Units
                  <select
                    value={prefs.units}
                    onChange={(e) =>
                      setPrefs({ units: e.target.value as Units })
                    }
                    aria-label="Units"
                  >
                    {(Object.keys(UNIT_NAMES) as Units[]).map((u) => (
                      <option key={u} value={u}>
                        {UNIT_NAMES[u]}
                      </option>
                    ))}
                  </select>
                  <span className="hint">rulers and geometry fields</span>
                </label>
                <label className="row">
                  One finger
                  <select
                    value={prefs.finger}
                    onChange={(e) => setPrefs({ finger: e.target.value as FingerDoes })}
                    aria-label="One finger"
                  >
                    <option value="auto">Draws, until a pen is used</option>
                    <option value="draw">Draws</option>
                    <option value="pan">Moves the page</option>
                  </select>
                  <span className="hint">on a touch screen; two fingers always move it</span>
                </label>
                {num("Arrow key moves", prefs.nudge, (v) => setPrefs({ nudge: v }), {
                  min: 0.1,
                  max: 100,
                  step: 0.5,
                  hint: "px",
                })}
                {num(
                  "With shift held",
                  prefs.nudgeBig,
                  (v) => setPrefs({ nudgeBig: v }),
                  { min: 0.1, max: 1000, step: 1, hint: "px" },
                )}
                {check(
                  "Remember the open document",
                  prefs.keepDraft,
                  (v) => setPrefs({ keepDraft: v }),
                  "offered back next visit",
                )}
                {/* Only the desktop app has a graphics card of its own to
                    draw with: the browser's engine draws on the processor. */}
                {isTauri() &&
                  check(
                    "Draw the page with the graphics card",
                    prefs.gpuFrames,
                    (v) => setPrefs({ gpuFrames: v }),
                    "falls back to the processor wherever the card cannot draw a frame exactly",
                  )}
              </>
            )}

            {group === "keys" && (
              <>
                <p className="modal-aside">
                  What each command answers to. Press the chord shown
                  beside one and then the keys you want; a chord another
                  command holds changes hands. Backspace puts one back to
                  what it shipped with and Delete leaves it with none.
                  The tools' own keys are on the Tools page.
                </p>
                {refused && <p className="modal-note">{refused}</p>}
                {COMMAND_GROUPS.map((section) => (
                  <div className="prefs-tools prefs-chords" key={section} role="group">
                    <span className="prefs-chord-group">{section}</span>
                    {COMMANDS.filter((c) => c.group === section).map((c) =>
                      chordRow(`cmd:${c.id}`, c.label, chords.of[c.id]),
                    )}
                  </div>
                ))}
                {menuRows.length > 0 && (
                  <div
                    className="prefs-tools prefs-chords"
                    role="group"
                    aria-label="Everything else in the menus"
                  >
                    <span className="prefs-chord-group">Everything else in the menus</span>
                    <input
                      type="search"
                      className="prefs-find"
                      placeholder="Find a command"
                      aria-label="Find a command"
                      value={rowQuery}
                      onChange={(e) => setRowQuery(e.target.value)}
                      onKeyDown={(e) => e.stopPropagation()}
                    />
                    {menuRows
                      .filter((r) => {
                        const q = rowQuery.trim().toLowerCase();
                        return !q || `${r.path} ${r.label}`.toLowerCase().includes(q);
                      })
                      .map((r) =>
                        chordRow(`row:${r.id}`, r.label, chords.menuOf[r.id] ?? "", r.path),
                      )}
                  </div>
                )}
              </>
            )}

            {group === "tools" && (
              <>
                <p className="modal-aside">
                  Which tools are on the rail, and the key each answers
                  to. One put away is not gone: its key still picks it,
                  and it waits behind the slot at the end of the rail
                  with the others put away. Press a key to change it;
                  a key another tool holds changes hands, and Backspace
                  puts a key back to what it shipped as.
                </p>
                {RAIL_ROWS.map((section, i) => (
                  <div className="prefs-tools" key={i} role="group">
                    {section.map((t) => {
                      const hidden = prefs.hiddenTools.includes(t);
                      const fixed = t === ALWAYS_SHOWN;
                      return (
                        <label
                          className="row prefs-check prefs-tool"
                          key={t}
                          title={fixed ? "The one tool the rail cannot do without" : TOOL_ABOUT[t]}
                        >
                          <input
                            type="checkbox"
                            checked={!hidden}
                            disabled={fixed}
                            onChange={(e) =>
                              setPrefs({
                                hiddenTools: e.target.checked
                                  ? prefs.hiddenTools.filter((h) => h !== t)
                                  : [...prefs.hiddenTools, t],
                              })
                            }
                            aria-label={`${t} on the rail`}
                          />
                          <Icon name={TOOL_ICONS[t]} size={16} />
                          <span>{t}</span>
                          {KEYED_TOOLS.has(t) ? (
                            <button
                              type="button"
                              className={
                                listening === t ? "prefs-key listening" : "prefs-key"
                              }
                              aria-label={`${t} key`}
                              title={
                                listening === t
                                  ? "Press the key for it; Escape leaves it as it is"
                                  : `The key that picks ${t}: press to change it${
                                      t in prefs.toolKeys ? " (rebound)" : ""
                                    }`
                              }
                              onClick={(e) => {
                                e.preventDefault();
                                setListening(listening === t ? null : t);
                              }}
                            >
                              {listening === t ? "press…" : keys.hint[t] || "none"}
                            </button>
                          ) : (
                            <span className="hint" title="A shift away from its family's key">
                              {keys.hint[t] ? `shift+${keys.hint[t]}` : "—"}
                            </span>
                          )}
                        </label>
                      );
                    })}
                  </div>
                ))}
                <p className="modal-aside">
                  And which groups of buttons the bar along the top shows.
                  Everything on them is on a menu as well.
                </p>
                {check("Document actions on the bar", prefs.barDocument, (v) =>
                  setPrefs({ barDocument: v }),
                  "new, open, place, save, export",
                )}
                {check("Region actions on the bar", prefs.barSelection, (v) =>
                  setPrefs({ barSelection: v }),
                  "pick out, invert, subject, feather",
                )}
                {check("Zoom on the bar", prefs.barZoom, (v) =>
                  setPrefs({ barZoom: v }),
                )}
              </>
            )}

            {group === "guides" && (
              <>
                {check("Show guides", prefs.showGuides, (v) =>
                  setPrefs({ showGuides: v }),
                )}
                <label className="row">
                  Grid
                  <select
                    value={prefs.grid}
                    onChange={(e) => setPrefs({ grid: Number(e.target.value) })}
                    aria-label="Grid"
                  >
                    <option value={0}>No grid</option>
                    {[8, 16, 32, 64].map((s) => (
                      <option key={s} value={s}>
                        Every {s} px
                      </option>
                    ))}
                  </select>
                </label>
                {num("Catches within", prefs.snap, (v) => setPrefs({ snap: v }), {
                  min: 0,
                  max: 64,
                  hint: "screen px · 0 to never catch",
                })}
              </>
            )}

            {group === "selection" && (
              <>
                {num("Soften the edge by", prefs.feather, (v) =>
                  setPrefs({ feather: v }),
                  { min: 0, max: 500, hint: "px · what a new region starts at" },
                )}
                <label className="row">
                  Subject pick lets go at
                  <input
                    type="range"
                    min={0}
                    max={100}
                    value={Math.round(prefs.subjectTolerance * 100)}
                    onChange={(e) =>
                      setPrefs({ subjectTolerance: Number(e.target.value) / 100 })
                    }
                    aria-label="Subject pick lets go at"
                  />
                  <span className="hint">
                    {Math.round(prefs.subjectTolerance * 100)}
                  </span>
                </label>
                <p className="modal-aside">
                  Higher takes in more of what surrounds the subject. Only the
                  app's own colour-based pick reads this — where the system has
                  a model of its own, that is asked first and answers on its
                  own terms.
                </p>
              </>
            )}

            {group === "colour" && (
              <>
                <p className="modal-aside">
                  How this screen shows colour. Nothing here changes the
                  document — the profile it will be printed through is the
                  file's own and is loaded from the File menu.
                </p>
                <div className="row">
                  <span>Monitor profile</span>
                  <button className="mask-button" onClick={onLoadScreenIcc}>
                    {hasScreenIcc ? "Replace…" : "Load…"}
                  </button>
                  <button className="mask-button" onClick={onDisplayP3}>
                    Display P3
                  </button>
                  {hasScreenIcc && (
                    <button className="mask-button" onClick={onClearScreenIcc}>
                      Show sRGB as it is
                    </button>
                  )}
                </div>
              </>
            )}

            {group === "new" && (
              <>
                {num("Width", prefs.newWidth, (v) => setPrefs({ newWidth: v }), {
                  min: 1,
                  max: 8192,
                  hint: "px",
                })}
                {num(
                  "Height",
                  prefs.newHeight,
                  (v) => setPrefs({ newHeight: v }),
                  { min: 1, max: 8192, hint: "px" },
                )}
                {num("Resolution", prefs.newDpi, (v) => setPrefs({ newDpi: v }), {
                  min: 1,
                  max: 2400,
                  hint: "dpi",
                })}
                <p className="modal-aside">
                  What the new-document window opens on. It can still be
                  changed there.
                </p>
              </>
            )}

            {group === "export" && (
              <>
                <label className="row">
                  Format
                  <select
                    value={prefs.exportFormat}
                    onChange={(e) =>
                      setPrefs({ exportFormat: e.target.value as ExportFormat })
                    }
                    aria-label="Format"
                  >
                    {(Object.keys(FORMAT_NAMES) as ExportFormat[]).map((f) => (
                      <option key={f} value={f}>
                        {FORMAT_NAMES[f]}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="row">
                  JPEG quality
                  <input
                    type="range"
                    min={1}
                    max={100}
                    value={prefs.jpegQuality}
                    onChange={(e) =>
                      setPrefs({ jpegQuality: Number(e.target.value) })
                    }
                    aria-label="JPEG quality"
                  />
                  <span className="hint">{prefs.jpegQuality}</span>
                </label>
                <p className="modal-aside">
                  What the export window opens on. The size of the file is
                  shown there before anything is written.
                </p>
              </>
            )}
          </div>
        </div>

        <div className="modal-actions">
          <button
            className="mask-button"
            onClick={resetPrefs}
            disabled={
              JSON.stringify(prefs) === JSON.stringify(DEFAULTS)
            }
          >
            Put everything back
          </button>
          <button className="mask-button primary" onClick={onClose}>
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
