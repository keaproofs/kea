import { Agent } from "node:http";
import { fileURLToPath } from "node:url";
import { defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";

// In development the API runs on the Kea server, reached through an SSH tunnel on 127.0.0.1:8740.
// Each new connection through the tunnel costs extra round trips to the server, so the proxy keeps
// a few open and reuses them.
const target = process.env.KEA_API ?? "http://127.0.0.1:8740";
const agent = new Agent({ keepAlive: true, maxSockets: 6 });
const api: ProxyOptions = { target, agent };
// The burn page's JSON: from a local `python3 -m http.server --bind 127.0.0.1 5175` over a kea-burns --out folder,
// or KEA_DATA=https://keaproofs.io for the live files.
const data: ProxyOptions = { target: process.env.KEA_DATA ?? "http://127.0.0.1:5175", changeOrigin: true };
const proxy = { "/v1": api, "/badge": api, "/feed": api, "/feed.xml": api, "/data/": data };

export default defineConfig({
  plugins: [react()],
  build: {
    // The 3D scene ships as one chunk the page loads after its first paint.
    chunkSizeWarningLimit: 650,
    // burn.html is the same app under /burn's own title and share card (nginx serves it for /burn): one bundle, two
    // HTML entries.
    rolldownOptions: {
      input: {
        index: fileURLToPath(new URL("index.html", import.meta.url)),
        burn: fileURLToPath(new URL("burn.html", import.meta.url)),
      },
      // Shared by both pages, the bundle would be named after main.tsx; it keeps the index-* name it has always had,
      // which the launch deploy's read-back looks for.
      output: {
        chunkFileNames: (chunk) => `assets/${chunk.name === "main" ? "index" : "[name]"}-[hash].js`,
        assetFileNames: (asset) => `assets/${asset.names.includes("main.css") ? "index" : "[name]"}-[hash][extname]`,
      },
    },
  },
  server: { port: 5173, strictPort: true, proxy },
  preview: { port: 5173, strictPort: true, proxy },
});
