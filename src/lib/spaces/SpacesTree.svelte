<script lang="ts">
  import { onDestroy } from "svelte";
  import Button from "$lib/ui/Button.svelte";
  import type { Space, SpaceAction, SpaceSelection, SpaceSnapshot } from "$lib/utils/wire";
  import { descendants, movedIds, spaceChildren, spaceTree } from "./spaces";

  let { account, generation, snapshot, selected, loading = false, busy = false, error = null,
    onselect, onaction, onexport, onimport }: {
    account: string | null; generation: number; snapshot: SpaceSnapshot; selected: SpaceSelection;
    loading?: boolean; busy?: boolean; error?: string | null;
    onselect: (selection: SpaceSelection) => void;
    onaction: (action: SpaceAction) => Promise<void>;
    onexport: () => Promise<string>; onimport: (json: string) => Promise<void>;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  let collapsed = $state<string[]>([]);
  let draft = $state<{ id: string; parent_id: string | null; name: string; icon: string; color: string; useColor: boolean } | null>(null);
  let moving = $state<{ space: Space; parent_id: string | null } | null>(null);
  let confirmation = $state<Space | null>(null);
  let metadata = $state<{ mode: "export" | "import"; json: string } | null>(null);
  let working = $state(false);
  let failure = $state("");
  let revision = 0, alive = true;
  const disabled = $derived(!account || busy || working || loading);
  const children = $derived(spaceChildren(snapshot.spaces));
  const parentRows = $derived(spaceTree(snapshot.spaces));
  const excluded = $derived(moving ? descendants(snapshot.spaces, moving.space.id) : new Set<string>());

  $effect(() => {
    account; generation;
    revision++;
    collapsed = [];
    draft = moving = confirmation = metadata = null;
    working = false;
    failure = "";
  });
  $effect(() => { if (dialog && !dialog.open) dialog.showModal(); });
  onDestroy(() => { alive = false; revision++; dialog?.close(); });

  function cancel() {
    revision++;
    dialog?.close();
    draft = moving = confirmation = metadata = null;
    working = false;
    failure = "";
  }

  async function run(task: () => Promise<unknown>, done?: () => void) {
    if (disabled || !alive) return;
    const owner = account, scope = generation, request = ++revision;
    const current = () => alive && owner === account && scope === generation && request === revision;
    working = true;
    failure = "";
    try { await task(); if (current()) done?.(); }
    catch (cause) { if (current()) failure = String(cause); }
    finally { if (current()) working = false; }
  }

  function create(parent_id: string | null = null) {
    if (disabled) return;
    draft = { id: "", parent_id, name: "", icon: "", color: "#00a884", useColor: false };
    failure = "";
  }

  function save() {
    if (disabled || !draft || !draft.name.trim()) return;
    const action: SpaceAction = draft.id ? { kind: "rename", id: draft.id, name: draft.name.trim() }
      : { kind: "create", id: crypto.randomUUID(), parent_id: draft.parent_id, name: draft.name.trim(),
        icon: draft.icon.trim() || null, color: draft.useColor ? draft.color : null };
    void run(() => onaction(action), cancel);
  }

  function move(space: Space, direction: -1 | 1) {
    const ids = movedIds(children.get(space.parent_id) ?? [], space.id, direction);
    if (ids) void run(() => onaction({ kind: "reorder", parent_id: space.parent_id, ids }));
  }

  function reparent() {
    if (!moving) return;
    const { space, parent_id } = moving;
    if (excluded.has(parent_id ?? "")) return;
    void run(() => onaction({ kind: "reparent", id: space.id, parent_id }), cancel);
  }

  function remove() {
    if (!confirmation) return;
    const id = confirmation.id;
    void run(() => onaction({ kind: "delete", id }), cancel);
  }

  function exportMetadata() {
    if (disabled || !alive) return;
    metadata = { mode: "export", json: "" };
    let json = "";
    void run(async () => { json = await onexport(); }, () => { metadata = { mode: "export", json }; });
  }
</script>

{#snippet branch(parent: string | null)}
  <ul>
    {#each children.get(parent) ?? [] as space, index (space.id)}
      {@const nested = children.get(space.id) ?? []}
      <li>
        <div class="space-row">
          {#if nested.length}
            <button class="toggle" aria-label={`${collapsed.includes(space.id) ? "Expand" : "Collapse"} ${space.name}`}
              aria-expanded={!collapsed.includes(space.id)} onclick={() => {
                collapsed = collapsed.includes(space.id) ? collapsed.filter((id) => id !== space.id) : [...collapsed, space.id];
              }}>{collapsed.includes(space.id) ? "›" : "⌄"}</button>
          {:else}<span class="toggle"></span>{/if}
          <button class="name" style:color={space.color ?? undefined} disabled={disabled}
            aria-pressed={selected.kind === "space" && selected.space_id === space.id}
            onclick={() => onselect({ kind: "space", space_id: space.id })}>
            {#if space.icon}<span aria-hidden="true">{space.icon}</span>{/if}{space.name}
          </button>
          <details><summary aria-label={`Manage ${space.name}`}>•••</summary><div class="actions">
            <button disabled={disabled} onclick={() => create(space.id)}>Add child Space</button>
            <button disabled={disabled} onclick={() => { draft = { id: space.id, parent_id: space.parent_id, name: space.name, icon: "", color: "#00a884", useColor: false }; failure = ""; }}>Rename</button>
            <button disabled={disabled} onclick={() => { moving = { space, parent_id: space.parent_id }; failure = ""; }}>Move to parent</button>
            <button disabled={disabled || index === 0} onclick={() => move(space, -1)}>Move up</button>
            <button disabled={disabled || index === (children.get(parent)?.length ?? 0) - 1} onclick={() => move(space, 1)}>Move down</button>
            <button disabled={disabled} onclick={() => { confirmation = space; failure = ""; }}>Delete Space</button>
          </div></details>
        </div>
        {#if nested.length && !collapsed.includes(space.id)}{@render branch(space.id)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<nav class="spaces" aria-label="Spaces" aria-busy={loading || working}>
  <header><h2>Spaces</h2><Button variant="icon" icon="plus" aria-label="Create Space" disabled={disabled} onclick={() => create()} /></header>
  <p class="muted">Local views on this device.</p>
  <div class="defaults">
    <button disabled={disabled} aria-pressed={selected.kind === "all"} onclick={() => onselect({ kind: "all" })}>All</button>
    <button disabled={disabled} aria-pressed={selected.kind === "unsorted"} onclick={() => onselect({ kind: "unsorted" })}>Unsorted</button>
  </div>
  {@render branch(null)}
  {#if loading}<p role="status">Loading Spaces…</p>
  {:else if !snapshot.spaces.length}<p class="muted">Create a Space to organize conversations and saved views.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if failure && !dialog?.open}<p class="error" role="alert">{failure}</p>{/if}
  <footer>
    <button disabled={disabled} onclick={exportMetadata}>Export metadata</button>
    <button disabled={disabled} onclick={() => { metadata = { mode: "import", json: "" }; failure = ""; }}>Import metadata</button>
  </footer>
</nav>

{#if draft || moving || confirmation || metadata}
  <dialog bind:this={dialog} aria-label={draft ? draft.id ? "Rename Space" : "Create Space" : moving ? "Move Space" : confirmation ? "Delete Space" : "Space metadata"}
    oncancel={(event) => { event.preventDefault(); cancel(); }}>
    <header><h2>{draft ? draft.id ? "Rename Space" : "Create Space" : moving ? "Move Space" : confirmation ? "Delete Space" : `${metadata!.mode === "export" ? "Export" : "Import"} metadata`}</h2>
      <Button variant="icon" icon="x" aria-label="Close Space dialog" onclick={cancel} /></header>
    {#if draft}
      <form onsubmit={(event) => { event.preventDefault(); save(); }}>
        <label>Name <input bind:value={draft.name} required maxlength="256" disabled={disabled} /></label>
        {#if !draft.id}
          <label>Parent <select bind:value={draft.parent_id} disabled={disabled}>
            <option value={null}>Top level</option>{#each parentRows as row (row.space.id)}<option value={row.space.id}>{"— ".repeat(row.depth)}{row.space.name}</option>{/each}
          </select></label>
          <label>Icon (optional) <input bind:value={draft.icon} disabled={disabled} /></label>
          <label><input type="checkbox" bind:checked={draft.useColor} disabled={disabled} /> Use a color</label>
          {#if draft.useColor}<label>Color <input type="color" bind:value={draft.color} disabled={disabled} /></label>{/if}
        {/if}
        <Button variant="primary" type="submit" disabled={disabled || !draft.name.trim()}>{draft.id ? "Save name" : "Create Space"}</Button>
      </form>
    {:else if moving}
      <label>Parent of {moving.space.name} <select bind:value={moving.parent_id} disabled={disabled}>
        <option value={null}>Top level</option>{#each parentRows.filter((row) => !excluded.has(row.space.id)) as row (row.space.id)}
          <option value={row.space.id}>{"— ".repeat(row.depth)}{row.space.name}</option>{/each}
      </select></label>
      <Button variant="primary" disabled={disabled || moving.parent_id === moving.space.parent_id} onclick={reparent}>Move Space</Button>
    {:else if confirmation}
      <p>Delete “{confirmation.name}” and its direct item references? Child Spaces move to its parent. Chats, messages and media stay unchanged.</p>
      <Button variant="ghost" danger disabled={disabled} onclick={remove}>Delete Space</Button>
    {:else if metadata}
      <p class="muted">{metadata.mode === "export" ? "Copy this JSON to keep the local Space definitions and references." : "Import adds local Space definitions and references. WhatsApp data stays unchanged."}</p>
      <label>Space metadata JSON <textarea bind:value={metadata.json} readonly={metadata.mode === "export"} disabled={working} rows="8"></textarea></label>
      {#if metadata.mode === "import"}<Button variant="primary" disabled={disabled || !metadata.json.trim()}
        onclick={() => { const json = metadata!.json; void run(() => onimport(json), cancel); }}>Import metadata</Button>{/if}
    {/if}
    {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  </dialog>
{/if}

<style>
  .spaces { padding: 12px; border-bottom: 1px solid var(--line); }
  header, .space-row, .defaults, footer { display: flex; align-items: center; gap: 6px; }
  header { justify-content: space-between; }
  h2 { margin: 0; font-size: 16px; }
  p { font-size: 12px; }
  .muted { color: var(--muted); }
  ul { margin: 0; padding: 0; list-style: none; }
  li ul { margin-left: 18px; }
  button, summary { font: inherit; cursor: pointer; }
  .defaults button, .name, .toggle, footer button, .actions button { padding: 6px 8px; border: 0; border-radius: 6px; background: transparent; color: var(--text); text-align: left; }
  button[aria-pressed="true"], button:hover { background: var(--raised); }
  .name { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .name span { margin-right: 6px; }
  .toggle { box-sizing: border-box; width: 24px; flex-shrink: 0; }
  summary { padding: 4px; color: var(--muted); }
  .actions { display: flex; flex-direction: column; min-width: 130px; background: var(--surface); }
  footer { flex-wrap: wrap; margin-top: 10px; }
  footer button { font-size: 12px; }
  .error { color: var(--danger); }
  dialog { width: min(440px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto; box-sizing: border-box; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  form, label { display: grid; gap: 6px; margin: 12px 0; font-size: 13px; }
  input:not([type="checkbox"]), select, textarea { box-sizing: border-box; width: 100%; padding: 8px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; }
  input[type="checkbox"] { width: auto; }
  textarea { resize: vertical; }
  button:disabled { opacity: .5; cursor: default; }
</style>
