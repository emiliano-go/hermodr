import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

test("shared message cards preserve quotes, links, special views and readable fallbacks", async () => {
  const server = await createServer({
    configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/message-card", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } },
    server: { middlewareMode: true, ws: false },
  });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const [{ default: MessageCard }, { default: Embed }, { render }, preview] = await Promise.all([
      load("../lib/messages/cards/MessageCard.svelte"), load("../lib/messages/cards/Embed.svelte"),
      server.ssrLoadModule("svelte/server"), load("../lib/utils/theme-preview.ts"),
    ]);
    const base = { ...preview.messages[0], text: "Synthetic body", spoiler: false };
    const draw = (message: object, variant = "message", extra = {}) => render(MessageCard, {
      props: { message, vm: { ...preview.bubbleView(message, 0, "chat"), ...extra }, api: preview.bubbleApi, variant },
    }).body;
    const normalize = (html: string) => html.replace(/<!--.*?-->/gs, "").replace(/svelte-[\w-]+/g, "").replace(/\s+/g, " ").trim();

    const quote = { ...base, reply_to_text: "Quoted <text>", reply_to_sender: "Synthetic author" };
    assert.equal(normalize(draw(quote, "quote")), normalize(render(Embed, {
      props: { compact: true, label: quote.reply_to_sender, text: quote.reply_to_text, tooltip: "Go to message", onclick() {} },
    }).body));
    const once = { ...quote, reply_to_view_once: true, reply_to_recoverable: true };
    assert.match(draw(once, "quote"), /Save the copy|Tap to save/);
    assert.match(draw({ ...once, reply_to_path: "synthetic-copy" }, "quote"), /Open the copy/);
    assert.doesNotMatch(draw({ ...once, from_me: true, reply_to_recoverable: false }, "quote"), /Tap to save/);
    assert.match(draw({ ...quote, from_me: true, reply_to_view_once: true }, "quote"), /No copy from this app/);

    const link = { ...base, preview_url: "https://example.invalid/path", preview_title: "Synthetic title",
      preview_desc: "Synthetic description", preview_thumb: "data:image/png;base64,AQ==", preview_color: "#123456" };
    assert.equal(normalize(draw(link, "link")), normalize(render(Embed, {
      props: { label: "example.invalid", title: link.preview_title, text: link.preview_desc,
        image: link.preview_thumb, color: link.preview_color, tooltip: link.preview_url, onopen() {} },
    }).body));
    const duplicate = draw({ ...link, preview_site: "  Example  ", preview_title: " Example ", preview_desc: link.preview_url }, "link");
    assert.doesNotMatch(duplicate, /embed-title|embed-text/);
    assert.equal(normalize(draw(base, "link")), "");

    for (const kind of ["contact", "location", "business", "album", "music", "ai_response", "future_card", "unknown"]) {
      const html = draw({ ...base, media_kind: kind, text: `Readable ${kind} <script> & details` });
      assert.match(html, new RegExp(`Readable ${kind}`));
      assert.match(html, /&lt;script(?:>|&gt;)/);
      assert.doesNotMatch(html, /<script>/);
      assert.match(html, /class="embed /);
    }

    const cases = [
      { kind: "poll", file: "PollCard", props: { poll: undefined, question: base.text, namer: preview.bubbleApi.namer,
        picture: preview.bubbleApi.avatarOf, onvote: async () => {} } },
      { kind: "event", file: "EventCard", props: { event: undefined, title: base.text, onopenurl() {}, onrespond: async () => {} } },
      { kind: "live_location", file: "LocationCard", props: {} },
    ];
    const live = { lat: 1, lng: 2, accuracy: 4, speed: null, heading: null, sequence: null,
      started_at: 0, updated_at: 0, expires_at: null, ended: true };
    for (const item of cases) {
      const message = { ...base, media_kind: item.kind, live_location: item.kind === "live_location" ? live : null };
      const { default: Component } = await load(`../lib/messages/cards/${item.file}.svelte`);
      const props = item.file === "LocationCard" ? { message, api: preview.bubbleApi } : item.props;
      assert.equal(normalize(draw(message)), normalize(render(Component, { props }).body));
      if (item.kind === "poll") {
        assert.equal(normalize(render(MessageCard, { props: { variant: "poll", ...props } }).body), normalize(render(Component, { props }).body));
      }
    }
    assert.match(draw({ ...base, media_kind: "group_invite" }, "link"), /Group invite|Synthetic body/);
  } finally {
    await server.close();
  }
});
