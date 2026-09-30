import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";

test("group invite controls stay disabled until a link is returned and never claim a reset during SSR", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/group-invites", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: GroupInviteLinks } = await server.ssrLoadModule(fileURLToPath(new URL("./chat/GroupInviteLinks.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const { body } = render(GroupInviteLinks, { props: { chat: "123@g.us", canReset: true,
      async onload() { throw new Error("SSR must not query a real group"); },
      async onreset() { throw new Error("SSR must not reset a real invite"); } } });
    assert.match(body, /Loading invite link/);
    assert.match(body, /<button[^>]*disabled[^>]*>Copy link/);
    assert.match(body, /<button[^>]*disabled[^>]*>Reset link/);
    assert.doesNotMatch(body, /Invite link reset|Invite link copied|role="dialog"/);
    const readonly = render(GroupInviteLinks, { props: { chat: "123@g.us",
      async onload() { throw new Error("SSR must not query a real group"); },
      async onreset() { throw new Error("readonly users must not reset invites"); } } }).body;
    assert.doesNotMatch(readonly, /Reset link/);
  } finally { await server.close(); }
});
