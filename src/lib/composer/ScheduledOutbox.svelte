<script lang="ts">
  import { scheduled, type ScheduledMessage } from "$lib/state/scheduled.svelte";
  import { session } from "$lib/state/session.svelte";
  import Button from "$lib/ui/Button.svelte";
  import ScheduleDialog from "./ScheduleDialog.svelte";

  let { enqueue, displayName = (chat: string) => chat }: {
    enqueue: <T>(task: (signal: AbortSignal) => Promise<T>) => Promise<T>;
    displayName?: (chat: string) => string;
  } = $props();
  let editing = $state<ScheduledMessage | null>(null);
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

<dialog bind:this={dialog} oncancel={() => (scheduled.open = false)} aria-label="Scheduled messages">
  <div class="header"><h2>Scheduled messages</h2><Button variant="icon" icon="x" title="Close" aria-label="Close scheduled messages" onclick={() => (scheduled.open = false)} /></div>
  <p>Messages send while Postal is open and this account is connected. Missed times send on its next connection.</p>
  {#if scheduled.error}<p class="error" role="alert">{scheduled.error}</p>{/if}
  {#if scheduled.items.length === 0}<p>No scheduled messages.</p>{/if}
  <ul>
    {#each scheduled.items as item (item.id)}
      <li>
        <div class="header"><strong>{displayName(item.chat)}</strong><span>{new Date(item.due_at * 1000).toLocaleString()}</span></div>
        <div class="message">{item.text}</div>
        <div class="status">{item.status === "uncertain" ? "Delivery unconfirmed" : item.status === "sending" ? "Sending…" : item.status === "failed" ? "Failed" : "Pending"}</div>
        {#if item.error}<p class="error">{item.error}</p>{/if}
        {#if item.status === "uncertain" || item.status === "failed"}<p>Delivery may have succeeded. Retry reuses the message ID.</p>{/if}
        <div class="actions">
          {#if item.status === "pending" && !item.attempted}<Button variant="ghost" disabled={changing !== null} onclick={() => (editing = item)}>Edit</Button>{/if}
          {#if item.status === "uncertain" || item.status === "failed"}<Button variant="ghost" disabled={changing !== null} onclick={() => void change("retry_scheduled_message", item.id)}>Retry</Button>{/if}
          <Button variant="ghost" disabled={item.status === "sending" || changing !== null} onclick={() => void change("cancel_scheduled_message", item.id)}>Cancel</Button>
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
  dialog { width: min(560px, 85vw); max-height: 80vh; color: var(--text); background: var(--surface); border: 1px solid var(--line-strong); border-radius: var(--radius-lg); padding: 22px; box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  h2 { margin: 0; font-size: 18px; }
  p, span, .status { color: var(--muted); font-size: 13px; line-height: 1.5; }
  ul { list-style: none; padding: 0; margin: 0; }
  li { border-top: 1px solid var(--line-strong); padding: 14px 0; }
  .header, .actions { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  .actions { justify-content: flex-end; }
  .header span { text-align: right; }
  .message { white-space: pre-wrap; overflow-wrap: anywhere; margin: 10px 0; }
  .error { color: var(--danger); overflow-wrap: anywhere; }
</style>
