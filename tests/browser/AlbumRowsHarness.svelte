<script lang="ts">
  import { onMount, tick } from "svelte";
  import AlbumGrid from "$lib/messages/AlbumGrid.svelte";
  import MessageRow from "$lib/messages/MessageRow.svelte";
  import { groupAlbumRows } from "$lib/utils/albums";
  import { captionOf, dayKey, isUnavailable } from "$lib/utils/message";
  import { keywords } from "$lib/state/keywords.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { BubbleApi, BubbleCtx, StoredMessage } from "$lib/utils/models";
  import { calls } from "./album-rows-ipc";

  const thumb = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="#00a884"/></svg>')}`;
  function row(id: string, patch: Partial<StoredMessage> = {}): StoredMessage {
    return { id, chat: "synthetic-album-chat", sender: "synthetic-sender", from_me: false, timestamp: 100, sort_order: 0,
      text: `Synthetic caption for ${id}. ${"longword".repeat(8)}`, media_kind: "image", media_path: null, media_thumb: thumb,
      media_duration: null, media_once_kind: null, spoiler: false, reply_to_view_once: false, revoked: false, deleted: false,
      system_kind: null, mentioned: false, status: null, reply_to_text: null, reply_to_sender: null, reply_to_id: null,
      preview_url: null, transcript: null, ...patch } as StoredMessage;
  }
  const catalog = Array.from({ length: 6 }, (_, index) => row(`child-${index}`, { timestamp: 100 + index, sort_order: index, media_kind: index % 2 ? "video" : "image" }));
  let items = $state.raw<StoredMessage[]>(catalog.slice(0, 2)), width = $state(400), timestamp = $state(200);
  let picked = $state<Record<string, StoredMessage> | null>(null), revealed = $state<Record<string, true>>({});
  let actions = $state<string[]>([]), downloadErrors = $state<Record<string, string>>({});
  let checks = $state<string[]>([]), complete = $state(false), failed = $state(""), readyKeyboard = $state(false);
  const groups = $derived(groupAlbumRows(items, () => "synthetic-parent", { visible: (message) => !keywords.hidden(message) }));
  const ctx = $derived<BubbleCtx>({ isGroup: true, picking: picked, dayKey, captionOf, senderLabel: () => "Synthetic sender with long display name",
    memberTagOf: () => null, hue: () => 120, viewOnceMarks: [], reactionsFor: new Map(), starredSet: new Set(["child-0"]), editedSet: new Set(["child-1"]),
    forwardedSet: new Set(), downloading: {}, downloadErrors, downloadTries: {}, replyingToId: null, highlightedId: null, menuId: null,
    polls: [], events: [], avatars: {}, revealedOnce: revealed, voiceAvatarOf: () => null, quoteAuthorOf: () => "Synthetic quoted sender",
    quoteTextOf: () => null, quoteChatNameOf: () => null, autoplayId: null, onceAudioOpenId: null });
  const record = (kind: string, message: StoredMessage) => actions.push(`${kind}:${message.id}`);
  const noop = () => {};
  const api: BubbleApi = { toWire: (text) => text, targetOf: (user) => ({ jid: user, name: user, self: false }), avatarOf: () => null,
    onprofile: noop, onopenurl: noop, formatTime: (value) => `[time-${value}]`, namer: (user) => user,
    onreplydraft: (message) => record("reply", message), onmenu: (_event, message) => record("menu", message), onpick: (message) => {
      picked = picked?.[message.id] ? Object.fromEntries(Object.entries(picked).filter(([id]) => id !== message.id)) : { ...picked, [message.id]: message };
    }, onjumpquoted: noop, onrecoverquote: noop, recovering: {}, ondownload: (message) => record("download", message),
    onopenviewer: (message) => record("viewer", message), onopenmedia: noop, onopenquote: noop, onvote: noop, onrespond: noop,
    oneditrequest: noop, oncancelevent: noop, onreact: noop, onopenreactions: noop, onmarkplayed: noop, onnextvoice: noop, onpausevoice: noop,
    onreplymenu: noop, ononce: (message) => record("once", message), oncloseonce: noop,
    onrevealonce: (message) => { record("reveal-once", message); revealed = { ...revealed, [message.id]: true }; },
    oninviteopen: noop, oninvitejoin: async () => { throw new Error("Invite join forbidden in album fixture"); } };
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 20));
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  const bubble = (id: string) => document.querySelector<HTMLElement>(`.bubble[data-id="${id}"]`)!;
  const grid = () => document.querySelector<HTMLElement>(".album-grid")!;
  function pick(event: MouseEvent) {
    if (!picked && !event.ctrlKey && !event.metaKey) return;
    const id = (event.target as HTMLElement)?.closest(".msg-row")?.querySelector<HTMLElement>(".bubble[data-id]")?.dataset.id;
    const message = items.find((message) => message.id === id);
    if (!message || message.revoked) return;
    event.preventDefault(); event.stopPropagation(); api.onpick(message);
  }

  onMount(() => {
    session.activeAccount = "synthetic-album-a";
    keywords.rules = { hide: ["hidden-token"], highlight: [] };
    const warnings: string[] = [], warn = console.warn;
    console.warn = (...args) => { warnings.push(args.join(" ")); warn(...args); };
    void (async () => {
      try {
        await tick();
        assert([...grid().querySelectorAll<HTMLElement>(".bubble[data-id]")].map((element) => element.dataset.id).join() === "child-0,child-1", "real rows preserve child IDs/order");
        assert(grid().querySelectorAll("time").length === 1 && (grid().textContent || "").split("[time-").length === 2, "real rows display only shared timestamp");
        assert(grid().textContent?.includes("[time-200]"), "shared timestamp uses envelope value");
        assert(bubble("child-0").querySelector(".star") && bubble("child-1").querySelector(".edited-mark"), "child star/edit markers survive compact mode");
        items = items.map((message) => ({ ...message, from_me: true, status: "read" })); await tick();
        assert(grid().querySelectorAll(".ticks.read").length === 2, "every outgoing child keeps read ticks");
        timestamp = items[0].timestamp; await tick(); assert(grid().textContent?.includes("[time-100]"), "shared fallback uses loaded first child");
        checks.push("production rows preserve child IDs/order, one shared timestamp, per-child star/edit/read ticks");

        bubble("child-0").closest(".msg-row")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
        bubble("child-1").closest(".msg-row")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
        bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!.click(); await tick();
        assert(actions.includes("menu:child-0") && actions.includes("reply:child-1") && actions.includes("download:child-0"), "real menu/reply/download retain child target");
        bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true })); await tick();
        assert(picked?.["child-0"] && bubble("child-0").closest(".msg-row")!.classList.contains("picked"), "real selected child marker preserved");
        picked = null; downloadErrors = { "child-1": "Synthetic download failure" }; await tick();
        const retry = bubble("child-1").querySelector<HTMLButtonElement>(".download-failed")!;
        assert(retry?.title === "Synthetic download failure", "real child retry error visible"); retry.click(); assert(actions.includes("download:child-1"), "real retry retains child ID");
        downloadErrors = {}; items = [row("child-0", { media_path: thumb }), row("child-1", { media_kind: "video" })]; await tick();
        const target = bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!;
        target.focus(); assert(document.activeElement === target, "real media child receives keyboard focus");
        readyKeyboard = true; await tick();
        const deadline = performance.now() + 10000;
        while (!actions.includes("viewer:child-0")) { if (performance.now() > deadline) throw new Error("Press Enter for production child keyboard check"); await wait(); }
        readyKeyboard = false;
        bubble("child-1").scrollIntoView({ block: "center" }); await tick();
        assert(bubble("child-1").getBoundingClientRect().top >= document.querySelector(".scroller")!.getBoundingClientRect().top, "real child jump anchor usable");
        checks.push("production menu/reply/download/retry/selection targets, trusted keyboard viewer action and child jump");

        for (const count of [2, 3, 4, 6]) for (const size of [400, 320]) for (const zoom of [100, 200]) {
          items = catalog.slice(0, count); width = size; document.documentElement.style.zoom = `${zoom}%`; await tick(); await wait();
          const parent = document.querySelector<HTMLElement>(".scroller")!, rect = grid().getBoundingClientRect();
          assert(rect.width <= innerWidth && rect.right <= parent.getBoundingClientRect().right + 1, `real grid fits ${size}px/${zoom}%`);
          assert(parent.scrollWidth <= parent.clientWidth + 1 && grid().scrollWidth <= grid().clientWidth + 1, `real grid no horizontal overflow ${count}/${size}/${zoom}`);
          for (const element of grid().querySelectorAll<HTMLElement>(".bubble")) assert(element.scrollWidth <= element.clientWidth + 1, `real child text fits ${count}/${size}/${zoom}`);
          assert((grid().textContent || "").split("[time-").length === 2, "shared timestamp remains once under zoom");
        }
        document.documentElement.style.zoom = "100%";
        checks.push("production 2/3/4/6 child grids at 400px/320px and 100%/200% page and text scale");

        items = [row("spoiler", { spoiler: true, text: "Private spoiler payload" }), row("once", { media_kind: "view_once", media_once_kind: "image", media_thumb: null }),
          row("kept", { media_once_kind: "image", media_path: thumb }), row("unavailable", { system_kind: "UNAVAILABLE_MESSAGE", text: "Private unavailable payload" }),
          row("hidden", { text: "hidden-token private payload" }), row("ordinary-one"), row("ordinary-two")];
        await tick();
        assert(groups.map((group) => group.messages.map((message) => message.id).join()).join(";") === "spoiler;once;kept;unavailable;ordinary-one,ordinary-two", "real fallback grouping preserves privacy boundaries");
        assert(!bubble("spoiler").querySelector("img") && !bubble("spoiler").textContent?.includes("Private spoiler payload") && bubble("spoiler").querySelector(".spoiler-reveal"), "real spoiler fallback hides body and pixels");
        assert(bubble("once").querySelector(".once.spent") && !bubble("once").querySelector("img"), "real view-once fallback has no media pixels");
        assert(bubble("kept").querySelector(".once-overlay"), "real kept-once fallback retains reveal filter");
        assert(!document.querySelector('[data-id="hidden"]') && !document.querySelector('[data-id="unavailable"] img'), "hidden child absent and unavailable placeholder has no pixels");
        assert(!grid().querySelector('[data-id="spoiler"], [data-id="once"], [data-id="kept"], [data-id="unavailable"], [data-id="hidden"]'), "private fallback stays outside album grid");
        const beforeReveal = actions.length;
        bubble("kept").querySelector<HTMLButtonElement>(".media-button")!.click(); await tick();
        assert(actions.length === beforeReveal + 1 && actions.at(-1) === "reveal-once:kept" && !bubble("kept").querySelector(".once-overlay"), "kept-once first click only reveals");
        bubble("kept").querySelector<HTMLButtonElement>(".media-button")!.click(); assert(actions.at(-1) === "viewer:kept", "kept-once second click opens exact child");
        session.activeAccount = "synthetic-album-b"; await tick();
        assert(!bubble("spoiler").querySelector("img"), "spoiler remains private after account transition");
        assert(calls.length === 0, `no IPC calls: ${calls.join(", ")}`);
        assert(warnings.length === 0, `no runtime warnings: ${warnings.join("; ")}`);
        checks.push("production spoiler/view-once/kept-once/unavailable/keyword-hidden fallback, zero IPC and runtime warnings");
        complete = true;
      } catch (failure) { failed = String(failure); complete = true; document.documentElement.style.zoom = "100%"; }
    })();
    return () => { console.warn = warn; document.documentElement.style.zoom = "100%"; };
  });
