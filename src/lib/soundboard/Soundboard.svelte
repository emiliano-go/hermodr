<script lang="ts">
  import { onDestroy, onMount, untrack } from "svelte";
  import { soundboard } from "./state";
  import { SOUND_FILE_ACCEPT } from "./library";
  import type { SoundClip, SoundScope } from "./library";
  import { shortcutClip, soundShortcutConflict, soundShortcutLabel } from "./shortcuts";

  let { open, account, chat, generation, disabled = false, onsend, onclose, onbusy = () => {} }: {
    open: boolean; account: string | null; chat: string | null; generation: number; disabled?: boolean;
    onsend: (file: File, scope: SoundScope) => Promise<void>; onclose: () => void; onbusy?: (busy: boolean) => void;
  } = $props();
  let file = $state<File | null>(null), name = $state(""), slot = $state<number | null>(null), editing = $state<string | null>(null);
  let sending = $state(false), error = $state<string | null>(null), outcome = $state("");
  let epoch = 0;
  const busy = $derived(sending || $soundboard.busy);
  const canSend = $derived(!!account && !!chat && !disabled && !busy);
  onMount(() => { void soundboard.load(); });
  onDestroy(() => onbusy(false));
  $effect(() => { onbusy(busy); });
  $effect(() => {
    account; chat; generation; open; epoch++;
    untrack(() => { sending = false; error = null; outcome = ""; if (!open) reset(); });
  });
  function reset() { file = null; name = ""; slot = null; editing = null; }
  function choose(event: Event) {
    file = (event.currentTarget as HTMLInputElement).files?.[0] ?? null;
    if (file && !name.trim()) name = file.name.replace(/\.[^.]+$/, "").slice(0, 64);
    error = null; outcome = "";
  }
  function edit(clip: SoundClip) { editing = clip.id; file = null; name = clip.name; slot = clip.shortcut; error = null; outcome = ""; }
  async function save() {
    if (busy) return;
    const conflict = soundShortcutConflict(slot, $soundboard.clips, editing ?? undefined);
    if (conflict) { error = conflict; return; }
    error = null; outcome = "";
    if (!editing && !file) { error = "Choose an audio file first."; return; }
    const saved = editing ? await soundboard.update(editing, name, slot) : await soundboard.add(file!, name, slot);
    if (saved) { reset(); outcome = "Clip saved on this device."; }
  }
  async function send(clip: SoundClip) {
    if (!canSend || !account || !chat) return;
    const scope = { account, chat, generation }, target = epoch;
    const current = () => target === epoch && account === scope.account && chat === scope.chat && generation === scope.generation && !disabled;
    sending = true; error = null; outcome = "";
    try { await soundboard.send(clip.id, scope, current, onsend); if (current()) outcome = `Sent “${clip.name}”.`; }
    catch (failure) { if (current()) error = String(failure); }
    finally { if (target === epoch) sending = false; }
  }
  async function remove(clip: SoundClip) { if (!busy && await soundboard.remove(clip.id)) { if (editing === clip.id) reset(); outcome = "Clip removed from this device."; } }
  function key(event: KeyboardEvent) {
    const target = event.target instanceof HTMLElement ? event.target : null;
    if (event.isComposing || event.keyCode === 229 || document.querySelector('dialog[open], [role="dialog"]:not(.soundboard)')) return;
    if (open && event.key === "Escape" && target?.closest(".soundboard, .composer")) {
      event.preventDefault(); event.stopPropagation(); if (!busy) onclose(); return;
    }
    if (!canSend || !target?.closest(".composer")) return;
    const clip = shortcutClip(event, $soundboard.clips);
    if (clip) { event.preventDefault(); event.stopPropagation(); void send(clip); }
  }
</script>

