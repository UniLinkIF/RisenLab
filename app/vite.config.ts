import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { risenlabDevApi } from "./vite-dev-api";

// Tauri expects a fixed dev server port (see src-tauri/tauri.conf.json's devUrl) and needs
// the dev server to keep running even if the Rust side fails to build, so it doesn't clear
// the screen on every reload.
export default defineConfig({
  plugins: [react(), risenlabDevApi()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  test: {
    environment: "node",
  },
});
