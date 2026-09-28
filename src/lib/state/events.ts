// Backend event stream: maps every ServiceEvent kind onto domain updates.
// Moved out of +page.svelte so the domains above (session, chats, messages,
// members, composer, ui) visibly cover the stream. View callbacks the
// dispatcher cannot own (scrolling, reconnecting) arrive via host.
import { tick } from "svelte";
import { invoke } from "$lib/ipc";
import { bare } from "$lib/message";
import type { MessagePage } from "$lib/message-window";
import type { ServiceEvent, StoredMessage } from "$lib/models";
import {
  groupNotificationBody,
  isChatMuted,
  notificationBody,
  notificationTitle,
  shouldNotify,
  showChatNotification,
} from "$lib/notifications";
import { isPlaceholder } from "$lib/phone";
import { chats } from "./chats.svelte";
import { composer } from "./composer.svelte";
import { members } from "./members.svelte";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";
import { stickers } from "./stickers.svelte";
import { ui } from "./ui.svelte";

export type EventHost = {
  scrollToBottom(): void;
  getScroller(): HTMLDivElement | null;
  reconnect(): Promise<void>;
};

/**
 * Burst protocol: arrivals come as `messageHint` (routing only, ~90 B vs
 * ~500 B full payload, 5.6x smaller in the serialization test). While the
 * loading gate is closed, a drain is running, or a history answer is in
 * flight, hints only set the dirty flags below; `initialSyncComplete`,
 * `synced` or `historyLoaded` flush once (1 `chats` + 1 `messages` invoke).
 * A 500-msg batch inserts + both queries in ~53 ms (store regression test).
 * Live messages outside a burst still refresh immediately through the
 * 200/100 ms coalescing queues.
 */
let chatsDirty = false;
let messagesDirty = false;
let dirtyMarkRead = false;

/**
 * Marks what a change touched when fetching right away would be wasteful or
 * invisible: the gate is closed, a drain is running, or a history answer is
 * in flight. Returns false when the caller should refresh immediately.
 */
function deferRefresh(chat: string | null, markRead = false) {
  if (session.uiUnlocked && session.syncPending === 0 && !messages.historyActive) return false;
  chatsDirty = true;
  if (chat && chat === chats.selectedChat) {
    messagesDirty = true;
    dirtyMarkRead ||= markRead;
  }
  return true;
}

/** Coalesces an event burst into at most one chat-list reload per 200 ms. */
let chatsQueued = false;
export function queueRefreshChats() {
  if (chatsQueued) return;
  chatsQueued = true;
  setTimeout(() => {
    chatsQueued = false;
    void chats.refreshChats();
  }, 200);
}

/** The same for the open chat; `markRead` marks what arrived as seen if the window has focus. */
let messagesQueued: { chat: string; follow: boolean; markRead: boolean } | null = null;
function queueReloadMessages(
  host: EventHost,
  chat: string | null,
  follow: boolean,
  markRead: boolean,
) {
  if (!chat) return;
  if (messagesQueued?.chat === chat) {
    messagesQueued.follow ||= follow;
    messagesQueued.markRead ||= markRead;
    return;
  }
  const queued = (messagesQueued = { chat, follow, markRead });
  setTimeout(async () => {
    if (messagesQueued === queued) messagesQueued = null;
    if (chats.selectedChat !== queued.chat) return;
    // Anchor the view across the reload: appended messages must not shift what
    // a scrolled-up reader is looking at. The follow below re-pins to the
    // bottom afterwards when the reader is there.
    await messages.reloadMessages(queued.chat, true, host.getScroller());
    // Decide the follow at fire time: the reader may have scrolled up while
    // the reload was in flight, and must not be yanked back down.
    if (queued.follow && messages.atLatest && !ui.scrolledUp) host.scrollToBottom();
    if (queued.markRead && !ui.scrolledUp && document.hasFocus()) {
      await invoke("mark_read", { chat: queued.chat }).catch(() => {});
      queueRefreshChats();
    }
  }, 100);
}

/** Refreshes the list when the core resolved group subjects in the background. */
export async function refreshResolvedNames() {
  if ((await members.resolveNames()) > 0) await chats.refreshChats();
}

/** Whether the chat is currently open: its messages are on screen, so it never pings. */
function isOpenChat(chat: string): boolean {
  return chat === chats.selectedChat;
}

/** The chat's mute state from the cached list; unknown chats read as unmuted. */
function mutedUntilOf(chat: string): number {
  return chats.chats.find((c) => c.chat === chat)?.muted_until ?? 0;
}

function notificationsOn(): boolean {
  return session.settings.notifications_enabled ?? true;
}

/** Sender name for a notification, preferring the stored push name. */
function notifySenderName(message: StoredMessage): string {
  const push = message.sender_name;
  if (push && !isPlaceholder(push)) return members.displayName(push, message.sender);
  return members.senderName(message.sender);
}

