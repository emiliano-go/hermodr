// Native permission prompts are required; WebView Notifications cannot request them.
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { invoke } from "@tauri-apps/api/core";
import { plain } from "./format.ts";
import { MEDIA_LABELS, captionOf } from "./message.ts";
import type { StoredMessage } from "./models.ts";
import { t } from "../i18n/localizer.ts";

/**
 * How old a message may be and still ping. `fresh` only means "arrival-shaped";
 * a startup drain or redelivery replays old rows as fresh, and those must stay
 * silent instead of spamming a notification per backlog message.
 */
export const NOTIFY_RECENT_SECONDS = 300;

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
  /** When the message was sent, Unix seconds. A replay older than the window stays silent. */
  sentAt: number;
  /** The message mentioned us only via @all. */
  mentionedAllOnly?: boolean;
  /** The chat mutes @all mentions; direct mentions still ping. */
  muteAtAll?: boolean;
};

/**
 * Single gate for every notification path: muted chats and the global toggle
 * stay silent, and only genuinely recent arrivals ping (a future timestamp,
 * from clock skew, counts as recent). A chat muting @all stays silent for
 * @all-only mentions; direct mentions still ping.
 */
export function shouldNotify(d: NotifyDecision, nowSec = Math.floor(Date.now() / 1000)): boolean {
  if (!d.notificationsEnabled) return false;
  if (d.fromMe) return false;
  if (!d.fresh) return false;
  if (nowSec - d.sentAt > NOTIFY_RECENT_SECONDS) return false;
  if (d.revoked || d.systemKind) return false;
  if (d.isOpenChat) return false;
  if (isChatMuted(d.mutedUntil, nowSec)) return false;
  if (d.mentionedAllOnly && d.muteAtAll) return false;
  return true;
}

/** Short preview of the message, the same wording the chat list shows. */
export function notificationBody(
  message: Pick<StoredMessage, "text" | "media_kind"> & { spoiler?: boolean },
  mentionName: (user: string) => string = (u) => u,
): string {
  if (message.spoiler) return t("content.spoiler_message");
  const kind = message.media_kind;
  const text = message.text ?? "";
  if (kind === "poll") return `📊 ${plain(text, mentionName)}`.trim();
  if (kind === "event") return `📅 ${plain(text, mentionName)}`.trim();
  if (kind === "view_once") return t("state.view_once");
  if (kind === "missed_call") return t("state.missed_call");
  if (kind && text.trim() === `[${kind}]`) return MEDIA_LABELS[kind] ?? t("state.attachment");
  const body = plain(text || captionOf(message as StoredMessage), mentionName).trim();
  if (body) return body.length > 300 ? `${body.slice(0, 297)}…` : body;
  if (kind) return MEDIA_LABELS[kind] ?? t("state.attachment");
  return t("state.new_message");
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

/** OS permission state for desktop notifications. */
export type NotifPermission = "granted" | "denied" | "prompt" | "unsupported";

function webPermission(): NotifPermission {
  if (typeof Notification === "undefined") return "unsupported";
  if (Notification.permission === "granted") return "granted";
  if (Notification.permission === "denied") return "denied";
  return "prompt";
}

/** Reads the current permission without prompting. Never throws. */
export async function notificationPermission(): Promise<NotifPermission> {
  try {
    return (await isPermissionGranted()) ? "granted" : "prompt";
  } catch {
    return webPermission();
  }
}

/**
 * Asks the OS for permission. A `denied` answer is final until the user
 * re-enables Postal in the system settings: no API can re-prompt from
 * that state, so the settings UI shows unblock steps instead of retrying.
 */
export async function requestNotificationPermission(): Promise<NotifPermission> {
  try {
    return (await requestPermission()) ? "granted" : "denied";
  } catch {
    if (typeof Notification === "undefined") return "unsupported";
    if (Notification.permission === "granted") return "granted";
    try {
      return (await Notification.requestPermission()) === "granted" ? "granted" : "denied";
    } catch {
      return webPermission();
    }
  }
}

/** Shows one desktop notification. No-ops without permission. Never throws. */
export async function showChatNotification(
  title: string, body: string, chat: string, accountId: string, current: () => boolean,
): Promise<void> {
  if (typeof window === "undefined" || !current()) return;
  try {
    if (!(await isPermissionGranted())) return;
    if (!current()) return;
    await invoke("show_chat_notification", { accountId, chat, title, body });
  } catch {
    // Outside Tauri (synthetic browser harness): best-effort Web API fallback.
    if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
    let muted = false;
    try { muted = (await invoke<boolean | null>("chat_sound_muted", { accountId, chat })) ?? false; } catch {}
    if (!current()) return;
    try {
      const note = new Notification(title, { body, tag: `postal-${chat}`, silent: muted });
      note.onclick = () => {
        if (!current()) return;
        window.focus();
        window.dispatchEvent(new CustomEvent("postal:open-chat", { detail: { account_id: accountId, chat } }));
      };
    } catch {
      // Notifications are best-effort; the chat list already shows the message.
    }
  }
}

/** Pings once from the settings test button. Never throws. */
export async function sendTestNotification(): Promise<void> {
  try {
    await sendNotification({ title: "Postal", body: "Notifications are on." });
  } catch {
    // Best-effort only.
  }
}
