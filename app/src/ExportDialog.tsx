/** Getting the picture out, in one window.
 *
 * This replaced thirteen rows on the File menu — PNG, PNG at 2×, PNG at
 * 3×, the same three again for what is picked, this artboard, every
 * artboard, JPEG, SVG, PDF, PDF of the frames, TIFF — which is a menu
 * doing a dialog's job. Every one of them was a guess made in advance
 * and then frozen into a row: 92 was the JPEG quality because somebody
 * typed 92 once, and there was no way to ask for 80.
 *
 * What the window adds over those rows is the thing Affinity and
 * Photoshop both put at the bottom of theirs: the size of the file
 * before you commit to it. Theirs is an estimate. This one is not — the
 * bytes are encoded in-process, so the number shown is the number of
 * bytes that will land on the disk, which is what makes a quality
 * slider worth dragging.
 *
 * Beside the settings is the picture, and it is the same bytes again:
 * the encode that gives the size is handed to the browser to decode, so
 * a JPEG at quality 5 shows its blocks before it is taken, which is the
 * one thing a number cannot tell you. A PDF and a TIFF the browser
 * cannot show, and for those the page is drawn as the engine draws it,
 * labelled as such — the picture, not the file.
 *
 * Three things decide an export and they are asked in the order that
 * matters: what form it takes, how much of the page goes, and how big.
 * Everything a format cannot do is disabled rather than hidden, so the
 * window does not change shape under the pointer — the reason a choice
 * is unavailable is more useful than its absence.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import type { WasmSession } from "./engine";
import type { ExportArea, ExportFormat, Prefs } from "./prefs";

/** What each format is called, what it writes, and what it can do.
 *
 * `scales` is whether the format is pixels at all: a PDF and an SVG
 * carry shapes, and asking one for "2×" is asking a question the file
 * has no place to record. `area` is whether anything but the whole page
 * can leave that way, which is the same distinction seen from the other
 * side. */
const FORMATS: Record<
  ExportFormat,
  {
    label: string;
    ext: string;
    mime: string;
    /** Whether a scale multiplier means anything. */
    scales: boolean;
    /** Whether it can carry less than the whole page. */
    area: boolean;
    /** Whether a browser can show the file itself, so the preview can
     * be the very bytes that will be written. */
    shows: boolean;
    note: string;
  }
> = {
  png: {
    label: "PNG",
    ext: "png",
    mime: "image/png",
    scales: true,
    area: true,
    shows: true,
    note: "Lossless, keeps transparency.",
  },
  jpeg: {
    label: "JPEG",
    ext: "jpg",
    mime: "image/jpeg",
    scales: true,
    area: true,
    shows: true,
    note: "Lossy, and transparency flattens onto white.",
  },
  pdf: {
    label: "PDF",
    ext: "pdf",
    mime: "application/pdf",
    scales: false,
    area: false,
    shows: false,
    note: "Vectors stay vectors.",
  },
  svg: {
    label: "SVG",
    ext: "svg",
    mime: "image/svg+xml",
    scales: false,
    area: false,
    shows: true,
    note: "Markup. Adjustment layers cannot go this way.",
  },
  tiff: {
    label: "TIFF",
    ext: "tif",
    mime: "image/tiff",
    scales: false,
    area: false,
    shows: false,
    note: "CMYK, separated through the press profile.",
  },
};

const SCALES = [0.5, 1, 2, 3];
/** The set an asset pipeline wants: one press, three files, named
 * `@1x`, `@2x` and `@3x`. Kept in the preference as a scale of zero,
 * which is not a scale, and never reaches the engine as one. */
const SET = [1, 2, 3];

/** A count of bytes as a person reads it. */
function inBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} kB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Above this many pixels the size is not worked out until it is asked
 * for. Encoding is synchronous and on the same thread as the window, so
 * a nine-megapixel page re-encoded on every drag of the quality slider
 * would freeze the very control being dragged.
 * ponytail: a fixed ceiling, not a measurement — move the encode to a
 * worker if this ever needs to be live at print sizes. */
