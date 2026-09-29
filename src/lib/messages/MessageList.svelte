<!-- The open conversation's scrollback: day dividers, one bubble per message,
  outgoing uploads and the typing indicator. Moved out of +page.svelte. -->
<script lang="ts">
  import MessageBubble from "$lib/messages/MessageBubble.svelte";
  import type { BubbleApi, BubbleVm } from "$lib/utils/models";
  import OutgoingItem from "$lib/messages/OutgoingItem.svelte";
  import TypingIndicator from "$lib/media/TypingIndicator.svelte";
  import { bare } from "$lib/utils/message";
  import { noticeText } from "$lib/utils/notices";
  import { MAX_DOWNLOAD_TRIES } from "$lib/state/messages.svelte";
  import type {
    ChatEvent,
    MentionTarget,
    Outgoing,
    Poll,
    Reaction,
    StoredMessage,
  } from "$lib/utils/models";

  let {
    messages,
    isGroup,
    switching,
    scroller = $bindable(),
    dayKey,
    dayLabel,
    senderLabel,
    memberTagOf,
    hue,
    captionOf,
    viewOnceMarks,
    reactionsFor,
    starredSet,
    editedSet,
    forwardedSet,
    downloading,
    downloadErrors,
    downloadTries,
    replyingToId,
    highlightedId,
    firstUnreadId,
    onjumpunread,
    menuId,
    polls,
    events,
    namer,
    avatarOf,
    avatars,
    voiceAvatarOf,
    quoteAuthorOf,
    quoteTextOf,
    quoteChatNameOf,
    autoplayId,
    onceAudioOpenId,
    loadingOlder,
    atLatest = true,
    onloadnewer = () => {},
    onlatest = () => {},
    onrecoverquote,
    recovering,
    onloadolder,
    uploads,
    typers,
    typerLabelOf,
    onscroll,
    toWire,
    targetOf,
    onprofile,
    onopenurl,
    formatTime,
    onreplydraft,
    onmenu,
    onpick,
    picking = null,
    onjumpquoted,
    ondownload,
    onopenviewer,
    onopenmedia,
    onopenquote,
    onvote,
    onrespond,
    oneditrequest,
    oncancelevent,
    onreact,
    onopenreactions,
    onmarkplayed,
    onnextvoice,
    onpausevoice,
    onreplymenu,
    ononce,
    oncloseonce,
    revealedOnce,
    onrevealonce,
    oninviteopen,
  }: {
    /** Oldest first, the order the conversation is drawn in. */
    messages: StoredMessage[];
    isGroup: boolean;
    /** True while a newly opened chat's messages load, so the old ones fade out. */
    switching: boolean;
    scroller: HTMLDivElement | undefined;
    dayKey: (ts: number) => string;
    dayLabel: (ts: number) => string;
    senderLabel: (m: StoredMessage) => string;
    memberTagOf: (sender: string) => string | null;
    hue: (jid: string) => number;
    captionOf: (m: StoredMessage) => string;
    viewOnceMarks: { id: string; opened: boolean; available: boolean }[];
    reactionsFor: Map<string, Reaction[]>;
    starredSet: Set<string>;
    editedSet: Set<string>;
    forwardedSet: Set<string>;
    downloading: Record<string, true>;
    downloadErrors: Record<string, string>;
    downloadTries: Record<string, number>;
    replyingToId: string | null;
    highlightedId: string | null;
    /** Oldest unread message id; the divider is drawn above it. */
    firstUnreadId: string | null;
    onjumpunread: (id: string) => void;
    menuId: string | null;
    polls: Poll[];
    events: ChatEvent[];
    namer: (jid: string) => string;
    avatarOf: (jid: string) => string | null;
    avatars: Record<string, string | null>;
    voiceAvatarOf: (m: StoredMessage) => string | null;
    quoteAuthorOf: (sender: string | null) => string;
    quoteTextOf: (m: StoredMessage) => string | null;
    quoteChatNameOf: (m: StoredMessage) => string | null;
    autoplayId: string | null;
    onceAudioOpenId: string | null;
    loadingOlder: boolean;
    atLatest?: boolean;
    onloadnewer?: () => void;
    onlatest?: () => void;
    onloadolder: () => void;
    uploads: Outgoing[];
    typers: { sender: string; state: string }[];
    typerLabelOf: (sender: string) => string;
    onscroll: () => void;
    toWire: (text: string) => string;
    targetOf: (user: string) => MentionTarget;
    onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
    onopenurl: (url: string) => void;
    formatTime: (ts: number) => string;
    onreplydraft: (m: StoredMessage) => void;
    onmenu: (e: MouseEvent, m: StoredMessage) => void;
    onpick: (m: StoredMessage) => void;
    /** Messages picked for a bulk action, in the open chat; null when not picking. */
    picking?: Record<string, true> | null;
    onjumpquoted: (m: StoredMessage) => void;
    onrecoverquote: (m: StoredMessage) => void;
    recovering: Record<string, true>;
    ondownload: (m: StoredMessage) => void;
    onopenviewer: (m: StoredMessage) => void;
    onopenmedia: (path: string) => void;
    onopenquote: (m: StoredMessage) => void;
    onvote: (m: StoredMessage, options: string[]) => unknown;
    onrespond: (m: StoredMessage, response: string) => unknown;
    oneditrequest: (m: StoredMessage) => void;
    oncancelevent: (m: StoredMessage) => void;
    onreact: (m: StoredMessage, emoji: string) => void;
    onopenreactions: (m: StoredMessage) => void;
    onmarkplayed: (m: StoredMessage) => void;
    onnextvoice: (m: StoredMessage) => void;
    onpausevoice: () => void;
    onreplymenu: (e: MouseEvent, m: StoredMessage) => void;
    ononce: (m: StoredMessage) => void;
    oncloseonce: () => void;
    /** Kept one-time media whose filter was dismissed in this visit. */
    revealedOnce: Record<string, true>;
    onrevealonce: (m: StoredMessage) => void;
    oninviteopen: (jid: string) => void;
  } = $props();

  const api: BubbleApi = $derived({
    toWire,
    targetOf,
    avatarOf,
    onprofile,
    onopenurl,
    formatTime,
    namer,
    onreplydraft,
    onmenu,
    onpick,
    onjumpquoted,
    onrecoverquote,
    recovering,
    ondownload,
    onopenviewer,
    onopenmedia,
    onopenquote,
    onvote,
    onrespond,
    oneditrequest,
    oncancelevent,
    onreact,
    onopenreactions,
    onmarkplayed,
    onnextvoice,
    onpausevoice,
    onreplymenu,
    ononce,
    oncloseonce,
    onrevealonce,
    oninviteopen,
  });

  function vmFor(message: StoredMessage, i: number): BubbleVm {
    const prev = messages[i - 1];
    const newDay = !prev || dayKey(prev.timestamp) !== dayKey(message.timestamp);
    const first = newDay || prev.from_me !== message.from_me || prev.sender !== message.sender;
    const mark = viewOnceMarks.find((v) => v.id === message.id) ?? null;
    // A stub is spent unless a reply carried a copy of it, which the mark knows.
    const viewOnce =
      message.media_kind === "view_once"
        ? { id: message.id, opened: true, available: mark?.available ?? false }
        : mark;
    // A copy the Android companion kept: an ordinary kind plus the once marker,
    // behind a one-time filter until it is clicked in this visit to the chat.
    const onceKept =
      !!message.media_once_kind && message.media_kind !== "view_once" && !!message.media_path;
    // A revoked message keeps its local copy, so it still counts as drawable.
    const hasBody =
      !!message.text.trim() ||
      !!message.media_kind ||
      !!message.media_path ||
      !!message.media_thumb ||
      !!message.reply_to_text;
    const visual =
      !viewOnce &&
      (message.media_kind === "image" ||
        message.media_kind === "video" ||
        message.media_kind === "gif") &&
      !!(message.media_path || message.media_thumb);
    const caption = visual ? captionOf(message) : "";
    const showSender = first && !message.from_me && isGroup;
    const senderText = senderLabel(message);
    return {
      first,
      showSender,
      senderText,
      senderHue: hue(message.sender),
      senderAvatar: avatars[bare(message.sender)] ?? null,
      memberTag: memberTagOf(message.sender),
      visual,
      caption,
      viewOnce,
      onceKept,
      onceRevealed: !!revealedOnce[message.id],
      inlineMeta:
        (message.revoked && !hasBody) ||
        (!message.preview_url && (!message.media_kind || (!!caption && !!message.media_path))),
      reactions: reactionsFor.get(message.id),
      isStarred: starredSet.has(message.id),
      isEdited: editedSet.has(message.id),
      isForwarded: forwardedSet.has(message.id),
      isReplying: replyingToId === message.id,
      highlighted: highlightedId === message.id,
      forMe: !message.from_me && (message.mentioned || message.reply_to_sender === "@me"),
      menuOpen: menuId === message.id,
      poll: polls.find((p) => p.id === message.id),
      chatEvent: events.find((e) => e.id === message.id),
      downloading: !!downloading[message.id],
      downloadError: message.media_path ? null : (downloadErrors[message.id] ?? null),
      downloadGaveUp: (downloadTries[message.id] ?? 0) >= MAX_DOWNLOAD_TRIES,
      hasBody,
      picking: !!picking && !message.revoked,
      picked: !!picking?.[message.id],
      onceAudioOpen: onceAudioOpenId === message.id,
      autoplay: autoplayId === message.id,
      voiceAvatar: voiceAvatarOf(message),
      quoteAuthor: message.reply_to_text ? quoteAuthorOf(message.reply_to_sender) : null,
      quoteText: quoteTextOf(message),
      quoteChatName: quoteChatNameOf(message),
    };
  }

  const typerItems = $derived(
    typers.map((t) => ({ ...t, label: typerLabelOf(t.sender), hue: hue(t.sender) })),
  );
