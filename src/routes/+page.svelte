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
  import type { ChatRetention } from "$lib/utils/models";
  import ProfileCard from "$lib/contacts/ProfileCard.svelte";
  import ContactInfo from "$lib/contacts/ContactInfo.svelte";
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
  import ScheduledOutbox from "$lib/composer/ScheduledOutbox.svelte";
  import SelectionBar from "$lib/messages/SelectionBar.svelte";
  import { hue } from "$lib/utils/avatar";
  import { bare, captionOf, dayKey, dayLabel, formatTime, isSvg, MEDIA_LABELS } from "$lib/utils/message";
  import { chats } from "$lib/state/chats.svelte";
  import { composer } from "$lib/state/composer.svelte";
  import { favorites } from "$lib/state/favorites.svelte";
  import { transcription } from "$lib/state/transcription.svelte";
  import { dispatchServiceEvent, queueRefreshChats } from "$lib/state/events";
  import { members } from "$lib/state/members.svelte";
  import { messages } from "$lib/state/messages.svelte";
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

  $effect(() => {
    session.applyZoom();
  });

  let composerInput: HTMLTextAreaElement | undefined = $state();
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

  /** Member names under a group's title, as far as they are known. */
  const subtitle = $derived(
    chats.selectedChat?.endsWith("@g.us") && members.participants.length > 0
      ? members.participants.map((p) => members.displayName(p.name, p.jid)).join(", ")
      : null,
  );

  async function openChat(chat: string, jumpToMention = false, label: string | null = null) {
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
    // One-to-one typing only arrives for contacts we are subscribed to.
    if (!chat.endsWith("@g.us")) invoke("watch_presence", { jid: chat }).catch(() => {});
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
    chats.showGroupInfo = false;
    chats.groupInfo = null;
    contactInfoFor = null;
    try {
      // Invalidate any in-flight reload from the previous chat.
      const seq = messages.nextSeq();
      // Mentions are captured before the chat is marked read, since that clears them.
      const [mentions, page] = await Promise.all([
        invoke<string[]>("unread_mentions", { chat }).catch(() => [] as string[]),
        invoke<MessagePage>("message_page", { chat, limit: messages.messageLimit }),
      ]);
      // A quicker click on another chat has already taken over.
      if (!current()) return;
      messages.mentionQueue = mentions;
      messages.mentionCursor = 0;
      if (seq === messages.messagesSeq) messages.acceptMessages(page.messages);
      else await messages.reloadMessages(chat, true);
      if (!current()) return;
      await messages.loadMarks(chat);
      if (!current()) return;
      const loaded = messages.messages;
      // Enter at the unread divider when there is one, as Discord does, rather
      // than at the newest message. The unread flags are still intact here
      // because marking is now driven by scrolling, not by opening.
      const oldestUnread = [...loaded].reverse().find((m) => !m.read && !m.from_me);
      const newestUnread = loaded.find((m) => !m.read && !m.from_me);
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
    const element = scroller?.querySelector(`[data-id="${id}"]`);
    if (!element) return;
    element.scrollIntoView({ block: "center" });
    ui.highlightedId = id;
    window.setTimeout(() => {
      if (ui.highlightedId === id) ui.highlightedId = null;
    }, 1600);
  }

  /**
   * Pins the unread divider to the top while the rows above it settle their
   * height. Images have no size until they load, and WebKitGTK has no scroll
   * anchoring, so a single scroll lands above the divider and drifts as media
   * loads. Runs only until the height holds still, or the user scrolls.
   */
  function pinUnreadDivider(chat: string) {
    // Polling replaces a per-frame measure loop: a chat open used to force
    // layout on every frame for up to five seconds while media above the
    // divider settled. Checking close together while the height still moves,
    // then backing off, costs the same result for a fraction of the work.
    const deadline = Date.now() + 5000;
    let settled = 0;
    let lastHeight = -1;
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const cancel = () => (cancelled = true);
    const el = scroller;
    el?.addEventListener("wheel", cancel, { passive: true });
    el?.addEventListener("touchstart", cancel, { passive: true });
    const finish = () => {
      cancelled = true;
      clearTimeout(timer);
      el?.removeEventListener("wheel", cancel);
      el?.removeEventListener("touchstart", cancel);
    };
    const pin = () => {
      if (cancelled || chats.selectedChat !== chat || !scroller) return finish();
      const divider = scroller.querySelector("[data-unread-divider]") as HTMLElement | null;
      if (!divider) return finish();
      const offset = divider.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      const height = scroller.scrollHeight;
      const furthest = height - scroller.clientHeight;
      const next = Math.min(Math.max(0, scroller.scrollTop + offset), furthest);
      const moved = Math.abs(next - scroller.scrollTop) > 0.5;
      if (moved) scroller.scrollTop = next;
      // Any height change above the divider invalidates the position.
      const grew = height !== lastHeight;
      settled = !moved && !grew ? settled + 1 : 0;
      lastHeight = height;
      // Five quiet checks: about the three quarters of a second the
      // per-frame loop waited, without the per-frame cost.
      if (settled >= 5 || Date.now() > deadline) return finish();
      timer = setTimeout(pin, moved || grew ? 50 : 200);
    };
    timer = setTimeout(pin, 0);
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

  function openProfile(jid: string, name: string, event: MouseEvent, self = false) {
    event.stopPropagation();
    // Pills and names render from cache only; the click is what fetches.
    chats.loadAvatar(bare(jid));
    ui.profileCard = { jid: bare(jid), name, x: event.clientX, y: event.clientY, self };
  }

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
    // Wait for the new messages to render before measuring, so the pin lands
    // on the laid-out bottom rather than the previous one.
    void tick().then(() => {
      requestAnimationFrame(() => {
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
        ui.scrolledUp = false;
      });
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

  // Scroll events can outpace frames; the handler measures layout, so one
  // run per frame is all that is useful.
  let scrollQueued = false;
  function onScroll() {
    if (scrollQueued) return;
    scrollQueued = true;
    requestAnimationFrame(() => {
      scrollQueued = false;
      if (!scroller) return;
      const distance = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
      const scrolledUp = !messages.atLatest || distance > 120;
      if (ui.scrolledUp !== scrolledUp) ui.scrolledUp = scrolledUp;
      if (
        scroller.scrollTop < 80 &&
        messages.loadOnScroll &&
        !messages.olderExhausted &&
        !messages.loadingOlder &&
        messages.messages.length > 0
      ) {
        void messages.loadOlder(chats.selectedChat, true, scroller);
      }
      scheduleReadMarking();
    });
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
      const bottom = scroller.getBoundingClientRect().bottom;
      // One pass over the rendered bubbles in draw order; the old form ran a
      // querySelector per message, which is quadratic on a long chat.
      let candidate: string | null = null;
      const rows = scroller.querySelectorAll<HTMLElement>(".bubble[data-id]");
      for (const row of rows) {
        if (row.getBoundingClientRect().top >= bottom) break;
        candidate = row.dataset.id ?? null;
      }
      if (!candidate || candidate === messages.lastMarkedId) return;
      messages.lastMarkedId = candidate;
      const ids = messages.ordered;
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
  async function jumpTo(chat: string, id: string) {
    if (chat !== chats.selectedChat) await openChat(chat);
    await tick();
    if (scroller?.querySelector(`[data-id="${id}"]`)) scrollToMessage(id);
    else {
      ui.pendingJump = { chat, id };
      void loadAndJump();
    }
  }

  /** Walks the chat's past back from the phone until the pending jump's message lands. */
  async function loadAndJump() {
    if (!ui.pendingJump || ui.seeking) return;
    const { chat, id } = ui.pendingJump;
    ui.seeking = true;
    try {
      if (await messages.showStoredMessage(chat, id)) {
        await tick();
        scrollToMessage(id);
        return;
      }
      for (let round = 0; round < 10 && chats.selectedChat === chat; round++) {
        const before = messages.messages.at(-1)?.id;
        await messages.recallDay(chat, scroller ?? null);
        if (chats.selectedChat !== chat) return;
        await messages.showStoredMessage(chat, id);
        await tick();
        if (scroller?.querySelector(`[data-id="${id}"]`)) {
          ui.pendingJump = null;
          scrollToMessage(id);
          return;
        }
        if (messages.messages.at(-1)?.id === before) break;
      }
      ui.fail("Your phone did not send that message; it may be older than it keeps, or deleted.");
    } catch (e) {
      ui.fail(e);
    } finally {
      ui.seeking = false;
      ui.pendingJump = null;
    }
  }


  /** Fetches a message's media on demand. */
  // Stickers, voice notes and SVG files read as part of the conversation, so ones that
  // arrived before automatic fetching are fetched as soon as they are shown.
  const autoFetched = new Set<string>();
  $effect(() => {
    if (!session.connected) return;
    // A Set, not a scan per message: the list can hold thousands of rows.
    const onceIds = new Set(messages.marks.view_once.map((v) => v.id));
    for (const m of messages.messages) {
      if (m.media_path || !(m.media_kind === "sticker" || m.media_kind === "audio" || isSvg(m))) continue;
      if (autoFetched.has(m.id) || onceIds.has(m.id)) continue;
      autoFetched.add(m.id);
      void untrack(() => messages.downloadMedia(chats.selectedChat, m, true));
    }
  });

  async function create(value: unknown) {
    const chat = chats.selectedChat;
    if (!chat) return;
    await composer.enqueue(() =>
      ui.creating === "poll"
        ? invoke("create_poll", { chat, ...(value as object) })
        : invoke("create_event", { chat, event: value }),
    );
    await messages.reloadMessages(chat);
    await messages.loadMarks(chat);
    await chats.refreshChats();
    scrollToBottom();
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
  const viewerItems = $derived<ViewerItem[]>(viewableMessages(messages.ordered, viewOnceIds).map(viewerItem));
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
    const at = viewerItems.findIndex((item) => item.id === message.id);
    if (at >= 0) ui.viewerIndex = at;
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
      ui.fail(event.message || "Unexpected error");
    };
    const onRejection = (event: PromiseRejectionEvent) => {
      ui.fail(event.reason ?? "Unexpected error");
    };
    window.addEventListener("error", onError);
    window.addEventListener("unhandledrejection", onRejection);

    // Clicking a desktop notification opens its chat.
    const onOpenChat = (event: Event) => {
      const chat = (event as CustomEvent<string>).detail;
      if (chat) void openChat(chat);
    };
    window.addEventListener("postal:open-chat", onOpenChat);

    // Typing anywhere lands in the composer, so a chat can be answered without
    // clicking the field first.
    const onAnyKey = (event: KeyboardEvent) => {
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
      getScroller: () => scroller ?? null,
      reconnect,
    };

    async function setup() {
      await session.loadSettings();
      await once.refresh();

      // The listener is attached before connecting so no event can be missed.
      unlisten = await listen<ServiceEvent>("service-event", (event) =>
        dispatchServiceEvent(event.payload, host),
      );
      unlistenOnce = await listen<ServiceEvent>("once-event", () => once.refresh());

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
  onpaste={onPaste}
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
  ondrop={onDrop}
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
    onconnect={connect}
    onchoose={chooseAccount}
    onswitch={switchTo}
    onsettings={() => openSettings("accounts")} />
{:else}
  <div class="layout" style="grid-template-columns: {layoutColumns}">
    <ChatSidebar
      bind:searchQuery={chats.searchQuery}
      searchResults={chats.searchResults}
      visibleChats={chats.visibleChats}
      selectedChat={chats.selectedChat}
      chatFilter={chats.chatFilter}
      favoriteChats={favorites.chats}
      favoriteBusy={favorites.busy !== null}
      ontogglefavorite={(chat) => void act(() => composer.enqueue(() => favorites.toggle(chat.chat)))}
      onfilter={(filter) => (chats.chatFilter = filter)}
      unreadChats={chats.unreadChats}
      unreadPings={chats.unreadPings}
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
      globalAutoDownload={session.settings.auto_download_media}
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
      onresize={startResize} />

    <section class="conversation">
      {#if chats.selectedChat}
        {@const selectedChat = chats.selectedChat}
        {@const title =
          members.displayName(chats.chats.find((c) => c.chat === selectedChat)?.display_name ?? chats.titleOverride, selectedChat)}
        {@const typingNow = members.typingLabel(selectedChat)}
        <ChatHeader
          {selectedChat}
          isGroup={selectedChat.endsWith("@g.us")}
          {title}
          avatar={chats.avatars[selectedChat] ?? null}
          {typingNow}
          {subtitle}
          groupContext={members.groupContext}
          presenceText={members.presenceLabel(selectedChat)}
          mentionTotal={messages.mentionQueue.length}
          mentionCursor={messages.mentionCursor}
          pinned={pinnedView}
          ongroupinfo={() =>
            selectedChat.endsWith("@g.us") ? chats.openGroupInfo() : (contactInfoFor = selectedChat)}
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

        <MessageList
          messages={messages.ordered}
          isGroup={selectedChat.endsWith("@g.us")}
          switching={ui.switching}
          bind:scroller
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
          atLatest={messages.atLatest}
          onloadolder={() => messages.loadOlder(chats.selectedChat, false, scroller ?? null)}
          onloadnewer={() => messages.loadNewer(chats.selectedChat, scroller ?? null)}
          onlatest={async () => { if (await messages.showLatest(chats.selectedChat)) scrollToBottom(); }}
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
          {formatTime}
          onreplydraft={(m) => {
            composer.editing = null;
            composer.replyingTo = m;
            composerInput?.focus();
          }}
          onmenu={(e, m) => {
            e.preventDefault();
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
          onvote={(m, options) =>
            act(() => invoke("vote_poll", { chat: m.chat, id: m.id, options }))}
          onrespond={(m, response) =>
            act(() => invoke("respond_event", { chat: m.chat, id: m.id, response }))}
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
          oninviteopen={async (jid) => {
            await chats.refreshChats();
            void openChat(jid);
          }} />

        {#if ui.scrolledUp}
          <button class="jump" onclick={scrollToBottom}>
            Latest <Icon name="chevronDown" size={15} />
          </button>
        {/if}

        {@render syncStatus()}

        {#if ui.picking}
          <SelectionBar
            count={Object.keys(ui.picking).length}
            allStarred={Object.keys(ui.picking).length > 0 && Object.keys(ui.picking).every((id) => messages.starred.has(id))}
            onforward={() => {
              const batch = pickedInOrder(ui.picking, messages.ordered);
              if (batch.length > 0) ui.forwarding = batch;
            }}
            ondelete={() => (ui.bulkDelete = Object.keys(ui.picking ?? {}))}
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
          <ComposerBar
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
          oncreatekind={(kind) => (ui.creating = kind)}
          oninput={(e) => composer.onComposerInput(e)}
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

        {#if members.chatGroup && !members.chatGroup.can_send}
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
    globalAutoDownload={session.settings.auto_download_media}
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

{#if ui.viewerIndex !== null && viewerItems.length > 0}
  <MediaViewer
    items={viewerItems}
    bind:index={ui.viewerIndex}
    onclose={() => (ui.viewerIndex = null)}
    onopen={openMedia}
    onreply={(id) => {
      composer.editing = null;
      composer.replyingTo = messages.messages.find((m) => m.id === id) ?? null;
      ui.viewerIndex = null;
      composerInput?.focus();
    }}
    onjump={(id) => {
      ui.viewerIndex = null;
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

{#if contactInfoFor}
  {@const jid = contactInfoFor}
  <ContactInfo
    {jid}
    title={members.displayName(chats.chats.find((c) => c.chat === jid)?.display_name ?? null, jid)}
    picture={chats.pictureOf(jid)}
    aliases={members.aliasesFor(jid)}
    onclose={() => (contactInfoFor = null)} />
{/if}

{#if chats.showGroupInfo && chats.selectedChat}
  {@const selectedChat = chats.selectedChat}
  {@const chat = chats.chats.find((c) => c.chat === selectedChat)}
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
    onreports={() => invoke<AdminReport[]>("admin_reports", { chat: selectedChat })}
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

{#if ui.showSettings}
  <Settings
    settings={session.settings}
    accounts={session.accountList}
    active={session.activeAccount}
    me={session.me}
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
  :global(:focus-visible) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  :global(input:focus-visible, textarea:focus-visible) {
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
    color: hsl(var(--hue) 65% 68%);
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
  .jump {
    position: absolute;
    bottom: 76px;
    right: 20px;
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
