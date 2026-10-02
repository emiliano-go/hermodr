<script lang="ts">
  import MemberSheet from "../../src/lib/contacts/MemberSheet.svelte";
  import GroupAudit from "../../src/lib/chat/GroupAudit.svelte";
  import type { GroupAuditCursor, Participant } from "../../src/lib/utils/wire";
  import type { MemberAction, MemberLocalView, MemberLiveView, MemberScope } from "../../src/lib/utils/member-sheet";
  import type { AuditEntry, AuditFilters, AuditPage, AuditScope } from "../../src/lib/utils/group-audit";

  let account = $state("alpha"), group = $state("test@g.us"), jid = $state("200@lid"), requestKey = $state(0);
  let open = $state(false), connected = $state(true), admin = $state(true), defer = $state(false), fail = $state(false), missing = $state(false);
  let calls = $state<string[]>([]), waiting = $state(0);
  let dataScope = $state<MemberScope | null>({ account: "alpha", group: "test@g.us", jid: "200@lid", requestKey: 0 });
  const releases: (() => void)[] = [];
  let member = $state<Participant>({ jid: "200@lid", name: "Push member", admin: false, owner: false, number: "59899000000", username: "member", label: "Engineer" });
  let local = $state<MemberLocalView>({ jid: "200@lid", identity: { contact_saved: true, saved_name: "Saved member", legacy_name: null, push_name: "Push member", username: "member",
    number: "59899000000", own: false }, addresses: ["200@lid", "59899000000@s.whatsapp.net"], pn_jid: "59899000000@s.whatsapp.net", lid_jid: "200@lid", scope_chat: "test@g.us",
    stats: { total: 8, first_at: 1000, last_at: 2000, media_total: 2, reactions_sent: 3, times_mentioned: 1, mention_contexts_recorded: 8, group_mention_contexts_recorded: 8 },
    note: { text: "Synthetic note", warnings: 2, updated_at: 2000 }, group: { chat: "test@g.us", subject: "Test group", observed_at: 2000, present: true, admin: false, owner: false, label: "Engineer", own_admin: true, member_observed_at: 2000, complete_snapshot: true },
    join: { timestamp: 900, actor: "100@lid", kind: "GROUP_PARTICIPANT_ADD", message_id: "join-message" },
    mutual_groups: [{ chat: "cached@g.us", subject: "Cached group", observed_at: 2000, present: true, admin: false, owner: false, label: null, own_admin: false, member_observed_at: 2000, complete_snapshot: true }],
    signals: { online: null, last_seen: null, presence_at: null, typing: null, typing_at: null } });
  let moderationVerifiedAt = $state(Math.floor(Date.now() / 1000));
  let live = $state<MemberLiveView>({ fetched_at: Math.floor(Date.now() / 1000), photo_id: null,
    about: { state: "restricted", value: null, error: "Access denied by server.", stale: false },
    username: { state: "available", value: "member", error: null, stale: false },
    photo: { state: "unavailable", value: null, error: null, stale: false },
    business_name: { state: "unavailable", value: null, error: null, stale: false },
    business: { state: "unavailable", value: null, error: null, stale: false },
    device_count: { state: "error", value: 2, error: "Synthetic device refresh failure", stale: true } });
  const entries: AuditEntry[] = [
    { id: 2, chat: "test@g.us", kind: "join", timestamp: 2000, observed_at: 2000, source: "notification", actor: "100@lid", target: "200@lid", old_value: null,
      new_value: "present", old_source: null, message_id: "add-message", jump_available: true },
    { id: 1, chat: "test@g.us", kind: "leave", timestamp: null, observed_at: 1000, source: "history", actor: "200@lid", target: "200@lid", old_value: "present",
      new_value: "absent", old_source: "cached", message_id: "leave-message", jump_available: false },
  ];
  async function waitAction(kind: string, args: unknown[]) {
    calls = [...calls, `${kind} ${JSON.stringify(args)}`];
    const refused = fail;
    if (defer) await new Promise<void>((resolve) => { releases.push(resolve); waiting = releases.length; });
    if (refused) throw new Error("Synthetic member/audit failure");
  }
  async function action(scope: MemberScope, action: MemberAction) {
    await waitAction("member action", [scope, action]);
    if (scope.account !== account || scope.group !== group || scope.jid !== jid || scope.requestKey !== requestKey) return;
    if (action === "promote") member = { ...member, admin: true };
    if (action === "demote") member = { ...member, admin: false };
  }
  async function notes(scope: MemberScope, notes: string, warnings: number) {
    await waitAction("notes", [scope, notes, warnings]);
    if (scope.account === account && scope.group === group && scope.jid === jid && scope.requestKey === requestKey) local = { ...local, note: { ...local.note, text: notes, warnings } };
  }
  async function audit(scope: AuditScope | MemberScope, filters: AuditFilters, cursor: GroupAuditCursor | null): Promise<AuditPage> {
    await waitAction("audit", [scope, filters, cursor]);
    const subject = "member" in scope ? scope.member : scope.jid;
    const matched = entries.filter((entry) => entry.chat === scope.group && (!filters.kind || entry.kind === filters.kind) && (!filters.actor || entry.actor === filters.actor)
      && (!subject || entry.actor === subject || entry.target === subject)
      && (filters.since === null || (entry.timestamp ?? entry.observed_at) >= filters.since) && (filters.until === null || (entry.timestamp ?? entry.observed_at) <= filters.until));
    const offset = cursor ? matched.findIndex((entry) => entry.id === cursor.id) + 1 : 0;
    const page = matched.slice(offset, offset + 1), has_more = offset + 1 < matched.length;
    return { entries: page, has_more, next_cursor: has_more ? { timestamp: page[0].timestamp ?? page[0].observed_at, id: page[0].id } : null };
  }
  function release() { const next = releases.shift(); waiting = releases.length; next?.(); }
  function scopeKey(event: KeyboardEvent) {
    if (!event.ctrlKey || !event.altKey || !["a", "g", "m", "q", "r"].includes(event.key.toLowerCase())) return;
    event.preventDefault();
    const key = event.key.toLowerCase();
    if (key === "a") account = account === "alpha" ? "beta" : "alpha";
    else if (key === "g") group = group === "test@g.us" ? "other@g.us" : "test@g.us";
    else if (key === "m") jid = jid === "200@lid" ? "300@lid" : "200@lid";
    else if (key === "q") requestKey++;
    else release();
  }
  const namer = (id: string) => id === "100@lid" ? "Admin" : id === "200@lid" ? "Saved member" : id;
  const formatTime = (timestamp: number) => new Date(timestamp * 1000).toLocaleString();