/** Chat name for a notification, from the list or the address. */
function notifyChatName(chat: string): string {
  const known = chats.chats.find((c) => c.chat === chat);
  if (known) return chats.chatLabel(known);
  return members.displayName(null, chat);
}

/** Shows a notification for a fully loaded message, when the gate allows it. */
function notifyForMessage(message: StoredMessage, fresh: boolean) {
  const chat = message.chat;
  if (
    !shouldNotify(
      {
        fromMe: message.from_me,
        systemKind: message.system_kind,
        revoked: message.revoked,
        mutedUntil: mutedUntilOf(chat),
        notificationsEnabled: notificationsOn(),
        fresh,
        isOpenChat: isOpenChat(chat),
      },
    )
  ) {
    return;
  }
  const isGroup = chat.endsWith("@g.us");
  const chatName = notifyChatName(chat);
  const senderName = message.from_me ? "You" : notifySenderName(message);
  const body = notificationBody(message, (user) => members.mentionName(user));
  showChatNotification(
    notificationTitle({ isGroup, chatName, senderName }),
    isGroup ? groupNotificationBody(senderName, body) : body,
    chat,
  );
}

/**
 * Live arrivals come as hints without a body, so the row is fetched for the
 * notification text. Best-effort: a failed fetch falls back to a generic
 * ping, and a muted or globally silenced chat stays silent either way.
 */
async function notifyForHint(chat: string, id: string, fresh: boolean) {
  if (!fresh || !notificationsOn()) return;
  if (isOpenChat(chat)) return;
  if (isChatMuted(mutedUntilOf(chat))) return;
  let message: StoredMessage | null = null;
  try {
    const page = await invoke<MessagePage>("message_page", {
      chat,
      limit: 1,
      anchorId: id,
      direction: "through",
    });
    message = page.messages.find((m) => m.id === id) ?? null;
  } catch {
    message = null;
  }
  if (message) {
    notifyForMessage(message, true);
    return;
  }
  // Re-check after the fetch: the chat may have been opened, muted or
  // silenced while it was in flight.
  if (!notificationsOn() || isOpenChat(chat) || isChatMuted(mutedUntilOf(chat))) return;
  // The row is not on this device yet; still ping with the chat name.
  const isGroup = chat.endsWith("@g.us");
  const chatName = notifyChatName(chat);
  showChatNotification(chatName, isGroup ? "New message" : `New message from ${chatName}`, chat);
}

/** When each unnamed group's subject was last asked for; the core backs off failed ones. */
const askedSubjects = new Map<string, number>();

