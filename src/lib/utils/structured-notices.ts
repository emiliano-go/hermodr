import type { StoredMessage } from "./wire";
import { formatDate, t } from "../i18n/localizer.ts";
import { noticeText } from "./notices.ts";

type NoticeMessage = Pick<StoredMessage, "media_kind" | "system_kind" | "system_params" | "text" | "sender" | "from_me"> & Partial<Pick<StoredMessage, "spoiler" | "revoked" | "deleted">>;

export function isPollNotice(message: Pick<NoticeMessage, "media_kind" | "system_kind" | "spoiler" | "revoked" | "deleted">): boolean {
  return (message.media_kind === "poll" && !message.system_kind || message.system_kind === "CHAT_POLL_CREATION_MESSAGE")
    && !message.spoiler && !message.revoked && !message.deleted;
}

export function structuredNoticeText(message: NoticeMessage, name: (jid: string) => string,
  isSelf: (jid: string) => boolean = (jid) => jid === "@me" || message.from_me && jid === message.sender): string | null {
  const resolve = (jid: string) => isSelf(jid) ? t("content.you") : name(jid);
  const actor = message.from_me ? t("content.you") : message.sender ? resolve(message.sender) : "";
  const titled = (base: string, value: string) => t("notice." + base + (actor ? "_by" : "") + (value?.trim() ? "_title" : ""), { actor, title: value });
  if (message.media_kind === "poll" && !message.system_kind || message.system_kind === "CHAT_POLL_CREATION_MESSAGE") {
    return isPollNotice(message) ? titled("poll", message.text) : null;
  }
  switch (message.system_kind) {
    case "EVENT_UPDATED": return titled("event_updated", message.text);
    case "EVENT_CANCELED": return titled("event_canceled", message.text);
    case "SCHEDULED_CALL_CREATED": {
      const [label, timestamp, type] = message.system_params;
      const at = Number(timestamp), date = new Date(at * 1000);
      const when = timestamp && Number.isSafeInteger(at) && at > 0 && !Number.isNaN(date.getTime())
        ? t("notice.call_when", { when: formatDate(at, { dateStyle: "medium", timeStyle: "short" }) }) : "";
      const kind = t(type === "voice" ? "notice.voice_call" : type === "video" ? "notice.video_call" : "notice.call");
      const call = label?.trim() ? t("notice.call_title", { call: kind, title: label }) : kind;
      return t(actor ? "notice.scheduled_call_by" : "notice.scheduled_call", { actor, call, when });
    }
    case "SCHEDULED_CALL_CANCEL": return titled("scheduled_canceled", message.text);
    case "SCHEDULED_CALL_START_MESSAGE": return t("notice.scheduled_started");
    default: return message.system_kind ? noticeText(message.system_kind, message.system_params, resolve, message.sender, isSelf) : null;
  }
}
