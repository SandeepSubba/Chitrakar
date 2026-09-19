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

import { useEffect, useRef, useState } from "react";
import { Icon, type IconName } from "./icons";
import { DEFAULTS, type ExportFormat, type Prefs, type Units } from "./prefs";

type Group = "general" | "guides" | "selection" | "colour" | "new" | "export";

const GROUPS: { id: Group; label: string; icon: IconName }[] = [
  { id: "general", label: "General", icon: "units" },
  { id: "guides", label: "Guides & grid", icon: "fit" },
  { id: "selection", label: "Selection", icon: "marquee" },
  { id: "colour", label: "Colour", icon: "proof" },
  { id: "new", label: "New documents", icon: "newDoc" },
  { id: "export", label: "Export", icon: "export" },
];

const UNIT_NAMES: Record<Units, string> = {
  px: "Pixels",
  mm: "Millimetres",
  in: "Inches",
};

const FORMAT_NAMES: Record<ExportFormat, string> = {
  png: "PNG",
  jpeg: "JPEG",
  pdf: "PDF",
  svg: "SVG",
  tiff: "TIFF (CMYK)",
};

export function PreferencesDialog({
  prefs,
  setPrefs,
  resetPrefs,
  hasScreenIcc,
  onLoadScreenIcc,
  onDisplayP3,
  onClearScreenIcc,
  onClose,
}: {
  prefs: Prefs;
  setPrefs: (p: Partial<Prefs>) => void;
  resetPrefs: () => void;
  hasScreenIcc: boolean;
  onLoadScreenIcc: () => void;
  onDisplayP3: () => void;
  onClearScreenIcc: () => void;
  onClose: () => void;
}) {
  const [group, setGroup] = useState<Group>("general");
  const closeRef = useRef(onClose);
  closeRef.current = onClose;

  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") closeRef.current();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, []);

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
