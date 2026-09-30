import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";
import type { ContactIdentity } from "./utils/wire";

test("stored identity overrides stale labels in all name paths and ignores old-account lookups", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/contact-identity", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL(".", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { members } = await load("./state/members.svelte.ts");
    const { session } = await load("./state/session.svelte.ts");
    const { chats } = await load("./state/chats.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const { plain } = await load("./utils/format.ts");
    const saved = { saved_name: "Saved", push_name: "Push", username: "username", number: "59891954564", own: false };
    let current: ContactIdentity = saved;
    let reached!: () => void;
    let delay: Promise<void> | null = null;
    setHandler(async (command: string, args?: Record<string, unknown>) => {
      assert.equal(command, "contact_identities");
      const snapshot = { ...current };
      reached?.();
      if (delay) await delay;
      return Object.fromEntries((args?.jids as string[]).map((jid) => [jid, snapshot]));
    });
    session.started = true; session.connected = false; session.activeAccount = "account-a";
    members.resetAccount();
    const settle = async (jid: string) => {
      const requested = new Promise<void>((resolve) => { reached = resolve; });
      members.displayName("stale", jid);
      await requested;
      await new Promise((resolve) => setTimeout(resolve, 0));
    };
    await settle("77@lid");
    assert.equal(members.displayName("stale", "77@lid"), "Saved");
    assert.equal(members.senderLabel({ sender: "77@lid", sender_name: "stale" }), "Saved");
    assert.equal(members.senderName("77@lid"), "Saved");
    assert.equal(members.mentionTarget("77").name, "Saved");
    assert.equal(chats.chatLabel({ chat: "77@lid", display_name: "stale" }), "Saved");

    current = { ...saved, saved_name: null };
    members.forgetUnresolvedNames(); await settle("77@lid");
    const unsaved = members.displayName("Saved stale", "77@lid");
    assert.ok(unsaved.includes("+598 91954564")); assert.ok(unsaved.endsWith("· ~Push"));
    assert.equal(members.senderLabel({ sender: "77@lid", sender_name: "Saved stale" }), unsaved);
    current = { ...saved, saved_name: null, number: null, username: "only_username" };
    members.forgetUnresolvedNames(); await settle("88000000000000@lid");
    assert.equal(members.displayName("Push", "88000000000000@lid"), "@only_username");
    assert.equal(members.mentionTarget("88000000000000").name, "only_username");
    assert.equal(plain("Hello @88000000000000", (user: string) => members.mentionName(user)), "Hello @only_username");
    assert.equal(members.mentionTarget("138947158093828").name, "138947158093828");

    members.forgetUnresolvedNames();
    current = saved;
    let release!: () => void;
    delay = new Promise<void>((resolve) => { release = resolve; });
    const pending = new Promise<void>((resolve) => { reached = resolve; });
    members.displayName(null, "77@lid"); await pending;
    members.resetAccount(); session.activeAccount = "account-b";
    release(); await new Promise((resolve) => setTimeout(resolve, 0));
    assert.deepEqual(members.identities, {}); assert.deepEqual(members.learnedNames, {});
    delay = null; current = { ...saved, saved_name: "Other account" };
    await settle("77@lid");
    assert.equal(members.displayName("Saved stale", "77@lid"), "Other account");
    members.resetAccount();
  } finally { await server.close(); }
});
