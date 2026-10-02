import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

const state = fileURLToPath(new URL("./theme-coverage-state.svelte.js", import.meta.url));
export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/theme-coverage", import.meta.url)),
  plugins: [svelte({ configFile: false })],
  optimizeDeps: { entries: ["theme-components.html"] },
  resolve: { alias: [
    ...["$lib/utils/ipc", "$lib/state/session.svelte", "$lib/state/transcription.svelte", "@tauri-apps/api/core", "@tauri-apps/api/event"]
      .map((find) => ({ find, replacement: state })),
    { find: "$lib", replacement: fileURLToPath(new URL("../../src/lib", import.meta.url)) },
  ] },
  server: { host: "127.0.0.1", port: 1439, strictPort: true },
});
