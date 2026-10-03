<script lang="ts">
  import { t } from "$lib/i18n/localizer";
  import { normalizeError, type LocalizedError } from "$lib/i18n/errors";
  import { untrack } from "svelte";
  import { invoke } from "$lib/utils/ipc";
  import { broadcastSendReason, guardBroadcastSend } from "$lib/utils/broadcast";
  import { displayName, phoneLabel } from "$lib/utils/phone";
  import { contactPhone, contactLinkJid, isContactJid, shareContacts, MAX_SHARED_CONTACTS, type SharedContact, type ContactShareScope } from "$lib/utils/vcard";
  import type { ContactIdentity } from "$lib/utils/wire";

  let { account, connected, canSend = true, chat = null, generation = 0, mode = "links", choices = [], onopenchat, onshare }: {
    account: string | null;
    connected: boolean;
    canSend?: boolean;
    chat?: string | null;
    generation?: number;
    mode?: "links" | "share";
    choices?: { jid: string; identity: ContactIdentity }[];
    onopenchat: (jid: string) => void | Promise<void>;
    onshare?: (contacts: SharedContact[], scope: ContactShareScope) => Promise<string>;
  } = $props();

  let ownLink = $state("");
  let qr = $state("");
  let pasted = $state("");
  let selected = $state<string[]>([]);
  let error = $state<LocalizedError | string>("");
  let result = $state("");
  let sentCount = $state(0);
  let busy = $state(false);
  let revision = 0;

  const contacts = $derived.by(() => {
    const seen = new Set<string>();
    return choices.flatMap(({ jid, identity }) => {
      if (!/^\d+@(s\.whatsapp\.net|lid)$/.test(jid)) return [];
      const source = identity.number ?? (/^\d+@s\.whatsapp\.net$/.test(jid) ? jid.split("@")[0] : null);
      if (!source) return [];
      let phone: string;
      try { phone = contactPhone(source); } catch { return []; }
      if (seen.has(phone)) return [];
      seen.add(phone);
      return [{ phone, name: displayName(identity.push_name, jid, identity) }];
    });
  });

  $effect(() => {
    account; chat; generation; mode;
    ++revision;
    ownLink = qr = pasted = error = result = "";
    selected = [];
    busy = false;
    return () => { ++revision; };
  });
  $effect(() => {
    const id = account, online = connected, view = mode;
    generation;
    const epoch = ++revision;
    if (!online) { busy = false; ownLink = qr = result = ""; return; }
    if (id && view === "links") untrack(() => { void loadLink(id, epoch); });
    return () => { ++revision; };
  });

  async function loadLink(id: string, epoch = revision) {
    busy = true;
    error = "";
    try {
      const link = await invoke<string>("own_contact_link", { account: id });
      if (id !== account || epoch !== revision || !connected) return;
      if (contactLinkJid(link) !== null) throw normalizeError({ kind: "postal_error", code: "error.contact_qr_missing", params: {} });
      const svg = await invoke<string>("qr_svg", { value: link });
      if (id !== account || epoch !== revision || !connected) return;
      ownLink = link;
      qr = `data:image/svg+xml,${encodeURIComponent(svg)}`;
    } catch (failure) {
      if (id === account && epoch === revision) error = normalizeError(failure);
    } finally {
      if (id === account && epoch === revision) busy = false;
    }
  }

  async function resolve() {
    if (!account || !connected || busy) return;
    const id = account, epoch = revision, link = pasted.trim();
    busy = true;
    error = result = "";
    try {
      const expected = contactLinkJid(link);
      const jid = await invoke<string>("resolve_contact_link", { account: id, link });
      if (id !== account || epoch !== revision || !connected) return;
      if (!isContactJid(jid) || (expected !== null && jid !== expected)) throw normalizeError({ kind: "postal_error", code: "error.contact_link_invalid", params: {} });
      await onopenchat(jid);
      if (id === account && epoch === revision) result = "contact.chat_opened";
    } catch (failure) {
      if (id === account && epoch === revision) error = normalizeError(failure);
    } finally {
      if (id === account && epoch === revision) busy = false;
    }
  }

  async function copyLink() {
    const id = account, epoch = revision, link = ownLink;
    if (!id || !link) return;
    try {
      await navigator.clipboard.writeText(link);
      if (id === account && epoch === revision) result = "contact.link_copied";
    } catch {
      if (id === account && epoch === revision) error = normalizeError({ kind: "postal_error", code: "error.contact_link_copy", params: {} });
    }
  }

  async function send() {
    if (!account || !chat || !connected || !canSend || busy || !onshare) return;
    const reason = broadcastSendReason(chat);
    if (reason) { error = reason; return; }
    const epoch = revision, scope = { account, chat, generation };
    const batch = contacts.filter((contact) => selected.includes(contact.phone)).map((contact) => [contact.name, contact.phone] as SharedContact);
    busy = true;
    error = result = "";
    try {
      guardBroadcastSend(scope.chat);
      const ack = await shareContacts(onshare, () => connected && canSend && account && chat && !broadcastSendReason(chat)
        ? { account, chat, generation } : null, scope, batch);
      if (!ack || epoch !== revision) return;
      selected = [];
      sentCount = batch.length;
      result = "contact.sent";
    } catch (failure) {
      if (epoch === revision) error = normalizeError(failure);
    } finally {
      if (epoch === revision) busy = false;
    }
  }
