<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import Embed from "./Embed.svelte";
  import { mediaSrc } from "$lib/media/MediaViewer.svelte";
  import { replyIcon } from "$lib/utils/message";
  import type { BubbleApi, StoredMessage } from "$lib/utils/models";

  let { message, author, text, chatName, api }: {
    message: StoredMessage; author: string | null; text: string | null; chatName: string | null; api: BubbleApi;
  } = $props();
  const sentHere = $derived(message.from_me && message.reply_to_view_once && !message.reply_to_recoverable && !message.reply_to_path);
  const onceCopy = $derived(message.reply_to_view_once && message.reply_to_recoverable && !sentHere);
</script>

<Embed compact tooltip={message.reply_to_path ? "Open the copy" : onceCopy ? "Save the copy" : sentHere
  ? "Only a reply sent from your phone, or by someone else, carries a copy of a view-once" : "Go to message"}
  label={author} {text}
  image={message.reply_to_thumb && ["image", "sticker", "gif"].includes(message.reply_to_kind ?? "") ? mediaSrc(message.reply_to_thumb) : null}
  icon={message.reply_to_kind ? replyIcon(message.reply_to_kind) : null}
  onclick={message.reply_to_path ? () => api.onopenquote(message) : onceCopy ? () => api.onrecoverquote(message) : () => api.onjumpquoted(message)}>
  {#if chatName}<span class="quote-where">{t("content.in")} {chatName}</span>{/if}
  {#if onceCopy && !message.reply_to_path}
    <span class="quote-once"><span class="once-mark">1</span>{api.recovering[message.id] ? t("content.saving_the_copy") : t("content.tap_to_save")}</span>
  {:else if sentHere}
    <span class="quote-once"><span class="once-mark">1</span>{t("content.no_copy_from_this_app")}</span>
  {/if}
</Embed>

<style>
  .quote-where { flex: none; max-width: 16ch; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 11px; color: var(--muted); }
  .quote-once { display: flex; flex: none; align-items: center; gap: 4px; margin-inline-start: auto; font-size: 11px;
    color: color-mix(in srgb, var(--text) 60%, transparent); white-space: nowrap; }
  .once-mark { display: grid; place-items: center; width: 26px; height: 26px; flex: none; border: 2px dashed var(--accent);
    border-radius: 50%; color: var(--accent); font-size: 12px; font-weight: 700; }
</style>
