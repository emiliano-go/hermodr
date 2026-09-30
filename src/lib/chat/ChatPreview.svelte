<script lang="ts">
  import { invoke } from "$lib/utils/ipc";
  import type { MessagePage } from "$lib/utils/message-window";
  import type { StoredMessage } from "$lib/utils/models";
  import { captionOf } from "$lib/utils/message";
  import { phoneLabel } from "$lib/utils/phone";

  let { chat, account, name, x, y }: { chat: string; account: string | null; name: string; x: number; y: number } = $props();
  let rows = $state<StoredMessage[] | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(false);

  $effect(() => {
    const jid = chat;
    void account;
    rows = null;
    error = null;
    loading = false;
    let active = true;
    const timer = setTimeout(async () => {
      loading = true;
      try {
        const page = await invoke<MessagePage>("message_page", { chat: jid, limit: 5 });
        if (active) rows = page.messages.toReversed();
      } catch (e) {
        if (active) error = String(e);
      }
    }, 300);
    return () => { active = false; clearTimeout(timer); };
  });

  function snippet(message: StoredMessage) {
    if (message.deleted || message.revoked) return "Message deleted";
    if (message.media_kind === "view_once") return "One-time media";
    if (message.system_kind) return "System notice";
    return `${message.media_kind ? `[${message.media_kind}] ` : ""}${captionOf(message)}`.slice(0, 160);
  }
</script>

{#if loading}
  <div id="chat-preview" role="tooltip" style:left={`${Math.max(8, Math.min(x, window.innerWidth - 328))}px`} style:top={`${Math.max(8, Math.min(y, window.innerHeight - 240))}px`}>
    <strong>{name}</strong>
    {#if error}<p>Could not load preview: {error}</p>
    {:else if rows === null}<p>Loading…</p>
    {:else if rows.length === 0}<p>No stored messages</p>
    {:else}
      <ul>
        {#each rows as message (message.id)}
          <li><b>{message.from_me ? "You" : message.sender_name || phoneLabel(message.sender.split("@")[0]) || message.sender}:</b> {snippet(message)}</li>
        {/each}
      </ul>
    {/if}
  </div>
{/if}

<style>
  div { position: fixed; z-index: 90; width: min(320px, calc(100vw - 16px)); max-height: calc(100vh - 16px); overflow: hidden; padding: 12px; border: 1px solid var(--line-strong); border-radius: 8px; background: var(--surface); color: var(--text); box-shadow: 0 4px 16px #0003; pointer-events: none; box-sizing: border-box; }
  strong { display: block; margin-bottom: 8px; }
  ul { list-style: none; padding: 0; margin: 0; }
  li { margin-top: 6px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
  b { font-weight: 600; }
  p { margin: 0; font-size: 12px; color: var(--muted); }
</style>
