<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
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
  let error = $state<LocalizedError | string | null>("");
  let dialog: HTMLDialogElement;

  $effect(() => { dialog?.showModal(); });

  async function save() {
    if (saving) return;
    const due = Math.floor(new Date(time).getTime() / 1000);
    if (!Number.isFinite(due) || due <= Math.floor(Date.now() / 1000)) {
      error = new LocalizedError({ kind: "postal_error", code: "error.content.choose_a_future_time", params: {} });
      return;
    }
    saving = true;
    try {
      if (await onsave(body, due)) onclose();
      else error = ui.error || new LocalizedError({ kind: "postal_error", code: "error.content.could_not_save_this_schedule_try_again", params: {} });
    } catch (failure) { error = normalizeError(failure); }
    finally { saving = false; }
  }
</script>

<dialog bind:this={dialog} oncancel={(event) => { event.preventDefault(); if (!saving) onclose(); }} aria-label={editing ? t("content.edit_scheduled_message") : t("content.schedule_message")}>
  <form onsubmit={(event) => { event.preventDefault(); void save(); }}>
    <h2>{editing ? t("content.edit_scheduled_message") : t("content.schedule_message")}</h2>
    <p>{t("content.postal_sends_when_this_account_is_connected_and_the_app_is_open_missed_t")}</p>
    <label>{t("content.message")}<textarea bind:value={body} readonly={!editing} rows="3" required></textarea></label>
    <label>{t("content.send_at_in_your_local_time")}<input type="datetime-local" bind:value={time} required /></label>
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <div class="actions">
      <Button variant="ghost" type="button" disabled={saving} onclick={onclose}>{t("content.cancel")}</Button>
      <Button variant="primary" type="submit" disabled={saving || !body.trim()}>{saving ? t("content.saving") : editing ? t("content.save") : t("content.schedule")}</Button>
    </div>
  </form>
</dialog>

<style>
  dialog { width: min(420px, 85vw); color: var(--text); background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 22px; box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  form, label { display: flex; flex-direction: column; gap: 10px; }
  form { gap: 16px; }
  h2, p { margin: 0; }
  h2 { font-size: 1.125rem; }
  p { color: var(--muted); font-size: 0.8125rem; line-height: 1.5; }
  input, textarea { padding: 9px 10px; color: inherit; font: inherit; background: var(--bg); border: 1px solid var(--line-strong); border-radius: 6px; }
  textarea { resize: vertical; }
  .actions { display: flex; gap: 8px; justify-content: flex-end; }
  .error { color: var(--danger); }
</style>
