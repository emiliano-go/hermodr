<!-- The open conversation's scrollback: day dividers, one bubble per message,
  outgoing uploads and the typing indicator. Only the rows in and around the
  viewport are mounted (virtua); everything else lives in SQLite behind the
  cursor pager. Moved out of +page.svelte. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { tick } from "svelte";
  import { VList, type VListHandle } from "virtua/svelte";
  import MessageRow from "$lib/messages/MessageRow.svelte";
  import AlbumGrid from "$lib/messages/AlbumGrid.svelte";
  import MessageQuote from "$lib/messages/cards/MessageQuote.svelte";
  import { albumTimeline } from "$lib/utils/album-timeline";
  import StructuredNotice from "$lib/messages/StructuredNotice.svelte";
  import { keywords } from "$lib/state/keywords.svelte";
  import type { BubbleApi, BubbleCtx } from "$lib/utils/models";
  import OutgoingItem from "$lib/messages/OutgoingItem.svelte";
  import TypingIndicator from "$lib/media/TypingIndicator.svelte";
  import { bare, isUnavailable } from "$lib/utils/message";
  import { unavailableLabel, unavailableExplanation } from "$lib/utils/notices";
  import { isPollNotice } from "$lib/utils/structured-notices";
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
    prepending = false,
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
    downloadDiagnostics = {},
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
    onrecoverquote,
    recovering,
    uploads,
    typers,
    typerLabelOf,
    onscroll,
    toWire,
    targetOf,
    onprofile,
    onopenurl,
    onopenchat,
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
    oninvitejoin,
  }: {
    /** Oldest first, the order the conversation is drawn in. */
    messages: StoredMessage[];
    isGroup: boolean;
    /** True while a newly opened chat's messages load, so the old ones fade out. */
    switching: boolean;
    scroller: HTMLDivElement | undefined;
    /** An older page just landed; the list keeps the reader's place. */
    prepending?: boolean;
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
    downloadDiagnostics?: Record<string, string>;
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
    uploads: Outgoing[];
    typers: { sender: string; state: string }[];
    typerLabelOf: (sender: string) => string;
    /** Scroll metrics from the virtual list, so the route can follow and page. */
    onscroll: (state: { offset: number; distance: number; viewport: number }) => void;
    toWire: (text: string) => string;
    targetOf: (user: string) => MentionTarget;
    onprofile: (jid: string, name: string, event: MouseEvent, self?: boolean) => void;
    onopenurl: (url: string) => void;
    onopenchat?: (chat: string) => void | Promise<void>;
    formatTime: (ts: number) => string;
    onreplydraft: (m: StoredMessage) => void;
    onmenu: (e: MouseEvent, m: StoredMessage) => void;
    onpick: (m: StoredMessage) => void;
    /** Messages picked for a bulk action, in the open chat; null when not picking. */
    picking?: Record<string, StoredMessage> | null;
    onjumpquoted: (m: StoredMessage) => void;
    onrecoverquote: (m: StoredMessage) => void;
    recovering: Record<string, true>;
    ondownload: (m: StoredMessage) => void;
    onopenviewer: (m: StoredMessage) => void;
    onopenmedia: (path: string) => void;
    onopenquote: (m: StoredMessage) => void;
    onvote: (m: StoredMessage, options: string[]) => unknown;
    onrespond: (m: StoredMessage, response: string, extraGuestCount?: number) => unknown;
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
    oninvitejoin: BubbleApi["oninvitejoin"];
  } = $props();

  const api: BubbleApi = $derived({
    toWire,
    targetOf,
    avatarOf,
    onprofile,
    onopenurl,
    onopenchat,
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
    oninvitejoin,
  });

  const ctx = $derived<BubbleCtx>({
    isGroup,
    picking,
    dayKey,
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
    downloadDiagnostics,
    downloadTries,
    replyingToId,
    highlightedId,
    menuId,
    polls,
    events,
    avatars,
    revealedOnce,
    voiceAvatarOf,
    quoteAuthorOf,
    quoteTextOf,
    quoteChatNameOf,
    autoplayId,
    onceAudioOpenId,
  });

  const typerItems = $derived(
    typers.map((t) => ({ ...t, label: typerLabelOf(t.sender), hue: hue(t.sender) })),
  );
  const visibleMessages = $derived(messages.filter((message) => !keywords.hidden(message)));
  const timeline = $derived(albumTimeline(messages, firstUnreadId, (message) => keywords.hidden(message), dayKey));

  type Vrow =
    | { kind: "e2e"; key: string }
    | { kind: "hidden"; key: string }
    | { kind: "day"; key: string; timestamp: number }
    | { kind: "unread"; key: string; id: string }
    | { kind: "group"; key: string; group: ReturnType<typeof albumTimeline>[number] }
    | { kind: "upload"; key: string; upload: Outgoing }
    | { kind: "typing"; key: string };

  /** One flat, oldest-first list: the virtualizer mounts only what is on screen. */
  const rows = $derived.by<Vrow[]>(() => {
    const out: Vrow[] = [];
    if (messages.length > 0) {
      out.push({ kind: "e2e", key: "e2e" });
      if (visibleMessages.length === 0) out.push({ kind: "hidden", key: "hidden" });
    }
    for (const group of timeline) {
      const message = group.messages[0], prev = group.prev;
      if (!prev || dayKey(prev.timestamp) !== dayKey(message.timestamp)) {
        out.push({ kind: "day", key: `day-${message.id}`, timestamp: message.timestamp });
      }
      if (group.unreadId && !isUnavailable(message)) {
        out.push({ kind: "unread", key: `unread-${group.unreadId}`, id: group.unreadId });
      }
      out.push({ kind: "group", key: JSON.stringify([message.chat, message.id]), group });
    }
    for (const upload of uploads) out.push({ kind: "upload", key: `upload-${upload.token}`, upload });
    if (typers.length > 0) out.push({ kind: "typing", key: "typing" });
    return out;
  });

  let list = $state<VListHandle>();
  let revealRequest = 0;
  const rowIndices = $derived.by(() => {
    const indices = new Map<string, number>();
    rows.forEach((row, index) => {
      if (row.kind !== "group") return;
      for (const message of row.group.messages) indices.set(message.id, index);
      for (const id of [...row.group.beforeIds, ...row.group.afterIds]) indices.set(id, index);
    });
    return indices;
  });
  function viewport() { return scroller?.querySelector<HTMLElement>(".message-viewport") ?? null; }

  export function visibleReadIds(): Set<string> {
    const element = viewport(), visible = new Set<string>();
    if (!element) return visible;
    const bottom = element.getBoundingClientRect().bottom;
    for (const marker of element.querySelectorAll<HTMLElement>(".bubble[data-id], .album-anchor[data-id]")) {
      if (marker.dataset.id && marker.getBoundingClientRect().top < bottom) visible.add(marker.dataset.id);
    }
    return visible;
  }

  function handleScroll(offset: number) {
    const size = list?.getScrollSize() ?? 0;
    const viewport = list?.getViewportSize() ?? 0;
    onscroll({ offset, distance: size - offset - viewport, viewport });
  }

  /** Keeps the newest row in view; used after sends and on follow. */
  export function scrollToBottom() {
    const last = rows.length - 1;
    if (last >= 0) list?.scrollToIndex(last, { align: "end" });
  }

  /** Scrolls the unread divider to the top, when the window still holds it. */
  export function scrollToUnread(): boolean {
    const index = rows.findIndex((row) => row.kind === "unread");
    if (index < 0) return false;
    list?.scrollToIndex(index, { align: "start" });
    return true;
  }

  export function hasMessage(id: string): boolean {
    return rowIndices.has(id);
  }

  /** Brings a loaded message into view; false when the window does not hold it. */
  export function revealMessage(id: string): boolean {
    const index = rowIndices.get(id);
    if (index === undefined) return false;
    const source = messages, request = ++revealRequest;
    list?.scrollToIndex(index, { align: "center" });
    void tick().then(() => requestAnimationFrame(() => {
      if (source !== messages || request !== revealRequest || !scroller?.isConnected) return;
      const target = [...(viewport()?.querySelectorAll<HTMLElement>("[data-id]") ?? [])].find((marker) => marker.dataset.id === id);
      target?.scrollIntoView({ block: "center" });
    }));
    return true;
  }

  /** The first row on screen, for restoring the reader's place across a reload. */
  export function anchorId(): string | null {
    const element = viewport();
    if (!element) return null;
    const bounds = element.getBoundingClientRect();
    let anchor: string | null = null, top = Infinity;
    for (const marker of element.querySelectorAll<HTMLElement>("[data-id]")) {
      const id = marker.dataset.id, rect = marker.getBoundingClientRect();
      if (id && rowIndices.has(id) && rect.bottom > bounds.top && rect.top < bounds.bottom && rect.top < top) { anchor = id; top = rect.top; }
    }
    return anchor;
  }

  // One capture listener for the list instead of one per row: picking and
  // ctrl-click intercept before any inner button sees the click.
  function captureClick(event: MouseEvent) {
    const row = (event.target as HTMLElement | null)?.closest?.(".msg-row");
    const id = row?.querySelector<HTMLElement>(".bubble[data-id]")?.dataset.id;
    if (!id) return;
    const message = messages.find((m) => m.id === id);
    if (!message || message.revoked) return;
    if (picking || event.ctrlKey || event.metaKey) {
      event.preventDefault();
      event.stopPropagation();
      onpick(message);
    }
  }
