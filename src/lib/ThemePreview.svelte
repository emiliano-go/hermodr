<script lang="ts">
  import Icon, { type IconName } from "$lib/Icon.svelte";
  import appIcon from "../../src-tauri/icons/128x128.png";

  export type Scene = "chat" | "signin" | "menu" | "dialog";
  let { scene }: { scene: Scene } = $props();

  // The real layout at a real window size, scaled down to the panel's width.
  const STAGE_W = 1000;
  const STAGE_H = 600;
  let width = $state(STAGE_W);
  const zoom = $derived(width / STAGE_W);

  function hue(jid: string) {
    let h = 0;
    for (const c of jid) h = (h * 31 + c.charCodeAt(0)) % 360;
    return h;
  }
  function initials(label: string) {
    const words = label.replace(/[^\p{L}\p{N}\s]/gu, "").trim().split(/\s+/).filter(Boolean);
    return words.length === 1 ? words[0].slice(0, 2).toUpperCase() : (words[0][0] + words[1][0]).toUpperCase();
  }

  type Row = { name: string; preview: string; time: string; unread?: number; mention?: boolean; active?: boolean; icon?: IconName };
  const rows: Row[] = [
    { name: "Design team", preview: "You: Will do 👍", time: "09:32", active: true },
    { name: "Mom", preview: "Dinner at 8?", time: "09:05", unread: 2 },
    { name: "Book club", preview: "Laura: Photo", time: "08:47", unread: 12, mention: true, icon: "image" },
    { name: "Diego Pérez", preview: "You: See you tomorrow", time: "Yesterday" },
    { name: "Laura", preview: "Thanks!", time: "Yesterday" },
    { name: "Work", preview: "Ana: Standup moved to 10", time: "Mon" },
    { name: "Family", preview: "Dad: 📍 Location", time: "Sun" },
  ];

  type Msg = {
    who?: string;
    mine?: boolean;
    first?: boolean;
    text: string;
    time: string;
    status?: "read" | "delivered";
    quote?: { author: string; text: string };
    forMe?: boolean;
    hovered?: boolean;
    replying?: boolean;
  };
  const messages: Msg[] = [
    { who: "Ana", first: true, text: "Morning! The new mockups are up 🎨", time: "09:12" },
    { who: "Ana", text: "Can someone check the header spacing?", time: "09:12", replying: true },
    { mine: true, first: true, quote: { author: "Ana", text: "Can someone check the header spacing?" }, text: "On it, it looks tight on mobile", time: "09:14", status: "read" },
    { who: "Diego", first: true, forMe: true, text: "", time: "09:20" },
    { who: "Laura", first: true, hovered: true, text: "The brief is here: ", time: "09:31" },
    { mine: true, first: true, text: "Will do 👍", time: "09:32", status: "delivered" },
  ];
</script>

