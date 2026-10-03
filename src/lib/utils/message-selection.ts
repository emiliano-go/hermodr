import type { StoredMessage } from "./wire";
import { isUnavailable } from "./message.ts";

export function loadedSelection(rows: StoredMessage[], hidden: (message: StoredMessage) => boolean): Record<string, StoredMessage> {
  return Object.fromEntries(rows.filter((message) => !message.deleted && !message.revoked && !isUnavailable(message) && !hidden(message))
    .map((message) => [message.id, message]));
}

export function pickLoaded(rows: StoredMessage[], selected: Record<string, StoredMessage> | null, anchor: string | null,
  target: StoredMessage, extend: boolean, hidden: (message: StoredMessage) => boolean) {
  const eligible = loadedSelection(rows, hidden);
  if (!Object.hasOwn(eligible, target.id)) return { selected, anchor };
  const next = { ...(selected ?? {}) };
  const from = rows.findIndex((message) => message.id === anchor), to = rows.findIndex((message) => message.id === target.id);
  if (extend && selected && from >= 0 && to >= 0) {
    for (const message of rows.slice(Math.min(from, to), Math.max(from, to) + 1)) {
      if (Object.hasOwn(eligible, message.id)) next[message.id] = message;
    }
    return { selected: next, anchor };
  }
  if (Object.hasOwn(next, target.id)) delete next[target.id]; else next[target.id] = target;
  return { selected: next, anchor: target.id };
}

export function selectionShortcut(event: Pick<KeyboardEvent, "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey" | "defaultPrevented" | "isComposing">, editable = false): "loaded" | "clear" | null {
  if (event.defaultPrevented || event.isComposing) return null;
  if (event.key === "Escape") return "clear";
  if (editable || event.altKey || event.shiftKey || !event.ctrlKey && !event.metaKey) return null;
  return event.key.toLowerCase() === "a" ? "loaded" : event.key.toLowerCase() === "d" ? "clear" : null;
}
