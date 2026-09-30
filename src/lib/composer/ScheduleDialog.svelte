<script lang="ts">
  import { untrack } from "svelte";
  import Button from "$lib/ui/Button.svelte";
  import { ui } from "$lib/state/ui.svelte";

  let { text, dueAt = Math.floor(Date.now() / 1000) + 3600, editing = false, onsave, onclose }: {
    text: string;
    dueAt?: number;
    editing?: boolean;
    onsave: (text: string, dueAt: number) => Promise<boolean>;
    onclose: () => void;
  } = $props();

  const localTime = (seconds: number) => {
    const date = new Date(seconds * 1000);
    return new Date(date.getTime() - date.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  };
  let body = $state(untrack(() => text));
  let time = $state(untrack(() => localTime(dueAt)));
  let saving = $state(false);
  let error = $state("");
  let dialog: HTMLDialogElement;

  $effect(() => { dialog?.showModal(); });

  async function save() {
    if (saving) return;
    const due = Math.floor(new Date(time).getTime() / 1000);
    if (!Number.isFinite(due) || due <= Math.floor(Date.now() / 1000)) {
      error = "Choose a future time.";
      return;
    }
    saving = true;
    try {
      if (await onsave(body, due)) onclose();
      else error = ui.error || "Could not save this schedule. Try again.";
    } catch (failure) { error = String(failure); }
    finally { saving = false; }
  }
</script>

<dialog bind:this={dialog} oncancel={(event) => { event.preventDefault(); if (!saving) onclose(); }} aria-label={editing ? "Edit scheduled message" : "Schedule message"}>
  <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <h2>{editing ? "Edit scheduled message" : "Schedule message"}</h2>
    <p>Postal sends when this account is connected and the app is open. Missed times send on the next connection.</p>
    <label>Message<textarea bind:value={body} readonly={!editing} rows="3" required></textarea></label>
    <label>Send at, in your local time<input type="datetime-local" bind:value={time} required /></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <Button variant="ghost" type="button" disabled={saving} onclick={onclose}>Cancel</Button>
      <Button variant="primary" type="submit" disabled={saving || !body.trim()}>{saving ? "Saving…" : editing ? "Save" : "Schedule"}</Button>
    </div>
  </form>
</dialog>

<style>
  dialog { width: min(420px, 85vw); color: var(--text); background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 22px; box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  form, label { display: flex; flex-direction: column; gap: 10px; }
  form { gap: 16px; }
  h2, p { margin: 0; }
  h2 { font-size: 18px; }
  p { color: var(--muted); font-size: 13px; line-height: 1.5; }
  input, textarea { padding: 9px 10px; color: inherit; font: inherit; background: var(--bg); border: 1px solid var(--line-strong); border-radius: 6px; }
  textarea { resize: vertical; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .error { color: var(--danger); }
</style>
