import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  // Tauri expects a fixed port and fails rather than silently using another.
  server: { port: 1420, strictPort: true },
  build: { target: "chrome110", outDir: "dist", emptyOutDir: true },
});
