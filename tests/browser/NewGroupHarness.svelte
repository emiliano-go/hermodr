<script lang="ts">
  import { onMount, tick } from "svelte";
  import NewGroup from "$lib/chat/NewGroup.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { GroupCreateResult, SearchResult } from "$lib/utils/wire";

  session.activeAccount = "synthetic-group-account-a";
  let opened = $state(false);
  let mode = "partial";
  let release: (() => void) | undefined;
  let openFails = true;
  let checks = $state<string[]>([]);
  let failure = $state("");
  let operations = $state<{ action: string; account: string | null; jid?: string; subject?: string; jids?: string[] }[]>([]);
  const contacts: SearchResult[] = ["Ada", "Bea", "Cy"].map((name, i) => ({
    name, jid: `${100 + i}@s.whatsapp.net`, number: `${100 + i}`, kind: "contact", saved: true, has_messages: false, aliases: [],
  }));

  async function create(subject: string, jids: string[]): Promise<GroupCreateResult> {
    const account = session.activeAccount;
    operations.push({ action: "create", account, subject, jids });
    if (mode === "reject") throw new Error("Synthetic server refused group creation");
    if (mode === "delay") await new Promise<void>((resolve) => { release = resolve; });
    return {
      jid: "12345-678@g.us", subject,
      participants: jids.map((jid, i) => ({ jid, state: mode === "partial" ? i === 0 ? "pending" : "unconfirmed" : "added" })),
      warnings: mode === "partial" ? ["Group created on WhatsApp, but could not be saved locally: synthetic failure"] : [],
    };
  }

  async function open(result: GroupCreateResult) {
    operations.push({ action: "open", account: session.activeAccount, jid: result.jid });
    if (openFails) { openFails = false; throw new Error("Synthetic chat refresh failed"); }
  }

  const wait = (ms = 25) => new Promise<void>((resolve) => setTimeout(resolve, ms));
  const assert = (condition: unknown, message: string) => { if (!condition) throw new Error(message); };
  const button = (text: string) => Array.from(document.querySelectorAll<HTMLButtonElement>("dialog button")).find((node) => node.textContent?.trim() === text)!;
  function subject(value: string) {
    const input = document.querySelector<HTMLInputElement>("dialog input.field")!;
    input.value = value;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }
  async function show() {
    opened = true;
    await tick();
    await wait();
    assert(document.querySelectorAll("dialog .contacts input").length === 3, "synthetic contacts must load");
  }
  async function pick() {
    document.querySelectorAll<HTMLInputElement>("dialog .contacts input")[0].click();
    document.querySelectorAll<HTMLInputElement>("dialog .contacts input")[1].click();
    await tick();
  }

  onMount(() => {
    void (async () => {
      await show();
      assert(button("Create group").disabled, "empty subject and picks must block creation");
      await pick();
      subject("🦀".repeat(101)); await tick();
      assert(button("Create group").disabled, "101 codepoints must block creation");
      subject("🦀".repeat(100)); await tick();
      assert(!button("Create group").disabled, "100 emoji must fit subject limit");
      checks.push("Subject codepoints and participant selection");

      subject(" Synthetic group "); mode = "reject"; await tick();
      button("Create group").click(); await wait();
      assert(document.querySelector('[role="alert"]')?.textContent?.includes("server refused"), "server refusal must stay visible");
      assert(document.querySelectorAll<HTMLInputElement>("dialog .contacts input:checked").length === 2, "refusal must preserve picks");
      checks.push("Server refusal preserves form and selected people");

      mode = "partial";
      button("Create group").click(); await wait();
      assert(!button("Create group"), "confirmed ACK must remove create action");
      assert(document.querySelector("dialog")?.textContent?.includes("Awaiting admin approval"), "pending result must be visible");
      assert(document.querySelector("dialog")?.textContent?.includes("Membership not confirmed"), "unknown result must stay unknown");
      assert(document.querySelector("dialog")?.textContent?.includes("saved locally"), "local failure must remain visible");
      button("Open group").click(); await wait();
      assert(document.querySelector('[role="alert"]')?.textContent?.includes("chat refresh failed"), "open failure must be retryable");
      assert(!button("Create group"), "open failure must never offer recreation");
      button("Open group").click(); await wait();
      assert(!document.querySelector("dialog"), "open success must close dialog");
      assert(operations.filter((row) => row.action === "create").length === 2, "open retry must not create a duplicate");
      assert(operations.filter((row) => row.action === "open").every((row) => row.jid === "12345-678@g.us"), "open must use ACK jid");
      checks.push("Partial ACK, pending, local failure, open retry without duplicate create");

      mode = "success"; await show(); await pick(); subject("Confirmed group"); await tick();
      button("Create group").click(); await wait();
      assert(!document.querySelector("dialog"), "fully confirmed creation must open immediately");
      checks.push("Confirmed group opens immediately");

      mode = "delay"; await show(); await pick(); subject("Delayed group"); await tick();
      const opensBefore = operations.filter((row) => row.action === "open").length;
      button("Create group").click(); await wait();
      opened = false; session.activeAccount = "synthetic-group-account-b"; await tick();
      release?.(); await wait();
      assert(!document.querySelector("dialog"), "account switch must remove old dialog");
      assert(operations.filter((row) => row.action === "open").length === opensBefore, "stale result must not open under new account");
      assert(operations.filter((row) => row.action === "create").every((row) => row.account === "synthetic-group-account-a"), "creation must keep captured account");
      checks.push("Account switch suppresses delayed old-account UI result");
    })().catch((error) => { failure = String(error); });
  });
</script>

<h1>New group synthetic checks</h1>
<p>No native API, account database, or protocol server is used.</p>
<output aria-label="Check result">{failure ? `FAIL: ${failure}` : checks.length === 5 ? "PASS: 5 checks" : "Running…"}</output>
<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
<output aria-label="Operations">{JSON.stringify(operations)}</output>

{#if opened}
  <NewGroup account="synthetic-group-account-a" me="999@s.whatsapp.net" avatars={{}} onavatar={() => {}}
    onsearch={async (query) => contacts.filter((person) => person.name.toLowerCase().includes(query.toLowerCase()))}
    oncreate={create} onopen={open} onclose={() => { opened = false; }} />
{/if}

<style>
  :global(:root) { --bg: #132029; --surface: #1b2c36; --raised: #29414d; --text: #eee; --muted: #b7c8d3; --accent: #2cd4a0; --accent-text: #0c241b; --line-strong: #547080; --scrim: #0008; --radius-lg: 12px; --shadow: 0 8px 40px #0008; --danger: #f15c6d; }
  :global(body) { font: 14px system-ui; background: var(--bg); color: var(--text); }
  output { display: block; overflow-wrap: anywhere; }
</style>
