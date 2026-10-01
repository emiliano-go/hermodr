<script lang="ts">
  import { onMount, tick } from "svelte";
  import ContactSharing from "$lib/contacts/ContactSharing.svelte";
  import ContactCard from "$lib/messages/cards/ContactCard.svelte";
  import { fixture } from "./contact-sharing-ipc";
  import { messages } from "$lib/utils/theme-preview";
  import type { SharedContact, ContactShareScope } from "$lib/utils/vcard";

  let account = $state("synthetic"), chat = $state("synthetic-chat"), generation = $state(1);
  let mode = $state<"links" | "share">("links");
  let canSend = $state(true);
  let canOpen = $state(true), spoiler = $state(false), spoilerRevealed = $state(false), once = $state(false);
  let privateFlags = $state({ revoked: false, deleted: false, system_kind: null as string | null });
  let draft = $state("Synthetic composer draft");
  let result = $state("Waiting");
  let root: HTMLDivElement;
  let sendMode = "fail", release: ((id: string) => void) | null = null;
  const sent: { contacts: SharedContact[]; scope: ContactShareScope }[] = [];
  const opened: string[] = [];
  const choices = ["12025550101", "12025550102"].map((number, index) => ({ jid: `${number}@s.whatsapp.net`, identity: {
    contact_saved: true, saved_name: index ? "Second" : "First", legacy_name: null, push_name: null, username: null, number, own: false,
  } }));
  const message = { ...messages[0], id: "synthetic-contact-card", media_kind: "contact", text: "Synthetic readable legacy card" };
  const assert = (ok: unknown, text: string) => { if (!ok) throw new Error(text); };
  async function until(check: () => boolean, name: string) {
    for (let i = 0; i < 150; i++) { if (check()) return; await new Promise(requestAnimationFrame); }
    throw new Error(`Timeout: ${name}`);
  }
  async function share(contacts: SharedContact[], scope: ContactShareScope) {
    sent.push(structuredClone({ contacts, scope }));
    if (sendMode === "fail") throw new Error("Synthetic send failure");
    if (sendMode === "empty") return "";
    if (sendMode === "hold") return await new Promise<string>((resolve) => { release = resolve; });
    return "synthetic-ack";
  }
  async function chooseBoth() {
    for (const input of root.querySelectorAll<HTMLInputElement>('.sharing input[type="checkbox"]')) {
      if (!input.checked) { input.checked = true; input.dispatchEvent(new Event("change", { bubbles: true })); }
    }
    await tick();
  }
  const sendButton = () => root.querySelector<HTMLButtonElement>('.sharing button')!;
  onMount(() => { void check().catch((error) => result = String(error)); });
  async function check() {
    await until(() => !!root.querySelector('.sharing img'), "own QR");
    assert(root.querySelector<HTMLInputElement>('.sharing input[readonly]')?.value === "https://wa.me/qr/OWN_FIRST_77", "own contact QR token link");
    const input = root.querySelector<HTMLInputElement>('.sharing input[type=url]')!;
    input.value = "https://wa.me/qr/SCAN_TOKEN_77"; input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick(); root.querySelector<HTMLButtonElement>('.sharing button[type=submit]')!.click();
    await until(() => opened.includes("9912345678901234@lid"), "opaque QR resolves server LID and opens chat");
    input.value = "https://wa.me/12025550102"; input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick(); root.querySelector<HTMLButtonElement>('.sharing button[type=submit]')!.click();
    await until(() => opened.includes("12025550102@s.whatsapp.net"), "secondary phone link opens chat");
    input.value = "https://evil.invalid/contact"; input.dispatchEvent(new Event("input", { bubbles: true }));
    await tick(); root.querySelector<HTMLButtonElement>('.sharing button[type=submit]')!.click();
    await tick();
    assert(fixture.calls.filter((call) => call.command === "resolve_contact_link").length === 2, "invalid URL reached native resolver");

    mode = "share"; await tick(); await chooseBoth();
    canSend = false; await tick();
    assert(sendButton().disabled && sent.length === 0, "read-only conversation allowed sending");
    canSend = true; await tick();
    sendButton().click();
    await until(() => !!root.querySelector('.sharing [role=alert]'), "send failure");
    assert(root.querySelectorAll('.sharing input:checked').length === 2, "failure cleared contact choices");
    sendMode = "empty"; sendButton().click();
    await until(() => root.querySelector('.sharing [role=alert]')?.textContent?.includes("not acknowledged") === true, "empty acknowledgement");
    assert(root.querySelectorAll('.sharing input:checked').length === 2, "empty acknowledgement cleared choices");
    sendMode = "ok"; sendButton().click();
    await until(() => root.querySelector('.sharing [role=status]')?.textContent === "2 contacts sent.", "acknowledged multi-contact send");
    assert(root.querySelectorAll('.sharing input:checked').length === 0 && draft === "Synthetic composer draft", "send changed composer draft or retained sent choices");
    assert(JSON.stringify(sent.at(-1)?.contacts) === '[["First","12025550101"],["Second","12025550102"]]', "shared fields differ from explicit choices");

    await chooseBoth(); sendMode = "hold"; sendButton().click();
    await until(() => release !== null, "held send");
    chat = "other-chat"; generation++; await tick();
    release!("late-ack"); release = null; await tick();
    assert(!root.querySelector('.sharing [role=status]') && draft === "Synthetic composer draft", "stale send acknowledged in new conversation");

    await until(() => root.querySelectorAll('.card .contact').length === 2, "received contacts");
    assert(root.querySelectorAll('.card [data-contact-meta]').length === 1 && root.querySelector('.card [data-contact-meta]')?.textContent === "12:34 · Read", "contact metadata missing or duplicated");
    assert(!root.querySelector('.card script, .card img') && !root.querySelector('.card')?.textContent?.includes("private@example.invalid"), "vCard private/remote fields leaked");
    root.querySelector<HTMLButtonElement>('.card .phone button')!.click();
    await until(() => opened.includes("12025550101@s.whatsapp.net"), "contact opens chat");
    canOpen = false; await tick();
    assert(!root.querySelector('.card .phone button'), "readonly card has fake Message action");
    const reads = () => fixture.calls.filter((call) => call.command === "message_contacts").length;
    const before = reads();
    spoiler = true; await tick();
    assert(reads() === before && !root.querySelector('.card')?.textContent?.includes(message.text) && !root.querySelector('.card .contact'), "hidden spoiler fetched or leaked contacts");
    spoilerRevealed = true; await tick();
    await until(() => root.querySelectorAll('.card .contact').length === 2, "explicit spoiler reveal");
    assert(fixture.calls.filter((call) => call.command === "message_contacts").at(-1)?.args?.revealSpoiler === true, "spoiler reveal not explicit at native boundary");
    const revealedReads = reads();
    once = true; await tick();
    assert(reads() === revealedReads && !root.querySelector('.card .contact'), "view-once contacts fetched or leaked");
    once = spoiler = spoilerRevealed = false; await tick();
    for (const flags of [{ revoked: true, deleted: false, system_kind: null }, { revoked: false, deleted: true, system_kind: null }, { revoked: false, deleted: false, system_kind: "UNAVAILABLE_MESSAGE" }]) {
      const currentReads = reads();
      privateFlags = flags; await tick();
      assert(reads() === currentReads && !root.querySelector('.card')?.textContent?.includes(message.text) && !root.querySelector('.card .contact'), "private direct card fetched or exposed fallback");
    }
    privateFlags = { revoked: false, deleted: false, system_kind: null }; await tick();
    fixture.failCards = true; generation++; await tick();
    await until(() => !!root.querySelector('.card [role=alert]'), "payload failure fallback");
    assert(root.querySelector('.card')?.textContent?.includes(message.text), "payload failure lost readable fallback");
    account = "other"; mode = "links"; await tick();
    await until(() => root.querySelector<HTMLInputElement>('.sharing input[readonly]')?.value === "https://wa.me/qr/OWN_OTHER_77", "account-specific QR token");
    assert(draft === "Synthetic composer draft", "account switch changed composer draft");
    result = "PASS: own opaque QR, scanned token resolves server LID, phone link secondary, URI guard, multi-contact fields, failure/ack/stale scope, draft retention, readonly actions, spoiler/V0 fetch guards, receive fallback, account QR isolation";
  }
