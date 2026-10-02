<script lang="ts">
  import "$lib/utils/legacy";
  import ThemeLayers from "$lib/settings/ThemeLayers.svelte";
  import { addAccount, chooseAccount, connect, reconnect, removeAccount, switchTo, syncState } from "$lib/state/accounts";
  import { onDrop, onPaste } from "$lib/state/attachments";
  import { openPings, openStarred, searchChat } from "$lib/state/finder";
  import { act, canDeleteForEveryone, canDeletePickedForEveryone, copyMessages, deleteMessage, deleteSelected, eventFields, forwardMessages, menuItems as messageMenuItems, pickedInOrder, reactMessages, saveEvent, starMessages, target, viewableMessages } from "$lib/state/message-actions";
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { listen } from "@tauri-apps/api/event";
  import StarredList from "$lib/messages/StarredList.svelte";
  import MessageFinder from "$lib/messages/MessageFinder.svelte";
  import ChatSettings from "$lib/chat/ChatSettings.svelte";
  import { formatBulkReadFailures } from "$lib/utils/bulk-chats";
  import NewGroup from "$lib/chat/NewGroup.svelte";
  import type { ChatRetention } from "$lib/utils/models";
  import ProfileCard from "$lib/contacts/ProfileCard.svelte";
  import MemberSheet from "$lib/contacts/MemberSheet.svelte";
  import { memberSheet } from "$lib/state/member-sheet.svelte";
  import { quickReplies } from "$lib/state/quick-replies.svelte";
  import ContactInfo from "$lib/contacts/ContactInfo.svelte";
  import BroadcastInfo from "$lib/chat/BroadcastInfo.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import { isBroadcastList, broadcastSendReason } from "$lib/utils/broadcast";
  import type { BroadcastList } from "$lib/utils/wire";
  import NewContact from "$lib/contacts/NewContact.svelte";
  import ContactSharing from "$lib/contacts/ContactSharing.svelte";
  import type { SharedContact, ContactShareScope } from "$lib/utils/vcard";
  import QuickSwitcher from "$lib/chat/QuickSwitcher.svelte";
  import UnifiedInbox from "$lib/chat/UnifiedInbox.svelte";
  import LabelDialog from "$lib/labels/LabelDialog.svelte";
  import { labels } from "$lib/state/labels.svelte";
  import { desktopChatTarget, desktopUnreadCount } from "$lib/utils/desktop";
  import type { InboxAction } from "$lib/utils/inbox";
  import MessageInfo from "$lib/messages/MessageInfo.svelte";
  import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";
  import flagFont from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";

  // Windows has no flag glyphs and draws the two letters instead. The font is
  // bundled, so nothing is fetched at runtime; elsewhere this is a no-op.
  polyfillCountryFlagEmojis("Twemoji Country Flags", flagFont);
  import Icon from "$lib/ui/Icon.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Spinner from "$lib/ui/Spinner.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import PairingView from "$lib/settings/PairingView.svelte";
  import ChatSidebar from "$lib/chat/ChatSidebar.svelte";
  import ChatHeader from "$lib/chat/ChatHeader.svelte";
  import MessageList from "$lib/messages/MessageList.svelte";
  import ComposerBar from "$lib/composer/ComposerBar.svelte";
  import AttachmentRecoveryPanel from "$lib/composer/AttachmentRecoveryPanel.svelte";
  import { findRecovery, saveAttachmentCopy } from "$lib/utils/attachment-recovery";
  import { visibleReadFrontier } from "$lib/utils/album-timeline";
  import ScheduledOutbox from "$lib/composer/ScheduledOutbox.svelte";
  import SelectionBar from "$lib/messages/SelectionBar.svelte";
  import { hue } from "$lib/utils/avatar";
  import { bare, captionOf, dayKey, dayLabel, formatTime, isUnavailable, MEDIA_LABELS } from "$lib/utils/message";
  import { chats } from "$lib/state/chats.svelte";
  import { composer } from "$lib/state/composer.svelte";
  import { favorites } from "$lib/state/favorites.svelte";
  import { transcription } from "$lib/state/transcription.svelte";
  import { dispatchServiceEvent, queueRefreshChats } from "$lib/state/events";
  import { members } from "$lib/state/members.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { keywords } from "$lib/state/keywords.svelte";
  import { notificationHistory } from "$lib/notifications/history-store";
  import type { MessagePage } from "$lib/utils/message-window";
  import { once } from "$lib/state/once.svelte";
  import { player } from "$lib/state/player.svelte";
  import { session } from "$lib/state/session.svelte";
  import { ui } from "$lib/state/ui.svelte";
  import Settings, { type Section } from "$lib/settings/Settings.svelte";
  import GroupInfo, { type AdminReport } from "$lib/chat/GroupInfo.svelte";
  import MediaViewer, { type ViewerItem } from "$lib/media/MediaViewer.svelte";
  import Gallery from "$lib/media/Gallery.svelte";
  import MessageMenu, { type MenuItem } from "$lib/messages/MessageMenu.svelte";
  import ExpressionPicker from "$lib/composer/ExpressionPicker.svelte";
  import ChatPicker from "$lib/chat/ChatPicker.svelte";
  import ReactionList from "$lib/messages/ReactionList.svelte";
  import { keybinds, matches } from "$lib/utils/keybinds.svelte";
  import CreateDialog from "$lib/chat/CreateDialog.svelte";
  import { plain } from "$lib/utils/format";
  import { customization, lensMap } from "$lib/utils/theme.svelte";

  import type {
    ParticipantChange,
    GroupHistoryResult,
    GroupMemberAddResult,
    GroupJoinRequest,
    SearchResult,
    ServiceEvent,
    StoredMessage,
  } from "$lib/utils/models";
  import { changeText } from "$lib/utils/group-actions";

  function openSettings(section: Section) {
    ui.settingsSection = section;
    ui.accountMenu = false;
    ui.showSettings = true;
  }

  async function inboxAction(account: string, chat: string, action: InboxAction) {
    const generation = messages.accountGeneration;
    const current = () => account === session.activeAccount && generation === messages.accountGeneration && session.connected;
    if (!current()) throw new Error("Inbox account changed or disconnected.");
    if (action.kind === "label") {
      await labels.applyChat(action.label, chat, action.applied);
      return;
    }
    await composer.enqueue((signal) => {
      if (signal.aborted || !current()) throw new Error("Inbox account changed before operation.");
      if (action.kind === "read") return invoke(action.read ? "mark_read" : "set_marked_unread", { account, chat, unread: true });
      if (action.kind === "archive") return invoke("set_archived", { account, chat, archived: action.archived });
      if (!Number.isInteger(action.seconds) || action.seconds < -1) throw new Error("Invalid mute duration.");
      const until = action.seconds <= 0 ? action.seconds : Math.floor(Date.now() / 1000) + action.seconds;
      return invoke("set_muted", { account, chat, until });
    });
    if (!current()) throw new Error("Inbox account changed during operation.");
    if (action.kind === "read" && action.read && chat === chats.selectedChat) {
      messages.firstUnreadId = null;
      messages.lastUnreadId = null;
    }
    await chats.refreshChats();
  }

  async function sendContacts(contacts: SharedContact[], scope: ContactShareScope): Promise<string> {
    const reason = broadcastSendReason(scope.chat);
    if (reason) throw new Error(reason);
    const current = () => scope.account === session.activeAccount && scope.chat === chats.selectedChat
      && scope.generation === messages.accountGeneration && session.connected;
    const allowed = () => current() && !isBroadcastList(scope.chat) && !composer.editing && !composer.recording && (!members.chatGroup || members.chatGroup.can_send);
    if (!allowed()) throw new Error("Contact sharing target changed or is unavailable.");
    const result = await composer.enqueue((signal) => {
      if (signal.aborted || !allowed()) throw new Error("Contact sharing target changed before send.");
      return invoke<import("$lib/utils/wire").ContactSendResult>("send_contacts", { account: scope.account, chat: scope.chat, contacts });
    });
    if (!result.message_id) throw new Error("Contact send was not acknowledged.");
    if (current()) {
      if (result.warning) ui.notify(result.warning);
      try { await messages.reloadMessages(scope.chat); await chats.refreshChats(); }
      catch (error) { if (current()) ui.fail(error); }
    }
    return result.message_id;
  }

  $effect(() => {
    const account = session.activeAccount;
    untrack(() => { keywords.load(account); notificationHistory.load(account); });
  });
  $effect(() => {
    const account = session.activeAccount;
    void keywords.revision;
    void chats.chats;
    void messages.accountGeneration;
    untrack(() => void keywords.refreshCounts(account, () => messages.accountGeneration));
  });
  $effect(() => {
    void keywords.revision;
    untrack(() => { if (ui.finder?.mode === "pings") void openPings(ui.finder.chat); });
  });
  const visibleChats = $derived(chats.visibleChats.filter((chat) => !chats.labelFilter || labels.account === session.activeAccount && labels.chatIds(chat.chat).includes(chats.labelFilter)).map((chat) => {
    const count = keywords.account === session.activeAccount ? keywords.counts[chat.chat] ?? 0 : 0;
    return count ? { ...chat, mention_count: chat.mention_count + count } : chat;
  }));
  const unreadPings = $derived(chats.chats.reduce((sum, chat) => sum + chat.mention_count
    + (keywords.account === session.activeAccount ? keywords.counts[chat.chat] ?? 0 : 0), 0));
  const inboxChats = $derived(chats.chats.map((chat) => ({ ...chat, mention_count: chat.mention_count
    + (keywords.account === session.activeAccount ? keywords.counts[chat.chat] ?? 0 : 0) })));
  const labelsByChat = $derived.by(() => {
    const result: Record<string, string[]> = {};
    if (labels.account === session.activeAccount) for (const row of labels.view.chats) (result[row.chat] ??= []).push(row.label_id);
    return result;
  });
  const labelSelection = $derived.by(() => {
    const targets = ui.labelTargets ?? [];
    const sets = targets.map((target) => new Set(target.id ? labels.messageIds(target.chat, target.id) : labels.chatIds(target.chat)));
    return {
      selected: labels.view.labels.filter((label) => sets.length && sets.every((set) => set.has(label.id))).map((label) => label.id),
      mixed: labels.view.labels.filter((label) => sets.some((set) => set.has(label.id)) && !sets.every((set) => set.has(label.id))).map((label) => label.id),
    };
  });
  $effect(() => {
    const accountId = session.activeAccount;
    const count = desktopUnreadCount(chats.chats);
    untrack(() => { void invoke("desktop_unread", { accountId, count: accountId ? count : 0 }).catch((error) => ui.fail(error)); });
  });

  $effect(() => {
    session.applyZoom();
  });

  let composerInput: HTMLTextAreaElement | undefined = $state();
  let contactDialog = $state<HTMLDialogElement>();
  $effect(() => { if (ui.sharingContacts && contactDialog && !contactDialog.open) contactDialog.showModal(); });
  let chatOpenSeq = 0;
  let galleryChat = $state<string | null>(null);
  $effect(() => { session.activeAccount; galleryChat = null; });
  onMount(() => transcription.start());
  // The composer module reads the element at event time; synced here.
  $effect(() => {
    composer.inputEl = composerInput;
  });

  // The chat list width is a customization setting; older builds kept it under its own key.
  if (customization.listWidth === undefined) {
    try {
      customization.listWidth = Number(localStorage.getItem("postal.sidebarWidth")) || 300;
    } catch {
      customization.listWidth = 300;
    }
  }
  let layoutColumns = $derived(`${customization.listWidth ?? 300}px 1fr`);

  function startResize(event: MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = customization.listWidth ?? 300;
    const onMove = (e: MouseEvent) => {
      customization.listWidth = Math.max(180, Math.min(640, startWidth + e.clientX - startX));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }

  $effect(() => {
    const root = document.documentElement.classList;
    root.toggle("density-compact", customization.density === "compact");
    root.toggle("density-cozy", customization.density === "cozy");
  });

  let scroller: HTMLDivElement | undefined = $state();
  let messageList = $state<ReturnType<typeof MessageList> | undefined>();

  /** Member names under a group's title, as far as they are known. */
  const subtitle = $derived(
    chats.selectedChat?.endsWith("@g.us") && members.participants.length > 0
      ? members.participants.map((p) => members.displayName(p.name, p.jid)).join(", ")
      : null,
  );

  async function openChat(chat: string, jumpToMention = false, label: string | null = null) {
    ui.showInbox = false;
    const opening = ++chatOpenSeq;
    const account = messages.accountGeneration;
    const current = () => opening === chatOpenSeq && account === messages.accountGeneration && chats.selectedChat === chat;
    if (chats.selectedChat !== chat) {
      composer.stopTyping();
      ui.switching = true;
      members.chatGroup = null;
      // A staged reply or edit belongs to the chat it was started in.
      composer.replyingTo = null;
      composer.editing = null;
      composer.resetHistory();
      ui.picking = null;
      ui.bulkDelete = null;
    }
    chats.selectedChat = chat;
    composer.restoreKnownUnsent(chat);
    // One-to-one typing only arrives for contacts we are subscribed to.
    if (!chat.endsWith("@g.us") && !isBroadcastList(chat)) invoke("watch_presence", { jid: chat }).catch(() => {});
    chats.titleOverride = label;
    ui.scrolledUp = false;
    messages.prepareChat(chat, session.settings.message_window_size);
    composer.chatPrivacy = { send_typing: null, send_receipts: null };
    invoke<import("$lib/utils/wire").ChatSettings>("chat_settings", { chat })
      .then((s) => {
        if (!current()) return;
        messages.loadOnScroll = s.retention.on_demand;
        composer.chatPrivacy = { send_typing: s.send_typing, send_receipts: s.send_receipts };
      })
      .catch(() => {});
    members.participants = members.memberCache[chat] ?? [];
    composer.chosenMentions = [];
    composer.mentionQuery = null;
    composer.draft = composer.drafts[chat] ?? "";
    composer.resetUndo();
    chats.showGroupInfo = false;
    chats.groupInfo = null;
    contactInfoFor = null;
    broadcastFor = null;
    try {
      // Invalidate any in-flight reload from the previous chat.
      const seq = messages.nextSeq();
      // Mentions are captured before the chat is marked read, since that clears them.
      const [mentions, page, keywordMatches] = await Promise.all([
        invoke<string[]>("unread_mentions", { chat }).catch(() => [] as string[]),
        invoke<MessagePage>("message_page", { chat, limit: messages.messageLimit }),
        session.activeAccount && keywords.rules.highlight.length
          ? invoke<StoredMessage[]>("keyword_matches", { accountId: session.activeAccount, chat,
            unreadOnly: true, highlight: [...keywords.rules.highlight], hide: [...keywords.rules.hide] }).catch(() => [] as StoredMessage[])
          : Promise.resolve([] as StoredMessage[]),
      ]);
      // A quicker click on another chat has already taken over.
      if (!current()) return;
      messages.mentionQueue = [...new Set([...mentions, ...keywordMatches.slice().reverse().map((message) => message.id)])];
      messages.mentionCursor = 0;
      if (seq === messages.messagesSeq) messages.acceptMessages(page.messages);
      else await messages.reloadMessages(chat);
      if (!current()) return;
      await messages.loadMarks(chat);
      if (!current()) return;
      const loaded = messages.messages;
      // Enter at the unread divider when there is one, as Discord does, rather
      // than at the newest message. The unread flags are still intact here
      // because marking is now driven by scrolling, not by opening.
      const oldestUnread = [...loaded].reverse().find((m) => !m.read && !m.from_me && !isUnavailable(m));
      const newestUnread = loaded.find((m) => !m.read && !m.from_me && !isUnavailable(m));
      messages.firstUnreadId = oldestUnread?.id ?? null;
      messages.lastUnreadId = newestUnread?.id ?? null;
      messages.lastMarkedId = null;
      if (oldestUnread) {
        ui.scrolledUp = true;
        await tick();
        pinUnreadDivider(chat);
      } else {
        scrollToBottom();
      }
      await tick();
      if (!current()) return;
      ui.switching = false;
    } catch (e) {
      if (!current()) return;
      ui.switching = false;
      ui.fail(e);
    }
    // Group members power the @ autocomplete, and the group's settings decide
    // whether we may write; a one-to-one chat has neither.
    await members.loadChatGroup(chat, current);
    // Opening a chat is the obvious moment to start typing.
    await tick();
    if (!current()) return;
    if (jumpToMention && messages.mentionQueue.length > 0) {
      messages.mentionCursor = 1;
      scrollToMessage(messages.mentionQueue[0]);
    }
    composerInput?.focus();
  }


  $effect(() => {
    if (session.connected) void chats.loadGroupKinds();
  });

  async function openUrl(url: string) {
    try {
      await invoke("open_url", { url });
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Deletes downloaded media, keeping the messages. */
  async function flushMedia() {
    try {
      const removed = await invoke<number>("flush_media");
      ui.notify(
        removed > 0
          ? `Removed media from ${removed} message(s).`
          : "There was no downloaded media to remove.",
      );
      await chats.refreshChats();
      if (chats.selectedChat) await messages.reloadMessages(chats.selectedChat);
    } catch (e) {
      ui.fail(e);
    }
  }

  async function clearHistory() {
    try {
      const removed = await invoke<number>("clear_history");
      ui.notify(`Deleted ${removed} message(s) from this computer.`);
      await chats.refreshChats();
      if (chats.selectedChat) await messages.reloadMessages(chats.selectedChat);
    } catch (e) {
      ui.fail(e);
    }
  }

  /** Chat label for a confirm sheet: list name, override or raw JID. */
  function confirmChatLabel(chat: string) {
    return members.displayName(
      chats.chats.find((c) => c.chat === chat)?.display_name ??
      (chat === chats.selectedChat ? chats.titleOverride : null) ??
      null, chat
    );
  }

  /** Clears one chat on this device only; the empty chat stays open. */
  async function doClearChat(chat: string) {
    ui.chatConfirm = null;
    ui.chatSettingsOpen = false;
    const ok = await chats.clearChat(chat);
    if (!ok) return;
    composer.forgetRecovery(chat);
    // Drop per-chat transient state that belonged to the removed messages.
    if (chat === chats.selectedChat) {
      composer.replyingTo = null;
      composer.editing = null;
      messages.firstUnreadId = null;
      messages.lastUnreadId = null;
      messages.mentionQueue = [];
      messages.mentionCursor = 0;
      await messages.reloadMessages(chat);
      await messages.loadMarks(chat);
    }
  }

  /** Deletes one chat on this device only; it leaves the list. */
  async function doDeleteChat(chat: string) {
    ui.chatConfirm = null;
    ui.chatSettingsOpen = false;
    const wasOpen = chat === chats.selectedChat;
    const ok = await chats.deleteChat(chat);
    if (!ok) return;
    composer.forgetRecovery(chat);
    if (wasOpen) {
      messages.acceptMessages([]);
      messages.marks = structuredClone({ reactions: [], starred: [], pinned: null, polls: [], events: [], view_once: [], forwarded: [], edited: [] });
      messages.mentionQueue = [];
      messages.mentionCursor = 0;
      messages.firstUnreadId = null;
      messages.lastUnreadId = null;
      composer.replyingTo = null;
      composer.editing = null;
      delete composer.drafts[chat];
      if (composer.draft && chats.selectedChat === null) composer.draft = "";
    }
  }

  /** Stops showing the video-without-preview warning. */
  async function muteNotice() {
    session.settings.warn_missing_video_preview = false;
    try {
      await invoke("set_settings", { settings: session.settings });
    } catch (e) {
      ui.fail(e);
    }
    ui.notice = null;
  }

  /** Opens a search result, even one with no local history. */
  function openFromSearch(result: SearchResult) {
    chats.clearSearch();
    openChat(result.jid, false, result.name);
  }

  /** Scrolls a message into view by its id, and highlights it briefly. */
  function scrollToMessage(id: string) {
    if (!messageList?.revealMessage(id)) {
      const element = scroller?.querySelector(`[data-id="${id}"]`);
      if (!element) return;
      element.scrollIntoView({ block: "center" });
    }
    ui.highlightedId = id;
    window.setTimeout(() => {
      if (ui.highlightedId === id) ui.highlightedId = null;
    }, 1600);
  }

  /** Brings the unread divider to the top once, once the rows exist. */
  function pinUnreadDivider(chat: string) {
    void tick().then(() => {
      if (chats.selectedChat === chat) messageList?.scrollToUnread();
    });
  }

/** Jumps to the next unread mention, oldest to newest, wrapping around. */
  function jumpNextMention() {
    if (messages.mentionQueue.length === 0) return;
    const id = messages.mentionQueue[messages.mentionCursor % messages.mentionQueue.length];
    messages.mentionCursor = (messages.mentionCursor + 1) % messages.mentionQueue.length;
    scrollToMessage(id);
  }

  $effect(() => {
    if (!session.connected) return;
    for (const chat of chats.chats) chats.loadAvatar(chat.chat);
  });

  /** A view-once copy from a reply, shown alone in the built-in viewer. */
  let quoteView = $state<ViewerItem[] | null>(null);

  /** The direct chat whose contact panel is open. */
  let contactInfoFor = $state<string | null>(null);
  let broadcastFor = $state<{ account: string; chat: string; generation: number } | null>(null);
  let broadcastInfo = $state<BroadcastList | null>(null);
  let broadcastLoading = $state(false), broadcastError = $state<string | null>(null);
  $effect(() => {
    const scope = broadcastFor;
    const account = session.activeAccount, chat = chats.selectedChat, generation = messages.accountGeneration;
    void messages.messages; void messages.marks; void chats.chats;
    if (!scope || scope.account !== account || scope.chat !== chat || scope.generation !== generation) {
      untrack(() => { broadcastInfo = null; broadcastLoading = false; broadcastError = null; });
      return;
    }
    let current = true;
    untrack(() => {
      broadcastInfo = null; broadcastError = null; broadcastLoading = true;
      void invoke<BroadcastList | null>("broadcast_list", { accountId: scope.account, chat: scope.chat })
        .then((info) => { if (current) broadcastInfo = info; })
        .catch((error) => { if (current) broadcastError = String(error); })
        .finally(() => { if (current) broadcastLoading = false; });
    });
    return () => { current = false; };
  });
  function openChatInfo(chat: string) {
    if (chat.endsWith("@g.us")) { void chats.openGroupInfo(); return; }
    if (isBroadcastList(chat) && session.activeAccount) {
      broadcastFor = { account: session.activeAccount, chat, generation: messages.accountGeneration };
    } else contactInfoFor = chat;
  }
  let newContact = $state(false);
  let quickSwitcher = $state(false);
  $effect(() => { session.activeAccount; newContact = quickSwitcher = false; });
  $effect(() => {
    const account = session.activeAccount;
    messages.accountGeneration;
    untrack(() => { quickReplies.reset(); if (account) void quickReplies.refresh(); });
  });

  function openProfile(jid: string, name: string, event: MouseEvent, self = false) {
    event.stopPropagation();
    if (chats.selectedChat?.endsWith("@g.us")) {
      ui.profileCard = null;
      memberSheet.open(chats.selectedChat, bare(jid), name);
      return;
    }
    // Pills and names render from cache only; the click is what fetches.
    chats.loadAvatar(bare(jid));
    ui.profileCard = { jid: bare(jid), name, x: event.clientX, y: event.clientY, self };
  }

  $effect(() => {
    const scope = memberSheet.scope;
    if (scope && (session.activeAccount !== scope.account || chats.selectedChat !== scope.group
      || messages.accountGeneration !== scope.generation)) memberSheet.close();
  });

  $effect(() => {
    if (!session.connected) return;
    for (const account of session.accountList) if (account.jid) chats.loadAvatar(account.jid);
  });

  // Group members get their picture next to their messages.
  $effect(() => {
    if (!session.connected || !chats.selectedChat?.endsWith("@g.us")) return;
    // Recent senders only: a group with hundreds of members must not fire a
    // picture request for every sender the moment the chat opens.
    for (const message of messages.messages.slice(-80)) {
      if (!message.from_me) chats.loadAvatar(bare(message.sender));
    }
  });

  $effect(() => {
    const own = session.activeAccount && session.me ? chats.avatars[session.me] : null;
    if (!own) return;
    try {
      localStorage.setItem(`postal.avatar.${session.activeAccount}`, own);
    } catch {
      // Storage may be unavailable; the picture then only shows once connected.
    }
  });


  $effect(() => {
    if (session.connected) session.setOnline(document.hasFocus());
  });

  $effect(() => {
    if (!session.connected) return;
    void session.loadPrivacy();
  });


  $effect(() => {
    if (!session.connected || session.me) return;
    invoke<string | null>("own_jid")
      .then((jid) => {
        session.me = jid;
        if (jid) chats.loadAvatar(jid);
        // The backend just recorded the JID, and named the account if it was
        // still on the default label; pick both up.
        return session.loadAccounts();
      })
      .catch(() => {});
  });

  function scrollToBottom() {
    if (!messages.atLatest && chats.selectedChat) {
      void messages.showLatest(chats.selectedChat).then((loaded) => { if (loaded) scrollToBottom(); });
      return;
    }
    // Wait for the new rows to land before scrolling to the last one.
    void tick().then(() => {
      messageList?.scrollToBottom();
      ui.scrolledUp = false;
    });
  }

  // Element access the composer domain cannot own; the module calls back here.
  composer.host = {
    scrollToBottom,
    focusComposer: () => composerInput?.focus(),
  };

  // The typing bubble coming and going moves the bottom; stay pinned to it.
  $effect(() => {
    if (!chats.selectedChat) return;
    void members.typing[chats.selectedChat]?.length;
    if (messages.atLatest && !untrack(() => ui.scrolledUp)) scrollToBottom();
  });

  /** The virtual list's scroll metrics: follow, page older, and mark read. */
  function onScroll({ offset, distance }: { offset: number; distance: number; viewport: number }) {
    const scrolledUp = !messages.atLatest || distance > 120;
    if (ui.scrolledUp !== scrolledUp) ui.scrolledUp = scrolledUp;
    if (
      offset < 80 &&
      messages.loadOnScroll &&
      !messages.olderExhausted &&
      !messages.loadingOlder &&
      !messages.loadingNewer &&
      messages.messages.length > 0
    ) {
      void messages.loadOlder(chats.selectedChat, true);
    } else if (
      distance < 80 &&
      !messages.atLatest &&
      !messages.loadingOlder &&
      !messages.loadingNewer &&
      messages.messages.length > 0
    ) {
      void messages.loadNewer(chats.selectedChat);
    }
    scheduleReadMarking();
  }

  /**
   * Advances the read marker to the oldest message still in view, throttled.
   *
   * Marking up to that id rather than the whole chat leaves messages below the
   * fold unread, which is what makes the divider meaningful.
   */
  function scheduleReadMarking() {
    clearTimeout(messages.readMarkTimer);
    messages.readMarkTimer = setTimeout(() => {
      if (!scroller || !chats.selectedChat || !document.hasFocus()) return;
      const chat = chats.selectedChat;
      const ids = messages.ordered;
      const visible = messageList?.visibleReadIds() ?? [];
      const candidate = visibleReadFrontier(ids, visible);
      if (!candidate || candidate === messages.lastMarkedId) return;
      messages.lastMarkedId = candidate;
      const firstIdx = messages.firstUnreadId
        ? ids.findIndex((m) => m.id === messages.firstUnreadId)
        : -1;
      // Without a recorded newest unread, fall back to the first.
      const lastIdx = messages.lastUnreadId
        ? ids.findIndex((m) => m.id === messages.lastUnreadId)
        : -1;
      const targetIdx = lastIdx >= 0 ? lastIdx : firstIdx;
      const markedIdx = ids.findIndex((m) => m.id === candidate);
      invoke<number>("mark_read_until", { chat, id: candidate })
        .then((changed) => {
          if (changed > 0) queueRefreshChats();
          // The divider stays until the newest unread is read too, so it does
          // not vanish the moment the first unread scrolls into view.
          if (firstIdx >= 0 && markedIdx >= targetIdx) {
            messages.firstUnreadId = null;
            messages.lastUnreadId = null;
          }
        })
        .catch(() => {});
    }, 200);
  }



  /**
   * Opens the chat a quoted message lives in and jumps to it. A private reply
   * is a direct message quoting a group message, so the target is often in a
   * different chat.
   */
  async function jumpToQuoted(message: StoredMessage) {
    const id = message.reply_to_id;
    if (!id) return;
    let chat = message.reply_to_chat;
    if (!chat) {
      try {
        chat = await invoke<string | null>("chat_for_message", { id });
      } catch {
        chat = null;
      }
    }
    if (chat ?? chats.selectedChat) await jumpTo(chat ?? chats.selectedChat!, id);
  }

  /** Takes back the view-once a reply quotes, then opens the recovered copy. */
  async function recoverQuote(message: StoredMessage) {
    const path = await messages.recoverQuote(chats.selectedChat, message);
    if (path) openQuote(message, path);
  }

  /** Shows the view-once copy a reply carries in the built-in viewer. */
  function openQuote(m: StoredMessage, path: string) {
    quoteView = [
      {
        id: m.id,
        path,
        thumb: null,
        kind: m.reply_to_kind ?? "image",
        caption: "",
        author: m.reply_to_sender === "@me" ? "You" : members.senderName(m.reply_to_sender ?? ""),
        avatar: null,
        timestamp: m.timestamp,
      },
    ];
  }

  /** Opens a chat at a message; one older than the loaded window offers to fetch it. */
  let jumpSeq = 0;
  async function jumpTo(chat: string, id: string) {
    const account = session.activeAccount, generation = messages.accountGeneration, request = ++jumpSeq;
    if (!account) return;
    const current = () => account === session.activeAccount && generation === messages.accountGeneration && request === jumpSeq;
    ui.pendingJump = null;
    ui.seeking = false;
    if (chat !== chats.selectedChat) await openChat(chat);
    if (!current() || chats.selectedChat !== chat) return;
    await tick();
    if (!current() || chats.selectedChat !== chat) return;
    if (messageList?.hasMessage(id)) scrollToMessage(id);
    else {
      ui.pendingJump = { chat, id };
      await loadAndJump();
    }
  }

  /** Walks the chat's past back from the phone until the pending jump's message lands. */
  async function loadAndJump() {
    if (!ui.pendingJump) return;
    const pending = ui.pendingJump, { chat, id } = pending;
    const account = session.activeAccount, generation = messages.accountGeneration, request = jumpSeq;
    const owns = () => account === session.activeAccount && generation === messages.accountGeneration
      && request === jumpSeq && ui.pendingJump === pending;
    const current = () => owns() && chats.selectedChat === chat;
    if (!account || !current()) return;
    ui.seeking = true;
    try {
      const stored = await messages.showStoredMessage(chat, id);
      if (!current()) return;
      if (stored) {
        await tick();
        if (!current()) return;
        const row = messages.messages.find((message) => message.id === id);
        if (row && keywords.hidden(row)) { ui.fail("This message is hidden by your keyword rules."); return; }
        scrollToMessage(id);
        return;
      }
      for (let round = 0; round < 10 && current(); round++) {
        const before = messages.messages.at(-1)?.id;
        await messages.recallDay(chat);
        if (!current()) return;
        await messages.showStoredMessage(chat, id);
        if (!current()) return;
        await tick();
        if (!current()) return;
        const row = messages.messages.find((message) => message.id === id);
        if (row && keywords.hidden(row)) { ui.fail("This message is hidden by your keyword rules."); return; }
        if (messageList?.hasMessage(id)) {
          scrollToMessage(id);
          return;
        }
        if (messages.messages.at(-1)?.id === before) break;
      }
      if (current()) ui.fail("Your phone did not send that message; it may be older than it keeps, or deleted.");
    } catch (e) {
      if (current()) ui.fail(e);
    } finally {
      if (owns()) { ui.seeking = false; ui.pendingJump = null; }
    }
  }


  let markingAllRead = $state(false);
  async function blockContact(jid: string) {
    const account = session.activeAccount, generation = messages.accountGeneration;
    if (!account) return;
    const current = () => account === session.activeAccount && generation === messages.accountGeneration;
    try {
      await composer.enqueue((signal) => {
        if (signal.aborted || !current()) throw new Error("account changed before operation");
        return invoke("set_contact_blocked", { account, jid, blocked: true });
      });
      if (current()) ui.notify("Contact blocked.");
    } catch (error) { if (current()) ui.fail(error); }
  }
  async function markAllRead() {
    const accountId = session.activeAccount;
    if (!accountId || markingAllRead) return;
    const generation = messages.accountGeneration;
    const current = () => accountId === session.activeAccount && generation === messages.accountGeneration;
    markingAllRead = true;
    try {
      const results = await composer.enqueue(() => invoke<import("$lib/utils/wire").MarkReadResult[]>("mark_all_read", { accountId }));
      if (!current()) return;
      const failures = formatBulkReadFailures(results, (chat) => {
        const known = chats.chats.find((item) => item.chat === chat);
        return known ? chats.chatLabel(known) : members.displayName(null, chat);
      });
      if (failures.length) ui.fail(failures.join("\n"));
      if (results.some((result) => result.chat === chats.selectedChat && !result.error)) {
        messages.firstUnreadId = null;
        messages.lastUnreadId = null;
      }
    } catch (error) {
      if (current()) ui.fail(error);
    } finally {
      try { if (current()) await chats.refreshChats(); }
      finally { markingAllRead = false; }
    }
  }

  async function create(value: unknown) {
    const kind = ui.creating, accountId = session.activeAccount, chat = chats.selectedChat;
    const generation = messages.accountGeneration;
    const current = () => accountId === session.activeAccount && chat === chats.selectedChat
      && generation === messages.accountGeneration;
    if (!kind || !accountId || !chat) throw new Error("Conversation changed before creating.");
    const reason = broadcastSendReason(chat);
    if (reason) throw new Error(reason);
    const payload: Record<string, unknown> = { ...(value as object) };
    if (Array.isArray(payload.options)) payload.options = Object.freeze([...payload.options]);
    Object.freeze(payload);
    const quiz = kind === "poll" && Object.hasOwn(payload, "correctIndex");
    const correct = payload.correctIndex;
    if (quiz && (!Array.isArray(payload.options) || typeof correct !== "number" || !Number.isInteger(correct)
      || correct < 0 || correct >= payload.options.length)) throw new Error("Invalid quiz correct answer.");
    const command = kind === "event" ? "create_event" : quiz ? "create_quiz" : "create_poll";
    const args = kind === "poll" ? { ...payload, accountId, chat } : { accountId, chat, event: payload };
    await composer.enqueue(async (signal) => {
      signal.throwIfAborted();
      if (!current()) throw new Error("Conversation changed before creating.");
      await invoke(command, args);
    });
    if (!current()) return;
    try {
      await messages.reloadMessages(chat);
      if (!current()) return;
      await messages.loadMarks(chat);
      if (!current()) return;
      await chats.refreshChats();
      if (current()) scrollToBottom();
    } catch (error) {
      if (current()) ui.fail(error);
    }
  }

  async function votePoll(message: StoredMessage, options: string[]) {
    const accountId = session.activeAccount, chat = message.chat, id = message.id;
    const generation = messages.accountGeneration, selected = [...options];
    const current = () => accountId === session.activeAccount && chat === chats.selectedChat
      && generation === messages.accountGeneration;
    if (!accountId || !current()) throw new Error("Conversation changed before voting.");
    const reason = broadcastSendReason(chat);
    if (reason) throw new Error(reason);
    await composer.enqueue(async (signal) => {
      signal.throwIfAborted();
      if (!current()) throw new Error("Conversation changed before voting.");
      await invoke("vote_poll", { accountId, chat, id, options: selected });
    });
    if (!current()) return;
    try {
      await messages.loadMarks(chat);
      if (!current()) return;
      await chats.refreshChats();
    } catch (error) {
      if (current()) ui.fail(error);
    }
  }

  async function respondEvent(message: StoredMessage, response: string) {
    const accountId = session.activeAccount, chat = message.chat, id = message.id;
    const generation = messages.accountGeneration;
    const reason = broadcastSendReason(chat);
    if (reason) throw new Error(reason);
    const current = () => !!accountId && accountId === session.activeAccount && chat === chats.selectedChat
      && generation === messages.accountGeneration;
    if (!current()) throw new Error("Conversation changed before responding.");
    await composer.enqueue(async (signal) => {
      signal.throwIfAborted();
      if (!current()) throw new Error("Conversation changed before responding.");
      await invoke("respond_event", { accountId, chat, id, response });
    });
  }

  /** Pinned bar content for the chat header. */
  const pinnedView = $derived.by(() => {
    const m = messages.pinnedMessage;
    if (!m) return null;
    return {
      id: m.id,
      author: m.from_me ? "You" : members.senderLabel(m),
      body: m.media_kind
        ? plain(captionOf(m), (user) => members.mentionName(user)) || MEDIA_LABELS[m.media_kind]
        : plain(m.text, (user) => members.mentionName(user)),
    };
  });


  function menuItems(message: StoredMessage): MenuItem[] {
    return messageMenuItems(message, openChat);
  }

  const QUICK_REACTIONS = ["👍", "❤️", "😂", "😮", "😢", "🙏"];

  /** Picker targets remain readable during teardown. */
  const emojiChat = $derived(ui.emojiFor?.messages[0]?.chat ?? "");
  const emojiAnchor = $derived(ui.emojiFor ? { x: ui.emojiFor.x, y: ui.emojiFor.y } : null);
  function openEmojiFor() {
    const anchor = ui.menu;
    if (anchor) ui.emojiFor = { messages: [anchor.message], x: anchor.x, y: anchor.y };
  }
  function pickCustomReaction(emoji: string) {
    const batch = ui.emojiFor?.messages;
    ui.emojiFor = null;
    if (!batch?.length) return;
    const mine = batch.length === 1 ? messages.reactionsFor.get(batch[0].id)?.find((r) => r.mine)?.emoji : null;
    void reactMessages(batch, mine === emoji ? "" : emoji);
  }
  /** Takes back our own reaction from the Reactions dialog. */
  function removeOwnReaction() {
    const m = ui.reactionsFor;
    if (!m) return;
    void reactMessages([m], "");
  }
  // Later readers in a group change no status, so the open info refreshes itself.
  $effect(() => {
    if (!ui.infoFor) return;
    const timer = setInterval(() => (ui.infoVersion += 1), 3000);
    return () => clearInterval(timer);
  });

  /** The open chat's downloaded pictures and videos, oldest first, for the viewer. */
  const viewOnceIds = $derived(new Set(messages.marks.view_once.map((v) => v.id)));
  function viewerItem(m: StoredMessage): ViewerItem {
    const selectedChat = chats.selectedChat;
    const who = m.from_me ? session.me : selectedChat?.endsWith("@g.us") ? bare(m.sender) : selectedChat;
    return {
      id: m.id,
      path: m.media_path!,
      thumb: m.media_thumb,
      kind: m.media_kind!,
      caption: plain(captionOf(m), (user) => members.mentionName(user)),
      author: m.from_me ? "You" : members.senderLabel(m),
      avatar: who ? (chats.avatars[who] ?? null) : null,
      timestamp: m.timestamp,
    };
  }
  const viewerItems = $derived<ViewerItem[]>(viewableMessages(messages.ordered.filter((message) => !keywords.hidden(message)), viewOnceIds).map(viewerItem));
  function viewerPosition() { return viewerItems.findIndex((item) => item.id === ui.viewerId); }
  function setViewerPosition(index: number) { ui.viewerId = viewerItems[index]?.id ?? null; }
  $effect(() => { if (ui.viewerId !== null && viewerPosition() < 0) ui.viewerId = null; });
  async function closeViewOnce() {
    const message = ui.onceOpen;
    ui.onceOpen = null;
    if (!message) return;
    try {
      await invoke("open_view_once", { chat: message.chat, id: message.id });
      messages.markPlayed(message);
    } catch (e) {
      ui.fail(e);
    }
    await messages.reloadMessages(chats.selectedChat);
    await messages.loadMarks(chats.selectedChat);
  }

  function openViewer(message: StoredMessage) {
    if (viewerItems.some((item) => item.id === message.id)) ui.viewerId = message.id;
  }

  /** Opens a downloaded media file in the desktop's default application. */
  async function openMedia(path: string) {
    if (/\.svg$/i.test(path)) return;
    try {
      await invoke("open_path", { path });
    } catch (e) {
      ui.fail(e);
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;
    let unlistenOnce: (() => void) | undefined;

    // Surface anything that escapes a handler, so a failure shows a message
    // rather than leaving the interface silently unresponsive.
    const onError = (event: ErrorEvent) => {
      // virtua measures rows with ResizeObserver, and the browser reports the
      // harmless loop warning as a window error. Ignore it, or every render
      // paints an error banner over the interface.
      if (event.message?.startsWith("ResizeObserver loop")) return;
      ui.fail(event.message || "Unexpected error");
    };
    const onRejection = (event: PromiseRejectionEvent) => {
      ui.fail(event.reason ?? "Unexpected error");
    };
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);

    // Clicking a desktop notification opens its chat.
    const onOpenChat = (event: Event) => {
      const target = desktopChatTarget((event as CustomEvent<unknown>).detail, session.activeAccount);
      if (target && session.connected) void openChat(target.chat);
    };
    window.addEventListener("postal:open-chat", onOpenChat);
    const desktopListener = listen<unknown>("desktop-open-chat", (event) => {
      const target = desktopChatTarget(event.payload, session.activeAccount);
      if (target && session.connected) void openChat(target.chat);
    });

    // Typing anywhere lands in the composer, so a chat can be answered without
    // clicking the field first.
    const onAnyKey = (event: KeyboardEvent) => {
      if (!event.altKey && !event.shiftKey && (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (session.activeAccount && session.started) quickSwitcher = !quickSwitcher;
        return;
      }
      if (event.defaultPrevented || quickSwitcher) return;
      // Ctrl +/-/0 resize the whole interface, whether or not a chat is open.
      if (event.ctrlKey) {
        if (event.key === "=" || event.key === "+") {
          event.preventDefault();
          session.setZoom(session.zoom + 0.1);
          return;
        }
        if (event.key === "-") {
          event.preventDefault();
          session.setZoom(session.zoom - 0.1);
          return;
        }
        if (event.key === "0") {
          event.preventDefault();
          session.setZoom(1);
          return;
        }
      }
      // Jump to the unread divider, wherever focus is.
      if (chats.selectedChat && messages.firstUnreadId && matches(event, keybinds.jumpUnread)) {
        event.preventDefault();
        scrollToMessage(messages.firstUnreadId);
        return;
      }
      if (!chats.selectedChat || !composerInput) return;
      if (event.ctrlKey || event.metaKey || event.altKey) return;
      const target = event.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" || target.tagName === "TEXTAREA" || target.isContentEditable)
      ) {
        return;
      }
      // Staged attachments go out on Enter even when focus left the composer.
      if (
        event.key === "Enter" &&
        !event.shiftKey &&
        composer.pending.length > 0 &&
        target?.tagName !== "BUTTON" &&
        !target?.closest?.("[role=dialog]")
      ) {
        event.preventDefault();
        void composer.send();
        return;
      }
      if (event.key.length !== 1) return;
      composerInput.focus();
    };
    window.addEventListener("keydown", onAnyKey);

    // View callbacks the event dispatcher cannot own (scrolling, reconnecting).
    const host = {
      scrollToBottom,
      anchor: () => messageList?.anchorId() ?? null,
      reveal: (id: string) => { messageList?.revealMessage(id); },
      reconnect,
    };

    async function setup() {
      await session.loadSettings();
      await once.refresh();

      // The listener is attached before connecting so no event can be missed.
      unlisten = await listen<ServiceEvent>("service-event", (event) =>
        dispatchServiceEvent(event.payload, host),
      );
      unlistenOnce = await listen<ServiceEvent>("once-event", (event) => void once.noteEvent(event.payload));

      // Reuse a stored session automatically: pairing is only needed the very
      // first time, so the button should never be shown to a paired account.
      // With several linked accounts the user picks one first.
      await session.loadAccounts();
      await syncState();
      if (!session.started && session.accountList.filter((a) => a.jid).length > 1) {
        session.choosingAccount = true;
      } else {
        await connect();
        await session.loadAccounts();
      }
    }

    setup();

    return () => {
      unlisten?.();
      unlistenOnce?.();
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
      window.removeEventListener("keydown", onAnyKey);
      window.removeEventListener("postal:open-chat", onOpenChat);
      void desktopListener.then((unlisten) => unlisten()).catch((error) => ui.fail(error));
    };
  });
</script>

<svelte:head>
  <title>Postal</title>
</svelte:head>

<ThemeLayers />
<ScheduledOutbox
  enqueue={<T>(task: (signal: AbortSignal) => Promise<T>) => composer.enqueue(task)}
  displayName={(chat) => members.displayName(chats.chats.find((item) => item.chat === chat)?.display_name ?? null, chat)} />

<!-- The glass lens: shifts the backdrop by lensMap, strongest at the rim. The map stretches to
     each element; the shift is in pixels, so short pills take a smaller one to avoid smearing. -->
<svg width="0" height="0" aria-hidden="true" style="position: absolute">
  {#each [{ id: "glass-lg", scale: 70 }, { id: "glass-md", scale: 38 }, { id: "glass-sm", scale: 22 }] as { id, scale } (id)}
    <filter {id} x="0%" y="0%" width="100%" height="100%" color-interpolation-filters="sRGB">
      <feImage href={lensMap("x")} x="0%" y="0%" width="100%" height="100%" preserveAspectRatio="none" result="dx" />
      <feImage href={lensMap("y")} x="0%" y="0%" width="100%" height="100%" preserveAspectRatio="none" result="dy" />
      <feComposite in="dx" in2="dy" operator="arithmetic" k2="1" k3="1" result="map" />
      <feDisplacementMap in="SourceGraphic" in2="map" {scale} xChannelSelector="R" yChannelSelector="G" />
    </filter>
  {/each}
</svg>

<!-- Avatar rendering lives in $lib/ui/Avatar.svelte (initials/hue in $lib/utils/avatar.ts). -->

<!-- Window-level so a paste/drop anywhere cannot navigate the webview. -->
<svelte:window
  onpaste={(event) => { if (!isBroadcastList(chats.selectedChat)) onPaste(event); }}
  onclick={(e) => {
    if (ui.accountMenu && !(e.target as Element).closest?.(".user-panel")) ui.accountMenu = false;
  }}
  oncontextmenu={(e) => {
    // The webview's own menu (Back, Refresh, Inspect) is meaningless here; keep
    // it only where it helps: text fields and a text selection.
    const el = e.target as HTMLElement;
    const editable = el.closest?.("input, textarea, [contenteditable]");
    if (!editable && !window.getSelection()?.toString()) e.preventDefault();
  }}
  onfocus={() => session.setOnline(true)}
  onblur={() => session.setOnline(false)}
  ondragover={(e) => e.preventDefault()}
  ondrop={(event) => {
    if (isBroadcastList(chats.selectedChat)) { event.preventDefault(); return; }
    onDrop(event);
  }}
/>

{#if ui.error}
  <div class="error" role="alert">
    <span>{ui.error}</span>
    <Button variant="icon" icon="x" iconSize={16} title="Dismiss" aria-label="Dismiss" onclick={() => (ui.error = null)} />
  </div>
{/if}

<div class="app">
{#if !session.connected || !session.uiUnlocked}
  <PairingView
    qrSvg={session.qrSvg}
    started={session.started}
    connecting={session.connecting}
    connected={session.connected}
    choosingAccount={session.choosingAccount}
    accounts={session.accountList}
    activeAccount={session.activeAccount}
    accountAvatars={chats.accountAvatars}
    linked={!session.qrSvg
      ? session.accountList.find((a) => a.id === session.activeAccount && a.jid)
      : undefined}
    finalizing={session.finalizing}
    syncPending={session.syncPending}
    syncApplied={session.syncApplied}
    syncPercent={session.syncPercent}
    syncTimedOut={session.syncTimedOut}
    pairCode={session.pairCode}
    pairCodeExpiresAt={session.pairCodeExpiresAt}
    pairCodeError={session.pairCodeError}
    pairCodeManual={session.pairCodeManual}
    pairCodeBusy={session.pairCodeBusy}
    onconnect={connect}
    onchoose={chooseAccount}
    onswitch={switchTo}
    onsettings={() => openSettings("accounts")}
    onrequestpaircode={(phone) => void session.requestPairCode(phone)}
    onrefreshpaircode={() => void session.refreshPairCode()}
    oncancelpaircode={() => void session.cancelPairCode()} />
{:else}
  <div class="layout" style="grid-template-columns: {layoutColumns}">
    <ChatSidebar
      bind:searchQuery={chats.searchQuery}
      searchResults={chats.searchResults}
      {visibleChats}
      selectedChat={chats.selectedChat}
      chatFilter={chats.chatFilter}
      favoriteChats={favorites.chats}
      favoriteBusy={favorites.busy !== null}
      ontogglefavorite={(chat) => void act(() => composer.enqueue(() => favorites.toggle(chat.chat)))}
      onfilter={(filter) => (chats.chatFilter = filter)}
      unreadChats={chats.unreadChats}
      {unreadPings}
      avatars={chats.avatars}
      chatLabelOf={(chat) => chats.chatLabel(chat)}
      {formatTime}
      typingLabelOf={(chat) => members.typingLabel(chat)}
      previewAuthorOf={(chat) => chats.previewAuthor(chat)}
      previewTextOf={(chat) => chats.previewText(chat)}
      mediaIconOf={(kind) => chats.mediaIcon(kind)}
      groupKinds={chats.groupKinds}
      accounts={session.accountList}
      activeAccount={session.activeAccount}
      activeLabel={session.activeLabel}
      accountAvatars={chats.accountAvatars}
      me={session.me}
      meVersion={session.meVersion}
      visibility={session.visibility}
      accountMenu={ui.accountMenu}
      onmenutoggle={() => (ui.accountMenu = !ui.accountMenu)}
      onswitchaccount={(id) => {
        ui.accountMenu = false;
        void switchTo(id);
      }}
      onaddaccount={() => {
        ui.accountMenu = false;
        void addAccount();
      }}
      onsettings={openSettings}
      onpings={() => openPings(null)}
      onstarred={openStarred}
      onsearch={() => chats.runSearch()}
      onopenresult={openFromSearch}
      onopenchat={openChat}
      ontogglepin={(chat, e) => chats.togglePin(chat, e)}
      onclearchat={(chat) => (ui.chatConfirm = { kind: "clear", chat: chat.chat })}
      ondeletechat={(chat) => (ui.chatConfirm = { kind: "delete", chat: chat.chat })}
      onchataction={(command, args) => chats.chatAction(command, args)}
      globalAutoDownload={session.settings.auto_download_types}
      onmarkallread={markAllRead}
      onnewgroup={() => (ui.newGroup = true)}
      onnewcontact={() => (newContact = true)}
      oninbox={() => { ui.showInbox = !ui.showInbox; void labels.refresh(); }}
      onlabels={() => { ui.manageLabels = true; void labels.refresh(); }}
      onchatlabels={(chat) => { ui.labelTargets = [{ chat }]; void labels.refresh(); }}
      bind:labelFilter={chats.labelFilter}
      canCreateGroup={!!session.activeAccount && session.connected}
      onblockcontact={blockContact}
      {markingAllRead}
      onmarkread={(chat) => {
        // Reading the whole chat from the list clears the divider with it.
        if (chat.chat === chats.selectedChat) {
          messages.firstUnreadId = null;
          messages.lastUnreadId = null;
        }
        return chats.chatAction("mark_read", { chat: chat.chat });
      }}
      archivedChats={chats.archivedChats}
      freezeOnHover={session.settings.freeze_chat_list_on_hover ?? false}
      chatPreview={session.settings.chat_preview ?? true}
      onresize={startResize} />

    <section class="conversation">
      {#if ui.showInbox}
        <UnifiedInbox account={session.activeAccount} requestKey={messages.accountGeneration} connected={session.connected}
          chats={inboxChats} labels={labels.loaded ? labels.view.labels : null} {labelsByChat}
          labelsWritable={session.connected && labels.loaded && !labels.busy} labelsLoading={labels.loading} labelsError={labels.error ?? ""} labelsComplete={labels.view.complete}
          chatLabelOf={(chat) => chats.chatLabel(chat)} avatarOf={(jid) => chats.avatars[jid] ?? null}
          previewTextOf={(chat) => chats.previewText(chat)} {formatTime}
          syncPending={session.syncPending} syncApplied={session.syncApplied} historyPercent={session.historyPercent}
          backfill={session.backfill} finalizing={session.finalizing}
          onopen={(chat, mention) => { ui.showInbox = false; void openChat(chat, mention); }}
          onaction={inboxAction} onretry={() => { void chats.refreshChats(); void labels.refresh(); }} />
      {:else if chats.selectedChat}
        {@const selectedChat = chats.selectedChat}
        {@const storedTitle = chats.chats.find((c) => c.chat === selectedChat)?.display_name ?? chats.titleOverride}
        {@const title = isBroadcastList(selectedChat) ? storedTitle || "Broadcast list" : members.displayName(storedTitle, selectedChat)}
        {@const typingNow = isBroadcastList(selectedChat) ? null : members.typingLabel(selectedChat)}
        <ChatHeader
          {selectedChat}
          isGroup={selectedChat.endsWith("@g.us")}
          isBroadcast={isBroadcastList(selectedChat)}
          {title}
          avatar={chats.avatars[selectedChat] ?? null}
          {typingNow}
          {subtitle}
          groupContext={members.groupContext}
          presenceText={isBroadcastList(selectedChat) ? null : members.presenceLabel(selectedChat)}
          mentionTotal={messages.mentionQueue.length}
          mentionCursor={messages.mentionCursor}
          pinned={pinnedView}
          ongroupinfo={() => openChatInfo(selectedChat)}
          onsearch={() =>
            (ui.finder = {
              mode: "search",
              chat: selectedChat,
              items: [],
              reach: messages.messages.at(-1)?.timestamp ?? null,
              more: !messages.olderExhausted,
            })}
          onpings={() => openPings(selectedChat)}
          ongallery={() => (galleryChat = selectedChat)}
          onsettings={() => (ui.chatSettingsOpen = true)}
          onjumpmention={jumpNextMention}
          onpinnedjump={(id) => scrollToMessage(id)}
          onclearchat={() => (ui.chatConfirm = { kind: "clear", chat: selectedChat })}
          ondeletechat={() => (ui.chatConfirm = { kind: "delete", chat: selectedChat })} />

        <div class="list-wrap">
        <MessageList
          messages={messages.ordered}
          isGroup={selectedChat.endsWith("@g.us")}
          switching={ui.switching}
          bind:scroller
          bind:this={messageList}
          prepending={messages.prepending}
          {dayKey}
          {dayLabel}
          senderLabel={(m) => members.senderLabel(m)}
          memberTagOf={(sender) => members.memberOf(sender)?.label ?? null}
          {hue}
          {captionOf}
          viewOnceMarks={messages.marks.view_once}
          reactionsFor={messages.reactionsFor}
          starredSet={messages.starred}
          editedSet={messages.edited}
          forwardedSet={messages.forwarded}
          downloading={messages.downloading}
          downloadErrors={messages.downloadErrors}
          downloadTries={messages.downloadTries}
          replyingToId={composer.replyingTo?.id ?? null}
          highlightedId={ui.highlightedId}
          firstUnreadId={messages.firstUnreadId}
          onjumpunread={(id) => scrollToMessage(id)}
          menuId={ui.menu?.message.id ?? null}
          picking={ui.picking}
          onpick={(m) => {
            const next = { ...(ui.picking ?? {}) };
            if (next[m.id]) delete next[m.id];
            else next[m.id] = m;
            ui.picking = next;
          }}
          polls={messages.marks.polls}
          events={messages.marks.events}
          namer={(jid) => (members.isMe(jid) ? "You" : members.senderName(jid))}
          avatarOf={(jid) => chats.pictureOf(jid)}
          avatars={chats.avatars}
          voiceAvatarOf={(m) => {
            const voiceFrom = m.from_me ? session.me : bare(m.sender);
            return voiceFrom ? chats.pictureOf(voiceFrom) : null;
          }}
          quoteAuthorOf={(sender) => members.quoteAuthor(sender)}
          quoteTextOf={(m) => (m.reply_to_text ? plain(m.reply_to_text, (user) => members.mentionName(user)) : null)}
          quoteChatNameOf={(m) =>
            m.reply_to_chat && m.reply_to_chat !== selectedChat ? chats.chatName(m.reply_to_chat) : null}
          autoplayId={messages.autoplayId}
          onceAudioOpenId={ui.onceOpen?.id ?? null}
          loadingOlder={messages.loadingOlder}
          uploads={composer.outgoing.filter((o) => o.chat === selectedChat)}
          typers={members.typing[selectedChat] ?? []}
          typerLabelOf={(sender) => {
            const member = members.memberOf(sender);
            return members.displayName(member?.name ?? null, sender);
          }}
          onscroll={onScroll}
          toWire={(text) => members.asWireMentions(text)}
          targetOf={(user) => members.mentionTarget(user)}
          onprofile={openProfile}
          onopenurl={openUrl}
          onopenchat={openChat}
          {formatTime}
          onreplydraft={(m) => {
            if (isUnavailable(m)) return;
            composer.editing = null;
            composer.replyingTo = m;
            composerInput?.focus();
          }}
          onmenu={(e, m) => {
            e.preventDefault();
            if (isUnavailable(m)) return;
            ui.menu = { x: e.clientX, y: e.clientY, message: m };
          }}
          onjumpquoted={jumpToQuoted}
          onrecoverquote={recoverQuote}
          recovering={messages.recovering}
          revealedOnce={messages.revealedOnce}
          onrevealonce={(m) => messages.revealOnce(m.id)}
          ondownload={(m) => messages.downloadMedia(chats.selectedChat, m)}
          onopenviewer={openViewer}
          onopenmedia={openMedia}
          onopenquote={(m) => openQuote(m, m.reply_to_path!)}
          onvote={votePoll}
          onrespond={(m, response) =>
            act(() => respondEvent(m, response))}
          oneditrequest={(m) => {
            const event = messages.marks.events.find((e) => e.id === m.id);
            if (event) ui.editingEvent = { chat: m.chat, event };
          }}
          oncancelevent={(m) => {
            const event = messages.marks.events.find((e) => e.id === m.id);
            if (event) return saveEvent(m.chat, m.id, { ...eventFields(event), canceled: true });
          }}
          onreact={(m, emoji) => reactMessages([m], emoji)}
          onopenreactions={(m) => (ui.reactionsFor = m)}
          onmarkplayed={(m) => messages.markPlayed(m)}
          onnextvoice={(m) => {
            // A note left playing in another chat has nothing to chain to.
            if (chats.selectedChat === m.chat && messages.playNextVoice(m)) return;
            // The queue is done: play the falling cue, then close the player.
            player.playCue("end");
            player.stop();
          }}
          onpausevoice={() => (messages.autoplayId = null)}
          onreplymenu={(e, m) => {
            const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
            ui.menu = { x: rect.left, y: rect.bottom + 4, message: m };
          }}
          ononce={(m) => {
            if (!m.media_path) {
              const chat = chats.selectedChat;
              return messages.downloadMedia(chat, m).then(() => messages.loadMarks(chat));
            }
            ui.onceIndex = 0;
            ui.onceOpen = m;
          }}
          oncloseonce={closeViewOnce}
          oninvitejoin={(message, link) => {
            const account = session.activeAccount, chat = message.chat, id = message.id;
            return composer.enqueue(() => message.media_kind === "group_invite"
              ? invoke<import("$lib/utils/wire").Joined>("join_group_invite_message", { account, chat, id })
              : invoke<import("$lib/utils/wire").Joined>("join_invite", { account, link }));
          }}
          oninviteopen={async (jid) => {
            const account = session.activeAccount, generation = messages.accountGeneration;
            await chats.refreshChats();
            if (account !== session.activeAccount || generation !== messages.accountGeneration) return;
            void openChat(jid);
          }} />

        {#if ui.scrolledUp}
          <button class="jump" onclick={scrollToBottom}>
            Latest <Icon name="chevronDown" size={15} />
          </button>
        {/if}
        </div>

        {@render syncStatus()}

        {#if ui.picking}
          <SelectionBar
            count={Object.keys(ui.picking).length}
            reactionReason={broadcastSendReason(selectedChat)}
            allStarred={Object.keys(ui.picking).length > 0 && Object.keys(ui.picking).every((id) => messages.starred.has(id))}
            onforward={() => {
              const batch = pickedInOrder(ui.picking, messages.ordered);
              if (batch.length > 0) ui.forwarding = batch;
            }}
            ondelete={() => (ui.bulkDelete = Object.keys(ui.picking ?? {}))}
            onlabel={() => {
              ui.labelTargets = pickedInOrder(ui.picking, messages.ordered).filter((m) => !isUnavailable(m) && !m.revoked && !m.deleted && !m.spoiler && !m.system_kind && !m.media_once_kind && m.media_kind !== "view_once" && m.media_kind !== "unknown")
                .map((m) => ({ chat: m.chat, id: m.id }));
              if (!ui.labelTargets.length) { ui.labelTargets = null; ui.notify("Select a message that can be labelled."); }
              else void labels.refresh();
            }}
            oncopy={() => copyMessages(pickedInOrder(ui.picking, messages.ordered))}
            onstar={() => {
              const batch = pickedInOrder(ui.picking, messages.ordered);
              return starMessages(batch, !batch.every((message) => messages.starred.has(message.id)));
            }}
            onreact={(event) => {
              const batch = pickedInOrder(ui.picking, messages.ordered);
              const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
              if (batch.length) ui.emojiFor = { messages: batch, x: box.left, y: box.top };
            }}
            oncancel={() => {
              ui.picking = null;
              ui.bulkDelete = null;
              ui.emojiFor = null;
            }} />
        {:else}
          <AttachmentRecoveryPanel records={composer.currentAttachmentRecoveries}
            onrestore={(key) => { const record = findRecovery(composer.currentAttachmentRecoveries, key); if (record) composer.restoreKnownUnsent(record.context.chat); }}
            ondismiss={(key) => { const record = findRecovery(composer.currentAttachmentRecoveries, key); if (record) composer.discardAttachmentRecovery(record); }}
            onsave={(key, id) => {
              const record = findRecovery(composer.currentAttachmentRecoveries, key);
              const item = record && [...record.retryable, ...record.uncertain].find((item) => item.id === id);
              if (item) saveAttachmentCopy(item.file);
            }} />
          <ComposerBar
            account={session.activeAccount}
            generation={messages.accountGeneration}
            connected={session.connected}
            disabled={!session.connected || isBroadcastList(selectedChat) || !!(members.chatGroup && !members.chatGroup.can_send)}
            defaultQuality={session.settings.media_quality}
            onsoundclip={(file, scope) => composer.sendSoundClip(file, scope)}
            onquickreply={(scope, reply) => {
              try {
                quickReplies.current(scope);
                const current = quickReplies.replies.find((entry) => entry.id === reply.id);
                if (!current || current.message !== reply.message) throw new Error("quick reply changed before insertion");
                void composer.insertAtCaret(current.message).catch((error) => ui.fail(error));
              } catch (error) { ui.fail(error); }
            }}
            onslashcommand={(command) => {
              if (command === "mention-all" && selectedChat.endsWith("@g.us") && !composer.chosenMentions.some((mention) => mention.jid === "@all")) {
                composer.chosenMentions = [...composer.chosenMentions, { jid: "@all", name: "all" }];
              }
            }}
            bind:draft={composer.draft}
          bind:composerInput
          replyingTo={composer.replyingTo}
          replyAuthor={composer.replyingTo
            ? (composer.replyingTo.from_me ? "yourself" : members.senderLabel(composer.replyingTo))
            : ""}
          replySnippet={composer.replyingTo ? members.replyPreviewText(composer.replyingTo) : ""}
          editing={composer.editing}
          pending={composer.pending}
          bind:recording={composer.recording}
          mentionMatches={composer.mentionMatches}
          bind:mentionIndex={composer.mentionIndex}
          onselectmention={(person) => composer.selectMention(person)}
          emojiToken={composer.emojiToken}
          emojiMatches={composer.emojiMatches}
          bind:emojiIndex={composer.emojiIndex}
          onselectemoji={(emoji) => composer.selectEmoji(emoji)}
          bind:pickerTab={composer.pickerTab}
          {selectedChat}
          enqueue={<T>(task: (signal: AbortSignal) => Promise<T>) => composer.enqueue(task)}
          onpickeremoji={(emoji) => composer.insertAtCaret(emoji)}
          takereply={(): Record<string, string> => {
            const reply = composer.replyingTo;
            composer.replyingTo = null;
            return reply ? { replyToId: reply.id, replyToSender: reply.sender, replyToText: reply.text } : {};
          }}
          onpickersent={async () => {
            await messages.reloadMessages(chats.selectedChat);
            await chats.refreshChats();
            scrollToBottom();
          }}
          onpickererror={(message) => (ui.error = message)}
          onstage={(file) => composer.stageFile(file)}
          onsharecontacts={() => { ui.sharingContacts = true; }}
          oncreatekind={(kind) => (ui.creating = kind)}
          oninput={(e) => composer.onComposerInput(e)}
          onbeforeinput={(e) => composer.onComposerBeforeInput(e)}
          onkey={(e) => composer.onComposerKey(e)}
          onsend={() => void composer.send()}
          onschedule={(dueAt) => composer.schedule(dueAt)}
          oncancelreply={() => (composer.replyingTo = null)}
          oncanceledit={() => composer.cancelEditing()}
          onremove={(id) => composer.removePending(id)}
          ontoggleonce={(id) => composer.toggleOnce(id)}
          onsendvoice={(note) => composer.sendVoice(note)}
          onvoiceerror={(message) => (ui.error = message)}
          onreceipts={() => composer.toggleChatReceipts()}
          ontyping={() => composer.toggleChatTyping()}
            receiptsHidden={composer.receiptsHidden}
            typingHidden={composer.typingHidden} />
        {/if}

        {#if isBroadcastList(selectedChat)}
          <div class="read-only" role="status"><Icon name="volume" size={16} />{broadcastSendReason(selectedChat)}</div>
        {:else if members.chatGroup && !members.chatGroup.can_send}
          <div class="read-only" role="status">
            <Icon name={members.chatGroup.community ? "users" : "volume"} size={16} />
            {#if members.chatGroup.community}
              This is a community. People talk in its groups; announcements go to its announcement group.
            {:else}
              Only admins can send messages{members.chatGroup.announcements
                ? " to this community's announcements"
                : " here"}.
            {/if}
          </div>
        {/if}
      {:else}
        <div class="placeholder">
          <span class="placeholder-icon"><Icon name="message" size={28} /></span>
          <p class="placeholder-title">No conversation open</p>
          <p class="hint">Pick a chat on the left, or search for a contact to start one.</p>
        </div>
        {@render syncStatus()}
      {/if}
    </section>

  </div>
{/if}
</div>

{#if ui.menu}
  {#key ui.menu}
  {@const m = ui.menu.message}
  <MessageMenu
    x={ui.menu.x}
    y={ui.menu.y}
    items={menuItems(m)}
    reactions={QUICK_REACTIONS}
    reactionReason={broadcastSendReason(m.chat)}
    current={messages.reactionsFor.get(m.id)?.find((r) => r.mine)?.emoji ?? null}
    onreact={(emoji) => reactMessages([m], emoji)}
    onmore={openEmojiFor}
    onclose={() => (ui.menu = null)} />
  {/key}
{/if}

{#if ui.emojiFor}
  <ExpressionPicker
    chat={emojiChat}
    tab="emoji"
    enqueue={<T>(task: (signal: AbortSignal) => Promise<T>) => composer.enqueue(task)}
    takereply={() => ({})}
    onemoji={pickCustomReaction}
    onsent={() => {}}
    onerror={(message) => (ui.error = message)}
    onclose={() => (ui.emojiFor = null)}
    emojiOnly
    anchor={emojiAnchor} />
{/if}

{#if ui.reactionsFor}
  {@const reactors = messages.reactorsFor.get(ui.reactionsFor.id) ?? []}
  <ReactionList
    groups={reactors.map((group) => ({
      emoji: group.emoji,
      people: group.senders.map((jid) => {
        const self = members.isMe(jid);
        return {
          // Our own reaction is stored under "@me", which is not an address the
          // contact card could ask about.
          jid: self ? (session.me ?? jid) : bare(jid),
          label: self ? "You" : members.senderName(jid),
          avatar: self ? (session.me ? chats.pictureOf(session.me) : null) : chats.pictureOf(bare(jid)),
          self,
        };
      }),
    }))}
    onprofile={openProfile}
    onremove={removeOwnReaction}
    onclose={() => (ui.reactionsFor = null)} />
{/if}

{#if ui.creating}
  <CreateDialog kind={ui.creating} oncreate={create} onclose={() => (ui.creating = null)} />
{/if}

{#if ui.editingEvent}
  {@const { chat, event } = ui.editingEvent}
  <CreateDialog
    kind="event"
    initial={event}
    oncreate={(value) => saveEvent(chat, event.id, value as object)}
    onclose={() => (ui.editingEvent = null)} />
{/if}

{#if ui.forwarding}
  {@const batch = ui.forwarding}
  <ChatPicker
    title={batch.length > 1 ? `Forward ${batch.length} messages to` : "Forward message to"}
    chats={chats.chats.map((c) => ({ jid: c.chat, label: chats.chatLabel(c), avatar: chats.avatars[c.chat] ?? null }))}
    onforward={(targets) => forwardMessages(batch, targets)}
    onclose={() => (ui.forwarding = null)} />
{/if}

{#if ui.deleting}
  {@const m = ui.deleting}
  <ConfirmDialog
    label="Delete message"
    title="Delete message?"
    hint={canDeleteForEveryone(m)
      ? "Delete it for everyone in this chat, or only on this computer (kept greyed out)."
      : "It is deleted on this computer only, and kept greyed out here."}
    onclose={() => (ui.deleting = null)}>
    {#snippet actions()}
      {#if canDeleteForEveryone(m)}
        <button class="danger" onclick={() => deleteMessage(true)}>Delete for everyone</button>
      {/if}
      <button class="danger" onclick={() => deleteMessage(false)}>Delete on this computer</button>
      <button onclick={() => (ui.deleting = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if ui.bulkDelete}
  {@const picked = ui.bulkDelete}
  <ConfirmDialog
    label="Delete messages"
    title={`Delete ${picked.length} message${picked.length === 1 ? "" : "s"}?`}
    hint={canDeletePickedForEveryone()
      ? "Delete them for everyone, or only on this computer (kept greyed out here)."
      : "They are deleted on this computer only, and kept greyed out here."}
    onclose={() => (ui.bulkDelete = null)}>
    {#snippet actions()}
      {#if canDeletePickedForEveryone()}
        <button class="danger" onclick={() => deleteSelected(true)}>Delete for everyone</button>
      {/if}
      <button class="danger" onclick={() => deleteSelected(false)}>Delete on this computer</button>
      <button onclick={() => (ui.bulkDelete = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if ui.reporting}
  {@const m = ui.reporting}
  <ConfirmDialog
    label="Report message"
    title="Report to admins?"
    hint="The group's admins see this message and that you reported it. WhatsApp is not told."
    onclose={() => (ui.reporting = null)}>
    {#snippet actions()}
      <button
        class="danger"
        onclick={() => {
          ui.reporting = null;
          act(() => invoke("report_message", { chat: m.chat, id: m.id }));
        }}>Report</button>
      <button onclick={() => (ui.reporting = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if ui.chatSettingsOpen && chats.selectedChat}
  <ChatSettings
    chat={chats.selectedChat}
    title={chats.chats.find((c) => c.chat === chats.selectedChat)
      ? chats.chatLabel(chats.chats.find((c) => c.chat === chats.selectedChat)!)
      : members.displayName(null, chats.selectedChat)}
    picture={chats.avatars[chats.selectedChat] ?? null}
    onchange={async (retention) => {
      messages.loadOnScroll = retention.on_demand;
      await messages.reloadMessages(chats.selectedChat);
      await chats.refreshChats();
    }}
    onclearchat={() => (ui.chatConfirm = { kind: "clear", chat: chats.selectedChat! })}
    ondeletechat={() => (ui.chatConfirm = { kind: "delete", chat: chats.selectedChat! })}
    onclose={() => (ui.chatSettingsOpen = false)} />
{/if}

{#if ui.chatConfirm}
  {@const target = ui.chatConfirm.chat}
  {@const isClear = ui.chatConfirm.kind === "clear"}
  <ConfirmDialog
    label={isClear ? "Clear chat" : "Delete chat"}
    title={isClear ? `Clear chat with ${confirmChatLabel(target)}?` : `Delete chat with ${confirmChatLabel(target)}?`}
    hint={isClear
      ? "Its messages are removed from this computer, but the chat stays in the list. The other side is not affected."
      : "Its messages are removed and the chat leaves the list until a new message arrives. The other side is not affected."}
    onclose={() => (ui.chatConfirm = null)}>
    {#snippet actions()}
      <button class="danger" onclick={() => (isClear ? doClearChat(target) : doDeleteChat(target))}
        >{isClear ? "Clear chat" : "Delete chat"}</button>
      <button onclick={() => (ui.chatConfirm = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if ui.removeMember}
  {@const member = ui.removeMember}
  <ConfirmDialog
    label="Remove from group"
    title={`Remove ${member.name}?`}
    hint="They leave the group on every linked device. You can add them again later."
    onclose={() => (ui.removeMember = null)}>
    {#snippet actions()}
      <button
        class="danger"
        onclick={async () => {
          ui.removeMember = null;
          try {
            const changes = await invoke<ParticipantChange[]>("remove_group_participants", {
              chat: member.chat,
              jids: [member.jid],
            });
            const refused = changes.map(changeText).find((text) => !!text);
            if (refused) ui.fail(refused);
          } catch (e) {
            ui.fail(e);
          }
        }}>Remove</button>
      <button onclick={() => (ui.removeMember = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if ui.infoFor}
  {@const m = ui.infoFor}
  <MessageInfo
    id={m.id}
    sentAt={m.timestamp}
    preview={members.replyPreviewText(m)}
    voice={m.media_kind === "audio"}
    group={m.chat.endsWith("@g.us")}
    audience={m.chat === chats.selectedChat ? Math.max(0, members.participants.length - 1) : 0}
    version={ui.infoVersion}
    namer={(name, jid) => members.displayName(name, jid)}
    picture={(jid) => chats.pictureOf(bare(jid))}
    onclose={() => (ui.infoFor = null)} />
{/if}

{#if memberSheet.scope}
  {@const scope = memberSheet.scope}
  <MemberSheet account={scope.account} group={scope.group} jid={scope.jid} title={scope.title}
    groupName={members.displayName(null, scope.group)} requestKey={scope.requestKey} dataScope={memberSheet.profile ? scope : null}
    connected={session.connected} local={memberSheet.profile?.local ?? null} live={memberSheet.profile?.live ?? null}
    member={memberSheet.member()} memberSource="cached" picture={memberSheet.profile?.live?.photo.value ?? null}
    localLoading={memberSheet.localLoading} liveLoading={memberSheet.liveLoading} error={memberSheet.error} liveError={memberSheet.liveError}
    admin={memberSheet.admin()} blocked={memberSheet.blocked} liveCached={memberSheet.profile?.live_cached ?? false}
    liveStale={memberSheet.profile?.live_stale ?? false} moderationAdminVerified={memberSheet.profile?.moderation_admin_verified ?? false}
    moderationVerifiedAt={memberSheet.profile?.moderation_verified_at ?? null} moderationError={memberSheet.profile?.moderation_error ?? null}
    community={members.chatGroup?.community ?? chats.groupInfo?.community ?? false} auditRevision={chats.auditRevision}
    supportedActions={members.chatGroup || chats.groupInfo ? ["promote", "demote", "remove", "block", "unblock"] : ["promote", "demote", "block", "unblock"]}
    namer={(jid) => members.displayName(null, jid)} formatTime={(at) => new Date(at * 1000).toLocaleString()}
    onaction={(scope, action) => memberSheet.action(scope, action)} onsavelocal={(scope, text, warnings) => memberSheet.save(scope, text, warnings)}
    onloadAudit={(scope, filter, cursor) => memberSheet.audit(scope, filter, cursor)}
    onrefresh={(scope) => { memberSheet.current(scope); void memberSheet.load(true, true); }}
    onmessage={(jid) => { memberSheet.close(); chats.showGroupInfo = false; void openChat(jid); }}
    ongroup={(jid, id) => { memberSheet.close(); chats.showGroupInfo = false; if (id) void jumpTo(jid, id); else void openChat(jid); }}
    onclose={() => memberSheet.close()} />
{/if}

{#if ui.profileCard}
  {@const card = ui.profileCard}
  <ProfileCard
    jid={card.jid}
    x={card.x}
    y={card.y}
    name={card.name}
    self={card.self}
    picture={chats.pictureOf(card.jid)}
    tag={members.memberOf(card.jid)?.label ?? null}
    aliases={members.aliasesFor(card.jid)}
    onaddalias={(alias) => members.addAlias(card.jid, alias)}
    onremovealias={(alias) => void members.removeAlias(card.jid, alias)}
    onmessage={(jid) => {
      ui.profileCard = null;
      chats.showGroupInfo = false;
      void openChat(jid);
    }}
    onclose={() => (ui.profileCard = null)} />
{/if}

{#if ui.showStarred}
  <StarredList
    items={ui.starredItems}
    onopen={(item) => {
      ui.showStarred = false;
      void jumpTo(item.chat, item.id);
    }}
    onunstar={(item) =>
      act(async () => {
        await invoke("star", {
          target: { chat: item.chat, id: item.id, sender: item.sender, fromMe: item.fromMe },
          starred: false,
        });
        ui.starredItems = ui.starredItems?.filter((i) => i !== item) ?? null;
      })}
    onclose={() => (ui.showStarred = false)} />
{/if}

{#if ui.finder}
  {@const inChat = ui.finder.chat ? chats.chatName(ui.finder.chat) : null}
  <MessageFinder
    title={ui.finder.mode === "search" ? "Search messages" : inChat ? "Your mentions" : "Mentions"}
    subtitle={ui.finder.mode === "search" && ui.finder.reach
      ? `${inChat} · searched back to ${new Date(ui.finder.reach * 1000).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })}`
      : (inChat ?? (ui.finder.mode === "pings" ? "Every message that pinged you" : null))}
    moreLabel="Load the previous day"
    placeholder={ui.finder.mode === "search" ? "Search this chat" : "Filter mentions"}
    items={ui.finder.items}
    empty={ui.finder.mode === "search" ? "Type to search the messages kept on this computer." : "Nobody has mentioned you yet."}
    onquery={ui.finder.mode === "search" ? (q) => searchChat(q) : undefined}
    onmore={ui.finder.mode === "search" && ui.finder.more ? () => searchChat(ui.finder?.query ?? "", true) : undefined}
    onopen={(item) => {
      ui.finder = null;
      void jumpTo(item.chat, item.id);
    }}
    onclose={() => (ui.finder = null)} />
{/if}

{#if galleryChat && session.activeAccount}
  {#key session.activeAccount}
    <Gallery accountKey={session.activeAccount} chat={galleryChat} chats={chats.chats}
      chatName={(chat) => chats.chatName(chat)} senderName={(message) => members.displayName(message.sender_name, message.sender)}
      onjump={jumpTo} onopen={openMedia} onclose={() => (galleryChat = null)}
      onreply={async (message) => {
        const account = session.activeAccount;
        await openChat(message.chat);
        if (session.activeAccount !== account || chats.selectedChat !== message.chat) return;
        galleryChat = null;
        composer.editing = null;
        composer.replyingTo = message;
        await tick();
        composerInput?.focus();
      }} />
  {/key}
{/if}

{#if ui.onceOpen && ui.onceOpen.media_kind !== "audio" && ui.onceOpen.media_path}
  <MediaViewer
    items={[viewerItem(ui.onceOpen)]}
    bind:index={ui.onceIndex}
    onclose={closeViewOnce}
    onreply={(id) => {
      composer.editing = null;
      composer.replyingTo = messages.messages.find((m) => m.id === id) ?? null;
      void closeViewOnce();
      composerInput?.focus();
    }}
    onjump={() => void closeViewOnce()} />
{/if}

{#if viewerPosition() >= 0}
  <MediaViewer
    items={viewerItems}
    bind:index={viewerPosition, setViewerPosition}
    onclose={() => (ui.viewerId = null)}
    onopen={openMedia}
    onreply={(id) => {
      composer.editing = null;
      composer.replyingTo = messages.messages.find((m) => m.id === id) ?? null;
      ui.viewerId = null;
      composerInput?.focus();
    }}
    onjump={(id) => {
      ui.viewerId = null;
      scrollToMessage(id);
    }} />
{/if}

{#if quoteView}
  <MediaViewer
    items={quoteView}
    index={0}
    onclose={() => (quoteView = null)}
    onreply={() => (quoteView = null)}
    onjump={(id) => {
      quoteView = null;
      scrollToMessage(id);
    }} />
{/if}

{#if broadcastFor && broadcastFor.account === session.activeAccount && broadcastFor.chat === chats.selectedChat && broadcastFor.generation === messages.accountGeneration}
  <Panel label="Broadcast recipients" nav={[{ id: "recipients", label: "Recipients", group: "Broadcast list" }]} section="recipients"
    onclose={() => (broadcastFor = null)}>
    {#snippet header()}<h2>Broadcast list</h2>{/snippet}
    {#snippet children()}<BroadcastInfo info={broadcastInfo} loading={broadcastLoading} error={broadcastError}
      nameOf={(jid) => members.displayName(null, jid)} />{/snippet}
  </Panel>
{/if}

{#if contactInfoFor}
  {@const jid = contactInfoFor}
  <ContactInfo
    {jid}
    account={session.activeAccount}
    connected={session.connected}
    generation={messages.accountGeneration}
    oncontactchange={() => { members.forgetUnresolvedNames(); void chats.refreshChats(); }}
    title={members.displayName(chats.chats.find((c) => c.chat === jid)?.display_name ?? null, jid)}
    picture={chats.pictureOf(jid)}
    aliases={members.aliasesFor(jid)}
    onclose={() => (contactInfoFor = null)} />
{/if}

{#if chats.showGroupInfo && chats.selectedChat}
  {@const selectedChat = chats.selectedChat}
  {@const chat = chats.chats.find((c) => c.chat === selectedChat)}
  {@const account = session.activeAccount}
  {@const generation = messages.accountGeneration}
  {@const currentGroup = (signal?: AbortSignal) => {
    if (signal?.aborted || !account || account !== session.activeAccount || generation !== messages.accountGeneration || chats.selectedChat !== selectedChat) {
      throw new Error("account or group changed during operation");
    }
  }}
  <GroupInfo
    jid={selectedChat}
    title={chat ? chats.chatLabel(chat) : members.displayName(null, selectedChat)}
    info={chats.groupInfo}
    error={chats.groupInfoError}
    avatars={chats.avatars}
    pinned={!!chat?.pinned}
    onavatar={(jid) => chats.loadAvatar(bare(jid))}
    onretry={() => chats.openGroupInfo()}
    onpin={() => chat && chats.togglePin(chat)}
    onopenurl={openUrl}
    onmessage={(jid) => {
      chats.showGroupInfo = false;
      openChat(bare(jid));
    }}
    onprofile={(jid, name, e) => openProfile(jid, name, e)}
    me={session.me}
    namer={(name, jid) => members.displayName(name, jid)}
    {account}
    onsettingsload={async () => {
      currentGroup();
      const settings = await invoke<import("$lib/utils/wire").GroupSettings>("group_settings", { account, chat: selectedChat });
      currentGroup();
      return settings;
    }}
    onsettingchange={(change) => composer.enqueue(async (signal) => {
      currentGroup(signal);
      await invoke<void>("change_group_setting", { account, chat: selectedChat, change });
      currentGroup(signal);
    })}
    onpicturechange={(data) => composer.enqueue(async (signal) => {
      currentGroup(signal);
      await invoke<void>("set_group_picture", { account, chat: selectedChat, data });
      currentGroup(signal);
    })}
    onreports={() => invoke<AdminReport[]>("admin_reports", { chat: selectedChat })}
    oninviteload={() => {
      const account = session.activeAccount, chat = selectedChat;
      return invoke<string>("group_invite_link", { account, chat, reset: false });
    }}
    oninvitereset={() => {
      const account = session.activeAccount, chat = selectedChat;
      return composer.enqueue(() => invoke<string>("group_invite_link", { account, chat, reset: true }));
    }}
    onrequests={() => invoke<GroupJoinRequest[]>("group_join_requests", { account: session.activeAccount, chat: selectedChat })}
    onrequestchange={(jids, approve) => {
      const account = session.activeAccount, chat = selectedChat;
      return composer.enqueue(() => invoke<ParticipantChange[]>("change_group_join_requests", { account, chat, jids, approve }));
    }}
    onallowreports={(allow) => invoke("set_allow_admin_reports", { chat: selectedChat, allow })}
    onadd={(jids, optedIn) => {
      const chat = selectedChat, account = session.activeAccount;
      return composer.enqueue(() => invoke<GroupMemberAddResult>("add_group_participants_with_history", { account, chat, jids, optedIn }));
    }}
    onretryhistory={(retryId) => {
      const chat = selectedChat, account = session.activeAccount;
      return composer.enqueue(() => invoke<GroupHistoryResult>("retry_group_history", { account, chat, retryId }));
    }}
    onremove={(jids) => invoke<ParticipantChange[]>("remove_group_participants", { chat: selectedChat, jids })}
    onpromote={(jids) => invoke<ParticipantChange[]>("promote_group_participants", { chat: selectedChat, jids })}
    ondemote={(jids) => invoke<ParticipantChange[]>("demote_group_participants", { chat: selectedChat, jids })}
    onmembersadd={(allow) => invoke("set_members_can_add", { chat: selectedChat, allow })}
    onjump={(id) => {
      chats.showGroupInfo = false;
      if (chats.selectedChat) void jumpTo(chats.selectedChat, id);
    }}
    auditRevision={chats.auditRevision}
    onloadAudit={async (scope, filter, before) => {
      currentGroup();
      if (scope.account !== account || scope.group !== selectedChat) throw new Error("audit scope changed");
      const page = await invoke<import("$lib/utils/wire").GroupAuditPage>("group_audit_page", {
        accountId: account, chat: selectedChat, filter: { ...filter, before, limit: 50 },
      });
      currentGroup();
      return page;
    }}
    onlabel={async (label) => {
      await invoke("set_member_label", { chat: selectedChat, label });
      const user = session.me?.split("@")[0];
      const info = chats.groupInfo?.participants.find((p) => p.jid === session.me || p.number === user);
      if (info) info.label = label || null;
      // The roster is raw, so the changed entry replaces its member.
      const self = members.participants.find((p) => p.jid === session.me || p.number === user);
      if (self) {
        members.participants = members.participants.map((p) =>
          p.jid === self.jid ? { ...p, label: label || null } : p,
        );
      }
    }}
    onclose={() => (chats.showGroupInfo = false)} />
{/if}

{#if ui.pendingJump}
  <div class="notice">
    <Spinner />
    <span>Fetching older messages from your phone to find it…</span>
  </div>
{/if}

{#snippet syncStatus()}
{#if session.connected && session.uiUnlocked && (session.syncPending > 0 || session.historyPercent !== null) && !ui.pendingJump && !ui.notice}
  <div class="notice sync-status" role="status">
    <Spinner />
    <span>
      {#if session.syncPending > 0}
        Syncing messages · {Math.max(0, session.syncPending - session.syncApplied)} left of {session.syncPending}
        ({session.syncPercent}%)
      {:else}
        Syncing history from your phone · {session.historyPercent}%
      {/if}
    </span>
  </div>
{/if}
{/snippet}

{#if session.backfill && !ui.notice}
  <div class="notice" role="status">
    <Spinner />
    <span>Downloading all history · {session.backfill.done} of {session.backfill.total} chats</span>
  </div>
{/if}

{#if ui.notice}
  <div class="notice">
    <span>{ui.notice}</span>
    <button class="link" onclick={muteNotice}>Do not warn again</button>
    <Button variant="icon" icon="x" iconSize={16} title="Dismiss" aria-label="Dismiss" onclick={() => (ui.notice = null)} />
  </div>
{/if}

{#if ui.newGroup && session.activeAccount}
  {@const account = session.activeAccount}
  {@const generation = messages.accountGeneration}
  {#key account}
    <NewGroup {account} me={session.me} avatars={chats.avatars} onavatar={(jid) => chats.loadAvatar(jid)}
      onsearch={async (query) => {
        if (account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed");
        const rows = await invoke<import("$lib/utils/wire").SearchResult[]>("group_creation_contacts", { account, query });
        if (account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed");
        return rows;
      }}
      oncreate={(subject, jids) => composer.enqueue((signal) => {
        if (signal.aborted || account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed before operation");
        return invoke<import("$lib/utils/wire").GroupCreateResult>("create_group", { account, subject, jids });
      })}
      onopen={async (result) => {
        const opening = chatOpenSeq;
        if (account !== session.activeAccount || generation !== messages.accountGeneration) return;
        await chats.refreshChats();
        if (account !== session.activeAccount || generation !== messages.accountGeneration || opening !== chatOpenSeq) return;
        await openChat(result.jid, false, result.subject);
        if (account === session.activeAccount && generation === messages.accountGeneration) ui.newGroup = false;
      }}
      onclose={() => (ui.newGroup = false)} />
  {/key}
{/if}

{#if newContact && session.activeAccount}
  {#key session.activeAccount}
    <NewContact account={session.activeAccount} connected={session.connected} onclose={() => (newContact = false)} onsaved={(jid) => {
        members.forgetUnresolvedNames();
        void chats.refreshChats();
        newContact = false;
        contactInfoFor = jid;
      }} />
  {/key}
{/if}

{#if quickSwitcher && session.activeAccount}
  {@const account = session.activeAccount}
  {@const generation = messages.accountGeneration}
  {#key account}
    <QuickSwitcher {account} chats={chats.chats}
      onload={() => invoke<SearchResult[]>("switcher_catalog", { accountId: account })}
      onmessages={(query) => invoke<StoredMessage[]>("switcher_messages", { accountId: account, query, limit: 50 })}
      onchoose={async (target) => {
        if (account !== session.activeAccount || generation !== messages.accountGeneration) return;
        quickSwitcher = false;
        if (target.messageId) await jumpTo(target.chat, target.messageId);
        else await openChat(target.chat, false, target.label);
      }} onclose={() => (quickSwitcher = false)} />
  {/key}
{/if}

{#if ui.sharingContacts && chats.selectedChat}
  <dialog bind:this={contactDialog} aria-label="Share contacts" oncancel={(event) => { event.preventDefault(); event.stopPropagation(); ui.sharingContacts = false; }}
    onkeydown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}>
    <button class="button" aria-label="Close contact sharing" onclick={() => { ui.sharingContacts = false; }}>Close</button>
    <ContactSharing mode="share" account={session.activeAccount} connected={session.connected} chat={chats.selectedChat}
      generation={messages.accountGeneration} canSend={!composer.editing && !composer.recording && (!members.chatGroup || members.chatGroup.can_send)}
      choices={Object.entries(members.identities).map(([jid, identity]) => ({ jid, identity }))} onopenchat={openChat} onshare={sendContacts} />
  </dialog>
{/if}

{#if ui.manageLabels || ui.labelTargets}
  <LabelDialog account={session.activeAccount} requestKey={JSON.stringify([messages.accountGeneration, ui.labelTargets, ui.manageLabels])}
    labels={labels.account === session.activeAccount ? labels.view.labels : []}
    selected={labelSelection.selected} mixed={labelSelection.mixed} mode={ui.labelTargets ? "apply" : "manage"}
    busy={labels.busy || !session.connected || !labels.loaded} error={labels.error} complete={labels.view.complete}
    onsave={(id, name, color) => labels.save(id, name, color)} ondelete={(id) => labels.delete(id)}
    onapply={async (id, labeled) => {
      const targets = ui.labelTargets?.map((target) => ({ ...target })) ?? [];
      if (!targets.length) throw new Error("Choose a chat or message to label.");
      if (targets.every((target): target is { chat: string; id: string } => typeof target.id === "string")) {
        await labels.applyMessages(id, targets, labeled);
      } else if (targets.length === 1 && !targets[0].id) {
        await labels.applyChat(id, targets[0].chat, labeled);
      } else throw new Error("Chat and message labels must be applied separately.");
    }} onclose={() => { ui.manageLabels = false; ui.labelTargets = null; }} />
{/if}

{#if ui.showSettings}
  <Settings
    settings={session.settings}
    accounts={session.accountList}
    active={session.activeAccount}
    onnotificationjump={async (account, chat, id) => {
      if (account !== session.activeAccount) throw new Error("account changed");
      const generation = messages.accountGeneration;
      await jumpTo(chat, id);
      if (account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed");
      ui.showSettings = false;
    }}
    onblockedload={async (account) => {
      const generation = messages.accountGeneration;
      if (account !== session.activeAccount) throw new Error("account changed");
      const rows = await invoke<import("$lib/utils/wire").BlockedContact[]>("blocked_contacts", { account });
      if (account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed");
      return rows;
    }}
    onunblockcontact={async (account, jid) => {
      const generation = messages.accountGeneration;
      await composer.enqueue((signal) => {
        if (signal.aborted || account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed before operation");
        return invoke("set_contact_blocked", { account, jid, blocked: false });
      });
      if (account !== session.activeAccount || generation !== messages.accountGeneration) throw new Error("account changed");
    }}
    me={session.me}
    onopencontact={async (jid) => { await openChat(jid); ui.showSettings = false; }}
    meAvatar={session.me ? (chats.avatars[session.me] ?? null) : null}
    accountAvatars={chats.accountAvatars}
    bind:section={ui.settingsSection}
    onclose={() => (ui.showSettings = false)}
    onsave={async (next) => {
      await session.saveSettings(next);
      messages.resizeWindow(session.settings.message_window_size);
      await once.refresh();
    }}
    onflush={flushMedia}
    onclearhistory={clearHistory}
    onrename={(id, label) => session.renameAccount(id, label)}
    onremove={removeAccount}
    onadd={() => {
      ui.showSettings = false;
      addAccount();
    }}
    onswitch={(id) => {
      ui.showSettings = false;
      switchTo(id);
    }}
    onprivacy={(next) => (session.privacy = next)}
    onpicture={() => {
      if (!session.me) return;
      session.meVersion += 1;
      chats.forgetAvatar(session.me);
      chats.loadAvatar(session.me);
    }} />
{/if}

<style>
  :global(:root) {
    --bg: #111b21;
    --chat-bg: #0b141a;
    --surface: #202c33;
    --raised: #2a3942;
    --raised-2: #374248;
    --line: #222d34;
    --line-soft: #1d282f;
    --line-strong: #3b4a54;
    --text: #e9edef;
    --muted: #8696a0;
    --faint: #667781;
    --accent: #00a884;
    --accent-hover: #06cf9c;
    --accent-ink: #111b21;
    --accent-text: #00a884;
    --accent-soft: rgba(0, 168, 132, 0.18);
    --link: #53bdeb;
    --mention: #f0b232;
    --mention-soft: rgba(240, 178, 50, 0.1);
    --mention-self-soft: rgba(240, 178, 50, 0.24);
    --mention-pill: #53bdeb;
    --mention-pill-soft: rgba(83, 189, 235, 0.18);
    --replying: #00a884;
    --replying-soft: rgba(0, 168, 132, 0.16);
    --jump-soft: rgba(0, 168, 132, 0.3);
    --row-hover: rgba(233, 237, 239, 0.03);
    --bubble: #202c33;
    --bubble-mine: #005c4b;
    --danger: #f15c6d;
    --danger-soft: #3b1e24;
    --shadow: 0 2px 12px rgba(0, 0, 0, 0.45);
    --scrim: rgba(0, 0, 0, 0.6);
    --radius-sm: 7.5px;
    --radius: 8px;
    --radius-lg: 10px;
    --font: "Segoe UI", "Helvetica Neue", system-ui, sans-serif;
    --font-size: 14.2px;
    --motion-scale: 1;
    --ease: cubic-bezier(0.2, 0.8, 0.2, 1);
    color-scheme: var(--scheme, dark);
  }
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    /* The flag font only covers flag codepoints, so it never shadows --font. */
    font-family: "Twemoji Country Flags", var(--font);
    font-size: var(--font-size);
    -webkit-font-smoothing: antialiased;
  }
  :global(*) {
    scrollbar-width: thin;
    scrollbar-color: var(--raised-2) transparent;
  }
  /* Density: comfortable is the default look; compact and cozy scale rows and bubbles. */
  :global(html.density-compact .chat-row) {
    height: 60px;
  }
  :global(html.density-compact .chat-row .avatar) {
    width: 42px;
    height: 42px;
    font-size: 14px;
  }
  :global(html.density-compact .chat-row::after) {
    left: 70px;
  }
  :global(html.density-compact .messages) {
    gap: 1px;
  }
  :global(html.density-compact .bubble) {
    padding: 4px 6px 5px 8px;
    line-height: 18px;
  }
  :global(html.density-compact .bubble.first) {
    margin-top: 6px;
  }
  :global(html.density-cozy .chat-row) {
    height: 82px;
  }
  :global(html.density-cozy .chat-row .avatar) {
    width: 54px;
    height: 54px;
    font-size: 16px;
  }
  :global(html.density-cozy .chat-row::after) {
    left: 82px;
  }
  :global(html.density-cozy .messages) {
    gap: 4px;
  }
  :global(html.density-cozy .bubble) {
    padding: 8px 10px 10px 12px;
    line-height: 21px;
  }
  :global(html.density-cozy .bubble.first) {
    margin-top: 14px;
  }
  /* Stop decorative motion; progress spinners honor the OS preference themselves. */
  :global(html.no-motion *),
  :global(html.no-motion *::before),
  :global(html.no-motion *::after) {
    animation: none !important;
    transition: none !important;
  }
  :global(button) {
    transition:
      background-color calc(0.15s * var(--motion-scale)) var(--ease),
      color calc(0.15s * var(--motion-scale)) var(--ease),
      opacity calc(0.15s * var(--motion-scale)) var(--ease),
      transform calc(0.1s * var(--motion-scale)) var(--ease);
  }
  :global(button:not(:disabled):active) {
    transform: translateY(1px);
  }
  :global(select) {
    background: var(--raised);
    color: var(--text);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    padding: 6px 8px;
    font: inherit;
  }
  :global(input[type="checkbox"], input[type="radio"], input[type="range"]) {
    accent-color: var(--accent);
  }
  :global(:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  :global(input:not([type="checkbox"]):not([type="radio"]):not([type="range"]):focus-visible, textarea:focus-visible) {
    outline: none;
    border-color: var(--accent) !important;
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .app {
    height: 100dvh;
    display: flex;
    flex-direction: column;
  }
  .app > .layout,
  .app > :global(.pairing) {
    flex: 1;
    min-height: 0;
  }
  .error {
    position: fixed;
    top: 12px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 300;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(640px, 90vw);
    padding: 8px 8px 8px 14px;
    background: var(--danger-soft);
    border: 1px solid color-mix(in srgb, var(--danger) 45%, transparent);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    font-size: 13px;
  }
  /* Dismiss buttons live in $lib/ui/Button.svelte (icon variant, already muted). */
  :global(.intro-settings) {
    margin-left: auto;
  }
  :global(.resume-avatar) {
    display: grid;
    place-items: center;
    width: 88px;
    height: 88px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--raised-2);
    font-size: 30px;
    font-weight: 600;
    box-shadow: 0 0 0 4px var(--accent-soft);
  }
  :global(.choice-avatar) {
    display: grid;
    place-items: center;
    flex: none;
    width: 44px;
    height: 44px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--raised-2);
    font-weight: 600;
  }
  :global(.intro-card.resume .account img) {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    object-fit: cover;
  }
  :global(.intro-accounts .account img),
  :global(.account-initial) {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    object-fit: cover;
  }
  :global(.account-initial) {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    font-size: 10px;
  }
  .layout {
    display: grid;
    grid-template-columns: 300px 1fr;
    overflow: hidden;
  }
  /* Fullscreen overlays (Panel, dialogs, viewer, preview sheet) are modal:
     the conversation and the chat list behind them must not scroll. */
  :global(body:has(.backdrop, .sheet-backdrop, .viewer, .lightbox) .messages),
  :global(body:has(.backdrop, .sheet-backdrop, .viewer, .lightbox) .chats ul) {
    overflow-y: hidden;
  }
  :global(.avatar) {
    grid-area: avatar;
    flex: none;
    width: 49px;
    height: 49px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: 15px;
    font-weight: 500;
    letter-spacing: 0.02em;
    user-select: none;
  }
  :global(img.avatar) {
    object-fit: cover;
    background: var(--raised);
  }
  :global(.conversation header .avatar) {
    width: 40px;
    height: 40px;
    font-size: 14px;
  }
  /* Message text lives in $lib/messages/MessageText.svelte. The mention avatar sizes
     stay global so they reach inside it. */
  :global(.mention-pill img),
  :global(.mention-initials) {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    flex: none;
    object-fit: cover;
  }
  :global(.mention-initials) {
    display: grid;
    place-items: center;
    font-size: 8px;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
  }
  .notice {
    position: fixed;
    left: 50%;
    bottom: 18px;
    transform: translateX(-50%);
    z-index: 200;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 80vw;
    padding: 8px 14px;
    background: var(--raised-2);
    border-radius: 8px;
    font-size: 13px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.5);
  }
  .notice .link {
    background: transparent;
    border: 0;
    color: var(--accent-text);
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }
  .notice.sync-status {
    position: static;
    align-self: center;
    flex-shrink: 0;
    transform: none;
    max-width: calc(100% - 24px);
    margin: 8px 12px;
  }
  /* Small dim helper text, shared by the pairing view and the empty-chat hint. */
  :global(.hint) {
    margin: 0;
    color: var(--faint);
    font-size: 12px;
    max-width: 44ch;
    text-wrap: balance;
  }
  /* The play triangle over pending videos and unloaded media, in the composer and bubbles. */
  :global(.play-badge) {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text);
    font-size: 28px;
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }
  /* Chat filter pills live in $lib/ui/Button.svelte (chip variant). */
  /* Beside the first bubble of a run, outside the tail. */
  /* Sender names and avatars render in MessageBubble and TypingIndicator. */
  :global(.sender-avatar) {
    position: absolute;
    left: -38px;
    top: 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  :global(.sender-avatar .avatar) {
    width: 28px;
    height: 28px;
    font-size: 11px;
  }
  :global(.me-avatar) {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    object-fit: cover;
    background: hsl(var(--hue, 160) 28% 24%);
    color: hsl(var(--hue, 160) 45% 80%);
    font-size: 13px;
    font-weight: 500;
  }
  /* Half-lit: online, but only contacts can see it. */
  /* Discord's invisible: a hollow grey ring. */
  /* Menu rows live in $lib/ui/Button.svelte (menu variant). */
  :global(.menu-avatar) {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 50%;
    object-fit: cover;
    background: hsl(var(--hue, 160) 28% 24%);
    color: hsl(var(--hue, 160) 45% 80%);
    font-size: 11px;
  }
  .conversation {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    position: relative;
    background: var(--chat-bg);
  }
  :global(.sender) {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    font-size: 12.8px;
    font-weight: 500;
    line-height: 22px;
    color: color-mix(in srgb, hsl(var(--hue) 65% 68%) 25%, var(--text));
    text-align: left;
  }
  :global(button.sender) {
    cursor: pointer;
  }
  :global(button.sender:hover) {
    text-decoration: underline;
  }
  /* In place of the composer where we may not write. */
  .read-only {
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 10px;
    min-height: 62px;
    padding: 10px 20px;
    box-sizing: border-box;
    background: var(--surface);
    color: var(--muted);
    font-size: 13.5px;
    text-align: center;
  }
  /* Active tint comes from Button itself. */
  :global(.tool-text) {
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.04em;
  }
  /* Discord-style completion list over the composer. */
  /* WhatsApp's reaction pill, hanging off the bubble's bottom edge. */
  /* Confirm sheets live in $lib/ui/ConfirmDialog.svelte. */
  .placeholder {
    margin: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    text-align: center;
  }
  .placeholder-icon {
    display: grid;
    place-items: center;
    width: 64px;
    height: 64px;
    border-radius: 20px;
    background: var(--surface);
    color: var(--faint);
    margin-bottom: 4px;
  }
  .placeholder-title {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .list-wrap {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .jump {
    position: absolute;
    bottom: 12px;
    right: 20px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--raised-2);
    color: inherit;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    padding: 6px 10px 6px 14px;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    box-shadow: var(--shadow);
  }
  .jump:hover {
    background: var(--line-strong);
  }
  /* Shared button shapes live in $lib/ui/Button.svelte. The attach button keeps
     its composer box here since it arrives through Button's `cls`. */
  :global(.attach) {
    width: 42px;
    height: 42px;
    margin-bottom: 5px;
  }
</style>
