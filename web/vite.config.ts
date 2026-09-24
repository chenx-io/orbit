import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    rollupOptions: {
      output: {
        // 脚本/请求体编辑器（CodeMirror）独立分包：不增大首屏主包
        manualChunks: {
          codemirror: [
            "@codemirror/state",
            "@codemirror/view",
            "@codemirror/language",
            "@codemirror/commands",
            "@codemirror/lang-javascript",
            "@codemirror/lang-json",
            "@codemirror/lang-xml",
            "@codemirror/autocomplete",
            "@codemirror/lint",
            "@codemirror/theme-one-dark",
            "@lezer/highlight",
            "@lezer/json",
          ],
        },
      },
    },
  },
});
