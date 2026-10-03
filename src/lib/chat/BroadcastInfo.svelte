<script lang="ts">
  import type { LocalizedError } from "$lib/i18n/errors";
  import { normalizeError } from "$lib/i18n/errors";
  import { t, formatDate } from "$lib/i18n/localizer";
  import type { BroadcastList } from "$lib/utils/wire";

  let { info, loading, error, nameOf }: {
    info: BroadcastList | null;
    loading: boolean;
    error: LocalizedError | string | null;
    nameOf: (jid: string) => string;
  } = $props();

  const recipients = $derived([...new Set(info?.recipients ?? [])]);
  const sourceDate = $derived.by(() => {
    const seconds = info?.source_timestamp ?? 0;
    if (!Number.isSafeInteger(seconds) || seconds <= 0) return null;
    const date = new Date(seconds * 1000);
    return Number.isNaN(date.getTime()) ? null : date;
  });
</script>

<section class="broadcast-info" aria-label={t("chat.broadcast_recipients")}>
  <h3>{t("chat.broadcast_recipients")}</h3>
  {#if loading}
    <p role="status">{t("chat.broadcast_loading")}</p>
  {:else if error}
    <p class="error" role="alert">{t("chat.broadcast_unavailable")} {error}</p>
  {:else if !recipients.length}
    <p role="status">{t("chat.broadcast_unavailable")}</p>
  {:else}
    <p class="source">{t("chat.broadcast_cached")}
      {#if sourceDate}
        <time datetime={sourceDate.toISOString()}>{formatDate(sourceDate.getTime() / 1000, { dateStyle: "medium", timeStyle: "short" })}</time>
      {:else}
        <span>{t("chat.message_time_unavailable")}</span>
      {/if}
    </p>
    <ul>
      {#each recipients as jid (jid)}
        <li>{nameOf(jid)}</li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .broadcast-info { padding: 16px; }
  h3 { margin: 0 0 10px; font-size: 0.9375rem; }
  p { margin: 0; }
  .source { color: var(--muted); font-size: 0.75rem; }
  .source time, .source span { display: block; margin-top: 4px; }
  .error { color: var(--danger); }
  ul { margin: 12px 0 0; padding: 0; list-style: none; max-height: 280px; overflow-y: auto; }
  li { padding: 8px 0; overflow-wrap: anywhere; }
  li + li { border-top: 1px solid var(--line); }
</style>
