<script lang="ts">
import { onDestroy } from "svelte";
import Button from "$lib/ui/Button.svelte";
import Icon, { ICON_NAMES, type IconName } from "$lib/ui/Icon.svelte";
import { loadEmojis, recentEmojis, rememberEmoji, searchEmojis, type Emoji } from "$lib/utils/emoji";
  import type { Space, SpaceAction, SpaceSelection, SpaceSnapshot } from "$lib/utils/wire";
  import { descendants, movedIds, spaceChildren, spaceTree } from "./spaces";

  let { account, generation, snapshot, selected, loading = false, busy = false, error = null,
    onselect, onaction }: {
    account: string | null; generation: number; snapshot: SpaceSnapshot; selected: SpaceSelection;
    loading?: boolean; busy?: boolean; error?: string | null;
    onselect: (selection: SpaceSelection) => void;
    onaction: (action: SpaceAction) => Promise<void>;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  let collapsed = $state<string[]>([]);
  let navCollapsed = $state(false);
  let draft = $state<{ id: string; parent_id: string | null; name: string; icon: string; color: string; useColor: boolean } | null>(null);
  let moving = $state<{ space: Space; parent_id: string | null } | null>(null);
  let confirmation = $state<Space | null>(null);
  let working = $state(false);
  let failure = $state("");
  let revision = 0, alive = true;
  const disabled = $derived(!account || busy || working || loading);
  const children = $derived(spaceChildren(snapshot.spaces));
  const parentRows = $derived(spaceTree(snapshot.spaces));
  const excluded = $derived(moving ? descendants(snapshot.spaces, moving.space.id) : new Set<string>());
  const isAppIcon = (value: string): value is IconName => (ICON_NAMES as readonly string[]).includes(value);
  /** Icon picker popup state; emoji data loads on first open. */
  let iconPopup = $state<"app" | "emoji" | null>(null);
  let emojis = $state<Emoji[]>([]);
  let emojiQuery = $state("");
  const recentRow = $derived(recentEmojis().filter((emoji) => emojis.some((row) => row.emoji === emoji)).slice(0, 8));
  const shownEmojis = $derived(emojiQuery.trim() ? searchEmojis(emojis, emojiQuery.trim(), 60) : emojis.slice(0, 60));

  $effect(() => {
    account; generation;
    revision++;
    collapsed = [];
    navCollapsed = false;
    iconPopup = null;
    emojiQuery = "";
    draft = moving = confirmation = null;
    working = false;
    failure = "";
  });
  $effect(() => { if (dialog && !dialog.open) dialog.showModal(); });
  onDestroy(() => { alive = false; revision++; dialog?.close(); });

  function cancel() {
    revision++;
    dialog?.close();
    draft = moving = confirmation = null;
    iconPopup = null;
    emojiQuery = "";
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
    iconPopup = null;
    emojiQuery = "";
    failure = "";
  }

  function pickIcon(value: string) {
    if (!draft || disabled) return;
    draft.icon = value;
    if (!isAppIcon(value)) rememberEmoji(value);
    iconPopup = null;
  }

  function openIconPopup(kind: "app" | "emoji") {
    if (disabled) return;
    iconPopup = iconPopup === kind ? null : kind;
    if (kind === "emoji" && !emojis.length) void loadEmojis().then((list) => { emojis = list; });
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
            {#if space.icon}
              {#if isAppIcon(space.icon)}<span class="glyph-icon" aria-hidden="true"><Icon name={space.icon} size={15} /></span>
              {:else}<span aria-hidden="true">{space.icon}</span>{/if}
            {/if}{space.name}
          </button>
          <details><summary aria-label={`Manage ${space.name}`}>•••</summary>
          <div class="actions">
            <button disabled={disabled} onclick={(event) => { create(space.id); event.currentTarget.closest("details")?.removeAttribute("open"); }}>Add child Space</button>
            <button disabled={disabled} onclick={(event) => { draft = { id: space.id, parent_id: space.parent_id, name: space.name, icon: "", color: "#00a884", useColor: false }; failure = ""; event.currentTarget.closest("details")?.removeAttribute("open"); }}>Rename</button>
            <button disabled={disabled} onclick={(event) => { moving = { space, parent_id: space.parent_id }; failure = ""; event.currentTarget.closest("details")?.removeAttribute("open"); }}>Move to parent</button>
            <button disabled={disabled || index === 0} onclick={(event) => { move(space, -1); event.currentTarget.closest("details")?.removeAttribute("open"); }}>Move up</button>
            <button disabled={disabled || index === (children.get(parent)?.length ?? 0) - 1} onclick={(event) => { move(space, 1); event.currentTarget.closest("details")?.removeAttribute("open"); }}>Move down</button>
            <button disabled={disabled} onclick={(event) => { confirmation = space; failure = ""; event.currentTarget.closest("details")?.removeAttribute("open"); }}>Delete Space</button>
          </div></details>
        </div>
        {#if nested.length && !collapsed.includes(space.id)}{@render branch(space.id)}{/if}
      </li>
    {/each}
  </ul>
{/snippet}

<nav class="spaces" aria-label="Spaces" aria-busy={loading || working}>
  <header>
    <span class="title-row">
      <button class="toggle" aria-label={`${navCollapsed ? "Expand" : "Collapse"} Spaces`} aria-expanded={!navCollapsed}
        onclick={() => (navCollapsed = !navCollapsed)}>{navCollapsed ? "›" : "⌄"}</button>
      <h2>Spaces</h2>
    </span>
    <Button variant="icon" icon="plus" aria-label="Create Space" disabled={disabled} onclick={() => create()} /></header>
  {#if !navCollapsed}
    <p class="muted">Local views on this device.</p>
    <div class="defaults">
      <Button variant="chip" selected={selected.kind === "all"} disabled={disabled} onclick={() => onselect({ kind: "all" })}>All</Button>
      <Button variant="chip" selected={selected.kind === "unsorted"} disabled={disabled} onclick={() => onselect({ kind: "unsorted" })}>Unsorted</Button>
    </div>
    {@render branch(null)}
    {#if loading}<p role="status">Loading Spaces…</p>
    {:else if !snapshot.spaces.length}<p class="muted">Create a Space to organize conversations and saved views.</p>{/if}
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if failure && !dialog?.open}<p class="error" role="alert">{failure}</p>{/if}
</nav>

{#if draft || moving || confirmation}
  <dialog bind:this={dialog} aria-label={draft ? draft.id ? "Rename Space" : "Create Space" : moving ? "Move Space" : "Delete Space"}
    oncancel={(event) => { event.preventDefault(); cancel(); }}>
    <header><h2>{draft ? draft.id ? "Rename Space" : "Create Space" : moving ? "Move Space" : "Delete Space"}</h2>
      <Button variant="icon" icon="x" aria-label="Close Space dialog" onclick={cancel} /></header>
    {#if draft}
      <form onsubmit={(event) => { event.preventDefault(); save(); }}>
        <label>Name <input bind:value={draft.name} required maxlength="256" disabled={disabled} /></label>
        {#if !draft.id}
          <label>Parent <select bind:value={draft.parent_id} disabled={disabled}>
            <option value={null}>Top level</option>{#each parentRows as row (row.space.id)}<option value={row.space.id}>{"— ".repeat(row.depth)}{row.space.name}</option>{/each}
          </select></label>
          <div class="icon-field">
            <span id="space-icon-label">Icon (optional)</span>
            <div class="icon-current">
              <span class="preview" aria-hidden="true">
                {#if draft.icon}{#if isAppIcon(draft.icon)}<Icon name={draft.icon} size={18} />{:else}{draft.icon}{/if}
                {:else}—{/if}
              </span>
              <button type="button" disabled={disabled} aria-expanded={iconPopup === "app"} onclick={() => openIconPopup("app")}>App icons</button>
              <button type="button" disabled={disabled} aria-expanded={iconPopup === "emoji"} onclick={() => openIconPopup("emoji")}>Emoji</button>
              {#if draft.icon}<button type="button" class="link" disabled={disabled} onclick={() => (draft!.icon = "")}>Remove</button>{/if}
            </div>
            {#if iconPopup}
              <div class="icon-popup" role="dialog" aria-label={iconPopup === "app" ? "Choose an app icon" : "Choose an emoji"}>
                {#if iconPopup === "app"}
                  <div class="icon-grid" role="group" aria-label="App icons">
                    {#each ICON_NAMES as name (name)}
                      <button type="button" class="glyph" class:chosen={draft.icon === name} aria-pressed={draft.icon === name}
                        aria-label={`App icon ${name}`} title={name} disabled={disabled}
                        onclick={() => pickIcon(draft!.icon === name ? "" : name)}><Icon name={name} size={18} /></button>
                    {/each}
                  </div>
                {:else}
                  <input type="search" aria-label="Search emoji" placeholder="Search emoji" bind:value={emojiQuery} disabled={disabled} />
                  {#if !emojiQuery.trim() && recentRow.length}
                    <p class="muted">Recent</p>
                    <div class="icon-grid" role="group" aria-label="Recent emoji">
                      {#each recentRow as emoji (emoji)}
                        <button type="button" class="glyph emoji" class:chosen={draft.icon === emoji} aria-pressed={draft.icon === emoji}
                          aria-label={`Emoji ${emoji}`} disabled={disabled} onclick={() => pickIcon(emoji)}>{emoji}</button>
                      {/each}
                    </div>
                  {/if}
                  <div class="icon-grid" role="group" aria-label="Emoji">
                    {#if !emojis.length}<p class="muted">Loading emoji…</p>{/if}
                    {#each shownEmojis as row (row.emoji)}
                      <button type="button" class="glyph emoji" class:chosen={draft.icon === row.emoji} aria-pressed={draft.icon === row.emoji}
                        aria-label={row.label} title={row.label} disabled={disabled} onclick={() => pickIcon(row.emoji)}>{row.emoji}</button>
                    {/each}
                  </div>
                {/if}
              </div>
            {/if}
          </div>
          <label class="inline"><input type="checkbox" bind:checked={draft.useColor} disabled={disabled} /> Use a color</label>
          {#if draft.useColor}<label class="color-row">Color <input type="color" class="swatch" bind:value={draft.color} disabled={disabled} /></label>{/if}
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
    {/if}
    {#if failure}<p class="error" role="alert">{failure}</p>{/if}
  </dialog>
{/if}

<svelte:window onclick={(event) => {
  const target = event.target as Element;
  for (const details of document.querySelectorAll(".spaces details[open]")) {
    if (!details.contains(target)) details.removeAttribute("open");
  }
  if (iconPopup && !target.closest?.(".icon-field")) iconPopup = null;
}} />

<style>
  .spaces { padding: 12px; border-bottom: 1px solid var(--line); }
  header, .space-row, .defaults, .title-row { display: flex; align-items: center; gap: 6px; }
  .space-row { padding: 2px 4px; border-radius: 8px; }
  .space-row:hover { background: var(--raised); }
  .space-row:has(> .name[aria-pressed="true"]) { background: var(--raised-2); }
  .space-row .name:hover, .space-row .name[aria-pressed="true"], .space-row .toggle:hover { background: transparent; }
  header { justify-content: space-between; }
  h2 { margin: 0; font-size: 16px; }
  p { font-size: 12px; }
  .muted { color: var(--muted); }
  ul { margin: 0; padding: 0; list-style: none; }
  li ul { margin-left: 18px; }
  button, summary { font: inherit; cursor: pointer; }
  .name, .toggle, .actions button { padding: 6px 8px; border: 0; border-radius: 6px; background: transparent; color: var(--text); text-align: left; }
  button[aria-pressed="true"], button:hover { background: var(--raised); }
  .name { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .name span { margin-right: 6px; }
  .name .glyph-icon { display: inline-flex; vertical-align: -2px; }
  .toggle { box-sizing: border-box; width: 24px; flex-shrink: 0; }
  summary { list-style: none; padding: 4px 6px; border-radius: 6px; color: var(--muted); font-size: 14px; line-height: 1; }
  summary::-webkit-details-marker { display: none; }
  summary::marker { content: none; }
  summary:hover { background: var(--raised); color: var(--text); }
  summary:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  details { position: relative; flex: none; }
  details[open] > summary { background: var(--raised); color: var(--text); }
  .actions {
    position: absolute; right: 0; top: calc(100% + 4px); z-index: 60;
    display: flex; flex-direction: column; min-width: 160px; padding: 6px;
    background: var(--surface); border: 1px solid var(--line-strong);
    border-radius: var(--radius); box-shadow: var(--shadow);
  }
  .actions button { font-size: 13px; }
  .error { color: var(--danger); }
  dialog { width: min(440px, calc(100vw - 32px)); max-height: calc(100vh - 64px); overflow: auto; box-sizing: border-box; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  form, label { display: grid; gap: 6px; margin: 12px 0; font-size: 13px; }
  .icon-field { position: relative; display: grid; gap: 6px; margin: 12px 0; font-size: 13px; }
  .icon-current { display: flex; align-items: center; gap: 8px; }
  .icon-current .preview { display: grid; place-items: center; width: 34px; height: 30px; flex: none; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); font-size: 18px; }
  .icon-current button:not(.link) { padding: 6px 10px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); color: var(--text); font-size: 12px; cursor: pointer; }
  .icon-current button:not(.link):hover:not(:disabled) { border-color: var(--accent); }
  .icon-current button:not(.link)[aria-expanded="true"] { border-color: var(--accent); background: var(--accent-soft); }
  .icon-current .link { padding: 0; border: 0; background: transparent; color: var(--accent-text); font-size: 12px; cursor: pointer; }
  .icon-popup { position: absolute; left: 0; right: 0; top: calc(100% + 4px); z-index: 60; max-height: 260px; overflow-y: auto; padding: 10px; background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius); box-shadow: var(--shadow); }
  .icon-popup input[type="search"] { margin-bottom: 4px; }
  .icon-popup .muted { margin: 6px 0 0; }
  .icon-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(32px, 1fr)); gap: 4px; margin-top: 8px; }
  button.glyph { display: grid; place-items: center; min-height: 32px; padding: 2px; border: 1px solid transparent; border-radius: 6px; background: transparent; color: var(--muted); font-size: 18px; cursor: pointer; }
  button.glyph:hover:not(:disabled) { background: var(--raised); color: var(--text); }
  button.glyph.chosen { background: var(--accent-soft); border-color: var(--accent); color: var(--accent); }
  button.glyph:disabled { opacity: 0.5; cursor: default; }
  label.inline, label.color-row { display: flex; align-items: center; gap: 8px; }
  label.inline { cursor: pointer; }
  label.color-row { justify-content: space-between; }
  input[type="color"].swatch { flex: none; width: 44px; height: 30px; padding: 3px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); cursor: pointer; }
  input[type="color"].swatch::-webkit-color-swatch-wrapper { padding: 0; }
  input[type="color"].swatch::-webkit-color-swatch { border: 0; border-radius: 3px; }
  input[type="color"].swatch::-moz-color-swatch { border: 0; border-radius: 3px; }
  input[type="color"].swatch:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  input[type="color"].swatch:disabled { opacity: 0.5; cursor: default; }
  input:not([type="checkbox"]), select { box-sizing: border-box; width: 100%; padding: 8px; border: 1px solid var(--line-strong); border-radius: 6px; background: var(--bg); color: var(--text); font: inherit; }
  input[type="checkbox"] { appearance: none; -webkit-appearance: none; flex: none; width: 18px; height: 18px; margin: 0; display: grid; place-items: center; border: 1.5px solid var(--line-strong); border-radius: 6px; background: var(--bg); cursor: pointer; transition: background-color 0.15s var(--ease), border-color 0.15s var(--ease); }
  input[type="checkbox"]:hover:not(:disabled) { border-color: var(--accent); }
  input[type="checkbox"]:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  input[type="checkbox"]:checked { background: var(--accent); border-color: var(--accent); }
  input[type="checkbox"]:checked::after { content: ""; width: 9px; height: 5px; border-left: 2px solid var(--accent-ink); border-bottom: 2px solid var(--accent-ink); transform: rotate(-45deg) translateY(-1px); }
  input[type="checkbox"]:disabled { opacity: 0.55; cursor: default; }
  button:disabled { opacity: .5; cursor: default; }
</style>
