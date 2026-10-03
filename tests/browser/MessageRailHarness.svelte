<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import MessageList from "$lib/messages/MessageList.svelte";
  import SelectionBar from "$lib/messages/SelectionBar.svelte";
  import { loadedSelection, pickLoaded } from "$lib/utils/message-selection";
  import type { StoredMessage } from "$lib/utils/models";
  import { fixture } from "$lib/utils/wire.fixture";
  import { keywords } from "$lib/state/keywords.svelte";
  import { session } from "$lib/state/session.svelte";

  let { manual = false }: { manual?: boolean } = $props();
  let opened = $state(untrack(() => !manual));
  const noop = () => {};
  const row = (index: number): StoredMessage => ({ ...fixture.message, chat: "rail-fixture@s", id: `message-${index}`, sender: "fixture@s", from_me: false,
    timestamp: 1000 + index, sort_order: index, text: `Synthetic message ${index}. ${"Message body. ".repeat(6)}`,
    system_kind: null, system_params: [], media_kind: null, read: true });
  let messages = $state.raw(Array.from({ length: 400 }, (_, index) => row(index)));
  let scroller = $state<HTMLDivElement>(), rail = $state<ReturnType<typeof MessageList>>();
  let firstUnreadId = $state<string | null>("message-300"), complete = $state(false), failed = $state("");
  let picking = $state<Record<string, StoredMessage> | null>(null), selectionAnchor = $state<string | null>(null);
  function pick(message: StoredMessage, extend = false) {
    const next = pickLoaded(messages, picking, selectionAnchor, message, extend, () => false);
    picking = next.selected; selectionAnchor = next.anchor;
  }
  const listProps = {
    isGroup: false, switching: false, dayKey: () => "day", dayLabel: () => "Synthetic day", senderLabel: () => "Fixture sender",
    memberTagOf: () => null, hue: () => 120, captionOf: (message: StoredMessage) => message.text,
    viewOnceMarks: [], reactionsFor: new Map(), starredSet: new Set<string>(), editedSet: new Set<string>(), forwardedSet: new Set<string>(),
    downloading: {}, downloadErrors: {}, downloadTries: {}, replyingToId: null, highlightedId: null, onjumpunread: noop, menuId: null,
    polls: [], events: [], namer: (name: string) => name, avatarOf: () => null, avatars: {}, voiceAvatarOf: () => null,
    quoteAuthorOf: () => "Fixture", quoteTextOf: () => null, quoteChatNameOf: () => null, autoplayId: null, onceAudioOpenId: null,
    loadingOlder: false, onrecoverquote: noop, recovering: {}, uploads: [], typers: [], typerLabelOf: () => "Fixture", onscroll: noop,
    toWire: (text: string) => text, targetOf: (user: string) => ({ jid: user, name: user, self: false }), onprofile: noop, onopenurl: noop,
    formatTime: () => "12:00", onreplydraft: noop, onmenu: noop, onpick: pick, onjumpquoted: noop, ondownload: noop, onopenviewer: noop,
    onopenmedia: noop, onopenquote: noop, onvote: noop, onrespond: noop, oneditrequest: noop, oncancelevent: noop, onreact: noop,
    onopenreactions: noop, onmarkplayed: noop, onnextvoice: noop, onpausevoice: noop, onreplymenu: noop, ononce: noop, oncloseonce: noop,
    revealedOnce: {}, onrevealonce: noop, oninviteopen: noop, oninvitejoin: async () => { throw new Error("Invites unavailable in rail fixture"); },
  };
  const assert = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  async function settle() { await tick(); await new Promise((resolve) => setTimeout(resolve, 100)); }
  function mounted() {
    const nodes = [...scroller!.querySelectorAll<HTMLElement>(".vrow")];
    assert(nodes.length > 0 && nodes.length < 30, `Mounted row budget: ${nodes.length}`);
    const viewport = scroller!.querySelector<HTMLElement>(".message-viewport")!.getBoundingClientRect();
    assert(nodes.filter((node) => { const box = node.getBoundingClientRect(); return box.bottom <= viewport.top || box.top >= viewport.bottom; }).length <= 2,
      "Offscreen rows mounted beyond boundary measurement");
  }
  onMount(() => {
    session.activeAccount = "rail-fixture";
    keywords.rules = { hide: [], highlight: [] };
    if (manual) return;
    void (async () => {
      try {
        await settle(); mounted();
        assert(rail!.revealMessage("message-200"), "Loaded offscreen message missing");
        await settle(); mounted();
        const anchor = rail!.anchorId();
        assert(anchor !== null, "Visible anchor missing");
        scroller!.querySelector('[data-id="message-200"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
        await settle();
        assert(rail!.revealMessage("message-220"), "Range target unavailable"); await settle();
        scroller!.querySelector('[data-id="message-220"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true, shiftKey: true }));
        await settle();
        assert(Object.keys(picking ?? {}).length === 21 && document.querySelector(".selection-bar .count")?.textContent?.includes("21"),
          "Shift-click range missed offscreen loaded rows or displayed count");
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "a", ctrlKey: true, bubbles: true, cancelable: true }));
        await settle(); assert(Object.keys(picking ?? {}).length === 400, "Select loaded shortcut missed loaded rows");
        window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        await settle(); assert(picking === null && !document.querySelector(".selection-bar"), "Selection escape did not clear");
        rail!.revealMessage("message-200"); await settle();
        messages = [...messages, row(400)]; await settle(); mounted();
        assert(rail!.anchorId() === anchor, "Live append shifted reader anchor");
        assert(!scroller!.querySelector('[data-id="message-400"]'), "Offscreen append mounted");
        assert(rail!.scrollToUnread(), "Unread divider lost after append");
        await settle(); mounted();
        const divider = scroller!.querySelector<HTMLElement>("[data-unread-divider]");
        assert(divider, "Unread divider failed to mount");
        rail!.scrollToBottom(); await settle(); mounted();
        messages = [...messages, row(401)]; await settle(); rail!.scrollToBottom(); await settle();
        assert(scroller!.querySelector('[data-id="message-401"]'), "Live append failed to render at bottom");
        mounted();
      } catch (error) { failed = String(error); }
      finally { complete = true; }
    })();
  });
</script>

{#if manual}
  <button id="fixture-open" onclick={() => { opened = true; }}>Open fixture chat</button>
  <button id="fixture-send" disabled={!opened} onclick={() => {
    messages = [...messages, row(400)];
    void tick().then(() => rail?.scrollToBottom());
  }}>Send fixture message</button>
{/if}
{#if opened}<div class="frame"><MessageList bind:this={rail} bind:scroller {messages} {firstUnreadId} {picking} {...listProps} /></div>{/if}
{#if picking}<SelectionBar count={Object.keys(picking).length} onselectloaded={() => { picking = loadedSelection(messages, () => false); }}
  oncancel={() => { picking = null; selectionAnchor = null; }} onforward={noop} ondelete={noop} oncopy={noop} onstar={noop} onreact={noop} />{/if}
{#if !manual}
  <p id="message-rail-result" data-complete={complete} data-pass={complete && !failed}>
    {failed || "400-message viewport, selection, anchor, unread and live append checks passed"}
  </p>
{/if}

<style>
  :global(body) { margin: 0; font: 14px sans-serif; }
  .frame { display: flex; height: 500px; }
</style>
