import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { createServer } from "vite";
import { FLOAT_HISTORY_LIMIT, floatContent, floatDraftKey, mergeFloatPage, readFloatDraft, writeFloatDraft } from "../lib/utils/float-chat.ts";
import { cursorOf } from "../lib/utils/message-window.ts";
import { keywordStorageKey, loadKeywordRules } from "../lib/utils/keywords.ts";
import type { StoredMessage } from "../lib/utils/wire.ts";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";

const context = { account_id: "a", chat: "actual@lid", title: "Synthetic chat", connected: true };
const row = (index: number, patch: Record<string, unknown> = {}): StoredMessage => ({ chat: context.chat, id: String(index), timestamp: index,
  sort_order: 0, text: `Message ${index}`, sender: "sender@lid", sender_name: "Stored name", from_me: false,
  spoiler: false, deleted: false, revoked: false, media_kind: null, media_once_kind: null, system_kind: null, ...patch } as unknown as StoredMessage);
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));
function storage() {
  const data = new Map<string, string>();
  return { data, getItem: (key: string) => data.get(key) ?? null, setItem: (key: string, value: string) => { data.set(key, value); }, removeItem: (key: string) => { data.delete(key); } };
}
function functions(path: string, c: Record<string, any>) {
  const source = readFileSync(new URL(path, import.meta.url), "utf8"), script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("component.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((node) => node.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText.replace(/^export /gm, ""), c);
  return c;
}
function route() {
  const calls: { command: string; args: any }[] = [], timers: (() => void)[] = [], channels: any[] = [];
  const store = storage();
  let handler: (command: string, args: any) => unknown = (command) => command === "float_context" ? { ...context }
    : command === "float_message_page" ? { messages: [], has_more: false } : undefined;
  const c: Record<string, any> = { context: { ...context }, rows: [], draft: "  Preserved draft  ", opacity: 0.85, loading: false,
    olderLoading: false, hasMore: true, sending: false, closing: false, draftReady: true, error: null, sendError: null, draftError: null,
    alive: true, epoch: 1, refreshRequest: 0, olderRequest: 0, draftVersion: 0, owner: JSON.stringify([context.account_id, context.chat]),
    subscribed: false, updates: null, refreshTimer: undefined, outbox: Promise.resolve(), view: null, localStorage: store,
    rules: { highlight: [], hide: [] }, rulesReady: false, keywordError: null, keywordStorageKey, loadKeywordRules,
    FLOAT_HISTORY_LIMIT, floatDraftKey, mergeFloatPage, readFloatDraft, writeFloatDraft, cursorOf,
    setTimeout: (run: () => void) => { timers.push(run); return timers.length; }, clearTimeout: () => {},
    Channel: class { onmessage?: () => void; constructor() { channels.push(this); } },
    LocalizedError, normalizeError,
    call: async (command: string, args: any) => { calls.push({ command, args }); return await handler(command, args); },
  };
  functions("../routes/float/+page.svelte", c);
  return { c, calls, store, timers, channels, handle: (next: typeof handler) => { handler = next; } };
}

test("floating content reuses readable labels and never reveals private, deleted or undecryptable text", () => {
  assert.deepEqual(floatContent(row(1, { text: "*Bold*", media_kind: "image" })), { text: "Bold", media: "Photo", notice: false });
  for (const patch of [{ spoiler: true }, { deleted: true }, { revoked: true }, { media_kind: "view_once" },
    { media_kind: "image", media_once_kind: "image" }, { system_kind: "UNAVAILABLE_MESSAGE" }, { system_kind: "NOTICE" }]) {
    assert.ok(!JSON.stringify(floatContent(row(1, { text: "PRIVATE CONTENT", media_path: "PRIVATE PATH", ...patch }))).includes("PRIVATE"));
  }
});

test("authoritative refresh replaces old history while older merges preserve current edits and cap 500", () => {
  const old = Array.from({ length: 120 }, (_, index) => row(index));
  const latest = old.slice(1).map((message) => message.id === "2" ? { ...message, revoked: true } : message).toReversed();
  const merged = mergeFloatPage(old, { messages: latest, has_more: true }, context.chat);
  assert.ok(!merged.some((message) => message.id === "0")); assert.equal(merged.find((message) => message.id === "2")?.revoked, true);
  assert.deepEqual(mergeFloatPage(old, { messages: [], has_more: true }, context.chat), []);
  const older = mergeFloatPage([row(2, { revoked: true })], { messages: [row(2), row(1), row(0, { chat: "other@lid" })], has_more: true }, context.chat, true);
  assert.equal(older.length, 2); assert.equal(older[1].revoked, true);
  assert.equal(mergeFloatPage([], { messages: Array.from({ length: 700 }, (_, index) => row(index)).toReversed(), has_more: true }, context.chat).length, 500);
});

test("float drafts stay separate by account/chat and storage failures propagate", () => {
  const store = storage(), other = { ...context, account_id: "b" };
  writeFloatDraft(store, context, "A draft"); writeFloatDraft(store, other, "B draft");
  assert.equal(readFloatDraft(store, context), "A draft"); assert.equal(readFloatDraft(store, other), "B draft");
  assert.notEqual(floatDraftKey(context), floatDraftKey(other));
  writeFloatDraft(store, context, ""); assert.equal(readFloatDraft(store, context), "");
  assert.throws(() => readFloatDraft({ getItem: () => { throw new Error("Denied"); } }, context), /Denied/);
});

test("offline edits persist locally while reply emits no native send", async () => {
  const f = route(); f.c.context.connected = false;
  f.c.changeDraft("Offline draft"); await f.c.send();
  assert.equal(readFloatDraft(f.store, context), "Offline draft"); assert.equal(f.calls.length, 0);
});

test("failed sends retain draft and quota failures prevent both send and close", async () => {
  const f = route(); f.handle(() => { throw new Error("Synthetic send failure"); }); await f.c.send();
  assert.equal(f.c.draft, "  Preserved draft  "); assert.equal(f.c.sendError.code, "error.operation_failed");
  assert.ok(f.c.sendError.diagnostic.includes("Synthetic send failure")); assert.equal(f.c.sending, false);
  const blocked = route(); blocked.c.localStorage.setItem = () => { throw new Error("Quota"); };
  await blocked.c.send(); assert.equal(blocked.c.draftError.code, "error.float_draft_save");
  await blocked.c.close();
  assert.equal(blocked.calls.length, 0); assert.equal(blocked.c.draftError.code, "error.float_close_draft_save");
  assert.ok(blocked.c.draftError.diagnostic.includes("Quota"));
  const startup = route(); startup.c.context = null; startup.c.draftReady = false; startup.c.draft = "";
  await startup.c.close(); assert.equal(startup.calls[0].command, "close_float_chat");
});

test("accepted text is not offered again when history refresh fails", async () => {
  const f = route(); f.handle((command) => {
    if (command === "float_context") return { ...context };
    if (command === "float_message_page") throw new Error("Synthetic history failure");
  });
  await f.c.send(); await tick();
  assert.equal(f.c.draft, ""); assert.equal(readFloatDraft(f.store, context), ""); assert.equal(f.c.sendError, null);
  assert.ok(f.c.error.diagnostic.includes("history failure")); assert.equal(f.c.context.connected, true);
  const send = f.calls.find(({ command }) => command === "float_send_text");
  assert.deepEqual({ ...send?.args }, { text: "Preserved draft" });
});

test("send ACK preserves newer draft and queued scope loss prevents dispatch", async () => {
  const f = route(), ready = deferred<void>(); f.handle((command) => command === "float_send_text" ? ready.promise
    : command === "float_context" ? { ...context } : { messages: [], has_more: false });
  const pending = f.c.send(); await tick(); f.c.changeDraft("Newer draft"); ready.resolve(); await pending;
  assert.equal(f.c.draft, "Newer draft"); assert.equal(readFloatDraft(f.store, context), "Newer draft");
  const queued = route(), gate = deferred<void>(); queued.c.outbox = gate.promise;
  const sending = queued.c.send(); queued.c.alive = false; queued.c.epoch++; gate.resolve(); await sending;
  assert.equal(queued.calls.length, 0);
});

test("refresh invalidates older page, replaces all loaded rows and bounds native limit", async () => {
  const f = route(), oldPage = deferred<any>(); f.c.rows = Array.from({ length: 120 }, (_, index) => row(index));
  f.handle((command, args) => command === "float_context" ? { ...context } : args.cursor ? oldPage.promise
    : { messages: [row(119, { revoked: true })], has_more: true });
  const loadingOlder = f.c.older(); await tick(); await f.c.refresh();
  oldPage.resolve({ messages: [row(-1, { text: "STALE SECRET" })], has_more: true }); await loadingOlder;
  assert.equal(f.c.rows.length, 1); assert.equal(f.c.rows[0].revoked, true); assert.equal(f.c.olderLoading, false);
  assert.ok(f.calls.some(({ command, args }) => command === "float_message_page" && !args.cursor && args.limit === 120));
  f.c.rows = Array.from({ length: 500 }, (_, index) => row(index)); f.c.hasMore = true;
  const count = f.calls.length; await f.c.older(); assert.equal(f.calls.length, count);
});

test("unmount and retargeted context cannot install another chat or account", async () => {
  const f = route(), pendingContext = deferred<any>(); f.handle(() => pendingContext.promise);
  const pending = f.c.refresh(); f.c.alive = false; f.c.epoch++; pendingContext.resolve({ ...context, account_id: "b" }); await pending;
  assert.equal(f.c.context.account_id, "a"); assert.equal(f.calls.length, 1);
  const changed = route(); changed.handle(() => ({ ...context, account_id: "b" })); await changed.c.refresh();
  assert.equal(changed.c.context.account_id, "a"); assert.equal(changed.c.context.connected, false);
  assert.equal(changed.c.error.code, "error.float_binding_changed");
});

test("private Channel uses unit invalidation, coalesces reload and only scoped native commands", async () => {
  const f = route(); await f.c.start();
  assert.equal(f.calls[0].command, "float_subscribe"); assert.deepEqual(Object.keys(f.calls[0].args), ["updates"]);
  for (let index = 0; index < 10; index++) f.channels[0].onmessage();
  assert.equal(f.timers.length, 1); f.timers[0](); await tick();
  assert.ok(f.calls.every(({ command, args }) => ["float_subscribe", "float_context", "float_message_page"].includes(command)
    && !Object.hasOwn(args ?? {}, "accountId") && !Object.hasOwn(args ?? {}, "chat")));
});

test("Enter handles Shift/IME and viewport restoration never overrides reader scroll", async () => {
  let sends = 0, prevented = 0;
  const c = functions("../lib/chat/FloatChat.svelte", { canSend: true, onsend: async () => { sends++; }, pinnedBottom: false, scrollRevision: 0,
    scroller: { isConnected: true, scrollTop: 100, scrollHeight: 1000 }, tick: async () => {} });
  for (const patch of [{ shiftKey: true }, { isComposing: true }, { keyCode: 229 }, {}])
    c.key({ key: "Enter", preventDefault: () => { prevented++; }, ...patch });
  assert.equal(sends, 1); assert.equal(prevented, 1);
  await c.preserveViewport(async () => { c.scroller.scrollHeight += 200; }, true); assert.equal(c.scroller.scrollTop, 300);
  await c.preserveViewport(async () => { c.scrollRevision++; c.scroller.scrollTop = 450; }); assert.equal(c.scroller.scrollTop, 450);
});

test("float renders safe text, truthful private fallback, native-background alpha and bounded history", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/float-chat", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server"), { default: Chat } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/FloatChat.svelte", import.meta.url)));
    const fail = () => { throw new Error("SSR must not invoke native actions"); };
    const props = { context: { ...context, connected: false }, rows: [row(1, { text: "*Bold* <img src=x onerror=bad>" }), row(2, { text: "PRIVATE", spoiler: true })],
      draft: "Saved text", draftReady: true, opacity: 0.35, ondraft: fail, onopacity: fail, onsend: fail, onolder: fail, onretry: fail, onretrydraft: fail, onclose: fail };
    const body = render(Chat, { props }).body;
    assert.ok(/--float-alpha:\s*35%/.test(body) && body.includes("<strong") && body.includes("Bold"));
    assert.ok(body.includes("&lt;img") && !body.includes("<img") && !body.includes("PRIVATE"));
    assert.ok(body.includes("Offline") && body.includes("Saved text") && /<button\b[^>]*type="submit"[^>]*disabled/.test(body));
    const capped = render(Chat, { props: { ...props, rows: Array.from({ length: 500 }, (_, index) => row(index)), hasMore: true } }).body;
    assert.ok(capped.includes("Showing up to 500") && capped.includes("Floating history limit reached"));
  } finally { await server.close(); }
});

