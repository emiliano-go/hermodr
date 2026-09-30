import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

test("spoiler content stays out of rendered text, captions, quotes, links and notification previews", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/special-messages", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const [{ default: MessageBubble }, { render }, preview, helpers, notifications] = await Promise.all([
      load("./messages/MessageBubble.svelte"), server.ssrLoadModule("svelte/server"),
      load("./utils/theme-preview.ts"), load("./utils/message.ts"), load("./utils/notifications.ts"),
    ]);
    const secret = "Synthetic secret https://example.invalid/hidden @200";
    const message = { ...preview.messages[0], spoiler: true, text: secret, media_kind: "image",
      media_thumb: "data:image/jpeg;base64,AQ==", reply_to_text: "Private quoted content",
      preview_url: "https://example.invalid/secret-link", preview_title: "Private preview title" };
    const vm = { ...preview.bubbleView(message, 0, "chat"), visual: true, caption: secret, quoteText: message.reply_to_text };
    const api = { ...preview.bubbleApi, toWire() { throw new Error("hidden spoilers must not parse their content"); } };
    const bubble = render(MessageBubble, { props: { message, vm, api } }).body;
    assert.match(bubble, /Reveal spoiler/);
    assert.doesNotMatch(bubble, /Synthetic secret|example\.invalid|Private quoted content|Private preview title|data:image/);
    for (const flags of [{ revoked: true }, { deleted: true }]) {
      const retained = render(MessageBubble, { props: { message: { ...message, ...flags }, vm, api: preview.bubbleApi } }).body;
      assert.match(retained, /Reveal spoiler/);
      assert.doesNotMatch(retained, /Synthetic secret|example\.invalid|Private quoted content|Private preview title|data:image/);
    }
    assert.equal(helpers.captionOf(message), "[Spoiler]");
    assert.equal(notifications.notificationBody(message), "Spoiler message");
  } finally { await server.close(); }
});

test("round video uses the circular player and kept one-time copies stay filtered", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/special-messages", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: MessageBubble } = await server.ssrLoadModule(fileURLToPath(new URL("./messages/MessageBubble.svelte", import.meta.url)));
    const preview = await server.ssrLoadModule(fileURLToPath(new URL("./utils/theme-preview.ts", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const source = "data:video/mp4;base64,AA==";
    const message = { ...preview.messages[0], spoiler: false, text: "[round_video]", media_kind: "round_video", media_path: source };
    const vm = preview.bubbleView(message, 0, "chat");
    const opened = render(MessageBubble, { props: { message, vm, api: preview.bubbleApi } }).body;
    assert.match(opened, /class="[^"]*player[^"]*round/);
    assert.match(opened, /<video[^>]*src="data:video\/mp4;base64,AA=="/);
    assert.doesNotMatch(opened, / autoplay/);
    const kept = render(MessageBubble, { props: { message, vm: { ...vm, onceKept: true, onceRevealed: false }, api: preview.bubbleApi } }).body;
    assert.match(kept, /One-time round video/);
    assert.doesNotMatch(kept, /<video|data:video/);
  } finally { await server.close(); }
});

test("music artwork and unsupported content render through the existing readable cards", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/special-messages", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: MessageBubble } = await server.ssrLoadModule(fileURLToPath(new URL("./messages/MessageBubble.svelte", import.meta.url)));
    const preview = await server.ssrLoadModule(fileURLToPath(new URL("./utils/theme-preview.ts", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const base = { ...preview.messages[0], spoiler: false, media_kind: "music", text: "Synthetic track — Artist" };
    const draw = (message: object) => render(MessageBubble, { props: { message, vm: preview.bubbleView(message, 0, "chat"), api: preview.bubbleApi } }).body;
    assert.match(draw(base), /Synthetic track|Load artwork/);
    const artwork = draw({ ...base, media_thumb: "data:image/jpeg;base64,AQ==" });
    assert.match(artwork, /src="data:image\/jpeg;base64,AQ=="/);
    assert.doesNotMatch(artwork, /Load artwork/);
    const unknown = draw({ ...base, media_kind: "unknown", text: "[Unsupported message]" });
    assert.match(unknown, /Unsupported message/);
  } finally { await server.close(); }
});
