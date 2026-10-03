import { LocalizedError } from "../i18n/errors.ts";
export const NOTIFICATION_HISTORY_LIMIT = 100;

export type NotificationHistoryEntry = {
  chat: string;
  id: string;
  sender: string;
  chat_name: string;
  sender_name: string;
  title: string;
  body: string;
  timestamp: number;
};

export type HistoryStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;
export type HistorySnapshot = { entries: NotificationHistoryEntry[]; error: LocalizedError | null; writable: boolean };

export function historyKey(account: string) { return `postal.notification-history.v1.${encodeURIComponent(account)}`; }

function entryOf(value: unknown): NotificationHistoryEntry | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const row = value as Record<string, unknown>;
  for (const field of ["chat", "id", "sender", "chat_name", "sender_name", "title", "body"]) {
    if (typeof row[field] !== "string") return null;
  }
  if (!row.chat || !row.id || (row.chat as string).length > 512 || (row.id as string).length > 512) return null;
  if (!Number.isSafeInteger(row.timestamp) || (row.timestamp as number) < 0 || (row.timestamp as number) > 8_640_000_000_000_000) return null;
  return {
    chat: row.chat as string, id: row.id as string, sender: (row.sender as string).slice(0, 512),
    chat_name: (row.chat_name as string).slice(0, 256), sender_name: (row.sender_name as string).slice(0, 256),
    title: (row.title as string).slice(0, 512), body: (row.body as string).slice(0, 1024), timestamp: row.timestamp as number,
  };
}

function capped(entries: NotificationHistoryEntry[]): NotificationHistoryEntry[] {
  const seen = new Set<string>();
  return [...entries].sort((left, right) => right.timestamp - left.timestamp || left.chat.localeCompare(right.chat) || left.id.localeCompare(right.id))
    .filter((entry) => {
      const key = JSON.stringify([entry.chat, entry.id]);
      if (seen.has(key)) return false;
      seen.add(key); return true;
    }).slice(0, NOTIFICATION_HISTORY_LIMIT);
}

export function loadHistory(account: string, storage: HistoryStorage): HistorySnapshot {
  try {
    const raw = storage.getItem(historyKey(account));
    if (raw === null) return { entries: [], error: null, writable: true };
    const value = JSON.parse(raw);
    if (value?.version !== 1 || value.account !== account || !Array.isArray(value.entries)) throw new Error();
    const entries = value.entries.map(entryOf);
    if (entries.some((entry: NotificationHistoryEntry | null) => !entry)) throw new Error();
    return { entries: capped(entries), error: null, writable: true };
  } catch {
    return { entries: [], error: new LocalizedError({ kind: "postal_error", code: "error.content.saved_notification_history_could_not_be_read_new_entries_are_temporary_c", params: {} }), writable: false };
  }
}

export function appendHistory(entries: NotificationHistoryEntry[], candidate: NotificationHistoryEntry): NotificationHistoryEntry[] {
  const entry = entryOf(candidate);
  if (!entry) return entries;
  if (entries.some((known) => known.chat === entry.chat && known.id === entry.id)) return entries;
  return capped([entry, ...entries]);
}

export function saveHistory(account: string, entries: NotificationHistoryEntry[], storage: HistoryStorage): HistorySnapshot {
  const safe = capped(entries.map(entryOf).filter((entry): entry is NotificationHistoryEntry => entry !== null));
  try {
    storage.setItem(historyKey(account), JSON.stringify({ version: 1, account, entries: safe }));
    return { entries: safe, error: null, writable: true };
  } catch {
    return { entries: safe, error: new LocalizedError({ kind: "postal_error", code: "error.content.notification_history_could_not_be_saved_recent_entries_are_kept_only_unt", params: {} }), writable: true };
  }
}

export function clearHistory(account: string, previous: HistorySnapshot, storage: HistoryStorage): HistorySnapshot {
  try {
    storage.removeItem(historyKey(account));
    return { entries: [], error: null, writable: true };
  } catch {
    return { ...previous, error: new LocalizedError({ kind: "postal_error", code: "error.content.notification_history_could_not_be_cleared_try_again_when_local_storage_i", params: {} }) };
  }
}
