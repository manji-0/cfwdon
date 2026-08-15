import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv, type Plugin } from "vite";

const webUiRoot = fileURLToPath(new URL(".", import.meta.url));

const injectSwCacheVersion = (): Plugin => ({
  name: "inject-sw-cache-version",
  apply: "build",
  enforce: "post",
  closeBundle() {
    const dist = join(webUiRoot, "dist");
    const swPath = join(dist, "sw.js");
    const assetsDir = join(dist, "assets");
    const indexPath = join(dist, "index.html");
    if (!existsSync(swPath) || !existsSync(assetsDir) || !existsSync(indexPath)) {
      return;
    }
      const assetNames = readdirSync(assetsDir).sort().join("|");
      const indexHtml = readFileSync(indexPath);
      const version = createHash("sha256")
        .update(assetNames)
        .update(indexHtml)
        .digest("hex")
        .slice(0, 12);
      const sw = readFileSync(swPath, "utf8").replaceAll("__SW_CACHE_VERSION__", version);
      writeFileSync(swPath, sw);
    },
});

const proxyPrefixes = ["/api", "/oauth", "/app/login", "/app/logout"] as const;

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), "");
  const devOrigin = env.CFWDON_DEV_ORIGIN?.trim();

  return {
    plugins: [react(), injectSwCacheVersion()],
    base: "/app/",
    build: {
      outDir: "dist",
      emptyOutDir: true,
      assetsDir: "assets",
      rolldownOptions: {
        output: {
          codeSplitting: {
            groups: [
              { name: "neverthrow", test: /node_modules[/\\]neverthrow(?:[/\\]|$)/ },
              { name: "arktype", test: /node_modules[/\\]arktype(?:[/\\]|$)/ },
              {
                name: "react-vendor",
                test: /node_modules[/\\](?:react-router(?:-dom)?|react-dom|react)(?:[/\\]|$)/,
              },
            ],
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
