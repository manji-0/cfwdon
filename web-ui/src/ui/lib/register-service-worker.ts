export const registerWebUiServiceWorker = (): void => {
  if (!import.meta.env.PROD || !("serviceWorker" in navigator)) {
    return;
  }
  void navigator.serviceWorker.register("/app/sw.js", {
    scope: "/app/",
    updateViaCache: "none",
  });
};
