<script lang="ts">
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
    live: incomingLive = null, picture: incomingPicture = null, localLoading = false, liveLoading = false, error = "", liveError = "", admin = false,
    blocked = null, supportedActions = [], liveCached = false, liveStale = false, moderationAdminVerified = false,
    moderationVerifiedAt = null, moderationError = "", community = false, auditRevision = 0, namer, formatTime, onaction, onsavelocal, onloadAudit,
    onmessage, ongroup, onrefresh, onclose }: {
    account: string | null; group: string; groupName: string; jid: string; requestKey: string | number; title: string;
    dataScope: MemberScope | null;
    connected: boolean; local?: MemberLocalView | null; member?: Participant | null; memberSource?: "cached" | "live";
    live?: MemberLiveView | null; picture?: string | null; localLoading?: boolean; liveLoading?: boolean;
    error?: string | null; liveError?: string | null; admin?: boolean; blocked?: boolean | null;
    supportedActions?: readonly MemberAction[];
    liveCached?: boolean; liveStale?: boolean; moderationAdminVerified?: boolean; moderationVerifiedAt?: number | null;
    moderationError?: string | null; community?: boolean; auditRevision?: number;
    namer: (jid: string) => string; formatTime: (timestamp: number) => string;
    onaction: (scope: MemberScope, action: MemberAction) => Promise<ParticipantChange[] | void>;
    onsavelocal: (scope: MemberScope, notes: string, warnings: number) => Promise<void>;
    onloadAudit?: (scope: MemberScope, filters: AuditFilters, cursor: GroupAuditCursor | null) => Promise<AuditPage>;
    onmessage: (jid: string) => void; ongroup: (jid: string, messageId?: string) => void;
    onrefresh?: (scope: MemberScope) => void; onclose: () => void;
  } = $props();

  const actions = [["promote", "Make admin"], ["demote", "Remove admin role"], ["remove", "Remove from group"],
    ["block", "Block"], ["unblock", "Unblock"], ["report", "Report"]] as const;
  let dialog = $state<HTMLDialogElement>();
  let notes = $state("");
  let warnings = $state<number | undefined>(0);
  let notesLoaded = $state(false);
  let busy = $state(false);
  let failure = $state("");
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
  const roleName = $derived(role?.owner === true ? "Group owner" : role?.owner === false && role.admin !== null ? role.admin ? "Admin" : "Member" : null);
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
    busy = enlarged = notesLoaded = false; confirmation = null; failure = saved = "";
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
    busy = true; failure = saved = "";
    try {
      const result = await onaction(owner, action);
      if (!current(owner, revision)) return;
      const refused = result?.map(changeText).filter(Boolean) ?? [];
      if (refused.length) failure = refused.join(" "); else { confirmation = null; saved = "Action request accepted."; }
    } catch (cause) { if (current(owner, revision)) failure = String(cause); }
    finally { if (current(owner, revision)) busy = false; }
  }
  async function saveLocal() {
    const owner = scope(), count = warnings;
    if (!owner || !local || busy || localLoading || count === undefined || memberNoteError(notes, count)) return;
    const revision = generation, draft = notes;
    busy = true; failure = saved = "";
    try { await onsavelocal(owner, draft, count); if (current(owner, revision)) saved = "Notes and warning count saved on this device."; }
    catch (cause) { if (current(owner, revision)) failure = String(cause); }
    finally { if (current(owner, revision)) busy = false; }
  }
  function loadAudit(owner: AuditScope, filters: AuditFilters, cursor: GroupAuditCursor | null): Promise<AuditPage> {
    if (!onloadAudit || !owner.member || owner.account !== account || owner.group !== group || owner.member !== jid || owner.requestKey !== auditKey) throw new Error("Member audit is unavailable.");
    return onloadAudit({ account: owner.account, group: owner.group, jid: owner.member, requestKey }, filters, cursor);
  }
</script>

<svelte:window onkeydowncapture={(event) => {
  if (enlarged && event.key === "Escape") { event.preventDefault(); event.stopImmediatePropagation(); enlarged = false; }
}} />

