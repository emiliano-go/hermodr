import type { StoredMessage } from "./wire";
import { isUnavailable } from "./message.ts";

export function latestUnreadCount(rows: StoredMessage[], visibleBoundary: string | null, chatUnread: number): number {
  const boundary = visibleBoundary === null ? -1 : rows.findIndex((message) => message.id === visibleBoundary);
  let loaded = 0;
  for (let index = boundary + 1; index < rows.length; index++) {
    const message = rows[index];
    if (!message.from_me && !message.read && !message.deleted && !message.revoked && !isUnavailable(message)) loaded++;
  }
  return Math.max(loaded, Number.isFinite(chatUnread) ? Math.max(0, Math.floor(chatUnread)) : 0);
}
