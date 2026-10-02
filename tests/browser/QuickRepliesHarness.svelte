<script lang="ts">
  import { flushSync, onMount, tick } from "svelte";
  import BusinessCard from "$lib/contacts/BusinessCard.svelte";
  import QuickRepliesMenu from "$lib/chat/QuickRepliesMenu.svelte";
  import { quickReplyScopeMatches, type QuickReplyScope } from "$lib/utils/quick-replies";
  import type { MemberProfileLive, QuickReply } from "$lib/utils/wire";

  let account = $state("synthetic-a"), chat = $state("100@s.whatsapp.net"), generation = $state(1), requestKey = $state(1);
  const scope = (): QuickReplyScope => ({ account, chat, generation, requestKey });
  let dataScope = $state<QuickReplyScope | null>(scope());
  const catalog: QuickReply[] = [
    { id: "hello", shortcut: "hello", message: "Hello\nHow can we help?", keywords: ["greeting"], count: 2, associated_label_ids: [] },
    { id: "hours", shortcut: "hours", message: "Open Monday to Friday", keywords: ["schedule"], count: 1, associated_label_ids: [] },
    { id: "empty", shortcut: "media", message: "", keywords: [], count: 0, associated_label_ids: [] },
  ];
  let replies = $state<QuickReply[]>(structuredClone(catalog));
  let field = $state<MemberProfileLive["business"]>({ state: "available", stale: false, error: null, value: {
    name: "Synthetic shop", description: "Synthetic business information", address: "123 Example Street",
    categories: ["Shop", "Services"], timezone: "America/Montevideo", hours: [{ day: "Monday", mode: "open", open_minutes: 540, close_minutes: 1020 }],
    email: "synthetic@example.invalid", websites: ["https://example.invalid/synthetic"],
  } });
  let connected = $state(true), loading = $state(false), syncing = $state(false), error = $state<string | null>(null), disabled = $state(false);
  let chosen = $state<{ scope: QuickReplyScope; reply: QuickReply }[]>([]), syncs = $state<QuickReplyScope[]>([]), refreshes = $state<QuickReplyScope[]>([]);
  let checks = $state<string[]>([]), failed = $state(""), complete = $state(false);
  const composer = { draft: "Existing draft", reply: "synthetic-reply", pending: ["synthetic-file"], mentions: ["200@lid"] };
  const before = structuredClone(composer);
  let sends = 0;
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 20));
  const trigger = () => document.querySelector<HTMLButtonElement>('[aria-label="Quick replies"]')!;
  const dialog = () => document.querySelector<HTMLDialogElement>("dialog")!;
  const input = () => dialog().querySelector<HTMLInputElement>("input")!;
  const options = () => [...dialog().querySelectorAll<HTMLButtonElement>('[role="option"]')];
  const card = () => document.querySelector<HTMLElement>('[aria-label="Business information"]')!;
  const sync = () => [...dialog().querySelectorAll<HTMLButtonElement>("button")].find((button) => /Sync from phone|Requesting sync/.test(button.textContent || ""))!;
  async function open(label = "") { trigger().click(); await tick(); await wait(); assert(dialog().open, `picker opens ${label}`); }
  async function close() { dialog().querySelector<HTMLButtonElement>('[aria-label="Close quick replies"]')!.click(); await tick(); await wait(); }
  function key(value: string, extra: KeyboardEventInit = {}) {
    const event = new KeyboardEvent("keydown", { key: value, bubbles: true, cancelable: true, ...extra });
    input().dispatchEvent(event); return event;
  }
  async function search(value: string) { input().value = value; input().dispatchEvent(new Event("input", { bubbles: true })); await tick(); }
  function select(owner: QuickReplyScope, reply: QuickReply) {
    if (quickReplyScopeMatches(owner, account, chat, generation, requestKey)) chosen.push({ scope: owner, reply });
  }

  onMount(() => {
    const warnings: string[] = [], warn = console.warn;
    console.warn = (...args) => { warnings.push(args.join(" ")); warn(...args); };
    const sendKey = (event: KeyboardEvent) => { if (event.key === "Enter") sends++; };
    window.addEventListener("keydown", sendKey);
    void (async () => {
      try {
        await tick();
        assert(/123 Example Street/.test(card().textContent || "") && /Shop, Services/.test(card().textContent || "") && /Monday: open 09:00 to 17:00/.test(card().textContent || ""), "business address, categories and formatted hours");
        const saved = structuredClone($state.snapshot(field));
        field = { ...field, state: "restricted", error: "403" }; await tick();
        assert(/Access denied by server/.test(card().textContent || "") && !/123 Example Street/.test(card().textContent || ""), "restricted fields never expose retained payload");
        field = { ...saved, state: "error", stale: true, error: "Synthetic refresh failed" }; await tick();
        assert(/Cached value: Synthetic shop/.test(card().textContent || "") && /Synthetic refresh failed/.test(card().textContent || "") && /123 Example Street/.test(card().textContent || ""), "cached field and failure stay visible");
        field = { ...saved, state: "unavailable", value: null }; await tick();
        assert(/Not provided/.test(card().textContent || "") && !/123 Example Street/.test(card().textContent || ""), "unavailable distinct from denied");
        field = saved; await tick();
        card().querySelector<HTMLButtonElement>("button")!.click(); assert(refreshes.length === 1, "refresh carries current scope");
        checks.push("business values, restricted/unavailable states, cached error and scoped refresh");

        await open();
        assert(/Stored quick replies/.test(dialog().textContent || "") && /More replies may arrive/.test(dialog().textContent || ""), "partial catalog wording");
        assert(options().length === 3 && options()[2].disabled, "empty text remains visible and cannot insert");
        await search("SCHEDULE Friday"); assert(options().length === 1 && /hours/.test(options()[0].textContent || ""), "keyword/text search");
        await search("/HELLO"); assert(options().length === 1, "shortcut search");
        key("Enter", { isComposing: true }); key("Enter", { keyCode: 229 }); key("Enter", { ctrlKey: true });
        assert(chosen.length === 0 && dialog().open && sends === 0, "IME and modified enter cannot select or bubble into send");
        input().dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
        key("Enter"); key("Escape"); dialog().dispatchEvent(new Event("cancel", { cancelable: true }));
        assert(dialog().open && chosen.length === 0, "composition blocks enter and escape even without isComposing");
        input().dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));
        key("Enter"); await tick(); await wait();
        assert(chosen.length === 1 && chosen[0].reply.message === catalog[0].message && !dialog().open && sends === 0, "enter selects exact stored text without sending");
        assert(JSON.stringify(composer) === JSON.stringify(before), "leaf never changes draft, reply, pending or mentions");
        checks.push("search, exact text callbacks, empty text, IME/modifiers and no automatic send");

        await open(); key("ArrowDown"); await tick();
        assert(options()[1].getAttribute("aria-selected") === "true", "arrow moves selection");
        key("ArrowDown"); await tick(); assert(options()[0].getAttribute("aria-selected") === "true", "arrows skip empty text and wrap");
        key("End"); await tick(); assert(options()[1].getAttribute("aria-selected") === "true", "end selects last available text");
        key("Home"); await tick(); assert(options()[0].getAttribute("aria-selected") === "true", "home selects first available text");
        sync().click(); assert(syncs.length === 1 && syncs[0].account === account, "sync callback scoped");
        syncing = true; await tick(); assert(sync().disabled, "sync pending disables repeat requests");
        syncing = false; error = "Synthetic sync failure"; await tick();
        assert(/Synthetic sync failure/.test(dialog().querySelector('[role="alert"]')?.textContent || ""), "sync failure visible");
        connected = false; await tick(); assert(sync().disabled, "offline sync disabled");
        connected = true; error = null; await close();
        assert(document.activeElement === trigger(), "native dialog restores trigger focus");
        checks.push("keyboard navigation, scoped sync, pending/offline guards, error and focus return");

        for (const axis of ["account", "chat", "generation", "requestKey"] as const) {
          dataScope = scope(); await open(axis);
          const count = chosen.length;
          dialog().addEventListener("click", () => flushSync(() => {
            if (axis === "account") account = `${account}-next`;
            if (axis === "chat") chat = `${chat}-next`;
            if (axis === "generation") generation++;
            if (axis === "requestKey") requestKey++;
          }), { capture: true, once: true });
          options()[0].click(); await tick(); await wait();
          assert(chosen.length === count && !dialog().open, `${axis} stale queued click rejected`);
          assert(!/123 Example Street/.test(card().textContent || ""), `${axis} hides stale business data`);
        }
        dataScope = scope(); await open("tombstone");
        const count = chosen.length;
        dialog().addEventListener("click", () => flushSync(() => { replies = []; }), { capture: true, once: true });
        options()[0].click(); await tick();
        assert(chosen.length === count && dialog().open, "deleted template cannot be chosen by queued click");
        dataScope = null; replies = structuredClone(catalog); error = "Stale account error"; await tick();
        assert(options().length === 0 && !/Stale account error/.test(dialog().textContent || ""), "mismatched data and errors hidden");
        disabled = true; await tick(); await wait(); assert(!dialog().open && trigger().disabled, "disabled composer closes picker");
        disabled = false; dataScope = scope(); error = null; await tick(); await open("viewport");
        const rect = dialog().getBoundingClientRect();
        assert(rect.left >= 0 && rect.right <= innerWidth && dialog().scrollWidth <= dialog().clientWidth + 1, "picker fits viewport without horizontal overflow");
        key("Escape"); await tick(); await wait(); assert(!dialog().open, "escape closes picker");
        assert(sends === 0, "all picker keys stay out of send path");
        assert(warnings.length === 0, `no Svelte runtime warnings: ${warnings.join("; ")}`);
        checks.push("account/chat/generation/request fences, stale click/tombstone, hidden stale errors, disabled state and viewport fit");
        complete = true;
      } catch (failure) { failed = String(failure); complete = true; }
    })();
    return () => { window.removeEventListener("keydown", sendKey); console.warn = warn; };
  });
