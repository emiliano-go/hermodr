<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onMount } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { transcription } from "$lib/state/transcription.svelte";
  import type { TranscriptionView, TranscriptionSettings as Settings } from "$lib/utils/wire";
  let { autoTranscribe, onAutoTranscribe }: { autoTranscribe: boolean; onAutoTranscribe: (enabled: boolean) => Promise<void> } = $props();
  let view = $state<TranscriptionView | null>(null);
  let draft = $state<Settings | null>(null);
  let busy = $state(false);
  let error = $state<LocalizedError | string>("");
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
    catch (failure) { error = normalizeError(failure); }
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

<section aria-label={t("settings.transcription_label")}>
  <h3>{t("settings.transcription")}</h3>
  {#if draft && view}
    <label>{t("settings.plugin")}
      <select value={draft.plugin_id ?? ""} disabled={busy} onchange={(event) => choosePlugin(event.currentTarget.value)}>
        <option value="">{t("ui.disabled")}</option>
        {#each view.plugins as plugin}<option value={plugin.id}>{plugin.name}</option>{/each}
      </select>
    </label>
    {#if !view.plugins.length}<p>{t("settings.transcription_install")}</p>{/if}
    {#if plugin}
      <label>{t("settings.provider")}
        <select bind:value={draft.provider} disabled={busy}>
          <option value="" disabled>{t("settings.provider_choose")}</option>
          {#each providers as provider}<option value={provider.id}>{provider.name}{provider.transmits_audio ? t("settings.provider_cloud_suffix") : t("settings.provider_local_suffix")}</option>{/each}
        </select>
      </label>
      {#if !plugin.enabled}
        <label class="setting">
          <div>
            <span class="setting-title">{t("settings.transcription_trust")}</span>
            <span class="setting-desc">{t("settings.native_plugin_access")}</span>
          </div>
          <input class="switch" type="checkbox" bind:checked={trust} disabled={busy} />
        </label>
        <button class="button" disabled={busy || !trust || !provider} onclick={() => perform(() => enable(true), true)}>{t("settings.grant_enable")}</button>
      {:else}
        <button class="button" disabled={busy} onclick={() => perform(() => enable(false), true)}>{t("settings.transcription_disable")}</button>
      {/if}
      {#if provider?.transmits_audio}
        <p>{t("settings.transcription_cloud_hint")}</p>
        <label class="setting">
          <div>
            <span class="setting-title">{t("settings.transcription_consent")}</span>
            <span class="setting-desc">{t("settings.transcription_consent_auto")}</span>
          </div>
          <input class="switch" type="checkbox" checked={cloudConsent} disabled={busy} onchange={(event) => perform(() => consent(event.currentTarget.checked), true)} />
        </label>
        {#if provider.requires_key}
          <p>{t("settings.api_key_state", { state: t(view.settings.plugin_id === draft.plugin_id && view.settings.provider === draft.provider && view.key_configured ? "settings.api_key_configured" : "settings.api_key_missing") })}</p>
          <button class="button" disabled={busy} onclick={() => perform(() => key(false), true)}>{t("settings.api_key_configure")}</button>
          <button class="button" disabled={busy} onclick={() => perform(() => key(true), true)}>{t("settings.api_key_remove")}</button>
        {/if}
      {/if}
      {#if provider?.kind === "local" || provider?.id === "openai"}
        <label>{t("settings.decoder_executable")}<input dir="ltr" value={draft.decoder_executable ?? ""} disabled={busy} onchange={(event) => draft && (draft.decoder_executable = event.currentTarget.value || null)} placeholder={t("settings.decoder_path")} /></label>
      {/if}
      {#if provider?.id === "local-whisper"}
        <label>{t("settings.whisper_executable")}<input dir="ltr" value={draft.whisper_executable ?? ""} disabled={busy} onchange={(event) => draft && (draft.whisper_executable = event.currentTarget.value || null)} placeholder={t("settings.whisper_path")} /></label>
        <label>{t("settings.model_filename")}<input dir="ltr" value={draft.model ?? ""} disabled={busy} onchange={(event) => draft && (draft.model = event.currentTarget.value || null)} /></label>
        <label>{t("settings.model_hash")}<input dir="ltr" value={draft.model_sha256 ?? ""} disabled={busy} onchange={(event) => draft && (draft.model_sha256 = event.currentTarget.value || null)} /></label>
        {#if view.data_directory}<p>{t("settings.models_directory", { path: view.data_directory })}</p>{/if}
        <label>{t("settings.model_url")}<input type="url" dir="ltr" bind:value={modelUrl} disabled={busy} placeholder={t("settings.url_example")} /></label>
        <p>{t("settings.model_download_hint")}</p>
        <button class="button" disabled={busy || !plugin.enabled || !modelUrl.startsWith("https://") || !draft.model || draft.model_sha256?.length !== 64} onclick={() => perform(installModel, true)}>{t("settings.model_download")}</button>
      {/if}
      <label>{t("settings.transcription_language")}<input dir="ltr" value={draft.language ?? ""} disabled={busy} onchange={(event) => draft && (draft.language = event.currentTarget.value || null)} placeholder={t("settings.transcription_language_hint")} /></label>
      <label>{t("settings.transcription_idle")}<input type="number" min="1" max="86400" value={draft.idle_timeout_secs ?? ""} disabled={busy} onchange={(event) => draft && (draft.idle_timeout_secs = event.currentTarget.value ? Number(event.currentTarget.value) : null)} placeholder={t("settings.immediate")} /></label>
      <button class="button" disabled={busy || !provider} onclick={() => perform(save, true)}>{t("settings.transcription_save")}</button>
    {:else}
      <button class="button" disabled={busy} onclick={() => perform(save, true)}>{t("settings.transcription_save")}</button>
    {/if}
    <label class="setting">
      <div>
        <span class="setting-title">{t("settings.transcription_auto_downloaded")}</span>
        <span class="setting-desc">{t("settings.transcription_auto_hint")}</span>
      </div>
      <input class="switch" type="checkbox" checked={autoTranscribe} disabled={busy} onchange={(event) => perform(() => onAutoTranscribe(event.currentTarget.checked))} />
    </label>
    {#each view.errors as failure}<p role="alert">{failure}</p>{/each}
  {/if}
  {#if busy}<p role="status">{t("ui.working")}</p>{/if}
  {#if error}<p role="alert">{error}</p>{/if}
</section>

<style>
  section { padding: 1rem 0; }
  h3 { margin: 0 0 .8rem; }
  label:not(.setting) { display: flex; flex-direction: column; gap: .4rem; margin: .7rem 0; }
  input:not([type="checkbox"]), select { color: var(--text); background: var(--raised); border: 1px solid var(--line); border-radius: 4px; padding: .5rem; font: inherit; }
  p { color: var(--muted); font-size: .85rem; overflow-wrap: anywhere; }
  button { margin: .3rem .5rem .3rem 0; }
  [role="alert"] { color: var(--danger, #b91c1c); }
</style>
