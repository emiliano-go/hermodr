<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { transcription } from "$lib/state/transcription.svelte";
  import type { TranscriptionView, TranscriptionSettings as Settings } from "$lib/utils/wire";
  let { autoTranscribe, onAutoTranscribe }: { autoTranscribe: boolean; onAutoTranscribe: (enabled: boolean) => Promise<void> } = $props();
  let view = $state<TranscriptionView | null>(null);
  let draft = $state<Settings | null>(null);
  let busy = $state(false);
  let error = $state("");
  let trust = $state(false);
  let modelUrl = $state("");
  let plugin = $derived(view?.plugins.find((p) => p.id === draft?.plugin_id));
  let providers = $derived(plugin?.contributes.transcription?.providers ?? []);
  let provider = $derived(providers.find((p) => p.id === draft?.provider));
  let cloudConsent = $derived(view?.cloud_consents.some((c) => c.plugin_id === draft?.plugin_id && c.provider === draft?.provider) ?? false);
  async function refresh() {
    view = await invoke<TranscriptionView>("transcription_settings");
    transcription.configure(view);
    draft = { ...view.settings };
  }
  async function perform(work: () => Promise<void>, reload = false) {
    if (busy) return;
    busy = true; error = "";
    try { await work(); if (reload) await refresh(); }
    catch (failure) { error = String(failure); }
    finally { busy = false; }
  }
  onMount(() => { void perform(refresh); });
  function choosePlugin(id: string) {
    if (!draft) return;
    draft.plugin_id = id || null;
    const providers = view?.plugins.find((p) => p.id === id)?.contributes.transcription?.providers ?? [];
    draft.provider = providers.find((p) => p.kind === "local")?.id ?? "";
    draft.model ??= "tiny.bin";
    trust = false;
  }
  async function save() {
    if (draft) await invoke("set_transcription_settings", { settings: draft });
  }
  async function enable(enabled: boolean) {
    if (!plugin || (enabled && !trust)) return;
    await save();
    await invoke("set_plugin_enabled", { id: plugin.id, enabled, capabilities: enabled ? ["transcribe"] : [] });
  }
  async function consent(approved: boolean) {
    if (!draft?.plugin_id) return;
    await save();
    await invoke("grant_transcription_cloud_consent", { pluginId: draft.plugin_id, providerId: draft.provider, approved });
  }
  async function key(forget: boolean) {
    if (!draft?.plugin_id) return;
    await save();
    await invoke(forget ? "forget_transcription_key" : "configure_transcription_key", { pluginId: draft.plugin_id, providerId: draft.provider });
  }
  async function installModel() {
    if (!draft?.plugin_id || !draft.model || !draft.model_sha256) return;
    await save();
    await invoke("install_transcription_model", { pluginId: draft.plugin_id, url: modelUrl, sha256: draft.model_sha256, filename: draft.model });
  }
</script>

