<script lang="ts">
  import { onMount, tick } from "svelte";
  import Embed from "$lib/messages/cards/Embed.svelte";
  import EventCard from "$lib/messages/cards/EventCard.svelte";
  import MessageCard from "$lib/messages/cards/MessageCard.svelte";
  import { messages, bubbleApi, bubbleView } from "$lib/utils/theme-preview";

  const image = (width: number) => `data:image/svg+xml,${encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="100"><rect width="100%" height="100%" fill="#407fa5"/></svg>`)}`;
  const cases = [false, true].flatMap((quote) => [260, 400].flatMap((panel) => [80, 440].map((width) => ({
    id: `${quote ? "quote" : "link"}-${width}-${panel}`, quote, panel,
    message: { ...messages[0], text: "Synthetic body", reply_to_sender: "Synthetic author", reply_to_text: "Synthetic quote",
      reply_to_kind: "image", reply_to_thumb: image(width), preview_url: "https://example.invalid/card",
      preview_title: "Synthetic title", preview_desc: "Synthetic description", preview_thumb: image(width) },
  }))));
  const poll = { id: "synthetic-poll", name: "Synthetic poll", options: ["A", "B"], multi: false, votes: [] };
  const event = { id: "synthetic-event", name: "Synthetic event", description: "Synthetic description", start: null,
    end: null, location: "Synthetic place", link: null, canceled: false, responses: [] };
  const special = ["contact", "location", "business", "album", "music", "ai_response", "future_card", "event", "live_location"].map((kind) => ({
    ...messages[0], id: `synthetic-${kind}`, media_kind: kind, text: `Readable ${kind} <script> & details`,
    live_location: kind === "live_location" ? { lat: 1, lng: 2, accuracy: 4, speed: null, heading: null,
      sequence: null, started_at: 0, updated_at: 0, expires_at: null, ended: true } : null,
  }));
  let root: HTMLDivElement;
  let result = $state("Waiting");
  let actions = $state<string[]>([]);

  async function check() {
    await tick();
    await document.fonts.ready;
    await Promise.all([...root.querySelectorAll("img")].map((img) => img.decode()));
    await tick();
    const failures: string[] = [];
    for (const item of cases) {
      const row = root.querySelector(`[data-case="${item.id}"]`)!;
      const [before, after] = [...row.querySelectorAll<HTMLElement>(".side > .embed")];
      const from = before.getBoundingClientRect(), to = after.getBoundingClientRect();
      const left = [before, ...before.querySelectorAll<HTMLElement>("*")];
      const right = [after, ...after.querySelectorAll<HTMLElement>("*")];
      if (left.length !== right.length) { failures.push(`${item.id}: DOM length`); continue; }
      for (let i = 0; i < left.length; i++) {
        const a = left[i], b = right[i], ar = a.getBoundingClientRect(), br = b.getBoundingClientRect();
        const text = (node: HTMLElement) => node.textContent?.replace(/\s+/g, " ").trim();
        if (a.tagName !== b.tagName || text(a) !== text(b)) failures.push(`${item.id}: content ${i}`);
        for (const [x, y] of [[ar.x - from.x, br.x - to.x], [ar.y - from.y, br.y - to.y], [ar.width, br.width], [ar.height, br.height]]) {
          if (Math.abs(x - y) > 0.1) failures.push(`${item.id}: geometry ${i}`);
        }
        for (const prop of ["display", "flex-direction", "gap", "padding", "border-radius", "font-size", "line-height", "color"]) {
          if (getComputedStyle(a).getPropertyValue(prop) !== getComputedStyle(b).getPropertyValue(prop)) failures.push(`${item.id}: ${prop} ${i}`);
        }
        if (b.tagName === "BUTTON" && !b.textContent?.trim() && !b.getAttribute("title") && !b.getAttribute("aria-label")) failures.push(`${item.id}: unnamed button`);
      }
      after.querySelector<HTMLButtonElement>("button")?.click();
      if (after.tagName === "BUTTON") after.click();
    }
    if (actions.join(",") !== cases.map((item) => `${item.id}:${item.quote ? "jump" : "open"}`).join(",")) failures.push(`actions: ${actions}`);
    for (const message of special) {
      const row = root.querySelector<HTMLElement>(`[data-special="${message.media_kind}"]`)!;
      const baseline = message.media_kind === "event" ? root.querySelector<HTMLElement>("[data-reference=event]") : null;
      if (row.scrollWidth > (baseline?.scrollWidth ?? row.clientWidth) || row.querySelector("script")) failures.push(`${message.media_kind}: fallback/width`);
      if (message.media_kind === "event") {
        const answer = row.querySelector<HTMLButtonElement>(".answer");
        if (!answer || answer.disabled) failures.push("event response unavailable");
        answer?.click();
      } else if (message.media_kind === "live_location") row.querySelector<HTMLButtonElement>(".map")?.click();
      else if (!row.textContent?.includes(message.text)) failures.push(`${message.media_kind}: missing readable text`);
    }
    root.querySelector<HTMLButtonElement>("[data-special=poll] .option")?.click();
    await tick();
    if (!actions.includes('vote:synthetic-poll:["A"]') || !actions.includes("respond:synthetic-event:going") || !actions.includes("live_location:https://maps.google.com/?q=1,2")) failures.push(`special callbacks: ${actions}`);
    result = failures.length ? failures.join("\n") : "PASS: 8 quote/link layouts at 260/400px, image sizing/styles, accessible buttons, 10 special cards/fallbacks, quote/link/vote/RSVP/Maps callbacks";
  }

  onMount(() => { check().catch((error) => result = String(error)); });
</script>

<main>
  <h1>Shared message cards</h1>
  <pre aria-label="Card comparison result">{result}</pre>
  <div bind:this={root}>
    {#each cases as item (item.id)}
      <h2>{item.id}</h2>
      <div class="pair" data-case={item.id}>
        <div class="side" style:width={`${item.panel}px`}>
          <Embed compact={item.quote} label={item.quote ? item.message.reply_to_sender : "example.invalid"}
            title={item.quote ? null : item.message.preview_title}
            text={item.quote ? item.message.reply_to_text : item.message.preview_desc}
            image={item.quote ? item.message.reply_to_thumb : item.message.preview_thumb}
            tooltip={item.quote ? "Go to message" : item.message.preview_url}
            onclick={item.quote ? () => {} : undefined} onopen={item.quote ? undefined : () => {}} />
        </div>
        <div class="side" style:width={`${item.panel}px`}>
          <MessageCard message={item.message} vm={bubbleView(item.message, 0, "chat")} variant={item.quote ? "quote" : "link"}
            api={{ ...bubbleApi, onjumpquoted: () => { actions.push(`${item.id}:jump`); }, onopenurl: () => { actions.push(`${item.id}:open`); } }} />
        </div>
      </div>
    {/each}
    <h2>Special cards at 260px</h2>
    <div class="special" data-reference="event">
      <EventCard {event} title="Readable event" onopenurl={() => {}} onrespond={async () => {}} />
    </div>
    <div class="special" data-special="poll">
      <MessageCard variant="poll" {poll} question={poll.name} namer={bubbleApi.namer} picture={bubbleApi.avatarOf}
        onvote={async (options) => { actions.push(`vote:${poll.id}:${JSON.stringify(options)}`); }} />
    </div>
    {#each special as message (message.id)}
      <div class="special" data-special={message.media_kind}>
        <MessageCard {message} vm={{ ...bubbleView(message, 0, "chat"), chatEvent: message.media_kind === "event" ? event : undefined }}
          api={{ ...bubbleApi, onrespond: async (message, answer) => { actions.push(`respond:${message.id}:${answer}`); },
            onopenurl: (url) => { actions.push(`${message.media_kind}:${url}`); } }} />
      </div>
    {/each}
  </div>
</main>

<style>
  :global(body) { margin: 24px; background: #111419; color: #e4e7eb; font: 14px "Segoe UI", sans-serif; --text: #e4e7eb; --muted: #8e99a7; --accent: #6caeda; --link: #8abdeb; }
  .pair { display: flex; gap: 24px; }
  .special { width: 260px; margin-top: 12px; }
  h1 { font-size: 20px; }
  h2 { font-size: 14px; font-weight: 500; }
  pre { white-space: pre-wrap; }
</style>
