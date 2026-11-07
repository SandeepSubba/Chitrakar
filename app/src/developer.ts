/** Developing a raw file in full on every core (`developWorker.ts`), where
 * the page can: only a cross-origin isolated page may share memory
 * between threads, which the threaded engine needs. The dev server, the
 * desktop shell and the installed app's service worker all send the two
 * headers that isolate it (COOP `same-origin`, COEP `require-corp`); a
 * page without them — a first visit before the service worker is in
 * charge, an old browser — develops on its own thread as before, and
 * `developElsewhere` says so by answering `null`.
 *
 * A failed worker is let go and the next development is the page's own:
 * the threaded build is a speed-up, never a thing a photograph waits
 * on. */

import type { DevelopReply, DevelopRequest } from "./developWorker";

/** Whether this page may run the threaded engine at all. */
export function canThread(): boolean {
  return (
    typeof SharedArrayBuffer !== "undefined" &&
    typeof Atomics !== "undefined" &&
    (globalThis as { crossOriginIsolated?: boolean }).crossOriginIsolated === true
  );
}

/** How many threads to give the pool: the machine's, at most eight (each
 * holds a band of the photograph while it works). */
export function threadCount(): number {
  const n = typeof navigator !== "undefined" ? navigator.hardwareConcurrency || 4 : 4;
  return Math.max(1, Math.min(8, n));
}

let worker: Worker | null = null;
let started: Promise<number | null> | null = null;
let broken = false;
let next = 0;
const waiting = new Map<number, (reply: DevelopReply) => void>();
/** The profiles the worker was last sent, by identity. */
let sentProfiles: string[] | null = null;

function ask(req: DevelopRequest, transfer: Transferable[] = []): Promise<DevelopReply> {
  return new Promise((done) => {
    waiting.set(req.id, done);
    worker!.postMessage(req, transfer);
  });
}

function retire() {
  worker?.terminate();
  worker = null;
  started = null;
  sentProfiles = null;
  broken = true;
  for (const [id, done] of waiting) done({ id, error: "the develop worker stopped" });
  waiting.clear();
}

/** Start the worker and its pool; how many threads it has, or `null`
 * where the page cannot thread or the worker would not start. */
export function startDeveloper(): Promise<number | null> {
  if (broken || !canThread()) return Promise.resolve(null);
  started ??= (async () => {
    try {
      worker = new Worker(new URL("./developWorker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (e: MessageEvent<DevelopReply>) => {
        const done = waiting.get(e.data.id);
        waiting.delete(e.data.id);
        done?.(e.data);
      };
      worker.onerror = (e) => {
        e.preventDefault();
        retire();
      };
      // A pool that has not started in twenty seconds is not going to
      // (a worker that could not load, a browser without nested
      // workers): the page develops on its own thread instead.
      const reply = await Promise.race([
        ask({ id: next++, kind: "start", threads: threadCount() }),
        new Promise<DevelopReply>((done) =>
          setTimeout(() => done({ id: -1, error: "the develop worker did not start" }), 20000),
        ),
      ]);
      if ("error" in reply || !("threads" in reply)) {
        retire();
        return null;
      }
      document.documentElement.dataset.developThreads = String(reply.threads);
      return reply.threads;
    } catch {
      retire();
      return null;
    }
  })();
  return started;
}

/** Develop a packed mosaic with `settings` (JSON) in the worker: the
 * pixels and their size, or `null` where it cannot (the page then
 * develops it itself). `profiles` are the lens profiles the page's engine
 * has, sent again only when they are another set. The mosaic is copied
 * across; the caller keeps its own. */
export async function developElsewhere(
  packed: Uint8Array,
  name: string,
  settings: string,
  profiles: string[],
): Promise<{ rgba: Uint8Array; width: number; height: number } | null> {
  if ((await startDeveloper()) === null || !worker) return null;
  const copy = packed.slice();
  const fresh = sentProfiles !== profiles;
  const reply = await ask(
    {
      id: next++,
      kind: "develop",
      packed: copy,
      name,
      settings,
      profiles: fresh ? profiles : undefined,
    },
    [copy.buffer],
  );
  if ("rgba" in reply) {
    if (fresh) sentProfiles = profiles;
    return { rgba: reply.rgba, width: reply.width, height: reply.height };
  }
  // A failure here is the worker's, not the photograph's: the page's own
  // engine is asked next, and says if it is the photograph's.
  retire();
  return null;
}
