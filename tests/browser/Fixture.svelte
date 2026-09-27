<script lang="ts">
  import BooleanProps from "$lib/BooleanProps.svelte";
  import ChatMetadata from "./ChatMetadata.svelte";
  import { fixture } from "./ipc";
  let mounted = $state(true);
  let calls = $state(0);
</script>

<h1>Synthetic flag diagnostics</h1>
<p>No backend, account, database, network service or native IPC.</p>
<button onclick={() => { fixture.updated = true; }}>Apply synthetic delta</button>
<button onclick={() => { fixture.failure = true; }}>Simulate failure</button>
<button onclick={() => { mounted = !mounted; }}>Toggle diagnostics</button>
<button onclick={() => { calls = fixture.calls; }}>Count reads</button>
<output aria-label="Read count">{calls}</output>
{#if mounted}<BooleanProps />{/if}
<details><summary>Retained chat metadata</summary><ChatMetadata /></details>

<style>
  :global(:root) { --raised: #233138; --text: #eee; --border: #53616a; --muted: #b5c5cd; }
  :global(body) { font: 16px system-ui; background: #162026; color: var(--text); max-width: 850px; margin: 30px auto; }
</style>
