import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// DotWallpaper UI - Vue 3 + Vite 5（独立于根目录的 Tauri 前端工程）
// devUrl 固定端口 1420，与 src-tauri/tauri.conf.json 保持一致
export default defineConfig({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  server: {
    port: 1420,
    strictPort: true,
    clearScreen: false,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
