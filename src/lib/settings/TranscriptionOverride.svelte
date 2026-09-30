<script lang="ts">
  import { invoke } from "$lib/utils/ipc";
  let { accountId, chat }: { accountId: string; chat: string } = $props();
  let value = $state("");
  let busy = $state(false);
  let error = $state("");
  let generation = 0;
  $effect(() => {
    const token = ++generation;
    value = ""; busy = true; error = "";
    void invoke<boolean | null>("chat_auto_transcribe", { accountId, chat }).then(
      (enabled) => { if (token === generation) value = enabled === null ? "" : String(enabled); },
      (failure) => { if (token === generation) error = String(failure); },
    ).finally(() => { if (token === generation) busy = false; });
  });
  async function change(next: string) {
    const token = generation;
    busy = true; error = "";
    try {
      await invoke("set_chat_auto_transcribe", { accountId, chat, enabled: next === "" ? null : next === "true" });
      if (token === generation) value = next;
    } catch (failure) { if (token === generation) error = String(failure); }
    finally { if (token === generation) busy = false; }
  }
</script>

<label>Auto-transcribe voice notes
  <select {value} disabled={busy} onchange={(event) => change(event.currentTarget.value)}>
    <option value="">Follow global setting</option>
    <option value="true">On</option>
    <option value="false">Off</option>
  </select>
</label>
<p>Waits for downloaded audio. Cloud providers require consent in transcription settings.</p>
{#if error}<p role="alert">{error}</p>{/if}

<style>
  label { display: flex; align-items: center; justify-content: space-between; gap: .75rem; }
  p { color: var(--muted); font-size: .8rem; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
