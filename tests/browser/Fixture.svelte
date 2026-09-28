<script lang="ts">
  import BooleanProps from "$lib/BooleanProps.svelte";
  import ChatMetadata from "./ChatMetadata.svelte";
  import RetentionSettings from "./RetentionSettings.svelte";
  import StorageManager from "$lib/StorageManager.svelte";
  import MessageWindow from "./MessageWindow.svelte";
  import MediaRetry from "./MediaRetry.svelte";
  import ArchiveManager from "$lib/ArchiveManager.svelte";
  import Upload from "./Upload.svelte";
  import { archiveFixture } from "./ipc";
  import { session } from "$lib/state/session.svelte";
  import { fixture } from "./ipc";
  let mounted = $state(true);
  let calls = $state(0);
  let cleanupCalls = $state(0);
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
<RetentionSettings />
<button onclick={() => { fixture.storageFailure = !fixture.storageFailure; }}>Toggle cleanup failure</button>
<button onclick={() => { cleanupCalls = fixture.storageCalls; }}>Count cleanup calls</button>
<output aria-label="Cleanup calls">{cleanupCalls}</output>
<StorageManager />
<MessageWindow />
<MediaRetry />
<button onclick={() => { archiveFixture.failure = !archiveFixture.failure; }}>Toggle archive failure</button>
<button onclick={() => { archiveFixture.cancelled = !archiveFixture.cancelled; }}>Toggle archive cancellation</button>
<button onclick={() => { archiveFixture.deferNext = true; }}>Delay next archive operation</button>
<button onclick={() => { archiveFixture.pending.shift()?.(); }}>Release archive operation</button>
<ArchiveManager />
<Upload />
<p aria-label="Account fixture status">Active: {session.activeAccount ?? "unloaded"}; accounts: {session.accountList.map((account) => account.id).join(", ")}</p>

<style>
  :global(:root) { --raised: #233138; --text: #eee; --border: #53616a; --muted: #b5c5cd; }
  :global(body) { font: 16px system-ui; background: #162026; color: var(--text); max-width: 850px; margin: 30px auto; }
</style>
