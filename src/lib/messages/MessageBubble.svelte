<script lang="ts">
  // One conversation message: the row, the bubble and every media branch.
  // The list computes the view model; every action funnels back through the
  // api so the page keeps owning state and IPC.
  import { convertFileSrc } from "@tauri-apps/api/core";
  import AudioPlayer from "$lib/media/AudioPlayer.svelte";
  import Embed from "$lib/messages/cards/Embed.svelte";
  import LocationCard from "$lib/messages/cards/LocationCard.svelte";
  import EventCard from "$lib/messages/cards/EventCard.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import InviteCard, { inviteLink } from "$lib/messages/cards/InviteCard.svelte";
  import MessageText from "$lib/messages/MessageText.svelte";
  import PollCard from "$lib/messages/cards/PollCard.svelte";
  import Spinner from "$lib/ui/Spinner.svelte";
import VideoPlayer from "$lib/media/VideoPlayer.svelte";
  import Avatar from "$lib/ui/Avatar.svelte";
  import { initials } from "$lib/utils/avatar";
  import { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import {
    CARD_LABELS,
    isSvg,
    DRAWN_KINDS,
    VIEW_ONCE_LABEL,
    replyIcon,
    statusMark,
  } from "$lib/utils/message";
  import type { BubbleApi, BubbleVm, StoredMessage } from "$lib/utils/models";

  let { message, vm, api }: { message: StoredMessage; vm: BubbleVm; api: BubbleApi } = $props();

  /** A sticker file the renderer cannot draw, such as a Lottie sticker. */
  let stickerBroken = $state(false);

  /** The media kind a downloaded file's extension implies. */
  function kindOfFile(path: string) {
    if (/\.(ogg|opus|mp3|m4a|aac|wav)$/i.test(path)) return "audio";
    if (/\.(mp4|mov|m4v|webm|mkv)$/i.test(path)) return "video";
    return "image";
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
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div
  class="msg-row"
  class:replying={vm.isReplying}
  class:jumped={vm.highlighted}
  class:for-me={vm.forMe}
  class:first-row={vm.first}
  class:mine={message.from_me}
  class:picking={vm.picking}
  class:picked={vm.picked}
  onclick={vm.picking ? () => api.onpick(message) : undefined}
  ondblclick={() => {
    if (!vm.picking) api.onreplydraft(message);
  }}
  oncontextmenu={(e) => {
    e.preventDefault();
    if (!vm.picking) api.onmenu(e, message);
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
  class:deleted={message.deleted || message.revoked}
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

  {#if message.revoked && !vm.hasBody}
    <!-- Nothing local to keep: only a revoke we heard about, never the message. -->
    <span class="revoked">This message was deleted<span class="meta-spacer"></span></span>
  {:else}
    {#if vm.isForwarded}
      <span class="forwarded-mark"><Icon name="forward" size={13} /> Forwarded</span>
    {/if}
    {#if message.reply_to_text}
      <!-- A reply sent from this app never carries the view-once it quotes; one from the phone,
           though also ours, does, and is told apart by holding the copy. -->
      {@const sentHere =
        message.from_me && message.reply_to_view_once && !message.reply_to_recoverable && !message.reply_to_path}
      {@const onceCopy = message.reply_to_view_once && message.reply_to_recoverable && !sentHere}
      <Embed
        compact
        tooltip={message.reply_to_path
          ? "Open the copy"
          : onceCopy
            ? "Save the copy"
            : sentHere
              ? "Only a reply sent from your phone, or by someone else, carries a copy of a view-once"
              : "Go to message"}
        label={vm.quoteAuthor}
        text={vm.quoteText}
        image={message.reply_to_thumb && ["image", "sticker", "gif"].includes(message.reply_to_kind ?? "")
          ? mediaSrc(message.reply_to_thumb)
          : null}
        icon={message.reply_to_kind ? replyIcon(message.reply_to_kind) : null}
        onclick={message.reply_to_path
          ? () => api.onopenquote(message)
          : onceCopy
            ? () => api.onrecoverquote(message)
            : () => api.onjumpquoted(message)}>
        {#if vm.quoteChatName}
          <span class="quote-where">in {vm.quoteChatName}</span>
        {/if}
        {#if onceCopy && !message.reply_to_path}
          <span class="quote-once">
            <span class="once-mark">1</span>
            {api.recovering[message.id] ? "Saving the copy…" : "Tap to save"}
          </span>
        {:else if sentHere}
          <span class="quote-once">
            <span class="once-mark">1</span>
            No copy from this app
          </span>
        {/if}
      </Embed>
    {/if}

    {#if vm.viewOnce && !message.media_path}
      {@const what = VIEW_ONCE_LABEL[message.media_kind ?? ""] ?? "View once message"}
      {#if vm.viewOnce.opened && !vm.viewOnce.available}
        <!-- Nothing arrived, or this account sent it and the sender cannot
             reopen it, and no reply carried a copy. There is nothing left to ask anyone for. -->
        <span class="once spent">
          <span class="once-mark">1</span>
          <span>{what}<small>Open it on your phone</small></span>
        </span>
      {:else}
        <button
          class="once"
          onclick={() => api.ononce(message)}>
          <span class="once-mark">1</span>
          <span>{what}<small>{vm.downloading
              ? "Asking for it…"
              : vm.viewOnce.available
                ? "Open"
                : "Tap to view"}</small></span>
        </button>
      {/if}
    {:else if vm.viewOnce && message.media_path}
      <!-- Shown where it sits, as an ordinary photo would be: the only copy is
           the one a reply carried, and it is ours to keep. -->
      <!-- Copies taken before the kind was recorded are told apart by their file. -->
      {@const onceKind = message.media_once_kind ?? kindOfFile(message.media_path)}
      {#if onceKind === "video" || onceKind === "gif"}
        <VideoPlayer src={convertFileSrc(message.media_path)} path={message.media_path} autoplay={false} />
      {:else if onceKind === "audio"}
        <AudioPlayer
          path={message.media_path}
          duration={message.media_duration}
          avatar={vm.voiceAvatar}
          title={message.from_me ? "You" : vm.senderText}
          initials={initials(message.from_me ? "You" : vm.senderText)} />
      {:else if onceKind === "sticker"}
        {#if stickerBroken}
          <span class="sticker sticker-unsupported" title="Unsupported sticker">Unsupported sticker</span>
        {:else}
          <img class="sticker" src={convertFileSrc(message.media_path)} alt="Sticker" onerror={() => (stickerBroken = true)} />
        {/if}
      {:else}
        <img class="media" src={convertFileSrc(message.media_path)} alt={message.text} />
      {/if}
    {:else if message.media_kind === "sticker" && message.media_path}
      {#if stickerBroken}
        <span class="sticker sticker-unsupported" title="Unsupported sticker">Unsupported sticker</span>
      {:else}
        <img class="sticker" src={convertFileSrc(message.media_path)} alt="Sticker" onerror={() => (stickerBroken = true)} />
      {/if}
    {:else if message.media_kind === "sticker"}
      <!-- Fetched on its own when shown; the placeholder keeps the sticker's space. -->
      <button
        class="sticker sticker-pending"
        title={vm.downloading ? "Loading sticker" : "Load sticker"}
        onclick={() => api.ondownload(message)}>
        {#if vm.downloading}<Spinner />{:else}<Icon name="sticker" size={28} />{/if}
      </button>
    {:else if message.media_kind === "image" && (message.media_path || message.media_thumb)}
      {@const filtered = vm.onceKept && !vm.onceRevealed}
      <button
        class="media-button"
        class:once-kept={filtered}
        title={filtered ? "One-time photo, click to reveal" : message.media_path ? "View" : "Download"}
        onclick={() =>
          filtered
            ? api.onrevealonce(message)
            : message.media_path
              ? api.onopenviewer(message)
              : api.ondownload(message)}>
        <img
          class="media"
          src={mediaSrc((message.media_path ?? message.media_thumb)!)}
          alt={message.text}
        />
        {#if filtered}
          <span class="media-overlay once-overlay">
            <span class="once-mark">1</span>
            <span>One-time photo<small>Click to reveal</small></span>
          </span>
        {:else if !message.media_path}
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
      {@const filtered = vm.onceKept && !vm.onceRevealed}
      <button
        class="media-button video"
        class:once-kept={filtered}
        title={filtered
          ? "One-time video, click to reveal"
          : message.media_path
            ? "Play"
            : "Download"}
        onclick={() =>
          filtered
            ? api.onrevealonce(message)
            : message.media_path
              ? api.onopenviewer(message)
              : api.ondownload(message)}>
        {#if message.media_thumb}
          <img class="media" src={mediaSrc(message.media_thumb)} alt="" />
        {:else}
          <!-- No stored thumbnail: draw the file's own first frame. -->
          <video class="media" src={convertFileSrc(message.media_path!)} preload="metadata" muted playsinline></video>
        {/if}
        {#if filtered}
          <span class="media-overlay once-overlay">
            <span class="once-mark">1</span>
            <span>{message.media_kind === "gif" ? "One-time GIF" : "One-time video"}<small
                >Click to reveal</small></span>
          </span>
        {:else}
          <span class="media-overlay">
            {#if message.media_kind === "gif"}GIF{:else}<span class="play">▶</span>{/if}
          </span>
        {/if}
      </button>
    {:else if message.media_kind === "audio" && message.media_path}
      {#if vm.onceKept && !vm.onceRevealed}
        <button
          class="voice-pending"
          title="One-time voice message, click to reveal"
          onclick={() => api.onrevealonce(message)}>
          <span class="voice-pending-icon"><span class="once-mark">1</span></span>
          <span class="voice-pending-bars" aria-hidden="true">
            {#each Array(34) as _, i (i)}<span style="height: {20 + ((i * 37) % 60)}%"></span>{/each}
          </span>
        </button>
      {:else}
        <AudioPlayer
          path={message.media_path}
          duration={message.media_duration}
          avatar={vm.voiceAvatar}
          mine={message.from_me}
          play={vm.autoplay}
          chained={vm.autoplay}
          title={message.from_me ? "You" : vm.senderText}
          onplayed={() => api.onmarkplayed(message)}
          onended={() => api.onnextvoice(message)}
          onpaused={() => api.onpausevoice()}
          initials={initials(message.from_me ? "You" : vm.senderText)} />
      {/if}
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
    {:else if message.media_kind === "live_location" && message.live_location}
      <LocationCard {message} {api} />
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

    {#if vm.downloadError && !vm.downloading}
      <button
        class="download-failed"
        title={vm.downloadError}
        disabled={vm.downloadGaveUp}
        onclick={() => api.ondownload(message)}>
        {#if vm.downloadGaveUp}
          Download failed · retry limit reached
        {:else}
          <Icon name="repeat" size={14} />
          Couldn't download · Retry
        {/if}
      </button>
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
    <button
      class="reactions"
      title="Show reactions"
      aria-label="Show reactions"
      onclick={() => api.onopenreactions(message)}>
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
  /* Bulk selection: the row picks, the bubble stops swallowing clicks. */
  .msg-row.picking {
    position: relative;
    cursor: pointer;
    padding-left: calc(var(--pad-l) + 30px);
  }
  .msg-row.picking .bubble {
    pointer-events: none;
  }
  .msg-row.picking::after {
    content: "";
    position: absolute;
    top: 50%;
    /* Clear of the 38px the sender avatar hangs left of its bubble. */
    left: 8px;
    width: 20px;
    height: 20px;
    margin-top: -10px;
    box-sizing: border-box;
    border: 2px solid var(--faint);
    border-radius: 50%;
  }
  .msg-row.picking.picked::after {
    content: "✓";
    display: grid;
    place-items: center;
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-text);
    font-size: 12px;
    font-weight: 700;
    line-height: 1;
  }
  .msg-row.picking:hover {
    background: var(--row-hover);
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
  /* Deleted locally: the row stays, greyed, and warms to red under the pointer. */
  .bubble.deleted {
    filter: grayscale(1);
    opacity: 0.55;
  }
  .bubble.deleted:hover {
    filter: none;
    opacity: 1;
    background: color-mix(in srgb, var(--danger) 18%, var(--bubble));
    box-shadow: inset 0 0 0 1px var(--danger);
  }
  .bubble.mine.deleted:hover {
    background: color-mix(in srgb, var(--danger) 18%, var(--bubble-mine));
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
    /* The media button zeroes line-height so the image has no baseline gap;
       the overlay, which can hold two lines, has to restore it. */
    line-height: 1.3;
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
  /* A kept one-time copy sits behind its filter until clicked once. */
  .media-button.once-kept {
    cursor: pointer;
  }
  .media-button.once-kept .media {
    filter: blur(14px);
  }
  .once-overlay {
    flex-direction: column;
    gap: 8px;
    background: rgba(6, 8, 10, 0.35);
    font-size: 13px;
    font-weight: 600;
    letter-spacing: 0;
  }
  .once-overlay span:last-child {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
  }
  .once-overlay small {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--muted);
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
  .download-failed {
    align-self: flex-start;
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
    background: var(--danger-soft);
    border: 0;
    border-radius: 999px;
    color: var(--danger);
    font: inherit;
    font-size: 12px;
    padding: 4px 12px 4px 10px;
    cursor: pointer;
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
  .download-failed:disabled {
    cursor: default;
  }
  .quote-once {
    display: flex;
    flex: none;
    align-items: center;
    gap: 4px;
    margin-left: auto;
    font-size: 11px;
    color: color-mix(in srgb, var(--text) 60%, transparent);
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
  /* A sticker file the renderer cannot draw, drawn as a static stand-in. */
  .sticker-unsupported {
    display: grid;
    place-items: center;
    padding: 8px;
    box-sizing: border-box;
    border-radius: 18px;
    border: 2px dashed var(--faint);
    background: var(--surface);
    color: var(--muted);
    font-size: 13px;
    font-weight: 600;
    text-align: center;
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
