<!-- The open conversation's header bar: title, presence, tools, the
  jump-to-mention pill and the pinned-message bar. Moved out of +page.svelte. -->
<script lang="ts">
  import Avatar from "$lib/Avatar.svelte";
  import Button from "$lib/Button.svelte";
  import Icon from "$lib/Icon.svelte";

  let {
    selectedChat,
    isGroup,
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
    onpings,
    onsettings,
    onjumpmention,
    onpinnedjump,
  }: {
    selectedChat: string;
    isGroup: boolean;
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
    onpings: () => void;
    onsettings: () => void;
    onjumpmention: () => void;
    onpinnedjump: (id: string) => void;
  } = $props();
</script>

<header class="chat-header">
  <div class="chat-heading">
    {#if isGroup}
      <button class="heading-avatar" title="Group info" aria-label="Group info" onclick={ongroupinfo}
        ><Avatar src={avatar} label={title} seed={selectedChat} /></button
      >
      <button class="chat-title" title="Group info" onclick={ongroupinfo}>
        {title}
        <span class="chat-sub" class:typing={typingNow}
          >{typingNow ?? ([groupContext, subtitle].filter(Boolean).join(" · ") || null) ??" "}</span
        >
      </button>
    {:else}
      <Avatar src={avatar} label={title} seed={selectedChat} />
      <span class="chat-title">
        {title}
        {#if typingNow}<span class="chat-sub typing">{typingNow}</span
          >{:else if presenceText}<span class="chat-sub">{presenceText}</span>{/if}
      </span>
    {/if}
  </div>
  <div class="header-tools">
    <Button
      variant="icon"
      icon="search"
      iconSize={18}
      title="Search in this chat"
      aria-label="Search in this chat"
      onclick={onsearch} />
    {#if isGroup}
      <Button
        variant="icon"
        icon="at"
        iconSize={18}
        title="Your mentions in this group"
        aria-label="Your mentions in this group"
        onclick={onpings} />
    {/if}
    <Button
      variant="icon"
      icon="sliders"
      iconSize={18}
      title="Chat settings"
      aria-label="Chat settings"
      onclick={onsettings} />
  </div>
  {#if mentionTotal > 0}
    <button class="jump-mention" title="Jump to mention" onclick={onjumpmention}>
      <Icon name="at" size={14} />
      {mentionCursor}/{mentionTotal}
    </button>
  {/if}
</header>

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
    overflow: hidden;
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
    margin-left: auto;
    flex: none;
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
    text-align: left;
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
    text-align: left;
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
