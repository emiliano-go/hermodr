import type { GalleryItem, StoredMessage } from "./wire";
import type { ViewerItem } from "../media/MediaViewer.svelte";

export function galleryDateRange(start: string, end: string) {
  const bound = (value: string, nextDay: boolean) => {
    if (!value) return null;
    const date = new Date(`${value}T00:00:00`);
    if (!Number.isFinite(date.getTime())) throw new Error("Choose a valid date range.");
    if (nextDay) date.setDate(date.getDate() + 1);
    return Math.floor(date.getTime() / 1000);
  };
  const since = bound(start, false);
  const until = bound(end, true);
  if (since !== null && until !== null && since >= until) throw new Error("End date must be on or after start date.");
  return { since, until };
}

export const galleryKey = (message: Pick<StoredMessage, "chat" | "id">) => JSON.stringify([message.chat, message.id]);
export const galleryVisible = (message: StoredMessage, revealed: ReadonlySet<string>) => !message.spoiler || revealed.has(galleryKey(message));

export function galleryUrl(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.href : null;
  } catch { return null; }
}

export function galleryViewerItems(items: GalleryItem[], revealed: ReadonlySet<string>, author: (message: StoredMessage) => string): ViewerItem[] {
  return items.flatMap(({ message }) => message.media_path && galleryVisible(message, revealed) &&
    ["image", "video", "round_video", "audio", "sticker", "gif"].includes(message.media_kind ?? "") ? [{
      id: galleryKey(message), path: message.media_path, thumb: message.media_thumb, kind: message.media_kind === "round_video" ? "video" : message.media_kind!,
      caption: message.text, author: author(message), avatar: null, timestamp: message.timestamp,
    }] : []);
}
