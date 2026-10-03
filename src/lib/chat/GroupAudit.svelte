<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import { AUDIT_KINDS, auditEntryRequested, auditEntryLabel, auditFilters, mergeAuditEntries } from "$lib/utils/group-audit";
  import type { GroupAuditCursor } from "$lib/utils/wire";
  import type { AuditEntry, AuditFilters, AuditPage, AuditScope } from "$lib/utils/group-audit";

  let { account, group, requestKey, onload, onjump, namer, formatTime, actor = "", member = null, title = null }: {
    account: string | null;
    group: string;
    requestKey: string | number;
    actor?: string;
    member?: string | null;
    title?: string | null;
    onload: (scope: AuditScope, filters: AuditFilters, cursor: GroupAuditCursor | null) => Promise<AuditPage>;
    onjump: (group: string, messageId: string) => void;
    namer: (jid: string) => string;
    formatTime: (timestamp: number) => string;
  } = $props();

  let kind = $state("");
  let actorDraft = $state("");
  let from = $state("");
  let to = $state("");
  let rows = $state.raw<AuditEntry[]>([]);
  let cursor = $state.raw<GroupAuditCursor | null>(null);
  let loaded = $state(false);
  let loading = $state(false);
  let error = $state<LocalizedError | string>("");
  let generation = 0;
  const checked = $derived(auditFilters(kind, actorDraft, from, to));

  $effect(() => {
    account; group; actor; member;
    ++generation;
    kind = from = to = "";
    actorDraft = actor;
    rows = []; cursor = null; loaded = loading = false; error = "";
    return () => { ++generation; };
  });
  $effect(() => {
    account; group; requestKey; member; checked;
    const revision = ++generation;
    rows = []; cursor = null; loaded = loading = false; error = "";
    if (account && group && checked.filters) void untrack(() => load(false, revision));
    return () => { ++generation; };
  });

  async function load(older: boolean, revision = generation) {
    if (!account || !group || loading || !checked.filters || older && cursor === null) return;
    const scope = { account, group, member, requestKey }, filters = checked.filters, pageCursor = older ? cursor : null;
    const current = () => revision === generation && scope.account === account && scope.group === group && scope.member === member && scope.requestKey === requestKey;
    loading = true;
    error = "";
    try {
      const page = await onload(scope, filters, pageCursor);
      if (!current()) return;
      if (page.entries.some((entry) => entry.chat !== scope.group)) throw normalizeError({ kind: "postal_error", code: "error.group_audit_scope", params: {} });
      rows = older ? mergeAuditEntries(rows, page.entries) : page.entries;
      cursor = page.has_more ? page.next_cursor : null;
      loaded = true;
    } catch (cause) { if (current()) error = normalizeError(cause); }
    finally { if (current()) loading = false; }
  }
</script>

<section class="audit" aria-label={title ?? t("group.audit")}>
  <header><h3>{title ?? t("group.audit")}</h3><button disabled={!account || loading || !checked.filters} onclick={() => load(false)}>{t("ui.refresh")}</button></header>
  <p class="muted">{t("group.audit_hint")}</p>
  <div class="filters">
    <label>{t("group.audit_kind")} <select aria-label={t("group.audit_kind")} bind:value={kind}><option value="">{t("group.audit_all_kinds")}</option>{#each AUDIT_KINDS as value}<option {value}>{t(`group.audit_kind_${value}`)}</option>{/each}</select></label>
    <label>{t("group.audit_actor_address")} <input bind:value={actorDraft} dir="ltr" placeholder={t("group.audit_all_actors")} /></label>
    <label>{t("ui.from")} <input type="date" bind:value={from} /></label>
    <label>{t("ui.to")} <input type="date" bind:value={to} /></label>
  </div>
  {#if !account}<p role="status">{t("group.audit_select_account")}</p>
  {:else if checked.error}<p class="error" role="alert">{checked.error}</p>
  {:else}
    {#if loading}<p class="muted" role="status">{t("group.audit_loading")}</p>{/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    {#if loaded && !loading && !error && rows.length === 0}<p class="muted" role="status">{t("group.audit_empty")}</p>{/if}
    <ol>
      {#each rows as entry (entry.id)}
        <li>
          <div class="row-head"><time>{entry.timestamp !== null ? formatTime(entry.timestamp) : t("group.audit_observed", { time: formatTime(entry.observed_at) })}</time><span>{entry.actor ? namer(entry.actor) : t("group.audit_actor_missing")}</span></div>
          <p>{auditEntryLabel(entry)}{entry.target ? ` · ${entry.target.includes("@") ? namer(entry.target) : entry.target}` : ""}</p>
          <dl><dt>{t("group.audit_before")}</dt><dd dir="auto">{entry.old_value === null ? t("ui.not_recorded") : entry.old_value || "(empty)"}</dd>
            <dt>{auditEntryRequested(entry) ? t("group.audit_requested") : t("group.audit_after")}</dt><dd>{entry.new_value === null ? t("ui.not_recorded") : entry.new_value || "(empty)"}</dd></dl>
          <small>{t("group.audit_source_label", { source: t(({ stored: "group.audit_source_stored", notification: "group.audit_source_notification", message: "group.audit_source_message", history: "group.audit_source_history", local: "group.audit_source_local" } as Record<string, string>)[entry.source] ?? "group.audit_source_unknown") })}{entry.old_source ? t("group.audit_previous_source", { source: t(entry.old_source === "cached" ? "group.audit_previous_cached" : "group.audit_previous_protocol") }) : ""}{entry.timestamp === null ? t("group.audit_time_missing") : ""}</small>
          {#if entry.jump_available && entry.message_id}<button onclick={() => onjump(group, entry.message_id!)}>{t("chat.open_message")}</button>
          {:else if entry.message_id}<small>{t("group.audit_message_unavailable")}</small>{/if}
        </li>
      {/each}
    </ol>
    {#if cursor !== null}<button disabled={loading} onclick={() => load(true)}>{t("group.audit_older")}</button>
    {:else if loaded && rows.length > 0}<p class="muted" role="status">{t("group.audit_end")}</p>{/if}
  {/if}
</section>

<style>
  .audit { min-width: 0; color: var(--text); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 10px; }
  h3 { margin: 0; font-size: 16px; }
  .filters { display: flex; flex-wrap: wrap; gap: 10px; margin: 14px 0; }
  label { flex: 1 1 150px; display: grid; gap: 5px; font-size: 12px; }
  input, select { min-width: 0; padding: 7px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg); color: inherit; font: inherit; }
  button { padding: 6px 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--surface); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
  button:disabled { opacity: 0.55; cursor: default; }
  .muted, small, time { color: var(--muted); font-size: 12px; }
  .error { color: var(--danger); font-size: 13px; overflow-wrap: anywhere; }
  ol { list-style: none; padding: 0; margin: 12px 0; }
  li { padding: 12px 0; border-bottom: 1px solid var(--line); overflow-wrap: anywhere; }
  .row-head { display: flex; flex-wrap: wrap; gap: 12px; font-size: 12px; }
  li p { margin: 6px 0; font-size: 13px; }
  small { display: block; margin-bottom: 6px; }
  dl { display: grid; grid-template-columns: 60px minmax(0, 1fr); gap: 5px; margin: 6px 0; font-size: 12px; } dt { color: var(--muted); } dd { margin: 0; white-space: pre-wrap; }
</style>
