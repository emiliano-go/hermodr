<script lang="ts">
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
    error?: string | null;
    labels?: InboxLabel[] | null;
    labelsByChat?: Readonly<Record<string, readonly string[]>>;
    labelsWritable?: boolean;
    labelsLoading?: boolean;
    labelsComplete?: boolean;
    labelsError?: string | null;
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

  const kinds = [["unread", "Unread"], ["mentions", "Mentions"], ["labelled", "Labelled"], ["muted", "Muted"], ["archived", "Archived"]] as const;
  const emptyFilters = (): InboxFilters => ({ unread: false, mentions: false, labelled: false, muted: false, archived: false, label: "", query: "" });
  let filters = $state(emptyFilters());
  let busy = $state<Record<string, boolean>>({});
  let failures = $state<Record<string, string>>({});
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
    catch (failure) { if (current()) failures = { ...failures, [chat.chat]: String(failure) }; }
    finally { if (current()) busy = { ...busy, [chat.chat]: false }; }
  }
</script>

<section class="inbox" aria-label="Unified inbox">
  <header><h2>Inbox</h2><span class="connection" role="status">{connected ? "Connected" : "Offline, showing cached chats"}</span></header>
  {#if syncPending > 0}
    <div class="sync" role="status">
      <span>{connected ? "Catching up" : "Catch-up paused while offline"}: {Math.min(syncPending, Math.max(0, syncApplied))} of {syncPending} messages</span>
      <progress aria-label="Offline message catch-up" max={syncPending} value={Math.min(syncPending, Math.max(0, syncApplied))}></progress>
    </div>
  {/if}
  {#if historyPercent !== null}
    <div class="sync" role="status"><span>History sync: {Math.min(100, Math.max(0, historyPercent))}%</span>
      <progress aria-label="History sync" max="100" value={Math.min(100, Math.max(0, historyPercent))}></progress></div>
  {/if}
  {#if backfill}<p class="status" role="status">Loading older history: {backfill.done} of {backfill.total} chats</p>{/if}
  {#if finalizing}<p class="status" role="status">Finishing message sync…</p>{/if}
  <div class="filters">
    <input class="search" type="search" aria-label="Search inbox chats" placeholder="Search chats" bind:value={filters.query} />
    <fieldset><legend>Match all selected filters</legend>
      {#each kinds as [kind, label]}
        <label class="pill" class:on={filters[kind]}>
          <input type="checkbox" bind:checked={filters[kind]} disabled={kind === "labelled" && labels === null} />
          <span>{label}</span>
        </label>
      {/each}
    </fieldset>
    <label class="choice">Label <select bind:value={filters.label} disabled={labels === null}>
      <option value="">Any label</option>{#each labels ?? [] as label (label.id)}<option value={label.id}>{label.name}</option>{/each}
    </select></label>
    <label class="choice">Mute duration <select bind:value={muteSeconds}>
      <option value={8 * 3600}>8 hours</option><option value={7 * 86400}>1 week</option><option value={-1}>Always</option>
    </select></label>
    {#if filtered}<button onclick={() => (filters = emptyFilters())}>Clear filters</button>{/if}
  </div>
  {#if labelsLoading}<p class="status" role="status">Loading labels…</p>
  {:else if labels === null && !labelsError}<p class="status">Labels are not available for this account.</p>
  {:else if !labelsWritable}<p class="status">Labels are read-only for this account.</p>{/if}
  {#if labels !== null && !labelsComplete}<p class="status">Showing labels stored on this device. WhatsApp may have more labels.</p>{/if}
  {#if labelsError}<p class="error" role="alert">Could not load labels: {labelsError}</p>{/if}
  {#if error}<div class="error" role="alert">{error}{#if onretry}<button onclick={onretry} disabled={loading}>Retry</button>{/if}</div>{/if}
  {#if !account}<p class="status" role="status">Select an account to open the inbox.</p>
  {:else}
    {#if loading}<p class="status" role="status">Loading inbox…</p>{/if}
    {#if labelsLoading && (filters.labelled || filters.label)}<p class="status" role="status">Waiting for labels to finish loading.</p>
    {:else if shown.length === 0 && !loading && !error}
      <p class="status" role="status">{filtered ? "No chats match these filters." : chats.length === 0 ? "No chats stored for this account." : "No unread, mentioned, labelled, muted or archived chats stored on this device."}</p>
    {/if}
    <ul>
      {#each shown as chat (chat.chat)}
        {@const name = chatLabelOf(chat)}
        {@const categories = inboxCategories(chat, assigned[chat.chat] ?? [], now)}
        {@const disabled = !connected || loading || !!busy[chat.chat]}
        <li>
          <button class="chat" onclick={() => onopen(chat.chat)} disabled={loading}>
            <Avatar src={avatarOf(chat.chat)} label={name} seed={chat.chat} cls="inbox-avatar" />
            <span class="body"><span class="title">{name}</span><span class="preview">{previewTextOf(chat) || "No stored messages"}</span></span>
            {#if chat.last_message_at > 0}<time>{formatTime(chat.last_message_at)}</time>{/if}
          </button>
          <div class="badges">
            {#if categories.unread}<span>{chat.unread_count > 0 ? `${chat.unread_count} unread` : "Marked unread"}</span>{/if}
            {#if categories.mentions}<button class="mention" onclick={() => onopen(chat.chat, true)} disabled={loading}>{chat.mention_count} {chat.mention_count === 1 ? "mention" : "mentions"}</button>{/if}
            {#if categories.muted}<span>Muted</span>{/if}{#if categories.archived}<span>Archived</span>{/if}
            {#each (labels ?? []).filter((label) => assigned[chat.chat]?.includes(label.id)) as label (label.id)}
              <span class="label">{label.name}{#if labelsWritable}<button aria-label={`Remove ${label.name} from ${name}`} disabled={disabled}
                onclick={() => act(chat, { kind: "label", label: label.id, applied: false })}>×</button>{/if}</span>
            {/each}
          </div>
          <div class="actions" aria-label={`Actions for ${name}`}>
            <button disabled={disabled} onclick={() => act(chat, { kind: "read", read: categories.unread })}>{categories.unread ? "Mark read" : "Mark unread"}</button>
            <button disabled={disabled} onclick={() => act(chat, { kind: "archive", archived: !chat.archived })}>{chat.archived ? "Unarchive" : "Archive"}</button>
            <button disabled={disabled} onclick={() => act(chat, { kind: "mute", seconds: categories.muted ? 0 : muteSeconds })}>{categories.muted ? "Unmute" : "Mute"}</button>
            <select aria-label={`Apply label to ${name}`} disabled={disabled || labels === null || !labelsWritable || labelsLoading}
              value="" onchange={(event) => { const label = event.currentTarget.value; event.currentTarget.value = ""; if (label) void act(chat, { kind: "label", label, applied: true }); }}>
              <option value="">Apply label</option>
              {#each appliableLabels(labels ?? [], assigned[chat.chat]) as label (label.id)}<option value={label.id}>{label.name}</option>{/each}
            </select>
            {#if busy[chat.chat]}<span role="status">Updating…</span>{/if}
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
  h2 { margin: 0; font-size: 20px; }
  .connection, .status, .sync { color: var(--muted); font-size: 13px; }
  .sync { display: grid; gap: 5px; margin: 12px 0; }
  progress { width: 100%; height: 7px; accent-color: var(--accent); }
  .filters { display: flex; flex-wrap: wrap; align-items: center; gap: 10px; margin: 18px 0 12px; }
  .search { flex: 1 1 100%; min-width: 0; }
  input, select { padding: 7px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; }
  fieldset { display: flex; flex-wrap: wrap; gap: 8px; margin: 0; padding: 0; border: 0; }
  legend { margin-bottom: 6px; color: var(--muted); font-size: 12px; }
  fieldset label, .choice { display: flex; align-items: center; gap: 5px; font-size: 13px; }
  .filters > .choice:first-of-type { margin-left: auto; }
  .choice select { border: 0; border-radius: 999px; background: var(--surface); color: var(--muted); padding: 5px 12px; font-size: 13px; }
  .choice select:hover { background: var(--raised); }
  .pill { position: relative; padding: 5px 12px; border-radius: 999px; background: var(--surface); color: var(--muted); cursor: pointer; }
  .pill:hover { background: var(--raised); }
  .pill.on { background: var(--accent-soft); color: var(--accent); }
  .pill input { position: absolute; inset: 0; margin: 0; padding: 0; opacity: 0; cursor: pointer; }
  .pill:has(input:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .pill:has(input:disabled) { opacity: 0.55; cursor: default; }
  button { padding: 6px 9px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
  button:hover:not(:disabled) { background: var(--raised-2); }
  button:disabled, select:disabled { opacity: 0.55; cursor: default; }
  ul { margin: 0; padding: 0; list-style: none; }
  li { padding: 14px 0; border-bottom: 1px solid var(--line); }
  .chat { display: flex; width: 100%; align-items: center; gap: 10px; padding: 0; border: 0; background: transparent; text-align: left; }
  .body { flex: 1; min-width: 0; display: grid; gap: 4px; }
  .title { font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
  .preview { color: var(--muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  time { flex: none; color: var(--muted); font-size: 11px; }
  .badges, .actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 10px; }
  .badges { color: var(--muted); font-size: 12px; }
  .badges > span, .mention { padding: 3px 7px; border: 0; border-radius: var(--radius-sm); background: var(--surface); }
  .mention { color: var(--accent-text); }
  .label { display: inline-flex; align-items: center; gap: 5px; }
  .label button { padding: 0 3px; border: 0; background: transparent; font-size: 15px; }
  .actions select { max-width: 100%; font-size: 12px; }
  .actions span { color: var(--muted); font-size: 12px; }
  .error { color: var(--danger); font-size: 13px; overflow-wrap: anywhere; }
  .error button { margin-left: 10px; }
  :global(.inbox-avatar) { width: 36px; height: 36px; flex: none; border-radius: 50%; object-fit: cover; display: grid; place-items: center; background: hsl(var(--hue, 0) 25% 35%); color: white; font-size: 14px; }
  @media (max-width: 480px) { .inbox { padding: 12px; } time { max-width: 80px; text-align: right; } }
</style>
