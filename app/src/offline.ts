/** The editor opening with no network, from a home screen: a service
 * worker (`public/sw.js`) keeps what the page loads. Only in a built
 * page in a browser — the shell's files are on the machine already,
 * and the dev server's change on every save. */

import { isTauri } from "./nativeMenu";

export function keepForOffline() {
  if (isTauri() || !import.meta.env.PROD || !("serviceWorker" in navigator)) return;
  window.addEventListener("load", () => {
    navigator.serviceWorker
      .register("./sw.js")
      .then(() => navigator.serviceWorker.ready)
      .then((reg) => {
        // What this visit loaded, said once the worker can keep it: the
        // page itself, its script, style and engine, and its fonts. And
        // said again whenever more has finished loading: the engine is
        // seven megabytes fetched by the script, and finishes after the
        // page's load as often as not — said only once, it was missing
        // from what was kept, and the editor opened offline without it.
        // And the build's whole list (`build-assets.json`), which the
        // worker keeps from being pruned though this visit did not load
        // all of it: the raw and PDF readers load when first asked for,
        // some from a worker, and pruned they failed offline.
        const built = fetch("./build-assets.json")
          .then((r) => (r.ok ? (r.json() as Promise<string[]>) : []))
          .then((list) => list.map((f) => new URL(`./${f}`, location.href).href))
          .catch(() => [] as string[]);
        const say = () => {
          const keep = [
            new URL("./", location.href).href,
            ...performance
              .getEntriesByType("resource")
              .map((e) => e.name)
              .filter((u) => new URL(u).origin === location.origin),
          ];
          void built.then((build) => reg.active?.postMessage({ keep, build }));
        };
        say();
        // And, once the editor is up and quiet, the readers a file is
        // opened with, fetched into the cache ahead of being asked for:
        // kept only when a visit had loaded them, an installed editor
        // offline could open neither a PDF nor a camera's raw file it had
        // not opened before. Not the subject model's runtime, which is
        // fourteen megabytes and asked for in so many words; nor on a
        // connection that asks to be spared.
        const spare = (navigator as Navigator & { connection?: { saveData?: boolean } }).connection?.saveData;
        if (!spare) {
          window.setTimeout(() => {
            void built.then((build) => {
              const readers = build.filter((u) =>
                /chitrakar_(pdf|raw)_in|rawWorker|rawThread|developWorker|wasm-threads\//.test(u),
              );
              if (readers.length > 0) reg.active?.postMessage({ keep: [new URL("./", location.href).href, ...readers], build });
            });
          }, 15000);
        }
        if (typeof PerformanceObserver !== "undefined") {
          let soon: number | undefined;
          new PerformanceObserver(() => {
            clearTimeout(soon);
            soon = window.setTimeout(say, 300);
          }).observe({ type: "resource" });
        }
      })
      .catch((err) => console.warn("offline:", err));
  });
}

/** Where the service worker keeps files shared to the app. */
const SHARED = "chitrakar-shared";

/** Files another app shared to this one through the system's share
 * sheet. The worker takes the share — a form posted to `share-target`,
 * which no page can receive — keeps the files, and opens the page with
 * `?shared`; the page takes them from there, once, and the address goes
 * back to plain so a reload does not take them twice. */
export async function takeShared(): Promise<File[]> {
  const url = new URL(location.href);
  if (!url.searchParams.has("shared")) return [];
  url.searchParams.delete("shared");
  history.replaceState(null, "", url.href);
  if (!("caches" in window) || !(await caches.has(SHARED))) return [];
  const cache = await caches.open(SHARED);
  const files: File[] = [];
  for (const req of await cache.keys()) {
    const got = await cache.match(req);
    if (got) {
      const name = decodeURIComponent(got.headers.get("X-Name") ?? "shared");
      const type = got.headers.get("Content-Type") ?? "";
      files.push(new File([await got.blob()], name, { type }));
    }
    await cache.delete(req);
  }
  return files;
}

/** Files the system opened with the app — a document or a picture
 * chosen in a phone's or a desktop's files, with the app installed and
 * named as one that opens them (`file_handlers` in the manifest). */
export function onLaunch(take: (files: File[]) => void) {
  const queue = (window as unknown as { launchQueue?: LaunchQueue }).launchQueue;
  queue?.setConsumer(async (params) => {
    const files = await Promise.all((params.files ?? []).map((h) => h.getFile()));
    if (files.length > 0) take(files);
  });
}

type LaunchQueue = {
  setConsumer(consumer: (params: { files?: { getFile(): Promise<File> }[] }) => void): void;
};
