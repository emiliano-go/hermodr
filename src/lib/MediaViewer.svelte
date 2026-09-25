<script lang="ts" module>
  import { convertFileSrc } from "@tauri-apps/api/core";

  /** Received thumbnails are stored inline as `data:` URIs; anything else is a file. */
  export const mediaSrc = (path: string) => (path.startsWith("data:") ? path : convertFileSrc(path));

  export type ViewerItem = {
    id: string;
    path: string;
    thumb: string | null;
    kind: string;
    caption: string;
    author: string;
    avatar: string | null;
    timestamp: number;
  };
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { fade } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import { invoke } from "$lib/ipc";
  import Icon from "$lib/Icon.svelte";
  import VideoPlayer from "$lib/VideoPlayer.svelte";

  let {
    items,
    index = $bindable(),
    onclose,
    onopen,
    onreply,
    onjump,
  }: {
    items: ViewerItem[];
    index: number;
    onclose: () => void;
    /** Opens the file in the desktop's own viewer; absent for view-once media. */
    onopen?: (path: string) => void;
    onreply: (id: string) => void;
    onjump: (id: string) => void;
  } = $props();

  const item = $derived(items[index]);
  const isVideo = $derived(item?.kind === "video" || item?.kind === "gif");

  let zoom = $state(1);
  let pan = $state({ x: 0, y: 0 });
  let dragging: { x: number; y: number; moved: boolean } | null = null;
  let strip: HTMLDivElement | undefined = $state();
  /** Blob URL for a video the asset scheme could not stream (WebKitGTK). */
  let videoFallback = $state<string | null>(null);

  $effect(() => {
    // Every new item starts unzoomed and centred.
    void index;
    zoom = 1;
    pan = { x: 0, y: 0 };
    if (videoFallback) URL.revokeObjectURL(videoFallback);
    videoFallback = null;
    tick().then(() =>
      strip?.querySelector(".active")?.scrollIntoView({ inline: "center", block: "nearest" }),
    );
  });

  function step(delta: number) {
    const next = index + delta;
    if (next >= 0 && next < items.length) index = next;
  }

  function setZoom(next: number) {
    zoom = Math.min(5, Math.max(1, next));
    if (zoom === 1) pan = { x: 0, y: 0 };
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowLeft") step(-1);
    else if (e.key === "ArrowRight") step(1);
    else if (e.key === "+" || e.key === "=") setZoom(zoom + 0.5);
    else if (e.key === "-") setZoom(zoom - 0.5);
  }

  function onPointerDown(e: PointerEvent) {
    dragging = { x: e.clientX, y: e.clientY, moved: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || zoom === 1) return;
    const dx = e.clientX - dragging.x;
    const dy = e.clientY - dragging.y;
    if (Math.abs(dx) + Math.abs(dy) > 3) dragging.moved = true;
    pan = { x: pan.x + dx, y: pan.y + dy };
    dragging.x = e.clientX;
    dragging.y = e.clientY;
  }

  /** A click toggles zoom; a drag only pans. */
  function onPointerUp() {
    if (dragging && !dragging.moved) setZoom(zoom > 1 ? 1 : 2.5);
    dragging = null;
  }

  async function loadVideoFallback() {
    if (videoFallback || !item) return;
    try {
      const data = await invoke<string>("read_file", { path: item.path });
      const bytes = Uint8Array.from(atob(data), (c) => c.charCodeAt(0));
      videoFallback = URL.createObjectURL(new Blob([bytes]));
    } catch {
      // Nothing more to try; the external viewer button still works.
    }
  }

  function when(ts: number) {
    const date = new Date(ts * 1000);
    const today = new Date().toDateString() === date.toDateString();
    const time = date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return today ? `Today at ${time}` : `${date.toLocaleDateString()} at ${time}`;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if item}
  <div class="viewer" role="dialog" aria-modal="true" aria-label="Media viewer" transition:fade={{ duration: motion(140) }}>
    <header>
      <div class="who">
        {#if item.avatar}
          <img class="avatar" src={convertFileSrc(item.avatar)} alt="" />
        {:else}
          <span class="avatar placeholder">{item.author.slice(0, 1).toUpperCase()}</span>
        {/if}
        <span class="who-text">
          <span class="who-name">{item.author}</span>
          <span class="who-time">{when(item.timestamp)}</span>
        </span>
      </div>
      <div class="tools">
        {#if !isVideo}
          <button class="tool" title="Zoom out" aria-label="Zoom out" disabled={zoom === 1} onclick={() => setZoom(zoom - 0.5)}
            ><Icon name="zoomOut" size={20} /></button>
          <button class="tool" title="Zoom in" aria-label="Zoom in" disabled={zoom === 5} onclick={() => setZoom(zoom + 0.5)}
            ><Icon name="zoomIn" size={20} /></button>
        {/if}
        <button class="tool" title="Go to message" aria-label="Go to message" onclick={() => onjump(item.id)}
          ><Icon name="message" size={20} /></button>
        <button class="tool" title="Reply" aria-label="Reply" onclick={() => onreply(item.id)}
          ><Icon name="reply" size={20} /></button>
        {#if onopen}
          <button class="tool" title="Open in default app" aria-label="Open in default app" onclick={() => onopen(item.path)}
            ><Icon name="external" size={20} /></button>
        {/if}
        <button class="tool" title="Close (Esc)" aria-label="Close" onclick={onclose}><Icon name="x" size={22} /></button>
      </div>
    </header>

    <div class="stage">
      <button class="nav prev" aria-label="Previous" disabled={index === 0} onclick={() => step(-1)}>
        <Icon name="chevronLeft" size={26} />
      </button>

      {#key item.id}
        {#if isVideo}
          <VideoPlayer
            src={videoFallback ?? convertFileSrc(item.path)}
            gif={item.kind === "gif"}
            onerror={loadVideoFallback} />
        {:else}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
          <img
            class="media"
            class:zoomed={zoom > 1}
            src={convertFileSrc(item.path)}
            alt={item.caption}
            draggable="false"
            style="transform: translate({pan.x}px, {pan.y}px) scale({zoom})"
            onpointerdown={onPointerDown}
            onpointermove={onPointerMove}
            onpointerup={onPointerUp}
            onwheel={(e) => {
              e.preventDefault();
              setZoom(zoom + (e.deltaY < 0 ? 0.25 : -0.25));
            }} />
        {/if}
      {/key}

      <button class="nav next" aria-label="Next" disabled={index === items.length - 1} onclick={() => step(1)}>
        <Icon name="chevronRight" size={26} />
      </button>
    </div>

    {#if item.caption}
      <p class="caption">{item.caption}</p>
    {/if}

    <div class="strip" bind:this={strip}>
      {#each items as entry, i (entry.id)}
        <button
          class="thumb"
          class:active={i === index}
          aria-label="Show item {i + 1} of {items.length}"
          onclick={() => (index = i)}>
          {#if entry.thumb || !(entry.kind === "video" || entry.kind === "gif")}
            <img src={mediaSrc(entry.thumb ?? entry.path)} alt="" />
          {:else}
            <!-- A video without a stored thumbnail shows its first frame. -->
            <video src={convertFileSrc(entry.path)} preload="metadata" muted></video>
          {/if}
          {#if entry.kind === "video" || entry.kind === "gif"}
            <span class="thumb-badge"><Icon name="video" size={12} /></span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .viewer {
    position: fixed;
    inset: 0;
    z-index: 280;
    display: flex;
    flex-direction: column;
    background: color-mix(in srgb, var(--chat-bg) 96%, transparent);
    backdrop-filter: blur(6px);
    color: var(--text);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 20px;
    flex: none;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 0;
  }
  .avatar {
    width: 40px;
    height: 40px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .placeholder {
    display: grid;
    place-items: center;
    background: var(--raised-2);
    font-weight: 600;
  }
  .who-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .who-name {
    font-weight: 600;
  }
  .who-time {
    font-size: 12.5px;
    color: var(--muted);
  }
  .tools {
    display: flex;
    gap: 4px;
  }
  .tool {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    background: transparent;
    border: 0;
    border-radius: 50%;
    color: var(--muted);
    cursor: pointer;
  }
  .tool:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }
  .tool:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .stage {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    padding: 0 72px;
    container-type: size;
  }
  .media {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 4px;
    user-select: none;
    cursor: zoom-in;
    transition: transform calc(0.12s * var(--motion-scale)) var(--ease);
  }
  .media.zoomed {
    cursor: grab;
  }
  .media.zoomed:active {
    cursor: grabbing;
    transition: none;
  }
  .nav {
    position: absolute;
    top: 50%;
    transform: translateY(-50%);
    z-index: 1;
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border: 0;
    border-radius: 50%;
    background: var(--surface);
    color: var(--text);
    cursor: pointer;
    box-shadow: var(--shadow);
  }
  .nav:hover:not(:disabled) {
    background: var(--raised);
  }
  .nav:disabled {
    opacity: 0;
    pointer-events: none;
  }
  .prev {
    left: 16px;
  }
  .next {
    right: 16px;
  }
  .caption {
    flex: none;
    margin: 10px auto 0;
    max-width: min(900px, 90%);
    text-align: center;
    white-space: pre-wrap;
  }
  .strip {
    flex: none;
    display: flex;
    gap: 6px;
    padding: 14px 20px;
    overflow-x: auto;
    justify-content: safe center;
  }
  .thumb {
    position: relative;
    flex: none;
    width: 56px;
    height: 56px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 6px;
    overflow: hidden;
    background: var(--surface);
    cursor: pointer;
    opacity: 0.6;
  }
  .thumb:hover {
    opacity: 0.9;
  }
  .thumb.active {
    border-color: var(--accent);
    opacity: 1;
  }
  .thumb img,
  .thumb video {
    width: 100%;
    height: 100%;
    object-fit: cover;
    display: block;
  }
  .thumb-badge {
    position: absolute;
    left: 3px;
    bottom: 3px;
    display: grid;
    place-items: center;
    padding: 2px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.6);
    color: #fff;
  }
</style>
