<script lang="ts">
  import { LocalizedError, normalizeError } from "$lib/i18n/errors";
  import { t } from "$lib/i18n/localizer";
  import type { Snippet } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import Embed from "$lib/messages/cards/Embed.svelte";
  import { phoneLabel } from "$lib/utils/phone";
  import { isUnavailable } from "$lib/utils/message";
  import { parseContactCards, contactPhoneJid, type ParsedContact } from "$lib/utils/vcard";
  import type { StoredMessage } from "$lib/utils/wire";

  let { message, account, generation = 0, spoilerRevealed = false, onopenchat, children, meta }: {
    message: StoredMessage;
    account: string | null;
    generation?: number;
    spoilerRevealed?: boolean;
    onopenchat?: (jid: string) => void | Promise<void>;
    children?: Snippet;
    meta?: Snippet;
  } = $props();

  let contacts = $state<ParsedContact[]>([]);
  let error = $state<LocalizedError | string | null>("");
  let attempt = $state(0);
  let pending = $state<string | null>(null);
  let revision = 0;
  const hidden = $derived(message.revoked || message.deleted || isUnavailable(message) || message.media_once_kind != null || message.media_kind === "view_once" || (message.spoiler && !spoilerRevealed));

  $effect(() => {
    const id = account, chat = message.chat, messageId = message.id;
    generation; attempt; spoilerRevealed;
    const epoch = ++revision;
    contacts = [];
    error = "";
    pending = null;
    if (!id || hidden || message.revoked || message.deleted) return;
    invoke<[string, string][]>("message_contacts", { account: id, chat, id: messageId, revealSpoiler: message.spoiler && spoilerRevealed }).then((cards) => {
      if (epoch === revision) contacts = parseContactCards(cards);
    }).catch((failure) => { if (epoch === revision) error = normalizeError(failure); });
    return () => { ++revision; };
  });

  async function open(jid: string) {
    if (!account || hidden || pending || !onopenchat) return;
    const epoch = revision;
    pending = jid;
    error = "";
    try { await onopenchat(jid); }
    catch (failure) { if (epoch === revision) error = normalizeError(failure); }
    finally { if (epoch === revision) pending = null; }
  }
</script>

<Embed label="👤 Contact">
  {#if hidden}
    <span>{message.spoiler && !spoilerRevealed ? t("content.spoiler_message") : message.revoked || message.deleted ? t("content.contact_message_removed") : isUnavailable(message) ? t("content.contact_message_unavailable") : t("content.view_once_contact")}</span>
  {:else if contacts.length}
    {#each contacts as contact, index (index)}
      <div class="contact">
        <strong><bdi dir="auto">{contact.name}</bdi></strong>
        {#each contact.phones as phone (phone)}
          {@const jid = contactPhoneJid(phone)}
          <div class="phone"><span>{jid ? phoneLabel(jid.split("@")[0]) ?? phone : phone}</span>
            {#if jid && onopenchat}<button type="button" disabled={!!pending} onclick={() => open(jid)} aria-label={t("content.message_value_at_value", { param0: (contact.name), param1: (phone) })}>{t("content.message")}</button>{/if}</div>
        {/each}
      </div>
    {/each}
  {:else if children}
    {@render children()}
  {:else}
    <span class="fallback"><bdi dir="auto">{message.text}</bdi></span>
  {/if}
  {#if error}<span class="error" role="alert">{error}</span><button type="button" class="retry" onclick={() => attempt++}>{t("content.reload_contact")}</button>{/if}
  {#if meta}<span class="meta">{@render meta()}</span>{/if}
</Embed>

<style>
  .contact { display: grid; gap: 5px; }
  .contact + .contact { border-top: 1px solid var(--line); padding-top: 8px; }
  strong { font-size: 0.875rem; font-weight: 600; overflow-wrap: anywhere; }
  .phone { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; font-size: 0.8125rem; }
  .phone span { overflow-wrap: anywhere; }
  button { border: 1px solid var(--line); border-radius: var(--radius-sm); color: var(--link); background: var(--raised); padding: 3px 7px; font: inherit; font-size: 0.75rem; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .retry { align-self: start; }
  .fallback { white-space: pre-wrap; overflow-wrap: anywhere; }
  .error { color: var(--danger); font-size: 0.75rem; overflow-wrap: anywhere; }
  .meta { display: flex; align-items: center; align-self: flex-end; gap: 3px; font-size: 0.6875rem; line-height: 15px; font-variant-numeric: tabular-nums; color: color-mix(in srgb, var(--text) 60%, transparent); white-space: nowrap; }
</style>
