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
  import Icon from "$lib/Icon.svelte";
  import Panel from "$lib/Panel.svelte";
  import { displayName, phoneLabel } from "$lib/phone";

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
    onlabel,
    me,
    namer = displayName,
    onreports,
    onallowreports,
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
    /** Sets our own tag in this group; empty clears it. */
    onlabel: (label: string) => Promise<void>;
    /** Our own JID, to find ourselves in the member list. */
    me: string | null;
    /** Readable name for a member; the default formats bare numbers only. */
    namer?: (name: string | null, jid: string) => string;
    /** Messages members reported to the admins; only admins may ask. */
    onreports: () => Promise<AdminReport[]>;
    onallowreports: (allow: boolean) => Promise<void>;
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
            .length} members{#if info.parent_name}
            · in {info.parent_name}{/if}{#if info.created_at}
            · created {new Date(info.created_at * 1000).toLocaleDateString()}{/if}
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
          class="button primary"
          disabled={tagBusy || tagValue.trim() === (self?.label ?? "")}
          onclick={saveTag}>{tagBusy ? "Saving…" : "Save"}</button>
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

    <label class="setting">
      <div>
        <span class="setting-title">Reports to admins</span>
        <span class="setting-desc">
          {self?.admin
            ? "Lets members report messages to this group's admins, not to WhatsApp."
            : info.allow_admin_reports
              ? "Members can report messages to the admins from a message's menu."
              : "The admins have turned reports off in this group."}
        </span>
      </div>
      <input
        class="switch"
        type="checkbox"
        checked={info.allow_admin_reports}
        disabled={!self?.admin || allowBusy}
        onchange={(e) => setAllow(e.currentTarget.checked)} />
    </label>
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
    <h2>Members</h2>
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

{#if enlarged}
  <button class="lightbox" aria-label="Close picture" onclick={() => (enlarged = null)}>
    <img src={convertFileSrc(enlarged)} alt={title} />
  </button>
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
  .lightbox {
    position: fixed;
    inset: 0;
    z-index: 100;
    display: grid;
    place-items: center;
    padding: 40px;
    border: 0;
    background: var(--scrim);
    cursor: zoom-out;
    animation: lightbox-in calc(0.18s * var(--motion-scale)) var(--ease) both;
  }
  .lightbox img {
    max-width: min(640px, 100%);
    max-height: 100%;
    border-radius: 12px;
    box-shadow: var(--shadow);
  }
  @keyframes lightbox-in {
    from {
      opacity: 0;
    }
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
  .tag-row {
    display: flex;
    gap: 8px;
  }
  .tag-row .field {
    flex: 1;
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
