<script lang="ts">
  import { formatDate as localeDate } from "$lib/i18n/localizer";
  import { t } from "$lib/i18n/localizer";
  import { scheduled } from "$lib/state/scheduled.svelte";
  import type { ScheduledMessageView } from "$lib/utils/wire";
  import { messageText } from "$lib/i18n/errors";
  import { session } from "$lib/state/session.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ScheduleDialog from "./ScheduleDialog.svelte";

  let { enqueue, displayName = (chat: string) => chat }: {
    enqueue: <T>(task: (signal: AbortSignal) => Promise<T>) => Promise<T>;
    displayName?: (chat: string) => string;
  } = $props();
  let editing = $state<ScheduledMessageView | null>(null);
  let changing = $state<string | null>(null);
  let dialog: HTMLDialogElement;

  $effect(() => { scheduled.selectAccount(session.activeAccount); editing = null; });
  $effect(() => {
    const account = session.activeAccount;
    if (!account || !session.started) return;
    void scheduled.refresh(account);
    if (!session.connected) return;
    void scheduled.tick(enqueue);
    const timer = setInterval(() => { void scheduled.tick(enqueue); }, 1000);
    return () => clearInterval(timer);
  });
  $effect(() => {
    if (scheduled.open && dialog && !dialog.open) dialog.showModal();
    else if (!scheduled.open && dialog?.open) dialog.close();
  });

  async function change(command: "cancel_scheduled_message" | "retry_scheduled_message", id: string) {
    changing = id;
    try { await scheduled.change(command, { id }); }
    finally { changing = null; }
  }
</script>

<dialog bind:this={dialog} oncancel={() => (scheduled.open = false)} aria-label={t("content.scheduled_messages")}>
  <div class="header"><h2>{t("content.scheduled_messages")}</h2><Button variant="icon" icon="x" title={t("content.close")} aria-label={t("content.close_scheduled_messages")} onclick={() => (scheduled.open = false)} /></div>
  <p>{t("content.messages_send_while_postal_is_open_and_this_account_is_connected_missed_")}</p>
  {#if scheduled.error}<p class="error" role="alert">{scheduled.error}</p>{/if}
  {#if scheduled.diagnostic}<details><summary>{t("content.technical_details")}</summary><pre dir="ltr">{scheduled.diagnostic}</pre></details>{/if}
  {#if scheduled.items.length === 0}<p>{t("content.no_scheduled_messages")}</p>{/if}
  <ul>
    {#each scheduled.items as item (item.id)}
      <li>
        <div class="header"><strong><bdi dir="auto">{displayName(item.chat)}</bdi></strong><span>{localeDate((new Date(item.due_at * 1000)).getTime()/1000, { dateStyle: "medium", timeStyle: "short" })}</span></div>
        <div class="message"><bdi dir="auto">{item.text}</bdi></div>
        <div class="status">{item.status === "uncertain" ? t("content.delivery_unconfirmed") : item.status === "sending" ? t("content.sending") : item.status === "failed" ? t("content.failed") : t("content.pending_e8nfto")}</div>
        {#if item.failure}<p class="error">{messageText(item.failure)}</p>{#if item.failure.diagnostic}<details><summary>{t("content.technical_details")}</summary><pre dir="ltr">{item.failure.diagnostic}</pre></details>{/if}{/if}
        {#if item.status === "uncertain" || item.status === "failed"}<p>{t("content.delivery_may_have_succeeded_retry_reuses_the_message_id")}</p>{/if}
        <div class="actions">
          {#if item.status === "pending" && !item.attempted}<Button variant="ghost" disabled={changing !== null} onclick={() => (editing = item)}>{t("content.edit")}</Button>{/if}
          {#if item.status === "uncertain" || item.status === "failed"}<Button variant="ghost" disabled={changing !== null} onclick={() => void change("retry_scheduled_message", item.id)}>{t("content.retry")}</Button>{/if}
          <Button variant="ghost" disabled={item.status === "sending" || changing !== null} onclick={() => void change("cancel_scheduled_message", item.id)}>{t("content.cancel")}</Button>
        </div>
      </li>
    {/each}
  </ul>
</dialog>

{#if editing}
  <ScheduleDialog text={editing.text} dueAt={editing.due_at} editing
    onsave={(text, dueAt) => scheduled.change("update_scheduled_message", { id: editing!.id, text, dueAt })}
    onclose={() => (editing = null)} />
{/if}

<style>
  pre { white-space: pre-wrap; overflow-wrap: anywhere; }
  dialog { width: min(560px, 85vw); max-height: 80vh; color: var(--text); background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 22px; box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  h2 { margin: 0; font-size: 1.125rem; }
  p, span, .status { color: var(--muted); font-size: 0.8125rem; line-height: 1.5; }
  ul { list-style: none; padding: 0; margin: 0; }
  li { border-top: 1px solid var(--line-strong); padding: 14px 0; }
  .header, .actions { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .actions { justify-content: flex-end; }
  .header span { text-align: end; }
  .message { white-space: pre-wrap; overflow-wrap: anywhere; margin: 10px 0; }
  .error { color: var(--danger); overflow-wrap: anywhere; }
</style>