</script>

<div
  class="messages"
  class:switching
  class:group={isGroup}
  bind:this={scroller}
  onscroll={onscroll}>
  {#if messages.length > 0}
    <button class="load-older" onclick={onloadolder} disabled={loadingOlder}>
      {loadingOlder ? "Loading messages…" : "Load older messages"}
    </button>
    <p class="system e2e">
      Messages are end-to-end encrypted. No one outside of this chat, not even WhatsApp, can read or listen to them.
    </p>
  {/if}
  {#each messages as message, i (message.id)}
    {@const prev = messages[i - 1]}
    {@const newDay = !prev || dayKey(prev.timestamp) !== dayKey(message.timestamp)}
    {#if newDay}
      <div class="day"><span>{dayLabel(message.timestamp)}</span></div>
    {/if}
    {#if firstUnreadId === message.id}
      <button class="unread-divider" data-unread-divider onclick={() => onjumpunread(message.id)}>
        <span>Unread messages</span>
      </button>
    {/if}
    {#if message.system_kind}
      {@const line = noticeText(message.system_kind, message.system_params, namer)}
      {#if line}<p class="system">{line}</p>{/if}
    {:else}
      <MessageBubble {message} vm={vmFor(message, i)} {api} />
    {/if}
  {/each}
  {#each uploads as upload (upload.token)}
    <OutgoingItem {upload} />
  {/each}
  {#if !atLatest}
    <button class="load-older" onclick={onloadnewer} disabled={loadingOlder}>Load newer messages</button>
    <button class="load-older" onclick={onlatest}>Back to latest</button>
  {/if}
  {#if typers.length > 0}
    <TypingIndicator typers={typerItems} {isGroup} avatarOf={avatarOf} />
  {/if}
</div>

<style>
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
  .messages.group {
    --pad-l: max(56px, 7%);
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
  .unread-divider {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    margin: 10px 0 6px;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .unread-divider::before,
  .unread-divider::after {
    content: "";
    flex: 1;
    height: 1px;
    background: var(--accent-soft);
  }
  .unread-divider:hover span {
    text-decoration: underline;
  }
  .system {
    align-self: center;
    max-width: min(60ch, 80%);
    margin: 6px 0;
    padding: 5px 12px;
    background: var(--surface);
    color: var(--muted);
    border-radius: var(--radius-sm);
    font-size: 12.5px;
    text-align: center;
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
  }
  .system.e2e {
    color: var(--faint);
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
</style>
