import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath } from "node:url";

test("keyword state rejects stale account/rule/request/generation counts and keeps failed saves unapplied", async () => {
  const server = await createServer({ configFile: false, plugins: [svelte({ configFile: false })],
    root: fileURLToPath(new URL("../../tests/browser", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/keywords", import.meta.url)),
    resolve: { alias: [
      { find: "$lib/utils/ipc", replacement: fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)) },
      { find: "$lib", replacement: fileURLToPath(new URL(".", import.meta.url)) },
    ] }, ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { KeywordsState } = await server.ssrLoadModule(fileURLToPath(new URL("./state/keywords.svelte.ts", import.meta.url)));
    const { setHandler } = await server.ssrLoadModule(fileURLToPath(new URL("../../tests/scheduled/ipc.ts", import.meta.url)));
    const data = new Map<string, string>(); let failSave = false, external = 0, calls = 0;
    const storage = { getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => {
      if (failSave) throw new Error("quota"); data.set(key, value);
    } };
    const state = new KeywordsState(storage);
    setHandler(() => { ++calls; throw new Error("unexpected empty-rules query"); });
    state.load("account-a"); await state.refreshCounts("account-a", () => external); assert.equal(calls, 0);
    assert.equal(state.save("account-a", { highlight: ["Needle"], hide: ["Secret"] }), true);
    let release!: (counts: Record<string, number>) => void;
    setHandler((command: string, args: Record<string, unknown>) => {
      assert.equal(command, "keyword_mentions"); assert.deepEqual(args, { accountId: "account-a", highlight: ["Needle"], hide: ["Secret"] });
      return new Promise((resolve) => { release = resolve; });
    });
    const oldAccount = state.refreshCounts("account-a", () => external);
    state.load("account-b"); release({ old: 99 }); await oldAccount; assert.deepEqual(state.counts, {});
    state.save("account-b", { highlight: ["Second"], hide: [] });
    setHandler(() => ({ current: 2 })); await state.refreshCounts("account-b", () => external); assert.deepEqual(state.counts, { current: 2 });
    setHandler(() => new Promise((resolve) => { release = resolve; }));
    const oldRules = state.refreshCounts("account-b", () => external);
    state.save("account-b", { highlight: ["New rule"], hide: [] }); release({ obsolete: 7 }); await oldRules; assert.deepEqual(state.counts, {});
    const pending: ((counts: Record<string, number>) => void)[] = [];
    setHandler(() => new Promise((resolve) => { pending.push(resolve); }));
    const first = state.refreshCounts("account-b", () => external), last = state.refreshCounts("account-b", () => external);
    pending[1]({ latest: 3 }); await last; pending[0]({ earlier: 8 }); await first; assert.deepEqual(state.counts, { latest: 3 });
    setHandler(() => new Promise((resolve) => { release = resolve; }));
    const oldGeneration = state.refreshCounts("account-b", () => external); ++external;
    release({ stale: 9 }); await oldGeneration; assert.deepEqual(state.counts, { latest: 3 });
    failSave = true;
    assert.equal(state.save("account-b", { highlight: ["Unpersisted"], hide: [] }), false);
    assert.deepEqual(state.rules, { highlight: ["New rule"], hide: [] }); assert.match(state.error, /quota/);
    assert.equal(state.save("account-a", { highlight: ["Wrong account"], hide: [] }), false);
    failSave = false;
    const restarted = new KeywordsState(storage); restarted.load("account-b");
    assert.deepEqual(restarted.rules, { highlight: ["New rule"], hide: [] });
    setHandler(() => { throw new Error("unavailable count"); });
    await state.refreshCounts("account-b", () => external); assert.deepEqual(state.counts, {}); assert.match(state.countError, /unavailable count/);
    state.load(null); assert.deepEqual(state.rules, { highlight: [], hide: [] }); assert.deepEqual(state.counts, {});
  } finally { await server.close(); }
});
