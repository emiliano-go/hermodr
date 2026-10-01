<script module lang="ts">
  import type { StoredMessage } from "$lib/utils/models";
  import { captionOf, isUnavailable, MEDIA_LABELS, CARD_LABELS } from "$lib/utils/message";
  import { plain } from "$lib/utils/format";
  import { UNAVAILABLE_LABEL, UNAVAILABLE_EXPLANATION } from "$lib/utils/notices";

  export function previewContent(message: StoredMessage) {
    if (isUnavailable(message)) return { text: `${UNAVAILABLE_LABEL}. ${UNAVAILABLE_EXPLANATION}`, media: null, notice: true };
    if (message.deleted || message.revoked) return { text: "Message deleted", media: null, notice: true };
    if (message.spoiler) return { text: "Spoiler", media: null, notice: true };
    if (message.media_kind === "view_once") return { text: "", media: "One-time media", notice: false };
    if (message.system_kind) return { text: "System notice", media: null, notice: true };
    const kind = message.media_kind;
    const media = kind ? MEDIA_LABELS[kind] ?? CARD_LABELS[kind] ?? (kind === "poll" ? "Poll" : kind === "event" ? "Event" : kind) : null;
    return { text: plain(captionOf(message)), media, notice: false };
  }
</script>

<script lang="ts">
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import type { MessagePage } from "$lib/utils/message-window";
  import { dayKey, dayLabel, formatTime } from "$lib/utils/message";
  import { displayName } from "$lib/utils/phone";

  let {
    chat, account, name, x, y,
    onpointerenter, onpointerleave, onfocusin, onfocusout, ondismiss, onopen, onsettings,
  }: {
    chat: string;
    account: string | null;
    name: string;
    x: number;
    y: number;
    onpointerenter?: (event: PointerEvent) => void;
    onpointerleave?: (event: PointerEvent) => void;
    onfocusin?: (event: FocusEvent) => void;
    onfocusout?: (event: FocusEvent) => void;
    ondismiss?: () => void;
    onopen?: (chat: string) => void;
    /** Opens Settings at the section with the preview toggle. */
    onsettings?: () => void;
  } = $props();
  let rows = $state<StoredMessage[] | null>(null);
  let error = $state<string | null>(null);
  let scroller = $state<HTMLElement>();
  let viewportWidth = $state(0);
  let viewportHeight = $state(0);

  $effect(() => {
    const jid = chat;
    const owner = account;
    rows = null;
    error = null;
    let active = true;
    const timer = setTimeout(async () => {
      if (!active || chat !== jid || account !== owner) return;
      try {
        const page = await invoke<MessagePage>("message_page", { chat: jid, limit: 30 });
        if (active && chat === jid && account === owner) rows = page.messages.toReversed();
      } catch (e) {
        if (active && chat === jid && account === owner) error = String(e);
      }
    }, 300);
    return () => { active = false; clearTimeout(timer); };
  });

  $effect(() => {
    if (rows && scroller) scroller.scrollTop = scroller.scrollHeight;
  });

</script>

