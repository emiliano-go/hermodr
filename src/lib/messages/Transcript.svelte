<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { invoke } from "$lib/utils/ipc";
  import type { StoredTranscript, TranscriptionEvent } from "$lib/utils/wire";
  let { accountId, chat, id, enabled, hidden = false }: {
    accountId: string; chat: string; id: string; enabled: boolean; hidden?: boolean;
  } = $props();
  let transcript = $state<StoredTranscript | null>(null);
  let busy = $state(false);
  let queued = $state(false);
  let error = $state<LocalizedError | null>(null);
  let generation = 0;
  $effect(() => {
    const token = ++generation;
    transcript = null; busy = false; queued = false; error = null;
    if (!accountId || !enabled || hidden) return;
    void invoke<StoredTranscript | null>("message_transcript", { accountId, chat, id }).then(
      (value) => { if (token === generation) transcript = value; },
      (failure) => { if (token === generation) error = normalizeError(failure); },
    );
  });
  onMount(() => {
    let alive = true;
    let off: (() => void) | undefined;
    void listen<TranscriptionEvent>("transcription-event", ({ payload }) => {
      if (!alive || hidden || payload.account_id !== accountId || payload.chat !== chat || payload.id !== id) return;
      queued = payload.status === "queued";
      busy = queued || payload.status === "started";
      if (payload.transcript) transcript = payload.transcript;
      error = payload.error_message || payload.error ? normalizeError({ kind: "postal_error",
        ...(payload.error_message ?? { code: "error.operation_failed", params: {} }),
        diagnostic: payload.diagnostic ?? payload.error ?? undefined }) : null;
    }).then((stop) => { if (alive) off = stop; else stop(); }, (failure) => { if (alive) error = normalizeError(failure); });
    return () => { alive = false; off?.(); generation++; };
  });
  async function transcribe(force = false) {
    if (busy || !enabled || hidden) return;
    const token = generation;
    busy = true; queued = false; error = null;
    try {
      const value = await invoke<StoredTranscript>("transcribe_message", { accountId, chat, id, force, automatic: false });
      if (token === generation) transcript = value;
    } catch (failure) { if (token === generation) error = normalizeError(failure); }
    finally { if (token === generation) busy = false; }
  }
  async function cancel() {
    const token = generation;
    try { await invoke("cancel_transcription", { accountId, chat, id }); }
    catch (failure) { if (token === generation) error = normalizeError(failure); }
  }
</script>

{#if enabled && !hidden}
  <div class="transcription" aria-label={t("content.voice_note_transcription")}>
    {#if busy}
      <span role="status">{queued ? t("content.transcription_queued") : t("content.transcribing")}</span>
      <button onclick={cancel}>{t("content.cancel")}</button>
    {:else if transcript}
      <p>{transcript.text || t("content.no_speech_detected")}</p>
      <small>{transcript.provider}{transcript.language ? ` · ${transcript.language}` : ""}</small>
      <button onclick={() => transcribe(true)}>{t("content.re_transcribe")}</button>
    {:else}
      <button onclick={() => transcribe()}>{t("content.transcribe")}</button>
    {/if}
    {#if error}<p role="alert">{error.message}</p>{#if error.diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="ltr">{error.diagnostic}</pre></details>{/if}{/if}
  </div>
{/if}

<style>
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  .transcription { margin-top: .5rem; font-size: .85rem; }
  p { white-space: pre-wrap; overflow-wrap: anywhere; margin: .4rem 0; }
  small, [role="status"] { color: var(--muted); }
  button { color: var(--muted); background: none; border: none; text-decoration: underline; cursor: pointer; margin-inline-end: .5rem; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
