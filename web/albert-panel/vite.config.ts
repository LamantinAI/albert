import { readFileSync } from "node:fs";

import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { defineConfig } from "vitest/config";

// Emit the exact locked distribution. No CDN, source maps or remote validator.
function swaggerAssets(): Plugin {
  const resources = [
    ["swagger-ui-bundle.js", "swagger-ui-bundle.js", "text/javascript"],
    ["swagger-ui.css", "swagger-ui.css", "text/css"],
    ["LICENSE", "swagger-ui-LICENSE.txt", "text/plain"],
  ].map(([source, name, type]) => ({
    name,
    type,
    bytes: readFileSync(
      new URL(`./node_modules/swagger-ui-dist/${source}`, import.meta.url),
    ),
  }));
  for (const name of ["index.html", "bootstrap.js"]) {
    resources.push({
      name,
      type: name.endsWith("html") ? "text/html" : "text/javascript",
      bytes: readFileSync(
        new URL(`../../connectors/web/swagger/${name}`, import.meta.url),
      ),
    });
  }
  return {
    name: "albert-swagger-assets",
    generateBundle() {
      for (const resource of resources)
        this.emitFile({
          type: "asset",
          fileName: `docs/${resource.name}`,
          source: resource.bytes,
        });
    },
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const resource = resources.find((entry) =>
          ["/ui/docs/", "/docs/"].some(
            (prefix) =>
              request.url?.split("?")[0] === `${prefix}${entry.name}` ||
              (entry.name === "index.html" &&
                request.url?.split("?")[0] === prefix),
          ),
        );
        if (!resource) return next();
        response.setHeader("Content-Type", resource.type);
        response.end(resource.bytes);
      });
    },
  };
}

export default defineConfig({
  plugins: [react(), swaggerAssets()],
  base: "/ui/",
  server: {
    watch: process.env.ALBERT_DEV_POLLING
      ? { usePolling: true, interval: 500 }
      : undefined,
    proxy: {
      "/api": {
        target: process.env.ALBERT_API_ORIGIN || "http://127.0.0.1:9877",
        changeOrigin: false,
      },
    },
  },
  build: { sourcemap: false, assetsInlineLimit: 0, target: "es2022" },
  test: {
    environment: "jsdom",
    setupFiles: ["./tests/setup.ts"],
    include: ["src/**/*.test.{ts,tsx}"],
    restoreMocks: true,
  },
});
