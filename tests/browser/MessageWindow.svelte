<script lang="ts">
  import { MessagesState } from "$lib/state/messages.svelte";
  import { ui } from "$lib/state/ui.svelte";
  import { windowFixture } from "./ipc";
  import type { StoredMessage } from "$lib/utils/models";
  const pager = new MessagesState();
  let scroller: HTMLDivElement;
  let metrics = $state("");
  function report() {
    metrics = `RAM ${pager.messages.length}/100; oldest ${pager.messages.at(-1)?.id ?? "none"}; newest ${pager.messages[0]?.id ?? "none"}; disk ${windowFixture.archive.length}; phone requests ${windowFixture.phoneRequests}; latest ${pager.atLatest}`;
  }
  async function open(chat = "window@s") {
    pager.prepareChat(chat, 100);
    await pager.reloadMessages(chat);
    report();
  }
  async function act(action: () => Promise<unknown>) { await action(); report(); }
</script>

<details>
  <summary>Bounded message window</summary>
  <button onclick={() => open()}>Open synthetic archive</button>
  <button onclick={() => act(() => pager.loadOlder("window@s", false))}>Older page</button>
  <button onclick={() => act(() => pager.loadNewer("window@s"))}>Newer page</button>
  <button onclick={() => act(() => pager.showLatest("window@s"))}>Latest page</button>
  <button onclick={() => act(() => pager.showStoredMessage("window@s", "0005"))}>Jump to stored 0005</button>
  <button onclick={() => act(async () => {
    const n = windowFixture.archive.length;
    windowFixture.archive.push({ chat: "window@s", id: String(n).padStart(4, "0"), timestamp: 100, text: `Incoming ${n}` } as StoredMessage);
    await pager.reloadMessages("window@s");
  })}>Synthetic incoming message</button>
  <button onclick={() => act(() => pager.finishOlder("window@s"))}>Phone has no older rows</button>
  <button onclick={() => { windowFixture.failure = !windowFixture.failure; }}>Toggle page failure</button>
  <button onclick={() => { windowFixture.deferNext = true; }}>Delay next page</button>
  <button onclick={() => open("other@s")}>Switch to empty chat</button>
  <button onclick={() => { for (const release of windowFixture.pending.splice(0)) release(); report(); }}>Release delayed page</button>
  <p role="status">{metrics}</p>
  {#if ui.error}<p role="alert">{ui.error}</p>{/if}
  <div class="fixture-window" bind:this={scroller}>
    {#each pager.ordered as message (message.id)}<div data-id={message.id}>{message.text}</div>{/each}
  </div>
</details>

<style>
  .fixture-window { height: 180px; overflow: auto; border: 1px solid #53616a; margin-top: .5rem; }
  .fixture-window > div { height: 24px; }
</style>
