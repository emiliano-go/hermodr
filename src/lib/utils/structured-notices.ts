import type { StoredMessage } from "./wire";
import { noticeText } from "./notices.ts";

type NoticeMessage = Pick<StoredMessage, "media_kind" | "system_kind" | "system_params" | "text" | "sender" | "from_me"> & Partial<Pick<StoredMessage, "spoiler" | "revoked" | "deleted">>;

export function isPollNotice(message: Pick<NoticeMessage, "media_kind" | "system_kind" | "spoiler" | "revoked" | "deleted">): boolean {
  return (message.media_kind === "poll" && !message.system_kind || message.system_kind === "CHAT_POLL_CREATION_MESSAGE")
    && !message.spoiler && !message.revoked && !message.deleted;
}

export function structuredNoticeText(message: NoticeMessage, name: (jid: string) => string): string | null {
  const actor = message.from_me ? "You" : message.sender ? name(message.sender) : "";
  const title = (value: string | undefined) => value?.trim() ? ` "${value}"` : "";
  if (message.media_kind === "poll" && !message.system_kind || message.system_kind === "CHAT_POLL_CREATION_MESSAGE") {
    if (!isPollNotice(message)) return null;
    return actor ? `${actor} created a poll${title(message.text)}.` : `A poll${title(message.text)} was created.`;
  }
  switch (message.system_kind) {
    case "EVENT_UPDATED":
      return actor ? `${actor} updated the event${title(message.text)}.` : `The event${title(message.text)} was updated.`;
    case "EVENT_CANCELED":
      return actor ? `${actor} canceled the event${title(message.text)}.` : `The event${title(message.text)} was canceled.`;
    case "SCHEDULED_CALL_CREATED": {
      const [label, timestamp, type] = message.system_params;
      const at = Number(timestamp);
      const date = new Date(at * 1000);
      const when = timestamp && Number.isSafeInteger(at) && at > 0 && !Number.isNaN(date.getTime()) ? ` for ${date.toLocaleString()}` : "";
      const call = type === "voice" || type === "video" ? `${type} call` : "call";
      return actor ? `${actor} scheduled a ${call}${title(label)}${when}.` : `A ${call}${title(label)} was scheduled${when}.`;
    }
    case "SCHEDULED_CALL_CANCEL":
      return actor ? `${actor} canceled a scheduled call${title(message.text)}.` : `A scheduled call${title(message.text)} was canceled.`;
    case "SCHEDULED_CALL_START_MESSAGE":
      return "A scheduled call started.";
    default:
      return message.system_kind ? noticeText(message.system_kind, message.system_params, name, message.sender) : null;
  }
}
