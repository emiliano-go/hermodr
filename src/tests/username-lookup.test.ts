import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { createServer } from "vite";
import type { UsernameLookupResult } from "../lib/utils/wire.ts";

const source = readFileSync(new URL("../lib/chat/UsernameLookup.svelte", import.meta.url), "utf8");
const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
const tree = ts.createSourceFile("username-lookup.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const effect = tree.statements.find((node) => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
  && node.expression.expression.getText(tree) === "$effect");
assert.ok(effect && ts.isExpressionStatement(effect) && ts.isCallExpression(effect.expression));
const compiled = ts.transpileModule(`${tree.statements.filter(ts.isFunctionDeclaration).map((node) => node.getText(tree)).join("\n")}
var scopeEffect = ${effect.expression.arguments[0].getText(tree)};`,
{ compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (failure: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

function fixture() {
  const requests: { username: string; key: string | undefined; result: ReturnType<typeof deferred<UsernameLookupResult>> }[] = [];
  const opened: { jid: string; username: string | null }[] = [];
  let closes = 0;
  let dialogCloses = 0;
  const context = {
    account: "a", generation: 1, openedAccount: "a", openedGeneration: 1,
    closed: false, request: 0, query: "", usernameKey: "", busy: false,
    result: null as UsernameLookupResult | null, error: null as string | null,
    dialog: { close: () => { dialogCloses++; } }, onclose: () => { closes++; },
    onlookup: (username: string, key?: string) => {
      const result = deferred<UsernameLookupResult>();
      requests.push({ username, key, result });
      return result.promise;
    },
    onfound: (jid: string, username: string | null): void | Promise<void> => { opened.push({ jid, username }); },
  };
  runInNewContext(compiled, context);
  const actions = context as typeof context & {
    lookup(): Promise<void>; changeQuery(value: string): void; changeKey(value: string): void;
    close(): void; scopeEffect(): void;
  };
  return { actions, requests, opened, closes: () => closes, dialogCloses: () => dialogCloses };
}

test("manual username lookup opens only returned address and closes once", async () => {
  for (const found of [
    { kind: "found", jid: "actual@lid", username: "resolved-user" },
    { kind: "found", jid: "12025550101@s.whatsapp.net", username: null },
  ] as const) {
    const f = fixture();
    await f.actions.lookup();
    f.actions.changeQuery("  @requested-user  ");
    assert.equal(f.requests.length, 0);
    const pending = f.actions.lookup();
    assert.equal(f.requests.length, 1);
    assert.equal(f.requests[0].username, "@requested-user");
    assert.equal(f.requests[0].key, undefined);
    await f.actions.lookup();
    assert.equal(f.requests.length, 1);
    f.requests[0].result.resolve(found);
    await pending;
    assert.deepEqual(f.opened, [{ jid: found.jid, username: found.username }]);
    f.actions.close();
    assert.equal(f.closes(), 1);
    assert.equal(f.dialogCloses(), 1);
    assert.equal(f.actions.query, "");
    assert.equal(f.actions.usernameKey, "");
    assert.equal(f.actions.busy, false);
  }
});

test("key-required lookup preserves opaque key and username changes clear key-bound state", async () => {
  const f = fixture();
  f.actions.changeQuery("first-user");
  const first = f.actions.lookup();
  f.requests[0].result.resolve({ kind: "keyRequired", username: "first-user" });
  await first;
  assert.equal(f.actions.result?.kind, "keyRequired");
  f.actions.changeKey(" Key+/= ");
  const retry = f.actions.lookup();
  assert.equal(f.requests[1].key, " Key+/= ");
  f.requests[1].result.resolve({ kind: "keyRequired", username: "first-user" });
  await retry;
  assert.equal(f.actions.usernameKey, " Key+/= ");
  f.actions.changeQuery("second-user");
  assert.equal(f.requests.length, 2);
  assert.equal(f.actions.result, null);
  assert.equal(f.actions.usernameKey, "");
  const second = f.actions.lookup();
  assert.equal(f.requests[2].key, undefined);
  f.requests[2].result.resolve({ kind: "notFound" });
  await second;
  assert.deepEqual(f.actions.result, { kind: "notFound" });
  assert.deepEqual(f.opened, []);
  assert.equal(f.closes(), 0);
});

test("current lookup and chat-opening failures remain visible and retryable", async () => {
  const f = fixture();
  f.actions.changeQuery("retry-user");
  const first = f.actions.lookup();
  f.requests[0].result.reject(new Error("Synthetic lookup failure"));
  await first;
  assert.equal(f.actions.error, "Error: Synthetic lookup failure");
  assert.equal(f.actions.busy, false);
  f.actions.onfound = async () => { throw new Error("Synthetic open failure"); };
  const retry = f.actions.lookup();
  assert.equal(f.actions.error, null);
  f.requests[1].result.resolve({ kind: "found", jid: "real@lid", username: null });
  await retry;
  assert.equal(f.actions.error, "Error: Synthetic open failure");
  assert.equal(f.actions.busy, false);
  assert.equal(f.closes(), 0);
});

const changes = [
  (a: ReturnType<typeof fixture>["actions"]) => { a.account = "b"; a.scopeEffect(); },
  (a: ReturnType<typeof fixture>["actions"]) => { a.generation++; a.scopeEffect(); },
  (a: ReturnType<typeof fixture>["actions"]) => { a.changeQuery("new-user"); },
  (a: ReturnType<typeof fixture>["actions"]) => { a.changeKey("new-key"); },
  (a: ReturnType<typeof fixture>["actions"]) => { a.close(); },
];
const snapshot = (a: ReturnType<typeof fixture>["actions"]) => [a.query, a.usernameKey, a.result, a.busy, a.error, a.closed];

test("account, generation, query, key and close changes discard every late lookup outcome", async () => {
  for (const change of changes) for (const result of [
    { kind: "found", jid: "obsolete@lid", username: "old-user" },
    { kind: "notFound" }, { kind: "keyRequired", username: "old-user" }, null,
  ] as const) {
    const f = fixture();
    f.actions.changeQuery("old-user");
    f.actions.result = { kind: "keyRequired", username: "old-user" };
    f.actions.changeKey("old-key");
    const pending = f.actions.lookup();
    change(f.actions);
    const state = snapshot(f.actions);
    const closes = f.closes();
    if (result) f.requests[0].result.resolve(result);
    else f.requests[0].result.reject(new Error("Obsolete failure"));
    await pending;
    assert.deepEqual(snapshot(f.actions), state);
    assert.deepEqual(f.opened, []);
    assert.equal(f.closes(), closes);
    if (f.actions.closed) assert.equal(f.actions.usernameKey, "");
  }
});

test("older requests and opening callbacks cannot clear or close newer input", async () => {
  const f = fixture();
  f.actions.changeQuery("old-user");
  const old = f.actions.lookup();
  f.actions.changeQuery("new-user");
  const latest = f.actions.lookup();
  f.requests[0].result.reject(new Error("Obsolete failure"));
  await old;
  assert.equal(f.actions.busy, true);
  assert.equal(f.actions.error, null);
  f.requests[1].result.resolve({ kind: "notFound" });
  await latest;
  assert.equal(f.actions.result?.kind, "notFound");
  assert.equal(f.actions.busy, false);

  for (const change of changes) for (const fail of [false, true]) {
    const opening = fixture();
    const completion = deferred<void>();
    opening.actions.onfound = () => completion.promise;
    opening.actions.changeQuery("open-user");
    const pending = opening.actions.lookup();
    opening.requests[0].result.resolve({ kind: "found", jid: "real@lid", username: null });
    await new Promise((resolve) => setImmediate(resolve));
    change(opening.actions);
    const state = snapshot(opening.actions);
    const closes = opening.closes();
    if (fail) completion.reject(new Error("Obsolete open failure"));
    else completion.resolve();
    await pending;
    assert.deepEqual(snapshot(opening.actions), state);
    assert.equal(opening.closes(), closes);
  }
});

test("username dialog renders accessible manual lookup without initial key field or requests", async () => {
  const server = await createServer({ configFile: fileURLToPath(new URL("../../tests/browser/vite.config.ts", import.meta.url)),
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/username-lookup", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } }, server: { middlewareMode: true, ws: false } });
  try {
    const { render } = await server.ssrLoadModule("svelte/server");
    const { default: UsernameLookup } = await server.ssrLoadModule(fileURLToPath(new URL("../lib/chat/UsernameLookup.svelte", import.meta.url)));
    const f = fixture();
    const body = render(UsernameLookup, { props: { account: "a", generation: 1,
      onlookup: f.actions.onlookup, onfound: f.actions.onfound, onclose: f.actions.onclose } }).body;
    assert.ok(body.includes('aria-labelledby="username-lookup-title"') && body.includes('for="username-lookup-query"'));
    assert.ok(body.includes('aria-label="Close username lookup"') && /<button\b[^>]*type="submit"[^>]*disabled/.test(body));
    assert.ok(!body.includes('type="password"'));
    assert.equal(f.requests.length, 0);
    assert.deepEqual(f.opened, []);
  } finally { await server.close(); }
});
