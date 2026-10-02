import type { ChatSummary } from "./models";

export function favoriteRows(chats: ChatSummary[], favorites: string[]): ChatSummary[] {
  const known = new Map(chats.map((chat) => [chat.chat, chat]));
  return [...new Set(favorites)].map((chat) => known.get(chat) ?? {
    chat, display_name: null, last_message_at: 0, last_text: "", last_from_me: false,
    last_sender_name: null, last_sender: "", last_media_kind: null, message_count: 0,
    unread_count: 0, mention_count: 0, pinned: false, archived: false,
    muted_until: 0, mute_at_all: false, marked_unread: false,
  });
}
