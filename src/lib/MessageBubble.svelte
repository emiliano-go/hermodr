<script lang="ts">
  // One conversation message: the row, the bubble and every media branch.
  // The list computes the view model; every action funnels back through the
  // api so the page keeps owning state and IPC.
  import { convertFileSrc } from "@tauri-apps/api/core";
  import AudioPlayer from "$lib/AudioPlayer.svelte";
  import Embed from "$lib/Embed.svelte";
  import EventCard from "$lib/EventCard.svelte";
  import Icon from "$lib/Icon.svelte";
  import InviteCard, { inviteLink } from "$lib/InviteCard.svelte";
  import MessageText from "$lib/MessageText.svelte";
  import PollCard from "$lib/PollCard.svelte";
  import Spinner from "$lib/Spinner.svelte";
  import Avatar from "$lib/Avatar.svelte";
  import { initials } from "$lib/avatar";
  import { mediaSrc } from "$lib/MediaViewer.svelte";
  import {
    CARD_LABELS,
    DRAWN_KINDS,
    VIEW_ONCE_LABEL,
    replyIcon,
    statusMark,
  } from "$lib/message";
  import type { BubbleApi, BubbleVm, StoredMessage } from "$lib/models";

  let { message, vm, api }: { message: StoredMessage; vm: BubbleVm; api: BubbleApi } = $props();

  /** An SVG file sent as a document, which is drawn in place like a picture. */
  function isSvg(m: StoredMessage) {
    return m.media_kind === "document" && /\.svg$/i.test(m.media_path ?? m.text.split("\n")[0].trim());
  }

  function hostOf(url: string) {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  }
</script>

<!-- The whole row answers double-click and right-click, not just the bubble. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="msg-row"
  class:replying={vm.isReplying}
  class:jumped={vm.highlighted}
  class:for-me={vm.forMe}
  class:first-row={vm.first}
  ondblclick={() => api.onreplydraft(message)}
  oncontextmenu={(e) => {
    e.preventDefault();
    api.onmenu(e, message);
  }}>
