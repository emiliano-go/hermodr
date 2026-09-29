<script lang="ts" module>
  export type Member = {
    jid: string;
    name: string;
    admin: boolean;
    owner: boolean;
    number: string | null;
    username: string | null;
    label: string | null;
  };
  export type GroupInfoData = {
    subject: string | null;
    description: string | null;
    created_at: number | null;
    owner: string | null;
    owner_jid: string | null;
    participants: Member[];
    allow_admin_reports: boolean;
    announce: boolean;
    locked: boolean;
    community: boolean;
    announcements: boolean;
    parent: string | null;
    parent_name: string | null;
    admin: boolean;
    can_send: boolean;
    /** Members may add participants, not just admins. */
    members_can_add: boolean;
  };
  export type AdminReport = {
    id: string;
    message: { text: string; sender: string; sender_name: string | null; timestamp: number } | null;
    /** Reporter JID and Unix time of each report. */
    reporters: [string, number][];
  };
</script>

<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import Icon from "$lib/ui/Icon.svelte";
  import Lightbox from "$lib/media/Lightbox.svelte";
  import Panel from "$lib/ui/Panel.svelte";
  import AddMembers from "$lib/chat/AddMembers.svelte";
  import ConfirmDialog from "$lib/ui/ConfirmDialog.svelte";
  import { changeText } from "$lib/utils/group-actions";
  import type { ParticipantChange } from "$lib/utils/models";
  import { displayName, phoneLabel } from "$lib/utils/phone";

  let {
    jid,
    title,
    info,
    error,
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
    onremove,
    onpromote,
    ondemote,
    onmembersadd,
    onjump,
    onclose,
  }: {
    jid: string;
    title: string;
    info: GroupInfoData | null;
    error: string | null;
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
    onadd: (jids: string[]) => Promise<ParticipantChange[]>;
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
    onclose: () => void;
  } = $props();

  const self = $derived(
    info?.participants.find((p) => p.jid === me || (me && p.number === me.split("@")[0])),
  );
  let tagDraft = $state<string | null>(null);
  let tagBusy = $state(false);
  let tagError = $state<string | null>(null);
  const tagValue = $derived(tagDraft ?? self?.label ?? "");

  async function saveTag() {
    tagBusy = true;
    tagError = null;
    try {
      await onlabel(tagValue.trim());
      tagDraft = null;
    } catch (e) {
      tagError = String(e);
    } finally {
      tagBusy = false;
    }
  }

  type Section = "overview" | "members" | "reports";
  let section = $state<Section>("overview");
  const nav = $derived<{ id: Section; label: string; group: string }[]>([
    { id: "overview", label: "Overview", group: title },
    {
      id: "members",
      label: info ? `Members (${info.participants.length})` : "Members",
      group: title,
    },
    ...(self?.admin ? [{ id: "reports" as Section, label: "Reports", group: "Admin" }] : []),
  ]);

  let reports = $state<AdminReport[] | null>(null);
  let reportsError = $state<string | null>(null);
  $effect(() => {
    if (section !== "reports" || reports !== null) return;
    onreports()
      .then((r) => (reports = r))
      .catch((e) => (reportsError = String(e)));
  });

  /** The add-members rule is saving. */
  let addModeBusy = $state(false);
  async function setMembersAdd(allow: boolean) {
    addModeBusy = true;
    memberError = null;
    try {
      await onmembersadd(allow);
      if (info) info.members_can_add = allow;
    } catch (e) {
      memberError = String(e);
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
  let memberError = $state<string | null>(null);
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
    memberError = null;
    try {
      const changes = await action(jids);
      const refusals = changes.map(changeText).filter((text): text is string => !!text);
      if (refusals.length > 0) memberError = refusals.join(" ");
      selected = {};
      selecting = false;
    } catch (e) {
      memberError = String(e);
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
      reportsError = String(e);
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
      { title: "Admins", list: members.filter((m) => m.admin) },
      { title: "Members", list: members.filter((m) => !m.admin) },
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

<Panel label="Group info" {nav} bind:section {onclose}>
  {#snippet header()}
    <div class="head">
      {@render avatar(jid, title, 44)}
      <span class="head-text">
        <span class="head-name">{title}</span>
        <span class="head-sub">Group · {info ? `${info.participants.length} members` : "…"}</span>
      </span>
    </div>
  {/snippet}

  {#if !info}
    {#if error}
      <h2>Could not load this group</h2>
      <p class="lede">{error}</p>
      <div class="actions-row"><button class="button primary" onclick={onretry}>Retry</button></div>
    {:else}
      <p class="muted">Loading group info…</p>
    {/if}
  {:else if section === "overview"}
    <div class="hero">
      <button
        class="hero-picture"
        title={avatars[jid] ? "View picture" : undefined}
        disabled={!avatars[jid]}
        onclick={() => (enlarged = avatars[jid] ?? null)}>{@render avatar(jid, title, 96)}</button>
      <div>
        <h2>{info.subject ?? title}</h2>
        <span class="muted">
          {info.community ? "Community" : info.announcements ? "Announcements" : "Group"} · {info.participants
            .length} members{#if info.parent_name}{" · "}in {info.parent_name}{/if}{#if info.created_at}{" · "}created
            {new Date(info.created_at * 1000).toLocaleDateString()}{/if}{#if info.owner}{" · "}owned by
            {#if info.owner_jid}{@const owner = info.owner_jid}<button
                class="owner"
                onclick={(e) => onprofile(owner, info.owner ?? owner, e)}>{info.owner}</button
              >{:else}{info.owner}{/if}{/if}
        </span>
      </div>
    </div>

    {#if info.description}
      <h3>Description</h3>
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
        <span class="setting-title">Who can send</span>
        <span class="setting-desc">
          {info.community
            ? "Nobody writes in the community itself; its groups hold the conversations."
            : info.announce
              ? `Only admins${info.admin ? ", including you" : ""}.`
              : "Everyone in the group."}
        </span>
      </div>
    </div>
    <div class="setting">
      <div>
        <span class="setting-title">Who can edit the group's info</span>
        <span class="setting-desc">{info.locked ? "Only admins." : "Everyone in the group."}</span>
      </div>
    </div>
    <div class="setting">
      <div>
        <span class="setting-title">Who can add members</span>
        <span class="setting-desc">
          {info.members_can_add
            ? info.admin
              ? "Everyone in the group."
              : "Everyone in the group, including you."
            : "Only admins."}
        </span>
      </div>
      {#if info.admin}
        <input
          class="switch"
          type="checkbox"
          checked={info.members_can_add}
          disabled={addModeBusy}
          aria-label="Members may add people"
          onchange={(e) => setMembersAdd(e.currentTarget.checked)} />
      {/if}
    </div>

    <div class="setting stack">
      <div>
        <span class="setting-title">Your tag in this group</span>
        <span class="setting-desc">Shown under your name on your messages here. Leave empty to remove it.</span>
      </div>
      <div class="tag-row">
        <input
          class="field"
          maxlength="30"
          placeholder="Add a tag"
          value={tagValue}
          oninput={(e) => (tagDraft = e.currentTarget.value)}
          onkeydown={(e) => e.key === "Enter" && saveTag()} />
        <button
          class="tag-add"
          title={self?.label ? "Save tag" : "Add tag"}
          aria-label={self?.label ? "Save tag" : "Add tag"}
          disabled={tagBusy || tagValue.trim() === (self?.label ?? "")}
          onclick={saveTag}><Icon name={self?.label ? "check" : "plus"} size={16} /></button>
      </div>
      {#if tagError}<p class="error-text">{tagError}</p>{/if}
    </div>

    <label class="setting">
      <div>
        <span class="setting-title">Pin chat</span>
        <span class="setting-desc">Keeps it at the top of the list, on every linked device.</span>
      </div>
      <input class="switch" type="checkbox" checked={pinned} onchange={onpin} />
    </label>

    {#if info.admin || self?.admin}
    <label class="setting">
      <div>
        <span class="setting-title">Reports to admins</span>
        <span class="setting-desc">Lets members report messages to this group's admins, not to WhatsApp.</span>
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
    <h2>Reported messages</h2>
    <p class="lede">Messages members reported to the admins. Only admins see this.</p>
    {#if reportsError}
      <p class="error-text">{reportsError}</p>
    {:else if reports === null}
      <p class="muted">Loading reports…</p>
    {:else if reports.length === 0}
      <p class="muted">Nothing has been reported.</p>
    {:else}
      <ul class="reports">
        {#each reports as report (report.id)}
          <li class="report">
            {#if report.message}
              <button class="report-message" onclick={() => onjump(report.id)}>
                <span class="report-author">{namer(report.message.sender_name, report.message.sender)}</span>
                <span class="report-text">{report.message.text}</span>
              </button>
            {:else}
              <span class="muted">This message is not on this device.</span>
            {/if}
            <span class="muted">
              Reported by {report.reporters
                .map(([who, at]) => `${namer(null, who)} (${new Date(at * 1000).toLocaleString()})`)
                .join(", ")}
            </span>
          </li>
        {/each}
      </ul>
    {/if}
  {:else}
    <div class="members-head">
      <h2>Members</h2>
      {#if info.admin}
        <button
          class="link-button"
          onclick={() => {
            selecting = !selecting;
            selected = {};
            memberError = null;
          }}>{selecting ? "Done" : "Select"}</button>
      {/if}
      {#if info.admin || info.members_can_add}
        <button class="button" onclick={() => (adding = true)}>
          <Icon name="plus" size={14} /> Add
        </button>
      {/if}
    </div>
    {#if memberError}<p class="error-text">{memberError}</p>{/if}
    {#if selecting && selectedList.length > 0}
      <div class="member-actions">
        <span class="muted">{selectedList.length} selected</span>
        <button class="button" disabled={memberBusy} onclick={() => runMemberAction(onpromote)}>Make admin</button>
        <button class="button" disabled={memberBusy} onclick={() => runMemberAction(ondemote)}>Dismiss as admin</button>
        <button class="button danger" disabled={memberBusy} onclick={() => (confirmRemove = true)}>Remove</button>
      </div>
    {/if}
    <label class="member-search">
      <Icon name="search" size={15} />
      <input placeholder="Search by name, number or username" bind:value={query} />
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
                aria-label="Select {member.display}"
                onchange={() => toggleSelect(member.jid)} />
            {/if}
            {@render avatar(member.jid, member.display, 38)}
            <span class="member-text">
              <span class="member-name">
                <span class="member-display">{member.display}</span>
                {#if member.isSelf}<span class="you">You</span>{/if}
              </span>
              {#if member.label}
                <span class="member-tag">{member.label}</span>
              {:else if secondary.length > 0}
                <span class="member-sub">{secondary.join(" · ")}</span>
              {/if}
            </span>
            {#if member.owner}
              <span class="role owner">Owner</span>
            {:else if member.admin}
              <span class="role">Admin</span>
            {/if}
            {#if !member.isSelf}
              <button
                class="message"
                title="Message"
                aria-label="Message {member.display}"
                onclick={() => onmessage(member.jid)}><Icon name="message" size={16} /></button>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="muted">No members match.</p>
    {/each}
  {/if}
</Panel>

{#if adding && info}
  <AddMembers
    title={info.subject ?? title}
    members={info.participants}
    {avatars}
    {me}
    {onavatar}
    {onadd}
    onclose={() => (adding = false)} />
{/if}

{#if confirmRemove}
  <ConfirmDialog
    label="Remove from group"
    title={`Remove ${selectedList.length} member${selectedList.length === 1 ? "" : "s"}?`}
    hint="They leave the group on every linked device. You can add them again later."
    onclose={() => (confirmRemove = false)}>
    {#snippet actions()}
      <button
        class="danger"
        onclick={() => {
          confirmRemove = false;
          void runMemberAction(onremove);
        }}>Remove</button>
      <button onclick={() => (confirmRemove = false)}>Cancel</button>
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
    font-size: 12.5px;
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
    font-size: 12px;
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
    font-size: 12.5px;
    cursor: pointer;
  }
  .link-button {
    border: 0;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
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
    font-size: 12.5px;
  }
  .member-actions .button {
    padding: 4px 8px;
    border: 0;
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 12px;
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
    border-left: 3px solid var(--danger);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .report-author {
    font-weight: 600;
    font-size: 13px;
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
    font-size: 11.5px;
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .you {
    flex: none;
    font-size: 11px;
    font-weight: 400;
    color: var(--muted);
  }
  .member-sub,
  .member-tag {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
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
    font-size: 11px;
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
    padding-right: 44px;
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
    right: 6px;
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
</style>
