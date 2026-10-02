<script lang="ts">
  import type { Label } from "$lib/utils/wire";
  import Icon from "$lib/ui/Icon.svelte";

  let { account, requestKey, labels, selected = [], mixed = [], mode, busy = false, error = "",
    complete = false, defaultColor = 0, onapply, onsave, ondelete, onclose }: {
    account: string | null;
    requestKey: string | number;
    labels: Label[];
    selected?: readonly string[];
    mixed?: readonly string[];
    mode: "manage" | "apply";
    busy?: boolean;
    error?: string | null;
    complete?: boolean;
    defaultColor?: number;
    onapply: (labelId: string, labeled: boolean) => Promise<void>;
    onsave: (labelId: string, name: string, color: number) => Promise<void>;
    ondelete: (labelId: string) => Promise<void>;
    onclose: () => void;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  let draft = $state<{ id: string; name: string; color: number | undefined }>({ id: "", name: "", color: 0 });
  let confirmation = $state<Label | null>(null);
  let working = $state(false);
  let failure = $state("");
  let generation = 0;
  const disabled = $derived(busy || working || !account);
  const validColor = $derived(draft.color !== undefined && Number.isInteger(draft.color) && draft.color >= -2147483648 && draft.color <= 2147483647);

  $effect(() => {
    account; requestKey; mode;
    ++generation;
    draft = { id: "", name: "", color: defaultColor };
    confirmation = null;
    working = false;
    failure = "";
    return () => { ++generation; };
  });
  $effect(() => { if (dialog && !dialog.open) dialog.showModal(); });

  function markMixed(node: HTMLInputElement, value: boolean) {
    node.indeterminate = value;
    return { update(value: boolean) { node.indeterminate = value; } };
  }

  async function run(task: () => Promise<void>, done?: () => void) {
    if (disabled) return;
    const owner = account, revision = generation;
    working = true;
    failure = "";
    const current = () => owner === account && revision === generation;
    try { await task(); if (current()) done?.(); }
    catch (cause) { if (current()) failure = String(cause); }
    finally { if (current()) working = false; }
  }

  function apply(event: Event, label: Label) {
    const input = event.currentTarget as HTMLInputElement;
    const labeled = input.checked;
    input.checked = selected.includes(label.id);
    input.indeterminate = mixed.includes(label.id);
    void run(() => onapply(label.id, labeled));
  }

  function save() {
    const { id, color } = draft, name = draft.name.trim();
    if (!name || color === undefined || !validColor) return;
    void run(() => onsave(id, name, color), () => { draft = { id: "", name: "", color: defaultColor }; });
  }
</script>

<dialog bind:this={dialog} aria-label={mode === "manage" ? "Manage labels" : "Apply labels"}
  onclick={(event) => { if (event.target === dialog) onclose(); }}
  oncancel={(event) => { event.preventDefault(); event.stopPropagation(); onclose(); }}
  onkeydown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}>
  <header><h2>{mode === "manage" ? "Manage labels" : "Apply labels"}</h2>
    <button class="close" aria-label="Close labels" onclick={onclose}><Icon name="x" size={18} /></button></header>
  {#if !account}<p role="status">Select an account to use labels.</p>{/if}
  {#if !complete}<p class="muted">Showing labels stored on this device. WhatsApp may have more labels.</p>{/if}
  {#if mode === "apply"}
    {#if labels.length === 0}<p role="status">No labels stored on this device.</p>{/if}
    <ul>
      {#each labels as label (label.id)}
        <li><label class="apply"><input type="checkbox" checked={selected.includes(label.id)} use:markMixed={mixed.includes(label.id)}
          disabled={disabled} onchange={(event) => apply(event, label)} /> {label.name}</label>
          {#if mixed.includes(label.id)}<span class="muted">Some selected items</span><button disabled={disabled}
            onclick={() => run(() => onapply(label.id, false))} aria-label={`Remove ${label.name} from all selected items`}>Remove from all</button>{/if}
        </li>
      {/each}
    </ul>
  {:else}
    {#if labels.length === 0}<p role="status">No labels stored on this device.</p>{/if}
    <ul>
      {#each labels as label (label.id)}
        <li><span class="label-name">{label.name}</span>
          <button disabled={disabled} aria-label={`Edit ${label.name}`} onclick={() => { confirmation = null; draft = { ...label }; failure = ""; }}>Edit</button>
          <button disabled={disabled} aria-label={`Delete ${label.name}`} onclick={() => (confirmation = { ...label })}>Delete</button></li>
      {/each}
    </ul>
    <form onsubmit={(event) => { event.preventDefault(); save(); }}>
      <h3>{draft.id ? "Edit label" : "New label"}</h3>
      <label>Name <input aria-label="Label name" bind:value={draft.name} required disabled={disabled} /></label>
      <div class="form-actions"><button type="submit" disabled={disabled || !draft.name.trim() || !validColor}>{draft.id ? "Save label" : "Create label"}</button>
        {#if draft.id}<button type="button" disabled={disabled} onclick={() => (draft = { id: "", name: "", color: defaultColor })}>Cancel edit</button>{/if}</div>
    </form>
    {#if confirmation}
      <section class="confirmation" aria-label="Confirm label deletion">
        <p>Delete "{confirmation.name}" and remove its associations?</p>
        <button class="danger" disabled={disabled} onclick={() => {
          const id = confirmation!.id;
          void run(() => ondelete(id), () => { confirmation = null; if (draft.id === id) draft = { id: "", name: "", color: defaultColor }; });
        }}>Confirm delete</button>
        <button disabled={disabled} onclick={() => (confirmation = null)}>Cancel deletion</button>
      </section>
    {/if}
  {/if}
  {#if working || busy}<p class="muted" role="status">Updating labels…</p>{/if}
  {#if error || failure}<p class="error" role="alert">{failure || error}</p>{/if}
</dialog>

<style>
  dialog { width: min(480px, calc(100vw - 32px)); max-height: calc(100vh - 32px); box-sizing: border-box; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); overflow: auto; }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: 18px; } h3 { margin: 0; font-size: 14px; }
  ul { padding: 0; margin: 16px 0; list-style: none; }
  li { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; padding: 9px 0; border-bottom: 1px solid var(--line); }
  .label-name, .apply { flex: 1; min-width: 0; overflow-wrap: anywhere; }
  .apply { display: flex; align-items: center; gap: 10px; cursor: pointer; border-radius: var(--radius-sm); padding: 2px 4px; margin: -2px -4px; }
  .apply:hover { background: var(--raised); }
  button { border: 1px solid var(--line-strong); border-radius: var(--radius-sm); padding: 6px 9px; background: var(--raised-2); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
  button:disabled, input:disabled { opacity: 0.55; cursor: default; }
  .close { display: grid; place-items: center; padding: 5px; border: 0; background: transparent; }
  form { display: grid; gap: 10px; margin-top: 18px; }
  form label { display: grid; gap: 5px; font-size: 13px; }
  input:not([type="checkbox"]) { width: 100%; min-width: 0; box-sizing: border-box; padding: 8px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg); color: inherit; font: inherit; }
  input[type="checkbox"] { appearance: none; -webkit-appearance: none; flex: none; width: 18px; height: 18px; margin: 0; display: grid; place-items: center; border: 1.5px solid var(--line-strong); border-radius: 6px; background: var(--bg); cursor: pointer; transition: background-color 0.15s var(--ease), border-color 0.15s var(--ease); }
  input[type="checkbox"]:hover:not(:disabled) { border-color: var(--accent); }
  input[type="checkbox"]:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  input[type="checkbox"]:checked, input[type="checkbox"]:indeterminate { background: var(--accent); border-color: var(--accent); }
  input[type="checkbox"]:checked::after { content: ""; width: 9px; height: 5px; border-left: 2px solid var(--accent-ink); border-bottom: 2px solid var(--accent-ink); transform: rotate(-45deg) translateY(-1px); }
  input[type="checkbox"]:indeterminate::after { content: ""; width: 9px; height: 2px; border-radius: 1px; background: var(--accent-ink); }
  .form-actions { display: flex; flex-wrap: wrap; gap: 8px; }
  .muted { color: var(--muted); font-size: 12px; }
  .confirmation { margin-top: 18px; padding-top: 10px; border-top: 1px solid var(--line-strong); }
  .confirmation p { overflow-wrap: anywhere; font-size: 13px; }
  .confirmation button + button { margin-left: 8px; }
  .danger, .error { color: var(--danger); } .error { overflow-wrap: anywhere; font-size: 13px; }
</style>
