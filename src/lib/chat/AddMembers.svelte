<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { invoke } from "$lib/utils/ipc";
  import Icon from "$lib/ui/Icon.svelte";
  import { changeText, historyReceivers, historyResultText } from "$lib/utils/group-actions";
  import { members as contactMembers } from "$lib/state/members.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { GroupHistoryOffer, GroupHistoryResult, GroupMemberAddResult, Member, SearchResult } from "$lib/utils/models";

  /** Picks people to add to a group, from the account's contacts. */
  let {
    chat,
    title,
    members,
    avatars,
    me,
    onavatar,
    onadd,
    onretryhistory,
    onclose,
  }: {
    chat: string;
    title: string;
    /** The group's current members, so they are not offered again. */
    members: Member[];
    avatars: Record<string, string | null>;
    /** Our own JID, so we are not offered. */
    me: string | null;
    onavatar: (jid: string) => void;
    onadd: (jids: string[], optedIn: string[]) => Promise<GroupMemberAddResult>;
    onretryhistory: (retryId: string) => Promise<GroupHistoryResult>;
    onclose: () => void;
  } = $props();

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let searching = $state(false);
  let selected = $state<Record<string, true>>({});
  let busy = $state(false);
  let error = $state<string | null>(null);
  let outcomes = $state<{ jid: string; name: string; text: string; bad: boolean }[] | null>(null);
  let offer = $state<GroupHistoryOffer | null>(null);
  let shareHistory = $state(false);
  let history = $state<GroupHistoryResult | null>(null);
  let retrying = $state(false);
  let debounce: ReturnType<typeof setTimeout> | undefined;
  let generation = 0;

  function current(target: string, account: string | null, revision: number) {
    return target === chat && account === session.activeAccount && revision === generation;
  }

  $effect(() => {
    const target = chat;
    const account = session.activeAccount;
    const revision = ++generation;
    let active = true;
    offer = null;
    shareHistory = false;
    history = null;
    outcomes = null;
    selected = {};
    results = [];
    busy = false;
    retrying = false;
    error = null;
    invoke<GroupHistoryOffer>("group_history_offer", { chat: target, account })
      .then((value) => {
        if (active && current(target, account, revision)) offer = value;
      })
      .catch((e) => {
        if (active && current(target, account, revision)) {
          offer = { enabled: false, reason: String(e), max_messages: 0, time_window_seconds: 0 };
        }
      });
    return () => { active = false; ++generation; };
  });

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
    const target = chat;
    const account = session.activeAccount;
    const revision = generation;
    let active = true;
    clearTimeout(debounce);
    if (!q) {
      results = [];
      searching = false;
      return;
    }
    searching = true;
    debounce = setTimeout(async () => {
      try {
        const found = await invoke<SearchResult[]>("search", { query: q });
        if (active && current(target, account, revision)) {
          results = found;
          error = null;
        }
      } catch (e) {
        if (active && current(target, account, revision)) error = String(e);
      } finally {
        if (active && current(target, account, revision)) searching = false;
      }
    }, 180);
    return () => { active = false; clearTimeout(debounce); };
  });

  $effect(() => {
    for (const result of shown) onavatar(result.jid);
  });

  function toggle(jid: string) {
    if (busy) return;
    const next = { ...selected };
    if (next[jid]) delete next[jid];
    else next[jid] = true;
    selected = next;
  }

  const names = $derived(new Map(shown.map((r) => [r.jid, contactMembers.displayName(r.name, r.jid)])));

  async function add() {
    if (busy || picked.length === 0) return;
    const target = chat;
    const account = session.activeAccount;
    const revision = generation;
    const requested = shareHistory;
    const labels = names;
    busy = true;
    error = null;
    outcomes = null;
    try {
      const result = await onadd(picked, historyReceivers(picked, requested, offer));
      if (!current(target, account, revision)) return;
      const reported = result.participants.map((change) => ({
        jid: change.jid,
        name: labels.get(change.jid) ?? change.jid,
        text: changeText(change) ?? "Added.",
        bad: !change.ok && !change.pending,
      }));
      selected = {};
      if (requested || result.participants.some((change) => !change.ok || change.pending)) {
        outcomes = reported;
        history = requested ? result.history : null;
        results = [];
        query = "";
      } else {
        onclose();
      }
    } catch (e) {
      if (current(target, account, revision)) error = String(e);
    } finally {
      if (current(target, account, revision)) busy = false;
    }
  }

  async function retryHistory() {
    if (busy || !history?.retry_id) return;
    const target = chat;
    const account = session.activeAccount;
    const revision = generation;
    const retryId = history.retry_id;
    busy = true;
    retrying = true;
    error = null;
    try {
      const result = await onretryhistory(retryId);
      if (current(target, account, revision)) history = result;
    } catch (e) {
      if (current(target, account, revision)) error = String(e);
    } finally {
      if (current(target, account, revision)) { busy = false; retrying = false; }
    }
  }

  function close() {
    if (!busy) onclose();
  }

  function escape(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    close();
  }

  function windowLabel(seconds: number) {
    const [unit, size] = seconds % 86400 === 0 ? ["day", 86400] : seconds % 3600 === 0 ? ["hour", 3600] : ["second", 1];
    const count = seconds / Number(size);
    return `${count.toLocaleString()} ${unit}${count === 1 ? "" : "s"}`;
  }
