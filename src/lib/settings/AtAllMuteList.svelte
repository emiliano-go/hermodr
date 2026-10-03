<!-- Chats muting @all mentions, each with a button to turn that mute off.
  Shown in Settings → Notifications while the mute-everywhere switch is off. -->
<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { chats } from "$lib/state/chats.svelte";

  const muted = $derived(chats.chats.filter((chat) => chat.mute_at_all));
  let busy = $state<string | null>(null);

  async function unmute(chat: string) {
    busy = chat;
    try {
      await chats.chatAction("set_chat_mute_at_all", { chat, muted: false });
    } finally {
      if (busy === chat) busy = null;
    }
  }
</script>

{#if muted.length === 0}
  <p class="muted">{t("settings.all_mute_empty")}</p>
{:else}
  <ul class="mute-list">
    {#each muted as chat (chat.chat)}
      <li>
        <span class="mute-name">{chats.chatLabel(chat)}</span>
        <button
          class="button"
          disabled={busy === chat.chat}
          onclick={() => unmute(chat.chat)}>
          {busy === chat.chat ? t("settings.unmuting") : t("settings.all_unmute")}
        </button>
      </li>
    {/each}
  </ul>
{/if}

<style>
  .muted {
    margin: 4px 0;
    color: var(--muted);
    font-size: 0.8125rem;
  }
  .mute-list {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .mute-list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 12px;
    background: var(--surface);
    border-radius: var(--radius);
  }
  .mute-name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 0.875rem;
  }
</style>
