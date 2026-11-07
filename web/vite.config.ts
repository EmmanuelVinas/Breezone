import { resolve } from "node:path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// In development, the Breezone server runs on :8427 (see scripts/dev.sh).
const server = process.env.BREEZONE_DEV_SERVER ?? "http://localhost:8427"

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": resolve(import.meta.dirname, "./src"),
    },
  },
  server: {
    proxy: {
      "/api": { target: server, ws: true, changeOrigin: true },
      "/health": { target: server },
    },
  },
})