<!-- The app's own markup and styles, drawn with the live theme variables. -->
<div class="frame" bind:clientWidth={width} style:height="{STAGE_H * zoom}px" aria-hidden="true">
  <div class="stage" style:zoom style:width="{STAGE_W}px" style:height="{STAGE_H}px">
    {#if scene === "signin"}
      <div class="pairing">
        <div class="intro-glow"></div>
        <header class="intro-head">
          <img class="intro-logo" src={appIcon} alt="" />
          <div>
            <h1>Hermóðr</h1>
            <span class="intro-tag">WhatsApp, native on your desktop</span>
          </div>
          <span class="icon intro-settings"><Icon name="settings" size={18} /></span>
        </header>
        <div class="intro-card resume">
          <span class="resume-avatar" style="--hue: {hue('me')}">WA</span>
          <h2>Signing in</h2>
          <span class="resume-who">WhatsApp · +598 97 504 482</span>
          <div class="resume-progress">
            <div class="resume-status">
              <span>Loading messages…</span>
              <span class="resume-count">120 of 400 · 30%</span>
            </div>
            <div class="resume-bar determinate"><span style:width="30%"></span></div>
          </div>
        </div>
        <p class="intro-foot">
          Your messages stay end-to-end encrypted. History is kept only on this computer, within the limits
          you set in Settings.
        </p>
      </div>
    {:else}
      <div class="layout">
        <aside class="chats">
          <header>
            <h1 class="title">Chats</h1>
            <span class="header-icons">
              <span class="icon badge-host"><Icon name="at" size={18} /><span class="icon-badge">1</span></span>
              <span class="icon"><Icon name="star" size={18} /></span>
            </span>
          </header>
          <div class="search"><Icon name="search" size={15} /><span class="placeholder">Search chats and contacts</span></div>
          <div class="filters">
            <span class="chip active">All</span>
            <span class="chip">Unread<span class="chip-count">2</span></span>
            <span class="chip">Groups</span>
          </div>
          <ul>
            {#each rows as row (row.name)}
              <li>
                <div class="chat-row" class:active={row.active}>
                  <span class="avatar" style="--hue: {hue(row.name)}">{initials(row.name)}</span>
                  <span class="name">{row.name}</span>
                  <span class="time" class:unread={row.unread}>{row.time}</span>
                  <span class="preview"
                    >{#if row.icon}<span class="preview-icon"><Icon name={row.icon} size={15} /></span>{/if}{row.preview}</span>
                  <span class="badges">
                    {#if row.mention}<span class="badge mention-badge">@</span>{/if}
                    {#if row.unread}<span class="badge">{row.unread}</span>{/if}
                  </span>
                </div>
              </li>
            {/each}
          </ul>
          <footer class="user-panel">
            <span class="me">
              <span class="me-avatar-wrap">
                <span class="me-avatar" style="--hue: {hue('me')}">WA</span>
                <span class="presence online"></span>
              </span>
              <span class="me-text"><span class="me-name">WhatsApp</span><span class="me-status">Online</span></span>
            </span>
            <span class="icon"><Icon name="settings" size={19} /></span>
          </footer>
        </aside>

        <section class="conversation">
          <header>
            <div class="chat-heading">
              <span class="avatar" style="--hue: {hue('Design team')}">DT</span>
              <span class="chat-title">Design team<span class="chat-sub">Ana, Diego, Laura, You</span></span>
            </div>
            <div class="header-tools">
              <span class="icon"><Icon name="search" size={18} /></span>
              <span class="icon"><Icon name="at" size={18} /></span>
              <span class="icon"><Icon name="sliders" size={18} /></span>
            </div>
          </header>

          <div class="messages group">
            <div class="day"><span>Today</span></div>
            {#each messages as m, i (i)}
              <div
                class="msg-row"
                class:first-row={m.first}
                class:for-me={m.forMe}
                class:hovered={m.hovered}
                class:replying={m.replying && scene === "chat"}>
                <div class="bubble inline-meta" class:mine={m.mine} class:first={m.first} class:menu-open={scene === "menu" && i === 2}>
                  {#if m.first && !m.mine}
                    <span class="sender-avatar"><span class="avatar" style="--hue: {hue(m.who!)}">{initials(m.who!)}</span></span>
                    <span class="sender" style="--hue: {hue(m.who!)}">{m.who}</span>
                  {/if}
                  {#if m.quote}
                    <span class="quote">
                      <span class="quote-author">{m.quote.author}</span>
                      <span class="quote-text">{m.quote.text}</span>
                    </span>
                  {/if}
                  <span class="text"
                    >{#if m.forMe}<span class="mention-pill self"
                        ><span class="mention-initials" style="--hue: {hue('me')}">WA</span>@You</span
                      > can you check the footer too? <span class="mention-pill"
                        ><span class="mention-initials" style="--hue: {hue('Ana')}">AN</span>@Ana</span
                      > says it's off{:else}{m.text}{#if m.hovered}<span class="link">example.com/brief</span>{/if}{/if}<span
                      class="meta-spacer"
                      class:mine={m.mine}></span></span>
                  <span class="reply-btn"><Icon name="chevronDown" size={16} /></span>
                  <span class="meta"
                    >{m.time}{#if m.mine}<span class="ticks" class:read={m.status === "read"}>✓✓</span>{/if}</span>
                </div>
              </div>
            {/each}
          </div>

          {#if scene === "chat"}
            <div class="reply-preview">
              <span class="reply-body">
                <span class="reply-to">Replying to Ana</span>
                <span class="reply-snippet">Can someone check the header spacing?</span>
              </span>
              <span class="icon"><Icon name="x" size={16} /></span>
            </div>
          {/if}
          <div class="composer">
            <span class="icon attach"><Icon name="plus" size={22} /></span>
            <span class="textarea">Type a message</span>
            <div class="composer-tools">
              <span class="icon tool-text">GIF</span>
              <span class="icon"><Icon name="sticker" size={20} /></span>
              <span class="icon"><Icon name="smile" size={20} /></span>
            </div>
            <span class="send ready"><Icon name="mic" size={19} /></span>
          </div>
        </section>
      </div>

      {#if scene === "menu"}
        <div class="menu">
          <div class="reactions">
            {#each ["👍", "❤️", "😂", "😮", "😢", "🙏"] as emoji (emoji)}
              <span class="reaction" class:mine={emoji === "❤️"}>{emoji}</span>
            {/each}
          </div>
          {#each [["reply", "Reply"], ["copy", "Copy"], ["forward", "Forward"], ["star", "Star"], ["pin", "Pin"]] as [icon, label], i (label)}
            <span class="item" class:hot={i === 1}><Icon name={icon as IconName} size={18} />{label}</span>
          {/each}
          <div class="sep"></div>
          <span class="item danger"><Icon name="trash" size={18} />Delete</span>
        </div>
      {:else if scene === "dialog"}
        <div class="sheet-backdrop">
          <div class="sheet confirm">
            <h2>Delete message?</h2>
            <p class="hint">Delete it for everyone in this chat, or only from your devices.</p>
            <div class="confirm-actions">
              <span class="button danger">Delete for everyone</span>
              <span class="button danger">Delete for me</span>
              <span class="button hot">Cancel</span>
            </div>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</div>

<style>
  .frame {
    position: relative;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--bg);
  }
  .stage {
    position: relative;
    overflow: hidden;
    background: var(--bg);
    color: var(--text);
    font-family: "Twemoji Country Flags", var(--font);
    font-size: var(--font-size);
    line-height: normal;
    -webkit-font-smoothing: antialiased;
    user-select: none;
    pointer-events: none;
  }
  .stage :global(*) {
    transition: none !important;
  }

  /* Everything below is copied from the app's own styles (+page.svelte and
     MessageMenu.svelte); hover states are drawn with a class instead. */
  .icon {
    display: inline-grid;
    place-items: center;
    min-width: 32px;
    min-height: 32px;
    border-radius: 8px;
    color: var(--muted);
  }
  .layout {
    display: grid;
    grid-template-columns: 300px 1fr;
    height: 100%;
    overflow: hidden;
  }
  .chats {
    position: relative;
    overflow: hidden;
    background: var(--bg);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .chats header,
  .conversation header {
    min-width: 0;
    height: 59px;
    box-sizing: border-box;
    flex: none;
    overflow: hidden;
    padding: 0 12px 0 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .chats header {
    height: 64px;
  }
  .conversation header {
    background: var(--surface);
  }
  .header-icons {
    display: flex;
  }
  .title {
    margin: 0;
    font-size: 22px;
    font-weight: 700;
  }
  .badge-host {
    position: relative;
  }
  .icon-badge {
    position: absolute;
    top: 2px;
    right: 0;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: 999px;
    background: var(--mention);
    color: var(--accent-ink);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px 8px;
    padding: 0 12px;
    height: 36px;
    flex: none;
    background: var(--surface);
    border: 1px solid transparent;
    border-radius: 999px;
    color: var(--faint);
  }
  .search .placeholder {
    color: var(--muted);
  }
  .filters {
    display: flex;
    gap: 8px;
    padding: 0 12px 8px;
    flex: none;
  }
  .chip {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--surface);
    border-radius: 999px;
    color: var(--muted);
    font-size: 14px;
    padding: 5px 12px;
  }
  .chip.active {
    background: var(--accent-soft);
    color: var(--accent);
  }
  .chip-count {
    font-size: 12px;
  }
  .chats ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow: hidden;
    flex: 1;
  }
  .chat-row {
    position: relative;
    box-sizing: border-box;
    width: 100%;
    height: 72px;
    min-width: 0;
    overflow: hidden;
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-areas: "avatar name time" "avatar preview badge";
    gap: 2px 15px;
    padding: 0 15px 0 13px;
    align-items: center;
    align-content: center;
  }
  .chat-row::after {
    content: "";
    position: absolute;
    left: 77px;
    right: 0;
    bottom: 0;
    border-bottom: 1px solid var(--line);
  }
  .chat-row.active {
    background: var(--raised);
  }
  .avatar {
    grid-area: avatar;
    flex: none;
    width: 49px;
    height: 49px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
    font-size: 15px;
    font-weight: 500;
    letter-spacing: 0.02em;
  }
  .conversation header .avatar {
    width: 40px;
    height: 40px;
    font-size: 14px;
  }
  .name {
    grid-area: name;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 17px;
    font-weight: 400;
  }
  .time {
    grid-area: time;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .time.unread {
    color: var(--accent);
  }
  .preview {
    grid-area: preview;
    min-width: 0;
    color: var(--muted);
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preview-icon {
    display: inline-flex;
    vertical-align: -2px;
    margin-right: 4px;
  }
  .badges {
    grid-area: badge;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .badge {
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    border-radius: 999px;
    height: 20px;
    padding: 0 6px;
    min-width: 20px;
    box-sizing: border-box;
  }
  .badge.mention-badge {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-size: 11px;
    font-weight: 700;
  }
  .user-panel {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 62px;
    box-sizing: border-box;
    padding: 0 8px;
    background: var(--surface);
  }
  .me {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    border-radius: 8px;
  }
  .me-avatar-wrap {
    position: relative;
    flex: none;
  }
  .me-avatar {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    background: hsl(var(--hue, 160) 28% 24%);
    color: hsl(var(--hue, 160) 45% 80%);
    font-size: 13px;
    font-weight: 500;
  }
  .presence {
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--faint);
    box-shadow: 0 0 0 3px var(--surface);
  }
  .presence.online {
    background: var(--accent);
  }
  .me-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .me-name {
    font-size: 14px;
    font-weight: 600;
  }
  .me-status {
    font-size: 12px;
    color: var(--muted);
  }

  .conversation {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    position: relative;
    background: var(--chat-bg);
  }
  .chat-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .chat-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    font-size: 16px;
    font-weight: 400;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chat-sub {
    font-size: 13px;
    font-weight: 400;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .header-tools {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
    flex: none;
  }
  .messages {
    flex: 1;
    overflow: hidden;
    min-width: 0;
    min-height: 0;
    --pad-l: clamp(16px, 7%, 90px);
    --pad-r: clamp(16px, 7%, 90px);
    padding: 12px 0 8px;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 2px;
  }
  .messages.group {
    --pad-l: max(56px, 7%);
  }
  .day {
    display: flex;
    justify-content: center;
    margin: 12px 0 8px;
  }
  .day span {
    background: var(--surface);
    color: var(--muted);
    font-size: 12.5px;
    padding: 5px 12px;
    border-radius: var(--radius-sm);
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
  }
  .msg-row {
    display: flex;
    flex-direction: column;
    padding: 1px var(--pad-r) 1px var(--pad-l);
  }
  .msg-row.hovered {
    background: var(--row-hover);
  }
  .msg-row.replying {
    background: var(--replying-soft);
    box-shadow: inset 3px 0 0 var(--replying);
  }
  .msg-row.for-me {
    background: var(--mention-soft);
    box-shadow: inset 3px 0 0 var(--mention);
  }
  .bubble {
    min-width: 0;
    flex-shrink: 0;
    align-self: flex-start;
    position: relative;
    max-width: 65%;
    background: var(--bubble);
    border-radius: var(--radius-sm);
    box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13);
    padding: 6px 7px 8px 9px;
    display: flex;
    flex-direction: column;
    gap: 3px;
    line-height: 19px;
    word-break: break-word;
    overflow-wrap: anywhere;
  }
  .bubble.first {
    margin-top: 10px;
  }
  .bubble.mine {
    align-self: flex-end;
    background: var(--bubble-mine);
  }
  .bubble.first:not(.mine) {
    border-top-left-radius: 0;
  }
  .bubble.first.mine {
    border-top-right-radius: 0;
  }
  .bubble.first::before {
    content: "";
    position: absolute;
    top: 0;
    width: 9px;
    height: 13px;
    background: inherit;
  }
  .bubble.first:not(.mine)::before {
    left: -8px;
    clip-path: polygon(0 0, 100% 0, 100% 100%);
  }
  .bubble.first.mine::before {
    right: -8px;
    clip-path: polygon(0 0, 100% 0, 0 100%);
  }
  .bubble.inline-meta .meta {
    position: absolute;
    right: 7px;
    bottom: 4px;
  }
  .meta-spacer {
    display: inline-block;
    width: 44px;
    height: 1px;
  }
  .meta-spacer.mine {
    width: 62px;
  }
  .sender-avatar {
    position: absolute;
    left: -38px;
    top: 0;
  }
  .sender-avatar .avatar {
    width: 28px;
    height: 28px;
    font-size: 11px;
  }
  .sender {
    align-self: flex-start;
    font-size: 12.8px;
    font-weight: 500;
    line-height: 22px;
    color: hsl(var(--hue) 65% 68%);
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .link {
    color: var(--link);
  }
  .mention-pill {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0 5px 0 2px;
    border-radius: 4px;
    vertical-align: bottom;
    font-weight: 500;
    color: var(--mention-pill);
    background: var(--mention-pill-soft);
    white-space: nowrap;
  }
  .mention-pill.self {
    color: var(--mention);
    background: var(--mention-self-soft);
  }
  .mention-initials {
    display: grid;
    place-items: center;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    flex: none;
    font-size: 8px;
    background: hsl(var(--hue) 28% 24%);
    color: hsl(var(--hue) 45% 80%);
  }
  .quote {
    display: grid;
    grid-template-columns: 1fr auto;
    grid-template-areas: "author thumb" "text thumb";
    align-items: center;
    column-gap: 8px;
    background: rgba(0, 0, 0, 0.18);
    border-left: 4px solid var(--accent);
    border-radius: 6px;
    font-size: 13px;
    line-height: 18px;
    color: var(--muted);
    padding: 5px 8px 6px;
    margin-bottom: 2px;
    overflow: hidden;
    min-width: 0;
    max-width: 100%;
  }
  .quote-author {
    grid-area: author;
    display: block;
    font-weight: 500;
    color: var(--accent);
  }
  .quote-text {
    grid-area: text;
    min-width: 0;
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    font-size: 11px;
    line-height: 15px;
    font-variant-numeric: tabular-nums;
    color: color-mix(in srgb, var(--text) 60%, transparent);
    align-self: flex-end;
    display: flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
  }
  .ticks {
    font-size: 10px;
    letter-spacing: -2px;
  }
  .ticks.read {
    color: var(--link);
  }
  .reply-btn {
    position: absolute;
    top: 3px;
    right: 3px;
    z-index: 1;
    display: flex;
    padding: 3px;
    border-radius: 999px;
    color: var(--muted);
    background: var(--bubble);
    opacity: 0;
  }
  .bubble.mine .reply-btn {
    background: var(--bubble-mine);
  }
  .bubble.menu-open .reply-btn {
    opacity: 1;
  }
  .reply-preview {
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 14px;
    padding: 8px 8px 8px 12px;
    background: var(--surface);
    border-left: 3px solid var(--accent);
    border-radius: var(--radius-sm) var(--radius-sm) 0 0;
    font-size: 12px;
    color: var(--muted);
  }
  .reply-body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .reply-to {
    color: var(--accent-text);
    font-weight: 600;
  }
  .composer {
    display: flex;
    align-items: flex-end;
    gap: 6px;
    padding: 5px 16px 5px 10px;
    min-height: 62px;
    box-sizing: border-box;
    background: var(--surface);
    flex: none;
  }
  .composer > .textarea {
    flex: 1;
    align-self: center;
    background: var(--raised);
    border: 1px solid transparent;
    border-radius: var(--radius);
    padding: 9px 12px;
    color: var(--faint);
    line-height: 1.4;
  }
  .composer-tools {
    display: flex;
    align-items: center;
    align-self: center;
    gap: 2px;
  }
  .composer-tools .icon {
    width: 38px;
    height: 38px;
  }
  .tool-text {
    font-size: 11px;
    font-weight: 700;
  }
  .attach,
  .send {
    width: 42px;
    height: 42px;
    margin-bottom: 5px;
  }
  .send {
    flex: none;
    display: grid;
    place-items: center;
    border-radius: 999px;
    color: var(--muted);
  }
  .send.ready {
    color: var(--accent);
  }

  /* MessageMenu, opened on the reply above. */
  .menu {
    position: absolute;
    left: 520px;
    top: 250px;
    z-index: 2;
    min-width: 230px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    display: flex;
    flex-direction: column;
  }
  .reactions {
    display: flex;
    gap: 2px;
    padding: 2px 2px 6px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--line-strong);
  }
  .reaction {
    flex: 1;
    height: 36px;
    display: grid;
    place-items: center;
    border-radius: 8px;
    font-size: 20px;
  }
  .reaction.mine {
    background: var(--accent-soft);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: 6px;
    color: var(--text);
    font-size: 14.5px;
  }
  .item :global(svg) {
    color: var(--muted);
  }
  .item.hot {
    background: var(--raised);
  }
  .item.danger,
  .item.danger :global(svg) {
    color: var(--danger);
  }
  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--line-strong);
  }

  /* Confirm sheet */
  .sheet-backdrop {
    position: absolute;
    inset: 0;
    z-index: 3;
    background: var(--scrim);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .sheet {
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow);
    padding: 20px 22px;
    width: 400px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }
  .sheet.confirm h2 {
    margin: 0;
    font-size: 17px;
    font-weight: 600;
  }
  .hint {
    margin: 0;
    color: var(--faint);
    font-size: 12px;
    max-width: 44ch;
    text-wrap: balance;
  }
  .confirm-actions {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
  .confirm-actions .button {
    padding: 8px 14px;
    border-radius: 999px;
    color: var(--accent);
    font-weight: 600;
  }
  .confirm-actions .button.hot {
    background: var(--raised);
  }
  .confirm-actions .button.danger {
    color: var(--danger);
  }

  /* Sign-in */
  .pairing {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 22px;
    padding: 32px 24px;
    box-sizing: border-box;
    background: var(--chat-bg);
  }
  .intro-glow {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(60% 50% at 15% 10%, var(--accent-soft), transparent 70%),
      radial-gradient(50% 40% at 90% 90%, color-mix(in srgb, var(--link) 12%, transparent), transparent 70%);
  }
  .intro-head,
  .intro-card,
  .intro-foot {
    position: relative;
    box-sizing: border-box;
    width: min(920px, 100%);
    flex: none;
  }
  .intro-head {
    display: flex;
    align-items: center;
    gap: 14px;
  }
  .intro-logo {
    width: 48px;
    height: 48px;
    border-radius: 12px;
  }
  .intro-head h1 {
    margin: 0;
    font-size: 26px;
    letter-spacing: -0.01em;
  }
  .intro-tag {
    color: var(--muted);
    font-size: 13.5px;
  }
  .intro-settings {
    margin-left: auto;
  }
  .intro-card {
    display: grid;
    padding: 40px 44px;
    border: 1px solid var(--line-strong);
    border-radius: 16px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .intro-card.resume {
    grid-template-columns: 1fr;
    justify-items: center;
    gap: 10px;
    width: min(460px, 100%);
    text-align: center;
  }
  .intro-card.resume h2 {
    margin: 8px 0 0;
    font-size: 22px;
    font-weight: 500;
  }
  .resume-avatar {
    display: grid;
    place-items: center;
    width: 88px;
    height: 88px;
    border-radius: 50%;
    background: var(--raised-2);
    font-size: 30px;
    font-weight: 600;
    box-shadow: 0 0 0 4px var(--accent-soft);
  }
  .resume-who {
    color: var(--muted);
    font-size: 13.5px;
  }
  .resume-progress {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(320px, 100%);
    margin-top: 18px;
  }
  .resume-status {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    color: var(--muted);
    font-size: 12.5px;
  }
  .resume-count {
    font-variant-numeric: tabular-nums;
  }
  .resume-bar {
    height: 6px;
    border-radius: 999px;
    background: var(--raised);
    overflow: hidden;
  }
  .resume-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .intro-foot {
    margin: 0;
    color: var(--faint);
    font-size: 12.5px;
    text-align: center;
  }
</style>
