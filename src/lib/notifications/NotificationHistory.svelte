<script lang="ts">
  import { formatDate as localeDate } from "$lib/i18n/localizer";
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import { notificationHistory } from "./history-store";
  import type { NotificationHistoryEntry } from "./history";

  let { account, connected = true, onjump, onclose }: {
    account: string | null;
    connected?: boolean;
    onjump: (account: string, chat: string, id: string) => Promise<void>;
    onclose?: () => void;
  } = $props();

  let busy = $state<string | null>(null);
  let jumpError = $state<LocalizedError | string | null>(null);
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
    catch (failure) { if (current()) jumpError = normalizeError(failure); }
    finally { if (current()) busy = null; }
  }
</script>

<section class="notification-history" aria-label={t("content.notification_history")}>
  <header>
    <div><h2>{t("content.notification_history")}</h2><p>{t("content.recent_notification_events_from_this_account")}</p></div>
    {#if onclose}<button type="button" class="close" onclick={onclose} aria-label={t("content.close_notification_history")}>×</button>{/if}
  </header>
  {#if !account}
    <p class="empty">{t("content.choose_an_account_to_view_its_notification_history")}</p>
  {:else}
    {#if !connected}<p class="offline" role="status">{t("content.offline_this_is_local_history")}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if jumpError}<p class="error" role="alert">{jumpError}</p>{/if}
    {#if entries.length === 0}
      <p class="empty">{t("content.no_recent_notification_events")}</p>
    {:else}
      <ol>
        {#each entries as entry (JSON.stringify([entry.chat, entry.id]))}
          <li><button type="button" class="entry" disabled={busy !== null} onclick={() => jump(entry)}>
            <span class="heading"><strong><bdi dir="auto">{entry.title || entry.chat_name || entry.chat}</bdi></strong><time datetime={new Date(entry.timestamp).toISOString()}>{localeDate((new Date(entry.timestamp)).getTime()/1000, { dateStyle: "medium", timeStyle: "short" })}</time></span>
            <span class="metadata">{t("content.chat_f7iyqr")} <bdi dir="auto">{entry.chat_name || entry.chat}</bdi> {t("content.sender")} <bdi dir="auto">{entry.sender_name || entry.sender || t("content.unknown_sender")}</bdi></span>
            <span class="body"><bdi dir="auto">{entry.body}</bdi></span>
          </button></li>
        {/each}
      </ol>
    {/if}
  {/if}
</section>

<style>
  .notification-history { min-width: 0; color: var(--text); padding-top: 1rem; }
  header { display: flex; align-items: start; justify-content: space-between; gap: 12px; }
  h2 { margin: 0; font-size: 18px; }
  header p, .metadata, time { color: var(--muted); font-size: 12px; }
  header p { margin: 6px 0 16px; }
  .close { background: transparent; color: inherit; border: 0; font-size: 22px; cursor: pointer; }
  ol { list-style: none; padding: 0; margin: 0; display: grid; gap: 8px; }
  .entry { width: 100%; display: grid; gap: 6px; padding: 12px; text-align: start; border: 1px solid var(--line); border-radius: var(--radius); background: var(--surface); color: inherit; cursor: pointer; }
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
