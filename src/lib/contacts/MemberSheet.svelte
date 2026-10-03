<script lang="ts">
  import { t, formatNumber } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import type { GroupAuditCursor, Participant, ParticipantChange } from "$lib/utils/wire";
  import type { MemberAction, MemberLocalView, MemberLiveView, MemberScope } from "$lib/utils/member-sheet";
  import type { AuditFilters, AuditPage, AuditScope } from "$lib/utils/group-audit";
  import { memberActionReason, memberBusinessHours, memberFieldText, memberFresh, memberLocalMatches, memberNoteError, memberScopeMatches, memberTyping } from "$lib/utils/member-sheet";
  import { displayName, phoneLabel } from "$lib/utils/phone";
  import { changeText } from "$lib/utils/group-actions";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import GroupAudit from "$lib/chat/GroupAudit.svelte";

  let { account, group, groupName, jid, requestKey, dataScope, title, connected, local: incomingLocal = null, member: incomingMember = null, memberSource = "cached",
    live: incomingLive = null, picture: incomingPicture = null, localLoading = false, liveLoading = false, error = "", liveError = "", liveDiagnostic = null, admin = false,
    blocked = null, supportedActions = [], liveCached = false, liveStale = false, moderationAdminVerified = false,
    moderationVerifiedAt = null, moderationError = "", community = false, auditRevision = 0, namer, formatTime, onaction, onsavelocal, onloadAudit,
    onmessage, ongroup, onrefresh, onclose }: {
    account: string | null; group: string; groupName: string; jid: string; requestKey: string | number; title: string;
    dataScope: MemberScope | null;
    connected: boolean; local?: MemberLocalView | null; member?: Participant | null; memberSource?: "cached" | "live";
    live?: MemberLiveView | null; picture?: string | null; localLoading?: boolean; liveLoading?: boolean;
    error?: LocalizedError | string | null; liveError?: LocalizedError | string | null; liveDiagnostic?: string | null; admin?: boolean; blocked?: boolean | null;
    supportedActions?: readonly MemberAction[];
    liveCached?: boolean; liveStale?: boolean; moderationAdminVerified?: boolean; moderationVerifiedAt?: number | null;
    moderationError?: LocalizedError | string | null; community?: boolean; auditRevision?: number;
    namer: (jid: string) => string; formatTime: (timestamp: number) => string;
    onaction: (scope: MemberScope, action: MemberAction) => Promise<ParticipantChange[] | void>;
    onsavelocal: (scope: MemberScope, notes: string, warnings: number) => Promise<void>;
    onloadAudit?: (scope: MemberScope, filters: AuditFilters, cursor: GroupAuditCursor | null) => Promise<AuditPage>;
    onmessage: (jid: string) => void; ongroup: (jid: string, messageId?: string) => void;
    onrefresh?: (scope: MemberScope) => void; onclose: () => void;
  } = $props();

  const actions = [["promote", "group.make_admin"], ["demote", "group.remove_admin"], ["remove", "group.remove_member"],
    ["block", "contact.block"], ["unblock", "contact.unblock"], ["report", "contact.report"]] as const;
  let dialog = $state<HTMLDialogElement>();
  let notes = $state("");
  let warnings = $state<number | undefined>(0);
  let notesLoaded = $state(false);
  let busy = $state(false);
  let failure = $state<LocalizedError | string>("");
  let refusedChanges = $state<ParticipantChange[]>([]);
  const refusal = $derived(refusedChanges.map(changeText).filter(Boolean).join(" "));
  let saved = $state("");
  let confirmation = $state<MemberAction | null>(null);
  let enlarged = $state(false);
  let now = $state(Date.now());
  let generation = 0;
  const ready = $derived(memberScopeMatches(dataScope, account, group, jid, requestKey));
  const local = $derived(ready && memberLocalMatches(incomingLocal, jid, group) ? incomingLocal : null);
  const member = $derived(ready && incomingMember && (incomingMember.jid === jid || local?.addresses.includes(incomingMember.jid)) && (incomingLocal === null || local !== null)
    && (memberSource === "live" || !local?.group || local.group.present) ? incomingMember : null);
  const role = $derived(member ?? (local?.group?.present ? local.group : null));
  const roleName = $derived(role?.owner === true ? t("group.owner") : role?.owner === false && role.admin !== null ? role.admin ? t("group.admin") : t("group.member") : null);
  const live = $derived(ready ? incomingLive : null);
  const picture = $derived(ready ? incomingPicture : null);
  const shown = $derived(displayName(member?.name ?? title, jid, local?.identity ?? undefined));
  const moderationFresh = $derived(moderationAdminVerified && memberFresh(moderationVerifiedAt, now));
  const liveOutdated = $derived(liveStale || (!!live && !memberFresh(live.fetched_at, now)));
  const permissions = $derived({ admin: admin && moderationFresh, connected, ready, self: local?.identity?.own ?? false, member: role, blocked, supported: supportedActions });
  const typing = $derived(connected && local?.signals.typing && local.signals.typing_at !== null
    ? memberTyping({ state: local.signals.typing, expires_at_ms: (local.signals.typing_at + 10) * 1000 }, now) : null);
  const auditKey = $derived(JSON.stringify([requestKey, auditRevision]));
  const noteError = $derived(memberNoteError(notes, warnings));

  $effect(() => {
    account; group; jid; requestKey;
    ++generation;
    busy = enlarged = notesLoaded = false; confirmation = null; failure = saved = ""; refusedChanges = [];
    untrack(() => { notes = local?.note.text ?? ""; warnings = local?.note.warnings ?? 0; notesLoaded = local !== null; });
    return () => { ++generation; };
  });
  $effect(() => { if (local && !notesLoaded) { notes = local.note.text; warnings = local.note.warnings; notesLoaded = true; } });
  $effect(() => { if (dialog && !dialog.open) dialog.showModal(); });
  $effect(() => { const timer = setInterval(() => { now = Date.now(); }, 1000); return () => clearInterval(timer); });

  function scope(): MemberScope | null { return account && group && jid ? { account, group, jid, requestKey } : null; }
  function current(owner: MemberScope, revision: number) {
    return revision === generation && owner.account === account && owner.group === group && owner.jid === jid && owner.requestKey === requestKey;
  }
  async function act(action: MemberAction) {
    const owner = scope();
    if (!owner || busy || localLoading || liveLoading || confirmation !== action || memberActionReason(action, permissions)) return;
    const revision = generation;
    busy = true; failure = saved = ""; refusedChanges = [];
    try {
      const result = await onaction(owner, action);
      if (!current(owner, revision)) return;
      refusedChanges = result ?? [];
      const refused = refusedChanges.map(changeText).filter(Boolean);
      if (!refused.length) { confirmation = null; saved = "contact.action_accepted"; }
    } catch (cause) { if (current(owner, revision)) failure = normalizeError(cause); }
    finally { if (current(owner, revision)) busy = false; }
  }
  async function saveLocal() {
    const owner = scope(), count = warnings;
    if (!owner || !local || busy || localLoading || count === undefined || memberNoteError(notes, count)) return;
    const revision = generation, draft = notes;
    busy = true; failure = saved = ""; refusedChanges = [];
    try { await onsavelocal(owner, draft, count); if (current(owner, revision)) saved = "contact.notes_saved"; }
    catch (cause) { if (current(owner, revision)) failure = normalizeError(cause); }
    finally { if (current(owner, revision)) busy = false; }
  }
  function loadAudit(owner: AuditScope, filters: AuditFilters, cursor: GroupAuditCursor | null): Promise<AuditPage> {
    if (!onloadAudit || !owner.member || owner.account !== account || owner.group !== group || owner.member !== jid || owner.requestKey !== auditKey) throw normalizeError({ kind: "postal_error", code: "error.member_audit_unavailable", params: {} });
    return onloadAudit({ account: owner.account, group: owner.group, jid: owner.member, requestKey }, filters, cursor);
  }
