import { invoke } from "$lib/utils/ipc";
import { memberActionReason, type MemberAction, type MemberScope } from "$lib/utils/member-sheet";
import type { AuditFilters, AuditPage } from "$lib/utils/group-audit";
import type { BlockedContact, GroupAuditCursor, MemberNote, MemberProfile, Participant, ParticipantChange } from "$lib/utils/wire";
import { session } from "./session.svelte";
import { chats } from "./chats.svelte";
import { messages } from "./messages.svelte";
import { composer } from "./composer.svelte";

type SheetScope = MemberScope & { generation: number; title: string };
const commands = { promote: "promote_group_participants", demote: "demote_group_participants", remove: "remove_group_participants" } as const;

export class MemberSheetState {
  scope = $state.raw<SheetScope | null>(null);
  profile = $state.raw<MemberProfile | null>(null);
  localLoading = $state(false);
  liveLoading = $state(false);
  error = $state("");
  liveError = $state("");
  blocked = $state<boolean | null>(null);
  private serial = 0;
  private request = 0;
  private presenceRequested = false;
  private signalsRevision = 0;
  private refreshTimer: ReturnType<typeof setTimeout> | undefined;

  open(group: string, jid: string, title: string) {
    const account = session.activeAccount;
    if (!account || !group.endsWith("@g.us")) return;
    this.close();
    this.scope = { account, group, jid, title, requestKey: ++this.serial, generation: messages.accountGeneration };
    const scope = this.scope;
    void this.load(false).then(() => { if (this.scope === scope && session.connected) void this.load(true); });
  }

  close() {
    ++this.request;
    this.scope = this.profile = null;
    this.localLoading = this.liveLoading = false;
    this.error = this.liveError = "";
    this.blocked = null;
    this.presenceRequested = false;
    clearTimeout(this.refreshTimer);
    this.refreshTimer = undefined;
  }

  current(scope: MemberScope, signal?: AbortSignal) {
    const owner = this.scope;
    if (signal?.aborted || !owner || owner.account !== scope.account || owner.group !== scope.group || owner.jid !== scope.jid
      || owner.requestKey !== scope.requestKey || session.activeAccount !== owner.account
      || messages.accountGeneration !== owner.generation || chats.selectedChat !== owner.group) {
      throw new Error("account, group or member changed during operation");
    }
  }

  member(): Participant | null {
    const local = this.profile?.local, group = local?.group;
    if (!local || !group?.present || group.admin === null || group.owner === null) return null;
    return { jid: local.jid, name: local.identity.saved_name ?? local.identity.push_name ?? local.jid,
      number: local.identity.number, username: local.identity.username, admin: group.admin, owner: group.owner, label: group.label };
  }

  admin() {
    const at = this.profile?.moderation_verified_at;
    const age = at == null ? Infinity : Date.now() / 1000 - at;
    return session.connected && !!this.profile?.moderation_admin_verified && age >= 0 && age < 30;
  }

  async load(live: boolean, force = false) {
    const scope = this.scope;
    if (!scope) return;
    const request = ++this.request;
    const signalsRevision = this.signalsRevision;
    this.localLoading = this.liveLoading = false;
    if (live) { this.liveLoading = true; this.liveError = ""; } else { this.localLoading = true; this.error = ""; }
    const current = () => { this.current(scope); if (request !== this.request) throw new Error("member request superseded"); };
    try {
      current();
      const profile = await invoke<MemberProfile>("user_profile", { accountId: scope.account, group: scope.group, jid: scope.jid, live, force });
      current();
      if (signalsRevision !== this.signalsRevision && this.profile) profile.local.signals = this.profile.local.signals;
      this.profile = profile;
      if (live && !this.presenceRequested) {
        this.presenceRequested = true;
        void invoke<void>("watch_presence", { account: scope.account, jid: scope.jid }).catch((error) => {
          try { current(); this.presenceRequested = false; this.liveError = `Presence subscription unavailable: ${error}`; } catch {}
        });
      }
      if (live && profile.moderation_admin_verified) {
        try {
          const blocked = await invoke<BlockedContact[]>("blocked_contacts", { account: scope.account });
          current();
          this.blocked = blocked.some((item) => item.jids.some((jid) => profile.local.addresses.includes(jid)));
        } catch (error) { current(); this.blocked = null; this.liveError = `Block status unavailable: ${error}`; }
      }
    } catch (error) {
      try { current(); } catch { return; }
      if (live) this.liveError = String(error); else this.error = String(error);
    } finally {
      if (request === this.request && this.scope === scope) {
        if (live) this.liveLoading = false; else this.localLoading = false;
      }
    }
  }

  presence(jid: string, online: boolean, last_seen: number | null) {
    if (!this.scope || !this.profile?.local.addresses.includes(jid)) return;
    try { this.current(this.scope); } catch { return; }
    ++this.signalsRevision;
    const local = this.profile.local;
    this.profile = { ...this.profile, local: { ...local, signals: { ...local.signals, online, last_seen, presence_at: Math.floor(Date.now() / 1000) } } };
  }

  typing(chat: string, jid: string, typing: string) {
    if (!this.scope || this.scope.group !== chat || !this.profile?.local.addresses.includes(jid)) return;
    try { this.current(this.scope); } catch { return; }
    ++this.signalsRevision;
    const local = this.profile.local;
    this.profile = { ...this.profile, local: { ...local, signals: { ...local.signals, typing, typing_at: Math.floor(Date.now() / 1000) } } };
  }

  queueRefresh(group: string | null) {
    const scope = this.scope;
    if (!scope || group !== null && scope.group !== group || this.refreshTimer) return;
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = undefined;
      try { this.current(scope); } catch { return; }
      void this.load(false).then(() => { if (this.scope === scope && session.connected) void this.load(true); });
    }, 200);
  }

  async save(scope: MemberScope, text: string, warnings: number) {
    this.current(scope);
    const note = await invoke<MemberNote>("set_member_note", { accountId: scope.account, group: scope.group, jid: scope.jid, text, warnings });
    this.current(scope);
    if (this.profile) this.profile = { ...this.profile, local: { ...this.profile.local, note } };
  }

  async audit(scope: MemberScope, filter: AuditFilters, before: GroupAuditCursor | null): Promise<AuditPage> {
    this.current(scope);
    const page = await invoke<AuditPage>("group_audit_page", { accountId: scope.account, chat: scope.group,
      filter: { ...filter, member: scope.jid, before, limit: 50 } });
    this.current(scope);
    return page;
  }

  async action(scope: MemberScope, action: MemberAction): Promise<ParticipantChange[] | void> {
    this.current(scope);
    return composer.enqueue(async (signal) => {
      this.current(scope, signal);
      const profile = await invoke<MemberProfile>("user_profile", { accountId: scope.account, group: scope.group, jid: scope.jid, live: true, force: false });
      this.current(scope, signal);
      this.profile = profile;
      const reason = memberActionReason(action, { admin: this.admin(), connected: session.connected, self: profile.local.identity.own,
        member: this.member(), blocked: this.blocked, supported: ["promote", "demote", "remove", "block", "unblock"] });
      if (reason) throw new Error(reason);
      const result = action in commands
        ? await invoke<ParticipantChange[]>(commands[action as keyof typeof commands], { account: scope.account, chat: scope.group, jids: [scope.jid] })
        : await invoke<void>("set_contact_blocked", { account: scope.account, jid: scope.jid, blocked: action === "block" });
      this.current(scope, signal);
      if (action === "block" || action === "unblock") this.blocked = action === "block";
      else void this.load(false);
      return result;
    });
  }
}

export const memberSheet = new MemberSheetState();
