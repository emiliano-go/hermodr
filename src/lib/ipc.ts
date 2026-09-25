import { invoke as call, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";

type Level = "error" | "warn" | "info" | "debug";

/** Writes a line to hermodr.log under the `ui` target. Never throws. */
export function log(level: Level, message: string) {
  call("frontend_log", { level, message }).catch(() => {});
}

/** Tauri's `invoke`, logging each failure with the command's name (not its arguments). */
export async function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  try {
    return await call<T>(cmd, args, options);
  } catch (e) {
    // Account commands fail this way until the account connects; that is routine.
    log(String(e) === "not connected yet" ? "debug" : "warn", `${cmd} failed: ${e}`);
    throw e;
  }
}

window.addEventListener("error", (e) =>
  log("error", e.error instanceof Error ? (e.error.stack ?? e.message) : `${e.message} at ${e.filename}:${e.lineno}`),
);
window.addEventListener("unhandledrejection", (e) =>
  log("error", `unhandled rejection: ${e.reason instanceof Error ? (e.reason.stack ?? e.reason) : e.reason}`),
);