</script>

<section aria-label={mode === "links" ? t("contact.qr_and_link") : t("contact.share")}>
  {#if mode === "links"}
    <h3>{t("contact.your_link")}</h3>
    {#if qr}<img class="qr" src={qr} alt={t("contact.qr_alt")} />{/if}
    {#if ownLink}
      <label>{t("contact.link")}<input dir="ltr" value={ownLink} readonly /></label>
      <button type="button" onclick={copyLink}>{t("contact.copy_link")}</button>
    {:else if account && connected}
      <button type="button" disabled={busy} onclick={() => account && loadLink(account)}>{busy ? t("contact.link_loading") : t("contact.link_load")}</button>
    {/if}
    <form onsubmit={(event) => { event.preventDefault(); void resolve(); }}>
      <label>{t("contact.link_paste")}<input type="url" dir="ltr" bind:value={pasted} maxlength="2048" placeholder={t("contact.link_example")} required disabled={busy || !account || !connected} /></label>
      <button type="submit" disabled={busy || !account || !connected || !pasted.trim()}>{t("chat.open")}</button>
    </form>
  {:else}
    <h3>{t("contact.share")}</h3>
    <p>{t("contact.share_hint")}</p>
    {#if !contacts.length}<p>{t("contact.share_empty")}</p>{/if}
    <fieldset disabled={busy || !account}>
      {#each contacts as contact (contact.phone)}
        <label class="choice"><input type="checkbox" value={contact.phone} bind:group={selected}
          disabled={!selected.includes(contact.phone) && selected.length >= MAX_SHARED_CONTACTS} />
          <span><strong><bdi>{contact.name}</bdi></strong><small><bdi>{phoneLabel(contact.phone) ?? `+${contact.phone}`}</bdi></small></span></label>
      {/each}
    </fieldset>
    {#if broadcastSendReason(chat)}<p role="status">{broadcastSendReason(chat)}</p>{/if}
    <button type="button" disabled={busy || !connected || !canSend || !!broadcastSendReason(chat) || !account || !chat || !onshare || !selected.length} onclick={send}>
      {busy ? t("ui.sending") : selected.length ? t("contact.send_count", { count: selected.length }) : t("contact.send_selected")}
    </button>
  {/if}
  {#if !connected}<p>{t("contact.share_connect")}</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if result}<p role="status">{t(result, { count: sentCount })}</p>{/if}
</section>

<style>
  section, form { display: grid; gap: 10px; }
  h3 { margin: 0; font-size: 0.9375rem; }
  p { margin: 0; color: var(--muted); font-size: 0.75rem; }
  label { display: grid; gap: 5px; font-size: 0.8125rem; }
  .qr { width: 180px; height: 180px; background: #fff; padding: 8px; border-radius: var(--radius); }
  button, input { font: inherit; color: var(--text); background: var(--raised); border: 1px solid var(--line); border-radius: var(--radius); padding: 8px; }
  input { width: 100%; min-width: 0; box-sizing: border-box; }
  button { justify-self: start; cursor: pointer; }
  button:disabled { opacity: .5; cursor: default; }
  button:focus-visible, input:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  fieldset { display: grid; gap: 8px; border: 0; padding: 0; margin: 0; max-height: 280px; overflow: auto; }
  .choice { display: flex; align-items: center; gap: 8px; }
  .choice input { width: auto; }
  .choice span { display: grid; gap: 2px; min-width: 0; }
  .choice strong { overflow-wrap: anywhere; font-weight: 500; }
  small { color: var(--muted); }
  .error { color: var(--danger); overflow-wrap: anywhere; }
</style>
