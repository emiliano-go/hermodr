<script lang="ts">
  import { untrack } from "svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import { invoke } from "$lib/utils/ipc";

  import type { BooleanProp as Prop } from "$lib/utils/wire";
  let open = $state(false);
  let watching = $state(false);
  let loading = $state(false);
  let error = $state("");
  let query = $state("");
  let props = $state<Prop[]>([]);
  let refreshed = $state("");
  let matching = $derived(props.filter((prop) => `${prop.name} ${prop.code}`.includes(query.trim().toLowerCase())));

  async function refresh() {
    if (loading) return;
    loading = true;
    try {
      props = await invoke<Prop[]>("boolean_props");
      refreshed = new Date().toLocaleTimeString();
      error = "";
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (!open) return;
    void untrack(refresh);
    if (!watching) return;
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  });
</script>

<details bind:open>
  <summary><span>Server feature flags</span><span class="chev"><Icon name="chevronDown" size={16} /></span></summary>
  <p>Boolean A/B properties received from WhatsApp. Available for diagnostics; Postal does not use these flags to gate features.</p>
  <div class="controls">
    <button onclick={refresh} disabled={loading}>Refresh flags</button>
    <label><input type="checkbox" bind:checked={watching} /> Watch every 5 seconds</label>
  </div>
  <label class="filter">Filter flags <input type="search" bind:value={query} /></label>
  {#if error}<p role="alert">{error}{refreshed ? " (Showing previous values.)" : ""}</p>{/if}
  <p role="status">{matching.length} flags{refreshed ? ` · Updated ${refreshed}` : ""}{matching.length > 100 ? " · Showing first 100; filter to narrow." : ""}</p>
  <div class="table">
    <table>
      <thead><tr><th>Flag</th><th>Server value</th><th>Default</th></tr></thead>
      <tbody>
        {#each matching.slice(0, 100) as prop}
          <tr><td>{prop.name}<small>{prop.code}</small></td><td>{prop.value === null ? "Not received" : String(prop.value)}</td><td>{String(prop.default)}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</details>

<style>
  details { margin-top: 1rem; }
  summary { display: flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 600; list-style: none; user-select: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: grid; margin-left: auto; color: var(--muted); transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease); }
  details[open] .chev { transform: rotate(180deg); }
  p { color: var(--muted); font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
  .controls { display: flex; align-items: center; flex-wrap: wrap; gap: .8rem; }
  .filter { display: grid; gap: .4rem; margin-top: .8rem; }
  input[type="search"] { width: 100%; box-sizing: border-box; }
  button, input[type="search"] { background: var(--raised); color: var(--text); border: 1px solid var(--line-strong); padding: .5rem; border-radius: .3rem; font: inherit; }
  .table { overflow: auto; max-height: 22rem; }
  table { width: 100%; border-collapse: collapse; font-size: .8rem; }
  th, td { text-align: left; padding: .4rem; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  small { display: block; color: var(--muted); }
</style>
