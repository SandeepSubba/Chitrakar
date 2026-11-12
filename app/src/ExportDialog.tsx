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

import { chipKeys } from "./chipKeys";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { WasmSession } from "./engine";
import { watermarkJson, type ExportArea, type ExportFormat, type ExportSetup, type Prefs } from "./prefs";
import { WatermarkRow } from "./WatermarkRow";
import { NumberField } from "./NumberField";
import { said } from "./imports";
import { enterMeansGo } from "./dialogKeys";

/** As far as the engine scales a page going out (`render_scaled`). */
const MOST_SCALE = 16;
import { fileSafe, type Outgoing } from "./files";

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
  webp: {
    label: "WebP",
    ext: "webp",
    mime: "image/webp",
    scales: true,
    area: true,
    shows: true,
    note: "Lossless and keeps transparency, like a PNG — usually smaller.",
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
  psd: {
    label: "PSD",
    ext: "psd",
    mime: "image/vnd.adobe.photoshop",
    scales: false,
    area: false,
    shows: false,
    note: "Layers for Photoshop: groups, names, opacity, blend modes, hidden layers and clipping, as Photoshop's own.",
  },
};

const SCALES = [0.5, 1, 2, 3];
/** How much a picture going out is sharpened for a screen, by name. */
const SHARPEN_FOR_SCREEN: [string, number][] = [
  ["Off", 0],
  ["Low", 0.3],
  ["Standard", 0.6],
  ["High", 1],
];
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
  keptRegions,
  keptComps,
  write,
  share,
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
  /** The regions kept by name, in the order the engine holds them: a
   * slice each, when the area asked for is all of them. */
  keptRegions: string[];
  /** The layer comps, in the order the engine holds them: the page once
   * for each, when the area asked for is all of them. */
  keptComps: string[];
  /** Write the files out, the way the platform writes them; false is a
   * panel cancelled, a failure throws. */
  write: (files: Outgoing[]) => Promise<boolean>;
  /** Hand the files to the system's share sheet instead, where the
   * device has one; absent where it does not. */
  share?: (files: Outgoing[]) => Promise<boolean>;
  onClose: () => void;
}) {
  const format = prefs.exportFormat;
  const spec = FORMATS[format];
  const asked: ExportArea = spec.area ? prefs.exportArea : "page";
  // Regions only where there are some, and only in a format that can
  // carry less than the whole page: a setup kept when there were
  // regions must not ask a PDF for one now.
  const area: ExportArea =
    (asked === "regions" && keptRegions.length === 0) ||
    (asked === "comps" && keptComps.length === 0)
      ? "page"
      : asked;
  /** The names a file each is written for, when the area is one of the
   * two that write several: the regions, or the comps. */
  const many: string[] = area === "regions" ? keptRegions : area === "comps" ? keptComps : [];
  /** A kept region's box, worked out by encoding nothing: the engine
   * knows where it is, and the window only needs its size. Nothing for
   * a region that is not on the page. */
  const regionBox = (at: number): [number, number, number, number] | null => {
    // The box the engine exports the region in, whichever the format.
    const b = session.region_export_box(at);
    return b.length === 4 ? [0, 0, b[2], b[3]] : null;
  };
  /** Which of `many` are written: every comp, and every region that is
   * on the page. One off it used to stop the whole write at the first
   * file, so a region that was there was never written either. */
  const ats: number[] = many.map((_, i) => i).filter((i) => area !== "regions" || regionBox(i) !== null);
  /** How many kept regions are on the page, whichever area is chosen. */
  const onPageRegions = keptRegions.filter((_, i) => regionBox(i) !== null).length;
  /** The regions left out for being off the page, said by name. */
  const offPage: string[] = area === "regions" ? many.filter((_, i) => !ats.includes(i)) : [];
  /** Every slice that will be written, with where it is among `many`,
   * or one file with no name of its own. `null` is the whole page or
   * what is picked out. */
  const slices: [string | null, number | null][] = many.length > 0 ? ats.map((i) => [many[i], i]) : [[null, null]];
  /** The first slice, which stands for the rest in what is shown. */
  const first: number | null = many.length > 0 ? (ats[0] ?? null) : null;
  /** Whether the set is asked for rather than one size. */
  const set = spec.scales && prefs.exportScale === 0;
  /** A long edge asked for in pixels, which stands in for the scale. */
  const longEdge = spec.scales && !set ? prefs.exportLongEdge : 0;
  /** The page's own size, which everything shown is worked out from. */
  const [pageW, pageH] = [session.width, session.height];

  /** What is exported at its own size: the page, or the box a picked
   * region sits in, since a region is exported in the shape it was
   * picked in. */
  const baseSize = (): [number, number] => {
    if (area === "regions") {
      // The first slice stands for the set here, as the first of the
      // `@1x` set does: they are different sizes and the window has one
      // line to say it in.
      const box = first === null ? null : regionBox(first);
      if (box) return [box[2] - box[0], box[3] - box[1]];
    }
    if (area === "selection") {
      const box = selectionBounds();
      if (box) return [box[2] - box[0], box[3] - box[1]];
    }
    return [pageW, pageH];
  };
  const [baseW, baseH] = baseSize();
  /** The one scale everything shown is worked out at; the set's other
   * two are multiples of it. A long edge is a scale worked out from
   * the size, so 2048 is 2048 whatever the photograph was. */
  const scale =
    spec.scales && !set
      ? longEdge > 0
        ? Math.min(MOST_SCALE, longEdge / Math.max(1, baseW, baseH))
        : prefs.exportScale
      : 1;
  /** A long edge further than the engine will scale to: what goes out is
   * as long as it can be, and the window says so rather than promising
   * 4000 and writing 3200. */
  const capped = longEdge > 0 && !set && longEdge / Math.max(1, baseW, baseH) > MOST_SCALE;
  /** The scale a slice is written at: a long edge is each kept
   * region's own, since they are different sizes; anything else is the
   * one scale. */
  const scaleFor = (at: number | null): number => {
    if (longEdge > 0 && !set && area === "regions" && at !== null) {
      const box = regionBox(at);
      if (box) return Math.min(MOST_SCALE, longEdge / Math.max(1, box[2] - box[0], box[3] - box[1]));
    }
    return scale;
  };
  /** The long edge a slice comes out at, which its file is named for. */
  const edgeOf = (at: number | null): number => {
    if (at !== null && area === "regions") {
      const box = regionBox(at);
      if (box) return Math.max(1, Math.round(Math.max(box[2] - box[0], box[3] - box[1]) * scaleFor(at)));
    }
    return Math.max(outW, outH);
  };
  /** Every scale that will be written. */
  const scales = set ? SET : [scale];
  /** What will actually be written, in pixels. */
  const [outW, outH] = [
    Math.max(1, Math.round(baseW * scale)),
    Math.max(1, Math.round(baseH * scale)),
  ];

  /** Every file's size, for the regions each its own: judged by the
   * first region alone, a page-sized second region at 3× was refused only
   * once it was pressed, and at 2× encoded 48 megapixels unasked. */
  const slicesOut: [number, number][] =
    area === "regions" && ats.length > 0
      ? ats.map((i) => {
          const b = regionBox(i);
          const k = scaleFor(i);
          return b
            ? [Math.max(1, Math.round((b[2] - b[0]) * k)), Math.max(1, Math.round((b[3] - b[1]) * k))]
            : [outW, outH];
        })
      : [[outW, outH]];
  const [largestW, largestH] = slicesOut.reduce<[number, number]>(
    ([a, b], [c, d]) => [Math.max(a, c), Math.max(b, d)],
    [1, 1],
  );
  const allPixels = slicesOut.reduce((n, [w, h]) => n + w * h, 0);

  /** Whether this combination can be exported at all, and why not. */
  const refuses = (): string | null => {
    if (many.length > 0 && ats.length === 0) {
      return many.length === 1
        ? `The region “${many[0]}” is not on the page.`
        : "None of the kept regions is on the page.";
    }
    if (format === "tiff" && !hasIcc) {
      return "A press profile has to be loaded before a CMYK TIFF can be written.";
    }
    if (area === "selection" && !hasRegion && !selectionBounds()) {
      return "Nothing is picked.";
    }
    // The engine's own limits (`canvas_fits`: 30,000 a side, 100
    // megapixels), and WebP's, which holds no more than 16,383 a side:
    // asked for and refused here, before the export is pressed, rather
    // than after it with a size the window never showed. It said 16,384
    // for every format, so a 20,000-wide page could not be exported at
    // all from here, though Quick export wrote it.
    const most = set ? 3 : 1;
    const side = format === "webp" ? 16383 : 30000;
    if (largestW * most > side || largestH * most > side || largestW * most * largestH * most > 100_000_000) {
      return `That comes to ${largestW * most} × ${largestH * most}, which is past what can be held at once (${side.toLocaleString("en")} a side${
        format === "webp" ? " in a WebP" : ""
      }, 100 megapixels).`;
    }
    return null;
  };
  const refusal = refuses();

  /** The bytes, made once and used for both the size shown and the file
   * written — so what the window says and what lands are the same
   * encode, not two that might differ. */
  const encodePlain = useCallback(
    (scale: number, slice: number | null = null): Uint8Array => {
      if (slice !== null && area === "comps") {
        return format === "webp"
          ? session.comp_webp(slice, scale)
          : format === "jpeg"
            ? session.comp_jpeg(slice, scale, prefs.jpegQuality)
            : session.comp_png(slice, scale);
      }
      if (slice !== null) {
        // A JPEG of a region is the box that holds it, on white — the
        // shape has nowhere to go. (It used to be a PNG's bytes under a
        // name ending .jpg.)
        return format === "webp"
          ? session.kept_region_webp(slice, scale)
          : format === "jpeg"
            ? session.kept_region_jpeg(slice, scale, prefs.jpegQuality)
            : session.kept_region_png(slice, scale);
      }
      return encodeOne(scale);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [session, format, area, hasRegion, prefs.jpegQuality, selectionBounds],
  );
  /** A JPEG goes out saying how its photograph was taken — the camera,
   * the lens, the exposure, when; never where — unless asked not to. In
   * the encode, so the size shown is the size written. */
  /** Whether the format is pixels, which sharpening for the screen is for. */
  const pixels = format === "png" || format === "jpeg" || format === "webp";
  // Each file made once for these settings: the size beside them was
  // worked out by encoding, and Export then encoded everything again —
  // a PSD of two thousand layers froze the window twice over.
  const encodeAt = useMemo(() => {
    const made = new Map<string, Uint8Array>();
    return (scale: number, slice: number | null = null): Uint8Array => {
      const key = `${scale}|${slice}`;
      const kept = made.get(key);
      if (kept) return kept;
      // Sharpened for the screen around this encode alone, and put back
      // after, so no other export is sharpened by what this window asked.
      session.set_output_sharpen(pixels ? prefs.exportSharpen : 0);
      session.set_output_watermark(pixels ? watermarkJson(prefs.exportWatermark) : "");
      let bytes: Uint8Array;
      try {
        bytes = encodePlain(scale, slice);
      } finally {
        session.set_output_sharpen(0);
        session.set_output_watermark("");
      }
      const out = format === "jpeg" && prefs.exportCameraData ? session.jpeg_with_capture(bytes) : bytes;
      made.set(key, out);
      return out;
    };
  }, [encodePlain, format, pixels, prefs.exportCameraData, prefs.exportSharpen, prefs.exportWatermark, session]);
  /** How many layers a Photoshop file draws, each alone over the page. */
  const layerCount = useMemo(() => {
    try {
      return (JSON.parse(session.layers_json()) as unknown[]).length;
    } catch {
      return 1;
    }
  }, [session]);
  /** The photograph's camera and lens, for the window to say what a JPEG
   * will carry; empty where the page holds no photograph that said. */
  const cameraSummary = useMemo(() => session.capture_summary(), [session]);

  const encodeOne = useCallback((scale: number): Uint8Array => {
    switch (format) {
      case "pdf":
        // Asked for no bleed and no marks, this is the PDF it always was.
        return session.export_pdf_marked(false, prefs.pdfBleed, prefs.pdfCropMarks);
      case "svg":
        return new TextEncoder().encode(session.export_svg());
      case "tiff":
        return session.export_cmyk_tiff();
      case "psd": {
        const bytes = session.export_psd();
        setPsdNotes(JSON.parse(session.export_notes()) as string[]);
        return bytes;
      }
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
        // A PNG or a WebP: the same picture either way, and the same
        // transparency, so the one question is which file to make.
        const webp = format === "webp";
        const at = (x: number, y: number, w: number, h: number) =>
          webp
            ? session.export_webp_at(scale, x, y, w, h)
            : session.export_png_at(scale, x, y, w, h);
        if (area === "selection") {
          // A region goes out in the shape it was picked in; picked
          // layers go out as the box that holds them.
          if (hasRegion) {
            return webp ? session.selection_webp(scale) : session.selection_png(scale);
          }
          const box = selectionBounds();
          if (box) return at(box[0], box[1], box[2] - box[0], box[3] - box[1]);
        }
        return at(0, 0, 0, 0);
      }
    }
  }, [
    format,
    area,
    hasRegion,
    prefs.jpegQuality,
    prefs.pdfBleed,
    prefs.pdfCropMarks,
    session,
    selectionBounds,
  ]);
  /** The bytes at the one scale shown — the set's first, or the first
   * slice when a slice each is what is being written. */
  const encode = useCallback(
    () => encodeAt(scale, first),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [encodeAt, scale, area, first],
  );

  /** How big the file comes out. `null` while it is being worked out,
   * `-1` when it is too big to work out without being asked. */
  const [size, setSize] = useState<number | null>(null);
  /** What a Photoshop file carries across differently, once made. */
  const [psdNotes, setPsdNotes] = useState<string[]>([]);
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
    // A Photoshop file draws every layer alone over the whole page, so
    // it costs the page once a layer: two thousand small shapes froze
    // the window for a minute and a half before it said a size.
    const layers = format === "psd" ? Math.max(1, layerCount) : 1;
    const big = allPixels * layers * (set ? SET.reduce((n, s) => n + s * s, 0) : 1) > AUTO_LIMIT;
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
        // Every file that will be written: each scale of the set, and
        // within it each slice, at the scale that slice is written at.
        const at: (number | null)[] = slices.map(([, i]) => i);
        let total = 0;
        for (const s of set ? SET : [null]) {
          for (const i of at) {
            // The first of them is the one already made.
            const first = (set ? s === 1 : true) && i === at[0];
            total += first ? bytes.length : encodeAt(s ?? scaleFor(i), i).length;
          }
        }
        setSize(total);
        setFailed(null);
        // The browser shows a PNG, a JPEG, a WebP or an SVG as it is; a PDF or
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
        setFailed(said(err));
      }
    }, 350);
    return () => clearTimeout(timer);
  }, [encode, encodeAt, set, refusal, outW, outH, measure, spec.shows, spec.mime, session, format, layerCount]);

  // Asking for a size is about the export as it now stands, so changing
  // any of it puts the question back.
  useEffect(
    () => setMeasure(0),
    [format, area, scale, prefs.jpegQuality, prefs.pdfBleed, prefs.pdfCropMarks, prefs.exportCameraData, prefs.exportSharpen, prefs.exportWatermark],
  );

  // Named for its size where a size was asked for — `-2048` — rather
  // than for the scale that came to, which is a fraction nobody chose.
  // Shown as it will be written: a region's slash or colon made safe.
  const nameAt = (s: number, slice: string | null = null, at: number | null = null) =>
    fileSafe(`${fileName()}${slice !== null ? `-${slice}` : area === "selection" ? "-selection" : ""}${
      longEdge > 0 && !set ? `-${edgeOf(at)}` : s !== 1 || set ? `@${s}x` : ""
    }.${spec.ext}`);
  const more = slices.length > 1 ? `, and ${slices.length - 1} more` : "";
  const [firstName] = slices[0] ?? [null];
  const name = set
    ? `${nameAt(1, firstName)}, @2x, @3x${more}`
    : first !== null
      ? `${nameAt(scale, firstName, first)}${more}`
      : nameAt(scale);

  const run = useCallback(async (via: "write" | "share" = "write") => {
    if (refusal) return;
    // Every scale asked for, and within each, every slice: the set
    // and the slices multiply rather than choosing between each
    // other, which is what an asset pipeline wanting three sizes of
    // four slices means by it.
    const files: Outgoing[] = scales.flatMap((s) =>
      slices.map(([slice, at]) => ({
        name: nameAt(s, slice, at),
        type: spec.mime,
        make: () => encodeAt(set ? s : scaleFor(at), at),
      })),
    );
    try {
      // A panel put away leaves the window up, with what was in it:
      // nobody who cancels a save panel meant to start again.
      if (await (via === "share" && share ? share : write)(files)) onClose();
    } catch (err) {
      setFailed(said(err));
    }
    // `scales` and `nameAt` are made afresh each render from what the
    // dependencies below say.
  }, [encodeAt, set, scale, area, format, fileName, spec.mime, spec.ext, refusal, write, share, onClose, ats.join(",")]);

  const runRef = useRef(run);
  runRef.current = run;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
      if (enterMeansGo(e)) runRef.current();
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

        {/* The exports kept by name: a press answers all four questions
            at once, alt forgets one, and + keeps what the window says
            now under a name made from it. What is in hand is marked. */}
        <div className="export-setups" role="group" aria-label="Setups">
          {prefs.exportSetups.map((st) => {
            const inHand =
              st.format === format &&
              st.area === area &&
              (st.longEdge ?? 0) === longEdge &&
              (longEdge > 0 || st.scale === (set ? 0 : scale)) &&
              (format !== "jpeg" || st.jpegQuality === prefs.jpegQuality) &&
              (format !== "pdf" ||
                (st.pdfBleed === prefs.pdfBleed && st.pdfCropMarks === prefs.pdfCropMarks));
            return (
              <button
                key={st.name}
                className={inHand ? "preset setup active" : "preset setup"}
                aria-pressed={inHand}
                aria-label={`Setup ${st.name}`}
                title={`${st.name} — alt-click or Delete to forget it`}
                onKeyDown={chipKeys(() =>
                  setPrefs({
                    exportSetups: prefs.exportSetups.filter((o) => o.name !== st.name),
                  }),
                )}
                onClick={(e) => {
                  if (e.altKey) {
                    setPrefs({
                      exportSetups: prefs.exportSetups.filter((o) => o.name !== st.name),
                    });
                    return;
                  }
                  setPrefs({
                    exportFormat: st.format,
                    exportArea: st.area,
                    exportScale: st.scale,
                    exportLongEdge: st.longEdge ?? 0,
                    jpegQuality: st.jpegQuality,
                    // A PDF kept for a press brings that press back; any
                    // other setup leaves what the PDF was last asked alone.
                    ...(st.format === "pdf"
                      ? { pdfBleed: st.pdfBleed, pdfCropMarks: st.pdfCropMarks }
                      : {}),
                  });
                }}
              >
                {st.name}
              </button>
            );
          })}
          <button
            className="preset setup add"
            aria-label="Keep this setup"
            title="Keep what the window says now — format, area, size, quality — by name"
            onClick={() => {
              // Named for what it is: the format, the size when it is not
              // one to one, the quality when it is a JPEG, the area when
              // it is not the page — and a number when that name is taken.
              const base = [
                spec.label,
                set
                  ? "set"
                  : longEdge > 0
                    ? `${longEdge}px`
                    : scale !== 1 && spec.scales
                      ? `${scale}×`
                      : "",
                format === "jpeg" ? `q${prefs.jpegQuality}` : "",
                format === "pdf" && prefs.pdfBleed > 0 ? `${prefs.pdfBleed} mm` : "",
                format === "pdf" && prefs.pdfCropMarks ? "marks" : "",
                area === "selection" ? "picked" : "",
              ]
                .filter(Boolean)
                .join(" ");
              const taken = new Set(prefs.exportSetups.map((o) => o.name));
              let name = base;
              for (let n = 2; taken.has(name); n++) name = `${base} ${n}`;
              const kept: ExportSetup = {
                name,
                format,
                area,
                scale: set ? 0 : longEdge > 0 ? 1 : scale,
                ...(longEdge > 0 ? { longEdge } : {}),
                jpegQuality: prefs.jpegQuality,
                pdfBleed: prefs.pdfBleed,
                pdfCropMarks: prefs.pdfCropMarks,
              };
              setPrefs({ exportSetups: [...prefs.exportSetups, kept] });
            }}
          >
            {/* Said in words until there is a setup beside it to say
                what the row is for; a bare + above the formats said
                nothing. */}
            {prefs.exportSetups.length === 0 ? "+ Keep these settings" : "+"}
          </button>
        </div>

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
        {format === "psd" && psdNotes.length > 0 && (
          <p className="modal-aside export-notes" role="status" aria-label="What changes">
            Not as they are here: {psdNotes.join("; ")}.
          </p>
        )}

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
            {keptRegions.length > 0 && (
              <option value="regions">
                {/* How many will be written, of how many kept: the ones off
                    the page are left out, and seven said here wrote two. */}
                Each kept region ({onPageRegions < keptRegions.length ? `${onPageRegions} of ${keptRegions.length}` : keptRegions.length})
              </option>
            )}
            {keptComps.length > 0 && (
              <option value="comps">Each layer comp ({keptComps.length})</option>
            )}
          </select>
          {!spec.area ? (
            <span className="hint">{spec.label} carries the whole page</span>
          ) : format === "jpeg" && (area === "selection" || area === "regions") ? (
            <span className="hint">as the box that holds it</span>
          ) : null}
        </label>

        <div className="row export-scale-row" role="group" aria-label="Scale">
          <span>Size</span>
          <div className="export-scales">
            {SCALES.map((s) => (
              <button
                key={s}
                className={!set && !longEdge && s === scale ? "preset active" : "preset"}
                disabled={!spec.scales}
                aria-pressed={!set && !longEdge && s === scale}
                onClick={() => setPrefs({ exportScale: s, exportLongEdge: 0 })}
              >
                {s === 1 ? "1×" : `${s}×`}
              </button>
            ))}
            <button
              className={set ? "preset active" : "preset"}
              disabled={!spec.scales}
              aria-pressed={set}
              title="1×, 2× and 3×: three files in one press, named @1x, @2x and @3x"
              onClick={() => setPrefs({ exportScale: 0, exportLongEdge: 0 })}
            >
              Set
            </button>
            {/* The way a photograph is sized for somewhere else: its long
                edge in pixels, whatever the camera gave it. */}
            <label
              className={longEdge ? "export-long active" : "export-long"}
              title="The long edge in pixels — 2048 for the web — whatever the picture's own size; empty for the scale"
            >
              <NumberField
                min={16}
                max={16384}
                step={1}
                integer
                blankAs={0}
                show={longEdge ? undefined : ""}
                placeholder="Long edge"
                disabled={!spec.scales}
                value={longEdge}
                onValue={(v) =>
                  setPrefs({
                    exportLongEdge: v,
                    // Asked for one size, the set is let go of.
                    ...(set ? { exportScale: 1 } : {}),
                  })
                }
                aria-label="Long edge"
              />
              px
            </label>
          </div>
          <span className="hint">
            {!spec.scales
              ? "as drawn"
              : set
                ? `${outW} × ${outH} px, ×2, ×3`
                : area === "regions"
                  ? `${outW} × ${outH} px, and each other region's own`
                  : `${outW} × ${outH} px${capped ? ` — as large as it goes, ${MOST_SCALE} times the page` : ""}`}
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

        {/* Lightroom's "sharpen for screen": what a photograph made
            smaller loses at its edges, given back at the size it goes
            out at. */}
        {pixels && (
          <label className="row">
            Sharpen for screen
            <select
              value={String(prefs.exportSharpen)}
              onChange={(e) => setPrefs({ exportSharpen: Number(e.target.value) })}
              aria-label="Sharpen for screen"
              title="Sharpening for a screen, at the size the picture goes out at — what making it smaller softened"
            >
              {SHARPEN_FOR_SCREEN.map(([name, v]) => (
                <option key={name} value={String(v)}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}

        {pixels && (
          <WatermarkRow mark={prefs.exportWatermark} onChange={(exportWatermark) => setPrefs({ exportWatermark })} />
        )}

        {format === "jpeg" && cameraSummary && (
          <label
            className="row check"
            title="The camera, lens, exposure and date the photograph was taken with, as its EXIF — never where it was taken"
          >
            <input
              type="checkbox"
              checked={prefs.exportCameraData}
              onChange={(e) => setPrefs({ exportCameraData: e.target.checked })}
              aria-label="Camera data"
            />
            Camera data
            <span className="hint">{cameraSummary}</span>
          </label>
        )}

        {format === "pdf" && (
          <div className="row" role="group" aria-label="For the press">
            <label>
              Bleed
              <input
                type="number"
                min={0}
                max={25}
                step={0.5}
                value={prefs.pdfBleed}
                onChange={(e) => {
                  const mm = Number(e.target.value);
                  if (Number.isFinite(mm)) setPrefs({ pdfBleed: Math.min(25, Math.max(0, mm)) });
                }}
                aria-label="Bleed"
              />
              <span className="hint">mm</span>
            </label>
            <label>
              <input
                type="checkbox"
                checked={prefs.pdfCropMarks}
                onChange={(e) => setPrefs({ pdfCropMarks: e.target.checked })}
                aria-label="Crop marks"
              />
              Crop marks
            </label>
          </div>
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
        {!refusal && offPage.length > 0 && (
          <p className="export-size export-left-out">
            Left out, not on the page: {offPage.map((n) => `“${n}”`).join(", ")}
          </p>
        )}

        <div className="modal-actions">
          <button className="mask-button" onClick={onClose}>
            Cancel
          </button>
          {share && (
            // The same bytes as Export, to the share sheet: on a phone
            // that is where a picture goes next, and Save to Files is
            // one of its answers.
            <button className="mask-button" disabled={!!refusal} onClick={() => run("share")}>
              Share…
            </button>
          )}
          <button
            className="mask-button primary"
            disabled={!!refusal}
            onClick={() => run()}
            // Beside Share rather than pushed away from it: the two are
            // the same question with two answers.
            style={share ? { marginLeft: 0 } : undefined}
          >
            Export
          </button>
        </div>
      </div>
    </div>
  );
}
