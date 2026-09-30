<script lang="ts">
  import { onDestroy } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Button from "$lib/ui/Button.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import { members } from "$lib/state/members.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { GroupCreateResult, SearchResult } from "$lib/utils/wire";

  let {
    account, me, avatars, onavatar, onsearch, oncreate, onopen, onclose,
  }: {
    account: string;
    me: string | null;
    avatars: Record<string, string | null>;
    onavatar: (jid: string) => void;
    onsearch: (query: string) => Promise<SearchResult[]>;
    oncreate: (subject: string, jids: string[]) => Promise<GroupCreateResult>;
    onopen: (result: GroupCreateResult) => Promise<void>;
    onclose: () => void;
  } = $props();

  let dialog: HTMLDialogElement;
  let subject = $state("");
  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let chosen = $state<Record<string, string>>({});
  let searching = $state(false);
  let busy = $state(false);
  let failed = $state<string | null>(null);
  let created = $state<GroupCreateResult | null>(null);
  let mounted = true;
  onDestroy(() => { mounted = false; });

  const current = () => mounted && account === session.activeAccount;
  const picked = $derived(Object.keys(chosen));
  const subjectLength = $derived(Array.from(subject.trim()).length);
  const valid = $derived(subjectLength > 0 && subjectLength <= 100 && picked.length > 0 && picked.length <= 256);
  const shown = $derived(results.filter((row) => row.kind === "contact" && row.jid !== me && row.number !== me?.split("@")[0]));

  $effect(() => {
    dialog.showModal();
    return () => dialog.close();
  });

  $effect(() => {
    const value = query.trim();
    let active = true;
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const found = await onsearch(value);
        if (active && current()) { results = found; failed = null; }
      } catch (error) {
        if (active && current()) failed = String(error);
      } finally {
        if (active && current()) searching = false;
      }
    }, value ? 180 : 0);
    return () => { active = false; clearTimeout(timer); };
  });

  $effect(() => {
    if (current()) for (const row of shown) onavatar(row.jid);
  });

  function toggle(row: SearchResult) {
    if (busy || created || !current()) return;
    const next = { ...chosen };
    if (next[row.jid]) delete next[row.jid];
    else if (picked.length < 256) next[row.jid] = members.displayName(row.name, row.jid);
    chosen = next;
  }

  function close() {
    if (!busy) onclose();
  }

  async function open() {
    if (!created || busy || !current()) return;
    busy = true;
    failed = null;
    try {
      await onopen(created);
      if (current()) onclose();
    } catch (error) {
      if (current()) failed = String(error);
    } finally {
      if (current()) busy = false;
    }
  }

  async function create() {
    if (!valid || busy || created || !current()) return;
    const name = subject.trim();
    const jids = [...picked];
    busy = true;
    failed = null;
    try {
      const result = await oncreate(name, jids);
      if (!current()) return;
      created = result;
      busy = false;
      if (!result.warnings.length && result.participants.every((person) => person.state === "added")) await open();
    } catch (error) {
      if (current()) failed = String(error);
    } finally {
      if (current()) busy = false;
    }
  }
</script>

