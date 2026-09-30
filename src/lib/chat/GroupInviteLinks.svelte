<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { session } from "$lib/state/session.svelte";

  let { chat, canReset = false, onload, onreset }: { chat: string; canReset?: boolean; onload: () => Promise<string>; onreset: () => Promise<string> } = $props();
  let link = $state<string | null>(null);
  let loading = $state(false);
  let busy = $state(false);
  let confirming = $state(false);
  let error = $state<string | null>(null);
  let outcome = $state("");
  let generation = 0;

  function current(target: string, account: string | null, revision: number) {
    return target === chat && account === session.activeAccount && revision === generation;
  }

  async function refresh() {
    if (loading || busy) return;
    const target = chat, account = session.activeAccount, revision = generation;
    loading = true; error = null;
    try {
      const result = await onload();
      if (current(target, account, revision)) link = result;
    } catch (failure) { if (current(target, account, revision)) error = String(failure); }
    finally { if (current(target, account, revision)) loading = false; }
  }

  $effect(() => {
    chat; session.activeAccount; ++generation;
    link = null; loading = false; busy = false; confirming = false; error = null; outcome = "";
    void untrack(refresh);
  });
  onDestroy(() => { generation++; });
  $effect(() => { if (!canReset) confirming = false; });

  async function reset() {
    if (!canReset || !confirming || busy || loading) return;
    const target = chat, account = session.activeAccount, revision = generation;
    confirming = false; busy = true; error = null; outcome = "";
    try {
      const result = await onreset();
      if (current(target, account, revision)) { link = result; outcome = "Invite link reset. The previous link no longer works."; }
    } catch (failure) { if (current(target, account, revision)) { link = null; error = String(failure); } }
    finally { if (current(target, account, revision)) busy = false; }
  }

  async function copy() {
    if (!link || busy || loading) return;
    const target = chat, account = session.activeAccount, revision = generation;
    try {
      await navigator.clipboard.writeText(link);
      if (current(target, account, revision)) outcome = "Invite link copied.";
    } catch (failure) { if (current(target, account, revision)) error = String(failure); }
  }
</script>

<div class="group-invite-links">
  <h3>Invite link</h3>
  {#if link}<input aria-label="Group invite link" readonly value={link} />{/if}
  {#if loading || (!link && !error)}<p>Loading invite link…</p>{:else if busy}<p>Resetting invite link…</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if outcome}<p role="status">{outcome}</p>{/if}
  <div class="actions">
    <button disabled={!link || loading || busy} onclick={copy}>Copy link</button>
    {#if canReset}<button disabled={!link || loading || busy} onclick={() => { confirming = true; }}>Reset link…</button>{/if}
    {#if error}<button disabled={loading || busy} onclick={refresh}>Retry</button>{/if}
  </div>
</div>

{#if confirming}
  <ConfirmDialog label="Reset group invite link" title="Reset this invite link?" hint="The current link will stop working. People will need the new link to join."
    onclose={() => { if (!busy) confirming = false; }}>
    {#snippet actions()}
      <button onclick={() => { confirming = false; }}>Cancel</button>
      <button class="danger" onclick={reset}>Reset link</button>
    {/snippet}
  </ConfirmDialog>
{/if}

<style>
  .group-invite-links { display: flex; flex-direction: column; gap: 10px; }
  h3, p { margin: 0; }
  input { width: 100%; box-sizing: border-box; padding: 8px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; }
  button { padding: 7px 10px; background: var(--raised); color: var(--text); border: 1px solid var(--line); border-radius: 6px; font: inherit; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  .error, .danger { color: var(--danger); }
</style>
