<script lang="ts">
  import { onMount, untrack } from "svelte";
  import Button from "$lib/ui/Button.svelte";
  import type { SpaceInboxFilters, SpaceItem, SpaceTarget } from "$lib/utils/wire";
  import { emptyInboxFilters, matchingCandidates, SPACE_KINDS, targetKey, targetTitle, type SpaceCandidate } from "./spaces";

  let { account, generation, spaceId, existing, catalog, loading = false, error = null, onadd, onclose }: {
    account: string | null; generation: number; spaceId: string; existing: SpaceItem[]; catalog: SpaceCandidate[];
    loading?: boolean; error?: string | null;
    onadd: (targets: SpaceTarget[]) => Promise<void>; onclose: () => void;
  } = $props();
  let dialog: HTMLDialogElement;
  const openedAccount = untrack(() => account), openedGeneration = untrack(() => generation), openedSpace = untrack(() => spaceId);
  let closed = false, request = 0;
  let kind = $state<SpaceTarget["kind"]>("chat"), query = $state("");
  let selected = $state.raw<SpaceTarget[]>([]);
  let searchQuery = $state(""), searchChat = $state("");
  let filters = $state<SpaceInboxFilters>(emptyInboxFilters());
  let working = $state(false), failure = $state("");
  const current = () => !closed && account === openedAccount && generation === openedGeneration && spaceId === openedSpace;
  const disabled = $derived(!account || working || !current());
  const assigned = $derived(new Set(existing.filter((item) => item.space_id === spaceId).map((item) => targetKey(item.target))));
  const picked = $derived(new Set(selected.map(targetKey)));
  const shown = $derived(matchingCandidates(catalog, kind, query));
  const chats = $derived.by(() => {
    const rows = new Map<string, string>();
    for (const row of catalog) if ("jid" in row.target && row.target.kind !== "community") rows.set(row.target.jid, row.title);
    return [...rows].map(([jid, title]) => ({ jid, title }));
  });
  const labels = $derived(catalog.filter((row) => row.target.kind === "label"));

  onMount(() => {
    const previous = document.activeElement;
    dialog.showModal();
    dialog.querySelector<HTMLInputElement>("input[type=search]")?.focus();
    return () => {
      closed = true; request++; dialog.close();
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  });
  $effect(() => { if (account !== openedAccount || generation !== openedGeneration || spaceId !== openedSpace) close(); });

  function close() {
    if (closed) return;
    closed = true;
    request++;
    selected = [];
    searchQuery = searchChat = query = "";
    filters = emptyInboxFilters();
    working = false;
    failure = "";
    dialog?.close();
    onclose();
  }

  function choose(target: SpaceTarget) {
    if (disabled || assigned.has(targetKey(target)) || picked.has(targetKey(target))) return;
    selected = [...selected, structuredClone(target)];
  }

  function toggle(target: SpaceTarget) {
    if (disabled) return;
    const key = targetKey(target);
    if (picked.has(key)) selected = selected.filter((item) => targetKey(item) !== key);
    else choose(target);
  }

  function addSearch() {
    if (!searchQuery.trim()) return;
    choose({ kind: "saved_search", query: searchQuery.trim(), chat: searchChat || null });
  }

  function addInbox() {
    choose({ kind: "inbox_view", filters: { ...filters } });
  }

  async function save() {
    if (disabled) return;
    const targets = structuredClone(selected.filter((target) => !assigned.has(targetKey(target))));
    if (!targets.length) { failure = "Choose items that are not already in this Space."; return; }
    const revision = ++request;
    working = true;
    failure = "";
    try { await onadd(targets); if (current() && revision === request) close(); }
    catch (cause) { if (current() && revision === request) failure = String(cause); }
    finally { if (current() && revision === request) working = false; }
  }
</script>

<dialog bind:this={dialog} aria-label="Add items to Space" oncancel={(event) => { event.preventDefault(); close(); }}>
  <header><h2>Add items to Space</h2><Button variant="icon" icon="x" aria-label="Close Space picker" onclick={close} /></header>
  <p class="muted">Adds local references. Existing chats and WhatsApp settings stay unchanged.</p>
  <label>Item type <select bind:value={kind} disabled={disabled}>
    {#each Object.entries(SPACE_KINDS) as [value, label]}<option value={value}>{label}</option>{/each}
  </select></label>
  <input type="search" aria-label="Find local items" placeholder="Find local items" bind:value={query} disabled={disabled} />
  <ul class="catalog" aria-busy={loading}>
    {#each shown as row (targetKey(row.target))}
      {@const key = targetKey(row.target)}
      <li><label><input type="checkbox" checked={picked.has(key) || assigned.has(key)} disabled={disabled || assigned.has(key)}
        onchange={() => toggle(row.target)} /><span>{row.title}{#if row.detail}<small>{row.detail}</small>{/if}</span></label>
        {#if assigned.has(key)}<small class="muted">Already in this Space</small>{/if}</li>
    {/each}
  </ul>
  {#if loading}<p class="muted" role="status">Loading local items…</p>
  {:else if !shown.length}<p class="muted">No matching items in the local catalog.</p>{/if}
  {#if kind === "saved_search"}
    <fieldset><legend>Save a search</legend>
      <label>Search query <input bind:value={searchQuery} disabled={disabled} /></label>
      <label>Search in <select bind:value={searchChat} disabled={disabled}><option value="">All local messages</option>
        {#each chats as chat (chat.jid)}<option value={chat.jid}>{chat.title}</option>{/each}</select></label>
      <button disabled={disabled || !searchQuery.trim()} onclick={addSearch}>Add search to selection</button>
    </fieldset>
  {:else if kind === "inbox_view"}
    <fieldset><legend>Save an inbox view</legend>
      <p class="muted">Match all selected filters.</p>
      {#each [["unread", "Unread"], ["mentions", "Mentions"], ["labelled", "Labelled"], ["muted", "Muted"], ["archived", "Archived"]] as [key, title]}
        <label class="check"><input type="checkbox" bind:checked={filters[key as "unread" | "mentions" | "labelled" | "muted" | "archived"]} disabled={disabled} /> {title}</label>
      {/each}
      <label>Label <select bind:value={filters.label} disabled={disabled}><option value="">Any label</option>
        {#each labels as label, index (index)}{#if label.target.kind === "label"}<option value={label.target.label_id}>{label.title}</option>{/if}{/each}
      </select></label>
      <label>Search chats <input bind:value={filters.query} disabled={disabled} /></label>
      <button disabled={disabled} onclick={addInbox}>Add inbox view to selection</button>
    </fieldset>
  {/if}
  {#if selected.length}
    <h3>Selected items</h3><ul class="selected">
      {#each selected as target (targetKey(target))}<li><span>{catalog.find((row) => targetKey(row.target) === targetKey(target))?.title ?? targetTitle(target)}</span>
        <button disabled={disabled} aria-label={`Remove ${targetTitle(target)} from selection`} onclick={() => toggle(target)}>Remove</button></li>{/each}
    </ul>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  <footer><Button variant="primary" disabled={disabled || !selected.length} onclick={save}>{working ? "Adding…" : `Add ${selected.length} items`}</Button></footer>
</dialog>

<style>
  dialog { width: min(560px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto; box-sizing: border-box; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header, footer, .selected li { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h2 { margin: 0; font-size: 17px; }
  h3 { font-size: 14px; }
  label { display: grid; gap: 6px; margin: 10px 0; font-size: 13px; }
  input:not([type="checkbox"]), select { box-sizing: border-box; width: 100%; padding: 8px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; }
  ul { margin: 0; padding: 0; list-style: none; }
  .catalog { max-height: 220px; overflow-y: auto; }
  .catalog label, .check { display: flex; align-items: flex-start; gap: 8px; }
  .catalog span, .selected span { overflow-wrap: anywhere; }
  small { display: block; margin-top: 3px; font-size: 12px; color: var(--muted); }
  .muted { color: var(--muted); font-size: 12px; }
  fieldset { margin: 12px 0; border: 1px solid var(--line-strong); border-radius: 6px; }
  button { padding: 5px 8px; border: 1px solid var(--line-strong); border-radius: 5px; background: var(--bg); color: var(--text); font: inherit; cursor: pointer; }
  .selected li { padding: 6px 0; font-size: 13px; }
  footer { justify-content: flex-end; margin-top: 16px; }
  .error { color: var(--danger); font-size: 13px; }
  button:disabled { opacity: .5; cursor: default; }
</style>
