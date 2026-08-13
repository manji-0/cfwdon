const CACHE_NAME = "cfwdon-app-v1";
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
    path === "/app/logout"
  );
};

const isHashedAsset = (url) => url.pathname.startsWith("/app/assets/");

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

  if (isHashedAsset(url) || url.pathname.startsWith("/app/icons/")) {
    event.respondWith(
      caches.open(CACHE_NAME).then(async (cache) => {
        const cached = await cache.match(request);
        if (cached) {
          return cached;
        }
        const response = await fetch(request);
        if (response.ok) {
          await cache.put(request, response.clone());
        }
        return response;
      }),
    );
    return;
  }

  event.respondWith(
    fetch(request)
      .then((response) => {
        if (response.ok && url.pathname.startsWith("/app")) {
          const copy = response.clone();
          void caches.open(CACHE_NAME).then((cache) => cache.put(request, copy));
        }
        return response;
      })
      .catch(async () => {
        const cached = await caches.match(request);
        return cached ?? caches.match(APP_SHELL);
      }),
  );
});
