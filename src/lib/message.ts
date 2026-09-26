// Small message display helpers shared by the page and the message views.
// Moved out of +page.svelte.
import type { StoredMessage } from "./models";

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
  if (kind === "video") return "\u{1F3AC}";
  if (kind === "document") return "\u{1F4C4}";
  return "\u{1F4CE}";
}

/** Human-readable delivery state for a message we sent. */
export function statusMark(status: string | null) {
  switch (status) {
    case "pending":
      return "🕓";
    case "sent":
      return "✓";
    case "delivered":
      return "✓✓";
    case "read":
      return "✓✓";
    default:
      return "";
  }
}

/** A media message's caption. Uncaptioned media is stored as `[kind]`. */
export function captionOf(message: StoredMessage) {
  const text = message.text.trim();
  return text === `[${message.media_kind}]` ? "" : text;
}

export function dayKey(ts: number) {
  return new Date(ts * 1000).toDateString();
}

export function dayLabel(ts: number) {
  const day = new Date(ts * 1000);
  const today = new Date();
  const yesterday = new Date();
  yesterday.setDate(today.getDate() - 1);
  if (day.toDateString() === today.toDateString()) return "Today";
  if (day.toDateString() === yesterday.toDateString()) return "Yesterday";
  return day.toLocaleDateString(undefined, {
    day: "numeric",
    month: "long",
    year: day.getFullYear() === today.getFullYear() ? undefined : "numeric",
  });
}

export function formatTime(seconds: number) {
  return new Date(seconds * 1000).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

export const MEDIA_LABELS: Record<string, string> = {
  image: "Photo",
  video: "Video",
  gif: "GIF",
  audio: "Audio",
  document: "Document",
  sticker: "Sticker",
};

/** Kinds with a view of their own; anything else is drawn as a card. */
export const DRAWN_KINDS = new Set([
  "image",
  "video",
  "gif",
  "audio",
  "document",
  "sticker",
  "poll",
  "event",
  "view_once",
]);

export const CARD_LABELS: Record<string, string> = {
  location: "📍 Location",
  live_location: "📍 Live location",
  contact: "👤 Contact",
};

export const VIEW_ONCE_LABEL: Record<string, string> = {
  image: "Photo",
  video: "Video",
  audio: "Voice message",
};
