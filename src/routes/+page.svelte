<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";
  import { listen } from "@tauri-apps/api/event";
  import AudioPlayer from "$lib/AudioPlayer.svelte";
  import VoiceRecorder, { type Recording } from "$lib/VoiceRecorder.svelte";
  import StarredList, { type StarredItem } from "$lib/StarredList.svelte";
  import MessageFinder, { type FoundItem } from "$lib/MessageFinder.svelte";
  import ChatSettings, { type ChatRetention } from "$lib/ChatSettings.svelte";
  import ProfileCard from "$lib/ProfileCard.svelte";
  import MessageInfo from "$lib/MessageInfo.svelte";
  import InviteCard, { inviteLink } from "$lib/InviteCard.svelte";
  import { displayName as phoneName, isPlaceholder, phoneLabel } from "$lib/phone";
  import { polyfillCountryFlagEmojis } from "country-flag-emoji-polyfill";
  import flagFont from "country-flag-emoji-polyfill/dist/TwemojiCountryFlags.woff2?url";

  // Windows has no flag glyphs and draws the two letters instead. The font is
  // bundled, so nothing is fetched at runtime; elsewhere this is a no-op.
  polyfillCountryFlagEmojis("Twemoji Country Flags", flagFont);
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import Settings, { type Section } from "$lib/Settings.svelte";
  import GroupInfo, { type AdminReport } from "$lib/GroupInfo.svelte";
  import MediaViewer, { mediaSrc, type ViewerItem } from "$lib/MediaViewer.svelte";
  import VideoPlayer from "$lib/VideoPlayer.svelte";
  import appIcon from "../../src-tauri/icons/128x128.png";
  import ImageCropper from "$lib/ImageCropper.svelte";
  import MessageMenu, { type MenuItem } from "$lib/MessageMenu.svelte";
  import ChatPicker from "$lib/ChatPicker.svelte";
  import ExpressionPicker, { type PickerTab } from "$lib/ExpressionPicker.svelte";
  import { loadEmojis, rememberEmoji, searchEmojis, type Emoji } from "$lib/emoji";
  import PollCard, { type Poll } from "$lib/PollCard.svelte";
  import EventCard, { type ChatEvent } from "$lib/EventCard.svelte";
  import CreateDialog from "$lib/CreateDialog.svelte";
  import { blocks, plain, type Inline } from "$lib/format";
  import { activeTheme, applyTheme, customization, motion, save as saveCustomization } from "$lib/theme.svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";

  type StoredMessage = {
    chat: string;
    id: string;
    sender: string;
    sender_name: string | null;
    timestamp: number;
    from_me: boolean;
    text: string;
    media_kind: string | null;
    media_path: string | null;
    media_thumb: string | null;
    reply_to_id: string | null;
    reply_to_text: string | null;
    reply_to_sender: string | null;
    reply_to_chat: string | null;
    reply_to_kind: string | null;
    reply_to_thumb: string | null;
    read: boolean;
    revoked: boolean;
    mentioned: boolean;
    preview_url: string | null;
    preview_title: string | null;
    preview_desc: string | null;
    preview_thumb: string | null;
    preview_site: string | null;
    preview_color: string | null;
    status: string | null;
  };
  type ChatSummary = {
    chat: string;
    display_name: string | null;
    last_message_at: number;
    last_text: string;
    last_from_me: boolean;
    last_sender_name: string | null;
    last_sender: string;
    last_media_kind: string | null;
    message_count: number;
    unread_count: number;
    mention_count: number;
    pinned: boolean;
  };
  type Account = { id: string; label: string; jid: string | null };
  type SearchResult = {
    jid: string;
    name: string;
    number: string;
    kind: string;
    saved: boolean;
    has_messages: boolean;
  };
  type GroupInfo = {
    subject: string | null;
    description: string | null;
    created_at: number | null;
    participants: {
      jid: string;
      name: string;
      admin: boolean;
      owner: boolean;
      number: string | null;
      username: string | null;
      label: string | null;
    }[];
    allow_admin_reports: boolean;
  };
  type Retention = {
    max_age_hours: number | null;
    max_messages_per_chat: number | null;
  };
  type UiSettings = {
    retention: Retention;
    accept_full_history: boolean;
    auto_download_media: boolean;
    warn_missing_video_preview: boolean;
    media_dir: string | null;
    send_typing: boolean;
    send_receipts: boolean;
  };
  type ConnectionState = { started: boolean; connected: boolean; qr: string | null };

  /** A file staged in the composer, before it is sent. */
  type PendingMedia = {
    id: number;
    file: File;
    url: string;
    kind: "image" | "video" | "other";
    caption: string;
  };

  type ServiceEvent =
    | { kind: "qrCode"; code: string }
    | { kind: "connected" }
    | { kind: "disconnected" }
    | { kind: "loggedOut" }
    | { kind: "uploadProgress"; token: string; sent: number; total: number }
    | { kind: "message"; message: StoredMessage }
    | { kind: "retentionApplied"; removed: number }
    | { kind: "namesUpdated"; count: number }
    | { kind: "syncing"; pending: number }
    | { kind: "synced" }
    | { kind: "historyLoaded"; chats: string[] }
    | { kind: "avatarChanged"; jid: string }
    | { kind: "typing"; chat: string; sender: string; state: string }
    | { kind: "presence"; jid: string; online: boolean; last_seen: number | null }
    | { kind: "memberLabel"; chat: string; jid: string; label: string }
    | { kind: "marks"; chat: string };

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

  /** CSS extensions, kept from closing their own style element. */
  const extensionCss = $derived(
    customization.extensions
      .filter((e) => e.enabled && e.css.trim())
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
  let messages: StoredMessage[] = $state([]);
  /** Offline-backlog progress: how many were announced and how many arrived. */
  let syncPending = $state(0);
  let syncSeen = $state(0);
  let syncPercent = $derived(
    syncPending > 0 ? Math.min(100, Math.round((syncSeen / syncPending) * 100)) : 0,
  );
  let draft = $state("");
  /** Per-chat composer text, so switching chats does not lose what was typed. */
  let drafts: Record<string, string> = $state({});
  let composerInput: HTMLTextAreaElement | undefined = $state();
  /** Group members for the @ autocomplete. */
  type Member = {
    jid: string;
    name: string;
    number: string | null;
    /** The member's reserved WhatsApp username, when they have one. */
    username: string | null;
    label: string | null;
    admin: boolean;
  };
  let participants: Member[] = $state([]);
  /** Last member list per group, shown while a switch reloads it so the header does not flash. */
  const memberCache: Record<string, Member[]> = {};
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
  let settings: UiSettings = $state({
    retention: { max_age_hours: 24, max_messages_per_chat: 500 },
    accept_full_history: true,
    auto_download_media: true,
    warn_missing_video_preview: true,
    media_dir: null,
    send_typing: true,
    send_receipts: true,
  });
  let showSettings = $state(false);
  let showGroupInfo = $state(false);
  let groupInfo: GroupInfo | null = $state(null);
  let groupInfoError = $state<string | null>(null);
  /** Sidebar widths, adjustable by dragging their edges. */
  const WIDTH_KEY = "hermodr.sidebarWidth";
  let leftWidth = $state(
    (() => {
      try {
        return Number(localStorage.getItem(WIDTH_KEY)) || 300;
      } catch {
        return 300;
      }
    })(),
  );
  let layoutColumns = $derived(`${leftWidth}px 1fr`);

  function startResize(event: MouseEvent) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = leftWidth;
    const onMove = (e: MouseEvent) => {
      leftWidth = Math.max(180, Math.min(640, startWidth + e.clientX - startX));
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
      try {
        localStorage.setItem(WIDTH_KEY, String(leftWidth));
      } catch {
        // Only the remembered width is lost.
      }
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  }
  let error = $state<string | null>(null);

  let scroller: HTMLDivElement | undefined = $state();
  /** True while the user is reading older messages with new ones below. */
  let scrolledUp = $state(false);
  let filePicker: HTMLInputElement | undefined = $state();
  /** Files staged for review before they are sent, shown above the composer. */
  let pending: PendingMedia[] = $state([]);
  /** Id of the staged file whose preview/caption sheet is open. */
  let previewId = $state<number | null>(null);
  let pendingSeq = 0;
  let previewItem = $derived(pending.find((p) => p.id === previewId) ?? null);

  function bareJid(jid: string) {
    return jid.replace(/@.*$/, "");
  }
  function chatLabel(chat: ChatSummary) {
    return displayName(chat.display_name, chat.chat);
  }
  /** Up to two letters for an avatar, or a digit pair for a bare number. */
  function initials(label: string) {
    const words = label.replace(/[^\p{L}\p{N}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
    if (words.length === 0) return "#";
    if (words.length === 1) return words[0].slice(0, 2).toUpperCase();
    return (words[0][0] + words[1][0]).toUpperCase();
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
  /** A stable hue per chat, so an avatar keeps its colour across sessions. */
  function hue(jid: string) {
    let h = 0;
    for (const c of jid) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
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
  const MEDIA_LABELS: Record<string, string> = {
    image: "Photo",
    video: "Video",
    gif: "GIF",
    audio: "Audio",
    document: "Document",
    sticker: "Sticker",
  };
  function mediaIcon(kind: string | null): IconName | null {
    if (kind === "image" || kind === "sticker") return "image";
    if (kind === "video" || kind === "gif") return "video";
    if (kind === "audio") return "mic";
    if (kind === "document") return "file";
    return null;
  }
  /** The group member a sender is, matched by LID or by phone number. */
  function memberOf(jid: string) {
    const b = bare(jid);
    const user = b.split("@")[0];
    return participants.find((p) => p.jid === b || p.number === user);
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
    const known = messages.find((m) => bare(m.sender) === b && m.sender_name)?.sender_name;
    const member = memberOf(jid)?.name;
    return displayName(known && !isPlaceholder(known) ? known : (member ?? known), jid);
  }
  /** Author shown on a quote; our own messages read "You". */
  function quoteAuthor(jid: string | null) {
    if (!jid) return "Message";
    return jid === "@me" ? "You" : senderName(jid);
  }
  /** Human-readable delivery state for a message we sent. */
  function statusMark(status: string | null) {
    switch (status) {
      case "pending":
        return "🕓";
      case "sent":
        return "✓";
      case "delivered":
        return "✓✓";
      case "read":
        return "✓✓";
      default:
        return "";
    }
  }

  function formatTime(seconds: number) {
    return new Date(seconds * 1000).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  /** Chat list only: cheap, local, never blocks on the network. */
  async function refreshChats() {
    try {
      chats = await invoke<ChatSummary[]>("chats");
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
    }
    selectedChat = chat;
    // One-to-one typing only arrives for contacts we are subscribed to.
    if (!chat.endsWith("@g.us")) invoke("watch_presence", { jid: chat }).catch(() => {});
    titleOverride = label;
    scrolledUp = false;
    recall = null;
    olderExhausted = false;
    loadOnScroll = true;
    invoke<{ retention: ChatRetention }>("chat_settings", { chat })
      .then((s) => selectedChat === chat && (loadOnScroll = s.retention.on_demand))
      .catch(() => {});
    participants = memberCache[chat] ?? [];
    chosenMentions = [];
    mentionQuery = null;
    draft = drafts[chat] ?? "";
    showGroupInfo = false;
    groupInfo = null;
    try {
      messageLimit = PAGE;
      // Mentions are captured before the chat is marked read, since that clears them.
      const [mentions, loaded] = await Promise.all([
        invoke<string[]>("unread_mentions", { chat }).catch(() => [] as string[]),
        invoke<StoredMessage[]>("messages", { chat, limit: messageLimit }),
      ]);
      // A quicker click on another chat has already taken over.
      if (selectedChat !== chat) return;
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
    // Group members power the @ autocomplete; a one-to-one chat returns none.
    try {
      const loaded = await invoke<Member[]>("participants", { chat });
      memberCache[chat] = loaded;
      if (selectedChat !== chat) return;
      participants = loaded;
      // Loading members stores their group display names, so numbers looked up
      // before now may have a name; ask again.
      forgetUnresolvedNames();
    } catch {
      if (selectedChat === chat) participants = memberCache[chat] ?? [];
    }
    // Opening a chat is the obvious moment to start typing.
    await tick();
    if (jumpToMention && mentionQueue.length > 0) {
      mentionCursor = 1;
      scrollToMessage(mentionQueue[0]);
    }
    composerInput?.focus();
  }


  /** Link embeds whose image is large enough to draw under the text. */
  let wideEmbeds = $state<Record<string, boolean>>({});

  function hostOf(url: string) {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  }

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
    participants = [];
    chosenMentions = [];
    mentionQueue = [];
    groupInfo = null;
    showGroupInfo = false;
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
      recall = null;
      olderExhausted = true;
      if (!auto) error = "Your phone did not answer. It has to be online for older messages to load.";
      settleRecall();
    }, 15000);
    try {
      await invoke("load_older", { chat: selectedChat, count: 50 });
    } catch (e) {
      loadingOlder = false;
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

  function replyIcon(kind: string | null) {
    if (kind === "audio") return "\u{1F3B5}";
    if (kind === "video") return "\u{1F3AC}";
    if (kind === "document") return "\u{1F4C4}";
    return "\u{1F4CE}";
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
      groupInfo = await invoke<GroupInfo>("group_info", { chat: selectedChat });
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
    const fromBottom = scroller ? scroller.scrollHeight - scroller.scrollTop : 0;
    try {
      messages = await invoke<StoredMessage[]>("messages", {
        chat: selectedChat,
        limit: messageLimit,
      });
    } catch (e) {
      error = String(e);
      return;
    }
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

  /** A JID without its device suffix, the form pictures and names are keyed by. */
  function bare(jid: string) {
    return jid.replace(/:\d+(?=@)/, "");
  }

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
    const lid = `${user}@lid`;
    const pn = `${user}@s.whatsapp.net`;
    const lidName = displayName(null, lid);
    const pnName = displayName(null, pn);
    const lidKnown = !!learnedNames[lid] && !/^\d+$/.test(learnedNames[lid]);
    return lidKnown ? { jid: lid, name: lidName, self: false } : { jid: pn, name: pnName, self: false };
  }
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
    for (const p of named) {
      const token = `@${p.name}`;
      if (text.includes(token)) text = text.split(token).join(`@${p.jid.split("@")[0]}`);
    }
    return text;
  }
  /** The profile card open beside a mention, name or picture. */
  let profileCard = $state<{ jid: string; name: string; x: number; y: number; self: boolean } | null>(null);
  function openProfile(jid: string, name: string, event: MouseEvent, self = false) {
    event.stopPropagation();
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
    for (const message of messages) if (!message.from_me) loadAvatar(bare(message.sender));
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

  function reportTyping() {
    const chat = selectedChat;
    if (!chat || !settings.send_typing) return;
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
  const STATUS_TEXT = {
    offline: "Connecting…",
    online: "Online",
    contacts: "Online to contacts",
    invisible: "Invisible",
  };

  $effect(() => {
    if (!connected || me) return;
    invoke<string | null>("own_jid")
      .then((jid) => {
        me = jid;
        if (jid) loadAvatar(jid);
        // The backend just recorded the JID on the account; pick it up.
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
    const { text, jids } = mentionPayload();
    const reply = replyingTo;
    draft = "";
    delete drafts[chat];
    replyingTo = null;
    chosenMentions = [];
    mentionQuery = null;
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

  /**
   * Builds a small preview image data URL.
   *
   * The full-size bitmap is never handed to the layout. Decoding a large photo
   * into the render tree is what killed the webview: pasting a screenshot
   * crashed the renderer and took the chat with it. Drawing a downscaled copy
   * keeps the decoded surface small.
   */
  async function imagePreview(file: File): Promise<string> {
    const bitmap = await createImageBitmap(file);
    const maxSide = 480;
    const scale = Math.min(1, maxSide / Math.max(bitmap.width, bitmap.height));
    const width = Math.max(1, Math.round(bitmap.width * scale));
    const height = Math.max(1, Math.round(bitmap.height * scale));

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) {
      bitmap.close();
      throw new Error("no 2d context for preview");
    }
    context.drawImage(bitmap, 0, 0, width, height);
    bitmap.close();
    return canvas.toDataURL("image/jpeg", 0.7);
  }

  /** Draws an SVG to a PNG whose longer side is Full HD, since WhatsApp cannot show SVGs. */
  async function rasterizeSvg(file: File): Promise<File> {
    const LONG_SIDE = 1920;
    const url = URL.createObjectURL(file);
    try {
      const image = new Image();
      image.src = url;
      await image.decode();
      // An SVG without width and height has no intrinsic size; treat it as square.
      const naturalWidth = image.naturalWidth || LONG_SIDE;
      const naturalHeight = image.naturalHeight || LONG_SIDE;
      const scale = LONG_SIDE / Math.max(naturalWidth, naturalHeight);
      const canvas = document.createElement("canvas");
      canvas.width = Math.max(1, Math.round(naturalWidth * scale));
      canvas.height = Math.max(1, Math.round(naturalHeight * scale));
      const context = canvas.getContext("2d");
      if (!context) throw new Error("no 2d context to draw the SVG");
      context.imageSmoothingQuality = "high";
      context.drawImage(image, 0, 0, canvas.width, canvas.height);
      const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, "image/png"));
      if (!blob) throw new Error("the SVG could not be drawn");
      return new File([blob], `${file.name.replace(/\.svg$/i, "")}.png`, { type: "image/png" });
    } finally {
      URL.revokeObjectURL(url);
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

  function attach(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    for (const file of files) void stageFile(file);
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

  let cropping = $state(false);
  $effect(() => {
    void previewId;
    cropping = false;
  });
  /** Swaps a staged image for its cropped or resized version. */
  async function replacePending(id: number, file: File) {
    const item = pending.find((p) => p.id === id);
    if (!item) return;
    if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    item.file = file;
    item.url = await imagePreview(file);
    cropping = false;
  }

  function removePending(id: number) {
    const item = pending.find((p) => p.id === id);
    if (item?.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    pending = pending.filter((p) => p.id !== id);
    if (previewId === id) previewId = null;
  }

  function clearPending() {
    for (const item of pending) {
      if (item.url.startsWith("blob:")) URL.revokeObjectURL(item.url);
    }
    pending = [];
    previewId = null;
  }

  /** Upload progress while attachments go out; also the double-send guard. */
  /** Files on their way out, drawn at the end of their chat until the sent message replaces them. */
  type Outgoing = {
    token: string;
    chat: string;
    kind: "image" | "video" | "other";
    url: string;
    name: string;
    caption: string;
    /** 0 to 1, from the core's upload progress. */
    progress: number;
  };
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
    previewId = null;
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

  /** An SVG file sent as a document, which is drawn in place like a picture. */
  function isSvg(m: StoredMessage) {
    return m.media_kind === "document" && /\.svg$/i.test(m.media_path ?? m.text.split("\n")[0].trim());
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

  type Marks = {
    reactions: { target: string; sender: string; emoji: string }[];
    starred: string[];
    pinned: string | null;
    polls: Poll[];
    events: ChatEvent[];
    view_once: { id: string; opened: boolean }[];
    forwarded: string[];
    edited: string[];
  };
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

  let attachMenu = $state(false);
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
      caption: captionOf(m),
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
  const VIEW_ONCE_LABEL: Record<string, string> = { image: "Photo", video: "Video", audio: "Voice message" };

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
    if (connected) await refreshChats();
  }

  /** Connects, reusing a stored session when there is one. */
  async function connect() {
    connecting = true;
    error = null;
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
            await refreshChats();
            resolveNames();
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
            if (syncPending > 0) syncSeen += 1;
            // A message ends the sender's typing, whether or not "paused" arrived.
            if (!payload.message.from_me) {
              setTyping(payload.message.chat, bare(payload.message.sender), "paused");
            }
            queueRefreshChats();
            if (payload.message.chat === selectedChat) {
              // Follow the stream when already at the bottom, but never yank
              // the view down while reading older messages.
              queueReloadMessages(payload.message.from_me || !scrolledUp, !payload.message.from_me);
            }
            // A group seen for the first time has no name yet; look it up in
            // the background so the list stops showing a raw number.
            if (
              !payload.message.from_me &&
              payload.message.chat.endsWith("@g.us") &&
              Date.now() - (askedSubjects.get(payload.message.chat) ?? 0) > 30_000 &&
              !chats.find((c) => c.chat === payload.message.chat)?.display_name
            ) {
              askedSubjects.set(payload.message.chat, Date.now());
              resolveNames();
            }
            break;
          case "retentionApplied":
            if (payload.removed > 0) {
              queueRefreshChats();
              queueReloadMessages(false, false);
            }
            break;
          case "namesUpdated":
            // Address-book names arrived after the initial fetch, so the cached
            // display names are stale until both lists reload.
            forgetUnresolvedNames();
            queueRefreshChats();
            queueReloadMessages(false, false);
            break;
          case "syncing":
            syncPending = payload.pending;
            syncSeen = 0;
            break;
          case "synced":
            // The backlog is in; refresh so the lists include everything the
            // burst delivered.
            queueRefreshChats();
            queueReloadMessages(false, false);
            syncPending = 0;
            syncSeen = 0;
            break;
          case "historyLoaded":
            await refreshChats();
            if (selectedChat && payload.chats.includes(selectedChat)) {
              if (loadingOlder) messageLimit += 50;
              loadingOlder = false;
              clearTimeout(olderTimer);
              const before = messages.length;
              await reloadMessages(true);
              continueRecall(messages.length - before);
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
</svelte:head>

{#snippet runs(nodes: Inline[])}{#each nodes as n, i (i)}{#if n.kind === "text"}{n.text}{:else if n.kind === "link"}<a
        class="link"
        href={n.url}
        onclick={(e) => {
          e.preventDefault();
          openUrl(n.url);
        }}>{n.url}</a
      >{:else if n.kind === "code"}<code class="inline-code">{n.text}</code>{:else if n.kind === "mention"}{@render
        mentionPill(n.user)}{:else if n.kind === "bold"}<strong
        >{@render runs(n.children)}</strong
      >{:else if n.kind === "italic"}<em>{@render runs(n.children)}</em>{:else}<s
        >{@render runs(n.children)}</s
      >{/if}{/each}{/snippet}

{#snippet mentionPill(user: string)}{@const target = mentionTarget(user)}{@const picture = pictureOf(target.jid)}<button
    type="button"
    class="mention-pill"
    class:self={target.self}
    onclick={(e) => openProfile(target.jid, target.name, e, target.self)}
    ondblclick={(e) => e.stopPropagation()}
    >{#if picture}<img src={convertFileSrc(picture)} alt="" />{:else}<span
        class="mention-initials"
        style="--hue: {hue(target.jid)}"
        >{#if /\p{L}/u.test(target.name)}{initials(target.name)}{:else}<Icon name="user" size={11} />{/if}</span
      >{/if}@{target.name}</button
  >{/snippet}

{#snippet lines(list: Inline[][])}{#each list as line, i (i)}{#if i > 0}<br />{/if}{@render runs(line)}{/each}{/snippet}

<!-- WhatsApp formatting, with the time's reserved space after the last line. -->
{#snippet formatted(text: string, mine: boolean)}
  <span class="text"
    >{#each blocks(asWireMentions(text)) as block, i (i)}{#if block.kind === "pre"}<pre class="pre">{block.text}</pre
        >{:else if block.kind === "quote"}<span class="quote-block">{@render lines(block.lines)}</span
        >{:else if block.kind === "list"}{#if block.ordered}<ol class="fmt-list">
            {#each block.items as item, j (j)}<li>{@render runs(item)}</li>{/each}
          </ol>{:else}<ul class="fmt-list">
            {#each block.items as item, j (j)}<li>{@render runs(item)}</li>{/each}
          </ul>{/if}{:else}{#if i > 0}<br />{/if}{@render lines(block.lines)}{/if}{/each}<span
      class="meta-spacer"
      class:mine></span
    ></span
  >
{/snippet}

{#snippet avatarFor(jid: string, label: string)}
  {#if avatars[jid]}
    <img class="avatar" src={convertFileSrc(avatars[jid]!)} alt="" />
  {:else}
    <span class="avatar" style="--hue: {hue(jid)}">{initials(label)}</span>
  {/if}
{/snippet}

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
    <button class="icon" title="Dismiss" aria-label="Dismiss" onclick={() => (error = null)}>
      <Icon name="x" size={16} />
    </button>
  </div>
{/if}

<div class="app">
{#if !connected}
  {@const stage = qrSvg ? 2 : started || connecting ? 1 : 0}
  <!-- An account that paired before signs straight back in; pairing only shows if WhatsApp asks for a code. -->
  {@const linked = !qrSvg ? accountList.find((a) => a.id === activeAccount && a.jid) : undefined}
  <div class="pairing">
    <div class="intro-glow" aria-hidden="true"></div>
    <header class="intro-head">
      <img class="intro-logo" src={appIcon} alt="" />
      <div>
        <h1>Hermóðr</h1>
        <span class="intro-tag">WhatsApp, native on your desktop</span>
      </div>
      <button class="icon intro-settings" title="Settings" aria-label="Settings" onclick={() => openSettings("accounts")}>
        <Icon name="settings" size={18} />
      </button>
    </header>

    {#if choosingAccount}
      <div class="intro-card resume">
        <h2>Choose an account</h2>
        <span class="resume-who">Several WhatsApp accounts are linked on this computer.</span>
        <div class="account-choices">
          {#each accountList.filter((a) => a.jid) as account (account.id)}
            <button class="account-choice" onclick={() => chooseAccount(account.id)}>
              {#if accountAvatars[account.id]}
                <img class="choice-avatar" src={convertFileSrc(accountAvatars[account.id]!)} alt="" />
              {:else}
                <span class="choice-avatar">{initials(account.label)}</span>
              {/if}
              <span class="choice-text">
                <strong>{account.label}</strong>
                <small>{phoneName(null, account.jid!)}</small>
              </span>
              <Icon name="chevronRight" size={16} />
            </button>
          {/each}
        </div>
      </div>
    {:else if linked}
      <div class="intro-card resume">
        {#if accountAvatars[linked.id]}
          <img class="resume-avatar" src={convertFileSrc(accountAvatars[linked.id]!)} alt="" />
        {:else}
          <span class="resume-avatar">{initials(linked.label)}</span>
        {/if}
        <h2>{started || connecting ? "Signing in" : "Welcome back"}</h2>
        <span class="resume-who">{linked.label} · {phoneName(null, linked.jid!)}</span>
        {#if started || connecting}
          <div class="resume-progress" role="status">
            <div class="resume-status">
              <span>{syncPending > 0 ? "Loading messages…" : "Connecting to WhatsApp…"}</span>
              {#if syncPending > 0}
                <span class="resume-count">{Math.min(syncSeen, syncPending)} of {syncPending} · {syncPercent}%</span>
              {/if}
            </div>
            <div
              class="resume-bar"
              class:determinate={syncPending > 0}
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={syncPending > 0 ? syncPercent : undefined}>
              <span style:width={syncPending > 0 ? `${syncPercent}%` : null}></span>
            </div>
          </div>
        {:else}
          <button class="primary" onclick={connect}>Connect</button>
        {/if}
        {#if accountList.length > 1}
          <div class="account-bar">
            {#each accountList as account (account.id)}
              {#if account.id !== linked.id}
                <button class="account" title="Switch to {account.label}" onclick={() => switchTo(account.id)}>
                  {#if accountAvatars[account.id]}
                    <img src={convertFileSrc(accountAvatars[account.id]!)} alt="" />
                  {:else}
                    <span class="account-initial">{initials(account.label)}</span>
                  {/if}
                  {account.label}
                </button>
              {/if}
            {/each}
          </div>
        {/if}
      </div>
    {:else}
    <div class="intro-card">
      <section class="intro-steps">
        <h2>Link this computer</h2>
        <ol>
          <li><span class="num">1</span><span>Open <strong>WhatsApp</strong> on your phone.</span></li>
          <li>
            <span class="num">2</span><span>Tap <strong>Menu</strong> or <strong>Settings</strong>, then <strong>Linked devices</strong>.</span>
          </li>
          <li><span class="num">3</span><span>Tap <strong>Link a device</strong>.</span></li>
          <li><span class="num">4</span><span>Point your phone at this screen to scan the code.</span></li>
        </ol>
        <!-- Each stage lights up as the connection actually reaches it. -->
        <div class="intro-progress" aria-label="Connection progress">
          {#each ["Connecting to WhatsApp", "Waiting for your phone", "Linked"] as label, i (label)}
            <span class="stage" class:done={stage > i} class:current={stage === i + 1 || (stage === 0 && i === 0)}>
              <span class="stage-dot"></span>{label}
            </span>
          {/each}
        </div>
        {#if accountList.length > 0}
          <div class="intro-accounts">
            <span class="intro-label">Accounts on this computer</span>
            <div class="account-bar">
              {#each accountList as account (account.id)}
                <button
                  class="account"
                  class:active={account.id === activeAccount}
                  title={account.label}
                  onclick={() => switchTo(account.id)}>
                  {#if accountAvatars[account.id]}
                    <img src={convertFileSrc(accountAvatars[account.id]!)} alt="" />
                  {:else}
                    <span class="account-initial">{initials(account.label)}</span>
                  {/if}
                  {account.label}
                </button>
              {/each}
            </div>
          </div>
        {/if}
      </section>

      <section class="intro-code">
        {#if qrSvg}
          <div class="qr" aria-label="Pairing QR code">
            {@html qrSvg}
            <img class="qr-logo" src={appIcon} alt="" />
          </div>
          <p class="hint">The code refreshes by itself. Keep this window open while you scan.</p>
        {:else if started || connecting}
          <div class="qr qr-loading" aria-label="Preparing a pairing code"><span class="spinner"></span></div>
          <p class="hint">Getting a pairing code from WhatsApp…</p>
        {:else}
          <div class="qr qr-idle"><Icon name="message" size={48} /></div>
          <button class="primary" onclick={connect}>Start pairing</button>
        {/if}
      </section>
    </div>
    {/if}

    <p class="intro-foot">
      Your messages stay end-to-end encrypted. History is kept only on this computer, within the limits
      you set in Settings.
    </p>
  </div>
{:else}
  <div class="layout" style="grid-template-columns: {layoutColumns}">
    <aside class="chats">
      <header>
        <h1 class="title">Chats</h1>
        <button class="icon badge-host" title="Mentions" aria-label="Mentions" onclick={() => openPings(null)}>
          <Icon name="at" size={18} />
          {#if unreadPings > 0}<span class="icon-badge">{unreadPings > 99 ? "99+" : unreadPings}</span>{/if}
        </button>
        <button class="icon" title="Starred messages" aria-label="Starred messages" onclick={openStarred}>
          <Icon name="star" size={18} />
        </button>
      </header>
      <label class="search">
        <Icon name="search" size={15} />
        <input
          placeholder="Search chats and contacts"
          bind:value={searchQuery}
          oninput={runSearch}
          autocomplete="off"
        />
      </label>
      {#if !searchQuery.trim()}
        <div class="filters" role="tablist" aria-label="Filter chats">
          <button
            class="chip"
            class:active={chatFilter === "all"}
            role="tab"
            aria-selected={chatFilter === "all"}
            onclick={() => (chatFilter = "all")}>All</button>
          <button
            class="chip"
            class:active={chatFilter === "unread"}
            role="tab"
            aria-selected={chatFilter === "unread"}
            onclick={() => (chatFilter = "unread")}
            >Unread{#if unreadChats > 0}<span class="chip-count">{unreadChats}</span>{/if}</button>
          <button
            class="chip"
            class:active={chatFilter === "groups"}
            role="tab"
            aria-selected={chatFilter === "groups"}
            onclick={() => (chatFilter = "groups")}>Groups</button>
        </div>
      {/if}
      {#if searchQuery.trim()}
        <ul class="results">
          {#each searchResults as result (result.jid)}
            <li>
              <div
                class="chat-row"
                role="button"
                tabindex="0"
                onclick={() => openFromSearch(result)}
                onkeydown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    openFromSearch(result);
                  }
                }}>
                {@render avatarFor(result.jid, result.name || result.number || result.jid)}
                <span class="name">
                  {#if result.kind === "group" || result.saved}
                    {result.name}
                  {:else}
                    {phoneLabel(result.number) ?? result.number}{result.name && result.name !== result.number
                      ? ` - ${result.name}`
                      : ""}
                  {/if}
                </span>
                <span class="preview">
                  {result.kind}{result.has_messages ? "" : " · no messages yet"}
                </span>
              </div>
            </li>
          {/each}
        </ul>
      {:else}
      <ul>
        {#each visibleChats as chat (chat.chat)}
          <li>
            <div
              class="chat-row"
              class:active={chat.chat === selectedChat}
              role="button"
              tabindex="0"
              onclick={() => openChat(chat.chat)}
              onkeydown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  openChat(chat.chat);
                }
              }}>
              {@render avatarFor(chat.chat, chatLabel(chat))}
              <span class="name">{#if chat.pinned}<span class="pin"><Icon name="pin" size={12} /></span>{/if}{chatLabel(chat)}</span>
              <span class="time" class:unread={chat.unread_count > 0}>{formatTime(chat.last_message_at)}</span>
              {#if typingLabel(chat.chat)}
                <span class="preview typing">{typingLabel(chat.chat)}</span>
              {:else}
                {@const author = previewAuthor(chat)}
                {@const icon = mediaIcon(chat.last_media_kind)}
                <span class="preview"
                  >{#if author}{author}:&nbsp;{/if}{#if icon}<span class="preview-icon"
                      ><Icon name={icon} size={15} /></span
                    >{/if}{previewText(chat)}</span
                >
              {/if}
              <span class="badges">
                {#if chat.mention_count > 0}
                  <button
                    class="badge mention-badge"
                    title="Jump to mention"
                    onclick={(e) => {
                      e.stopPropagation();
                      openChat(chat.chat, true);
                    }}>@</button>
                {/if}
                {#if chat.unread_count > 0}
                  <span class="badge">{chat.unread_count > 99 ? "99+" : chat.unread_count}</span>
                {/if}
                <button
                  class="pin-toggle"
                  title={chat.pinned ? "Unpin" : "Pin"}
                  aria-label={chat.pinned ? "Unpin" : "Pin"}
                  onclick={(e) => togglePin(chat, e)}><Icon name="pin" size={14} /></button>
              </span>
            </div>
          </li>
        {/each}
        {#if visibleChats.length === 0}
          <li class="empty">
            {chatFilter === "unread"
              ? "No unread chats."
              : chatFilter === "groups"
                ? "No groups yet."
                : "No conversations yet."}
          </li>
        {/if}
      </ul>
      {/if}

      <footer class="user-panel">
        {#if accountMenu}
          <div class="account-menu" role="menu">
            <span class="menu-label">Accounts</span>
            {#each accountList as account (account.id)}
              <button
                class="menu-item"
                class:current={account.id === activeAccount}
                role="menuitem"
                onclick={() => {
                  accountMenu = false;
                  switchTo(account.id);
                }}>
                {#if accountAvatars[account.id]}
                  <img class="menu-avatar" src={convertFileSrc(accountAvatars[account.id]!)} alt="" />
                {:else}
                  <span class="menu-avatar" style="--hue: {hue(account.id)}">{initials(account.label)}</span>
                {/if}
                <span class="menu-name">{account.label}</span>
                <span class="menu-dot"></span>
              </button>
            {/each}
            <div class="menu-sep"></div>
            <button
              class="menu-item"
              role="menuitem"
              onclick={() => {
                accountMenu = false;
                addAccount();
              }}><Icon name="plus" size={15} /> Add account</button>
            <button class="menu-item" role="menuitem" onclick={() => openSettings("accounts")}>
              <Icon name="users" size={15} /> Manage accounts
            </button>
          </div>
        {/if}
        <button
          class="me"
          title="Switch account"
          aria-expanded={accountMenu}
          onclick={() => (accountMenu = !accountMenu)}>
          <span class="me-avatar-wrap">
            {#if me && avatars[me]}
              <img class="me-avatar" src="{convertFileSrc(avatars[me]!)}?v={meVersion}" alt="" />
            {:else}
              <span class="me-avatar" style="--hue: {hue(activeAccount ?? '')}">{initials(activeLabel)}</span>
            {/if}
            <span class="presence {visibility}"></span>
          </span>
          <span class="me-text" title={me ? `+${me.split("@")[0]}` : undefined}>
            <span class="me-name">{activeLabel}</span>
            <span class="me-status">{STATUS_TEXT[visibility]}</span>
          </span>
        </button>
        <button
          class="icon"
          title="Settings"
          aria-label="Settings"
          onclick={() => openSettings("profile")}><Icon name="settings" size={19} /></button>
      </footer>
      <button type="button" class="resizer" aria-label="Resize chat list" onmousedown={startResize}></button>
    </aside>

    <section class="conversation">
      {#if selectedChat}
        {@const title =
          chats.find((c) => c.chat === selectedChat)?.display_name ??
          titleOverride ??
          displayName(null, selectedChat)}
        {@const typingNow = typingLabel(selectedChat)}
        <header>
          <div class="chat-heading">
            {#if selectedChat.endsWith("@g.us")}
              <button class="heading-avatar" title="Group info" aria-label="Group info" onclick={openGroupInfo}
                >{@render avatarFor(selectedChat, title)}</button
              >
              <button class="chat-title" title="Group info" onclick={openGroupInfo}>
                {title}
                <span class="chat-sub" class:typing={typingNow}
                  >{typingNow ?? subtitle ?? " "}</span
                >
              </button>
            {:else}
              {@render avatarFor(selectedChat, title)}
              <span class="chat-title">
                {title}
                {#if typingNow}<span class="chat-sub typing">{typingNow}</span
                  >{:else if presenceLabel(selectedChat)}<span class="chat-sub">{presenceLabel(selectedChat)}</span>{/if}
              </span>
            {/if}
          </div>
          <div class="header-tools">
            <button
              class="icon"
              title="Search in this chat"
              aria-label="Search in this chat"
              onclick={() =>
                (finder = {
                  mode: "search",
                  chat: selectedChat,
                  items: [],
                  reach: messages.at(-1)?.timestamp ?? null,
                  more: !olderExhausted,
                })}
              ><Icon name="search" size={18} /></button>
            {#if selectedChat.endsWith("@g.us")}
              <button
                class="icon"
                title="Your mentions in this group"
                aria-label="Your mentions in this group"
                onclick={() => openPings(selectedChat)}><Icon name="at" size={18} /></button>
            {/if}
            <button
              class="icon"
              title="Chat settings"
              aria-label="Chat settings"
              onclick={() => (chatSettingsOpen = true)}><Icon name="sliders" size={18} /></button>
          </div>
          {#if mentionQueue.length > 0}
            <button class="jump-mention" title="Jump to mention" onclick={jumpNextMention}>
              <Icon name="at" size={14} />
              {mentionCursor}/{mentionQueue.length}
            </button>
          {/if}
        </header>

        {#if pinnedMessage}
          <button class="pinned-bar" onclick={() => scrollToMessage(pinnedMessage.id)}>
            <Icon name="pin" size={16} />
            <span class="pinned-text">
              <strong>{pinnedMessage.from_me ? "You" : senderLabel(pinnedMessage)}:</strong>
              {pinnedMessage.media_kind ? captionOf(pinnedMessage) || MEDIA_LABELS[pinnedMessage.media_kind] : pinnedMessage.text}
            </span>
          </button>
        {/if}

        <div
          class="messages"
          class:switching
          class:group={selectedChat.endsWith("@g.us")}
          bind:this={scroller}
          onscroll={onScroll}>
          {#if messages.length > 0}
            <button class="load-older" onclick={() => loadOlder()} disabled={loadingOlder}>
              {loadingOlder ? "Asking your phone…" : "Load older messages"}
            </button>
          {/if}
          {#each ordered as message, i (message.id)}
            {@const prev = ordered[i - 1]}
            {@const newDay = !prev || dayKey(prev.timestamp) !== dayKey(message.timestamp)}
            {@const first =
              newDay || prev.from_me !== message.from_me || prev.sender !== message.sender}
            {@const isGroup = !!selectedChat?.endsWith("@g.us")}
            {@const viewOnce =
              message.media_kind === "view_once"
                ? { id: message.id, opened: true }
                : marks.view_once.find((v) => v.id === message.id)}
            {@const visual =
              !message.revoked &&
              !viewOnce &&
              (message.media_kind === "image" ||
                message.media_kind === "video" ||
                message.media_kind === "gif") &&
              !!(message.media_path || message.media_thumb)}
            {@const caption = visual ? captionOf(message) : ""}
            {@const showSender = first && !message.from_me && isGroup}
            {@const reactions = reactionsFor.get(message.id)}
            <!-- The time sits on the last line of text, as long as text ends the bubble. -->
            {@const inlineMeta =
              message.revoked ||
              (!message.preview_url &&
                (!message.media_kind || (!!caption && !!message.media_path)))}
            {#if newDay}
              <div class="day"><span>{dayLabel(message.timestamp)}</span></div>
            {/if}
            <!-- The whole row answers double-click and right-click, not just the bubble. -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="msg-row"
              class:replying={replyingTo?.id === message.id}
              class:jumped={message.id === highlightedId}
              class:for-me={!message.from_me &&
                (message.mentioned || message.reply_to_sender === "@me")}
              class:first-row={first}
              ondblclick={() => {
                replyingTo = message;
                composerInput?.focus();
              }}
              oncontextmenu={(e) => {
                e.preventDefault();
                menu = { x: e.clientX, y: e.clientY, message };
              }}>
            <div
              class="bubble"
              class:media-only={visual &&
                !caption &&
                !message.reply_to_text &&
                !message.preview_url &&
                !showSender}
              class:mine={message.from_me}
              class:first
              class:inline-meta={inlineMeta}
              class:has-reactions={!!reactions}
              class:sticker-only={message.media_kind === "sticker" &&
                !message.reply_to_text &&
                !showSender}
              class:menu-open={menu?.message.id === message.id}
              class:edited={edited.has(message.id)}
              data-id={message.id}>
              {#if showSender}
                <button
                  type="button"
                  class="sender-avatar"
                  title="Profile"
                  onclick={(e) => openProfile(message.sender, senderLabel(message), e)}
                  >{@render avatarFor(bare(message.sender), senderLabel(message))}</button>
                <button
                  type="button"
                  class="sender"
                  style="--hue: {hue(message.sender)}"
                  onclick={(e) => openProfile(message.sender, senderLabel(message), e)}>{senderLabel(message)}</button>
                {#if memberOf(message.sender)?.label}
                  <span class="member-label">{memberOf(message.sender)?.label}</span>
                {/if}
              {/if}

              {#if message.revoked}
                <span class="revoked">This message was deleted<span class="meta-spacer"></span></span>
              {:else}
                {#if forwarded.has(message.id)}
                  <span class="forwarded-mark"><Icon name="forward" size={13} /> Forwarded</span>
                {/if}
                {#if message.reply_to_text}
                  <button
                    type="button"
                    class="quote"
                    title="Go to message"
                    onclick={() => jumpToQuoted(message)}>
                    {#if message.reply_to_kind === "image" && message.reply_to_thumb}
                      <img
                        class="quote-thumb"
                        src={mediaSrc(message.reply_to_thumb)}
                        alt=""
                      />
                    {:else if message.reply_to_kind}
                      <span class="quote-icon">{replyIcon(message.reply_to_kind)}</span>
                    {/if}
                    <span class="quote-author">
                      {quoteAuthor(message.reply_to_sender)}
                    </span>
                    <span class="quote-text">{plain(message.reply_to_text, mentionName)}</span>
                    {#if message.reply_to_chat && message.reply_to_chat !== selectedChat}
                      <span class="quote-where">in {chatName(message.reply_to_chat)}</span>
                    {/if}
                  </button>
                {/if}

                {#if viewOnce}
                  {@const what = VIEW_ONCE_LABEL[message.media_kind ?? ""] ?? "View once message"}
                  {#if onceOpen?.id === message.id && message.media_kind === "audio" && message.media_path}
                    <AudioPlayer path={message.media_path} />
                    <button class="once-done" onclick={closeViewOnce}>Done</button>
                  {:else if message.media_kind === "view_once"}
                    <span class="once spent">
                      <span class="once-mark">1</span>
                      <span>View once message<small>Open it on your phone</small></span>
                    </span>
                  {:else if viewOnce.opened}
                    <span class="once spent">
                      <span class="once-mark">1</span>
                      <span>{what}<small>{message.from_me ? "View once" : "Opened"}</small></span>
                    </span>
                  {:else}
                    <button
                      class="once"
                      onclick={() => {
                        if (!message.media_path) return downloadMedia(message);
                        onceIndex = 0;
                        onceOpen = message;
                      }}>
                      <span class="once-mark">1</span>
                      <span>{what}<small>{message.media_path ? "View once" : "Download to view once"}</small></span>
                    </button>
                  {/if}
                {:else if message.media_kind === "sticker" && message.media_path}
                  <img class="sticker" src={convertFileSrc(message.media_path)} alt="Sticker" />
                {:else if message.media_kind === "sticker"}
                  <!-- Fetched on its own when shown; the placeholder keeps the sticker's space. -->
                  <button
                    class="sticker sticker-pending"
                    title={downloading[message.id] ? "Loading sticker" : "Load sticker"}
                    onclick={() => downloadMedia(message)}>
                    {#if downloading[message.id]}<span class="spinner"></span>{:else}<Icon name="sticker" size={28} />{/if}
                  </button>
                {:else if message.media_kind === "image" && (message.media_path || message.media_thumb)}
                  <button
                    class="media-button"
                    title={message.media_path ? "View" : "Download"}
                    onclick={() => (message.media_path ? openViewer(message) : downloadMedia(message))}>
                    <img
                      class="media"
                      src={mediaSrc((message.media_path ?? message.media_thumb)!)}
                      alt={message.text}
                    />
                    {#if !message.media_path}
                      <span class="media-overlay">
                        <span class="media-fetch">
                          {#if downloading[message.id]}<span class="spinner"></span>{:else}<Icon name="download" size={22} />{/if}
                        </span>
                      </span>
                    {/if}
                  </button>
                {:else if ["image", "video", "gif"].includes(message.media_kind ?? "") && !message.media_path}
                  <button
                    class="media-stub"
                    title="Download"
                    disabled={!!downloading[message.id]}
                    onclick={() => downloadMedia(message)}>
                    <span class="media-fetch">
                      {#if downloading[message.id]}<span class="spinner"></span>{:else}<Icon name="download" size={22} />{/if}
                    </span>
                    <span>{message.media_kind === "image" ? "Photo" : message.media_kind === "gif" ? "GIF" : "Video"}</span>
                  </button>
                {:else if (message.media_kind === "video" || message.media_kind === "gif") &&
                (message.media_path || message.media_thumb)}
                  <button
                    class="media-button video"
                    title={message.media_path ? "Play" : "Download"}
                    onclick={() => (message.media_path ? openViewer(message) : downloadMedia(message))}>
                    {#if message.media_thumb}
                      <img class="media" src={mediaSrc(message.media_thumb)} alt="" />
                    {/if}
                    <span class="media-overlay">
                      {#if message.media_kind === "gif"}GIF{:else}<span class="play">▶</span>{/if}
                    </span>
                  </button>
                {:else if message.media_kind === "audio" && message.media_path}
                  {@const voiceFrom = message.from_me ? me : bare(message.sender)}
                  <AudioPlayer
                    path={message.media_path}
                    avatar={voiceFrom ? pictureOf(voiceFrom) : null}
                    mine={message.from_me}
                    onplayed={() => markPlayed(message)}
                    initials={initials(message.from_me ? "You" : senderLabel(message))} />
                {:else if message.media_kind === "audio"}
                  <!-- Not downloaded yet: the note's own row, with the download where play will be. -->
                  <button
                    class="voice-pending"
                    title="Download voice message"
                    disabled={!!downloading[message.id]}
                    onclick={() => downloadMedia(message)}>
                    <span class="voice-pending-icon">
                      {#if downloading[message.id]}<span class="spinner"></span>{:else}<Icon
                          name="download"
                          size={18} />{/if}
                    </span>
                    <span class="voice-pending-bars" aria-hidden="true">
                      {#each Array(34) as _, i (i)}<span style="height: {20 + ((i * 37) % 60)}%"></span>{/each}
                    </span>
                  </button>
                {:else if message.media_kind === "poll"}
                  <PollCard
                    poll={marks.polls.find((p) => p.id === message.id)}
                    question={message.text}
                    namer={(jid) => (jid === "@me" ? "You" : senderName(jid))}
                    picture={(jid) => (jid === "@me" ? (me ? pictureOf(me) : null) : pictureOf(bare(jid)))}
                    onvote={(options) =>
                      act(() => invoke("vote_poll", { chat: message.chat, id: message.id, options }))} />
                {:else if message.media_kind === "event"}
                  {@const event = marks.events.find((e) => e.id === message.id)}
                  <EventCard
                    {event}
                    title={message.text}
                    onopenurl={openUrl}
                    onrespond={(response) =>
                      act(() => invoke("respond_event", { chat: message.chat, id: message.id, response }))}
                    onedit={message.from_me && event
                      ? () => (editingEvent = { chat: message.chat, event })
                      : undefined}
                    oncancel={message.from_me && event
                      ? () =>
                          act(() =>
                            saveEvent(message.chat, message.id, { ...eventFields(event), canceled: true }),
                          )
                      : undefined} />
                {:else if isSvg(message) && message.media_path}
                  <!-- An <img> never runs an SVG's scripts, so drawing it in place is safe. -->
                  <span class="svg-file">
                    <img class="media" src={convertFileSrc(message.media_path)} alt={message.text} />
                  </span>
                {:else if message.media_kind && (message.media_path || message.media_thumb)}
                  <button
                    class="file"
                    onclick={() => message.media_path && openMedia(message.media_path)}>
                    <Icon name="file" size={20} />
                    <span>{message.text || message.media_kind}</span>
                  </button>
                {:else}
                  {@render formatted(message.text, message.from_me)}
                {/if}

                {#if caption}
                  {@render formatted(caption, message.from_me)}
                {/if}

                {#if message.media_kind && !message.media_path && !viewOnce && !["poll", "event", "audio", "sticker", "image", "video", "gif"].includes(message.media_kind)}
                  <button class="download" onclick={() => downloadMedia(message)}>
                    <Icon name="download" size={14} />
                    Download {message.media_kind}
                  </button>
                {/if}

                {@const invite = inviteLink(message.text)}
                {#if invite}
                  <InviteCard
                    link={invite}
                    onopen={async (jid) => {
                      await refreshChats();
                      void openChat(jid);
                    }} />
                {:else if message.preview_url}
                  {@const url = message.preview_url}
                  {@const provider = message.preview_site?.trim() || hostOf(url)}
                  {@const title = message.preview_title?.trim() !== provider ? message.preview_title?.trim() : null}
                  {@const desc = message.preview_desc?.trim() !== url ? message.preview_desc?.trim() : null}
                  <!-- As Discord draws embeds: a small image sits beside the text, a large one under it. -->
                  <div class="embed" class:wide={wideEmbeds[message.id]} style:--embed-color={message.preview_color}>
                    <div class="embed-body">
                      <span class="embed-provider">{provider}</span>
                      {#if title}
                        <button type="button" class="embed-title" title={url} onclick={() => openUrl(url)}>{title}</button>
                      {/if}
                      {#if desc}<span class="embed-desc">{desc}</span>{/if}
                    </div>
                    {#if message.preview_thumb}
                      <button type="button" class="embed-image" title={url} onclick={() => openUrl(url)}>
                        <img
                          src={mediaSrc(message.preview_thumb)}
                          alt=""
                          onload={(e) => {
                            if ((e.currentTarget as HTMLImageElement).naturalWidth >= 300) wideEmbeds[message.id] = true;
                          }} />
                      </button>
                    {/if}
                  </div>
                {/if}
              {/if}

              <button
                class="reply-btn"
                title="Message options"
                aria-label="Message options"
                onclick={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  menu = { x: rect.left, y: rect.bottom + 4, message };
                }}><Icon name="chevronDown" size={16} /></button
              >
              <span class="meta">
                {#if starred.has(message.id)}<span class="star"><Icon name="star" size={11} /></span>{/if}
                {#if edited.has(message.id)}<span class="edited-mark">Edited</span>{/if}
                {formatTime(message.timestamp)}
                {#if message.from_me}
                  <span
                    class="ticks"
                    class:read={message.status === "read"}
                    title={message.status ?? "pending"}
                    >{#if message.status === "pending"}<Icon name="clock" size={11} />{:else}{statusMark(
                        message.status,
                      )}{/if}</span
                  >
                {/if}
              </span>
              {#if reactions}
                {@const mine = reactions.find((r) => r.mine)?.emoji ?? null}
                <button
                  class="reactions"
                  title={mine ? "Tap to remove your reaction" : "Reactions"}
                  onclick={() =>
                    mine &&
                    act(() => invoke("react", { target: target(message), emoji: "" }))}>
                  {#each reactions.slice(0, 3) as r (r.emoji)}<span>{r.emoji}</span>{/each}
                  {#if reactions.reduce((n, r) => n + r.count, 0) > 1}
                    <span class="reaction-count">{reactions.reduce((n, r) => n + r.count, 0)}</span>
                  {/if}
                </button>
              {/if}
            </div>
            </div>
          {/each}
          {#each outgoing.filter((o) => o.chat === selectedChat) as upload (upload.token)}
            <div class="msg-row" in:fly={{ y: 48, duration: motion(260), easing: cubicOut }}>
              <div class="bubble mine first outgoing-upload" class:media-only={upload.kind !== "other" && !upload.caption}>
                <div class="upload-visual" class:file-upload={upload.kind === "other" || !upload.url}>
                  {#if upload.kind === "image" && upload.url}
                    <img class="media" src={upload.url} alt={upload.name} />
                  {:else if upload.kind === "video" && upload.url}
                    <!-- svelte-ignore a11y_media_has_caption -->
                    <video class="media" src={upload.url} preload="metadata" muted></video>
                  {:else}
                    <span class="upload-name"><Icon name="file" size={20} />{upload.name}</span>
                  {/if}
                  <span class="upload-ring" aria-label="Uploading, {Math.round(upload.progress * 100)}%">
                    <svg viewBox="0 0 48 48" width="48" height="48">
                      <circle class="ring-track" cx="24" cy="24" r="20" />
                      <circle
                        class="ring-fill"
                        class:spinning={upload.progress === 0}
                        cx="24"
                        cy="24"
                        r="20"
                        stroke-dasharray="125.66"
                        stroke-dashoffset={125.66 * (1 - (upload.progress || 0.12))} />
                    </svg>
                    <span class="ring-label">{upload.progress > 0 ? `${Math.round(upload.progress * 100)}%` : ""}</span>
                  </span>
                </div>
                {#if upload.caption}<span class="upload-caption">{upload.caption}</span>{/if}
              </div>
            </div>
          {/each}
          {#if typing[selectedChat]?.length}
            {@const typer = typing[selectedChat][0]}
            <div class="bubble typing-bubble first">
              {#if isGroupChat}
                <span class="sender-avatar">{@render avatarFor(typer.sender, senderName(typer.sender))}</span>
                <span class="sender" style="--hue: {hue(typer.sender)}">
                  {memberOf(typer.sender)?.name && !isPlaceholder(memberOf(typer.sender)!.name)
                    ? memberOf(typer.sender)!.name
                    : senderName(typer.sender)}
                </span>
              {/if}
              {#if typer.state === "recording"}
                <span class="recording"><Icon name="mic" size={15} /> recording audio…</span>
              {:else}
                <span class="dots" aria-label="typing"><i></i><i></i><i></i></span>
              {/if}
            </div>
          {/if}
        </div>

        {#if scrolledUp}
          <button class="jump" onclick={scrollToBottom}>
            Latest <Icon name="chevronDown" size={15} />
          </button>
        {/if}

        {#if replyingTo}
          <div class="reply-preview">
            {#if replyingTo.media_kind === "image" && replyingTo.media_path}
              <img
                class="reply-thumb"
                src={convertFileSrc(replyingTo.media_path)}
                alt=""
              />
            {:else if replyingTo.media_kind}
              <span class="reply-icon">{replyIcon(replyingTo.media_kind)}</span>
            {/if}
            <span class="reply-body">
              <span class="reply-to"
                >Replying to {replyingTo.from_me ? "yourself" : senderLabel(replyingTo)}</span
              >
              <span class="reply-snippet">{replyPreviewText(replyingTo)}</span>
            </span>
            <button
              class="icon"
              title="Cancel reply"
              aria-label="Cancel reply"
              onclick={() => (replyingTo = null)}><Icon name="x" size={16} /></button>
          </div>
        {/if}

        {#if pending.length > 0}
          <div class="pending">
            {#each pending as item (item.id)}
              <div class="pending-item">
                <button class="pending-thumb" title="Preview and caption" onclick={() => (previewId = item.id)}>
                  {#if item.kind === "image"}
                    <img src={item.url} alt={item.file.name} />
                  {:else if item.kind === "video"}
                    <!-- svelte-ignore a11y_media_has_caption -->
                    <video src={item.url} preload="metadata" muted></video>
                    <span class="play-badge">▶</span>
                  {:else}
                    <span class="file-icon"><Icon name="file" size={26} /></span>
                  {/if}
                </button>
                <span class="pending-name" title={item.file.name}>{item.file.name}</span>
                {#if item.caption}
                  <span class="pending-caption">{item.caption}</span>
                {/if}
                <button
                  class="icon remove"
                  title="Remove"
                  aria-label="Remove"
                  onclick={() => removePending(item.id)}><Icon name="x" size={12} /></button
                >
              </div>
            {/each}
            <span class="pending-status">Type a caption below, then press Enter.</span>
          </div>
        {/if}

        {#if mentionQuery !== null && mentionMatches.length > 0}
          <div class="mentions">
            {#each mentionMatches as person, i (person.jid)}
              <button
                type="button"
                class="mention"
                class:active={i === mentionIndex}
                onclick={() => selectMention(person)}
                onmouseenter={() => (mentionIndex = i)}>
                {person.name}
                {#if person.username && person.username !== person.name}<span class="mention-handle"
                    >@{person.username}</span
                  >{/if}
              </button>
            {/each}
          </div>
        {/if}

        <div class="composer-area">
        {#if emojiToken && emojiMatches.length > 0}
          <div class="suggest" role="listbox" aria-label="Emoji suggestions">
            <span class="suggest-title">Emoji matching :{emojiToken.query}</span>
            {#each emojiMatches as e, i (e.emoji)}
              <button
                type="button"
                class="suggest-row"
                class:active={i === emojiIndex}
                role="option"
                aria-selected={i === emojiIndex}
                onmouseenter={() => (emojiIndex = i)}
                onclick={() => selectEmoji(e.emoji)}>
                <span class="suggest-emoji">{e.emoji}</span>
                :{e.shortcodes.find((c) => c.startsWith(emojiToken?.query.toLowerCase() ?? "")) ?? e.shortcodes[0] ?? e.label}:
              </button>
            {/each}
          </div>
        {/if}
        {#if pickerTab}
          <ExpressionPicker
            chat={selectedChat}
            bind:tab={pickerTab}
            {enqueue}
            onemoji={(emoji) => insertAtCaret(emoji)}
            onsent={async () => {
              await reloadMessages();
              await refreshChats();
              scrollToBottom();
            }}
            onerror={(message) => (error = message)}
            onclose={() => (pickerTab = null)} />
        {/if}
        <form class="composer" onsubmit={(e) => (e.preventDefault(), send())}>
          {#if recording}
            <VoiceRecorder
              onsend={sendVoice}
              oncancel={() => (recording = false)}
              onerror={(message) => (error = message)} />
          {:else}
          <button
            type="button"
            class="icon attach"
            class:active={attachMenu}
            title="Attach"
            aria-label="Attach"
            aria-expanded={attachMenu}
            onclick={() => (attachMenu = !attachMenu)}><Icon name="plus" size={22} /></button
          >
          {#if attachMenu}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div class="attach-catcher" role="presentation" onclick={() => (attachMenu = false)}></div>
            <div class="attach-menu" role="menu">
              <button
                type="button"
                role="menuitem"
                onclick={() => {
                  attachMenu = false;
                  filePicker?.click();
                }}><Icon name="paperclip" size={18} /> Upload a file</button>
              <button
                type="button"
                role="menuitem"
                onclick={() => {
                  attachMenu = false;
                  creating = "poll";
                }}><Icon name="poll" size={18} /> Create poll</button>
              <button
                type="button"
                role="menuitem"
                onclick={() => {
                  attachMenu = false;
                  creating = "event";
                }}><Icon name="calendar" size={18} /> Create event</button>
            </div>
          {/if}
          <input
            class="file-input"
            type="file"
            multiple
            bind:this={filePicker}
            onchange={attach}
          />
          <textarea
            bind:this={composerInput}
            value={draft}
            oninput={onComposerInput}
            onkeydown={onComposerKey}
            rows="1"
            placeholder={pending.length > 0 ? "Add a caption (optional)" : "Type a message"}
          ></textarea>
          <div class="composer-tools">
            <button
              type="button"
              class="icon tool-text"
              class:active={pickerTab === "gif"}
              title="GIFs"
              onclick={() => (pickerTab = pickerTab === "gif" ? null : "gif")}>GIF</button>
            <button
              type="button"
              class="icon"
              class:active={pickerTab === "sticker"}
              title="Stickers"
              aria-label="Stickers"
              onclick={() => (pickerTab = pickerTab === "sticker" ? null : "sticker")}
              ><Icon name="sticker" size={20} /></button>
            <button
              type="button"
              class="icon"
              class:active={pickerTab === "emoji"}
              title="Emoji"
              aria-label="Emoji"
              onclick={() => (pickerTab = pickerTab === "emoji" ? null : "emoji")}
              ><Icon name="smile" size={20} /></button>
            {#if pending.some((p) => p.kind !== "other")}
              <button
                type="button"
                class="icon once-toggle"
                class:active={sendOnce}
                title="View once"
                aria-label="View once"
                aria-pressed={sendOnce}
                onclick={() => (sendOnce = !sendOnce)}>1</button>
            {/if}
          </div>
          {#if !draft.trim() && pending.length === 0}
            <button
              class="send ready"
              type="button"
              title="Record a voice message"
              aria-label="Record a voice message"
              onclick={() => (recording = true)}><Icon name="mic" size={19} /></button>
          {:else}
            <button
              class="send ready"
              type="submit"
              title="Send"
              aria-label="Send"><Icon name="send" size={18} /></button>
          {/if}
          {/if}
        </form>
        </div>
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
  {@const m = menu.message}
  <MessageMenu
    x={menu.x}
    y={menu.y}
    items={menuItems(m)}
    reactions={QUICK_REACTIONS}
    current={reactionsFor.get(m.id)?.find((r) => r.mine)?.emoji ?? null}
    onreact={(emoji) => {
      menu = null;
      act(() => invoke("react", { target: target(m), emoji }));
    }}
    onclose={() => (menu = null)} />
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
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="sheet-backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (deleting = null)}>
    <div class="sheet confirm" role="dialog" aria-modal="true" aria-label="Delete message">
      <h2>Delete message?</h2>
      <p class="hint">
        {canDeleteForEveryone(m)
          ? "Delete it for everyone in this chat, or only from your devices."
          : "It is removed from your devices only."}
      </p>
      <div class="confirm-actions">
        {#if canDeleteForEveryone(m)}
          <button class="danger" onclick={() => deleteMessage(true)}>Delete for everyone</button>
        {/if}
        <button class="danger" onclick={() => deleteMessage(false)}>Delete for me</button>
        <button onclick={() => (deleting = null)}>Cancel</button>
      </div>
    </div>
  </div>
{/if}

{#if reporting}
  {@const m = reporting}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="sheet-backdrop"
    role="presentation"
    onclick={(e) => e.target === e.currentTarget && (reporting = null)}>
    <div class="sheet confirm" role="dialog" aria-modal="true" aria-label="Report message">
      <h2>Report to admins?</h2>
      <p class="hint">The group's admins see this message and that you reported it. WhatsApp is not told.</p>
      <div class="confirm-actions">
        <button
          class="danger"
          onclick={() => {
            reporting = null;
            act(() => invoke("report_message", { chat: m.chat, id: m.id }));
          }}>Report</button>
        <button onclick={() => (reporting = null)}>Cancel</button>
      </div>
    </div>
  </div>
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

{#if previewItem}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="sheet-backdrop"
    role="presentation"
    onclick={(e) => {
      if (e.target === e.currentTarget) previewId = null;
    }}>
    <div
      class="sheet preview-sheet"
      role="dialog"
      aria-modal="true"
      aria-label="Attachment preview">
      {#if previewItem.kind === "image" && cropping}
        <ImageCropper
          file={previewItem.file}
          onapply={(file) => void replacePending(previewItem!.id, file)}
          oncancel={() => (cropping = false)} />
      {:else if previewItem.kind === "image"}
        <img class="preview-large" src={previewItem.url} alt={previewItem.file.name} />
        <button class="ghost crop-button" onclick={() => (cropping = true)}>Crop or resize</button>
      {:else if previewItem.kind === "video"}
        <div class="preview-video">
          <VideoPlayer src={previewItem.url} autoplay={false} />
        </div>
      {:else}
        <span class="file-icon large"><Icon name="file" size={56} /></span>
      {/if}
      <span class="pending-name">{previewItem.file.name}</span>
      <input
        class="caption"
        value={previewItem.caption}
        oninput={(e) => previewItem && (previewItem.caption = e.currentTarget.value)}
        placeholder="Add a caption"
        autocomplete="off"
      />
      <button class="primary" onclick={() => (previewId = null)}>Done</button>
    </div>
  </div>
{/if}

{#if pendingJump}
  <div class="notice">
    <span class="spinner"></span>
    <span>Fetching older messages from your phone to find it…</span>
  </div>
{/if}

{#if notice}
  <div class="notice">
    <span>{notice}</span>
    <button class="link" onclick={muteNotice}>Do not warn again</button>
    <button class="icon" title="Dismiss" aria-label="Dismiss" onclick={() => (notice = null)}>
      <Icon name="x" size={16} />
    </button>
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
  .app > .pairing {
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
  .error .icon {
    color: var(--muted);
  }
  .pairing {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 22px;
    padding: 32px 24px;
    box-sizing: border-box;
    overflow: auto;
    background: var(--chat-bg);
  }
  .intro-glow {
    position: absolute;
    inset: 0;
    pointer-events: none;
    background:
      radial-gradient(60% 50% at 15% 10%, var(--accent-soft), transparent 70%),
      radial-gradient(50% 40% at 90% 90%, color-mix(in srgb, var(--link) 12%, transparent), transparent 70%);
  }
  .intro-head,
  .intro-card,
  .intro-foot {
    position: relative;
    box-sizing: border-box;
    width: min(920px, 100%);
    flex: none;
  }
  .intro-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .intro-logo {
    width: 48px;
    height: 48px;
    border-radius: 12px;
  }
  .intro-head h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.01em;
  }
  .intro-tag {
    color: var(--muted);
    font-size: 13.5px;
  }
  .intro-settings {
    margin-left: auto;
  }
  .intro-card {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 40px;
    padding: 40px 44px;
    border: 1px solid var(--line-strong);
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
    animation: intro-in calc(0.35s * var(--motion-scale)) var(--ease) both;
  }
  @keyframes intro-in {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
  @media (max-width: 760px) {
    .intro-card {
      grid-template-columns: 1fr;
      padding: 28px 22px;
    }
  }
  .intro-card.resume {
    grid-template-columns: 1fr;
    justify-items: center;
    gap: 10px;
    width: min(460px, 100%);
    text-align: center;
  }
  .intro-card.resume h2 {
    margin: 8px 0 0;
    font-size: 22px;
    font-weight: 500;
  }
  .resume-avatar {
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
  .resume-who {
    color: var(--muted);
    font-size: 13.5px;
  }
  .account-choices {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    margin-top: 16px;
  }
  .account-choice {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition:
      background calc(0.15s * var(--motion-scale)),
      border-color calc(0.15s * var(--motion-scale)),
      transform calc(0.15s * var(--motion-scale));
  }
  .account-choice:hover {
    background: var(--raised-2);
    border-color: var(--accent);
  }
  .account-choice:active {
    transform: scale(0.99);
  }
  .choice-avatar {
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
  .choice-text {
    flex: 1;
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .choice-text small {
    color: var(--muted);
  }
  .resume-progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(320px, 100%);
    margin-top: 18px;
  }
  .resume-status {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .resume-count {
    font-variant-numeric: tabular-nums;
  }
  .resume-bar {
    height: 6px;
    border-radius: 999px;
    background: var(--raised);
    overflow: hidden;
  }
  .resume-bar span {
    display: block;
    width: 40%;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    animation: indeterminate 1.2s ease-in-out infinite;
  }
  .resume-bar.determinate span {
    animation: none;
    transition: width calc(0.25s * var(--motion-scale)) var(--ease);
  }
  @keyframes indeterminate {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(250%);
    }
  }
  .intro-card.resume .account-bar {
    margin-top: 14px;
    justify-content: center;
  }
  .intro-card.resume .account {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .intro-card.resume .account img {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    object-fit: cover;
  }
  .intro-steps h2 {
    margin: 0 0 18px;
    font-size: 24px;
    font-weight: 400;
  }
  .intro-steps ol {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
    font-size: 15px;
    line-height: 1.45;
  }
  .intro-steps li {
    display: flex;
    gap: 12px;
    align-items: center;
  }
  .intro-steps .num {
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    border: 1px solid var(--line-strong);
    color: var(--muted);
    font-size: 12.5px;
  }
  .intro-progress {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 18px;
    margin-top: 26px;
    font-size: 12.5px;
    color: var(--faint);
  }
  .stage {
    display: flex;
    align-items: center;
    gap: 7px;
  }
  .stage-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--line-strong);
  }
  .stage.current {
    color: var(--text);
  }
  .stage.current .stage-dot {
    background: var(--accent);
    box-shadow: 0 0 0 4px var(--accent-soft);
    animation: blink-dot 1.4s ease-in-out infinite;
  }
  .stage.done {
    color: var(--muted);
  }
  .stage.done .stage-dot {
    background: var(--accent);
  }
  @keyframes blink-dot {
    50% {
      box-shadow: 0 0 0 7px transparent;
    }
  }
  .intro-accounts {
    margin-top: 26px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .intro-label {
    font-size: 12px;
    color: var(--faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .intro-accounts .account-bar {
    justify-content: flex-start;
    flex-wrap: wrap;
  }
  .intro-accounts .account {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .intro-accounts .account img,
  .account-initial {
    width: 22px;
    height: 22px;
    border-radius: 50%;
    object-fit: cover;
  }
  .account-initial {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    font-size: 10px;
  }
  .intro-code {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    text-align: center;
    max-width: 280px;
  }
  .intro-foot {
    margin: 0;
    color: var(--faint);
    font-size: 12.5px;
    text-align: center;
  }
  .lede {
    margin: 0;
    color: var(--muted);
    max-width: 44ch;
    line-height: 1.5;
  }
  .hint {
    margin: 0;
    color: var(--faint);
    font-size: 12px;
    max-width: 44ch;
    text-wrap: balance;
  }
  .qr {
    position: relative;
    background: var(--bg);
    padding: 12px;
    border-radius: 12px;
    line-height: 0;
  }
  .qr-logo {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 44px;
    height: 44px;
    transform: translate(-50%, -50%);
    border-radius: 10px;
    border: 4px solid var(--bg);
    background: var(--bg);
  }
  .qr-loading,
  .qr-idle {
    display: grid;
    place-items: center;
    width: 264px;
    height: 264px;
    box-sizing: border-box;
    color: var(--faint);
  }
  .qr-loading {
    background: linear-gradient(100deg, var(--bg) 40%, var(--raised) 50%, var(--bg) 60%) 0 0 / 300% 100%;
    animation: shimmer 1.4s linear infinite;
  }
  @keyframes shimmer {
    to {
      background-position: -150% 0;
    }
  }
  .layout {
    display: grid;
    grid-template-columns: 300px 1fr;
    overflow: hidden;
  }
  .resizer {
    border: 0;
    background: transparent;
    padding: 0;
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: col-resize;
    z-index: 5;
  }
  .chats .resizer {
    display: block;
    right: -3px;
  }
  .chat-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .header-tools {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    flex: none;
  }
  .badge-host {
    position: relative;
  }
  .icon-badge {
    position: absolute;
    top: 2px;
    right: 0;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: 999px;
    background: var(--mention);
    color: var(--accent-ink);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
    pointer-events: none;
  }
  .heading-avatar {
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .heading-avatar:hover {
    filter: brightness(1.12);
  }
  .chat-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: transparent;
    border: 0;
    color: inherit;
    font: inherit;
    font-weight: 600;
    padding: 0;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button.chat-title {
    cursor: pointer;
  }
  button.chat-title:hover .chat-sub {
    color: var(--accent-text);
  }
  .chat-sub {
    font-size: 13px;
    font-weight: 400;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chat-title {
    font-size: 16px;
    font-weight: 400;
  }
  .avatar {
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
  img.avatar {
    object-fit: cover;
    background: var(--raised);
  }
  .conversation header .avatar {
    width: 40px;
    height: 40px;
    font-size: 14px;
  }
  .link {
    color: var(--link);
    cursor: pointer;
  }
  .embed {
    display: flex;
    gap: 16px;
    max-width: 432px;
    margin-top: 4px;
    padding: 10px 14px 14px 12px;
    box-sizing: border-box;
    background: rgba(0, 0, 0, 0.18);
    border-left: 4px solid var(--embed-color, var(--accent));
    border-radius: 4px;
  }
  .embed.wide {
    flex-direction: column;
    gap: 10px;
  }
  .embed-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .embed-provider {
    font-size: 12px;
    color: var(--muted);
  }
  .embed-title {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--link);
    font: inherit;
    font-size: 15px;
    font-weight: 600;
    line-height: 20px;
    text-align: left;
    cursor: pointer;
  }
  .embed-title:hover {
    text-decoration: underline;
  }
  .embed-desc {
    font-size: 13.5px;
    line-height: 18px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .embed-image {
    flex: none;
    align-self: flex-start;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    overflow: hidden;
    cursor: pointer;
  }
  .embed-image img {
    display: block;
    width: 80px;
    height: 80px;
    object-fit: cover;
  }
  .embed.wide .embed-image img {
    width: auto;
    height: auto;
    max-width: 100%;
    max-height: 300px;
  }
  /* Keep the spaces the sender typed, and wrap long tokens. */
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .inline-code,
  .pre {
    font-family: ui-monospace, "Cascadia Code", Consolas, monospace;
    font-size: 0.92em;
  }
  .inline-code {
    padding: 1px 4px;
    border-radius: 4px;
    background: color-mix(in srgb, var(--text) 10%, transparent);
  }
  .mention-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 5px 0 2px;
    border-radius: 4px;
    vertical-align: bottom;
    font-weight: 500;
    color: var(--mention-pill);
    background: var(--mention-pill-soft);
    white-space: nowrap;
    border: 0;
    font: inherit;
    line-height: inherit;
    cursor: pointer;
  }
  .mention-pill:hover {
    text-decoration: underline;
  }
  .mention-pill.self {
    color: var(--mention);
    background: var(--mention-self-soft);
  }
  .mention-pill img,
  .mention-initials {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    flex: none;
    object-fit: cover;
  }
  .mention-initials {
    display: grid;
    place-items: center;
    font-size: 8px;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
  }
  .pre {
    display: block;
    margin: 2px 0;
    padding: 6px 8px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--text) 8%, transparent);
    white-space: pre-wrap;
  }
  .quote-block {
    display: block;
    margin: 2px 0;
    padding-left: 8px;
    border-left: 3px solid color-mix(in srgb, var(--text) 30%, transparent);
    color: var(--muted);
  }
  .fmt-list {
    margin: 2px 0;
    padding-left: 20px;
    white-space: normal;
  }
  .chats {
    position: relative;
    overflow: hidden;
    background: var(--bg);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .chats header,
  .conversation header {
    min-width: 0;
    height: 59px;
    box-sizing: border-box;
    flex: none;
    overflow: hidden;
    padding: 0 12px 0 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .chats header {
    height: 64px;
  }
  .conversation header {
    background: var(--surface);
  }
  .title {
    margin: 0;
    font-size: 22px;
    font-weight: 700;
  }
  .account-bar {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    overflow: hidden;
  }
  .account {
    background: transparent;
    border: 0;
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account:hover {
    color: var(--text);
  }
  .account.active {
    background: var(--raised);
    color: var(--text);
    font-weight: 600;
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
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px 8px;
    padding: 0 12px;
    height: 36px;
    flex: none;
    background: var(--surface);
    border: 1px solid transparent;
    border-radius: 999px;
    color: var(--faint);
    cursor: text;
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    outline: none;
    padding: 0;
    color: var(--text);
    font: inherit;
  }
  .search input:focus-visible {
    box-shadow: none;
  }
  .search input::placeholder {
    color: var(--muted);
  }
  .filters {
    display: flex;
    gap: 8px;
    padding: 0 12px 8px;
    flex: none;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--surface);
    border: 0;
    border-radius: 999px;
    color: var(--muted);
    font: inherit;
    font-size: 14px;
    padding: 5px 12px;
    cursor: pointer;
  }
  .chip:hover {
    background: var(--raised);
  }
  .chip.active {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .chip-count {
    font-size: 12px;
  }
  .results .preview {
    color: var(--faint);
  }
  .chats ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
  }
  .chat-row {
    position: relative;
    box-sizing: border-box;
    width: 100%;
    height: 72px;
    min-width: 0;
    overflow: hidden;
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-areas: "avatar name time" "avatar preview badge";
    gap: 2px 15px;
    text-align: left;
    background: transparent;
    color: inherit;
    border: 0;
    padding: 0 15px 0 13px;
    cursor: pointer;
    font: inherit;
    align-items: center;
    align-content: center;
  }
  /* The divider starts after the avatar, as in WhatsApp. */
  .chat-row::after {
    content: "";
    position: absolute;
    left: 77px;
    right: 0;
    bottom: 0;
    border-bottom: 1px solid var(--line);
  }
  .chat-row:hover {
    background: var(--surface);
  }
  .chat-row.active {
    background: var(--raised);
  }
  .chat-row:focus-visible {
    outline-offset: -2px;
  }
  .badges {
    grid-area: badge;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .pin {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 4px;
    color: var(--faint);
  }
  .badge.mention-badge {
    background: var(--accent-soft);
    color: var(--accent-text);
    border: 0;
    cursor: pointer;
    font: inherit;
    font-size: 11px;
    font-weight: 700;
  }
  .pin-toggle {
    display: none;
    background: transparent;
    border: 0;
    color: var(--faint);
    padding: 2px;
    border-radius: 4px;
    cursor: pointer;
  }
  .pin-toggle:hover {
    color: var(--text);
  }
  .chat-row:hover .pin-toggle,
  .pin-toggle:focus-visible {
    display: block;
  }
  .jump-mention {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--accent-soft);
    color: var(--accent-text);
    border: 0;
    border-radius: 999px;
    padding: 4px 10px;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .name {
    grid-area: name;
    min-width: 0;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    grid-area: time;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .time.unread {
    color: var(--accent);
  }
  .chat-row .name {
    font-size: 17px;
    font-weight: 400;
  }
  .chat-row .preview {
    font-size: 14px;
    color: var(--muted);
  }
  .preview.typing,
  .chat-sub.typing {
    color: var(--accent);
  }
  .preview-icon {
    display: inline-flex;
    vertical-align: -2px;
    margin-right: 4px;
  }
  .messages.group {
    --pad-l: max(56px, 7%);
  }
  /* Beside the first bubble of a run, outside the tail. */
  .sender-avatar {
    position: absolute;
    left: -38px;
    top: 0;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .sender-avatar .avatar {
    width: 28px;
    height: 28px;
    font-size: 11px;
  }
  .preview {
    grid-area: preview;
    min-width: 0;
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    border-radius: 999px;
    height: 20px;
    padding: 0 6px;
    min-width: 20px;
    box-sizing: border-box;
  }
  .empty {
    padding: 16px 10px;
    color: var(--faint);
  }
  .user-panel {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 62px;
    box-sizing: border-box;
    padding: 0 8px;
    background: var(--surface);
  }
  .user-panel .me {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .user-panel .me:hover,
  .user-panel .me[aria-expanded="true"] {
    background: var(--raised);
  }
  .me-avatar-wrap {
    position: relative;
    flex: none;
  }
  .me-avatar {
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
  .presence {
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--faint);
    box-shadow: 0 0 0 3px var(--surface);
  }
  .presence.online {
    background: var(--accent);
  }
  /* Half-lit: online, but only contacts can see it. */
  .presence.contacts {
    background: linear-gradient(90deg, var(--accent) 50%, var(--faint) 50%);
  }
  /* Discord's invisible: a hollow grey ring. */
  .presence.invisible {
    background: var(--surface);
    border: 3px solid var(--muted);
    box-sizing: border-box;
  }
  .me-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .me-name {
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .me-status {
    font-size: 12px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-menu {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: calc(100% + 6px);
    z-index: 20;
    display: flex;
    flex-direction: column;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .menu-label {
    padding: 6px 8px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .menu-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--text);
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
  }
  .menu-item:hover {
    background: var(--raised);
  }
  .menu-avatar {
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
  .menu-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .menu-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 2px solid var(--muted);
    box-sizing: border-box;
  }
  .menu-item.current .menu-dot {
    background: var(--accent);
    border-color: var(--accent);
  }
  .menu-sep {
    height: 1px;
    margin: 6px 4px;
    background: var(--line-strong);
  }
  .account.add {
    display: grid;
    place-items: center;
  }
  .conversation {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    position: relative;
    background: var(--chat-bg);
  }
  .day {
    display: flex;
    justify-content: center;
    margin: 12px 0 8px;
  }
  .day span {
    background: var(--surface);
    color: var(--muted);
    font-size: 12.5px;
    padding: 5px 12px;
    border-radius: var(--radius-sm);
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
  }
  .load-older {
    align-self: center;
    background: var(--surface);
    border: 0;
    border-radius: var(--radius-sm);
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    padding: 5px 12px;
    margin-bottom: 4px;
    cursor: pointer;
  }
  .load-older:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }
  .load-older:disabled {
    cursor: progress;
    opacity: 0.7;
  }
  .messages {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    min-width: 0;
    /* Rows carry the side padding so their highlight spans the full width. */
    --pad-l: clamp(16px, 7%, 90px);
    --pad-r: clamp(16px, 7%, 90px);
    padding: 12px 0 8px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    transition:
      opacity calc(0.22s * var(--motion-scale)) var(--ease),
      transform calc(0.22s * var(--motion-scale)) var(--ease);
  }
  .messages.switching {
    opacity: 0;
    transform: translateY(8px);
    transition-duration: calc(0.08s * var(--motion-scale));
  }
  @media (prefers-reduced-motion: reduce) {
    .messages,
    .messages.switching {
      transition: none;
      transform: none;
    }
  }
  .messages > .bubble {
    margin-left: var(--pad-l);
    margin-right: var(--pad-r);
  }
  .msg-row {
    display: flex;
    flex-direction: column;
    padding: 1px var(--pad-r) 1px var(--pad-l);
    transition: background-color calc(0.6s * var(--motion-scale)) var(--ease);
  }
  .msg-row:hover {
    background: var(--row-hover);
    transition-duration: calc(0.15s * var(--motion-scale));
  }
  /* The message a reply is being drafted to. */
  .msg-row.replying {
    background: var(--replying-soft);
    box-shadow: inset 3px 0 0 var(--replying);
  }
  /* Mentions of us and replies to us, as Discord marks them. */
  .msg-row.for-me {
    background: var(--mention-soft);
    box-shadow: inset 3px 0 0 var(--mention);
  }
  .msg-row.for-me:hover {
    background: color-mix(in srgb, var(--mention-soft), var(--row-hover));
  }
  /* The message a quote or mention jump landed on. */
  .msg-row.jumped {
    background: var(--jump-soft);
    transition-duration: calc(0.15s * var(--motion-scale));
  }
  .bubble {
    max-width: 70%;
    /* Without this a flex item refuses to shrink below its content, so a large
       image stretches the bubble instead of being scaled down to fit it. */
    min-width: 0;
    /* The message list is a flex column, so bubbles shrink by default. With a
       few hundred of them they squash to one line and `overflow: hidden` clips
       the text away, which looks like every message collapsing. */
    flex-shrink: 0;
    align-self: flex-start;
    position: relative;
    max-width: 65%;
    background: var(--bubble);
    border-radius: var(--radius-sm);
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
    padding: 6px 7px 8px 9px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    line-height: 19px;
    word-break: break-word;
    overflow-wrap: anywhere;
  }
  .bubble.first {
    margin-top: 10px;
  }
  .bubble.mine {
    align-self: flex-end;
    background: var(--bubble-mine);
  }
  /* The tail marks the first bubble of a run from one sender. */
  .bubble.first:not(.mine) {
    border-top-left-radius: 0;
  }
  .bubble.first.mine {
    border-top-right-radius: 0;
  }
  /* 1px wider than it shows and tucked into the bubble, so no seam renders
     where the two meet. */
  .bubble.first::before {
    content: "";
    position: absolute;
    top: 0;
    width: 9px;
    height: 13px;
    background: inherit;
  }
  .bubble.first:not(.mine)::before {
    left: -8px;
    clip-path: polygon(0 0, 100% 0, 100% 100%);
  }
  .bubble.first.mine::before {
    right: -8px;
    clip-path: polygon(0 0, 100% 0, 0 100%);
  }
  .bubble.media-only {
    padding: 3px;
  }
  .bubble.media-only .media,
  .bubble.media-only .media-button.video {
    border-radius: calc(var(--radius-sm) - 2px);
  }
  .bubble.inline-meta .meta {
    position: absolute;
    right: 7px;
    bottom: 4px;
  }
  /* Reserves room on the last line so the time never covers text. */
  .meta-spacer {
    display: inline-block;
    width: 44px;
    height: 1px;
  }
  .meta-spacer.mine {
    width: 62px;
  }
  .bubble.edited .meta-spacer {
    width: 86px;
  }
  .bubble.edited .meta-spacer.mine {
    width: 104px;
  }
  .edited-mark {
    font-style: italic;
  }
  .forwarded-mark {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 2px;
    color: var(--muted);
    font-size: 12.5px;
    font-style: italic;
  }
  /* The time sits on the picture instead of below it. */
  .bubble.media-only .meta {
    position: absolute;
    right: 9px;
    bottom: 8px;
    padding: 1px 7px;
    border-radius: 999px;
    background: rgba(6, 8, 10, 0.55);
    color: #eef0f2;
  }
  .sender {
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
  button.sender {
    cursor: pointer;
  }
  button.sender:hover {
    text-decoration: underline;
  }
  .member-label {
    margin-top: -4px;
    font-size: 12px;
    color: var(--muted);
  }
  .revoked {
    font-style: italic;
    color: var(--faint);
  }
  .quote {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: "author thumb" "text thumb";
    align-items: center;
    column-gap: 8px;
    background: rgba(0, 0, 0, 0.18);
    border: 0;
    border-left: 4px solid var(--accent);
    border-radius: 6px;
    font: inherit;
    text-align: left;
    cursor: pointer;
    font-size: 13px;
    line-height: 18px;
    color: var(--muted);
    padding: 5px 8px 6px;
    margin-bottom: 2px;
    overflow: hidden;
    min-width: 0;
    max-width: 100%;
  }
  .quote:hover {
    background: rgba(0, 0, 0, 0.26);
  }
  .media {
    /* Cap both axes: width keeps it inside the bubble, height stops a tall
       photo from filling the viewport. */
    max-width: 100%;
    max-height: 320px;
    width: auto;
    height: auto;
    object-fit: contain;
    border-radius: 6px;
    display: block;
  }
  .voice-pending {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 280px;
    max-width: 100%;
    padding: 6px 2px;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .voice-pending:disabled {
    cursor: progress;
  }
  .voice-pending-icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex: none;
    border-radius: 50%;
    border: 2px solid var(--accent);
    color: var(--accent);
  }
  .voice-pending:hover:not(:disabled) .voice-pending-icon {
    background: var(--accent-soft);
  }
  .voice-pending-bars {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 24px;
  }
  .voice-pending-bars span {
    flex: 1;
    border-radius: 2px;
    background: color-mix(in srgb, var(--text) 22%, transparent);
  }
  .once {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 200px;
    padding: 6px 4px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .once > span:last-child {
    display: flex;
    flex-direction: column;
  }
  .once small {
    color: var(--muted);
    font-size: 12px;
  }
  .once.spent {
    cursor: default;
    color: var(--muted);
  }
  .once-mark {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border: 2px dashed var(--accent);
    border-radius: 50%;
    color: var(--accent);
    font-size: 12px;
    font-weight: 700;
  }
  .once.spent .once-mark {
    border-color: var(--faint);
    color: var(--faint);
  }
  .once-done {
    align-self: flex-end;
    padding: 4px 12px;
    border: 0;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(0, 0, 0, 0.18);
    border: 0;
    border-radius: var(--radius-sm);
    padding: 8px 12px 8px 10px;
    font: inherit;
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  .file:hover {
    background: rgba(0, 0, 0, 0.28);
  }
  .file :global(svg) {
    color: var(--accent-text);
  }
  /* Media opens in the system viewer, so the whole preview is the button. */
  .media-button {
    position: relative;
    display: block;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: zoom-in;
    line-height: 0;
  }
  .media-button.video {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 14px 16px;
    cursor: pointer;
    background: var(--bg);
    border-radius: 6px;
    min-width: 180px;
    min-height: 100px;
  }
  .media-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text);
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 1px;
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }
  .media-overlay .play {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding-left: 3px;
    box-sizing: border-box;
    border-radius: 999px;
    background: rgba(6, 8, 10, 0.6);
    font-size: 16px;
    text-shadow: none;
  }
  .play-badge {
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
  .upload-visual {
    position: relative;
    display: grid;
    place-items: center;
    border-radius: 8px;
    overflow: hidden;
  }
  .upload-visual .media {
    display: block;
    max-width: 280px;
    max-height: 320px;
    filter: brightness(0.7);
  }
  .upload-visual.file-upload {
    min-width: 220px;
    min-height: 72px;
    gap: 8px;
    padding: 10px;
  }
  .upload-name {
    display: flex;
    align-items: center;
    gap: 8px;
    max-width: 240px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .upload-ring {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
  }
  .file-upload .upload-ring {
    position: static;
  }
  .upload-ring svg {
    grid-area: 1 / 1;
    transform: rotate(-90deg);
    border-radius: 50%;
    background: var(--scrim);
  }
  .upload-ring circle {
    fill: none;
    stroke-width: 3.5;
  }
  .ring-track {
    stroke: rgba(255, 255, 255, 0.2);
  }
  .ring-fill {
    stroke: #fff;
    stroke-linecap: round;
    transition: stroke-dashoffset calc(0.25s * var(--motion-scale)) linear;
  }
  .ring-fill.spinning {
    transform-origin: 24px 24px;
    animation: spin 0.9s linear infinite;
  }
  .ring-label {
    grid-area: 1 / 1;
    color: #fff;
    font-size: 11px;
    font-weight: 600;
  }
  .upload-caption {
    display: block;
    padding: 6px 4px 2px;
  }
  .svg-file .media {
    width: 280px;
    max-width: 100%;
    max-height: 320px;
    object-fit: contain;
    padding: 8px;
    box-sizing: border-box;
    background: repeating-conic-gradient(var(--raised) 0 25%, var(--raised-2) 0 50%) 0 0 / 16px 16px;
  }
  .media-fetch {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: var(--scrim);
    color: #fff;
    font-size: 0;
    text-shadow: none;
  }
  .media-stub {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 260px;
    max-width: 100%;
    aspect-ratio: 4 / 3;
    border: 0;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--raised), var(--raised-2));
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .media-stub:hover .media-fetch {
    background: var(--accent);
  }
  .download {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(0, 0, 0, 0.2);
    border: 0;
    border-radius: 999px;
    color: var(--accent-text);
    font: inherit;
    font-size: 12px;
    padding: 4px 12px 4px 10px;
    cursor: pointer;
  }
  .download:hover {
    background: rgba(0, 0, 0, 0.32);
  }
  .file-icon {
    color: var(--muted);
  }
  .meta {
    font-size: 11px;
    line-height: 15px;
    font-variant-numeric: tabular-nums;
    color: color-mix(in srgb, var(--text) 60%, transparent);
    align-self: flex-end;
    display: flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
  }
  .reply-btn {
    position: absolute;
    top: 3px;
    right: 3px;
    z-index: 1;
    display: flex;
    padding: 3px;
    background: inherit;
    border: 0;
    border-radius: 999px;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .bubble.mine .reply-btn {
    background: var(--bubble-mine);
  }
  .bubble:not(.mine) .reply-btn {
    background: var(--bubble);
  }
  .bubble:hover .reply-btn,
  .bubble.menu-open .reply-btn,
  .reply-btn:focus-visible {
    opacity: 1;
  }
  .bubble.has-reactions {
    margin-bottom: 16px;
  }
  .sticker {
    width: 160px;
    height: 160px;
    object-fit: contain;
    display: block;
  }
  .sticker-pending {
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 18px;
    color: var(--faint);
    cursor: pointer;
    background: linear-gradient(100deg, var(--surface) 40%, var(--raised) 50%, var(--surface) 60%) 0 0 / 300% 100%;
    animation: shimmer 1.4s linear infinite;
  }
  /* Stickers float free of a bubble, as in WhatsApp. */
  .bubble.sticker-only {
    background: transparent;
    box-shadow: none;
    padding: 0;
  }
  .bubble.sticker-only::before {
    display: none;
  }
  .bubble.sticker-only .meta {
    align-self: flex-end;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--surface);
  }
  .composer-area {
    position: relative;
    flex: none;
  }
  .attach.active {
    color: var(--accent);
  }
  .attach-catcher {
    position: fixed;
    inset: 0;
    z-index: 55;
  }
  .attach-menu {
    position: absolute;
    left: 10px;
    bottom: calc(100% + 6px);
    z-index: 56;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .attach-menu button {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 9px 10px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    text-align: left;
    cursor: pointer;
  }
  .attach-menu button :global(svg) {
    color: var(--accent);
  }
  .attach-menu button:hover {
    background: var(--raised);
  }
  .composer-tools {
    display: flex;
    align-items: center;
    align-self: center;
    gap: 2px;
  }
  .composer-tools .icon {
    width: 38px;
    height: 38px;
  }
  .composer-tools .icon.active {
    color: var(--accent);
  }
  .once-toggle {
    width: 24px;
    height: 24px;
    border: 2px dashed currentColor;
    border-radius: 50%;
    font: inherit;
    font-size: 11px;
    font-weight: 800;
  }
  .once-toggle.active {
    border-style: solid;
  }
  .tool-text {
    font: inherit;
    font-size: 11px;
    font-weight: 800;
    letter-spacing: 0.04em;
  }
  /* Discord-style completion list over the composer. */
  .suggest {
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: calc(100% + 6px);
    z-index: 50;
    display: flex;
    flex-direction: column;
    max-height: 360px;
    overflow-y: auto;
    padding: 8px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  .suggest-title {
    padding: 2px 8px 6px;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.02em;
    text-transform: uppercase;
    color: var(--muted);
  }
  .suggest-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    text-align: left;
    cursor: pointer;
  }
  .suggest-row.active {
    background: var(--raised);
  }
  .suggest-emoji {
    font-size: 20px;
    width: 26px;
    text-align: center;
  }
  /* WhatsApp's reaction pill, hanging off the bubble's bottom edge. */
  .reactions {
    position: absolute;
    bottom: -16px;
    left: 8px;
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 2px 6px;
    border: 1px solid var(--chat-bg);
    border-radius: 999px;
    background: var(--surface);
    font: inherit;
    font-size: 13px;
    line-height: 18px;
    color: var(--muted);
    cursor: pointer;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }
  .bubble.mine .reactions {
    left: auto;
    right: 8px;
  }
  .reaction-count {
    margin-left: 3px;
    font-size: 12px;
  }
  .star {
    display: inline-flex;
  }
  .pinned-bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    border: 0;
    border-top: 1px solid var(--line);
    background: var(--surface);
    color: var(--muted);
    font: inherit;
    font-size: 13.5px;
    text-align: left;
    cursor: pointer;
  }
  .pinned-bar:hover {
    background: var(--raised);
  }
  .pinned-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pinned-text strong {
    color: var(--text);
    font-weight: 600;
  }
  .sheet.confirm {
    width: min(400px, 90vw);
  }
  .sheet.confirm h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .confirm-actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .confirm-actions button {
    padding: 8px 14px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--accent);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .confirm-actions button:hover {
    background: var(--raised);
  }
  .confirm-actions button.danger {
    color: var(--danger);
  }
  .typing-bubble {
    padding: 8px 12px;
  }
  .dots {
    display: flex;
    gap: 4px;
    padding: 4px 2px;
  }
  .dots i {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    animation: blink 1.2s infinite ease-in-out;
  }
  .dots i:nth-child(2) {
    animation-delay: calc(0.15s * var(--motion-scale));
  }
  .dots i:nth-child(3) {
    animation-delay: calc(0.3s * var(--motion-scale));
  }
  @keyframes blink {
    0%,
    60%,
    100% {
      opacity: 0.35;
      transform: translateY(0);
    }
    30% {
      opacity: 1;
      transform: translateY(-3px);
    }
  }
  .recording {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    font-size: 13px;
  }
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
  .pending {
    display: flex;
    align-items: flex-end;
    flex-wrap: wrap;
    gap: 12px;
    padding: 8px 14px;
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .pending-item {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 120px;
  }
  .pending-thumb {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    width: 120px;
    height: 72px;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    cursor: pointer;
  }
  .pending-status {
    align-self: center;
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--muted);
    font-size: 13px;
  }
  .spinner {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid color-mix(in srgb, var(--accent) 30%, transparent);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  .send.ready {
    color: var(--accent);
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .pending-thumb img,
  .pending-thumb video {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .pending-name {
    font-size: 11px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pending-caption {
    font-size: 11px;
    color: var(--accent-text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pending .remove {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 20px;
    height: 20px;
    min-width: 0;
    min-height: 0;
    border-radius: 999px;
    background: var(--raised-2);
    color: var(--text);
    box-shadow: 0 0 0 2px var(--surface);
  }
  .caption {
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: 6px;
    padding: 8px 10px;
    color: inherit;
    font: inherit;
  }
  .preview-sheet {
    width: min(680px, 80vw);
  }
  .preview-large {
    max-width: 100%;
    max-height: 60vh;
    border-radius: 8px;
    object-fit: contain;
  }
  /* VideoPlayer sizes itself to a size container. */
  .preview-video {
    width: 100%;
    height: 50vh;
    container-type: size;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .crop-button {
    align-self: center;
  }
  .quote-author {
    grid-area: author;
    display: block;
    font-weight: 500;
    color: var(--accent);
  }
  .quote-thumb {
    grid-area: thumb;
    width: 42px;
    height: 42px;
    object-fit: cover;
    border-radius: 4px;
  }
  .quote-where {
    flex: none;
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: #71717a;
  }
  .quote-icon {
    grid-area: thumb;
    font-size: 14px;
  }
  .quote-text {
    grid-area: text;
    min-width: 0;
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ticks {
    font-size: 10px;
    letter-spacing: -2px;
  }
  .ticks.read {
    color: var(--link);
  }
  .reply-preview {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 14px;
    padding: 8px 8px 8px 12px;
    background: var(--surface);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .reply-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .reply-to {
    color: var(--accent-text);
    font-weight: 600;
  }
  .reply-body span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reply-thumb {
    width: 32px;
    height: 32px;
    object-fit: cover;
    border-radius: 4px;
    flex: none;
  }
  .reply-icon {
    font-size: 16px;
    flex: none;
  }
  .mentions {
    display: flex;
    flex-direction: column;
    max-height: 180px;
    overflow-y: auto;
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .mention {
    text-align: left;
    background: transparent;
    border: 0;
    color: inherit;
    font: inherit;
    padding: 7px 14px;
    cursor: pointer;
  }
  .mention.active,
  .mention:hover {
    background: var(--raised);
  }
  .mention-handle {
    margin-left: 6px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 5px 16px 5px 10px;
    min-height: 62px;
    box-sizing: border-box;
    background: var(--surface);
    flex: none;
  }
  .composer > input:not(.file-input),
  .composer > textarea {
    flex: 1;
    align-self: center;
    background: var(--raised);
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: 9px 12px;
    color: inherit;
    font: inherit;
  }
  .composer > textarea {
    resize: none;
    line-height: 1.4;
    field-sizing: content;
    box-sizing: border-box;
    max-height: 140px;
  }
  .composer > textarea::placeholder {
    color: var(--faint);
  }
  .file-input {
    display: none;
  }
  .attach,
  .send {
    width: 42px;
    height: 42px;
    margin-bottom: 5px;
  }
  .send {
    flex: none;
    display: grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .send:hover:not(:disabled) {
    color: var(--accent);
  }
  .send:disabled {
    color: var(--faint);
    cursor: default;
  }
  .primary {
    background: var(--accent);
    color: var(--accent-ink);
    border: 0;
    border-radius: var(--radius-sm);
    padding: 8px 16px;
    cursor: pointer;
    font: inherit;
    font-weight: 600;
  }
  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    min-width: 32px;
    min-height: 32px;
    padding: 0;
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--raised);
    color: var(--text);
  }
  .sheet-backdrop {
    position: fixed;
    inset: 0;
    z-index: 250;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 20px 22px;
    width: min(460px, 90vw);
    max-height: 86vh;
    overflow-y: auto;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
</style>