<svelte:window bind:innerWidth={viewportWidth} bind:innerHeight={viewportHeight} />

  <div
    id="chat-preview"
    role="dialog"
    aria-modal="false"
    aria-label={`${name} message preview`}
    title="Open chat"
    tabindex="0"
    {onpointerenter}
    {onpointerleave}
    {onfocusin}
    {onfocusout}
    onkeydown={(event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        ondismiss?.();
      } else if (event.key === "Enter" && (event.target === event.currentTarget)) {
        event.preventDefault();
        onopen?.(chat);
      }
    }}
    onclick={() => onopen?.(chat)}
    style:left={`${Math.max(12, Math.min(x, viewportWidth - 412))}px`}
    style:top={`${Math.max(12, Math.min(y, viewportHeight - 532))}px`}>
    <header>
      <div class="titles">
        <strong>{name}</strong>
        <span>Read-only preview</span>
      </div>
      <button
        type="button"
        class="gear"
        title="Chat preview settings"
        aria-label="Chat preview settings"
        onclick={(e) => {
          e.stopPropagation();
          onsettings?.();
        }}><Icon name="settings" size={16} /></button>
    </header>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Scrollable content needs keyboard focus.) -->
    <div class="preview-messages" role="region" aria-label="Recent stored messages" tabindex="0" bind:this={scroller}>
      {#if error}<p class="status" role="alert">Could not load preview: {error}</p>
      {:else if rows === null}<p class="status" role="status">Loading…</p>
      {:else if rows.length === 0}<p class="status">No stored messages</p>
      {:else}
        <ol>
          {#each rows as message, index (message.id)}
            {@const content = previewContent(message)}
            {#if index === 0 || dayKey(rows[index - 1].timestamp) !== dayKey(message.timestamp)}
              <li class="day">{dayLabel(message.timestamp)}</li>
            {/if}
            <li class="message" class:mine={message.from_me}>
              <div class="bubble">
                <b class="sender">{message.from_me ? "You" : displayName(message.sender_name, message.sender)}</b>
                {#if content.media}<p class="media">{content.media}</p>{/if}
                {#if content.text}<p class="text" class:notice={content.notice}>{content.text}</p>{/if}
                <time datetime={new Date(message.timestamp * 1000).toISOString()}>{formatTime(message.timestamp)}</time>
              </div>
            </li>
          {/each}
        </ol>
      {/if}
    </div>
  </div>

<style>
  #chat-preview {
    position: fixed;
    z-index: 90;
    display: flex;
    flex-direction: column;
    width: min(400px, calc(100vw - 24px));
    height: min(520px, calc(100vh - 24px));
    box-sizing: border-box;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: 18px;
    background: var(--surface);
    background: color-mix(in srgb, var(--surface) 85%, transparent);
    color: var(--text);
    backdrop-filter: blur(20px);
    box-shadow: 0 12px 36px #0005;
    font-size: max(15px, var(--font-size, 15px));
    line-height: 1.45;
    cursor: pointer;
  }
  #chat-preview:focus-visible, .preview-messages:focus-visible { outline: 2px solid var(--accent); outline-offset: -3px; }
  header { flex: none; display: flex; align-items: center; gap: 8px; padding: 14px 12px 14px 18px; border-bottom: 1px solid var(--line-strong); }
  .titles { flex: 1; min-width: 0; }
  .gear {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .gear:hover { background: var(--raised); color: var(--text); }
  header strong { display: block; overflow-wrap: anywhere; font-size: 1.08em; line-height: 1.3; }
  header span { display: block; margin-top: 3px; font-size: 0.85em; color: var(--muted); }
  .preview-messages {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
    scrollbar-gutter: stable;
    scrollbar-width: thin;
    scrollbar-color: var(--line-strong) transparent;
    padding: 12px 14px;
    background: color-mix(in srgb, var(--chat-bg) 55%, transparent);
  }
  ol { list-style: none; padding: 0; margin: 0; }
  .day { margin: 4px 0 12px; color: var(--muted); text-align: center; font-size: 0.85em; }
  .message { display: flex; justify-content: flex-start; margin: 0 0 10px; }
  .message.mine { justify-content: flex-end; }
  .bubble { min-width: 0; max-width: 88%; padding: 9px 12px 6px; border-radius: 12px; background: var(--bubble); box-shadow: 0 1px 2px #0002; }
  .mine .bubble { background: var(--bubble-mine); }
  .sender { display: block; margin-bottom: 4px; font-size: 0.9em; font-weight: 600; overflow-wrap: anywhere; }
  p { margin: 0; }
  .text { white-space: pre-wrap; overflow-wrap: anywhere; }
  .media { margin-bottom: 4px; padding: 8px 10px; border: 1px solid var(--line-strong); border-radius: 7px; font-weight: 500; }
  .notice { font-style: italic; color: var(--muted); }
  time { display: block; margin-top: 5px; color: var(--muted); font-size: 0.8em; text-align: right; }
  .status { padding: 16px 4px; color: var(--muted); overflow-wrap: anywhere; }
  @media (prefers-reduced-transparency: reduce) {
    #chat-preview { background: var(--surface); backdrop-filter: none; }
    .preview-messages { background: var(--chat-bg); }
  }
</style>
