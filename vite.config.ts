import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// 前端源码在 ui/，产物在 ui/dist（tauri.conf.json 的 frontendDist）
export default defineConfig({
  root: "ui",
  plugins: [svelte()],
  clearScreen: false,
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  build: { outDir: "dist", emptyOutDir: true, target: "es2022" },
});