</script>

<div
  class="messages"
  class:switching={switching && messages.length > 0}
  class:group={isGroup}
  bind:this={scroller}
  onclickcapture={captureClick}>
  {#if switching && messages.length === 0}
    <p class="loading">{t("content.loading_messages")}</p>
  {/if}
  {#if loadingOlder}
    <p class="paging" role="status">{t("content.loading_messages")}</p>
  {/if}
  <VList
    bind:this={list}
    class="message-viewport"
    data={rows}
    getKey={(row) => row.key}
    shift={prepending}
    bufferSize={1200}
    ssrCount={30}
    onscroll={handleScroll}
    style="height: 100%;">
    {#snippet children(row: Vrow)}
      <div class="vrow">
        {#if row.kind === "e2e"}
          <p class="system e2e">
            {t("content.messages_are_end_to_end_encrypted_no_one_outside_of_this_chat_not_even_w")}
          </p>
        {:else if row.kind === "hidden"}
          <p class="system" role="status">{t("content.loaded_messages_are_hidden_by_your_keyword_rules")}</p>
        {:else if row.kind === "day"}
          <div class="day"><span>{dayLabel(row.timestamp)}</span></div>
        {:else if row.kind === "unread"}
          <button class="unread-divider" data-unread-divider onclick={() => onjumpunread(row.id)}>
            <span>{t("content.unread_messages")}</span>
          </button>
        {:else if row.kind === "group"}
          {@const group = row.group}
          {@const message = group.messages[0]}
          {@const prev = group.prev}
          {#each group.beforeIds as id (id)}<span class="album-anchor" data-id={id} aria-hidden="true"></span>{/each}
          {#if group.parentId}
            <AlbumGrid items={group.messages} {prev} timestamp={group.timestamp} {formatTime}>
              {#snippet header()}
                {#if group.parent?.reply_to_text}
                  <MessageQuote message={group.parent} author={ctx.quoteAuthorOf(group.parent.reply_to_sender)}
                    text={ctx.quoteTextOf(group.parent)} chatName={ctx.quoteChatNameOf(group.parent)} {api} />
                {/if}
              {/snippet}
              {#snippet children(child, previous)}
                <MessageRow message={child} prev={previous?.system_kind || (previous && isPollNotice(previous)) ? undefined : previous} {ctx} {api} albumCell />
              {/snippet}
            </AlbumGrid>
          {:else if isUnavailable(message)}
            <article class="unavailable-message" class:mine={message.from_me} data-id={message.id} data-chat={message.chat}>
              <header>
                <b>{message.from_me ? t("content.you") : senderLabel(message)}</b>
                <time datetime={new Date(message.timestamp * 1000).toISOString()}>{formatTime(message.timestamp)}</time>
              </header>
              <strong>{unavailableLabel()}</strong>
              <p>{unavailableExplanation()}</p>
            </article>
          {:else if message.system_kind || isPollNotice(message)}
            <StructuredNotice {message} poll={polls.find((poll) => poll.id === message.id)} {namer}
              picture={avatarOf} onvote={async (options) => { await onvote(message, options); }} highlighted={highlightedId === message.id || keywords.highlighted(message)} />
          {:else}
            <MessageRow
              {message}
              prev={prev?.system_kind || (prev && isPollNotice(prev)) ? undefined : prev}
              {ctx}
              {api} />
          {/if}
          {#each group.afterIds as id (id)}<span class="album-anchor" data-id={id} aria-hidden="true"></span>{/each}
        {:else if row.kind === "upload"}
          <OutgoingItem upload={row.upload} />
        {:else if row.kind === "typing"}
          <TypingIndicator typers={typerItems} {isGroup} avatarOf={avatarOf} />
        {/if}
      </div>
    {/snippet}
  </VList>
</div>

<style>
  .album-anchor { display: block; height: 0; }
  .messages {
    flex: 1;
    min-width: 0;
    min-height: 0;
    position: relative;
    /* Rows carry the side padding so their highlight spans the full width. */
    --pad-l: clamp(16px, 7%, 90px);
    --pad-r: clamp(16px, 7%, 90px);
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
  /* Every virtual row is its own flex column, as the old scroll container was. */
  .vrow {
    display: flex;
    flex-direction: column;
    width: 100%;
    padding-bottom: 2px;
  }
  .loading {
    margin: auto;
    color: var(--muted);
    font-size: 13px;
  }
  /* A pill over the list rather than a row in it: adding and removing a row at
     the top would shift the reader's place while a page is fetched. */
  .paging {
    position: absolute;
    top: 8px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 3;
    margin: 0;
    padding: 5px 12px;
    border-radius: var(--radius-sm);
    background: var(--surface);
    color: var(--muted);
    font-size: 12.5px;
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
    pointer-events: none;
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
  .unavailable-message { align-self: flex-start; max-width: min(60ch, calc(100% - var(--pad-l) - var(--pad-r))); box-sizing: border-box; margin: 6px var(--pad-r) 6px var(--pad-l); padding: 12px 14px; border: 1px solid var(--line-strong); border-radius: var(--radius); background: var(--surface); color: var(--text); overflow-wrap: anywhere; }
  .unavailable-message.mine { align-self: flex-end; }
  .unavailable-message header { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: 8px; margin-bottom: 8px; font-size: 0.9em; }
  .unavailable-message time { color: var(--muted); }
  .unavailable-message strong { display: block; }
  .unavailable-message p { margin: 5px 0 0; color: var(--muted); font-size: 0.95em; line-height: 1.45; }
</style>
