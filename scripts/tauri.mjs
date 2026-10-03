import cli from "@tauri-apps/cli";
import { sqlcipherEnvironment } from "./sqlcipher-env.mjs";

Object.assign(process.env, sqlcipherEnvironment(process.env, process.platform, process.arch));
try { await cli.run(process.argv.slice(2), "pnpm run tauri"); }
catch (error) { console.error(error instanceof Error ? error.message : String(error)); process.exitCode = 1; }
