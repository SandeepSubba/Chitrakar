/** Select › Subject in the browser, by a model that knows what a subject
 * is — U^2-Net, run by onnxruntime-web in a worker (`subjectWorker.ts`).
 *
 * The engine's own pick (`Session::pick_subject`) reasons about colour
 * and cannot tell a white shirt from the white wall behind it; a model
 * trained on photographs with their subjects drawn round can, and picks
 * the way a person would: the skier with the skis and poles, every child
 * in a group, both cats on the sofa.
 *
 * The model is 176 MB (Apache-2.0), so it is never shipped: the person is
 * asked once, with the size said, and it is fetched from Hugging Face —
 * pinned to one commit and checked against its SHA-256 — and kept in the
 * browser's Cache Storage. Until then, or if they would rather not, the
 * colour pick answers as before. The desktop shell asks the system's
 * model or one on disk instead (`subject_matte`). */

/** The model file, pinned to a commit so what is fetched is what was
 * measured, and the digest it must have. */
export const SUBJECT_MODEL = {
  url: "https://huggingface.co/BritishWerewolf/U-2-Net/resolve/c08042b462485d8597c89d3ce831d00be8582c5c/onnx/model.onnx",
  bytes: 175_997_641,
  sha256: "8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491",
};

const CACHE = "chitrakar-models";

/** Whether the model is kept in this browser. */
export async function hasSubjectModel(): Promise<boolean> {
  try {
    const cache = await caches.open(CACHE);
    return (await cache.match(SUBJECT_MODEL.url)) !== undefined;
  } catch {
    return false;
  }
}

/** Fetch the model and keep it, telling `progress` the fraction done. It
 * is checked against its digest before it is kept, so a cut-off or
 * altered download is refused rather than run. */
export async function fetchSubjectModel(progress: (done: number) => void): Promise<void> {
  const got = await fetch(SUBJECT_MODEL.url, { mode: "cors" });
  // Said as what the person can do about it; "could not be downloaded:
  // the model could not be fetched" said the one thing twice.
  if (!got.ok || !got.body) throw new Error(`the server answered ${got.status} — try again later`);
  const reader = got.body.getReader();
  const parts: Uint8Array[] = [];
  let have = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    parts.push(value);
    have += value.length;
    progress(Math.min(1, have / SUBJECT_MODEL.bytes));
  }
  const bytes = new Uint8Array(have);
  let at = 0;
  for (const p of parts) {
    bytes.set(p, at);
    at += p.length;
  }
  const digest = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  if (digest !== SUBJECT_MODEL.sha256) {
    throw new Error("the downloaded model is not the one expected, so it was not kept");
  }
  const cache = await caches.open(CACHE);
  await cache.put(
    SUBJECT_MODEL.url,
    new Response(bytes, { headers: { "Content-Type": "application/octet-stream" } }),
  );
}

/** Forget the model: a person giving the space back. */
export async function forgetSubjectModel(): Promise<void> {
  try {
    const cache = await caches.open(CACHE);
    await cache.delete(SUBJECT_MODEL.url);
  } catch {
    // Nothing kept, nothing to forget.
  }
  worker?.terminate();
  worker = null;
}

let worker: Worker | null = null;
let next = 0;
const waiting = new Map<number, (r: { matte?: Uint8Array; side?: number; error?: string }) => void>();

/** The subject's matte for a picture (a PNG, which the worker decodes):
 * `side` by `side` bytes, nought for background. Throws where the model
 * is not kept or would not run; the caller falls back to the colour
 * pick. */
export function subjectMatte(png: Uint8Array): Promise<{ matte: Uint8Array; side: number }> {
  if (!worker) {
    worker = new Worker(new URL("./subjectWorker.ts", import.meta.url), { type: "module" });
    worker.onmessage = (e: MessageEvent<{ id: number; matte?: Uint8Array; side?: number; error?: string }>) => {
      const done = waiting.get(e.data.id);
      waiting.delete(e.data.id);
      done?.(e.data);
    };
    worker.onerror = (e) => {
      e.preventDefault();
      for (const [, done] of waiting) done({ error: e.message || "the subject model stopped" });
      waiting.clear();
      worker?.terminate();
      worker = null;
    };
  }
  const id = next++;
  const copy = png.slice();
  return new Promise((resolve, reject) => {
    waiting.set(id, (r) =>
      r.matte && r.side ? resolve({ matte: r.matte, side: r.side }) : reject(new Error(r.error ?? "no matte")),
    );
    worker!.postMessage({ id, png: copy, url: SUBJECT_MODEL.url, cache: CACHE }, [copy.buffer]);
  });
}
