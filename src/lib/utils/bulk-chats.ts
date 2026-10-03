import type { MarkReadResult } from "./wire";
import { normalizeError } from "../i18n/errors.ts";
import { t } from "../i18n/localizer.ts";
import { uiError } from "../state/localized.ts";

export function formatBulkReadFailures(results: MarkReadResult[], displayName: (chat: string) => string): string[] {
  return results.filter((result) => result.error !== null)
    .map((result) => t("page.bulk_read_failed_line", { chat: displayName(result.chat), reason: normalizeError(result.error).message }));
}

export function bulkReadError(results: MarkReadResult[], displayName: (chat: string) => string) {
  const failed = results.filter((result) => result.error !== null);
  if (!failed.length) return null;
  const diagnostic = failed.map((result) => {
    const failure = normalizeError(result.error);
    return `${displayName(result.chat)}: ${failure.diagnostic ?? JSON.stringify(failure.descriptor)}`;
  }).join("\n");
  const failure = uiError("error.page.bulk_read_failed", { count: failed.length }, diagnostic);
  Object.defineProperty(failure, "message", { get: () => formatBulkReadFailures(failed, displayName).join("\n") });
  return failure;
}
