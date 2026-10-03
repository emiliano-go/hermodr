<script lang="ts">
  import { t, formatTime } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import { invoke } from "$lib/utils/ipc";

  import type { BooleanProp as Prop } from "$lib/utils/wire";
  let open = $state(false);
  let watching = $state(false);
  let loading = $state(false);
  let error = $state<LocalizedError | string>("");
  let query = $state("");
  let props = $state<Prop[]>([]);
  let refreshed = $state<number | null>(null);
  let matching = $derived(props.filter((prop) => `${prop.name} ${prop.code}`.includes(query.trim().toLowerCase())));

  async function refresh() {
    if (loading) return;
    loading = true;
    try {
      props = await invoke<Prop[]>("boolean_props");
      refreshed = Date.now() / 1000;
      error = "";
    } catch (e) {
      error = normalizeError(e);
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
  <summary><span>{t("ui.server_feature_flags")}</span><span class="chev"><Icon name="chevronDown" size={16} /></span></summary>
  <p>{t("ui.flags_hint")}</p>
  <div class="controls">
    <button onclick={refresh} disabled={loading}>{t("ui.refresh_flags")}</button>
    <label><input type="checkbox" bind:checked={watching} /> {t("ui.watch_flags")}</label>
  </div>
  <label class="filter">{t("ui.filter_flags")} <input type="search" dir="auto" bind:value={query} /></label>
  {#if error}<p role="alert">{error}{refreshed ? t("ui.previous_values") : ""}</p>{/if}
  <p role="status">{t("ui.flags_count", { count: matching.length })}{refreshed ? t("ui.flags_updated", { time: formatTime(refreshed) }) : ""}{matching.length > 100 ? t("ui.first_flags") : ""}</p>
  <div class="table">
    <table>
      <thead><tr><th>{t("ui.flag")}</th><th>{t("ui.server_value")}</th><th>{t("ui.default")}</th></tr></thead>
      <tbody>
        {#each matching.slice(0, 100) as prop}
          <tr><td>{prop.name}<small>{prop.code}</small></td><td>{prop.value === null ? t("ui.not_received") : String(prop.value)}</td><td>{String(prop.default)}</td></tr>
        {/each}
      </tbody>
    </table>
  </div>
</details>

<style>
  details { margin-top: 1rem; }
  summary { display: flex; align-items: center; gap: 8px; cursor: pointer; font-weight: 600; list-style: none; user-select: none; }
  summary::-webkit-details-marker { display: none; }
  .chev { display: grid; margin-inline-start: auto; color: var(--muted); transition: transform calc(150ms * var(--motion-scale, 1)) var(--ease); }
  details[open] .chev { transform: rotate(180deg); }
  p { color: var(--muted); font-size: .85rem; }
  [role="alert"] { color: var(--danger); }
  .controls { display: flex; align-items: center; flex-wrap: wrap; gap: .8rem; }
  .filter { display: grid; gap: .4rem; margin-top: .8rem; }
  input[type="search"] { width: 100%; box-sizing: border-box; }
  button, input[type="search"] { background: var(--raised); color: var(--text); border: 1px solid var(--line-strong); padding: .5rem; border-radius: .3rem; font: inherit; }
  .table { overflow: auto; max-height: 22rem; }
  table { width: 100%; border-collapse: collapse; font-size: .8rem; }
  th, td { text-align: start; padding: .4rem; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  small { display: block; color: var(--muted); }
</style>
