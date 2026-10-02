import { defineConfig } from "vite";
import base from "./album-rows.vite.config.ts";

export default defineConfig({ ...base, server: { host: "127.0.0.1", port: 1464, strictPort: true } });
