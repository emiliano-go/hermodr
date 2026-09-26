<script lang="ts" module>
  export type ChatRetention = {
    max_age_hours: number | null;
    max_messages: number | null;
    on_demand: boolean;
  };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { fade, scale } from "svelte/transition";
  import {
    chatPicture,
    customization,
    motion,
    pictureDataUrl,
    removeChatPicture,
    setChatPicture,
  } from "$lib/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";
  import Button from "$lib/Button.svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    chat,
    title,
    picture = null,
    globalAutoDownload,
    onchange,
    onclose,
  }: {
    chat: string;
    title: string;
    /** The chat's cached picture, when there is one. */
    picture?: string | null;
    globalAutoDownload: boolean;
    /** The chat's retention after a save, so the page can follow it. */
    onchange: (retention: ChatRetention) => void;
    onclose: () => void;
  } = $props();

  // `null` follows the global setting; 0 keeps without limit.
  const WINDOWS: [number | null, string][] = [
    [null, "Default"],
    [24, "1 day"],
    [24 * 7, "1 week"],
    [24 * 30, "30 days"],
    [24 * 365, "1 year"],
    [0, "Forever"],
  ];
  const CAPS: [number | null, string][] = [
    [null, "Default"],
    [200, "200"],
    [1000, "1,000"],
    [5000, "5,000"],
    [0, "No limit"],
  ];

  let loaded = $state(false);
  let failed = $state<string | null>(null);
  let busy = $state(false);
  let retention = $state<ChatRetention>({ max_age_hours: null, max_messages: null, on_demand: true });
  let autoDownload = $state<boolean | null>(null);
  let initial = "";

  const snapshot = $derived(JSON.stringify([retention, autoDownload]));
  const dirty = $derived(loaded && snapshot !== initial);

  onMount(async () => {
    try {
      const got = await invoke<{ auto_download: boolean | null; retention: ChatRetention }>("chat_settings", { chat });
      retention = got.retention;
      autoDownload = got.auto_download;
      initial = JSON.stringify([got.retention, got.auto_download]);
      loaded = true;
    } catch (e) {
      failed = String(e);
    }
  });

  async function save() {
    busy = true;
    failed = null;
    try {
      await invoke("set_chat_retention", { chat, retention });
      if (autoDownload !== null) {
        await invoke("set_chat_auto_download", { chat, enabled: autoDownload });
      }
      onchange(retention);
      onclose();
    } catch (e) {
      failed = String(e);
    } finally {
      busy = false;
    }
  }

  // The chat's picture applies at once; it is not part of Save.
  let picker: HTMLInputElement | undefined = $state();
  let pictureUrl = $state<string | null>(null);
  const pictureMeta = $derived(customization.chatBackgrounds?.[chat]);
  $effect(() => {
    void pictureMeta?.v;
    if (!pictureMeta) {
      pictureUrl = null;
      return;
    }
    chatPicture(chat)
      .then((url) => (pictureUrl = url ?? null))
      .catch(() => {});
  });

  async function choosePicture(file: File | undefined) {
    if (!file) return;
    try {
      await setChatPicture(chat, await pictureDataUrl(file));
    } catch (e) {
      failed = String(e);
    }
    if (picker) picker.value = "";
  }

  function initials(label: string) {
    const words = label.replace(/[^\p{L}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
    return words.length === 0 ? "#" : (words.length === 1 ? words[0].slice(0, 2) : words[0][0] + words[1][0]).toUpperCase();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

{#snippet choices(options: [number | null, string][], value: number | null, set: (v: number | null) => void, label: string)}
  <div class="choices" role="radiogroup" aria-label={label}>
    {#each options as [option, text] (text)}
      <button
        class="choice"
        class:on={value === option}
        role="radio"
        aria-checked={value === option}
        onclick={() => set(option)}>{text}</button>
    {/each}
  </div>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-label="Chat settings"
    transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      {#if picture}
        <img class="avatar" src={convertFileSrc(picture)} alt="" />
      {:else}
        <span class="avatar">{initials(title)}</span>
      {/if}
      <div class="heading">
        <h2>{title}</h2>
        <span class="sub">Settings on this computer only</span>
      </div>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>

    <div class="body">
      {#if !loaded && !failed}
        <p class="muted">Loading…</p>
      {:else if loaded}
        <section>
          <h3><Icon name="clock" size={14} /> Message history</h3>
          <div class="field">
            <span class="name">Keep messages for</span>
            <span class="desc">Older ones are removed from this computer, never from your phone.</span>
            {@render choices(WINDOWS, retention.max_age_hours, (v) => (retention.max_age_hours = v), "Keep messages for")}
          </div>
          <div class="field">
            <span class="name">Keep at most</span>
            <span class="desc">Only the newest messages are kept. The latest one always stays.</span>
            {@render choices(CAPS, retention.max_messages, (v) => (retention.max_messages = v), "Keep at most")}
          </div>
          <label class="toggle-row">
            <span>
              <span class="name">Load older messages when scrolling up</span>
              <span class="desc">Asks your phone for about a day at a time. Your phone has to be online.</span>
            </span>
            <input class="toggle" type="checkbox" bind:checked={retention.on_demand} />
          </label>
        </section>

        <section>
          <h3><Icon name="download" size={14} /> Media</h3>
          <label class="toggle-row">
            <span>
              <span class="name">Download media automatically</span>
              <span class="desc">
                {#if autoDownload === null}
                  Following the global setting ({globalAutoDownload ? "on" : "off"}).
                {:else}
                  Set for this chat.
                {/if}
              </span>
            </span>
            <input
              class="toggle"
              type="checkbox"
              checked={autoDownload ?? globalAutoDownload}
              onchange={(e) => (autoDownload = e.currentTarget.checked)} />
          </label>
        </section>

        <section>
          <h3><Icon name="image" size={14} /> Background</h3>
          <div class="picture-row">
            {#if pictureUrl}<img class="picture" src={pictureUrl} alt="" />{/if}
            <span class="grow">
              <span class="name">Picture for this chat</span>
              <span class="desc">Shown behind its messages instead of the app's background.</span>
            </span>
            <input
              class="file"
              type="file"
              accept="image/*"
              bind:this={picker}
              onchange={(e) => choosePicture(e.currentTarget.files?.[0])} />
            <button class="choice" onclick={() => picker?.click()}>{pictureMeta ? "Change" : "Choose…"}</button>
            {#if pictureMeta}
              <button class="choice" onclick={() => removeChatPicture(chat)}>Remove</button>
            {/if}
          </div>
          {#if pictureMeta}
            <label class="toggle-row">
              <span>
                <span class="name">Darken picture</span>
                <span class="desc">{Math.round(pictureMeta.dim * 100)} %</span>
              </span>
              <input
                class="range"
                type="range"
                min="0"
                max="0.85"
                step="0.05"
                value={pictureMeta.dim}
                aria-label="Darken picture"
                oninput={(e) => (pictureMeta.dim = Number(e.currentTarget.value))} />
            </label>
          {/if}
        </section>
      {/if}
      {#if failed}<p class="error">{failed}</p>{/if}
    </div>

    <footer>
      <Button variant="ghost" onclick={onclose}>Cancel</Button>
      <Button variant="primary" disabled={!dirty || busy} onclick={save}>{busy ? "Saving…" : "Save"}</Button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 275;
    display: grid;
    place-items: center;
    background: var(--scrim);
  }
  .dialog {
    width: min(520px, 92vw);
    max-height: 88vh;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    overflow: hidden;
  }
  header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 16px 14px 16px 20px;
    border-bottom: 1px solid var(--line);
  }
  .avatar {
    display: grid;
    place-items: center;
    flex: none;
    width: 40px;
    height: 40px;
    border-radius: 50%;
    object-fit: cover;
    background: var(--raised-2);
    font-size: 14px;
    font-weight: 600;
  }
  .heading {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  h2 {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 16px;
    font-weight: 600;
  }
  .sub,
  .muted,
  .desc {
    color: var(--muted);
    font-size: 12.5px;
    line-height: 1.4;
  }
  .close {
    display: grid;
    place-items: center;
    flex: none;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--raised);
    color: var(--text);
  }
  .body {
    overflow-y: auto;
    padding: 6px 20px 8px;
  }
  section + section {
    border-top: 1px solid var(--line);
  }
  section {
    padding: 12px 0 6px;
  }
  h3 {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0 0 10px;
    color: var(--muted);
    font-size: 11.5px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 14px;
  }
  .name {
    font-size: 14px;
  }
  .choices {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
  .choice {
    padding: 5px 12px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    transition:
      background calc(0.15s * var(--motion-scale)) var(--ease),
      border-color calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .choice:hover {
    background: var(--raised);
  }
  .choice.on {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .toggle-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 4px 0 12px;
    cursor: pointer;
  }
  .toggle-row > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .toggle {
    appearance: none;
    position: relative;
    flex: none;
    width: 38px;
    height: 22px;
    margin: 0;
    border-radius: 999px;
    background: var(--line-strong);
    cursor: pointer;
    transition: background calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .toggle::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text);
    transition: transform calc(0.15s * var(--motion-scale)) var(--ease);
  }
  .toggle:checked {
    background: var(--accent);
  }
  .toggle:checked::after {
    transform: translateX(16px);
    background: var(--accent-ink);
  }
  .toggle:focus-visible,
  .choice:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .picture-row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-bottom: 12px;
  }
  .grow {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .picture {
    width: 56px;
    height: 40px;
    object-fit: cover;
    border-radius: 6px;
    box-shadow: 0 0 0 1px var(--line-strong);
  }
  .file {
    display: none;
  }
  .range {
    width: 160px;
    accent-color: var(--accent);
  }
  .error {
    margin: 4px 0;
    color: var(--danger);
    font-size: 13px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
    background: var(--surface);
  }
  /* Footer actions live in $lib/Button.svelte (ghost/primary variants). */
</style>
