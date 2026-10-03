// Backend event stream: maps every ServiceEvent kind onto domain updates.
// Moved out of +page.svelte so the domains above (session, chats, messages,
// members, composer, ui) visibly cover the stream. View callbacks the
// dispatcher cannot own (scrolling, reconnecting) arrive via host.
import { tick } from "svelte";
import { invoke, log } from "$lib/utils/ipc";
import { bare, isUnavailable } from "$lib/utils/message";
import type { MessagePage } from "$lib/utils/message-window";
import type { ServiceEvent, StoredMessage } from "$lib/utils/models";
import {
  groupNotificationBody,
  isChatMuted,
  notificationBody,
  notificationTitle,
  shouldNotify,
  showChatNotification,
} from "$lib/utils/notifications";
import { isPlaceholder } from "$lib/utils/phone";
import { chats } from "./chats.svelte";
import { labels } from "./labels.svelte";
import { composer } from "./composer.svelte";
import { favorites } from "./favorites.svelte";
import { members } from "./members.svelte";
import { memberSheet } from "./member-sheet.svelte";
import { quickReplies } from "./quick-replies.svelte";
import { messages } from "./messages.svelte";
import { session } from "./session.svelte";
import { stickers } from "./stickers.svelte";
import { ui } from "./ui.svelte";
import { keywords } from "./keywords.svelte";
import { t } from "../i18n/localizer.ts";
import { uiMessage } from "./localized.ts";
import { notificationHistory } from "$lib/notifications/history-store";

export type EventHost = {
  scrollToBottom(): void;
  /** The first row on screen, so a full reload can keep the reader's place. */
  anchor(): string | null;
  reveal(id: string): void;
  reconnect(): Promise<void>;
};

/**
 * Burst protocol: arrivals come as `messageHint` (routing only, ~90 B vs
 * ~500 B full payload, 5.6x smaller in the serialization test). While the
 * loading gate is closed, a drain is running, or a history answer is in
 * flight, hints set dirty flags; completion or the watchdog flushes them.
 * A 500-msg batch inserts + both queries in ~53 ms (store regression test).
 * Live messages outside a burst still refresh immediately through the
 * 500/100 ms coalescing queues.
 */
let chatsDirty = false;
let messagesDirty = false;
let dirtyMarkRead = false;
let deferredTimer: ReturnType<typeof setTimeout> | null = null;
let deferredAccount: string | null = null;
let deferredGeneration = -1;

function resetDeferred() {
  if (deferredTimer !== null) clearTimeout(deferredTimer);
  deferredTimer = null;
  chatsDirty = messagesDirty = dirtyMarkRead = false;
}

function flushDeferred(host: EventHost) {
  if (deferredAccount !== session.activeAccount || deferredGeneration !== messages.accountGeneration) { resetDeferred(); return; }
  if (deferredTimer !== null) clearTimeout(deferredTimer);
  deferredTimer = null;
  if (!session.uiUnlocked) { watchDeferred(host); return; }
  if (chatsDirty) queueRefreshChats();
  if (messagesDirty) queueReloadMessages(host, chats.selectedChat, !ui.scrolledUp, dirtyMarkRead);
  chatsDirty = messagesDirty = dirtyMarkRead = false;
}

function watchDeferred(host: EventHost) {
  if (deferredTimer !== null || !chatsDirty && !messagesDirty) return;
  const account = session.activeAccount, generation = messages.accountGeneration;
  deferredAccount = account;
  deferredGeneration = generation;
  deferredTimer = setTimeout(() => {
    deferredTimer = null;
    if (account !== session.activeAccount || generation !== messages.accountGeneration) { resetDeferred(); return; }
    if (!session.uiUnlocked) { watchDeferred(host); return; }
    log("warn", `event watchdog flush: syncPending=${session.syncPending} historyActive=${messages.historyActive} chatsDirty=${chatsDirty} messagesDirty=${messagesDirty}`);
    flushDeferred(host);
  }, 3_000);
}

/**
 * Marks what a change touched when fetching right away would be wasteful or
 * invisible: the gate is closed, a drain is running, or a history answer is
 * in flight. Returns false when the caller should refresh immediately.
 */
function deferRefresh(host: EventHost, chat: string | null, markRead = false) {
  if (deferredAccount !== session.activeAccount || deferredGeneration !== messages.accountGeneration) {
    resetDeferred();
  }
  if (session.uiUnlocked && session.syncPending === 0 && !messages.historyActive) return false;
  chatsDirty = true;
  if (chat && chat === chats.selectedChat) {
    messagesDirty = true;
    dirtyMarkRead ||= markRead;
  }
  watchDeferred(host);
  return true;
}

