<script lang="ts" module>
  export type FoundItem = {
    chat: string;
    id: string;
    /** The chat's name, shown when results span several chats. */
    where: string | null;
    author: string;
    text: string;
    timestamp: number;
    unread: boolean;
  };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/theme.svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    title,
    subtitle = null,
    placeholder,
    items,
    empty,
    onquery,
    onmore,
    moreLabel = "Load more",
    onopen,
    onclose,
  }: {
    title: string;
    subtitle?: string | null;
    placeholder: string;
    /** `null` while loading. */
    items: FoundItem[] | null;
    /** Shown when there is nothing at all. */
    empty: string;
    /** Asks for fresh results; without it the query only filters `items`. */
    onquery?: (query: string) => void;
    /** Present while more results can be fetched. */
    onmore?: () => Promise<void>;
    moreLabel?: string;
    onopen: (item: FoundItem) => void;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let loadingMore = $state(false);
  async function more() {
    if (!onmore || loadingMore) return;
    loadingMore = true;
    try {
      await onmore();
    } finally {
      loadingMore = false;
    }
  }
  let timer: ReturnType<typeof setTimeout> | undefined;
  function typed() {
    if (!onquery) return;
    clearTimeout(timer);
    timer = setTimeout(() => onquery(query), 200);
  }

  const needle = $derived(query.trim().toLowerCase());
  const shown = $derived(
    onquery
      ? (items ?? [])
      : (items ?? []).filter((i) => `${i.text} ${i.author} ${i.where ?? ""}`.toLowerCase().includes(needle)),
  );

  /** The text split around every match of the query, for highlighting. */
  function parts(text: string): { hit: boolean; text: string }[] {
    if (!needle) return [{ hit: false, text }];
    const out: { hit: boolean; text: string }[] = [];
    const lower = text.toLowerCase();
    let at = 0;
    for (let found = lower.indexOf(needle); found !== -1; found = lower.indexOf(needle, at)) {
      if (found > at) out.push({ hit: false, text: text.slice(at, found) });
      out.push({ hit: true, text: text.slice(found, found + needle.length) });
      at = found + needle.length;
    }
    if (at < text.length) out.push({ hit: false, text: text.slice(at) });
    return out;
  }

  function when(ts: number) {
    const date = new Date(ts * 1000);
    return new Date().toDateString() === date.toDateString()
      ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
      : date.toLocaleDateString([], { day: "numeric", month: "short", year: "numeric" });
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onclose()} />

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
    aria-label={title}
    transition:scale|global={{ start: 0.96, duration: motion(160) }}>
    <header>
      <div class="heading">
        <h2>{title}</h2>
        {#if subtitle}<span class="sub">{subtitle}</span>{/if}
      </div>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input {placeholder} bind:value={query} oninput={typed} autofocus />
    </label>
    <ul>
      {#if items === null}
        <li class="empty">Loading…</li>
      {:else if shown.length === 0}
        <li class="empty">{items.length === 0 && !(onquery && needle) ? empty : "Nothing matches."}</li>
      {:else}
        <li class="count">{shown.length} {shown.length === 1 ? "message" : "messages"}</li>
      {/if}
      {#each shown as item (`${item.chat}/${item.id}`)}
        <li>
          <button class="row" class:unread={item.unread} onclick={() => onopen(item)}>
            <span class="head">
              <span class="who">
                {#if item.unread}<span class="dot" aria-label="Unread"></span>{/if}
                {item.author}
                {#if item.where}<span class="where">in {item.where}</span>{/if}
              </span>
              <span class="when">{when(item.timestamp)}</span>
            </span>
            <span class="text"
              >{#each parts(item.text) as part, i (i)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}</span>
          </button>
        </li>
      {/each}
      {#if onmore && items !== null && needle}
        <li class="more-row">
          <button class="more" disabled={loadingMore} onclick={more}>
            {#if loadingMore}<span class="spinner"></span> Asking your phone…{:else}{moreLabel}{/if}
          </button>
        </li>
      {/if}
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
    width: min(560px, 92vw);
    max-height: min(700px, 86vh);
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
    gap: 12px;
    padding: 16px 16px 8px 20px;
  }
  .heading {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--muted);
    font-size: 12.5px;
  }
  .close {
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
  ul {
    list-style: none;
    margin: 0;
    padding: 0 8px 8px;
    overflow-y: auto;
  }
  .empty {
    padding: 28px 12px;
    text-align: center;
    color: var(--muted);
  }
  .count {
    padding: 2px 12px 6px;
    color: var(--faint);
    font-size: 11.5px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .row {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 10px 12px;
    border: 0;
    border-left: 3px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background calc(0.12s * var(--motion-scale)) var(--ease);
  }
  .row:hover {
    background: var(--surface);
  }
  .row.unread {
    border-left-color: var(--mention);
    background: var(--mention-soft);
  }
  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-size: 12.5px;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--mention);
  }
  .where,
  .when {
    flex: none;
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
    overflow-wrap: anywhere;
  }
  .more-row {
    display: flex;
    justify-content: center;
    padding: 8px 0 4px;
  }
  .more {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 7px 18px;
    border: 1px solid var(--line-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .more:hover:not(:disabled) {
    background: var(--raised);
  }
  .more:disabled {
    color: var(--muted);
    cursor: default;
  }
  .spinner {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    border: 2px solid var(--line-strong);
    border-top-color: var(--accent);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  mark {
    border-radius: 3px;
    background: var(--mention-self-soft);
    color: inherit;
  }
</style>