</script>

<main>
  <h1>Business and quick replies synthetic checks</h1>
  <div class="card"><BusinessCard {account} {chat} {generation} {requestKey} {dataScope} {field} {loading} {error} {connected}
    onrefresh={(owner) => refreshes.push(owner)} /></div>
  <QuickRepliesMenu {account} {chat} {generation} {requestKey} {dataScope} {replies} {loading} {syncing} {error} {connected} {disabled}
    onselect={select} onsync={(owner) => syncs.push(owner)} />
  <div id="quick-replies-result" data-complete={complete} data-pass={complete && !failed}>
    {#if failed}<p role="alert">{failed}</p>{/if}
    <ul>{#each checks as check}<li>{check}</li>{/each}</ul>
  </div>
</main>

<style>
  :global(:root) { --bg: #111b21; --surface: #202c33; --raised: #2a3942; --line: #3b4a54; --line-strong: #52616c; --text: #e9edef; --muted: #aebac1; --danger: #f47b7b; --accent: #00a884; --scrim: #0009; --radius-sm: 6px; --radius-lg: 14px; --shadow: 0 6px 24px #0006; font: 14px "Segoe UI", sans-serif; }
  :global(body) { margin: 0; padding: 12px; background: var(--bg); color: var(--text); }
  main { max-width: 520px; margin: auto; }
  h1 { font-size: 18px; }
  .card { padding: 12px; margin: 12px 0; background: var(--surface); border-radius: var(--radius-lg); }
</style>
