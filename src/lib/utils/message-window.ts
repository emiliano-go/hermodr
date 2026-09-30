import type { StoredMessage } from "./models";
import type { MessageCursor, MessagePage } from "./wire";
export type { MessageCursor, MessagePage } from "./wire";

export const DEFAULT_MESSAGE_WINDOW = 250;
export const MAX_MESSAGE_WINDOW = 2_000;
export const cursorOf = (message: StoredMessage): MessageCursor => ({ id: message.id, timestamp: message.timestamp, sort_order: message.sort_order });
export const compareMessages = (a: MessageCursor, b: MessageCursor) => a.timestamp - b.timestamp ||
  (a.sort_order ?? 0) - (b.sort_order ?? 0) || (a.id === b.id ? 0 : a.id < b.id ? -1 : 1);

export class MessageWindow {
  readonly limit: number;
  loaded: StoredMessage[] = [];

  constructor(limit = DEFAULT_MESSAGE_WINDOW) {
    if (!Number.isInteger(limit) || limit < 50 || limit > MAX_MESSAGE_WINDOW) throw new RangeError("Message window must contain 50–2,000 messages");
    this.limit = limit;
  }

  retain(rows: StoredMessage[], direction: "older" | "newer" = "newer"): StoredMessage[] {
    const byId = new Map(this.loaded.map((m) => [`${m.chat}\0${m.id}`, m]));
    for (const row of rows) byId.set(`${row.chat}\0${row.id}`, row);
    const ordered = [...byId.values()].sort((a, b) => compareMessages(b, a));
    this.loaded = direction === "older" ? ordered.slice(-this.limit) : ordered.slice(0, this.limit);
    return this.loaded;
  }

  replace(rows: StoredMessage[]): StoredMessage[] {
    this.evict();
    return this.retain(rows);
  }

  /** Replaces one loaded row, keeping every other row's identity. */
  patch(row: StoredMessage): StoredMessage[] {
    const at = this.loaded.findIndex((m) => m.chat === row.chat && m.id === row.id);
    if (at < 0) return this.loaded;
    const next = this.loaded.slice();
    next[at] = row;
    this.loaded = next;
    return this.loaded;
  }

  /** Adds a row in its sorted place, or replaces it when already loaded. */
  insert(row: StoredMessage): StoredMessage[] {
    if (this.loaded.some((m) => m.chat === row.chat && m.id === row.id)) return this.patch(row);
    const next = this.loaded.slice();
    // `loaded` is newest first: skip the rows newer than the one arriving.
    let at = 0;
    while (at < next.length && compareMessages(next[at], row) > 0) at++;
    next.splice(at, 0, row);
    this.loaded = next.length > this.limit ? next.slice(0, this.limit) : next;
    return this.loaded;
  }

  evict() { this.loaded = []; }
}
