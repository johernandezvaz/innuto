import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built files statically; the dev server only backs `tauri dev`.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: "es2021", outDir: "dist" },
});
