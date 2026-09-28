// Desktop notifications for direct messages and groups.
//
// Pure helpers (`isChatMuted`, `shouldNotify`, `notificationTitle`,
// `notificationBody`) are unit-tested in `notifications.test.ts`. The thin
// sender at the bottom uses the Web Notification API so no new native
// dependency is needed; it works in the Tauri webview and in a browser.
import { plain } from "./format.ts";
import { MEDIA_LABELS, captionOf } from "./message.ts";
import type { StoredMessage } from "./models.ts";

/** A chat is muted when `muted_until` is -1 (forever) or still in the future. */
export function isChatMuted(mutedUntil: number, nowSec = Math.floor(Date.now() / 1000)): boolean {
  if (mutedUntil < 0) return true;
  if (!mutedUntil) return false;
  return mutedUntil > nowSec;
}

export type NotifyDecision = {
  /** Our own outgoing message; never notifies. */
  fromMe: boolean;
  /** System rows (group changes, security notices) and revokes stay silent. */
  systemKind: string | null;
  revoked: boolean;
  /** The chat's `muted_until`: -1 forever, 0 unmuted, else Unix seconds. */
  mutedUntil: number;
  /** Global kill switch from settings/notifications. */
  notificationsEnabled: boolean;
  /** History catch-up, not a live arrival. */
  fresh: boolean;
  /** The chat is currently open, so the message is already on screen. */
  isOpenChat: boolean;
};

/** Single gate for every notification path: muted chats and the global toggle stay silent. */
export function shouldNotify(d: NotifyDecision, nowSec = Math.floor(Date.now() / 1000)): boolean {
  if (!d.notificationsEnabled) return false;
  if (d.fromMe) return false;
  if (!d.fresh) return false;
  if (d.revoked || d.systemKind) return false;
  if (d.isOpenChat) return false;
  if (isChatMuted(d.mutedUntil, nowSec)) return false;
  return true;
}

/** Short preview of the message, the same wording the chat list shows. */
export function notificationBody(
  message: Pick<StoredMessage, "text" | "media_kind">,
  mentionName: (user: string) => string = (u) => u,
): string {
  const kind = message.media_kind;
  const text = message.text ?? "";
  if (kind === "poll") return `📊 ${plain(text, mentionName)}`.trim();
  if (kind === "event") return `📅 ${plain(text, mentionName)}`.trim();
  if (kind === "view_once") return "View once message";
  if (kind === "missed_call") return "Missed call";
  if (kind && text.trim() === `[${kind}]`) return MEDIA_LABELS[kind] ?? "Attachment";
  const body = plain(text || captionOf(message as StoredMessage), mentionName).trim();
  if (body) return body.length > 300 ? `${body.slice(0, 297)}…` : body;
  if (kind) return MEDIA_LABELS[kind] ?? "Attachment";
  return "New message";
}

/**
 * Who the notification is from. DMs read as the contact; groups read as the
 * group so the sender goes in the body instead (`sender: text`).
 */
export function notificationTitle(opts: {
  isGroup: boolean;
  chatName: string;
  senderName: string;
}): string {
  return opts.isGroup ? opts.chatName : opts.senderName || opts.chatName;
}

/** Body for a group message, prefixed with its sender. */
export function groupNotificationBody(senderName: string, body: string): string {
  return senderName ? `${senderName}: ${body}` : body;
}

/** Asks the OS for permission when it has not been decided yet. */
export async function ensureNotificationPermission(): Promise<NotificationPermission | null> {
  if (typeof Notification === "undefined") return null;
  if (Notification.permission === "default") {
    try {
      return await Notification.requestPermission();
    } catch {
      return Notification.permission;
    }
  }
  return Notification.permission;
}

/** Shows one desktop notification; clicking it opens the chat. No-ops without permission. */
export function showChatNotification(title: string, body: string, chat: string): void {
  if (typeof Notification === "undefined" || typeof window === "undefined") return;
  if (Notification.permission !== "granted") return;
  try {
    const note = new Notification(title, { body, tag: `postal-${chat}`, silent: false });
    note.onclick = () => {
      window.focus();
      window.dispatchEvent(new CustomEvent<string>("postal:open-chat", { detail: chat }));
    };
  } catch {
    // Notifications are best-effort; the chat list already shows the message.
  }
}