</script>

{#snippet fieldDetails(key: "photo" | "about" | "username" | "business_name" | "business" | "device_count")}
  {@const diagnostic = live?.field_failures?.[key]?.diagnostic ?? live?.[key].error}
  {#if diagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{diagnostic}</pre></details>{/if}
{/snippet}

<svelte:window onkeydowncapture={(event) => {
  if (enlarged && event.key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); enlarged = false; }
}} />

<dialog bind:this={dialog} aria-label={t("contact.member_info", { name: shown })} oncancel={(event) => { event.preventDefault(); event.stopPropagation(); onclose(); }}
  onkeydown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}>
  <header><div><h2><bdi>{shown}</bdi></h2><span class="muted">{groupName}</span></div><button class="close" aria-label={t("contact.member_close")} onclick={onclose}><Icon name="x" size={18} /></button></header>
  <div class="identity-head"><button class="photo" disabled={!picture} aria-label={t("contact.member_photo")} onclick={() => (enlarged = true)}>
    <Avatar src={picture} label={shown} seed={jid} cls="member-sheet-avatar" /></button>
    <div><p>{roleName ? member && memberSource === "live" ? roleName : t("contact.cached_role", { role: roleName }) : local?.group?.present === false ? t("contact.member_absent") : t("contact.member_role_unavailable")}</p>
      <p>{t("contact.group_tag")} {role?.label || t("ui.not_recorded")}</p><button onclick={() => onmessage(jid)} disabled={!account}>{t("chat.message")}</button></div></div>
  {#if !account}<p role="status">{t("contact.member_select_account")}</p>{/if}
  {#if localLoading}<p class="muted" role="status">{t("contact.member_loading")}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <section><h3>{t("contact.local_identity")}</h3><dl>
    <dt>{t("contact.saved_name")}</dt><dd dir="auto">{local?.identity?.saved_name ?? t("ui.not_recorded")}</dd>
    <dt>{t("contact.push_name")}</dt><dd dir="auto">{local?.identity?.push_name ?? t("ui.not_recorded")}</dd>
    <dt>{t("contact.phone")}</dt><dd dir="auto">{local?.identity?.number ? phoneLabel(local.identity.number) ?? `+${local.identity.number}` : t("ui.not_recorded")}</dd>
    <dt>{t("contact.username")}</dt><dd dir="auto">{local?.identity?.username ?? member?.username ?? t("ui.not_recorded")}</dd>
    <dt>{t("contact.phone_address")}</dt><dd dir="auto">{local?.pn_jid ?? t("ui.not_recorded")}</dd><dt>{t("contact.lid")}</dt><dd dir="auto">{local?.lid_jid ?? t("ui.not_recorded")}</dd>
    <dt>{t("contact.addresses")}</dt><dd dir="auto">{(local?.addresses.length ? local.addresses : [jid]).join(", ")}</dd>
  </dl></section>
  <section><h3>{local?.scope_chat ? t("contact.group_messages") : t("contact.local_messages")}</h3><dl>
    <dt>{t("contact.first_message")}</dt><dd dir="auto">{local?.stats.first_at != null ? formatTime(local.stats.first_at) : t("ui.not_recorded")}</dd>
    <dt>{t("contact.last_message")}</dt><dd dir="auto">{local?.stats.last_at != null ? formatTime(local.stats.last_at) : t("ui.not_recorded")}</dd>
    <dt>{t("chat.messages")}</dt><dd dir="auto">{local?.stats.total != null ? formatNumber(local?.stats.total) : t("ui.unavailable")}</dd><dt>{t("chat.media")}</dt><dd dir="auto">{local?.stats.media_total != null ? formatNumber(local?.stats.media_total) : t("ui.unavailable")}</dd>
    <dt>{t("contact.reactions_sent")}</dt><dd dir="auto">{local?.stats.reactions_sent != null ? formatNumber(local?.stats.reactions_sent) : t("ui.unavailable")}</dd><dt>{t("chat.mentions")}</dt><dd dir="auto">{local?.stats.times_mentioned != null ? formatNumber(local?.stats.times_mentioned) : t("ui.unavailable")}{#if local}<small>{t("contact.mention_contexts", { contexts: local.stats.mention_contexts_recorded, mentions: local.stats.group_mention_contexts_recorded })}</small>{/if}</dd>
  </dl></section>
  <section><h3>{t("contact.local_notes")}</h3><p class="muted">{t("contact.notes_hint")}</p>
    <label>{t("contact.notes")} <textarea dir="auto" bind:value={notes} disabled={!local || busy || localLoading} rows="3"></textarea></label>
    <label>{t("contact.warning_count")} <input type="number" min="0" max="100000" step="1" bind:value={warnings} disabled={!local || busy || localLoading} /></label>
    {#if noteError}<p class="error" role="alert">{noteError}</p>{/if}
    <button disabled={!account || !local || busy || localLoading || !!noteError} onclick={saveLocal}>{t("contact.notes_save")}</button>
  </section>
  <section><header><h3>{t("contact.live_info")}</h3>{#if onrefresh}<button disabled={!connected || busy || liveLoading} onclick={() => { const owner = scope(); if (owner) onrefresh?.(owner); }}>{t("contact.live_refresh")}</button>{/if}</header>
    {#if !connected}<p class="muted" role="status">{t("contact.live_offline")}</p>{/if}
    {#if liveLoading}<p class="muted" role="status">{t("contact.live_loading")}</p>{/if}
    {#if liveError}<p class="error" role="alert">{liveError}</p>{#if liveDiagnostic}<details><summary>{t("error.technical_details")}</summary><pre dir="auto">{liveDiagnostic}</pre></details>{/if}{/if}
    {#if liveCached || liveOutdated}<p class="muted">{liveCached ? t("contact.live_cached") : ""} {liveOutdated ? t("contact.live_outdated") : ""}</p>{/if}
    {#if live?.fetched_at}<p class="muted">{t("contact.last_fetched", { time: formatTime(live.fetched_at) })}</p>{/if}
    <dl><dt>{t("contact.photo")}</dt><dd dir="auto">{memberFieldText(live?.photo ?? null, () => t("contact.photo_available"), live?.field_failures?.photo)}{@render fieldDetails("photo")}</dd>
      <dt>{t("contact.about")}</dt><dd class="about">{memberFieldText(live?.about ?? null, undefined, live?.field_failures?.about)}{@render fieldDetails("about")}</dd>
      <dt>{t("contact.live_username")}</dt><dd dir="auto">{memberFieldText(live?.username ?? null, undefined, live?.field_failures?.username)}{@render fieldDetails("username")}</dd>
      <dt>{t("contact.business_verified")}</dt><dd dir="auto">{memberFieldText(live?.business_name ?? null, undefined, live?.field_failures?.business_name)}{@render fieldDetails("business_name")}</dd>
      <dt>{t("contact.business_profile")}</dt><dd dir="auto">{memberFieldText(live?.business ?? null, () => live?.business.value?.name || t("ui.available"), live?.field_failures?.business)}{@render fieldDetails("business")}</dd>
      <dt>{t("contact.device_count")}</dt><dd dir="auto">{memberFieldText(live?.device_count ?? null, undefined, live?.field_failures?.device_count)}{@render fieldDetails("device_count")}</dd>
      <dt>{t("contact.presence")}</dt><dd dir="auto">{local?.signals.online != null ? local.signals.online ? t("contact.online_last") : t("contact.offline_last") : t("ui.not_recorded")}
        {#if local?.signals.last_seen != null}<small>{t("contact.last_seen", { time: formatTime(local.signals.last_seen) })}</small>{/if}
        {#if local?.signals.presence_at != null}<small>{t("contact.observed", { time: formatTime(local.signals.presence_at) })}</small>{/if}</dd>
      <dt>{t("contact.typing")}</dt><dd dir="auto">{typing ?? t("contact.typing_empty")}</dd></dl>
    {#if live?.business.value && (live.business.state === "available" || live.business.state === "error" && live.business.stale)}
      {@const business = live.business.value}
      <dl class="business"><dt>{t("contact.description")}</dt><dd dir="auto">{business.description || t("ui.not_provided")}</dd><dt>{t("contact.address")}</dt><dd dir="auto">{business.address ?? t("ui.not_provided")}</dd>
        <dt>{t("contact.email")}</dt><dd dir="auto">{business.email ?? t("ui.not_provided")}</dd><dt>{t("contact.websites")}</dt><dd dir="auto">{business.websites.join(", ") || t("ui.not_provided")}</dd>
        <dt>{t("contact.categories")}</dt><dd dir="auto">{business.categories.join(", ") || t("ui.not_provided")}</dd><dt>{t("contact.time_zone")}</dt><dd dir="auto">{business.timezone ?? t("ui.not_provided")}</dd>
        <dt>{t("contact.hours")}</dt><dd dir="auto">{business.hours === null ? t("ui.not_provided") : business.hours.length === 0 ? t("contact.no_hours") : business.hours.map(memberBusinessHours).join("; ")}</dd></dl>
    {/if}
  </section>
  <section><h3>{t("contact.group_join_local")}</h3><dl><dt>{t("contact.joined")}</dt><dd dir="auto">{local?.join ? formatTime(local.join.timestamp) : t("ui.not_recorded")}</dd>
    <dt>{t("contact.actor")}</dt><dd dir="auto">{local?.join?.actor ? namer(local.join.actor) : t("ui.not_recorded")}</dd>
    <dt>{t("contact.join_method")}</dt><dd dir="auto">{local?.join?.kind ?? t("ui.not_recorded")}</dd></dl></section>
  <section><h3>{t("contact.mutual_cached")}</h3>{#if local?.mutual_groups == null}<p class="muted">{t("contact.cache_unavailable")}</p>
    {:else if local.mutual_groups.length === 0}<p class="muted">{t("contact.mutual_empty")}</p>
    {:else}<ul>{#each local.mutual_groups as cached (cached.chat)}<li><button onclick={() => ongroup(cached.chat)}>{cached.subject ?? cached.chat}</button><small>{t("contact.observed", { time: formatTime(cached.observed_at) })}</small></li>{/each}</ul>{/if}</section>
  <section><h3>{t("contact.member_actions")}</h3>{#if !admin || !moderationFresh}<p class="muted">{t("contact.admin_verification_required")}</p>{/if}
    {#if moderationVerifiedAt !== null}<p class="muted">{t("contact.admin_verification", { time: formatTime(moderationVerifiedAt) })}</p>{/if}
    {#if moderationError}<p class="error" role="alert">{moderationError}</p>{/if}
    {#if ready && admin && moderationFresh}<div class="actions">{#each actions as [action, label]}
      {@const reason = memberActionReason(action, permissions)}
      <button disabled={busy || localLoading || liveLoading || !!reason} title={reason ?? undefined} onclick={() => { confirmation = action; failure = saved = ""; refusedChanges = []; }}>{t(label)}</button>
    {/each}</div>{/if}
    {#if ready && confirmation && admin && moderationFresh}<div class="confirmation" aria-label={t("contact.member_confirm_title")}><p>{t("contact.member_confirm_action", { action: t(actions.find(([action]) => action === confirmation)?.[1] ?? "contact.member_actions") })} <bdi>{shown}</bdi> {t("contact.member_confirm_group")} <bdi>{groupName}</bdi>{t("ui.question_mark")}</p>
      {#if community && confirmation === "remove"}<p class="error">{t("contact.community_remove_hint")}</p>{/if}
      <button disabled={busy || localLoading || liveLoading || !!memberActionReason(confirmation, permissions)} onclick={() => act(confirmation!)}>{t("ui.confirm_action")}</button>
      <button disabled={busy} onclick={() => (confirmation = null)}>{t("ui.cancel_action")}</button></div>{/if}
    {#if busy}<p class="muted" role="status">{t("ui.updating")}</p>{/if}{#if failure || refusal}<p class="error" role="alert">{failure || refusal}</p>{/if}{#if saved}<p class="muted" role="status">{t(saved)}</p>{/if}
  </section>
  <section>{#if onloadAudit}<GroupAudit {account} {group} requestKey={auditKey} member={jid} {namer} {formatTime} title={t("contact.member_audit")}
    onload={loadAudit}
    onjump={(group, messageId) => ongroup(group, messageId)} />{:else}<h3>{t("contact.member_audit")}</h3><p class="muted">{t("group.audit_unavailable")}</p>{/if}</section>
  {#if enlarged && picture}<Lightbox {jid} preview={picture} alt={shown} onclose={() => (enlarged = false)} />{/if}
</dialog>

<style>
  dialog { width: min(760px, calc(100vw - 32px)); max-height: calc(100vh - 32px); box-sizing: border-box; overflow: auto; padding: 24px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h2 { margin: 0 0 5px; font-size: 1.25rem; overflow-wrap: anywhere; } h3 { margin: 0 0 12px; font-size: 0.9375rem; }
  .close { border: 0; background: transparent; padding: 5px; display: grid; place-items: center; }
  .identity-head { display: flex; align-items: center; gap: 18px; margin: 20px 0; }
  .photo { padding: 0; border: 0; border-radius: 50%; background: transparent; }
  :global(.member-sheet-avatar) { width: 72px; height: 72px; border-radius: 50%; display: grid; place-items: center; object-fit: cover; background: hsl(var(--hue, 0) 25% 35%); color: white; font-size: 1.5625rem; }
  section { padding-top: 18px; margin-top: 18px; border-top: 1px solid var(--line); }
  dl { display: grid; grid-template-columns: minmax(110px, 0.35fr) minmax(0, 1fr); gap: 8px 14px; margin: 0; font-size: 0.8125rem; }
  dt { color: var(--muted); } dd { margin: 0; overflow-wrap: anywhere; } .about { white-space: pre-wrap; }
  label { display: grid; gap: 6px; margin-bottom: 10px; font-size: 0.8125rem; }
  input, textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 8px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg); color: inherit; font: inherit; }
  textarea { resize: vertical; } button { padding: 6px 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--raised-2); color: inherit; font: inherit; font-size: 0.75rem; cursor: pointer; }
  button:disabled, input:disabled, textarea:disabled { opacity: 0.55; cursor: default; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; } .confirmation { margin-top: 14px; padding: 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); }
  .confirmation button + button { margin-inline-start: 8px; } .muted { color: var(--muted); font-size: 0.75rem; } .error { color: var(--danger); overflow-wrap: anywhere; font-size: 0.8125rem; }
  ul { padding-inline-start: 20px; } li { margin-bottom: 6px; }
  small { display: block; color: var(--muted); font-size: 0.6875rem; margin-top: 5px; }
  .business { margin-top: 14px; }
  @media (max-width: 480px) { dialog { padding: 16px; } dl { grid-template-columns: 1fr; gap: 4px; } dd { margin-bottom: 8px; } }
  details pre { max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
