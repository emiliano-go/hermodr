import type { StoredMessage } from "./models";

export const DEFAULT_MESSAGE_WINDOW = 500;
export const MAX_MESSAGE_WINDOW = 2_000;
export type MessageCursor = Pick<StoredMessage, "id" | "timestamp" | "sort_order">;
export type MessagePage = { messages: StoredMessage[]; has_more: boolean };
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

  evict() { this.loaded = []; }
}
