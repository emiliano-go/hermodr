<script lang="ts">
  import type { Snippet } from "svelte";
  import Embed from "$lib/messages/cards/Embed.svelte";
  import PollCard from "$lib/messages/cards/PollCard.svelte";
  import EventCard from "$lib/messages/cards/EventCard.svelte";
  import LocationCard from "$lib/messages/cards/LocationCard.svelte";
  import InviteCard, { inviteLink } from "$lib/messages/cards/InviteCard.svelte";
  import MessageText from "$lib/messages/MessageText.svelte";
  import ContactCard from "$lib/messages/cards/ContactCard.svelte";
  import { session } from "$lib/state/session.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import { CARD_LABELS, replyIcon } from "$lib/utils/message";
  import type { BubbleApi, BubbleVm, Poll, StoredMessage } from "$lib/utils/models";

  let props: {
    message: StoredMessage;
    vm: BubbleVm;
    api: BubbleApi;
    spoilerRevealed?: boolean;
    variant?: "message" | "quote" | "link";
    meta?: Snippet;
  } | {
    variant: "poll";
    poll?: Poll;
    question: string;
    namer: (jid: string) => string;
    picture: (jid: string) => string | null;
    onvote: (options: string[]) => Promise<void>;
  } = $props();

  function hostOf(url: string) {
    try {
      return new URL(url).hostname;
    } catch {
      return url;
    }
  }
</script>

{#if props.variant === "poll"}
  <PollCard poll={props.poll} question={props.question} namer={props.namer} picture={props.picture} onvote={props.onvote} />
{:else}
{@const { message, vm, api, variant = "message", meta, spoilerRevealed = false } = props}
{#if variant === "quote"}
  {@const sentHere = message.from_me && message.reply_to_view_once && !message.reply_to_recoverable && !message.reply_to_path}
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
    {#if vm.quoteChatName}<span class="quote-where">in {vm.quoteChatName}</span>{/if}
    {#if onceCopy && !message.reply_to_path}
      <span class="quote-once"><span class="once-mark">1</span>{api.recovering[message.id] ? "Saving the copy…" : "Tap to save"}</span>
    {:else if sentHere}
      <span class="quote-once"><span class="once-mark">1</span>No copy from this app</span>
    {/if}
  </Embed>
{:else if variant === "link"}
  {@const invite = inviteLink(message.text)}
  {#if invite || message.media_kind === "group_invite"}
    <InviteCard
      link={invite}
      message={message.media_kind === "group_invite" ? message : null}
      onjoin={() => api.oninvitejoin(message, invite)}
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
{:else if message.media_kind === "contact"}
  <ContactCard {message} account={session.activeAccount} generation={messages.accountGeneration} {spoilerRevealed}
    onopenchat={api.onopenchat} meta={vm.inlineMeta ? meta : undefined} />
{:else if message.media_kind === "poll"}
  <PollCard
    poll={vm.poll}
    question={message.text}
    namer={api.namer}
    picture={api.avatarOf}
    onvote={async (options) => { await api.onvote(message, options); }} />
{:else if message.media_kind === "event"}
  <EventCard
    event={vm.chatEvent}
    title={message.text}
    onopenurl={api.onopenurl}
    onrespond={async (response) => { await api.onrespond(message, response); }}
    onedit={message.from_me && vm.chatEvent ? () => api.oneditrequest(message) : undefined}
    oncancel={message.from_me && vm.chatEvent ? async () => { await api.oncancelevent(message); } : undefined} />
{:else if message.media_kind === "live_location" && message.live_location}
  <LocationCard {message} {api} />
{:else}
  {@const link = message.text.match(/https?:\/\/\S+/)?.[0]}
  <Embed
    label={CARD_LABELS[message.media_kind ?? "unknown"] ?? message.media_kind}
    image={message.media_thumb ? mediaSrc(message.media_thumb) : null}
    tooltip={link}
    onopen={link ? () => api.onopenurl(link) : undefined}>
    <MessageText
      text={message.text}
      mine={message.from_me}
      meta={vm.inlineMeta ? meta : undefined}
      toWire={api.toWire}
      targetOf={api.targetOf}
      avatarOf={api.avatarOf}
      onprofile={api.onprofile}
      onopenurl={api.onopenurl} />
  </Embed>
{/if}
{/if}

<style>
  .quote-where {
    flex: none;
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: #71717a;
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
</style>
