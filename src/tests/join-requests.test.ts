import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";
import { joinRequestResults } from "../lib/utils/join-requests.ts";

test("join request bulk results complete only acknowledged requested successes", () => {
  const changes = [
    { jid: "approved", ok: true, pending: false, code: "200", error: null },
    { jid: "refused", ok: false, pending: false, code: "403", error: "not-admin" },
    { jid: "pending", ok: true, pending: true, code: "200", error: null },
    { jid: "unrequested", ok: true, pending: false, code: "200", error: null },
  ];
  const results = joinRequestResults(["approved", "refused", "pending", "missing"], changes, true);
  assert.deepEqual(results.filter((row) => row.completed).map((row) => row.jid), ["approved"]);
  assert.deepEqual(results.filter((row) => !row.completed).map((row) => row.jid), ["refused", "pending", "missing"]);
  assert.equal(results[0].text, "Approved.");
  assert.match(results[1].text, /Not allowed/);
  assert.equal(results[2].text, "Sent for approval.");
  assert.match(results[3].text, /did not return/);
  assert.equal(joinRequestResults(["approved"], changes, false)[0].text, "Denied.");
});

test("the group request widget renders without claiming an unloaded queue is empty", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/join-requests", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: GroupRequests } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/GroupRequests.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const { body } = render(GroupRequests, { props: { chat: "1@g.us",
      async onload() { throw new Error("SSR must not read a real queue"); },
      async onchange() { throw new Error("SSR must not mutate requests"); } } });
    assert.match(body, /Join requests/);
    assert.match(body, /Loading join requests/);
    assert.doesNotMatch(body, /No pending join requests/);
  } finally { await server.close(); }
});
