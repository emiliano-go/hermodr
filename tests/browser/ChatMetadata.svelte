<script lang="ts">
  import ChatSidebar from "$lib/chat/ChatSidebar.svelte";
  import type { ComponentProps } from "svelte";
  import type { ChatSummary } from "$lib/utils/models";
  import { fixture, previewFixture } from "./ipc";
  import { session } from "$lib/state/session.svelte";
  let selected = $state<string | null>(null);
  let account = $state("synthetic-a");
  const quiet = $state<ChatSummary>({
    chat: "quiet@s.whatsapp.net", display_name: "Quiet contact",
    last_message_at: 1700000000, last_text: "", last_from_me: false,
    last_sender_name: null, last_sender: "", last_media_kind: null,
    message_count: 0, unread_count: 0, mention_count: 0,
    pinned: true, archived: false, muted_until: 0, marked_unread: false,
  });
  let action = $state("");
  let previewCalls = $state("");
  const noop = () => {};
  const props: ComponentProps<typeof ChatSidebar> = {
    searchQuery: "", searchResults: [], visibleChats: [quiet, ...Array.from({ length: 10 }, (_, n) => ({
      ...quiet, chat: `preview-${n}@s.whatsapp.net`, display_name: `Preview contact ${n}`, pinned: false,
    }))], selectedChat: null,
    chatFilter: "all", onfilter: noop, unreadChats: 0, unreadPings: 0, avatars: {},
    chatLabelOf: (chat) => chat.display_name ?? chat.chat, formatTime: () => "Nov 14",
    typingLabelOf: () => null, previewAuthorOf: () => null,
    previewTextOf: (chat) => chat.last_text, mediaIconOf: () => null,
    groupKinds: {}, accounts: [], activeAccount: null, activeLabel: "Synthetic",
    accountAvatars: {}, me: null, meVersion: 0, visibility: "offline", accountMenu: false,
    onmenutoggle: noop, onswitchaccount: noop, onaddaccount: noop, onsettings: noop,
    onpings: noop, onstarred: noop, onsearch: noop, onopenresult: noop,
    onopenchat: (chat) => { selected = chat; }, ontogglepin: () => { quiet.pinned = !quiet.pinned; }, onclearchat: noop,
    ondeletechat: noop, onchataction: (command, args) => {
      action = JSON.stringify({ command, args });
      if (command === "set_archived") quiet.archived = Boolean(args.archived);
      if (command === "set_muted") quiet.muted_until = Number(args.until);
      if (command === "set_marked_unread") quiet.marked_unread = Boolean(args.unread);
      if (command === "set_chat_auto_download") fixture.chatAutoDownload = Boolean(args.enabled);
    }, onmarkread: noop, archivedChats: 0, onresize: noop,
  };
</script>

<p role="status">Selected: {selected ?? "none"}</p>
<button onclick={() => { fixture.chatSettingsFailure = !fixture.chatSettingsFailure; }}>Toggle chat settings failure</button>
<button onclick={() => { fixture.chatSettingsDelay = true; }}>Delay next chat settings read</button>
<button onclick={() => { fixture.chatSettingsPending.shift()?.(); }}>Release chat settings read</button>
<button onclick={() => { account = account === "synthetic-a" ? "synthetic-b" : "synthetic-a"; }}>Switch synthetic account</button>
<label><input type="checkbox" bind:checked={session.settings.send_receipts} />Test send receipts</label>
<button onclick={() => { previewFixture.calls = []; previewCalls = ""; }}>Clear preview calls</button>
<button onclick={() => { previewCalls = JSON.stringify(previewFixture.calls); }}>Show preview calls</button>
<button onclick={() => { previewFixture.deferNext = true; }}>Delay next preview read</button>
<button onclick={() => { previewFixture.pending.shift()?.(); }}>Release preview read</button>
<button onclick={() => { previewFixture.failure = !previewFixture.failure; }}>Toggle preview failure</button>
<output aria-label="Preview calls">{previewCalls}</output>
<output aria-label="Chat action">{action}</output>
<div class="sidebar"><ChatSidebar {...props} activeAccount={account} selectedChat={selected} /></div>

<style>
  .sidebar { height: 500px; display: flex; }
</style>
