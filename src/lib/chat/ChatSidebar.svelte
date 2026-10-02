<!-- The left bar: search, filter pills, chat rows and the account footer.
  Moved out of +page.svelte. -->
<script lang="ts">
  import { members } from "$lib/state/members.svelte";
  import { labels } from "$lib/state/labels.svelte";
  import { MEDIA_TYPES, emptyMediaOverrides } from "$lib/utils/auto-download";
  import type { MediaAutoDownload, MediaAutoDownloadOverrides } from "$lib/utils/wire";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Icon, { type IconName } from "$lib/ui/Icon.svelte";
  import NowPlaying from "$lib/media/NowPlaying.svelte";
  import ChatPreview from "./ChatPreview.svelte";
  import type { Section } from "$lib/settings/Settings.svelte";
  import type {
    Account,
    ChatFilter,
    ChatSummary,
    SearchResult,
  } from "$lib/utils/models";
  import { onDestroy, tick } from "svelte";
  import type { Snippet } from "svelte";
  import { invoke } from "$lib/utils/ipc";

  const STATUS_TEXT: Record<string, string> = {
    offline: "Connecting…",
    online: "Online",
    contacts: "Online to contacts",
    invisible: "Invisible",
  };

  let {
    searchQuery = $bindable(),
    searchResults,
    visibleChats,
    selectedChat,
    chatFilter,
    favoriteChats = [],
    favoriteBusy = false,
    ontogglefavorite,
    onfilter,
    unreadChats,
    unreadPings,
    avatars,
    chatLabelOf,
    formatTime,
    typingLabelOf,
    previewAuthorOf,
    previewTextOf,
    mediaIconOf,
    groupKinds,
    accounts,
    activeAccount,
    activeLabel,
    accountAvatars,
    me,
    meVersion,
    visibility,
    accountMenu,
    onmenutoggle,
    onswitchaccount,
    onaddaccount,
    onsettings,
    onpings,
    onstarred,
    onsearch,
    onopenresult,
    onopenchat,
    ontogglepin,
    onclearchat,
    ondeletechat,
    onchataction,
    onmarkread,
    onmarkallread,
    onnewgroup,
    onnewcontact = () => {},
    oninbox = () => {},
    onlabels = () => {},
    onchatlabels = () => {},
    labelFilter = $bindable(""),
    canCreateGroup = false,
    onblockcontact,
    markingAllRead = false,
    archivedChats,
    onresize,
    freezeOnHover = true,
    chatPreview = true,
    globalAutoDownload,
    spacesContent,
  }: {
    spacesContent?: Snippet;
    searchQuery: string;
    searchResults: SearchResult[];
    visibleChats: ChatSummary[];
    selectedChat: string | null;
    chatFilter: ChatFilter;
    favoriteChats?: string[];
    favoriteBusy?: boolean;
    ontogglefavorite?: (chat: ChatSummary) => void;
    onfilter: (filter: ChatFilter) => void;
    unreadChats: number;
    unreadPings: number;
    avatars: Record<string, string | null>;
    chatLabelOf: (chat: ChatSummary) => string;
    formatTime: (ts: number) => string;
    typingLabelOf: (chat: string) => string | null;
    previewAuthorOf: (chat: ChatSummary) => string | null;
    previewTextOf: (chat: ChatSummary) => string;
    mediaIconOf: (kind: string | null) => IconName | null;
    groupKinds: Record<string, { community: boolean; announcements: boolean; parent: string | null }>;
    accounts: Account[];
    activeAccount: string | null;
    activeLabel: string;
    accountAvatars: Record<string, string | null>;
    me: string | null;
    meVersion: number;
    visibility: string;
    accountMenu: boolean;
    onmenutoggle: () => void;
    onswitchaccount: (id: string) => void;
    onaddaccount: () => void;
    onsettings: (section: Section) => void;
    onpings: () => void;
    onstarred: () => void;
    onsearch: () => void;
    onopenresult: (result: SearchResult) => void;
    onopenchat: (chat: string, jumpToMention?: boolean) => void;
    ontogglepin: (chat: ChatSummary, event?: MouseEvent) => void;
    onclearchat: (chat: ChatSummary) => void;
    ondeletechat: (chat: ChatSummary) => void;
    onchataction: (command: string, args: Record<string, unknown>) => void;
    onmarkread: (chat: ChatSummary) => void;
    archivedChats: number;
    onresize: (event: MouseEvent) => void;
    /** Pause list reordering while the pointer is over the list. */
    freezeOnHover?: boolean;
    /** Whether hovering a row shows the recent-messages popup. */
    chatPreview?: boolean;
    globalAutoDownload: MediaAutoDownload;
    onmarkallread: () => void;
    onnewgroup: () => void;
    onnewcontact?: () => void;
    oninbox?: () => void;
    onlabels?: () => void;
    onchatlabels?: (chat: string) => void;
    labelFilter?: string;
    canCreateGroup?: boolean;
    onblockcontact: (jid: string) => Promise<void>;
    markingAllRead?: boolean;
  } = $props();

  const MUTES: [string, number][] = [
    ["Mute for 8 hours", 8 * 3600],
    ["Mute for 1 week", 7 * 86400],
    ["Mute always", -1],
  ];

  function isMuted(chat: ChatSummary) {
    return chat.muted_until < 0 || chat.muted_until * 1000 > Date.now();
  }

  /** Right-click menu on a chat row. */
  let chatMenu = $state<{ x: number; y: number; chat: ChatSummary } | null>(null);
  let menuAutoDownload = $state<MediaAutoDownloadOverrides>(emptyMediaOverrides());
  const menuDownloadsEnabled = $derived(MEDIA_TYPES.some(([kind]) => menuAutoDownload[kind] ?? globalAutoDownload[kind]));
  let menuLoaded = $state(false);
  let menuError = $state<string | null>(null);
  let menuRequest = 0;
  let menuOwner: HTMLElement | null = null;
  /** Whether the right-click menu's "More" section is expanded. */
  let moreOpen = $state(false);
  /** Whether the menu's chat mutes @all mentions; from the list, then the settings read. */
  let menuMuteAtAll = $state(false);
  let menuMuteBusy = $state(false);
  let preview = $state<{ chat: ChatSummary; x: number; y: number } | null>(null);
  let previewOwner: HTMLElement | null = null;
  let previewClose: ReturnType<typeof setTimeout> | undefined;
  let previewDelay: ReturnType<typeof setTimeout> | undefined;
  let previewFocusFrame: number | undefined;
  let restoringPreviewFocus = false;

  /** Hovering this long before the preview appears, so passing over the list does not open it. */
  const PREVIEW_HOVER_DELAY = 3000;

  function cancelPreviewFocus() {
    if (previewFocusFrame !== undefined) cancelAnimationFrame(previewFocusFrame);
    previewFocusFrame = undefined;
  }

  /** Arms the hover delay: the row must stay under the pointer for three seconds. */
  function schedulePreview(owner: HTMLElement, chat: ChatSummary) {
    if (!chatPreview) return;
    cancelPreviewFocus();
    holdPreview();
    clearTimeout(previewDelay);
    previewDelay = setTimeout(() => {
      previewDelay = undefined;
      // The row may have moved out from under the pointer while waiting.
      if (!owner.isConnected || !visibleChats.some((row) => row.chat === chat.chat)) return;
      showPreview(owner, chat);
    }, PREVIEW_HOVER_DELAY);
  }

  function showPreview(owner: HTMLElement, chat: ChatSummary) {
    if (!chatPreview) return;
    cancelPreviewFocus();
    holdPreview();
    if (chatMenu || restoringPreviewFocus) return;
    if (chat.chat === selectedChat) { hidePreview(); return; }
    previewOwner = owner;
    const box = previewOwner.getBoundingClientRect();
    preview = { chat, x: box.right + 8, y: box.top };
  }

  function focusPreview(owner: HTMLElement, chat: ChatSummary, enter = false) {
    if (restoringPreviewFocus) return;
    cancelPreviewFocus();
    const account = activeAccount;
    previewFocusFrame = requestAnimationFrame(async () => {
      previewFocusFrame = undefined;
      if (account !== activeAccount || !owner.isConnected || document.activeElement !== owner || !visibleChats.some((row) => row.chat === chat.chat)) return;
      showPreview(owner, chat);
      if (enter) {
        await tick();
        if (account === activeAccount && preview?.chat.chat === chat.chat) {
          document.querySelector<HTMLElement>("#chat-preview .preview-messages")?.focus({ preventScroll: true });
        }
      }
    });
  }

  function holdPreview() {
    clearTimeout(previewClose);
    previewClose = undefined;
  }

  function hidePreview() {
    cancelPreviewFocus();
    holdPreview();
    clearTimeout(previewDelay);
    previewDelay = undefined;
    preview = null;
    previewOwner = null;
  }

  function leavePreview() {
    holdPreview();
    clearTimeout(previewDelay);
    previewDelay = undefined;
    previewClose = setTimeout(() => {
      const overlay = document.getElementById("chat-preview");
      if (overlay?.matches(":hover") || overlay?.contains(document.activeElement) || previewOwner === document.activeElement) return;
      hidePreview();
    }, 180);
  }

  function dismissPreview() {
    const owner = previewOwner;
    hidePreview();
    if (owner?.isConnected) {
      restoringPreviewFocus = true;
      owner.focus({ preventScroll: true });
      restoringPreviewFocus = false;
    }
  }

  onDestroy(hidePreview);

  /** A search result's row as a summary: the list's own row when known, else the result's fields. */
  function resultChat(result: SearchResult): ChatSummary {
    return visibleChats.find((c) => c.chat === result.jid) ?? {
      chat: result.jid,
      display_name: result.name || null,
      last_message_at: 0,
      last_text: "",
      last_from_me: false,
      last_sender_name: null,
      last_sender: "",
      last_media_kind: null,
      message_count: result.has_messages ? 1 : 0,
      unread_count: 0,
      mention_count: 0,
      pinned: false,
      archived: false,
      muted_until: 0,
      mute_at_all: false,
      marked_unread: false,
    };
  }

  function openResultMenu(event: MouseEvent | KeyboardEvent, result: SearchResult) {
    void openChatMenu(event, resultChat(result));
  }

  async function openChatMenu(event: MouseEvent | KeyboardEvent, chat: ChatSummary) {
    event.preventDefault();
    event.stopPropagation();
    hidePreview();
    menuOwner = event.currentTarget as HTMLElement;
    const box = menuOwner.getBoundingClientRect();
    chatMenu = { x: "clientX" in event ? event.clientX : box.left, y: "clientY" in event ? event.clientY : box.bottom, chat };
    menuLoaded = false;
    menuError = null;
    moreOpen = false;
    menuMuteAtAll = chat.mute_at_all;
    menuMuteBusy = false;
    const request = ++menuRequest;
    const account = activeAccount;
    void layoutMenu(event.type === "keydown");
    try {
      const settings = await invoke<import("$lib/utils/wire").ChatSettings>("chat_settings", { chat: chat.chat });
      if (request !== menuRequest || account !== activeAccount) return;
      menuAutoDownload = settings.auto_download_types;
      menuMuteAtAll = settings.mute_at_all ?? chat.mute_at_all;
      menuLoaded = true;
    } catch (e) {
      if (request !== menuRequest || account !== activeAccount) return;
      menuError = String(e);
    }
    void layoutMenu();
  }

  async function layoutMenu(focus = false) {
    await tick();
    const menu = document.querySelector<HTMLElement>(".chat-menu");
    if (!menu || !chatMenu) return;
    const box = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(chatMenu.x, window.innerWidth - box.width - 8))}px`;
    menu.style.top = `${Math.max(8, Math.min(chatMenu.y, window.innerHeight - box.height - 8))}px`;
    if (focus) menu.querySelector<HTMLElement>("[role=menuitem]")?.focus();
  }

  function closeChatMenu(restoreFocus = true) {
    menuRequest++;
    chatMenu = null;
    moreOpen = false;
    if (restoreFocus) menuOwner?.focus();
  }

  $effect(() => { void activeAccount; closeChatMenu(false); hidePreview(); });
  $effect(() => { if (!chatPreview) hidePreview(); });
  $effect(() => {
    const chat = preview?.chat.chat;
    if (chat && !visibleChats.some((row) => row.chat === chat)) hidePreview();
  });
  $effect(() => {
    if (preview && preview.chat.chat === selectedChat) hidePreview();
  });

  // Hover freeze: while the pointer is over the list, new arrivals update each
  // row in place but keep the captured order, so the row under the cursor
  // cannot jump away. The pending order applies on leave or on open/action.
  let listHover = $state(false);
  let frozenOrder = $state<string[]>([]);

  // Searching swaps the list for results, which removes a hovered <ul> without
  // firing mouseleave; without this reset the captured order would outlive the
  // pointer and the list would never re-sort again.
  $effect(() => {
    void searchQuery.trim();
    hidePreview();
    listHover = false;
    frozenOrder = [];
  });

  function onListEnter() {
    listHover = true;
    if (freezeOnHover) frozenOrder = visibleChats.map((c) => c.chat);
  }

  function onListLeave() {
    listHover = false;
    frozenOrder = [];
  }

  function releaseFreeze() {
    frozenOrder = [];
  }

  const displayedChats = $derived.by(() => {
    if (!freezeOnHover || !listHover || frozenOrder.length === 0) return visibleChats;
    const pos = new Map(frozenOrder.map((id, i) => [id, i] as const));
    return [...visibleChats].sort((a, b) => {
      const pa = pos.get(a.chat);
      const pb = pos.get(b.chat);
      if (pa === undefined && pb === undefined) return 0;
      if (pa === undefined) return 1;
      if (pb === undefined) return -1;
      return pa - pb;
    });
  });
</script>

<aside class="chats">
  <header>
    <h1 class="title">Chats</h1>
    <Button
      variant="icon"
      icon="at"
      iconSize={18}
      title="Mentions"
      aria-label="Mentions"
      cls="badge-host"
      onclick={onpings}>
      {#if unreadPings > 0}<span class="icon-badge">{unreadPings > 99 ? "99+" : unreadPings}</span>{/if}
    </Button>
    <Button variant="icon" icon="star" iconSize={18} title="Starred messages" aria-label="Starred messages" onclick={onstarred} />
    <Button variant="icon" icon="check" iconSize={18} title="Mark all chats as read" aria-label="Mark all chats as read"
      disabled={markingAllRead} onclick={onmarkallread} />
    <Button variant="icon" icon="users" iconSize={18} title="New group" aria-label="New group"
      disabled={!canCreateGroup} onclick={onnewgroup} />
    <Button variant="icon" icon="user" iconSize={18} title="New contact" aria-label="New contact"
      disabled={!canCreateGroup} onclick={onnewcontact} />
  </header>
  {@render spacesContent?.()}
  <label class="search">
    <Icon name="search" size={15} />
    <input
      placeholder="Search chats and contacts"
      bind:value={searchQuery}
      oninput={onsearch}
      autocomplete="off"
    />
  </label>
  {#if !searchQuery.trim()}
    <div class="filters">
      <Button variant="chip" onclick={oninbox}>Inbox</Button>
      <Button variant="chip" onclick={onlabels}>Manage labels</Button>
      <select aria-label="Filter by label" bind:value={labelFilter}>
        <option value="">All labels</option>
        {#if labels.account === activeAccount}{#each labels.view.labels as label (label.id)}<option value={label.id}>{label.name}</option>{/each}{/if}
      </select>
    </div>
    <div class="filters" role="tablist" aria-label="Filter chats">
      <Button variant="chip" selected={chatFilter === "all"} onclick={() => onfilter("all")}>All</Button>
      <Button variant="chip" selected={chatFilter === "favorites"} onclick={() => onfilter("favorites")}>Favorites</Button>
      <Button
        variant="chip"
        selected={chatFilter === "unread"}
        count={unreadChats > 0 ? unreadChats : undefined}
        onclick={() => onfilter("unread")}>Unread</Button>
      <Button variant="chip" selected={chatFilter === "groups"} onclick={() => onfilter("groups")}
        >Groups</Button>
    </div>
    {#if archivedChats > 0 || chatFilter === "archived"}
      <button
        type="button"
        class="archived-entry"
        class:active={chatFilter === "archived"}
        aria-pressed={chatFilter === "archived"}
        onclick={() => onfilter(chatFilter === "archived" ? "all" : "archived")}>
        <Icon name="archive" size={18} />
        <span class="archived-label">{chatFilter === "archived" ? "Back to chats" : "Archived"}</span>
        {#if archivedChats > 0}
          <span class="archived-count">{archivedChats > 99 ? "99+" : archivedChats}</span>
        {/if}
      </button>
    {/if}
  {/if}
  {#if searchQuery.trim()}
    <ul class="results">
      {#each searchResults as result (result.jid)}
        <li>
          <div
            class="chat-row"
            role="button"
            tabindex="0"
            onclick={() => onopenresult(result)}
            oncontextmenu={(e) => openResultMenu(e, result)}
            onkeydown={(e) => {
              if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
                void openResultMenu(e, result);
              } else if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onopenresult(result);
              }
            }}>
            <Avatar
              src={avatars[result.jid] ?? null}
              label={members.displayName(result.name, result.jid)}
              seed={result.jid}
            />
            <span class="name">
              {members.displayName(result.name, result.jid)}
            </span>
            <span class="preview">
              {result.kind}{result.has_messages ? "" : " · no messages yet"}
            </span>
          </div>
        </li>
      {/each}
    </ul>
  {:else}
  <ul onmouseenter={onListEnter} onmouseleave={onListLeave} onwheel={hidePreview}
    onscroll={(e) => { if (e.target === e.currentTarget && previewFocusFrame === undefined) hidePreview(); }}>
    {#each displayedChats as chat (chat.chat)}
      <li>
        <div
          class="chat-row"
          class:active={chat.chat === selectedChat}
          role="button"
          tabindex="0"
          aria-describedby={preview?.chat.chat === chat.chat ? "chat-preview" : undefined}
          onpointerenter={(e) => schedulePreview(e.currentTarget, chat)}
          onpointerleave={leavePreview}
          onfocus={(e) => focusPreview(e.currentTarget, chat)}
          onblur={leavePreview}
          onclick={() => {
            hidePreview();
            releaseFreeze();
            onopenchat(chat.chat);
          }}
          oncontextmenu={(e) => openChatMenu(e, chat)}
          onkeydown={(e) => {
            if (e.key === "ArrowRight") {
              e.preventDefault();
              focusPreview(e.currentTarget, chat, true);
            } else if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
              void openChatMenu(e, chat);
            } else if (e.key === "Enter" || e.key === " ") {
              e.preventDefault();
              hidePreview();
              releaseFreeze();
              onopenchat(chat.chat);
            }
          }}>
          <Avatar src={avatars[chat.chat] ?? null} label={chatLabelOf(chat)} seed={chat.chat} />
          <span class="name"
            >{#if chat.pinned}<span class="pin"><Icon name="pin" size={12} /></span>{/if}{#if groupKinds[chat.chat]?.community}<span
                class="kind"
                title="Community"><Icon name="users" size={13} /></span
              >{:else if groupKinds[chat.chat]?.announcements}<span class="kind" title="Community announcements"
                ><Icon name="volume" size={13} /></span
              >{/if}{chatLabelOf(chat)}</span>
          <span class="time" class:unread={chat.unread_count > 0}
            >{chat.last_message_at > 0 ? formatTime(chat.last_message_at) : ""}</span>
          {#if typingLabelOf(chat.chat)}
            <span class="preview typing">{typingLabelOf(chat.chat)}</span>
          {:else if chat.message_count === 0}
            <span class="preview empty-chat">No stored messages</span>
          {:else}
            {@const author = previewAuthorOf(chat)}
            {@const icon = mediaIconOf(chat.last_media_kind)}
            <span class="preview"
              >{#if author}{author}:&nbsp;{/if}{#if icon}<span class="preview-icon"
                  ><Icon name={icon} size={15} /></span
                >{/if}{previewTextOf(chat)}</span
            >
          {/if}
          {#if labels.account === activeAccount && labels.chatIds(chat.chat).length}
            <div class="chat-labels" aria-label="Chat labels">
              {#each labels.view.labels.filter((label) => labels.chatIds(chat.chat).includes(label.id)) as label (label.id)}
                <span class="chat-label" title={`Label: ${label.name}`}>{label.name}</span>
              {/each}
            </div>
          {/if}
          <span class="badges">
            {#if chat.mention_count > 0}
              <button
                class="badge mention-badge"
                title="Jump to mention"
                onclick={(e) => {
                  e.stopPropagation();
                  releaseFreeze();
                  onopenchat(chat.chat, true);
                }}>@</button>
            {/if}
            {#if isMuted(chat)}
              <span class="muted-mark" title="Muted"><Icon name="volume" size={13} /></span>
            {/if}
            {#if chat.mute_at_all}
              <span class="muted-mark at-muted" title="@all mentions muted"><Icon name="at" size={13} /></span>
            {/if}
            {#if chat.unread_count > 0}
              <span class="badge">{chat.unread_count > 99 ? "99+" : chat.unread_count}</span>
            {:else if chat.marked_unread}
              <span class="badge" title="Marked unread">&nbsp;</span>
            {/if}
            <button
              class="pin-toggle"
              title={chat.pinned ? "Unpin" : "Pin"}
              aria-label={chat.pinned ? "Unpin" : "Pin"}
              onclick={(e) => {
                releaseFreeze();
                ontogglepin(chat, e);
              }}><Icon name="pin" size={14} /></button>
          </span>
        </div>
      </li>
    {/each}
    {#if displayedChats.length === 0}
      <li class="empty">
        {chatFilter === "unread"
          ? "No unread chats."
          : chatFilter === "archived"
            ? "No archived chats."
          : chatFilter === "groups"
            ? "No groups yet."
            : "No conversations yet."}
      </li>
    {/if}
  </ul>
  {/if}

  <NowPlaying />
  <footer class="user-panel">
    {#if accountMenu}
      <div class="account-menu" role="menu">
        <span class="menu-label">Accounts</span>
        {#each accounts as account (account.id)}
          <Button
            variant="menu"
            active={account.id === activeAccount}
            role="menuitem"
            onclick={() => onswitchaccount(account.id)}>
            <Avatar
              src={accountAvatars[account.id] ?? null}
              label={account.label}
              seed={account.id}
              cls="menu-avatar"
            />
            <span class="menu-name">{account.label}</span>
            <span class="menu-dot"></span>
          </Button>
        {/each}
        <div class="menu-sep"></div>
        <Button
          variant="menu"
          icon="plus"
          iconSize={15}
          role="menuitem"
          onclick={onaddaccount}>Add account</Button>
        <Button variant="menu" icon="users" iconSize={15} role="menuitem" onclick={() => onsettings("accounts")}>
          Manage accounts
        </Button>
      </div>
    {/if}
    <button
      class="me"
      title="Switch account"
      aria-expanded={accountMenu}
      onclick={onmenutoggle}>
      <span class="me-avatar-wrap">
        <Avatar
          src={me ? (avatars[me] ?? null) : null}
          label={activeLabel}
          seed={activeAccount ?? ""}
          cls="me-avatar"
          version={meVersion}
        />
        <span class="presence {visibility}"></span>
      </span>
      <span class="me-text" title={me ? `+${me.split("@")[0]}` : undefined}>
        <span class="me-name">{activeLabel}</span>
        <span class="me-status">{STATUS_TEXT[visibility] ?? visibility}</span>
      </span>
    </button>
    <Button
      variant="icon"
      icon="settings"
      iconSize={19}
      title="Settings"
      aria-label="Settings"
      onclick={() => onsettings("profile")} />
  </footer>
  <button type="button" class="resizer" aria-label="Resize chat list" onmousedown={onresize}></button>
</aside>

{#if preview}
  <ChatPreview chat={preview.chat.chat} account={activeAccount} name={chatLabelOf(preview.chat)} x={preview.x} y={preview.y}
    onpointerenter={holdPreview} onpointerleave={leavePreview} onfocusin={holdPreview} onfocusout={leavePreview} ondismiss={dismissPreview}
    onsettings={() => {
      hidePreview();
      onsettings("chats");
    }}
    onopen={(jid) => {
      if (jid === selectedChat) return;
      hidePreview();
      releaseFreeze();
      onopenchat(jid);
    }} />
{/if}

{#if chatMenu}
  {@const menuChat = chatMenu.chat}
  <div
    class="chat-menu"
    role="menu"
    style="left: {Math.min(chatMenu.x, window.innerWidth - 220)}px; top: {Math.min(chatMenu.y, window.innerHeight - 160)}px">
    {#if menuError}<p role="alert">Could not load chat settings: {menuError}</p>{/if}
    <Button
      variant="menu"
      icon="pin"
      iconSize={15}
      role="menuitem"
      onclick={() => {
        releaseFreeze();
        ontogglepin(menuChat);
        closeChatMenu();
      }}>{menuChat.pinned ? "Unpin" : "Pin"}</Button>
    <Button
      variant="menu"
      icon="star"
      iconSize={15}
      role="menuitem"
      disabled={favoriteBusy || !ontogglefavorite}
      onclick={() => {
        releaseFreeze();
        if (menuChat) ontogglefavorite?.(menuChat);
        closeChatMenu();
      }}>{favoriteChats.includes(menuChat.chat) ? "Remove from favorites" : "Add to favorites"}</Button>
    {#if /@(s\.whatsapp\.net|lid)$/.test(menuChat.chat) && !members.isMe(menuChat.chat)}
      <Button variant="menu" icon="x" iconSize={15} role="menuitem" onclick={() => {
        void onblockcontact(menuChat.chat);
        closeChatMenu();
      }}>Block contact</Button>
    {/if}
    <Button variant="menu" icon="edit" iconSize={15} role="menuitem" onclick={() => {
      onchatlabels(menuChat.chat); closeChatMenu();
    }}>Labels</Button>
    <Button
      variant="menu"
      icon="download"
      iconSize={15}
      role="menuitem"
      onclick={() => {
        releaseFreeze();
        onchataction("set_archived", { chat: menuChat.chat, archived: !menuChat.archived });
        closeChatMenu();
      }}>{menuChat.archived ? "Unarchive" : "Archive"}</Button>
    {#if isMuted(menuChat)}
      <Button
        variant="menu"
        icon="volume"
        iconSize={15}
        role="menuitem"
        onclick={() => {
          onchataction("set_muted", { chat: menuChat.chat, until: 0 });
          closeChatMenu();
        }}>Unmute</Button>
    {:else}
      {#each MUTES as [label, seconds] (label)}
        <Button
          variant="menu"
          icon="clock"
          iconSize={15}
          role="menuitem"
          onclick={() => {
            const until = seconds < 0 ? -1 : Math.floor(Date.now() / 1000) + seconds;
            onchataction("set_muted", { chat: menuChat.chat, until });
            closeChatMenu();
          }}>{label}</Button>
      {/each}
    {/if}
    <Button
      variant="menu"
      icon={menuChat.unread_count > 0 || menuChat.marked_unread ? "check" : "message"}
      iconSize={15}
      role="menuitem"
      onclick={() => {
        if (menuChat.unread_count > 0) onmarkread(menuChat);
        else onchataction("set_marked_unread", { chat: menuChat.chat, unread: !menuChat.marked_unread });
        releaseFreeze();
        closeChatMenu();
      }}>{menuChat.unread_count > 0 || menuChat.marked_unread ? "Mark as read" : "Mark as unread"}</Button>
    <Button
      variant="menu"
      icon="download"
      iconSize={15}
      role="menuitem"
      disabled={!menuLoaded}
      onclick={() => {
        onchataction("set_chat_auto_download", { chat: menuChat.chat, enabled: !menuDownloadsEnabled });
        closeChatMenu();
      }}>{menuLoaded ? `${menuDownloadsEnabled ? "Disable" : "Enable"} all media auto-download` : menuError ? "Media settings unavailable" : "Loading media settings…"}</Button>
    <div class="more-wrap">
      <Button
        variant="menu"
        icon="chevronRight"
        iconSize={15}
        role="menuitem"
        aria-expanded={moreOpen}
        aria-haspopup="true"
        onclick={() => {
          moreOpen = !moreOpen;
          void layoutMenu();
        }}><span class="more-label">More</span><span class="more-chevron" class:open={moreOpen}><Icon name="chevronRight" size={14} /></span></Button>
      {#if moreOpen}
        <div class="more-sub" role="menu" aria-label="More chat options">
          <Button
            variant="menu"
            icon="at"
            iconSize={15}
            role="menuitem"
            disabled={menuMuteBusy}
            onclick={() => {
              const target = !menuMuteAtAll;
              menuMuteBusy = true;
              menuMuteAtAll = target;
              try {
                onchataction("set_chat_mute_at_all", { chat: menuChat.chat, muted: target });
              } finally {
                menuMuteBusy = false;
                closeChatMenu();
              }
            }}>{menuMuteAtAll ? "Unmute @all mentions" : "Mute @all mentions"}</Button>
        </div>
      {/if}
    </div>
    <Button
      variant="menu"
      icon="edit"
      iconSize={15}
      role="menuitem"
      onclick={() => {
        const c = menuChat;
        releaseFreeze();
        closeChatMenu();
        onclearchat(c);
      }}>Clear chat</Button>
    <Button
      variant="menu"
      icon="trash"
      iconSize={15}
      role="menuitem"
      onclick={() => {
        const c = menuChat;
        releaseFreeze();
        closeChatMenu();
        ondeletechat(c);
      }}>Delete chat</Button>
    {#if menuChat.chat.endsWith("@g.us")}
      <Button
        variant="menu"
        icon="x"
        iconSize={15}
        role="menuitem"
        onclick={() => {
          const c = menuChat;
          releaseFreeze();
          closeChatMenu();
          if (confirm(`Exit ${c.display_name ?? "this group"}?`)) onchataction("leave_group", { chat: c.chat });
        }}>Exit group</Button>
    {/if}
  </div>
{/if}

<svelte:window
  onclick={(e) => {
    if (chatMenu && !(e.target as Element).closest?.(".chat-menu")) closeChatMenu(false);
  }}
  oncontextmenu={(e) => {
    if (chatMenu && !(e.target as Element).closest?.(".chat-menu")) closeChatMenu(false);
  }}
  onkeydown={(e) => {
    if (e.key === "Escape" && preview && !e.defaultPrevented) dismissPreview();
    if (e.key === "Escape" && chatMenu) closeChatMenu();
  }}
  onblur={hidePreview} />

<style>
  .chat-label { max-width: 80px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; padding: 1px 5px; border-radius: 6px; background: var(--raised); color: var(--muted); font-size: 10px; }
  .chat-labels { grid-column: 2 / 4; grid-row: 3; min-width: 0; display: flex; flex-wrap: wrap; gap: 3px; }
  .chats {
    position: relative;
    overflow: hidden;
    background: var(--bg);
    border-right: 1px solid var(--line);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .chats header {
    min-width: 0;
    height: 64px;
    box-sizing: border-box;
    flex: none;
    overflow: hidden;
    padding: 0 12px 0 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
  }
  .title {
    margin: 0;
    font-size: 22px;
    font-weight: 700;
  }
  /* Shared button shapes live in $lib/ui/Button.svelte; only spot tweaks stay here. */
  :global(.badge-host) {
    position: relative;
  }
  /* Keeps the mentions button beside the starred one instead of centred. */
  .chats header :global(.badge-host) {
    margin-left: auto;
  }
  .icon-badge {
    position: absolute;
    top: 2px;
    right: 0;
    min-width: 16px;
    height: 16px;
    padding: 0 4px;
    box-sizing: border-box;
    border-radius: 999px;
    background: var(--mention);
    color: var(--accent-ink);
    font-size: 10px;
    font-weight: 700;
    line-height: 16px;
    text-align: center;
    pointer-events: none;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 12px 8px;
    padding: 0 12px;
    height: 36px;
    flex: none;
    background: var(--surface);
    border: 1px solid transparent;
    border-radius: 999px;
    color: var(--faint);
    cursor: text;
  }
  .search:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }
  .search input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: 0;
    outline: none;
    padding: 0;
    color: var(--text);
    font: inherit;
  }
  .search input:focus-visible {
    box-shadow: none;
  }
  .search input::placeholder {
    color: var(--muted);
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    padding: 0 12px 8px;
    flex: none;
  }
  .archived-entry {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0 12px 8px;
    padding: 0 14px;
    height: 46px;
    border: 1px solid transparent;
    border-radius: 10px;
    background: var(--raised);
    color: var(--text);
    font: inherit;
    font-size: 14.5px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }
  .archived-entry:hover {
    background: var(--raised-2);
  }
  .archived-entry.active {
    border-color: var(--accent);
    color: var(--accent);
  }
  .archived-label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .archived-count {
    flex: none;
    min-width: 20px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 12px;
    font-weight: 700;
    text-align: center;
  }
  .results .preview {
    color: var(--faint);
  }
  .chats ul {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    overflow-x: hidden;
    flex: 1;
  }
  .chat-row {
    position: relative;
    box-sizing: border-box;
    width: 100%;
    height: 72px;
    min-width: 0;
    overflow: hidden;
    display: grid;
    grid-template-columns: auto 1fr auto;
    grid-template-areas: "avatar name time" "avatar preview badge";
    gap: 2px 15px;
    text-align: left;
    background: transparent;
    color: inherit;
    border: 0;
    padding: 0 15px 0 13px;
    cursor: pointer;
    font: inherit;
    align-items: center;
    align-content: center;
  }
  /* The divider starts after the avatar, as in WhatsApp. */
  .chat-row::after {
    content: "";
    position: absolute;
    left: 77px;
    right: 0;
    bottom: 0;
    border-bottom: 1px solid var(--line);
  }
  .chat-row:hover {
    background: var(--surface);
  }
  .chat-row.active {
    background: var(--raised);
  }
  .chat-row:focus-visible {
    outline-offset: -2px;
  }
  .badges {
    grid-area: badge;
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .pin {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 4px;
    color: var(--faint);
  }
  .badge.mention-badge {
    background: var(--accent-soft);
    color: var(--accent-text);
    border: 0;
    cursor: pointer;
    font: inherit;
    font-size: 11px;
    font-weight: 700;
  }
  .muted-mark {
    display: inline-flex;
    color: var(--muted);
  }

  .pin-toggle {
    display: none;
    background: transparent;
    border: 0;
    color: var(--faint);
    padding: 2px;
    border-radius: 4px;
    cursor: pointer;
  }
  .pin-toggle:hover {
    color: var(--text);
  }
  .chat-row:hover .pin-toggle,
  .pin-toggle:focus-visible {
    display: block;
  }
  .name {
    grid-area: name;
    min-width: 0;
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    grid-area: time;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .time.unread {
    color: var(--accent);
  }
  .chat-row .name {
    font-size: 17px;
    font-weight: 400;
  }
  .chat-row .preview {
    font-size: 14px;
    color: var(--muted);
  }
  .preview.typing {
    color: var(--accent);
  }
  .preview-icon {
    display: inline-flex;
    vertical-align: -2px;
    margin-right: 4px;
  }
  .preview {
    grid-area: preview;
    min-width: 0;
    color: var(--muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .badge {
    display: grid;
    place-items: center;
    background: var(--accent);
    color: var(--accent-ink);
    font-size: 12px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    border-radius: 999px;
    height: 20px;
    padding: 0 6px;
    min-width: 20px;
    box-sizing: border-box;
  }
  .kind {
    display: inline-flex;
    vertical-align: -1px;
    margin-right: 5px;
    color: var(--muted);
  }
  .empty {
    padding: 16px 10px;
    color: var(--faint);
  }
  .user-panel {
    position: relative;
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 62px;
    box-sizing: border-box;
    padding: 0 8px;
    background: var(--surface);
  }
  .user-panel .me {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 6px;
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .user-panel .me:hover,
  .user-panel .me[aria-expanded="true"] {
    background: var(--raised);
  }
  .me-avatar-wrap {
    position: relative;
    flex: none;
  }
  .presence {
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: 11px;
    height: 11px;
    border-radius: 50%;
    background: var(--faint);
    box-shadow: 0 0 0 3px var(--surface);
  }
  .presence.online {
    background: var(--accent);
  }
  /* Half-lit: online, but only contacts can see it. */
  .presence.contacts {
    background: linear-gradient(90deg, var(--accent) 50%, var(--faint) 50%);
  }
  /* Discord's invisible: a hollow grey ring. */
  .presence.invisible {
    background: var(--surface);
    border: 3px solid var(--muted);
    box-sizing: border-box;
  }
  .me-text {
    display: flex;
    flex-direction: column;
    min-width: 0;
    line-height: 1.25;
  }
  .me-name {
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .me-status {
    font-size: 12px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .account-menu {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: calc(100% + 6px);
    z-index: 20;
    display: flex;
    flex-direction: column;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .menu-label {
    padding: 6px 8px 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }
  .menu-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .menu-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    border: 2px solid var(--muted);
    box-sizing: border-box;
  }
  /* The dot fills on the active account's row (Button renders .btn-menu.on). */
  :global(.btn-menu.on) .menu-dot {
    background: var(--accent);
    border-color: var(--accent);
  }
  .menu-sep {
    height: 1px;
    margin: 6px 4px;
    background: var(--line-strong);
  }
  .resizer {
    border: 0;
    background: transparent;
    padding: 0;
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: col-resize;
    z-index: 5;
  }
  .chats .resizer {
    display: block;
    right: -3px;
  }
  .preview.empty-chat {
    font-style: italic;
  }
  .chat-menu {
    max-height: calc(100vh - 16px);
    max-width: calc(100vw - 16px);
    overflow-y: auto;
    position: fixed;
    z-index: 100;
    min-width: 200px;
    display: flex;
    flex-direction: column;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .more-wrap {
    display: flex;
    flex-direction: column;
  }
  .more-label {
    flex: 1;
  }
  .more-chevron {
    display: inline-flex;
    margin-left: auto;
    transition: transform 0.15s var(--ease);
  }
  .more-chevron.open {
    transform: rotate(90deg);
  }
  .more-sub {
    display: flex;
    flex-direction: column;
    margin-left: 12px;
    padding-left: 8px;
    border-left: 2px solid var(--line-strong);
  }
  .at-muted {
    opacity: 0.8;
  }
</style>
