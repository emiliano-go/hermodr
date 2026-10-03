<script lang="ts">
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import MessageCard from "$lib/messages/cards/MessageCard.svelte";
  import Icon from "$lib/ui/Icon.svelte";
  import { isPollNotice, structuredNoticeText } from "$lib/utils/structured-notices";
  import type { Poll, StoredMessage } from "$lib/utils/models";

  let { message, poll, namer, picture, onvote, highlighted = false }: {
    message: StoredMessage;
    poll?: Poll;
    namer: (jid: string) => string;
    picture: (jid: string) => string | null;
    onvote: (options: string[]) => Promise<void>;
    highlighted?: boolean;
  } = $props();

  let dialog = $state<HTMLDialogElement>();
  let error = $state<LocalizedError | string | null>("");
  const line = $derived(structuredNoticeText(message, namer));

  async function vote(options: string[]) {
    error = "";
    try { await onvote(options); }
    catch (cause) { error = normalizeError(cause); }
  }
</script>

{#if line}
  {#if isPollNotice(message)}
    <button class="system poll-notice bubble" class:highlighted data-id={message.id} data-chat={message.chat}
      aria-haspopup="dialog" onclick={() => { error = ""; dialog?.showModal(); }}>
      <Icon name="poll" size={14} /> {line} <span class="open">{t("content.open_poll")}</span>
    </button>
    <dialog bind:this={dialog} aria-label={t("content.poll")} oncancel={(event) => event.stopPropagation()}
      onclick={(event) => { if (event.target === dialog) dialog?.close(); }}
      onkeydown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}>
      <header><h2>{t("content.poll")}</h2><button class="close" aria-label={t("content.close_poll")} onclick={() => dialog?.close()}><Icon name="x" size={18} /></button></header>
      <MessageCard variant="poll" {poll} question={message.text} {namer} {picture} onvote={vote} />
      {#if error}<p class="error" role="alert">{error}</p>{/if}
    </dialog>
  {:else}
    <p class="system" class:highlighted data-id={message.id} data-chat={message.chat}>{line}</p>
  {/if}
{/if}

<style>
  .system { align-self: center; max-width: min(60ch, 80%); margin: 6px 0; padding: 5px 12px; border: 0; background: var(--surface); color: var(--muted); border-radius: var(--radius-sm); font: inherit; font-size: 0.7812rem; text-align: center; box-shadow: 0 1px 0.5px rgba(11, 20, 26, 0.13); overflow-wrap: anywhere; }
  .poll-notice { display: flex; flex-wrap: wrap; align-items: center; justify-content: center; gap: 6px; cursor: pointer; }
  .poll-notice:hover, .poll-notice:focus-visible { color: var(--text); }
  .highlighted { outline: 2px solid var(--accent); outline-offset: 2px; }
  .open { color: var(--link); text-decoration: underline; }
  dialog { width: min(420px, calc(100vw - 32px)); max-height: calc(100vh - 32px); box-sizing: border-box; padding: 20px; border: 1px solid var(--line-strong); border-radius: var(--radius-lg); background: var(--surface); color: var(--text); box-shadow: var(--shadow); overflow: auto; }
  dialog::backdrop { background: var(--scrim); }
  dialog :global(.poll) { min-width: 0; }
  header { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 12px; }
  h2 { margin: 0; font-size: 1rem; }
  .close { display: grid; place-items: center; padding: 5px; border: 0; border-radius: var(--radius-sm); background: transparent; color: var(--muted); cursor: pointer; }
  .close:hover { color: var(--text); background: var(--raised-2); }
  .error { color: var(--danger); font-size: 0.8125rem; }
</style>
