import { defineConfig } from "vite";

// Tauri expects a fixed port and must not be hidden behind the overlay.
export default defineConfig({
  clearScreen: false,
  // Ignore the Rust side: on Windows, watching a locked .exe in target/ crashes Vite (EBUSY).
  server: { port: 1420, strictPort: true, watch: { ignored: ["**/src-tauri/**"] } },
  envPrefix: ["VITE_", "TAURI_"],
  build: { target: "es2021", outDir: "dist", emptyOutDir: true },
});
