import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Talks to the same Rust server on 9999 as the simple template. Port differs
// so both dashboards can run side by side. The page and the socket share one
// origin through the proxy, so the Rust side needs no CORS handling.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5174,
    proxy: {
      "/ws": { target: "ws://127.0.0.1:9999", ws: true },
      "/health": "http://127.0.0.1:9999",
    },
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
