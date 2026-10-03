<script lang="ts">
  import { onMount, tick } from "svelte";
  import MessageList from "$lib/messages/MessageList.svelte";
  import { captionOf } from "$lib/utils/message";
  import { visibleReadFrontier } from "$lib/utils/album-timeline";
  import { keywords } from "$lib/state/keywords.svelte";
  import { session } from "$lib/state/session.svelte";
  import type { BubbleApi, BubbleCtx, StoredMessage } from "$lib/utils/models";
  import { calls } from "./album-rows-ipc";

  const thumb = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="#00a884"/></svg>')}`;
  function row(id: string, parent: string | null, patch: Partial<StoredMessage> = {}): StoredMessage {
    return { id, chat: "synthetic-timeline-chat", sender: "synthetic-sender", from_me: false, timestamp: 100, sort_order: 0,
      text: `Synthetic caption ${id}. ${"longword".repeat(8)}`, media_kind: parent ? "image" : "album", media_path: null,
      media_thumb: parent ? thumb : null, media_duration: null, media_once_kind: null, spoiler: false, revoked: false, deleted: false,
      system_kind: null, system_params: [], mentioned: false, read: false, status: null, preview_url: null,
      reply_to_id: null, reply_to_text: null, reply_to_sender: null, reply_to_chat: null, reply_to_kind: null, reply_to_thumb: null,
      reply_to_view_once: false, reply_to_recoverable: false, reply_to_path: null, transcript: null,
      album: { parent_id: parent, expected_images: parent ? null : 98765, expected_videos: parent ? null : 43210, index: parent ? 7 : null },
      ...patch } as StoredMessage;
  }
  const parent = row("parent", null, { timestamp: 99, text: "[album]", reply_to_id: "quoted-original", reply_to_text: "Synthetic parent quote",
    reply_to_sender: "synthetic-quoted-sender", reply_to_chat: "synthetic-quoted-chat" });
  const children = Array.from({ length: 6 }, (_, index) => row(`child-${index}`, "parent", { timestamp: 100 + index, sort_order: index,
    media_kind: index % 2 ? "video" : "image" }));
  let rows = $state.raw<StoredMessage[]>([parent, ...children.slice(0, 2)]), width = $state(400);
  let scroller = $state<HTMLDivElement>(), firstUnreadId = $state<string | null>(null), picking = $state<Record<string, StoredMessage> | null>(null);
  let rail = $state<ReturnType<typeof MessageList> | undefined>(), prepending = $state(false);
  let metrics = $state({ offset: 0, distance: 0, viewport: 0 });
  let revealed = $state<Record<string, true>>({}), actions = $state<string[]>([]);
  let checks = $state<string[]>([]), complete = $state(false), failed = $state(""), readyKeyboard = $state(false);
  let frameHeight = $state(3000);
  const dayKey = (timestamp: number) => String(Math.floor(timestamp / 86400));
  const noop = () => {};
  const record = (kind: string, message: StoredMessage) => actions.push(`${kind}:${message.id}:${message.reply_to_id || ""}`);
  const api: BubbleApi = { toWire: (text) => text, targetOf: (user) => ({ jid: user, name: user, self: false }), avatarOf: () => null,
    onprofile: noop, onopenurl: noop, formatTime: (value) => `[time-${value}]`, namer: (user) => user,
    onreplydraft: (message) => record("reply", message), onmenu: (_event, message) => record("menu", message),
    onpick: (message) => { picking = picking?.[message.id] ? Object.fromEntries(Object.entries(picking).filter(([id]) => id !== message.id)) : { ...picking, [message.id]: message }; },
    onjumpquoted: (message) => record("quote-jump", message), onrecoverquote: (message) => record("quote-save", message), recovering: {},
    ondownload: (message) => record("download", message), onopenviewer: (message) => record("viewer", message), onopenmedia: noop,
    onopenquote: (message) => record("quote-open", message), onvote: noop, onrespond: noop, oneditrequest: noop, oncancelevent: noop,
    onreact: noop, onopenreactions: noop, onmarkplayed: noop, onnextvoice: noop, onpausevoice: noop, onreplymenu: noop,
    ononce: (message) => record("once", message), oncloseonce: noop,
    onrevealonce: (message) => { record("reveal", message); revealed = { ...revealed, [message.id]: true }; }, oninviteopen: noop,
    oninvitejoin: async () => { throw new Error("Invite join forbidden in album timeline fixture"); } };
  const ctx = $derived<BubbleCtx>({ isGroup: true, picking, dayKey, captionOf, senderLabel: () => "Synthetic sender with long display name",
    memberTagOf: () => null, hue: () => 120, viewOnceMarks: [], reactionsFor: new Map(), starredSet: new Set(["child-0"]), editedSet: new Set(["child-1"]),
    forwardedSet: new Set(), downloading: {}, downloadErrors: {}, downloadTries: {}, replyingToId: null, highlightedId: null, menuId: null,
    polls: [], events: [], avatars: {}, revealedOnce: revealed, voiceAvatarOf: () => null, quoteAuthorOf: (sender) => sender || "Synthetic quoted sender",
    quoteTextOf: (message) => message.reply_to_view_once && !message.reply_to_path ? "[View once]" : message.reply_to_text,
    quoteChatNameOf: (message) => message.reply_to_chat ? "Synthetic quoted chat" : null, autoplayId: null, onceAudioOpenId: null });
  const props = $derived({ ...ctx, ...api, switching: false, firstUnreadId, onjumpunread: (id: string) => actions.push(`unread:${id}`),
    dayLabel: (value: number) => `Day ${dayKey(value)}`, loadingOlder: false, prepending,
    uploads: [], typers: [], typerLabelOf: (sender: string) => sender,
    onscroll: (state: { offset: number; distance: number; viewport: number }) => { metrics = state; } });
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 20));
  const grids = () => [...scroller!.querySelectorAll<HTMLElement>(".album-grid")];
  const bubble = (id: string) => scroller!.querySelector<HTMLElement>(`.bubble[data-id="${id}"]`)!;
  const ids = () => [...scroller!.querySelectorAll<HTMLElement>(".bubble[data-id]")].map((element) => element.dataset.id);
  const quote = (index = 0) => grids()[index].querySelector<HTMLButtonElement>(".embed.quote")!;
  const viewport = () => scroller!.querySelector<HTMLDivElement>(".message-viewport")!;
  async function replace(next: StoredMessage[], unread: string | null = null) {
    rows = next; firstUnreadId = unread; picking = null; await tick();
    rail!.scrollToBottom();
    if (next.length <= 12) await until(() => next.filter((message) => !keywords.hidden(message)).every((message) => {
      const marker = scroller!.querySelector<HTMLElement>(`[data-id="${message.id}"]`);
      return marker && getComputedStyle(marker).visibility !== "hidden";
    }), "small virtual timeline did not mount all expected markers");
    await wait();
  }
  function boundary(label: string) {
    assert(grids().length === 0, `${label} root rejects folding`);
    assert(ids().includes("child-0") && ids().includes("child-1"), `${label} preserves ordinary child markers`);
  }
  function frontier() {
    return visibleReadFrontier(rows, rail?.visibleReadIds() ?? []);
  }
  function inView(id: string) {
    const marker = scroller!.querySelector<HTMLElement>(`[data-id="${id}"]`);
    if (!marker) return false;
    const rect = marker.getBoundingClientRect(), bounds = viewport().getBoundingClientRect();
    return rect.top < bounds.bottom && rect.bottom > bounds.top;
  }
  async function until(condition: () => unknown, label: string) {
    const deadline = performance.now() + 5000;
    while (!condition()) { if (performance.now() > deadline) throw new Error(label); await tick(); await wait(); }
  }

  onMount(() => {
    session.activeAccount = "synthetic-timeline-a";
    keywords.rules = { hide: ["hidden-token"], highlight: [] };
    const warnings: string[] = [], warn = console.warn;
    console.warn = (...args) => { warnings.push(args.join(" ")); warn(...args); };
    void (async () => {
      try {
        await tick();
        await until(() => grids().length === 1 && ids().join() === "child-0,child-1", "initial virtual rows did not mount");
        const raw = JSON.stringify(rows), childStatus = rows.map((message) => [message.id, message.read, message.status]);
        assert(grids().length === 1 && ids().join() === "child-0,child-1", "real list folds parent and preserves child IDs/order");
        assert(scroller!.querySelectorAll('[data-id="parent"]').length === 1 && scroller!.querySelector('[data-id="parent"]')?.classList.contains("album-anchor"), "parent has one separate jump anchor");
        assert(grids()[0].querySelectorAll("time").length === 1 && (grids()[0].textContent || "").split("[time-").length === 2 && grids()[0].textContent?.includes("[time-99]"), "folded grid has one envelope time");
        assert(quote() && grids()[0].querySelectorAll(".embed.quote").length === 1, "folded parent quote appears once");
        assert(!/98765|43210/.test(grids()[0].textContent || ""), "expected counts never fabricate loaded contents");
        quote().click(); assert(actions.at(-1) === "quote-jump:parent:quoted-original", "parent quote jumps correct original using parent callback");
        assert(JSON.stringify(rows) === raw && JSON.stringify(rows.map((message) => [message.id, message.read, message.status])) === JSON.stringify(childStatus), "list never rewrites raw paging/read/status data");
        checks.push("actual MessageList folds envelope into one grid/time/quote, preserves raw child IDs/read state and parent jump anchor");

        bubble("child-0").closest(".msg-row")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
        bubble("child-1").closest(".msg-row")!.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
        bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!.click(); await tick();
        assert(actions.includes("menu:child-0:") && actions.includes("reply:child-1:") && actions.includes("download:child-0:"), "real list menu/reply/download target child IDs");
        bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true })); await tick();
        assert(picking?.["child-0"] && !picking?.parent && bubble("child-0").closest(".msg-row")?.classList.contains("picked"), "real list selection captures only clicked child");
        await replace([parent, { ...children[0], media_path: thumb }, children[1]]);
        const media = bubble("child-0").querySelector<HTMLButtonElement>(".media-button")!;
        media.focus(); readyKeyboard = true; await tick();
        const deadline = performance.now() + 10000;
        while (!actions.includes("viewer:child-0:")) { if (performance.now() > deadline) throw new Error("Press Enter for real list child keyboard check"); await wait(); }
        readyKeyboard = false;
        bubble("child-1").scrollIntoView({ block: "center" }); await tick();
        assert(bubble("child-1").isConnected, "real list child jump marker usable");
        checks.push("actual MessageList preserves per-child context menu/reply/download/selection/viewer and trusted Enter/jump");

        await replace([parent, ...children.slice(0, 2)], "parent");
        assert(grids().length === 1 && scroller!.querySelectorAll("[data-unread-divider]").length === 1, "unread parent maps to first loaded child");
        scroller!.querySelector<HTMLButtonElement>("[data-unread-divider]")!.click(); assert(actions.at(-1) === "unread:child-0", "unread parent jumps first child");
        await replace([parent, ...children.slice(0, 2)], "child-1");
        assert(grids().length === 2 && scroller!.querySelectorAll('[data-id="parent"]').length === 1, "unread child splits grid with one parent anchor");
        assert(grids().every((grid) => grid.querySelectorAll("time").length === 1), "unread fragments each have one shared time");
        scroller!.querySelector<HTMLButtonElement>("[data-unread-divider]")!.click(); assert(actions.at(-1) === "unread:child-1", "unread fragment callback uses exact child");
        await replace([parent, children[0]]); assert(grids().length === 1 && ids().join() === "child-0" && quote(), "explicit single-child partial keeps parent quote and footer");
        await replace(children.slice(0, 2)); assert(grids().length === 1 && !scroller!.querySelector('[data-id="parent"]') && !quote(), "unloaded parent needs no fabricated anchor or quote");
        assert(grids()[0].textContent?.includes("[time-100]"), "missing envelope time falls back to first loaded child");
        await replace([parent, children[0], { ...children[1], timestamp: 86501 }]);
        rail!.revealMessage("child-1");
        await until(() => inView("child-1") && bubble("child-1").closest(".album-grid")?.textContent?.includes("[time-86501]"), "next-day child did not mount with its timestamp");
        const nextDayGroup = bubble("child-1").closest(".album-grid");
        rail!.revealMessage("child-0");
        await until(() => inView("child-0") && bubble("child-0").closest(".album-grid")?.textContent?.includes("[time-99]"), "first-day child did not mount with envelope timestamp");
        assert(bubble("child-0").closest(".album-grid") !== nextDayGroup, "day split preserves separate virtual groups and timestamps");
        checks.push("parent/child unread mapping, split/partial single-child grids, missing envelope fallback and day boundaries");

        const quoteVariants: Partial<StoredMessage>[] = [{ reply_to_view_once: true, reply_to_recoverable: true },
          { reply_to_view_once: true, reply_to_recoverable: true, reply_to_path: thumb }, { from_me: true, reply_to_view_once: true, reply_to_recoverable: false }];
        for (const [index, patch] of quoteVariants.entries()) {
          const root = { ...parent, ...patch };
          await replace([root, ...children.slice(0, 2).map((message) => ({ ...message, from_me: root.from_me }))]);
          const before = actions.length; quote().click();
          const expected = index === 0 ? "quote-save" : index === 1 ? "quote-open" : "quote-jump";
          assert(actions.length === before + 1 && actions.at(-1) === `${expected}:parent:quoted-original`, "quoted copy action targets parent with correct privacy gate");
          if (index === 2) assert(quote().textContent?.includes("No copy from this app") && !actions.slice(before).some((action) => action.startsWith("quote-save")), "sent-here view-once quote cannot save unavailable copy");
          for (const zoom of [100, 200]) {
            document.documentElement.style.zoom = `${zoom}%`; width = 320; await tick(); await wait();
            assert(grids()[0].scrollWidth <= grids()[0].clientWidth + 1, `parent quote fits 320px/${zoom}% variant ${index}`);
          }
          document.documentElement.style.zoom = "100%";
        }
        checks.push("parent quote jump/save/open callbacks, unavailable sent-here copy privacy and narrow quote layouts");

        await replace([parent, children[0], row("hidden", "parent", { text: "hidden-token private caption" }), children[1]]);
        assert(!scroller!.querySelector('[data-id="hidden"]') && grids().length === 2, "keyword-hidden child leaves no marker or count and splits groups");
        assert(!scroller!.textContent?.includes("private caption"), "hidden child caption absent");
        await replace([parent, children[0], row("spoiler", "parent", { spoiler: true, text: "private spoiler payload" }), children[1]]);
        assert(grids().length === 2 && !grids().some((grid) => grid.querySelector('[data-id="spoiler"]')) && bubble("spoiler").querySelector(".spoiler-reveal") && !bubble("spoiler").querySelector("img"), "private child remains ordinary protected row");
        for (const patch of [{ sender: "foreign-sender" }, { chat: "foreign-chat" }, { from_me: true }, { spoiler: true }, { deleted: true },
          { revoked: true }, { media_kind: "image" }, { media_kind: "view_once", media_once_kind: "album", media_thumb: thumb }]) {
          await replace([{ ...parent, ...patch }, ...children.slice(0, 2)]); boundary(JSON.stringify(patch));
          if (patch.media_kind === "view_once") assert(!bubble("parent").querySelector("img"), "view-once album root carries no visible preview");
        }
        await replace([{ ...parent, text: "hidden-token private parent quote", reply_to_text: "private parent quote" }, ...children.slice(0, 2)]);
        boundary("hidden parent"); assert(!scroller!.querySelector('[data-id="parent"]') && !scroller!.textContent?.includes("private parent quote"), "hidden parent quote and anchor absent");
        checks.push("keyword-hidden/private child boundaries and hidden/private/tombstone/view-once/foreign/non-album root rejection");

        await replace([parent, ...children.slice(1, 3)]);
        const partialRaw = JSON.stringify(rows);
        assert(ids().join() === "child-1,child-2", "partial page raw order retained");
        await replace([parent, ...children.slice(0, 3)]); assert(ids().join() === "child-0,child-1,child-2", "prepend child appears once in raw draw order");
        await replace([parent, ...children.slice(1, 3)]); assert(ids().join() === "child-1,child-2" && JSON.stringify(rows) === partialRaw, "eviction preserves raw IDs and original data");
        const outgoing = [parent, ...children.slice(0, 2)].map((message) => ({ ...message, from_me: true, status: "sent" }));
        await replace(outgoing);
        const patch = outgoing.map((message) => message.id === "child-1" ? { ...message, status: "read", read: true } : message), patchRaw = JSON.stringify(patch);
        await replace(patch);
        assert(bubble("child-1").querySelector(".ticks.read") && !bubble("child-0").querySelector(".ticks.read"), "status patch updates only correct child's read ticks");
        assert(ids().join() === "child-0,child-1" && JSON.stringify(rows) === patchRaw, "read markers and raw status remain unchanged by rendering");
        checks.push("partial/prepend/evict/status patch raw order/read markers and immutable source data");

        const lateChildren = children.map((message, index) => ({ ...message, timestamp: 100, sort_order: index + 1 }));
        const lateParent = { ...parent, timestamp: 100, sort_order: lateChildren.length + 1 };
        await replace([...lateChildren, lateParent], "parent");
        const lateRaw = JSON.stringify(rows), anchor = scroller!.querySelector<HTMLElement>('.album-anchor[data-id="parent"]')!;
        assert(grids().length === 1 && (grids()[0].compareDocumentPosition(anchor) & Node.DOCUMENT_POSITION_FOLLOWING), "late envelope marker follows continuous child grid");
        assert(grids()[0].querySelectorAll("time").length === 1 && ids().join() === lateChildren.map((message) => message.id).join(), "late parent keeps one time and raw child markers");
        frameHeight = 380; await tick(); await wait();
        viewport().scrollTop = 0; await tick(); await wait();
        assert(anchor.getBoundingClientRect().top >= viewport().getBoundingClientRect().bottom && frontier() !== "parent", "late parent cannot become frontier before its chronological marker is visible");
        const markers = [...scroller!.querySelectorAll<HTMLElement>(".bubble[data-id], .album-anchor[data-id]")];
        const transforms = markers.map((marker) => marker.style.transform);
        markers.forEach((marker) => { if (marker.dataset.id !== "child-1") marker.style.transform = "translateY(2000px)"; });
        assert(bubble("child-0").getBoundingClientRect().top >= viewport().getBoundingClientRect().bottom && bubble("child-1").getBoundingClientRect().top < viewport().getBoundingClientRect().bottom, "fixture creates non-monotone column positions");
        assert(frontier() === "child-1", "raw frontier scans past offscreen first DOM column");
        markers.forEach((marker, index) => { marker.style.transform = transforms[index]; });
        rail!.scrollToBottom(); await tick(); await wait();
        assert(frontier() === "parent", "visible late parent becomes highest raw frontier");
        assert(JSON.stringify(rows) === lateRaw && rows.at(-1)?.read === false, "frontier decision never fabricates mark-read success or changes raw metadata");
        await replace([lateChildren[0], { ...lateParent, sort_order: 2 }, { ...lateChildren[1], sort_order: 3 }]);
        assert(grids().length === 1 && scroller!.querySelectorAll('.album-anchor[data-id="parent"]').length === 1, "interleaved envelope does not split otherwise continuous children");
        await replace([lateChildren.at(-1)!, lateParent]);
        assert(grids().length === 1 && ids().join() === "child-5", "paged single child retains late parent marker without invented children");
        rail!.scrollToBottom(); await tick(); await wait(); assert(frontier() === "parent", "partial paging retains late control-row frontier");
        await replace(lateChildren.slice(0, 2));
        assert(!scroller!.querySelector('.album-anchor[data-id="parent"]') && frontier() !== "parent", "evicted parent never creates a synthetic read marker");
        checks.push("late/interleaved envelope chronology, non-monotone column raw frontier, partial paging and no synthetic mark-read success");

        const ordinary = Array.from({ length: 90 }, (_, index) => row(`rail-${index}`, null, { media_kind: null, timestamp: 20 + index, text: `Synthetic rail ${index}. ${"bodyword".repeat(8)}` }));
        frameHeight = 380;
        const virtualParent = { ...parent, timestamp: 1000 };
        const virtualChildren = children.map((message, index) => ({ ...message, timestamp: 1001 + index }));
        await replace([...ordinary, virtualParent, ...virtualChildren]);
        rail!.scrollToBottom(); await until(() => inView("child-5"), "VList bottom did not reveal last child");
        assert(ids().length < rows.length - 1, "VList leaves offscreen raw rows unmounted");
        assert(rail!.hasMessage("rail-0") && rail!.hasMessage("parent") && virtualChildren.every((message) => rail!.hasMessage(message.id)), "virtual lookup includes offscreen children and loaded suppressed parent");
        assert(!rail!.hasMessage("unknown") && !rail!.revealMessage("unknown"), "virtual lookup rejects unknown IDs");
        assert(rail!.revealMessage("rail-0"), "virtual reveal accepts offscreen ordinary row");
        await until(() => inView("rail-0"), "offscreen ordinary row did not mount into viewport");
        assert(rail!.revealMessage("child-0"), "virtual reveal accepts first child in tall album group");
        await until(() => inView("child-0"), "first child in tall album group remained offscreen");
        assert(rail!.revealMessage("parent"), "virtual reveal accepts loaded parent marker");
        await until(() => inView("parent"), "suppressed parent marker did not enter viewport");
        rail!.scrollToBottom(); await until(() => inView("child-5"), "virtual bottom did not restore last child");
        const visibleAnchor = rail!.anchorId();
        assert(visibleAnchor && inView(visibleAnchor), "virtual anchor comes from actual visible content instead of overscan");
        assert(metrics.viewport > 0 && metrics.offset >= 0 && metrics.distance >= -1, "collaborator VList scroll metrics remain available");
        await replace([...ordinary, virtualParent, ...virtualChildren], "child-3");
        assert(rail!.scrollToUnread(), "virtual unread divider exists after album split");
        await until(() => {
          const divider = scroller!.querySelector<HTMLElement>("[data-unread-divider]");
          return divider && divider.getBoundingClientRect().bottom > viewport().getBoundingClientRect().top && divider.getBoundingClientRect().top < viewport().getBoundingClientRect().bottom;
        }, "virtual unread divider did not enter viewport");
        await replace(virtualChildren.slice(0, 2));
        assert(!rail!.hasMessage("parent"), "missing envelope is not fabricated by child association lookup");
        checks.push("actual VList unmounting, every child/parent lookup, offscreen and tall-group reveals, visible anchor, metrics and unread split");
        frameHeight = 3000;

        for (const count of [1, 2, 3, 4, 6]) for (const size of [400, 320]) for (const zoom of [100, 200]) {
          await replace([parent, ...children.slice(0, count)]); width = size; document.documentElement.style.zoom = `${zoom}%`; await tick(); await wait();
          const grid = grids()[0], rect = grid.getBoundingClientRect();
          assert(rect.right <= scroller!.getBoundingClientRect().right + 1 && rect.width <= innerWidth, `real timeline grid fits ${count}/${size}/${zoom}`);
          assert(grid.scrollWidth <= grid.clientWidth + 1 && scroller!.scrollWidth <= scroller!.clientWidth + 1, `real timeline no horizontal overflow ${count}/${size}/${zoom}`);
          assert(grid.querySelectorAll("time").length === 1 && (grid.textContent || "").split("[time-").length === 2, "one shared timestamp through responsive layouts");
        }
        document.documentElement.style.zoom = "100%";
        assert(calls.length === 0, `no IPC calls: ${calls.join(", ")}`);
        assert(warnings.length === 0, `no runtime warnings: ${warnings.join("; ")}`);
        checks.push("actual timeline 1/2/3/4/6 children at 400px/320px and 200% text/page scale, zero IPC/runtime warnings");
        complete = true;
      } catch (failure) { failed = String(failure); complete = true; document.documentElement.style.zoom = "100%"; }
    })();
    return () => { console.warn = warn; document.documentElement.style.zoom = "100%"; };
  });
