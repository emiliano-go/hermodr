<script lang="ts">
  import Gallery from "$lib/media/Gallery.svelte";
  import type { ChatSummary, GalleryCursor, GalleryFilter, GalleryItem, GalleryPage, StoredMessage } from "$lib/utils/wire";

  const thumb = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='200' height='160'%3E%3Crect width='200' height='160' fill='%2300a884'/%3E%3C/svg%3E";
  const rows: GalleryItem[] = Array.from({ length: 65 }, (_, n) => ({
    message: { chat: n % 2 ? "beta@g.us" : "alpha@s", id: `fixture-${n}`, sender: "peer@s", from_me: n % 3 === 0,
      timestamp: 1790722800 + n * 86400, sort_order: n + 1, text: n === 64 ? "SECRET SPOILER CAPTION" : `Fixture ${n}`,
      media_kind: n % 2 ? "video" : "image", media_path: null, media_thumb: thumb, spoiler: n === 64 } as StoredMessage, urls: [],
  }));
  rows.push({ message: { chat: "alpha@s", id: "links", sender: "peer@s", from_me: false, timestamp: 1790722800,
    sort_order: 66, text: "Plain link without preview", media_kind: null, media_path: null, media_thumb: null, preview_thumb: null, spoiler: false } as StoredMessage,
    urls: ["https://synthetic.test/?a=1&b=2"] });
  rows.push({ message: { ...rows.at(-1)!.message, id: "hidden-links", timestamp: 1790809200, sort_order: 67,
    text: "SECRET LINK CAPTION", preview_title: "SECRET PREVIEW TITLE", preview_thumb: thumb, spoiler: true }, urls: ["https://hidden.synthetic.test"] });
  let account = $state("synthetic-A");
  let requests = $state("");
  let action = $state("");
  const chats = [{ chat: "alpha@s" }, { chat: "beta@g.us" }] as ChatSummary[];

  async function fetchPage(account: string, filter: GalleryFilter, cursor: GalleryCursor | null, limit: number): Promise<GalleryPage> {
    requests = JSON.stringify({ account, filter, cursor, limit });
    const matches = rows.filter(({ message: m, urls }) => (!filter.chat || filter.chat === m.chat) &&
      (filter.kind === "link" ? urls.length > 0 : filter.kind ? m.media_kind === filter.kind : m.media_kind !== null) &&
      (filter.from_me === null || m.from_me === filter.from_me) && (filter.since === null || m.timestamp >= filter.since) &&
      (filter.until === null || m.timestamp < filter.until) && (!cursor || m.timestamp < cursor.timestamp || m.timestamp === cursor.timestamp && m.sort_order < cursor.sort_order))
      .toSorted((a, b) => b.message.timestamp - a.message.timestamp || b.message.sort_order - a.message.sort_order);
    const items = matches.slice(0, limit);
    const last = items.at(-1)?.message;
    return { items, next_cursor: matches.length > limit && last ? { timestamp: last.timestamp, sort_order: last.sort_order, chat: last.chat, id: last.id } : null };
  }
</script>

<div class="fixture-controls"><button onclick={() => account = account === "synthetic-A" ? "synthetic-B" : "synthetic-A"}>Switch synthetic account</button><span>{account}</span><output>{action}</output></div>
<main>
  <Gallery accountKey={account} chat="alpha@s" {chats} chatName={(chat) => chat === "alpha@s" ? "Alpha" : "Beta"}
    senderName={() => "Synthetic peer"} onjump={(chat, id) => action = `jump ${chat} ${id}`} onopen={(path) => action = `open ${path}`}
    onreply={(message) => action = `reply ${message.chat} ${message.id}`} onclose={() => action = "close"} {fetchPage} />
</main>
<output class="request" aria-label="Last gallery request">{requests}</output>

<style>
  :global(:root) { --bg: #111b21; --surface: #202c33; --raised: #2a3942; --line: #3b4a54; --text: #e9edef; --muted: #8696a0; --accent: #00a884; --danger: #f15c6d; }
  :global(body) { margin: 0; background: var(--bg); color: var(--text); font-family: system-ui; }
  main { display: flex; height: 700px; }
  .fixture-controls { display: flex; gap: 15px; align-items: center; padding: 10px; }
  .request { display: block; font-size: 12px; padding: 8px; overflow-wrap: anywhere; }
</style>
