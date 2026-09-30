<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import type { PluginInfo as Plugin, PluginsView as View } from "$lib/utils/wire";
  let view = $state<View>({ plugins: [], directory: "", errors: [] });
  let error = $state("");
  let loadError = $state("");
  let busy = $state(false);
  let selected = $state<Plugin | null>(null);
  let consent = $state(false);
  async function refresh() {
    try { view = await invoke<View>("list_plugins"); loadError = ""; }
    catch (failure) { loadError = String(failure); }
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
    } catch (failure) { error = String(failure); }
    finally { busy = false; }
  }
</script>

<div class="plugins">
  {#if view.directory}<p class="path">Plugin directory: {view.directory}</p>{/if}
  {#if !view.plugins.length}<p>No plugins discovered.</p>{/if}
  {#each view.plugins as plugin (plugin.id)}
    <article>
      <h3>{plugin.name} <small>{plugin.version}</small></h3>
      <p>{plugin.id} · {plugin.activation} · {plugin.enabled ? plugin.state : "disabled"}</p>
      <p>Capabilities: {plugin.capabilities.join(", ")}</p>
      {#if plugin.activation === "lazy"}<p>Unload after completed work{plugin.idle_timeout_secs ? ` and ${plugin.idle_timeout_secs}s idle` : ""}.</p>{/if}
      {#if plugin.error}<p role="alert">{plugin.error}</p>{/if}
      {#if plugin.enabled}
        <button class="button" disabled={busy} onclick={() => change(plugin, false)}>Disable {plugin.name}</button>
      {:else}
        <button class="button" disabled={busy} onclick={() => { selected = plugin; consent = false; }}>Enable {plugin.name}…</button>
      {/if}
    </article>
  {/each}
  {#if selected}
    <section aria-label="Plugin permission request">
      <h3>Enable {selected.name}?</h3>
      {#if selected.capabilities.includes("transcribe")}
        <p><strong>transcribe</strong> shares audio selected for transcription. Local providers keep audio on this device; cloud providers need separate explicit consent.</p>
      {:else}
        <p><strong>events:read</strong> shares message content, message details and service events from every connected account. Pairing QR codes are excluded.</p>
      {/if}
      <p>This permission restricts Postal’s plugin API. Native plugins are not sandboxed: they can access files and networks with your operating-system permissions.</p>
      <label><input type="checkbox" bind:checked={consent} disabled={busy} /> I trust this plugin and grant {selected.capabilities.join(", ")} for all accounts.</label>
      <div class="actions">
        <button class="button" disabled={busy || !consent} onclick={() => selected && change(selected, true)}>Grant and enable</button>
        <button class="button" disabled={busy} onclick={() => { selected = null; consent = false; }}>Cancel</button>
      </div>
    </section>
  {/if}
  {#each view.errors as failure}<p role="alert">{failure}</p>{/each}
  {#if error}<p role="alert">{error}</p>{/if}
  {#if loadError}<p role="alert">{loadError}</p>{/if}
</div>

<style>
  p { color: var(--muted); font-size: .85rem; overflow-wrap: anywhere; }
  article, section { border: 1px solid var(--border); padding: 1rem; margin: 1rem 0; border-radius: 6px; }
  h3 { margin: 0; font-size: 1rem; }
  small { color: var(--muted); font-weight: normal; }
  label { display: flex; gap: .5rem; align-items: start; }
  .actions { display: flex; gap: .5rem; margin-top: 1rem; }
  [role="alert"] { color: #f66; }
</style>
