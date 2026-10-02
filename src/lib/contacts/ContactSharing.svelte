<script lang="ts">
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
  let error = $state("");
  let result = $state("");
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
      if (contactLinkJid(link) !== null) throw new Error("WhatsApp returned no contact QR link.");
      const svg = await invoke<string>("qr_svg", { value: link });
      if (id !== account || epoch !== revision || !connected) return;
      ownLink = link;
      qr = `data:image/svg+xml,${encodeURIComponent(svg)}`;
    } catch (failure) {
      if (id === account && epoch === revision) error = String(failure);
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
      if (!isContactJid(jid) || (expected !== null && jid !== expected)) throw new Error("Contact link resolved to an invalid contact address.");
      await onopenchat(jid);
      if (id === account && epoch === revision) result = "Chat opened.";
    } catch (failure) {
      if (id === account && epoch === revision) error = String(failure);
    } finally {
      if (id === account && epoch === revision) busy = false;
    }
  }

  async function copyLink() {
    const id = account, epoch = revision, link = ownLink;
    if (!id || !link) return;
    try {
      await navigator.clipboard.writeText(link);
      if (id === account && epoch === revision) result = "Contact link copied.";
    } catch {
      if (id === account && epoch === revision) error = "Could not copy the link. Select and copy it below.";
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
      result = batch.length === 1 ? "Contact sent." : `${batch.length} contacts sent.`;
    } catch (failure) {
      if (epoch === revision) error = String(failure);
    } finally {
      if (epoch === revision) busy = false;
    }
  }
</script>

<section aria-label={mode === "links" ? "Contact QR and link" : "Share contacts"}>
  {#if mode === "links"}
    <h3>Your contact link</h3>
    {#if qr}<img class="qr" src={qr} alt="QR code for your WhatsApp contact link" />{/if}
    {#if ownLink}
      <label>Contact link<input value={ownLink} readonly /></label>
      <button type="button" onclick={copyLink}>Copy link</button>
    {:else if account && connected}
      <button type="button" disabled={busy} onclick={() => account && loadLink(account)}>{busy ? "Loading contact link…" : "Load contact link"}</button>
    {/if}
    <form onsubmit={(event) => { event.preventDefault(); void resolve(); }}>
      <label>Paste a contact link<input type="url" bind:value={pasted} maxlength="2048" placeholder="https://wa.me/qr/…" required disabled={busy || !account || !connected} /></label>
      <button type="submit" disabled={busy || !account || !connected || !pasted.trim()}>Open chat</button>
    </form>
  {:else}
    <h3>Share contacts</h3>
    <p>Only each selected contact's name and phone number will be shared.</p>
    {#if !contacts.length}<p>No contacts with a known phone number are available.</p>{/if}
    <fieldset disabled={busy || !account}>
      {#each contacts as contact (contact.phone)}
        <label class="choice"><input type="checkbox" value={contact.phone} bind:group={selected}
          disabled={!selected.includes(contact.phone) && selected.length >= MAX_SHARED_CONTACTS} />
          <span><strong>{contact.name}</strong><small>{phoneLabel(contact.phone) ?? `+${contact.phone}`}</small></span></label>
      {/each}
    </fieldset>
    {#if broadcastSendReason(chat)}<p role="status">{broadcastSendReason(chat)}</p>{/if}
    <button type="button" disabled={busy || !connected || !canSend || !!broadcastSendReason(chat) || !account || !chat || !onshare || !selected.length} onclick={send}>
      {busy ? "Sending…" : `Send ${selected.length || "selected"} ${selected.length === 1 ? "contact" : "contacts"}`}
    </button>
  {/if}
  {#if !connected}<p>Connect WhatsApp to share contacts or open a contact link.</p>{/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
  {#if result}<p role="status">{result}</p>{/if}
</section>

<style>
  section, form { display: grid; gap: 10px; }
  h3 { margin: 0; font-size: 15px; }
  p { margin: 0; color: var(--muted); font-size: 12px; }
  label { display: grid; gap: 5px; font-size: 13px; }
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
