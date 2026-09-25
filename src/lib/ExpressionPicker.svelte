<script lang="ts" module>
  export type PickerTab = "emoji" | "gif" | "sticker";
</script>

<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";
  import Icon from "$lib/Icon.svelte";
  import ImageCropper from "$lib/ImageCropper.svelte";
  import {
    GROUPS,
    loadEmojis,
    recentEmojis,
    rememberEmoji,
    searchEmojis,
    type Emoji,
  } from "$lib/emoji";

  let {
    chat,
    tab = $bindable("emoji"),
    enqueue,
    onemoji,
    onsent,
    onerror,
    onclose,
  }: {
    chat: string;
    tab?: PickerTab;
    /** The app's ordered outbox, so picks go out in sequence with everything else. */
    enqueue: <T>(task: () => Promise<T>) => Promise<T>;
    onemoji: (emoji: string) => void;
    onsent: () => void;
    onerror: (message: string) => void;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let emojis = $state<Emoji[]>([]);
  let recents = $state<string[]>(recentEmojis());
  let library = $state<Record<"gif" | "sticker", string[]>>({ gif: [], sticker: [] });
  let grid: HTMLDivElement | undefined = $state();
  let fileInput: HTMLInputElement | undefined = $state();

  const FAV_KEY = "hermodr.favStickers";
  let favourites = $state<string[]>(
    (() => {
      try {
        return JSON.parse(localStorage.getItem(FAV_KEY) ?? "[]");
      } catch {
        return [];
      }
    })(),
  );

  onMount(() => {
    loadEmojis().then((list) => (emojis = list));
  });

  const SIZE_KEY = "hermodr.pickerSize";
  let size = $state<{ w: number; h: number }>(
    (() => {
      try {
        const saved = JSON.parse(localStorage.getItem(SIZE_KEY) ?? "null");
        if (saved && typeof saved.w === "number" && typeof saved.h === "number") return saved;
      } catch {
        // Unreadable storage falls back to the default size.
      }
      return { w: 440, h: 460 };
    })(),
  );
  let resizing: { x: number; y: number; w: number; h: number } | null = null;

  /** The picker hangs from its bottom-right corner, so the handle grows it up and left. */
  function onResizeDown(e: PointerEvent) {
    resizing = { x: e.clientX, y: e.clientY, ...size };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onResizeMove(e: PointerEvent) {
    if (!resizing) return;
    size = {
      w: Math.round(Math.min(window.innerWidth - 32, Math.max(320, resizing.w + resizing.x - e.clientX))),
      h: Math.round(Math.min(window.innerHeight - 120, Math.max(300, resizing.h + resizing.y - e.clientY))),
    };
  }
  function onResizeUp() {
    resizing = null;
    try {
      localStorage.setItem(SIZE_KEY, JSON.stringify(size));
    } catch {
      // The size lasts this session then.
    }
  }

  $effect(() => {
    if (tab === "emoji") return;
    const kind = tab;
    invoke<string[]>("media_library", { kind, prefer: kind === "sticker" ? untrack(() => favourites) : [] })
      .then((paths) => (library[kind] = paths))
      .catch(() => {});
  });

  const results = $derived(query.trim() ? searchEmojis(emojis, query, 120) : []);

  function pickEmoji(emoji: string) {
    rememberEmoji(emoji);
    recents = recentEmojis();
    onemoji(emoji);
  }

  function toggleFavourite(path: string) {
    favourites = favourites.includes(path)
      ? favourites.filter((p) => p !== path)
      : [path, ...favourites];
    try {
      localStorage.setItem(FAV_KEY, JSON.stringify(favourites));
    } catch {
      // Favourites only last this session then.
    }
  }

  async function send(task: () => Promise<unknown>) {
    onclose();
    try {
      await enqueue(task);
      onsent();
    } catch (e) {
      onerror(String(e));
    }
  }

  function sendFromLibrary(path: string, kind: "gif" | "sticker") {
    send(() => invoke("send_from_library", { chat, path, kind }));
  }

  function toBase64(file: File) {
    return new Promise<string>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result).split(",")[1] ?? "");
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  async function uploadFile(file: File) {
    if (tab === "sticker") {
      making = file;
      return;
    }
    const data = await toBase64(file);
    send(() => invoke("send_media", { chat, name: file.name, data, gif: true }));
  }

  /** A picture being cropped into a sticker. */
  let making = $state<File | null>(null);
  async function sendMade(file: File) {
    making = null;
    const data = await toBase64(file);
    send(() => invoke("send_sticker", { chat, data }));
  }
  async function saveMade(file: File) {
    making = null;
    try {
      const path = await invoke<string>("save_sticker", { data: await toBase64(file) });
      toggleFavourite(path);
      library.sticker = await invoke<string[]>("media_library", { kind: "sticker", prefer: favourites });
    } catch (e) {
      onerror(String(e));
    }
  }

  function jumpTo(group: number) {
    grid?.querySelector(`[data-group="${group}"]`)?.scrollIntoView({ block: "start" });
  }

  const stickers = $derived([
    ...favourites.filter((p) => library.sticker.includes(p)),
    ...library.sticker.filter((p) => !favourites.includes(p)),
  ]);
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="catcher" role="presentation" onclick={onclose}></div>
<div
  class="picker"
  role="dialog"
  aria-label="Emoji, GIFs and stickers"
  style="width: min({size.w}px, calc(100vw - 32px)); height: min({size.h}px, calc(100vh - 120px)); --picker-h: {size.h}px"
  transition:fly={{ y: 8, duration: motion(140) }}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="resize"
    title="Drag to resize"
    onpointerdown={onResizeDown}
    onpointermove={onResizeMove}
    onpointerup={onResizeUp}></div>
  <div class="tabs" role="tablist">
    <button role="tab" class:active={tab === "gif"} aria-selected={tab === "gif"} onclick={() => (tab = "gif")}>GIFs</button>
    <button role="tab" class:active={tab === "sticker"} aria-selected={tab === "sticker"} onclick={() => (tab = "sticker")}>Stickers</button>
    <button role="tab" class:active={tab === "emoji"} aria-selected={tab === "emoji"} onclick={() => (tab = "emoji")}>Emoji</button>
  </div>

  {#if tab === "emoji"}
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder="Find the perfect emoji" bind:value={query} autofocus />
    </label>
    <div class="emoji-body">
      {#if !query.trim()}
        <div class="groups">
          {#each GROUPS as group (group.id)}
            <button title={group.label} aria-label={group.label} onclick={() => jumpTo(group.id)}>{group.icon}</button>
          {/each}
        </div>
      {/if}
      <div class="grid-scroll" bind:this={grid}>
        {#if query.trim()}
          <div class="grid">
            {#each results as e (e.emoji)}
              <button title=":{e.shortcodes[0] ?? e.label}:" onclick={() => pickEmoji(e.emoji)}>{e.emoji}</button>
            {/each}
          </div>
          {#if results.length === 0}<p class="empty">No emoji matches "{query}".</p>{/if}
        {:else}
          {#if recents.length}
            <h4>Frequently used</h4>
            <div class="grid">
              {#each recents as emoji (emoji)}
                <button onclick={() => pickEmoji(emoji)}>{emoji}</button>
              {/each}
            </div>
          {/if}
          {#each GROUPS as group (group.id)}
            <h4 data-group={group.id}>{group.label}</h4>
            <div class="grid">
              {#each emojis.filter((e) => e.group === group.id) as e (e.emoji)}
                <button title=":{e.shortcodes[0] ?? e.label}:" onclick={() => pickEmoji(e.emoji)}>{e.emoji}</button>
              {/each}
            </div>
          {/each}
          {#if emojis.length === 0}<p class="empty">Loading emoji…</p>{/if}
        {/if}
      </div>
    </div>
  {:else if making}
    <div class="maker">
      <ImageCropper
        file={making}
        square
        sizes={false}
        applyLabel="Send sticker"
        altLabel="Save"
        onapply={sendMade}
        onalt={saveMade}
        oncancel={() => (making = null)} />
    </div>
  {:else}
    <div class="library">
      <button class="upload" onclick={() => fileInput?.click()}>
        <Icon name="plus" size={16} />
        {tab === "sticker" ? "Send an image as a sticker" : "Send a video as a GIF"}
      </button>
      <input
        class="file-input"
        type="file"
        accept={tab === "sticker" ? "image/*" : "video/mp4,video/webm"}
        bind:this={fileInput}
        onchange={(e) => {
          const file = e.currentTarget.files?.[0];
          if (file) uploadFile(file);
          e.currentTarget.value = "";
        }} />
      {#if tab === "sticker"}
        <div class="tiles stickers">
          {#each stickers as path (path)}
            <div class="tile">
              <button class="tile-send" title="Send sticker" onclick={() => sendFromLibrary(path, "sticker")}>
                <img src={convertFileSrc(path)} alt="" loading="lazy" />
              </button>
              <button
                class="fav"
                class:on={favourites.includes(path)}
                title={favourites.includes(path) ? "Remove from favourites" : "Add to favourites"}
                aria-label="Favourite"
                onclick={() => toggleFavourite(path)}><Icon name="star" size={14} /></button>
            </div>
          {/each}
        </div>
        {#if stickers.length === 0}
          <p class="empty">Stickers you receive show up here, ready to send back.</p>
        {/if}
      {:else}
        <div class="tiles gifs">
          {#each library.gif as path (path)}
            <button class="tile-send" title="Send GIF" onclick={() => sendFromLibrary(path, "gif")}>
              <!-- svelte-ignore a11y_media_has_caption -->
              <video src={convertFileSrc(path)} autoplay loop muted playsinline></video>
            </button>
          {/each}
        </div>
        {#if library.gif.length === 0}
          <p class="empty">GIFs you receive show up here, ready to send again.</p>
        {/if}
      {/if}
    </div>
  {/if}
</div>

<style>
  .catcher {
    position: fixed;
    inset: 0;
    z-index: 60;
  }
  .picker {
    position: absolute;
    right: 12px;
    bottom: calc(100% + 8px);
    z-index: 61;
    display: flex;
    flex-direction: column;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  .maker {
    flex: 1;
    min-height: 0;
    display: flex;
    padding: 12px;
    overflow: auto;
    /* Tabs and the cropper's controls take about this much of the picker. */
    --crop-max-height: calc(min(var(--picker-h), 100vh - 120px) - 150px);
  }
  .resize {
    position: absolute;
    top: 0;
    left: 0;
    z-index: 2;
    width: 16px;
    height: 16px;
    cursor: nwse-resize;
    touch-action: none;
  }
  .resize::before {
    content: "";
    position: absolute;
    top: 4px;
    left: 4px;
    width: 7px;
    height: 7px;
    border-top: 2px solid var(--faint);
    border-left: 2px solid var(--faint);
    border-top-left-radius: 3px;
    opacity: 0;
    transition: opacity calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .picker:hover .resize::before {
    opacity: 1;
  }
  .tabs {
    display: flex;
    gap: 4px;
    padding: 10px 10px 0;
  }
  .tabs button {
    padding: 6px 12px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .tabs button:hover {
    color: var(--text);
  }
  .tabs button.active {
    background: var(--raised);
    color: var(--text);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 10px;
    padding: 0 10px;
    height: 34px;
    background: var(--bg);
    border-radius: 6px;
    color: var(--muted);
  }
  .search input {
    flex: 1;
    background: transparent;
    border: 0;
    outline: none;
    color: var(--text);
    font: inherit;
  }
  .emoji-body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .groups {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 4px 8px 8px;
    overflow-y: auto;
  }
  .groups button {
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    font-size: 18px;
    cursor: pointer;
    filter: grayscale(0.6);
  }
  .groups button:hover {
    background: var(--raised);
    filter: none;
  }
  .grid-scroll {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: 0 8px 8px 4px;
  }
  h4 {
    margin: 8px 4px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(38px, 1fr));
  }
  .grid button {
    height: 38px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    font-size: 24px;
    cursor: pointer;
  }
  .grid button:hover {
    background: var(--raised);
  }
  .empty {
    margin: 16px;
    color: var(--muted);
    font-size: 13px;
    text-align: center;
  }
  .library {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 10px;
  }
  .upload {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    margin-bottom: 10px;
    padding: 10px;
    border: 1px dashed var(--line-strong);
    border-radius: 8px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 13.5px;
    cursor: pointer;
  }
  .upload:hover {
    color: var(--text);
    border-color: var(--accent);
  }
  .file-input {
    display: none;
  }
  .tiles {
    display: grid;
    gap: 6px;
  }
  .stickers {
    grid-template-columns: repeat(auto-fill, minmax(90px, 1fr));
  }
  .gifs {
    grid-template-columns: repeat(2, 1fr);
  }
  .tile {
    position: relative;
  }
  .tile-send {
    width: 100%;
    padding: 4px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    cursor: pointer;
    display: block;
  }
  .tile-send:hover {
    background: var(--raised);
  }
  .tile-send img {
    width: 100%;
    aspect-ratio: 1;
    object-fit: contain;
    display: block;
  }
  .tile-send video {
    width: 100%;
    border-radius: 6px;
    display: block;
  }
  .fav {
    position: absolute;
    top: 4px;
    right: 4px;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    cursor: pointer;
    opacity: 0;
  }
  .tile:hover .fav,
  .fav.on {
    opacity: 1;
  }
  .fav.on {
    color: #f5c518;
  }
</style>
