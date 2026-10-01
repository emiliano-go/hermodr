import type { ChatSummary, Label } from "./wire";
import { isChatMuted } from "./notifications.ts";

export type InboxLabel = Pick<Label, "id" | "name">;
export type InboxAction =
  | { kind: "read"; read: boolean }
  | { kind: "archive"; archived: boolean }
  | { kind: "mute"; seconds: number }
  | { kind: "label"; label: string; applied: boolean };
export type InboxFilters = { unread: boolean; mentions: boolean; labelled: boolean; muted: boolean; archived: boolean; label: string; query: string };

export function inboxCategories(chat: ChatSummary, labels: readonly string[], nowSec = Math.floor(Date.now() / 1000)) {
  return { unread: chat.unread_count > 0 || chat.marked_unread, mentions: chat.mention_count > 0,
    labelled: labels.length > 0, muted: isChatMuted(chat.muted_until, nowSec), archived: chat.archived };
}

export function inboxChats(chats: ChatSummary[], filters: InboxFilters, labelsByChat: Readonly<Record<string, readonly string[]>>,
  labelOf: (chat: ChatSummary) => string, nowSec = Math.floor(Date.now() / 1000)): ChatSummary[] {
  const query = filters.query.trim().toLocaleLowerCase();
  return chats.filter((chat) => {
    const labels = labelsByChat[chat.chat] ?? [];
    const categories = inboxCategories(chat, labels, nowSec);
    const selected = (Object.keys(categories) as (keyof typeof categories)[]).filter((kind) => filters[kind]);
    return (selected.length ? selected.every((kind) => categories[kind]) : Object.values(categories).some(Boolean))
      && (!filters.label || labels.includes(filters.label))
      && (!query || labelOf(chat).toLocaleLowerCase().includes(query) || chat.chat.toLocaleLowerCase().includes(query));
  });
}
