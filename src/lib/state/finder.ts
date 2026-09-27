import { invoke } from "$lib/ipc";
import type { FoundItem, StoredMessage } from "$lib/models";
import { chats } from "./chats.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";

export async function openStarred() {
  ui.showStarred = true;
  ui.starredItems = null;
  try {
    const starred = await invoke<StoredMessage[]>("starred_messages");
    ui.starredItems = starred.map((m) => ({
      chat: m.chat,
      id: m.id,
      where: chats.chatName(m.chat),
      author: m.from_me ? "You" : members.displayName(m.sender_name, m.sender),
      text: members.replyPreviewText(m),
      timestamp: m.timestamp,
      sender: m.sender,
      fromMe: m.from_me,
    }));
  } catch (e) {
    ui.showStarred = false;
    ui.fail(e);
  }
}

const SEARCH_LIMIT = 500;

function found(m: StoredMessage, across: boolean): FoundItem {
  return {
    chat: m.chat,
    id: m.id,
    where: across ? chats.chatName(m.chat) : null,
    author: m.from_me ? "You" : members.displayName(m.sender_name, m.sender),
    text: members.replyPreviewText(m),
    timestamp: m.timestamp,
    unread: !m.read && !m.from_me,
  };
}

export async function openPings(chat: string | null) {
  ui.finder = { mode: "pings", chat, items: null };
  try {
    const got = await invoke<StoredMessage[]>("pings", { chat });
    if (ui.finder?.mode === "pings" && ui.finder.chat === chat) {
      ui.finder.items = got.map((m) => found(m, chat === null));
    }
  } catch (e) {
    ui.finder = null;
    ui.fail(e);
  }
}

/**
 * Searches the open finder's chat. `more` first asks the phone for the
 * previous 24 hours of the chat, then searches again over everything kept.
 */
export async function searchChat(query: string, more = false) {
  const current = ui.finder;
  const chat = current?.chat;
  if (!current || !chat) return;
  if (more && chat === chats.selectedChat) await messages.recallDay(chat);
  if (ui.finder !== current) return;
  const reach = messages.messages.at(-1)?.timestamp ?? null;
  if (!query.trim()) {
    Object.assign(current, { items: [], query, reach, more: false });
    return;
  }
  if (!more) current.items = null;
  try {
    const got = await invoke<StoredMessage[]>("search_messages", { chat, query, limit: SEARCH_LIMIT });
    if (ui.finder !== current) return;
    Object.assign(current, {
      items: got.map((m) => found(m, false)),
      query,
      reach,
      more: !messages.olderExhausted,
    });
  } catch (e) {
    ui.fail(e);
  }
}