</script>

<main>
  <h1>Production album rows with synthetic data</h1>
  {#if readyKeyboard}<p>Press Enter to open focused synthetic child.</p>{/if}
  <div class="scroller" style:width="{width}px" onclickcapture={pick} role="group" aria-label="Synthetic production rows">
    {#each groups as group (group.messages[0].id)}
      {#if group.parentId && group.messages.length > 1}
        <AlbumGrid items={group.messages} prev={group.prev} {timestamp} formatTime={api.formatTime}>
          {#snippet children(message, previous)}<MessageRow {message} prev={previous} {ctx} {api} albumCell />{/snippet}
        </AlbumGrid>
      {:else if isUnavailable(group.messages[0])}
        <article class="unavailable-message" data-id={group.messages[0].id}>Message unavailable</article>
      {:else}<MessageRow message={group.messages[0]} prev={group.prev} {ctx} {api} />{/if}
    {/each}
  </div>
  <div id="album-rows-result" data-complete={complete} data-pass={complete && !failed} data-keyboard={readyKeyboard} data-viewport={innerWidth}>
    {#if failed}<p role="alert">{failed}</p>{/if}<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
  </div>
</main>

<style>
  :global(:root) { --muted: #aebac1; --text: #e9edef; --bubble: #202c33; --bubble-mine: #005c4b; --raised: #2a3942; --line: #3b4a54; --accent: #00a884; --accent-text: #66d9b9; --danger: #f47b7b; --row-hover: #ffffff08; --radius-sm: 6px; --faint: #667781; --motion-scale: 0; --ease: linear; font: 14px "Segoe UI", sans-serif; }
  :global(body) { margin: 0; padding: 12px; background: #111b21; color: var(--text); }
  main { max-width: 520px; margin: auto; }
  h1 { font-size: 18px; }
  .scroller { display: flex; flex-direction: column; width: 400px; max-width: 100%; height: 350px; overflow-y: auto; box-sizing: border-box; gap: 8px; }
  .unavailable-message { padding: 10px; background: var(--raised); }
</style>
