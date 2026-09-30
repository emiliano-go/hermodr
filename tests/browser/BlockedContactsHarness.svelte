<script lang="ts">
  import BlockedContacts from "../../src/lib/settings/BlockedContacts.svelte";
  import type { BlockedContact } from "../../src/lib/utils/wire";

  let account = $state("alpha");
  let connected = $state(true);
  let deferList = $state(false);
  let failList = $state(false);
  let failUnblock = $state(false);
  let calls = $state<string[]>([]);
  let waiting = $state(0);
  const releases: (() => void)[] = [];
  const removed: Record<string, boolean> = {};

  async function load(id: string): Promise<BlockedContact[]> {
    calls = [...calls, `load ${id}`];
    const failure = failList;
    const number = id === "alpha" ? "59899000000" : "59899000001";
    const lid = id === "alpha" ? "77@lid" : "88@lid";
    const rows: BlockedContact[] = removed[id] ? [] : [{ jid: lid, jids: [lid, `${number}@s.whatsapp.net`],
      identity: { contact_saved: true, saved_name: `${id} contact`, legacy_name: null, push_name: null,
        username: null, number, own: false } }];
    if (deferList) await new Promise<void>((resolve) => { releases.push(resolve); waiting = releases.length; });
    if (failure) throw new Error("Synthetic blocked list failure");
    return rows;
  }

  async function unblock(id: string, jid: string) {
    calls = [...calls, `unblock ${id} ${jid}`];
    if (failUnblock) throw new Error("Synthetic unblock refusal");
    removed[id] = true;
  }

  function release() { const next = releases.shift(); waiting = releases.length; next?.(); }
</script>

<h1>Synthetic blocked contacts</h1>
<label>Account <select bind:value={account}><option>alpha</option><option>beta</option></select></label>
<label><input type="checkbox" bind:checked={connected} /> Connected</label>
<label><input type="checkbox" bind:checked={deferList} /> Defer list</label>
<label><input type="checkbox" bind:checked={failList} /> Fail list</label>
<label><input type="checkbox" bind:checked={failUnblock} /> Fail unblock</label>
<button onclick={release} disabled={!waiting}>Release list {waiting}</button>
<p>Current account {account}</p>
<pre aria-label="Synthetic calls">{calls.join("\n")}</pre>
<BlockedContacts {account} {connected} onload={load} onunblock={unblock} />

<style>
  :global(body) { --muted: #555; --border: #aaa; font-family: sans-serif; max-width: 800px; margin: 2rem auto; }
  label { margin-right: 1rem; }
</style>
