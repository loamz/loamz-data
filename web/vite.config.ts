import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

// In development the page and the Data API share one origin through this proxy, so the browser needs no CORS rule.
// In production the same is done by whatever serves the page (the Data server, or a Cloudflare route to it).
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: { proxy: { "/api": process.env.DATA_URL ?? "http://127.0.0.1:8020" } },
  test: { environment: "node" },
});
