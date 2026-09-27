import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

export default defineConfig({
  root: fileURLToPath(new URL(".", import.meta.url)),
  plugins: [svelte({ configFile: false })],
  resolve: {
    alias: [
      { find: "$lib/ipc", replacement: fileURLToPath(new URL("./ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../../src/lib", import.meta.url)) },
    ],
  },
  server: { host: "127.0.0.1", port: 1432, strictPort: true },
});
