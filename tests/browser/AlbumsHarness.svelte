<script lang="ts">
  import { onMount, tick } from "svelte";
  import AlbumGrid from "$lib/messages/AlbumGrid.svelte";
  import { groupAlbumRows } from "$lib/utils/albums";
  import type { StoredMessage } from "$lib/utils/wire";

  const catalog = Array.from({ length: 6 }, (_, index) => ({ id: `synthetic-child-${index}`, chat: "synthetic-chat", sender: "synthetic-sender",
    from_me: false, media_kind: index % 2 ? "video" : "image", timestamp: 100 + index, text: `Synthetic caption ${index}. ${"longword".repeat(10)}`,
    sort_order: index, spoiler: false, media_once_kind: null, reply_to_view_once: false, revoked: false, deleted: false, system_kind: null } as StoredMessage));
  let items = $state.raw<StoredMessage[]>(catalog.slice(1, 3));
  let prev = $state.raw<StoredMessage | undefined>(catalog[0]);
  let timestamp = $state(200), width = $state(400), scale = $state(100);
  let selected = $state<string[]>([]), viewed = $state(""), menu = $state(""), replied = $state(""), downloaded = $state(""), jumped = $state("");
  let checks = $state<string[]>([]), complete = $state(false), failed = $state("");
  const assert = (condition: unknown, label: string) => { if (!condition) throw new Error(label); };
  const marker = (id: string) => document.querySelector<HTMLButtonElement>(`.bubble[data-id="${id}"]`)!;
  const grid = () => document.querySelector<HTMLElement>(".album-grid")!;
  const markers = () => [...grid().querySelectorAll<HTMLElement>(".bubble[data-id]")];
  const child = (event: Event) => items.find((message) => message.id === (event.currentTarget as HTMLElement).dataset.id);
  function openChild(event: MouseEvent) { const message = child(event); if (message) viewed = message.id; }
  function contextChild(event: MouseEvent) { event.preventDefault(); const message = child(event); if (message) menu = message.id; }
  function keyChild(event: KeyboardEvent) {
    if (event.isComposing || event.keyCode === 229) return;
    if (event.key === "Enter" && !event.shiftKey && !event.ctrlKey && !event.metaKey) { event.preventDefault(); const message = child(event); if (message) viewed = message.id; }
  }
  function pick(event: MouseEvent) {
    if (!event.ctrlKey && !event.metaKey) return;
    const row = (event.target as HTMLElement).closest(".msg-row");
    const id = row?.querySelector<HTMLElement>(".bubble[data-id]")?.dataset.id;
    if (!id) return;
    event.preventDefault(); event.stopPropagation();
    selected = selected.includes(id) ? selected.filter((value) => value !== id) : [...selected, id];
  }
  function jump(id: string) { marker(id)?.scrollIntoView({ block: "center" }); jumped = id; }

  onMount(() => {
    const warnings: string[] = [], warn = console.warn;
    console.warn = (...args) => { warnings.push(args.join(" ")); warn(...args); };
    void (async () => {
      try {
        await tick();
        assert(markers().map((element) => element.dataset.id).join() === items.map((message) => message.id).join(), "partial loaded children preserve raw IDs/order");
        assert(grid().querySelectorAll("time").length === 1 && grid().querySelector("time")?.textContent === "time:200", "one shared envelope timestamp");
        assert(grid().querySelector("time")?.dateTime === new Date(200_000).toISOString(), "shared time machine value");
        assert(!/\bof\b|complete|total|6 items/i.test(grid().textContent || ""), "partial group makes no completeness/count claims");
        const retained = marker(catalog[2].id);
        items = catalog.slice(0, 4); await tick();
        assert(marker(catalog[2].id) === retained, "prepend keeps existing child DOM identity");
        assert(markers().map((element) => element.dataset.id).join() === items.map((message) => message.id).join(), "prepend keeps draw order");
        items = items.slice(1); await tick(); assert(marker(catalog[2].id) === retained, "eviction keeps remaining child identity");
        timestamp = items[0].timestamp; await tick();
        assert(grid().querySelector("time")?.textContent === `time:${items[0].timestamp}`, "missing parent uses supplied first-loaded-child timestamp");
        checks.push("partial group, one shared time, child markers/order and stable DOM through prepend/eviction");

        const target = items[1].id;
        marker(target).dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true })); await tick();
        assert(selected.includes(target) && viewed === "", "child selection intercepts media action");
        marker(target).dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true })); await tick(); assert(menu === target, "child context menu retains exact ID");
        marker(target).dispatchEvent(new MouseEvent("dblclick", { bubbles: true })); await tick(); assert(replied === target, "child reply retains exact ID");
        marker(target).focus(); assert(document.activeElement === marker(target), "child focus reachable");
        marker(target).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true, isComposing: true }));
        assert(viewed === "", "IME cannot activate child");
        marker(target).dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true })); await tick(); assert(viewed === target, "keyboard opens exact child");
        document.querySelector<HTMLButtonElement>(`[data-download="${target}"]`)!.click(); await tick(); assert(downloaded === target, "child download preserves target");
        jump(target); await tick(); assert(jumped === target && marker(target).getBoundingClientRect().top >= document.querySelector(".scroller")!.getBoundingClientRect().top, "child jump marker usable");
        checks.push("per-child selection, menu, reply, keyboard/IME, download and jump targets");

        for (const count of [1, 2, 3, 4, 6]) {
          items = catalog.slice(0, count);
          for (const size of [400, 320]) for (const percent of [100, 200]) {
            width = size; scale = percent; await tick();
            const parent = document.querySelector<HTMLElement>(".scroller")!, rect = grid().getBoundingClientRect();
            assert(rect.width <= size && rect.right <= parent.getBoundingClientRect().right + 1, `${count} children fit ${size}px/${percent}%`);
            assert(grid().scrollWidth <= grid().clientWidth + 1 && parent.scrollWidth <= parent.clientWidth + 1, `no horizontal overflow ${count}/${size}/${percent}`);
            for (const element of markers()) assert(element.scrollWidth <= element.clientWidth + 1, `child text fits ${count}/${size}/${percent}`);
            assert(grid().querySelectorAll("time").length === 1, "one shared time at every size/count");
          }
        }
        checks.push("1/2/3/4/6 loaded children at 400px/320px and 100%/200% text without overflow");

        const privateRows = [{ ...catalog[0], spoiler: true }, { ...catalog[1], media_once_kind: "image" }, catalog[2], catalog[3]];
        const groups = groupAlbumRows(privateRows, () => "synthetic-parent", { visible: (message) => message.id !== catalog[0].id });
        items = groups.find((group) => group.messages.length > 1)!.messages; await tick();
        assert(!grid().querySelector(`[data-id="${catalog[0].id}"]`) && !grid().querySelector(`[data-id="${catalog[1].id}"]`), "hidden and private rows absent from album grid");
        assert(markers().map((element) => element.dataset.id).join() === [catalog[2].id, catalog[3].id].join(), "eligible children remain usable after privacy boundary");
        assert(warnings.length === 0, `no runtime warnings: ${warnings.join("; ")}`);
        checks.push("hidden/private rows excluded, visible child IDs preserved and zero runtime warnings");
        complete = true;
      } catch (failure) { failed = String(failure); complete = true; }
    })();
    return () => { console.warn = warn; };
  });
