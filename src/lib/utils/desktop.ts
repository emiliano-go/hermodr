import type { ChatSummary, DesktopChatTarget } from "./wire.ts";

export function desktopUnreadCount(chats: ReadonlyArray<Pick<ChatSummary, "unread_count" | "marked_unread" | "archived">>): number {
  return chats.reduce((total, chat) => {
    if (chat.archived) return total;
    const count = Number.isFinite(chat.unread_count) ? Math.max(0, Math.floor(chat.unread_count)) : 0;
    return Math.min(0xffffffff, total + (count || (chat.marked_unread ? 1 : 0)));
  }, 0);
}

export function desktopChatTarget(value: unknown, activeAccount: string | null): DesktopChatTarget | null {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const target = value as Record<string, unknown>;
  if (!activeAccount || target.account_id !== activeAccount || typeof target.chat !== "string" || !target.chat || target.chat.length > 512 || /[\u0000-\u001f]/.test(target.chat)) return null;
  return { account_id: activeAccount, chat: target.chat };
}
