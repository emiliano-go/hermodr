<script lang="ts">
  import { onMount, tick } from "svelte";
  import QuickSwitcher from "$lib/chat/QuickSwitcher.svelte";
  import { session } from "$lib/state/session.svelte";
  import { t } from "$lib/i18n/localizer";
  import { previewFixture } from "./ipc";
  import type { ChatSummary, SearchResult, StoredMessage } from "$lib/utils/wire";
  import type { QuickSwitchTarget } from "$lib/utils/quick-switcher";

  let account = $state("synthetic-switcher-a");
  session.activeAccount = "synthetic-switcher-a";
  let opened = $state(false);
  let checks = $state<string[]>([]);
  let failure = $state("");
  let selections = $state<QuickSwitchTarget[]>([]);
  let calls = $state<string[]>([]);
  let release: (() => void) | undefined;
  let chooseFails = false;
  let catalogFails = false;
  const chats = [
    { chat: "100@s.whatsapp.net", display_name: "Áda", last_message_at: 500 },
    { chat: "200@s.whatsapp.net", display_name: "Bob", last_message_at: 300 },
    { chat: "300@g.us", display_name: "Development Group", last_message_at: 200 },
    { chat: "400@newsletter", display_name: "Postal Updates", last_message_at: 100 },
  ] as ChatSummary[];
  const directory: SearchResult[] = [...chats.map((chat) => ({ jid: chat.chat, name: chat.display_name!,
    number: chat.chat.split("@")[0], kind: chat.chat.endsWith("@g.us") ? "group" : chat.chat.endsWith("@newsletter") ? "channel" : "contact",
    saved: true, has_messages: true, aliases: [] })),
    { jid: "500@s.whatsapp.net", name: "Carol Current", number: "500", kind: "contact", saved: true, has_messages: false, aliases: ["maintainer"] }];
  const message = (id: string, text: string): StoredMessage => ({ chat: "200@s.whatsapp.net", id, text,
    timestamp: 100, sender: "200@s.whatsapp.net", from_me: false, spoiler: false } as StoredMessage);
  async function load(query: string) {
    calls.push(`catalog:${query}`);
    if (catalogFails) throw new Error("synthetic catalog failure");
    return directory;
  }
  async function search(query: string) {
    calls.push(`messages:${query}`);
    if (query === "old") return await new Promise<StoredMessage[]>((resolve) => { release = () => resolve([message("old-hit", "old delayed body")]); });
    if (query === "fail") throw new Error("synthetic body failure");
    if (query === "needle") return [message("body-hit", "Before the NEEDLE body hit and after")];
    if (query === "new") return [message("new-hit", "new current body")];
    return [];
  }
  async function choose(target: QuickSwitchTarget) {
    if (chooseFails) throw new Error("synthetic navigation failure");
    selections.push(target);
  }
  const wait = (ms = 25) => new Promise<void>((resolve) => setTimeout(resolve, ms));
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  async function until(condition: () => unknown) {
    const end = performance.now() + 5000;
    while (!condition()) { if (performance.now() > end) throw new Error("synthetic UI timeout"); await wait(); }
    await tick();
  }
  const input = () => document.querySelector<HTMLInputElement>('dialog [role="combobox"]')!;
  const options = () => Array.from(document.querySelectorAll<HTMLButtonElement>('dialog [role="option"]'));
  async function show() {
    document.querySelector<HTMLButtonElement>("#open-switcher")!.focus();
    opened = true;
    await tick();
    await until(() => document.querySelector<HTMLDialogElement>("dialog")?.open && document.activeElement === input());
  }
  function query(value: string) {
    input().value = value;
    input().dispatchEvent(new Event("input", { bubbles: true }));
  }
  function key(value: string, target: HTMLElement = input()) {
    target.dispatchEvent(new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true }));
  }
  const closed = () => !document.querySelector("dialog");

  onMount(() => {
    void (async () => {
      await show();
      assert(options().length === 4 && options()[0].textContent?.includes("Áda"), "recents must use activity order");
      assert(selections.length === 0 && calls.length === 0, "opening must not search catalog, bodies or navigate");
      key("ArrowDown"); await tick();
      assert(options()[1].getAttribute("aria-selected") === "true", "ArrowDown must select second result");
      key("Enter"); await until(closed);
      assert(selections[0]?.chat === "200@s.whatsapp.net" && !selections[0]?.messageId, "Enter must choose selected chat");
      assert(document.activeElement?.id === "open-switcher", "closing must restore previous focus");
      checks.push("Immediate recents, focus and Arrow/Enter selection");

      await show(); query("mntnr");
      await until(() => options().some((row) => row.textContent?.includes("Carol Current")));
      key("Enter"); await until(closed);
      assert(selections[1]?.chat === "500@s.whatsapp.net", "fuzzy alias must open contact without chat history");
      assert(calls.includes("catalog:mntnr"), "typed query must search the contact catalog");
      checks.push("Fuzzy contact alias from bounded query catalog");

      await show(); query("needle");
      await until(() => options().some((row) => row.textContent?.includes("NEEDLE body hit")));
      assert(selections.length === 2, "body search must not navigate");
      key("Enter"); await until(closed);
      assert(selections[2]?.chat === "200@s.whatsapp.net" && selections[2]?.messageId === "body-hit", "body hit must retain jump chat and id");
      checks.push("Global body hit carries exact jump target without search side effects");

      await show(); key("ArrowUp"); await tick();
      assert(options().at(-1)?.getAttribute("aria-selected") === "true", "ArrowUp must wrap");
      key("Escape"); await until(closed);
      assert(selections.length === 3 && document.activeElement?.id === "open-switcher", "Escape must close and restore focus without navigation");
      checks.push("Arrow wrap and Escape without selection side effects");

      await show();
      const close = document.querySelector<HTMLButtonElement>('dialog [aria-label="Close quick switcher"]')!;
      close.focus(); key("Enter", close); await tick();
      assert(selections.length === 3, "Enter on Close must never choose a result");
      close.click(); await until(closed);
      checks.push("Close button Enter avoids accidental selection");

      await show(); query("old"); await until(() => !!release);
      query("new"); await until(() => options().some((row) => row.textContent?.includes("new current body")));
      release!(); release = undefined; await wait();
      assert(!document.querySelector("dialog")?.textContent?.includes("old delayed body"), "stale query response must not overwrite current results");
      key("Escape"); await until(closed);
      checks.push("Delayed stale query response discarded");

      await show(); query("old"); await until(() => !!release);
      account = "synthetic-switcher-b"; session.activeAccount = account; await until(closed);
      release!(); release = undefined; await wait();
      assert(closed() && selections.length === 3, "old-account reply must not reopen or navigate");
      account = "synthetic-switcher-a"; session.activeAccount = account;
      checks.push("Account switch closes palette and suppresses old response");

      await show(); query("fail");
      await until(() => document.querySelector('[role="alert"]')?.textContent?.includes(t("error.quick_search")));
      key("Escape"); await until(closed);
      await show(); chooseFails = true; key("Enter");
      await until(() => document.querySelector('[role="alert"]')?.textContent?.includes(t("error.operation_failed")));
      assert(!!document.querySelector("dialog") && selections.length === 3, "navigation failure must stay visible");
      chooseFails = false; key("Escape"); await until(closed);
      checks.push("Search and navigation failures stay visible");

      catalogFails = true; await show(); query("ada");
      await until(() => document.querySelector('[role="alert"]')?.textContent?.includes(t("nav.contacts_load_error", { error: t("error.operation_failed") })));
      assert(options().some((row) => row.textContent?.includes("Áda")), "catalog failure must preserve known recent chats");
      catalogFails = false; key("Escape"); await until(closed);
      assert(!previewFixture.calls.some((command) => /mark_read|send_read|send_chat_read/.test(command)), "no synthetic native read or receipt operation is allowed");
      checks.push("Catalog failure preserves recents, no read/receipt operations");
    })().catch((error) => { failure = String(error); });
  });
</script>

<h1>Quick switcher synthetic checks</h1>
<p>No native API, account database, or protocol server is used.</p>
<output aria-label="Check result">{failure ? `FAIL: ${failure}` : checks.length === 9 ? "PASS: 9 checks" : "Running…"}</output>
<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
<output aria-label="Selections">{JSON.stringify(selections)}</output>
<output aria-label="Query calls">{JSON.stringify(calls)}</output>
<button id="open-switcher" onclick={() => (opened = true)}>Open quick switcher</button>
{#if opened}
  <QuickSwitcher {account} {chats} onload={load} onmessages={search} onchoose={choose} onclose={() => (opened = false)} />
{/if}

<style>
  :global(:root) { --bg: #132029; --surface: #1b2c36; --raised: #29414d; --text: #eee; --muted: #b7c8d3; --line-strong: #547080; --scrim: #0008; --radius-lg: 12px; --shadow: 0 8px 40px #0008; --danger: #f15c6d; }
  :global(body) { font: 14px system-ui; background: var(--bg); color: var(--text); }
  output { display: block; overflow-wrap: anywhere; }
</style>
