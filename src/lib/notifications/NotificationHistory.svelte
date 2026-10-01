<script lang="ts">
  import { notificationHistory } from "./history-store";
  import type { NotificationHistoryEntry } from "./history";

  let { account, connected = true, onjump, onclose }: {
    account: string | null;
    connected?: boolean;
    onjump: (account: string, chat: string, id: string) => Promise<void>;
    onclose?: () => void;
  } = $props();

  let busy = $state<string | null>(null);
  let jumpError = $state<string | null>(null);
  let generation = 0;
  const entries = $derived($notificationHistory.account === account ? $notificationHistory.entries : []);
  const error = $derived($notificationHistory.account === account ? $notificationHistory.error : null);

  $effect(() => {
    generation++; busy = null; jumpError = null;
    notificationHistory.load(account);
  });

  async function jump(entry: NotificationHistoryEntry) {
    if (!account || busy) return;
    const owner = account, epoch = generation;
    const current = () => owner === account && epoch === generation;
    busy = JSON.stringify([entry.chat, entry.id]); jumpError = null;
    try { await onjump(owner, entry.chat, entry.id); if (current()) onclose?.(); }
    catch (failure) { if (current()) jumpError = String(failure); }
    finally { if (current()) busy = null; }
  }
</script>

<section class="notification-history" aria-label="Notification history">
  <header>
    <div><h2>Notification history</h2><p>Recent notification events from this account.</p></div>
    {#if onclose}<button type="button" class="close" onclick={onclose} aria-label="Close notification history">×</button>{/if}
  </header>
  {#if !account}
    <p class="empty">Choose an account to view its notification history.</p>
  {:else}
    {#if !connected}<p class="offline" role="status">Offline. This is local history.</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if jumpError}<p class="error" role="alert">{jumpError}</p>{/if}
    {#if entries.length === 0}
      <p class="empty">No recent notification events.</p>
    {:else}
      <ol>
        {#each entries as entry (JSON.stringify([entry.chat, entry.id]))}
          <li><button type="button" class="entry" disabled={busy !== null} onclick={() => jump(entry)}>
            <span class="heading"><strong>{entry.title || entry.chat_name || entry.chat}</strong><time datetime={new Date(entry.timestamp).toISOString()}>{new Date(entry.timestamp).toLocaleString()}</time></span>
            <span class="metadata">Chat: {entry.chat_name || entry.chat} · Sender: {entry.sender_name || entry.sender || "Unknown sender"}</span>
            <span class="body">{entry.body}</span>
          </button></li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  .notification-history { min-width: 0; color: var(--text); }
  header { display: flex; align-items: start; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: 18px; }
  header p, .metadata, time { color: var(--muted); font-size: 12px; }
  header p { margin: 6px 0 16px; }
  .close { background: transparent; color: inherit; border: 0; font-size: 22px; cursor: pointer; }
  ol { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
  .entry { width: 100%; display: grid; gap: 6px; padding: 12px; text-align: left; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); color: inherit; cursor: pointer; }
  .entry:hover { background: var(--raised); }
  .entry:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .entry:disabled { cursor: wait; opacity: .7; }
  .heading { display: flex; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  strong, .metadata, .body { overflow-wrap: anywhere; }
  time { white-space: nowrap; }
  .body { font-size: 13px; white-space: pre-wrap; }
  .empty, .offline { color: var(--muted); font-size: 13px; }
  .error { color: var(--danger); font-size: 13px; overflow-wrap: anywhere; }
</style>
