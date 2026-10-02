import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("keyword finder merges reachable hits, preserves physical ping semantics and rejects stale scopes", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/keyword-finder", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL("../lib", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  const previousStorage = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  const data = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", { configurable: true, value: {
    getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value); },
  } });
  try {
    const load = (path: string) => server.ssrLoadModule(fileURLToPath(new URL(path, import.meta.url)));
    const { openPings } = await load("../lib/state/finder.ts");
    const { session } = await load("../lib/state/session.svelte.ts");
    const { keywords } = await load("../lib/state/keywords.svelte.ts");
    const { messages } = await load("../lib/state/messages.svelte.ts");
    const { ui } = await load("../lib/state/ui.svelte.ts");
    const { setHandler } = await load("../../tests/scheduled/ipc.ts");
    const row = (chat: string, id: string, text = "Needle", timestamp = 1) => ({ chat, id, text, timestamp, sort_order: 0,
      sender: "12025550000@s.whatsapp.net", sender_name: "Synthetic", from_me: false, read: false, mentioned: true,
      deleted: false, revoked: false, spoiler: false, system_kind: null, media_kind: null, media_once_kind: null });
    session.activeAccount = "account-a"; session.started = false; keywords.load("account-a");
    let commands: string[] = [];
    setHandler((command: string) => { commands.push(command); assert.equal(command, "pings");
      return [{ ...row("scope@g.us", "spoiler"), spoiler: true }, row("scope@g.us", "ordinary")]; });
    await openPings("scope@g.us");
    assert.deepEqual(commands, ["pings"]); assert.equal(ui.finder.items.length, 2);
    assert.ok(ui.finder.items.some((item: { id: string }) => item.id === "spoiler"));
    keywords.save("account-a", { highlight: ["needle"], hide: ["secret"] }); commands = [];
    setHandler((command: string, args: Record<string, unknown>) => {
      commands.push(command);
      if (command === "pings") return [row("scope@g.us", "duplicate", "Needle", 1), row("scope@g.us", "hidden", "secret", 5),
        row("scope@g.us", "same", "Needle", 4)];
      assert.equal(command, "keyword_matches");
      assert.deepEqual(args, { accountId: "account-a", chat: null, unreadOnly: false, highlight: ["needle"], hide: ["secret"] });
      return [row("scope@g.us", "duplicate", "New needle", 2), row("other@g.us", "same", "Needle", 3)];
    });
    await openPings(null); assert.deepEqual(commands, ["pings", "keyword_matches"]);
    assert.deepEqual(ui.finder.items.map((item: { chat: string; id: string }) => [item.chat, item.id]),
      [["scope@g.us", "same"], ["other@g.us", "same"], ["scope@g.us", "duplicate"]]);
    setHandler((command: string) => command === "pings" ? [] : Array.from({ length: 510 }, (_, index) => row("scope@g.us", String(index), "Needle", index)));
    await openPings("scope@g.us"); assert.equal(ui.finder.items.length, 500); assert.equal(ui.finder.items[0].id, "509");
    let release!: (rows: unknown[]) => void;
    setHandler((command: string) => command === "pings" ? [] : new Promise((resolve) => { release = resolve; }));
    const oldAccount = openPings("scope@g.us"); session.activeAccount = "account-b"; keywords.load("account-b"); messages.resetAccount();
    setHandler(() => [row("scope@g.us", "current-account")]); await openPings("scope@g.us");
    release([row("scope@g.us", "old-account")]); await oldAccount; assert.equal(ui.finder.items[0].id, "current-account");
    keywords.save("account-b", { highlight: ["needle"], hide: [] });
    setHandler((command: string) => command === "pings" ? [] : new Promise((resolve) => { release = resolve; }));
    const oldRules = openPings("scope@g.us"); keywords.save("account-b", { highlight: ["new"], hide: [] });
    release([row("scope@g.us", "old-rules")]); await oldRules; assert.equal(ui.finder.items, null);
    const closed = openPings("scope@g.us"); ui.finder = null; release([row("scope@g.us", "closed")]); await closed; assert.equal(ui.finder, null);
    const pending: ((rows: unknown[]) => void)[] = [];
    setHandler((command: string) => command === "pings" ? [] : new Promise((resolve) => { pending.push(resolve); }));
    const earlier = openPings("scope@g.us"), latest = openPings("scope@g.us");
    pending[1]([row("scope@g.us", "latest")]); await latest; pending[0]([row("scope@g.us", "earlier")]); await earlier;
    assert.equal(ui.finder.items[0].id, "latest");
  } finally {
    if (previousStorage) Object.defineProperty(globalThis, "localStorage", previousStorage);
    else Reflect.deleteProperty(globalThis, "localStorage");
    await server.close();
  }
});