<dialog bind:this={dialog} aria-labelledby="new-group-title" oncancel={(event) => { event.preventDefault(); close(); }}>
  <form onsubmit={(event) => { event.preventDefault(); if (!created) void create(); }}>
    <header>
      <h2 id="new-group-title">{created ? "Group created" : "New group"}</h2>
      <button class="close" type="button" aria-label="Close" disabled={busy} onclick={close}><Icon name="x" size={18} /></button>
    </header>
    {#if created}
      <p class="subject">{created.subject}</p>
      <ul class="outcomes" aria-label="Participant results">
        {#each created.participants as person (person.jid)}
          <li class:unconfirmed={person.state === "unconfirmed"}>
            <span>{chosen[person.jid] ?? members.displayName(null, person.jid)}</span>
            <span class="note">{person.state === "added" ? "Added" : person.state === "pending" ? "Awaiting admin approval" : "Membership not confirmed"}</span>
          </li>
        {/each}
      </ul>
      {#if created.participants.some((person) => person.state === "unconfirmed")}
        <p class="note">Check group members and join requests for anyone whose membership was not confirmed.</p>
      {/if}
      {#each created.warnings as warning}<p class="error" role="status">{warning}</p>{/each}
    {:else}
      <label class="field-label">
        Group subject
        <input class="field" bind:value={subject} disabled={busy} aria-describedby="group-subject-limit" placeholder="Name this group" />
      </label>
      <p id="group-subject-limit" class="note" class:error={subjectLength > 100}>{subjectLength}/100 characters</p>
      <label class="search">
        <Icon name="search" size={15} />
        <input bind:value={query} disabled={busy} aria-label="Search contacts" placeholder="Search contacts" />
      </label>
      {#if picked.length}
        <div class="picked" aria-label="Selected participants">
          {#each picked as jid (jid)}
            <button type="button" disabled={busy} aria-label="Remove {chosen[jid]}" onclick={() => { const next = { ...chosen }; delete next[jid]; chosen = next; }}>
              {chosen[jid]} <Icon name="x" size={12} />
            </button>
          {/each}
        </div>
      {/if}
      <p class="note">{picked.length}/256 people selected. You are included as group creator.</p>
      <ul class="contacts" aria-label="Contacts">
        {#each shown as row (row.jid)}
          <li>
            <label class="row" class:chosen={!!chosen[row.jid]}>
              {#if avatars[row.jid]}
                <img class="avatar" src={convertFileSrc(avatars[row.jid]!)} alt="" />
              {:else}
                <span class="avatar placeholder">{members.displayName(row.name, row.jid).slice(0, 1).toUpperCase()}</span>
              {/if}
              <span class="label">{members.displayName(row.name, row.jid)}</span>
              <input type="checkbox" checked={!!chosen[row.jid]} disabled={busy || (!chosen[row.jid] && picked.length >= 256)} onchange={() => toggle(row)} />
            </label>
          </li>
        {/each}
      </ul>
      {#if searching}<p class="note" role="status">Searching…</p>
      {:else if !shown.length}<p class="note">No contacts found.</p>{/if}
    {/if}
    {#if failed}<p class="error" role="alert">{failed}</p>{/if}
    <footer>
      <Button variant="ghost" type="button" disabled={busy} onclick={close}>{created ? "Done" : "Cancel"}</Button>
      {#if created}
        <Button variant="primary" type="button" disabled={busy} onclick={open}>{busy ? "Opening…" : "Open group"}</Button>
      {:else}
        <Button variant="primary" type="submit" disabled={busy || !valid}>{busy ? "Creating…" : "Create group"}</Button>
      {/if}
    </footer>
  </form>
</dialog>

<style>
  dialog { width: min(440px, calc(100vw - 32px)); max-height: min(680px, calc(100vh - 64px)); padding: 0; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--bg); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  form { display: flex; flex-direction: column; gap: 10px; padding: 18px 20px; }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { margin: 0; font-size: 17px; font-weight: 600; }
  .close { display: grid; place-items: center; width: 32px; height: 32px; border: 0; border-radius: 50%; background: transparent; color: var(--muted); cursor: pointer; }
  .close:hover { background: var(--raised); color: var(--text); }
  .field-label { display: flex; flex-direction: column; gap: 6px; color: var(--muted); font-size: 12px; font-weight: 600; }
  .field { padding: 8px 10px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: 6px; color: var(--text); font: inherit; font-size: 14px; font-weight: 400; }
  .search { display: flex; align-items: center; gap: 8px; padding: 0 10px; height: 34px; background: var(--surface); border-radius: 6px; color: var(--muted); }
  .search input { flex: 1; min-width: 0; background: transparent; border: 0; color: var(--text); font: inherit; }
  ul { margin: 0; padding: 0; list-style: none; }
  .contacts { overflow-y: auto; max-height: 240px; }
  .row { display: flex; align-items: center; gap: 10px; padding: 6px 8px; border-radius: 8px; cursor: pointer; }
  .row:hover, .row.chosen { background: var(--raised); }
  .row input { accent-color: var(--accent); }
  .avatar { width: 34px; height: 34px; border-radius: 50%; object-fit: cover; flex: none; }
  .placeholder { display: grid; place-items: center; background: var(--raised); color: var(--muted); font-weight: 600; }
  .label { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .picked { display: flex; flex-wrap: wrap; gap: 6px; }
  .picked button { display: flex; align-items: center; gap: 5px; padding: 5px 8px; border: 0; border-radius: 6px; background: var(--raised); color: var(--text); font: inherit; font-size: 12px; cursor: pointer; }
  .note { margin: 0; font-size: 12.5px; color: var(--muted); }
  .error, .unconfirmed .note { color: var(--danger); }
  .error { margin: 0; font-size: 13px; }
  .subject { margin: 0; font-weight: 600; overflow-wrap: anywhere; }
  .outcomes { max-height: 240px; overflow-y: auto; }
  .outcomes li { display: flex; flex-direction: column; gap: 3px; padding: 6px 8px; }
  footer { display: flex; justify-content: flex-end; gap: 8px; padding-top: 6px; }
  button:disabled { cursor: default; opacity: 0.6; }
</style>
