import type { FloatContext, MessagePage, StoredMessage } from "./wire.ts";
import { plain } from "./format.ts";
import { compareMessages, cursorOf } from "./message-window.ts";
import { t } from "../i18n/localizer.ts";
import { captionOf, MEDIA_LABELS, CARD_LABELS } from "./message.ts";

export const FLOAT_HISTORY_LIMIT = 500;

export function floatContent(message: StoredMessage) {
  if (message.system_kind === "UNAVAILABLE_MESSAGE") return { text: `${t("message.unavailable")}. ${t("message.unavailable_explanation")}`, media: null, notice: true };
  if (message.deleted || message.revoked) return { text: t("message.deleted"), media: null, notice: true };
  if (message.spoiler) return { text: t("message.spoiler"), media: null, notice: true };
  if (message.media_kind === "view_once" || message.media_once_kind) return { text: "", media: t("message.view_once"), notice: false };
  if (message.system_kind) return { text: t("message.system_notice"), media: null, notice: true };
  const kind = message.media_kind;
  return { text: plain(captionOf(message)), media: kind ? MEDIA_LABELS[kind] ?? CARD_LABELS[kind] ?? (kind === "poll" ? t("message.poll") : kind === "event" ? t("message.event") : t("message.unsupported")) : null, notice: false };
}

export function mergeFloatPage(rows: readonly StoredMessage[], page: MessagePage, chat: string, older = false): StoredMessage[] {
  const incoming = page.messages.filter((row) => row.chat === chat);
  const combined = older ? [...incoming, ...rows] : incoming;
  const byId = new Map(combined.filter((row) => row.chat === chat).map((row) => [row.id, row]));
  return [...byId.values()].sort((a, b) => compareMessages(cursorOf(a), cursorOf(b))).slice(-FLOAT_HISTORY_LIMIT);
}

export function floatDraftKey(context: Pick<FloatContext, "account_id" | "chat">): string {
  return `postal.floatDraft.${JSON.stringify([context.account_id, context.chat])}`;
}

export function readFloatDraft(storage: Pick<Storage, "getItem">, context: Pick<FloatContext, "account_id" | "chat">): string {
  return storage.getItem(floatDraftKey(context)) ?? "";
}

export function writeFloatDraft(storage: Pick<Storage, "setItem" | "removeItem">, context: Pick<FloatContext, "account_id" | "chat">, text: string) {
  const key = floatDraftKey(context);
  if (text) storage.setItem(key, text); else storage.removeItem(key);
}