<section aria-label="Speech to text settings">
  <h3>Speech to text</h3>
  {#if draft && view}
    <label>Plugin
      <select value={draft.plugin_id ?? ""} disabled={busy} onchange={(event) => choosePlugin(event.currentTarget.value)}>
        <option value="">Disabled</option>
        {#each view.plugins as plugin}<option value={plugin.id}>{plugin.name}</option>{/each}
      </select>
    </label>
    {#if !view.plugins.length}<p>Install a verified transcription sidecar, then restart Postal.</p>{/if}
    {#if plugin}
      <label>Provider
        <select bind:value={draft.provider} disabled={busy}>
          <option value="" disabled>Choose provider</option>
          {#each providers as provider}<option value={provider.id}>{provider.name}{provider.transmits_audio ? " · sends audio off-device" : " · local"}</option>{/each}
        </select>
      </label>
      {#if !plugin.enabled}
        <label class="toggle"><input type="checkbox" bind:checked={trust} disabled={busy} /> I trust this native plugin and grant transcription of selected audio. Native plugins can access files and networks.</label>
        <button class="button" disabled={busy || !trust || !provider} onclick={() => perform(() => enable(true), true)}>Grant and enable</button>
      {:else}
        <button class="button" disabled={busy} onclick={() => perform(() => enable(false), true)}>Disable transcription plugin</button>
      {/if}
      {#if provider?.transmits_audio}
        <p>This provider sends voice-note audio off-device. API usage may incur charges.</p>
        <label class="toggle"><input type="checkbox" checked={cloudConsent} disabled={busy} onchange={(event) => perform(() => consent(event.currentTarget.checked), true)} /> I consent to this provider receiving audio, including automatically transcribed notes.</label>
        {#if provider.requires_key}
          <p>API key {view.settings.plugin_id === draft.plugin_id && view.settings.provider === draft.provider && view.key_configured ? "configured" : "not configured"}. Keys stay in the operating-system credential store; entry opens a native prompt.</p>
          <button class="button" disabled={busy} onclick={() => perform(() => key(false), true)}>Configure API key</button>
          <button class="button" disabled={busy} onclick={() => perform(() => key(true), true)}>Remove API key</button>
        {/if}
      {/if}
      {#if provider?.kind === "local" || provider?.id === "openai"}
        <label>Audio decoder executable<input value={draft.decoder_executable ?? ""} disabled={busy} onchange={(event) => draft && (draft.decoder_executable = event.currentTarget.value || null)} placeholder="Absolute path to user-installed ffmpeg" /></label>
      {/if}
      {#if provider?.id === "local-whisper"}
        <label>Whisper executable<input value={draft.whisper_executable ?? ""} disabled={busy} onchange={(event) => draft && (draft.whisper_executable = event.currentTarget.value || null)} placeholder="Absolute path to whisper-cli" /></label>
        <label>Model filename<input value={draft.model ?? ""} disabled={busy} onchange={(event) => draft && (draft.model = event.currentTarget.value || null)} /></label>
        <label>Model SHA-256<input value={draft.model_sha256 ?? ""} disabled={busy} onchange={(event) => draft && (draft.model_sha256 = event.currentTarget.value || null)} /></label>
        {#if view.data_directory}<p>Models stay in {view.data_directory}.</p>{/if}
        <label>Model download URL<input type="url" bind:value={modelUrl} disabled={busy} placeholder="https://…" /></label>
        <p>Explicit download only, up to 256 MiB. Postal verifies SHA-256 before installing; existing models are kept.</p>
        <button class="button" disabled={busy || !plugin.enabled || !modelUrl.startsWith("https://") || !draft.model || draft.model_sha256?.length !== 64} onclick={() => perform(installModel, true)}>Download and verify model</button>
      {/if}
      <label>Language<input value={draft.language ?? ""} disabled={busy} onchange={(event) => draft && (draft.language = event.currentTarget.value || null)} placeholder="Auto-detect, or language code" /></label>
      <label>Idle unload delay, seconds<input type="number" min="1" max="86400" value={draft.idle_timeout_secs ?? ""} disabled={busy} onchange={(event) => draft && (draft.idle_timeout_secs = event.currentTarget.value ? Number(event.currentTarget.value) : null)} placeholder="Immediate" /></label>
      <button class="button" disabled={busy || !provider} onclick={() => perform(save, true)}>Save transcription settings</button>
    {:else}
      <button class="button" disabled={busy} onclick={() => perform(save, true)}>Save transcription settings</button>
    {/if}
    <label class="toggle"><input type="checkbox" checked={autoTranscribe} disabled={busy} onchange={(event) => perform(() => onAutoTranscribe(event.currentTarget.checked))} /> Auto-transcribe downloaded voice notes</label>
    <p>Off by default. Chat overrides take precedence. Automatic transcription never downloads audio and skips concealed spoilers.</p>
    {#each view.errors as failure}<p role="alert">{failure}</p>{/each}
  {/if}
  {#if busy}<p role="status">Working…</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>

<style>
  section { padding: 1rem 0; }
  h3 { margin: 0 0 .8rem; }
  label { display: flex; flex-direction: column; gap: .4rem; margin: .7rem 0; }
  .toggle { flex-direction: row; align-items: start; }
  input, select { color: var(--text); background: var(--raised); border: 1px solid var(--line); border-radius: 4px; padding: .5rem; }
  input[type="checkbox"] { margin-top: .2rem; }
  p { color: var(--muted); font-size: .85rem; overflow-wrap: anywhere; }
  button { margin: .3rem .5rem .3rem 0; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
