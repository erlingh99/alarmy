import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import path from "path";
import { visualizer } from "rollup-plugin-visualizer";
import { mockBackend } from "./mockBackend.ts";

const backendUrl = process.env.BACKEND_URL;

// https://vitejs.dev/config/
export default defineConfig(() => ({
  server: {
    host: "::",
    port: 8080,
    proxy: backendUrl ? { "/api": backendUrl } : undefined,
  },
  plugins: [
    react(),
    backendUrl ? null : mockBackend(),
    process.env.ANALYZE ? visualizer({ open: true, filename: "dist/stats.html" }) : null,
  ],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  build: {
    rollupOptions: {
      treeshake: {
        moduleSideEffects: false,
      },
      output: {
        manualChunks(id) {
          if (id.includes("node_modules/react") || id.includes("node_modules/react-dom") || id.includes("node_modules/react-router")) return "react";
          if (id.includes("node_modules/@tanstack/")) return "query";
        },
      },
    },
  }
}));