</script>

<main>
  <h1>Production album timeline with synthetic data</h1>
  {#if readyKeyboard}<p>Press Enter to open focused synthetic child.</p>{/if}
  <div class="frame" style:width="{width}px" style:height="{frameHeight}px"><MessageList {...props} messages={rows} bind:scroller bind:this={rail} /></div>
  <div id="album-list-result" data-complete={complete} data-pass={complete && !failed} data-keyboard={readyKeyboard} data-viewport={innerWidth}>
    {#if failed}<p role="alert">{failed}</p>{/if}<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
  </div>
</main>

<style>
  :global(:root) { --muted: #aebac1; --text: #e9edef; --surface: #202c33; --bubble: #202c33; --bubble-mine: #005c4b; --raised: #2a3942; --line: #3b4a54; --accent: #00a884; --accent-text: #66d9b9; --danger: #f47b7b; --row-hover: #ffffff08; --radius-sm: 6px; --faint: #667781; --motion-scale: 0; --ease: linear; font: 14px "Segoe UI", sans-serif; }
  :global(body) { margin: 0; padding: 12px; background: #111b21; color: var(--text); }
  main { max-width: 520px; margin: auto; }
  h1 { font-size: 18px; }
  .frame { display: flex; max-width: 100%; height: 380px; box-sizing: border-box; }
</style>