/** Coalesces an event burst into at most one chat-list reload per 500 ms. */
let chatsQueued = false;
export function queueRefreshChats() {
  if (chatsQueued) return;
  chatsQueued = true;
  setTimeout(() => {
    chatsQueued = false;
    void chats.refreshChats();
  }, 500);
}

/**
 * Coalesces read marks for the open chat: each live message used to invoke
 * `mark_read` on its own, and a burst of ten meant ten store writes and ten
 * chat-list reloads.
 */
let markReadQueued: string | null = null;
function queueMarkRead(chat: string) {
  if (markReadQueued === chat) return;
  markReadQueued = chat;
  setTimeout(() => {
    if (markReadQueued === chat) markReadQueued = null;
    if (chat !== chats.selectedChat || ui.scrolledUp || !document.hasFocus()) return;
    if (!messages.ordered.some((message) => !message.from_me && !message.read && !isUnavailable(message))) return;
    invoke("mark_read", { chat })
      .then(() => queueRefreshChats())
      .catch(() => {});
  }, 300);
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
    const anchor = host.anchor();
    await messages.reloadMessages(queued.chat);
    if (anchor) host.reveal(anchor);
    // Decide the follow at fire time: the reader may have scrolled up while
    // the reload was in flight, and must not be yanked back down.
    if (queued.follow && messages.atLatest && !ui.scrolledUp) host.scrollToBottom();
    if (queued.markRead) queueMarkRead(queued.chat);
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

/** Whether the chat mutes @all mentions; unknown chats read as unmuted. */
function muteAtAllOf(chat: string): boolean {
  return chats.chats.find((c) => c.chat === chat)?.mute_at_all ?? false;
}

/** Effective @all mute: the chat's own mute, or the global mute-everywhere setting. */
function muteAtAllEffective(chat: string): boolean {
  return muteAtAllOf(chat) || (session.settings.mute_all_at_all ?? false);
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
  const account = session.activeAccount;
  if (!account) return;
  const generation = messages.accountGeneration;
  const current = () => account === session.activeAccount && generation === messages.accountGeneration
    && !message.deleted && keywords.account === account && !keywords.hidden(message)
    && shouldNotify(
      {
        fromMe: message.from_me,
        systemKind: message.system_kind,
        revoked: message.revoked,
        mutedUntil: mutedUntilOf(chat),
        notificationsEnabled: notificationsOn(),
        fresh,
        isOpenChat: isOpenChat(chat),
        sentAt: message.timestamp,
        mentionedAllOnly: message.mentioned_all_only,
        muteAtAll: muteAtAllEffective(chat),
      },
    );
  if (!current()) return;
  const isGroup = chat.endsWith("@g.us");
  const chatName = notifyChatName(chat);
  const senderName = message.from_me ? t("chat.you") : notifySenderName(message);
  const preview = notificationBody({ ...message, media_kind: message.media_once_kind ? "view_once" : message.media_kind }, (user) => members.mentionName(user));
  const title = notificationTitle({ isGroup, chatName, senderName });
  const body = isGroup ? groupNotificationBody(senderName, preview) : preview;
  notificationHistory.record(account, {
    chat, id: message.id, sender: message.sender, chat_name: chatName, sender_name: senderName,
    title, body, timestamp: Date.now(),
  }, current);
  showChatNotification(
    title,
    body,
    chat,
    account,
    current,
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
  // A chat muting @all still pings for direct mentions; the fetched row
  // decides. The pre-fetch gate only skips when the mute state is already
  // known to silence everything, which an @all mute alone does not.
  const account = session.activeAccount;
  if (!account) return;
  const generation = messages.accountGeneration;
  let message: StoredMessage | null = null;
  try {
    const page = await invoke<MessagePage>("message_page", {
      chat,
      limit: 1,
      anchorId: id,
      direction: "through",
    });
    message = page.messages.find((m) => m.chat === chat && m.id === id) ?? null;
  } catch {
    message = null;
  }
  if (account !== session.activeAccount || generation !== messages.accountGeneration) return;
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
  showChatNotification(chatName, isGroup ? t("state.new_message") : t("state.new_message_from", { name: chatName }), chat, account,
    () => account === session.activeAccount && generation === messages.accountGeneration
      && notificationsOn() && !isOpenChat(chat) && !isChatMuted(mutedUntilOf(chat)));
}

/** When each unnamed group's subject was last asked for; the core backs off failed ones. */
const askedSubjects = new Map<string, number>();

export async function dispatchServiceEvent(payload: ServiceEvent, host: EventHost) {
  log("debug", `event ${payload.kind}: uiUnlocked=${session.uiUnlocked} syncPending=${session.syncPending} historyActive=${messages.historyActive} chatsDirty=${chatsDirty} messagesDirty=${messagesDirty}`);
  switch (payload.kind) {
    case "qrCode":
      await session.showQr(payload.code);
      break;
    case "pairingCode":
      session.pairCode = payload.code;
      session.pairCodeExpiresAt = Date.now() + payload.timeout_secs * 1000;
      session.pairCodeError = null;
      session.pairCodeManual = false;
      break;
    case "pairingCodeRefresh":
      // The server cleared the flow, so a replacement can be minted right away.
      if (payload.force_manual) {
        session.pairCode = null;
        session.pairCodeExpiresAt = null;
        session.pairCodeManual = true;
      } else {
        void session.refreshPairCode();
      }
      break;
    case "pairingCodeError":
      session.pairCode = null;
      session.pairCodeExpiresAt = null;
      session.pairCodeManual = false;
      session.pairCodeError = {
        message: payload.message,
        throttled: payload.throttled,
        unavailable: payload.unavailable,
      };
      break;
    case "connected":
      session.connected = true;
      session.syncPending = session.syncApplied = 0;
      session.clearPairCode();
      void favorites.refresh();
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
      flushDeferred(host);
      break;
    case "disconnected":
      session.connected = false;
      session.syncPending = session.syncApplied = 0;
      // A dropped connection spends any code in flight.
      session.clearPairCode();
      break;
    case "uploadProgress":
      composer.noteUploadProgress(payload.token, payload.sent, payload.total);
      break;
    case "loggedOut":
      await host.reconnect();
      break;
    case "message":
    case "messageHint": {
      // Only the delivery state changed: patch the loaded row and stop.
      // Refetching it and refreshing the chat list per receipt is what made
      // a busy account's read receipts cost as much as its messages.
      if (payload.kind === "messageHint" && payload.change === "status") {
        if (payload.status) messages.setStatus(payload.chat, payload.id, payload.status);
        break;
      }
      const chat = payload.kind === "message" ? payload.message.chat : payload.chat;
      const sender = payload.kind === "message" ? payload.message.sender : payload.sender;
      const fromMe = payload.kind === "message" ? payload.message.from_me : payload.from_me;
      const fresh = payload.kind === "message" ? true : payload.fresh;
      // A burst, a closed gate or an in-flight history answer: mark what
      // changed and let the completion event flush once.
      if (payload.kind === "message" && chat === chats.selectedChat) messages.append(payload.message);
      if (session.uiUnlocked && fresh) queueRefreshChats();
      if (deferRefresh(host, chat, !fromMe)) {
        if (payload.kind === "message" && session.uiUnlocked && !ui.scrolledUp && messages.atLatest) host.scrollToBottom();
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
          // Fold the row in rather than reloading the window: a full reload
          // rebuilds every row object and re-renders every bubble, which a
          // busy account otherwise pays for on each message. Bursts above
          // still take one deferred full reload.
          if (payload.kind === "messageHint" && !fresh && !messages.messages.some((message) => message.chat === chat && message.id === payload.id)) {
            queueReloadMessages(host, chat, false, false);
          } else if (payload.kind === "messageHint") void messages.refreshRow(chat, payload.id, fresh);
          // Follow the stream when already at the bottom, but never yank
          // the view down while reading older messages. Status-only
          // updates never follow or mark.
          if (fresh && !ui.scrolledUp) {
            if (messages.atLatest) host.scrollToBottom();
            if (!fromMe) queueMarkRead(chat);
          }
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
      if (payload.removed > 0 && !deferRefresh(host, null)) {
        queueRefreshChats();
        queueReloadMessages(host, chats.selectedChat, false, false);
      }
      break;
    case "chatStateChanged":
      if (!deferRefresh(host, null)) queueRefreshChats();
      break;
    case "chatPinRemoved":
      ui.notify(uiMessage("state.pin_removed"));
      if (!deferRefresh(host, null)) queueRefreshChats();
      break;
    case "namesUpdated":
      ++session.profileVersion;
      void favorites.refresh();
      // Address-book names arrived after the initial fetch, so the cached
      // display names are stale until both lists reload.
      members.forgetUnresolvedNames();
      if (!deferRefresh(host, null)) {
        queueRefreshChats();
        queueReloadMessages(host, chats.selectedChat, false, false);
      }
      break;
    case "syncing":
      // A new drain must not discard refreshes from an unfinished one.
      session.syncPending = payload.pending;
      session.syncApplied = payload.applied;
      break;
    case "historyProgress":
      session.historyPercent = payload.percent < 100 ? payload.percent : null;
      break;
    case "backfill":
      session.backfill = payload.done < payload.total ? { done: payload.done, total: payload.total } : null;
      break;
    case "initialSyncComplete": {
      const account = session.activeAccount, generation = messages.accountGeneration;
      // The backlog is in: paint it before the loading screen lifts, so
      // the first thing seen is the account as it now stands.
      session.finalizing = true;
      session.syncPending = 0;
      session.syncApplied = 0;
      resetDeferred();
      try {
        await chats.refreshChats();
        if (account !== session.activeAccount || generation !== messages.accountGeneration) break;
        await messages.reloadMessages(chats.selectedChat);
        await tick();
      } finally {
        if (account === session.activeAccount && generation === messages.accountGeneration) {
          session.gateDone = true;
          session.finalizing = false;
          flushDeferred(host);
        }
      }
      break;
    }
    case "synced": {
      // The backlog is in; flush once so the burst's queued refreshes land together.
      session.syncPending = 0;
      session.syncApplied = 0;
      flushDeferred(host);
      break;
    }
    case "historyLoaded":
      session.syncPending = session.syncApplied = 0;
      messages.historyActive = false;
      if (!session.uiUnlocked) {
        deferRefresh(host, chats.selectedChat && payload.chats.includes(chats.selectedChat) ? chats.selectedChat : null);
        break;
      }
      {
        const account = session.activeAccount, generation = messages.accountGeneration, chat = chats.selectedChat;
        const reload = messagesDirty, markRead = dirtyMarkRead;
        resetDeferred();
        try {
          await chats.refreshChats();
          if (account !== session.activeAccount || generation !== messages.accountGeneration || chat !== chats.selectedChat) break;
          if (chat && payload.chats.includes(chat)) {
            const requestedOlder = messages.loadingOlder && messages.recall !== null;
            if (requestedOlder) await messages.finishOlder(chat);
            else {
              const anchor = host.anchor();
              await messages.reloadMessages(chat);
              if (anchor && chat === chats.selectedChat && generation === messages.accountGeneration) host.reveal(anchor);
            }
          } else if (reload && chat) queueReloadMessages(host, chat, false, markRead);
        } finally {
          flushDeferred(host);
        }
      }
      break;
    case "avatarChanged":
      chats.forgetAvatar(payload.jid);
      chats.loadAvatar(payload.jid);
      break;
    case "typing":
      members.setTyping(payload.chat, payload.sender, payload.state);
      memberSheet.typing(payload.chat, payload.sender, payload.state);
      break;
    case "presence":
      members.setPresence(payload.jid, payload.online, payload.last_seen);
      memberSheet.presence(payload.jid, payload.online, payload.last_seen);
      break;
    case "marks":
      if (!deferRefresh(host, payload.chat)) queueRefreshChats();
      if (payload.chat === chats.selectedChat) {
        await messages.loadMarks(payload.chat);
        // A kept one-time media arrives as a mark change: the row is ordinary
        // media now, so reload it behind the marks it just lost.
        queueReloadMessages(host, payload.chat, false, false);
      }
      break;
    case "storeChanged":
      labels.queueRefresh();
      memberSheet.queueRefresh(null);
      // A listener lagged and missed store changes with no chat to name them;
      // reload everything the open view could be showing.
      if (!deferRefresh(host, chats.selectedChat)) {
        queueRefreshChats();
        queueReloadMessages(host, chats.selectedChat, false, false);
        if (chats.selectedChat) await messages.loadMarks(chats.selectedChat);
      }
      break;
    case "stickerLibraryChanged":
      stickers.touch();
      break;
    case "memberLabel":
      memberSheet.queueRefresh(payload.chat);
      if (payload.chat === chats.selectedChat) {
        const label = payload.label || null;
        // The roster is raw, so the changed member replaces its entry.
        if (members.participants.some((p) => p.jid === payload.jid)) {
          members.participants = members.participants.map((p) =>
            p.jid === payload.jid ? { ...p, label } : p,
          );
        }
        const info = chats.groupInfo?.participants.find((p) => p.jid === payload.jid);
        if (info) info.label = label;
      }
      break;
    case "groupChanged":
      memberSheet.queueRefresh(payload.chat);
      // Who may send, who is admin, or the name changed; the core dropped its cache.
      if (payload.chat === chats.selectedChat) {
        await members.loadChatGroup(payload.chat, () => chats.selectedChat === payload.chat);
        if (chats.groupInfo) chats.groupInfo = members.chatGroup;
      }
      void chats.loadGroupKinds();
      void chats.refreshChats();
      break;
    case "groupAuditChanged":
      if (payload.chat === chats.selectedChat) ++chats.auditRevision;
      memberSheet.queueRefresh(payload.chat);
      break;
    case "favoritesChanged":
      void favorites.refresh();
      void labels.refresh();
      break;
    case "labelsChanged":
      labels.queueRefresh();
      if (!deferRefresh(host, null)) queueRefreshChats();
      break;
    case "quickRepliesChanged":
      quickReplies.queueRefresh();
      break;
  }
}
