<script lang="ts" module>
  export type StarredItem = {
    chat: string;
    id: string;
    where: string;
    author: string;
    text: string;
    timestamp: number;
    /** What the star action needs to name the message. */
    sender: string;
    fromMe: boolean;
  };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import Icon from "$lib/Icon.svelte";

  let {
    items,
    onopen,
    onunstar,
    onclose,
  }: {
    /** `null` while loading. */
    items: StarredItem[] | null;
    onopen: (item: StarredItem) => void;
    onunstar: (item: StarredItem) => void;
    onclose: () => void;
  } = $props();

  let query = $state("");
  const shown = $derived(
    (items ?? []).filter((i) =>
      `${i.text} ${i.author} ${i.where}`.toLowerCase().includes(query.trim().toLowerCase()),
    ),
  );

  function when(ts: number) {
    const date = new Date(ts * 1000);
    return new Date().toDateString() === date.toDateString()
      ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
      : date.toLocaleDateString();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  transition:fade|global={{ duration: 140 }}
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Starred messages" transition:scale|global={{ start: 0.96, duration: 160 }}>
    <header>
      <h2>Starred messages</h2>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder="Search starred messages" bind:value={query} autofocus />
    </label>
    <ul>
      {#if items === null}
        <li class="empty">Loading…</li>
      {:else if shown.length === 0}
        <li class="empty">{items.length === 0 ? "Star a message from its menu to keep it here." : "Nothing matches."}</li>
      {/if}
      {#each shown as item (`${item.chat}/${item.id}`)}
        <li class="item">
          <button class="row" onclick={() => onopen(item)}>
            <span class="head">
              <span class="who">{item.author} <span class="where">in {item.where}</span></span>
              <span class="when">{when(item.timestamp)}</span>
            </span>
            <span class="text">{item.text}</span>
          </button>
          <button class="unstar" title="Unstar" aria-label="Unstar" onclick={() => onunstar(item)}>
            <Icon name="star" size={15} filled />
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
    width: min(520px, 92vw);
    max-height: min(680px, 86vh);
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
  .close,
  .unstar {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    flex: none;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover,
  .unstar:hover {
    background: var(--raised);
    color: var(--text);
  }
  .unstar {
    color: var(--mention);
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
  ul {
    list-style: none;
    margin: 0;
    padding: 0 8px 8px;
    overflow-y: auto;
  }
  .empty {
    padding: 24px 12px;
    text-align: center;
    color: var(--muted);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .row {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 12px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--surface);
  }
  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 12.5px;
  }
  .who {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .where,
  .when {
    color: var(--muted);
    font-weight: 400;
  }
  .text {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    white-space: pre-wrap;
  }
</style>
