import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import type { StoredMessage } from "./utils/models.ts";

test("passive preview preserves full text while concealing deleted, spoiler and one-time contents", async () => {
  const server = await createServer({
    configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/chat-preview", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } },
    server: { middlewareMode: true, ws: false, watch: null },
  });
  try {
    const { previewContent } = await server.ssrLoadModule(fileURLToPath(new URL("./chat/ChatPreview.svelte", import.meta.url)));
    const message = Object.freeze({
      text: `*Readable* ${"long message ".repeat(40)}https://example.invalid`,
      media_kind: null, media_path: "unopened-local-file", media_thumb: "unopened-thumbnail",
      deleted: false, revoked: false, spoiler: false, system_kind: null,
    }) as StoredMessage;
    const before = JSON.stringify(message);
    const content = previewContent(message);
    assert.ok(content.text.length > 160);
    assert.ok(content.text.startsWith("Readable "));
    assert.ok(content.text.endsWith("https://example.invalid"));
    assert.equal(content.media, null);
    assert.equal(JSON.stringify(message), before);
    for (const hidden of [{ deleted: true }, { revoked: true }, { spoiler: true }, { media_kind: "view_once" }, { system_kind: "NOTICE" }]) {
      const result = previewContent({ ...message, ...hidden, text: "PRIVATE CONTENT", media_path: "PRIVATE FILE" });
      assert.ok(!JSON.stringify(result).includes("PRIVATE"));
    }
    assert.deepEqual(previewContent({ ...message, media_kind: "image", text: "[image]" }), { text: "", media: "Photo", notice: false });
    assert.deepEqual(previewContent({ ...message, media_kind: "audio", text: "*Voice caption*" }), { text: "Voice caption", media: "Audio", notice: false });
    assert.deepEqual(previewContent({ ...message, media_kind: "poll", text: "Question" }), { text: "Question", media: "Poll", notice: false });
  } finally {
    await server.close();
  }
});
