// Generated values identify one complete package, not a mutable runtime cache.
const VERSION = "__CAPY_VERSION__";
const FILES = __CAPY_FILES__;
const BASE = self.registration.scope;
const PREFIX = `capycanvas:${BASE}:`;
const CACHE = PREFIX + VERSION;
const INDEX = new URL("index.html", BASE).href;
const ASSETS = new URL("assets/", BASE).href;
const urls = new Set(FILES.map((file) => new URL(file.path, BASE).href));

self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const existed = (await caches.keys()).includes(CACHE);
    const cache = await caches.open(CACHE);
    try {
      // Integrity makes an incomplete/mixed deployment fail installation, leaving
      // the previous version usable. HTTP caches cannot substitute stale assets.
      await cache.addAll(FILES.map((file) => new Request(new URL(file.path, BASE), {
        cache: "reload", integrity: file.integrity,
      })));
    } catch (error) {
      if (!existed) await caches.delete(CACHE);
      throw error;
    }
    // Switching request handlers does not reload an open drawing. Old hashed
    // resources remain available below, including during the first migration.
    await self.skipWaiting();
  })());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

async function pruneUnusedReleases(resultingClientId) {
  const keys = await caches.keys();
  if (!keys.includes(CACHE)) return;
  // Only older caches: a newer worker may be installing concurrently.
  const older = keys.slice(0, keys.indexOf(CACHE)).filter(key => key.startsWith(PREFIX));
  if (!older.length) return;
  const clients = await self.clients.matchAll({ type: "all", includeUncontrolled: true });
  if (clients.some(client => client.id !== resultingClientId && client.url.startsWith(BASE))) return;
  // A cold navigation with no previous clients is the safe cleanup boundary.
  // Ordinary refreshes may keep the outgoing client alive; defer in that case.
  await Promise.all(older.map(key => caches.delete(key)));
}

async function page(request) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 5000);
  try {
    // GitHub Pages gives HTML a ten-minute HTTP lifetime. Network-first alone
    // would still reuse it; explicitly validate on every startup/refresh.
    const response = await fetch(request, { cache: "no-cache", signal: controller.signal });
    if (response.ok && response.headers.get("content-type")?.includes("text/html")) return response;
  } catch { /* Offline/timeout: use the last integrity-checked complete package. */ }
  finally { clearTimeout(timer); }
  // Never overwrite this fallback with HTML whose dependencies may be missing.
  return await caches.match(INDEX, { cacheName: CACHE }) || unavailable();
}

const unavailable = () => new Response("App cache unavailable. Reopen online.", { status: 503 });

async function asset(request, url) {
  // An older tab can fetch a worker, icon or filter long after a new release
  // activates. Exact hashed URLs let both releases coexist without mixing bytes.
  const current = await caches.match(url, { cacheName: CACHE });
  if (current) return current;
  for (const key of await caches.keys()) {
    if (key === CACHE || !key.startsWith(PREFIX)) continue;
    const response = await caches.match(url, { cacheName: key });
    if (response) return response;
  }
  // Fresh HTML can arrive before its worker. Its new hashes must reach the
  // network; precaching, rather than runtime writes, publishes offline releases.
  return fetch(request);
}

self.addEventListener("fetch", (event) => {
  if (event.request.method !== "GET") return;
  const url = new URL(event.request.url);
  url.search = "";
  if (url.href === BASE) url.pathname += "index.html";
  if (url.href === INDEX && event.request.mode === "navigate") {
    event.respondWith(page(event.request));
    event.waitUntil(pruneUnusedReleases(event.resultingClientId));
    return;
  }
  if (url.href.startsWith(ASSETS) && /\.[0-9a-f]{20}\.[a-z0-9]+$/.test(url.pathname)) {
    event.respondWith(asset(event.request, url.href));
    return;
  }
  if (!urls.has(url.href)) return; // Never cache user data, APIs or neighboring apps.
  event.respondWith((async () => {
    const response = await caches.match(url.href, { cacheName: CACHE });
    return response || unavailable();
  })());
});
