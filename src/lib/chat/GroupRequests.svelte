<script lang="ts">
  import { untrack } from "svelte";
  import type { GroupJoinRequest } from "$lib/utils/wire";
  import type { ParticipantChange } from "$lib/utils/models";
  import { session } from "$lib/state/session.svelte";
  import { displayName } from "$lib/utils/phone";
  import { joinRequestResults } from "$lib/utils/join-requests";

  let { chat, onload, onchange, namer = displayName }: {
    chat: string;
    onload: () => Promise<GroupJoinRequest[]>;
    onchange: (jids: string[], approve: boolean) => Promise<ParticipantChange[]>;
    namer?: (name: string | null, jid: string) => string;
  } = $props();

  let requests = $state<GroupJoinRequest[] | null>(null);
  let selected = $state<Record<string, true>>({});
  let loading = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let outcomes = $state<{ jid: string; name: string; completed: boolean; text: string }[]>([]);
  let generation = 0;
  const picked = $derived(Object.keys(selected));

  function current(target: string, account: string | null, revision: number) {
    return target === chat && account === session.activeAccount && revision === generation;
  }

  async function refresh() {
    if (loading || busy) return;
    const target = chat, account = session.activeAccount, revision = generation;
    loading = true;
    error = null;
    try {
      const rows = await onload();
      if (current(target, account, revision)) {
        requests = rows;
        selected = Object.fromEntries(picked.filter((jid) => rows.some((row) => row.jid === jid)).map((jid) => [jid, true] as const));
      }
    } catch (e) {
      if (current(target, account, revision)) error = String(e);
    } finally {
      if (current(target, account, revision)) loading = false;
    }
  }

  $effect(() => {
    chat;
    session.activeAccount;
    ++generation;
    requests = null;
    selected = {};
    outcomes = [];
    error = null;
    busy = false;
    loading = false;
    void untrack(refresh);
    return () => { ++generation; };
  });

  function toggle(jid: string) {
    if (busy || loading) return;
    const next = { ...selected };
    if (next[jid]) delete next[jid]; else next[jid] = true;
    selected = next;
  }

  async function act(jids: string[], approve: boolean) {
    if (busy || loading || jids.length === 0) return;
    const target = chat, account = session.activeAccount, revision = generation;
    const names = new Map((requests ?? []).map((row) => [row.jid, namer(row.name, row.jid)]));
    busy = true;
    error = null;
    try {
      const changes = await onchange([...jids], approve);
      if (!current(target, account, revision)) return;
      outcomes = joinRequestResults(jids, changes, approve).map((row) => ({ ...row, name: names.get(row.jid) ?? row.jid }));
      const completed = new Set(outcomes.filter((row) => row.completed).map((row) => row.jid));
      requests = (requests ?? []).filter((row) => !completed.has(row.jid));
      selected = Object.fromEntries(picked.filter((jid) => !completed.has(jid)).map((jid) => [jid, true] as const));
      try {
        const rows = await onload();
        if (current(target, account, revision)) {
          requests = rows;
          selected = Object.fromEntries(picked.filter((jid) => rows.some((row) => row.jid === jid)).map((jid) => [jid, true] as const));
        }
      } catch (e) {
        if (current(target, account, revision)) error = `Could not refresh the remaining requests: ${e}`;
      }
    } catch (e) {
      if (current(target, account, revision)) error = String(e);
    } finally {
      if (current(target, account, revision)) busy = false;
    }
  }
</script>

<h2>Join requests</h2>
<p class="lede">Approve or deny people waiting to join this group.</p>
<div class="actions">
  <button class="button" disabled={busy || loading} onclick={refresh}>{loading ? "Refreshing…" : "Refresh"}</button>
  {#if requests && requests.length > 0}
    <label><input type="checkbox" disabled={busy || loading} checked={requests.every((row) => !!selected[row.jid])}
      onchange={(e) => { selected = e.currentTarget.checked ? Object.fromEntries(requests!.map((row) => [row.jid, true] as const)) : {}; }} /> Select all</label>
    <span class="muted">{picked.length} selected</span>
    <button class="button primary" disabled={busy || loading || picked.length === 0} onclick={() => act(picked, true)}>Approve selected</button>
    <button class="button deny" disabled={busy || loading || picked.length === 0} onclick={() => act(picked, false)}>Deny selected</button>
  {/if}
</div>
{#if error}<p class="error-text" role="alert">{error}</p>{/if}
{#if outcomes.length > 0}
  <ul class="outcomes" aria-label="Join request results">
    {#each outcomes as outcome (outcome.jid)}
      <li><strong>{outcome.name}</strong><span class:refused={!outcome.completed}>{outcome.text}</span></li>
    {/each}
  </ul>
{/if}
{#if requests === null}
  <p class="muted">{error ? "Join requests could not be loaded." : "Loading join requests…"}</p>
{:else if requests.length === 0}
  <p class="muted">No pending join requests.</p>
{:else}
  <ul class="requests">
    {#each requests as request (request.jid)}
      <li>
        <label><input type="checkbox" checked={!!selected[request.jid]} disabled={busy || loading}
          onchange={() => toggle(request.jid)} /><span>{namer(request.name, request.jid)}</span></label>
        <button class="button primary" disabled={busy || loading} onclick={() => act([request.jid], true)}>Approve</button>
        <button class="button deny" disabled={busy || loading} onclick={() => act([request.jid], false)}>Deny</button>
      </li>
    {/each}
  </ul>
{/if}
{#if busy}<p class="muted" role="status">Updating join requests…</p>{/if}

<style>
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 12px 0; }
  .actions label, .requests label { display: flex; align-items: center; gap: 8px; }
  input { accent-color: var(--accent); }
  ul { list-style: none; padding: 0; }
  .requests li { display: flex; align-items: center; gap: 8px; padding: 10px 0; border-bottom: 1px solid var(--line); }
  .requests label { flex: 1; min-width: 0; }
  .requests label span { overflow-wrap: anywhere; }
  .outcomes li { display: flex; flex-direction: column; gap: 3px; margin: 8px 0; font-size: 13px; }
  .refused, .deny { color: var(--danger); }
</style>
