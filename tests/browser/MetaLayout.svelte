<script lang="ts">
  import { tick } from "svelte";
  import MessageBubble from "$lib/messages/MessageBubble.svelte";
  import { messages, bubbleView, bubbleApi } from "$lib/utils/theme-preview";

  const cases = ["09:32", "12:34 PM", "١٢:٣٤ مساءً"].flatMap((time) =>
    ["ok", "+1"].flatMap((text) => [false, true].flatMap((own) =>
      [false, true].flatMap((edited) => [false, true].flatMap((starred) =>
        [1, 2].flatMap((zoom) => [false, true].map((caption) => ({ time, text, own, edited, starred, zoom, caption }))))))));
  let root: HTMLDivElement;
  let result = $state("Not checked");

  async function check() {
    await tick();
    await document.fonts.ready;
    const failures: string[] = [];
    for (const row of root.querySelectorAll<HTMLElement>("[data-case]")) {
      const text = row.querySelector(".text")!;
      const walker = document.createTreeWalker(text, NodeFilter.SHOW_TEXT);
      let node: Node | null;
      let content: Node | null = null;
      while ((node = walker.nextNode())) {
        if (node.textContent?.trim() && !node.parentElement?.closest(".meta-spacer")) { content = node; break; }
      }
      if (!content) { failures.push("Missing text"); continue; }
      const range = document.createRange();
      range.selectNodeContents(content);
      const letters = range.getBoundingClientRect();
      const meta = row.querySelector(".meta")!.getBoundingClientRect();
      const spacer = row.querySelector(".meta-spacer")!.getBoundingClientRect();
      const zoom = Number(row.dataset.zoom);
      if (letters.right + 3 * zoom > meta.left || Math.abs(letters.bottom - meta.bottom) > 8 * zoom || spacer.width < meta.width) {
        failures.push(`${row.dataset.case}: letters ${letters.right.toFixed(1)}, time ${meta.left.toFixed(1)}, baseline ${(letters.bottom - meta.bottom).toFixed(1)}`);
      }
    }
    result = failures.length ? failures.join("\n") : `PASS: ${cases.length} layouts; no overlap or empty time line`;
  }
</script>

<details open>
  <summary>Timestamp layout regression</summary>
  <button onclick={check}>Check timestamp layouts</button>
  <pre aria-label="Timestamp layout result">{result}</pre>
  <div bind:this={root}>
    {#each cases as c, index (index)}
      {@const message = { ...messages[0], id: String(index), text: c.text, from_me: c.own, status: c.own ? "read" : null, media_kind: c.caption ? "image" : null, media_thumb: c.caption ? 'data:image/svg+xml,<svg xmlns="http://www.w3.org/2000/svg" width="160" height="60"><rect width="160" height="60" fill="gray"/></svg>' : null }}
      {@const vm = { ...bubbleView(message, 1, "fixture"), isEdited: c.edited, isStarred: c.starred, visual: c.caption, caption: c.caption ? c.text : "" }}
      <div class="case" data-case={`${c.text} ${c.time} own=${c.own} edited=${c.edited} starred=${c.starred} scale=${c.zoom} caption=${c.caption}`} data-zoom={c.zoom} style:zoom={c.zoom}>
        <MessageBubble {message} {vm} api={{ ...bubbleApi, formatTime: () => c.time }} />
      </div>
    {/each}
  </div>
</details>

<style>
  .case { width: 600px; margin-bottom: 12px; font: 14px "Segoe UI", sans-serif; }
  pre { white-space: pre-wrap; }
</style>
