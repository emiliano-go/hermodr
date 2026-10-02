<script lang="ts">
  import { untrack } from "svelte";
  import { displayName, phoneLabel } from "$lib/utils/phone";
  import type { BlockedContact } from "$lib/utils/wire";

  let { account, connected, onload, onunblock }: {
    account: string | null;
    connected: boolean;
    onload: (account: string) => Promise<BlockedContact[]>;
    onunblock: (account: string, jid: string) => Promise<void>;
  } = $props();

  let contacts = $state<BlockedContact[] | null>(null);
  let loading = $state(false);
  let busy = $state<string | null>(null);
  let error = $state("");
  let result = $state("");
  let generation = 0;

  function current(id: string, revision: number) {
    return connected && id === account && revision === generation;
  }

  async function refresh() {
    if (!account || !connected || busy || loading) return;
    const id = account, revision = generation;
    loading = true;
    error = result = "";
    try {
      const rows = await onload(id);
      if (current(id, revision)) contacts = rows;
    } catch (failure) {
      if (current(id, revision)) { contacts = null; error = String(failure); }
    } finally {
      if (current(id, revision)) loading = false;
    }
  }

  $effect(() => {
    account;
    connected;
    ++generation;
    contacts = null;
    loading = false;
    busy = null;
    error = result = "";
    void untrack(refresh);
    return () => { ++generation; };
  });

  async function unblock(contact: BlockedContact) {
    if (!account || !connected || busy || loading) return;
    const id = account, revision = generation;
    busy = contact.jid;
    error = result = "";
    try {
      await onunblock(id, contact.jid);
      if (!current(id, revision)) return;
      result = `WhatsApp accepted unblock for ${displayName(undefined, contact.jid, contact.identity)}.`;
      try {
        const rows = await onload(id);
        if (current(id, revision)) contacts = rows;
      } catch (failure) {
        if (current(id, revision)) {
          contacts = null;
          error = `Unblock accepted. Could not refresh blocked contacts: ${failure}`;
        }
      }
    } catch (failure) {
      if (current(id, revision)) error = String(failure);
    } finally {
      if (current(id, revision)) busy = null;
    }
  }
</script>

<p>WhatsApp's blocked contacts for this account. Changes apply to your phone too.</p>
{#if !account || !connected}
  <p role="status">Connect this account to view blocked contacts.</p>
{:else}
  <button class="button" disabled={!!busy || loading} onclick={refresh}>{loading ? "Refreshing…" : "Refresh"}</button>
  {#if contacts}
    {#if contacts.length === 0}<p role="status">No blocked contacts.</p>{/if}
    <ul>
      {#each contacts as contact (contact.jid)}
        <li>
          <span>
            {displayName(undefined, contact.jid, contact.identity)}
            {#if contact.identity.number && (contact.identity.saved_name || contact.identity.legacy_name)}
              <small>{phoneLabel(contact.identity.number) ?? `+${contact.identity.number}`}</small>
            {/if}
          </span>
          <button class="button" disabled={!!busy || loading} onclick={() => unblock(contact)}
            aria-label={`Unblock ${displayName(undefined, contact.jid, contact.identity)}`}>
            {busy === contact.jid ? "Unblocking…" : "Unblock"}
          </button>
        </li>
      {/each}
    </ul>
  {:else if !error}
    <p role="status">Loading blocked contacts…</p>
  {/if}
{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result}<p role="status">{result}</p>{/if}

<style>
  p, small { color: var(--muted); font-size: .85rem; }
  ul { list-style: none; padding: 0; }
  li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .75rem; padding: .8rem 0; border-bottom: 1px solid var(--line); }
  li > span { overflow-wrap: anywhere; }
  small { display: block; margin-top: .25rem; }
  [role="alert"] { color: var(--danger); overflow-wrap: anywhere; }
</style>
