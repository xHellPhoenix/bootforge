import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Einstellungen nach Tauri-v2-Vorlage: fester Port, damit `tauri dev` den Server findet,
// und src-tauri nicht beobachten (sonst lädt Vite bei jeder Rust-Änderung neu).
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "es2022",
  },
});
