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
  import { tick } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import MediaViewer, { mediaSrc, type ViewerItem } from "$lib/media/MediaViewer.svelte";
  import type { MessagePage } from "$lib/utils/message-window";
  import { cursorOf } from "$lib/utils/message-window";
  import { dayKey, dayLabel, formatTime } from "$lib/utils/message";
  import { displayName } from "$lib/utils/phone";
  import { hue } from "$lib/utils/avatar";
  import { player } from "$lib/state/player.svelte";
  import { ui } from "$lib/state/ui.svelte";

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
  let olderError = $state<string | null>(null);
  let hasMore = $state(false);
  let loadingOlder = $state(false);
  let scroller = $state<HTMLElement>();
  let viewportWidth = $state(0);
  let viewportHeight = $state(0);

  /** The message whose media the built-in viewer is showing, if any. */
  let viewerId = $state<string | null>(null);

  $effect(() => {
    const jid = chat;
    const owner = account;
    rows = null;
    error = null;
    olderError = null;
    hasMore = false;
    loadingOlder = false;
    viewerId = null;
    let active = true;
    void (async () => {
      try {
        const page = await invoke<MessagePage>("message_page", { chat: jid, limit: 30 });
        if (!active || chat !== jid || account !== owner) return;
        rows = page.messages.toReversed();
        hasMore = page.has_more;
        await tick();
        if (active && scroller) {
          scroller.scrollTop = scroller.scrollHeight;
        }
      } catch (e) {
        if (active && chat === jid && account === owner) error = String(e);
      }
    })();
    return () => { active = false; };
  });

  /** Pages the local store back when the reader reaches the top. */
  async function loadOlder() {
    if (!hasMore || loadingOlder || !rows?.length) return;
    const jid = chat;
    const owner = account;
    const el = scroller;
    const before = el?.scrollHeight ?? 0;
    loadingOlder = true;
    olderError = null;
    try {
      const page = await invoke<MessagePage>("message_page", {
        chat: jid, limit: 30, cursor: cursorOf(rows[0]), direction: "before",
      });
      if (chat !== jid || account !== owner) return;
      rows = [...page.messages.toReversed(), ...rows];
      hasMore = page.has_more;
      await tick();
      // Hold the viewport still while older rows grow above it.
      if (el) el.scrollTop += el.scrollHeight - before;
    } catch (e) {
      if (chat === jid && account === owner) olderError = String(e);
    } finally {
      if (chat === jid && account === owner) loadingOlder = false;
    }
  }

  function onPreviewScroll() {
    if (scroller && scroller.scrollTop < 40) void loadOlder();
  }

  /** Local images, videos and GIFs the built-in viewer can page through. */
  const viewable = $derived(
    (rows ?? []).filter(
      (message) =>
        !isUnavailable(message) &&
        !!message.media_path &&
        (message.media_kind === "image" || message.media_kind === "video" || message.media_kind === "gif"),
    ),
  );
  const viewerItems = $derived(viewable.map(viewerItem));
  const viewerIndex = $derived(viewerId ? viewable.findIndex((message) => message.id === viewerId) : -1);

  function viewerItem(message: StoredMessage): ViewerItem {
    const content = previewContent(message);
    return {
      id: message.id,
      path: message.media_path!,
      thumb: message.media_thumb,
      kind: message.media_kind!,
      caption: content.text || content.media || "",
      author: message.from_me ? "You" : displayName(message.sender_name, message.sender),
      avatar: null,
      timestamp: message.timestamp,
    };
  }

  function viewableKind(kind: string | null) {
    return kind === "image" || kind === "video" || kind === "gif";
  }

  const isPlaying = (message: StoredMessage) =>
    player.track?.path === message.media_path && !player.paused;

  function toggleAudio(message: StoredMessage) {
    if (isPlaying(message)) {
      player.pause();
      return;
    }
    void player.play({
      path: message.media_path!,
      duration: message.media_duration,
      avatar: null,
      initials: "",
      title: message.from_me ? "You" : displayName(message.sender_name, message.sender),
    });
  }

  /** Opens a downloaded file in the desktop's own viewer. */
  async function openExternal(path: string) {
    if (/\.svg$/i.test(path)) return;
    try {
      await invoke("open_path", { path });
    } catch (e) {
      ui.fail(e);
    }
  }
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
    <div
      class="preview-messages"
      role="region"
      aria-label="Recent stored messages"
      tabindex="0"
      bind:this={scroller}
      onscroll={onPreviewScroll}>
      {#if error}<p class="status" role="alert">Could not load preview: {error}</p>
      {:else if rows === null}<p class="status" role="status">Loading…</p>
      {:else if rows.length === 0}<p class="status">No stored messages</p>
      {:else}
        {#if loadingOlder}<p class="status" role="status">Loading older…</p>{/if}
        {#if olderError}<p class="status" role="alert">Could not load older messages: {olderError}</p>{/if}
        <ol>
          {#each rows as message, index (message.id)}
            {@const content = previewContent(message)}
            {#if index === 0 || dayKey(rows[index - 1].timestamp) !== dayKey(message.timestamp)}
              <li class="day">{dayLabel(message.timestamp)}</li>
            {/if}
            <li class="message" class:mine={message.from_me}>
              <div class="bubble">
                <b
                  class="sender"
                  class:themed={!message.from_me}
                  style={message.from_me ? undefined : `--hue: ${hue(message.sender)}`}
                  >{message.from_me ? "You" : displayName(message.sender_name, message.sender)}</b>
                {#if content.media}
                  {#if message.media_kind === "audio" && message.media_path}
                    <button
                      type="button"
                      class="media audio"
                      class:playing={isPlaying(message)}
                      title={isPlaying(message) ? "Pause voice message" : "Play voice message"}
                      onclick={(e) => {
                        e.stopPropagation();
                        toggleAudio(message);
                      }}>
                      <Icon name={isPlaying(message) ? "pause" : "play"} size={15} />
                      <span>{content.media}</span>
                    </button>
                  {:else if viewableKind(message.media_kind) && message.media_path}
                    <button
                      type="button"
                      class="media open"
                      title="Open in the viewer"
                      onclick={(e) => {
                        e.stopPropagation();
                        viewerId = message.id;
                      }}>
                      {#if message.media_thumb || message.media_kind === "image" || message.media_kind === "gif"}
                        <img src={mediaSrc(message.media_thumb ?? message.media_path)} alt="" loading="lazy" />
                      {:else}
                        <span class="play-badge"><Icon name="play" size={14} /></span>
                      {/if}
                      <span>{content.media}</span>
                    </button>
                  {:else if message.media_path}
                    <button
                      type="button"
                      class="media open"
                      title="Open in the desktop"
                      onclick={(e) => {
                        e.stopPropagation();
                        void openExternal(message.media_path!);
                      }}>
                      <span>{content.media}</span>
                    </button>
                  {:else}
                    <p class="media">{content.media}</p>
                  {/if}
                {/if}
                {#if content.text}<p class="text" class:notice={content.notice}>{content.text}</p>{/if}
                <time datetime={new Date(message.timestamp * 1000).toISOString()}>{formatTime(message.timestamp)}</time>
              </div>
            </li>
          {/each}
        </ol>
      {/if}
    </div>
  </div>

{#if viewerIndex >= 0}
  <MediaViewer
    items={viewerItems}
    index={viewerIndex}
    onclose={() => (viewerId = null)}
    onopen={(path) => void openExternal(path)}
    onreply={() => {}}
    onjump={() => {}} />
{/if}

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
    box-shadow: var(--shadow);
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
  .sender.themed { color: color-mix(in srgb, hsl(var(--hue) 65% 68%) 25%, var(--text)); }
  p { margin: 0; }
  .text { white-space: pre-wrap; overflow-wrap: anywhere; }
  .media { margin-bottom: 4px; padding: 8px 10px; border: 1px solid var(--line-strong); border-radius: 7px; font-weight: 500; }
  button.media {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    box-sizing: border-box;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button.media:hover { background: var(--raised); }
  button.media.audio { width: auto; }
  button.media img { width: 64px; height: 64px; flex: none; object-fit: cover; border-radius: 6px; }
  .play-badge { display: grid; place-items: center; width: 26px; height: 26px; flex: none; border-radius: 50%; background: var(--accent); color: var(--accent-ink, #fff); }
  .notice { font-style: italic; color: var(--muted); }
  time { display: block; margin-top: 5px; color: var(--muted); font-size: 0.8em; text-align: right; }
  .status { padding: 16px 4px; color: var(--muted); overflow-wrap: anywhere; }
  @media (prefers-reduced-transparency: reduce) {
    #chat-preview { background: var(--surface); backdrop-filter: none; }
    .preview-messages { background: var(--chat-bg); }
  }
</style>
