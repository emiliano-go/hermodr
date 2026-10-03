<script lang="ts">
  import { t, formatNumber } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import Avatar from "$lib/ui/Avatar.svelte";
  import { appliableLabels, inboxCategories, inboxChats } from "$lib/utils/inbox";
  import type { InboxAction, InboxFilters, InboxLabel } from "$lib/utils/inbox";
  import type { ChatSummary } from "$lib/utils/wire";

  let { account, requestKey = 0, connected, chats, loading = false, error = "", labels = null,
    labelsByChat = {}, labelsWritable = false, labelsLoading = false, labelsComplete = false, labelsError = "", chatLabelOf, avatarOf = () => null,
    previewTextOf = (chat) => chat.last_text, formatTime, syncPending = 0, syncApplied = 0,
    historyPercent = null, backfill = null, finalizing = false, onopen, onaction, onretry, initialFilters, onfilterschange }: {
    account: string | null;
    requestKey?: string | number;
    connected: boolean;
    chats: ChatSummary[];
    loading?: boolean;
    error?: LocalizedError | string | null;
    labels?: InboxLabel[] | null;
    labelsByChat?: Readonly<Record<string, readonly string[]>>;
    labelsWritable?: boolean;
    labelsLoading?: boolean;
    labelsComplete?: boolean;
    labelsError?: LocalizedError | string | null;
    chatLabelOf: (chat: ChatSummary) => string;
    avatarOf?: (jid: string) => string | null;
    previewTextOf?: (chat: ChatSummary) => string;
    formatTime: (timestamp: number) => string;
    syncPending?: number;
    syncApplied?: number;
    historyPercent?: number | null;
    backfill?: { done: number; total: number } | null;
    finalizing?: boolean;
    onopen: (chat: string, mention?: boolean) => void;
    onaction: (account: string, chat: string, action: InboxAction) => Promise<void>;
    onretry?: () => void;
    initialFilters?: InboxFilters;
    onfilterschange?: (filters: InboxFilters) => void;
  } = $props();

  const kinds = [["unread", "chat.unread"], ["mentions", "chat.mentions"], ["labelled", "chat.labelled"], ["muted", "chat.muted"], ["archived", "chat.archived"]] as const;
  const emptyFilters = (): InboxFilters => ({ unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" });
  let filters = $state(emptyFilters());
  let busy = $state<Record<string, boolean>>({});
  let failures = $state<Record<string, LocalizedError | string>>({});
  let muteSeconds = $state(8 * 3600);
  let now = $state(Math.floor(Date.now() / 1000));
  let generation = 0;
  const assigned = $derived(labels === null ? {} : labelsByChat);
  const shown = $derived(inboxChats(chats, filters, assigned, chatLabelOf, now));
  const filtered = $derived(filters.query.trim() || filters.label || kinds.some(([kind]) => filters[kind]));

  $effect(() => {
    account; requestKey; connected;
    ++generation;
    busy = {};
    failures = {};
    return () => { ++generation; };
  });
  $effect(() => { account; requestKey; filters = initialFilters ? { ...initialFilters } : emptyFilters(); });
  $effect(() => { const current = { ...filters }; untrack(() => onfilterschange?.(current)); });
  $effect(() => {
    const timer = setInterval(() => { now = Math.floor(Date.now() / 1000); }, 1000);
    return () => clearInterval(timer);
  });

  async function act(chat: ChatSummary, action: InboxAction) {
    if (!account || !connected || loading || busy[chat.chat] || action.kind === "label" && (!labels || !labelsWritable || labelsLoading)) return;
    const owner = account, revision = generation;
    busy = { ...busy, [chat.chat]: true };
    failures = { ...failures, [chat.chat]: "" };
    const current = () => owner === account && revision === generation && connected;
    try { await onaction(owner, chat.chat, action); }
    catch (failure) { if (current()) failures = { ...failures, [chat.chat]: normalizeError(failure) }; }
    finally { if (current()) busy = { ...busy, [chat.chat]: false }; }
  }
</script>

<section class="inbox" aria-label={t("chat.unified_inbox")}>
  <header><h2>{t("chat.inbox")}</h2><span class="connection" role="status">{connected ? t("settings.connected") : t("chat.inbox_offline")}</span></header>
  {#if syncPending > 0}
    <div class="sync" role="status">
      <span>{connected ? t("chat.catching_up") : t("chat.catch_up_paused")}: {t("chat.catch_up_counts", { applied: Math.min(syncPending, Math.max(0, syncApplied)), count: syncPending })}</span>
      <progress aria-label={t("chat.catch_up_label")} max={syncPending} value={Math.min(syncPending, Math.max(0, syncApplied))}></progress>
    </div>
  {/if}
  {#if historyPercent !== null}
    <div class="sync" role="status"><span>{t("chat.history_progress", { percent: formatNumber(Math.min(100, Math.max(0, historyPercent)) / 100, { style: "percent", maximumFractionDigits: 0 }) })}</span>
      <progress aria-label={t("chat.history_sync")} max="100" value={Math.min(100, Math.max(0, historyPercent))}></progress></div>
  {/if}
  {#if backfill}<p class="status" role="status">{t("chat.history_backfill", { done: backfill.done, count: backfill.total })}</p>{/if}
  {#if finalizing}<p class="status" role="status">{t("chat.sync_finishing")}</p>{/if}
  <div class="filters">
    <input class="search" type="search" dir="auto" aria-label={t("chat.inbox_search")} placeholder={t("chat.search")} bind:value={filters.query} />
    <fieldset><legend>{t("chat.match_filters")}</legend>
      {#each kinds as [kind, label]}
        <label class="pill" class:on={filters[kind]}>
          <input type="checkbox" bind:checked={filters[kind]} disabled={kind === "labelled" && labels === null} />
          <span>{t(label)}</span>
        </label>
      {/each}
    </fieldset>
    <label class="choice">{t("labels.label")} <select bind:value={filters.label} disabled={labels === null}>
      <option value="">{t("labels.any")}</option>{#each labels ?? [] as label (label.id)}<option value={label.id}><bdi>{label.name}</bdi></option>{/each}
    </select></label>
    <label class="choice">{t("chat.mute_duration")} <select bind:value={muteSeconds}>
      <option value={8 * 3600}>{t("chat.mute_eight_hours")}</option><option value={7 * 86400}>{t("chat.mute_week")}</option><option value={-1}>{t("ui.always")}</option>
    </select></label>
    {#if filtered}<button onclick={() => (filters = emptyFilters())}>{t("chat.filters_clear")}</button>{/if}
  </div>
  {#if labelsLoading}<p class="status" role="status">{t("labels.loading")}</p>
  {:else if labels === null && !labelsError}<p class="status">{t("labels.unavailable_account")}</p>
  {:else if !labelsWritable}<p class="status">{t("labels.read_only")}</p>{/if}
  {#if labels !== null && !labelsComplete}<p class="status">{t("labels.cached_hint")}</p>{/if}
  {#if labelsError}<p class="error" role="alert">{t("labels.load_failed", { error: normalizeError(labelsError).message })}</p>{/if}
  {#if error}<div class="error" role="alert">{error}{#if onretry}<button onclick={onretry} disabled={loading}>{t("ui.retry")}</button>{/if}</div>{/if}
  {#if !account}<p class="status" role="status">{t("chat.inbox_select_account")}</p>
  {:else}
    {#if loading}<p class="status" role="status">{t("chat.inbox_loading")}</p>{/if}
    {#if labelsLoading && (filters.labelled || filters.label)}<p class="status" role="status">{t("chat.inbox_wait_labels")}</p>
    {:else if shown.length === 0 && !loading && !error}
      <p class="status" role="status">{filtered ? t("chat.inbox_no_matches") : chats.length === 0 ? t("chat.inbox_empty_account") : t("chat.inbox_empty")}</p>
    {/if}
    <ul>
      {#each shown as chat (chat.chat)}
        {@const name = chatLabelOf(chat)}
        {@const categories = inboxCategories(chat, assigned[chat.chat] ?? [], now)}
        {@const disabled = !connected || loading || !!busy[chat.chat]}
        <li>
          <button class="chat" onclick={() => onopen(chat.chat)} disabled={loading}>
            <Avatar src={avatarOf(chat.chat)} label={name} seed={chat.chat} cls="inbox-avatar" />
            <span class="body"><span class="title"><bdi>{name}</bdi></span><span class="preview">{previewTextOf(chat) || t("chat.no_messages")}</span></span>
            {#if chat.last_message_at > 0}<time>{formatTime(chat.last_message_at)}</time>{/if}
          </button>
          <div class="badges">
            {#if categories.unread}<span>{chat.unread_count > 0 ? t("chat.unread_count", { count: chat.unread_count }) : t("chat.marked_unread")}</span>{/if}
            {#if categories.mentions}<button class="mention" onclick={() => onopen(chat.chat, true)} disabled={loading}>{chat.mention_count} {chat.mention_count === 1 ? "mention" : "mentions"}</button>{/if}
            {#if categories.muted}<span>{t("chat.muted")}</span>{/if}{#if categories.archived}<span>{t("chat.archived")}</span>{/if}
            {#each (labels ?? []).filter((label) => assigned[chat.chat]?.includes(label.id)) as label (label.id)}
              <span class="label">{label.name}{#if labelsWritable}<button aria-label={t("labels.remove_from_chat", { label: label.name, chat: name })} disabled={disabled}
                onclick={() => act(chat, { kind: "label", label: label.id, applied: false })}>×</button>{/if}</span>
            {/each}
          </div>
          <div class="actions" aria-label={t("chat.actions_for", { name })}>
            <button disabled={disabled} onclick={() => act(chat, { kind: "read", read: categories.unread })}>{categories.unread ? t("chat.mark_read") : t("chat.mark_unread")}</button>
            <button disabled={disabled} onclick={() => act(chat, { kind: "archive", archived: !chat.archived })}>{chat.archived ? t("chat.unarchive") : t("chat.archive")}</button>
            <button disabled={disabled} onclick={() => act(chat, { kind: "mute", seconds: categories.muted ? 0 : muteSeconds })}>{categories.muted ? t("chat.unmute") : t("chat.mute")}</button>
            <select aria-label={t("labels.apply_to_chat", { name })} disabled={disabled || labels === null || !labelsWritable || labelsLoading}
              value="" onchange={(event) => { const label = event.currentTarget.value; event.currentTarget.value = ""; if (label) void act(chat, { kind: "label", label, applied: true }); }}>
              <option value="">{t("labels.apply_one")}</option>
              {#each appliableLabels(labels ?? [], assigned[chat.chat]) as label (label.id)}<option value={label.id}>{label.name}</option>{/each}
            </select>
            {#if busy[chat.chat]}<span role="status">{t("ui.updating")}</span>{/if}
          </div>
          {#if failures[chat.chat]}<p class="error" role="alert">{failures[chat.chat]}</p>{/if}
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  .inbox { min-width: 0; min-height: 0; overflow: auto; padding: 20px; color: var(--text); background: var(--bg); }
  header { display: flex; align-items: baseline; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
  h2 { margin: 0; font-size: 1.25rem; }
  .connection, .status, .sync { color: var(--muted); font-size: 0.8125rem; }
  .sync { display: grid; gap: 5px; margin: 12px 0; }
  progress { width: 100%; height: 7px; accent-color: var(--accent); }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin: 18px 0 12px; }
  .search { flex: 1 1 100%; min-width: 0; }
  input, select { padding: 7px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; }
  fieldset { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: 6px; color: var(--muted); font-size: 0.75rem; }
  fieldset label, .choice { display: flex; align-items: center; gap: 5px; font-size: 0.8125rem; }
  .filters > .choice:first-of-type { margin-inline-start: auto; }
  .choice select { border: 0; border-radius: 999px; background: var(--surface); color: var(--muted); padding: 5px 12px; font-size: 0.8125rem; }
  .choice select:hover { background: var(--raised); }
  .pill { position: relative; padding: 5px 12px; border-radius: 999px; background: var(--surface); color: var(--muted); cursor: pointer; }
  .pill:hover { background: var(--raised); }
  .pill.on { background: var(--accent-soft); color: var(--accent); }
  .pill input { position: absolute; inset: 0; margin: 0; padding: 0; opacity: 0; cursor: pointer; }
  .pill:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .pill:has(input:disabled) { opacity: 0.55; cursor: default; }
  button { padding: 6px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; font-size: 0.75rem; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--raised-2); }
  button:disabled, select:disabled { opacity: 0.55; cursor: default; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { padding: 14px 0; border-bottom: 1px solid var(--line); }
  .chat { display: flex; width: 100%; align-items: center; gap: 10px; padding: 0; border: 0; background: transparent; text-align: start; }
  .body { flex: 1; min-width: 0; display: grid; gap: 4px; }
  .title { font-size: 0.875rem; font-weight: 600; overflow-wrap: anywhere; }
  .preview { color: var(--muted); font-size: 0.75rem; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  time { flex: none; color: var(--muted); font-size: 0.6875rem; }
  .badges, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 10px; }
  .badges { color: var(--muted); font-size: 0.75rem; }
  .badges > span, .mention { padding: 3px 7px; border: 0; border-radius: var(--radius-sm); background: var(--surface); }
  .mention { color: var(--accent-text); }
  .label { display: inline-flex; align-items: center; gap: 5px; }
  .label button { padding: 0 3px; border: 0; background: transparent; font-size: 0.9375rem; }
  .actions select { max-width: 100%; font-size: 0.75rem; }
  .actions span { color: var(--muted); font-size: 0.75rem; }
  .error { color: var(--danger); font-size: 0.8125rem; overflow-wrap: anywhere; }
  .error button { margin-inline-start: 10px; }
  :global(.inbox-avatar) { width: 36px; height: 36px; flex: none; border-radius: 50%; object-fit: cover; display: grid; place-items: center; background: hsl(var(--hue, 0) 25% 35%); color: white; font-size: 0.875rem; }
  @media (max-width: 480px) { .inbox { padding: 12px; } time { max-width: 80px; text-align: end; } }
</style>
