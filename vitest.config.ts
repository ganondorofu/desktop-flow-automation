import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Separate from vite.config.ts on purpose — that one's `server` block
// is tuned for `tauri dev` (fixed port, strictPort, watch excludes)
// and has nothing to do with running tests.
export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    css: false,
  },
});
