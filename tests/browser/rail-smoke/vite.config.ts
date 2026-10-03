import { defineConfig } from "vite";
import { fileURLToPath } from "node:url";
import base from "../album-rows.vite.config.ts";

export default defineConfig({ ...base, root: fileURLToPath(new URL(".", import.meta.url)),
  build: { outDir: fileURLToPath(new URL("../../../build/rail-smoke", import.meta.url)), emptyOutDir: true } });
