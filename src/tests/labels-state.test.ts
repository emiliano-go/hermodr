import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { compileModule } from "svelte/compiler";
import ts from "typescript";
import { normalizeError } from "../lib/i18n/errors.ts";
import { uiError } from "../lib/state/localized.ts";
import type { LabelsState } from "../lib/state/labels.svelte";

const source = readFileSync(new URL("../lib/state/labels.svelte.ts", import.meta.url), "utf8");
const js = ts.transpileModule(source, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
const compiled = compileModule(js, { generate: "server", filename: "labels.svelte.js" }).js.code.replace(/^import .*;$/gm, "").replace(/^export /gm, "");
const catalog = (name: string) => ({ labels: [{ id: "17", name, color: 3 }], chats: [], messages: [], complete: true });
type Run = (signal: AbortSignal) => Promise<void>;

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

function fixture() {
  const calls: { command: string; args: Record<string, unknown> }[] = [], uuids: string[] = [];
  const session = { activeAccount: "synthetic-a" as string | null, connected: true }, messages = { accountGeneration: 1 };
  let handler: (command: string, args: Record<string, unknown>) => unknown = (command) => command === "labels_view" ? catalog("current") : undefined;
  let enqueue = (run: Run) => run(new AbortController().signal);
  const invoke = (command: string, args: Record<string, unknown>) => { calls.push({ command, args }); return handler(command, args); };
  const crypto = { randomUUID: () => { const id = `${uuids.length ? "b" : "a"}2345678-1234-1234-1234-123456789abc`; uuids.push(id); return id; } };
  const State = new Function("invoke", "composer", "messages", "session", "crypto", "normalizeError", "uiError", `${compiled}\nreturn LabelsState;`)
    (invoke, { enqueue: (run: Run) => enqueue(run) }, messages, session, crypto, normalizeError, uiError) as new () => LabelsState;
  const state = new State(); state.account = session.activeAccount;
  return { state, session, messages, calls, uuids,
    handle: (next: typeof handler) => { handler = next; }, queue: (next: typeof enqueue) => { enqueue = next; } };
}

test("label catalog rejects stale account, generation and request responses", async () => {
  for (const change of ["account", "generation", "request"]) {
    const f = fixture(), pending = [deferred<ReturnType<typeof catalog>>(), deferred<ReturnType<typeof catalog>>()];
    f.handle(() => pending[f.calls.length - 1].promise);
    const initial = f.state.view, old = f.state.refresh(), value = catalog("latest");
    if (change === "account") f.session.activeAccount = "synthetic-b";
    if (change === "generation") f.messages.accountGeneration++;
    if (change === "request") { const latest = f.state.refresh(); pending[1].resolve(value); await latest; }
    pending[0].resolve(catalog("obsolete")); await old;
    assert.deepEqual(f.state.view, change === "request" ? value : initial, `${change} guard must work independently`);
    if (change !== "request") { assert.equal(f.state.loaded, false); const latest = f.state.refresh(); pending[1].resolve(value); await latest; }
    assert.deepEqual(f.state.view, value, change); assert.equal(f.state.loaded, true); assert.equal(f.state.loading, false);
  }
});

test("queued label operations revalidate account, generation, abort and connection before native mutations", async () => {
  for (const change of ["account", "generation", "abort", "disconnect"]) {
    const f = fixture(), controller = new AbortController(); let queued!: () => Promise<void>;
    f.queue((run) => new Promise<void>((resolve, reject) => { queued = () => run(controller.signal).then(resolve, reject); }));
    const pending = f.state.applyChat("17", "synthetic-chat", true);
    assert.equal(f.calls.length, 0);
    if (change === "account") f.session.activeAccount = "synthetic-b";
    if (change === "generation") f.messages.accountGeneration++;
    if (change === "abort") controller.abort();
    if (change === "disconnect") f.session.connected = false;
    const rejected = assert.rejects(pending, /changed|unavailable|disconnected/i);
    await queued(); await rejected;
    assert.equal(f.calls.filter(({ command }) => command !== "labels_view").length, 0, change);
    if (change === "account" || change === "generation") assert.equal(f.calls.length, 0, `${change} must not reload another scope`);
    else { assert.equal(f.state.busy, false); assert.ok(f.calls.every(({ args }) => args.accountId === "synthetic-a")); }
  }
});

test("bulk labeling stops at first failure, reloads partial successes and preserves original error", async () => {
  const f = fixture(), failure = new Error("synthetic second mutation failed");
  const partial = { ...catalog("partial"), messages: [{ label_id: "17", chat: "synthetic-chat", message_id: "first" }] };
  f.handle((command, args) => {
    if (command === "labels_view") return partial;
    assert.equal(command, "label_message");
    if (args.messageId === "second") throw failure;
  });
  await assert.rejects(f.state.applyMessages("17", ["first", "second", "third"].map((id) => ({ chat: "synthetic-chat", id })), true), (error) => error === failure);
  assert.deepEqual(f.calls.map(({ command, args }) => [command, args.messageId]), [["label_message", "first"], ["label_message", "second"], ["labels_view", undefined]]);
  assert.deepEqual(f.state.view, partial); assert.equal(f.state.error, "Operation failed."); assert.equal(JSON.parse(f.state.diagnostic!).message, failure.message); assert.equal(f.state.busy, false);

  f.calls.length = 0;
  f.handle((command) => { throw command === "labels_view" ? new Error("synthetic reload failed") : failure; });
  await assert.rejects(f.state.applyChat("17", "synthetic-chat", true), (error) => error === failure);
  assert.equal(f.state.error, "Operation failed."); assert.equal(JSON.parse(f.state.diagnostic!).message, failure.message); assert.equal(f.state.busy, false);
});

test("bulk labeling never reloads a new account and stale recovery cannot install its error", async () => {
  const changed = fixture();
  changed.handle(() => { changed.session.activeAccount = "synthetic-b"; changed.messages.accountGeneration++; });
  await assert.rejects(changed.state.applyMessages("17", [{ chat: "synthetic-chat", id: "first" }, { chat: "synthetic-chat", id: "second" }], true), /Account changed/);
  assert.equal(changed.calls.length, 1); assert.equal(changed.calls[0].command, "label_message");

  const f = fixture(), failure = new Error("obsolete account failure"), old = deferred<ReturnType<typeof catalog>>(), latest = deferred<ReturnType<typeof catalog>>(), recovering = deferred<void>();
  f.handle((command, args) => {
    if (command !== "labels_view") throw failure;
    if (args.accountId === "synthetic-a") { recovering.resolve(); return old.promise; }
    return latest.promise;
  });
  const pending = f.state.applyChat("17", "synthetic-chat", true), rejected = assert.rejects(pending, (error) => error === failure);
  await recovering.promise;
  f.session.activeAccount = "synthetic-b"; f.messages.accountGeneration++; f.state.reset();
  const current = f.state.refresh(), value = catalog("new account"); latest.resolve(value); await current;
  old.resolve(catalog("obsolete")); await rejected;
  assert.deepEqual(f.state.view, value); assert.equal(f.state.error, null); assert.equal(f.state.busy, false);
});

test("label creation uses fresh decimal UUID IDs while edits preserve existing IDs", async () => {
  const f = fixture();
  await f.state.save("", "Created", 5); await f.state.save("", "Another", 6); await f.state.save("17", "Edited", 7);
  const saves = f.calls.filter(({ command }) => command === "save_label");
  assert.equal(f.uuids.length, 2);
  for (const [index, name] of ["Created", "Another"].entries()) {
    assert.deepEqual(saves[index].args, { accountId: "synthetic-a", labelId: BigInt(`0x${f.uuids[index].replaceAll("-", "")}`).toString(), name, color: index + 5, create: true });
    assert.match(String(saves[index].args.labelId), /^\d+$/);
  }
  assert.notEqual(saves[0].args.labelId, saves[1].args.labelId);
  assert.deepEqual(saves[2].args, { accountId: "synthetic-a", labelId: "17", name: "Edited", color: 7, create: false });
});

test("disconnected label mutations reject without queuing or native calls", async () => {
  const f = fixture(); let queued = false; f.session.connected = false;
  f.queue(async () => { queued = true; });
  await assert.rejects(f.state.applyChat("17", "synthetic-chat", true), /unavailable/);
  assert.equal(queued, false); assert.equal(f.calls.length, 0); assert.equal(f.state.busy, false);
});
