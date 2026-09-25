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
  import { invoke } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";

  let {
    chat,
    title,
    globalAutoDownload,
    onchange,
    onclose,
  }: {
    chat: string;
    title: string;
    globalAutoDownload: boolean;
    /** The chat's retention after a save, so the page can follow it. */
    onchange: (retention: ChatRetention) => void;
    onclose: () => void;
  } = $props();

  // `null` follows the global setting; 0 keeps without limit.
  const WINDOWS: [number | null, string][] = [
    [null, "Use the global setting"],
    [24, "1 day"],
    [24 * 7, "1 week"],
    [24 * 30, "30 days"],
    [24 * 365, "1 year"],
    [0, "Keep everything"],
  ];
  const CAPS: [number | null, string][] = [
    [null, "Use the global setting"],
    [200, "200 messages"],
    [1000, "1,000 messages"],
    [5000, "5,000 messages"],
    [0, "No limit"],
  ];

  let loaded = $state(false);
  let failed = $state<string | null>(null);
  let busy = $state(false);
  let retention = $state<ChatRetention>({ max_age_hours: null, max_messages: null, on_demand: true });
  let autoDownload = $state<boolean | null>(null);
  let initialAuto: boolean | null = null;

  onMount(async () => {
    try {
      const got = await invoke<{ auto_download: boolean | null; retention: ChatRetention }>("chat_settings", { chat });
      retention = got.retention;
      autoDownload = initialAuto = got.auto_download;
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
      if (autoDownload !== null && autoDownload !== initialAuto) {
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
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: 140 }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Chat settings" transition:scale|global={{ start: 0.96, duration: 160 }}>
    <header>
      <div>
        <h2>Chat settings</h2>
        <span class="sub">{title}</span>
      </div>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>

    {#if !loaded && !failed}
      <p class="muted">Loading…</p>
    {:else if loaded}
      <label class="row">
        <span>
          <span class="name">Keep messages for</span>
          <span class="desc">Older ones are removed from this computer, not from your phone.</span>
        </span>
        <select class="field" bind:value={retention.max_age_hours}>
          {#each WINDOWS as [value, label] (label)}<option {value}>{label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span>
          <span class="name">Keep at most</span>
          <span class="desc">The newest messages are kept.</span>
        </span>
        <select class="field" bind:value={retention.max_messages}>
          {#each CAPS as [value, label] (label)}<option {value}>{label}</option>{/each}
        </select>
      </label>
      <label class="row">
        <span>
          <span class="name">Load older messages when scrolling up</span>
          <span class="desc">Asks your phone for about a day at a time. Your phone has to be online.</span>
        </span>
        <input class="switch" type="checkbox" bind:checked={retention.on_demand} />
      </label>
      <label class="row">
        <span>
          <span class="name">Download media automatically</span>
          <span class="desc">
            {autoDownload === null ? `Following the global setting (${globalAutoDownload ? "on" : "off"}).` : "Set for this chat."}
          </span>
        </span>
        <input
          class="switch"
          type="checkbox"
          checked={autoDownload ?? globalAutoDownload}
          onchange={(e) => (autoDownload = e.currentTarget.checked)} />
      </label>
    {/if}

    {#if failed}<p class="error">{failed}</p>{/if}
    <div class="actions">
      <button class="ghost" onclick={onclose}>Cancel</button>
      <button class="primary" disabled={!loaded || busy} onclick={save}>{busy ? "Saving…" : "Save"}</button>
    </div>
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
    width: min(500px, 92vw);
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding-bottom: 16px;
    background: var(--bg);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 16px 16px 8px 20px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .sub,
  .muted,
  .desc {
    color: var(--muted);
    font-size: 12.5px;
  }
  .muted {
    padding: 0 20px;
  }
  .close {
    display: grid;
    place-items: center;
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
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
  }
  .row > span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: 14px;
  }
  .field {
    flex: none;
    padding: 6px 8px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text);
    font: inherit;
  }
  .switch {
    flex: none;
    width: 18px;
    height: 18px;
    accent-color: var(--accent);
  }
  .error {
    margin: 4px 20px;
    color: var(--danger);
    font-size: 13px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px 0;
  }
  .ghost,
  .primary {
    padding: 8px 16px;
    border: 0;
    border-radius: var(--radius);
    font: inherit;
    cursor: pointer;
  }
  .ghost {
    background: transparent;
    color: var(--text);
  }
  .ghost:hover {
    background: var(--raised);
  }
  .primary {
    background: var(--accent);
    color: var(--accent-ink);
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
