<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "$lib/ipc";
  import { listen } from "@tauri-apps/api/event";
  import StarredList from "$lib/StarredList.svelte";
  import MessageFinder, { type FoundItem } from "$lib/MessageFinder.svelte";
  import ChatSettings, { type ChatRetention } from "$lib/ChatSettings.svelte";
  import ProfileCard from "$lib/ProfileCard.svelte";
  import MessageInfo from "$lib/MessageInfo.svelte";
  import { isPlaceholder } from "$lib/phone";
  import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";
  import flagFont from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";

  // Windows has no flag glyphs and draws the two letters instead. The font is
  // bundled, so nothing is fetched at runtime; elsewhere this is a no-op.
  polyfillCountryFlagEmojis("Twemoji Country Flags", flagFont);
  import Icon from "$lib/Icon.svelte";
  import Button from "$lib/Button.svelte";
  import Spinner from "$lib/Spinner.svelte";
  import ConfirmDialog from "$lib/ConfirmDialog.svelte";
  import PairingView from "$lib/PairingView.svelte";
  import ChatSidebar from "$lib/ChatSidebar.svelte";
  import ChatHeader from "$lib/ChatHeader.svelte";
  import MessageList from "$lib/MessageList.svelte";
  import ComposerBar from "$lib/ComposerBar.svelte";
  import { hue } from "$lib/avatar";
  import { bare, captionOf, dayKey, dayLabel, formatTime, isSvg, MEDIA_LABELS } from "$lib/message";
  import { chats } from "$lib/state/chats.svelte";
  import { composer } from "$lib/state/composer.svelte";
  import { dispatchServiceEvent, refreshResolvedNames } from "$lib/state/events";
  import { members } from "$lib/state/members.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { session } from "$lib/state/session.svelte";
  import { ui } from "$lib/state/ui.svelte";
  import Settings, { type Section } from "$lib/Settings.svelte";
  import GroupInfo, { type AdminReport } from "$lib/GroupInfo.svelte";
  import MediaViewer, { type ViewerItem } from "$lib/MediaViewer.svelte";
  import MessageMenu, { type MenuItem } from "$lib/MessageMenu.svelte";
  import ChatPicker from "$lib/ChatPicker.svelte";
  import type { ChatEvent } from "$lib/models";
  import CreateDialog from "$lib/CreateDialog.svelte";
  import { plain } from "$lib/format";
  import {
    activeTheme,
    appPicture,
    applyTheme,
    chatPicture,
    customization,
    lensMap,
    save as saveCustomization,
  } from "$lib/theme.svelte";

  import type {
    ChatPrivacy,
    ConnectionState,
    SearchResult,
    ServiceEvent,
    StoredMessage,
  } from "$lib/models";

  function openSettings(section: Section) {
    ui.settingsSection = section;
    ui.accountMenu = false;
    ui.showSettings = true;
  }

  $effect(() => {
    applyTheme(activeTheme());
    saveCustomization();
  });

  /** The app's background picture, loaded from IndexedDB. */
  let appPictureUrl = $state<string | null>(null);
  $effect(() => {
    void customization.background?.v;
    if (!customization.background) {
      appPictureUrl = null;
      return;
    }
    appPicture()
      .then((url) => (appPictureUrl = url ?? null))
      .catch(() => {});
  });

  /** The user's background picture, else the theme's wallpaper, as a CSS background. */
  const wallpaper = $derived.by(() => {
    if (!appPictureUrl || !customization.background) return activeTheme().wallpaper ?? null;
    const dim = `rgba(0, 0, 0, ${customization.background.dim})`;
    return `linear-gradient(${dim}, ${dim}), url("${appPictureUrl}") center / cover no-repeat, #000`;
  });

  /** The theme's own layer, then CSS extensions, kept from closing their style element. */
  const extensionCss = $derived(
    [
      {
        id: `theme-${activeTheme().id}`,
        // The wallpaper is its own oversized layer behind everything, so a theme can move it
        // cheaply. A picture also shows through the chat, which is otherwise opaque.
        css:
          (wallpaper
            ? `html, body { background: transparent !important; }
               .stage { isolation: isolate; }
               body::before, .stage::before { content: ""; position: fixed; inset: -25%; z-index: -1;
                 pointer-events: none; background: ${wallpaper}; }
               .stage::before { position: absolute; }`
            : "") +
          // A picture stays still: moving it would re-filter every glass surface on each frame.
          (appPictureUrl
            ? `.conversation, .pairing { background: color-mix(in srgb, var(--chat-bg) 55%, transparent) !important; }
               body::before, .stage::before { animation: none !important; }`
            : "") +
          (activeTheme().css ?? ""),
      },
      ...customization.extensions.filter((e) => e.enabled),
    ]
      .filter((e) => e.css.trim())
      .map((e) => `<style data-extension="${e.id}">${e.css.replace(/<\/style/gi, "<\\/style")}</style>`)
      .join(""),
  );

  $effect(() => {
    session.applyZoom();
  });

  /** The open chat's own background picture, loaded from IndexedDB. */
  let chatPictureUrl = $state<string | null>(null);
  $effect(() => {
    const jid = chats.selectedChat;
    const meta = jid ? customization.chatBackgrounds?.[jid] : undefined;
    if (!jid || !meta) {
      chatPictureUrl = null;
      return;
    }
    let live = true;
    void meta.v;
    chatPicture(jid)
      .then((url) => live && (chatPictureUrl = url ?? null))
      .catch(() => {});
    return () => {
      live = false;
    };
  });
  const chatPictureCss = $derived.by(() => {
    if (!chatPictureUrl || !chats.selectedChat) return "";
    const dim = `rgba(0, 0, 0, ${customization.chatBackgrounds?.[chats.selectedChat]?.dim ?? 0.25})`;
    return `<style data-chat-picture>.conversation { background: linear-gradient(${dim}, ${dim}), url("${chatPictureUrl}") center / cover no-repeat !important; }</style>`;
  });
  let composerInput: HTMLTextAreaElement | undefined = $state();
  // The composer module reads the element at event time; synced here.
  $effect(() => {
    composer.inputEl = composerInput;
  });

  // The chat list width is a customization setting; older builds kept it under its own key.
  if (customization.listWidth === undefined) {
    try {
      customization.listWidth = Number(localStorage.getItem("hermodr.sidebarWidth")) || 300;
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
    if (chats.selectedChat !== chat) {
      composer.stopTyping();
      ui.switching = true;
      members.chatGroup = null;
      // A staged reply or edit belongs to the chat it was started in.
      composer.replyingTo = null;
      composer.editing = null;
      composer.resetHistory();
    }
    chats.selectedChat = chat;
    // One-to-one typing only arrives for contacts we are subscribed to.
    if (!chat.endsWith("@g.us")) invoke("watch_presence", { jid: chat }).catch(() => {});
    chats.titleOverride = label;
    ui.scrolledUp = false;
    messages.prepareChat();
    composer.chatPrivacy = { send_typing: null, send_receipts: null };
    invoke<{ retention: ChatRetention } & ChatPrivacy>("chat_settings", { chat })
      .then((s) => {
        if (chats.selectedChat !== chat) return;
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
    try {
      // Invalidate any in-flight reload from the previous chat.
      const seq = messages.nextSeq();
      // Mentions are captured before the chat is marked read, since that clears them.
      const [mentions, loaded] = await Promise.all([
        invoke<string[]>("unread_mentions", { chat }).catch(() => [] as string[]),
        invoke<StoredMessage[]>("messages", { chat, limit: messages.messageLimit }),
      ]);
      // A quicker click on another chat has already taken over.
      if (chats.selectedChat !== chat || seq !== messages.messagesSeq) return;
      messages.mentionQueue = mentions;
      messages.mentionCursor = 0;
      messages.messages = loaded;
      await messages.loadMarks(chat);
      scrollToBottom();
      await tick();
      ui.switching = false;
      // Opening a conversation is what marks it seen.
      await invoke("mark_read", { chat });
      await chats.refreshChats();
    } catch (e) {
      ui.switching = false;
      ui.fail(e);
    }
    // Group members power the @ autocomplete, and the group's settings decide
    // whether we may write; a one-to-one chat has neither.
    await members.loadChatGroup(chat, () => chats.selectedChat === chat);
    // Opening a chat is the obvious moment to start typing.
    await tick();
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

  /** Clears everything tied to the current account before switching. */
  function resetUi() {
    chats.resetAccount();
    session.resetAccount();
    messages.resetAccount();
    members.resetAccount();
    composer.resetAccount();
    ui.resetAccount();
  }

  /** Starts the account picked on the launch chooser. */
  async function chooseAccount(id: string) {
    session.choosingAccount = false;
    if (id === session.activeAccount) await connect();
    else await switchTo(id);
  }

  async function switchTo(id: string) {
    if (id === session.activeAccount) return;
    try {
      resetUi();
      session.connected = false;
      await session.showQr(null);
      await invoke("switch_account", { id });
      await session.loadAccounts();
      await syncState();
    } catch (e) {
      ui.fail(e);
    }
  }

  async function removeAccount(id: string) {
    try {
      if (id === session.activeAccount) {
        resetUi();
        session.connected = false;
        await session.showQr(null);
      }
      await invoke("remove_account", { id });
      await session.loadAccounts();
      await syncState();
    } catch (e) {
      ui.fail(e);
    }
  }

  async function addAccount() {
    try {
      resetUi();
      session.connected = false;
      await session.showQr(null);
      await invoke("add_account", {});
      await session.loadAccounts();
      await syncState();
    } catch (e) {
      ui.fail(e);
    }
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
      localStorage.setItem(`hermodr.avatar.${session.activeAccount}`, own);
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
    // Wait for the new messages to render before measuring.
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
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
    if (!untrack(() => ui.scrolledUp)) scrollToBottom();
  });

  function onScroll() {
    if (!scroller) return;
    const distance = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    ui.scrolledUp = distance > 120;
    if (
      scroller.scrollTop < 80 &&
      messages.loadOnScroll &&
      !messages.olderExhausted &&
      !messages.loadingOlder &&
      messages.messages.length > 0
    ) {
      void messages.loadOlder(chats.selectedChat, true);
    }
  }



  /** Media extensions that can be staged from a pasted file path. */
  const PASTABLE = /\.(jpe?g|png|gif|webp|svg|mp4|mov|m4v|webm|mkv|ogg|opus|mp3|m4a|aac|wav)$/i;

  function mimeForName(name: string) {
    const extension = name.slice(name.lastIndexOf(".") + 1).toLowerCase();
    const table: Record<string, string> = {
      jpg: "image/jpeg",
      jpeg: "image/jpeg",
      png: "image/png",
      gif: "image/gif",
      webp: "image/webp",
      svg: "image/svg+xml",
      mp4: "video/mp4",
      mov: "video/mp4",
      m4v: "video/mp4",
      webm: "video/webm",
      mkv: "video/x-matroska",
      ogg: "audio/ogg",
      opus: "audio/ogg",
      mp3: "audio/mpeg",
      m4a: "audio/mp4",
      aac: "audio/mp4",
      wav: "audio/wav",
    };
    return table[extension] ?? "application/octet-stream";
  }

  /**
   * Reads a pasted file, either as clipboard bytes or from a copied file path.
   *
   * WebKitGTK does not put clipboard images in the paste event's
   * `clipboardData`; only the async clipboard API reaches them. Copying a file
   * in the file manager usually exposes just a `text/uri-list`, so that path is
   * read back through the shell (restricted to media extensions) instead.
   */
  async function clipboardFile(): Promise<File | null> {
    try {
      const items = await navigator.clipboard?.read();
      for (const item of items ?? []) {
        const media = item.types.find(
          (t) => t.startsWith("image/") || t.startsWith("video/") || t.startsWith("audio/"),
        );
        if (media) {
          const blob = await item.getType(media);
          // send_media classifies by file extension, so a pasted item needs a
          // real one or a photo goes out as a document.
          const sub = media.split("/")[1]?.split(";")[0] || "bin";
          const extension = sub === "jpeg" ? "jpg" : sub;
          return new File([blob], `pasted.${extension}`, { type: media });
        }
        if (item.types.includes("text/uri-list")) {
          const text = await (await item.getType("text/uri-list")).text();
          const uri = text
            .split("\n")
            .map((line) => line.trim())
            .find((line) => line.length > 0);
          if (!uri?.startsWith("file://")) continue;
          // `file:///C:/x` has the pathname `/C:/x` on Windows.
          const path = decodeURIComponent(new URL(uri).pathname).replace(/^\/([A-Za-z]:)/, "$1");
          if (!PASTABLE.test(path)) continue;
          const data = await invoke<string>("read_file", { path });
          const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
          return new File([bytes], path.split("/").pop() ?? "pasted", { type: mimeForName(path) });
        }
      }
    } catch {
      // Nothing readable; the caller falls back to a hint.
    }
    return null;
  }

  /**
   * Stages pasted image bytes and blocks the default paste otherwise.
   *
   * Pasting a *file* (copying it in the file manager) puts a `text/uri-list` on
   * the clipboard rather than an image. Without `preventDefault` WebKit then
   * navigates the whole webview to that URI. That navigation is fatal: wry's
   * page-load handler does `webview.uri().unwrap()`, the URI is absent for such
   * a load, and the panic aborts the process. So anything file-like is
   * swallowed; only real image bytes are staged, and plain text stays native.
   */
  async function onPaste(event: ClipboardEvent) {
    const data = event.clipboardData;
    const item = data
      ? Array.from(data.items).find(
          // A file copied in Explorer arrives here as a file item of any type.
          (i) => i.type.startsWith("image/") || i.type.startsWith("video/") || i.kind === "file",
        )
      : undefined;
    const isUriList = data ? Array.from(data.types).includes("text/uri-list") : false;
    const isPlainText =
      !item && !isUriList && !!data && Array.from(data.types).includes("text/plain");
    if (isPlainText) return;
    if (item || isUriList || data) event.preventDefault();

    const file = item?.getAsFile() ?? (await clipboardFile());
    if (file) void composer.stageFile(file);
    else if (isUriList) ui.fail("Could not read that file. Try the 📎 button.");
  }

  /** Dropping files stages them; dropping anything else must not navigate. */
  function onDrop(event: DragEvent) {
    event.preventDefault();
    for (const file of Array.from(event.dataTransfer?.files ?? [])) void composer.stageFile(file);
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

  async function openStarred() {
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

  async function openPings(chat: string | null) {
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
  async function searchChat(query: string, more = false) {
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

  /** Walks the chat's past back from the phone until the pending jump's message lands. */
  async function loadAndJump() {
    if (!ui.pendingJump || ui.seeking) return;
    const { chat, id } = ui.pendingJump;
    ui.seeking = true;
    try {
      // Ten rounds of 50 reach about 500 messages back before giving up.
      for (let round = 0; round < 10 && chats.selectedChat === chat; round++) {
        const before = messages.messages.length;
        await invoke("load_older", { chat, count: 50 });
        // The phone answers as a history sync event; give it a moment to land.
        await new Promise((r) => setTimeout(r, 2500));
        messages.messageLimit += 50;
        await messages.reloadMessages(chat, true, scroller);
        await tick();
        if (scroller?.querySelector(`[data-id="${id}"]`)) {
          ui.pendingJump = null;
          scrollToMessage(id);
          return;
        }
        if (messages.messages.length === before) break;
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
    for (const m of messages.messages) {
      if (m.media_path || !(m.media_kind === "sticker" || m.media_kind === "audio" || isSvg(m))) continue;
      if (autoFetched.has(m.id) || messages.marks.view_once.some((v) => v.id === m.id)) continue;
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

  function eventFields(event: ChatEvent) {
    const { name, description, start, end, location, link } = event;
    return { name, description, start, end, location, link };
  }

  async function saveEvent(chat: string, id: string, fields: object) {
    await composer.enqueue(() => invoke("edit_event", { chat, id, event: fields }));
    await messages.reloadMessages(chats.selectedChat);
    await messages.loadMarks(chats.selectedChat);
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


  const QUICK_REACTIONS = ["👍", "❤️", "😂", "😮", "😢", "🙏"];
  // Later readers in a group change no status, so the open info refreshes itself.
  $effect(() => {
    if (!ui.infoFor) return;
    const timer = setInterval(() => (ui.infoVersion += 1), 3000);
    return () => clearInterval(timer);
  });

  function target(m: StoredMessage) {
    return { chat: m.chat, id: m.id, sender: m.sender, fromMe: m.from_me };
  }

  /** Runs a message action, surfacing a failure instead of dropping it. */
  async function act(run: () => Promise<unknown>) {
    try {
      await run();
    } catch (e) {
      ui.fail(e);
    }
  }

  function menuItems(m: StoredMessage): MenuItem[] {
    const group = m.chat.endsWith("@g.us");
    const other = group && !m.from_me;
    const text = m.media_kind ? captionOf(m) : m.text;
    const items: MenuItem[] = [
      {
        label: "Reply",
        icon: "reply",
        action: () => {
          composer.editing = null;
          composer.replyingTo = m;
          composerInput?.focus();
        },
      },
    ];
    if (m.from_me) {
      items.push({ label: "Message info", icon: "check", action: () => (ui.infoFor = m) });
    }
    if (other) {
      items.push(
        {
          label: "Reply privately",
          icon: "users",
          action: async () => {
            await openChat(bare(m.sender));
            composer.editing = null;
            composer.replyingTo = m;
            composerInput?.focus();
          },
        },
        {
          label: `Message ${members.senderLabel(m)}`,
          icon: "message",
          action: () => openChat(bare(m.sender)),
        },
      );
    }
    if (text && !m.revoked) {
      items.push({
        label: "Copy",
        icon: "copy",
        action: () => act(() => navigator.clipboard.writeText(text)),
      });
    }
    if (!m.revoked) {
      items.push(
        { label: "Forward", icon: "forward", action: () => (ui.forwarding = m) },
        {
          label: messages.marks.pinned === m.id ? "Unpin" : "Pin",
          icon: "pin",
          action: () =>
            act(() =>
              invoke("pin_message", { target: target(m), pinned: messages.marks.pinned !== m.id }),
            ),
        },
        {
          label: messages.starred.has(m.id) ? "Unstar" : "Star",
          icon: "star",
          action: () =>
            act(() => invoke("star", { target: target(m), starred: !messages.starred.has(m.id) })),
        },
      );
    }
    if (other) {
      items.push({
        label: "Report to admins",
        icon: "flag",
        separated: true,
        action: () => (ui.reporting = m),
      });
    }
    items.push({
      label: "Delete",
      icon: "trash",
      danger: true,
      separated: !other,
      action: () => (ui.deleting = m),
    });
    return items;
  }

  /** Whether we may delete this message for everyone: ours, or ours to moderate. */
  function canDeleteForEveryone(m: StoredMessage) {
    if (m.revoked) return false;
    if (m.from_me) return true;
    return !!session.me && !!members.memberOf(session.me)?.admin;
  }

  async function deleteMessage(everyone: boolean) {
    const m = ui.deleting;
    ui.deleting = null;
    if (!m) return;
    await act(async () => {
      await invoke("delete_message", { target: target(m), everyone, timestamp: m.timestamp });
      await messages.reloadMessages(chats.selectedChat);
      await chats.refreshChats();
    });
  }

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
  const viewerItems = $derived<ViewerItem[]>(
    messages.ordered
      .filter(
        (m) =>
          !m.revoked &&
          !!m.media_path &&
          !viewOnceIds.has(m.id) &&
          (m.media_kind === "image" || m.media_kind === "video" || m.media_kind === "gif"),
      )
      .map(viewerItem),
  );
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

  async function syncState() {
    const state = await invoke<ConnectionState>("connection_state");
    session.started = state.started;
    session.connected = state.connected;
    await session.showQr(state.connected ? null : state.qr);
    if (session.connected) {
      // Already connected when the UI loaded without an explicit connect (e.g.
      // a webview reload): there is no fresh backlog to gate on, so do not hold
      // the loading screen. A cold start reaches here disconnected, then gates.
      if (!session.connectRequested && !session.gateDone) session.gateDone = true;
      await chats.refreshChats();
    }
  }

  /** Connects, reusing a stored session when there is one. */
  async function connect() {
    session.connecting = true;
    ui.error = null;
    session.connectRequested = true;
    try {
      await invoke("connect");
      await syncState();
      void refreshResolvedNames();
    } catch (e) {
      ui.fail(e);
    } finally {
      session.connecting = false;
    }
  }

  /** Reconnects after the backend logged the account out. */
  async function reconnect() {
    session.connected = false;
    session.started = false;
    await session.showQr(null);
    await session.loadAccounts();
    await connect();
  }


  onMount(() => {
    let unlisten: (() => void) | undefined;

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

      // The listener is attached before connecting so no event can be missed.
      unlisten = await listen<ServiceEvent>("service-event", (event) =>
        dispatchServiceEvent(event.payload, host),
      );

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
      window.removeEventListener("error", onError);
      window.removeEventListener("unhandledrejection", onRejection);
      window.removeEventListener("keydown", onAnyKey);
    };
  });
</script>

<svelte:head>
  <title>Hermóðr</title>
  {@html extensionCss}
  {@html chatPictureCss}
</svelte:head>

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

<!-- Avatar rendering lives in $lib/Avatar.svelte (initials/hue in $lib/avatar.ts). -->

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
      onresize={startResize} />

    <section class="conversation">
      {#if chats.selectedChat}
        {@const selectedChat = chats.selectedChat}
        {@const title =
          chats.chats.find((c) => c.chat === selectedChat)?.display_name ??
          chats.titleOverride ??
          members.displayName(null, selectedChat)}
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
          ongroupinfo={() => chats.openGroupInfo()}
          onsearch={() =>
            (ui.finder = {
              mode: "search",
              chat: selectedChat,
              items: [],
              reach: messages.messages.at(-1)?.timestamp ?? null,
              more: !messages.olderExhausted,
            })}
          onpings={() => openPings(selectedChat)}
          onsettings={() => (ui.chatSettingsOpen = true)}
          onjumpmention={jumpNextMention}
          onpinnedjump={(id) => scrollToMessage(id)} />

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
          replyingToId={composer.replyingTo?.id ?? null}
          highlightedId={ui.highlightedId}
          menuId={ui.menu?.message.id ?? null}
          polls={messages.marks.polls}
          events={messages.marks.events}
          namer={(jid) => (jid === "@me" ? "You" : members.senderName(jid))}
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
          onloadolder={() => messages.loadOlder(chats.selectedChat)}
          uploads={composer.outgoing.filter((o) => o.chat === selectedChat)}
          typers={members.typing[selectedChat] ?? []}
          typerLabelOf={(sender) => {
            const member = members.memberOf(sender);
            const name = member && !isPlaceholder(member.name) ? member.name : null;
            return name ?? members.senderName(sender);
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
          ondownload={(m) => messages.downloadMedia(chats.selectedChat, m)}
          onopenviewer={openViewer}
          onopenmedia={openMedia}
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
          onreact={(m, emoji) => act(() => invoke("react", { target: target(m), emoji }))}
          onmarkplayed={(m) => messages.markPlayed(m)}
          onnextvoice={(m) => messages.playNextVoice(m)}
          onpausevoice={() => (messages.autoplayId = null)}
          onreplymenu={(e, m) => {
            const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
            ui.menu = { x: rect.left, y: rect.bottom + 4, message: m };
          }}
          ononce={(m) => {
            if (!m.media_path) return messages.downloadMedia(chats.selectedChat, m);
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
          bind:sendOnce={composer.sendOnce}
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
          enqueue={<T>(task: () => Promise<T>) => composer.enqueue(task)}
          onpickeremoji={(emoji) => composer.insertAtCaret(emoji)}
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
          oncancelreply={() => (composer.replyingTo = null)}
          oncanceledit={() => composer.cancelEditing()}
          onremove={(id) => composer.removePending(id)}
          onsendvoice={(note) => composer.sendVoice(note)}
          onvoiceerror={(message) => (ui.error = message)}
          onreceipts={() => composer.toggleChatReceipts()}
          ontyping={() => composer.toggleChatTyping()}
          receiptsHidden={composer.receiptsHidden}
          typingHidden={composer.typingHidden} />

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
      {/if}
    </section>

  </div>
{/if}
</div>

{#if ui.menu}
  {@const m = ui.menu.message}
  <MessageMenu
    x={ui.menu.x}
    y={ui.menu.y}
    items={menuItems(m)}
    reactions={QUICK_REACTIONS}
    current={messages.reactionsFor.get(m.id)?.find((r) => r.mine)?.emoji ?? null}
    onreact={(emoji) => act(() => invoke("react", { target: target(m), emoji }))}
    onclose={() => (ui.menu = null)} />
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
  {@const m = ui.forwarding}
  <ChatPicker
    title="Forward message to"
    chats={chats.chats.map((c) => ({ jid: c.chat, label: chats.chatLabel(c), avatar: chats.avatars[c.chat] ?? null }))}
    onpick={async (to) => {
      await composer.enqueue(() => invoke("forward_message", { chat: m.chat, id: m.id, to }));
      await chats.refreshChats();
    }}
    onclose={() => (ui.forwarding = null)} />
{/if}

{#if ui.deleting}
  {@const m = ui.deleting}
  <ConfirmDialog
    label="Delete message"
    title="Delete message?"
    hint={canDeleteForEveryone(m)
      ? "Delete it for everyone in this chat, or only from your devices."
      : "It is removed from your devices only."}
    onclose={() => (ui.deleting = null)}>
    {#snippet actions()}
      {#if canDeleteForEveryone(m)}
        <button class="danger" onclick={() => deleteMessage(true)}>Delete for everyone</button>
      {/if}
      <button class="danger" onclick={() => deleteMessage(false)}>Delete for me</button>
      <button onclick={() => (ui.deleting = null)}>Cancel</button>
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
    onclose={() => (ui.chatSettingsOpen = false)} />
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
    namer={(name, jid) => (name && !isPlaceholder(name) ? name : members.senderName(jid))}
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
    onallowreports={(allow) => invoke("set_allow_admin_reports", { chat: selectedChat, allow })}
    onjump={(id) => {
      chats.showGroupInfo = false;
      if (chats.selectedChat) void jumpTo(chats.selectedChat, id);
    }}
    onlabel={async (label) => {
      await invoke("set_member_label", { chat: selectedChat, label });
      const user = session.me?.split("@")[0];
      for (const list of [chats.groupInfo?.participants ?? [], members.participants]) {
        const self = list.find((p) => p.jid === session.me || p.number === user);
        if (self) self.label = label || null;
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
    onsave={(next) => session.saveSettings(next)}
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
  /* Animations off (setting or OS): nothing moves, whatever its duration. */
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
  /* Dismiss buttons live in $lib/Button.svelte (icon variant, already muted). */
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
  /* Message text lives in $lib/MessageText.svelte. The mention avatar sizes
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
  /* Chat filter pills live in $lib/Button.svelte (chip variant). */
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
  /* Menu rows live in $lib/Button.svelte (menu variant). */
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
  /* Reserves room on the last line so the time never covers text. The spacer
     renders in MessageText while the bubble context lives in MessageBubble.
     Widths cover the widest metas measured (12h clock + star + "Edited":
     theirs 48px, mine 76px, edited mine 109px) plus headroom for wider fonts. */
  :global(.meta-spacer) {
    display: inline-block;
    width: 52px;
    height: 1px;
  }
  :global(.meta-spacer.mine) {
    width: 88px;
  }
  :global(.bubble.edited .meta-spacer) {
    width: 94px;
  }
  :global(.bubble.edited .meta-spacer.mine) {
    width: 120px;
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
  /* Active tint comes from Button itself; the ring keeps its dashed/solid states. */
  :global(.once-toggle) {
    width: 24px;
    height: 24px;
    border: 2px dashed currentColor;
    border-radius: 50%;
    font: inherit;
    font-size: 11px;
    font-weight: 800;
  }
  :global(.once-toggle.on) {
    border-style: solid;
  }
  :global(.tool-text) {
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.04em;
  }
  /* Discord-style completion list over the composer. */
  /* WhatsApp's reaction pill, hanging off the bubble's bottom edge. */
  /* Confirm sheets live in $lib/ConfirmDialog.svelte. */
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
  /* Shared button shapes live in $lib/Button.svelte. The attach button keeps
     its composer box here since it arrives through Button's `cls`. */
  :global(.attach) {
    width: 42px;
    height: 42px;
    margin-bottom: 5px;
  }
</style>
