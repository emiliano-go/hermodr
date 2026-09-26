<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { invoke } from "$lib/ipc";
  import { listen } from "@tauri-apps/api/event";
  import StarredList, { type StarredItem } from "$lib/StarredList.svelte";
  import MessageFinder, { type FoundItem } from "$lib/MessageFinder.svelte";
  import ChatSettings, { type ChatRetention } from "$lib/ChatSettings.svelte";
  import ProfileCard from "$lib/ProfileCard.svelte";
  import MessageInfo from "$lib/MessageInfo.svelte";
  import { displayName as phoneName, isPlaceholder } from "$lib/phone";
  import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";
  import flagFont from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";

  // Windows has no flag glyphs and draws the two letters instead. The font is
  // bundled, so nothing is fetched at runtime; elsewhere this is a no-op.
  polyfillCountryFlagEmojis("Twemoji Country Flags", flagFont);
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import Button from "$lib/Button.svelte";
  import Spinner from "$lib/Spinner.svelte";
  import ConfirmDialog from "$lib/ConfirmDialog.svelte";
  import PairingView from "$lib/PairingView.svelte";
  import ChatSidebar from "$lib/ChatSidebar.svelte";
  import ChatHeader from "$lib/ChatHeader.svelte";
  import MessageList from "$lib/MessageList.svelte";
  import ComposerBar from "$lib/ComposerBar.svelte";
  import { hue } from "$lib/avatar";
  import { imagePreview, rasterizeSvg } from "$lib/files";
  import { bare, isSvg, MEDIA_LABELS } from "$lib/message";
  import Settings, { type Section } from "$lib/Settings.svelte";
  import GroupInfo, { type AdminReport } from "$lib/GroupInfo.svelte";
  import MediaViewer, { type ViewerItem } from "$lib/MediaViewer.svelte";
  import MessageMenu, { type MenuItem } from "$lib/MessageMenu.svelte";
  import ChatPicker from "$lib/ChatPicker.svelte";
  import { keybinds, matches } from "$lib/keybinds.svelte";
  import type { PickerTab } from "$lib/ExpressionPicker.svelte";
  import type { Recording } from "$lib/VoiceRecorder.svelte";
  import { loadEmojis, rememberEmoji, searchEmojis, type Emoji } from "$lib/emoji";
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
    Account,
    ChatPrivacy,
    ChatSummary,
    ConnectionState,
    GroupInfo as GroupData,
    Marks,
    Member,
    Outgoing,
    PendingMedia,
    Retention,
    SearchResult,
    ServiceEvent,
    StoredMessage,
    UiSettings,
  } from "$lib/models";

  let settingsSection = $state<Section>("accounts");
  let accountMenu = $state(false);
  /** Our own JID, for the account panel's picture and number. */
  let me = $state<string | null>(null);
  /** Bumped when we replace our picture, which keeps its file name. */
  let meVersion = $state(0);

  function openSettings(section: Section) {
    settingsSection = section;
    accountMenu = false;
    showSettings = true;
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

  /** How many messages the open chat shows; "load older" raises it. */
  const PAGE = 200;
  let messageLimit = $state(PAGE);
  let loadingOlder = $state(false);
  let olderTimer: ReturnType<typeof setTimeout> | undefined;
  /** Cached profile picture per chat; `null` means it has none. */
  let avatars: Record<string, string | null> = $state({});
  const requestedAvatars = new Set<string>();
  const avatarQueue: string[] = [];
  let avatarWorkers = 0;

  let connected = $state(false);
  let connecting = $state(false);
  let started = $state(false);
  /** Launch chooser, shown when several linked accounts could be signed into. */
  let choosingAccount = $state(false);
  /** True while a newly opened chat's messages load, so the old ones fade out. */
  let switching = $state(false);
  let qrSvg = $state<string | null>(null);
  let chats: ChatSummary[] = $state([]);
  let selectedChat = $state<string | null>(null);
  /** Interface scale, persisted under `hermodr.zoom`; Ctrl +/-/0 adjust it. */
  function storedZoom() {
    try {
      const saved = Number(localStorage.getItem("hermodr.zoom"));
      return Number.isFinite(saved) && saved ? Math.min(2, Math.max(0.6, saved)) : 1;
    } catch {
      return 1;
    }
  }
  let zoom = $state(storedZoom());
  function setZoom(next: number) {
    zoom = Math.min(2, Math.max(0.6, Math.round(next * 10) / 10));
  }
  $effect(() => {
    document.documentElement.style.zoom = String(zoom);
    try {
      localStorage.setItem("hermodr.zoom", String(zoom));
    } catch {
      // The scale lasts this session then.
    }
  });

  /** The open chat's own background picture, loaded from IndexedDB. */
  let chatPictureUrl = $state<string | null>(null);
  $effect(() => {
    const jid = selectedChat;
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
    if (!chatPictureUrl || !selectedChat) return "";
    const dim = `rgba(0, 0, 0, ${customization.chatBackgrounds?.[selectedChat]?.dim ?? 0.25})`;
    return `<style data-chat-picture>.conversation { background: linear-gradient(${dim}, ${dim}), url("${chatPictureUrl}") center / cover no-repeat !important; }</style>`;
  });
  let messages: StoredMessage[] = $state([]);
  /** Newest-first request id; a slow `messages` response must not win over a newer one. */
  let messagesSeq = 0;
  /** Voice note to play next, set when the previous one ends on its own. */
  let autoplayId = $state<string | null>(null);
  /** Offline-backlog progress: how many the server announced and how many stored. */
  let syncPending = $state(0);
  let syncApplied = $state(0);  
  /** Backlog applied, waiting for the first chat/message paint to land. */
  let finalizing = $state(false);
  /** The loading screen may be left. Survives reconnects for this launch. */
  let gateDone = $state(false);
  /** The gate hit its cap and revealed; sync keeps going in the background. */
  let syncTimedOut = $state(false);
  let gateTimer: ReturnType<typeof setTimeout> | undefined;
  /** Set once an explicit connect starts, so a reload without one still reveals. */
  let connectRequested = false;
  /** Caps how long the loading screen can hold, so a stuck sync never hangs the app. */
  function startGateTimeout() {
    clearTimeout(gateTimer);
    gateTimer = setTimeout(() => {
      if (!gateDone) {
        syncTimedOut = true;
        gateDone = true;
      }
    }, 60_000);
  }
  const syncPercent = $derived(
    syncPending > 0 ? Math.min(100, Math.round((syncApplied / syncPending) * 100)) : 0,
  );
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
  /** Set while a "load older" answer is in flight; its `historyLoaded` is the flush. */
  let historyActive = false;

  /**
   * Marks what a change touched when fetching right away would be wasteful or
   * invisible: the gate is closed, a drain is running, or a history answer is
   * in flight. Returns false when the caller should refresh immediately.
   */
  function deferRefresh(chat: string | null, markRead = false) {
    if (uiUnlocked && syncPending === 0 && !historyActive) return false;
    chatsDirty = true;
    if (chat && chat === selectedChat) {
      messagesDirty = true;
      dirtyMarkRead ||= markRead;
    }
    return true;
  }
  let draft = $state("");
  /** Per-chat composer text, so switching chats does not lose what was typed. */
  let drafts: Record<string, string> = $state({});
  let composerInput: HTMLTextAreaElement | undefined = $state();
  /** Group members for the @ autocomplete. */
  let participants: Member[] = $state([]);
  /** Last member list per group, shown while a switch reloads it so the header does not flash. */
  const memberCache: Record<string, Member[]> = {};
  /** O(1) member lookups, rebuilt only when the member list changes. */
  const memberByJid = $derived.by(() => {
    const m = new Map<string, Member>();
    for (const p of participants) if (!m.has(p.jid)) m.set(p.jid, p);
    return m;
  });
  const memberByNumber = $derived.by(() => {
    const m = new Map<string, Member>();
    for (const p of participants) if (p.number && !m.has(p.number)) m.set(p.number, p);
    return m;
  });
  /** `@<user>` token to member, by either address form. First entry wins, as `find` did. */
  const memberByUser = $derived.by(() => {
    const m = new Map<string, Member>();
    for (const p of participants) {
      const user = p.jid.split("@")[0];
      if (!m.has(user)) m.set(user, p);
      if (p.number && !m.has(p.number)) m.set(p.number, p);
    }
    return m;
  });
  /** First push name seen per member, replacing the per-mention `messages.find` scan. */
  const spokenByMember = $derived.by(() => {
    const m = new Map<Member, string>();
    for (const msg of messages) {
      if (!msg.sender_name || isPlaceholder(msg.sender_name)) continue;
      const member = memberOf(msg.sender);
      if (member && !m.has(member)) m.set(member, msg.sender_name);
    }
    return m;
  });
  /** First sender name seen per bare JID, replacing the per-render `messages.find` scan. */
  const senderNameByJid = $derived.by(() => {
    const m = new Map<string, string>();
    for (const msg of messages) {
      if (!msg.sender_name) continue;
      const b = bare(msg.sender);
      if (!m.has(b)) m.set(b, msg.sender_name);
    }
    return m;
  });
  /** Mentionable members, longest names first, computed once per member list. */
  const sortedNamedMembers = $derived.by(() =>
    participants
      .filter((p) => p.name.length > 1 && !isPlaceholder(p.name))
      .sort((a, b) => b.name.length - a.name.length)
      .map((p) => ({ token: `@${p.name}`, wire: `@${p.jid.split("@")[0]}` })),
  );
  /** Open mention query, or null while the autocomplete is closed. */
  let mentionQuery = $state<string | null>(null);
  /** Unread mentions in the open chat, oldest first, for jump-to-mention. */
  let accountList: Account[] = $state([]);
  let activeAccount = $state<string | null>(null);
  const activeLabel = $derived(
    accountList.find((a) => a.id === activeAccount)?.label ?? "WhatsApp",
  );
  /** A transient notice, such as a video sent without a preview. */
  let notice = $state<string | null>(null);
  /** A quote whose target is not loaded yet, offered as a load action. */
  let pendingJump: { chat: string; id: string } | null = $state(null);
  let searchQuery = $state("");
  let searchResults: SearchResult[] = $state([]);
  let titleOverride = $state<string | null>(null);
  let mentionQueue: string[] = $state([]);
  /** Message briefly outlined after a jump, so it is easy to spot. */
  let highlightedId = $state<string | null>(null);
  let mentionCursor = $state(0);
  let mentionIndex = $state(0);
  /** Mentions picked from the autocomplete, used to convert the text on send. */
  let chosenMentions: { name: string; jid: string }[] = $state([]);
  let mentionMatches = $derived.by(() => {
    const query = mentionQuery;
    if (query === null) return [];
    const needle = query.toLowerCase();
    // `@all` is a group mention (respects mutes); `@all-override` also lists
    // every member, which notifies them even with the chat muted.
    const all = selectedChat?.endsWith("@g.us")
      ? [
          { jid: "@all", name: "all", username: null, number: null },
          { jid: "@all-override", name: "all-override", username: null, number: null },
        ]
      : [];
    // A member is found by nickname, reserved username or number, and inserted by nickname.
    const members = participants.map((p) => ({
      jid: p.jid,
      name: isPlaceholder(p.name) && p.username ? p.username : displayName(p.name, p.jid),
      username: p.username,
      number: p.number,
    }));
    return [...all, ...members]
      .filter((p) =>
        [p.name, p.username, p.number].some((field) => field?.toLowerCase().includes(needle)),
      )
      .slice(0, 8);
  });
  let replyingTo: StoredMessage | null = $state(null);
  /** Our own message being edited in the composer, if any. */
  let editing: { chat: string; id: string; original: string } | null = $state(null);
  /** Texts we sent this session, newest first, recalled with the history keybind. */
  let sentHistory: string[] = $state([]);
  /** Position while recalling `sentHistory`; -1 means not browsing. */
  let historyIndex = $state(-1);
  /** The draft to restore when arrowing forward past the newest sent message. */
  let historyDraft = "";
  let settings: UiSettings = $state({
    retention: { max_age_hours: 24, max_messages_per_chat: 500 },
    accept_full_history: true,
    auto_download_media: true,
    warn_missing_video_preview: true,
    media_dir: null,
    send_typing: true,
    send_receipts: true,
    keep_history: true,
    skip_loading_screen: false,
  });
  /** The chat UI may be shown and refreshed: the gate opened, or the user opted out of it. */
  const uiUnlocked = $derived(gateDone || settings.skip_loading_screen);
  let showSettings = $state(false);
  let showGroupInfo = $state(false);
  let groupInfo: GroupData | null = $state(null);
  let groupInfoError = $state<string | null>(null);
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

  let error = $state<string | null>(null);

  let scroller: HTMLDivElement | undefined = $state();
  /** True while the user is reading older messages with new ones below. */
  let scrolledUp = $state(false);
  /** Files staged for review before they are sent, shown above the composer. */
  let pending: PendingMedia[] = $state([]);
  let pendingSeq = 0;

  function bareJid(jid: string) {
    return jid.replace(/@.*$/, "");
  }
  function chatLabel(chat: ChatSummary) {
    return displayName(chat.display_name, chat.chat);
  }
  /** Oldest first, the order the conversation is drawn in. */
  const ordered = $derived(messages.slice().reverse());

  const isGroupChat = $derived(!!selectedChat?.endsWith("@g.us"));

  let chatFilter = $state<"all" | "unread" | "groups">("all");
  const visibleChats = $derived(
    chats.filter((c) =>
      chatFilter === "all"
        ? true
        : chatFilter === "unread"
          ? c.unread_count > 0
          : c.chat.endsWith("@g.us"),
    ),
  );
  const unreadChats = $derived(chats.filter((c) => c.unread_count > 0).length);

  /** Member names under a group's title, as far as they are known. */
  const subtitle = $derived(
    selectedChat?.endsWith("@g.us") && participants.length > 0
      ? participants.map((p) => displayName(p.name, p.jid)).join(", ")
      : null,
  );

  function dayKey(ts: number) {
    return new Date(ts * 1000).toDateString();
  }
  function dayLabel(ts: number) {
    const day = new Date(ts * 1000);
    const today = new Date();
    const yesterday = new Date();
    yesterday.setDate(today.getDate() - 1);
    if (day.toDateString() === today.toDateString()) return "Today";
    if (day.toDateString() === yesterday.toDateString()) return "Yesterday";
    return day.toLocaleDateString(undefined, {
      day: "numeric",
      month: "long",
      year: day.getFullYear() === today.getFullYear() ? undefined : "numeric",
    });
  }

  /** A media message's caption. Uncaptioned media is stored as `[kind]`. */
  function captionOf(message: StoredMessage) {
    const text = message.text.trim();
    return text === `[${message.media_kind}]` ? "" : text;
  }
  /** Preview line: our own messages are prefixed "You", group peers by name. */
  /** Who sent a chat's last message, as the list prefixes it. */
  function previewAuthor(chat: ChatSummary) {
    if (chat.last_from_me) return "You";
    if (!chat.chat.endsWith("@g.us")) return null;
    return displayName(chat.last_sender_name, chat.last_sender);
  }
  /** The last message's text; media without a caption reads as its kind. */
  function previewText(chat: ChatSummary) {
    const kind = chat.last_media_kind;
    if (kind === "poll") return `📊 ${chat.last_text}`;
    if (kind === "event") return `📅 ${chat.last_text}`;
    if (kind === "view_once") return "View once message";
    if (!kind || chat.last_text.trim() !== `[${kind}]`) return plain(chat.last_text, mentionName);
    return MEDIA_LABELS[kind] ?? "Attachment";
  }
  function mediaIcon(kind: string | null): IconName | null {
    if (kind === "image" || kind === "sticker") return "image";
    if (kind === "video" || kind === "gif") return "video";
    if (kind === "audio") return "mic";
    if (kind === "document") return "file";
    return null;
  }
  /** Every address form for a member, so lookups do not scan the roster per render. */
  const memberByAddress = $derived.by(() => {
    const m = new Map<string, Member>();
    for (const p of participants) {
      const b = bare(p.jid);
      m.set(b, p);
      m.set(p.jid, p);
      m.set(b.split("@")[0], p);
      if (p.number) m.set(p.number, p);
    }
    return m;
  });
  /** First real push name per sender address, collected once instead of a scan per render. */
  const senderNames = $derived.by(() => {
    const m = new Map<string, string>();
    for (const msg of messages) {
      if (msg.from_me || !msg.sender_name || isPlaceholder(msg.sender_name)) continue;
      const b = bare(msg.sender);
      if (!m.has(b)) m.set(b, msg.sender_name);
    }
    return m;
  });
  /** Push names keyed by member, so a LID sender still names its member. */
  const memberNames = $derived.by(() => {
    const m = new Map<string, string>();
    for (const [sender, name] of senderNames) {
      const member = memberByAddress.get(sender);
      if (member && !m.has(member.jid)) m.set(member.jid, name);
    }
    return m;
  });
  /** The group member a sender is, matched by LID or by phone number. */
  function memberOf(jid: string) {
    return memberByAddress.get(bare(jid));
  }
  function senderLabel(message: StoredMessage) {
    // The message row joins names on one address form only; the member list
    // resolves both, so it rescues senders whose name is keyed by the other.
    const own = message.sender_name;
    const known = own && !isPlaceholder(own) ? own : memberOf(message.sender)?.name;
    return displayName(known && !isPlaceholder(known) ? known : own, message.sender);
  }
  /** Resolves a JID to a known name, falling back to the bare address. */
  function senderName(jid: string) {
    const b = bare(jid);
    const known = senderNames.get(b);
    const member = memberOf(jid)?.name;
    return displayName(known && !isPlaceholder(known) ? known : (member ?? known), jid);
  }
  /** Author shown on a quote; our own messages read "You". */
  function quoteAuthor(jid: string | null) {
    if (!jid) return "Message";
    return jid === "@me" ? "You" : senderName(jid);
  }

  function formatTime(seconds: number) {
    return new Date(seconds * 1000).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  /** Chat list only: cheap, local, never blocks on the network. */
  let chatsSeq = 0;
  async function refreshChats() {
    const seq = ++chatsSeq;
    try {
      const next = await invoke<ChatSummary[]>("chats");
      // A slow response must not overwrite a newer list.
      if (seq !== chatsSeq) return;
      chats = next;
    } catch (e) {
      error = String(e);
    }
  }

  /** Coalesces an event burst into at most one chat-list reload per 200 ms. */
  let chatsQueued = false;
  function queueRefreshChats() {
    if (chatsQueued) return;
    chatsQueued = true;
    setTimeout(() => {
      chatsQueued = false;
      void refreshChats();
    }, 200);
  }

  /** The same for the open chat; `markRead` marks what arrived as seen if the window has focus. */
  let messagesQueued: { chat: string; follow: boolean; markRead: boolean } | null = null;
  function queueReloadMessages(follow: boolean, markRead: boolean) {
    if (!selectedChat) return;
    if (messagesQueued?.chat === selectedChat) {
      messagesQueued.follow ||= follow;
      messagesQueued.markRead ||= markRead;
      return;
    }
    const queued = (messagesQueued = { chat: selectedChat, follow, markRead });
    setTimeout(async () => {
      if (messagesQueued === queued) messagesQueued = null;
      if (selectedChat !== queued.chat) return;
      await reloadMessages();
      if (queued.follow) scrollToBottom();
      if (queued.markRead && document.hasFocus()) {
        await invoke("mark_read", { chat: queued.chat }).catch(() => {});
        queueRefreshChats();
      }
    }, 100);
  }

  /** When each unnamed group's subject was last asked for; the core backs off failed ones. */
  const askedSubjects = new Map<string, number>();

  /**
   * Group subjects need a network query. Runs after the list is already
   * rendered so a slow query cannot delay showing new messages.
   */
  async function resolveNames() {
    try {
      const resolved = await invoke<number>("resolve_names");
      if (resolved > 0) await refreshChats();
    } catch {
      // Names are cosmetic.
    }
  }

  async function openChat(chat: string, jumpToMention = false, label: string | null = null) {
    if (selectedChat !== chat) {
      stopTyping();
      switching = true;
      chatGroup = null;
      // A staged reply or edit belongs to the chat it was started in.
      replyingTo = null;
      editing = null;
      resetHistory();
    }
    selectedChat = chat;
    // One-to-one typing only arrives for contacts we are subscribed to.
    if (!chat.endsWith("@g.us")) invoke("watch_presence", { jid: chat }).catch(() => {});
    titleOverride = label;
    scrolledUp = false;
    autoplayId = null;
    recall = null;
    olderExhausted = false;
    loadOnScroll = true;
    chatPrivacy = { send_typing: null, send_receipts: null };
    invoke<{ retention: ChatRetention } & ChatPrivacy>("chat_settings", { chat })
      .then((s) => {
        if (selectedChat !== chat) return;
        loadOnScroll = s.retention.on_demand;
        chatPrivacy = { send_typing: s.send_typing, send_receipts: s.send_receipts };
      })
      .catch(() => {});
    participants = memberCache[chat] ?? [];
    chosenMentions = [];
    mentionQuery = null;
    draft = drafts[chat] ?? "";
    showGroupInfo = false;
    groupInfo = null;
    try {
      messageLimit = PAGE;
      // Invalidate any in-flight reload from the previous chat.
      const seq = ++messagesSeq;
      // Mentions are captured before the chat is marked read, since that clears them.
      const [mentions, loaded] = await Promise.all([
        invoke<string[]>("unread_mentions", { chat }).catch(() => [] as string[]),
        invoke<StoredMessage[]>("messages", { chat, limit: messageLimit }),
      ]);
      // A quicker click on another chat has already taken over.
      if (selectedChat !== chat || seq !== messagesSeq) return;
      mentionQueue = mentions;
      mentionCursor = 0;
      messages = loaded;
      await loadMarks();
      scrollToBottom();
      await tick();
      switching = false;
      // Opening a conversation is what marks it seen.
      await invoke("mark_read", { chat });
      await refreshChats();
    } catch (e) {
      switching = false;
      error = String(e);
    }
    // Group members power the @ autocomplete, and the group's settings decide
    // whether we may write; a one-to-one chat has neither.
    await loadChatGroup(chat);
    // Opening a chat is the obvious moment to start typing.
    await tick();
    if (jumpToMention && mentionQueue.length > 0) {
      mentionCursor = 1;
      scrollToMessage(mentionQueue[0]);
    }
    composerInput?.focus();
  }


  /** The open group's info: members, and whether we may send (announcement mode, communities). */
  let chatGroup = $state<GroupData | null>(null);
  /** What kind of group the open chat is, shown before its members in the header. */
  const groupContext = $derived.by(() => {
    if (!chatGroup) return null;
    if (chatGroup.community) return "Community";
    const parent = chatGroup.parent_name ?? (chatGroup.parent ? "a community" : null);
    if (chatGroup.announcements) return parent ? `Announcements · ${parent}` : "Announcements";
    return parent ? `In ${parent}` : null;
  });
  /** Communities and their subgroups among our groups, for the chat list. */
  let groupKinds = $state<Record<string, { community: boolean; announcements: boolean; parent: string | null }>>({});

  async function loadChatGroup(chat: string) {
    if (!chat.endsWith("@g.us")) {
      chatGroup = null;
      participants = [];
      return;
    }
    try {
      const info = await invoke<GroupData>("group_info", { chat });
      memberCache[chat] = info.participants;
      if (selectedChat !== chat) return;
      chatGroup = info;
      participants = info.participants;
      // Loading members stores their group display names, so numbers looked up
      // before now may have a name; ask again.
      forgetUnresolvedNames();
    } catch {
      if (selectedChat !== chat) return;
      chatGroup = null;
      participants = memberCache[chat] ?? [];
    }
  }

  async function loadGroupKinds() {
    try {
      groupKinds = await invoke("group_kinds");
    } catch {
      // Not connected yet; the next connection loads them.
    }
  }
  $effect(() => {
    if (connected) void loadGroupKinds();
  });

  async function openUrl(url: string) {
    try {
      await invoke("open_url", { url });
    } catch (e) {
      error = String(e);
    }
  }

  /** Deletes downloaded media, keeping the messages. */
  async function flushMedia() {
    try {
      const removed = await invoke<number>("flush_media");
      notice =
        removed > 0
          ? `Removed media from ${removed} message(s).`
          : "There was no downloaded media to remove.";
      await refreshChats();
      if (selectedChat) await reloadMessages();
    } catch (e) {
      error = String(e);
    }
  }

  async function clearHistory() {
    try {
      const removed = await invoke<number>("clear_history");
      notice = `Deleted ${removed} message(s) from this computer.`;
      await refreshChats();
      if (selectedChat) await reloadMessages();
    } catch (e) {
      error = String(e);
    }
  }

  /** Stops showing the video-without-preview warning. */
  async function muteNotice() {
    settings.warn_missing_video_preview = false;
    try {
      await invoke("set_settings", { settings });
    } catch (e) {
      error = String(e);
    }
    notice = null;
  }

  async function loadAccounts() {
    const view = await invoke<{ accounts: Account[]; active: string | null }>("accounts");
    accountList = view.accounts;
    activeAccount = view.active;
  }

  /** Clears everything tied to the current account before switching. */
  function resetUi() {
    chats = [];
    avatars = {};
    requestedAvatars.clear();
    me = null;
    accountMenu = false;
    messages = [];
    selectedChat = null;
    draft = "";
    drafts = {};
    pending = [];
    replyingTo = null;
    editing = null;
    sentHistory = [];
    resetHistory();
    participants = [];
    chosenMentions = [];
    mentionQueue = [];
    groupInfo = null;
    showGroupInfo = false;
    // A switch starts a fresh catch-up, so the loading gate applies again.
    clearTimeout(gateTimer);
    gateDone = false;
    finalizing = false;
    syncTimedOut = false;
    syncPending = 0;
    syncApplied = 0;
  }

  /** Starts the account picked on the launch chooser. */
  async function chooseAccount(id: string) {
    choosingAccount = false;
    if (id === activeAccount) await connect();
    else await switchTo(id);
  }

  async function switchTo(id: string) {
    if (id === activeAccount) return;
    try {
      resetUi();
      connected = false;
      await showQr(null);
      await invoke("switch_account", { id });
      await loadAccounts();
      await syncState();
    } catch (e) {
      error = String(e);
    }
  }

  async function renameAccount(id: string, label: string) {
    try {
      await invoke("rename_account", { id, label });
      await loadAccounts();
    } catch (e) {
      error = String(e);
    }
  }

  async function removeAccount(id: string) {
    try {
      if (id === activeAccount) {
        resetUi();
        connected = false;
        await showQr(null);
      }
      await invoke("remove_account", { id });
      await loadAccounts();
      await syncState();
    } catch (e) {
      error = String(e);
    }
  }

  async function addAccount() {
    try {
      resetUi();
      connected = false;
      await showQr(null);
      await invoke("add_account", {});
      await loadAccounts();
      await syncState();
    } catch (e) {
      error = String(e);
    }
  }

  let chatSettingsOpen = $state(false);
  /** Whether reaching the top of the open chat asks the phone for more. */
  let loadOnScroll = $state(true);
  /** Set once the phone had nothing older, so scrolling stops asking. */
  let olderExhausted = false;
  /**
   * A recall walks back about a day, 50 messages per request, since the phone
   * answers by count and not by time.
   */
  let recall: { chat: string; until: number; rounds: number; auto: boolean } | null = null;

  /** Asks the phone for about a day of older messages in the open chat. */
  /** Resolved whenever a recall ends, however it ends. */
  let recallWaiters: (() => void)[] = [];
  function settleRecall() {
    const waiters = recallWaiters;
    recallWaiters = [];
    for (const done of waiters) done();
  }
  /** Asks the phone for the chat's previous day and waits until it has landed or given up. */
  function recallDay(): Promise<void> {
    if (!selectedChat || olderExhausted) return Promise.resolve();
    const done = new Promise<void>((resolve) => recallWaiters.push(resolve));
    if (!loadingOlder) void loadOlder();
    return done;
  }

  async function loadOlder(auto = false) {
    if (!selectedChat || loadingOlder) return;
    const oldest = messages.at(-1)?.timestamp ?? Math.floor(Date.now() / 1000);
    recall = { chat: selectedChat, until: oldest - 86_400, rounds: 0, auto };
    await requestOlder();
  }

  async function requestOlder() {
    if (!selectedChat || !recall) return;
    loadingOlder = true;
    const auto = recall.auto;
    // The phone answers asynchronously, or not at all when it has nothing
    // older or is offline, so the spinner gives up on its own.
    clearTimeout(olderTimer);
    olderTimer = setTimeout(() => {
      loadingOlder = false;
      historyActive = false;
      recall = null;
      olderExhausted = true;
      if (!auto) error = "Your phone did not answer. It has to be online for older messages to load.";
      settleRecall();
    }, 15000);
    try {
      historyActive = true;
      await invoke("load_older", { chat: selectedChat, count: 50 });
    } catch (e) {
      loadingOlder = false;
      historyActive = false;
      recall = null;
      error = String(e);
      settleRecall();
    }
  }

  /** After a batch lands: keep walking back until the day is covered. */
  function continueRecall(added: number) {
    const oldest = messages.at(-1)?.timestamp;
    if (!recall || recall.chat !== selectedChat) {
      settleRecall();
      return;
    }
    if (added === 0) olderExhausted = true;
    if (added > 0 && oldest && oldest > recall.until && ++recall.rounds < 10) void requestOlder();
    else {
      recall = null;
      settleRecall();
    }
  }

  /** Runs the chat/contact/group search. */
  async function runSearch() {
    const query = searchQuery.trim();
    if (!query) {
      searchResults = [];
      return;
    }
    try {
      searchResults = await invoke<SearchResult[]>("search", { query });
    } catch (e) {
      error = String(e);
    }
  }

  /** Opens a search result, even one with no local history. */
  function openFromSearch(result: SearchResult) {
    searchQuery = "";
    searchResults = [];
    openChat(result.jid, false, result.name);
  }

  /** Label for a media message with no caption, used in reply previews. */
  function replyPreviewText(message: StoredMessage) {
    // Media stores a "[image]" style placeholder when it has no caption.
    if (message.text && !message.text.startsWith("[")) return plain(message.text, mentionName);
    switch (message.media_kind) {
      case "image":
        return "Photo";
      case "video":
        return "Video";
      case "audio":
        return "Voice message";
      case "document":
        return "Document";
      default:
        return "";
    }
  }

  /** Scrolls a message into view by its id, and highlights it briefly. */
  function scrollToMessage(id: string) {
    const element = scroller?.querySelector(`[data-id="${id}"]`);
    if (!element) return;
    element.scrollIntoView({ block: "center" });
    highlightedId = id;
    window.setTimeout(() => {
      if (highlightedId === id) highlightedId = null;
    }, 1600);
  }

  /** Jumps to the next unread mention, oldest to newest, wrapping around. */
  function jumpNextMention() {
    if (mentionQueue.length === 0) return;
    const id = mentionQueue[mentionCursor % mentionQueue.length];
    mentionCursor = (mentionCursor + 1) % mentionQueue.length;
    scrollToMessage(id);
  }

  /** Pins or unpins a chat, mirrored to the account. */
  async function togglePin(chat: ChatSummary, event?: MouseEvent) {
    event?.stopPropagation();
    try {
      await invoke("set_pinned", { chat: chat.chat, pinned: !chat.pinned });
      await refreshChats();
    } catch (e) {
      error = String(e);
    }
  }

  /** Opens the right sidebar with the group's subject, description and members. */
  async function openGroupInfo() {
    if (!selectedChat) return;
    showGroupInfo = true;
    groupInfo = null;
    groupInfoError = null;
    try {
      groupInfo = await invoke<GroupData>("group_info", { chat: selectedChat });
    } catch (e) {
      // The query can time out on a busy server; keep the panel open so the
      // failure is visible and retryable rather than looking like a dead click.
      groupInfoError = String(e);
    }
  }

  /**
   * Reloads the open conversation without touching the unread state.
   *
   * `keepPlace` holds the view on the same message when older ones are added
   * above it, instead of letting them push it down.
   */
  async function reloadMessages(keepPlace = false) {
    if (!selectedChat) return;
    const chat = selectedChat;
    const seq = ++messagesSeq;
    const fromBottom = scroller ? scroller.scrollHeight - scroller.scrollTop : 0;
    let loaded: StoredMessage[];
    try {
      loaded = await invoke<StoredMessage[]>("messages", {
        chat,
        limit: messageLimit,
      });
    } catch (e) {
      error = String(e);
      return;
    }
    // A slow response must not overwrite a newer conversation.
    if (seq !== messagesSeq || selectedChat !== chat) return;
    messages = loaded;
    if (keepPlace && scroller) {
      await tick();
      scroller.scrollTop = scroller.scrollHeight - fromBottom;
    }
  }

  /** Fetches a profile picture once, a few requests at a time. */
  function loadAvatar(jid: string) {
    if (requestedAvatars.has(jid)) return;
    requestedAvatars.add(jid);
    avatarQueue.push(jid);
    while (avatarWorkers < 4 && avatarQueue.length > 0) void avatarWorker();
  }
  async function avatarWorker() {
    avatarWorkers++;
    try {
      for (let jid = avatarQueue.shift(); jid; jid = avatarQueue.shift()) {
        try {
          avatars[jid] = await invoke<string | null>("avatar", { jid });
        } catch {
          avatars[jid] = null;
        }
      }
    } finally {
      avatarWorkers--;
    }
  }

  /** A JID's cached picture, fetching it on first use. */
  function pictureOf(jid: string) {
    loadAvatar(jid);
    return avatars[jid] ?? null;
  }

  $effect(() => {
    if (!connected) return;
    for (const chat of chats) loadAvatar(chat.chat);
  });

  /**
   * Names the core found under either address form (a LID sender, a phone
   * quote author), fetched in batches for JIDs rendered without one.
   */
  let learnedNames: Record<string, string> = $state({});
  const requestedNames = new Set<string>();
  let queuedNames: string[] = [];
  let nameTimer: ReturnType<typeof setTimeout> | undefined;
  function requestName(jid: string) {
    if (!connected || requestedNames.has(jid) || !jid.includes("@")) return;
    requestedNames.add(jid);
    queuedNames.push(jid);
    clearTimeout(nameTimer);
    nameTimer = setTimeout(async () => {
      const jids = queuedNames;
      queuedNames = [];
      try {
        Object.assign(learnedNames, await invoke<Record<string, string>>("names", { jids }));
      } catch {
        for (const jid of jids) requestedNames.delete(jid);
      }
    }, 30);
  }
  /** Asks again for every JID the core had no name for, once it may have learned some. */
  function forgetUnresolvedNames() {
    const kept: Record<string, string> = {};
    for (const [jid, name] of Object.entries(learnedNames)) {
      if (isPlaceholder(name)) requestedNames.delete(jid);
      else kept[jid] = name;
    }
    for (const jid of requestedNames) if (!(jid in kept)) requestedNames.delete(jid);
    learnedNames = kept;
  }
  /** A name for a JID, asking the core when the given one is missing or a bare number. */
  function displayName(name: string | null | undefined, jid: string) {
    if (!name || isPlaceholder(name)) {
      const key = bare(jid);
      const learned = learnedNames[key];
      if (learned) return phoneName(learned, key);
      requestName(key);
    }
    return phoneName(name, jid);
  }

  /** Who an `@<user>` token names: us, a group member, or whichever address form the core knows. */
  function mentionTarget(user: string): { jid: string; name: string; self: boolean } {
    const own = me ? bare(me) : null;
    const member = participants.find((p) => p.jid.split("@")[0] === user || p.number === user);
    if (own && (own.split("@")[0] === user || member?.number === own.split("@")[0])) {
      // Our own contact card may be saved under a nickname; show our push name.
      return { jid: own, name: displayName(null, own), self: true };
    }
    if (member) {
      // A push name seen on any of their messages here beats the member list's bare number.
      const spoken = messages.find(
        (m) => m.sender_name && !isPlaceholder(m.sender_name) && memberOf(m.sender) === member,
      )?.sender_name;
      const named = spoken ?? (isPlaceholder(member.name) ? null : member.name);
      return { jid: member.jid, name: displayName(named, member.jid), self: false };
    }
    // Cached names only: one message can mention hundreds of numbers, and a
    // lookup for each would flood the core and lag the whole app.
    const lid = `${user}@lid`;
    const pn = `${user}@s.whatsapp.net`;
    const lidName = learnedNames[lid];
    const lidKnown = !!lidName && !/^\d+$/.test(lidName);
    return lidKnown
      ? { jid: lid, name: phoneName(lidName, lid), self: false }
      : { jid: pn, name: phoneName(null, pn), self: false };
  }
  /** Members whose name is worth rewriting, longest first, rebuilt only on roster change. */
  const namedMembers = $derived(
    participants
      .filter((p) => p.name.length > 1 && !isPlaceholder(p.name))
      .sort((a, b) => b.name.length - a.name.length),
  );
  /** Already rewritten texts, so a re-render does not scan every member name again. */
  const wireMentionCache = new Map<string, string>();
  $effect(() => {
    void namedMembers;
    wireMentionCache.clear();
  });
  /**
   * `@Name` typed for a member, as older captions were sent, rewritten to the
   * wire's `@<number>` so it draws as a mention tag too. Longest names first,
   * so "Ana María" wins over "Ana".
   */
  function asWireMentions(text: string) {
    if (!text.includes("@") || participants.length === 0) return text;
    const named = participants
      .filter((p) => p.name.length > 1 && !isPlaceholder(p.name))
      .sort((a, b) => b.name.length - a.name.length);
    let out = text;
    for (const p of named) {
      const token = `@${p.name}`;
      if (out.includes(token)) out = out.split(token).join(`@${p.jid.split("@")[0]}`);
    }
    wireMentionCache.set(text, out);
    return out;
  }
  /** The profile card open beside a mention, name or picture. */
  let profileCard = $state<{ jid: string; name: string; x: number; y: number; self: boolean } | null>(null);
  function openProfile(jid: string, name: string, event: MouseEvent, self = false) {
    event.stopPropagation();
    // Pills and names render from cache only; the click is what fetches.
    loadAvatar(bare(jid));
    profileCard = { jid: bare(jid), name, x: event.clientX, y: event.clientY, self };
  }

  function mentionName(user: string) {
    return mentionTarget(user).name;
  }

  $effect(() => {
    if (!connected) return;
    for (const account of accountList) if (account.jid) loadAvatar(account.jid);
  });

  // Group members get their picture next to their messages.
  $effect(() => {
    if (!connected || !selectedChat?.endsWith("@g.us")) return;
    // Recent senders only: a group with hundreds of members must not fire a
    // picture request for every sender the moment the chat opens.
    for (const message of messages.slice(-80)) if (!message.from_me) loadAvatar(bare(message.sender));
  });

  /** Each account's own picture as last seen, so it shows before that account connects. */
  function rememberedAvatar(id: string): string | null {
    try {
      return localStorage.getItem(`hermodr.avatar.${id}`);
    } catch {
      return null;
    }
  }
  const accountAvatars = $derived(
    Object.fromEntries(
      accountList.map((a) => {
        const jid = a.id === activeAccount ? (me ?? a.jid) : a.jid;
        return [a.id, (jid ? avatars[jid] : null) ?? rememberedAvatar(a.id)];
      }),
    ) as Record<string, string | null>,
  );
  $effect(() => {
    const own = activeAccount && me ? avatars[me] : null;
    if (!own) return;
    try {
      localStorage.setItem(`hermodr.avatar.${activeAccount}`, own);
    } catch {
      // Storage may be unavailable; the picture then only shows once connected.
    }
  });

  /** Who is typing in each chat, until they pause or ten seconds pass. */
  let typing: Record<string, { sender: string; state: string }[]> = $state({});
  const typingTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function setTyping(chat: string, sender: string, state: string) {
    const key = `${chat} ${sender}`;
    clearTimeout(typingTimers.get(key));
    const others = (typing[chat] ?? []).filter((t) => t.sender !== sender);
    typing[chat] = state === "paused" ? others : [...others, { sender, state }];
    if (state !== "paused") {
      typingTimers.set(key, setTimeout(() => setTyping(chat, sender, "paused"), 10000));
    }
  }

  /** Online state of contacts we watch, as far as their privacy lets us see it. */
  let presence: Record<string, { online: boolean; last_seen: number | null }> = $state({});
  function presenceLabel(chat: string) {
    const seen = presence[chat];
    if (!seen) return null;
    if (seen.online) return "online";
    if (!seen.last_seen) return null;
    const date = new Date(seen.last_seen * 1000);
    const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    const days = Math.floor((Date.now() - date.getTime()) / 86_400_000);
    const day =
      new Date().toDateString() === date.toDateString()
        ? "today"
        : days < 2
          ? "yesterday"
          : date.toLocaleDateString([], { day: "numeric", month: "short" });
    return `last seen ${day} at ${time}`;
  }

  function typingLabel(chat: string) {
    const who = typing[chat];
    if (!who?.length) return null;
    const verb = who.some((t) => t.state === "recording") ? "recording audio" : "typing";
    if (!chat.endsWith("@g.us")) return `${verb}…`;
    if (who.length > 1) return `${who.length} people are ${verb}…`;
    const person = memberOf(who[0].sender);
    const name = person && !isPlaceholder(person.name) ? person.name : null;
    return `${name ?? senderName(who[0].sender)} is ${verb}…`;
  }

  // Our own typing: announced at most every five seconds, withdrawn after four
  // idle ones or when the message goes out.
  let typingSentAt = 0;
  let typingIdle: ReturnType<typeof setTimeout> | undefined;

  /** The open chat's overrides of the typing and read receipt settings; `null` follows them. */
  let chatPrivacy: ChatPrivacy = $state({ send_typing: null, send_receipts: null });
  const chatSendsTyping = $derived(chatPrivacy.send_typing ?? settings.send_typing);
  const chatSendsReceipts = $derived(chatPrivacy.send_receipts ?? settings.send_receipts);
  const typingHidden = $derived(!chatSendsTyping);
  const receiptsHidden = $derived(!chatSendsReceipts);

  /** Sets one of the two per-chat privacy overrides, dropping it when it matches the default. */
  async function setChatPrivacy(patch: Partial<ChatPrivacy>) {
    const chat = selectedChat;
    if (!chat) return;
    const next: ChatPrivacy = {
      send_typing: patch.send_typing ?? chatPrivacy.send_typing,
      send_receipts: patch.send_receipts ?? chatPrivacy.send_receipts,
    };
    if (next.send_typing === settings.send_typing) next.send_typing = null;
    if (next.send_receipts === settings.send_receipts) next.send_receipts = null;
    try {
      await invoke("set_chat_privacy", {
        chat,
        typing: next.send_typing,
        receipts: next.send_receipts,
      });
      if (selectedChat === chat) chatPrivacy = next;
      if (!(next.send_typing ?? settings.send_typing)) stopTyping(chat);
    } catch (e) {
      error = String(e);
    }
  }

  function toggleChatTyping() {
    void setChatPrivacy({ send_typing: typingHidden });
  }

  function toggleChatReceipts() {
    void setChatPrivacy({ send_receipts: receiptsHidden });
  }

  function reportTyping() {
    const chat = selectedChat;
    if (!chat || !chatSendsTyping) return;
    if (Date.now() - typingSentAt > 5000) {
      typingSentAt = Date.now();
      invoke("send_typing", { chat, typing: true }).catch(() => {});
    }
    clearTimeout(typingIdle);
    typingIdle = setTimeout(() => stopTyping(chat), 4000);
  }

  function stopTyping(chat = selectedChat) {
    clearTimeout(typingIdle);
    if (!chat || !typingSentAt) return;
    typingSentAt = 0;
    invoke("send_typing", { chat, typing: false }).catch(() => {});
  }

  /** Online while the window has focus, as WhatsApp Web does; typing only arrives then. */
  function setOnline(online: boolean) {
    if (connected) invoke("set_online", { online }).catch(() => {});
  }

  $effect(() => {
    if (connected) setOnline(document.hasFocus());
  });

  /** WhatsApp privacy categories to values, for who can see us online. */
  let privacy = $state<Record<string, string>>({});

  $effect(() => {
    if (!connected) return;
    invoke<{ privacy: Record<string, string> }>("profile")
      .then((p) => (privacy = p.privacy))
      .catch(() => {});
  });

  /**
   * Who sees us as online. "Same as last seen" defers to last seen, so with
   * last seen hidden we are effectively invisible, as Discord shows it.
   */
  const visibility = $derived.by(() => {
    if (!connected) return "offline";
    const audience = privacy.online === "all" ? "all" : (privacy.last ?? "all");
    return audience === "none" ? "invisible" : audience === "all" ? "online" : "contacts";
  });

  $effect(() => {
    if (!connected || me) return;
    invoke<string | null>("own_jid")
      .then((jid) => {
        me = jid;
        if (jid) loadAvatar(jid);
        // The backend just recorded the JID, and named the account if it was
        // still on the default label; pick both up.
        return loadAccounts();
      })
      .catch(() => {});
  });

  function scrollToBottom() {
    // Wait for the new messages to render before measuring.
    requestAnimationFrame(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight;
      scrolledUp = false;
    });
  }

  // The typing bubble coming and going moves the bottom; stay pinned to it.
  $effect(() => {
    if (!selectedChat) return;
    void typing[selectedChat]?.length;
    if (!untrack(() => scrolledUp)) scrollToBottom();
  });

  function onScroll() {
    if (!scroller) return;
    const distance = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    scrolledUp = distance > 120;
    if (scroller.scrollTop < 80 && loadOnScroll && !olderExhausted && !loadingOlder && messages.length > 0) {
      void loadOlder(true);
    }
  }

  /** The `@…` token immediately before the caret, if the user is typing one. */
  function currentMentionQuery(): { query: string; start: number } | null {
    const input = composerInput;
    if (!input) return null;
    const caret = input.selectionStart ?? draft.length;
    const before = draft.slice(0, caret);
    const at = before.lastIndexOf("@");
    if (at === -1) return null;
    if (at > 0 && !/\s/.test(before[at - 1])) return null;
    const query = before.slice(at + 1);
    if (/\s/.test(query)) return null;
    return { query, start: at };
  }

  let pickerTab = $state<PickerTab | null>(null);

  // `:name` completion, as Discord does it.
  let emojiTable = $state<Emoji[]>([]);
  let emojiToken = $state<{ query: string; start: number } | null>(null);
  let emojiIndex = $state(0);
  const emojiMatches = $derived(
    emojiToken ? searchEmojis(emojiTable, emojiToken.query, 12) : [],
  );

  /** A `:word` right before the caret, at least two letters long. */
  function currentEmojiQuery() {
    const caret = composerInput?.selectionStart ?? draft.length;
    const match = /(?:^|\s)(:([a-z0-9_+-]{2,}))$/i.exec(draft.slice(0, caret));
    return match ? { query: match[2], start: caret - match[1].length } : null;
  }

  /** Puts text at the caret, or in place of the characters from `start` to it. */
  async function insertAtCaret(text: string, start?: number) {
    const input = composerInput;
    const caret = input?.selectionStart ?? draft.length;
    const from = start ?? caret;
    draft = draft.slice(0, from) + text + draft.slice(caret);
    if (selectedChat) drafts[selectedChat] = draft;
    await tick();
    const position = from + text.length;
    input?.focus();
    input?.setSelectionRange(position, position);
  }

  function selectEmoji(emoji: string) {
    const token = emojiToken;
    emojiToken = null;
    rememberEmoji(emoji);
    void insertAtCaret(emoji, token?.start);
  }

  function onComposerInput(event: Event) {
    draft = (event.currentTarget as HTMLTextAreaElement).value;
    if (selectedChat) drafts[selectedChat] = draft;
    // Typing ends a history recall, so the next Up starts from the newest again.
    historyIndex = -1;
    if (draft.trim()) reportTyping();
    else stopTyping();
    // A complete `:shortcode:` turns into its emoji the moment it is closed.
    const caret = composerInput?.selectionStart ?? draft.length;
    const closed = /(?:^|\s)(:([a-z0-9_+-]+):)$/i.exec(draft.slice(0, caret));
    const exact = closed && emojiTable.find((e) => e.shortcodes.includes(closed[2].toLowerCase()));
    if (closed && exact) {
      emojiToken = null;
      rememberEmoji(exact.emoji);
      void insertAtCaret(exact.emoji, caret - closed[1].length);
      return;
    }
    emojiToken = currentEmojiQuery();
    emojiIndex = 0;
    if ((emojiToken || draft.includes(":")) && emojiTable.length === 0) {
      loadEmojis().then((list) => (emojiTable = list));
    }
    const token = currentMentionQuery();
    if (token && participants.length > 0) {
      mentionQuery = token.query;
      mentionIndex = 0;
    } else {
      mentionQuery = null;
    }
  }

  async function selectMention(person: { jid: string; name: string }) {
    const input = composerInput;
    const token = currentMentionQuery();
    if (!input || !token) return;
    const caret = input.selectionStart ?? draft.length;
    draft = draft.slice(0, token.start) + `@${person.name} ` + draft.slice(caret);
    mentionQuery = null;
    if (!chosenMentions.some((m) => m.jid === person.jid)) {
      chosenMentions = [...chosenMentions, { name: person.name, jid: person.jid }];
    }
    await tick();
    const position = token.start + person.name.length + 2;
    input.focus();
    input.setSelectionRange(position, position);
  }

  /** Loads our last editable text message into the composer. */
  function startEditing() {
    if (!selectedChat) return;
    const candidate = [...messages]
      .reverse()
      .find((m) => m.from_me && !m.media_kind && m.text.trim() && !m.revoked);
    if (!candidate) return;
    editing = { chat: candidate.chat, id: candidate.id, original: candidate.text };
    replyingTo = null;
    draft = candidate.text;
    resetHistory();
    composerInput?.focus();
  }

  function cancelEditing() {
    if (!editing) return;
    editing = null;
    draft = "";
    composerInput?.focus();
  }

  function resetHistory() {
    historyIndex = -1;
    historyDraft = "";
  }

  /** Recalls older sent messages; returns false to leave the caret alone. */
  function recallPrev(): boolean {
    if (sentHistory.length === 0) return false;
    if (historyIndex === -1) {
      if (draft !== "" || editing) return false;
      historyDraft = draft;
    }
    historyIndex = Math.min(historyIndex + 1, sentHistory.length - 1);
    draft = sentHistory[historyIndex];
    return true;
  }

  function recallNext(): boolean {
    if (historyIndex === -1) return false;
    historyIndex -= 1;
    draft = historyIndex === -1 ? historyDraft : sentHistory[historyIndex];
    return true;
  }

  function onComposerKey(event: KeyboardEvent) {
    if (emojiToken && emojiMatches.length > 0) {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        emojiIndex = (emojiIndex + step + emojiMatches.length) % emojiMatches.length;
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        selectEmoji(emojiMatches[emojiIndex].emoji);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        emojiToken = null;
        return;
      }
    }
    if (mentionQuery !== null && mentionMatches.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        mentionIndex = (mentionIndex + 1) % mentionMatches.length;
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        mentionIndex = (mentionIndex - 1 + mentionMatches.length) % mentionMatches.length;
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        void selectMention(mentionMatches[mentionIndex]);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        mentionQuery = null;
        return;
      }
    }
    // Configurable composer shortcuts, only once the popups above are out of the way.
    if (matches(event, keybinds.cancelReply)) {
      if (editing || replyingTo) {
        event.preventDefault();
        if (editing) cancelEditing();
        else replyingTo = null;
      }
      return;
    }
    if (matches(event, keybinds.editLast)) {
      event.preventDefault();
      startEditing();
      return;
    }
    if (matches(event, keybinds.historyPrev) && recallPrev()) {
      event.preventDefault();
      return;
    }
    if (matches(event, keybinds.historyNext) && recallNext()) {
      event.preventDefault();
      return;
    }
    // Enter sends; Shift+Enter keeps the newline the textarea just added.
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  /** Turns the display text into wire text, naming mentions by number. */
  function mentionPayload() {
    let text = draft.trim();
    const jids: string[] = [];
    for (const mention of chosenMentions) {
      const token = `@${mention.name}`;
      if (!text.includes(token)) continue;
      if (mention.jid === "@all") {
        jids.push("@all");
        continue;
      }
      if (mention.jid === "@all-override") {
        // Everyone is also listed explicitly, which bypasses their mute.
        text = text.replace("@all-override", "@all");
        jids.push("@all");
        for (const person of participants) jids.push(person.jid);
        continue;
      }
      const user = mention.jid.split("@")[0].split(":")[0];
      text = text.replace(token, `@${user}`);
      jids.push(mention.jid);
    }
    return { text, jids };
  }

  /**
   * Every outgoing message goes through here, one at a time, so they reach the
   * chat in the order they were sent even when an earlier one is slow.
   */
  let outbox: Promise<unknown> = Promise.resolve();
  function enqueue<T>(task: () => Promise<T>): Promise<T> {
    const run = outbox.then(task, task);
    outbox = run.catch(() => {});
    return run;
  }

  async function send() {
    if (!selectedChat) return;
    // Editing replaces an existing message rather than sending a new one.
    if (editing) {
      const current = editing;
      const text = draft.trim();
      if (!text) return;
      draft = "";
      delete drafts[selectedChat];
      editing = null;
      stopTyping();
      resetHistory();
      composerInput?.focus();
      try {
        await enqueue(() => invoke("edit_message", { chat: current.chat, id: current.id, text }));
        await reloadMessages();
        await refreshChats();
      } catch (e) {
        error = String(e);
      }
      return;
    }
    // With attachments staged, the typed text goes out as their caption.
    if (pending.length > 0) {
      // Mentions in a caption go out as `@<number>` with their JIDs, as in text.
      const { text: caption, jids } = mentionPayload();
      draft = "";
      delete drafts[selectedChat];
      chosenMentions = [];
      mentionQuery = null;
      stopTyping();
      await sendPending(caption, jids.filter((j) => j !== "@all"));
      return;
    }
    if (!draft.trim()) return;
    const chat = selectedChat;
    stopTyping();
    const typed = draft;
    const { text, jids } = mentionPayload();
    const reply = replyingTo;
    draft = "";
    delete drafts[chat];
    replyingTo = null;
    chosenMentions = [];
    mentionQuery = null;
    // Keep the typed text for the history keybind, newest first, without dupes.
    sentHistory = [typed, ...sentHistory.filter((t) => t !== typed)].slice(0, 100);
    resetHistory();
    composerInput?.focus();
    try {
      await enqueue(() =>
        reply
          ? invoke("send_reply", {
              chat,
              text,
              replyToId: reply.id,
              replyToSender: reply.sender,
              replyToText: reply.text,
              mentions: jids,
              // A group message answered privately quotes across chats.
              replyToChat: reply.chat,
            })
          : invoke("send_text", { chat, text, mentions: jids }),
      );
      await reloadMessages();
      await refreshChats();
      scrollToBottom();
    } catch (e) {
      error = String(e);
    }
  }

  /** Stages a file for review rather than sending it straight away. */
  async function stageFile(file: File) {
    try {
      if (file.type === "image/svg+xml" || /\.svg$/i.test(file.name)) file = await rasterizeSvg(file);
      const kind = file.type.startsWith("image/")
        ? "image"
        : file.type.startsWith("video/")
          ? "video"
          : "other";

      // Staged before the preview is drawn, so Enter can send it straight away.
      const id = pendingSeq++;
      pending = [...pending, { id, file, url: kind === "video" ? URL.createObjectURL(file) : "", kind, caption: "" }];
      composerInput?.focus();
      if (kind === "image") {
        const url = await imagePreview(file);
        pending = pending.map((p) => (p.id === id ? { ...p, url } : p));
      }
    } catch (e) {
      // Staging must never take the chat down with it.
      error = `Could not preview that file: ${e}`;
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
    if (file) void stageFile(file);
    else if (isUriList) error = "Could not read that file. Try the 📎 button.";
  }

  /** Dropping files stages them; dropping anything else must not navigate. */
  function onDrop(event: DragEvent) {
    event.preventDefault();
    for (const file of Array.from(event.dataTransfer?.files ?? [])) void stageFile(file);
  }

  function removePending(id: number) {
    const item = pending.find((p) => p.id === id);
    if (item?.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    pending = pending.filter((p) => p.id !== id);
  }

  /** Upload progress while attachments go out; also the double-send guard. */
  /** Files on their way out, drawn at the end of their chat until the sent message replaces them. */
  let outgoing = $state<Outgoing[]>([]);

  /** `text` from the composer becomes the first attachment's caption, unless it has its own. */
  async function sendPending(text = "", mentions: string[] = []) {
    if (!selectedChat || pending.length === 0) return;
    const captioned = !!text && !pending[0].caption.trim();
    if (captioned) pending[0].caption = text;
    const firstId = pending[0].id;
    const chat = selectedChat;
    const items = [...pending];
    const reply = replyingTo;
    const once = sendOnce;
    // The tray empties at once; each file waits in the chat as a bubble instead.
    pending = [];
    replyingTo = null;
    sendOnce = false;
    const batch: Outgoing[] = items.map((item) => ({
      token: `upload-${item.id}-${Date.now()}`,
      chat,
      kind: item.kind,
      url: item.url,
      name: item.file.name,
      caption: item.caption.trim(),
      progress: 0,
    }));
    outgoing = [...outgoing, ...batch];
    scrollToBottom();
    const finish = (token: string) => {
      const done = outgoing.find((o) => o.token === token);
      if (done?.url.startsWith("blob:")) URL.revokeObjectURL(done.url);
      outgoing = outgoing.filter((o) => o.token !== token);
    };
    for (const [i, item] of items.entries()) {
      const { token } = batch[i];
      try {
        const data = await base64Of(item.file);
        const warning = await enqueue(() =>
          invoke<string | null>("send_media", {
            chat,
            name: item.file.name,
            data,
            caption: item.caption.trim() || null,
            replyToId: reply?.id ?? null,
            replyToSender: reply?.sender ?? null,
            replyToText: reply?.text ?? null,
            viewOnce: once && item.kind !== "other",
            mentions: captioned && item.id === firstId ? mentions : [],
            progress: token,
          }),
        );
        if (warning && settings.warn_missing_video_preview) notice = warning;
        if (selectedChat === chat) await reloadMessages();
        finish(token);
        if (selectedChat === chat) scrollToBottom();
      } catch (e) {
        error = String(e);
        // What did not go out returns to the tray, so it can be sent again.
        for (const rest of batch.slice(i)) outgoing = outgoing.filter((o) => o.token !== rest.token);
        pending = [...items.slice(i), ...pending];
        break;
      }
    }
    await refreshChats();
  }

  /** Base64 keeps a file a single IPC value; fine for attachments and voice notes. */
  async function base64Of(blob: Blob) {
    const buffer = new Uint8Array(await blob.arrayBuffer());
    let binary = "";
    for (let i = 0; i < buffer.length; i += 0x8000) {
      binary += String.fromCharCode(...buffer.subarray(i, i + 0x8000));
    }
    return btoa(binary);
  }

  /** Whether staged photos and videos go out as view once. */
  let sendOnce = $state(false);
  let recording = $state(false);

  async function sendVoice(note: Recording) {
    recording = false;
    if (!selectedChat) return;
    const chat = selectedChat;
    const reply = replyingTo;
    replyingTo = null;
    try {
      const data = await base64Of(note.blob);
      await enqueue(() =>
        invoke("send_voice", {
          chat,
          data,
          seconds: note.seconds,
          waveform: note.waveform,
          replyToId: reply?.id ?? null,
          replyToSender: reply?.sender ?? null,
          replyToText: reply?.text ?? null,
          viewOnce: note.viewOnce,
        }),
      );
      await reloadMessages();
      await refreshChats();
      scrollToBottom();
    } catch (e) {
      error = String(e);
    }
  }

  /** The display name of a chat, for cross chat quotes. */
  function chatName(jid: string) {
    return chats.find((c) => c.chat === jid)?.display_name ?? bareJid(jid);
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
    if (chat ?? selectedChat) await jumpTo(chat ?? selectedChat!, id);
  }

  /** Opens a chat at a message; one older than the loaded window offers to fetch it. */
  async function jumpTo(chat: string, id: string) {
    if (chat !== selectedChat) await openChat(chat);
    await tick();
    if (scroller?.querySelector(`[data-id="${id}"]`)) scrollToMessage(id);
    else {
      pendingJump = { chat, id };
      void loadAndJump();
    }
  }

  let starredItems = $state<StarredItem[] | null>(null);
  let showStarred = $state(false);
  async function openStarred() {
    showStarred = true;
    starredItems = null;
    try {
      const found = await invoke<StoredMessage[]>("starred_messages");
      starredItems = found.map((m) => ({
        chat: m.chat,
        id: m.id,
        where: chatName(m.chat),
        author: m.from_me ? "You" : displayName(m.sender_name, m.sender),
        text: replyPreviewText(m),
        timestamp: m.timestamp,
        sender: m.sender,
        fromMe: m.from_me,
      }));
    } catch (e) {
      showStarred = false;
      error = String(e);
    }
  }

  /** Mentions of us (everywhere or in one chat), or a search inside one chat. */
  let finder = $state<{
    mode: "pings" | "search";
    chat: string | null;
    items: FoundItem[] | null;
    /** Search only: the query shown, how far back the chat is loaded, and whether the phone may have older days. */
    query?: string;
    reach?: number | null;
    more?: boolean;
  } | null>(null);
  const SEARCH_LIMIT = 500;
  const unreadPings = $derived(chats.reduce((n, c) => n + c.mention_count, 0));

  function found(m: StoredMessage, across: boolean): FoundItem {
    return {
      chat: m.chat,
      id: m.id,
      where: across ? chatName(m.chat) : null,
      author: m.from_me ? "You" : displayName(m.sender_name, m.sender),
      text: replyPreviewText(m),
      timestamp: m.timestamp,
      unread: !m.read && !m.from_me,
    };
  }

  async function openPings(chat: string | null) {
    finder = { mode: "pings", chat, items: null };
    try {
      const got = await invoke<StoredMessage[]>("pings", { chat });
      if (finder?.mode === "pings" && finder.chat === chat) finder.items = got.map((m) => found(m, chat === null));
    } catch (e) {
      finder = null;
      error = String(e);
    }
  }

  /**
   * Searches the open finder's chat. `more` first asks the phone for the
   * previous 24 hours of the chat, then searches again over everything kept.
   */
  async function searchChat(query: string, more = false) {
    const current = finder;
    const chat = current?.chat;
    if (!current || !chat) return;
    if (more && chat === selectedChat) await recallDay();
    if (finder !== current) return;
    const reach = messages.at(-1)?.timestamp ?? null;
    if (!query.trim()) {
      Object.assign(current, { items: [], query, reach, more: false });
      return;
    }
    if (!more) current.items = null;
    try {
      const got = await invoke<StoredMessage[]>("search_messages", { chat, query, limit: SEARCH_LIMIT });
      if (finder !== current) return;
      Object.assign(current, {
        items: got.map((m) => found(m, false)),
        query,
        reach,
        more: !olderExhausted,
      });
    } catch (e) {
      error = String(e);
    }
  }

  /** Walks the chat's past back from the phone until the pending jump's message lands. */
  let seeking = $state(false);
  async function loadAndJump() {
    if (!pendingJump || seeking) return;
    const { chat, id } = pendingJump;
    seeking = true;
    try {
      // Ten rounds of 50 reach about 500 messages back before giving up.
      for (let round = 0; round < 10 && selectedChat === chat; round++) {
        const before = messages.length;
        await invoke("load_older", { chat, count: 50 });
        // The phone answers as a history sync event; give it a moment to land.
        await new Promise((r) => setTimeout(r, 2500));
        messageLimit += 50;
        await reloadMessages(true);
        await tick();
        if (scroller?.querySelector(`[data-id="${id}"]`)) {
          pendingJump = null;
          scrollToMessage(id);
          return;
        }
        if (messages.length === before) break;
      }
      error = "Your phone did not send that message; it may be older than it keeps, or deleted.";
    } catch (e) {
      error = String(e);
    } finally {
      seeking = false;
      pendingJump = null;
    }
  }

  /** Fetches a message's media on demand. */
  // Stickers, voice notes and SVG files read as part of the conversation, so ones that
  // arrived before automatic fetching are fetched as soon as they are shown.
  const autoFetched = new Set<string>();
  $effect(() => {
    if (!connected) return;
    for (const m of messages) {
      if (m.media_path || !(m.media_kind === "sticker" || m.media_kind === "audio" || isSvg(m))) continue;
      if (autoFetched.has(m.id) || marks.view_once.some((v) => v.id === m.id)) continue;
      autoFetched.add(m.id);
      void untrack(() => downloadMedia(m, true));
    }
  });

  /** Media downloads in flight, so a second click does not start another. */
  let downloading = $state<Record<string, true>>({});
  /** `quiet` for background fetches, whose failures only matter once clicked. */
  async function downloadMedia(message: StoredMessage, quiet = false) {
    if (!selectedChat || downloading[message.id]) return;
    downloading[message.id] = true;
    try {
      await invoke("download_media", { chat: selectedChat, id: message.id });
      await reloadMessages();
    } catch (e) {
      if (!quiet) error = String(e);
    } finally {
      delete downloading[message.id];
    }
  }

  const NO_MARKS: Marks = {
    reactions: [],
    starred: [],
    pinned: null,
    polls: [],
    events: [],
    view_once: [],
    forwarded: [],
    edited: [],
  };
  /** Reactions, stars, the pinned message, polls and events of the open chat. */
  let marks = $state<Marks>(NO_MARKS);

  async function loadMarks() {
    if (!selectedChat) return;
    try {
      marks = await invoke<Marks>("marks", { chat: selectedChat });
    } catch {
      marks = NO_MARKS;
    }
  }

  let creating = $state<"poll" | "event" | null>(null);

  async function create(value: unknown) {
    const chat = selectedChat;
    if (!chat) return;
    await enqueue(() =>
      creating === "poll"
        ? invoke("create_poll", { chat, ...(value as object) })
        : invoke("create_event", { chat, event: value }),
    );
    await reloadMessages();
    await loadMarks();
    await refreshChats();
    scrollToBottom();
  }

  /** Our own event being edited in the create dialog. */
  let editingEvent = $state<{ chat: string; event: ChatEvent } | null>(null);
  async function saveEvent(chat: string, id: string, fields: object) {
    await enqueue(() => invoke("edit_event", { chat, id, event: fields }));
    await reloadMessages();
    await loadMarks();
  }
  function eventFields(event: ChatEvent) {
    const { name, description, start, end, location, link } = event;
    return { name, description, start, end, location, link };
  }

  /** Per message: each emoji with its count, and whether one of them is ours. */
  const reactionsFor = $derived.by(() => {
    const byMessage = new Map<string, { emoji: string; count: number; mine: boolean }[]>();
    for (const r of marks.reactions) {
      const list = byMessage.get(r.target) ?? [];
      const entry = list.find((e) => e.emoji === r.emoji);
      if (entry) {
        entry.count += 1;
        entry.mine ||= r.sender === "@me";
      } else {
        list.push({ emoji: r.emoji, count: 1, mine: r.sender === "@me" });
      }
      byMessage.set(r.target, list);
    }
    return byMessage;
  });
  const starred = $derived(new Set(marks.starred));
  const edited = $derived(new Set(marks.edited));
  const forwarded = $derived(new Set(marks.forwarded));
  const pinnedMessage = $derived(
    marks.pinned ? (messages.find((m) => m.id === marks.pinned) ?? null) : null,
  );
  /** Pinned bar content for the chat header. */
  const pinnedView = $derived.by(() => {
    const m = pinnedMessage;
    if (!m) return null;
    return {
      id: m.id,
      author: m.from_me ? "You" : senderLabel(m),
      body: m.media_kind
        ? plain(captionOf(m), mentionName) || MEDIA_LABELS[m.media_kind]
        : plain(m.text, mentionName),
    };
  });

  const QUICK_REACTIONS = ["👍", "❤️", "😂", "😮", "😢", "🙏"];
  let menu = $state<{ x: number; y: number; message: StoredMessage } | null>(null);
  let forwarding = $state<StoredMessage | null>(null);
  let deleting = $state<StoredMessage | null>(null);
  let reporting = $state<StoredMessage | null>(null);
  /** Our message whose delivery and reads are shown; the version reloads it on new receipts. */
  let infoFor = $state<StoredMessage | null>(null);
  let infoVersion = $state(0);
  // Later readers in a group change no status, so the open info refreshes itself.
  $effect(() => {
    if (!infoFor) return;
    const timer = setInterval(() => (infoVersion += 1), 3000);
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
      error = String(e);
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
          editing = null;
          replyingTo = m;
          composerInput?.focus();
        },
      },
    ];
    if (m.from_me) {
      items.push({ label: "Message info", icon: "check", action: () => (infoFor = m) });
    }
    if (other) {
      items.push(
        {
          label: "Reply privately",
          icon: "users",
          action: async () => {
            await openChat(bare(m.sender));
            editing = null;
            replyingTo = m;
            composerInput?.focus();
          },
        },
        {
          label: `Message ${senderLabel(m)}`,
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
        { label: "Forward", icon: "forward", action: () => (forwarding = m) },
        {
          label: marks.pinned === m.id ? "Unpin" : "Pin",
          icon: "pin",
          action: () =>
            act(() =>
              invoke("pin_message", { target: target(m), pinned: marks.pinned !== m.id }),
            ),
        },
        {
          label: starred.has(m.id) ? "Unstar" : "Star",
          icon: "star",
          action: () =>
            act(() => invoke("star", { target: target(m), starred: !starred.has(m.id) })),
        },
      );
    }
    if (other) {
      items.push({
        label: "Report to admins",
        icon: "flag",
        separated: true,
        action: () => (reporting = m),
      });
    }
    items.push({
      label: "Delete",
      icon: "trash",
      danger: true,
      separated: !other,
      action: () => (deleting = m),
    });
    return items;
  }

  /** Whether we may delete this message for everyone: ours, or ours to moderate. */
  function canDeleteForEveryone(m: StoredMessage) {
    if (m.revoked) return false;
    if (m.from_me) return true;
    return !!me && !!memberOf(me)?.admin;
  }

  async function deleteMessage(everyone: boolean) {
    const m = deleting;
    deleting = null;
    if (!m) return;
    await act(async () => {
      await invoke("delete_message", { target: target(m), everyone, timestamp: m.timestamp });
      await reloadMessages();
      await refreshChats();
    });
  }

  /** The open chat's downloaded pictures and videos, oldest first, for the viewer. */
  const viewOnceIds = $derived(new Set(marks.view_once.map((v) => v.id)));
  function viewerItem(m: StoredMessage): ViewerItem {
    const who = m.from_me ? me : selectedChat?.endsWith("@g.us") ? bare(m.sender) : selectedChat;
    return {
      id: m.id,
      path: m.media_path!,
      thumb: m.media_thumb,
      kind: m.media_kind!,
      caption: plain(captionOf(m), mentionName),
      author: m.from_me ? "You" : senderLabel(m),
      avatar: who ? (avatars[who] ?? null) : null,
      timestamp: m.timestamp,
    };
  }
  const viewerItems = $derived<ViewerItem[]>(
    ordered
      .filter(
        (m) =>
          !m.revoked &&
          !!m.media_path &&
          !viewOnceIds.has(m.id) &&
          (m.media_kind === "image" || m.media_kind === "video" || m.media_kind === "gif"),
      )
      .map(viewerItem),
  );
  let viewerIndex = $state<number | null>(null);

  /** The view-once message being shown; closing it spends it. */
  let onceOpen = $state<StoredMessage | null>(null);
  let onceIndex = $state(0);
  async function closeViewOnce() {
    const message = onceOpen;
    onceOpen = null;
    if (!message) return;
    try {
      await invoke("open_view_once", { chat: message.chat, id: message.id });
      markPlayed(message);
    } catch (e) {
      error = String(e);
    }
    await reloadMessages();
    await loadMarks();
  }
  /** Tells the sender a voice note was heard or view-once media opened; the core honours the receipts setting. */
  function markPlayed(message: StoredMessage) {
    if (message.from_me) return;
    invoke("mark_played", { chat: message.chat, id: message.id, sender: message.sender }).catch(() => {});
  }
  /** The note after `finished` in the conversation, so the next one can autoplay. */
  function playNextVoice(finished: StoredMessage) {
    const at = ordered.findIndex((m) => m.id === finished.id);
    autoplayId = ordered.slice(at + 1).find((m) => m.media_kind === "audio" && m.media_path)?.id ?? null;
  }
  function openViewer(message: StoredMessage) {
    const at = viewerItems.findIndex((item) => item.id === message.id);
    if (at >= 0) viewerIndex = at;
  }

  /** Opens a downloaded media file in the desktop's default application. */
  async function openMedia(path: string) {
    if (/\.svg$/i.test(path)) return;
    try {
      await invoke("open_path", { path });
    } catch (e) {
      error = String(e);
    }
  }

  async function showQr(code: string | null) {
    qrSvg = code ? await invoke<string>("qr_svg", { value: code }) : null;
  }

  async function syncState() {
    const state = await invoke<ConnectionState>("connection_state");
    started = state.started;
    connected = state.connected;
    await showQr(state.connected ? null : state.qr);
    if (connected) {
      // Already connected when the UI loaded without an explicit connect (e.g.
      // a webview reload): there is no fresh backlog to gate on, so do not hold
      // the loading screen. A cold start reaches here disconnected, then gates.
      if (!connectRequested && !gateDone) gateDone = true;
      await refreshChats();
    }
  }

  /** Connects, reusing a stored session when there is one. */
  async function connect() {
    connecting = true;
    error = null;
    connectRequested = true;
    try {
      await invoke("connect");
      await syncState();
      resolveNames();
    } catch (e) {
      error = String(e);
    } finally {
      connecting = false;
    }
  }

  async function saveSettings(next: UiSettings) {
    try {
      await invoke("set_settings", { settings: next });
      settings = next;
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    let unlisten: (() => void) | undefined;

    // Surface anything that escapes a handler, so a failure shows a message
    // rather than leaving the interface silently unresponsive.
    const onError = (event: ErrorEvent) => {
      error = event.message || "Unexpected error";
    };
    const onRejection = (event: PromiseRejectionEvent) => {
      error = String(event.reason ?? "Unexpected error");
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
          setZoom(zoom + 0.1);
          return;
        }
        if (event.key === "-") {
          event.preventDefault();
          setZoom(zoom - 0.1);
          return;
        }
        if (event.key === "0") {
          event.preventDefault();
          setZoom(1);
          return;
        }
      }
      if (!selectedChat || !composerInput) return;
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
        pending.length > 0 &&
        target?.tagName !== "BUTTON" &&
        !target?.closest?.("[role=dialog]")
      ) {
        event.preventDefault();
        void send();
        return;
      }
      if (event.key.length !== 1) return;
      composerInput.focus();
    };
    window.addEventListener("keydown", onAnyKey);

    async function setup() {
      settings = await invoke<UiSettings>("get_settings");

      // The listener is attached before connecting so no event can be missed.
      unlisten = await listen<ServiceEvent>("service-event", async (event) => {
        const payload = event.payload;
        switch (payload.kind) {
          case "qrCode":
            await showQr(payload.code);
            break;
          case "connected":
            connected = true;
            await showQr(null);
            if (settings.skip_loading_screen) {
              gateDone = true;
              await refreshChats();
              resolveNames();
            } else {
              startGateTimeout();
            }
            break;
          case "disconnected":
            connected = false;
            break;
          case "uploadProgress": {
            const upload = outgoing.find((o) => o.token === payload.token);
            if (upload) upload.progress = payload.total > 0 ? payload.sent / payload.total : 0;
            break;
          }
          case "loggedOut":
            connected = false;
            started = false;
            await showQr(null);
            await loadAccounts();
            await connect();
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
              if (!fromMe) setTyping(chat, bare(sender), "paused");
              break;
            }
            // A message ends the sender's typing, whether or not "paused" arrived.
            if (!fromMe) {
              setTyping(chat, bare(sender), "paused");
            }
            if (uiUnlocked) {
              queueRefreshChats();
              if (chat === selectedChat) {
                // Follow the stream when already at the bottom, but never yank
                // the view down while reading older messages. Status-only
                // updates never follow or mark.
                queueReloadMessages(fresh && (fromMe || !scrolledUp), fresh && !fromMe);
              }
            }
            // A group seen for the first time has no name yet; look it up in
            // the background so the list stops showing a raw number.
            if (
              fresh &&
              !fromMe &&
              chat.endsWith("@g.us") &&
              Date.now() - (askedSubjects.get(chat) ?? 0) > 30_000 &&
              !chats.find((c) => c.chat === chat)?.display_name
            ) {
              askedSubjects.set(chat, Date.now());
              resolveNames();
            }
            break;
          }
          case "retentionApplied":
            if (payload.removed > 0 && !deferRefresh(null)) {
              queueRefreshChats();
              queueReloadMessages(false, false);
            }
            break;
          case "namesUpdated":
            // Address-book names arrived after the initial fetch, so the cached
            // display names are stale until both lists reload.
            forgetUnresolvedNames();
            if (!deferRefresh(null)) {
              queueRefreshChats();
              queueReloadMessages(false, false);
            }
            break;
          case "syncing":
            // The core counts what it stored, so the bar cannot run ahead of
            // the rows. Starting a new drain clears the previous dirty marks.
            syncPending = payload.pending;
            syncApplied = payload.applied;
            chatsDirty = false;
            messagesDirty = false;
            dirtyMarkRead = false;
            break;
          case "initialSyncComplete":
            // The backlog is in: paint it before the loading screen lifts, so
            // the first thing seen is the account as it now stands.
            finalizing = true;
            syncPending = 0;
            syncApplied = 0;
            try {
              await refreshChats();
              await reloadMessages();
              await tick();
            } finally {
              chatsDirty = false;
              messagesDirty = false;
              dirtyMarkRead = false;
              gateDone = true;
              finalizing = false;
            }
            break;
          case "synced": {
            // The backlog is in; flush once so the burst's queued refreshes land together.
            syncPending = 0;
            syncApplied = 0;
            if (chatsDirty) {
              chatsDirty = false;
              queueRefreshChats();
            }
            if (messagesDirty) {
              messagesDirty = false;
              const markRead = dirtyMarkRead;
              dirtyMarkRead = false;
              // Decide follow at flush time: a flag set mid-burst would use stale scroll state.
              queueReloadMessages(!scrolledUp, markRead);
            }
            break;
          }
          case "historyLoaded":
            historyActive = false;
            if (!uiUnlocked) {
              // The gate is still closed; the initial paint will pick this up.
              chatsDirty = true;
              if (selectedChat && payload.chats.includes(selectedChat)) messagesDirty = true;
              break;
            }
            await refreshChats();
            chatsDirty = false;
            if (selectedChat && payload.chats.includes(selectedChat)) {
              if (loadingOlder) messageLimit += 50;
              loadingOlder = false;
              clearTimeout(olderTimer);
              // The full reload covers any hints that landed while loading.
              messagesDirty = false;
              dirtyMarkRead = false;
              const before = messages.length;
              await reloadMessages(true);
              continueRecall(messages.length - before);
            } else if (messagesDirty && selectedChat) {
              // Burst hints that landed while older history was loading.
              messagesDirty = false;
              const markRead = dirtyMarkRead;
              dirtyMarkRead = false;
              queueReloadMessages(false, markRead);
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
            requestedAvatars.delete(payload.jid);
            delete avatars[payload.jid];
            loadAvatar(payload.jid);
            break;
          case "typing":
            setTyping(payload.chat, payload.sender, payload.state);
            break;
          case "presence":
            presence[payload.jid] = { online: payload.online, last_seen: payload.last_seen };
            break;
          case "marks":
            if (payload.chat === selectedChat) await loadMarks();
            break;
          case "memberLabel":
            if (payload.chat === selectedChat) {
              const label = payload.label || null;
              const member = participants.find((p) => p.jid === payload.jid);
              if (member) member.label = label;
              const info = groupInfo?.participants.find((p) => p.jid === payload.jid);
              if (info) info.label = label;
            }
            break;
          case "groupChanged":
            // Who may send, who is admin, or the name changed; the core dropped its cache.
            if (payload.chat === selectedChat) {
              await loadChatGroup(payload.chat);
              if (groupInfo) groupInfo = chatGroup;
            }
            void loadGroupKinds();
            void refreshChats();
            break;
        }
      });

      // Reuse a stored session automatically: pairing is only needed the very
      // first time, so the button should never be shown to a paired account.
      // With several linked accounts the user picks one first.
      await loadAccounts();
      await syncState();
      if (!started && accountList.filter((a) => a.jid).length > 1) {
        choosingAccount = true;
      } else {
        await connect();
        await loadAccounts();
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
    if (accountMenu && !(e.target as Element).closest?.(".user-panel")) accountMenu = false;
  }}
  oncontextmenu={(e) => {
    // The webview's own menu (Back, Refresh, Inspect) is meaningless here; keep
    // it only where it helps: text fields and a text selection.
    const el = e.target as HTMLElement;
    const editable = el.closest?.("input, textarea, [contenteditable]");
    if (!editable && !window.getSelection()?.toString()) e.preventDefault();
  }}
  onfocus={() => setOnline(true)}
  onblur={() => setOnline(false)}
  ondragover={(e) => e.preventDefault()}
  ondrop={onDrop}
