import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// DotWallpaper UI - Vue 3 + Vite 5（由原生 WKWebView 承载）
// 开发服务器固定端口，便于浏览器调试；发布构建使用内联 file:// 资源
export default defineConfig({
  // WKWebView loads the bundle from file://; assets must be relative.
  base: "./",
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
