<!-- One message row. The view model is derived here, not in the list, so a
     list update that only replaced another row leaves this bubble alone. -->
<script lang="ts">
  import MessageBubble from "$lib/messages/MessageBubble.svelte";
  import { MAX_DOWNLOAD_TRIES } from "$lib/state/messages.svelte";
  import { bare } from "$lib/utils/message";
  import type { BubbleApi, BubbleCtx, BubbleVm, StoredMessage } from "$lib/utils/models";

  let {
    message,
    prev,
    ctx,
    api,
    albumCell = false,
  }: {
    message: StoredMessage;
    prev: StoredMessage | undefined;
    ctx: BubbleCtx;
    api: BubbleApi;
    albumCell?: boolean;
  } = $props();

  const vm = $derived(buildVm(message, prev, ctx));

  function buildVm(message: StoredMessage, prev: StoredMessage | undefined, ctx: BubbleCtx): BubbleVm {
    const newDay = !prev || ctx.dayKey(prev.timestamp) !== ctx.dayKey(message.timestamp);
    const first = newDay || prev.from_me !== message.from_me || prev.sender !== message.sender;
    const mark = ctx.viewOnceMarks.find((v) => v.id === message.id) ?? null;
    // A stub is spent unless a reply carried a copy of it, which the mark knows.
    const viewOnce =
      message.media_kind === "view_once"
        ? { id: message.id, opened: true, available: mark?.available ?? false }
        : mark;
    // A copy the Android companion kept: an ordinary kind plus the once marker,
    // behind a one-time filter until it is clicked in this visit to the chat.
    const onceKept =
      !!message.media_once_kind && message.media_kind !== "view_once" && !!message.media_path;
    // A revoked message keeps its local copy, so it still counts as drawable.
    const hasBody =
      !!message.text.trim() ||
      !!message.media_kind ||
      !!message.media_path ||
      !!message.media_thumb ||
      !!message.reply_to_text;
    const visual =
      !viewOnce &&
      (message.media_kind === "image" ||
        message.media_kind === "video" ||
        message.media_kind === "round_video" ||
        message.media_kind === "gif") &&
      !!(message.media_path || message.media_thumb);
    const caption = visual ? ctx.captionOf(message) : "";
    const showSender = first && !message.from_me && ctx.isGroup;
    const senderText = ctx.senderLabel(message);
    return {
      first,
      showSender,
      senderText,
      senderHue: ctx.hue(message.sender),
      senderAvatar: ctx.avatars[bare(message.sender)] ?? null,
      memberTag: ctx.memberTagOf(message.sender),
      visual,
      caption,
      viewOnce,
      onceKept,
      onceRevealed: !!ctx.revealedOnce[message.id],
      inlineMeta:
        (message.revoked && !hasBody) ||
        (!message.preview_url && (!message.media_kind || (!!caption && !!message.media_path))),
      reactions: ctx.reactionsFor.get(message.id),
      isStarred: ctx.starredSet.has(message.id),
      isEdited: ctx.editedSet.has(message.id),
      isForwarded: ctx.forwardedSet.has(message.id),
      isReplying: ctx.replyingToId === message.id,
      highlighted: ctx.highlightedId === message.id,
      forMe: !message.from_me && (message.mentioned || message.reply_to_sender === "@me"),
      menuOpen: ctx.menuId === message.id,
      poll: ctx.polls.find((p) => p.id === message.id),
      chatEvent: ctx.events.find((e) => e.id === message.id),
      downloading: !!ctx.downloading[message.id],
      downloadError: message.media_path ? null : (ctx.downloadErrors[message.id] ?? null),
      downloadGaveUp: (ctx.downloadTries[message.id] ?? 0) >= MAX_DOWNLOAD_TRIES,
      hasBody,
      picking: !!ctx.picking && !message.revoked,
      picked: !!ctx.picking?.[message.id],
      onceAudioOpen: ctx.onceAudioOpenId === message.id,
      autoplay: ctx.autoplayId === message.id,
      voiceAvatar: ctx.voiceAvatarOf(message),
      quoteAuthor: message.reply_to_text ? ctx.quoteAuthorOf(message.reply_to_sender) : null,
      quoteText: ctx.quoteTextOf(message),
      quoteChatName: ctx.quoteChatNameOf(message),
    };
  }
</script>

<MessageBubble {message} {vm} {api} {albumCell} />
