<script lang="ts" module>
  export type PickerTab = "emoji" | "gif" | "sticker";
</script>

<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { onMount, untrack } from "svelte";
  import { fly } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import { broadcastSendReason, guardBroadcastSend } from "$lib/utils/broadcast";
  import { base64Of as toBase64 } from "$lib/utils/files";
  import { sendAttachment } from "$lib/utils/upload";
  import Icon from "$lib/ui/Icon.svelte";
  import ImageCropper from "$lib/composer/ImageCropper.svelte";
  import StickerSync from "$lib/media/StickerSync.svelte";
  import { stickers as stickerEvents } from "$lib/state/stickers.svelte";
  import { session } from "$lib/state/session.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { stickerScopeMatches, type StickerScope } from "$lib/utils/sticker-sync";
  import type { Sticker, StickerLibrary, StickerPack, StickerResyncReport } from "$lib/utils/wire";
  import {
    GROUPS,
    loadEmojis,
    recentEmojis,
    rememberEmoji,
    searchEmojis,
    type Emoji,
  } from "$lib/utils/emoji";

  let {
    chat,
    disabled = false,
    tab = $bindable("emoji"),
    enqueue,
    takereply,
    onemoji,
    onsent,
    onerror,
    onclose,
    /** Reaction mode: emoji grid only, no GIF/sticker tabs or send paths. */
    emojiOnly = false,
    /** Fixed anchor for reaction mode; without it the picker hangs off the composer. */
    anchor = null,
  }: {
    chat: string;
    disabled?: boolean;
    tab?: PickerTab;
    /** The app's ordered outbox, so picks go out in sequence with everything else. */
    enqueue: <T>(task: (signal: AbortSignal) => Promise<T>) => Promise<T>;
    /** Takes the reply being composed, if any, as send arguments, clearing it. */
    takereply: () => Record<string, string>;
    onemoji: (emoji: string) => void;
    onsent: () => void;
    onerror: (message: string | LocalizedError) => void;
    onclose: () => void;
    emojiOnly?: boolean;
    anchor?: { x: number; y: number } | null;
  } = $props();

  let query = $state("");
  let emojis = $state<Emoji[]>([]);
  let recents = $state<string[]>(recentEmojis());
  let library = $state<Record<"gif" | "sticker", string[]>>({ gif: [], sticker: [] });
  let grid: HTMLDivElement | undefined = $state();
  let fileInput: HTMLInputElement | undefined = $state();
  /** Sticker files the renderer cannot draw (Lottie). */
  let broken = $state<Record<string, true>>({});
  /** The synced sticker library: packs, favourites and recents. */
  let lib = $state<StickerLibrary>({ packs: [], favorites: [], recent: [], catalog_complete: false });
  let openPack = $state<{ pack: StickerPack; stickers: Sticker[] } | null>(null);
  let packLoading = $state(false), packError = $state<LocalizedError | string | null>(""), libraryError = $state<LocalizedError | string | null>("");
  let packRequest = 0;

  const FAV_KEY = "postal.favStickers";
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

  const SIZE_KEY = "postal.pickerSize";
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

  /**
   * Reaction-mode anchor: the picker is fixed at the menu's spot, clamped to
   * the viewport after paint (same approach as MessageMenu's own clamp).
   */
  let pickerEl: HTMLDivElement | undefined = $state();
  let anchorPos = $state({ left: 0, top: 0 });
  $effect(() => {
    if (!emojiOnly || !anchor) return;
    void size;
    requestAnimationFrame(() => {
      const rect = pickerEl?.getBoundingClientRect();
      const w = rect?.width ?? 360;
      const h = rect?.height ?? 440;
      anchorPos = {
        left: Math.max(8, Math.min(anchor.x, window.innerWidth - w - 8)),
        top: Math.max(8, Math.min(anchor.y, window.innerHeight - h - 8)),
      };
    });
  });

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
    void session.activeAccount;
    void messages.accountGeneration;
    void chat;
    lib = { packs: [], favorites: [], recent: [], catalog_complete: false };
    library = { gif: [], sticker: [] };
    openPack = null;
    packLoading = false;
    fetching = {};
    fetchingAll = {};
    packError = libraryError = "";
    ++packRequest;
    return () => { ++packRequest; };
  });
  $effect(() => { void session.connected; ++packRequest; packLoading = false; fetching = {}; fetchingAll = {}; });

  $effect(() => {
    const changed = stickerEvents.version;
    if (emojiOnly || tab === "emoji") return;
    void changed;
    const kind = tab, owner = pickerScope();
    if (!owner) return;
    let active = true;
    libraryError = "";
    invoke<string[]>("media_library", { kind, prefer: kind === "sticker" ? untrack(() => favourites) : [] })
      .then((paths) => { if (active && pickerCurrent(owner) && tab === kind) library[kind] = paths; })
      .catch((failure) => { if (active && pickerCurrent(owner) && tab === kind) libraryError = normalizeError(failure); });
    return () => { active = false; };
  });

  const results = $derived(query.trim() ? searchEmojis(emojis, query, 120) : []);

  function pickEmoji(emoji: string) {
    rememberEmoji(emoji);
    recents = recentEmojis();
    onemoji(emoji);
  }

  async function toggleFavourite(path: string) {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return;
    packError = "";
    const on = favourites.includes(path);
    favourites = on ? favourites.filter((p) => p !== path) : [path, ...favourites];
    try {
      localStorage.setItem(FAV_KEY, JSON.stringify(favourites));
    } catch {
      // Favourites only last this session then.
    }
    if (!session.connected) { packError = new LocalizedError({ kind: "postal_error", code: "error.content.favorite_saved_locally_connect_this_account_to_request_phone_sync", params: {} }); return; }
    try {
      await invoke("favorite_sticker_path", { accountId: owner.account, path, favorite: !on });
    } catch (failure) {
      if (request === packRequest && pickerCurrent(owner)) packError = normalizeError(failure);
    } finally {
      if (request === packRequest && pickerCurrent(owner)) stickerEvents.touch();
    }
  }

  function pickerScope(): StickerScope | null {
    return session.activeAccount ? { account: session.activeAccount, chat, generation: messages.accountGeneration } : null;
  }
  function pickerCurrent(owner: StickerScope): boolean {
    return stickerScopeMatches(owner, session.activeAccount, chat, messages.accountGeneration);
  }

  async function openPackView(pack: StickerPack) {
    const owner = pickerScope();
    if (!owner) return;
    const request = ++packRequest;
    const current = () => request === packRequest && pickerCurrent(owner) && tab === "sticker";
    openPack = { pack, stickers: openPack?.pack.pack_id === pack.pack_id ? openPack.stickers : [] };
    packLoading = true;
    packError = "";
    try {
      const cached = await invoke<Sticker[]>("sticker_pack", { accountId: owner.account, pack: pack.pack_id });
      if (!current()) return;
      openPack = { pack, stickers: cached };
      if (!session.connected) return;
      await invoke("fetch_sticker_pack", { accountId: owner.account, pack: pack.pack_id });
      if (!current()) return;
      const list = await invoke<Sticker[]>("sticker_pack", { accountId: owner.account, pack: pack.pack_id });
      if (!current()) return;
      openPack = { pack, stickers: list };
    } catch (e) {
      if (current()) packError = normalizeError(e);
    } finally {
      if (current()) packLoading = false;
    }
  }

  /** Files downloading right now, keyed by filehash, so a second click does not start another. */
  let fetching = $state<Record<string, true>>({});
  /** Categories whose fetch-all is running, keyed by pack id. */
  let fetchingAll = $state<Record<string, true>>({});

  /** WhatsApp's download rate limiter, told apart from ordinary failures. */
  class RateLimited extends Error {}

  function isRateLimit(e: unknown) {
    return /429|rate[- ]?overlimit/i.test(normalizeError(e).diagnostic ?? String(e));
  }

  function rateText(e: unknown) {
    return e instanceof RateLimited
      ? new LocalizedError({ kind: "postal_error", code: "error.content.whatsapp_is_rate_limiting_sticker_downloads_try_again_in_a_few_minutes", params: {} })
      : normalizeError(e);
  }

  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
  /** Gap between downloads in a fetch-all, so a burst does not trip the limiter. */
  const FETCH_GAP_MS = 350;

  /** Downloads a sticker without sending it; an already-downloaded file is reused. */
  async function fetchSticker(sticker: Sticker): Promise<string | null> {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return null;
    const current = () => request === packRequest && pickerCurrent(owner);
    if (sticker.path) return sticker.path;
    if (!session.connected) { packError = new LocalizedError({ kind: "postal_error", code: "error.content.connect_this_account_to_download_stickers", params: {} }); return null; }
    if (fetching[sticker.filehash]) return null;
    const pending = fetching;
    pending[sticker.filehash] = true;
    packError = "";
    try {
      const path = await invoke<string>("download_sticker", { accountId: owner.account, filehash: sticker.filehash });
      if (!current()) return null;
      sticker.path = path;
      return path;
    } catch (e) {
      if (!current()) return null;
      if (isRateLimit(e)) throw new RateLimited(String(e));
      packError = normalizeError(e);
      return null;
    } finally {
      delete pending[sticker.filehash];
    }
  }

  /** Fetches a whole set in order, pacing the downloads and cooling off on 429. */
  async function fetchAll(key: string, stickers: Sticker[]) {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return;
    const current = () => request === packRequest && pickerCurrent(owner);
    if (fetchingAll[key]) return;
    const pending = fetchingAll;
    pending[key] = true;
    try {
      for (const sticker of stickers) {
        if (!current()) return;
        if (sticker.path) continue;
        let fetched = false;
        for (let attempt = 0; attempt < 3 && !fetched; attempt++) {
          try {
            const path = await fetchSticker(sticker);
            if (!current() || !path) return;
            fetched = true;
          } catch (e) {
            if (!current()) return;
            if (!(e instanceof RateLimited)) throw e;
            await sleep(4000 * (attempt + 1));
            if (!current()) return;
          }
        }
        if (!fetched) {
          packError = new LocalizedError({ kind: "postal_error", code: "error.content.whatsapp_is_rate_limiting_sticker_downloads_try_again_in_a_few_minutes", params: {} });
          return;
        }
        await sleep(FETCH_GAP_MS);
      }
    } finally {
      delete pending[key];
    }
  }

  /** A tile fetches a missing file; only a ready one is sent. */
  function clickSticker(sticker: Sticker) {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return;
    if (!sticker.path) {
      void fetchSticker(sticker).catch((e) => { if (request === packRequest && pickerCurrent(owner)) packError = rateText(e); });
      return;
    }
    const destination = chat;
    if (!canSendTo(destination)) return;
    const reply = takereply();
    void send(() =>
      invoke("send_from_library", { chat: destination, path: sticker.path!, kind: "sticker", ...reply }),
      destination,
    );
  }

  async function toggleSyncedFavorite(sticker: Sticker) {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return;
    if (!session.connected) { packError = new LocalizedError({ kind: "postal_error", code: "error.content.connect_this_account_to_update_synced_favorites", params: {} }); return; }
    packError = "";
    try {
      await invoke("favorite_sticker", { accountId: owner.account, filehash: sticker.filehash, favorite: !sticker.favorite });
    } catch (failure) {
      if (request === packRequest && pickerCurrent(owner)) packError = normalizeError(failure);
    } finally {
      if (request === packRequest && pickerCurrent(owner)) stickerEvents.touch();
    }
  }

  function canSendTo(destination: string) {
    const reason = broadcastSendReason(destination) ?? (disabled ? t("content.message_sending_is_disabled_here") : null);
    if (reason) onerror(reason);
    return !reason;
  }

  async function send(task: (signal: AbortSignal) => Promise<unknown>, destination = chat) {
    if (!canSendTo(destination)) return;
    const owner = pickerScope();
    if (!owner) return;
    onclose();
    try {
      await enqueue((signal) => {
        if (disabled) throw new LocalizedError({ kind: "postal_error", code: "error.content.message_sending_is_disabled_here", params: {} });
        guardBroadcastSend(destination);
        if (!pickerCurrent(owner)) throw new LocalizedError({ kind: "postal_error", code: "error.content.sticker_destination_changed_before_sending", params: {} });
        return task(signal);
      });
      if (pickerCurrent(owner)) onsent();
    } catch (e) {
      if (pickerCurrent(owner)) onerror(normalizeError(e));
    }
  }

  function sendFromLibrary(path: string, kind: "gif" | "sticker") {
    const destination = chat;
    if (!canSendTo(destination)) return;
    const reply = takereply();
    void send(() => invoke("send_from_library", { chat: destination, path, kind, ...reply }), destination);
  }

  async function uploadFile(file: File) {
    if (tab === "sticker") {
      making = file;
      return;
    }
    const destination = chat;
    if (!canSendTo(destination)) return;
    const reply = takereply();
    await send((signal) => sendAttachment(file, { chat: destination, gif: true, ...reply }, signal), destination);
  }

  /** A picture being cropped into a sticker. */
  let making = $state<File | null>(null);
  async function sendMade(file: File) {
    const destination = chat;
    if (!canSendTo(destination)) return;
    making = null;
    const reply = takereply();
    await send(async (signal) => {
      const data = await toBase64(file);
      signal.throwIfAborted();
      return invoke("send_sticker", { chat: destination, data, ...reply });
    }, destination);
  }
  async function saveMade(file: File) {
    const owner = pickerScope(), request = packRequest;
    if (!owner) return;
    const current = () => request === packRequest && pickerCurrent(owner);
    making = null;
    try {
      const data = await toBase64(file);
      if (!current()) return;
      const path = await invoke<string>("save_sticker", { data });
      if (!current()) return;
      await toggleFavourite(path);
      if (!current()) return;
      const paths = await invoke<string[]>("media_library", { kind: "sticker", prefer: favourites });
      if (current()) library.sticker = paths;
    } catch (e) {
      if (current()) packError = normalizeError(e);
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

{#snippet syncedTile(sticker: Sticker)}
  <div class="tile">
    <button
      class="tile-send"
      title={sticker.path ? t("content.send_sticker") : t("content.fetch_sticker")}
      onclick={() => clickSticker(sticker)}>
      {#if sticker.path && !broken[sticker.path]}
        <img src={convertFileSrc(sticker.path!)} alt="" loading="lazy" onerror={() => (broken[sticker.path!] = true)} />
      {:else}
        <span class="tile-unsupported">
          {sticker.lottie
            ? t("content.lottie_sticker")
            : fetching[sticker.filehash]
              ? t("content.fetching")
              : t("content.tap_to_fetch")}
        </span>
      {/if}
    </button>
    <button
      class="fav"
      class:on={sticker.favorite}
      title={sticker.favorite ? t("content.remove_from_favourites") : t("content.add_to_favourites")}
      aria-label={t("content.favourite")}
      onclick={() => toggleSyncedFavorite(sticker)}><Icon name="star" size={14} /></button>
  </div>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="catcher" class:anchored={emojiOnly && anchor} role="presentation" onclick={onclose}></div>
<div
  class="picker"
  class:anchored={emojiOnly && anchor}
  role="dialog"
  aria-label={emojiOnly ? t("content.choose_a_reaction") : t("content.emoji_gifs_and_stickers")}
  bind:this={pickerEl}
  style={emojiOnly && anchor
    ? `left: ${anchorPos.left}px; top: ${anchorPos.top}px; width: min(360px, calc(100vw - 32px)); height: min(440px, calc(100vh - 120px));`
    : `width: min(${size.w}px, calc(100vw - 32px)); height: min(${size.h}px, calc(100vh - 120px)); --picker-h: ${size.h}px`}
  transition:fly={{ y: 8, duration: motion(140) }}>
  {#if !emojiOnly}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="resize"
      title={t("content.drag_to_resize")}
      onpointerdown={onResizeDown}
      onpointermove={onResizeMove}
      onpointerup={onResizeUp}></div>
    <div class="tabs" role="tablist">
      <button role="tab" class:active={tab === "gif"} aria-selected={tab === "gif"} onclick={() => (tab = "gif")}>{t("content.gifs")}</button>
      <button role="tab" class:active={tab === "sticker"} aria-selected={tab === "sticker"} onclick={() => (tab = "sticker")}>{t("content.stickers")}</button>
      <button role="tab" class:active={tab === "emoji"} aria-selected={tab === "emoji"} onclick={() => (tab = "emoji")}>{t("content.emoji")}</button>
    </div>
  {/if}

  {#if emojiOnly || tab === "emoji"}
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder={t("content.find_the_perfect_emoji")} bind:value={query} autofocus />
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
          {#if results.length === 0}<p class="empty">{t("content.no_emoji_matches", { query })}</p>{/if}
        {:else}
          {#if recents.length}
            <h4>{t("content.frequently_used")}</h4>
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
          {#if emojis.length === 0}<p class="empty">{t("content.loading_emoji")}</p>{/if}
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
      {#if libraryError}<p class="sticker-error" role="alert">{libraryError}</p>{/if}
      <button class="upload" onclick={() => fileInput?.click()}>
        <Icon name="plus" size={16} />
        {tab === "sticker" ? t("content.send_an_image_as_a_sticker") : t("content.send_a_video_as_a_gif")}
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
        <StickerSync account={session.activeAccount} {chat} generation={messages.accountGeneration} connected={session.connected} version={stickerEvents.version}
          onload={(owner) => invoke<StickerLibrary>("sticker_library", { accountId: owner.account })}
          onresync={(owner) => invoke<StickerResyncReport>("resync_stickers", { accountId: owner.account })}
          onlibrary={(owner, value) => { if (pickerCurrent(owner)) lib = value; }}
          onsynced={(owner) => { if (pickerCurrent(owner)) stickerEvents.touch(); }} />
        {#if packError}<p class="sticker-error" role="alert">{packError}</p>{/if}
        {#if openPack}
          {@const pack = openPack}
          <div class="pack-head">
            <button class="pack-back" title={t("content.all_packs")} onclick={() => { ++packRequest; packLoading = false; packError = ""; openPack = null; }}><Icon name="chevronLeft" size={15} /></button>
            <span class="pack-name">{pack.pack.name ?? pack.pack.publisher ?? t("content.sticker_pack")}</span>
            {#if pack.stickers.length > 0}
              <button
                class="fetch-all"
                disabled={!!fetchingAll[pack.pack.pack_id]}
                onclick={() => fetchAll(pack.pack.pack_id, pack.stickers)}>
                {fetchingAll[pack.pack.pack_id] ? t("content.fetching") : t("content.fetch_all")}
              </button>
            {/if}
          </div>
          {#if packLoading}<p class="empty" role="status">{t("content.refreshing_pack_cached_stickers_remain_available")}</p>{/if}
          <div class="tiles stickers">
            {#each pack.stickers as sticker (sticker.filehash)}
              <button
                class="tile-send"
                title={sticker.path ? t("content.send_sticker") : t("content.fetch_sticker")}
                onclick={() => clickSticker(sticker)}>
                {#if sticker.path && !broken[sticker.path]}
                  <img src={convertFileSrc(sticker.path!)} alt="" loading="lazy" onerror={() => (broken[sticker.path!] = true)} />
                {:else}
                  <span class="tile-unsupported">
                    {sticker.lottie
                      ? t("content.lottie_sticker")
                      : fetching[sticker.filehash]
                        ? t("content.fetching")
                        : t("content.tap_to_fetch")}
                  </span>
                {/if}
              </button>
            {/each}
          </div>
          {#if pack.stickers.length === 0 && !packLoading}<p class="empty">{t("content.no_cached_stickers_in_this_pack")}</p>{/if}
        {:else}
          {#if lib.favorites.length > 0}
            <div class="section-head">
              <h4>{t("content.favorites")}</h4>
              {#if lib.favorites.some((sticker) => !sticker.path)}
                <button
                  class="fetch-all"
                  disabled={!!fetchingAll["favorites"]}
                  onclick={() => fetchAll("favorites", lib.favorites)}>
                  {fetchingAll["favorites"] ? t("content.fetching") : t("content.fetch_all")}
                </button>
              {/if}
            </div>
            <div class="tiles stickers">
              {#each lib.favorites as sticker (sticker.filehash)}
                {@render syncedTile(sticker)}
              {/each}
            </div>
          {/if}
          {#if lib.recent.length > 0}
            <h4>{t("content.recent")}</h4>
            <div class="tiles stickers">
              {#each lib.recent as sticker (sticker.filehash)}
                {@render syncedTile(sticker)}
              {/each}
            </div>
          {/if}
          {#if lib.packs.length > 0}
            <h4>{t("content.categories")}</h4>
            <div class="pack-bar">
              {#each lib.packs as pack (pack.pack_id)}
                <button class="pack-chip" title={pack.publisher ?? t("content.pack")} onclick={() => openPackView(pack)}>
                  {#if pack.tray_path}<img src={convertFileSrc(pack.tray_path)} alt="" />{/if}
                  <span>{pack.name ?? pack.pack_id.slice(0, 8)}</span>
                </button>
              {/each}
            </div>
          {/if}
          {#if lib.favorites.length === 0 && lib.recent.length === 0 && stickers.length === 0}
            <p class="empty">{t("content.your_stickers_sync_from_your_phone_send_a_sticker_and_it_appears_here")}</p>
          {/if}
          {#if stickers.length > 0}
            <h4>{t("content.all_received")}</h4>
            <div class="tiles stickers">
              {#each stickers as path (path)}
                <div class="tile">
                  <button class="tile-send" title={t("content.send_sticker")} onclick={() => sendFromLibrary(path, "sticker")}>
                    {#if broken[path]}
                      <span class="tile-unsupported">{t("content.unsupported_sticker")}</span>
                    {:else}
                      <img src={convertFileSrc(path)} alt="" loading="lazy" onerror={() => (broken[path] = true)} />
                    {/if}
                  </button>
                  <button
                    class="fav"
                    class:on={favourites.includes(path)}
                    title={favourites.includes(path) ? t("content.remove_from_favourites") : t("content.add_to_favourites")}
                    aria-label={t("content.favourite")}
                    onclick={() => toggleFavourite(path)}><Icon name="star" size={14} /></button>
                </div>
              {/each}
            </div>
          {/if}
        {/if}
      {:else}
        <div class="tiles gifs">
          {#each library.gif as path (path)}
            <button class="tile-send" title={t("content.send_gif")} onclick={() => sendFromLibrary(path, "gif")}>
              <!-- svelte-ignore a11y_media_has_caption -->
              <video src={convertFileSrc(path)} autoplay loop muted playsinline></video>
            </button>
          {/each}
        </div>
        {#if library.gif.length === 0}
          <p class="empty">{t("content.gifs_you_receive_show_up_here_ready_to_send_again")}</p>
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
  /* Reaction mode floats above the message menu (271) and its scrim. */
  .catcher.anchored {
    z-index: 276;
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
  .picker.anchored {
    position: fixed;
    right: auto;
    bottom: auto;
    z-index: 277;
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
    border-inline-start: 2px solid var(--faint);
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
  .section-head {
    display: flex;
    align-items: center;
    gap: 8px;
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
  .pack-bar {
    display: flex;
    gap: 6px;
    overflow-x: auto;
    padding-bottom: 8px;
  }
  .pack-chip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: 140px;
    padding: 4px 8px 4px 4px;
    border: 1px solid var(--line-strong);
    border-radius: 8px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .pack-chip img {
    width: 24px;
    height: 24px;
    object-fit: contain;
  }
  .pack-chip span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pack-head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 8px;
  }
  .pack-back {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: 6px;
    background: var(--raised);
    color: var(--text);
    cursor: pointer;
  }
  .pack-name {
    font-size: 13px;
    font-weight: 600;
  }
  .fetch-all {
    margin-inline-start: auto;
    padding: 5px 10px;
    border: 0;
    border-radius: 6px;
    background: var(--raised);
    color: var(--muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .fetch-all:hover:not(:disabled) {
    color: var(--text);
  }
  .fetch-all:disabled {
    opacity: 0.7;
    cursor: default;
  }
  .sticker-error { color: var(--danger, #ef7777); font-size: 12px; white-space: pre-wrap; }
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
  .tile-unsupported {
    display: grid;
    place-items: center;
    aspect-ratio: 1;
    border-radius: 8px;
    border: 2px dashed var(--faint);
    color: var(--muted);
    font-size: 11px;
    font-weight: 600;
    text-align: center;
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
