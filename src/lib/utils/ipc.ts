import { invoke as call, type InvokeArgs, type InvokeOptions } from "@tauri-apps/api/core";
import { createMediaAssetPreparer } from "./media-assets";

type Level = "error" | "warn" | "info" | "debug";

const prepareMedia = createMediaAssetPreparer((paths) => call<Record<string, string | null>>("authorize_media_assets", { paths }).catch((error) => {
  log(String(error) === "not connected yet" ? "debug" : "warn", `authorize_media_assets failed: ${error}`);
  throw error;
}));

/** Writes a line to postal.log under the `ui` target. Never throws. */
export function log(level: Level, message: string) {
  call("frontend_log", { level, message }).catch(() => {});
}

/** Tauri's `invoke`, logging each failure with the command's name (not its arguments). */
export async function invoke<T>(cmd: string, args?: InvokeArgs, options?: InvokeOptions): Promise<T> {
  try {
    return await prepareMedia(cmd, await call<T>(cmd, args, options));
  } catch (e) {
    // Account commands fail this way until the account connects; that is routine.
    log(String(e) === "not connected yet" ? "debug" : "warn", `${cmd} failed: ${e}`);
    throw e;
  }
}

window.addEventListener("error", (e) =>
  log("error", e.error instanceof Error ? (e.error.stack ?? e.message) : `${e.message} at ${e.filename}:${e.lineno}`),
);
document.addEventListener("securitypolicyviolation", (e) =>
  log("warn", `CSP blocked ${e.blockedURI || "inline content"} (${e.violatedDirective}) in ${e.sourceFile}:${e.lineNumber}`),
);
window.addEventListener("unhandledrejection", (e) =>
  log("error", `unhandled rejection: ${e.reason instanceof Error ? (e.reason.stack ?? e.reason) : e.reason}`),
);
