/**
 * A shoot developed and sent out in one go: raw files chosen together,
 * each developed with the same settings (the plain development or one
 * kept by name) and written as a JPEG — at a long edge, sharpened for a
 * screen, carrying its camera's data — without one of them being opened
 * on the page. What a raw developer's batch export is.
 *
 * Each photograph is developed on a session of its own (an engine of
 * its own, made and let go of per file), so the page's document is not
 * touched, and its pixels are let go of before the next is read: a
 * forty-megapixel photograph is a third of a gigabyte developed. The
 * JPEGs, a few megabytes each, are kept until they are written together
 * — the downloads in a browser, one folder in the shell.
 */
import { useEffect, useRef, useState } from "react";
import { withDefaults, type DevelopPreset } from "./develop";
import { said } from "./imports";
import { WasmSession } from "./engine";
import type { Outgoing } from "./files";
import { developElsewhere } from "./developer";
import { giveLensProfiles, lensProfileTexts } from "./lensProfiles";
import { watermarkJson, type Prefs } from "./prefs";
import { WatermarkRow } from "./WatermarkRow";
import { NumberField } from "./NumberField";
import { readRaw } from "./raw";

/** A file chosen, as read. */
/** A raw file chosen for the batch, read only when its turn comes: read
 * up front, eight 43 MB files took the page from 51 MB to 396 MB before
 * anything was developed, and a whole shoot could not fit in a tab. */
export type BatchFile = { name: string; read: () => Promise<Uint8Array> };

const SHARPEN: [string, number][] = [
  ["Off", 0],
  ["Low", 0.3],
  ["Standard", 0.6],
  ["High", 1],
];

/** One raw file developed with `settings` and made a JPEG, as `opts`
 * asks: never made bigger than it is, named for its long edge when it
 * is given one. */
export async function developToJpeg(
  file: BatchFile,
  settings: string,
  opts: { longEdge: number; quality: number; sharpen: number; cameraData: boolean; watermark: string },
): Promise<Outgoing> {
  const raw = await file.read();
  const packed = await readRaw(raw);
  const temp = new WasmSession(1, 1, false);
  try {
    await giveLensProfiles(temp);
    temp.raw_open(packed, raw, file.name);
    // On every core where the page can wait for them (the develop
    // worker), else here.
    const got = await developElsewhere(packed, file.name, settings, await lensProfileTexts());
    if (got) temp.place_raw_pixels(got.rgba, got.width, got.height, settings);
    else temp.place_raw(settings);
    temp.raw_close();
    const long = Math.max(temp.width, temp.height, 1);
    const scale = opts.longEdge > 0 ? Math.min(1, opts.longEdge / long) : 1;
    temp.set_output_sharpen(opts.sharpen);
    temp.set_output_watermark(opts.watermark);
    let bytes = temp.export_jpeg_at(scale, 0, 0, 0, 0, opts.quality);
    if (opts.cameraData) bytes = temp.jpeg_with_capture(bytes);
    const base = file.name.replace(/\.[^.]+$/, "");
    const edge = opts.longEdge > 0 && scale < 1 ? `-${opts.longEdge}` : "";
    return { name: `${base}${edge}.jpg`, type: "image/jpeg", make: () => bytes };
  } finally {
    temp.free();
  }
}

