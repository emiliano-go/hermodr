// Small message display helpers shared by the page and the message views.
// Moved out of +page.svelte.
import type { StoredMessage } from "./models";
import { t, formatDate, formatRelative, formatTime as localeTime } from "../i18n/localizer.ts";

/** An SVG file sent as a document, which is drawn in place like a picture. */
export function isSvg(m: StoredMessage) {
  return m.media_kind === "document" && /\.svg$/i.test(m.media_path ?? m.text.split("\n")[0].trim());
}
/** A JID without its device suffix, the form pictures and names are keyed by. */
export function bare(jid: string) {
  return jid.replace(/:\d+(?=@)/, "");
}

export function replyIcon(kind: string | null) {
  if (kind === "audio") return "\u{1F3B5}";
  if (kind === "video" || kind === "round_video" || kind === "gif") return "\u{1F3AC}";
  if (kind === "document") return "\u{1F4C4}";
  if (kind === "sticker") return "\u{1F600}";
  if (kind === "image") return "\u{1F5BC}";
  return "\u{1F4CE}";
}

/** A media message's caption. Uncaptioned media is stored as `[kind]`. */
export function captionOf(message: StoredMessage) {
  if (message.spoiler) return t("message.spoiler_caption");
  const text = message.text.trim();
  return text === `[${message.media_kind}]` ? "" : text;
}

export function isUnavailable(message: Pick<StoredMessage, "system_kind">) {
  return message.system_kind === "UNAVAILABLE_MESSAGE";
}

export function dayKey(ts: number) {
  return new Date(ts * 1000).toDateString();
}

export function dayLabel(ts: number) {
  const day = new Date(ts * 1000);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (day.toDateString() === today.toDateString()) return formatRelative(0, "day");
  if (day.toDateString() === yesterday.toDateString()) return formatRelative(-1, "day");
  return formatDate(ts, {
    day: "numeric",
    month: "long",
    year: day.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

export function formatTime(seconds: number) {
  return localeTime(seconds, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function labels(keys: Record<string, string>): Record<string, string> {
  return Object.defineProperties(Object.create(null), Object.fromEntries(Object.entries(keys)
    .map(([kind, code]) => [kind, { enumerable: true, get: () => t(code) }])));
}

export const MEDIA_LABELS = labels({
  image: "media.photo", video: "media.video", round_video: "media.round_video", gif: "media.gif",
  audio: "media.audio", document: "media.document", sticker: "media.sticker",
});

/** Kinds with a view of their own; anything else is drawn as a card. */
export const DRAWN_KINDS = new Set([
  "image",
  "video",
  "round_video",
  "gif",
  "audio",
  "document",
  "sticker",
  "poll",
  "event",
  "view_once",
]);

export const CARD_LABELS = labels({
  album: "message.album", location: "message.location", live_location: "message.live_location",
  contact: "message.contact", music: "message.music", unknown: "message.unsupported",
});

export const VIEW_ONCE_LABEL = labels({
  image: "media.photo", video: "media.video", round_video: "media.round_video", audio: "media.voice",
  gif: "media.gif", sticker: "media.sticker",
});
