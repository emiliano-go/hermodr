import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { stickerResyncText, stickerScopeMatches } from "./utils/sticker-sync.ts";
import type { StickerResyncReport } from "./utils/wire";

function functions(context: Record<string, any>, file = "./media/StickerSync.svelte") {
  const source = readFileSync(new URL(file, import.meta.url), "utf8").match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("StickerSync.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((item) => item.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

function picker(overrides: Record<string, any> = {}) {
  return functions({ session: { activeAccount: "alpha", connected: true }, messages: { accountGeneration: 1 }, chat: "chat", tab: "sticker",
    stickerScopeMatches, packRequest: 0, packError: "", fetching: {}, fetchingAll: {}, favourites: [], FAV_KEY: "synthetic-favorites",
    localStorage: { setItem: () => {} }, stickerEvents: { touch: () => {} }, sleep: async () => {}, FETCH_GAP_MS: 1,
    RateLimited: class extends Error {}, onerror: () => assert.fail("Failure must stay in captured picker"), ...overrides }, "./composer/ExpressionPicker.svelte");
}

const cached = { packs: [{ pack_id: "known", name: "Known shared pack" }], favorites: [], recent: [] };
const reported: StickerResyncReport = { packs: 1, stickers: 3, known_packs: 2, packs_changed: 1, stickers_changed: 2, skipped_stickers: 1,
  app_state_synced: true, app_state_retryable: false, app_state_fatal: false, app_state_error: null,
  pack_failures: [{ pack_id: "failed", error: "Synthetic pack failure" }], mirror_verified: false, catalog_complete: false };
const base = () => ({ account: "alpha", chat: "chat", generation: 1, connected: true, epoch: 1, request: 0,
  loading: false, busy: false, error: "", report: null, library: cached, stickerScopeMatches, onlibrary: undefined, onsynced: undefined });

test("sticker resync rejects stale account, chat, generation and service epoch results", async () => {
  for (const field of ["account", "chat", "generation", "epoch"] as const) {
    let release!: (value: StickerResyncReport) => void;
    let callbacks = 0;
    const context = functions({ ...base(), onresync: () => new Promise((yes) => { release = yes; }),
      onload: async () => { callbacks++; return cached; }, onlibrary: () => { callbacks++; }, onsynced: () => { callbacks++; } });
    const pending = context.resync();
    assert.equal(context.busy, true);
    context[field] = field === "account" || field === "chat" ? "next" : 2;
    release(reported);
    await pending;
    assert.equal(callbacks, 0);
    assert.equal(context.report, null);
    assert.equal(context.error, "");
  }
});

test("failed library refresh preserves cached packs and rejects stale reads", async () => {
  const context = functions({ ...base(), onload: async () => { throw new Error("offline cache read failed"); } });
  await context.refresh();
  assert.equal(context.library, cached);
  assert.match(context.error, /offline cache read failed/);
  assert.equal(context.loading, false);
  let release!: (value: typeof cached) => void;
  context.onload = () => new Promise((yes) => { release = yes; });
  const pending = context.refresh();
  context.generation++;
  release({ ...cached, packs: [] });
  await pending;
  assert.equal(context.library, cached);
});

test("resync failure and post-resync cache failure stay visible without clearing cached packs", async () => {
  let starts = 0;
  const context = functions({ ...base(), onresync: async () => { starts++; throw new Error("snapshot unavailable"); }, onload: async () => cached });
  await context.resync();
  assert.equal(starts, 1);
  assert.equal(context.library, cached);
  assert.match(context.error, /snapshot unavailable/);
  assert.equal(context.report, null);
  context.connected = false;
  await context.resync();
  assert.equal(starts, 1);
  context.connected = true;
  context.onresync = async () => reported;
  context.onload = async () => { throw new Error("cache refresh failed"); };
  await context.resync();
  assert.equal(context.library, cached);
  assert.match(context.error, /cache refresh failed/);
  assert.deepEqual(context.report, reported);
  assert.equal(context.busy, false);
  assert.match(stickerResyncText(reported), /Partial resync result\. Snapshot request returned\. Refetched 1\/2 known packs/);
});

test("partial report describes request phase and real counts without claiming mirrored state", () => {
  const text = stickerResyncText(reported);
  assert.match(text, /changed 1 packs\/2 stickers; skipped 1 stickers/);
  assert.doesNotMatch(text, /\bsynced\b|\bcomplete\b/i);
  assert.match(stickerResyncText({ ...reported, app_state_retryable: true }), /needs retry/);
  assert.match(stickerResyncText({ ...reported, app_state_fatal: true }), /request failed/);
  assert.match(stickerResyncText({ packs: 1, stickers: 3 } as StickerResyncReport), /unconfirmed.*1\/unknown.*changed unknown/);
});

test("picker opens cached pack before refresh and retains it when network refresh fails", async () => {
  const calls: string[] = [];
  const stickers = [{ filehash: "cached-sticker", path: "synthetic.webp" }];
  const context = functions({ session: { activeAccount: "alpha", connected: true }, messages: { accountGeneration: 1 }, chat: "chat", tab: "sticker",
    stickerScopeMatches, packRequest: 0, openPack: null, packLoading: false, packError: "", invoke: async (command: string, args: { accountId: string }) => {
      assert.equal(args.accountId, "alpha");
      calls.push(command);
      if (command === "sticker_pack") return stickers;
      assert.equal(context.openPack.stickers, stickers);
      throw new Error("Synthetic pack refresh failure");
    } }, "./composer/ExpressionPicker.svelte");
  await context.openPackView({ pack_id: "known" });
  assert.deepEqual(calls, ["sticker_pack", "fetch_sticker_pack"]);
  assert.equal(context.openPack.stickers, stickers);
  assert.match(context.packError, /Synthetic pack refresh failure/);
  assert.equal(context.packLoading, false);
});

test("picker never refreshes cached pack on another account, chat, generation or closed view", async () => {
  for (const change of ["account", "chat", "generation", "packRequest"] as const) {
    let release!: (value: unknown[]) => void;
    const calls: string[] = [];
    const context = functions({ session: { activeAccount: "alpha", connected: true }, messages: { accountGeneration: 1 }, chat: "chat", tab: "sticker",
      stickerScopeMatches, packRequest: 0, openPack: null, packLoading: false, packError: "", invoke: (command: string, args: { accountId: string }) => {
        assert.equal(args.accountId, "alpha");
        calls.push(command);
        return new Promise((yes) => { release = yes; });
      } }, "./composer/ExpressionPicker.svelte");
    const pending = context.openPackView({ pack_id: "known" });
    if (change === "account") context.session.activeAccount = "beta";
    else if (change === "generation") context.messages.accountGeneration++;
    else if (change === "chat") context.chat = "next";
    else context.packRequest++;
    release([{ filehash: "old" }]);
    await pending;
    assert.deepEqual(calls, ["sticker_pack"]);
    assert.equal(context.openPack.stickers.length, 0);
    assert.equal(context.packError, "");
  }
});

test("download uses captured account and never mutates stale sticker or clears new pending marker", async () => {
  for (const change of ["account", "chat", "generation", "packRequest"] as const) {
    let release!: (value: string) => void;
    const context = picker({ invoke: (command: string, args: { accountId: string }) => {
      assert.equal(command, "download_sticker");
      assert.equal(args.accountId, "alpha");
      return new Promise((yes) => { release = yes; });
    } });
    const sticker = { filehash: "same", path: null };
    const pending = context.fetchSticker(sticker);
    if (change === "account") context.session.activeAccount = "beta";
    else if (change === "chat") context.chat = "next";
    else if (change === "generation") context.messages.accountGeneration++;
    else context.packRequest++;
    context.fetching = { same: true };
    release("old-scope.webp");
    assert.equal(await pending, null);
    assert.equal(sticker.path, null);
    assert.equal(context.fetching.same, true);
    assert.equal(context.packError, "");
  }
});

test("favorite failures remain visible and preserve local cached favorite", async () => {
  let touched = 0;
  const context = picker({ stickerEvents: { touch: () => touched++ }, invoke: async (command: string, args: { accountId: string }) => {
    assert.ok(["favorite_sticker_path", "favorite_sticker"].includes(command));
    assert.equal(args.accountId, "alpha");
    throw new Error("Synthetic phone request failure");
  } });
  await context.toggleFavourite("cached.webp");
  assert.equal(context.favourites[0], "cached.webp");
  assert.match(context.packError, /local favorite kept.*Synthetic phone request failure/);
  await context.toggleSyncedFavorite({ filehash: "hash", favorite: false });
  assert.match(context.packError, /Favorite request failed.*Synthetic phone request failure/);
  assert.equal(touched, 2);
});

test("stale favorite errors never notify new account or refresh its library", async () => {
  for (const action of ["toggleFavourite", "toggleSyncedFavorite"]) {
    let reject!: (error: Error) => void;
    let touched = 0;
    const context = picker({ stickerEvents: { touch: () => touched++ }, invoke: () => new Promise((_, no) => { reject = no; }) });
    const pending = context[action](action === "toggleFavourite" ? "cached.webp" : { filehash: "hash", favorite: false });
    context.session.activeAccount = "beta";
    reject(new Error("Old account failure"));
    await pending;
    assert.equal(context.packError, "");
    assert.equal(touched, 0);
  }
});

test("bulk downloads stop on cache failure and on scope change during rate-limit cooldown", async () => {
  let calls = 0;
  const context = picker({ invoke: async () => { calls++; throw new Error("Synthetic download failure"); } });
  const stickers = [{ filehash: "one", path: null }, { filehash: "two", path: null }];
  await context.fetchAll("pack", stickers);
  assert.equal(calls, 1);
  assert.match(context.packError, /Synthetic download failure/);
  let release!: () => void;
  context.invoke = async () => { calls++; throw new Error("429 rate limit"); };
  context.sleep = () => new Promise<void>((yes) => { release = yes; });
  const pending = context.fetchAll("pack", stickers);
  await new Promise<void>((yes) => setImmediate(yes));
  assert.equal(calls, 2);
  context.chat = "next";
  release();
  await pending;
  assert.equal(calls, 2);
  assert.equal(context.fetchingAll.pack, undefined);
});

test("saved image encoding cannot start sticker write after owner changes", async () => {
  let release!: (value: string) => void;
  let writes = 0;
  const context = picker({ making: {}, toBase64: () => new Promise((yes) => { release = yes; }), invoke: () => { writes++; } });
  const pending = context.saveMade({});
  context.messages.accountGeneration++;
  release("synthetic");
  await pending;
  assert.equal(writes, 0);
});
