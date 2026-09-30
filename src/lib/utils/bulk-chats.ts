import type { MarkReadResult } from "./wire";

export function formatBulkReadFailures(results: MarkReadResult[], displayName: (chat: string) => string): string[] {
  return results.filter((result) => result.error !== null).map((result) => `${displayName(result.chat)}: ${result.error}`);
}
