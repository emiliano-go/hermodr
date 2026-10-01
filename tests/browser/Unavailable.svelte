<script module lang="ts">
  import MessageList from "$lib/messages/MessageList.svelte";
  import type { ComponentProps } from "svelte";
  import { bubbleApi, messages as samples, noop } from "$lib/utils/theme-preview";
  import { dayKey, dayLabel, formatTime } from "$lib/utils/message";

  export const unavailable = { ...samples[0], chat: "unavailable@s.whatsapp.net", id: "unavailable-id",
    sender: "200@s.whatsapp.net", sender_name: "Saved sender", timestamp: 1700000000,
    system_kind: "UNAVAILABLE_MESSAGE", system_params: [], text: "PRIVATE UNDECRYPTED PAYLOAD",
    read: false, media_kind: null, media_path: null, media_thumb: null };
  export const unavailableActions: string[] = [];
  export const listProps: Omit<ComponentProps<typeof MessageList>, "messages"> = {
    ...bubbleApi, isGroup: true, switching: false, scroller: undefined, dayKey, dayLabel, formatTime,
    senderLabel: (message) => message.sender_name ?? message.sender, memberTagOf: () => null,
    hue: () => 160, captionOf: (message) => message.text, viewOnceMarks: [], reactionsFor: new Map(),
    starredSet: new Set(), editedSet: new Set(), forwardedSet: new Set(), downloading: {},
    downloadErrors: {}, downloadTries: {}, replyingToId: null, highlightedId: null,
    firstUnreadId: null, onjumpunread: noop, menuId: null, polls: [], events: [], avatars: {},
    voiceAvatarOf: () => null, quoteAuthorOf: () => "", quoteTextOf: () => null, quoteChatNameOf: () => null,
    autoplayId: null, onceAudioOpenId: null, loadingOlder: false, onloadolder: noop, uploads: [], typers: [],
    typerLabelOf: () => "", onscroll: noop, revealedOnce: {},
    onreplydraft: () => unavailableActions.push("reply"),
    onmenu: (event) => { event.preventDefault(); unavailableActions.push("menu"); },
    ondownload: () => unavailableActions.push("download"),
    onreact: () => unavailableActions.push("reaction"),
  };
</script>

<script lang="ts">
  let recovered = $state(false);
  let actions = $state("");
  const row = $derived(recovered ? { ...unavailable, system_kind: null, text: "Recovered synthetic message" } : unavailable);
</script>

<details>
  <summary>Unavailable message fixture</summary>
  <label><input type="checkbox" bind:checked={recovered} />Replace unavailable with recovered message</label>
  <button onclick={() => { actions = JSON.stringify(unavailableActions); }}>Show unavailable actions</button>
  <output aria-label="Unavailable actions">{actions}</output>
  <div class="fixture-chat"><MessageList {...listProps} messages={[row]} /></div>
</details>

<style>
  .fixture-chat { height: 350px; display: flex; background: #0b141a; --surface: #202c33; --text: #e9edef; --muted: #8696a0; --line-strong: #3b4a54; --radius: 8px; --bubble: #202c33; --bubble-mine: #005c4b; }
</style>
