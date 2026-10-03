import { spawn } from "node:child_process";
import { sqlcipherEnvironment } from "./sqlcipher-env.mjs";

const child = spawn("cargo", process.argv.slice(2), {
  stdio: "inherit", env: sqlcipherEnvironment(process.env, process.platform, process.arch), windowsHide: true,
});
child.on("error", (error) => { console.error(error.message); process.exitCode = 1; });
child.on("exit", (code, signal) => { process.exitCode = code ?? (signal ? 1 : 0); });
