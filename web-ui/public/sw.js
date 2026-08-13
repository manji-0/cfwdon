const CACHE_NAME = "cfwdon-app-__SW_CACHE_VERSION__";
const APP_SHELL = "/app/";

const shouldBypass = (url) => {
  if (url.origin !== self.location.origin) {
    return true;
  }
  const path = url.pathname;
  return (
    path.startsWith("/api/") ||
    path.startsWith("/oauth") ||
    path === "/app/login" ||
    path === "/app/logout" ||
    path === "/app/sw.js"
  );
};

const isHashedAsset = (url) => url.pathname.startsWith("/app/assets/");
const isIcon = (url) => url.pathname.startsWith("/app/icons/");
const isManifest = (url) => url.pathname.endsWith(".webmanifest");

const cacheFirst = async (request) => {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  if (cached) {
    return cached;
  }
  const response = await fetch(request);
  if (response.ok) {
    await cache.put(request, response.clone());
  }
  return response;
};

const staleWhileRevalidate = async (request) => {
  const cache = await caches.open(CACHE_NAME);
  const cached = await cache.match(request);
  const network = fetch(request).then((response) => {
    if (response.ok) {
      void cache.put(request, response.clone());
    }
    return response;
  });
  if (cached) {
    void network.catch(() => undefined);
    return cached;
  }
  return network;
};

const networkFirst = async (request, cacheKey = request) => {
  const cache = await caches.open(CACHE_NAME);
  try {
    const response = await fetch(request);
    if (response.ok) {
      await cache.put(cacheKey, response.clone());
    }
    return response;
  } catch (error) {
    const fallback = (await cache.match(request)) ?? (await cache.match(APP_SHELL));
    if (fallback) {
      return fallback;
    }
    throw error;
  }
};

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE_NAME)
      .then((cache) => cache.addAll([APP_SHELL, "/app/manifest.webmanifest"]))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((key) => key !== CACHE_NAME).map((key) => caches.delete(key))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") {
    return;
  }
  const url = new URL(request.url);
  if (shouldBypass(url)) {
    return;
  }
  if (!url.pathname.startsWith("/app")) {
    return;
  }

  if (isHashedAsset(url)) {
    event.respondWith(cacheFirst(request));
    return;
  }
  if (isIcon(url)) {
    event.respondWith(staleWhileRevalidate(request));
    return;
  }
  if (isManifest(url)) {
    event.respondWith(networkFirst(request));
    return;
  }

  event.respondWith(networkFirst(request, APP_SHELL));
});
