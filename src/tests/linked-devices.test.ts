import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("linked device UI waits for account data and never invents names or dates", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/linked-devices", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: LinkedDevices } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/settings/LinkedDevices.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const callbacks = { async onload() { throw new Error("SSR must not read real devices"); },
      async onunlink() { throw new Error("SSR must not unlink devices"); } };
    const { body } = render(LinkedDevices, { props: { account: "synthetic", connected: true, ...callbacks } });
    assert.match(body, /Loading linked devices/);
    assert.match(body, /names and activity dates are unavailable/);
    assert.doesNotMatch(body, /No linked devices|Confirm logout|Last active/);
    const offline = render(LinkedDevices, { props: { account: "synthetic", connected: false, ...callbacks } }).body;
    assert.match(offline, /Connect this account/);
    assert.doesNotMatch(offline, /Refreshing|Loading linked devices/);
  } finally { await server.close(); }
});
