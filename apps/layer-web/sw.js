// Generated values identify one complete package, not a mutable runtime cache.
const VERSION = "__CAPY_VERSION__";
const FILES = __CAPY_FILES__;
const BASE = self.registration.scope;
const PREFIX = `capycanvas:${BASE}:`;
const CACHE = PREFIX + VERSION;
const urls = new Set(FILES.map((file) => new URL(file.path, BASE).href));

self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(CACHE);
    try {
      // Integrity makes an incomplete/mixed deployment fail installation, leaving
      // the previous version usable. HTTP caches cannot substitute stale assets.
      await cache.addAll(FILES.map((file) => new Request(new URL(file.path, BASE), {
        cache: "reload", integrity: file.integrity,
      })));
    } catch (error) {
      await caches.delete(CACHE);
      throw error;
    }
  })());
  // Deliberately no skipWaiting: an update must never replace an open drawing.
});

self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    for (const key of await caches.keys()) {
      if (key.startsWith(PREFIX) && key !== CACHE) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  url.search = "";
  if (url.href === BASE) url.pathname += "index.html";
  if (!urls.has(url.href)) return; // Never cache user data, APIs or neighboring apps.
  event.respondWith((async () => {
    const response = await (await caches.open(CACHE)).match(url.href);
    // A missing versioned resource must not fall through to another release.
    return response || new Response("App cache unavailable. Reopen online.", { status: 503 });
  })());
});
