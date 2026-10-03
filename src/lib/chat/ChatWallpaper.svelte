<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import Button from "$lib/ui/Button.svelte";
  import { wallpaperDataUrl } from "$lib/utils/chat-wallpaper";
  import { chatPicture, customization, removeChatPicture, setChatPicture } from "$lib/utils/theme.svelte";

  let { account, chat }: { account: string; chat: string } = $props();
  let picker: HTMLInputElement | undefined = $state();
  let picture = $state<string | null>(null);
  let failed = $state<LocalizedError | string>("");
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
      .catch(() => { if (live) failed = normalizeError({ kind: "postal_error", code: "error.wallpaper_load", params: {} }); });
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
      if (request === generation && account === currentAccount && chat === currentChat) failed = normalizeError({ kind: "postal_error", code: "error.wallpaper_save", params: {}, diagnostic: normalizeError(error).diagnostic });
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
      if (request === generation && account === currentAccount && chat === currentChat) failed = normalizeError({ kind: "postal_error", code: "error.wallpaper_remove", params: {} });
    } finally { if (request === generation) busy = false; }
  }
</script>

<section class="wallpaper" aria-label={t("chat.wallpaper")}>
  <div class="heading"><div><span class="name">{t("chat.wallpaper")}</span><span class="hint">{t("chat.wallpaper_hint")}</span></div>
    {#if picture}<img src={picture} alt={t("chat.wallpaper_current")} />{/if}
  </div>
  <input class="file" type="file" accept="image/png,image/jpeg,image/webp,image/gif" aria-label={t("chat.wallpaper_choose")}
    bind:this={picker} onchange={(event) => choose(event.currentTarget.files?.[0])} disabled={busy || !account || !chat} />
  <div class="actions">
    <Button variant="ghost" disabled={busy || !account || !chat} onclick={() => picker?.click()}>{busy ? t("ui.saving") : meta ? t("ui.change") : t("ui.choose")}</Button>
    {#if meta}<Button variant="ghost" disabled={busy} onclick={remove}>{t("ui.remove")}</Button>{/if}
  </div>
  {#if meta}
    <label class="darken"><span>{t("settings.picture_darken")} <span class="hint">{Math.round(meta.dim * 100)} %</span></span>
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
  .name { font-size: 0.875rem; }
  .hint { display: block; color: var(--muted); font-size: 0.75rem; }
  img { width: 56px; height: 40px; border-radius: var(--radius-sm); object-fit: cover; }
  .file { display: none; }
  .actions { display: flex; gap: 6px; }
  input[type="range"] { width: 140px; accent-color: var(--accent); }
  .error { margin: 0; color: var(--danger); font-size: 0.8125rem; }
</style>
