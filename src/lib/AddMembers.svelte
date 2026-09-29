<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/ipc";
  import Icon from "$lib/Icon.svelte";
  import { changeText } from "$lib/group-actions";
  import type { Member, ParticipantChange, SearchResult } from "$lib/models";

  /** Picks people to add to a group, from the account's contacts. */
  let {
    title,
    members,
    avatars,
    me,
    onavatar,
    onadd,
    onclose,
  }: {
    title: string;
    /** The group's current members, so they are not offered again. */
    members: Member[];
    avatars: Record<string, string | null>;
    /** Our own JID, so we are not offered. */
    me: string | null;
    onavatar: (jid: string) => void;
    onadd: (jids: string[]) => Promise<ParticipantChange[]>;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let selected = $state<Record<string, true>>({});
  let busy = $state(false);
  let error = $state<string | null>(null);
  let outcomes = $state<{ jid: string; name: string; text: string; bad: boolean }[] | null>(null);
  let debounce: ReturnType<typeof setTimeout> | undefined;

  /** Every address form a member is already known by. */
  const existing = $derived(
    new Set(members.flatMap((m) => [m.jid, m.number].filter((v): v is string => !!v))),
  );
  const meUser = $derived(me?.split("@")[0] ?? null);
  const shown = $derived(
    results.filter(
      (r) =>
        r.kind !== "group" &&
        r.jid !== me &&
        r.number !== meUser &&
        !existing.has(r.jid) &&
        !(r.number && existing.has(r.number)),
    ),
  );
  const picked = $derived(Object.keys(selected));

  $effect(() => {
    const q = query.trim();
    clearTimeout(debounce);
    if (!q) {
      results = [];
      return;
    }
    searching = true;
    debounce = setTimeout(async () => {
      try {
        results = await invoke<SearchResult[]>("search", { query: q });
        error = null;
      } catch (e) {
        error = String(e);
      } finally {
        searching = false;
      }
    }, 180);
    return () => clearTimeout(debounce);
  });

  $effect(() => {
    for (const result of shown) onavatar(result.jid);
  });

  function toggle(jid: string) {
    const next = { ...selected };
    if (next[jid]) delete next[jid];
    else next[jid] = true;
    selected = next;
  }

  const names = $derived(new Map(results.map((r) => [r.jid, r.name])));

  async function add() {
    if (picked.length === 0) return;
    busy = true;
    error = null;
    outcomes = null;
    try {
      const changes = await onadd(picked);
      const reported: { jid: string; name: string; text: string; bad: boolean }[] = [];
      for (const change of changes) {
        const text = changeText(change);
        if (text) {
          reported.push({
            jid: change.jid,
            name: names.get(change.jid) ?? change.jid,
            text,
            bad: !change.pending,
          });
        }
      }
      selected = {};
      if (reported.some((o) => o.bad)) {
        outcomes = reported;
        results = [];
        query = "";
      } else {
        onclose();
      }
    } catch (e) {
      error = String(e);
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
  onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Add participants to {title}">
    <header>
      <h2>Add to {title}</h2>
      <button class="close" aria-label="Close" onclick={onclose}><Icon name="x" size={18} /></button>
    </header>
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder="Search contacts" bind:value={query} autofocus />
    </label>
    {#if error}<p class="error">{error}</p>{/if}
    {#if outcomes}
      <ul class="outcomes">
        {#each outcomes as outcome (outcome.jid)}
          <li class:bad={outcome.bad}>
            <span class="label">{outcome.name}</span>
            <span class="note">{outcome.text}</span>
          </li>
        {/each}
      </ul>
    {:else}
      <ul>
        {#each shown as result (result.jid)}
          <li>
            <button class="row" onclick={() => toggle(result.jid)}>
              {#if avatars[result.jid]}
                <img class="avatar" src={convertFileSrc(avatars[result.jid]!)} alt="" />
              {:else}
                <span class="avatar placeholder">{result.name.slice(0, 1).toUpperCase()}</span>
              {/if}
              <span class="label">{result.name}</span>
              <input
                type="checkbox"
                tabindex="-1"
                checked={!!selected[result.jid]}
                onchange={() => toggle(result.jid)}
                onclick={(e) => e.stopPropagation()}
                aria-label="Select {result.name}" />
            </button>
          </li>
        {/each}
      </ul>
      {#if query.trim() && !searching && shown.length === 0}
        <p class="empty">Nobody matches, or everyone here is already a member.</p>
      {/if}
      {#if searching}<p class="empty">Searching…</p>{/if}
    {/if}
    <footer>
      <button class="button primary" disabled={busy || picked.length === 0} onclick={add}>
        {busy ? "Adding…" : `Add${picked.length > 0 ? ` ${picked.length}` : ""}`}
      </button>
      <button class="button" onclick={onclose}>Done</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 250;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--scrim);
    backdrop-filter: blur(2px);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(420px, calc(100vw - 32px));
    max-height: min(560px, calc(100vh - 80px));
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 14px;
    gap: 10px;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 16px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: 0;
    border-radius: 6px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
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
  ul {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px 8px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: var(--raised);
  }
  .avatar {
    width: 34px;
    height: 34px;
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .placeholder {
    display: grid;
    place-items: center;
    background: var(--raised);
    color: var(--muted);
    font-weight: 600;
  }
  .label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .outcomes li {
    display: flex;
    flex-direction: column;
    padding: 6px 8px;
    color: var(--muted);
  }
  .outcomes li.bad .note {
    color: var(--danger, #f15c6d);
  }
  .note {
    font-size: 12.5px;
  }
  .empty {
    margin: 12px 8px;
    color: var(--muted);
    font-size: 13px;
  }
  .error {
    margin: 0 8px;
    color: var(--danger, #f15c6d);
    font-size: 13px;
  }
  footer {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }
  .button {
    padding: 6px 12px;
    border: 0;
    border-radius: 6px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .button.primary {
    background: var(--accent);
    color: var(--accent-text);
  }
  .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