</script>

<h1>Contact sharing</h1>
<pre aria-label="Contact sharing result">{result}</pre>
<label>Unrelated composer draft<input bind:value={draft} /></label>
<div bind:this={root}>
  <div class="sharing"><ContactSharing {account} {chat} {generation} {mode} {canSend} connected {choices} onopenchat={(jid) => { opened.push(jid); }} onshare={share} /></div>
  <div class="card"><ContactCard message={{ ...message, ...privateFlags, spoiler, media_once_kind: once ? "" : null }} {account} {generation} {spoilerRevealed} onopenchat={canOpen ? (jid) => { opened.push(jid); } : undefined}>
    {#snippet children()}<span data-private-fallback>{message.text}</span>{/snippet}
    {#snippet meta()}<span data-contact-meta>12:34 · Read</span>{/snippet}
  </ContactCard></div>
</div>

<style>
  :global(body) { margin: 24px; color: #e5e7eb; background: #111418; font: 14px "Segoe UI", sans-serif; --text: #e5e7eb; --muted: #95a1b2; --link: #79bce8; --accent: #79bce8; --danger: #f38b92; --raised: #252a32; --line: #424955; --radius: 8px; --radius-sm: 5px; }
  h1 { font-size: 20px; } pre { white-space: pre-wrap; }
  label { display: grid; gap: 5px; width: 380px; margin-bottom: 20px; }
  .sharing { width: 400px; margin-bottom: 24px; }
  .card { width: 400px; }
</style>
