<script lang="ts">
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { onDestroy } from "svelte";
  import Button from "$lib/ui/Button.svelte";
  import type { Space, SpaceAction, SpaceItem, SpaceResolution, SpaceTarget } from "$lib/utils/wire";
  import { directItems, movedIds, SPACE_KINDS, targetKey, targetTitle, type SpaceCandidate } from "./spaces";

  let { account, generation, space, items, resolution, catalog, loading = false, busy = false, error = null,
    onopen, onaction, onadd }: {
    account: string | null; generation: number; space: Space; items: SpaceItem[];
    resolution: SpaceResolution | null; catalog: SpaceCandidate[]; loading?: boolean; busy?: boolean; error?: LocalizedError | string | null;
    onopen: (target: SpaceTarget) => void | Promise<void>;
    onaction: (action: SpaceAction) => Promise<void>; onadd: () => void;
  } = $props();

  let working = $state(false), failure = $state<LocalizedError | string>("");
  let revision = 0, alive = true;
  const disabled = $derived(!account || busy || working);
  const rows = $derived(directItems(items, space.id));
  const names = $derived(new Map(catalog.map((row) => [targetKey(row.target), row])));
  const resolved = $derived(new Map(resolution?.items.map((item) => [item.item_id, item]) ?? []));
  $effect(() => { account; generation; space.id; revision++; working = false; failure = ""; });
  onDestroy(() => { alive = false; revision++; });

  async function run(task: () => void | Promise<void>) {
    if (disabled || !alive) return;
    const owner = account, scope = generation, spaceId = space.id, request = ++revision;
    const current = () => alive && owner === account && scope === generation && spaceId === space.id && request === revision;
    working = true;
    failure = "";
    try { await task(); }
    catch (cause) { if (current()) failure = normalizeError(cause); }
    finally { if (current()) working = false; }
  }

  function open(item: SpaceItem) {
    const status = resolved.get(item.id);
    if (item.space_id !== space.id || !rows.some((row) => row.id === item.id) || loading || !status || status.unavailable !== null) return;
    const target = structuredClone(item.target);
    void run(() => onopen(target));
  }

  function move(item: SpaceItem, direction: -1 | 1) {
    const ids = movedIds(rows, item.id, direction), space_id = space.id;
    if (ids) void run(() => onaction({ kind: "reorder_items", space_id, ids }));
  }

  function remove(item: SpaceItem) {
    if (item.space_id !== space.id || !rows.some((row) => row.id === item.id)) return;
    const id = item.id;
    void run(() => onaction({ kind: "remove_item", id }));
  }
</script>

<section class="space-items" aria-label={t("spaces.items_in", { name: space.name })} aria-busy={loading || working}>
  <header><h2>{space.name}</h2><Button variant="ghost" icon="plus" disabled={disabled} onclick={onadd}>{t("spaces.add_items")}</Button></header>
  <p class="muted">{t("spaces.items_hint")}</p>
  <ol>
    {#each rows as item, index (item.id)}
      {@const title = names.get(targetKey(item.target))?.title ?? targetTitle(item.target)}
      {@const status = resolved.get(item.id)}
      <li>
        <div class="description"><button class="open" disabled={disabled || loading || !status || status.unavailable !== null} onclick={() => open(item)}>{title}</button>
          <span class="kind">{t(SPACE_KINDS[item.target.kind])}</span>
          {#if loading || !resolution}<span class="muted" role="status">{t("spaces.checking")}</span>
          {:else if !status}<span class="muted">{t("spaces.unresolved")}</span>
          {:else if status.unavailable !== null}<span class="unavailable">{t("spaces.unavailable_prefix")} {status.unavailable}</span>{/if}
        </div>
        <div class="actions">
          <button disabled={disabled || index === 0} aria-label={t("spaces.move_item_up", { name: title })} onclick={() => move(item, -1)}>↑</button>
          <button disabled={disabled || index === rows.length - 1} aria-label={t("spaces.move_item_down", { name: title })} onclick={() => move(item, 1)}>↓</button>
          <button disabled={disabled} aria-label={t("spaces.remove_item", { name: title, space: space.name })} onclick={() => remove(item)}>{t("ui.remove")}</button>
        </div>
      </li>
    {/each}
  </ol>
  {#if !rows.length}<p class="muted">{t("spaces.items_empty")}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if failure}<p class="error" role="alert">{failure}</p>{/if}
</section>

<style>
  .space-items { padding: 12px; border-bottom: 1px solid var(--line); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 8px; }
  h2 { margin: 0; font-size: 16px; overflow-wrap: anywhere; }
  p, .kind, .unavailable, .muted { font-size: 12px; }
  .muted, .kind { color: var(--muted); }
  ol { margin: 0; padding-inline-start: 20px; }
  li { padding: 8px 0; border-bottom: 1px solid var(--line); }
  .description { display: flex; flex-direction: column; gap: 4px; }
  button { font: inherit; cursor: pointer; }
  .open { padding: 0; border: 0; background: transparent; color: var(--text); text-align: start; overflow-wrap: anywhere; }
  .open:not(:disabled):hover { color: var(--accent-text); }
  .actions { display: flex; gap: 6px; margin-top: 6px; }
  .actions button { padding: 4px 6px; border: 1px solid var(--line-strong); border-radius: 5px; background: var(--surface); color: var(--text); font-size: 12px; }
  .error, .unavailable { color: var(--danger); }
  button:disabled { opacity: .5; cursor: default; }
</style>
