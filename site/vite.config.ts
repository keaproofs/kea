import { Agent } from "node:http";
import { defineConfig, type ProxyOptions } from "vite";
import react from "@vitejs/plugin-react";

// In development the API runs on the Kea server, reached through an SSH tunnel on 127.0.0.1:8740.
// Each new connection through the tunnel costs extra round trips to the server, so the proxy keeps
// a few open and reuses them.
const target = process.env.KEA_API ?? "http://127.0.0.1:8740";
const agent = new Agent({ keepAlive: true, maxSockets: 6 });
const api: ProxyOptions = { target, agent };
const proxy = { "/v1": api, "/badge": api, "/feed": api, "/feed.xml": api };

export default defineConfig({
  plugins: [react()],
  server: { port: 5173, strictPort: true, proxy },
  preview: { port: 5173, strictPort: true, proxy },
});
