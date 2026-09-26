// Chats domain: the conversation list, selection, search, pins, avatars and
// the group-info panel. Moved out of +page.svelte. Reads members (names) and
// session (self, accounts); cross-domain flows (openChat) stay in the route.
import { invoke } from "$lib/ipc";
import { plain } from "$lib/format";
import { bare, MEDIA_LABELS } from "$lib/message";
import type { IconName } from "$lib/Icon.svelte";
import type {
  Account,
  ChatFilter,
  ChatSummary,
  GroupInfo,
  SearchResult,
} from "$lib/models";
import { members } from "./members.svelte";
import { session } from "./session.svelte";
import { ui } from "./ui.svelte";

function bareJid(jid: string) {
  return jid.replace(/@.*$/, "");
}

export class ChatsState {
  chats: ChatSummary[] = $state([]);
  /** Chat list only: cheap, local, never blocks on the network. */
  chatsSeq = 0;
  selectedChat = $state<string | null>(null);
  titleOverride = $state<string | null>(null);
  chatFilter = $state<ChatFilter>("all");

  searchQuery = $state("");
  searchResults = $state<SearchResult[]>([]);

  /** Communities and their subgroups among our groups, for the chat list. */
  groupKinds = $state<
    Record<string, { community: boolean; announcements: boolean; parent: string | null }>
  >({});

  showGroupInfo = $state(false);
  groupInfo = $state<GroupInfo | null>(null);
  groupInfoError = $state<string | null>(null);

  /** Cached profile picture per chat; `null` means it has none. */
  avatars: Record<string, string | null> = $state({});
  requestedAvatars = new Set<string>();
  avatarQueue: string[] = [];
  avatarWorkers = 0;

  /** Coalesces an event burst into at most one chat-list reload per 200 ms. */
  chatsQueued = false;

  visibleChats = $derived(
    this.chats.filter((c) =>
      this.chatFilter === "all"
        ? true
        : this.chatFilter === "unread"
          ? c.unread_count > 0
          : c.chat.endsWith("@g.us"),
    ),
  );
  unreadChats = $derived(this.chats.filter((c) => c.unread_count > 0).length);
  unreadPings = $derived(this.chats.reduce((n, c) => n + c.mention_count, 0));

  /** Each account's own picture as last seen, so it shows before that account connects. */
  accountAvatars = $derived(
    Object.fromEntries(
      session.accountList.map((a) => {
        const jid = a.id === session.activeAccount ? (session.me ?? a.jid) : a.jid;
        return [a.id, (jid ? this.avatars[jid] : null) ?? this.rememberedAvatar(a.id)];
      }),
    ) as Record<string, string | null>,
  );

  chatLabel(chat: ChatSummary) {
    return members.displayName(chat.display_name, chat.chat);
  }

  /** Who sent a chat's last message, as the list prefixes it. */
  previewAuthor(chat: ChatSummary) {
    if (chat.last_from_me) return "You";
    if (!chat.chat.endsWith("@g.us")) return null;
    return members.displayName(chat.last_sender_name, chat.last_sender);
  }

  /** The last message's text; media without a caption reads as its kind. */
  previewText(chat: ChatSummary) {
    const kind = chat.last_media_kind;
    if (kind === "poll") return `📊 ${chat.last_text}`;
    if (kind === "event") return `📅 ${chat.last_text}`;
    if (kind === "view_once") return "View once message";
    if (!kind || chat.last_text.trim() !== `[${kind}]`) {
      return plain(chat.last_text, (user) => members.mentionName(user));
    }
    return MEDIA_LABELS[kind] ?? "Attachment";
  }

  mediaIcon(kind: string | null): IconName | null {
    if (kind === "image" || kind === "sticker") return "image";
    if (kind === "video" || kind === "gif") return "video";
    if (kind === "audio") return "mic";
    if (kind === "document") return "file";
    return null;
  }

  /** The display name of a chat, for cross chat quotes. */
  chatName(jid: string) {
    return this.chats.find((c) => c.chat === jid)?.display_name ?? bareJid(jid);
  }

  async refreshChats() {
    const seq = ++this.chatsSeq;
    try {
      const next = await invoke<ChatSummary[]>("chats");
      // A slow response must not overwrite a newer list.
      if (seq !== this.chatsSeq) return;
      this.chats = next;
    } catch (e) {
      ui.fail(e);
    }
  }

  queueRefreshChats() {
    if (this.chatsQueued) return;
    this.chatsQueued = true;
    setTimeout(() => {
      this.chatsQueued = false;
      void this.refreshChats();
    }, 200);
  }

  /** Runs the chat/contact/group search. */
  async runSearch() {
    const query = this.searchQuery.trim();
    if (!query) {
      this.searchResults = [];
      return;
    }
    try {
      this.searchResults = await invoke<SearchResult[]>("search", { query });
    } catch (e) {
      ui.fail(e);
    }
  }

  clearSearch() {
    this.searchQuery = "";
    this.searchResults = [];
  }

  /** Pins or unpins a chat, mirrored to the account. */
  async togglePin(chat: ChatSummary, event?: MouseEvent) {
    event?.stopPropagation();
    try {
      await invoke("set_pinned", { chat: chat.chat, pinned: !chat.pinned });
      await this.refreshChats();
    } catch (e) {
      ui.fail(e);
    }
  }

  async loadGroupKinds() {
    try {
      this.groupKinds = await invoke("group_kinds");
    } catch {
      // Not connected yet; the next connection loads them.
    }
  }

  /** Opens the right sidebar with the group's subject, description and members. */
  async openGroupInfo() {
    const selectedChat = this.selectedChat;
    if (!selectedChat) return;
    this.showGroupInfo = true;
    this.groupInfo = null;
    this.groupInfoError = null;
    try {
      this.groupInfo = await invoke<GroupInfo>("group_info", { chat: selectedChat });
    } catch (e) {
      // The query can time out on a busy server; keep the panel open so the
      // failure is visible and retryable rather than looking like a dead click.
      this.groupInfoError = String(e);
    }
  }

  /** Fetches a profile picture once, a few requests at a time. */
  loadAvatar(jid: string) {
    if (this.requestedAvatars.has(jid)) return;
    this.requestedAvatars.add(jid);
    this.avatarQueue.push(jid);
    while (this.avatarWorkers < 4 && this.avatarQueue.length > 0) void this.avatarWorker();
  }

  async avatarWorker() {
    this.avatarWorkers++;
    try {
      for (let jid = this.avatarQueue.shift(); jid; jid = this.avatarQueue.shift()) {
        try {
          this.avatars[jid] = await invoke<string | null>("avatar", { jid });
        } catch {
          this.avatars[jid] = null;
        }
      }
    } finally {
      this.avatarWorkers--;
    }
  }

  /** A JID's cached picture, fetching it on first use. */
  pictureOf(jid: string) {
    this.loadAvatar(jid);
    return this.avatars[jid] ?? null;
  }

  /** Drops one cached picture so the next read fetches it again. */
  forgetAvatar(jid: string) {
    this.requestedAvatars.delete(jid);
    delete this.avatars[jid];
  }

  rememberedAvatar(id: string): string | null {
    try {
      return localStorage.getItem(`hermodr.avatar.${id}`);
    } catch {
      return null;
    }
  }

  /** Mirrors resetUi: list, selection, pictures and the group panel are dropped. */
  resetAccount() {
    this.chats = [];
    this.avatars = {};
    this.requestedAvatars.clear();
    this.selectedChat = null;
    this.groupInfo = null;
    this.showGroupInfo = false;
  }
}

export const chats = new ChatsState();

export type { Account };