const AUTO_LIMIT = 4_000_000;

export function ExportDialog({
  session,
  prefs,
  setPrefs,
  fileName,
  hasRegion,
  selectionBounds,
  hasIcc,
  onClose,
}: {
  session: WasmSession;
  prefs: Prefs;
  setPrefs: (p: Partial<Prefs>) => void;
  fileName: () => string;
  /** Whether a *region* is picked out of the page — ants on the canvas.
   * Not the same question as whether layers are picked, and the two
   * leave by different doors: a region goes out in the shape it was
   * picked in, layers go out as the box that holds them. */
  hasRegion: boolean;
  /** The picked layers' box, when there is no region but there are layers. */
  selectionBounds: () => [number, number, number, number] | null;
  /** Whether a press profile is loaded — the only way a TIFF can go. */
  hasIcc: boolean;
  onClose: () => void;
}) {
  const format = prefs.exportFormat;
  const spec = FORMATS[format];
  const area: ExportArea = spec.area ? prefs.exportArea : "page";
  /** Whether the set is asked for rather than one size. */
  const set = spec.scales && prefs.exportScale === 0;
  /** The one scale everything shown is worked out at; the set's other
   * two are multiples of it. */
  const scale = spec.scales && !set ? prefs.exportScale : 1;
  /** Every scale that will be written. */
  const scales = set ? SET : [scale];

  /** The page's own size, which everything shown is worked out from. */
  const [pageW, pageH] = [session.width, session.height];

  /** What will actually be written, in pixels. A region that is picked
   * is exported in the shape it was picked in, so the box it sits in is
   * what decides the size. */
  const outSize = (): [number, number] => {
    if (area === "selection") {
      const box = selectionBounds();
      if (box) {
        return [
          Math.max(1, Math.round((box[2] - box[0]) * scale)),
          Math.max(1, Math.round((box[3] - box[1]) * scale)),
        ];
      }
    }
    return [
      Math.max(1, Math.round(pageW * scale)),
      Math.max(1, Math.round(pageH * scale)),
    ];
  };
  const [outW, outH] = outSize();

  /** Whether this combination can be exported at all, and why not. */
  const refuses = (): string | null => {
    if (format === "tiff" && !hasIcc) {
      return "A press profile has to be loaded before a CMYK TIFF can be written.";
    }
    if (area === "selection" && !hasRegion && !selectionBounds()) {
      return "Nothing is picked.";
    }
    if (outW > 16384 || outH > 16384) {
      return `That comes to ${outW} × ${outH}, which is past what can be held at once.`;
    }
    return null;
  };
  const refusal = refuses();

  /** The bytes, made once and used for both the size shown and the file
   * written — so what the window says and what lands are the same
   * encode, not two that might differ. */
  const encodeAt = useCallback((scale: number): Uint8Array => {
    switch (format) {
      case "pdf":
        return session.export_pdf();
      case "svg":
        return new TextEncoder().encode(session.export_svg());
      case "tiff":
        return session.export_cmyk_tiff();
      case "jpeg": {
        // A JPEG has no alpha, so a region leaves as the box that holds
        // it, flattened onto white — the shape it was picked in has
        // nowhere to be recorded.
        const box = area === "selection" ? selectionBounds() : null;
        return box
          ? session.export_jpeg_at(
              scale,
              box[0],
              box[1],
              box[2] - box[0],
              box[3] - box[1],
              prefs.jpegQuality,
            )
          : session.export_jpeg_at(scale, 0, 0, 0, 0, prefs.jpegQuality);
      }
      default: {
        if (area === "selection") {
          // A region goes out in the shape it was picked in; picked
          // layers go out as the box that holds them.
          if (hasRegion) return session.selection_png(scale);
          const box = selectionBounds();
          if (box) {
            return session.export_png_at(
              scale,
              box[0],
              box[1],
              box[2] - box[0],
              box[3] - box[1],
            );
          }
        }
        return session.export_png_at(scale, 0, 0, 0, 0);
      }
    }
  }, [
    format,
    area,
    hasRegion,
    prefs.jpegQuality,
    session,
    selectionBounds,
  ]);
  /** The bytes at the one scale shown — the set's first. */
  const encode = useCallback(() => encodeAt(scale), [encodeAt, scale]);

  /** How big the file comes out. `null` while it is being worked out,
   * `-1` when it is too big to work out without being asked. */
  const [size, setSize] = useState<number | null>(null);
  const [failed, setFailed] = useState<string | null>(null);
  /** The picture beside the settings: a URL the browser can show, and
   * whether it is the file itself or the page standing in for one the
   * browser cannot decode. */
  const [preview, setPreview] = useState<{ url: string; standIn: boolean } | null>(null);
  // A blob URL holds its bytes until it is let go of, so the last one is
  // revoked whenever a new one takes its place, and when the window goes.
  useEffect(() => () => {
    if (preview) URL.revokeObjectURL(preview.url);
  }, [preview]);
  /** Bumped to ask for a size that was too big to take automatically. */
  const [measure, setMeasure] = useState(0);

  useEffect(() => {
    if (refusal) {
      setSize(null);
      setFailed(null);
      setPreview(null);
      return;
    }
    // The set is one, four and nine times the page — `outW` by `outH`
    // is its first — and the ceiling counts all of it.
    const big = outW * outH * (set ? SET.reduce((n, s) => n + s * s, 0) : 1) > AUTO_LIMIT;
    if (big && measure === 0) {
      setSize(-1);
      setFailed(null);
      setPreview(null);
      return;
    }
    setSize(null);
    // A breath after the last change, so dragging a slider does not
    // encode the page once a frame.
    const timer = setTimeout(() => {
      try {
        const bytes = encode();
        setSize(
          set
            ? SET.slice(1).reduce((n, s) => n + encodeAt(s).length, bytes.length)
            : bytes.length,
        );
        setFailed(null);
        // The browser shows a PNG, a JPEG or an SVG as it is; a PDF or
        // a TIFF it cannot, so the page stands in, drawn as the engine
        // draws it.
        const shown = spec.shows
          ? { bytes, mime: spec.mime, standIn: false }
          : {
              bytes: session.export_png_at(1, 0, 0, 0, 0),
              mime: "image/png",
              standIn: true,
            };
        setPreview({
          url: URL.createObjectURL(
            new Blob([shown.bytes as BlobPart], { type: shown.mime }),
          ),
          standIn: shown.standIn,
        });
      } catch (err) {
        setSize(null);
        setPreview(null);
        setFailed(String(err));
      }
    }, 350);
    return () => clearTimeout(timer);
  }, [encode, encodeAt, set, refusal, outW, outH, measure, spec.shows, spec.mime, session]);

  // Asking for a size is about the export as it now stands, so changing
  // any of it puts the question back.
  useEffect(() => setMeasure(0), [format, area, scale, prefs.jpegQuality]);

  const nameAt = (s: number) =>
    `${fileName()}${area === "selection" ? "-selection" : ""}${
      s !== 1 || set ? `@${s}x` : ""
    }.${spec.ext}`;
  const name = set ? `${nameAt(1)}, @2x, @3x` : nameAt(scale);

  const run = useCallback(() => {
    if (refusal) return;
    try {
      for (const s of scales) {
        const bytes = encodeAt(s);
        const url = URL.createObjectURL(
          new Blob([bytes as BlobPart], { type: spec.mime }),
        );
        const a = document.createElement("a");
        a.href = url;
        a.download = nameAt(s);
        a.click();
        URL.revokeObjectURL(url);
      }
      onClose();
    } catch (err) {
      setFailed(String(err));
    }
    // `scales` and `nameAt` are made afresh each render from what the
    // dependencies below say.
  }, [encodeAt, set, scale, area, format, fileName, spec.mime, spec.ext, refusal, onClose]);

  const runRef = useRef(run);
  runRef.current = run;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (e.key === "Enter") runRef.current();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [onClose]);

  return (
    <div className="modal-scrim" onPointerDown={onClose}>
      <div
        className="modal export-modal"
        role="dialog"
        aria-label="Export"
        onPointerDown={(e) => e.stopPropagation()}
      >
        <h2>Export</h2>

        <div className="export-body">
        <figure className="export-preview" aria-label="Preview">
          {preview ? (
            <img
              src={preview.url}
              alt={preview.standIn ? "The page as it draws" : "The file as it will be written"}
            />
          ) : (
            <span className="hint">
              {refusal ? "" : size === -1 ? "Too big to show unasked" : "…"}
            </span>
          )}
          <figcaption className="hint">
            {preview
              ? preview.standIn
                ? `As the page draws — a ${spec.label} carries what the browser cannot show`
                : "The file itself, decoded"
              : "\u00a0"}
          </figcaption>
        </figure>
        <div className="export-settings">

        <div className="export-formats" role="group" aria-label="Format">
          {(Object.keys(FORMATS) as ExportFormat[]).map((f) => (
            <button
              key={f}
              className={f === format ? "preset active" : "preset"}
              aria-pressed={f === format}
              onClick={() => setPrefs({ exportFormat: f })}
            >
              {FORMATS[f].label}
            </button>
          ))}
        </div>
        <p className="modal-aside">{spec.note}</p>

        <label className="row">
          Area
          <select
            value={area}
            disabled={!spec.area}
            onChange={(e) =>
              setPrefs({ exportArea: e.target.value as ExportArea })
            }
            aria-label="Area"
          >
            <option value="page">Whole page</option>
            <option value="selection">What is picked</option>
          </select>
          {!spec.area ? (
            <span className="hint">{spec.label} carries the whole page</span>
          ) : format === "jpeg" && area === "selection" ? (
            <span className="hint">as the box that holds it</span>
          ) : null}
        </label>

        <div className="row" role="group" aria-label="Scale">
          <span>Size</span>
          <div className="export-scales">
            {SCALES.map((s) => (
              <button
                key={s}
                className={!set && s === scale ? "preset active" : "preset"}
                disabled={!spec.scales}
                aria-pressed={!set && s === scale}
                onClick={() => setPrefs({ exportScale: s })}
              >
                {s === 1 ? "1×" : `${s}×`}
              </button>
            ))}
            <button
              className={set ? "preset active" : "preset"}
              disabled={!spec.scales}
              aria-pressed={set}
              title="1×, 2× and 3×: three files in one press, named @1x, @2x and @3x"
              onClick={() => setPrefs({ exportScale: 0 })}
            >
              Set
            </button>
          </div>
          <span className="hint">
            {!spec.scales
              ? "as drawn"
              : set
                ? `${outW} × ${outH} px, ×2, ×3`
                : `${outW} × ${outH} px`}
          </span>
        </div>

        {format === "jpeg" && (
          <label className="row">
            Quality
            <input
              type="range"
              min={1}
              max={100}
              value={prefs.jpegQuality}
              onChange={(e) =>
                setPrefs({ jpegQuality: Number(e.target.value) })
              }
              aria-label="Quality"
            />
            <span className="hint">{prefs.jpegQuality}</span>
          </label>
        )}

        </div>
        </div>

        <p className="export-size" role="status">
          {refusal ? (
            <span className="export-refusal">{refusal}</span>
          ) : failed ? (
            <span className="export-refusal">Could not be made: {failed}</span>
          ) : size === -1 ? (
            <>
              <span>{name}</span>
              <button className="mask-button" onClick={() => setMeasure(1)}>
                Work out the size
              </button>
            </>
          ) : (
            <>
              <span>{name}</span>
              <strong>{size === null ? "…" : inBytes(size)}</strong>
            </>
          )}
        </p>

        <div className="modal-actions">
          <button className="mask-button" onClick={onClose}>
            Cancel
          </button>
          <button
            className="mask-button primary"
            disabled={!!refusal}
            onClick={run}
          >
            Export
          </button>
        </div>
      </div>
    </div>
  );
}
