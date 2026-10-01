<script lang="ts">
  import Button from "$lib/ui/Button.svelte";
  import { wallpaperDataUrl } from "$lib/utils/chat-wallpaper";
  import { chatPicture, customization, removeChatPicture, setChatPicture } from "$lib/utils/theme.svelte";

  let { account, chat }: { account: string; chat: string } = $props();
  let picker: HTMLInputElement | undefined = $state();
  let picture = $state<string | null>(null);
  let failed = $state("");
  let busy = $state(false);
  let generation = 0;
  const meta = $derived(customization.chatBackgrounds?.[chat]);
  $effect(() => {
    void account; void chat;
    generation++;
    busy = false;
    failed = "";
    return () => { generation++; };
  });
  $effect(() => {
    const currentAccount = account, currentChat = chat;
    void meta?.v;
    picture = null;
    if (!currentAccount || !currentChat || !meta) return;
    let live = true;
    chatPicture(currentChat).then((image) => { if (live) picture = image ?? null; })
      .catch(() => { if (live) failed = "Could not load the saved wallpaper."; });
    return () => { live = false; };
  });

  async function choose(file: File | undefined) {
    if (!file || busy || !account || !chat) return;
    const currentAccount = account, currentChat = chat, request = generation;
    busy = true;
    failed = "";
    try {
      const image = await wallpaperDataUrl(file);
      if (request !== generation || account !== currentAccount || chat !== currentChat) return;
      await setChatPicture(currentChat, image);
    } catch (error) {
      if (request === generation && account === currentAccount && chat === currentChat) failed = `Could not save wallpaper: ${String(error)}`;
    } finally {
      if (request === generation) { busy = false; if (picker) picker.value = ""; }
    }
  }

  async function remove() {
    const currentAccount = account, currentChat = chat, request = generation;
    busy = true;
    failed = "";
    try {
      await removeChatPicture(currentChat);
    } catch {
      if (request === generation && account === currentAccount && chat === currentChat) failed = "Could not remove wallpaper. Local storage may be unavailable.";
    } finally { if (request === generation) busy = false; }
  }
</script>

<section class="wallpaper" aria-label="Chat wallpaper">
  <div class="heading"><div><span class="name">Chat wallpaper</span><span class="hint">Stored on this device for this chat. PNG, JPEG, WebP or GIF; up to 8 MB and 16 megapixels.</span></div>
    {#if picture}<img src={picture} alt="Current chat wallpaper" />{/if}
  </div>
  <input class="file" type="file" accept="image/png,image/jpeg,image/webp,image/gif" aria-label="Choose chat wallpaper"
    bind:this={picker} onchange={(event) => choose(event.currentTarget.files?.[0])} disabled={busy || !account || !chat} />
  <div class="actions">
    <Button variant="ghost" disabled={busy || !account || !chat} onclick={() => picker?.click()}>{busy ? "Saving…" : meta ? "Change" : "Choose…"}</Button>
    {#if meta}<Button variant="ghost" disabled={busy} onclick={remove}>Remove</Button>{/if}
  </div>
  {#if meta}
    <label class="darken"><span>Darken picture <span class="hint">{Math.round(meta.dim * 100)} %</span></span>
      <input type="range" min="0" max="0.85" step="0.05" value={meta.dim} disabled={busy}
        oninput={(event) => { if (meta) customization.chatBackgrounds = { ...customization.chatBackgrounds, [chat]: { ...meta, dim: Number(event.currentTarget.value) } }; }} />
    </label>
  {/if}
  {#if failed}<p class="error" role="alert">{failed}</p>{/if}
</section>

<style>
  .wallpaper { display: grid; gap: 10px; }
  .heading, .darken { display: flex; align-items: center; justify-content: space-between; gap: 16px; }
  .heading > div { display: grid; gap: 4px; }
  .name { font-size: 14px; }
  .hint { display: block; color: var(--muted); font-size: 12px; }
  img { width: 56px; height: 40px; border-radius: var(--radius-sm); object-fit: cover; }
  .file { display: none; }
  .actions { display: flex; gap: 6px; }
  input[type="range"] { width: 140px; accent-color: var(--accent); }
  .error { margin: 0; color: var(--danger); font-size: 13px; }
</style>
