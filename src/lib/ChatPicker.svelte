<script lang="ts" module>
  export type PickerChat = { jid: string; label: string; avatar: string | null };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/Icon.svelte";

  let {
    title,
    chats,
    onpick,
    onclose,
  }: {
    title: string;
    chats: PickerChat[];
    onpick: (jid: string) => Promise<void>;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let busy = $state<string | null>(null);
  let failed = $state<string | null>(null);
  const shown = $derived(
    chats.filter((c) => c.label.toLowerCase().includes(query.trim().toLowerCase())),
  );

  async function pick(jid: string) {
    if (busy) return;
    busy = jid;
    failed = null;
    try {
      await onpick(jid);
      onclose();
    } catch (e) {
      failed = String(e);
    } finally {
      busy = null;
    }
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: motion(140) }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label={title} transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      <h2>{title}</h2>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder="Search chats" bind:value={query} autofocus />
    </label>
    {#if failed}<p class="error">{failed}</p>{/if}
    <ul>
      {#each shown as chat (chat.jid)}
        <li>
          <button class="row" disabled={!!busy} onclick={() => pick(chat.jid)}>
            {#if chat.avatar}
              <img class="avatar" src={convertFileSrc(chat.avatar)} alt="" />
            {:else}
              <span class="avatar placeholder">{chat.label.slice(0, 1).toUpperCase()}</span>
            {/if}
            <span class="label">{chat.label}</span>
            {#if busy === chat.jid}<span class="sending">Sending…</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
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
    width: min(440px, 92vw);
    max-height: min(620px, 86vh);
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
    justify-content: space-between;
    padding: 16px 16px 8px 20px;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
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
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 16px 8px;
    padding: 0 12px;
    height: 36px;
    background: var(--surface);
    border-radius: 999px;
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
  .error {
    margin: 0 20px 8px;
    color: var(--danger);
    font-size: 13px;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0 8px 8px;
    overflow-y: auto;
  }
  .row {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover:not(:disabled) {
    background: var(--surface);
  }
  .row:disabled {
    cursor: progress;
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
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sending {
    font-size: 12.5px;
    color: var(--accent);
  }
</style>