<div
  class="bubble"
  class:media-only={vm.visual &&
    !vm.caption &&
    !message.reply_to_text &&
    !message.preview_url &&
    !vm.showSender}
  class:mine={message.from_me}
  class:first={true}
  class:inline-meta={vm.inlineMeta}
  class:has-reactions={!!vm.reactions}
  class:sticker-only={message.media_kind === "sticker" &&
    !message.reply_to_text &&
    !vm.showSender}
  class:menu-open={vm.menuOpen}
  class:edited={vm.isEdited}
  data-id={message.id}>
  {#if vm.showSender}
    <button
      type="button"
      class="sender-avatar"
      title="Profile"
      onclick={(e) => api.onprofile(message.sender, vm.senderText, e)}
      ><Avatar
        src={vm.senderAvatar}
        label={vm.senderText}
        seed={message.sender}
      /></button>
    <button
      type="button"
      class="sender"
      style="--hue: {vm.senderHue}"
      onclick={(e) => api.onprofile(message.sender, vm.senderText, e)}>{vm.senderText}</button>
    {#if vm.memberTag}
      <span class="member-label">{vm.memberTag}</span>
    {/if}
  {/if}

  {#if message.revoked}
    <span class="revoked">This message was deleted<span class="meta-spacer"></span></span>
  {:else}
    {#if vm.isForwarded}
      <span class="forwarded-mark"><Icon name="forward" size={13} /> Forwarded</span>
    {/if}
    {#if message.reply_to_text}
      <Embed
        compact
        tooltip="Go to message"
        label={vm.quoteAuthor}
        text={vm.quoteText}
        image={message.reply_to_kind === "image" && message.reply_to_thumb
          ? mediaSrc(message.reply_to_thumb)
          : null}
        icon={message.reply_to_kind ? replyIcon(message.reply_to_kind) : null}
        onclick={() => api.onjumpquoted(message)}>
        {#if vm.quoteChatName}
          <span class="quote-where">in {vm.quoteChatName}</span>
        {/if}
      </Embed>
    {/if}

    {#if vm.viewOnce}
      {@const what = VIEW_ONCE_LABEL[message.media_kind ?? ""] ?? "View once message"}
      {#if vm.onceAudioOpen && message.media_kind === "audio" && message.media_path}
        <AudioPlayer path={message.media_path} />
        <button class="once-done" onclick={() => api.oncloseonce()}>Done</button>
      {:else if message.media_kind === "view_once"}
        <span class="once spent">
          <span class="once-mark">1</span>
          <span>View once message<small>Open it on your phone</small></span>
        </span>
      {:else if vm.viewOnce.opened}
        <span class="once spent">
          <span class="once-mark">1</span>
          <span>{what}<small>{message.from_me ? "View once" : "Opened"}</small></span>
        </span>
      {:else}
        <button
          class="once"
          onclick={() => api.ononce(message)}>
          <span class="once-mark">1</span>
          <span>{what}<small>{message.media_path ? "View once" : "Download to view once"}</small></span>
        </button>
      {/if}
    {:else if message.media_kind === "sticker" && message.media_path}
      <img class="sticker" src={convertFileSrc(message.media_path)} alt="Sticker" />
    {:else if message.media_kind === "sticker"}
      <!-- Fetched on its own when shown; the placeholder keeps the sticker's space. -->
      <button
        class="sticker sticker-pending"
        title={vm.downloading ? "Loading sticker" : "Load sticker"}
        onclick={() => api.ondownload(message)}>
        {#if vm.downloading}<Spinner />{:else}<Icon name="sticker" size={28} />{/if}
      </button>
    {:else if message.media_kind === "image" && (message.media_path || message.media_thumb)}
      <button
        class="media-button"
        title={message.media_path ? "View" : "Download"}
        onclick={() => (message.media_path ? api.onopenviewer(message) : api.ondownload(message))}>
        <img
          class="media"
          src={mediaSrc((message.media_path ?? message.media_thumb)!)}
          alt={message.text}
        />
        {#if !message.media_path}
          <span class="media-overlay">
            <span class="media-fetch">
              {#if vm.downloading}<Spinner />{:else}<Icon name="download" size={22} />{/if}
            </span>
          </span>
        {/if}
      </button>
    {:else if ["image", "video", "gif"].includes(message.media_kind ?? "") && !message.media_path}
      <button
        class="media-stub"
        title="Download"
        disabled={vm.downloading}
        onclick={() => api.ondownload(message)}>
        <span class="media-fetch">
          {#if vm.downloading}<Spinner />{:else}<Icon name="download" size={22} />{/if}
        </span>
        <span>{message.media_kind === "image" ? "Photo" : message.media_kind === "gif" ? "GIF" : "Video"}</span>
      </button>
    {:else if (message.media_kind === "video" || message.media_kind === "gif") &&
    (message.media_path || message.media_thumb)}
      <button
        class="media-button video"
        title={message.media_path ? "Play" : "Download"}
        onclick={() => (message.media_path ? api.onopenviewer(message) : api.ondownload(message))}>
        {#if message.media_thumb}
          <img class="media" src={mediaSrc(message.media_thumb)} alt="" />
        {/if}
        <span class="media-overlay">
          {#if message.media_kind === "gif"}GIF{:else}<span class="play">▶</span>{/if}
        </span>
      </button>
    {:else if message.media_kind === "audio" && message.media_path}
      <AudioPlayer
        path={message.media_path}
        avatar={vm.voiceAvatar}
        mine={message.from_me}
        play={vm.autoplay}
        onplayed={() => api.onmarkplayed(message)}
        onended={() => api.onnextvoice(message)}
        onpaused={() => api.onpausevoice()}
        initials={initials(message.from_me ? "You" : vm.senderText)} />
    {:else if message.media_kind === "audio"}
      <!-- Not downloaded yet: the note's own row, with the download where play will be. -->
      <button
        class="voice-pending"
        title="Download voice message"
        disabled={vm.downloading}
        onclick={() => api.ondownload(message)}>
        <span class="voice-pending-icon">
          {#if vm.downloading}<Spinner />{:else}<Icon
              name="download"
              size={18} />{/if}
        </span>
        <span class="voice-pending-bars" aria-hidden="true">
          {#each Array(34) as _, i (i)}<span style="height: {20 + ((i * 37) % 60)}%"></span>{/each}
        </span>
      </button>
    {:else if message.media_kind === "poll"}
      <PollCard
        poll={vm.poll}
        question={message.text}
        namer={api.namer}
        picture={api.avatarOf}
        onvote={async (options) => {
          await api.onvote(message, options);
        }} />
    {:else if message.media_kind === "event"}
      <EventCard
        event={vm.chatEvent}
        title={message.text}
        onopenurl={api.onopenurl}
        onrespond={async (response) => {
          await api.onrespond(message, response);
        }}
        onedit={message.from_me && vm.chatEvent
          ? () => api.oneditrequest(message)
          : undefined}
        oncancel={message.from_me && vm.chatEvent
          ? async () => {
              await api.oncancelevent(message);
            }
          : undefined} />
    {:else if isSvg(message) && message.media_path}
      <!-- An <img> never runs an SVG's scripts, so drawing it in place is safe. -->
      <span class="svg-file">
        <img class="media" src={convertFileSrc(message.media_path)} alt={message.text} />
      </span>
    {:else if message.media_kind && !DRAWN_KINDS.has(message.media_kind)}
      <!-- Kinds without a view of their own: a card with what the core could read. -->
      {@const link = message.text.match(/https?:\/\/\S+/)?.[0]}
      <Embed
        label={CARD_LABELS[message.media_kind] ?? message.media_kind}
        image={message.media_thumb ? mediaSrc(message.media_thumb) : null}
        tooltip={link}
        onopen={link ? () => api.onopenurl(link) : undefined}>
        <MessageText
          text={message.text}
          mine={message.from_me}
          toWire={api.toWire}
          targetOf={api.targetOf}
          avatarOf={api.avatarOf}
          onprofile={api.onprofile}
          onopenurl={api.onopenurl} />
      </Embed>
    {:else if message.media_kind && (message.media_path || message.media_thumb)}
      <button
        class="file"
        onclick={() => message.media_path && api.onopenmedia(message.media_path)}>
        <Icon name="file" size={20} />
        <span>{message.text || message.media_kind}</span>
      </button>
    {:else}
      <MessageText
        text={message.text}
        mine={message.from_me}
        toWire={api.toWire}
        targetOf={api.targetOf}
        avatarOf={api.avatarOf}
        onprofile={api.onprofile}
        onopenurl={api.onopenurl} />
    {/if}

    {#if vm.caption}
      <MessageText
        text={vm.caption}
        mine={message.from_me}
        toWire={api.toWire}
        targetOf={api.targetOf}
        avatarOf={api.avatarOf}
        onprofile={api.onprofile}
        onopenurl={api.onopenurl} />
    {/if}

    {#if message.media_kind === "document" && !message.media_path && !vm.viewOnce}
      <button class="download" onclick={() => api.ondownload(message)}>
        <Icon name="download" size={14} />
        Download {message.media_kind}
      </button>
    {/if}

    {@const invite = inviteLink(message.text)}
    {#if invite}
      <InviteCard
        link={invite}
        onopen={(jid) => api.oninviteopen(jid)} />
    {:else if message.preview_url}
      {@const url = message.preview_url}
      {@const provider = message.preview_site?.trim() || hostOf(url)}
      {@const title = message.preview_title?.trim() !== provider ? message.preview_title?.trim() : null}
      {@const desc = message.preview_desc?.trim() !== url ? message.preview_desc?.trim() : null}
      <Embed
        label={provider}
        {title}
        text={desc}
        image={message.preview_thumb ? mediaSrc(message.preview_thumb) : null}
        color={message.preview_color}
        tooltip={url}
        onopen={() => api.onopenurl(url)} />
    {/if}
  {/if}

  <button
    class="reply-btn"
    title="Message options"
    aria-label="Message options"
    onclick={(e) => api.onreplymenu(e, message)}><Icon name="chevronDown" size={16} /></button
  >
  <span class="meta">
    {#if vm.isStarred}<span class="star"><Icon name="star" size={11} /></span>{/if}
    {#if vm.isEdited}<span class="edited-mark">Edited</span>{/if}
    {api.formatTime(message.timestamp)}
    {#if message.from_me}
      <span
        class="ticks"
        class:read={message.status === "read"}
        title={message.status ?? "pending"}
        >{#if message.status === "pending"}<Icon name="clock" size={11} />{:else}{statusMark(
            message.status,
          )}{/if}</span
      >
    {/if}
  </span>
  {#if vm.reactions}
    {@const mine = vm.reactions.find((r) => r.mine)?.emoji ?? null}
    <button
      class="reactions"
      title={mine ? "Tap to remove your reaction" : "Reactions"}
      onclick={() => mine && api.onreact(message, "")}>
      {#each vm.reactions.slice(0, 3) as r (r.emoji)}<span>{r.emoji}</span>{/each}
      {#if vm.reactions.reduce((n, r) => n + r.count, 0) > 1}
        <span class="reaction-count">{vm.reactions.reduce((n, r) => n + r.count, 0)}</span>
      {/if}
    </button>
  {/if}
</div>
</div>

<style>
  .msg-row {
    display: flex;
    flex-direction: column;
    padding: 1px var(--pad-l) 1px var(--pad-r);
    transition: background-color calc(0.6s * var(--motion-scale)) var(--ease);
  }
  .msg-row:hover {
    background: var(--row-hover);
    transition-duration: calc(0.15s * var(--motion-scale));
  }
  /* The message a reply is being drafted to. */
  .msg-row.replying {
    background: var(--replying-soft);
    box-shadow: inset 3px 0 0 var(--replying);
  }
  /* Mentions of us and replies to us, as Discord marks them. */
  .msg-row.for-me {
    background: var(--mention-soft);
    box-shadow: inset 3px 0 0 var(--mention);
  }
  .msg-row.for-me:hover {
    background: color-mix(in srgb, var(--mention-soft), var(--row-hover));
  }
  /* The message a quote or mention jump landed on. */
  .msg-row.jumped {
    background: var(--jump-soft);
    transition-duration: calc(0.15s * var(--motion-scale));
  }
  .bubble {
    max-width: 70%;
    /* Without this a flex item refuses to shrink below its content, so a large
       image stretches the bubble instead of being scaled down to fit it. */
    min-width: 0;
    /* The message list is a flex column, so bubbles shrink by default. With a
       few hundred of them they squash to one line and `overflow: hidden` clips
       the text away, which looks like every message collapsing. */
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
  /* The tail marks the first bubble of a run from one sender. */
  .bubble.first:not(.mine) {
    border-top-left-radius: 0;
  }
  .bubble.first.mine {
    border-top-right-radius: 0;
  }
  /* 1px wider than it shows and tucked into the bubble, so no seam renders
     where the two meet. */
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
  .bubble.media-only {
    padding: 3px;
  }
  .bubble.media-only .media,
  .bubble.media-only .media-button.video {
    border-radius: calc(var(--radius-sm) - 2px);
  }
  .bubble.inline-meta .meta {
    position: absolute;
    right: 7px;
    bottom: 4px;
  }
  /* The time sits on the picture instead of below it. */
  .bubble.media-only .meta {
    position: absolute;
    right: 9px;
    bottom: 8px;
    padding: 1px 7px;
    border-radius: 999px;
    background: rgba(6, 8, 10, 0.55);
    color: #eef0f2;
  }
  .edited-mark {
    font-style: italic;
  }
  .forwarded-mark {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 2px;
    color: var(--muted);
    font-size: 12.5px;
    font-style: italic;
  }
  .member-label {
    margin-top: -4px;
    font-size: 12px;
    color: var(--muted);
  }
  .revoked {
    font-style: italic;
    color: var(--faint);
  }
  .media {
    /* Cap both axes: width keeps it inside the bubble, height stops a tall
       photo from filling the viewport. */
    max-width: 100%;
    max-height: 320px;
    width: auto;
    height: auto;
    object-fit: contain;
    border-radius: 6px;
    display: block;
  }
  .voice-pending {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 280px;
    max-width: 100%;
    padding: 6px 2px;
    border: 0;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .voice-pending:disabled {
    cursor: progress;
  }
  .voice-pending-icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    flex: none;
    border-radius: 50%;
    border: 2px solid var(--accent);
    color: var(--accent);
  }
  .voice-pending:hover:not(:disabled) .voice-pending-icon {
    background: var(--accent-soft);
  }
  .voice-pending-bars {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 24px;
  }
  .voice-pending-bars span {
    flex: 1;
    border-radius: 2px;
    background: color-mix(in srgb, var(--text) 22%, transparent);
  }
  .once {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 200px;
    padding: 6px 4px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .once > span:last-child {
    display: flex;
    flex-direction: column;
  }
  .once small {
    color: var(--muted);
    font-size: 12px;
  }
  .once.spent {
    cursor: default;
    color: var(--muted);
  }
  .once-mark {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    flex: none;
    border: 2px dashed var(--accent);
    border-radius: 50%;
    color: var(--accent);
    font-size: 12px;
    font-weight: 700;
  }
  .once.spent .once-mark {
    border-color: var(--faint);
    color: var(--faint);
  }
  .once-done {
    align-self: flex-end;
    padding: 4px 12px;
    border: 0;
    border-radius: 999px;
    background: var(--accent-soft);
    color: var(--accent-text);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    background: rgba(0, 0, 0, 0.18);
    border: 0;
    border-radius: var(--radius-sm);
    padding: 8px 12px 8px 10px;
    font: inherit;
    color: var(--text);
    cursor: pointer;
    text-align: left;
  }
  .file:hover {
    background: rgba(0, 0, 0, 0.28);
  }
  .file :global(svg) {
    color: var(--accent-text);
  }
  /* Media opens in the system viewer, so the whole preview is the button. */
  .media-button {
    position: relative;
    display: block;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: zoom-in;
    line-height: 0;
  }
  .media-button.video {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 14px 16px;
    cursor: pointer;
    background: var(--bg);
    border-radius: 6px;
    min-width: 180px;
    min-height: 100px;
  }
  .media-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text);
    font-size: 14px;
    font-weight: 700;
    letter-spacing: 1px;
    text-shadow: 0 1px 6px rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }
  .media-overlay .play {
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    padding-left: 3px;
    box-sizing: border-box;
    border-radius: 999px;
    background: rgba(6, 8, 10, 0.6);
    font-size: 16px;
    text-shadow: none;
  }
  .media-fetch {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border-radius: 50%;
    background: var(--scrim);
    color: #fff;
    font-size: 0;
    text-shadow: none;
  }
  .media-stub {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 260px;
    max-width: 100%;
    aspect-ratio: 4 / 3;
    border: 0;
    border-radius: 8px;
    background: linear-gradient(135deg, var(--raised), var(--raised-2));
    color: var(--muted);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }
  .media-stub:hover .media-fetch {
    background: var(--accent);
  }
  .svg-file .media {
    width: 280px;
    max-width: 100%;
    max-height: 320px;
    object-fit: contain;
    padding: 8px;
    box-sizing: border-box;
    background: repeating-conic-gradient(var(--raised) 0 25%, var(--raised-2) 0 50%) 0 0 / 16px 16px;
  }
  .download {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    background: rgba(0, 0, 0, 0.2);
    border: 0;
    border-radius: 999px;
    color: var(--accent-text);
    font: inherit;
    font-size: 12px;
    padding: 4px 12px 4px 10px;
    cursor: pointer;
  }
  .download:hover {
    background: rgba(0, 0, 0, 0.32);
  }
  .quote-where {
    flex: none;
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: #71717a;
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
  .reply-btn {
    position: absolute;
    top: 3px;
    right: 3px;
    z-index: 1;
    display: flex;
    padding: 3px;
    background: inherit;
    border: 0;
    border-radius: 999px;
    color: var(--muted);
    cursor: pointer;
    opacity: 0;
  }
  .bubble.mine .reply-btn {
    background: var(--bubble-mine);
  }
  .bubble:not(.mine) .reply-btn {
    background: var(--bubble);
  }
  .bubble:hover .reply-btn,
  .bubble.menu-open .reply-btn,
  .reply-btn:focus-visible {
    opacity: 1;
  }
  .bubble.has-reactions {
    margin-bottom: 16px;
  }
  .sticker {
    width: 160px;
    height: 160px;
    object-fit: contain;
    display: block;
  }
  .sticker-pending {
    display: grid;
    place-items: center;
    border: 0;
    border-radius: 18px;
    color: var(--faint);
    cursor: pointer;
    background: linear-gradient(100deg, var(--surface) 40%, var(--raised) 50%, var(--surface) 60%) 0 0 / 300% 100%;
    animation: shimmer 1.4s linear infinite;
  }
  /* Stickers float free of a bubble, as in WhatsApp. */
  .bubble.sticker-only {
    background: transparent;
    box-shadow: none;
    padding: 0;
  }
  .bubble.sticker-only::before {
    display: none;
  }
  .bubble.sticker-only .meta {
    align-self: flex-end;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--surface);
  }
  .star {
    display: inline-flex;
  }
  .ticks {
    font-size: 10px;
    letter-spacing: -2px;
  }
  .ticks.read {
    color: var(--link);
  }
  /* WhatsApp's reaction pill, hanging off the bubble's bottom edge. */
  .reactions {
    position: absolute;
    bottom: -16px;
    left: 8px;
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 2px 6px;
    border: 1px solid var(--chat-bg);
    border-radius: 999px;
    background: var(--surface);
    font: inherit;
    font-size: 13px;
    line-height: 18px;
    color: var(--muted);
    cursor: pointer;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
  }
  .bubble.mine .reactions {
    left: auto;
    right: 8px;
  }
  .reaction-count {
    margin-left: 3px;
    font-size: 12px;
  }
  @keyframes shimmer {
    to {
      background-position: -150% 0;
    }
  }
</style>
