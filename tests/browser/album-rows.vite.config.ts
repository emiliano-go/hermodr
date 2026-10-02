import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import base from "./vite.config.ts";

export default defineConfig({
  ...base,
  resolve: { alias: [
    { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("./album-rows-ipc.ts", import.meta.url)) },
    { find: "$lib", replacement: fileURLToPath(new URL("../../src/lib", import.meta.url)) },
  ] },
  server: { host: "127.0.0.1", port: 1465, strictPort: true },
});
