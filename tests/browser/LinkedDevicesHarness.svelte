<script lang="ts">
  import LinkedDevices from "../../src/lib/settings/LinkedDevices.svelte";
  import type { LinkedDevice } from "../../src/lib/utils/wire";
  let account = $state("alpha");
  let connected = $state(true);
  let defer = $state(false);
  let failLoad = $state(false);
  let failUnlink = $state(false);
  let calls = $state<string[]>([]);
  let waiting = $state(0);
  let releases: (() => void)[] = [];
  let removed: Record<string, boolean> = {};
  async function load(id: string): Promise<LinkedDevice[]> {
    calls = [...calls, `load ${id}`];
    const failure = failLoad;
    const rows = [{ jid: `${id}:2@s.whatsapp.net`, device_id: 2, is_current: true, can_unlink: false },
      ...(!removed[id] ? [{ jid: `${id}:${id === "alpha" ? 3 : 7}@s.whatsapp.net`, device_id: id === "alpha" ? 3 : 7,
        is_current: false, can_unlink: true }] : [])];
    if (defer) await new Promise<void>((resolve) => { releases.push(resolve); waiting = releases.length; });
    if (failure) throw new Error("Synthetic device list failure");
    return rows;
  }
  async function unlink(id: string, jid: string) {
    calls = [...calls, `unlink ${id} ${jid}`];
    if (failUnlink) throw new Error("Synthetic unlink refusal");
    removed[id] = true;
  }
  function release() { const next = releases.shift(); waiting = releases.length; next?.(); }
</script>

<h1>Synthetic linked devices</h1>
<label>Account <select bind:value={account}><option>alpha</option><option>beta</option></select></label>
<label><input type="checkbox" bind:checked={connected} /> Connected</label>
<label><input type="checkbox" bind:checked={defer} /> Defer list</label>
<label><input type="checkbox" bind:checked={failLoad} /> Fail list</label>
<label><input type="checkbox" bind:checked={failUnlink} /> Fail unlink</label>
<button onclick={release} disabled={!waiting}>Release list {waiting}</button>
<p>Current account {account}</p>
<pre aria-label="Synthetic calls">{calls.join("\n")}</pre>
<LinkedDevices {account} {connected} onload={load} onunlink={unlink} />

<style>
  :global(body) { --muted: #555; --border: #aaa; font-family: sans-serif; max-width: 800px; margin: 2rem auto; }
  label { margin-right: 1rem; }
</style>