<dialog bind:this={dialog} aria-label={`Member info: ${shown}`} oncancel={(event) => { event.preventDefault(); event.stopPropagation(); onclose(); }}
  onkeydown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}>
  <header><div><h2>{shown}</h2><span class="muted">{groupName}</span></div><button class="close" aria-label="Close member info" onclick={onclose}><Icon name="x" size={18} /></button></header>
  <div class="identity-head"><button class="photo" disabled={!picture} aria-label="View member photo" onclick={() => (enlarged = true)}>
    <Avatar src={picture} label={shown} seed={jid} cls="member-sheet-avatar" /></button>
    <div><p>{roleName ? `${member && memberSource === "live" ? "" : "Cached "}${roleName}` : local?.group?.present === false ? "Absent at last cached group update" : "Group role unavailable"}</p>
      <p>Group tag: {role?.label || "Not recorded"}</p><button onclick={() => onmessage(jid)} disabled={!account}>Message</button></div></div>
  {#if !account}<p role="status">Select an account to read member info.</p>{/if}
  {#if localLoading}<p class="muted" role="status">Loading locally stored member info…</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  <section><h3>Identity stored on this device</h3><dl>
    <dt>Saved name</dt><dd>{local?.identity?.saved_name ?? "Not recorded"}</dd>
    <dt>Push name</dt><dd>{local?.identity?.push_name ?? "Not recorded"}</dd>
    <dt>Phone</dt><dd>{local?.identity?.number ? phoneLabel(local.identity.number) ?? `+${local.identity.number}` : "Not recorded"}</dd>
    <dt>Username</dt><dd>{local?.identity?.username ?? member?.username ?? "Not recorded"}</dd>
    <dt>Phone address</dt><dd>{local?.pn_jid ?? "Not recorded"}</dd><dt>LID</dt><dd>{local?.lid_jid ?? "Not recorded"}</dd>
    <dt>Addresses</dt><dd>{(local?.addresses.length ? local.addresses : [jid]).join(", ")}</dd>
  </dl></section>
  <section><h3>{local?.scope_chat ? "Messages stored for this group" : "Messages stored on this device"}</h3><dl>
    <dt>First message</dt><dd>{local?.stats.first_at != null ? formatTime(local.stats.first_at) : "Not recorded"}</dd>
    <dt>Last message</dt><dd>{local?.stats.last_at != null ? formatTime(local.stats.last_at) : "Not recorded"}</dd>
    <dt>Messages</dt><dd>{local?.stats.total ?? "Unavailable"}</dd><dt>Media</dt><dd>{local?.stats.media_total ?? "Unavailable"}</dd>
    <dt>Reactions sent</dt><dd>{local?.stats.reactions_sent ?? "Unavailable"}</dd><dt>Mentions</dt><dd>{local?.stats.times_mentioned ?? "Unavailable"}{#if local}<small>Across {local.stats.mention_contexts_recorded} locally recorded mention contexts; {local.stats.group_mention_contexts_recorded} in this group.</small>{/if}</dd>
  </dl></section>
  <section><h3>Local notes</h3><p class="muted">Notes and warning count stay on this device. Up to 4096 characters; warning count 0–100000.</p>
    <label>Notes <textarea bind:value={notes} disabled={!local || busy || localLoading} rows="3"></textarea></label>
    <label>Warning count <input type="number" min="0" max="100000" step="1" bind:value={warnings} disabled={!local || busy || localLoading} /></label>
    {#if noteError}<p class="error" role="alert">{noteError}</p>{/if}
    <button disabled={!account || !local || busy || localLoading || !!noteError} onclick={saveLocal}>Save local notes</button>
  </section>
  <section><header><h3>Live information</h3>{#if onrefresh}<button disabled={!connected || busy || liveLoading} onclick={() => { const owner = scope(); if (owner) onrefresh?.(owner); }}>Refresh live info</button>{/if}</header>
    {#if !connected}<p class="muted" role="status">Offline. Previously fetched fields may be outdated.</p>{/if}
    {#if liveLoading}<p class="muted" role="status">Fetching live info…</p>{/if}
    {#if liveError}<p class="error" role="alert">{liveError}</p>{/if}
    {#if liveCached || liveOutdated}<p class="muted">{liveCached ? "Cached live fields." : ""} {liveOutdated ? "These fields may be outdated." : ""}</p>{/if}
    {#if live?.fetched_at}<p class="muted">Last fetched {formatTime(live.fetched_at)}</p>{/if}
    <dl><dt>Photo</dt><dd>{memberFieldText(live?.photo ?? null, () => "Photo available")}</dd>
      <dt>About</dt><dd class="about">{memberFieldText(live?.about ?? null)}</dd>
      <dt>Live username</dt><dd>{memberFieldText(live?.username ?? null)}</dd>
      <dt>Verified business name</dt><dd>{memberFieldText(live?.business_name ?? null)}</dd>
      <dt>Business profile</dt><dd>{memberFieldText(live?.business ?? null, () => live?.business.value?.name || "Available")}</dd>
      <dt>Device count</dt><dd>{memberFieldText(live?.device_count ?? null)}</dd>
      <dt>Presence</dt><dd>{local?.signals.online != null ? local.signals.online ? "Online at last update" : "Offline at last update" : "Not recorded"}
        {#if local?.signals.last_seen != null}<small>Last seen {formatTime(local.signals.last_seen)}</small>{/if}
        {#if local?.signals.presence_at != null}<small>Observed {formatTime(local.signals.presence_at)}</small>{/if}</dd>
      <dt>Typing</dt><dd>{typing ?? "No current typing update"}</dd></dl>
    {#if live?.business.value && (live.business.state === "available" || live.business.state === "error" && live.business.stale)}
      {@const business = live.business.value}
      <dl class="business"><dt>Description</dt><dd>{business.description || "Not provided"}</dd><dt>Address</dt><dd>{business.address ?? "Not provided"}</dd>
        <dt>Email</dt><dd>{business.email ?? "Not provided"}</dd><dt>Websites</dt><dd>{business.websites.join(", ") || "Not provided"}</dd>
        <dt>Categories</dt><dd>{business.categories.join(", ") || "Not provided"}</dd><dt>Time zone</dt><dd>{business.timezone ?? "Not provided"}</dd>
        <dt>Hours</dt><dd>{business.hours === null ? "Not provided" : business.hours.length === 0 ? "No hours provided" : business.hours.map(memberBusinessHours).join("; ")}</dd></dl>
    {/if}
  </section>
  <section><h3>Group join recorded locally</h3><dl><dt>Joined</dt><dd>{local?.join ? formatTime(local.join.timestamp) : "Not recorded"}</dd>
    <dt>Actor</dt><dd>{local?.join?.actor ? namer(local.join.actor) : "Not recorded"}</dd>
    <dt>Join method</dt><dd>{local?.join?.kind ?? "Not recorded"}</dd></dl></section>
  <section><h3>Cached mutual groups</h3>{#if local?.mutual_groups == null}<p class="muted">Unavailable in the local cache.</p>
    {:else if local.mutual_groups.length === 0}<p class="muted">No mutual groups recorded in the local cache.</p>
    {:else}<ul>{#each local.mutual_groups as cached (cached.chat)}<li><button onclick={() => ongroup(cached.chat)}>{cached.subject ?? cached.chat}</button><small>Observed {formatTime(cached.observed_at)}</small></li>{/each}</ul>{/if}</section>
  <section><h3>Member actions</h3>{#if !admin || !moderationFresh}<p class="muted">Live group admin verification is required for member actions.</p>{/if}
    {#if moderationVerifiedAt !== null}<p class="muted">Admin verification {formatTime(moderationVerifiedAt)}</p>{/if}
    {#if moderationError}<p class="error" role="alert">{moderationError}</p>{/if}
    {#if ready && admin && moderationFresh}<div class="actions">{#each actions as [action, label]}
      {@const reason = memberActionReason(action, permissions)}
      <button disabled={busy || localLoading || liveLoading || !!reason} title={reason ?? undefined} onclick={() => { confirmation = action; failure = saved = ""; }}>{label}</button>
    {/each}</div>{/if}
    {#if ready && confirmation && admin && moderationFresh}<div class="confirmation" aria-label="Confirm member action"><p>{actions.find(([action]) => action === confirmation)?.[1]} for {shown} in {groupName}?</p>
      {#if community && confirmation === "remove"}<p class="error">This removes the member from the community and its linked groups.</p>{/if}
      <button disabled={busy || localLoading || liveLoading || !!memberActionReason(confirmation, permissions)} onclick={() => act(confirmation!)}>Confirm action</button>
      <button disabled={busy} onclick={() => (confirmation = null)}>Cancel action</button></div>{/if}
    {#if busy}<p class="muted" role="status">Updating…</p>{/if}{#if failure}<p class="error" role="alert">{failure}</p>{/if}{#if saved}<p class="muted" role="status">{saved}</p>{/if}
  </section>
  <section>{#if onloadAudit}<GroupAudit {account} {group} requestKey={auditKey} member={jid} {namer} {formatTime} title="Member audit timeline"
    onload={loadAudit}
    onjump={(group, messageId) => ongroup(group, messageId)} />{:else}<h3>Member audit timeline</h3><p class="muted">Group audit is unavailable.</p>{/if}</section>
  {#if enlarged && picture}<Lightbox {jid} preview={picture} alt={shown} onclose={() => (enlarged = false)} />{/if}
</dialog>

<style>
  dialog { width: min(760px, calc(100vw - 32px)); max-height: calc(100vh - 32px); box-sizing: border-box; overflow: auto; padding: 24px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); }
  dialog::backdrop { background: var(--scrim); }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
  h2 { margin: 0 0 5px; font-size: 20px; overflow-wrap: anywhere; } h3 { margin: 0 0 12px; font-size: 15px; }
  .close { border: 0; background: transparent; padding: 5px; display: grid; place-items: center; }
  .identity-head { display: flex; align-items: center; gap: 18px; margin: 20px 0; }
  .photo { padding: 0; border: 0; border-radius: 50%; background: transparent; }
  :global(.member-sheet-avatar) { width: 72px; height: 72px; border-radius: 50%; display: grid; place-items: center; object-fit: cover; background: hsl(var(--hue, 0) 25% 35%); color: white; font-size: 25px; }
  section { padding-top: 18px; margin-top: 18px; border-top: 1px solid var(--line); }
  dl { display: grid; grid-template-columns: minmax(110px, 0.35fr) minmax(0, 1fr); gap: 8px 14px; margin: 0; font-size: 13px; }
  dt { color: var(--muted); } dd { margin: 0; overflow-wrap: anywhere; } .about { white-space: pre-wrap; }
  label { display: grid; gap: 6px; margin-bottom: 10px; font-size: 13px; }
  input, textarea { box-sizing: border-box; width: 100%; min-width: 0; padding: 8px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--bg); color: inherit; font: inherit; }
  textarea { resize: vertical; } button { padding: 6px 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); background: var(--raised-2); color: inherit; font: inherit; font-size: 12px; cursor: pointer; }
  button:disabled, input:disabled, textarea:disabled { opacity: 0.55; cursor: default; }
  .actions { display: flex; flex-wrap: wrap; gap: 8px; } .confirmation { margin-top: 14px; padding: 10px; border: 1px solid var(--line-strong); border-radius: var(--radius-sm); }
  .confirmation button + button { margin-left: 8px; } .muted { color: var(--muted); font-size: 12px; } .error { color: var(--danger); overflow-wrap: anywhere; font-size: 13px; }
  ul { padding-left: 20px; } li { margin-bottom: 6px; }
  small { display: block; color: var(--muted); font-size: 11px; margin-top: 5px; }
  .business { margin-top: 14px; }
  @media (max-width: 480px) { dialog { padding: 16px; } dl { grid-template-columns: 1fr; gap: 4px; } dd { margin-bottom: 8px; } }
</style>