export function BatchDialog({
  files,
  presets,
  prefs,
  setPrefs,
  write,
  onClose,
}: {
  files: BatchFile[];
  presets: DevelopPreset[];
  prefs: Prefs;
  setPrefs: (p: Partial<Prefs>) => void;
  write: (files: Outgoing[]) => Promise<boolean>;
  onClose: () => void;
}) {
  /** Which settings: "" is the plain development, else a kept one's name. */
  // The one last used, while it is still kept: chosen again each time,
  // where the rest of the window was remembered.
  const [preset, choosePreset] = useState(() =>
    presets.some((p) => p.name === prefs.batchPreset) ? prefs.batchPreset : "",
  );
  const setPreset = (name: string) => {
    choosePreset(name);
    setPrefs({ batchPreset: name });
  };
  const [running, setRunning] = useState<string | null>(null);
  const [failures, setFailures] = useState<string[]>([]);
  const stop = useRef(false);
  // Escape puts the window away like any other — not while a shoot is
  // being developed, which "Stop after this one" is for.
  const runningRef = useRef(running);
  runningRef.current = running;
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !runningRef.current) onClose();
    };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [onClose]);

  const unwritten = useRef<{ key: string; done: Outgoing[]; failed: string[] } | null>(null);
  const run = async () => {
    const kept = presets.find((p) => p.name === preset);
    const settings = JSON.stringify(withDefaults(kept?.settings ?? {}));
    const opts = {
      longEdge: prefs.exportLongEdge,
      quality: prefs.jpegQuality,
      sharpen: prefs.exportSharpen,
      cameraData: prefs.exportCameraData,
      watermark: watermarkJson(prefs.exportWatermark),
    };
    stop.current = false;
    // What was developed and not written — a folder panel put away, a
    // Replace answered No — is kept, so asking again writes it rather
    // than developing the whole shoot again.
    const key = JSON.stringify([settings, opts, files.map((f) => f.name)]);
    const waiting = unwritten.current?.key === key ? unwritten.current : null;
    const done: Outgoing[] = waiting ? [...waiting.done] : [];
    const failed: string[] = waiting ? [...waiting.failed] : [];
    for (const [i, f] of (waiting ? [] : files).entries()) {
      if (stop.current) {
        // Stopped is said, with what was left: the window closed on one
        // of two written and nothing said of the other.
        const left = files.slice(i).map((g) => g.name);
        failed.push(
          `Stopped: ${left.length} not developed (${left.slice(0, 6).join(", ")}${left.length > 6 ? ", …" : ""})`,
        );
        break;
      }
      setRunning(`Developing ${i + 1} of ${files.length}: ${f.name}`);
      try {
        done.push(await developToJpeg(f, settings, opts));
      } catch (err) {
        // As Place says it, in words a person reads (`said`): the
        // reader's own ("subview_until_eof(): Offset … is behind EOF")
        // were put in brackets after ours.
        const text = String(err);
        const plain = said(err);
        failed.push(
          /outside the bounds/i.test(text)
            ? `${f.name}: the file is damaged (its parts point outside it)`
            : plain !== text.replace(/^(?:Error|RuntimeError|JsError):\s*/i, "").trim()
              ? `${f.name}: ${plain}`
              : /camera raw|decode|EOF|corrupt/i.test(text)
                ? `${f.name}: it may be damaged, or from a camera this editor does not know yet`
                : `${f.name}: ${plain}`,
        );
      }
    }
    setRunning(done.length > 0 ? "Writing…" : null);
    let written = true;
    try {
      if (done.length > 0) written = await write(done);
    } catch (err) {
      failed.push(`Writing: ${err}`);
    }
    setRunning(null);
    // Nothing written, and nothing gone wrong: the person put the folder
    // panel away or would not replace a file. The window stays, with
    // what it developed, as the Export window does — it closed, and the
    // shoot developed was thrown away without a word.
    if (!written) {
      unwritten.current = { key, done, failed };
      setFailures([
        `Nothing was written. ${done.length} developed ${done.length === 1 ? "photograph waits" : "photographs wait"}: export again to choose where.`,
      ]);
      return;
    }
    unwritten.current = null;
    if (failed.length > 0) setFailures(failed);
    else onClose();
  };

  return (
    <div className="modal-scrim">
      <div className="modal batch" role="dialog" aria-label="Batch export" onPointerDown={(e) => e.stopPropagation()}>
        <h2>
          Develop and export {files.length} {files.length === 1 ? "photograph" : "photographs"}
        </h2>
        <p className="modal-aside" aria-label="Files">
          {files.length <= 4
            ? files.map((f) => f.name).join(", ")
            : `${files.slice(0, 3).map((f) => f.name).join(", ")} and ${files.length - 3} more`}
        </p>
        <label className="row">
          Develop with
          <select value={preset} onChange={(e) => setPreset(e.target.value)} aria-label="Develop with" disabled={!!running}>
            <option value="">The plain development</option>
            {presets.map((p) => (
              <option key={p.name} value={p.name}>
                {p.name}
              </option>
            ))}
          </select>
        </label>
        <label className="row">
          Long edge
          <NumberField
            min={16}
            max={16384}
            integer
            blankAs={0}
            show={prefs.exportLongEdge ? undefined : ""}
            placeholder="Full size"
            value={prefs.exportLongEdge}
            disabled={!!running}
            onValue={(v) => setPrefs({ exportLongEdge: v })}
            aria-label="Batch long edge"
          />
          px
        </label>
        <label className="row">
          Quality
          <input
            type="range"
            min={1}
            max={100}
            value={prefs.jpegQuality}
            disabled={!!running}
            onChange={(e) => setPrefs({ jpegQuality: Number(e.target.value) })}
            aria-label="Batch quality"
          />
          <span className="hint">{prefs.jpegQuality}</span>
        </label>
        <label className="row">
          Sharpen for screen
          <select
            value={String(prefs.exportSharpen)}
            disabled={!!running}
            onChange={(e) => setPrefs({ exportSharpen: Number(e.target.value) })}
            aria-label="Batch sharpen for screen"
          >
            {SHARPEN.map(([name, v]) => (
              <option key={name} value={String(v)}>
                {name}
              </option>
            ))}
          </select>
        </label>
        <WatermarkRow
          mark={prefs.exportWatermark}
          onChange={(exportWatermark) => setPrefs({ exportWatermark })}
          disabled={!!running}
        />
        <label className="row check">
          <input
            type="checkbox"
            checked={prefs.exportCameraData}
            disabled={!!running}
            onChange={(e) => setPrefs({ exportCameraData: e.target.checked })}
            aria-label="Batch camera data"
          />
          Camera data (never where it was taken)
        </label>
        {running && (
          <p className="modal-aside" role="status" aria-label="Batch progress">
            {running}
          </p>
        )}
        {failures.length > 0 && (
          <div className="modal-aside" role="alert" aria-label="Batch failures">
            {failures.map((f) => (
              <p key={f}>{f}</p>
            ))}
          </div>
        )}
        <div className="modal-actions">
          {running ? (
            <button
              // It takes the place of the button pressed to start, which
              // went from under the focus: the focus goes to it rather
              // than to the page for the whole run.
              ref={(el) => {
                if (el && (document.activeElement === document.body || document.activeElement === null)) el.focus();
              }}
              onClick={() => (stop.current = true)}
            >
              Stop after this one
            </button>
          ) : (
            <>
              <button onClick={onClose}>{failures.length > 0 ? "Close" : "Cancel"}</button>
              {(failures.length === 0 || unwritten.current !== null) && (
                <button className="primary" onClick={() => void run()}>
                  Export {files.length}
                </button>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