</script>

<svelte:window onkeydown={scopeKey} />
<h1>Synthetic member sheet and group audit</h1>
<p>Ctrl+Alt+A account, G group, M member, Q request, R release pending request.</p>
<div class="controls"><button onclick={() => (open = true)}>Open member sheet</button>
  <label><input type="checkbox" bind:checked={connected} /> Connected</label><label><input type="checkbox" bind:checked={admin} /> Admin</label>
  <label><input type="checkbox" bind:checked={missing} /> Missing metadata</label><label><input type="checkbox" bind:checked={defer} /> Defer requests</label>
  <label><input type="checkbox" bind:checked={fail} /> Fail requests</label><button onclick={release} disabled={!waiting}>Release requests {waiting}</button></div>
<button onclick={() => { dataScope = { account, group, jid, requestKey }; local = { ...local, jid, scope_chat: group, addresses: [jid] }; member = { ...member, jid }; moderationVerifiedAt = Math.floor(Date.now() / 1000); live = { ...live, fetched_at: moderationVerifiedAt }; }}>Supply current snapshot</button>
<p>Scope {account} / {group} / {jid} / {requestKey}</p><pre aria-label="Synthetic member operations">{calls.join("\n")}</pre>
<GroupAudit {account} {group} {requestKey} onload={audit} {namer} {formatTime} onjump={(group, id) => (calls = [...calls, `jump ${group} ${id}`])} />
{#if open}<MemberSheet {account} {group} groupName="Test group" {jid} {requestKey} {dataScope} title="Synthetic member" {connected}
  local={missing ? null : local} member={missing ? null : member} live={missing ? null : live} {admin} moderationAdminVerified={admin} {moderationVerifiedAt} blocked={false}
  supportedActions={["promote", "demote", "remove", "block", "unblock", "report"]} {namer} {formatTime}
  onaction={action} onsavelocal={notes} onloadAudit={audit} onmessage={(jid) => (calls = [...calls, `message ${jid}`])}
  ongroup={(group, id) => (calls = [...calls, `group ${group} ${id ?? ""}`])}
  onrefresh={(scope) => { calls = [...calls, `refresh ${JSON.stringify(scope)}`]; moderationVerifiedAt = Math.floor(Date.now() / 1000); live = { ...live, fetched_at: moderationVerifiedAt }; }} onclose={() => (open = false)} />{/if}

<style>
  :global(body) { font-family: sans-serif; max-width: 900px; margin: 20px auto; --bg: #fff; --surface: #f3f4f5; --raised-2: #e7ebee; --text: #222; --muted: #555;
    --line: #ddd; --line-strong: #aaa; --radius-sm: 5px; --radius-lg: 12px; --accent: #067a68; --scrim: #0008; --danger: #a00; --shadow: 0 5px 25px #0003; }
  .controls { display: flex; flex-wrap: wrap; gap: 8px; } pre { white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
