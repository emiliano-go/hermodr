import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { stickerResyncText, stickerScopeMatches } from "../lib/utils/sticker-sync.ts";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";
import { englishCatalog, installLocaleProvider, loadCatalog, t, type Catalog } from "../lib/i18n/localizer.ts";
import type { StickerResyncReport } from "../lib/utils/wire";

function functions(context: Record<string, any>, file = "../lib/media/StickerSync.svelte") {
  Object.assign(context, { LocalizedError, normalizeError, t });
  const source = readFileSync(new URL(file, import.meta.url), "utf8").match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("StickerSync.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((item) => item.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}

function expectFailure(value: unknown, detail: RegExp) {
  assert.ok(value instanceof LocalizedError);
  assert.equal(value.code, "error.operation_failed");
  assert.equal(value.message, t(value.code, value.params));
  assert.match(value.diagnostic ?? "", detail);
}

function picker(overrides: Record<string, any> = {}) {
  return functions({ session: { activeAccount: "alpha", connected: true }, messages: { accountGeneration: 1 }, chat: "chat", tab: "sticker",
    stickerScopeMatches, packRequest: 0, packError: "", fetching: {}, fetchingAll: {}, favourites: [], FAV_KEY: "synthetic-favorites",
    localStorage: { setItem: () => {} }, stickerEvents: { touch: () => {} }, sleep: async () => {}, FETCH_GAP_MS: 1,
    RateLimited: class extends Error {}, onerror: () => assert.fail("Failure must stay in captured picker"), ...overrides }, "../lib/composer/ExpressionPicker.svelte");
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
  expectFailure(context.error, /offline cache read failed/);
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
  expectFailure(context.error, /snapshot unavailable/);
  assert.equal(context.report, null);
  context.connected = false;
  await context.resync();
  assert.equal(starts, 1);
  context.connected = true;
  context.onresync = async () => reported;
  context.onload = async () => { throw new Error("cache refresh failed"); };
  await context.resync();
  assert.equal(context.library, cached);
  expectFailure(context.error, /cache refresh failed/);
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

test("sticker report follows live locale, formats every count and preserves all phase and partial flags", async () => {
  const arabic = await loadCatalog("ar");
  let snapshot: { locale: string; catalog: Catalog } = { locale: "en", catalog: englishCatalog };
  const restore = installLocaleProvider(() => snapshot);
  const full: StickerResyncReport = { ...reported, mirror_verified: true, catalog_complete: true, pack_failures: [], skipped_stickers: 0 };
  try {
    const before = JSON.stringify(full);
    for (const language of ["en", "ar"]) {
      snapshot = { locale: language, catalog: language === "ar" ? arabic : englishCatalog };
      const complete = stickerResyncText(full);
      assert.ok(complete.startsWith(t("sticker_sync.phase_returned")));
      assert.ok(!complete.includes(t("sticker_sync.partial")));
      for (const [flags, phase] of [
        [{ app_state_fatal: true, app_state_retryable: true }, "sticker_sync.phase_failed"],
        [{ app_state_retryable: true }, "sticker_sync.phase_retry"],
        [{ app_state_synced: false }, "sticker_sync.phase_unconfirmed"],
      ] as const) {
        const text = stickerResyncText({ ...full, ...flags });
        assert.ok(text.startsWith(`${t("sticker_sync.partial")} ${t(phase)}`));
      }
      for (const flags of [{ mirror_verified: false }, { catalog_complete: false }, { app_state_error: "synthetic" },
        { pack_failures: reported.pack_failures }, { skipped_stickers: 1 }])
        assert.ok(stickerResyncText({ ...full, ...flags }).startsWith(`${t("sticker_sync.partial")} ${t("sticker_sync.phase_returned")}`));
      const counts = [1234, 2345, 3456, 4567, 5678, 6789];
      const text = stickerResyncText({ ...full, packs: counts[0], known_packs: counts[1], stickers: counts[2],
        packs_changed: counts[3], stickers_changed: counts[4], skipped_stickers: counts[5] });
      for (const count of counts) assert.ok(text.includes(new Intl.NumberFormat(language).format(count)));
      const missing = stickerResyncText({ packs: 1, stickers: 3 } as StickerResyncReport);
      assert.ok(missing.startsWith(`${t("sticker_sync.partial")} ${t("sticker_sync.phase_unconfirmed")}`));
      assert.equal(missing.split(t("sticker_sync.unknown")).length - 1, 4);
    }
    assert.notEqual(stickerResyncText(full), completeEnglish(full));
    assert.equal(JSON.stringify(full), before);
  } finally { restore(); }
});

function completeEnglish(report: StickerResyncReport) {
  const restore = installLocaleProvider(() => ({ locale: "en", catalog: englishCatalog }));
  try { return stickerResyncText(report); } finally { restore(); }
}

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
    } }, "../lib/composer/ExpressionPicker.svelte");
  await context.openPackView({ pack_id: "known" });
  assert.deepEqual(calls, ["sticker_pack", "fetch_sticker_pack"]);
  assert.equal(context.openPack.stickers, stickers);
  expectFailure(context.packError, /Synthetic pack refresh failure/);
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
      } }, "../lib/composer/ExpressionPicker.svelte");
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
  expectFailure(context.packError, /Synthetic phone request failure/);
  await context.toggleSyncedFavorite({ filehash: "hash", favorite: false });
  expectFailure(context.packError, /Synthetic phone request failure/);
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
  expectFailure(context.packError, /Synthetic download failure/);
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
