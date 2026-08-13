import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "@/ui/App";
import { registerWebUiServiceWorker } from "@/ui/lib/register-service-worker";

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root element");
}

registerWebUiServiceWorker();

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
