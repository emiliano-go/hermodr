<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import type { LinkedDevice } from "$lib/utils/wire";

  let { account, connected, onload = (id) => invoke<LinkedDevice[]>("linked_devices", { account: id }),
    onunlink = (id, jid) => invoke<void>("unlink_device", { account: id, jid }) }: {
    account: string | null;
    connected: boolean;
    onload?: (account: string) => Promise<LinkedDevice[]>;
    onunlink?: (account: string, jid: string) => Promise<void>;
  } = $props();

  let devices = $state<LinkedDevice[] | null>(null);
  let pending = $state<LinkedDevice | null>(null);
  let loading = $state(false);
  let busy = $state(false);
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
    pending = null;
    error = "";
    try {
      const rows = await onload(id);
      if (current(id, revision)) devices = rows;
    } catch (failure) {
      if (current(id, revision)) error = String(failure);
    } finally {
      if (current(id, revision)) loading = false;
    }
  }

  $effect(() => {
    account;
    connected;
    ++generation;
    devices = pending = null;
    loading = busy = false;
    error = result = "";
    void untrack(refresh);
    return () => { ++generation; };
  });

  async function unlink() {
    if (!account || !connected || !pending?.can_unlink || busy || loading) return;
    const id = account, device = pending, revision = generation;
    busy = true;
    error = result = "";
    try {
      await onunlink(id, device.jid);
      if (!current(id, revision)) return;
      pending = null;
      result = `WhatsApp accepted logout for device ${device.device_id}.`;
      try {
        const rows = await onload(id);
        if (current(id, revision)) devices = rows;
      } catch (failure) {
        if (current(id, revision)) {
          devices = null;
          error = `Logout accepted. Could not refresh linked devices: ${failure}`;
        }
      }
    } catch (failure) {
      if (current(id, revision)) error = String(failure);
    } finally {
      if (current(id, revision)) busy = false;
    }
  }
</script>

<h2>Linked devices</h2>
<p>Devices linked to this WhatsApp account. Device names and activity dates are unavailable from WhatsApp on this client.</p>
{#if !account || !connected}
  <p role="status">Connect this account to view linked devices.</p>
{:else}
  <button class="button" disabled={busy || loading} onclick={refresh}>{loading ? "Refreshing…" : "Refresh"}</button>
  {#if devices}
    <ul>
      {#each devices as device (device.jid)}
        <li>
          <span>Device {device.device_id}{#if device.is_current}<small> This device</small>{/if}</span>
          {#if device.can_unlink}
            <button class="button" disabled={busy || loading} onclick={() => { pending = device; error = result = ""; }}>
              Log out device {device.device_id}
            </button>
          {:else if device.is_current}
            <small>Use account logout to disconnect this device.</small>
          {:else}
            <small>Logout unavailable for this device.</small>
          {/if}
        </li>
      {/each}
    </ul>
  {:else if !error}
    <p role="status">Loading linked devices…</p>
  {/if}
  {#if pending}
    <section role="group" aria-label="Confirm device logout">
      <h3>Log out device {pending.device_id}?</h3>
      <p>This device will stop receiving messages and must be linked again to reconnect.</p>
      <div class="actions">
        <button class="button" disabled={busy} onclick={unlink}>{busy ? "Logging out…" : "Confirm logout"}</button>
        <button class="button" disabled={busy} onclick={() => { pending = null; }}>Cancel</button>
      </div>
    </section>
  {/if}
{/if}
{#if error}<p role="alert">{error}</p>{/if}
{#if result}<p role="status">{result}</p>{/if}

<style>
  h2 { margin-top: 0; }
  p, small { color: var(--muted); font-size: .85rem; }
  ul { list-style: none; padding: 0; }
  li { display: flex; flex-wrap: wrap; align-items: center; justify-content: space-between; gap: .75rem; padding: .8rem 0; border-bottom: 1px solid var(--border); }
  li > span small { margin-left: .5rem; }
  section { border: 1px solid var(--border); padding: 1rem; margin-top: 1rem; border-radius: 6px; }
  h3 { margin: 0; font-size: 1rem; }
  .actions { display: flex; flex-wrap: wrap; gap: .5rem; }
  [role="alert"] { color: #f66; overflow-wrap: anywhere; }
</style>
