/** The subject model's worker (`subject.ts`): U^2-Net run by
 * onnxruntime-web, off the page's thread.
 *
 * The model is read from Cache Storage once and kept as a session; each
 * request is a PNG of the picture, decoded here, averaged down to the
 * model's 320 by 320, normalised as it was trained, and answered with the
 * saliency map stretched to nought–255 — a matte the engine lays over the
 * picture (`pick_matte_grey`).
 *
 * onnxruntime runs on threads of its own where the page is cross-origin
 * isolated (it is, for the develop worker's sake), with SIMD. */

import * as ort from "onnxruntime-web/wasm";
import wasmUrl from "onnxruntime-web/ort-wasm-simd-threaded.wasm?url";
// Its glue, served as it is (vite.config.ts): its threads are started
// from this file's own address.
import mjsUrl from "onnxruntime-web/ort-wasm-simd-threaded.mjs?url";

const SIDE = 320;

ort.env.wasm.wasmPaths = { mjs: mjsUrl, wasm: wasmUrl };
ort.env.wasm.numThreads = Math.max(1, Math.min(8, navigator.hardwareConcurrency || 4));

let session: Promise<ort.InferenceSession> | null = null;

async function load(url: string, cacheName: string): Promise<ort.InferenceSession> {
  const cache = await caches.open(cacheName);
  const kept = await cache.match(url);
  if (!kept) throw new Error("the subject model is not kept in this browser");
  const bytes = new Uint8Array(await kept.arrayBuffer());
  return ort.InferenceSession.create(bytes, { executionProviders: ["wasm"], graphOptimizationLevel: "all" });
}

/** A PNG decoded to its pixels, through the browser's own decoder. */
async function pixels(png: Uint8Array): Promise<{ data: Uint8ClampedArray; w: number; h: number }> {
  const bitmap = await createImageBitmap(new Blob([png as BlobPart], { type: "image/png" }));
  const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("this browser cannot draw a picture in a worker");
  ctx.drawImage(bitmap, 0, 0);
  bitmap.close();
  return { data: ctx.getImageData(0, 0, canvas.width, canvas.height).data, w: canvas.width, h: canvas.height };
}

/** The picture averaged down to the model's square (each cell the mean of
 * the pixels over it, rather than one pixel in seven), normalised. */
function input({ data, w, h }: { data: Uint8ClampedArray; w: number; h: number }): Float32Array {
  const sum = new Float32Array(SIDE * SIDE * 3);
  const count = new Float32Array(SIDE * SIDE);
  for (let y = 0; y < h; y++) {
    const ty = Math.min(SIDE - 1, Math.floor((y * SIDE) / h));
    for (let x = 0; x < w; x++) {
      const tx = Math.min(SIDE - 1, Math.floor((x * SIDE) / w));
      const k = ty * SIDE + tx;
      const p = (y * w + x) * 4;
      sum[k * 3] += data[p];
      sum[k * 3 + 1] += data[p + 1];
      sum[k * 3 + 2] += data[p + 2];
      count[k] += 1;
    }
  }
  const mean = [0.485, 0.456, 0.406];
  const sd = [0.229, 0.224, 0.225];
  const out = new Float32Array(3 * SIDE * SIDE);
  for (let k = 0; k < SIDE * SIDE; k++) {
    let n = count[k];
    let rgb = [sum[k * 3], sum[k * 3 + 1], sum[k * 3 + 2]];
    if (n === 0) {
      // A picture smaller than the square: the nearest pixel stands in.
      const sy = Math.floor(((k / SIDE) | 0) * (h / SIDE));
      const sx = Math.floor((k % SIDE) * (w / SIDE));
      const p = (sy * w + sx) * 4;
      rgb = [data[p], data[p + 1], data[p + 2]];
      n = 1;
    }
    for (let c = 0; c < 3; c++) out[c * SIDE * SIDE + k] = (rgb[c] / (n * 255) - mean[c]) / sd[c];
  }
  return out;
}

self.onmessage = async (
  e: MessageEvent<{ id: number; png: Uint8Array; url: string; cache: string }>,
) => {
  const { id, png, url, cache } = e.data;
  const post = (msg: object, transfer: Transferable[] = []) =>
    (self as unknown as Worker).postMessage(msg, transfer);
  try {
    session ??= load(url, cache);
    const s = await session;
    const tensor = new ort.Tensor("float32", input(await pixels(png)), [1, 3, SIDE, SIDE]);
    const out = await s.run({ [s.inputNames[0]]: tensor });
    // The first output is the fused map; the rest are its side views.
    const map = out[s.outputNames[0]].data as Float32Array;
    let lo = Infinity;
    let hi = -Infinity;
    for (const v of map) {
      lo = Math.min(lo, v);
      hi = Math.max(hi, v);
    }
    const span = Math.max(hi - lo, 1e-6);
    const matte = new Uint8Array(SIDE * SIDE);
    for (let i = 0; i < matte.length; i++) matte[i] = Math.round(((map[i] - lo) / span) * 255);
    post({ id, matte, side: SIDE }, [matte.buffer]);
  } catch (err) {
    session = null;
    post({ id, error: err instanceof Error ? err.message : String(err) });
  }
};

export {};
