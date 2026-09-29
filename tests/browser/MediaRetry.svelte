<script lang="ts">
  import { onMount } from "svelte";
  import MessageBubble from "$lib/messages/MessageBubble.svelte";
  import { MessagesState, MAX_DOWNLOAD_TRIES } from "$lib/state/messages.svelte";
  import type { BubbleApi, BubbleVm, StoredMessage } from "$lib/utils/models";
  import { mediaFixture, windowFixture } from "./ipc";
  const pager = new MessagesState();
  const seed = { chat: "media@s", id: "retry-fixture", sender: "sender@s", text: "Synthetic caption", timestamp: 100, from_me: false,
    media_kind: "image", media_path: null } as StoredMessage;
  let calls = $state(0);
  let message = $derived(pager.messages[0] ?? seed);
  let vm = $derived({ reactions: [], visual: true, caption: message.text, inlineMeta: true,
    downloading: pager.downloading[message.id], downloadError: message.media_path ? null : pager.downloadErrors[message.id],
    downloadGaveUp: (pager.downloadTries[message.id] ?? 0) >= MAX_DOWNLOAD_TRIES } as unknown as BubbleVm);
  async function download() { await pager.downloadMedia(seed.chat, message, true); calls = mediaFixture.calls; }
  const api = { ondownload: download, formatTime: () => "12:00", toWire: () => "sender@s", targetOf: () => null,
    avatarOf: () => null, namer: () => "Synthetic sender", onmenu: () => {}, onreplydraft: () => {}, onopenviewer: () => {} } as unknown as BubbleApi;
  async function reset() {
    pager.resetAccount();
    Object.assign(mediaFixture, { calls: 0, failure: true, deferNext: false, pending: [] });
    windowFixture.archive = windowFixture.archive.filter((m) => m.chat !== seed.chat);
    windowFixture.archive.push({ ...seed });
    pager.prepareChat(seed.chat, 100);
    await pager.reloadMessages(seed.chat);
    calls = 0;
  }
  onMount(() => { void reset(); return () => pager.resetAccount(); });
</script>

<details>
  <summary>Media retry fixture</summary>
  <button onclick={reset}>Reset media fixture</button>
  <button onclick={() => { mediaFixture.failure = false; }}>Allow synthetic download</button>
  <button onclick={download}>Download synthetic attachment</button>
  <p role="status">Download calls: {calls}; stored file: {message.media_path ? "yes" : "no"}</p>
  <MessageBubble {message} {vm} {api} />
</details>
