<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
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
  let error = $state<LocalizedError | string | null>(null);
  let outcomeBatch = $state.raw<{ jids: string[]; changes: ParticipantChange[]; approve: boolean; names: Map<string, string> } | null>(null);
  const outcomes = $derived(outcomeBatch ? joinRequestResults(outcomeBatch.jids, outcomeBatch.changes, outcomeBatch.approve)
    .map((row) => ({ ...row, name: outcomeBatch!.names.get(row.jid) ?? row.jid })) : []);
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
      if (current(target, account, revision)) error = normalizeError(e);
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
    outcomeBatch = null;
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
      outcomeBatch = { jids: [...jids], changes, approve, names };
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
        if (current(target, account, revision)) error = normalizeError({ kind: "postal_error", code: "error.group_requests_refresh", params: {}, diagnostic: normalizeError(e).diagnostic });
      }
    } catch (e) {
      if (current(target, account, revision)) error = normalizeError(e);
    } finally {
      if (current(target, account, revision)) busy = false;
    }
  }
</script>

<h2>{t("group.requests")}</h2>
<p class="lede">{t("group.requests_hint")}</p>
<div class="actions">
  <button class="button" disabled={busy || loading} onclick={refresh}>{loading ? t("ui.refreshing") : t("ui.refresh")}</button>
  {#if requests && requests.length > 0}
    <label><input type="checkbox" disabled={busy || loading} checked={requests.every((row) => !!selected[row.jid])}
      onchange={(e) => { selected = e.currentTarget.checked ? Object.fromEntries(requests!.map((row) => [row.jid, true] as const)) : {}; }} /> {t("ui.select_all")}</label>
    <span class="muted">{t("group.requests_selected", { count: picked.length })}</span>
    <button class="button primary" disabled={busy || loading || picked.length === 0} onclick={() => act(picked, true)}>{t("group.approve_selected")}</button>
    <button class="button deny" disabled={busy || loading || picked.length === 0} onclick={() => act(picked, false)}>{t("group.deny_selected")}</button>
  {/if}
</div>
{#if error}<p class="error-text" role="alert">{error}</p>{/if}
{#if outcomes.length > 0}
  <ul class="outcomes" aria-label={t("group.request_results")}>
    {#each outcomes as outcome (outcome.jid)}
      <li><strong>{outcome.name}</strong><span class:refused={!outcome.completed}>{outcome.text}</span></li>
    {/each}
  </ul>
{/if}
{#if requests === null}
  <p class="muted">{error ? t("group.requests_failed") : t("group.requests_loading")}</p>
{:else if requests.length === 0}
  <p class="muted">{t("group.requests_empty")}</p>
{:else}
  <ul class="requests">
    {#each requests as request (request.jid)}
      <li>
        <label><input type="checkbox" checked={!!selected[request.jid]} disabled={busy || loading}
          onchange={() => toggle(request.jid)} /><span>{namer(request.name, request.jid)}</span></label>
        <button class="button primary" disabled={busy || loading} onclick={() => act([request.jid], true)}>{t("group.approve")}</button>
        <button class="button deny" disabled={busy || loading} onclick={() => act([request.jid], false)}>{t("group.deny")}</button>
      </li>
    {/each}
  </ul>
{/if}
{#if busy}<p class="muted" role="status">{t("group.requests_updating")}</p>{/if}

<style>
  .actions { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; margin: 12px 0; }
  .actions label, .requests label { display: flex; align-items: center; gap: 8px; }
  input { accent-color: var(--accent); }
  ul { list-style: none; padding: 0; }
  .requests li { display: flex; align-items: center; gap: 8px; padding: 10px 0; border-bottom: 1px solid var(--line); }
  .requests label { flex: 1; min-width: 0; }
  .requests label span { overflow-wrap: anywhere; }
  .outcomes li { display: flex; flex-direction: column; gap: 3px; margin: 8px 0; font-size: 0.8125rem; }
  .refused, .deny { color: var(--danger); }
</style>