export async function dispatchServiceEvent(payload: ServiceEvent, host: EventHost) {
  switch (payload.kind) {
    case "qrCode":
      await session.showQr(payload.code);
      break;
    case "connected":
      session.connected = true;
      // A code was on screen, so this is a fresh link: the phone's history sync starts now.
      if (session.qrSvg) session.historyPercent = 0;
      await session.showQr(null);
      if (session.settings.skip_loading_screen) {
        session.gateDone = true;
        await chats.refreshChats();
        void refreshResolvedNames();
      } else {
        session.startGateTimeout();
      }
      break;
    case "disconnected":
      session.connected = false;
      break;
    case "uploadProgress":
      composer.noteUploadProgress(payload.token, payload.sent, payload.total);
      break;
    case "loggedOut":
      await host.reconnect();
      break;
    case "message":
    case "messageHint": {
      const chat = payload.kind === "message" ? payload.message.chat : payload.chat;
      const sender = payload.kind === "message" ? payload.message.sender : payload.sender;
      const fromMe = payload.kind === "message" ? payload.message.from_me : payload.from_me;
      const fresh = payload.kind === "message" ? true : payload.fresh;
      // A burst, a closed gate or an in-flight history answer: mark what
      // changed and let the completion event flush once.
      if (deferRefresh(chat, !fromMe)) {
        if (!fromMe) members.setTyping(chat, bare(sender), "paused");
        break;
      }
      // A message ends the sender's typing, whether or not "paused" arrived.
      if (!fromMe) {
        members.setTyping(chat, bare(sender), "paused");
      }
      if (session.uiUnlocked) {
        queueRefreshChats();
        if (chat === chats.selectedChat) {
          // Follow the stream when already at the bottom, but never yank
          // the view down while reading older messages. Status-only
          // updates never follow or mark.
          queueReloadMessages(host, chat, fresh && (fromMe || !ui.scrolledUp), fresh && !fromMe);
        }
      }
      // A group seen for the first time has no name yet; look it up in
      // the background so the list stops showing a raw number.
      if (
        fresh &&
        !fromMe &&
        chat.endsWith("@g.us") &&
        Date.now() - (askedSubjects.get(chat) ?? 0) > 30_000 &&
        !chats.chats.find((c) => c.chat === chat)?.display_name
      ) {
        askedSubjects.set(chat, Date.now());
        void refreshResolvedNames();
      }
      // Desktop notification for a live incoming message. History catch-up
      // took the deferred path above, so it never pings; muted chats and
      // the global toggle are gated inside the helpers.
      if (fresh && !fromMe) {
        if (payload.kind === "message") notifyForMessage(payload.message, true);
        else void notifyForHint(chat, payload.id, true);
      }
      break;
    }
    case "retentionApplied":
      if (payload.removed > 0 && !deferRefresh(null)) {
        queueRefreshChats();
        queueReloadMessages(host, chats.selectedChat, false, false);
      }
      break;
    case "chatStateChanged":
      if (!deferRefresh(null)) queueRefreshChats();
      break;
    case "namesUpdated":
      // Address-book names arrived after the initial fetch, so the cached
      // display names are stale until both lists reload.
      members.forgetUnresolvedNames();
      if (!deferRefresh(null)) {
        queueRefreshChats();
        queueReloadMessages(host, chats.selectedChat, false, false);
      }
      break;
    case "syncing":
      // The core counts what it stored, so the bar cannot run ahead of
      // the rows. Starting a new drain clears the previous dirty marks.
      session.syncPending = payload.pending;
      session.syncApplied = payload.applied;
      chatsDirty = false;
      messagesDirty = false;
      dirtyMarkRead = false;
      break;
    case "historyProgress":
      session.historyPercent = payload.percent < 100 ? payload.percent : null;
      break;
    case "backfill":
      session.backfill = payload.done < payload.total ? { done: payload.done, total: payload.total } : null;
      break;
    case "initialSyncComplete":
      // The backlog is in: paint it before the loading screen lifts, so
      // the first thing seen is the account as it now stands.
      session.finalizing = true;
      session.syncPending = 0;
      session.syncApplied = 0;
      try {
        await chats.refreshChats();
        await messages.reloadMessages(chats.selectedChat);
        await tick();
      } finally {
        chatsDirty = false;
        messagesDirty = false;
        dirtyMarkRead = false;
        session.gateDone = true;
        session.finalizing = false;
      }
      break;
    case "synced": {
      // The backlog is in; flush once so the burst's queued refreshes land together.
      session.syncPending = 0;
      session.syncApplied = 0;
      if (chatsDirty) {
        chatsDirty = false;
        queueRefreshChats();
      }
      if (messagesDirty) {
        messagesDirty = false;
        const markRead = dirtyMarkRead;
        dirtyMarkRead = false;
        // Decide follow at flush time: a flag set mid-burst would use stale scroll state.
        queueReloadMessages(host, chats.selectedChat, !ui.scrolledUp, markRead);
      }
      break;
    }
    case "historyLoaded":
      messages.historyActive = false;
      if (!session.uiUnlocked) {
        // The gate is still closed; the initial paint will pick this up.
        chatsDirty = true;
        if (chats.selectedChat && payload.chats.includes(chats.selectedChat)) messagesDirty = true;
        break;
      }
      await chats.refreshChats();
      chatsDirty = false;
      if (chats.selectedChat && payload.chats.includes(chats.selectedChat)) {
        const requestedOlder = messages.loadingOlder && messages.recall !== null;
        // The full reload covers any hints that landed while loading.
        messagesDirty = false;
        dirtyMarkRead = false;
        if (requestedOlder) await messages.finishOlder(chats.selectedChat, host.getScroller());
        else await messages.reloadMessages(chats.selectedChat, true, host.getScroller());
      } else if (messagesDirty && chats.selectedChat) {
        // Burst hints that landed while older history was loading.
        messagesDirty = false;
        const markRead = dirtyMarkRead;
        dirtyMarkRead = false;
        queueReloadMessages(host, chats.selectedChat, false, markRead);
        if (chatsDirty) {
          chatsDirty = false;
          queueRefreshChats();
        }
      } else if (chatsDirty) {
        chatsDirty = false;
        queueRefreshChats();
      }
      break;
    case "avatarChanged":
      chats.forgetAvatar(payload.jid);
      chats.loadAvatar(payload.jid);
      break;
    case "typing":
      members.setTyping(payload.chat, payload.sender, payload.state);
      break;
    case "presence":
      members.presence[payload.jid] = { online: payload.online, last_seen: payload.last_seen };
      break;
    case "marks":
      if (payload.chat === chats.selectedChat) await messages.loadMarks(payload.chat);
      break;
    case "stickerLibraryChanged":
      stickers.touch();
      break;
    case "memberLabel":
      if (payload.chat === chats.selectedChat) {
        const label = payload.label || null;
        const member = members.participants.find((p) => p.jid === payload.jid);
        if (member) member.label = label;
        const info = chats.groupInfo?.participants.find((p) => p.jid === payload.jid);
        if (info) info.label = label;
      }
      break;
    case "groupChanged":
      // Who may send, who is admin, or the name changed; the core dropped its cache.
      if (payload.chat === chats.selectedChat) {
        await members.loadChatGroup(payload.chat, () => chats.selectedChat === payload.chat);
        if (chats.groupInfo) chats.groupInfo = members.chatGroup;
      }
      void chats.loadGroupKinds();
      void chats.refreshChats();
      break;
  }
}
