<script lang="ts">
  import Icon from "$lib/Icon.svelte";
  import appIcon from "../../src-tauri/icons/128x128.png";

  let { scene }: { scene: "chat" | "signin" | "dialog" } = $props();

  const chats = [
    { name: "Design team", text: "Ana: @You can you check the mockups?", time: "12:41", unread: 3, mention: true },
    { name: "Mom", text: "Yes! Bringing dessert 🍰", time: "12:29", selected: true },
    { name: "Diego", text: "See you tomorrow", time: "Yesterday", mine: true },
    { name: "Book club", text: "📷 Photo", time: "Mon", unread: 12 },
    { name: "Laura", text: "Thanks!", time: "Sun" },
  ];
</script>

<!-- Drawn with the live theme variables, so every edit shows here at once. -->
<div class="pv" aria-hidden="true">
  {#if scene === "chat"}
    <div class="pv-app">
      <aside class="pv-list">
        <div class="pv-search"><Icon name="search" size={12} /> Search</div>
        {#each chats as chat (chat.name)}
          <div class="pv-row" class:selected={chat.selected}>
            <span class="pv-avatar">{chat.name[0]}</span>
            <span class="pv-row-text">
              <span class="pv-row-top">
                <span class="pv-name">{chat.name}</span>
                <span class="pv-time" class:fresh={chat.unread}>{chat.time}</span>
              </span>
              <span class="pv-row-top">
                <span class="pv-snippet">
                  {#if chat.mine}<span class="pv-ticks">✓✓</span>{/if}{chat.text}
                </span>
                {#if chat.mention}<span class="pv-at">@</span>{/if}
                {#if chat.unread}<span class="pv-badge">{chat.unread}</span>{/if}
              </span>
            </span>
          </div>
        {/each}
      </aside>

      <section class="pv-chat">
        <header class="pv-head">
          <span class="pv-avatar">M</span>
          <span class="pv-head-text"><span class="pv-name">Mom</span><span class="pv-sub">online</span></span>
        </header>
        <div class="pv-messages">
          <span class="pv-day">Today</span>
          <div class="pv-msg in">
            <span class="pv-bubble">Are you coming for dinner? <span class="pv-meta">12:28</span></span>
          </div>
          <div class="pv-msg out">
            <span class="pv-bubble">
              <span class="pv-quote"><span class="pv-quote-who">Mom</span>Are you coming for dinner?</span>
              Yes! Bringing dessert 🍰 <span class="pv-meta">12:29 <span class="pv-ticks">✓✓</span></span>
            </span>
          </div>
          <div class="pv-msg in mention">
            <span class="pv-bubble">
              <span class="pv-sender">Diego</span>
              <span class="pv-pill self">@You</span> said you'd bring the wine, ask
              <span class="pv-pill">@Laura</span>
              <span class="pv-meta">12:31</span>
            </span>
          </div>
          <div class="pv-msg in hover">
            <span class="pv-bubble">Recipe: <span class="pv-link">example.com/tiramisu</span> <span class="pv-meta">12:33</span></span>
          </div>
          <div class="pv-msg out jump">
            <span class="pv-bubble">On my way <span class="pv-meta">12:40 <span class="pv-ticks">✓✓</span></span></span>
          </div>
        </div>
        <div class="pv-replying">
          <span class="pv-quote"><span class="pv-quote-who">Replying to Diego</span>said you'd bring the wine</span>
        </div>
        <footer class="pv-composer">
          <span class="pv-input">Type a message</span>
          <span class="pv-send"><Icon name="send" size={13} /></span>
        </footer>
      </section>
    </div>
  {:else if scene === "signin"}
    <div class="pv-pairing">
      <div class="pv-glow"></div>
      <div class="pv-brand">
        <img src={appIcon} alt="" />
        <span><span class="pv-title">Hermóðr</span><span class="pv-sub">WhatsApp, native on your desktop</span></span>
      </div>
      <div class="pv-signin">
        <span class="pv-avatar big">W</span>
        <span class="pv-title">Signing in</span>
        <span class="pv-sub">WhatsApp · +598 97 504 482</span>
        <span class="pv-status"><span>Loading messages…</span><span class="pv-count">120 of 400 · 30%</span></span>
        <span class="pv-bar"><span style:width="30%"></span></span>
        <span class="pv-status"><span>Connecting to WhatsApp…</span></span>
        <span class="pv-bar busy"><span></span></span>
        <span class="pv-btn primary">Connect</span>
      </div>
      <span class="pv-foot">Your messages stay end-to-end encrypted.</span>
    </div>
  {:else}
    <div class="pv-dialog-scene">
      <div class="pv-backdrop-app">
        {#each [0, 1, 2] as i (i)}
          <div class="pv-msg {i % 2 ? 'out' : 'in'}"><span class="pv-bubble">Message {i + 1}</span></div>
        {/each}
        <div class="pv-menu">
          <span class="pv-item"><Icon name="reply" size={13} /> Reply</span>
          <span class="pv-item hot"><Icon name="forward" size={13} /> Forward</span>
          <span class="pv-item"><Icon name="star" size={13} /> Star</span>
          <span class="pv-sep"></span>
          <span class="pv-item danger"><Icon name="trash" size={13} /> Delete</span>
        </div>
      </div>
      <div class="pv-scrim"></div>
      <div class="pv-dialog">
        <span class="pv-title">Rename group</span>
        <span class="pv-sub">Everyone in the group sees the new name.</span>
        <span class="pv-field focused">Weekend plans</span>
        <span class="pv-field">Add a description</span>
        <span class="pv-error">Names can be at most 100 characters.</span>
        <span class="pv-toggle"><span class="pv-switch on"></span> Only admins can edit</span>
        <span class="pv-actions">
          <span class="pv-btn danger">Leave group</span>
          <span class="pv-spacer"></span>
          <span class="pv-btn">Cancel</span>
          <span class="pv-btn primary">Save</span>
        </span>
      </div>
    </div>
  {/if}
</div>

<style>
  .pv {
    height: 310px;
    overflow: hidden;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--chat-bg);
    color: var(--text);
    font-family: var(--font);
    font-size: calc(var(--font-size) * 0.86);
    line-height: 1.35;
    color-scheme: var(--scheme);
    user-select: none;
    pointer-events: none;
  }
  .pv-avatar {
    display: grid;
    place-items: center;
    flex: none;
    width: 2.3em;
    height: 2.3em;
    border-radius: 50%;
    background: var(--raised-2);
    color: var(--muted);
    font-weight: 600;
  }
  .pv-avatar.big {
    width: 3.4em;
    height: 3.4em;
    font-size: 1.1em;
  }
  .pv-name {
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pv-sub {
    display: block;
    color: var(--muted);
    font-size: 0.86em;
  }
  .pv-title {
    display: block;
    font-weight: 600;
    font-size: 1.15em;
  }

  /* Chat scene */
  .pv-app {
    display: grid;
    grid-template-columns: 38% 1fr;
    grid-template-rows: minmax(0, 1fr);
    height: 100%;
  }
  .pv-list {
    display: flex;
    flex-direction: column;
    gap: 1px;
    overflow: hidden;
    padding: 8px 6px;
    background: var(--bg);
    border-right: 1px solid var(--line);
    min-width: 0;
  }
  .pv-search {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
    padding: 5px 9px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--faint);
  }
  .pv-row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    border-radius: var(--radius);
  }
  .pv-row.selected {
    background: var(--raised-2);
  }
  .pv-row-text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .pv-row-top {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
  }
  .pv-row-top .pv-name {
    flex: 1;
  }
  .pv-time {
    font-size: 0.8em;
    color: var(--faint);
  }
  .pv-time.fresh {
    color: var(--accent-text);
  }
  .pv-snippet {
    flex: 1;
    min-width: 0;
    color: var(--muted);
    font-size: 0.9em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .pv-ticks {
    color: var(--link);
    margin-right: 3px;
    font-size: 0.85em;
  }
  .pv-badge,
  .pv-at {
    display: grid;
    place-items: center;
    min-width: 1.5em;
    height: 1.5em;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 0.75em;
    font-weight: 700;
  }
  .pv-at {
    background: var(--mention);
  }
  .pv-chat {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .pv-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--surface);
    border-bottom: 1px solid var(--line);
  }
  .pv-head .pv-avatar {
    width: 2em;
    height: 2em;
  }
  /* Anchored to the bottom like the real chat, so overflow clips the oldest message. */
  .pv-messages {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 3px;
    padding: 8px 0;
    overflow: hidden;
  }
  .pv-day {
    align-self: center;
    margin-bottom: 4px;
    padding: 2px 9px;
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--muted);
    font-size: 0.78em;
  }
  .pv-msg {
    display: flex;
    padding: 1px 10px;
  }
  .pv-msg.out {
    justify-content: flex-end;
  }
  .pv-msg.mention {
    background: var(--mention-soft);
    box-shadow: inset 3px 0 var(--mention);
  }
  .pv-msg.hover {
    background: var(--row-hover);
  }
  .pv-msg.jump {
    background: var(--jump-soft);
  }
  .pv-bubble {
    max-width: 78%;
    padding: 4px 8px;
    border-radius: var(--radius-sm);
    background: var(--bubble);
    box-shadow: 0 1px 0.5px rgba(0, 0, 0, 0.13);
  }
  .out .pv-bubble {
    background: var(--bubble-mine);
  }
  .pv-sender {
    display: block;
    color: var(--accent-text);
    font-size: 0.85em;
    font-weight: 600;
  }
  .pv-meta {
    float: right;
    margin: 0.35em 0 0 8px;
    color: var(--muted);
    font-size: 0.72em;
  }
  .pv-quote {
    display: block;
    margin-bottom: 3px;
    padding: 3px 7px;
    border-radius: calc(var(--radius-sm) - 2px);
    background: rgba(0, 0, 0, 0.12);
    box-shadow: inset 3px 0 var(--replying);
    color: var(--muted);
    font-size: 0.86em;
  }
  .pv-quote-who {
    display: block;
    color: var(--replying);
    font-weight: 600;
  }
  .pv-pill {
    padding: 0 3px;
    border-radius: 4px;
    background: var(--mention-pill-soft);
    color: var(--mention-pill);
  }
  .pv-pill.self {
    background: var(--mention-self-soft);
    color: var(--mention);
  }
  .pv-link {
    color: var(--link);
    text-decoration: underline;
  }
  .pv-replying {
    padding: 5px 10px 0;
    background: var(--surface);
    border-top: 1px solid var(--line);
  }
  .pv-replying .pv-quote {
    margin: 0;
    background: var(--replying-soft);
  }
  .pv-composer {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--surface);
  }
  .pv-input {
    flex: 1;
    padding: 5px 10px;
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--faint);
  }
  .pv-send {
    display: grid;
    place-items: center;
    width: 2em;
    height: 2em;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-ink);
  }

  /* Sign-in scene */
  .pv-pairing {
    position: relative;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
  }
  .pv-glow {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(60% 50% at 15% 10%, var(--accent-soft), transparent 70%),
      radial-gradient(50% 40% at 90% 90%, color-mix(in srgb, var(--link) 12%, transparent), transparent 70%);
  }
  .pv-brand {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 66%;
  }
  .pv-brand img {
    width: 2.6em;
    height: 2.6em;
  }
  .pv-signin {
    position: relative;
    box-sizing: border-box;
    width: 66%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 16px 22px;
    border: 1px solid var(--line-strong);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .pv-status {
    display: flex;
    justify-content: space-between;
    width: 100%;
    margin-top: 4px;
    color: var(--muted);
    font-size: 0.86em;
  }
  .pv-count {
    color: var(--faint);
  }
  .pv-bar {
    display: block;
    width: 100%;
    height: 5px;
    border-radius: 999px;
    background: var(--raised);
    overflow: hidden;
  }
  .pv-bar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .pv-bar.busy span {
    width: 40%;
    animation: pv-busy calc(1.2s * var(--motion-scale, 1)) ease-in-out infinite;
  }
  @keyframes pv-busy {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(250%);
    }
  }
  .pv-foot {
    position: relative;
    color: var(--faint);
    font-size: 0.8em;
  }

  /* Buttons, shared by the sign-in and dialog scenes */
  .pv-btn {
    padding: 5px 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--raised);
    font-size: 0.92em;
  }
  .pv-btn.primary {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-ink);
    font-weight: 600;
  }
  .pv-signin .pv-btn {
    margin-top: 6px;
  }
  .pv-btn.danger {
    color: var(--danger);
    background: var(--danger-soft);
    border-color: transparent;
  }

  /* Dialog scene */
  .pv-dialog-scene {
    position: relative;
    height: 100%;
  }
  .pv-backdrop-app {
    display: flex;
    flex-direction: column;
    gap: 6px;
    height: 100%;
    padding: 14px 0;
    box-sizing: border-box;
  }
  .pv-menu {
    position: absolute;
    left: 18px;
    top: 58px;
    z-index: 1;
    display: flex;
    flex-direction: column;
    width: 130px;
    padding: 4px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
  }
  .pv-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px;
    border-radius: calc(var(--radius) - 2px);
  }
  .pv-item.hot {
    background: var(--raised);
  }
  .pv-item.danger {
    color: var(--danger);
  }
  .pv-sep {
    height: 1px;
    margin: 3px 0;
    background: var(--line-soft);
  }
  .pv-scrim {
    position: absolute;
    inset: 0;
    left: 45%;
    background: var(--scrim);
  }
  .pv-dialog {
    position: absolute;
    top: 50%;
    right: 5%;
    transform: translateY(-50%);
    width: 44%;
    display: flex;
    flex-direction: column;
    gap: 7px;
    padding: 14px 16px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--bg);
    box-shadow: var(--shadow);
  }
  .pv-dialog .pv-sub {
    margin-bottom: 2px;
  }
  .pv-field {
    padding: 5px 9px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--faint);
  }
  .pv-field.focused {
    border-color: var(--accent);
    color: var(--text);
  }
  .pv-error {
    color: var(--danger);
    font-size: 0.82em;
  }
  .pv-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 0.92em;
  }
  .pv-switch {
    position: relative;
    width: 26px;
    height: 15px;
    border-radius: 999px;
    background: var(--raised-2);
  }
  .pv-switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: #fff;
  }
  .pv-switch.on {
    background: var(--accent);
  }
  .pv-switch.on::after {
    left: 13px;
  }
  .pv-actions {
    display: flex;
    gap: 6px;
    margin-top: 4px;
  }
  .pv-spacer {
    flex: 1;
  }
</style>
