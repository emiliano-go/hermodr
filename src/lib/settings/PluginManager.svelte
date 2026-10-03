<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onMount } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import type { PluginInfo as Plugin, PluginsView as View } from "$lib/utils/wire";
  let view = $state<View>({ plugins: [], directory: "", errors: [] });
  let error = $state<LocalizedError | string>("");
  let loadError = $state<LocalizedError | string>("");
  let busy = $state(false);
  let selected = $state<Plugin | null>(null);
  let consent = $state(false);
  async function refresh() {
    try { view = await invoke<View>("list_plugins"); loadError = ""; }
    catch (failure) { loadError = normalizeError(failure); }
  }
  onMount(() => {
    void refresh();
    const interval = setInterval(() => { if (!busy) void refresh(); }, 2000);
    return () => clearInterval(interval);
  });
  async function change(plugin: Plugin, enabled: boolean) {
    if (busy || (enabled && !consent)) return;
    busy = true;
    error = "";
    try {
      await invoke("set_plugin_enabled", { id: plugin.id, enabled, capabilities: enabled ? plugin.capabilities : [] });
      selected = null;
      consent = false;
      await refresh();
    } catch (failure) { error = normalizeError(failure); }
    finally { busy = false; }
  }
</script>

<div class="plugins">
  {#if view.directory}<p class="path">{t("settings.plugin_directory")} <bdi>{view.directory}</bdi></p>{/if}
  {#if !view.plugins.length}<p>{t("settings.plugins_empty")}</p>{/if}
  {#each view.plugins as plugin (plugin.id)}
    <article>
      <h3><bdi>{plugin.name}</bdi> <small>{plugin.version}</small></h3>
      <p>{plugin.id} · {plugin.activation} · {plugin.enabled ? plugin.state : "disabled"}</p>
      <p>{t("settings.plugin_capabilities")} <bdi>{plugin.capabilities.join(", ")}</bdi></p>
      {#if plugin.activation === "lazy"}<p>{t("settings.plugin_unload")}{plugin.idle_timeout_secs ? t("settings.plugin_idle", { count: plugin.idle_timeout_secs }) : ""}.</p>{/if}
      {#if plugin.error}<p role="alert">{plugin.error}</p>{/if}
      {#if plugin.enabled}
        <button class="button" disabled={busy} onclick={() => change(plugin, false)}>{t("settings.plugin_disable", { name: plugin.name })}</button>
      {:else}
        <button class="button" disabled={busy} onclick={() => { selected = plugin; consent = false; }}>{t("settings.plugin_enable", { name: plugin.name })}…</button>
      {/if}
    </article>
  {/each}
  {#if selected}
    <section aria-label={t("settings.plugin_permission")}>
      <h3>{t("settings.plugin_enable", { name: selected.name })}?</h3>
      {#if selected.capabilities.includes("transcribe")}
        <p>{t("settings.plugin_transcribe_hint")}</p>
      {:else}
        <p>{t("settings.plugin_events_hint")}</p>
      {/if}
      <p>{t("settings.plugin_security")}</p>
      <label><input type="checkbox" bind:checked={consent} disabled={busy} /> {t("settings.plugin_trust", { capabilities: selected.capabilities.join(", ") })}</label>
      <div class="actions">
        <button class="button" disabled={busy || !consent} onclick={() => selected && change(selected, true)}>{t("settings.grant_enable")}</button>
        <button class="button" disabled={busy} onclick={() => { selected = null; consent = false; }}>{t("ui.cancel")}</button>
      </div>
    </section>
  {/if}
  {#each view.errors as failure}<p role="alert">{failure}</p>{/each}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if loadError}<p role="alert">{loadError}</p>{/if}
</div>

<style>
  p { color: var(--muted); font-size: .85rem; overflow-wrap: anywhere; }
  article, section { border: 1px solid var(--line-strong); padding: 1rem; margin: 1rem 0; border-radius: 6px; }
  h3 { margin: 0; font-size: 1rem; }
  small { color: var(--muted); font-weight: normal; }
  label { display: flex; gap: .5rem; align-items: start; }
  .actions { display: flex; gap: .5rem; margin-top: 1rem; }
  [role="alert"] { color: var(--danger); }
</style>
