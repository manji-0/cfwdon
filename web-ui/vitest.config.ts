import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    environment: "node",
    // Persist module transforms under node_modules/.vite/vitest between runs.
    fsModuleCache: true,
    include: ["src/**/*.test.{ts,tsx}"],
  },
  resolve: {
    alias: {
      "@": new URL("./src", import.meta.url).pathname,
    },
  },
});
