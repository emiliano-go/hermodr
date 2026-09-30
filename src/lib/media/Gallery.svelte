<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import MediaViewer, { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import { GalleryState } from "$lib/state/gallery.svelte";
  import { galleryDateRange, galleryKey, galleryUrl, galleryViewerItems, galleryVisible } from "$lib/utils/gallery";
  import type { ChatSummary, GalleryCursor, GalleryFilter, GalleryItem, GalleryKind, GalleryPage, StoredMessage } from "$lib/utils/wire";

  let { accountKey, chat, chats, chatName, senderName, onjump, onopen, onreply, onclose, fetchPage =
    (account, filter, cursor, limit) => invoke<GalleryPage>("gallery_page", { account, filter, cursor, limit }) }: {
    accountKey: string | null;
    chat: string;
    chats: ChatSummary[];
    chatName: (chat: string) => string;
    senderName: (message: StoredMessage) => string;
    onjump: (chat: string, id: string) => void;
    onopen: (path: string) => void;
    onreply: (message: StoredMessage) => void;
    onclose: () => void;
    fetchPage?: (account: string, filter: GalleryFilter, cursor: GalleryCursor | null, limit: number) => Promise<GalleryPage>;
  } = $props();

  const gallery = new GalleryState((filter, cursor, limit) => accountKey ? fetchPage(accountKey, filter, cursor, limit) : Promise.reject(new Error("No account selected.")));
  let scope = $state("");
  let kind = $state<GalleryKind | "">("");
  let direction = $state("");
  let start = $state("");
  let end = $state("");
  let revealed = $state(new Set<string>());
  let viewerIndex = $state<number | null>(null);
  let openError = $state<string | null>(null);
  let invalidRange = $state(false);
  const author = (message: StoredMessage) => message.from_me ? "You" : senderName(message);
  const viewerItems = $derived(galleryViewerItems(gallery.items, revealed, author));
  const kindIcon = { image: "image", video: "video", round_video: "video", audio: "mic", document: "file", sticker: "sticker", gif: "video" } as const;

  $effect(() => {
    void accountKey;
    scope = chat;
    untrack(() => { gallery.resetAccount(); revealed = new Set(); viewerIndex = null; openError = null; });
  });

  $effect(() => {
    void accountKey;
    const selection = { chat: scope || null, kind: kind || null,
      from_me: direction === "" ? null : direction === "sent", start, end };
    untrack(() => {
      revealed = new Set();
      viewerIndex = null;
      openError = null;
      invalidRange = false;
      try {
        const { start: from, end: through, ...filter } = selection;
        void gallery.reset({ ...filter, ...galleryDateRange(from, through) });
      } catch (error) { gallery.resetAccount(); gallery.error = String(error); invalidRange = true; }
    });
  });

  $effect(() => () => gallery.resetAccount());

  function reveal(message: StoredMessage) { revealed = new Set([...revealed, galleryKey(message)]); }

  function jumpMessage(message: StoredMessage) { onclose(); onjump(message.chat, message.id); }

  function openItem({ message }: GalleryItem) {
    if (!galleryVisible(message, revealed)) { reveal(message); return; }
    const index = viewerItems.findIndex((item) => item.id === galleryKey(message));
    if (index >= 0) viewerIndex = index;
    else if (message.media_path) onopen(message.media_path);
    else jumpMessage(message);
  }

  function jumpViewer(id: string) {
    const item = gallery.items.find(({ message }) => galleryKey(message) === id);
    viewerIndex = null;
    if (item) jumpMessage(item.message);
  }

  function replyViewer(id: string) {
    const item = gallery.items.find(({ message }) => galleryKey(message) === id);
    viewerIndex = null;
    if (item) { onclose(); onreply(item.message); }
  }

  async function openLink(url: string) {
    const target = galleryUrl(url);
    if (!target) { openError = "Only valid HTTP or HTTPS links can be opened."; return; }
    try { await invoke("open_url", { url: target }); openError = null; }
    catch (error) { openError = String(error); }
  }

  async function turnPage(previous = false) {
    viewerIndex = null;
    if (previous) await gallery.previousPage();
    else await gallery.nextPage();
  }

  const when = (timestamp: number) => new Date(timestamp * 1000).toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
</script>

<section class="gallery" aria-label="Media gallery">
  <header>
    <h2>Gallery</h2>
    <button class="icon" title="Close gallery" aria-label="Close gallery" onclick={onclose}><Icon name="x" size={22} /></button>
  </header>
  <nav aria-label="Gallery section">
    <button class:active={kind !== "link"} aria-pressed={kind !== "link"} onclick={() => kind = ""}><Icon name="image" /> Media</button>
    <button class:active={kind === "link"} aria-pressed={kind === "link"} onclick={() => kind = "link"}><Icon name="link" /> Links</button>
  </nav>
  <div class="filters">
    <label>Chat<select bind:value={scope}>
      <option value="">All chats</option>
      {#if !chats.some((item) => item.chat === chat)}<option value={chat}>{chatName(chat)}</option>{/if}
      {#each chats as item (item.chat)}<option value={item.chat}>{chatName(item.chat)}</option>{/each}
    </select></label>
    <label>Type<select bind:value={kind}>
      <option value="">All media</option><option value="image">Images</option><option value="video">Videos</option>
      <option value="audio">Audio / voice</option><option value="document">Documents</option><option value="sticker">Stickers</option>
      <option value="gif">GIFs</option><option value="link">Links</option>
    </select></label>
    <label>Direction<select bind:value={direction}><option value="">Sent and received</option><option value="sent">Sent</option><option value="received">Received</option></select></label>
    <label>From<input type="date" bind:value={start} /></label>
    <label>Through<input type="date" bind:value={end} min={start || undefined} /></label>
  </div>
  {#if openError}<p class="error" role="alert">{openError}</p>{/if}
  {#if gallery.error}
    <div class="error" role="alert">{gallery.error}{#if !invalidRange}<button onclick={() => gallery.retry()} disabled={gallery.loading}>Retry</button>{/if}</div>
  {/if}
  <div class="results" aria-busy={gallery.loading}>
    {#if gallery.loading && gallery.items.length === 0}<p class="empty" role="status">Loading gallery...</p>
    {:else if gallery.items.length === 0 && !gallery.error}<p class="empty">No {kind === "link" ? "links" : "media"} match these filters.</p>
    {:else}
      <div class:links={kind === "link"} class="grid">
        {#each gallery.items as item (galleryKey(item.message))}
          {@const message = item.message}
          {@const visible = galleryVisible(message, revealed)}
          {@const mediaKind = message.media_kind as keyof typeof kindIcon}
          {@const label = mediaKind === "round_video" ? "video message" : mediaKind}
          {@const thumbnail = visible ? (kind === "link" ? message.preview_thumb : message.media_thumb ?? (["image", "sticker"].includes(mediaKind) ? message.media_path : null)) : null}
          <article>
            {#if !visible}
              <button class="preview hidden" onclick={() => reveal(message)} aria-label="Reveal spoiler"><Icon name="eyeOff" size={28} /><span>Reveal spoiler</span></button>
            {:else if kind === "link"}
              <div class="link-content">
                {#if thumbnail}<img class="link-thumb" src={mediaSrc(thumbnail)} alt="" loading="lazy" />{/if}
                {#if message.preview_title}<strong>{message.preview_title}</strong>{/if}
                {#each item.urls as url (url)}<button class="url" onclick={() => openLink(url)}>{url}<Icon name="external" size={14} /></button>{/each}
                {#if message.text}<p>{message.text}</p>{/if}
              </div>
            {:else}
              <button class="preview" onclick={() => openItem(item)} aria-label={`Open ${label ?? "media"} from ${author(message)}`}>
                {#if thumbnail}<img src={mediaSrc(thumbnail)} alt="" loading="lazy" />
                {:else}<Icon name={kindIcon[mediaKind] ?? "file"} size={32} />{/if}
                <span class="kind">{label}{message.media_duration ? ` · ${message.media_duration}s` : ""}</span>
              </button>
              {#if message.text}<p class="caption">{message.text}</p>{/if}
            {/if}
            <footer>
              <div class="details"><strong>{author(message)}</strong>{#if !scope}<span>{chatName(message.chat)}</span>{/if}<time datetime={new Date(message.timestamp * 1000).toISOString()}>{when(message.timestamp)}</time></div>
              <button class="icon" onclick={() => jumpMessage(message)} title="Go to message" aria-label="Go to message"><Icon name="message" size={18} /></button>
            </footer>
          </article>
        {/each}
      </div>
    {/if}
  </div>
  <div class="paging">
    <button disabled={gallery.loading || gallery.pageIndex === 0} onclick={() => turnPage(true)}>Previous</button>
    <span role="status">{gallery.loading ? "Loading..." : `Page ${gallery.pageIndex + 1}`}</span>
    <button disabled={gallery.loading || !gallery.next} onclick={() => turnPage()}>Next</button>
  </div>
</section>

{#if viewerIndex !== null}
  <MediaViewer items={viewerItems} bind:index={viewerIndex} onclose={() => viewerIndex = null} onopen={onopen} onjump={jumpViewer} onreply={replyViewer} />
{/if}

<style>
  .gallery { display: flex; flex-direction: column; min-height: 0; flex: 1; background: var(--bg); color: var(--text); }
  header { display: flex; align-items: center; justify-content: space-between; padding: 14px 20px; border-bottom: 1px solid var(--line); }
  h2 { font-size: 18px; margin: 0; }
  button, select, input { font: inherit; color: inherit; }
  button { cursor: pointer; border: 1px solid var(--line); border-radius: 7px; padding: 8px 12px; background: var(--surface); }
  button:hover { background: var(--raised); }
  button:disabled { opacity: .45; cursor: default; }
  button:focus-visible, select:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .icon { display: inline-flex; align-items: center; justify-content: center; padding: 7px; border: 0; background: transparent; flex: none; }
  nav { display: flex; gap: 8px; padding: 12px 20px 0; }
  nav button { display: inline-flex; align-items: center; gap: 6px; }
  nav button.active { color: var(--accent); border-color: var(--accent); }
  .filters { display: flex; flex-wrap: wrap; gap: 10px; padding: 14px 20px; border-bottom: 1px solid var(--line); }
  label { display: flex; flex-direction: column; gap: 5px; font-size: 12px; color: var(--muted); }
  select, input { padding: 7px 8px; border: 1px solid var(--line); border-radius: 6px; background: var(--surface); min-width: 0; max-width: 220px; }
  .results { flex: 1; min-height: 0; overflow: auto; padding: 20px; }
  .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 14px; }
  .grid.links { grid-template-columns: repeat(auto-fill, minmax(min(100%, 320px), 1fr)); }
  article { min-width: 0; overflow: hidden; border: 1px solid var(--line); border-radius: 9px; background: var(--surface); }
  .preview { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 10px; width: 100%; height: 160px; padding: 0; position: relative; border: 0; border-radius: 0; background: var(--raised); }
  .preview img { width: 100%; height: 100%; object-fit: cover; }
  .kind { position: absolute; bottom: 6px; left: 6px; padding: 3px 6px; border-radius: 5px; color: white; background: #0009; font-size: 11px; }
  .hidden { color: var(--muted); }
  .caption { margin: 9px 12px 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; }
  footer { display: flex; align-items: center; gap: 8px; padding: 10px 12px; }
  .details { display: flex; flex-direction: column; gap: 3px; min-width: 0; flex: 1; font-size: 12px; }
  .details strong, .details span { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  time, .details span { color: var(--muted); font-size: 11px; }
  .link-content { padding: 12px; overflow-wrap: anywhere; }
  .link-thumb { width: 64px; height: 64px; object-fit: cover; float: right; margin: 0 0 8px 8px; border-radius: 5px; }
  .url { display: flex; align-items: baseline; gap: 5px; padding: 5px 0; width: 100%; text-align: left; color: var(--accent); border: 0; background: transparent; overflow-wrap: anywhere; }
  .link-content p { font-size: 13px; margin: 8px 0 0; white-space: pre-wrap; }
  .paging { display: flex; align-items: center; justify-content: center; gap: 18px; padding: 12px 20px; border-top: 1px solid var(--line); font-size: 13px; }
  .empty { text-align: center; padding: 35px 15px; color: var(--muted); }
  .error { color: var(--danger); padding: 10px 20px; overflow-wrap: anywhere; }
  .error button { margin-left: 10px; }
</style>
