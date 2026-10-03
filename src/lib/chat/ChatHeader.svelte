<!-- The open conversation's header bar: title, presence, tools, the
  jump-to-mention pill and the pinned-message bar. Moved out of +page.svelte. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import Avatar from "$lib/ui/Avatar.svelte";
  import Button from "$lib/ui/Button.svelte";
  import Icon from "$lib/ui/Icon.svelte";

  let {
    selectedChat,
    isGroup,
    isBroadcast = false,
    title,
    avatar,
    typingNow,
    subtitle,
    groupContext,
    presenceText,
    mentionTotal,
    mentionCursor,
    pinned,
    ongroupinfo,
    onsearch,
    ongallery,
    onpings,
    onsettings,
    onjumpmention,
    onpinnedjump,
    onclearchat,
    ondeletechat,
  }: {
    selectedChat: string;
    isGroup: boolean;
    isBroadcast?: boolean;
    title: string;
    avatar: string | null;
    typingNow: string | null;
    subtitle: string | null;
    groupContext: string | null;
    presenceText: string | null;
    mentionTotal: number;
    mentionCursor: number;
    pinned: { id: string; author: string; body: string } | null;
    ongroupinfo: () => void;
    onsearch: () => void;
    ongallery: () => void;
    onpings: () => void;
    onsettings: () => void;
    onjumpmention: () => void;
    onpinnedjump: (id: string) => void;
    onclearchat: () => void;
    ondeletechat: () => void;
  } = $props();

  let optionsOpen = $state(false);

  function toggleOptions(event: MouseEvent) {
    event.stopPropagation();
    optionsOpen = !optionsOpen;
  }

  function closeOptions() {
    optionsOpen = false;
  }
</script>

<header class="chat-header">
  <div class="chat-heading">
    {#if isGroup}
      <button class="heading-avatar" title={t("group.info")} aria-label={t("group.info")} onclick={ongroupinfo}
        ><Avatar src={avatar} label={title} seed={selectedChat} /></button
      >
      <button class="chat-title" title={t("group.info")} onclick={ongroupinfo}>
        {title}
        <span class="chat-sub" class:typing={typingNow}
          >{typingNow ?? ([groupContext, subtitle].filter(Boolean).join(" · ") || null) ??" "}</span
        >
      </button>
    {:else if isBroadcast}
      <button class="heading-avatar" title={t("chat.broadcast_recipients")} aria-label={t("chat.broadcast_recipients")} onclick={ongroupinfo}
        ><Avatar src={avatar} label={title} seed={selectedChat} /></button>
      <button class="chat-title" title={t("chat.broadcast_recipients")} onclick={ongroupinfo}>
        {title}<span class="chat-sub">{t("chat.broadcast_list")}</span>
      </button>
    {:else}
      <button class="heading-avatar" title={t("contact.info")} aria-label={t("contact.info")} onclick={ongroupinfo}
        ><Avatar src={avatar} label={title} seed={selectedChat} /></button
      >
      <button class="chat-title" title={t("contact.info")} onclick={ongroupinfo}>
        {title}
        {#if typingNow}<span class="chat-sub typing">{typingNow}</span
          >{:else if presenceText}<span class="chat-sub">{presenceText}</span>{/if}
      </button>
    {/if}
  </div>
  <div class="header-tools">
    <Button variant="icon" icon="image" iconSize={18} title={t("chat.gallery")} aria-label={t("chat.gallery")} onclick={ongallery} />
    <Button
      variant="icon"
      icon="search"
      iconSize={18}
      title={t("chat.search_in")}
      aria-label={t("chat.search_in")}
      onclick={onsearch} />
    {#if isGroup}
      <Button
        variant="icon"
        icon="at"
        iconSize={18}
        title={t("chat.your_mentions")}
        aria-label={t("chat.your_mentions")}
        onclick={onpings} />
    {/if}
    <Button
      variant="icon"
      icon="sliders"
      iconSize={18}
      title={t("chat.settings")}
      aria-label={t("chat.settings")}
      onclick={onsettings} />
    <div class="options-wrap">
      <Button
        variant="icon"
        iconSize={18}
        title={t("chat.options")}
        aria-label={t("chat.options")}
        aria-expanded={optionsOpen}
        onclick={toggleOptions}>⋯</Button>
      {#if optionsOpen}
        <div class="options-menu" role="menu">
          <Button
            variant="menu"
            icon="edit"
            iconSize={15}
            role="menuitem"
            onclick={() => {
              closeOptions();
              onclearchat();
            }}>{t("chat.clear")}</Button>
          <Button
            variant="menu"
            icon="trash"
            iconSize={15}
            role="menuitem"
            onclick={() => {
              closeOptions();
              ondeletechat();
            }}>{t("chat.delete")}</Button>
        </div>
      {/if}
    </div>
  </div>
  {#if mentionTotal > 0}
    <button class="jump-mention" title={t("chat.jump_mention")} onclick={onjumpmention}>
      <Icon name="at" size={14} />
      {mentionCursor}/{mentionTotal}
    </button>
  {/if}
</header>

<svelte:window
  onclick={(e) => {
    if (optionsOpen && !(e.target as Element).closest?.(".options-wrap")) closeOptions();
  }}
  onkeydown={(e) => {
    if (e.key === "Escape") closeOptions();
  }} />

{#if pinned}
  <button class="pinned-bar" onclick={() => onpinnedjump(pinned.id)}>
    <Icon name="pin" size={16} />
    <span class="pinned-text">
      <strong>{pinned.author}:</strong>
      {pinned.body}
    </span>
  </button>
{/if}

<style>
  .chat-header {
    min-width: 0;
    height: 59px;
    box-sizing: border-box;
    flex: none;
    overflow: visible;
    padding: 0 12px 0 16px;
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    background: var(--surface);
  }
  .chat-heading {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .header-tools {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-inline-start: auto;
    flex: none;
    overflow: visible;
  }
  .options-wrap {
    position: relative;
  }
  .options-menu {
    position: absolute;
    top: calc(100% + 6px);
    inset-inline-end: 0;
    z-index: 50;
    min-width: 180px;
    display: flex;
    flex-direction: column;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .heading-avatar {
    flex: none;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    cursor: pointer;
  }
  .heading-avatar:hover {
    filter: brightness(1.12);
  }
  .chat-title {
    display: flex;
    flex-direction: column;
    min-width: 0;
    background: transparent;
    border: 0;
    color: inherit;
    font: inherit;
    font-weight: 600;
    padding: 0;
    text-align: start;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  button.chat-title {
    cursor: pointer;
  }
  button.chat-title:hover .chat-sub {
    color: var(--accent-text);
  }
  .chat-sub {
    font-size: 13px;
    font-weight: 400;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .chat-sub.typing {
    color: var(--accent);
  }
  .chat-title {
    font-size: 16px;
    font-weight: 400;
  }
  .jump-mention {
    display: flex;
    align-items: center;
    gap: 4px;
    background: var(--accent-soft);
    color: var(--accent-text);
    border: 0;
    border-radius: 999px;
    padding: 4px 10px;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .pinned-bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 16px;
    border: 0;
    border-top: 1px solid var(--line);
    background: var(--surface);
    color: var(--muted);
    font: inherit;
    font-size: 13.5px;
    text-align: start;
    cursor: pointer;
  }
  .pinned-bar:hover {
    background: var(--raised);
  }
  .pinned-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .pinned-text strong {
    color: var(--text);
    font-weight: 600;
  }
</style>
