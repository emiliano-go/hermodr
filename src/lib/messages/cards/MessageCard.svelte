<script lang="ts">
  import type { Snippet } from "svelte";
  import Embed from "$lib/messages/cards/Embed.svelte";
  import MessageQuote from "$lib/messages/cards/MessageQuote.svelte";
  import PollCard from "$lib/messages/cards/PollCard.svelte";
  import EventCard from "$lib/messages/cards/EventCard.svelte";
  import LocationCard from "$lib/messages/cards/LocationCard.svelte";
  import InviteCard, { inviteLink } from "$lib/messages/cards/InviteCard.svelte";
  import MessageText from "$lib/messages/MessageText.svelte";
  import ContactCard from "$lib/messages/cards/ContactCard.svelte";
  import { session } from "$lib/state/session.svelte";
  import { messages } from "$lib/state/messages.svelte";
  import { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import { CARD_LABELS } from "$lib/utils/message";
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
  <MessageQuote {message} {api} author={vm.quoteAuthor} text={vm.quoteText} chatName={vm.quoteChatName} />
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
    scope={{ account: session.activeAccount, chat: message.chat, generation: messages.accountGeneration, requestKey: message.id }}
    question={message.text}
    namer={api.namer}
    picture={api.avatarOf}
    onvote={async (options) => { await api.onvote(message, options); }} />
{:else if message.media_kind === "event"}
  <EventCard
    chat={message.chat}
    scope={{ account: session.activeAccount, chat: message.chat, generation: messages.accountGeneration, requestKey: message.id }}
    event={vm.chatEvent}
    names={api.namer}
    picture={api.avatarOf}
    pinned={!!vm.chatEvent?.pinned}
    title={message.text}
    onopenurl={api.onopenurl}
    onrespond={async (response, extraGuestCount) => { await api.onrespond(message, response, extraGuestCount); }}
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
