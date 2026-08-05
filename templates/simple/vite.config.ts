import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// The dashboard talks to the Rust server on 9999. Proxying both the page and
// the socket through Vite keeps the browser on a single origin, so the Rust
// side needs no CORS handling.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
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