test("float keyword rules load only fixed account and matching storage changes without RPC", () => {
  const f = route(), key = keywordStorageKey(context.account_id), other = keywordStorageKey("b");
  f.store.setItem(key, JSON.stringify({ version: 1, highlight: ["Needle"], hide: ["Secret"] }));
  f.store.setItem(other, JSON.stringify({ version: 1, highlight: [], hide: ["Other account"] }));
  f.c.loadRules(); assert.deepEqual(f.c.rules, { highlight: ["Needle"], hide: ["Secret"] });
  const current = f.c.rules;
  f.c.rulesChanged({ key: other }); assert.equal(f.c.rules, current);
  f.store.removeItem(key); f.c.rulesChanged({ key: null });
  assert.deepEqual(f.c.rules, { highlight: [], hide: [] });
  f.c.alive = false; f.store.setItem(key, "invalid"); f.c.rulesChanged({ key });
  assert.equal(f.c.keywordError, null); assert.equal(f.calls.length, 0);
});

test("keyword parsing/storage failures are visible and never fall back to revealing history", () => {
  const f = route(), key = keywordStorageKey(context.account_id);
  f.store.setItem(key, JSON.stringify({ version: 1, highlight: [], hide: ["Secret"] })); f.c.loadRules();
  assert.equal(f.c.rulesReady, true);
  f.store.setItem(key, "broken JSON"); f.c.rulesChanged({ key });
  assert.equal(f.c.rulesReady, false); assert.equal(f.c.keywordError.code, "error.float_rules_read");
  assert.ok(f.c.keywordError.diagnostic.includes("Could not load keyword rules"));
  assert.deepEqual(f.c.rules.hide, ["Secret"]);
  f.c.localStorage.getItem = () => { throw new Error("Denied"); }; f.c.loadRules();
  assert.equal(f.c.rulesReady, false); assert.ok(f.c.keywordError.diagnostic.includes("Denied")); assert.equal(f.calls.length, 0);
});

