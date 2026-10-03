import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("composer restores account-scoped drafts and retains text when storage fails", async () => {
  const items = new Map<string, string>();
  let fail = false;
  const previous = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => { if (fail) throw new Error("Synthetic storage quota"); items.set(key, value); },
    removeItem: (key: string) => { items.delete(key); },
  } });
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/drafts-state", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { ComposerState } = await load("../lib/state/composer.svelte.ts");
    const { chats } = await load("../lib/state/chats.svelte.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    chats.selectedChat = "draft-chat@s";
    session.activeAccount = "draft-a";
    const first = new ComposerState();
    first.draft = "Account A draft";
    session.activeAccount = "draft-b";
    first.resetAccount();
    assert.equal(first.draftFor(session.activeAccount, chats.selectedChat), "");
    first.draft = "Account B draft";
    const restored = new ComposerState();
    assert.equal(restored.draftFor("draft-a", chats.selectedChat), "Account A draft");
    assert.equal(restored.draftFor("draft-b", chats.selectedChat), "Account B draft");
    restored.draft = "Account B draft";
    restored.clearDraft(chats.selectedChat, "draft-a");
    assert.equal(restored.draft, "Account B draft", "late clear cannot erase the current account draft");
    assert.equal(new ComposerState().draftFor("draft-a", chats.selectedChat), "");
    fail = true;
    restored.draft = "Retained despite quota";
    assert.equal(restored.draftFor("draft-b", chats.selectedChat), "Retained despite quota");
    assert.match(ui.error?.diagnostic ?? "", /Synthetic storage quota/);
    restored.resetAccount();
  } finally {
    await server.close();
    if (previous) Object.defineProperty(globalThis, "localStorage", previous); else Reflect.deleteProperty(globalThis, "localStorage");
  }
});
