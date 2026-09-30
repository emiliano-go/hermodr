<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "$lib/utils/ipc";
  import type { StoredTranscript, TranscriptionEvent } from "$lib/utils/wire";
  let { accountId, chat, id, enabled, hidden = false }: {
    accountId: string; chat: string; id: string; enabled: boolean; hidden?: boolean;
  } = $props();
  let transcript = $state<StoredTranscript | null>(null);
  let busy = $state(false);
  let error = $state("");
  let generation = 0;
  $effect(() => {
    const token = ++generation;
    transcript = null; busy = false; error = "";
    if (!accountId || !enabled || hidden) return;
    void invoke<StoredTranscript | null>("message_transcript", { accountId, chat, id }).then(
      (value) => { if (token === generation) transcript = value; },
      (failure) => { if (token === generation) error = String(failure); },
    );
  });
  onMount(() => {
    let alive = true;
    let off: (() => void) | undefined;
    void listen<TranscriptionEvent>("transcription-event", ({ payload }) => {
      if (!alive || hidden || payload.account_id !== accountId || payload.chat !== chat || payload.id !== id) return;
      busy = payload.status === "started";
      if (payload.transcript) transcript = payload.transcript;
      error = payload.error ?? "";
    }).then((stop) => { if (alive) off = stop; else stop(); }, (failure) => { if (alive) error = String(failure); });
    return () => { alive = false; off?.(); generation++; };
  });
  async function transcribe(force = false) {
    if (busy || !enabled || hidden) return;
    const token = generation;
    busy = true; error = "";
    try {
      const value = await invoke<StoredTranscript>("transcribe_message", { accountId, chat, id, force, automatic: false });
      if (token === generation) transcript = value;
    } catch (failure) { if (token === generation) error = String(failure); }
    finally { if (token === generation) busy = false; }
  }
  async function cancel() {
    const token = generation;
    try { await invoke("cancel_transcription", { accountId, chat, id }); }
    catch (failure) { if (token === generation) error = String(failure); }
  }
</script>

{#if enabled && !hidden}
  <div class="transcription" aria-label="Voice note transcription">
    {#if busy}
      <span role="status">Transcribing…</span>
      <button onclick={cancel}>Cancel</button>
    {:else if transcript}
      <p>{transcript.text || "No speech detected."}</p>
      <small>{transcript.provider}{transcript.language ? ` · ${transcript.language}` : ""}</small>
      <button onclick={() => transcribe(true)}>Re-transcribe</button>
    {:else}
      <button onclick={() => transcribe()}>Transcribe</button>
    {/if}
    {#if error}<p role="alert">{error}</p>{/if}
  </div>
{/if}

<style>
  .transcription { margin-top: .5rem; font-size: .85rem; }
  p { white-space: pre-wrap; overflow-wrap: anywhere; margin: .4rem 0; }
  small, [role="status"] { color: var(--muted); }
  button { color: var(--muted); background: none; border: none; text-decoration: underline; cursor: pointer; margin-right: .5rem; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