<svelte:window onkeydowncapture={key} />
{#if !open && error}<p class="soundboard-error error" role="alert">{error}</p>{/if}
{#if open}
  <div class="soundboard" role="dialog" tabindex="-1" aria-label="Soundboard">
    <header><h2>Soundboard</h2><button type="button" onclick={onclose} disabled={busy} aria-label="Close soundboard">×</button></header>
    <p class="description">Named audio clips saved on this device. Click a clip to send it to the current chat.</p>
    {#if $soundboard.loading}<p role="status">Loading clips…</p>{/if}
    {#if error ?? $soundboard.error}<p class="error" role="alert">{error ?? $soundboard.error}</p>{/if}
    {#if outcome}<p role="status">{outcome}</p>{/if}
    {#if $soundboard.error}<button type="button" disabled={busy || $soundboard.loading} onclick={() => void soundboard.load(true)}>Reload clips</button>{/if}
    {#if !$soundboard.loading && !$soundboard.clips.length}<p>No saved audio clips.</p>{/if}
    <ul>
      {#each $soundboard.clips as clip (clip.id)}
        <li>
          <button type="button" class="send" disabled={!canSend} onclick={() => void send(clip)} aria-label={`Send ${clip.name}`}><strong>{clip.name}</strong><span>{soundShortcutLabel(clip.shortcut)} · {(clip.size / 1024).toFixed(0)} KiB</span></button>
          <button type="button" disabled={busy} onclick={() => edit(clip)} aria-label={`Edit ${clip.name}`}>Edit</button>
          <button type="button" disabled={busy} onclick={() => void remove(clip)} aria-label={`Remove ${clip.name}`}>Remove</button>
        </li>
      {/each}
    </ul>
    <fieldset disabled={busy}>
      <legend>{editing ? "Edit clip" : "Add an audio clip"}</legend>
      {#if !editing}<label>Audio file<input type="file" accept={SOUND_FILE_ACCEPT} onchange={choose} /></label>{/if}
      <label>Clip name<input bind:value={name} maxlength="64" /></label>
      <label>Optional shortcut<select bind:value={slot}><option value={null}>None</option>{#each [1,2,3,4,5,6,7,8,9] as value}<option value={value}>{soundShortcutLabel(value)}</option>{/each}</select></label>
      <div class="actions"><button type="button" onclick={() => void save()}>{editing ? "Save changes" : "Save clip"}</button>{#if editing}<button type="button" onclick={reset}>Cancel edit</button>{/if}</div>
    </fieldset>
    <p class="description">Up to 24 clips, 8 MiB each and 32 MiB total. Shortcuts work in the composer.</p>
  </div>
{/if}

<style>
  .soundboard { position: absolute; bottom: calc(100% + 8px); left: 0; z-index: 30; width: min(420px, calc(100vw - 32px)); max-height: min(65vh, 600px); overflow: auto; box-sizing: border-box; padding: 14px; color: var(--text); background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); box-shadow: 0 8px 24px var(--shadow); }
  header, li, .actions { display: flex; align-items: center; gap: 8px; }
  header { justify-content: space-between; }
  h2 { font-size: 17px; margin: 0; }
  p, .description { font-size: 12px; color: var(--muted); }
  .error { color: var(--danger); overflow-wrap: anywhere; }
  .soundboard-error { position: absolute; bottom: calc(100% + 8px); left: 0; z-index: 30; margin: 0; max-width: min(420px, calc(100vw - 32px)); padding: 8px; background: var(--surface); border: 1px solid var(--line); border-radius: var(--radius); }
  ul { list-style: none; padding: 0; display: grid; gap: 8px; }
  .send { flex: 1; min-width: 0; display: grid; gap: 3px; text-align: left; }
  .send strong { overflow-wrap: anywhere; }
  .send span { color: var(--muted); font-size: 11px; }
  button, input, select { font: inherit; color: var(--text); background: var(--raised); border: 1px solid var(--line); border-radius: var(--radius); padding: 7px; }
  button { cursor: pointer; } button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  fieldset { border: 0; margin: 0; padding: 8px 0 0; display: grid; gap: 8px; }
  legend { font-size: 13px; } label { font-size: 12px; display: grid; gap: 4px; }
  input, select { min-width: 0; box-sizing: border-box; width: 100%; }
</style>
