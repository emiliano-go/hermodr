<script lang="ts">
  import PluginManager from "$lib/settings/PluginManager.svelte";
  import { pluginFixture } from "./ipc";
</script>

<h1>Synthetic plugin consent fixture</h1>
<p>No native host, installed plugins, real accounts or database.</p>
<button onclick={() => { pluginFixture.failure = !pluginFixture.failure; }}>Toggle plugin save failure</button>
<button onclick={() => { pluginFixture.enabled = false; pluginFixture.crashed = true; }}>Simulate plugin crash limit</button>
<PluginManager />
