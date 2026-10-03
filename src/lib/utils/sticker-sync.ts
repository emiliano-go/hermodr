import type { StickerResyncReport } from "./wire";
import { t } from "../i18n/localizer.ts";

export type StickerScope = { account: string; chat: string | null; generation: number };

export function stickerScopeMatches(scope: StickerScope, account: string | null, chat: string | null, generation: number): boolean {
  return scope.account === account && scope.chat === chat && scope.generation === generation;
}

export function stickerResyncText(report: StickerResyncReport): string {
  const partial = report.mirror_verified !== true || report.catalog_complete !== true || report.app_state_synced !== true
    || report.app_state_retryable || report.app_state_fatal || !!report.app_state_error || !!report.pack_failures?.length || report.skipped_stickers > 0;
  const phase = report.app_state_fatal ? "sticker_sync.phase_failed" : report.app_state_retryable ? "sticker_sync.phase_retry"
    : report.app_state_synced ? "sticker_sync.phase_returned" : "sticker_sync.phase_unconfirmed";
  const unknown = t("sticker_sync.unknown");
  return t("sticker_sync.report", {
    prefix: partial ? `${t("sticker_sync.partial")} ` : "", phase: t(phase), packs: report.packs,
    known: report.known_packs ?? unknown, stickers: report.stickers, changedPacks: report.packs_changed ?? unknown,
    changedStickers: report.stickers_changed ?? unknown, skipped: report.skipped_stickers ?? unknown,
  });
}
