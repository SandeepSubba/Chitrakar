/** Reading a camera's raw file, off the page: the reader (`core/rawin`)
 * runs in a worker (`rawWorker.ts`) fetched the first time a raw file is
 * opened, and hands back the sensor's values packed for the engine to
 * develop (`session.raw_open`).
 *
 * After any failure the worker is let go and the next read starts a new
 * one: a failure can be the reader's own panic, which leaves its module
 * in a state nothing should run in again. */

/** A raw file the reader could not read; printed as what it said. */
export class RawError extends Error {
  toString() {
    return this.message;
  }
}

let worker: Worker | null = null;
let next = 0;
const waiting = new Map<number, { done: (b: Uint8Array) => void; fail: (e: Error) => void }>();

function start(): Worker {
  const w = new Worker(new URL("./rawWorker.ts", import.meta.url), { type: "module" });
  w.onmessage = (e: MessageEvent<{ id: number; packed?: Uint8Array; error?: string }>) => {
    const job = waiting.get(e.data.id);
    waiting.delete(e.data.id);
    if (!job) return;
    if (e.data.packed) {
      job.done(e.data.packed);
    } else {
      job.fail(new RawError(e.data.error ?? "the raw file could not be read"));
      retire(w);
    }
  };
  // A worker that dies outright (its module would not load, say) fails
  // everything waiting on it.
  w.onerror = (e) => {
    e.preventDefault();
    for (const [, job] of waiting) job.fail(new RawError(e.message || "the raw reader stopped"));
    waiting.clear();
    retire(w);
  };
  return w;
}

function retire(w: Worker) {
  w.terminate();
  if (worker === w) worker = null;
}

/** A raw file's mosaic, packed for the engine. The file is copied across,
 * so the caller keeps its own bytes for the document. */
export function readRaw(bytes: Uint8Array): Promise<Uint8Array> {
  worker ??= start();
  const id = next++;
  const copy = bytes.slice();
  return new Promise((done, fail) => {
    waiting.set(id, { done, fail });
    worker!.postMessage({ id, bytes: copy }, [copy.buffer]);
  });
}
