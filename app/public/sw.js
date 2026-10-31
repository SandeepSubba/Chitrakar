// Chitrakar's service worker: the editor opens with no network, the
// way an app on a home screen is expected to.
//
// Two kinds of file, two rules. Everything under assets/ is named by
// its own contents, so a copy kept is right for as long as anything
// asks for that name: kept ones answer first. Everything else — the
// page itself, the manifest, the fonts — goes to the network first, so
// a new version arrives the moment there is one, and the kept copy
// answers only when the network does not.
//
// A page is not served by a worker on the visit that installed it, so
// what that first visit loaded never came through here. The page says
// what it loaded once the worker is in charge (see `offline.ts`), and
// those are kept then; kept files the page no longer loads go, so a
// cache does not grow by a build's worth on every update.

const CACHE = "chitrakar";

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

const ours = (url) => new URL(url).origin === self.location.origin;
const named = (url) => /\/assets\//.test(new URL(url).pathname);

// A share from another app's share sheet arrives as a form posted here
// (`share_target` in the manifest), which no page can receive: the files
// are kept, and the page is opened to take them (`takeShared`).
const SHARED = "chitrakar-shared";
const sharing = (req) => req.method === "POST" && new URL(req.url).pathname.endsWith("/share-target");

async function share(req) {
  const form = await req.formData();
  const cache = await caches.open(SHARED);
  let i = 0;
  for (const f of form.getAll("files")) {
    if (typeof f === "string") continue;
    await cache.put(
      new URL(`./shared/${Date.now()}-${i++}`, self.registration.scope).href,
      new Response(f, { headers: { "Content-Type": f.type, "X-Name": encodeURIComponent(f.name) } }),
    );
  }
  return Response.redirect(new URL("./?shared=1", self.registration.scope).href, 303);
}

self.addEventListener("fetch", (e) => {
  const req = e.request;
  if (sharing(req) && ours(req.url)) {
    e.respondWith(share(req));
    return;
  }
  if (req.method !== "GET" || !ours(req.url)) return;
  e.respondWith(
    (async () => {
      const cache = await caches.open(CACHE);
      if (named(req.url)) {
        const kept = await cache.match(req);
        if (kept) return kept;
        const got = await fetch(req);
        if (got.ok) await cache.put(req, got.clone());
        return got;
      }
      try {
        const got = await fetch(req);
        if (got.ok) await cache.put(req, got.clone());
        // A host that has no page at an address with a query in it — the
        // share's "?shared=1" — still has the page.
        if (!got.ok && req.mode === "navigate") {
          const page = await cache.match(new URL("./", self.location.href).href);
          if (page) return page;
        }
        return got;
      } catch (err) {
        // A navigation to the page under any query is the page.
        const kept =
          (await cache.match(req, { ignoreSearch: true })) ??
          (req.mode === "navigate" ? await cache.match(new URL("./", self.location.href).href) : undefined);
        if (kept) return kept;
        throw err;
      }
    })(),
  );
});

self.addEventListener("message", (e) => {
  const urls = Array.isArray(e.data?.keep) ? e.data.keep.filter((u) => typeof u === "string" && ours(u)) : [];
  if (urls.length === 0) return;
  e.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      const wanted = new Set(urls.map((u) => new URL(u).href.split("#")[0]));
      for (const req of await cache.keys()) {
        if (named(req.url) && !wanted.has(req.url)) await cache.delete(req);
      }
      for (const url of wanted) {
        if (!(await cache.match(url))) {
          try {
            const got = await fetch(url);
            if (got.ok) await cache.put(url, got);
          } catch {
            // Offline already: what is kept is what there is.
          }
        }
      }
      e.source?.postMessage({ kept: wanted.size });
    })(),
  );
});
