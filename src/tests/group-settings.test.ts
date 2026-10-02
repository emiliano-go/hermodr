import assert from "node:assert/strict";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { createServer } from "vite";
import { saveGroupMetadata } from "../lib/chat/group-settings-actions.ts";
import type { GroupSettings } from "../lib/utils/wire.ts";

const snapshot: GroupSettings = { subject: "Name", description: "Description", description_id: "token",
  announce: false, locked: false, approval: false, member: true, admin: true,
  can_edit_info: true, can_edit_picture: true, community: false };

test("group metadata keeps the acknowledged result when the later refresh fails", async () => {
  const calls: string[] = [];
  let saved = false;
  const result = await saveGroupMetadata(async () => { calls.push("write"); }, async () => {
    calls.push("read"); assert.equal(saved, true); throw new Error("offline");
  }, () => true, () => { saved = true; calls.push("ack"); });
  assert.deepEqual(calls, ["write", "ack", "read"]);
  assert.equal(saved, true);
  assert.equal(result.snapshot, null);
  assert.match(result.refreshError!, /offline/);
  let reloaded = false;
  await assert.rejects(saveGroupMetadata(async () => { throw new Error("not an admin"); }, async () => {
    reloaded = true; return snapshot;
  }, () => true, () => { throw new Error("a refused write must not claim success"); }), /not an admin/);
  assert.equal(reloaded, false);
});

test("group metadata drops stale account results before acknowledgement or refresh", async () => {
  let active = true;
  let release!: () => void;
  let acknowledged = false;
  let reloaded = false;
  const pending = saveGroupMetadata(() => new Promise<void>((resolve) => { release = resolve; }), async () => {
    reloaded = true; return snapshot;
  }, () => active, () => { acknowledged = true; });
  active = false; release();
  assert.deepEqual(await pending, { snapshot: null, refreshError: null });
  assert.equal(acknowledged, false);
  assert.equal(reloaded, false);
  let invoked = false;
  await saveGroupMetadata(async () => { invoked = true; }, async () => snapshot, () => false, () => {});
  assert.equal(invoked, false);
  active = true;
  const afterRefresh = await saveGroupMetadata(async () => {}, async () => { active = false; return snapshot; },
    () => active, () => { acknowledged = true; });
  assert.equal(acknowledged, true);
  assert.equal(afterRefresh.snapshot, null);
});

test("group metadata SSR stays read-only until fresh settings arrive and never performs a protocol action", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/group-settings", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { default: GroupSettings } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/GroupSettings.svelte", import.meta.url)));
    const { render } = await server.ssrLoadModule("svelte/server");
    const refuse = async () => { throw new Error("SSR must not access an account or change a group"); };
    const { body } = render(GroupSettings, { props: { chat: "123@g.us", account: "synthetic", onload: refuse, onchange: refuse, onpicture: refuse } });
    assert.match(body, /Loading group settings/);
    assert.doesNotMatch(body, /Save name|Save description|Remove picture|saved\.|type="checkbox"/);
  } finally { await server.close(); }
});
