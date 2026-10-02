import { invoke } from "$lib/utils/ipc";
import type { FoundItem, StoredMessage } from "$lib/utils/models";
import { chats } from "./chats.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { ui } from "./ui.svelte";
import { session } from "./session.svelte";
import { keywords } from "./keywords.svelte";
import { keywordHidden } from "$lib/utils/keywords";
import { compareMessages } from "$lib/utils/message-window";
import { labels } from "./labels.svelte";
import { labelSearch } from "$lib/utils/label-search";

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
let pingsRequest = 0;

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
  const account = session.activeAccount;
  if (!account) return;
  if (keywords.account !== account) keywords.load(account);
  ui.finder = { mode: "pings", chat, items: null };
  const opened = ui.finder, generation = messages.accountGeneration, revision = keywords.revision, request = ++pingsRequest;
  const rules = { highlight: [...keywords.rules.highlight], hide: [...keywords.rules.hide] };
  const current = () => ui.finder === opened && account === session.activeAccount && generation === messages.accountGeneration
    && revision === keywords.revision && request === pingsRequest;
  try {
    const [pings, matches] = await Promise.all([
      invoke<StoredMessage[]>("pings", { chat, mute_all_at_all: session.settings.mute_all_at_all ?? false }),
      rules.highlight.length ? invoke<StoredMessage[]>("keyword_matches", { accountId: account, chat, unreadOnly: false, ...rules }) : Promise.resolve([]),
    ]);
    if (!current()) return;
    const unique = new Map<string, StoredMessage>();
    for (const message of [...pings, ...matches]) {
      if (!keywordHidden(message, rules)) unique.set(JSON.stringify([message.chat, message.id]), message);
    }
    const got = [...unique.values()].sort((a, b) => compareMessages(b, a) || a.chat.localeCompare(b.chat)).slice(0, SEARCH_LIMIT);
    ui.finder = { ...opened, items: got.map((m) => found(m, chat === null)) };
  } catch (e) {
    if (current()) { ui.finder = null; ui.fail(e); }
  }
}

/**
 * Searches the open finder's chat. `more` first asks the phone for the
 * previous 24 hours of the chat, then searches again over everything kept.
 */
export async function searchChat(query: string, more = false) {
  let current = ui.finder;
  const chat = current?.chat;
  if (!current || !chat) return;
  if (more && chat === chats.selectedChat) await messages.recallDay(chat);
  if (ui.finder !== current) return;
  const reach = messages.messages.at(-1)?.timestamp ?? null;
  if (!query.trim()) {
    ui.finder = { ...current, items: [], query, reach, more: false };
    return;
  }
  if (!more) {
    // The finder is raw, so clearing rows replaces the object.
    current = { ...current, items: null };
    ui.finder = current;
  }
  try {
    const account = session.activeAccount;
    const generation = messages.accountGeneration;
    const filter = labelSearch(query);
    const ids = filter && labels.account === account ? labels.view.labels.filter((label) => label.name.toLocaleLowerCase() === filter.name.toLocaleLowerCase()).map((label) => label.id) : [];
    const got = filter
      ? ids.length ? await invoke<StoredMessage[]>("labelled_messages", { accountId: account, labelIds: ids, chat, query: filter.query, limit: SEARCH_LIMIT }) : []
      : await invoke<StoredMessage[]>("search_messages", { chat, query, limit: SEARCH_LIMIT });
    if (account !== session.activeAccount || generation !== messages.accountGeneration) return;
    if (ui.finder !== current) return;
    ui.finder = {
      ...current,
      items: got.map((m) => found(m, false)),
      query,
      reach,
      more: !messages.olderExhausted,
    };
  } catch (e) {
    if (ui.finder === current) ui.fail(e);
  }
}
