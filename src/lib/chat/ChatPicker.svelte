<script lang="ts" module>
  export type PickerChat = { jid: string; label: string; avatar: string | null };
</script>

<script lang="ts">
  import { fade, scale } from "svelte/transition";
  import { motion } from "$lib/utils/theme.svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/ui/Icon.svelte";
  import { broadcastSendReason, guardBroadcastSend } from "$lib/utils/broadcast";

  let {
    title,
    chats,
    onforward,
    onclose,
  }: {
    title: string;
    chats: PickerChat[];
    /** Forwards to every chosen chat; thrown errors show in the dialog. */
    onforward: (jids: string[]) => Promise<void>;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let busy = $state(false);
  let failed = $state<string | null>(null);
  let chosen = $state<Record<string, true>>({});
  const shown = $derived(
    chats.filter((c) => c.label.toLowerCase().includes(query.trim().toLowerCase())),
  );
  const picked = $derived(Object.keys(chosen));

  function toggle(jid: string) {
    if (busy || broadcastSendReason(jid)) return;
    const next = { ...chosen };
    if (next[jid]) delete next[jid];
    else next[jid] = true;
    chosen = next;
  }

  async function forward() {
    if (busy || picked.length === 0) return;
    const destinations = [...picked];
    const reason = destinations.map(broadcastSendReason).find(Boolean);
    if (reason) { failed = reason; return; }
    busy = true;
    failed = null;
    try {
      for (const jid of destinations) guardBroadcastSend(jid);
      await onforward(destinations);
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
        {@const reason = broadcastSendReason(chat.jid)}
        <li>
          <button
            class="row"
            class:chosen={chosen[chat.jid]}
            disabled={busy || !!reason}
            title={reason ?? undefined}
            onclick={() => toggle(chat.jid)}>
            {#if chat.avatar}
              <img class="avatar" src={convertFileSrc(chat.avatar)} alt="" />
            {:else}
              <span class="avatar placeholder">{chat.label.slice(0, 1).toUpperCase()}</span>
            {/if}
            <span class="label">{chat.label}{#if reason}<span class="reason">{reason}</span>{/if}</span>
            <span class="check" class:on={chosen[chat.jid]} aria-hidden="true">
              {#if chosen[chat.jid]}<Icon name="check" size={14} />{/if}
            </span>
          </button>
        </li>
      {/each}
    </ul>
    <footer>
      <button class="go" disabled={busy || picked.length === 0} onclick={forward}>
        {busy ? "Forwarding…" : `Forward${picked.length > 0 ? ` (${picked.length})` : ""}`}
      </button>
    </footer>
  </div>
</div>

<style>
  .reason { display: block; color: var(--muted); font-size: 11px; white-space: normal; }
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
  .row.chosen {
    background: var(--raised);
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
  .check {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    box-sizing: border-box;
    border: 2px solid var(--faint);
    border-radius: 50%;
    color: var(--accent-text);
    flex: none;
  }
  .check.on {
    border-color: var(--accent);
    background: var(--accent);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    padding: 8px 16px 14px;
  }
  .go {
    padding: 8px 18px;
    border: 0;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-text);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .go:hover:not(:disabled) {
    filter: brightness(1.06);
  }
  .go:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