test("floating history hides account keywords, highlights safe visible rows and preserves private fallbacks", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/float-keywords", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server"), { default: Chat } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/FloatChat.svelte", import.meta.url)));
    const raw = [row(1, { text: "HIDDEN incoming" }), row(2, { text: "*Needle* <img src=x>" }), row(3, { text: "Needle PRIVATE", spoiler: true })];
    const before = JSON.stringify(raw), fail = () => { throw new Error("SSR must not invoke actions"); };
    const props = { context, rows: raw, draft: "", draftReady: true, rulesReady: true, rules: { highlight: ["Needle"], hide: ["HIDDEN"] },
      ondraft: fail, onopacity: fail, onsend: fail, onolder: fail, onretry: fail, onretrydraft: fail, onclose: fail };
    const body = render(Chat, { props }).body;
    assert.ok(!body.includes("HIDDEN incoming") && !body.includes("PRIVATE") && !body.includes("<img"));
    assert.ok(body.includes("keyword-highlighted") && body.includes("&lt;img") && body.includes("Spoiler"));
    assert.equal([...body.matchAll(/class="[^"]*keyword-highlighted/g)].length, 1); assert.equal(JSON.stringify(raw), before);
    const hidden = render(Chat, { props: { ...props, rows: [raw[0]] } }).body;
    assert.ok(hidden.includes("All loaded messages are hidden by your keyword rules") && !hidden.includes("No stored messages"));
    const failed = render(Chat, { props: { ...props, rulesReady: false, keywordError: "Rules unreadable" } }).body;
    assert.ok(failed.includes("Rules unreadable") && failed.includes("Messages are hidden until keyword rules can be read") && !failed.includes("Needle"));
  } finally { await server.close(); }
});