</script>

<main>
  <h1>Album grid synthetic checks</h1>
  <div class="scroller" style:width="{width}px" style:font-size="{scale}%" onclickcapture={pick} role="group" aria-label="Synthetic messages">
    <AlbumGrid {items} {prev} {timestamp} formatTime={(value) => `time:${value}`}>
      {#snippet children(message, previous)}
        <div class="msg-row" data-previous={previous?.id}>
          <button type="button" class="bubble" data-id={message.id} data-chat={message.chat} aria-label="Open {message.id}"
            aria-pressed={selected.includes(message.id)} onclick={openChild} oncontextmenu={contextChild} onkeydown={keyChild}
            ondblclick={(event) => { const message = child(event); if (message) replied = message.id; }}>
            <span class="thumbnail" aria-hidden="true">{message.media_kind}</span><span class="caption">{message.text}</span>
            <span class="status" aria-label="Child status">✓ · Star · Edited</span>
          </button>
          <button type="button" class="download" data-download={message.id} onclick={(event) => { downloaded = event.currentTarget.dataset.download || ""; }}>Download child</button>
        </div>
      {/snippet}
    </AlbumGrid>
  </div>
  <div id="albums-result" data-complete={complete} data-pass={complete && !failed} data-viewport={innerWidth}>
    {#if failed}<p role="alert">{failed}</p>{/if}<ul>{#each checks as check}<li>{check}</li>{/each}</ul>
  </div>
</main>

<style>
  :global(:root) { --muted: #aebac1; font: 14px "Segoe UI", sans-serif; }
  :global(body) { margin: 0; padding: 12px; background: #111b21; color: #e9edef; }
  main { max-width: 520px; margin: auto; }
  h1 { font-size: 18px; }
  .scroller { display: flex; flex-direction: column; max-width: 100%; height: 300px; overflow-y: auto; box-sizing: border-box; }
  .msg-row { min-width: 0; }
  .bubble, .download { display: flex; width: 100%; min-width: 0; box-sizing: border-box; padding: 6px; border: 1px solid #3b4a54; border-radius: 6px; background: #202c33; color: inherit; font: inherit; text-align: left; overflow-wrap: anywhere; cursor: pointer; }
  .bubble { flex-direction: column; gap: 4px; }
  .bubble:focus-visible, .download:focus-visible { outline: 2px solid #00a884; outline-offset: -2px; }
  .thumbnail { display: grid; place-items: center; width: 100%; min-height: 65px; background: #2a3942; }
  .caption, .status { width: 100%; overflow-wrap: anywhere; }
  .status, .download { font-size: .75em; }
  .download { margin-top: 4px; }
</style>
