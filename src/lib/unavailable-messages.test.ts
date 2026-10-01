import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("unavailable rows keep identity, sender and time without content actions, then render recovered content", async () => {
  const server = await createServer({
    configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/unavailable", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false, watch: null },
  });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const [{ default: MessageList }, { listProps, unavailable }, { render }] = await Promise.all([
      load("./messages/MessageList.svelte"), load("../../tests/browser/Unavailable.svelte"), server.ssrLoadModule("svelte/server"),
    ]);
    const html = render(MessageList, { props: { ...listProps, messages: [unavailable], firstUnreadId: unavailable.id } }).body;
    assert.match(html, /data-id="unavailable-id"/);
    assert.match(html, /data-chat="unavailable@s.whatsapp.net"/);
    assert.match(html, /Saved sender/);
    assert.match(html, /datetime="2023-11-14T22:13:20.000Z"/);
    assert.match(html, /Message unavailable/);
    assert.match(html, /cannot request it again from your phone/);
    assert.doesNotMatch(html, /PRIVATE|class="[^"]*bubble|<img|<audio|<video|Retry|React |Forward|Reply|Unread messages/);
    const recovered = render(MessageList, { props: { ...listProps, messages: [{ ...unavailable, system_kind: null, text: "Recovered synthetic message" }] } }).body;
    assert.match(recovered, /Recovered synthetic message/);
    assert.match(recovered, /data-id="unavailable-id"/);
    assert.doesNotMatch(recovered, /Message unavailable|cannot request it again/);
  } finally { await server.close(); }
});
