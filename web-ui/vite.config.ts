import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

const proxyPrefixes = ["/api", "/oauth", "/app/login", "/app/logout"] as const;

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const devOrigin = env.CFWDON_DEV_ORIGIN?.trim();

  return {
    plugins: [react()],
    base: "/app/",
    build: {
      outDir: "dist",
      emptyOutDir: true,
      assetsDir: "assets",
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (!id.includes("node_modules")) {
              return undefined;
            }
            if (id.includes("neverthrow")) {
              return "neverthrow";
            }
            if (id.includes("arktype")) {
              return "arktype";
            }
            if (
              id.includes("react-router") ||
              id.includes("react-dom") ||
              id.includes("/react/")
            ) {
              return "react-vendor";
            }
            return undefined;
          },
        },
      },
    },
    resolve: {
      alias: {
        "@": new URL("./src", import.meta.url).pathname,
      },
    },
    server: devOrigin
      ? {
          proxy: Object.fromEntries(
            proxyPrefixes.map((prefix) => [
              prefix,
              {
                target: devOrigin,
                changeOrigin: true,
                secure: devOrigin.startsWith("https://"),
                // Stream Hub DO path: browser WS → Vite → worker `/api/v1/streaming` upgrade.
                ws: prefix === "/api",
              },
            ]),
          ),
        }
      : undefined,
  };
});
