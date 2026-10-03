<script lang="ts" module>
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { t, formatDate } from "$lib/i18n/localizer";
  import type { Participant as Member, GroupInfo as GroupInfoData, AdminReport } from "$lib/utils/wire";
  export type { Member, GroupInfoData, AdminReport };
</script>

<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import AddMembers from "$lib/chat/AddMembers.svelte";
  import GroupRequests from "$lib/chat/GroupRequests.svelte";
  import TranscriptionOverride from "$lib/settings/TranscriptionOverride.svelte";
  import AutoDownloadOverride from "$lib/settings/AutoDownloadOverride.svelte";
  import NotificationSoundOverride from "$lib/settings/NotificationSoundOverride.svelte";
  import GroupInviteLinks from "$lib/chat/GroupInviteLinks.svelte";
  import GroupSettings from "$lib/chat/GroupSettings.svelte";
  import GroupAudit from "$lib/chat/GroupAudit.svelte";
  import type { AuditFilters, AuditPage, AuditScope } from "$lib/utils/group-audit";
  import type { GroupAuditCursor } from "$lib/utils/wire";
  import { session } from "$lib/state/session.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { changeText } from "$lib/utils/group-actions";
  import type { GroupHistoryResult, GroupMemberAddResult, ParticipantChange } from "$lib/utils/models";
  import type { GroupJoinRequest, GroupSettingChange, GroupSettings as GroupSettingsData } from "$lib/utils/wire";
  import { displayName, phoneLabel } from "$lib/utils/phone";

  let {
    jid,
    title,
    info,
    error,
    groupInfoDiagnostic = null,
    avatars,
    pinned,
    onavatar,
    onretry,
    onpin,
    onopenurl,
    onmessage,
    onprofile,
    onlabel,
    me,
    namer = displayName,
    onreports,
    onallowreports,
    onadd,
    onretryhistory,
    oninviteload,
    oninvitereset,
    account,
    onsettingsload,
    onsettingchange,
    onpicturechange,
    onrequests,
    onrequestchange,
    onremove,
    onpromote,
    ondemote,
    onmembersadd,
    onjump,
    onloadAudit,
    auditRevision = 0,
    onclose,
  }: {
    jid: string;
    title: string;
    info: GroupInfoData | null;
    error: LocalizedError | string | null;
    groupInfoDiagnostic?: string | null;
    avatars: Record<string, string | null>;
    pinned: boolean;
    onavatar: (jid: string) => void;
    onretry: () => void;
    onpin: () => void;
    onopenurl: (url: string) => void;
    onmessage: (jid: string) => void;
    /** Opens someone's profile card at the click. */
    onprofile: (jid: string, name: string, event: MouseEvent) => void;
    /** Sets our own tag in this group; empty clears it. */
    onlabel: (label: string) => Promise<void>;
    /** Our own JID, to find ourselves in the member list. */
    me: string | null;
    /** Readable name for a member; the default formats bare numbers only. */
    namer?: (name: string | null, jid: string) => string;
    /** Messages members reported to the admins; only admins may ask. */
    onreports: () => Promise<AdminReport[]>;
    onallowreports: (allow: boolean) => Promise<void>;
    /** Adds people to the group; the server answers per person. */
    onadd: (jids: string[], optedIn: string[]) => Promise<GroupMemberAddResult>;
    onretryhistory: (retryId: string) => Promise<GroupHistoryResult>;
    oninviteload: () => Promise<string>;
    oninvitereset: () => Promise<string>;
    account: string | null;
    onsettingsload: () => Promise<GroupSettingsData>;
    onsettingchange: (change: GroupSettingChange) => Promise<void>;
    onpicturechange: (data: string) => Promise<void>;
    onrequests: () => Promise<GroupJoinRequest[]>;
    onrequestchange: (jids: string[], approve: boolean) => Promise<ParticipantChange[]>;
    /** Removes people from the group. */
    onremove: (jids: string[]) => Promise<ParticipantChange[]>;
    /** Gives people admin rights. */
    onpromote: (jids: string[]) => Promise<ParticipantChange[]>;
    /** Takes admin rights back. */
    ondemote: (jids: string[]) => Promise<ParticipantChange[]>;
    /** Whether members, or only admins, may add people. */
    onmembersadd: (allow: boolean) => Promise<void>;
    /** Shows a reported message in the conversation. */
    onjump: (id: string) => void;
    onloadAudit?: (scope: AuditScope, filters: AuditFilters, cursor: GroupAuditCursor | null) => Promise<AuditPage>;
    auditRevision?: number;
    onclose: () => void;
  } = $props();

  const self = $derived(
    info?.participants.find((p) => p.jid === me || (me && p.number === me.split("@")[0])),
  );
  let inviteAdmin = $state<string | null>(null);
  const inviteOwner = $derived(JSON.stringify([session.activeAccount, jid]));
  $effect(() => { if (info) inviteAdmin = info.admin ? inviteOwner : null; });
  let tagDraft = $state<string | null>(null);
  let tagBusy = $state(false);
  let tagError = $state<LocalizedError | string | null>(null);
  const tagValue = $derived(tagDraft ?? self?.label ?? "");

  async function saveTag() {
    tagBusy = true;
    tagError = null;
    try {
      await onlabel(tagValue.trim());
      tagDraft = null;
    } catch (e) {
      tagError = normalizeError(e);
    } finally {
      tagBusy = false;
    }
  }

  type Section = "overview" | "settings" | "members" | "reports" | "requests" | "audit";
  let section = $state<Section>("overview");
  let settingsBusy = $state(false);
  const nav = $derived<{ id: Section; label: string; group: string }[]>([
    { id: "overview", label: t("ui.overview"), group: title },
    { id: "settings", label: t("settings.title"), group: title },
    ...(onloadAudit ? [{ id: "audit" as Section, label: t("group.audit_log"), group: title }] : []),
    {
      id: "members",
      label: info ? t("group.members_count", { count: info.participants.length }) : t("group.members"),
      group: title,
    },
    ...(self?.admin ? [{ id: "reports" as Section, label: t("group.reports"), group: t("group.admin") }] : []),
    ...(info?.admin ? [{ id: "requests" as Section, label: t("group.requests"), group: t("group.admin") }] : []),
  ]);

  let reports = $state<AdminReport[] | null>(null);
  let reportsError = $state<LocalizedError | string | null>(null);
  $effect(() => {
    if (section !== "reports" || reports !== null) return;
    onreports()
      .then((r) => (reports = r))
      .catch((e) => (reportsError = normalizeError(e)));
  });

  /** The add-members rule is saving. */
  let addModeBusy = $state(false);
  async function setMembersAdd(allow: boolean) {
    addModeBusy = true;
    memberError = null; memberRefusals = [];
    try {
      await onmembersadd(allow);
      if (info) info.members_can_add = allow;
    } catch (e) {
      memberError = normalizeError(e);
    } finally {
      addModeBusy = false;
    }
  }

  /** The add-participants dialog. */
  let adding = $state(false);
  /** Selection mode for the member list, for remove/promote/demote. */
  let selecting = $state(false);
  let selected = $state<Record<string, true>>({});
  let memberBusy = $state(false);
  let memberRefusals = $state<ParticipantChange[]>([]);
  const refusal = $derived(memberRefusals.map(changeText).filter(Boolean).join(" "));
  let memberError = $state<LocalizedError | string | null>(null);
  let confirmRemove = $state(false);
  const selectedList = $derived(Object.keys(selected));

  function toggleSelect(jid: string) {
    const next = { ...selected };
    if (next[jid]) delete next[jid];
    else next[jid] = true;
    selected = next;
  }

  /** Runs a member change for the selection, reporting the server's refusals. */
  async function runMemberAction(action: (jids: string[]) => Promise<ParticipantChange[]>) {
    const jids = selectedList;
    if (jids.length === 0) return;
    memberBusy = true;
    memberError = null; memberRefusals = [];
    try {
      const changes = await action(jids);
      memberRefusals = changes;
      selected = {};
      selecting = false;
    } catch (e) {
      memberError = normalizeError(e);
    } finally {
      memberBusy = false;
    }
  }

  let allowBusy = $state(false);
  async function setAllow(allow: boolean) {
    allowBusy = true;
    try {
      await onallowreports(allow);
      if (info) info.allow_admin_reports = allow;
    } catch (e) {
      reportsError = normalizeError(e);
    } finally {
      allowBusy = false;
    }
  }

  let query = $state("");
  const members = $derived(
    (info?.participants ?? [])
      // Our own entry may carry a nickname from our address book; show our push name.
      .map((m) => ({
        ...m,
        isSelf: m === self,
        display: me && m === self ? namer(null, me) : namer(m.name, m.jid),
      }))
      .filter((m) => {
        const q = query.trim().toLowerCase();
        return (
          !q ||
          m.display.toLowerCase().includes(q) ||
          (m.number ?? "").includes(q) ||
          (m.username ?? "").toLowerCase().includes(q)
        );
      })
      .sort(
        (a, b) =>
          Number(b.isSelf) - Number(a.isSelf) ||
          Number(b.owner) - Number(a.owner) ||
          a.display.localeCompare(b.display),
      ),
  );
  const groups = $derived(
    [
      { title: t("group.admins"), list: members.filter((m) => m.admin) },
      { title: t("group.members"), list: members.filter((m) => !m.admin) },
    ].filter((g) => g.list.length > 0),
  );

  /** The group picture shown large, or null while closed. */
  let enlarged = $state<string | null>(null);

  $effect(() => {
    if (section === "members") for (const m of info?.participants ?? []) onavatar(m.jid);
  });

  /** Up to two letters, or null for a label with none (a bare number). */
  function initials(label: string) {
    const words = label.replace(/[^\p{L}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
    if (words.length === 0) return null;
    return (words.length === 1 ? words[0].slice(0, 2) : words[0][0] + words[1][0]).toUpperCase();
  }
  function hue(id: string) {
    let h = 0;
    for (const c of id) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  }

  function linkParts(text: string) {
    return text.split(/(https?:\/\/[^\s<>()\[\]{}"']+)/g).filter(Boolean);
  }
</script>

{#snippet avatar(id: string, label: string, size: number)}
  {#if avatars[id]}
    <img class="avatar" style="--size: {size}px" src={convertFileSrc(avatars[id]!)} alt="" />
  {:else}
    {@const letters = initials(label)}
    <span class="avatar placeholder" style="--size: {size}px; --hue: {hue(id)}"
      >{#if letters}{letters}{:else}<Icon name="user" size={Math.round(size * 0.5)} />{/if}</span
    >
  {/if}
{/snippet}

<Panel label={t("group.info")} {nav} bind:section onclose={() => { if (!settingsBusy) onclose(); }}>
  {#snippet header()}
    <div class="head">
      {@render avatar(jid, title, 44)}
      <span class="head-text">
        <span class="head-name"><bdi>{title}</bdi></span>
        <span class="head-sub">{t("group.group")} · {info ? t("group.member_count", { count: info.participants.length }) : "…"}</span>
      </span>
    </div>
  {/snippet}

  {#if error && groupInfoDiagnostic}<details class="diagnostic"><summary>{t("error.technical_details")}</summary><pre dir="auto">{groupInfoDiagnostic}</pre></details>{/if}
  {#if section === "overview" && session.activeAccount && inviteAdmin === inviteOwner}
    <div class="setting stack"><GroupInviteLinks chat={jid} canReset onload={oninviteload} onreset={oninvitereset} /></div>
  {/if}
  {#if section === "audit" && onloadAudit}
    <GroupAudit {account} group={jid} requestKey={auditRevision} onload={onloadAudit}
      onjump={(_group, id) => onjump(id)} namer={(jid) => namer(null, jid)}
      formatTime={(at) => formatDate(at, { dateStyle: "medium", timeStyle: "short" })} />
  {:else if section === "settings" && account}
    <h2>{t("group.settings")}</h2>
    {#key JSON.stringify([account, jid])}
      <GroupSettings chat={jid} {account} onload={onsettingsload} onchange={onsettingchange} onpicture={onpicturechange}
        onbusy={(busy) => settingsBusy = busy} />
    {/key}
  {:else if section === "requests"}
    {#if info && !info.admin}
      <p class="error-text">{t("group.requests_admin_only")}</p>
    {:else}
      <GroupRequests chat={jid} {namer} onload={onrequests} onchange={onrequestchange} />
    {/if}
  {:else if !info}
    {#if error}
      <h2>{t("group.load_failed")}</h2>
      <p class="lede">{error}</p>
      <div class="actions-row"><button class="button primary" onclick={onretry}>{t("ui.retry")}</button></div>
    {:else}
      <p class="muted">{t("group.loading")}</p>
    {/if}
  {:else if section === "overview"}
    <div class="hero">
      <button
        class="hero-picture"
        title={avatars[jid] ? t("contact.view_picture") : undefined}
        disabled={!avatars[jid]}
        onclick={() => (enlarged = avatars[jid] ?? null)}>{@render avatar(jid, title, 96)}</button>
      <div>
        <h2><bdi>{info.subject ?? title}</bdi></h2>
        <span class="muted">
          {info.community ? t("group.community") : info.announcements ? t("group.announcements") : t("group.group")} · {t("group.member_count", { count: info.participants.length })}{#if info.parent_name}{" · "}{t("group.in_parent")} <bdi>{info.parent_name}</bdi>{/if}{#if info.created_at}{" · "}{t("group.created_on", { date: formatDate(info.created_at) })}{/if}{#if info.owner}{" · "}{t("group.owned_by")}
            {#if info.owner_jid}{@const owner = info.owner_jid}<button
                class="owner"
                onclick={(e) => onprofile(owner, info.owner ?? owner, e)}><bdi>{info.owner}</bdi></button
              >{:else}{info.owner}{/if}{/if}
        </span>
      </div>
    </div>

    {#if info.description}
      <h3>{t("contact.description")}</h3>
      <p class="description">
        {#each linkParts(info.description) as part}{#if /^https?:\/\//.test(part)}<a
              href={part}
              onclick={(e) => {
                e.preventDefault();
                onopenurl(part);
              }}>{part}</a
            >{:else}{part}{/if}{/each}
      </p>
    {/if}

    <div class="setting">
      <div>
        <span class="setting-title">{t("group.who_send")}</span>
        <span class="setting-desc">
          {info.community
            ? t("group.community_send_hint")
            : info.announce
              ? t(info.admin ? "group.send_admins_you" : "group.send_admins")
              : t("group.everyone")}
        </span>
      </div>
    </div>
    <div class="setting">
      <div>
        <span class="setting-title">{t("group.who_edit")}</span>
        <span class="setting-desc">{info.locked ? t("group.admin_only") : t("group.everyone")}</span>
      </div>
    </div>
    <div class="setting">
      <div>
        <span class="setting-title">{t("group.who_add")}</span>
        <span class="setting-desc">
          {info.members_can_add
            ? info.admin
              ? t("group.everyone")
              : t("group.everyone_you")
            : t("group.admin_only")}
        </span>
      </div>
      {#if info.admin}
        <input
          class="switch"
          type="checkbox"
          checked={info.members_can_add}
          disabled={addModeBusy}
          aria-label={t("group.members_add")}
          onchange={(e) => setMembersAdd(e.currentTarget.checked)} />
      {/if}
    </div>

    {#if session.activeAccount}
      <div class="setting stack"><AutoDownloadOverride accountId={session.activeAccount} chat={jid} /></div>
      <div class="setting stack"><TranscriptionOverride accountId={session.activeAccount} chat={jid} /></div>
      <div class="setting stack"><NotificationSoundOverride accountId={session.activeAccount} chat={jid} /></div>
    {/if}
    <div class="setting stack">
      <div>
        <span class="setting-title">{t("group.your_tag")}</span>
        <span class="setting-desc">{t("group.tag_hint")}</span>
      </div>
      <div class="tag-row">
        <input
          class="field"
          maxlength="30"
          placeholder={t("group.tag_placeholder")}
          value={tagValue}
          oninput={(e) => (tagDraft = e.currentTarget.value)}
          onkeydown={(e) => e.key === "Enter" && saveTag()} />
        <button
          class="tag-add"
          title={self?.label ? t("group.tag_save") : t("group.tag_add")}
          aria-label={self?.label ? t("group.tag_save") : t("group.tag_add")}
          disabled={tagBusy || tagValue.trim() === (self?.label ?? "")}
          onclick={saveTag}><Icon name={self?.label ? "check" : "plus"} size={16} /></button>
      </div>
      {#if tagError}<p class="error-text">{tagError}</p>{/if}
    </div>

    <label class="setting">
      <div>
        <span class="setting-title">{t("chat.pin_chat")}</span>
        <span class="setting-desc">{t("chat.pin_hint")}</span>
      </div>
      <input class="switch" type="checkbox" checked={pinned} onchange={onpin} />
    </label>

    {#if info.admin || self?.admin}
    <label class="setting">
      <div>
        <span class="setting-title">{t("group.admin_reports")}</span>
        <span class="setting-desc">{t("group.admin_reports_hint")}</span>
      </div>
      <input
        class="switch"
        type="checkbox"
        checked={info.allow_admin_reports}
        disabled={allowBusy}
        onchange={(e) => setAllow(e.currentTarget.checked)} />
    </label>
    {/if}
  {:else if section === "reports"}
    <h2>{t("group.reported_messages")}</h2>
    <p class="lede">{t("group.reported_hint")}</p>
    {#if reportsError}
      <p class="error-text">{reportsError}</p>
    {:else if reports === null}
      <p class="muted">{t("group.reports_loading")}</p>
    {:else if reports.length === 0}
      <p class="muted">{t("group.reports_empty")}</p>
    {:else}
      <ul class="reports">
        {#each reports as report (report.id)}
          <li class="report">
            {#if report.message}
              <button class="report-message" onclick={() => onjump(report.id)}>
                <span class="report-author">{namer(report.message.sender_name, report.message.sender)}</span>
                <span class="report-text" dir="auto">{report.message.text}</span>
              </button>
            {:else}
              <span class="muted">{t("group.report_message_missing")}</span>
            {/if}
            <span class="muted">
              {t("group.reported_by")} {report.reporters
                .map(([who, at]) => `${namer(null, who)} (${formatDate(at, { dateStyle: "medium", timeStyle: "short" })})`)
                .join(", ")}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="members-head">
      <h2>{t("group.members")}</h2>
      {#if info.admin}
        <button
          class="link-button"
          onclick={() => {
            selecting = !selecting;
            selected = {};
            memberError = null; memberRefusals = [];
          }}>{selecting ? t("ui.done") : t("ui.select")}</button>
      {/if}
      {#if info.admin || info.members_can_add}
        <button class="button" onclick={() => (adding = true)}>
          <Icon name="plus" size={14} /> {t("ui.add")}
        </button>
      {/if}
    </div>
    {#if memberError || refusal}<p class="error-text">{memberError || refusal}</p>{/if}
    {#if selecting && selectedList.length > 0}
      <div class="member-actions">
        <span class="muted">{t("group.selected_count", { count: selectedList.length })}</span>
        <button class="button" disabled={memberBusy} onclick={() => runMemberAction(onpromote)}>{t("group.make_admin")}</button>
        <button class="button" disabled={memberBusy} onclick={() => runMemberAction(ondemote)}>{t("group.dismiss_admin")}</button>
        <button class="button danger" disabled={memberBusy} onclick={() => (confirmRemove = true)}>{t("ui.remove")}</button>
      </div>
    {/if}
    <label class="member-search">
      <Icon name="search" size={15} />
      <input placeholder={t("group.search_members")} dir="auto" bind:value={query} />
    </label>
    {#each groups as group (group.title)}
      <h3 class="member-group">{group.title} <span>{group.list.length}</span></h3>
      <ul class="members">
        {#each group.list as member (member.jid)}
          {@const secondary = [
            member.number && member.display !== phoneLabel(member.number)
              ? (phoneLabel(member.number) ?? member.number)
              : null,
            member.username ? `@${member.username}` : null,
          ].filter(Boolean)}
          <li class="member">
            {#if selecting}
              <input
                class="pick"
                type="checkbox"
                checked={!!selected[member.jid]}
                disabled={member.isSelf || member.owner}
                aria-label={t("group.select_member", { name: member.display })}
                onchange={() => toggleSelect(member.jid)} />
            {/if}
            {@render avatar(member.jid, member.display, 38)}
            <span class="member-text">
              <span class="member-name">
                <button class="member-display" onclick={(event) => onprofile(member.jid, member.display, event)}
                  aria-label={t("group.open_member", { name: member.display })}>{member.display}</button>
                {#if member.isSelf}<span class="you">{t("chat.you")}</span>{/if}
              </span>
              {#if member.label}
                <span class="member-tag">{member.label}</span>
              {:else if secondary.length > 0}
                <span class="member-sub">{secondary.join(" · ")}</span>
              {/if}
            </span>
            {#if member.owner}
              <span class="role owner">{t("group.owner_badge")}</span>
            {:else if member.admin}
              <span class="role">{t("group.admin")}</span>
            {/if}
            {#if !member.isSelf}
              <button
                class="message"
                title={t("chat.message")}
                aria-label={t("contact.message_name", { name: member.display })}
                onclick={() => onmessage(member.jid)}><Icon name="message" size={16} /></button>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted">{t("group.members_no_matches")}</p>
    {/each}
  {/if}
</Panel>

{#if adding}
  <AddMembers
    chat={jid}
    title={info?.subject ?? title}
    members={info?.participants ?? []}
    {avatars}
    {me}
    {onavatar}
    {onadd}
    {onretryhistory}
    onclose={() => (adding = false)} />
{/if}

{#if confirmRemove}
  <ConfirmDialog
    label={t("group.remove_member")}
    title={t("group.remove_question", { count: selectedList.length })}
    hint={t("group.remove_hint")}
    onclose={() => (confirmRemove = false)}>
    {#snippet actions()}
      <button
        class="danger"
        onclick={() => {
          confirmRemove = false;
          void runMemberAction(onremove);
        }}>{t("ui.remove")}</button>
      <button onclick={() => (confirmRemove = false)}>{t("ui.cancel")}</button>
    {/snippet}
  </ConfirmDialog>
{/if}

{#if enlarged}
  <Lightbox {jid} preview={enlarged} alt={title} onclose={() => (enlarged = null)} />
{/if}

<style>
  .avatar {
    width: var(--size);
    height: var(--size);
    border-radius: 50%;
    object-fit: cover;
    flex: none;
  }
  .placeholder {
    display: grid;
    place-items: center;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: calc(var(--size) * 0.36);
    font-weight: 600;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px 12px;
  }
  .head-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head-name {
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .head-sub {
    font-size: 0.7812rem;
    color: var(--muted);
  }
  .hero {
    display: flex;
    align-items: center;
    gap: 20px;
    margin-bottom: 16px;
  }
  .hero h2 {
    margin: 0 0 4px;
  }
  h3 {
    margin: 12px 0 6px;
    font-size: 0.75rem;
    font-weight: 600;
    color: var(--muted);
  }
  .description {
    margin: 0 0 8px;
    padding: 14px 16px;
    background: var(--surface);
    border-radius: var(--radius);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.5;
  }
  .description a {
    color: var(--link);
  }
  .member-search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 8px;
    padding: 0 12px;
    height: 36px;
    background: var(--surface);
    border-radius: 999px;
    color: var(--muted);
  }
  .member-search input {
    flex: 1;
    background: transparent;
    border: 0;
    outline: none;
    color: var(--text);
    font: inherit;
  }
  .members-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 6px;
  }
  .members-head h2 {
    flex: 1;
    margin: 0;
  }
  .members-head .button {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border: 0;
    border-radius: 6px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-size: 0.7812rem;
    cursor: pointer;
  }
  .link-button {
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 0.7812rem;
    cursor: pointer;
  }
  .link-button:hover {
    color: var(--text);
  }
  .member-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 4px 0 8px;
    padding: 6px 8px;
    background: var(--raised);
    border-radius: 8px;
  }
  .member-actions .muted {
    flex: 1;
    font-size: 0.7812rem;
  }
  .member-actions .button {
    padding: 4px 8px;
    border: 0;
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 0.75rem;
    cursor: pointer;
  }
  .member-actions .button.danger {
    color: var(--danger, #f15c6d);
  }
  .member-actions .button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .pick {
    flex: none;
    accent-color: var(--accent, #00a884);
  }
  .members,
  .reports {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .report {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }
  .report-message {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 10px;
    border: 0;
    border-inline-start: 3px solid var(--danger);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    text-align: start;
    cursor: pointer;
  }
  .report-author {
    font-weight: 600;
    font-size: 0.8125rem;
  }
  .report-text {
    white-space: pre-wrap;
  }
  .hero-picture {
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: zoom-in;
    transition: filter calc(0.15s * var(--motion-scale));
  }
  .hero-picture:disabled {
    cursor: default;
  }
  .hero-picture:not(:disabled):hover {
    filter: brightness(1.12);
  }
  .member-group {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 18px 0 6px;
    padding: 0 10px 6px;
    border-bottom: 1px solid var(--line);
    font-size: 0.7188rem;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .member-group span {
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--raised);
    color: var(--muted);
    letter-spacing: 0;
  }
  .members + .member-group {
    margin-top: 22px;
  }
  .member {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 10px;
    border-radius: var(--radius);
    transition: background calc(0.12s * var(--motion-scale));
  }
  .member:hover {
    background: var(--surface);
  }
  .member-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .member-name {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-weight: 500;
  }
  .member-display {
    border: 0;
    padding: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .you {
    flex: none;
    font-size: 0.6875rem;
    font-weight: 400;
    color: var(--muted);
  }
  .member-sub,
  .member-tag {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.7812rem;
    color: var(--muted);
  }
  .member-tag {
    color: var(--accent-text);
  }
  .role {
    flex: none;
    padding: 2px 8px;
    border-radius: 6px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 0.6875rem;
    font-weight: 600;
  }
  .role.owner {
    background: var(--mention-self-soft);
    color: var(--text);
  }
  /* One field with its add button inside, on the right. */
  .tag-row {
    position: relative;
    display: flex;
  }
  .tag-row .field {
    flex: 1;
    padding-inline-end: 44px;
  }
  .owner {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .owner:hover {
    text-decoration: underline;
  }
  .tag-add {
    position: absolute;
    top: 50%;
    inset-inline-end: 6px;
    transform: translateY(-50%);
    display: grid;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: var(--accent-ink);
    cursor: pointer;
  }
  .tag-add:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .tag-add:disabled {
    background: transparent;
    color: var(--faint);
    cursor: default;
  }
  .message {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    background: var(--raised);
    border: 0;
    border-radius: 50%;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
    flex: none;
  }
  .member:hover .message,
  .message:focus-visible {
    opacity: 1;
  }
  .message:hover {
    color: var(--text);
  }
  .diagnostic pre { max-height: 180px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
