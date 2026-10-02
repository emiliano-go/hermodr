import type { StickerResyncReport } from "./wire";

export type StickerScope = { account: string; chat: string | null; generation: number };

export function stickerScopeMatches(scope: StickerScope, account: string | null, chat: string | null, generation: number): boolean {
  return scope.account === account && scope.chat === chat && scope.generation === generation;
}

export function stickerResyncText(report: StickerResyncReport): string {
  const partial = report.mirror_verified !== true || report.catalog_complete !== true || report.app_state_synced !== true
    || report.app_state_retryable || report.app_state_fatal || !!report.app_state_error || !!report.pack_failures?.length || report.skipped_stickers > 0;
  const phase = report.app_state_fatal ? "Snapshot request failed." : report.app_state_retryable ? "Snapshot request needs retry."
    : report.app_state_synced ? "Snapshot request returned." : "Snapshot request unconfirmed.";
  return `${partial ? "Partial resync result. " : ""}${phase} Refetched ${report.packs}/${report.known_packs ?? "unknown"} known packs (${report.stickers} stickers); changed ${report.packs_changed ?? "unknown"} packs/${report.stickers_changed ?? "unknown"} stickers; skipped ${report.skipped_stickers ?? "unknown"} stickers.`;
}
