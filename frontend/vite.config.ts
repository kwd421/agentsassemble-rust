import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const apiTarget = process.env.AGENTSASSEMBLE_API_TARGET ?? "http://127.0.0.1:8765";

export default defineConfig({
  // Relative assets preserve the original browser UI while allowing the same
  // production bundle to load from Tauri's non-HTTP application origin.
  base: "./",
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: apiTarget,
        changeOrigin: true,
      },
    },
  },
});
