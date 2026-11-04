import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed dev port and must not have it silently moved.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // src-tauri is Cargo's domain; watching it causes needless reloads.
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "chrome110", // WebView2 on Win10+ is well past this
    sourcemap: true,
  },
});