/>

{#if error}
  <div class="error" role="alert">
    <span>{error}</span>
    <Button variant="icon" icon="x" iconSize={16} title="Dismiss" aria-label="Dismiss" onclick={() => (error = null)} />
  </div>
{/if}

<div class="app">
{#if !connected || !uiUnlocked}
  <PairingView
    {qrSvg}
    {started}
    {connecting}
    {connected}
    {choosingAccount}
    accounts={accountList}
    {activeAccount}
    {accountAvatars}
    linked={!qrSvg ? accountList.find((a) => a.id === activeAccount && a.jid) : undefined}
    {finalizing}
    {syncPending}
    {syncApplied}
    {syncPercent}
    {syncTimedOut}
    onconnect={connect}
    onchoose={chooseAccount}
    onswitch={switchTo}
    onsettings={() => openSettings("accounts")} />
{:else}
  <div class="layout" style="grid-template-columns: {layoutColumns}">
    <ChatSidebar
      bind:searchQuery
      {searchResults}
      {visibleChats}
      {selectedChat}
      {chatFilter}
      onfilter={(filter) => (chatFilter = filter)}
      {unreadChats}
      {unreadPings}
      {avatars}
      chatLabelOf={chatLabel}
      {formatTime}
      typingLabelOf={typingLabel}
      previewAuthorOf={previewAuthor}
      previewTextOf={previewText}
      mediaIconOf={mediaIcon}
      {groupKinds}
      accounts={accountList}
      {activeAccount}
      {activeLabel}
      {accountAvatars}
      {me}
      {meVersion}
      {visibility}
      {accountMenu}
      onmenutoggle={() => (accountMenu = !accountMenu)}
      onswitchaccount={(id) => {
        accountMenu = false;
        void switchTo(id);
      }}
      onaddaccount={() => {
        accountMenu = false;
        void addAccount();
      }}
      onsettings={openSettings}
      onpings={() => openPings(null)}
      onstarred={openStarred}
      onsearch={runSearch}
      onopenresult={openFromSearch}
      onopenchat={openChat}
      ontogglepin={togglePin}
      onresize={startResize} />

    <section class="conversation">
      {#if selectedChat}
        {@const title =
          chats.find((c) => c.chat === selectedChat)?.display_name ??
          titleOverride ??
          displayName(null, selectedChat)}
        {@const typingNow = typingLabel(selectedChat)}
        <ChatHeader
          {selectedChat}
          isGroup={selectedChat.endsWith("@g.us")}
          {title}
          avatar={avatars[selectedChat] ?? null}
          {typingNow}
          {subtitle}
          {groupContext}
          presenceText={presenceLabel(selectedChat)}
          mentionTotal={mentionQueue.length}
          {mentionCursor}
          pinned={pinnedView}
          ongroupinfo={openGroupInfo}
          onsearch={() =>
            (finder = {
              mode: "search",
              chat: selectedChat,
              items: [],
              reach: messages.at(-1)?.timestamp ?? null,
              more: !olderExhausted,
            })}
          onpings={() => openPings(selectedChat)}
          onsettings={() => (chatSettingsOpen = true)}
          onjumpmention={jumpNextMention}
          onpinnedjump={(id) => scrollToMessage(id)} />

        <MessageList
          messages={ordered}
          isGroup={selectedChat.endsWith("@g.us")}
          {switching}
          bind:scroller
          {dayKey}
          {dayLabel}
          {senderLabel}
          memberTagOf={(sender) => memberOf(sender)?.label ?? null}
          {hue}
          {captionOf}
          viewOnceMarks={marks.view_once}
          {reactionsFor}
          starredSet={starred}
          editedSet={edited}
          forwardedSet={forwarded}
          {downloading}
          replyingToId={replyingTo?.id ?? null}
          {highlightedId}
          menuId={menu?.message.id ?? null}
          polls={marks.polls}
          events={marks.events}
          namer={(jid) => (jid === "@me" ? "You" : senderName(jid))}
          avatarOf={pictureOf}
          {avatars}
          voiceAvatarOf={(m) => {
            const voiceFrom = m.from_me ? me : bare(m.sender);
            return voiceFrom ? pictureOf(voiceFrom) : null;
          }}
          quoteAuthorOf={quoteAuthor}
          quoteTextOf={(m) => (m.reply_to_text ? plain(m.reply_to_text, mentionName) : null)}
          quoteChatNameOf={(m) =>
            m.reply_to_chat && m.reply_to_chat !== selectedChat ? chatName(m.reply_to_chat) : null}
          {autoplayId}
          onceAudioOpenId={onceOpen?.id ?? null}
          {loadingOlder}
          onloadolder={() => loadOlder()}
          uploads={outgoing.filter((o) => o.chat === selectedChat)}
          typers={typing[selectedChat] ?? []}
          typerLabelOf={(sender) => {
            const member = memberOf(sender);
            const name = member && !isPlaceholder(member.name) ? member.name : null;
            return name ?? senderName(sender);
          }}
          onscroll={onScroll}
          toWire={asWireMentions}
          targetOf={mentionTarget}
          onprofile={openProfile}
          onopenurl={openUrl}
          {formatTime}
          onreplydraft={(m) => {
            editing = null;
            replyingTo = m;
            composerInput?.focus();
          }}
          onmenu={(e, m) => {
            e.preventDefault();
            menu = { x: e.clientX, y: e.clientY, message: m };
          }}
          onjumpquoted={jumpToQuoted}
          ondownload={downloadMedia}
          onopenviewer={openViewer}
          onopenmedia={openMedia}
          onvote={(m, options) =>
            act(() => invoke("vote_poll", { chat: m.chat, id: m.id, options }))}
          onrespond={(m, response) =>
            act(() => invoke("respond_event", { chat: m.chat, id: m.id, response }))}
          oneditrequest={(m) => {
            const event = marks.events.find((e) => e.id === m.id);
            if (event) editingEvent = { chat: m.chat, event };
          }}
          oncancelevent={(m) => {
            const event = marks.events.find((e) => e.id === m.id);
            if (event) return saveEvent(m.chat, m.id, { ...eventFields(event), canceled: true });
          }}
          onreact={(m, emoji) => act(() => invoke("react", { target: target(m), emoji }))}
          onmarkplayed={markPlayed}
          onnextvoice={playNextVoice}
          onpausevoice={() => (autoplayId = null)}
          onreplymenu={(e, m) => {
            const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
            menu = { x: rect.left, y: rect.bottom + 4, message: m };
          }}
          ononce={(m) => {
            if (!m.media_path) return downloadMedia(m);
            onceIndex = 0;
            onceOpen = m;
          }}
          oncloseonce={closeViewOnce}
          oninviteopen={async (jid) => {
            await refreshChats();
            void openChat(jid);
          }} />

        {#if scrolledUp}
          <button class="jump" onclick={scrollToBottom}>
            Latest <Icon name="chevronDown" size={15} />
          </button>
        {/if}

        <ComposerBar
          bind:draft
          bind:composerInput
          {replyingTo}
          replyAuthor={replyingTo ? (replyingTo.from_me ? "yourself" : senderLabel(replyingTo)) : ""}
          replySnippet={replyingTo ? replyPreviewText(replyingTo) : ""}
          {editing}
          {pending}
          bind:sendOnce
          bind:recording
          {mentionMatches}
          bind:mentionIndex
          onselectmention={selectMention}
          {emojiToken}
          {emojiMatches}
          bind:emojiIndex
          onselectemoji={selectEmoji}
          bind:pickerTab
          {selectedChat}
          {enqueue}
          onpickeremoji={(emoji) => insertAtCaret(emoji)}
          onpickersent={async () => {
            await reloadMessages();
            await refreshChats();
            scrollToBottom();
          }}
          onpickererror={(message) => (error = message)}
          onstage={stageFile}
          oncreatekind={(kind) => (creating = kind)}
          oninput={onComposerInput}
          onkey={onComposerKey}
          onsend={() => void send()}
          oncancelreply={() => (replyingTo = null)}
          oncanceledit={cancelEditing}
          onremove={removePending}
          onsendvoice={sendVoice}
          onvoiceerror={(message) => (error = message)}
          onreceipts={toggleChatReceipts}
          ontyping={toggleChatTyping}
          {receiptsHidden}
          {typingHidden} />

        {#if chatGroup && !chatGroup.can_send}
          <div class="read-only" role="status">
            <Icon name={chatGroup.community ? "users" : "volume"} size={16} />
            {#if chatGroup.community}
              This is a community. People talk in its groups; announcements go to its announcement group.
            {:else}
              Only admins can send messages{chatGroup.announcements ? " to this community's announcements" : " here"}.
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

{#if menu}
  {#key menu}
    {@const m = menu.message}
    <MessageMenu
      x={menu.x}
      y={menu.y}
      items={menuItems(m)}
      reactions={QUICK_REACTIONS}
      current={reactionsFor.get(m.id)?.find((r) => r.mine)?.emoji ?? null}
      onreact={(emoji) => act(() => invoke("react", { target: target(m), emoji }))}
      onclose={() => (menu = null)} />
  {/key}
{/if}

{#if creating}
  <CreateDialog kind={creating} oncreate={create} onclose={() => (creating = null)} />
{/if}

{#if editingEvent}
  {@const { chat, event } = editingEvent}
  <CreateDialog
    kind="event"
    initial={event}
    oncreate={(value) => saveEvent(chat, event.id, value as object)}
    onclose={() => (editingEvent = null)} />
{/if}

{#if forwarding}
  {@const m = forwarding}
  <ChatPicker
    title="Forward message to"
    chats={chats.map((c) => ({ jid: c.chat, label: chatLabel(c), avatar: avatars[c.chat] ?? null }))}
    onpick={async (to) => {
      await enqueue(() => invoke("forward_message", { chat: m.chat, id: m.id, to }));
      await refreshChats();
    }}
    onclose={() => (forwarding = null)} />
{/if}

{#if deleting}
  {@const m = deleting}
  <ConfirmDialog
    label="Delete message"
    title="Delete message?"
    hint={canDeleteForEveryone(m)
      ? "Delete it for everyone in this chat, or only from your devices."
      : "It is removed from your devices only."}
    onclose={() => (deleting = null)}>
    {#snippet actions()}
      {#if canDeleteForEveryone(m)}
        <button class="danger" onclick={() => deleteMessage(true)}>Delete for everyone</button>
      {/if}
      <button class="danger" onclick={() => deleteMessage(false)}>Delete for me</button>
      <button onclick={() => (deleting = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if reporting}
  {@const m = reporting}
  <ConfirmDialog
    label="Report message"
    title="Report to admins?"
    hint="The group's admins see this message and that you reported it. WhatsApp is not told."
    onclose={() => (reporting = null)}>
    {#snippet actions()}
      <button
        class="danger"
        onclick={() => {
          reporting = null;
          act(() => invoke("report_message", { chat: m.chat, id: m.id }));
        }}>Report</button>
      <button onclick={() => (reporting = null)}>Cancel</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if chatSettingsOpen && selectedChat}
  <ChatSettings
    chat={selectedChat}
    title={chats.find((c) => c.chat === selectedChat) ? chatLabel(chats.find((c) => c.chat === selectedChat)!) : displayName(null, selectedChat)}
    globalAutoDownload={settings.auto_download_media}
    picture={avatars[selectedChat] ?? null}
    onchange={async (retention) => {
      loadOnScroll = retention.on_demand;
      await reloadMessages();
      await refreshChats();
    }}
    onclose={() => (chatSettingsOpen = false)} />
{/if}

{#if infoFor}
  {@const m = infoFor}
  <MessageInfo
    id={m.id}
    sentAt={m.timestamp}
    preview={replyPreviewText(m)}
    voice={m.media_kind === "audio"}
    group={m.chat.endsWith("@g.us")}
    audience={m.chat === selectedChat ? Math.max(0, participants.length - 1) : 0}
    version={infoVersion}
    namer={(name, jid) => (name && !isPlaceholder(name) ? name : senderName(jid))}
    picture={(jid) => pictureOf(bare(jid))}
    onclose={() => (infoFor = null)} />
{/if}

{#if profileCard}
  {@const card = profileCard}
  <ProfileCard
    jid={card.jid}
    x={card.x}
    y={card.y}
    name={card.name}
    self={card.self}
    picture={pictureOf(card.jid)}
    tag={memberOf(card.jid)?.label ?? null}
    onmessage={(jid) => {
      profileCard = null;
      showGroupInfo = false;
      void openChat(jid);
    }}
    onclose={() => (profileCard = null)} />
{/if}

{#if showStarred}
  <StarredList
    items={starredItems}
    onopen={(item) => {
      showStarred = false;
      void jumpTo(item.chat, item.id);
    }}
    onunstar={(item) =>
      act(async () => {
        await invoke("star", {
          target: { chat: item.chat, id: item.id, sender: item.sender, fromMe: item.fromMe },
          starred: false,
        });
        starredItems = starredItems?.filter((i) => i !== item) ?? null;
      })}
    onclose={() => (showStarred = false)} />
{/if}

{#if finder}
  {@const inChat = finder.chat ? chatName(finder.chat) : null}
  <MessageFinder
    title={finder.mode === "search" ? "Search messages" : inChat ? "Your mentions" : "Mentions"}
    subtitle={finder.mode === "search" && finder.reach
      ? `${inChat} · searched back to ${new Date(finder.reach * 1000).toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" })}`
      : (inChat ?? (finder.mode === "pings" ? "Every message that pinged you" : null))}
    moreLabel="Load the previous day"
    placeholder={finder.mode === "search" ? "Search this chat" : "Filter mentions"}
    items={finder.items}
    empty={finder.mode === "search" ? "Type to search the messages kept on this computer." : "Nobody has mentioned you yet."}
    onquery={finder.mode === "search" ? (q) => searchChat(q) : undefined}
    onmore={finder.mode === "search" && finder.more ? () => searchChat(finder?.query ?? "", true) : undefined}
    onopen={(item) => {
      finder = null;
      void jumpTo(item.chat, item.id);
    }}
    onclose={() => (finder = null)} />
{/if}

{#if onceOpen && onceOpen.media_kind !== "audio" && onceOpen.media_path}
  <MediaViewer
    items={[viewerItem(onceOpen)]}
    bind:index={onceIndex}
    onclose={closeViewOnce}
    onreply={(id) => {
      editing = null;
      replyingTo = messages.find((m) => m.id === id) ?? null;
      void closeViewOnce();
      composerInput?.focus();
    }}
    onjump={() => void closeViewOnce()} />
{/if}

{#if viewerIndex !== null && viewerItems.length > 0}
  <MediaViewer
    items={viewerItems}
    bind:index={viewerIndex}
    onclose={() => (viewerIndex = null)}
    onopen={openMedia}
    onreply={(id) => {
      editing = null;
      replyingTo = messages.find((m) => m.id === id) ?? null;
      viewerIndex = null;
      composerInput?.focus();
    }}
    onjump={(id) => {
      viewerIndex = null;
      scrollToMessage(id);
    }} />
{/if}

{#if showGroupInfo && selectedChat}
  {@const chat = chats.find((c) => c.chat === selectedChat)}
  <GroupInfo
    jid={selectedChat}
    title={chat ? chatLabel(chat) : displayName(null, selectedChat)}
    info={groupInfo}
    error={groupInfoError}
    {avatars}
    pinned={!!chat?.pinned}
    onavatar={(jid) => loadAvatar(bare(jid))}
    onretry={openGroupInfo}
    onpin={() => chat && togglePin(chat)}
    onopenurl={openUrl}
    onmessage={(jid) => {
      showGroupInfo = false;
      openChat(bare(jid));
    }}
    onprofile={(jid, name, e) => openProfile(jid, name, e)}
    {me}
    namer={displayName}
    onreports={() => invoke<AdminReport[]>("admin_reports", { chat: selectedChat })}
    onallowreports={(allow) => invoke("set_allow_admin_reports", { chat: selectedChat, allow })}
    onjump={(id) => {
      showGroupInfo = false;
      if (selectedChat) void jumpTo(selectedChat, id);
    }}
    onlabel={async (label) => {
      await invoke("set_member_label", { chat: selectedChat, label });
      const user = me?.split("@")[0];
      for (const list of [groupInfo?.participants ?? [], participants]) {
        const self = list.find((p) => p.jid === me || p.number === user);
        if (self) self.label = label || null;
      }
    }}
    onclose={() => (showGroupInfo = false)} />
{/if}

{#if pendingJump}
  <div class="notice">
    <Spinner />
    <span>Fetching older messages from your phone to find it…</span>
  </div>
{/if}

{#if notice}
  <div class="notice">
    <span>{notice}</span>
    <button class="link" onclick={muteNotice}>Do not warn again</button>
    <Button variant="icon" icon="x" iconSize={16} title="Dismiss" aria-label="Dismiss" onclick={() => (notice = null)} />
  </div>
{/if}

{#if showSettings}
  <Settings
    {settings}
    accounts={accountList}
    active={activeAccount}
    {me}
    meAvatar={me ? (avatars[me] ?? null) : null}
    {accountAvatars}
    bind:section={settingsSection}
    onclose={() => (showSettings = false)}
    onsave={saveSettings}
    onflush={flushMedia}
    onclearhistory={clearHistory}
    onrename={renameAccount}
    onremove={removeAccount}
    onadd={() => {
      showSettings = false;
      addAccount();
    }}
    onswitch={(id) => {
      showSettings = false;
      switchTo(id);
    }}
    onprivacy={(next) => (privacy = next)}
    onpicture={() => {
      if (!me) return;
      meVersion += 1;
      requestedAvatars.delete(me);
      delete avatars[me];
      loadAvatar(me);
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
