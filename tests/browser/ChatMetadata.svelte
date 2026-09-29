<script lang="ts">
  import ChatSidebar from "$lib/chat/ChatSidebar.svelte";
  import type { ComponentProps } from "svelte";
  import type { ChatSummary } from "$lib/utils/models";
  let selected = $state<string | null>(null);
  const quiet: ChatSummary = {
    chat: "quiet@s.whatsapp.net", display_name: "Quiet contact",
    last_message_at: 1700000000, last_text: "", last_from_me: false,
    last_sender_name: null, last_sender: "", last_media_kind: null,
    message_count: 0, unread_count: 0, mention_count: 0,
    pinned: true, archived: false, muted_until: 0, marked_unread: false,
  };
  const noop = () => {};
  const props: ComponentProps<typeof ChatSidebar> = {
    searchQuery: "", searchResults: [], visibleChats: [quiet], selectedChat: null,
    chatFilter: "all", onfilter: noop, unreadChats: 0, unreadPings: 0, avatars: {},
    chatLabelOf: (chat) => chat.display_name ?? chat.chat, formatTime: () => "Nov 14",
    typingLabelOf: () => null, previewAuthorOf: () => null,
    previewTextOf: (chat) => chat.last_text, mediaIconOf: () => null,
    groupKinds: {}, accounts: [], activeAccount: null, activeLabel: "Synthetic",
    accountAvatars: {}, me: null, meVersion: 0, visibility: "offline", accountMenu: false,
    onmenutoggle: noop, onswitchaccount: noop, onaddaccount: noop, onsettings: noop,
    onpings: noop, onstarred: noop, onsearch: noop, onopenresult: noop,
    onopenchat: (chat) => { selected = chat; }, ontogglepin: noop, onclearchat: noop,
    ondeletechat: noop, onchataction: noop, onmarkread: noop, archivedChats: 0, onresize: noop,
  };
</script>

<p role="status">Selected: {selected ?? "none"}</p>
<div class="sidebar"><ChatSidebar {...props} selectedChat={selected} /></div>

<style>
  .sidebar { height: 500px; display: flex; }
</style>
