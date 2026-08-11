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
              },
            ]),
          ),
        }
      : undefined,
  };
});