</script>

<svelte:window onkeydowncapture={escape} />

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="backdrop"
  role="presentation"
  onclick={(e) => e.target === e.currentTarget && close()}>
  <div class="dialog" role="dialog" aria-modal="true" aria-label="Add participants to {title}">
    <header>
      <h2>Add to {title}</h2>
      <button class="close" aria-label="Close" disabled={busy} onclick={close}><Icon name="x" size={18} /></button>
    </header>
    <label class="search">
      <Icon name="search" size={15} />
      <!-- svelte-ignore a11y_autofocus -->
      <input placeholder="Search contacts" bind:value={query} disabled={busy || outcomes !== null} autofocus />
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
            <button class="row" disabled={busy} onclick={() => toggle(result.jid)}>
              {#if avatars[result.jid]}
                <img class="avatar" src={convertFileSrc(avatars[result.jid]!)} alt="" />
              {:else}
                <span class="avatar placeholder">{contactMembers.displayName(result.name, result.jid).slice(0, 1).toUpperCase()}</span>
              {/if}
              <span class="label">{contactMembers.displayName(result.name, result.jid)}</span>
              <input
                type="checkbox"
                disabled={busy}
                tabindex="-1"
                checked={!!selected[result.jid]}
                onchange={() => toggle(result.jid)}
                onclick={(e) => e.stopPropagation()}
                aria-label="Select {contactMembers.displayName(result.name, result.jid)}" />
            </button>
          </li>
        {/each}
      </ul>
      {#if query.trim() && !searching && shown.length === 0}
        <p class="empty">Nobody matches, or everyone here is already a member.</p>
      {/if}
      {#if searching}<p class="empty">Searching…</p>{/if}
    {/if}
    {#if history}
      <div class="history-result" role="status">
        <p class="note">History: {historyResultText(history)}</p>
        {#if history.retry_id}
          <button class="button" disabled={busy} onclick={retryHistory}>
            {retrying ? "Retrying history…" : "Retry history"}
          </button>
        {/if}
      </div>
    {:else if !outcomes}
      <div class="history-offer">
        <label>
          <input type="checkbox" bind:checked={shareHistory} disabled={busy || !offer?.enabled} />
          Share recent history with selected people
        </label>
        <p class="note">
          {#if !offer}Checking history sharing availability…
          {:else if offer.enabled}Up to {offer.max_messages} messages from the last {windowLabel(offer.time_window_seconds)}.
          {:else}{offer.reason ?? "History sharing is unavailable."}{/if}
        </p>
      </div>
    {/if}
    <footer>
      {#if !outcomes}
        <button class="button primary" disabled={busy || picked.length === 0} onclick={add}>
          {busy ? "Adding…" : `Add${picked.length > 0 ? ` ${picked.length}` : ""}`}
        </button>
      {/if}
      <button class="button" disabled={busy} onclick={close}>Done</button>
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
  .history-offer, .history-result {
    padding: 0 8px;
  }
  .history-offer label {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .history-offer .note, .history-result .note {
    margin: 6px 0;
    color: var(--muted);
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
