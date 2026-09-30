import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("blocked contacts UI waits for server data and requires queued mutation callback", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/blocked-contacts", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: BlockedContacts } = await server.ssrLoadModule(fileURLToPath(new URL("./settings/BlockedContacts.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const callbacks = { async onload() { throw new Error("SSR must not fetch blocked contacts"); },
      async onunblock() { throw new Error("SSR must not unblock contacts"); } };
    const { body } = render(BlockedContacts, { props: { account: "synthetic", connected: true, ...callbacks } });
    assert.match(body, /Loading blocked contacts/);
    assert.doesNotMatch(body, /No blocked contacts|Unblocking/);
    const offline = render(BlockedContacts, { props: { account: "synthetic", connected: false, ...callbacks } }).body;
    assert.match(offline, /Connect this account/);
    assert.doesNotMatch(offline, /Loading blocked contacts|<button/);
  } finally { await server.close(); }
});
