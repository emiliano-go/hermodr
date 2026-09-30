<script lang="ts">
  import { onMount } from "svelte";
  import { fixture } from "./transcription164";
  import Transcript from "$lib/messages/Transcript.svelte";
  import TranscriptionSettings from "$lib/settings/TranscriptionSettings.svelte";
  import TranscriptionOverride from "$lib/settings/TranscriptionOverride.svelte";
  import { transcription } from "$lib/state/transcription.svelte";
  let account = $state("synthetic-A");
  let auto = $state(false);
  let hold = $state(false);
  let failure = $state(false);
  let hidden = $state(false);
  let revision = $state(0);
  onMount(() => transcription.start());
</script>

<main>
  <h1>Synthetic transcription fixture</h1>
  <p>No provider, credentials, native prompt, media file or WhatsApp access. Synthetic responses only.</p>
  <section aria-label="Synthetic controls">
    <button onclick={() => account = account === "synthetic-A" ? "synthetic-B" : "synthetic-A"}>Switch synthetic account</button>
    <span>{account}</span>
    <label><input type="checkbox" bind:checked={hold} onchange={() => fixture.hold = hold} /> Hold synthetic transcription</label>
    <label><input type="checkbox" bind:checked={failure} onchange={() => fixture.failure = failure} /> Synthetic provider failure</label>
    <label><input type="checkbox" bind:checked={hidden} /> Conceal synthetic spoiler</label>
    <button onclick={() => { for (const pending of [...fixture.pending.values()]) pending.finish(); }}>Resolve synthetic transcription</button>
    <button onclick={() => revision++}>Show synthetic requests</button>
  </section>
  <article aria-label="Synthetic voice note">
    <h2>Voice note</h2>
    <Transcript accountId={account} chat="synthetic@chat" id="voice" enabled={transcription.enabled} {hidden} />
  </article>
  <TranscriptionOverride accountId={account} chat="synthetic@chat" />
  <TranscriptionSettings autoTranscribe={auto} onAutoTranscribe={async (enabled) => { auto = enabled; }} />
  {#key revision}<output aria-label="Synthetic requests">{JSON.stringify(fixture.calls)}</output>{/key}
</main>

<style>
  :global(:root) { --bg: #111b21; --raised: #2a3942; --line: #3b4a54; --text: #e9edef; --muted: #8696a0; --danger: #f15c6d; }
  :global(body) { margin: 0; background: var(--bg); color: var(--text); font-family: system-ui; }
  main { max-width: 760px; padding: 20px; margin: auto; }
  section, article { border: 1px solid var(--line); padding: 12px; margin: 12px 0; }
  label { display: block; margin: 10px 0; }
  button { margin: 5px; }
  output { display: block; overflow-wrap: anywhere; font-size: 12px; }
</style>
